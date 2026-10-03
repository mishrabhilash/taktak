//! `pack.json`: types, parsing, structural validation and the key → sound resolution chain.
//! Pure logic: no filesystem access and no audio decoding.

use super::{Problem, printable};
use crate::input::KeyAction;
use crate::key::{Key, KeyGroup};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const FORMAT_VERSION: u32 = 1;
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

pub const GROUP_NAMES: [&str; 6] =
    ["alphanumeric", "space", "enter", "backspace", "modifiers", "other"];

/// Redistributable, MIT-compatible licenses (SPDX ids).
pub const ALLOWED_LICENSES: [&str; 10] = [
    "CC0-1.0",
    "CC-BY-3.0",
    "CC-BY-4.0",
    "MIT",
    "0BSD",
    "Unlicense",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
];

/// Loads locally with a warning; rejected by strict validation.
pub const PERSONAL_LICENSE: &str = "LicenseRef-Personal";

pub const DEFAULT_PITCH_VARIATION: f32 = 0.03;
pub const DEFAULT_VOLUME_VARIATION: f32 = 0.10;

const MAX_ID_CHARS: usize = 64;
const MAX_NAME_CHARS: usize = 64;
const MAX_VERSION_CHARS: usize = 32;
const MAX_AUTHOR_CHARS: usize = 128;
const MAX_TEXT_CHARS: usize = 500;
const MAX_VOLUME: f32 = 2.0;
const MAX_PITCH_VARIATION: f32 = 0.10;
const MAX_VOLUME_VARIATION: f32 = 0.50;
const AUDIO_EXTENSIONS: [&str; 3] = [".wav", ".ogg", ".mp3"];

/// Top-level fields this version understands; anything else is an "unknown field" warning.
const KNOWN_FIELDS: [&str; 15] = [
    "format",
    "id",
    "name",
    "version",
    "author",
    "license",
    "description",
    "source",
    "attribution",
    "preview",
    "volume",
    "trim_silence",
    "variation",
    "groups",
    "keys",
];
const REQUIRED_FIELDS: [&str; 5] = ["format", "id", "name", "author", "license"];
const SOUND_SET_FIELDS: [&str; 2] = ["press", "release"];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SoundSet {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub press: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub release: Vec<String>,
    /// Unknown fields, kept so they can be reported as warnings.
    #[serde(flatten, skip_serializing)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl SoundSet {
    pub fn files(&self, action: KeyAction) -> &[String] {
        match action {
            KeyAction::Down => &self.press,
            KeyAction::Up => &self.release,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VariationSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub author: String,
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim_silence: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation: Option<VariationSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub groups: BTreeMap<String, SoundSet>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: BTreeMap<String, SoundSet>,
    /// Unknown fields, kept so they can be reported as warnings.
    #[serde(flatten, skip_serializing)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicenseStatus {
    Allowed,
    Personal,
    Rejected,
}

pub fn license_status(spdx: &str) -> LicenseStatus {
    if ALLOWED_LICENSES.contains(&spdx) {
        LicenseStatus::Allowed
    } else if spdx == PERSONAL_LICENSE {
        LicenseStatus::Personal
    } else {
        LicenseStatus::Rejected
    }
}

/// Parses `pack.json` bytes. JSON syntax/type errors become `Problem`s located at
/// `"pack.json:<line>:<col>"`. Does not validate semantics; see [`validate`].
///
/// A leading UTF-8 BOM is ignored. Columns count characters, starting at 1. Every type
/// error, missing required field and duplicate key is reported, in document order, except
/// when `format` is newer than [`FORMAT_VERSION`]: then only "needs a newer TakTak" is.
/// Duplicate keys count wherever TakTak reads the value; inside unknown fields (and unknown
/// `variation` entries), which are ignored whole, they are ignored too.
pub fn parse(bytes: &[u8]) -> Result<Manifest, Vec<Problem>> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(vec![Problem::error(
            "pack.json",
            format!(
                "pack.json is too large ({} bytes; the limit is {} MB)",
                bytes.len(),
                MAX_MANIFEST_BYTES / (1024 * 1024)
            ),
        )]);
    }
    let src = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if src.iter().all(|&b| is_json_whitespace(b)) {
        return Err(vec![Problem::error("pack.json:1:1", "pack.json is empty")]);
    }
    // Syntax first, on an untyped tree, so a broken file never produces type noise.
    let value: Value = serde_json::from_slice(src).map_err(|e| vec![serde_problem(src, &e)])?;
    let index = Index::build(src);
    let Some(root) = value.as_object() else {
        return Err(vec![Problem::error(
            index.location(""),
            format!("pack.json must contain a JSON object ({{ … }}), found {}", describe(&value)),
        )]);
    };
    let problems = shape_problems(root, &index);
    if !problems.is_empty() {
        let format = root.get("format").and_then(Value::as_u64);
        if let Some(newer) = format.filter(|&f| f > u64::from(FORMAT_VERSION)) {
            // A newer format may have a different shape; say that instead of listing noise.
            return Err(vec![Problem::error(
                index.location("format"),
                newer_format_message(newer),
            )]);
        }
        return Err(problems);
    }
    // The shape check is at least as strict as serde, so this only fails if they diverge.
    serde_json::from_slice(src).map_err(|e| vec![serde_problem(src, &e)])
}

/// Structural and semantic validation per `docs/pack-format.md`: field rules, license,
/// group and key names (with "did you mean" suggestions), path syntax and extensions,
/// value ranges, the fallback-press rule, unknown-field warnings, and the
/// "no release sounds" warning. Does NOT check that files exist (see `load`).
/// With `strict`, `LicenseRef-Personal` is an error instead of a warning.
///
/// A `format` newer than [`FORMAT_VERSION`] yields only the "needs a newer TakTak" error.
/// Otherwise errors come before warnings; within each, problems follow the spec's field
/// order, with groups and keys in name order.
pub fn validate(m: &Manifest, strict: bool) -> Vec<Problem> {
    let mut out = Vec::new();
    match m.format {
        FORMAT_VERSION => {}
        0 => out.push(Problem::error(
            "format",
            format!("invalid pack format 0; the current format is {FORMAT_VERSION}"),
        )),
        // Every other rule here is a format-1 rule and may not apply to a newer pack.
        newer => return vec![Problem::error("format", newer_format_message(newer.into()))],
    }
    check_id(&mut out, &m.id);
    check_text(&mut out, "name", &m.name, true, MAX_NAME_CHARS);
    if let Some(version) = &m.version {
        check_text(&mut out, "version", version, false, MAX_VERSION_CHARS);
    }
    check_text(&mut out, "author", &m.author, true, MAX_AUTHOR_CHARS);
    check_license(&mut out, &m.license, m.attribution.as_deref(), strict);
    for (field, text) in
        [("description", &m.description), ("source", &m.source), ("attribution", &m.attribution)]
    {
        if let Some(text) = text {
            check_text(&mut out, field, text, false, MAX_TEXT_CHARS);
        }
    }
    if let Some(preview) = &m.preview {
        check_file(&mut out, "preview".to_owned(), preview);
    }
    if let Some(volume) = m.volume {
        check_range(&mut out, "volume", volume, MAX_VOLUME);
    }
    if let Some(variation) = &m.variation {
        if let Some(pitch) = variation.pitch {
            check_range(&mut out, "variation.pitch", pitch, MAX_PITCH_VARIATION);
        }
        if let Some(volume) = variation.volume {
            check_range(&mut out, "variation.volume", volume, MAX_VOLUME_VARIATION);
        }
    }
    for (name, set) in &m.groups {
        let location = format!("groups.{}", printable(name));
        if let Some(message) = group_name_problem(name) {
            out.push(Problem::error(&location, message));
        }
        check_sound_set(&mut out, &location, set);
    }
    for (name, set) in &m.keys {
        let location = format!("keys.{}", printable(name));
        if let Some(message) = key_name_problem(name) {
            out.push(Problem::error(&location, message));
        }
        check_sound_set(&mut out, &location, set);
    }
    let has_press = |group: &str| m.groups.get(group).is_some_and(|s| !s.press.is_empty());
    if !has_press("alphanumeric") && !has_press("other") {
        out.push(Problem::error(
            "groups",
            "no fallback press sound: add at least one file to groups.alphanumeric.press or \
             groups.other.press, so that every key makes a sound",
        ));
    }
    if m.groups.values().chain(m.keys.values()).all(|s| s.release.is_empty()) {
        out.push(Problem::warning("release", "pack has no release sounds; key-up will be silent"));
    }
    for name in m.extra.keys() {
        out.push(Problem::warning(
            printable(name),
            unknown_field_message(name, "", suggest(name, &KNOWN_FIELDS, FIELD_ALIASES)),
        ));
    }
    out.sort_by_key(|p| Reverse(p.severity));
    out
}

/// Checks one file path's syntax and extension per the spec's Paths section.
pub fn check_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("path is empty".to_owned());
    }
    if path.starts_with('/') {
        return Err("must be relative to the pack root (no leading /)".to_owned());
    }
    if path.contains('\\') {
        return Err("must use / as the separator, not \\".to_owned());
    }
    // Anywhere, not just as a drive prefix: "a.wav:x" names an NTFS alternate data stream.
    if path.contains(':') {
        return Err("must not contain ':' (drive prefix such as C:, or a file stream)".to_owned());
    }
    if path.chars().any(char::is_control) {
        return Err("must not contain control characters".to_owned());
    }
    // The rest keeps every name valid on Windows, so a pack folder can be copied, unzipped or
    // checked out there.
    if path.contains(WINDOWS_FORBIDDEN) {
        return Err(
            "must not contain < > \" | ? or * (not allowed in Windows file names)".to_owned()
        );
    }
    for segment in path.split('/') {
        if segment.is_empty() {
            return Err("must not contain empty segments (//)".to_owned());
        }
        if segment == "." || segment == ".." {
            return Err("must not contain \".\" or \"..\" segments".to_owned());
        }
        // The registry ignores hidden entries, so it would miss changes to such a file.
        if segment.starts_with('.') {
            return Err(
                "must not contain names starting with \".\" (hidden files are ignored)".to_owned()
            );
        }
        if segment.ends_with(['.', ' ']) {
            return Err(
                "names must not end in \".\" or a space (Windows drops them, so the name would \
                 change)"
                    .to_owned(),
            );
        }
        if let Some(device) = windows_device_name(segment) {
            return Err(format!(
                "{} is a reserved device name on Windows, even with an extension",
                quote(device)
            ));
        }
    }
    let lower = path.to_ascii_lowercase();
    if !AUDIO_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        return Err("must end in .wav, .ogg or .mp3".to_owned());
    }
    Ok(())
}

/// Characters Windows does not allow in file names (besides `\`, `:` and control characters,
/// which `check_path` reports on their own).
const WINDOWS_FORBIDDEN: [char; 6] = ['<', '>', '"', '|', '?', '*'];

