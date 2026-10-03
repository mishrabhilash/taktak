//! A minimal logger: one line per record on stderr. Nothing is written to files or sent
//! anywhere, and nothing this app logs contains a key identity or typed text (`Key`'s `Debug`
//! is redacted in `taktak-core` as a backstop).
//!
//! Messages carry pack folder names, pack errors and device names, which anyone can choose, so
//! every control character is escaped here ([`line`]): a hostile name cannot forge log lines
//! or steer the terminal, whichever call site forgot `pack::printable`.
//!
//! `TAKTAK_LOG` sets the level (`error`, `warn`, `info`, `debug`, `trace`, `off`); the default
//! is `info`. Other crates (Tauri, the file watcher, …) only get through at `warn` and above.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::io::Write;
use taktak_core::pack::printable;

struct Stderr;

static LOGGER: Stderr = Stderr;

/// Whether `target` belongs to TakTak's own crates.
fn ours(target: &str) -> bool {
    target.starts_with("taktak")
}

impl Log for Stderr {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let max = log::max_level();
        let max = if ours(metadata.target()) { max } else { max.min(LevelFilter::Warn) };
        metadata.level() <= max
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let line = line(record.level(), record.target(), &record.args().to_string());
            let _ = writeln!(std::io::stderr().lock(), "{line}");
        }
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

/// One log line, without its line break; control characters in `message` are escaped.
fn line(level: Level, target: &str, message: &str) -> String {
    format!("[TakTak {level:<5} {target}] {}", printable(message))
}

/// The level `spec` names, or `info`.
pub fn level(spec: Option<&str>) -> LevelFilter {
    spec.and_then(|s| s.trim().parse().ok()).unwrap_or(LevelFilter::Info)
}

/// Installs the logger (once; later calls do nothing).
pub fn init() {
    let level = level(std::env::var("TAKTAK_LOG").ok().as_deref());
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_parses_or_defaults_to_info() {
        assert_eq!(level(None), LevelFilter::Info);
        assert_eq!(level(Some("debug")), LevelFilter::Debug);
        assert_eq!(level(Some(" WARN ")), LevelFilter::Warn);
        assert_eq!(level(Some("off")), LevelFilter::Off);
        assert_eq!(level(Some("loud")), LevelFilter::Info);
    }

    #[test]
    fn hostile_names_cannot_forge_lines_or_steer_the_terminal() {
        let folder = "/u/evil\x1b[2J\n[TakTak WARN  forged] x";
        let logged = line(Level::Warn, "taktak_lib::service", &format!("{folder} broke on disk"));
        assert_eq!(
            logged,
            "[TakTak WARN  taktak_lib::service] /u/evil\\u{1b}[2J\\n[TakTak WARN  forged] x \
             broke on disk"
        );
        assert!(!logged.chars().any(char::is_control));
        assert_eq!(line(Level::Info, "t", "plain"), "[TakTak INFO  t] plain");
    }

    #[test]
    fn only_our_crates_log_below_warn() {
        assert!(ours("taktak_lib::service"));
        assert!(ours("taktak_core::pack::registry"));
        assert!(!ours("tauri::manager"));
        assert!(!ours("notify"));
    }
}
