//! `relaunch` (`docs/ui-contract.md` § Relaunch): quit like `quit`, and end with exactly one new
//! TakTak running. For when macOS only lets a relaunched TakTak listen to the keyboard.
//!
//! The old process starts the new one and then quits normally (settings flushed, audio
//! stopped, `instance.sock` removed). The new one is started with [`FLAG`], so it does not hand
//! over to the old one (which is still running when the new one starts): it first waits up to
//! [`LOCK_WAIT`] for the old process to let go of its instance lock, which happens only when the
//! old process has exited. With [`ONBOARDING_FLAG`] it opens the onboarding window again.
//!
//! - macOS: the instance lock is `instance.lock` (`instance.rs`). A TakTak running from its
//!   `.app` bundle is started through LaunchServices (`open -n`), the way the user starts it, so
//!   macOS attributes Input Monitoring to TakTak itself rather than to the exiting parent.
//! - Windows and Linux: the single-instance plugin lets go at exit, before the process ends, so
//!   the first instance also holds [`ExitLock`] for its lifetime and the new one waits for it
//!   before the plugin runs.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

/// Passed to the new instance: wait for the old one instead of handing over to it.
pub const FLAG: &str = "--relaunch";
/// Passed to the new instance when the onboarding window was open: open it again.
pub const ONBOARDING_FLAG: &str = "--onboarding";
/// How long the new instance waits for the old one to exit.
pub const LOCK_WAIT: Duration = Duration::from_secs(5);
/// How often it tries the lock meanwhile.
const LOCK_RETRY: Duration = Duration::from_millis(50);
/// The lock file the first instance holds on Windows and Linux.
#[cfg_attr(target_os = "macos", allow(dead_code))]
const EXIT_LOCK: &str = "running.lock";

/// The new instance's arguments.
pub fn args(onboarding: bool) -> Vec<&'static str> {
    let mut args = vec![FLAG];
    if onboarding {
        args.push(ONBOARDING_FLAG);
    }
    args
}

/// Whether this process was started by `relaunch` (`args` without the program name).
pub fn relaunched(args: &[String]) -> bool {
    args.iter().any(|a| a == FLAG)
}

/// Whether the onboarding window should open again (`args` without the program name).
pub fn reopen_onboarding(args: &[String]) -> bool {
    args.iter().any(|a| a == ONBOARDING_FLAG)
}

/// The `.app` bundle that `exe` runs from (`…/TakTak.app/Contents/MacOS/taktak`), if any.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    is_bundle.then(|| bundle.to_path_buf())
}

/// Takes `file`'s exclusive lock, trying for up to `timeout`. `Ok(false)` if another process
/// still holds it then.
pub fn try_lock_for(file: &File, timeout: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(true),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(LOCK_RETRY),
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Error(e)) => return Err(e),
        }
    }
}

/// Starts the new instance (see the module docs). Returns once it has been started; the
/// caller then quits.
pub fn spawn(onboarding: bool) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    #[cfg(target_os = "macos")]
    if let Some(bundle) = bundle_of(&exe) {
        let mut open = Command::new("/usr/bin/open");
        open.arg("-n").arg(&bundle).arg("--args").args(args(onboarding));
        // `open` returns once LaunchServices has started the app.
        let status = open.status()?;
        return if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("open exited with {status}")))
        };
    }
    // A binary outside a bundle (development builds): start it directly. It outlives us.
    Command::new(exe).args(args(onboarding)).spawn().map(drop)
}

/// The folder for the lock: the app data folder (where the user packs folder lives).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn lock_dir() -> Option<PathBuf> {
    taktak_core::pack::registry::default_user_dir().and_then(|d| d.parent().map(Path::to_owned))
}

/// Windows and Linux: held by the running instance for its whole life, so a relaunched one can
/// tell when it is gone (see the module docs). The OS lets go when the process ends.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub struct ExitLock {
    _file: File,
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
impl ExitLock {
    /// Takes the lock in `dir`. Another holder (an instance that is still exiting) is waited
    /// for, up to [`LOCK_WAIT`].
    pub fn hold(dir: &Path) -> io::Result<ExitLock> {
        fs::create_dir_all(dir)?;
        let file = open_lock(&dir.join(EXIT_LOCK))?;
        if try_lock_for(&file, LOCK_WAIT)? {
            Ok(ExitLock { _file: file })
        } else {
            Err(io::Error::other("another TakTak still holds the lock"))
        }
    }
}

/// Windows and Linux, in a relaunched instance: waits up to `timeout` for the previous instance
/// to exit (its [`ExitLock`] to go). Returns whether it did.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn wait_for_previous(dir: &Path, timeout: Duration) -> bool {
    let Ok(file) = open_lock(&dir.join(EXIT_LOCK)) else { return true };
    // Locked and dropped at once: only the wait matters.
    try_lock_for(&file, timeout).unwrap_or(true)
}

fn open_lock(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).truncate(false).write(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_new_instance_is_told_why_it_runs() {
        assert_eq!(args(false), [FLAG]);
        assert_eq!(args(true), [FLAG, ONBOARDING_FLAG]);
        assert!(relaunched(&strings(&["--relaunch", "--onboarding"])));
        assert!(reopen_onboarding(&strings(&["--relaunch", "--onboarding"])));
        assert!(!relaunched(&strings(&["--selftest"])));
        assert!(!reopen_onboarding(&strings(&[])));
    }

    #[test]
    fn bundles_are_found_from_their_executable() {
        assert_eq!(
            bundle_of(Path::new("/Applications/TakTak.app/Contents/MacOS/taktak")),
            Some(PathBuf::from("/Applications/TakTak.app"))
        );
        assert_eq!(
            bundle_of(Path::new("/Users/me/My Apps/TakTak.app/Contents/MacOS/taktak")),
            Some(PathBuf::from("/Users/me/My Apps/TakTak.app"))
        );
        assert_eq!(bundle_of(Path::new("/repo/target/debug/taktak")), None);
        assert_eq!(bundle_of(Path::new("/x/TakTak.app/Contents/Resources/taktak")), None);
        assert_eq!(bundle_of(Path::new("/x/TakTak/Contents/MacOS/taktak")), None);
        assert_eq!(bundle_of(Path::new("taktak")), None);
    }

    #[test]
    fn a_relaunched_instance_waits_for_the_old_one_to_exit() {
        let dir = tempfile::tempdir().unwrap();
        let old = ExitLock::hold(dir.path()).unwrap();
        // The old instance is still exiting: the wait times out.
        assert!(!wait_for_previous(dir.path(), Duration::from_millis(100)));
        // It exits while the new one waits.
        let release = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            drop(old);
        });
        let started = Instant::now();
        assert!(wait_for_previous(dir.path(), Duration::from_secs(5)));
        assert!(started.elapsed() >= Duration::from_millis(100));
        release.join().unwrap();
        // The new instance then holds the lock itself.
        let _new = ExitLock::hold(dir.path()).unwrap();
        assert!(!wait_for_previous(dir.path(), Duration::ZERO));
    }
}
