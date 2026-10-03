//! Reading pack files from a folder or a `.zip`, with path-traversal and zip-bomb guards.

use super::Problem;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use zip::result::ZipError;

pub const MAX_AUDIO_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_ZIP_ENTRIES: usize = 5_000;
pub const MAX_ZIP_UNCOMPRESSED_BYTES: u64 = 200 * 1024 * 1024;
pub const MAX_ZIP_COMPRESSION_RATIO: u64 = 100;

/// Entries up to this size skip the ratio check. Small files legitimately compress far
/// beyond 100:1 (a run of digital silence, a padded JSON file), and they cannot hurt: the
/// guard exists to stop a tiny entry inflating to hundreds of MB, and below this size the
/// total-size limit already bounds them, because no entry is ever read past its declared size
/// and no two entries may share compressed data.
const RATIO_CHECK_MIN_BYTES: u64 = 1024 * 1024;

/// A valid pack zip holds at most 200 MB of content plus per-entry headers, so anything much
/// larger is rejected before its central directory is parsed into memory.
const MAX_ZIP_FILE_BYTES: u64 = MAX_ZIP_UNCOMPRESSED_BYTES + 16 * 1024 * 1024;

const MANIFEST: &str = "pack.json";

/// An opened pack. For zips, `root_prefix` is `""` or `"<top-folder>/"`.
pub enum PackSource {
    Dir { root: PathBuf, cache: DirCache },
    Zip { path: PathBuf, archive: zip::ZipArchive<std::fs::File>, root_prefix: String },
}

/// The canonical root of a folder pack and the directory listings read so far. Listings back
/// the case-sensitive lookups and are read once per directory, not once per file.
pub struct DirCache {
    canonical_root: PathBuf,
    /// Pack-relative directory (`""` for the root) → exact names of its entries.
    listings: HashMap<String, HashSet<OsString>>,
}

/// Why a pack-relative path does not name a readable regular file.
enum Missing {
    /// No entry with exactly this name. `hint` is an entry differing only in letter case.
    NotFound {
        hint: Option<String>,
    },
    Invalid(String),
}

impl Missing {
    fn message(self) -> String {
        match self {
            Missing::NotFound { hint: Some(hint) } => {
                format!(
                    "file not found; names are case-sensitive and the pack has {}",
                    quoted(&hint)
                )
            }
            Missing::NotFound { hint: None } => "file not found in the pack".into(),
            Missing::Invalid(message) => message,
        }
    }
}

impl PackSource {
    /// Opens a pack folder (must contain `pack.json`) or a `.zip` (pack.json at the root or
    /// inside exactly one top-level folder). Validates zip entry names and limits up front.
    pub fn open(path: &Path) -> Result<PackSource, Problem> {
        let meta = fs::metadata(path)
            .map_err(|e| Problem::error("pack", format!("cannot open: {}", io_reason(&e))))?;
        if meta.is_dir() {
            open_dir(path)
        } else if meta.is_file()
            && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        {
            open_zip(path, meta.len())
        } else {
            Err(Problem::error(
                "pack",
                "not a pack: expected a folder with pack.json or a .zip file",
            ))
        }
    }

    /// Reads a file by its pack-relative path (already syntax-checked by
    /// `manifest::check_path`). Case-sensitive on every platform; rejects anything that
    /// resolves outside the pack root (symlinks included) and files over `max_bytes`.
    pub fn read(&mut self, rel: &str, max_bytes: u64) -> Result<Vec<u8>, Problem> {
        let fail = |message: String| Problem::error(rel, message);
        match self {
            PackSource::Dir { cache, .. } => {
                let (path, len) = cache.find(rel).map_err(|m| fail(m.message()))?;
                if len > max_bytes {
                    return Err(fail(too_large(len, max_bytes)));
                }
                let file = File::open(&path)
                    .map_err(|e| fail(format!("cannot read: {}", io_reason(&e))))?;
                // A file that grew between the size check and the read.
                read_capped(file, len, max_bytes).map_err(|e| {
                    fail(e.message(|| {
                        format!("file is larger than the limit of {}", human_size(max_bytes))
                    }))
                })
            }
            PackSource::Zip { archive, root_prefix, .. } => {
                check_syntax(rel).map_err(|m| fail(format!("invalid path: {m}")))?;
                let full = format!("{root_prefix}{rel}");
                let index = zip_index(archive, &full).ok_or_else(|| {
                    fail(
                        Missing::NotFound { hint: zip_case_hint(archive, root_prefix, rel) }
                            .message(),
                    )
                })?;
                let entry = archive.by_index(index).map_err(|e| fail(zip_reason(&e)))?;
                if entry.is_dir() {
                    return Err(fail("is a folder, not a file".into()));
                }
                if entry.is_symlink() {
                    return Err(fail("is a symbolic link, not a file".into()));
                }
                let declared = entry.size();
                if declared > max_bytes {
                    return Err(fail(too_large(declared, max_bytes)));
                }
                // Declared sizes can lie, and the deflate reader does not stop at them. Reading
                // no more than declared is what makes the size and ratio checks in `open_zip`
                // (which only see declared sizes) bound the bytes actually inflated.
                read_capped(entry, declared, declared).map_err(|e| {
                    fail(e.message(|| {
                        format!(
                            "the zip entry holds more data than its declared size of {}; the zip \
                             is corrupt or was tampered with",
                            human_size(declared)
                        )
                    }))
                })
            }
        }
    }

    /// Whether `rel` names an existing regular file in the pack (case-sensitive).
    pub fn exists(&mut self, rel: &str) -> bool {
        match self {
            PackSource::Dir { cache, .. } => cache.find(rel).is_ok(),
            PackSource::Zip { archive, root_prefix, .. } => {
                check_syntax(rel).is_ok()
                    && zip_index(archive, &format!("{root_prefix}{rel}"))
                        .and_then(|i| archive.by_index_raw(i).ok())
                        .is_some_and(|entry| entry.is_file())
            }
        }
    }

