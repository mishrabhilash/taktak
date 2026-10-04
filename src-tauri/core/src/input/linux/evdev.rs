//! evdev backend: reads key events straight from the keyboard device nodes
//! (`/dev/input/event*`). This is the only way to hear keys on Wayland, which by design has no
//! protocol for reading other clients' input; it also works on X11 and the console.
//!
//! - **Access.** The nodes are readable by root and the `input` group only (logind's seat ACLs
//!   do not cover keyboards). Joining the group (`sudo usermod -aG input "$USER"`, then log
//!   out and in) lets every program the user runs read every keystroke, so it must be an
//!   explicit, informed opt-in. Without it [`open_all`] fails with
//!   [`InputError::PermissionDenied`]. Sandboxes (Flatpak, Snap) hide `/dev/input` entirely.
//! - **Listen-only.** Devices are opened read-only and never grabbed (`EVIOCGRAB`), so every
//!   other reader (the compositor, X) still gets every event. Only `EV_KEY` events are read:
//!   key code and down/up. No keymap, no XKB, no characters.
//! - **Which devices.** Those that report at least one key of the main block, numpad or F1–F12
//!   (codes 1–88): keyboards, keypads, and the keyboard interface of some mice; not power
//!   buttons, lid switches or plain mice. Virtual (uinput) keyboards are included: remappers
//!   such as keyd or kanata grab the physical keyboard and re-emit through one.
//! - **Hotplug.** inotify on `/dev/input` (`IN_CREATE`, and `IN_ATTRIB` because udev applies
//!   permissions after creating a node); unplugged devices fail their next read with `ENODEV`
//!   and are dropped.
//! - **Repeat and overruns.** Auto-repeat (`value` 2) is ignored. `SYN_DROPPED` (the device's
//!   buffer overflowed) discards events up to the next `SYN_REPORT`, forgets held keys and is
//!   counted for [`super::Handle::take_reenabled`].
//! - **Timestamps.** `EVIOCSCLOCKID(CLOCK_MONOTONIC)` makes the kernel stamp events on the
//!   monotonic clock (µs), from which `event_ns` is derived; if a kernel refuses, both
//!   timestamps are the receive time.

use super::super::keymap_linux::from_evdev_code;
use super::super::{InputError, KeyAction, KeyEvent, PressState, backdate};
use super::{Callback, monotonic_ns, poll, pollfd};
use std::ffi::{CString, OsStr};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

const INPUT_DIR: &str = "/dev/input";

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const SYN_REPORT: u16 = 0;
const SYN_DROPPED: u16 = 3;
const KEY_MAX: usize = 0x2FF;
/// A device is a keyboard if it reports any of these codes (Esc … F12, including the numpad).
const KEYBOARD_CODES: std::ops::RangeInclusive<usize> = 1..=88;

/// `struct input_event` from `linux/input.h`: the timestamp is two native `unsigned long`s on
/// every architecture TakTak targets (the kernel's `__kernel_ulong_t`, also with 64-bit
/// `time_t` on 32-bit systems).
#[repr(C)]
#[derive(Clone, Copy)]
struct RawEvent {
    sec: libc::c_ulong,
    usec: libc::c_ulong,
    kind: u16,
    code: u16,
    value: i32,
}

impl RawEvent {
    const ZERO: RawEvent = RawEvent { sec: 0, usec: 0, kind: 0, code: 0, value: 0 };

    #[allow(clippy::useless_conversion)] // c_ulong is u32 on 32-bit targets
    fn time_ns(&self) -> u64 {
        u64::from(self.sec)
            .saturating_mul(1_000_000_000)
            .saturating_add(u64::from(self.usec).saturating_mul(1_000))
    }
}

// ioctl request encoding (`asm-generic/ioctl.h`; a few architectures use other field widths).
#[cfg(any(
    target_arch = "powerpc",
    target_arch = "powerpc64",
    target_arch = "mips",
    target_arch = "mips64",
    target_arch = "mips32r6",
    target_arch = "mips64r6",
    target_arch = "sparc",
    target_arch = "sparc64"
))]
mod ioc {
    pub const WRITE: u64 = 4;
    pub const READ: u64 = 2;
    pub const SIZE_BITS: u64 = 13;
}
#[cfg(not(any(
    target_arch = "powerpc",
    target_arch = "powerpc64",
    target_arch = "mips",
    target_arch = "mips64",
    target_arch = "mips32r6",
    target_arch = "mips64r6",
    target_arch = "sparc",
    target_arch = "sparc64"
)))]
mod ioc {
    pub const WRITE: u64 = 1;
    pub const READ: u64 = 2;
    pub const SIZE_BITS: u64 = 14;
}

const fn evdev_ioc(dir: u64, nr: u64, size: usize) -> u64 {
    (dir << (16 + ioc::SIZE_BITS)) | ((size as u64) << 16) | ((b'E' as u64) << 8) | nr
}

/// `EVIOCGBIT(ev, len)`: which codes of event type `ev` the device reports.
const fn eviocgbit(ev: u16, len: usize) -> u64 {
    evdev_ioc(ioc::READ, 0x20 + ev as u64, len)
}

