//! The files of a pack being imported: a folder or a `.zip`, its `config.json` found by
//! basename at any depth, and lenient resolution of the file names a config refers to.

use super::ImportError;
use crate::pack::source::{MAX_ZIP_COMPRESSION_RATIO, MAX_ZIP_ENTRIES, MAX_ZIP_UNCOMPRESSED_BYTES};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Largest config file read (real configs are 0.3–15 KB).
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
/// Largest audio file read (official sprites are at most 2.7 MB).
pub(crate) const MAX_SOURCE_AUDIO_BYTES: u64 = 64 * 1024 * 1024;
/// How deep a folder is searched for `config.json` and audio.
const MAX_DEPTH: usize = 6;
/// Zip entries up to this size skip the ratio check (silence compresses very well).
const RATIO_CHECK_MIN_BYTES: u64 = 1024 * 1024;

pub(crate) struct Input {
    /// The folder or zip file name, e.g. `cherrymx-black-abs` or `nk-cream.zip`.
    pub origin_name: String,
    /// Pack-relative path of the config (`config.json`, or `CONFIG.JSON`).
    pub config_name: String,
    /// Every file under the pack root, pack-relative with `/` separators.
    pub files: BTreeSet<String>,
    backend: Backend,
}

enum Backend {
    Dir { root: PathBuf },
    Zip { archive: Box<zip::ZipArchive<File>>, entries: BTreeMap<String, usize> },
}

impl Input {
    pub(crate) fn open(src: &Path) -> Result<Input, ImportError> {
        let meta = fs::metadata(src).map_err(|e| ImportError::io(src, e))?;
        let origin_name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "pack".into());
        if meta.is_dir() {
            let mut all = Vec::new();
            walk(src, "", 0, &mut all)?;
            let (prefix, config) = locate_config(all.iter().map(String::as_str))?;
            let files = under(&all, &prefix);
            let root = src.join(prefix.trim_end_matches('/'));
            Ok(Input { origin_name, config_name: config, files, backend: Backend::Dir { root } })
        } else if src.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
            open_zip(src, origin_name)
        } else {
            Err(ImportError::NotAPack(
                "expected a Mechvibes pack folder (with config.json) or a .zip of one".into(),
            ))
        }
    }

    /// A sibling of the config, if the pack has it (case-insensitive).
    pub(crate) fn find_exact_ci(&self, rel: &str) -> Option<String> {
        if self.files.contains(rel) {
            return Some(rel.to_owned());
        }
        let lower = rel.to_lowercase();
        self.files.iter().find(|f| f.to_lowercase() == lower).cloned()
    }

    pub(crate) fn read(&mut self, rel: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
        match &mut self.backend {
            Backend::Dir { root } => {
                let path = root.join(rel);
                let len = fs::metadata(&path).map_err(|e| e.to_string())?.len();
                if len > max_bytes {
                    return Err(format!("file is larger than {} MB", max_bytes / (1024 * 1024)));
                }
                let mut bytes = Vec::with_capacity(len as usize);
                File::open(&path)
                    .and_then(|f| f.take(max_bytes).read_to_end(&mut bytes))
                    .map_err(|e| e.to_string())?;
                Ok(bytes)
            }
            Backend::Zip { archive, entries } => {
                let index = *entries.get(rel).ok_or("file not found in the zip")?;
                let entry = archive.by_index(index).map_err(|e| e.to_string())?;
                let declared = entry.size();
                if declared > max_bytes {
                    return Err(format!("file is larger than {} MB", max_bytes / (1024 * 1024)));
                }
                // Never inflate past the declared size: that is what the size checks bound.
                let mut bytes = Vec::with_capacity(declared as usize);
                entry.take(declared).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                Ok(bytes)
            }
        }
    }

    pub(crate) fn read_config(&mut self, rel: &str) -> Result<Vec<u8>, ImportError> {
        self.read(rel, MAX_CONFIG_BYTES)
            .map_err(|e| ImportError::InvalidConfig(format!("cannot read {rel}: {e}")))
    }

    /// Whether the pack holds any file other than configs and documents.
    pub(crate) fn has_audio_candidates(&self) -> bool {
        self.files.iter().any(|f| {
            let lower = f.to_lowercase();
            !(lower.ends_with(".json")
                || lower.ends_with(".backup")
                || lower.ends_with(".txt")
                || lower.ends_with(".md")
                || lower.ends_with(".png")
                || lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.rsplit('/').next().is_some_and(|n| n.starts_with("license")))
        })
    }

    /// Finds the file a config refers to, as leniently as Mechvibes users need: the exact
    /// path, then ignoring case, then a unique file with that name anywhere in the pack, then
    /// (for names without an extension) a unique `name.*`.
    pub(crate) fn resolve(&self, reference: &str) -> Option<String> {
        let normalized = normalize_reference(reference)?;
        if let Some(found) = self.find_exact_ci(&normalized) {
            return Some(found);
        }
        let base = normalized.rsplit('/').next().unwrap_or(&normalized).to_lowercase();
        let by_name: Vec<&String> = self.files.iter().filter(|f| basename_lc(f) == base).collect();
        if let [only] = by_name.as_slice() {
            return Some((*only).clone());
        }
        if !base.contains('.') {
            let stem: Vec<&String> = self
                .files
                .iter()
                .filter(|f| basename_lc(f).rsplit_once('.').is_some_and(|(s, _)| s == base))
                .collect();
            if let [only] = stem.as_slice() {
                return Some((*only).clone());
            }
        }
        None
    }
}

