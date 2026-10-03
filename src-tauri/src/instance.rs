//! One TakTak per user on macOS. A second launch that starts a new process (`open -n`, running
//! the binary again) hands over to the running instance, which shows its settings window, and
//! exits before it creates anything. (Opening TakTak from Finder, Spotlight or `open` while it
//! runs starts no process at all: macOS sends the running one a reopen event, see `lib.rs`.)
//!
//! Both files live in the user's own app data folder (`~/Library/Application Support/
//! tech.taktak.app`), never in the shared `/tmp`, so another account can neither block nor
//! impersonate this user's instance:
//! - `instance.lock`, held with an exclusive `flock` for the process's lifetime. The OS lets
//!   go when the process ends, however it ends. Whoever holds it is the instance.
//! - `instance.sock`, a Unix socket (mode 0600) the instance listens on. A second launch
//!   connects, only to a socket this user owns, and closes again: the connection is the whole
//!   message, so nothing (no arguments, no working directory) is passed along.
//!
//! On Windows and Linux `tauri-plugin-single-instance` does this per user session.

use std::fs::{self, File, OpenOptions, Permissions, TryLockError};
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const LOCK: &str = "instance.lock";
const SOCKET: &str = "instance.sock";
/// A second launch tries this often to reach an instance that may still be starting.
const NOTIFY_ATTEMPTS: u32 = 20;
const NOTIFY_RETRY: Duration = Duration::from_millis(50);

/// What [`claim`] found.
pub enum Claim {
    /// This process is the instance; keep the [`Instance`] until it exits.
    First(Instance),
    /// Another instance runs (and was asked to show its settings window): exit.
    Second,
}

/// The lock that makes this process the instance, and the socket later launches reach it on.
pub struct Instance {
    _lock: File,
    socket: Option<(PathBuf, UnixListener)>,
}

/// The folder for the lock and socket: the app data folder.
pub fn default_dir() -> Option<PathBuf> {
    taktak_core::pack::registry::default_user_dir().and_then(|d| d.parent().map(Path::to_owned))
}

/// Becomes the instance for this user, or hands over to the one that is.
pub fn claim(dir: &Path) -> io::Result<Claim> {
    fs::create_dir_all(dir)?;
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(LOCK))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            notify(&dir.join(SOCKET));
            return Ok(Claim::Second);
        }
        Err(TryLockError::Error(e)) => return Err(e),
    }
    let path = dir.join(SOCKET);
    let socket = match bind(&path) {
        Ok(listener) => Some((path, listener)),
        Err(e) => {
            // Still the only instance; a second launch just cannot ask for Settings.
            log::warn!("second launches cannot reach TakTak: {e}");
            None
        }
    };
    Ok(Claim::First(Instance { _lock: lock, socket }))
}

/// Listens on `path`, replacing a socket an instance that was killed left behind (nobody
/// listens on it: we hold the lock).
fn bind(path: &Path) -> io::Result<UnixListener> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Asks the running instance to show its settings window. Best effort: it may still be
/// starting (it holds the lock before it listens).
fn notify(path: &Path) {
    for _ in 0..NOTIFY_ATTEMPTS {
        if owned_socket(path) && UnixStream::connect(path).is_ok() {
            return;
        }
        thread::sleep(NOTIFY_RETRY);
    }
    log::info!("TakTak is already running but did not answer");
}

/// Whether `path` is a socket this user owns: never talk to one someone else put there.
fn owned_socket(path: &Path) -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    let uid = unsafe { libc::geteuid() };
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_socket() && m.uid() == uid)
}

impl Instance {
    /// Calls `on_launch` (on the `taktak-instance` thread, which blocks between launches)
    /// whenever another launch hands over.
    pub fn listen(&self, on_launch: impl Fn() + Send + 'static) -> io::Result<()> {
        let Some((_, listener)) = &self.socket else { return Ok(()) };
        let listener = listener.try_clone()?;
        thread::Builder::new().name("taktak-instance".into()).spawn(move || {
            for connection in listener.incoming() {
                match connection {
                    // Connecting is the message; nothing is read from it.
                    Ok(_) => on_launch(),
                    Err(e) => {
                        log::warn!("second-launch socket: {e}");
                        thread::sleep(Duration::from_secs(1));
                    }
                }
            }
        })?;
        Ok(())
    }

    /// Removes the socket (on quit). The lock goes with the process.
    pub fn release(&self) {
        if let Some((path, _)) = &self.socket {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn first(dir: &Path) -> Instance {
        match claim(dir).unwrap() {
            Claim::First(instance) => instance,
            Claim::Second => panic!("another instance holds {}", dir.display()),
        }
    }

    #[test]
    fn a_second_launch_hands_over_and_does_not_run() {
        let dir = tempfile::tempdir().unwrap();
        let instance = first(dir.path());
        let (tx, launches) = mpsc::channel();
        instance.listen(move || tx.send(()).unwrap()).unwrap();
        assert!(matches!(claim(dir.path()).unwrap(), Claim::Second));
        launches.recv_timeout(Duration::from_secs(5)).expect("the instance heard the launch");
        // Only this user can reach the socket.
        let mode = fs::metadata(dir.path().join(SOCKET)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        // Once the instance is gone, the next launch is the instance (its lock went with it).
        instance.release();
        drop(instance);
        let _next = first(dir.path());
    }

    #[test]
    fn a_socket_left_by_a_killed_instance_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        drop(UnixListener::bind(dir.path().join(SOCKET)).unwrap());
        assert!(dir.path().join(SOCKET).exists());
        let instance = first(dir.path());
        let (tx, launches) = mpsc::channel();
        instance.listen(move || tx.send(()).unwrap()).unwrap();
        assert!(matches!(claim(dir.path()).unwrap(), Claim::Second));
        launches.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn only_sockets_this_user_owns_are_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plain");
        fs::write(&file, "").unwrap();
        assert!(!owned_socket(&file), "not a socket");
        assert!(!owned_socket(&dir.path().join("missing")));
        let socket = dir.path().join("s.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        assert!(owned_socket(&socket));
    }

    #[test]
    fn the_folder_is_the_users_app_data_folder() {
        if let Some(dir) = default_dir() {
            assert!(dir.ends_with("tech.taktak.app"), "{}", dir.display());
            assert!(!dir.starts_with("/tmp") && !dir.starts_with("/private/tmp"));
        }
    }
}