    /// The folder or zip path this source was opened from.
    pub fn location(&self) -> &Path {
        match self {
            PackSource::Dir { root, .. } => root,
            PackSource::Zip { path, .. } => path,
        }
    }
}

fn open_dir(path: &Path) -> Result<PackSource, Problem> {
    let canonical_root = fs::canonicalize(path)
        .map_err(|e| Problem::error("pack", format!("cannot open: {}", io_reason(&e))))?;
    let mut cache = DirCache { canonical_root, listings: HashMap::new() };
    match cache.find(MANIFEST) {
        Ok(_) => Ok(PackSource::Dir { root: path.to_path_buf(), cache }),
        Err(Missing::NotFound { hint }) => Err(Problem::error(
            MANIFEST,
            match hint {
                Some(hint) => {
                    format!(
                        "missing; found {} but the name must be exactly pack.json",
                        quoted(&hint)
                    )
                }
                None => "missing; a pack folder must contain pack.json".into(),
            },
        )),
        Err(missing) => Err(Problem::error(MANIFEST, missing.message())),
    }
}

impl DirCache {
    /// Resolves `rel` to a regular file inside the root, matching every component's name
    /// exactly. Returns the resolved path and the file size.
    fn find(&mut self, rel: &str) -> Result<(PathBuf, u64), Missing> {
        check_syntax(rel).map_err(|m| Missing::Invalid(format!("invalid path: {m}")))?;
        // macOS and Windows file systems match names case-insensitively, so opening the path
        // directly would accept "Click.WAV" for "click.wav". Compare against the listings.
        let segments: Vec<&str> = rel.split('/').collect();
        let mut dir = String::new();
        for (i, segment) in segments.iter().enumerate() {
            let names = self.listing(&dir)?;
            if !names.contains(OsStr::new(segment)) {
                let lower = segment.to_lowercase();
                let hint = names
                    .iter()
                    .filter_map(|n| n.to_str())
                    .filter(|n| n.to_lowercase() == lower)
                    .min()
                    .map(|n| if dir.is_empty() { n.to_owned() } else { format!("{dir}/{n}") });
                return Err(Missing::NotFound { hint });
            }
            if i + 1 < segments.len() {
                if !dir.is_empty() {
                    dir.push('/');
                }
                dir.push_str(segment);
            }
        }

        let resolved = self.resolve_inside(rel)?;
        let meta = fs::metadata(&resolved)
            .map_err(|e| Missing::Invalid(format!("cannot read: {}", io_reason(&e))))?;
        if !meta.is_file() {
            return Err(Missing::Invalid("not a regular file".into()));
        }
        Ok((resolved, meta.len()))
    }

    /// Exact entry names of the pack-relative directory `dir`, read on first use.
    fn listing(&mut self, dir: &str) -> Result<&HashSet<OsString>, Missing> {
        if !self.listings.contains_key(dir) {
            let resolved = self.resolve_inside(dir)?;
            if !resolved.is_dir() {
                return Err(Missing::Invalid(format!("\"{dir}\" is not a folder")));
            }
            let names = fs::read_dir(&resolved)
                .map_err(|e| Missing::Invalid(format!("cannot list \"{dir}\": {}", io_reason(&e))))?
                .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
                .collect();
            self.listings.insert(dir.to_owned(), names);
        }
        Ok(&self.listings[dir])
    }

    /// Canonicalizes a pack-relative path and requires it to stay under the root, which
    /// catches symlinks (of files or folders) pointing out of the pack.
    fn resolve_inside(&self, rel: &str) -> Result<PathBuf, Missing> {
        let mut path = self.canonical_root.clone();
        path.extend(rel.split('/').filter(|s| !s.is_empty()));
        let resolved = fs::canonicalize(&path)
            .map_err(|e| Missing::Invalid(format!("cannot resolve: {}", io_reason(&e))))?;
        if resolved.starts_with(&self.canonical_root) {
            Ok(resolved)
        } else {
            Err(Missing::Invalid("resolves outside the pack folder (symbolic link?)".into()))
        }
    }
}

fn open_zip(path: &Path, file_len: u64) -> Result<PackSource, Problem> {
    let zip_error = |message: String| Problem::error("zip", message);
    if file_len > MAX_ZIP_FILE_BYTES {
        return Err(zip_error(format!(
            "zip file is {}; packs are limited to {} uncompressed",
            human_size(file_len),
            human_size(MAX_ZIP_UNCOMPRESSED_BYTES)
        )));
    }
    let file =
        File::open(path).map_err(|e| zip_error(format!("cannot open: {}", io_reason(&e))))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| zip_error(format!("not a valid zip file: {e}")))?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(zip_error(format!(
            "zip has {} entries; the limit is {MAX_ZIP_ENTRIES}",
            archive.len()
        )));
    }

    let mut names = Vec::with_capacity(archive.len());
    let mut unsafe_names = Vec::new();
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let entry = archive
            .by_index_raw(i)
            .map_err(|e| zip_error(format!("corrupt zip entry #{}: {e}", i + 1)))?;
        // Many zip tools write UTF-8 names without setting the UTF-8 flag; prefer the raw
        // bytes when they are valid UTF-8 (lookups by name then match them byte for byte).
        let name = std::str::from_utf8(entry.name_raw()).unwrap_or(entry.name()).to_owned();
        if let Err(reason) = check_entry_escape(&name) {
            unsafe_names.push(format!("{} ({reason})", quoted(&name)));
            continue;
        }
        let (size, compressed) = (entry.size(), entry.compressed_size());
        if size > RATIO_CHECK_MIN_BYTES
            && size > compressed.saturating_mul(MAX_ZIP_COMPRESSION_RATIO)
        {
            return Err(zip_error(format!(
                "entry {} expands from {} to {} (over {MAX_ZIP_COMPRESSION_RATIO}:1); \
                 rejected as a possible zip bomb",
                quoted(&name),
                human_size(compressed),
                human_size(size)
            )));
        }
        total = total.saturating_add(size);
        names.push((name, entry.is_dir()));
    }

    if let Some(first) = unsafe_names.first() {
        return Err(zip_error(format!(
            "unsafe entry path {first}{}; zip entries must not have absolute paths, drive \
             prefixes or \"..\" segments",
            and_more(unsafe_names.len())
        )));
    }
    if total > MAX_ZIP_UNCOMPRESSED_BYTES {
        return Err(zip_error(format!(
            "zip expands to {}; the limit is {}",
            human_size(total),
            human_size(MAX_ZIP_UNCOMPRESSED_BYTES)
        )));
    }

    let root_prefix = zip_root(&names).map_err(zip_error)?;
    // Entries outside the pack folder are never read; only the pack's own names must be ones
    // that pack.json could refer to.
    let bad_names: Vec<String> = names
        .iter()
        .filter(|(name, _)| name.starts_with(&root_prefix) && !is_ignored(name))
        .filter_map(|(name, _)| {
            check_pack_entry_name(name).err().map(|reason| format!("{} ({reason})", quoted(name)))
        })
        .collect();
    if let Some(first) = bad_names.first() {
        return Err(zip_error(format!(
            "invalid entry name {first}{}; names in the pack folder must use / as the separator",
            and_more(bad_names.len())
        )));
    }
    // Entries sharing one compressed stream would each inflate it again.
    match archive.has_overlapping_files() {
        Ok(false) => {}
        Ok(true) => {
            return Err(zip_error(
                "zip entries share compressed data, a zip-bomb technique; re-create the zip with \
                 a standard tool"
                    .into(),
            ));
        }
        Err(e) => return Err(zip_error(format!("corrupt zip: {e}"))),
    }
    Ok(PackSource::Zip { path: path.to_path_buf(), archive, root_prefix })
}