/// The reserved device name a path segment stands for on Windows, if any: the part before the
/// first `.` (trailing spaces ignored) is CON, PRN, AUX, NUL, COM1–9 or LPT1–9 (also with a
/// superscript digit), in any letter case. `"con.wav"` and `"Aux.tar.ogg"` are; `"console.wav"`
/// and `"com10.wav"` are not.
fn windows_device_name(segment: &str) -> Option<&str> {
    let stem = segment.split('.').next().unwrap_or_default().trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    let reserved = match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => ["COM", "LPT"].iter().any(|prefix| {
            upper.strip_prefix(prefix).is_some_and(|n| {
                matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
            })
        }),
    };
    reserved.then_some(stem)
}

/// Every distinct file path the manifest references (sound sets and preview).
pub fn referenced_files(m: &Manifest) -> BTreeSet<String> {
    m.groups
        .values()
        .chain(m.keys.values())
        .flat_map(|set| set.press.iter().chain(&set.release))
        .chain(&m.preview)
        .cloned()
        .collect()
}

/// The resolution chain: keys[key] → groups[key.group()] → groups.other →
/// groups.alphanumeric → empty. Press and release are resolved independently.
pub fn resolve(m: &Manifest, key: Key, action: KeyAction) -> &[String] {
    let group = |name: &str| m.groups.get(name).map(|set| set.files(action));
    [
        m.keys.get(key.code_name()).map(|set| set.files(action)),
        group(group_name(key.group())),
        group("other"),
        group("alphanumeric"),
    ]
    .into_iter()
    .flatten()
    .find(|files| !files.is_empty())
    .unwrap_or(&[])
}

/// The preview file: `preview`, else the first alphanumeric press, else the first other press.
pub fn preview_file(m: &Manifest) -> Option<&str> {
    let first_press =
        |group: &str| m.groups.get(group).and_then(|set| set.press.first()).map(String::as_str);
    m.preview.as_deref().or_else(|| first_press("alphanumeric")).or_else(|| first_press("other"))
}

/// Group name as used in pack.json.
pub fn group_name(g: KeyGroup) -> &'static str {
    g.name()
}

// ---------------------------------------------------------------------------------------------
// Validation helpers

fn newer_format_message(format: u64) -> String {
    format!(
        "this pack needs a newer TakTak (it uses pack format {format}; this version reads \
         format {FORMAT_VERSION})"
    )
}

fn check_id(out: &mut Vec<Problem>, id: &str) {
    if id.is_empty() {
        out.push(Problem::error("id", "id must not be empty"));
        return;
    }
    let chars = id.chars().count();
    if chars > MAX_ID_CHARS {
        out.push(Problem::error(
            "id",
            format!("id is too long ({chars} characters; the limit is {MAX_ID_CHARS})"),
        ));
    }
    let valid = id.split('-').all(|part| {
        !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    if !valid {
        let mut message = format!(
            "invalid id {}: use lowercase letters, digits and single hyphens, e.g. \"deep-thock\"",
            quote(id)
        );
        let slug = slugify(id);
        if !slug.is_empty() && slug.len() <= MAX_ID_CHARS {
            message.push_str(&format!(" (did you mean {slug:?}?)"));
        }
        out.push(Problem::error("id", message));
    }
}

/// `"Deep Thock_2"` → `"deep-thock-2"`.
fn slugify(s: &str) -> String {
    let mut slug = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_owned()
}

/// Length limits apply to the trimmed text, counted in characters. The trimmed text (what is
/// displayed) must not contain control characters: line breaks have no place in a menu, and
/// escape sequences would reach the terminal of anyone running `taktak-pack info`.
fn check_text(out: &mut Vec<Problem>, field: &str, text: &str, required: bool, max: usize) {
    let trimmed = text.trim();
    let chars = trimmed.chars().count();
    if required && chars == 0 {
        out.push(Problem::error(field, format!("{field} must not be empty")));
    } else if chars > max {
        out.push(Problem::error(
            field,
            format!("{field} is too long ({chars} characters; the limit is {max})"),
        ));
    }
    if trimmed.chars().any(char::is_control) {
        out.push(Problem::error(
            field,
            format!("{field} must not contain control characters (line breaks, tabs, escapes)"),
        ));
    }
}

fn check_license(out: &mut Vec<Problem>, license: &str, attribution: Option<&str>, strict: bool) {
    let allowed_list = ALLOWED_LICENSES.join(", ");
    match license_status(license) {
        LicenseStatus::Allowed => {
            let credited = attribution.is_some_and(|a| !a.trim().is_empty());
            if license.starts_with("CC-BY-") && !credited {
                out.push(Problem::error(
                    "attribution",
                    format!(
                        "attribution is required for {license}: add the credit line to \
                         display, e.g. \"Recordings by Jane Doe\""
                    ),
                ));
            }
        }
        LicenseStatus::Personal if strict => out.push(Problem::error(
            "license",
            format!(
                "{PERSONAL_LICENSE} packs are for personal use only and cannot be bundled or \
                 shared; use one of {allowed_list}"
            ),
        )),
        LicenseStatus::Personal => out.push(Problem::warning(
            "license",
            format!("{PERSONAL_LICENSE}: this pack is for personal use only; do not share it"),
        )),
        LicenseStatus::Rejected if license.trim().is_empty() => out.push(Problem::error(
            "license",
            format!(
                "license must not be empty; use one of {allowed_list}, or {PERSONAL_LICENSE} \
                 for personal use"
            ),
        )),
        LicenseStatus::Rejected => {
            let mut message = format!("license {} is not allowed", quote(license));
            if let Some(why) = why_rejected(license) {
                message.push_str(": ");
                message.push_str(why);
            }
            // Spelling fixes only: a near-miss like "BSD-4-Clause" is a different license.
            if let Some(s) = same_name(license, &ALLOWED_LICENSES, LICENSE_ALIASES) {
                message.push_str(&format!(" (did you mean {s:?}?)"));
            }
            message.push_str(&format!(
                "; use one of {allowed_list}, or {PERSONAL_LICENSE} for personal use"
            ));
            out.push(Problem::error("license", message));
        }
    }
}

/// Why a Creative Commons license with extra conditions cannot be accepted.
fn why_rejected(license: &str) -> Option<&'static str> {
    let upper = license.to_ascii_uppercase();
    let rest = upper.strip_prefix("CC-BY-")?;
    let has = |term: &str| rest.split('-').any(|part| part == term);
    if has("NC") {
        Some("its non-commercial condition prevents free redistribution")
    } else if has("ND") {
        Some("its no-derivatives condition forbids the trimming and resampling TakTak does")
    } else if has("SA") {
        Some("its share-alike condition is not compatible with TakTak's MIT license")
    } else {
        None
    }
}

fn check_range(out: &mut Vec<Problem>, field: &str, value: f32, max: f32) {
    if !(0.0..=max).contains(&value) {
        out.push(Problem::error(
            field,
            format!("{field} must be between 0.0 and {max:?} (found {value:?})"),
        ));
    }
}

fn check_file(out: &mut Vec<Problem>, location: String, path: &str) {
    if let Err(why) = check_path(path) {
        out.push(Problem::error(location, format!("invalid path {}: {why}", quote(path))));
    }
}

fn check_sound_set(out: &mut Vec<Problem>, location: &str, set: &SoundSet) {
    for (field, files) in [("press", &set.press), ("release", &set.release)] {
        for (i, path) in files.iter().enumerate() {
            check_file(out, format!("{location}.{field}[{i}]"), path);
        }
    }
    for name in set.extra.keys() {
        out.push(Problem::warning(
            format!("{location}.{}", printable(name)),
            unknown_field_message(
                name,
                " in sound set",
                suggest(name, &SOUND_SET_FIELDS, SOUND_SET_ALIASES),
            ),
        ));
    }
    if set.press.is_empty() && set.release.is_empty() {
        out.push(Problem::warning(location, "empty sound set; it has no effect"));
    }
}

fn unknown_field_message(name: &str, context: &str, suggestion: Option<&str>) -> String {
    let mut message = format!("unknown field {}{context} is ignored", quote(name));
    if let Some(s) = suggestion {
        message.push_str(&format!(" (did you mean {s:?}?)"));
    }
    message
}

fn group_name_problem(name: &str) -> Option<String> {
    if GROUP_NAMES.contains(&name) {
        return None;
    }
    Some(if let Some(s) = suggest(name, &GROUP_NAMES, GROUP_ALIASES) {
        format!("unknown group {} (did you mean {s:?}?)", quote(name))
    } else if name.parse::<Key>().is_ok() {
        format!("{} is a key name, not a group; move it to \"keys\"", quote(name))
    } else {
        format!("unknown group {} (groups are {})", quote(name), GROUP_NAMES.join(", "))
    })
}

fn key_name_problem(name: &str) -> Option<String> {
    if name.parse::<Key>().is_ok() {
        return None;
    }
    Some(if let Some(s) = suggest_key(name) {
        format!("unknown key name {} (did you mean {s:?}?)", quote(name))
    } else if GROUP_NAMES.contains(&name.to_ascii_lowercase().as_str()) {
        format!("{} is a group name, not a key; move it to \"groups\"", quote(name))
    } else {
        format!(
            "unknown key name {} (expected a KeyboardEvent.code name such as \"KeyA\")",
            quote(name)
        )
    })
}

/// Debug-quotes user text for a message, shortening very long values.
fn quote(s: &str) -> String {
    const MAX: usize = 60;
    match s.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{:?}", format!("{}…", &s[..cut])),
        None => format!("{s:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// "Did you mean" suggestions

const GROUP_ALIASES: &[(&str, &str)] = &[
    ("modifier", "modifiers"),
    ("mod", "modifiers"),
    ("mods", "modifiers"),
    ("alpha", "alphanumeric"),
    ("alnum", "alphanumeric"),
    ("alphanum", "alphanumeric"),
    ("letter", "alphanumeric"),
    ("letters", "alphanumeric"),
    ("default", "other"),
    ("fallback", "other"),
    ("others", "other"),
    ("rest", "other"),
    ("spacebar", "space"),
    ("return", "enter"),
    ("delete", "backspace"),
    ("del", "backspace"),
    ("bksp", "backspace"),
];

/// Matched against the input reduced to lowercase ASCII letters and digits.
const KEY_ALIASES: &[(&str, &str)] = &[
    ("return", "Enter"),
    ("esc", "Escape"),
    ("shift", "ShiftLeft"),
    ("lshift", "ShiftLeft"),
    ("rshift", "ShiftRight"),
    ("ctrl", "ControlLeft"),
    ("control", "ControlLeft"),
    ("lctrl", "ControlLeft"),
    ("rctrl", "ControlRight"),
    ("alt", "AltLeft"),
    ("option", "AltLeft"),
    ("opt", "AltLeft"),
    ("ralt", "AltRight"),
    ("altgr", "AltRight"),
    ("cmd", "MetaLeft"),
    ("command", "MetaLeft"),
    ("meta", "MetaLeft"),
    ("win", "MetaLeft"),
    ("windows", "MetaLeft"),
    ("super", "MetaLeft"),
    ("bksp", "Backspace"),
    ("bs", "Backspace"),
    ("del", "Delete"),
    ("left", "ArrowLeft"),
    ("right", "ArrowRight"),
    ("up", "ArrowUp"),
    ("down", "ArrowDown"),
    ("caps", "CapsLock"),
    ("spacebar", "Space"),
    ("pgup", "PageUp"),
    ("pgdn", "PageDown"),
    ("pgdown", "PageDown"),
    ("ins", "Insert"),
    ("prtsc", "PrintScreen"),
    ("menu", "ContextMenu"),
    ("grave", "Backquote"),
    ("tilde", "Backquote"),
    ("hyphen", "Minus"),
    ("dash", "Minus"),
    ("equals", "Equal"),
    ("apostrophe", "Quote"),
    ("dot", "Period"),
];

/// Single characters, matched verbatim (US layout positions).
const PUNCTUATION_KEYS: &[(char, &str)] = &[
    ('`', "Backquote"),
    ('-', "Minus"),
    ('=', "Equal"),
    ('[', "BracketLeft"),
    (']', "BracketRight"),
    ('\\', "Backslash"),
    (';', "Semicolon"),
    ('\'', "Quote"),
    (',', "Comma"),
    ('.', "Period"),
    ('/', "Slash"),
    (' ', "Space"),
];

const FIELD_ALIASES: &[(&str, &str)] = &[
    ("desc", "description"),
    ("credit", "attribution"),
    ("credits", "attribution"),
    ("gain", "volume"),
];

const SOUND_SET_ALIASES: &[(&str, &str)] = &[
    ("down", "press"),
    ("keydown", "press"),
    ("pressed", "press"),
    ("up", "release"),
    ("keyup", "release"),
    ("released", "release"),
];

/// Only names that cannot mean anything else (CC0 has a single version).
const LICENSE_ALIASES: &[(&str, &str)] = &[("cc0", "CC0-1.0")];

fn suggest_key(name: &str) -> Option<&'static str> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if let Some(&(_, key)) = PUNCTUATION_KEYS.iter().find(|(p, _)| *p == c) {
            return Some(key);
        }
        let code = if c.is_ascii_alphabetic() {
            format!("Key{}", c.to_ascii_uppercase())
        } else {
            format!("Digit{c}")
        };
        if let Ok(key) = code.parse::<Key>() {
            return Some(key.code_name());
        }
    }
    let names: Vec<&'static str> = Key::ALL.iter().map(|k| k.code_name()).collect();
    suggest(name, &names, KEY_ALIASES)
}

