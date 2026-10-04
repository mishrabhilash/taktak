//! macOS backend: a listen-only `CGEventTap` on its own CFRunLoop thread.
//!
//! Listen-only taps need the Input Monitoring permission (not Accessibility) and cannot
//! modify or swallow events. We read only the virtual keycode, the autorepeat flag and, for
//! modifiers, the device flag bits — never `CGEventKeyboardGetUnicodeString` or any
//! layout/TIS API, so no characters are ever produced.

use super::keymap_macos::{from_virtual_keycode, modifier_held_mask};
use super::{Access, InputError, KeyAction, KeyEvent, PressState};
use crate::clock;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

type CFMachPortRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFStringRef = *const c_void;
type CGEventRef = *mut c_void;
type CGEventTapProxy = *mut c_void;

type TapCallback = extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

const K_CG_SESSION_EVENT_TAP: u32 = 1;
const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;

const K_CG_EVENT_KEY_DOWN: u32 = 10;
const K_CG_EVENT_KEY_UP: u32 = 11;
const K_CG_EVENT_FLAGS_CHANGED: u32 = 12;
const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

const K_CG_KEYBOARD_EVENT_AUTOREPEAT: u32 = 8;
const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;

const CAPS_LOCK: u16 = 0x39;

const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;

unsafe extern "C" {
    fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventGetTimestamp(event: CGEventRef) -> u64;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

/// `kIOHIDRequestTypeListenEvent` (IOKit/hidsystem/IOHIDLib.h).
const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;

// Both since macOS 10.15. `IOHIDRequestAccess` for listening is what adds TakTak to Privacy &
// Security → Input Monitoring (macOS shows its alert, which also creates the entry, switched
// off); `IOHIDCheckAccess` tells "listed but off" (denied) from "not listed" (unknown).
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDCheckAccess(request_type: u32) -> u32;
    fn IOHIDRequestAccess(request_type: u32) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: CFStringRef;
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFMachPortInvalidate(port: CFMachPortRef);
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source: bool) -> i32;
    fn CFRunLoopStop(rl: CFRunLoopRef);
    fn CFRelease(cf: *const c_void);
    fn CFRetain(cf: *const c_void) -> *const c_void;
}

pub fn preflight() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

/// Never prompts.
pub fn access() -> Access {
    let raw = unsafe { IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) };
    super::access_from_iohid(raw, preflight)
}

/// May show macOS's Input Monitoring alert (not always: see docs/platform-notes.md § macOS).
pub fn request() -> bool {
    let granted = unsafe { IOHIDRequestAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) };
    granted || preflight()
}

/// The CoreGraphics form of [`request`], kept as a fallback.
pub fn request_fallback() -> bool {
    unsafe { CGRequestListenEventAccess() }
}

struct TapState {
    callback: Box<dyn FnMut(KeyEvent) + Send>,
    press: PressState,
    port: CFMachPortRef,
    /// Times the system disabled the tap and the callback re-enabled it. Counted, not logged:
    /// the callback must not lock or block (see [`TapHandle::take_reenabled`]).
    reenabled: Arc<AtomicU32>,
    /// `CGEventGetTimestamp` is documented as nanoseconds but returns mach ticks on some
    /// systems (Apple Silicon); resolved on the first event.
    timestamp_is_ticks: Option<bool>,
}

impl TapState {
    fn event_ns(&mut self, raw: u64, received_ns: u64) -> u64 {
        let is_ticks =
            *self.timestamp_is_ticks.get_or_insert_with(|| raw.saturating_mul(2) < received_ns);
        let ns = if is_ticks { clock::ticks_to_ns(raw) } else { raw };
        ns.min(received_ns)
    }
}