fn and_more(count: usize) -> String {
    match count {
        0 | 1 => String::new(),
        n => format!(" and {} more", n - 1),
    }
}

/// Finds the pack root: pack.json at the zip root, else the one top-level folder that
/// contains pack.json. macOS metadata and directory entries are not considered.
fn zip_root(entries: &[(String, bool)]) -> Result<String, String> {
    let files = entries.iter().filter(|(name, is_dir)| !is_dir && !is_ignored(name));
    let mut folders = BTreeSet::new();
    for (name, _) in files.clone() {
        if name == MANIFEST {
            return Ok(String::new());
        }
        if let Some((top, MANIFEST)) = name.split_once('/') {
            folders.insert(top);
        }
    }
    let mut folders = folders.into_iter();
    match (folders.next(), folders.next()) {
        (Some(top), None) => Ok(format!("{top}/")),
        (Some(a), Some(b)) => Err(format!(
            "zip contains more than one pack ({}, {}); put each pack in its own zip",
            quoted(&format!("{a}/")),
            quoted(&format!("{b}/"))
        )),
        (None, _) => {
            const LAYOUT: &str = "the zip must contain pack.json at its root, or a single \
                                  folder that contains pack.json";
            let near_miss = files.map(|(name, _)| name.as_str()).find(|name| {
                name.rsplit(['/', '\\']).next().is_some_and(|f| f.eq_ignore_ascii_case(MANIFEST))
            });
            Err(match near_miss {
                Some(name) if name.contains('\\') => {
                    format!(
                        "found {}, but zip entry names must use / as the separator, not \\; \
                         re-create the zip with a standard tool",
                        quoted(name)
                    )
                }
                Some(name) if name.ends_with(MANIFEST) => {
                    format!("pack.json is at {}, too deep; {LAYOUT}", quoted(name))
                }
                Some(name) => {
                    format!(
                        "found {} but the name must be exactly pack.json; {LAYOUT}",
                        quoted(name)
                    )
                }
                None => format!("no pack.json found; {LAYOUT}"),
            })
        }
    }
}

/// macOS Finder adds `__MACOSX/` resource forks and `.DS_Store` files to zips it creates.
fn is_ignored(name: &str) -> bool {
    name == "__MACOSX"
        || name.starts_with("__MACOSX/")
        || name.rsplit('/').next() == Some(".DS_Store")
}

/// Entry names that would escape the folder the zip is extracted into make the zip invalid,
/// wherever they are in it. `\` counts as a separator here, as it does when extracting on
/// Windows; a drive prefix is a letter and a colon (`C:`).
fn check_entry_escape(name: &str) -> Result<(), &'static str> {
    let bytes = name.as_bytes();
    if name.starts_with(['/', '\\']) {
        Err("absolute path")
    } else if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        Err("drive prefix")
    } else if name.split(['/', '\\']).any(|segment| segment == "..") {
        Err("contains \"..\"")
    } else {
        Ok(())
    }
}

/// Names inside the pack folder: `/` separators only (a name with `\` could never match a path
/// in pack.json), and no NUL.
fn check_pack_entry_name(name: &str) -> Result<(), &'static str> {
    if name.contains('\0') {
        Err("contains a NUL character")
    } else if name.contains('\\') {
        Err("contains a backslash")
    } else {
        Ok(())
    }
}

/// Exact-name lookup. The fast path matches the raw name bytes (UTF-8 names); the scan
/// covers names the zip crate decoded from CP437.
fn zip_index(archive: &zip::ZipArchive<File>, name: &str) -> Option<usize> {
    archive.index_for_name(name).or_else(|| archive.file_names().position(|n| n == name))
}

fn zip_case_hint(archive: &zip::ZipArchive<File>, root_prefix: &str, rel: &str) -> Option<String> {
    let wanted = format!("{root_prefix}{rel}").to_lowercase();
    archive
        .file_names()
        .find(|n| n.to_lowercase() == wanted)
        .and_then(|n| n.strip_prefix(root_prefix).map(str::to_owned))
}