/// `EVIOCSCLOCKID`: which clock stamps this file's events.
const EVIOCSCLOCKID: u64 = evdev_ioc(ioc::WRITE, 0xA0, size_of::<libc::c_int>());

fn bit(bits: &[u8], n: usize) -> bool {
    bits.get(n / 8).is_some_and(|b| b & (1 << (n % 8)) != 0)
}

pub struct Device {
    /// N of `/dev/input/eventN`.
    number: u32,
    fd: OwnedFd,
    /// Whether the kernel stamps this device's events with `CLOCK_MONOTONIC`.
    monotonic: bool,
    /// Between `SYN_DROPPED` and the next `SYN_REPORT`.
    dropping: bool,
}

/// Whether keyboards can be read, for `has_permission`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// At least one keyboard is readable.
    Readable,
    /// Some device nodes exist but none could be opened, and no keyboard was readable.
    Denied,
    /// No keyboard device right now (hotplug will pick one up), or no `/dev/input` at all.
    NoDevices,
}

struct Scan {
    devices: Vec<Device>,
    denied: usize,
    dir_error: Option<io::Error>,
}

fn event_number(name: &OsStr) -> Option<u32> {
    name.as_bytes().strip_prefix(b"event").and_then(|n| std::str::from_utf8(n).ok()?.parse().ok())
}

fn scan() -> Scan {
    let mut scan = Scan { devices: Vec::new(), denied: 0, dir_error: None };
    let entries = match std::fs::read_dir(INPUT_DIR) {
        Ok(entries) => entries,
        Err(e) => {
            scan.dir_error = Some(e);
            return scan;
        }
    };
    let mut numbers: Vec<u32> =
        entries.filter_map(|e| event_number(&e.ok()?.file_name())).collect();
    numbers.sort_unstable();
    for number in numbers {
        match open_keyboard(number) {
            Ok(Some(device)) => scan.devices.push(device),
            Ok(None) => {}
            Err(e) if matches!(e.raw_os_error(), Some(libc::EACCES | libc::EPERM)) => {
                scan.denied += 1
            }
            Err(_) => {}
        }
    }
    scan
}

pub fn access() -> Access {
    let scan = scan();
    if !scan.devices.is_empty() {
        Access::Readable
    } else if scan.denied > 0 {
        Access::Denied
    } else {
        Access::NoDevices
    }
}

/// Opens every readable keyboard, on the caller's thread so that refusal is reported by
/// `input::start`.
pub fn open_all() -> Result<Vec<Device>, InputError> {
    let scan = scan();
    if let Some(e) = scan.dir_error {
        return Err(if e.kind() == io::ErrorKind::PermissionDenied {
            InputError::PermissionDenied
        } else {
            InputError::Platform(format!(
                "cannot list {INPUT_DIR} ({e}); sandboxed packages (Flatpak, Snap) hide the \
                 input devices"
            ))
        });
    }
    if scan.devices.is_empty() && scan.denied > 0 {
        return Err(InputError::PermissionDenied);
    }
    Ok(scan.devices)
}

/// Opens `/dev/input/eventN` read-only; `Ok(None)` if it is not a keyboard.
fn open_keyboard(number: u32) -> io::Result<Option<Device>> {
    let path = CString::new(format!("{INPUT_DIR}/event{number}")).expect("no NUL in path");
    // SAFETY: valid C string; the result is checked before it is wrapped.
    let raw =
        unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a fresh descriptor we own.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    if !is_keyboard(fd.as_raw_fd()) {
        return Ok(None);
    }
    let clock: libc::c_int = libc::CLOCK_MONOTONIC;
    // SAFETY: EVIOCSCLOCKID reads one c_int from the pointer.
    let monotonic = unsafe { libc::ioctl(fd.as_raw_fd(), EVIOCSCLOCKID as _, &clock) } == 0;
    Ok(Some(Device { number, fd, monotonic, dropping: false }))
}

fn is_keyboard(fd: RawFd) -> bool {
    let mut types = [0u8; 4];
    let mut keys = [0u8; KEY_MAX / 8 + 1];
    // SAFETY: each EVIOCGBIT writes at most the length encoded in the request into the buffer.
    unsafe {
        if libc::ioctl(fd, eviocgbit(0, types.len()) as _, types.as_mut_ptr()) < 0
            || !bit(&types, EV_KEY as usize)
            || libc::ioctl(fd, eviocgbit(EV_KEY, keys.len()) as _, keys.as_mut_ptr()) < 0
        {
            return false;
        }
    }
    KEYBOARD_CODES.into_iter().any(|code| bit(&keys, code))
}

/// inotify on `/dev/input` for new nodes and permission changes; `None` disables hotplug.
fn watch_dir() -> Option<OwnedFd> {
    // SAFETY: plain syscalls; results checked.
    unsafe {
        let raw = libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC);
        if raw < 0 {
            return None;
        }
        let fd = OwnedFd::from_raw_fd(raw);
        let dir = CString::new(INPUT_DIR).expect("no NUL in path");
        if libc::inotify_add_watch(raw, dir.as_ptr(), libc::IN_CREATE | libc::IN_ATTRIB) < 0 {
            return None;
        }
        Some(fd)
    }
}