/// [`same_name`], else the closest candidate within edit distance 2.
fn suggest(
    name: &str,
    candidates: &[&'static str],
    aliases: &[(&str, &'static str)],
) -> Option<&'static str> {
    same_name(name, candidates, aliases).or_else(|| closest(&normalize(name), candidates))
}

/// The candidate `name` spells differently (case, separators) or is an alias of.
fn same_name(
    name: &str,
    candidates: &[&'static str],
    aliases: &[(&str, &'static str)],
) -> Option<&'static str> {
    let norm = normalize(name);
    if norm.is_empty() {
        return None;
    }
    candidates
        .iter()
        .copied()
        .find(|c| normalize(c) == norm)
        .or_else(|| aliases.iter().find(|(alias, _)| *alias == norm).map(|&(_, c)| c))
}

/// Lowercase ASCII letters and digits only: `"Trim_Silence"` → `"trimsilence"`.
fn normalize(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

fn closest(norm: &str, candidates: &[&'static str]) -> Option<&'static str> {
    const MAX_DISTANCE: usize = 2;
    let input: Vec<char> = norm.chars().collect();
    let mut best: Option<(usize, &'static str)> = None;
    for &candidate in candidates {
        let cand: Vec<char> = normalize(candidate).chars().collect();
        if input.len().abs_diff(cand.len()) > MAX_DISTANCE {
            continue;
        }
        let d = edit_distance(&input, &cand);
        // `d < len` keeps very short inputs from matching almost anything.
        if d <= MAX_DISTANCE && d < input.len() && best.is_none_or(|(best_d, _)| d < best_d) {
            best = Some((d, candidate));
        }
    }
    best.map(|(_, c)| c)
}

/// Levenshtein distance.
fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = prev[j] + usize::from(ca != cb);
            cur[j + 1] = substitution.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ---------------------------------------------------------------------------------------------
// Parsing helpers

/// Type errors, missing required fields and duplicate keys, sorted by position. At least as
/// strict as the `Deserialize` impls of [`Manifest`], [`VariationSpec`] and [`SoundSet`], so
/// an empty result means `serde_json` will accept the document. Stricter on purpose in two
/// places: duplicate group and key names (serde keeps the last) and `variation` given as an
/// array (serde's struct-from-sequence form). Duplicates inside unknown fields are not
/// reported; serde keeps the last of those too, and nothing reads them.
fn shape_problems(root: &Map<String, Value>, index: &Index) -> Vec<Problem> {
    let mut shape = Shape { index, found: Vec::new() };
    for field in REQUIRED_FIELDS {
        if !root.contains_key(field) {
            shape.error("", format!("missing required field {field:?}"));
        }
    }
    for (field, value) in root {
        let path = field.as_str();
        match path {
            "format" => {
                let ok = value.as_u64().is_some_and(|n| u32::try_from(n).is_ok());
                shape.expect(path, value, ok, "a whole number (the format version, 1)");
            }
            "id" | "name" | "author" | "license" => {
                shape.expect(path, value, value.is_string(), "a string");
            }
            "version" | "description" | "source" | "attribution" | "preview" => {
                shape.expect(path, value, value.is_string() || value.is_null(), "a string");
            }
            "volume" => shape.expect(path, value, value.is_number() || value.is_null(), "a number"),
            "trim_silence" => {
                shape.expect(path, value, value.is_boolean() || value.is_null(), "true or false");
            }
            "variation" => match value {
                Value::Null => {}
                Value::Object(variation) => {
                    for (sub, v) in variation {
                        if sub == "pitch" || sub == "volume" {
                            let ok = v.is_number() || v.is_null();
                            shape.expect(&format!("variation.{sub}"), v, ok, "a number");
                        }
                    }
                }
                _ => shape.expect(
                    path,
                    value,
                    false,
                    "an object like { \"pitch\": 0.03, \"volume\": 0.1 }",
                ),
            },
            "groups" | "keys" => match value.as_object() {
                Some(sets) => {
                    for (name, set) in sets {
                        shape.sound_set(&format!("{path}.{name}"), set);
                    }
                }
                None => shape.expect(path, value, false, "an object of sound sets"),
            },
            _ => {}
        }
    }
    for (path, pos) in &index.duplicates {
        shape
            .found
            .push((*pos, Problem::error(pos.location(), format!("duplicate field {path:?}"))));
    }
    shape.found.sort_by_key(|(pos, _)| *pos);
    shape.found.into_iter().map(|(_, problem)| problem).collect()
}

struct Shape<'a> {
    index: &'a Index,
    found: Vec<(Pos, Problem)>,
}

impl Shape<'_> {
    fn error(&mut self, path: &str, message: String) {
        let pos = self.index.pos(path);
        self.found.push((pos, Problem::error(pos.location(), message)));
    }

    fn expect(&mut self, path: &str, value: &Value, ok: bool, expected: &str) {
        if !ok {
            self.error(
                path,
                format!("wrong type for {path:?}: expected {expected}, found {}", describe(value)),
            );
        }
    }

    fn sound_set(&mut self, path: &str, value: &Value) {
        let Some(set) = value.as_object() else {
            let expected = "a sound set like { \"press\": [...], \"release\": [...] }";
            return self.expect(path, value, false, expected);
        };
        for field in SOUND_SET_FIELDS {
            let Some(files) = set.get(field) else { continue };
            let files_path = format!("{path}.{field}");
            match files.as_array() {
                Some(files) => {
                    for (i, file) in files.iter().enumerate() {
                        let file_path = format!("{files_path}[{i}]");
                        self.expect(&file_path, file, file.is_string(), "a file path string");
                    }
                }
                None => self.expect(&files_path, files, false, "an array of file paths"),
            }
        }
    }
}

fn describe(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => format!("the number {n}"),
        Value::String(_) => "a string".to_owned(),
        Value::Array(_) => "an array".to_owned(),
        Value::Object(_) => "an object".to_owned(),
    }
}

fn is_json_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// Converts a `serde_json` error into a located problem with a friendlier message.
fn serde_problem(src: &[u8], e: &serde_json::Error) -> Problem {
    let full = e.to_string();
    let suffix = format!(" at line {} column {}", e.line(), e.column());
    let bare = full.strip_suffix(&suffix).unwrap_or(&full);
    let (offset, pos) = locate(src, e.line(), e.column());
    let message = if e.is_syntax() || e.is_eof() {
        let detail = match bare {
            "EOF while parsing an object" => "unexpected end of file: an object is missing its }",
            "EOF while parsing a list" => "unexpected end of file: an array is missing its ]",
            "EOF while parsing a string" => "unexpected end of file: a string is missing its \"",
            "EOF while parsing a value" => "unexpected end of file: a value is missing",
            "trailing comma" => "trailing comma (remove the comma before } or ])",
            "trailing characters" => "unexpected text after the end of the JSON document",
            other => other,
        };
        let hint =
            if src.get(offset) == Some(&b'/') { " (comments are not allowed in JSON)" } else { "" };
        format!("invalid JSON: {detail}{hint}")
    } else if let Some(field) =
        bare.strip_prefix("missing field `").and_then(|r| r.strip_suffix('`'))
    {
        format!("missing required field {field:?}")
    } else if let Some(field) =
        bare.strip_prefix("duplicate field `").and_then(|r| r.strip_suffix('`'))
    {
        format!("duplicate field {field:?}")
    } else if let Some((found, expected)) =
        bare.strip_prefix("invalid type: ").and_then(|r| r.split_once(", expected "))
    {
        format!("wrong type: expected {expected}, found {found}")
    } else {
        bare.to_owned()
    };
    Problem::error(pos.location(), message)
}

/// Maps serde_json's 1-based line and byte column to a byte offset and a character position.
fn locate(src: &[u8], line: usize, byte_col: usize) -> (usize, Pos) {
    let line_start = if line <= 1 {
        0
    } else {
        src.iter()
            .enumerate()
            .filter(|&(_, &b)| b == b'\n')
            .nth(line - 2)
            .map_or(src.len(), |(i, _)| i + 1)
    };
    let offset = (line_start + byte_col.saturating_sub(1)).min(src.len());
    let mut cursor = LineCol { at: line_start, line: line.max(1), chars: 0 };
    (offset, cursor.advance(src, offset))
}

/// A 1-based line and character column in `pack.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Pos {
    line: usize,
    col: usize,
}

impl Pos {
    fn location(self) -> String {
        format!("pack.json:{}:{}", self.line, self.col)
    }
}

/// Incremental byte offset → line/column conversion (offsets must not decrease).
struct LineCol {
    at: usize,
    line: usize,
    /// Characters between the start of the line and `at`.
    chars: usize,
}

impl LineCol {
    fn advance(&mut self, src: &[u8], to: usize) -> Pos {
        if let Some(bytes) = src.get(self.at..to) {
            for &b in bytes {
                if b == b'\n' {
                    self.line += 1;
                    self.chars = 0;
                } else if b & 0xC0 != 0x80 {
                    // Count UTF-8 lead bytes only, so columns are in characters.
                    self.chars += 1;
                }
            }
            self.at = to;
        }
        Pos { line: self.line, col: self.chars + 1 }
    }
}

/// Source positions of the values in a syntactically valid JSON document, keyed by the same
/// dotted paths used in problem locations (`groups.space.press[1]`), plus duplicate keys.
struct Index {
    positions: HashMap<String, Pos>,
    /// Duplicate keys in objects TakTak reads (see [`Scope`]).
    duplicates: Vec<(String, Pos)>,
}

/// What a JSON value is to TakTak, which decides whether duplicate keys in it matter. A
/// duplicate is an error wherever TakTak reads the value (JSON parsers disagree on which copy
/// wins, so a pack must not depend on one). Unknown fields are ignored whole, as the spec says,
/// so a duplicate inside one is ignored too: a newer pack's extension field must not stop it
/// loading here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    /// The document itself: known fields are read, unknown ones ignored (with a warning).
    Root,
    /// `variation`: `pitch` and `volume` are read, anything else ignored without a warning.
    Variation,
    /// `groups` or `keys`: every entry is a sound set.
    SoundSets,
    /// One sound set: `press` and `release` are read, unknown fields ignored (with a warning).
    SoundSet,
    /// A value TakTak reads in full.
    Read,
    /// A value TakTak never looks at.
    Ignored,
}

impl Scope {
    /// The scope of the value stored under `key` in an object of this scope.
    fn field(self, key: &str) -> Scope {
        let read_if = |known: bool| if known { Scope::Read } else { Scope::Ignored };
        match self {
            Scope::Root => match key {
                "variation" => Scope::Variation,
                "groups" | "keys" => Scope::SoundSets,
                _ => read_if(KNOWN_FIELDS.contains(&key)),
            },
            Scope::Variation => read_if(key == "pitch" || key == "volume"),
            Scope::SoundSets => Scope::SoundSet,
            Scope::SoundSet => read_if(SOUND_SET_FIELDS.contains(&key)),
            Scope::Read | Scope::Ignored => self,
        }
    }

    /// The scope of an array element. Only `Read` arrays are valid; an array in place of an
    /// object is a type error, and its contents count as read.
    fn element(self) -> Scope {
        if self == Scope::Ignored { Scope::Ignored } else { Scope::Read }
    }
}

impl Index {
    /// Deepest path that can carry a type error: `groups.<name>.press[<i>]`.
    const MAX_DEPTH: usize = 4;

    fn build(src: &[u8]) -> Index {
        let mut scanner = Scanner {
            src,
            i: 0,
            cursor: LineCol { at: 0, line: 1, chars: 0 },
            index: Index { positions: HashMap::new(), duplicates: Vec::new() },
        };
        scanner.value("", 0, Scope::Root);
        scanner.index
    }

    fn pos(&self, path: &str) -> Pos {
        self.positions
            .get(path)
            .or_else(|| self.positions.get(""))
            .copied()
            .unwrap_or(Pos { line: 1, col: 1 })
    }

    fn location(&self, path: &str) -> String {
        self.pos(path).location()
    }
}

/// A minimal JSON walker. Only ever run on input `serde_json` has already accepted; on
/// anything unexpected it stops early instead of failing (positions then fall back to 1:1).
struct Scanner<'a> {
    src: &'a [u8],
    i: usize,
    cursor: LineCol,
    index: Index,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.i).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(is_json_whitespace) {
            self.i += 1;
        }
    }

    fn pos(&mut self) -> Pos {
        self.cursor.advance(self.src, self.i)
    }

    fn value(&mut self, path: &str, depth: usize, scope: Scope) {
        self.skip_whitespace();
        let pos = self.pos();
        if depth <= Index::MAX_DEPTH {
            self.index.positions.insert(path.to_owned(), pos);
        }
        match self.peek() {
            Some(b'{') => self.object(path, depth, scope),
            Some(b'[') => self.array(path, depth, scope),
            Some(b'"') => {
                self.string();
            }
            Some(_) => {
                while self.peek().is_some_and(|b| !matches!(b, b',' | b'}' | b']'))
                    && !self.peek().is_some_and(is_json_whitespace)
                {
                    self.i += 1;
                }
            }
            None => {}
        }
    }

    fn object(&mut self, path: &str, depth: usize, scope: Scope) {
        self.i += 1;
        let mut seen = HashSet::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b'"') => {}
                Some(b'}') => {
                    self.i += 1;
                    return;
                }
                _ => return,
            }
            let key_pos = self.pos();
            let key = self.string();
            let child = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
            let child_scope = scope.field(&key);
            if !seen.insert(key) && child_scope != Scope::Ignored {
                self.index.duplicates.push((child.clone(), key_pos));
            }
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return;
            }
            self.i += 1;
            self.value(&child, depth + 1, child_scope);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return;
                }
                _ => return,
            }
        }
    }

    fn array(&mut self, path: &str, depth: usize, scope: Scope) {
        self.i += 1;
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.i += 1;
            return;
        }
        for n in 0.. {
            self.value(&format!("{path}[{n}]"), depth + 1, scope.element());
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return;
                }
                _ => return,
            }
        }
    }

    /// Consumes a string token starting at `"` and returns its decoded contents.
    fn string(&mut self) -> String {
        let start = self.i;
        self.i += 1;
        let mut escaped = false;
        while let Some(b) = self.peek() {
            match b {
                b'\\' => {
                    escaped = true;
                    self.i += 2;
                }
                b'"' => break,
                _ => self.i += 1,
            }
        }
        self.i = (self.i + 1).min(self.src.len());
        let token = self.src.get(start..self.i).unwrap_or_default();
        let raw = token.get(1..token.len().saturating_sub(1)).unwrap_or_default();
        if escaped {
            serde_json::from_slice(token)
                .unwrap_or_else(|_| String::from_utf8_lossy(raw).into_owned())
        } else {
            String::from_utf8_lossy(raw).into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::Severity;

    const MINIMAL: &str = r#"{
        "format": 1,
        "id": "tiny",
        "name": "Tiny",
        "author": "Someone",
        "license": "CC0-1.0",
        "groups": { "alphanumeric": { "press": ["a.wav"] } }
    }"#;

    /// The example from docs/pack-format.md.
    const FULL: &str = r#"{
      "format": 1,
      "id": "deep-thock",
      "name": "Deep Thock",
      "version": "1.0.0",
      "author": "TakTak contributors",
      "license": "CC0-1.0",
      "description": "Lubed linear switches on a heavy aluminium case.",
      "source": "Synthesized by tools/synth-packs",
      "attribution": null,
      "preview": "preview.wav",
      "volume": 1.0,
      "trim_silence": true,
      "variation": { "pitch": 0.03, "volume": 0.10 },
      "groups": {
        "alphanumeric": { "press": ["sounds/alnum-1.wav", "sounds/alnum-2.wav"], "release": ["sounds/alnum-up.wav"] },
        "space":        { "press": ["sounds/space.wav"], "release": ["sounds/space-up.wav"] },
        "other":        { "press": ["sounds/alnum-1.wav"] }
      },
      "keys": {
        "KeyA": { "press": ["sounds/KeyA-press.wav"], "release": ["sounds/KeyA-release.wav"] }
      }
    }"#;

    fn manifest(json: &str) -> Manifest {
        parse(json.as_bytes()).expect("manifest should parse")
    }

    /// `MINIMAL` with extra top-level fields spliced in, e.g. `r#""volume": 3"#`.
    fn minimal_with(fields: &str) -> Manifest {
        manifest(&MINIMAL.replacen('{', &format!("{{ {fields},"), 1))
    }

    fn parse_errors(json: &str) -> Vec<Problem> {
        let problems = parse(json.as_bytes()).expect_err("parse should fail");
        assert!(problems.iter().all(Problem::is_error));
        problems
    }

    fn render(problems: &[Problem]) -> String {
        problems.iter().map(|p| format!("  {p}\n")).collect()
    }

    fn set(press: &[&str], release: &[&str]) -> SoundSet {
        SoundSet {
            press: press.iter().map(|s| s.to_string()).collect(),
            release: release.iter().map(|s| s.to_string()).collect(),
            extra: BTreeMap::new(),
        }
    }

    // --- licenses -----------------------------------------------------------------------------

    #[test]
    fn license_status_is_exact_and_case_sensitive() {
        for id in ALLOWED_LICENSES {
            assert_eq!(license_status(id), LicenseStatus::Allowed, "{id}");
        }
        assert_eq!(license_status("LicenseRef-Personal"), LicenseStatus::Personal);
        for id in ["cc0-1.0", "mit", "CC-BY-SA-4.0", "CC-BY-NC-4.0", "GPL-3.0", "", " MIT"] {
            assert_eq!(license_status(id), LicenseStatus::Rejected, "{id:?}");
        }
        assert_eq!(license_status("licenseref-personal"), LicenseStatus::Rejected);
    }

    // --- paths --------------------------------------------------------------------------------

    #[test]
    fn valid_paths() {
        for p in [
            "a.wav",
            "sounds/KeyA-press.wav",
            "x/y/z.MP3",
            "deep/n.Ogg",
            "with space.wav",
            "é.wav",
            // Not reserved on Windows: the device names only match the whole stem.
            "console.wav",
            "com10.wav",
            "lpt0.wav",
            "sounds/nul-ish.wav",
            "a.b.wav",
        ] {
            assert_eq!(check_path(p), Ok(()), "{p}");
        }
    }

    #[test]
    fn invalid_paths_have_reasons() {
        let cases = [
            ("", "empty"),
            ("/abs.wav", "leading /"),
            ("sounds\\a.wav", "separator"),
            ("C:/a.wav", "drive prefix"),
            ("c:a.wav", "drive prefix"),
            ("sounds/D:/a.wav", "drive prefix"),
            // ':' anywhere: NTFS alternate data streams, and names Windows cannot store.
            ("a.wav:stream", "':'"),
            ("a.wav:stream.wav", "':'"),
            ("sounds/key:a.wav", "':'"),
            ("ab:/c.wav", "':'"),
            (":a.wav", "':'"),
            ("a//b.wav", "empty segments"),
            ("sounds/", "empty segments"),
            ("./a.wav", "\"..\""),
            ("a/../b.wav", "\"..\""),
            ("..", "\"..\""),
            // Hidden names: the registry skips them when fingerprinting a pack.
            ("sounds/.click.wav", "starting with \".\""),
            (".snd/k.wav", "starting with \".\""),
            ("...wav", "starting with \".\""),
            // Names Windows cannot store.
            ("a?.wav", "Windows"),
            ("b*.wav", "Windows"),
            ("c|d.wav", "Windows"),
            ("e<f>.wav", "Windows"),
            ("g\".wav", "Windows"),
            ("sounds./a.wav", "end in"),
            ("sounds /a.wav", "end in"),
            ("a.wav.", "end in"),
            ("con.wav", "\"con\" is a reserved device name"),
            ("sounds/aux.wav", "\"aux\" is a reserved device name"),
            ("NUL.mp3", "\"NUL\" is a reserved device name"),
            ("Com1.wav", "\"Com1\" is a reserved device name"),
            ("lpt9.tar.wav", "reserved device name"),
            ("com².wav", "reserved device name"),
            ("con /a.wav", "end in"),
            ("con/a.wav", "reserved device name"),
            ("prn .wav", "reserved device name"),
            ("a\u{1}.wav", "control characters"),
            ("a\n.wav", "control characters"),
            ("a\u{1b}[2J.wav", "control characters"),
            ("a\u{85}.wav", "control characters"),
            ("a.flac", ".wav, .ogg or .mp3"),
            ("a.wav.txt", ".wav, .ogg or .mp3"),
            ("sounds/a", ".wav, .ogg or .mp3"),
        ];
        for (path, reason) in cases {
            let err = check_path(path).expect_err(path);
            assert!(err.contains(reason), "{path:?}: {err:?} should mention {reason:?}");
        }
    }

    // --- parse: success -----------------------------------------------------------------------

    #[test]
    fn parses_minimal_manifest_with_defaults() {
        let m = manifest(MINIMAL);
        assert_eq!(m.format, 1);
        assert_eq!(m.id, "tiny");
        assert_eq!(m.version, None);
        assert_eq!(m.volume, None);
        assert_eq!(m.trim_silence, None);
        assert_eq!(m.variation, None);
        assert!(m.keys.is_empty() && m.extra.is_empty());
        assert_eq!(m.groups["alphanumeric"].press, ["a.wav"]);
        assert!(m.groups["alphanumeric"].release.is_empty());
    }

    #[test]
    fn parses_full_spec_example() {
        let m = manifest(FULL);
        assert_eq!(m.version.as_deref(), Some("1.0.0"));
        assert_eq!(m.attribution, None);
        assert_eq!(m.preview.as_deref(), Some("preview.wav"));
        assert_eq!(m.volume, Some(1.0));
        assert_eq!(m.trim_silence, Some(true));
        assert_eq!(m.variation, Some(VariationSpec { pitch: Some(0.03), volume: Some(0.10) }));
        assert_eq!(m.groups.len(), 3);
        assert_eq!(m.keys["KeyA"].release, ["sounds/KeyA-release.wav"]);
        assert!(validate(&m, true).is_empty(), "{}", render(&validate(&m, true)));
    }

    #[test]
    fn serialize_round_trips() {
        for json in [MINIMAL, FULL] {
            let m = manifest(json);
            let out = serde_json::to_vec_pretty(&m).expect("serialize");
            assert_eq!(parse(&out).expect("reparse"), m);
        }
    }

    #[test]
    fn strips_utf8_bom() {
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend_from_slice(MINIMAL.as_bytes());
        assert_eq!(parse(&bytes).expect("bom"), manifest(MINIMAL));
    }

    #[test]
    fn keeps_unknown_fields_for_warnings() {
        let m = minimal_with(r#""future": {"x": [1, 2]}, "groups_v2": null"#);
        assert_eq!(m.extra.keys().collect::<Vec<_>>(), ["future", "groups_v2"]);
        let m = manifest(
            r#"{"format":1,"id":"a","name":"A","author":"B","license":"MIT",
                "groups":{"other":{"press":["a.wav"],"gain":2}}}"#,
        );
        assert_eq!(m.groups["other"].extra["gain"], 2);
    }

    #[test]
    fn null_optionals_are_absent() {
        let m = minimal_with(
            r#""version": null, "description": null, "source": null, "preview": null,
               "volume": null, "trim_silence": null, "variation": {"pitch": null}"#,
        );
        assert_eq!(m.version, None);
        assert_eq!(m.volume, None);
        assert_eq!(m.variation, Some(VariationSpec::default()));
        let m = minimal_with(r#""variation": null"#);
        assert_eq!(m.variation, None);
    }

    #[test]
    fn integers_are_accepted_as_floats() {
        let m = minimal_with(r#""volume": 2, "variation": {"volume": 0}"#);
        assert_eq!(m.volume, Some(2.0));
        assert_eq!(m.variation.and_then(|v| v.volume), Some(0.0));
    }

    // --- parse: failures ----------------------------------------------------------------------

    #[test]
    fn rejects_oversized_manifest() {
        let mut bytes = MINIMAL.as_bytes().to_vec();
        bytes.resize(MAX_MANIFEST_BYTES + 1, b' ');
        let problems = parse(&bytes).expect_err("too large");
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].location, "pack.json");
        assert!(problems[0].message.contains("too large"), "{}", problems[0].message);

        bytes.truncate(MAX_MANIFEST_BYTES);
        assert!(parse(&bytes).is_ok(), "exactly the limit is fine");
    }

    #[test]
    fn large_single_line_manifest_with_errors() {
        // Close to the size limit, minified, with an error at the very end: exercises the
        // index on its worst case (one long line).
        let mut src = String::from(
            r#"{"format":1,"id":"big","name":"Big","author":"A","license":"MIT","groups":{"other":{"press":["#,
        );
        while src.len() < MAX_MANIFEST_BYTES - 64 {
            src.push_str(r#""sounds/a-long-file-name.wav","#);
        }
        src.push_str("7]}}}");
        let problems = parse_errors(&src);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].location, format!("pack.json:1:{}", src.len() - 4));
        assert!(problems[0].message.contains("groups.other.press["), "{}", problems[0]);
    }

    #[test]
    fn empty_file() {
        for src in ["", "  \n", "\u{FEFF}"] {
            let problems = parse_errors(src);
            assert_eq!(problems, [Problem::error("pack.json:1:1", "pack.json is empty")]);
        }
    }

    #[test]
    fn syntax_error_has_line_and_column() {
        let src = "{\n  \"format\": 1,\n  \"id\": \"x\"\n  \"name\": \"y\"\n}";
        let problems = parse_errors(src);
        assert_eq!(
            problems,
            [Problem::error("pack.json:4:3", "invalid JSON: expected `,` or `}`")]
        );
    }

    #[test]
    fn syntax_error_columns_count_characters() {
        // "é" is two bytes; the column must still point at the stray `x`.
        let src = "{\"name\": \"é\" x}";
        let problems = parse_errors(src);
        assert_eq!(problems[0].location, "pack.json:1:14");
    }

    #[test]
    fn friendly_syntax_messages() {
        let cases = [
            ("{\"id\": \"a\",}", "trailing comma"),
            ("{\"id\": \"a\"", "an object is missing its }"),
            ("{\"id\": [\"a\"", "an array is missing its ]"),
            ("{\"id\": \"a", "a string is missing its \""),
            ("{} x", "unexpected text after the end of the JSON document"),
            ("{\n  // note\n  \"id\": \"a\"\n}", "comments are not allowed"),
        ];
        for (src, needle) in cases {
            let problems = parse_errors(src);
            assert_eq!(problems.len(), 1);
            assert!(problems[0].message.starts_with("invalid JSON: "), "{}", problems[0]);
            assert!(problems[0].message.contains(needle), "{src:?}: {}", problems[0]);
        }
        assert_eq!(parse_errors("{\n  // note\n}")[0].location, "pack.json:2:3");
    }

    #[test]
    fn root_must_be_an_object() {
        let problems = parse_errors("\n  [1, 2]");
        assert_eq!(
            problems,
            [Problem::error(
                "pack.json:2:3",
                "pack.json must contain a JSON object ({ … }), found an array"
            )]
        );
        assert!(parse_errors("\"pack\"")[0].message.ends_with("found a string"));
    }

    #[test]
    fn missing_required_fields_are_all_reported() {
        let problems = parse_errors("{\n \"format\": 1, \"name\": \"N\"\n}");
        let messages: Vec<_> = problems.iter().map(|p| p.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "missing required field \"id\"",
                "missing required field \"author\"",
                "missing required field \"license\"",
            ]
        );
        assert!(problems.iter().all(|p| p.location == "pack.json:1:1"));
    }

    #[test]
    fn wrong_types_are_all_reported_in_document_order() {
        let src = r#"{
  "format": "1",
  "id": "x",
  "name": 7,
  "author": "a",
  "license": "MIT",
  "volume": "loud",
  "trim_silence": "yes",
  "variation": { "pitch": "high", "volume": 0.1 },
  "groups": {
    "space": { "press": ["ok.wav", 3], "release": "up.wav" },
    "enter": ["enter.wav"]
  },
  "keys": null
}"#;
        let problems = parse_errors(src);
        let got: Vec<(&str, &str)> =
            problems.iter().map(|p| (p.location.as_str(), p.message.as_str())).collect();
        assert_eq!(
            got,
            [
                (
                    "pack.json:2:13",
                    "wrong type for \"format\": expected a whole number (the format version, \
                     1), found a string"
                ),
                (
                    "pack.json:4:11",
                    "wrong type for \"name\": expected a string, found the number 7"
                ),
                ("pack.json:7:13", "wrong type for \"volume\": expected a number, found a string"),
                (
                    "pack.json:8:19",
                    "wrong type for \"trim_silence\": expected true or false, found a string"
                ),
                (
                    "pack.json:9:27",
                    "wrong type for \"variation.pitch\": expected a number, found a string"
                ),
                (
                    "pack.json:11:36",
                    "wrong type for \"groups.space.press[1]\": expected a file path string, \
                     found the number 3"
                ),
                (
                    "pack.json:11:51",
                    "wrong type for \"groups.space.release\": expected an array of file paths, \
                     found a string"
                ),
                (
                    "pack.json:12:14",
                    "wrong type for \"groups.enter\": expected a sound set like { \"press\": \
                     [...], \"release\": [...] }, found an array"
                ),
                (
                    "pack.json:14:11",
                    "wrong type for \"keys\": expected an object of sound sets, found null"
                ),
            ],
            "\n{}",
            render(&problems)
        );
    }

    #[test]
    fn format_must_be_a_u32() {
        for bad in ["1.5", "-1", "true", "null", "\"1\""] {
            let src = MINIMAL.replace("\"format\": 1", &format!("\"format\": {bad}"));
            let problems = parse_errors(&src);
            assert_eq!(problems.len(), 1, "{bad}");
            assert_eq!(problems[0].location, "pack.json:2:19", "{bad}");
            assert!(problems[0].message.starts_with("wrong type for \"format\""), "{bad}");
        }
        let src = MINIMAL.replace("\"format\": 1", "\"format\": 4294967295");
        assert_eq!(manifest(&src).format, u32::MAX);
        // Beyond u32 is still "newer", not a type error.
        let src = MINIMAL.replace("\"format\": 1", "\"format\": 4294967296");
        assert!(parse_errors(&src)[0].message.starts_with("this pack needs a newer TakTak"));
    }

    #[test]
    fn non_object_variation_is_a_type_error() {
        let src = MINIMAL.replace("\"id\"", "\"variation\": 0.1, \"id\"");
        let problems = parse_errors(&src);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message.starts_with("wrong type for \"variation\""));
    }

    #[test]
    fn duplicate_keys_are_errors() {
        let src = "{\n\"id\": \"a\",\n\"id\": \"b\", \"format\": 1, \"name\": \"N\", \"author\": \
                   \"A\", \"license\": \"MIT\",\n\"keys\": {\"KeyA\": {}, \"KeyA\": {}}}";
        let problems = parse_errors(src);
        assert_eq!(
            problems,
            [
                Problem::error("pack.json:3:1", "duplicate field \"id\""),
                Problem::error("pack.json:4:22", "duplicate field \"keys.KeyA\""),
            ]
        );
    }

    #[test]
    fn duplicate_keys_count_only_where_taktak_reads_the_value() {
        let with = |body: &str| MINIMAL.replace("\"groups\"", &format!("{body}, \"groups\""));
        // Read by TakTak: an error, wherever it is.
        for (body, path) in [
            (r#""variation": {"pitch": 0.01, "pitch": 0.02}"#, "variation.pitch"),
            (r#""variation": {}, "variation": {}"#, "variation"),
            (r#""keys": {"KeyA": {"press": ["a.wav"], "press": ["b.wav"]}}"#, "keys.KeyA.press"),
            (r#""keys": {"KeyB": {}, "KeyB": {}}"#, "keys.KeyB"),
            (r#""volume": 1, "volume": 1"#, "volume"),
        ] {
            let found = parse_errors(&with(body));
            assert_eq!(found.len(), 1, "{body}: {found:?}");
            assert_eq!(found[0].message, format!("duplicate field {path:?}"), "{body}");
        }
        // Inside what the spec says is ignored: unknown fields (top level or in a sound set,
        // whole and inside) and unknown `variation` entries. These packs load.
        for body in [
            r#""variation": {"future": 1, "future": 2}"#,
            r#""x-meta": {"a": 1, "a": 2}"#,
            r#""x-meta": [{"a": 1, "a": 2}]"#,
            r#""x-meta": 1, "x-meta": 2"#,
            r#""keys": {"KeyA": {"press": ["a.wav"], "gain": {"x": 1, "x": 2}}}"#,
            r#""keys": {"KeyA": {"press": ["a.wav"], "gain": 1, "gain": 2}}"#,
        ] {
            let m = manifest(&with(body));
            assert!(validate(&m, true).iter().all(|p| !p.is_error()), "{body}");
        }
    }

    #[test]
    fn escaped_keys_are_decoded() {
        // "\u0069d" is "id": serde sees it as the id field, and so must the shape check.
        let src = MINIMAL.replace("\"id\"", "\"\\u0069d\"");
        assert_eq!(manifest(&src).id, "tiny");
        let src = r#"{"format":1,"id":"a","name":"A","author":"B","license":"MIT",
                      "keys":{"Key\u0041":{"press":[1]}}}"#;
        let problems = parse_errors(src);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message.contains("\"keys.KeyA.press[0]\""), "{}", problems[0]);
        assert_eq!(problems[0].location, "pack.json:2:53");
    }

    #[test]
    fn shape_check_is_at_least_as_strict_as_serde() {
        // (body, accepted by the shape check)
        let cases = [
            (r#""volume": 1e3"#, true),
            (r#""volume": -5"#, true),
            (r#""version": 3"#, false),
            (r#""preview": ["a.wav"]"#, false),
            (r#""trim_silence": 1"#, false),
            (r#""variation": {}"#, true),
            (r#""variation": {"pitch": 1, "extra": "x"}"#, true),
            (r#""variation": {"pitch": [0.1]}"#, false),
            (r#""keys": {}"#, true),
            (r#""keys": {"KeyA": {"press": null}}"#, false),
            (r#""keys": {"KeyA": {"release": []}}"#, true),
            (r#""keys": {"KeyA": null}"#, false),
            (r#""keys": {"KeyA": {"press": [null]}}"#, false),
            (r#""keys": {"KeyA": {"press": "a.wav"}}"#, false),
            (r#""unknown": null"#, true),
            // Serde would accept these; the format does not.
            (r#""variation": []"#, false),
            (r#""variation": [0.05, 0.2]"#, false),
            (r#""keys": {"KeyA": {}, "KeyA": {}}"#, false),
        ];
        for (body, expected) in cases {
            let src = MINIMAL.replace("\"groups\"", &format!("{body}, \"groups\""));
            let value: Value = serde_json::from_str(&src).expect("valid json");
            let index = Index::build(src.as_bytes());
            let shape_ok = shape_problems(value.as_object().expect("object"), &index).is_empty();
            assert_eq!(shape_ok, expected, "{body}");
            if shape_ok {
                assert!(serde_json::from_str::<Manifest>(&src).is_ok(), "{body}");
            }
        }
    }

    #[test]
    fn serde_fallback_messages_are_friendly() {
        // Direct use of the fallback path, which normally never runs.
        let src = br#"{"format": 1}"#;
        let err = serde_json::from_slice::<Manifest>(src).expect_err("missing id");
        let p = serde_problem(src, &err);
        assert_eq!(p.message, "missing required field \"id\"");
        assert_eq!(p.location, "pack.json:1:13");

        let src = br#"{"format": "x"}"#;
        let err = serde_json::from_slice::<Manifest>(src).expect_err("bad type");
        let p = serde_problem(src, &err);
        assert!(p.message.starts_with("wrong type: expected u32, found string"), "{p}");
    }

    // --- validate -----------------------------------------------------------------------------

    #[test]
    fn minimal_manifest_only_warns_about_release() {
        let problems = validate(&manifest(MINIMAL), true);
        assert_eq!(
            problems,
            [Problem::warning("release", "pack has no release sounds; key-up will be silent")]
        );
    }

    /// `MINIMAL`, changed by `f`.
    fn minimal(f: impl FnOnce(&mut Manifest)) -> Manifest {
        let mut m = manifest(MINIMAL);
        f(&mut m);
        m
    }

    /// Validation problems other than the "no release sounds" warning.
    fn problems(m: &Manifest) -> Vec<Problem> {
        validate(m, false).into_iter().filter(|p| p.location != "release").collect()
    }

    fn single(m: &Manifest) -> Problem {
        let found = problems(m);
        assert_eq!(found.len(), 1, "expected one problem, got:\n{}", render(&found));
        found.into_iter().next().expect("one problem")
    }

    #[test]
    fn full_manifest_is_clean_even_in_strict_mode() {
        assert_eq!(validate(&manifest(FULL), true), []);
    }

    #[test]
    fn format_version() {
        let p = single(&minimal(|m| m.format = 2));
        assert_eq!(p.location, "format");
        assert!(p.is_error());
        assert!(p.message.starts_with("this pack needs a newer TakTak"), "{p}");
        assert!(p.message.contains("format 2"), "{p}");

        let p = single(&minimal(|m| m.format = 0));
        assert_eq!(p, Problem::error("format", "invalid pack format 0; the current format is 1"));
    }

    #[test]
    fn newer_format_hides_format_1_rules() {
        let m = minimal(|m| {
            m.format = 2;
            m.id = "Not Valid".to_owned();
            m.groups.clear();
        });
        let found = validate(&m, true);
        assert_eq!(found.len(), 1, "{}", render(&found));
        assert_eq!(found[0].location, "format");
        assert!(found[0].message.starts_with("this pack needs a newer TakTak"));

        // A newer pack that does not even match the format-1 shape.
        let problems = parse_errors("{\n  \"format\": 2,\n  \"sounds\": []\n}");
        assert_eq!(
            problems,
            [Problem::error(
                "pack.json:2:13",
                "this pack needs a newer TakTak (it uses pack format 2; this version reads \
                 format 1)"
            )]
        );
    }

    #[test]
    fn id_rules() {
        for id in ["a", "0", "deep-thock", "deep-thock-2", &"a".repeat(64)] {
            assert_eq!(problems(&minimal(|m| m.id = id.to_owned())), [], "{id}");
        }
        let p = single(&minimal(|m| m.id = String::new()));
        assert_eq!(p, Problem::error("id", "id must not be empty"));

        let p = single(&minimal(|m| m.id = "a".repeat(65)));
        assert_eq!(p, Problem::error("id", "id is too long (65 characters; the limit is 64)"));

        let p = single(&minimal(|m| m.id = "Deep Thock".to_owned()));
        assert_eq!(p.location, "id");
        assert_eq!(
            p.message,
            "invalid id \"Deep Thock\": use lowercase letters, digits and single hyphens, e.g. \
             \"deep-thock\" (did you mean \"deep-thock\"?)"
        );
        for id in ["a--b", "-a", "a-", "a_b", "ä", "A"] {
            let p = single(&minimal(|m| m.id = id.to_owned()));
            assert!(p.message.starts_with("invalid id"), "{id}: {p}");
        }
        assert!(!single(&minimal(|m| m.id = "--".to_owned())).message.contains("did you mean"));
    }

    #[test]
    fn slugify_ids() {
        assert_eq!(slugify("Deep Thock_2"), "deep-thock-2");
        assert_eq!(slugify("  --Cherry MX Blue!! "), "cherry-mx-blue");
        assert_eq!(slugify("ü"), "");
    }

    #[test]
    fn text_lengths_are_trimmed_and_counted_in_characters() {
        let p = single(&minimal(|m| m.name = "   ".to_owned()));
        assert_eq!(p, Problem::error("name", "name must not be empty"));
        let p = single(&minimal(|m| m.name = "n".repeat(65)));
        assert_eq!(p, Problem::error("name", "name is too long (65 characters; the limit is 64)"));
        assert_eq!(problems(&minimal(|m| m.name = format!("  {}  ", "é".repeat(64)))), []);

        let p = single(&minimal(|m| m.author = "\t".to_owned()));
        assert_eq!(p, Problem::error("author", "author must not be empty"));
        let p = single(&minimal(|m| m.author = "a".repeat(129)));
        assert_eq!(p.location, "author");

        let p = single(&minimal(|m| m.version = Some("v".repeat(33))));
        assert_eq!(p.location, "version");
        assert_eq!(problems(&minimal(|m| m.version = Some(String::new()))), []);

        for field in ["description", "source", "attribution"] {
            let long = Some("x".repeat(501));
            let m = minimal(|m| match field {
                "description" => m.description = long,
                "source" => m.source = long,
                _ => m.attribution = long,
            });
            let p = single(&m);
            assert_eq!(p.location, field);
            assert_eq!(
                p.message,
                format!("{field} is too long (501 characters; the limit is 500)")
            );
        }
        assert_eq!(problems(&minimal(|m| m.description = Some("x".repeat(500)))), []);
    }

    #[test]
    fn text_must_not_contain_control_characters() {
        let p = single(&minimal(|m| m.author = "line one\nok    fake-line".to_owned()));
        assert_eq!(
            p,
            Problem::error(
                "author",
                "author must not contain control characters (line breaks, tabs, escapes)"
            )
        );
        for (field, text) in [
            ("name", "Red\u{1b}[31m"),
            ("version", "1\t2"),
            ("description", "desc\u{1b}]0;TITLE\u{7}end"),
            ("source", "a\u{0}b"),
            ("attribution", "x\u{9b}y"),
        ] {
            let text = Some(text.to_owned());
            let m = minimal(|m| match field {
                "name" => m.name = text.clone().unwrap(),
                "version" => m.version = text,
                "description" => m.description = text,
                "source" => m.source = text,
                _ => m.attribution = text,
            });
            assert_eq!(single(&m).location, field);
        }
        // Surrounding whitespace is trimmed before display, so it is not an error.
        assert_eq!(problems(&minimal(|m| m.description = Some("\tNice.\n".to_owned()))), []);
    }

    #[test]
    fn json_keys_in_locations_are_escaped() {
        let m = minimal_with(
            r#""x\nok    spoofed": 1, "z\u001b[2K": 2,
               "keys": {"Key\nA": {"press": ["a.wav"], "y\u0007": 1}}"#,
        );
        let found = problems(&m);
        let got: Vec<&str> = found.iter().map(|p| p.location.as_str()).collect();
        assert_eq!(
            got,
            ["keys.Key\\nA", "keys.Key\\nA.y\\u{7}", "x\\nok    spoofed", "z\\u{1b}[2K"]
        );
        assert!(
            render(&found)
                .lines()
                .all(|line| line.starts_with("  error   ") || line.starts_with("  warning "))
        );
        // Ordinary names are unchanged.
        let m = minimal_with(r#""colour": "red""#);
        assert_eq!(single(&m).location, "colour");
    }

    #[test]
    fn personal_license_warns_or_fails_in_strict_mode() {
        let m = minimal(|m| m.license = PERSONAL_LICENSE.to_owned());
        let p = single(&m);
        assert_eq!(p.location, "license");
        assert_eq!(p.severity, Severity::Warning);

        let strict: Vec<_> = validate(&m, true).into_iter().filter(Problem::is_error).collect();
        assert_eq!(strict.len(), 1);
        assert_eq!(strict[0].location, "license");
        assert!(strict[0].message.contains("CC0-1.0, CC-BY-3.0"), "{}", strict[0]);
    }

    #[test]
    fn rejected_licenses_explain_and_list_allowed_ids() {
        let cases = [
            ("GPL-3.0", None, None),
            ("BSD-4-Clause", None, None),
            ("CC-BY-2.0", None, None),
            ("CC-BY-SA-4.0", Some("share-alike"), None),
            ("CC-BY-NC-4.0", Some("non-commercial"), None),
            ("CC-BY-NC-SA-3.0", Some("non-commercial"), None),
            ("CC-BY-ND-4.0", Some("no-derivatives"), None),
            ("cc0-1.0", None, Some("CC0-1.0")),
            ("CC0", None, Some("CC0-1.0")),
            ("Apache 2.0", None, Some("Apache-2.0")),
            ("mit", None, Some("MIT")),
            ("CC BY 4.0", None, Some("CC-BY-4.0")),
        ];
        for (license, why, suggestion) in cases {
            let p = single(&minimal(|m| m.license = license.to_owned()));
            assert_eq!(p.location, "license");
            assert!(p.is_error());
            assert!(p.message.starts_with(&format!("license {license:?} is not allowed")), "{p}");
            assert!(p.message.contains(&ALLOWED_LICENSES.join(", ")), "{p}");
            assert!(p.message.contains(PERSONAL_LICENSE), "{p}");
            if let Some(why) = why {
                assert!(p.message.contains(why), "{license}: {p}");
            }
            match suggestion {
                Some(s) => assert!(p.message.contains(&format!("did you mean {s:?}")), "{p}"),
                None => assert!(!p.message.contains("did you mean"), "{p}"),
            }
        }
        let p = single(&minimal(|m| m.license = " ".to_owned()));
        assert!(p.message.starts_with("license must not be empty"), "{p}");
    }

    #[test]
    fn cc_by_requires_attribution() {
        for license in ["CC-BY-3.0", "CC-BY-4.0"] {
            let p = single(&minimal(|m| m.license = license.to_owned()));
            assert_eq!(p.location, "attribution");
            assert!(p.message.starts_with(&format!("attribution is required for {license}")));

            let blank = minimal(|m| {
                m.license = license.to_owned();
                m.attribution = Some("  ".to_owned());
            });
            assert_eq!(single(&blank).location, "attribution");

            let credited = minimal(|m| {
                m.license = license.to_owned();
                m.attribution = Some("Recordings by Jane Doe".to_owned());
            });
            assert_eq!(problems(&credited), []);
        }
        assert_eq!(problems(&minimal(|m| m.license = "MIT".to_owned())), []);
    }

    #[test]
    fn volume_and_variation_ranges() {
        for ok in [0.0, 1.0, 2.0] {
            assert_eq!(problems(&minimal(|m| m.volume = Some(ok))), [], "{ok}");
        }
        let p = single(&minimal(|m| m.volume = Some(3.0)));
        assert_eq!(p, Problem::error("volume", "volume must be between 0.0 and 2.0 (found 3.0)"));
        assert_eq!(single(&minimal(|m| m.volume = Some(-0.1))).location, "volume");
        assert_eq!(single(&minimal(|m| m.volume = Some(f32::INFINITY))).location, "volume");

        // JSON 0.10 and 0.50 must count as inside the range.
        let edge = minimal_with(r#""variation": {"pitch": 0.10, "volume": 0.50}"#);
        assert_eq!(problems(&edge), []);
        let m = minimal_with(r#""variation": {"pitch": 0.11, "volume": 0.51}"#);
        let found = problems(&m);
        let locations: Vec<_> = found.iter().map(|p| p.location.as_str()).collect();
        assert_eq!(locations, ["variation.pitch", "variation.volume"]);
        assert_eq!(found[0].message, "variation.pitch must be between 0.0 and 0.1 (found 0.11)");
        let m = minimal(|m| m.variation = Some(VariationSpec { pitch: Some(-0.01), volume: None }));
        assert_eq!(single(&m).location, "variation.pitch");
    }

    #[test]
    fn group_names_get_suggestions() {
        let cases = [
            ("modifier", "modifiers"),
            ("mods", "modifiers"),
            ("alpha", "alphanumeric"),
            ("alnum", "alphanumeric"),
            ("letters", "alphanumeric"),
            ("Alpha_Numeric", "alphanumeric"),
            ("default", "other"),
            ("fallback", "other"),
            ("others", "other"),
            ("Space", "space"),
            ("spacebar", "space"),
            ("return", "enter"),
            ("delete", "backspace"),
            ("bakspace", "backspace"),
        ];
        for (name, suggestion) in cases {
            let m = minimal(|m| {
                m.groups.insert(name.to_owned(), set(&["x.wav"], &[]));
            });
            let p = single(&m);
            assert_eq!(p.location, format!("groups.{name}"));
            assert_eq!(
                p.message,
                format!("unknown group {name:?} (did you mean {suggestion:?}?)"),
                "{name}"
            );
        }
        let m = minimal(|m| {
            m.groups.insert("KeyA".to_owned(), set(&["x.wav"], &[]));
            m.groups.insert("wibble".to_owned(), set(&["x.wav"], &[]));
        });
        let found = problems(&m);
        assert_eq!(found[0].location, "groups.KeyA");
        assert_eq!(found[0].message, "\"KeyA\" is a key name, not a group; move it to \"keys\"");
        assert_eq!(found[1].location, "groups.wibble");
        assert_eq!(
            found[1].message,
            "unknown group \"wibble\" (groups are alphanumeric, space, enter, backspace, \
             modifiers, other)"
        );
    }

    #[test]
    fn key_names_get_suggestions() {
        let cases = [
            ("keya", "KeyA"),
            ("KEYA", "KeyA"),
            ("key-a", "KeyA"),
            ("a", "KeyA"),
            ("Z", "KeyZ"),
            ("1", "Digit1"),
            ("space", "Space"),
            ("spacebar", "Space"),
            ("return", "Enter"),
            ("enter", "Enter"),
            ("esc", "Escape"),
            ("shift", "ShiftLeft"),
            ("ctrl", "ControlLeft"),
            ("control", "ControlLeft"),
            ("alt", "AltLeft"),
            ("option", "AltLeft"),
            ("cmd", "MetaLeft"),
            ("command", "MetaLeft"),
            ("meta", "MetaLeft"),
            ("win", "MetaLeft"),
            ("super", "MetaLeft"),
            ("bksp", "Backspace"),
            ("backspace", "Backspace"),
            ("del", "Delete"),
            ("left", "ArrowLeft"),
            ("right", "ArrowRight"),
            ("up", "ArrowUp"),
            ("down", "ArrowDown"),
            ("f5", "F5"),
            ("arrow_up", "ArrowUp"),
            (";", "Semicolon"),
            ("/", "Slash"),
            ("Spcae", "Space"),
            ("Digt1", "Digit1"),
            ("ArowUp", "ArrowUp"),
            ("Backspce", "Backspace"),
        ];
        for (name, suggestion) in cases {
            let m = minimal(|m| {
                m.keys.insert(name.to_owned(), set(&["x.wav"], &[]));
            });
            let p = single(&m);
            assert_eq!(p.location, format!("keys.{name}"));
            assert_eq!(
                p.message,
                format!("unknown key name {name:?} (did you mean {suggestion:?}?)"),
                "{name}"
            );
        }
    }

    #[test]
    fn key_name_without_suggestion_and_group_names_as_keys() {
        let m = minimal(|m| {
            m.keys.insert("modifiers".to_owned(), set(&["x.wav"], &[]));
            m.keys.insert("zzzzzz".to_owned(), set(&["x.wav"], &[]));
        });
        let found = problems(&m);
        assert_eq!(found[0].location, "keys.modifiers");
        assert_eq!(
            found[0].message,
            "\"modifiers\" is a group name, not a key; move it to \"groups\""
        );
        assert_eq!(found[1].location, "keys.zzzzzz");
        assert_eq!(
            found[1].message,
            "unknown key name \"zzzzzz\" (expected a KeyboardEvent.code name such as \"KeyA\")"
        );
    }

    #[test]
    fn spec_example_message_for_unknown_key() {
        let m = minimal(|m| {
            m.keys.insert("keya".to_owned(), set(&["x.wav"], &[]));
        });
        let p = single(&m);
        assert_eq!(p.to_string(), format!("{:<7} {:<18} {}", "error", "keys.keya", p.message));
        assert_eq!(p.message, "unknown key name \"keya\" (did you mean \"KeyA\"?)");
    }

    #[test]
    fn long_names_are_shortened_in_messages() {
        let name = "q".repeat(500);
        let m = minimal(|m| {
            m.keys.insert(name.clone(), set(&["x.wav"], &[]));
        });
        let p = single(&m);
        assert_eq!(p.location, format!("keys.{name}"));
        assert!(p.message.len() < 200, "{}", p.message);
        assert!(p.message.contains(&format!("\"{}…\"", "q".repeat(60))), "{}", p.message);
    }

    #[test]
    fn every_path_is_checked_with_its_location() {
        let m = minimal(|m| {
            m.preview = Some("/preview.wav".to_owned());
            m.groups.insert("space".to_owned(), set(&["ok.wav", "sp\\ace.wav"], &["up.flac"]));
            m.keys.insert("KeyA".to_owned(), set(&[], &["../a.wav"]));
            m.keys.insert("Digit1".to_owned(), set(&["", "fine.ogg"], &[]));
        });
        let found = problems(&m);
        let got: Vec<_> = found.iter().map(|p| (p.location.as_str(), p.message.as_str())).collect();
        assert_eq!(
            got,
            [
                (
                    "preview",
                    "invalid path \"/preview.wav\": must be relative to the pack root (no leading /)"
                ),
                (
                    "groups.space.press[1]",
                    "invalid path \"sp\\\\ace.wav\": must use / as the separator, not \\"
                ),
                (
                    "groups.space.release[0]",
                    "invalid path \"up.flac\": must end in .wav, .ogg or .mp3"
                ),
                ("keys.Digit1.press[0]", "invalid path \"\": path is empty"),
                (
                    "keys.KeyA.release[0]",
                    "invalid path \"../a.wav\": must not contain \".\" or \"..\" segments"
                ),
            ],
            "\n{}",
            render(&found)
        );
    }

    #[test]
    fn empty_sound_sets_warn() {
        let m = minimal(|m| {
            m.groups.insert("enter".to_owned(), SoundSet::default());
            m.keys.insert("KeyB".to_owned(), set(&[], &[]));
        });
        let found = problems(&m);
        assert_eq!(
            found,
            [
                Problem::warning("groups.enter", "empty sound set; it has no effect"),
                Problem::warning("keys.KeyB", "empty sound set; it has no effect"),
            ]
        );
    }

    #[test]
    fn fallback_press_rule() {
        let message = "no fallback press sound: add at least one file to \
                       groups.alphanumeric.press or groups.other.press, so that every key \
                       makes a sound";
        let m = minimal(|m| {
            m.groups.clear();
            m.groups.insert("space".to_owned(), set(&["space.wav"], &[]));
            m.keys.insert("KeyA".to_owned(), set(&["a.wav"], &[]));
        });
        assert_eq!(single(&m), Problem::error("groups", message));

        let m = minimal(|m| {
            m.groups.clear();
            m.groups.insert("alphanumeric".to_owned(), set(&[], &["up.wav"]));
        });
        assert_eq!(single(&m), Problem::error("groups", message));

        let m = minimal(|m| {
            m.groups.clear();
            m.groups.insert("other".to_owned(), set(&["o.wav"], &[]));
        });
        assert_eq!(problems(&m), []);
    }

    #[test]
    fn release_warning_only_when_there_is_no_release_anywhere() {
        let warning =
            Problem::warning("release", "pack has no release sounds; key-up will be silent");
        assert!(validate(&manifest(MINIMAL), false).contains(&warning));
        let m = minimal(|m| {
            m.keys.insert("Space".to_owned(), set(&[], &["space-up.wav"]));
        });
        assert!(!validate(&m, false).contains(&warning));
    }

    #[test]
    fn unknown_fields_warn_with_suggestions() {
        let m = minimal_with(
            r#""licence": "MIT", "trimSilence": false, "colour": "red",
               "keys": {"KeyA": {"press": ["a.wav"], "down": ["b.wav"], "pitch": 2}}"#,
        );
        let found = problems(&m);
        let got: Vec<_> = found.iter().map(|p| (p.location.as_str(), p.message.as_str())).collect();
        assert_eq!(
            got,
            [
                (
                    "keys.KeyA.down",
                    "unknown field \"down\" in sound set is ignored (did you mean \"press\"?)"
                ),
                ("keys.KeyA.pitch", "unknown field \"pitch\" in sound set is ignored"),
                ("colour", "unknown field \"colour\" is ignored"),
                ("licence", "unknown field \"licence\" is ignored (did you mean \"license\"?)"),
                (
                    "trimSilence",
                    "unknown field \"trimSilence\" is ignored (did you mean \"trim_silence\"?)"
                ),
            ],
            "\n{}",
            render(&found)
        );
        assert!(found.iter().all(|p| p.severity == Severity::Warning));
    }

    #[test]
    fn output_is_deterministic_with_errors_first() {
        let m = minimal(|m| {
            m.format = 0;
            m.id = "Bad Id".to_owned();
            m.license = "CC-BY-SA-4.0".to_owned();
            m.volume = Some(5.0);
            m.extra.insert("zzz".to_owned(), Value::Null);
            m.groups.insert("enter".to_owned(), SoundSet::default());
            m.groups.insert("modifier".to_owned(), set(&["m.ogg"], &[]));
            m.keys.insert("keyb".to_owned(), set(&["b.mp3"], &[]));
        });
        let first = validate(&m, false);
        assert_eq!(first, validate(&m.clone(), false));
        let got: Vec<_> = first.iter().map(|p| (p.severity, p.location.as_str())).collect();
        use Severity::{Error, Warning};
        assert_eq!(
            got,
            [
                (Error, "format"),
                (Error, "id"),
                (Error, "license"),
                (Error, "volume"),
                (Error, "groups.modifier"),
                (Error, "keys.keyb"),
                (Warning, "groups.enter"),
                (Warning, "release"),
                (Warning, "zzz"),
            ],
            "\n{}",
            render(&first)
        );
    }

    #[test]
    fn suggestion_tables_point_at_real_names() {
        let key_targets =
            KEY_ALIASES.iter().map(|(_, k)| k).chain(PUNCTUATION_KEYS.iter().map(|(_, k)| k));
        for key in key_targets {
            assert!(key.parse::<Key>().is_ok(), "{key}");
        }
        for (_, group) in GROUP_ALIASES {
            assert!(GROUP_NAMES.contains(group), "{group}");
        }
        for (_, field) in FIELD_ALIASES {
            assert!(KNOWN_FIELDS.contains(field), "{field}");
        }
        for (_, field) in SOUND_SET_ALIASES {
            assert!(SOUND_SET_FIELDS.contains(field), "{field}");
        }
        for (_, license) in LICENSE_ALIASES {
            assert!(ALLOWED_LICENSES.contains(license), "{license}");
        }
    }

    #[test]
    fn edit_distances() {
        let d = |a: &str, b: &str| {
            edit_distance(&a.chars().collect::<Vec<_>>(), &b.chars().collect::<Vec<_>>())
        };
        assert_eq!(d("", ""), 0);
        assert_eq!(d("abc", ""), 3);
        assert_eq!(d("kitten", "sitting"), 3);
        assert_eq!(d("spcae", "space"), 2);
        assert_eq!(d("modifier", "modifiers"), 1);
    }

    // --- resolution ---------------------------------------------------------------------------

    fn files(m: &Manifest, key: Key, action: KeyAction) -> Vec<&str> {
        resolve(m, key, action).iter().map(String::as_str).collect()
    }

    #[test]
    fn key_override_press_only_falls_back_for_release() {
        let m = minimal(|m| {
            m.groups.insert("alphanumeric".to_owned(), set(&["a.wav"], &["a-up.wav"]));
            m.keys.insert("KeyA".to_owned(), set(&["ka.wav"], &[]));
        });
        assert_eq!(files(&m, Key::KeyA, KeyAction::Down), ["ka.wav"]);
        assert_eq!(files(&m, Key::KeyA, KeyAction::Up), ["a-up.wav"]);
        assert_eq!(files(&m, Key::KeyB, KeyAction::Down), ["a.wav"]);
    }

    #[test]
    fn space_walks_the_whole_chain() {
        let mut m = minimal(|m| {
            m.groups.insert("alphanumeric".to_owned(), set(&["alnum.wav"], &[]));
            m.groups.insert("other".to_owned(), set(&["other.wav"], &[]));
            m.groups.insert("space".to_owned(), set(&["space.wav"], &[]));
        });
        assert_eq!(files(&m, Key::Space, KeyAction::Down), ["space.wav"]);
        m.groups.remove("space");
        assert_eq!(files(&m, Key::Space, KeyAction::Down), ["other.wav"]);
        m.groups.remove("other");
        assert_eq!(files(&m, Key::Space, KeyAction::Down), ["alnum.wav"]);
        m.groups.clear();
        assert_eq!(files(&m, Key::Space, KeyAction::Down), Vec::<&str>::new());
    }

    #[test]
    fn empty_arrays_do_not_stop_the_chain() {
        let m = minimal(|m| {
            m.keys.insert("Enter".to_owned(), set(&[], &[]));
            m.groups.insert("enter".to_owned(), set(&[], &["enter-up.wav"]));
            m.groups.insert("other".to_owned(), set(&["other.wav"], &[]));
        });
        assert_eq!(files(&m, Key::Enter, KeyAction::Down), ["other.wav"]);
        assert_eq!(files(&m, Key::NumpadEnter, KeyAction::Up), ["enter-up.wav"]);
    }

    #[test]
    fn silent_when_nothing_matches() {
        let m = manifest(MINIMAL);
        assert_eq!(files(&m, Key::KeyQ, KeyAction::Up), Vec::<&str>::new());
        assert_eq!(files(&m, Key::F5, KeyAction::Down), ["a.wav"]);
    }

    #[test]
    fn every_key_resolves_through_its_group() {
        let m = minimal(|m| {
            for g in GROUP_NAMES {
                m.groups.insert(g.to_owned(), set(&[&format!("{g}.wav")], &[]));
            }
        });
        for &key in Key::ALL {
            let expected = format!("{}.wav", group_name(key.group()));
            assert_eq!(files(&m, key, KeyAction::Down), [expected.as_str()]);
        }
        assert_eq!(files(&m, Key::MetaRight, KeyAction::Down), ["modifiers.wav"]);
        assert_eq!(files(&m, Key::Delete, KeyAction::Down), ["backspace.wav"]);
    }

    #[test]
    fn preview_fallbacks() {
        let full = manifest(FULL);
        assert_eq!(preview_file(&full), Some("preview.wav"));
        let mut m = minimal(|m| {
            m.groups.insert("other".to_owned(), set(&["o.wav"], &[]));
        });
        assert_eq!(preview_file(&m), Some("a.wav"));
        m.groups.insert("alphanumeric".to_owned(), set(&[], &["a-up.wav"]));
        assert_eq!(preview_file(&m), Some("o.wav"));
        m.groups.clear();
        assert_eq!(preview_file(&m), None);
    }

    #[test]
    fn referenced_files_are_distinct_and_include_preview() {
        let got = referenced_files(&manifest(FULL));
        let expected: BTreeSet<String> = [
            "preview.wav",
            "sounds/alnum-1.wav",
            "sounds/alnum-2.wav",
            "sounds/alnum-up.wav",
            "sounds/space.wav",
            "sounds/space-up.wav",
            "sounds/KeyA-press.wav",
            "sounds/KeyA-release.wav",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(got, expected);
    }
}
