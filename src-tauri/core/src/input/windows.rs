//! Windows backend: a `WH_KEYBOARD_LL` low-level keyboard hook on its own message-loop thread.
//!
//! The hook is listen-only in effect: it always passes the event on with `CallNextHookEx` and
//! never blocks one. It reads only `scanCode` and `flags` from `KBDLLHOOKSTRUCT` and the
//! message (down/up); never `vkCode` (layout dependent) and never `ToUnicode`/`ToUnicodeEx`, so
//! no characters are produced. See [`super::keymap_windows::from_hook`] for which events count.
//!
//! - **Timing.** Windows calls the hook synchronously, on this thread, before any app sees the
//!   key, and removes the hook for good (silently) if it does not return within
//!   `LowLevelHooksTimeout` (~300 ms, at most 1 s). The callback therefore only enqueues, and the
//!   thread runs at `THREAD_PRIORITY_TIME_CRITICAL` so it is scheduled at once: while it is
//!   late, the whole system's keyboard input waits for it.
//! - **Timestamps.** `KBDLLHOOKSTRUCT.time` is `GetTickCount` milliseconds, quantized to the
//!   system tick (~15.6 ms), far too coarse to measure input latency. Both `event_ns` and
//!   `received_ns` are therefore the time the hook ran (the input stage reads 0 on Windows).
//! - **UIPI.** Keys typed into a window of higher integrity (an app run as administrator) are
//!   not delivered to a hook in a normal process, so TakTak is silent there. The secure desktop
//!   (sign-in, lock screen, UAC prompts) is never visible to any hook.
//! - **Removal.** A hook Windows removed after a timeout cannot be detected from inside the
//!   process; [`HookHandle::take_reenabled`] always reports 0.

use super::keymap_windows::from_hook;
use super::{InputError, KeyEvent, PressState};
use crate::clock;
use std::cell::Cell;
use std::sync::mpsc;
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{GetLastError, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, PeekMessageW,
    PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_QUIT, WM_USER,
};

struct HookState {
    callback: Box<dyn FnMut(KeyEvent) + Send>,
    press: PressState,
}

thread_local! {
    /// The low-level hook procedure has no user-data argument; Windows calls it on the thread
    /// that installed it, so the state lives in that thread's local storage.
    static STATE: Cell<*mut HookState> = const { Cell::new(std::ptr::null_mut()) };
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let received_ns = clock::now_ns();
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT valid for this call.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let state = STATE.with(Cell::get);
        if !state.is_null()
            && let Some((key, action)) = from_hook(info.scanCode, info.flags, wparam as u32)
        {
            // SAFETY: the state is owned by this thread (see `run_hook`) and outlives the hook.
            let state = unsafe { &mut *state };
            // Auto-repeat arrives as repeated downs; a stray up (the key went down while
            // another desktop or an elevated window had the input) is dropped.
            if state.press.accept(key, action) {
                (state.callback)(KeyEvent { key, action, event_ns: received_ns, received_ns });
            }
        }
    }
    // Always pass the event on: TakTak never blocks or alters input.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

pub struct HookHandle {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl HookHandle {
    /// Windows gives no signal when it removes a slow hook, so there is nothing to count.
    pub fn take_reenabled(&self) -> u32 {
        0
    }
}

pub fn start(callback: Box<dyn FnMut(KeyEvent) + Send>) -> Result<HookHandle, InputError> {
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, InputError>>();
    let thread = std::thread::Builder::new()
        .name("taktak-input".into())
        .spawn(move || run_hook(callback, ready_tx))
        .map_err(|e| InputError::Platform(e.to_string()))?;

    match ready_rx.recv() {
        Ok(Ok(thread_id)) => Ok(HookHandle { thread_id, thread: Some(thread) }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(InputError::Platform("input thread exited during startup".into())),
    }
}

fn run_hook(
    callback: Box<dyn FnMut(KeyEvent) + Send>,
    ready: mpsc::Sender<Result<u32, InputError>>,
) {
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        // Create this thread's message queue before anyone can post WM_QUIT to it.
        let mut msg: MSG = std::mem::zeroed();
        PeekMessageW(&mut msg, std::ptr::null_mut(), WM_USER, WM_USER, PM_NOREMOVE);

        let state = Box::into_raw(Box::new(HookState { callback, press: PressState::default() }));
        STATE.with(|s| s.set(state));
        let hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(hook_proc),
            GetModuleHandleW(std::ptr::null()),
            0,
        );
        if hook.is_null() {
            let error = GetLastError();
            STATE.with(|s| s.set(std::ptr::null_mut()));
            drop(Box::from_raw(state));
            let _ = ready.send(Err(InputError::Platform(format!(
                "SetWindowsHookExW(WH_KEYBOARD_LL) failed (error {error})"
            ))));
            return;
        }
        let _ = ready.send(Ok(GetCurrentThreadId()));

        // The hook procedure runs inside GetMessageW. No windows belong to this thread, so the
        // only message is the WM_QUIT from `HookHandle::drop` (GetMessageW returns 0; -1 is an
        // error, which cannot happen with these arguments but must not spin).
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}

        UnhookWindowsHookEx(hook);
        STATE.with(|s| s.set(std::ptr::null_mut()));
        drop(Box::from_raw(state));
    }
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        // The queue exists (created before `start` returned), so the post cannot be lost; if
        // the thread is already gone it fails harmlessly.
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
