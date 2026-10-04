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
//!   process; [`HookHandle::take_reenabled`] always reports 0. As a cheap, event-driven guard
//!   the thread reinstalls the hook whenever the session is unlocked or reconnected
//!   (`WM_WTSSESSION_CHANGE`) and when the system resumes from sleep (`WM_POWERBROADCAST`),
//!   the moments a hook is most often lost. Both arrive at a hidden, never shown window this
//!   thread owns (`WTSRegisterSessionNotification`; power broadcasts reach every top-level
//!   window). No polling.

use super::keymap_windows::from_hook;
use super::{InputError, KeyEvent, PressState};
use crate::clock;
use std::cell::Cell;
use std::sync::mpsc;
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND,
    PM_NOREMOVE, PeekMessageW, PostThreadMessageW, RegisterClassW, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_POWERBROADCAST, WM_QUIT, WM_USER,
    WM_WTSSESSION_CHANGE, WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPED, WTS_CONSOLE_CONNECT,
    WTS_REMOTE_CONNECT, WTS_SESSION_UNLOCK,
};

/// Posted to the hook thread (by [`watch_proc`]) to reinstall the hook.
const WM_REINSTALL: u32 = WM_APP + 1;

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

/// Whether a session or power notification calls for reinstalling the hook: the session was
/// unlocked or (re)connected, or the system resumed from sleep.
fn reinstall_after(msg: u32, wparam: WPARAM) -> bool {
    let event = wparam as u32;
    match msg {
        WM_WTSSESSION_CHANGE => {
            matches!(event, WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT)
        }
        WM_POWERBROADCAST => matches!(event, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND),
        _ => false,
    }
}

/// The hidden watch window's procedure. Both notifications are *sent*, so they run inside
/// `GetMessageW` without making it return; posting a thread message wakes the loop, which
/// reinstalls the hook outside this call.
unsafe extern "system" fn watch_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if reinstall_after(msg, wparam) {
        unsafe { PostThreadMessageW(GetCurrentThreadId(), WM_REINSTALL, 0, 0) };
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// UTF-16, NUL-terminated.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Creates the hidden window that receives session and power notifications, or `None` (the
/// hook then simply is not reinstalled). Never shown; a tool window, so it would stay off the
/// taskbar and Alt+Tab even if something showed it.
unsafe fn create_watch_window(instance: HINSTANCE) -> Option<HWND> {
    let class = wide("TakTakInputWatch");
    let mut wc: WNDCLASSW = unsafe { std::mem::zeroed() };
    wc.lpfnWndProc = Some(watch_proc);
    wc.hInstance = instance;
    wc.lpszClassName = class.as_ptr();
    // Registered once per process; later listeners find it there.
    if unsafe { RegisterClassW(&wc) } == 0
        && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
    {
        log::warn!("keyboard hook: no session watch (RegisterClassW error {})", unsafe {
            GetLastError()
        });
        return None;
    }
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        log::warn!("keyboard hook: no session watch (CreateWindowExW error {})", unsafe {
            GetLastError()
        });
        return None;
    }
    if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } == 0 {
        // Power resumes still arrive; only unlocks are missed.
        log::warn!("keyboard hook: no unlock notifications (error {})", unsafe { GetLastError() });
    }
    Some(hwnd)
}

/// Removes `old` (if any) and installs the hook again, forgetting held keys (their ups went to
/// the lock screen or were lost in sleep). Returns the new hook, null if that failed (the next
/// unlock or resume tries again).
unsafe fn reinstall(old: HHOOK, instance: HINSTANCE) -> HHOOK {
    if !old.is_null() {
        unsafe { UnhookWindowsHookEx(old) };
    }
    let state = STATE.with(Cell::get);
    if !state.is_null() {
        // SAFETY: owned by this thread (see `run_hook`); the hook is not installed right now.
        unsafe { (*state).press.clear() };
    }
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), instance, 0) };
    if hook.is_null() {
        log::warn!("keyboard hook could not be reinstalled (error {})", unsafe { GetLastError() });
    }
    hook
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
        let instance = GetModuleHandleW(std::ptr::null());
        let mut hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), instance, 0);
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
        let watch = create_watch_window(instance);

        // The hook procedure and the watch window's (sent) notifications run inside
        // GetMessageW. What it returns: WM_REINSTALL from `watch_proc`, anything posted to the
        // watch window, and the WM_QUIT from `HookHandle::drop` (GetMessageW returns 0; -1 is
        // an error, which cannot happen with these arguments but must not spin).
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() && msg.message == WM_REINSTALL {
                log::debug!("session unlocked or system resumed; reinstalling the keyboard hook");
                hook = reinstall(hook, instance);
            } else {
                DispatchMessageW(&msg);
            }
        }

        if let Some(hwnd) = watch {
            WTSUnRegisterSessionNotification(hwnd);
            DestroyWindow(hwnd);
        }
        if !hook.is_null() {
            UnhookWindowsHookEx(hook);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlocks_reconnects_and_resumes_reinstall_the_hook() {
        for event in [WTS_SESSION_UNLOCK, WTS_CONSOLE_CONNECT, WTS_REMOTE_CONNECT] {
            assert!(reinstall_after(WM_WTSSESSION_CHANGE, event as WPARAM));
        }
        for event in [PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND] {
            assert!(reinstall_after(WM_POWERBROADCAST, event as WPARAM));
        }
        // Lock (7) and suspend (4) do not; nothing else does.
        assert!(!reinstall_after(WM_WTSSESSION_CHANGE, 7));
        assert!(!reinstall_after(WM_POWERBROADCAST, 4));
        assert!(!reinstall_after(WM_USER, WTS_SESSION_UNLOCK as WPARAM));
    }
}