/// Drains the inotify fd into `numbers` (the `eventN` nodes that appeared or changed);
/// returns `true` if the queue overflowed and the directory must be rescanned.
fn read_notify(fd: RawFd, numbers: &mut Vec<u32>) -> bool {
    const HEADER: usize = size_of::<libc::inotify_event>();
    let mut buf = [0u8; 4096];
    let mut overflow = false;
    loop {
        // SAFETY: reading into a valid buffer of the given length.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            if n < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return overflow;
        }
        let bytes = &buf[..n as usize];
        let mut at = 0;
        while at + HEADER <= bytes.len() {
            // SAFETY: a whole header lies within `bytes`; read_unaligned has no alignment needs.
            let event: libc::inotify_event =
                unsafe { std::ptr::read_unaligned(bytes[at..].as_ptr().cast()) };
            let name_end = (at + HEADER + event.len as usize).min(bytes.len());
            let name = &bytes[at + HEADER..name_end];
            let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
            if event.mask & libc::IN_Q_OVERFLOW != 0 {
                overflow = true;
            } else if let Some(number) = event_number(OsStr::from_bytes(name)) {
                numbers.push(number);
            }
            at = name_end;
        }
    }
}

/// The listener thread: sleeps in `poll` on the stop eventfd, the inotify fd and every
/// keyboard; returns on stop.
pub fn run(mut devices: Vec<Device>, mut callback: Callback, wake: RawFd, missed: Arc<AtomicU32>) {
    let mut press = PressState::default();
    let notify = watch_dir();
    let mut buf = [RawEvent::ZERO; 64];
    let mut fds: Vec<libc::pollfd> = Vec::new();
    let mut changed: Vec<u32> = Vec::new();
    loop {
        fds.clear();
        fds.push(pollfd(wake));
        // poll ignores negative descriptors.
        fds.push(pollfd(notify.as_ref().map_or(-1, AsRawFd::as_raw_fd)));
        fds.extend(devices.iter().map(|d| pollfd(d.fd.as_raw_fd())));
        if poll(&mut fds).is_err() || fds[0].revents != 0 {
            return;
        }

        // Highest index first, so swap_remove only moves devices already handled.
        for i in (0..devices.len()).rev() {
            if fds[i + 2].revents != 0
                && !read_device(&mut devices[i], &mut buf, &mut press, &mut callback, &missed)
            {
                devices.swap_remove(i);
                // Keys held on it will never send their ups.
                press.clear();
            }
        }

        if fds[1].revents != 0
            && let Some(notify) = &notify
        {
            changed.clear();
            if read_notify(notify.as_raw_fd(), &mut changed)
                && let Ok(entries) = std::fs::read_dir(INPUT_DIR)
            {
                changed.extend(entries.filter_map(|e| event_number(&e.ok()?.file_name())));
            }
            for &number in &changed {
                if !devices.iter().any(|d| d.number == number)
                    && let Ok(Some(device)) = open_keyboard(number)
                {
                    devices.push(device);
                }
            }
        }
    }
}

/// Reads everything queued on `device`. Returns `false` if the device is gone.
fn read_device(
    device: &mut Device,
    buf: &mut [RawEvent],
    press: &mut PressState,
    callback: &mut Callback,
    missed: &AtomicU32,
) -> bool {
    let capacity = size_of_val(buf);
    loop {
        // SAFETY: reading into a valid buffer of plain-old-data structs of the given size.
        let n = unsafe { libc::read(device.fd.as_raw_fd(), buf.as_mut_ptr().cast(), capacity) };
        if n < 0 {
            return match io::Error::last_os_error().kind() {
                io::ErrorKind::WouldBlock => true,
                io::ErrorKind::Interrupted => continue,
                _ => false, // ENODEV: unplugged
            };
        }
        if n == 0 {
            return false;
        }
        let received_ns = crate::clock::now_ns();
        let now_mono = if device.monotonic { monotonic_ns() } else { 0 };
        for event in &buf[..n as usize / size_of::<RawEvent>()] {
            match (event.kind, event.code) {
                (EV_SYN, SYN_DROPPED) => {
                    device.dropping = true;
                    press.clear();
                    missed.fetch_add(1, Ordering::Relaxed);
                }
                (EV_SYN, SYN_REPORT) => device.dropping = false,
                (EV_KEY, code) if !device.dropping => {
                    let action = match event.value {
                        0 => KeyAction::Up,
                        1 => KeyAction::Down,
                        _ => continue, // 2: auto-repeat
                    };
                    let Some(key) = from_evdev_code(code) else { continue };
                    if !press.accept(key, action) {
                        continue;
                    }
                    let event_ns = if device.monotonic {
                        backdate(received_ns, now_mono.saturating_sub(event.time_ns()))
                    } else {
                        received_ns
                    };
                    callback(KeyEvent { key, action, event_ns, received_ns });
                }
                _ => {}
            }
        }
        if (n as usize) < capacity {
            return true;
        }
    }
}