fn basename_lc(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_lowercase()
}

/// `\` → `/`, without `./` or leading `/`; `None` for empty names or `..` segments.
fn normalize_reference(reference: &str) -> Option<String> {
    let replaced = reference.trim().replace('\\', "/");
    let mut parts = Vec::new();
    for part in replaced.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            other => parts.push(other),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// macOS resource forks and Finder files, which are never pack content.
fn is_junk(path: &str) -> bool {
    path.split('/').any(|seg| seg == "__MACOSX")
        || path.rsplit('/').next().is_some_and(|n| n.starts_with("._") || n == ".DS_Store")
}

fn walk(dir: &Path, rel: &str, depth: usize, out: &mut Vec<String>) -> Result<(), ImportError> {
    let entries = fs::read_dir(dir).map_err(|e| ImportError::io(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| ImportError::io(dir, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "__MACOSX" {
            continue;
        }
        let path = entry.path();
        let child = if rel.is_empty() { name } else { format!("{rel}/{name}") };
        // Folder symlinks are not followed (no loops, nothing outside the pack); file
        // symlinks are read like files.
        let link_meta = fs::symlink_metadata(&path).map_err(|e| ImportError::io(&path, e))?;
        if link_meta.is_dir() {
            if depth < MAX_DEPTH {
                walk(&path, &child, depth + 1, out)?;
            }
        } else if fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            out.push(child);
        }
        if out.len() > MAX_ZIP_ENTRIES {
            return Err(ImportError::NotAPack(format!(
                "the folder holds more than {MAX_ZIP_ENTRIES} files; choose the pack's own folder"
            )));
        }
    }
    Ok(())
}

/// The shallowest `config.json` (any letter case). Returns the pack root as a prefix
/// (`""` or `"folder/"`) and the config's name relative to it.
fn locate_config<'a>(
    paths: impl Iterator<Item = &'a str>,
) -> Result<(String, String), ImportError> {
    let mut found: Vec<&str> =
        paths.filter(|p| !is_junk(p) && basename_lc(p) == "config.json").collect();
    found.sort_by_key(|p| (p.matches('/').count(), *p));
    let Some(first) = found.first() else {
        return Err(ImportError::NotAPack("no config.json found".into()));
    };
    let depth = first.matches('/').count();
    if let Some(second) = found.get(1).filter(|p| p.matches('/').count() == depth) {
        return Err(ImportError::NotAPack(format!(
            "found more than one pack ({first:?} and {second:?}); import them one at a time"
        )));
    }
    let (prefix, name) = match first.rsplit_once('/') {
        Some((dir, name)) => (format!("{dir}/"), name.to_owned()),
        None => (String::new(), (*first).to_owned()),
    };
    Ok((prefix, name))
}

fn under(all: &[String], prefix: &str) -> BTreeSet<String> {
    all.iter()
        .filter(|p| !is_junk(p))
        .filter_map(|p| p.strip_prefix(prefix))
        .map(str::to_owned)
        .collect()
}

fn open_zip(src: &Path, origin_name: String) -> Result<Input, ImportError> {
    let bad = |m: String| ImportError::NotAPack(format!("{}: {m}", src.display()));
    let file = File::open(src).map_err(|e| ImportError::io(src, e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| bad(format!("not a valid zip: {e}")))?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(bad(format!("more than {MAX_ZIP_ENTRIES} entries")));
    }
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).map_err(|e| bad(format!("corrupt entry: {e}")))?;
        if !entry.is_file() {
            continue;
        }
        let (size, compressed) = (entry.size(), entry.compressed_size());
        if size > RATIO_CHECK_MIN_BYTES
            && size > compressed.saturating_mul(MAX_ZIP_COMPRESSION_RATIO)
        {
            return Err(bad("an entry expands over 100:1; rejected as a possible zip bomb".into()));
        }
        total = total.saturating_add(size);
        // `enclosed_name` drops entries that would escape the folder and turns `\` into `/`.
        let Some(path) = entry.enclosed_name() else { continue };
        let name: Vec<String> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        if name.is_empty() {
            continue;
        }
        names.entry(name.join("/")).or_insert(i);
    }
    if total > MAX_ZIP_UNCOMPRESSED_BYTES {
        return Err(bad(format!(
            "expands to more than {} MB",
            MAX_ZIP_UNCOMPRESSED_BYTES / (1024 * 1024)
        )));
    }
    if archive.has_overlapping_files().unwrap_or(true) {
        return Err(bad("entries share compressed data (a zip-bomb technique)".into()));
    }
    let (prefix, config) = locate_config(names.keys().map(String::as_str))?;
    let entries: BTreeMap<String, usize> = names
        .iter()
        .filter(|(p, _)| !is_junk(p))
        .filter_map(|(p, &i)| p.strip_prefix(&prefix).map(|rel| (rel.to_owned(), i)))
        .collect();
    let files = entries.keys().cloned().collect();
    Ok(Input {
        origin_name,
        config_name: config,
        files,
        backend: Backend::Zip { archive: Box::new(archive), entries },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(files: &[&str]) -> Input {
        Input {
            origin_name: "t".into(),
            config_name: "config.json".into(),
            files: files.iter().map(|s| s.to_string()).collect(),
            backend: Backend::Dir { root: PathBuf::new() },
        }
    }

    #[test]
    fn resolution_is_lenient() {
        let i = input(&["config.json", "BACKSPACE.mp3", "press/ENTER.mp3", "sfx-blink", "k.wav"]);
        assert_eq!(i.resolve("BACKSPACE.mp3").as_deref(), Some("BACKSPACE.mp3"));
        assert_eq!(i.resolve("backspace.mp3").as_deref(), Some("BACKSPACE.mp3"));
        assert_eq!(i.resolve("./press\\enter.MP3").as_deref(), Some("press/ENTER.mp3"));
        assert_eq!(i.resolve("release/ENTER.mp3").as_deref(), Some("press/ENTER.mp3"));
        assert_eq!(i.resolve("sfx-blink").as_deref(), Some("sfx-blink"));
        assert_eq!(i.resolve("k").as_deref(), Some("k.wav"));
        assert_eq!(i.resolve("../k.wav"), None);
        assert_eq!(i.resolve("missing.wav"), None);
        assert_eq!(i.resolve("  "), None);
    }

    #[test]
    fn ambiguous_basenames_do_not_resolve() {
        let i = input(&["a/x.wav", "b/x.wav"]);
        assert_eq!(i.resolve("x.wav"), None);
        assert_eq!(i.resolve("b/x.wav").as_deref(), Some("b/x.wav"));
    }

    #[test]
    fn config_is_found_shallowest_and_junk_is_ignored() {
        let paths = ["__MACOSX/p/._config.json", "p/config.json", "p/sub/config.json", "p/a.wav"];
        let (prefix, name) = locate_config(paths.iter().copied()).unwrap();
        assert_eq!((prefix.as_str(), name.as_str()), ("p/", "config.json"));
        let (prefix, name) = locate_config(["CONFIG.JSON"].iter().copied()).unwrap();
        assert_eq!((prefix.as_str(), name.as_str()), ("", "CONFIG.JSON"));
        assert!(locate_config(["a/config.json", "b/config.json"].iter().copied()).is_err());
        assert!(locate_config(["readme.txt"].iter().copied()).is_err());
    }
}
