//! X11 backend: XInput2 raw key events (`XI_RawKeyPress` / `XI_RawKeyRelease`) selected on the
//! root window for all master devices, through x11rb's pure-Rust connection (no libX11/libxcb).
//!
//! Raw events are delivered whatever window has the focus and (XI 2.1+) even while another
//! client grabs the keyboard. They carry only the X key code (`detail`, the evdev code + 8);
//! no keysyms, XKB state or text are requested. Under XWayland they only arrive while an X11
//! window has the focus, which is why Wayland sessions use evdev instead.

use super::super::keymap_linux::from_x11_keycode;
use super::super::{InputError, KeyAction, KeyEvent, PressState, backdate};
use super::{Callback, monotonic_ns, poll, pollfd};
use std::os::fd::{AsRawFd, RawFd};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::protocol::Event;
use x11rb::protocol::xinput::{
    self, ConnectionExt as _, Device, EventMask, KeyEventFlags, XIEventMask,
};
use x11rb::rust_connection::RustConnection;

/// Connects to `$DISPLAY` and selects raw key events on the root window. Done on the caller's
/// thread so a missing server or extension can fall back to evdev.
pub fn connect() -> Result<RustConnection, InputError> {
    let fail = |what: &str, e: &dyn std::fmt::Display| {
        InputError::Platform(format!("X11 key listening is unavailable: {what}: {e}"))
    };
    let (conn, screen) =
        x11rb::connect(None).map_err(|e| fail("cannot connect to the X server", &e))?;
    if conn
        .extension_information(xinput::X11_EXTENSION_NAME)
        .map_err(|e| fail("cannot query extensions", &e))?
        .is_none()
    {
        return Err(InputError::Platform(
            "X11 key listening is unavailable: the X server has no XInput extension".into(),
        ));
    }
    // Announcing 2.2 also opts in to the XI 2.1 rule that raw events ignore grabs.
    let version = conn
        .xinput_xi_query_version(2, 2)
        .map_err(|e| fail("XIQueryVersion", &e))?
        .reply()
        .map_err(|e| fail("XIQueryVersion", &e))?;
    if version.major_version < 2 {
        return Err(InputError::Platform(format!(
            "X11 key listening is unavailable: XInput {}.{} is older than 2.0",
            version.major_version, version.minor_version
        )));
    }
    let root = conn
        .setup()
        .roots
        .get(screen)
        .ok_or_else(|| InputError::Platform("X11: no such screen".into()))?
        .root;
    let mask = EventMask {
        deviceid: u16::from(Device::ALL_MASTER),
        mask: vec![XIEventMask::RAW_KEY_PRESS | XIEventMask::RAW_KEY_RELEASE],
    };
    conn.xinput_xi_select_events(root, &[mask])
        .map_err(|e| fail("XISelectEvents", &e))?
        .check()
        .map_err(|e| fail("XISelectEvents", &e))?;
    Ok(conn)
}

/// The listener thread: drains buffered events, then sleeps in `poll` on the X socket and the
/// stop eventfd. Returns on stop or when the connection breaks (the X server went away).
pub fn run(conn: RustConnection, mut callback: Callback, wake: RawFd, _missed: Arc<AtomicU32>) {
    let mut press = PressState::default();
    let x_fd = conn.stream().as_raw_fd();
    loop {
        // Events read while waiting for a reply sit in x11rb's buffer although the socket is
        // no longer readable, so always drain before polling.
        loop {
            match conn.poll_for_event() {
                Ok(Some(event)) => handle(&event, &mut press, &mut callback),
                Ok(None) => break,
                Err(_) => return,
            }
        }
        let mut fds = [pollfd(wake), pollfd(x_fd)];
        if poll(&mut fds).is_err() || fds[0].revents != 0 {
            return;
        }
        if fds[1].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
            && fds[1].revents & libc::POLLIN == 0
        {
            return;
        }
    }
}

fn handle(event: &Event, press: &mut PressState, callback: &mut Callback) {
    let received_ns = crate::clock::now_ns();
    let (raw, action) = match event {
        Event::XinputRawKeyPress(raw) => (raw, KeyAction::Down),
        Event::XinputRawKeyRelease(raw) => (raw, KeyAction::Up),
        _ => return,
    };
    if u32::from(raw.flags) & u32::from(KeyEventFlags::KEY_REPEAT) != 0 {
        return;
    }
    let Some(key) = from_x11_keycode(raw.detail) else { return };
    if !press.accept(key, action) {
        return;
    }
    let event_ns = backdate(received_ns, server_age_ns(raw.time));
    callback(KeyEvent { key, action, event_ns, received_ns });
}

/// How long ago the X server stamped the event. Xorg and XWayland use `CLOCK_MONOTONIC` in
/// milliseconds (wrapping at 2^32), so this is accurate to about 1 ms; a remote or unusual
/// server gives an implausible age, which [`backdate`] ignores.
fn server_age_ns(server_ms: u32) -> u64 {
    let now_ms = (monotonic_ns() / 1_000_000) as u32;
    u64::from(now_ms.wrapping_sub(server_ms)) * 1_000_000
}