/// Defensive re-check of the manifest path rules: never trust a caller with the file system.
/// `:` is rejected everywhere (drive prefixes, NTFS alternate data streams).
fn check_syntax(rel: &str) -> Result<(), &'static str> {
    if rel.is_empty() {
        Err("empty path")
    } else if rel.starts_with('/') {
        Err("absolute paths are not allowed")
    } else if rel.contains('\\') {
        Err("use / as the separator, not \\")
    } else if rel.contains('\0') {
        Err("contains a NUL character")
    } else if rel.contains(':') {
        Err("':' is not allowed")
    } else if rel.split('/').any(str::is_empty) {
        Err("empty path segment")
    } else if rel.split('/').any(|s| s == "." || s == "..") {
        Err("'.' and '..' segments are not allowed")
    } else {
        Ok(())
    }
}

/// Why [`read_capped`] failed.
enum ReadFailure {
    Io(io::Error),
    /// There was more than `max_bytes`.
    OverLimit,
}

impl ReadFailure {
    /// The I/O error, or `over_limit`'s explanation (it depends on what the limit was).
    fn message(self, over_limit: impl FnOnce() -> String) -> String {
        match self {
            ReadFailure::Io(e) => format!("cannot read: {e}"),
            ReadFailure::OverLimit => over_limit(),
        }
    }
}

/// Reads at most `max_bytes`, failing if there is more (a file that grew, or a zip entry
/// whose declared size was a lie). `expected` only sizes the buffer.
fn read_capped(reader: impl Read, expected: u64, max_bytes: u64) -> Result<Vec<u8>, ReadFailure> {
    let mut buf = Vec::with_capacity(expected.min(max_bytes) as usize);
    reader.take(max_bytes.saturating_add(1)).read_to_end(&mut buf).map_err(ReadFailure::Io)?;
    if buf.len() as u64 > max_bytes {
        return Err(ReadFailure::OverLimit);
    }
    Ok(buf)
}

fn too_large(len: u64, max_bytes: u64) -> String {
    let (len_text, max_text) = (human_size(len), human_size(max_bytes));
    if len_text == max_text {
        format!("file is {len} bytes; the limit is {max_bytes} bytes")
    } else {
        format!("file is {len_text}; the limit is {max_text}")
    }
}

/// A file name for a message, quoted, with control characters escaped.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.escape_debug())
}

fn human_size(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} bytes")
    }
}

fn io_reason(e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::NotFound => "not found".into(),
        io::ErrorKind::PermissionDenied => "permission denied".into(),
        _ => e.to_string(),
    }
}

