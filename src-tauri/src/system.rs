//! Handing things to the OS: revealing a folder in Finder / Explorer / the file manager, and
//! opening the Input Monitoring pane of System Settings. Local only; nothing here touches the
//! network. Blocks briefly, so call it from an async command, not the main thread.

use std::io;
use std::path::Path;
use std::process::Command;

/// The Privacy & Security → Input Monitoring pane.
#[cfg(target_os = "macos")]
const INPUT_MONITORING: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";

/// Runs `command` and waits for it (`open` returns at once). A non-zero exit is an error.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn run(mut command: Command) -> io::Result<()> {
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{command:?} exited with {status}")))
    }
}

/// Starts `command` without waiting for it, reaping it on a small thread so it never lingers
/// as a zombie (file managers can stay running).
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn spawn(mut command: Command) -> io::Result<()> {
    let mut child = command.spawn()?;
    std::thread::Builder::new().name("taktak-reap".into()).spawn(move || child.wait())?;
    Ok(())
}

/// Creates `dir` if needed and shows it in the platform's file manager.
pub fn reveal_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(target_os = "macos")]
    {
        let mut open = Command::new("/usr/bin/open");
        open.arg(dir);
        run(open)
    }
    #[cfg(target_os = "windows")]
    {
        // Explorer's exit code is meaningless (1 on success), so it is not checked.
        let mut explorer = Command::new("explorer");
        explorer.arg(dir);
        spawn(explorer)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut xdg = Command::new("xdg-open");
        xdg.arg(dir);
        spawn(xdg)
    }
}

/// Opens the settings pane where the user grants keyboard access (macOS: Input Monitoring).
/// Other platforms need no permission, so there is nothing to open.
pub fn open_input_monitoring() -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut open = Command::new("/usr/bin/open");
        open.arg(INPUT_MONITORING);
        run(open)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}