extern "C" fn tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    // SAFETY: user_info is the Box<TapState> leaked in `run_tap`, only touched on this thread.
    let state = unsafe { &mut *(user_info as *mut TapState) };
    let received_ns = clock::now_ns();

    match event_type {
        K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT | K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT => {
            // The system disables taps that stall. Our callback only enqueues, but re-enable
            // defensively, first thing, and forget held keys, since we may have missed their
            // ups. Never log here: a blocked stderr would keep the tap disabled.
            unsafe { CGEventTapEnable(state.port, true) };
            state.press.clear();
            state.reenabled.fetch_add(1, Ordering::Relaxed);
            return event;
        }
        K_CG_EVENT_KEY_DOWN | K_CG_EVENT_KEY_UP | K_CG_EVENT_FLAGS_CHANGED => {}
        _ => return event,
    }

    let code = unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) } as u16;
    let Some(key) = from_virtual_keycode(code) else { return event };

    let action = match event_type {
        K_CG_EVENT_KEY_DOWN => {
            if unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_AUTOREPEAT) } != 0 {
                return event;
            }
            KeyAction::Down
        }
        K_CG_EVENT_KEY_UP => KeyAction::Up,
        _ if code == CAPS_LOCK => KeyAction::Down,
        _ => match modifier_held_mask(code) {
            Some(mask) if unsafe { CGEventGetFlags(event) } & mask != 0 => KeyAction::Down,
            Some(_) => KeyAction::Up,
            None => return event,
        },
    };

    if code == CAPS_LOCK {
        // Press-only key: never mark it held, so the next press plays again.
        state.press.accept(key, KeyAction::Up);
    } else if !state.press.accept(key, action) {
        return event;
    }

    let event_ns = state.event_ns(unsafe { CGEventGetTimestamp(event) }, received_ns);
    (state.callback)(KeyEvent { key, action, event_ns, received_ns });
    event
}

struct SendPtr(CFRunLoopRef);
// SAFETY: CFRunLoopStop is documented as callable from any thread.
unsafe impl Send for SendPtr {}

pub struct TapHandle {
    run_loop: SendPtr,
    stop: Arc<AtomicBool>,
    reenabled: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
}

impl TapHandle {
    /// Times the system disabled the tap (and it was re-enabled) since the last call.
    pub fn take_reenabled(&self) -> u32 {
        self.reenabled.swap(0, Ordering::Relaxed)
    }
}

pub fn start(callback: Box<dyn FnMut(KeyEvent) + Send>) -> Result<TapHandle, InputError> {
    let stop = Arc::new(AtomicBool::new(false));
    let reenabled = Arc::new(AtomicU32::new(0));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<SendPtr, InputError>>();
    let (thread_stop, thread_reenabled) = (stop.clone(), reenabled.clone());
    let thread = std::thread::Builder::new()
        .name("taktak-input".into())
        .spawn(move || run_tap(callback, thread_stop, thread_reenabled, ready_tx))
        .map_err(|e| InputError::Platform(e.to_string()))?;

    match ready_rx.recv() {
        Ok(Ok(run_loop)) => Ok(TapHandle { run_loop, stop, reenabled, thread: Some(thread) }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(InputError::Platform("input thread exited during startup".into())),
    }
}

fn run_tap(
    callback: Box<dyn FnMut(KeyEvent) + Send>,
    stop: Arc<AtomicBool>,
    reenabled: Arc<AtomicU32>,
    ready: mpsc::Sender<Result<SendPtr, InputError>>,
) {
    // Without this the scheduler may park the thread on an efficiency core and wake it late
    // after idle, which shows up as multi-millisecond spikes in the input stage.
    unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    let mask: u64 =
        (1 << K_CG_EVENT_KEY_DOWN) | (1 << K_CG_EVENT_KEY_UP) | (1 << K_CG_EVENT_FLAGS_CHANGED);
    let state = Box::into_raw(Box::new(TapState {
        callback,
        press: PressState::default(),
        port: std::ptr::null_mut(),
        reenabled,
        timestamp_is_ticks: None,
    }));

    unsafe {
        let port = CGEventTapCreate(
            K_CG_SESSION_EVENT_TAP,
            K_CG_HEAD_INSERT_EVENT_TAP,
            K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
            mask,
            tap_callback,
            state as *mut c_void,
        );
        if port.is_null() {
            drop(Box::from_raw(state));
            let err = if preflight() {
                InputError::Platform("CGEventTapCreate returned null".into())
            } else {
                InputError::PermissionDenied
            };
            let _ = ready.send(Err(err));
            return;
        }
        (*state).port = port;

        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
        let run_loop = CFRunLoopGetCurrent();
        CFRunLoopAddSource(run_loop, source, kCFRunLoopDefaultMode);
        CGEventTapEnable(port, true);
        // Retained so `TapHandle::drop` can safely call CFRunLoopStop even if this thread
        // has already exited; released there after the join.
        CFRetain(run_loop);
        let _ = ready.send(Ok(SendPtr(run_loop)));

        // Sleeps in the kernel until an event arrives; the timeout only bounds how long a
        // `stop` issued before the loop started can go unnoticed.
        while !stop.load(Ordering::Acquire) {
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 1.0, false);
        }

        CGEventTapEnable(port, false);
        CFMachPortInvalidate(port);
        CFRelease(source);
        CFRelease(port);
        drop(Box::from_raw(state));
    }
}

impl Drop for TapHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe { CFRunLoopStop(self.run_loop.0) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        unsafe { CFRelease(self.run_loop.0) };
    }
}
