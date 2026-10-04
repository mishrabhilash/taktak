//! Linux backends, chosen at start:
//!
//! - **X11** ([`x11`]): XInput2 raw key events on the root window. Listen-only, no special
//!   rights, sees every key typed into any X11 window.
//! - **evdev** ([`evdev`]): reads `/dev/input/event*` directly. Works on Wayland, X11 and the
//!   console, but needs read access to the device nodes (normally membership of the `input`
//!   group, which lets every program the user runs read every keystroke).
//!
//! Choice: `TAKTAK_INPUT=x11` or `TAKTAK_INPUT=evdev` forces one. Otherwise a Wayland session
//! (`WAYLAND_DISPLAY` set or `XDG_SESSION_TYPE=wayland`) or no `DISPLAY` uses evdev, because
//! XWayland only delivers keys typed into X11 windows; an X11 session uses XInput2 and falls
//! back to evdev if the X server or its XInput extension cannot be used.
//!
//! Both run on a dedicated `taktak-input` thread that sleeps in `poll(2)` on its event sources
//! plus an eventfd that [`Handle`]'s `Drop` signals: no busy loop, no timeout, prompt stop.

mod evdev;
mod x11;

use super::{InputError, KeyEvent};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::JoinHandle;

type Callback = Box<dyn FnMut(KeyEvent) + Send>;

/// Forces a backend: `x11` or `evdev`.
const BACKEND_ENV: &str = "TAKTAK_INPUT";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    X11,
    Evdev,
}

/// Which backend to use and whether the user forced it (no fallback then).
fn choose() -> (Backend, bool) {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    match std::env::var(BACKEND_ENV).ok().as_deref().map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("x11") => return (Backend::X11, true),
        Some(v) if v.eq_ignore_ascii_case("evdev") => return (Backend::Evdev, true),
        _ => {}
    }
    let wayland = var("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"));
    if wayland || var("DISPLAY").is_none() {
        (Backend::Evdev, false)
    } else {
        (Backend::X11, false)
    }
}

pub fn start(callback: Callback) -> Result<Handle, InputError> {
    let (backend, forced) = choose();
    if backend == Backend::X11 {
        match x11::connect() {
            Ok(conn) => {
                log::info!("keyboard: X11 XInput2 raw key events");
                return spawn(move |wake, missed| x11::run(conn, callback, wake, missed));
            }
            Err(e) if forced => return Err(e),
            Err(e) => log::warn!("{e}; trying the input devices (evdev) instead"),
        }
    }
    let devices = evdev::open_all()?;
    log::info!("keyboard: evdev, {} keyboard device(s)", devices.len());
    spawn(move |wake, missed| evdev::run(devices, callback, wake, missed))
}

/// X11: always (any X client may listen). evdev: whether keyboard devices can be read.
pub fn has_permission() -> bool {
    match choose().0 {
        Backend::X11 => true,
        Backend::Evdev => evdev::access() != evdev::Access::Denied,
    }
}

/// Linux has no permission prompt: group membership is the user's (or admin's) decision.
pub fn request_permission() -> bool {
    has_permission()
}

pub struct Handle {
    wake: Arc<OwnedFd>,
    missed: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
}

impl Handle {
    /// Times the kernel dropped events (evdev `SYN_DROPPED`, a full device buffer) since the
    /// last call; keys in that window may have made no sound.
    pub fn take_reenabled(&self) -> u32 {
        self.missed.swap(0, Ordering::Relaxed)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let one: u64 = 1;
        // SAFETY: writing 8 bytes from a valid u64 to our eventfd.
        unsafe {
            libc::write(self.wake.as_raw_fd(), (&one as *const u64).cast(), size_of::<u64>())
        };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn spawn<F>(body: F) -> Result<Handle, InputError>
where
    F: FnOnce(RawFd, Arc<AtomicU32>) + Send + 'static,
{
    // SAFETY: plain syscall; the result is checked before it is wrapped.
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if fd < 0 {
        return Err(InputError::Platform(format!("eventfd: {}", io::Error::last_os_error())));
    }
    // SAFETY: `fd` is a fresh descriptor we own.
    let wake = Arc::new(unsafe { OwnedFd::from_raw_fd(fd) });
    let missed = Arc::new(AtomicU32::new(0));
    let (thread_wake, thread_missed) = (wake.clone(), missed.clone());
    // Everything that can fail (connecting, opening devices) already happened on the caller's
    // thread, so `start` can report it; events arriving before the thread polls stay queued
    // in the connection or the device buffers.
    let thread = std::thread::Builder::new()
        .name("taktak-input".into())
        .spawn(move || body(thread_wake.as_raw_fd(), thread_missed))
        .map_err(|e| InputError::Platform(e.to_string()))?;
    Ok(Handle { wake, missed, thread: Some(thread) })
}

/// Blocks until at least one of `fds` is ready (no timeout), retrying on EINTR.
fn poll(fds: &mut [libc::pollfd]) -> io::Result<()> {
    loop {
        // SAFETY: `fds` is a valid, exclusively borrowed slice of pollfd.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if n >= 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

fn pollfd(fd: RawFd) -> libc::pollfd {
    libc::pollfd { fd, events: libc::POLLIN, revents: 0 }
}

/// `CLOCK_MONOTONIC` in ns: the clock evdev timestamps use (once set with `EVIOCSCLOCKID`) and
/// that Xorg and XWayland derive their millisecond timestamps from.
fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: valid out pointer; CLOCK_MONOTONIC always exists on Linux.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}
