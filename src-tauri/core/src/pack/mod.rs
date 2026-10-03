//! Sound packs: manifest parsing and validation, folder/zip sources, audio decoding,
//! loading into a [`crate::audio::SoundBank`], and the hot-reloading registry.
//! The format is specified in `docs/pack-format.md`; this module is its implementation.

pub mod decode;
pub mod load;
pub mod manifest;
pub mod registry;
pub mod source;

use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;

pub use load::{LoadedPack, check_all, inspect, load};
pub use manifest::{Manifest, SoundSet};
pub use registry::{PackRegistry, RegistryEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// One validation finding. `location` points into the pack, e.g. `"pack.json:14:9"`,
/// `"keys.keya"`, `"groups.space.press[1]"`, `"sounds/a.wav"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub severity: Severity,
    pub location: String,
    pub message: String,
}

impl Problem {
    pub fn error(location: impl Into<String>, message: impl Into<String>) -> Problem {
        Problem { severity: Severity::Error, location: location.into(), message: message.into() }
    }

    pub fn warning(location: impl Into<String>, message: impl Into<String>) -> Problem {
        Problem { severity: Severity::Warning, location: location.into(), message: message.into() }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        // Locations and messages quote pack contents with escapes already; this is the
        // backstop that keeps a hostile pack from forging lines or steering a terminal.
        write!(f, "{sev:<7} {:<18} {}", printable(&self.location), printable(&self.message))
    }
}

/// A pack that failed to open, validate or load. Carries *all* problems found (errors and
/// warnings), so authors can fix everything in one pass.
#[derive(Clone, Debug)]
pub struct PackError {
    pub pack: PathBuf,
    pub problems: Vec<Problem>,
}

impl PackError {
    pub fn new(pack: impl Into<PathBuf>, problems: Vec<Problem>) -> PackError {
        PackError { pack: pack.into(), problems }
    }

    pub fn single(pack: impl Into<PathBuf>, problem: Problem) -> PackError {
        PackError::new(pack, vec![problem])
    }

    pub fn errors(&self) -> impl Iterator<Item = &Problem> {
        self.problems.iter().filter(|p| p.is_error())
    }
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let errors = self.problems.iter().filter(|p| p.is_error()).count();
        let warnings = self.problems.len() - errors;
        write!(
            f,
            "{}: {errors} error{}, {warnings} warning{}",
            printable(&self.pack.display().to_string()),
            if errors == 1 { "" } else { "s" },
            if warnings == 1 { "" } else { "s" },
        )?;
        for p in &self.problems {
            write!(f, "\n  {p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PackError {}

/// `text` with control characters (line breaks, escape sequences) escaped, for printing pack
/// contents and file names: a name like `"evil\x1b[2J"` must not reach a terminal raw.
pub fn printable(text: &str) -> Cow<'_, str> {
    if !text.chars().any(char::is_control) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if c.is_control() {
            out.extend(c.escape_debug());
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Where a pack was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackOrigin {
    Bundled,
    User,
}

/// Display metadata for a pack, available without decoding any audio.
#[derive(Clone, Debug, PartialEq)]
pub struct PackInfo {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub author: String,
    pub license: String,
    pub description: Option<String>,
    pub source: Option<String>,
    pub attribution: Option<String>,
    /// The pack folder or `.zip` file.
    pub location: PathBuf,
    pub origin: PackOrigin,
}