fn zip_reason(e: &ZipError) -> String {
    match e {
        ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED) => {
            "is encrypted; password-protected zips are not supported".into()
        }
        ZipError::CompressionMethodNotSupported(_) => {
            "uses an unsupported compression method; re-zip with standard (deflate) compression"
                .into()
        }
        other => format!("cannot read from zip: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::CompressionMethod;
    use zip::write::SimpleFileOptions;

    const PACK_JSON: &[u8] = br#"{"format": 1}"#;

    type Entries<'a> = &'a [(&'a str, &'a [u8])];

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn dir_pack(files: Entries) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (rel, bytes) in files {
            write(tmp.path(), rel, bytes);
        }
        tmp
    }

    fn open_err(path: &Path) -> Problem {
        match PackSource::open(path) {
            Ok(_) => panic!("{} opened", path.display()),
            Err(p) => p,
        }
    }

    /// A zip at `<tmp>/pack.zip`. Names ending in `/` become directory entries.
    fn zip_pack(entries: Entries, method: CompressionMethod) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("pack.zip");
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = SimpleFileOptions::default().compression_method(method);
        for (name, bytes) in entries {
            if name.ends_with('/') {
                w.add_directory(*name, options).unwrap();
            } else {
                w.start_file(*name, options).unwrap();
                w.write_all(bytes).unwrap();
            }
        }
        w.finish().unwrap();
        (tmp, path)
    }

    fn zip_stored(entries: Entries) -> (tempfile::TempDir, PathBuf) {
        zip_pack(entries, CompressionMethod::Stored)
    }

    fn root_prefix(source: &PackSource) -> &str {
        match source {
            PackSource::Zip { root_prefix, .. } => root_prefix,
            PackSource::Dir { .. } => panic!("not a zip"),
        }
    }

    /// Whether the temp dir's file system ignores letter case (macOS and Windows defaults).
    fn case_insensitive_fs(dir: &Path) -> bool {
        fs::write(dir.join("CaseProbe"), b"").unwrap();
        let insensitive = dir.join("caseprobe").exists();
        fs::remove_file(dir.join("CaseProbe")).unwrap();
        insensitive
    }

    // ---- folders -------------------------------------------------------------------------

    #[test]
    fn dir_reads_root_and_nested_files() {
        let tmp = dir_pack(&[
            ("pack.json", PACK_JSON),
            ("click.wav", b"root"),
            ("sounds/a.wav", b"nested"),
            ("sounds/deep/er/b.wav", b"deeper"),
        ]);
        let mut source = PackSource::open(tmp.path()).unwrap();
        assert_eq!(source.location(), tmp.path());
        assert_eq!(source.read("pack.json", 100).unwrap(), PACK_JSON);
        assert_eq!(source.read("click.wav", 100).unwrap(), b"root");
        assert_eq!(source.read("sounds/a.wav", 100).unwrap(), b"nested");
        assert_eq!(source.read("sounds/deep/er/b.wav", 100).unwrap(), b"deeper");
        assert!(source.exists("sounds/deep/er/b.wav"));
        assert!(!source.exists("sounds/missing.wav"));
        assert!(!source.exists("nope/a.wav"));
        let err = source.read("sounds/missing.wav", 100).unwrap_err();
        assert_eq!(err.location, "sounds/missing.wav");
        assert_eq!(err.message, "file not found in the pack");
    }

    #[test]
    fn dir_requires_exact_pack_json() {
        let tmp = dir_pack(&[("sounds/a.wav", b"x")]);
        let err = open_err(tmp.path());
        assert_eq!(
            (err.location.as_str(), err.message.as_str()),
            ("pack.json", "missing; a pack folder must contain pack.json")
        );

        let tmp = dir_pack(&[("Pack.JSON", PACK_JSON)]);
        let err = open_err(tmp.path());
        assert_eq!(err.location, "pack.json");
        assert!(err.message.contains("\"Pack.JSON\""), "{}", err.message);

        let tmp = dir_pack(&[("pack.json/inner", b"x")]);
        assert_eq!(open_err(tmp.path()).message, "not a regular file");
    }

    #[test]
    fn open_rejects_missing_paths_and_other_files() {
        let tmp = dir_pack(&[("notes.txt", b"hi")]);
        let err = open_err(&tmp.path().join("missing"));
        assert_eq!(
            (err.location.as_str(), err.message.as_str()),
            ("pack", "cannot open: not found")
        );
        let err = open_err(&tmp.path().join("notes.txt"));
        assert_eq!(err.location, "pack");
        assert!(err.message.starts_with("not a pack"), "{}", err.message);
    }

    #[test]
    fn dir_matching_is_case_sensitive_everywhere() {
        let tmp = dir_pack(&[("pack.json", PACK_JSON), ("Sounds/Click.wav", b"x")]);
        if case_insensitive_fs(tmp.path()) {
            // The OS itself would happily open the wrong-case name; the listings must not.
            assert!(tmp.path().join("sounds/click.wav").is_file());
        }
        let mut source = PackSource::open(tmp.path()).unwrap();
        assert_eq!(source.read("Sounds/Click.wav", 10).unwrap(), b"x");
        for wrong in ["sounds/Click.wav", "Sounds/click.wav", "SOUNDS/CLICK.WAV", "Pack.json"] {
            assert!(!source.exists(wrong), "{wrong}");
            let err = source.read(wrong, 10).unwrap_err();
            assert!(err.message.contains("case-sensitive"), "{wrong}: {}", err.message);
        }
        let err = source.read("sounds/Click.wav", 10).unwrap_err();
        assert_eq!(
            err.message,
            "file not found; names are case-sensitive and the pack has \"Sounds\""
        );
        let err = source.read("Sounds/click.wav", 10).unwrap_err();
        assert!(err.message.ends_with("the pack has \"Sounds/Click.wav\""), "{}", err.message);
    }

    #[test]
    fn dir_rejects_oversized_files() {
        let tmp = dir_pack(&[("pack.json", PACK_JSON), ("big.wav", &[7; 2000])]);
        let mut source = PackSource::open(tmp.path()).unwrap();
        assert_eq!(source.read("big.wav", 2000).unwrap().len(), 2000);
        let err = source.read("big.wav", 1999).unwrap_err();
        assert_eq!(err.location, "big.wav");
        assert_eq!(err.message, "file is 2000 bytes; the limit is 1999 bytes");
        let err = source.read("big.wav", 100).unwrap_err();
        assert_eq!(err.message, "file is 2.0 KB; the limit is 100 bytes");
    }

    #[test]
    fn read_capped_stops_at_the_limit() {
        // A file that grows between stat and read, or a zip entry that lies about its size.
        assert_eq!(read_capped(&[1u8; 10][..], 0, 10).ok().unwrap().len(), 10);
        let err = read_capped(&[1u8; 11][..], 5, 10).err().unwrap();
        assert!(matches!(err, ReadFailure::OverLimit));
    }

    #[test]
    fn dir_rejects_bad_syntax_defensively() {
        let tmp = dir_pack(&[("pack.json", PACK_JSON), ("a.wav", b"x"), ("sounds/b.wav", b"y")]);
        let mut source = PackSource::open(tmp.path()).unwrap();
        for bad in [
            "",
            "/a.wav",
            "../a.wav",
            "sounds/../a.wav",
            "./a.wav",
            "sounds//b.wav",
            "sounds/",
            "sounds\\b.wav",
            "C:/a.wav",
            "a.wav:stream",
            "a\0.wav",
        ] {
            assert!(!source.exists(bad), "{bad:?}");
            let err = source.read(bad, 100).unwrap_err();
            assert!(err.message.starts_with("invalid path: "), "{bad:?}: {}", err.message);
        }
    }

    #[test]
    fn dir_rejects_folders_as_files() {
        let tmp = dir_pack(&[("pack.json", PACK_JSON), ("sounds/x.wav/inner", b"x")]);
        let mut source = PackSource::open(tmp.path()).unwrap();
        assert!(!source.exists("sounds/x.wav"));
        assert_eq!(source.read("sounds/x.wav", 100).unwrap_err().message, "not a regular file");
        let err = source.read("pack.json/a.wav", 100).unwrap_err();
        assert_eq!(err.message, "\"pack.json\" is not a folder");
    }

    #[cfg(unix)]
    #[test]
    fn dir_rejects_symlinks_escaping_the_pack() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "secret.wav", b"secret");
        let tmp = dir_pack(&[("pack.json", PACK_JSON), ("sounds/a.wav", b"inside")]);
        let root = tmp.path();
        symlink(outside.path().join("secret.wav"), root.join("sounds/evil.wav")).unwrap();
        symlink(outside.path(), root.join("linked")).unwrap();
        symlink("sounds/a.wav", root.join("alias.wav")).unwrap();
        symlink("../sounds", root.join("sounds/up-and-back")).unwrap();
        symlink("a.wav", root.join("sounds/same-dir.wav")).unwrap();

        let mut source = PackSource::open(root).unwrap();
        for escaping in ["sounds/evil.wav", "linked/secret.wav"] {
            assert!(!source.exists(escaping), "{escaping}");
            let err = source.read(escaping, 100).unwrap_err();
            assert!(err.message.contains("outside the pack"), "{escaping}: {}", err.message);
        }
        // Links that stay inside the pack are fine.
        assert_eq!(source.read("alias.wav", 100).unwrap(), b"inside");
        assert_eq!(source.read("sounds/same-dir.wav", 100).unwrap(), b"inside");
        assert_eq!(source.read("sounds/up-and-back/a.wav", 100).unwrap(), b"inside");
    }

    #[cfg(unix)]
    #[test]
    fn dir_pack_json_symlinked_outside_is_rejected() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "pack.json", PACK_JSON);
        let tmp = tempfile::tempdir().unwrap();
        symlink(outside.path().join("pack.json"), tmp.path().join("pack.json")).unwrap();
        assert!(open_err(tmp.path()).message.contains("outside the pack"));
    }

    #[cfg(unix)]
    #[test]
    fn dir_opened_through_a_symlink_reports_the_given_location() {
        use std::os::unix::fs::symlink;
        let tmp = dir_pack(&[("real/pack.json", PACK_JSON), ("real/a.wav", b"x")]);
        let link = tmp.path().join("link");
        symlink(tmp.path().join("real"), &link).unwrap();
        let mut source = PackSource::open(&link).unwrap();
        assert_eq!(source.location(), link);
        assert_eq!(source.read("a.wav", 10).unwrap(), b"x");
    }

    // ---- zips ----------------------------------------------------------------------------

    #[test]
    fn zip_with_pack_json_at_root() {
        let (_tmp, path) = zip_pack(
            &[("pack.json", PACK_JSON), ("sounds/", b""), ("sounds/a.wav", b"audio")],
            CompressionMethod::Deflated,
        );
        let mut source = PackSource::open(&path).unwrap();
        assert_eq!(source.location(), path);
        assert_eq!(root_prefix(&source), "");
        assert_eq!(source.read("pack.json", 100).unwrap(), PACK_JSON);
        assert_eq!(source.read("sounds/a.wav", 100).unwrap(), b"audio");
        assert!(source.exists("sounds/a.wav"));
        assert!(!source.exists("sounds/b.wav"));
        assert_eq!(
            source.read("sounds/b.wav", 100).unwrap_err().message,
            "file not found in the pack"
        );
    }

    #[test]
    fn zip_with_single_top_level_folder() {
        let (_tmp, path) = zip_stored(&[
            ("deep-thock/", b""),
            ("deep-thock/pack.json", PACK_JSON),
            ("deep-thock/sounds/a.wav", b"audio"),
            ("README.txt", b"stray files outside the pack folder are ignored"),
        ]);
        let mut source = PackSource::open(&path).unwrap();
        assert_eq!(root_prefix(&source), "deep-thock/");
        assert_eq!(source.read("pack.json", 100).unwrap(), PACK_JSON);
        assert_eq!(source.read("sounds/a.wav", 100).unwrap(), b"audio");
        assert!(!source.exists("deep-thock/sounds/a.wav"));
        assert!(!source.exists("README.txt"));
    }

    #[test]
    fn zip_ignores_macos_metadata() {
        let (_tmp, path) = zip_stored(&[
            ("my-pack/", b""),
            ("my-pack/pack.json", PACK_JSON),
            ("my-pack/.DS_Store", b"junk"),
            (".DS_Store", b"junk"),
            ("__MACOSX/", b""),
            ("__MACOSX/my-pack/", b""),
            ("__MACOSX/my-pack/._pack.json", b"resource fork"),
            ("__MACOSX/other/pack.json", PACK_JSON),
        ]);
        let source = PackSource::open(&path).unwrap();
        assert_eq!(root_prefix(&source), "my-pack/");
    }

    #[test]
    fn zip_layout_problems_explain_the_expected_layout() {
        let cases: [(Entries, &str); 5] = [
            (
                &[("sounds/a.wav", b"x")],
                "no pack.json found; the zip must contain pack.json at its root",
            ),
            (&[("a/b/pack.json", PACK_JSON)], "pack.json is at \"a/b/pack.json\", too deep"),
            (
                &[("Pack.json", PACK_JSON)],
                "found \"Pack.json\" but the name must be exactly pack.json",
            ),
            (
                &[("a/pack.json", PACK_JSON), ("b/pack.json", PACK_JSON)],
                "more than one pack (\"a/\", \"b/\")",
            ),
            (&[("pack.json/", b""), ("x/", b"")], "no pack.json found"),
        ];
        for (entries, expected) in cases {
            let (_tmp, path) = zip_stored(entries);
            let err = open_err(&path);
            assert_eq!(err.location, "zip");
            assert!(err.message.contains(expected), "{expected:?} not in {:?}", err.message);
        }
    }

    #[test]
    fn zip_with_unsafe_entry_names_is_rejected() {
        // Anywhere in the zip, inside the pack folder or not: these escape when extracted.
        for bad in [
            "../evil.wav",
            "pack/../../evil.wav",
            "/etc/evil.wav",
            "C:/evil.wav",
            "C:evil.wav",
            "z:x",
            "docs/../x.txt",
            "..\\evil.txt",
            "docs\\..\\x.txt",
            "\\abs.txt",
        ] {
            for prefix in ["", "elsewhere/"] {
                let name = if bad.starts_with(['/', '\\']) || bad.contains(':') {
                    bad.to_owned()
                } else {
                    format!("{prefix}{bad}")
                };
                let (_tmp, path) = zip_stored(&[("p/pack.json", PACK_JSON), (name.as_str(), b"x")]);
                let err = open_err(&path);
                assert_eq!(err.location, "zip", "{name}");
                assert!(err.message.starts_with("unsafe entry path"), "{name}: {}", err.message);
            }
        }
        let (_tmp, path) =
            zip_stored(&[("pack.json", PACK_JSON), ("../a", b"x"), ("../b", b"x"), ("/c", b"x")]);
        let message = open_err(&path).message;
        assert!(
            message.starts_with("unsafe entry path \"../a\" (contains \"..\") and 2 more"),
            "{message}"
        );
    }

    #[test]
    fn zip_pack_folder_names_must_use_slashes() {
        for bad in ["sounds\\evil.wav", "a\0b.wav"] {
            let (_tmp, path) = zip_stored(&[("pack.json", PACK_JSON), (bad, b"x")]);
            let err = open_err(&path);
            assert!(err.message.starts_with("invalid entry name"), "{bad:?}: {}", err.message);
            assert!(err.message.contains("must use / as the separator"), "{}", err.message);
        }
        // A zip written with \ separators throughout gets told so.
        let (_tmp, path) = zip_stored(&[("mypack\\pack.json", PACK_JSON), ("mypack\\a.wav", b"x")]);
        let err = open_err(&path);
        assert!(
            err.message
                .starts_with("found \"mypack\\\\pack.json\", but zip entry names must use /"),
            "{}",
            err.message
        );
    }

    #[test]
    fn zip_names_outside_the_pack_folder_only_need_to_be_safe() {
        // Never read, so only the escape rules apply to them. A colon is a drive prefix only
        // after a single letter (macOS Finder stores a "/" typed in a file name as ":").
        let (_tmp, path) = zip_stored(&[
            ("mypack/pack.json", PACK_JSON),
            ("mypack/a.wav", b"x"),
            ("README.txt", b"hi"),
            ("docs\\readme.txt", b"hi"),
            ("notes:2024.txt", b"hi"),
            ("docs/notes:2024.txt", b"hi"),
        ]);
        let mut source = PackSource::open(&path).unwrap();
        assert_eq!(root_prefix(&source), "mypack/");
        assert_eq!(source.read("a.wav", 10).unwrap(), b"x");
    }

    #[test]
    fn zip_with_too_many_entries_is_rejected() {
        let names: Vec<String> = (0..MAX_ZIP_ENTRIES).map(|i| format!("f/{i}.wav")).collect();
        let mut entries: Vec<(&str, &[u8])> =
            names.iter().map(|n| (n.as_str(), &b""[..])).collect();
        entries.push(("pack.json", PACK_JSON));
        let (_tmp, path) = zip_stored(&entries);
        let err = open_err(&path);
        assert_eq!(
            err.message,
            format!("zip has {} entries; the limit is {MAX_ZIP_ENTRIES}", MAX_ZIP_ENTRIES + 1)
        );

        entries.pop();
        entries[0] = ("pack.json", PACK_JSON);
        let (_tmp, path) = zip_stored(&entries);
        assert!(PackSource::open(&path).is_ok(), "exactly {MAX_ZIP_ENTRIES} entries is allowed");
    }

    #[test]
    fn zip_bomb_ratio_is_rejected_above_the_size_threshold() {
        let zeros = vec![0u8; 2 * 1024 * 1024];
        let (_tmp, path) = zip_pack(
            &[("pack.json", PACK_JSON), ("bomb.wav", &zeros)],
            CompressionMethod::Deflated,
        );
        let err = open_err(&path);
        assert_eq!(err.location, "zip");
        assert!(err.message.contains("possible zip bomb"), "{}", err.message);

        // Small, highly compressible entries (silence, padded JSON) are fine.
        let zeros = vec![0u8; 512 * 1024];
        let (_tmp, path) = zip_pack(
            &[("pack.json", PACK_JSON), ("quiet.wav", &zeros)],
            CompressionMethod::Deflated,
        );
        let mut source = PackSource::open(&path).unwrap();
        assert_eq!(source.read("quiet.wav", MAX_AUDIO_FILE_BYTES).unwrap(), zeros);
    }

    /// Rewrites the declared sizes of `name` in the central directory, as a malicious zip would.
    fn patch_declared_sizes(path: &Path, name: &str, compressed: u32, uncompressed: u32) {
        let mut bytes = fs::read(path).unwrap();
        let sig = [0x50, 0x4b, 0x01, 0x02];
        let at = (0..bytes.len() - 46)
            .find(|&i| bytes[i..i + 4] == sig && bytes[i + 46..].starts_with(name.as_bytes()))
            .unwrap();
        bytes[at + 20..at + 24].copy_from_slice(&compressed.to_le_bytes());
        bytes[at + 24..at + 28].copy_from_slice(&uncompressed.to_le_bytes());
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn zip_total_uncompressed_size_is_limited() {
        let (_tmp, path) =
            zip_stored(&[("pack.json", PACK_JSON), ("a.wav", b"x"), ("b.wav", b"y")]);
        let declared = 101 * 1024 * 1024;
        patch_declared_sizes(&path, "a.wav", declared, declared);
        patch_declared_sizes(&path, "b.wav", declared, declared);
        let err = open_err(&path);
        assert_eq!(err.message, "zip expands to 202.0 MB; the limit is 200.0 MB");
    }

    #[test]
    fn zip_read_enforces_declared_and_actual_size() {
        let (_tmp, path) = zip_pack(
            &[("pack.json", PACK_JSON), ("a.wav", &[1; 3000])],
            CompressionMethod::Deflated,
        );
        let mut source = PackSource::open(&path).unwrap();
        assert_eq!(source.read("a.wav", 3000).unwrap().len(), 3000);
        let err = source.read("a.wav", 2999).unwrap_err();
        assert_eq!(
            (err.location.as_str(), err.message.as_str()),
            ("a.wav", "file is 3000 bytes; the limit is 2999 bytes")
        );
    }

    #[test]
    fn zip_entry_lying_about_its_size_is_capped() {
        let (_tmp, path) = zip_stored(&[("pack.json", PACK_JSON), ("a.wav", &[1; 3000])]);
        // Claims 10 bytes uncompressed; actually stores 3000.
        patch_declared_sizes(&path, "a.wav", 3000, 10);
        let mut source = PackSource::open(&path).unwrap();
        let err = source.read("a.wav", 100).unwrap_err();
        assert_eq!(err.location, "a.wav");
        assert!(err.message.contains("more data than its declared size of 10 bytes"), "{}", err);
    }

    /// The declared sizes are all `open_zip` can check, so a read must never go past them, even
    /// when the caller's limit is far higher: else a tiny zip could inflate to gigabytes.
    #[test]
    fn deflated_entry_is_never_read_past_its_declared_size() {
        let (_tmp, path) = zip_pack(
            &[("pack.json", PACK_JSON), ("a.wav", &[1; 300_000])],
            CompressionMethod::Deflated,
        );
        let compressed = {
            let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
            archive.by_name("a.wav").unwrap().compressed_size()
        };
        assert!(compressed < 3000, "{compressed}");
        patch_declared_sizes(&path, "a.wav", compressed as u32, 1000);
        let mut source = PackSource::open(&path).unwrap();
        let err = source.read("a.wav", MAX_AUDIO_FILE_BYTES).unwrap_err();
        assert!(err.message.contains("more data than its declared size of 1000 bytes"), "{err}");
    }

    /// Rewrites the local-header offset of `name` in the central directory.
    fn patch_local_header_offset(path: &Path, name: &str, offset: u32) {
        let mut bytes = fs::read(path).unwrap();
        let sig = [0x50, 0x4b, 0x01, 0x02];
        let at = (0..bytes.len() - 46)
            .find(|&i| bytes[i..i + 4] == sig && bytes[i + 46..].starts_with(name.as_bytes()))
            .unwrap();
        bytes[at + 42..at + 46].copy_from_slice(&offset.to_le_bytes());
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn zip_entries_sharing_compressed_data_are_rejected() {
        // Entries pointing at one compressed stream would each inflate it again.
        let (_tmp, path) = zip_pack(
            &[("pack.json", PACK_JSON), ("a.wav", &[1; 3000]), ("b.wav", &[1; 3000])],
            CompressionMethod::Deflated,
        );
        let a_offset = {
            let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
            archive.by_name("a.wav").unwrap().header_start() as u32
        };
        assert!(PackSource::open(&path).is_ok());
        patch_local_header_offset(&path, "b.wav", a_offset);
        let err = open_err(&path);
        assert_eq!(err.location, "zip");
        assert!(err.message.contains("share compressed data"), "{}", err.message);
    }

    #[test]
    fn zip_matching_is_case_sensitive() {
        let (_tmp, path) = zip_stored(&[("p/pack.json", PACK_JSON), ("p/Sounds/Click.wav", b"x")]);
        let mut source = PackSource::open(&path).unwrap();
        assert_eq!(source.read("Sounds/Click.wav", 10).unwrap(), b"x");
        assert!(!source.exists("sounds/click.wav"));
        let err = source.read("sounds/click.wav", 10).unwrap_err();
        assert_eq!(
            err.message,
            "file not found; names are case-sensitive and the pack has \"Sounds/Click.wav\""
        );
    }

    #[test]
    fn zip_rejects_bad_syntax_folders_and_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("pack.zip");
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = SimpleFileOptions::default();
        w.start_file("pack.json", options).unwrap();
        w.write_all(PACK_JSON).unwrap();
        w.add_directory("dir.wav/", options).unwrap();
        w.add_symlink("link.wav", "/etc/passwd", options).unwrap();
        w.finish().unwrap();

        let mut source = PackSource::open(&path).unwrap();
        assert!(!source.exists("link.wav"));
        assert_eq!(
            source.read("link.wav", 100).unwrap_err().message,
            "is a symbolic link, not a file"
        );
        assert!(!source.exists("dir.wav"));
        for bad in ["../pack.json", "/pack.json", "dir.wav/", "a\\b.wav", "./pack.json"] {
            assert!(!source.exists(bad), "{bad}");
            assert!(
                source.read(bad, 100).unwrap_err().message.starts_with("invalid path: "),
                "{bad}"
            );
        }
    }

    #[test]
    fn zip_encrypted_entry_gives_a_clear_error() {
        use zip::unstable::write::FileOptionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("pack.zip");
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        w.start_file("pack.json", SimpleFileOptions::default()).unwrap();
        w.write_all(PACK_JSON).unwrap();
        let secret = SimpleFileOptions::default().with_deprecated_encryption(b"hunter2").unwrap();
        w.start_file("a.wav", secret).unwrap();
        w.write_all(b"audio").unwrap();
        w.finish().unwrap();

        let mut source = PackSource::open(&path).unwrap();
        let err = source.read("a.wav", 100).unwrap_err();
        assert_eq!(err.message, "is encrypted; password-protected zips are not supported");
    }

    #[test]
    fn invalid_zip_files_are_reported() {
        let tmp = dir_pack(&[("garbage.zip", b"this is not a zip archive at all")]);
        let err = open_err(&tmp.path().join("garbage.zip"));
        assert_eq!(err.location, "zip");
        assert!(err.message.starts_with("not a valid zip file: "), "{}", err.message);

        // Extension matching ignores case.
        let (_tmp, path) = zip_stored(&[("pack.json", PACK_JSON)]);
        let upper = path.with_file_name("PACK.ZIP");
        fs::rename(&path, &upper).unwrap();
        assert!(PackSource::open(&upper).is_ok());
    }

    #[test]
    fn non_utf8_flagged_names_still_match() {
        // Written without the UTF-8 flag but with UTF-8 bytes, as many zip tools do.
        let (_tmp, path) = zip_stored(&[("pack.json", PACK_JSON), ("sounds/kläck.wav", b"x")]);
        let mut bytes = fs::read(&path).unwrap();
        // Clear general-purpose bit 11 (language encoding) in local and central headers.
        for i in 0..bytes.len() - 4 {
            let sig = &bytes[i..i + 4];
            if sig == [0x50, 0x4b, 0x03, 0x04] {
                bytes[i + 7] &= !0x08;
            } else if sig == [0x50, 0x4b, 0x01, 0x02] {
                bytes[i + 9] &= !0x08;
            }
        }
        fs::write(&path, bytes).unwrap();
        let mut source = PackSource::open(&path).unwrap();
        assert_eq!(source.read("sounds/kläck.wav", 10).unwrap(), b"x");
    }
}
