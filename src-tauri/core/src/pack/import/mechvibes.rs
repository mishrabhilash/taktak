//! Reading Mechvibes `config.json` files (v1 `single`/`multi`, v2, Mechvibes++ and
//! MechvibesDX) into an import [`Plan`]: which clip of which file each key plays on press and
//! release. No audio is touched here.

use super::input::Input;
use super::keycodes::{self, is_secondary, is_strong_evdev_signal};
use super::{ImportError, ImportOptions, Notes, SourceFormat};
use crate::key::Key;
use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fmt;

/// Most files one `{a-b}` range may expand to.
const MAX_RANGE: u64 = 64;

/// A piece of audio: a whole file, or `[start, end)` of it in microseconds.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Clip {
    pub file: String,
    pub range: Option<(i64, i64)>,
}

/// How to treat a press clip that holds a whole keystroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SplitHint {
    /// A Mechvibes v1 sprite slice: split at the detected release transient, if the pack's
    /// slices have clear ones.
    Detect,
    /// A MechvibesDX press/release pair that was cut at its midpoint: split at the detected
    /// release transient, else at the pair's own boundary (microseconds).
    Boundary(i64),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct KeyPlan {
    /// Variants (several only for Mechvibes v2 `{a-b}` ranges).
    pub press: Vec<Clip>,
    pub release: Vec<Clip>,
    /// Set when `press` is one clip with both strokes in it and `release` is empty.
    pub split: Option<SplitHint>,
}

pub(crate) struct Plan {
    pub format: SourceFormat,
    pub name: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub keys: BTreeMap<Key, KeyPlan>,
    /// Sounds for keys the pack does not define (Mechvibes v2 `sound`/`soundup`).
    pub fallback: KeyPlan,
    pub volume: Option<f32>,
    pub pitch: Option<f32>,
}

/// A JSON object's entries in document order, duplicates included.
struct Entries(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Entries;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Entries, A::Error> {
                let mut out = Vec::new();
                while let Some(entry) = map.next_entry::<String, Value>()? {
                    out.push(entry);
                }
                Ok(Entries(out))
            }
        }
        d.deserialize_map(V)
    }
}

#[derive(Deserialize)]
struct RawConfig {
    #[serde(default)]
    defines: Option<Entries>,
    #[serde(default)]
    definitions: Option<Entries>,
    #[serde(default)]
    defs: Option<Entries>,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

/// Parses a config: strict JSON first (after stripping a BOM), then with comments and
/// trailing commas removed.
fn parse_config(bytes: &[u8], notes: &mut Notes) -> Result<RawConfig, ImportError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(ImportError::InvalidConfig("config.json is empty".into()));
    }
    match serde_json::from_slice::<RawConfig>(bytes) {
        Ok(config) => Ok(config),
        Err(strict) => {
            let text = String::from_utf8_lossy(bytes);
            let relaxed = strip_jsonc(&text);
            match serde_json::from_str::<RawConfig>(&relaxed) {
                Ok(config) => {
                    notes.warn("config.json is not strict JSON (comments or trailing commas)");
                    Ok(config)
                }
                Err(_) => Err(ImportError::InvalidConfig(format!("config.json: {strict}"))),
            }
        }
    }
}

/// Removes `//` and `/* */` comments and trailing commas outside strings.
fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
            }
            _ => out.push(c),
        }
    }
    // Trailing commas: a comma whose next non-space character closes an object or array.
    let mut result = String::with_capacity(out.len());
    let chars: Vec<char> = out.chars().collect();
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            result.push(c);
            if c == '\\' {
                if let Some(&n) = chars.get(i + 1) {
                    result.push(n);
                    i += 1;
                }
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            result.push(c);
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                result.push(c);
            }
        } else {
            result.push(c);
        }
        i += 1;
    }
    result
}

fn text(rest: &Map<String, Value>, field: &str) -> Option<String> {
    rest.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|n| n.is_finite())
}

fn micros(ms: f64) -> i64 {
    (ms * 1000.0).round() as i64
}

/// Builds the import plan from the pack's config.
pub(crate) fn plan(
    input: &mut Input,
    config_name: &str,
    opts: &ImportOptions,
    notes: &mut Notes,
) -> Result<Plan, ImportError> {
    let bytes = input.read_config(config_name)?;
    let raw = parse_config(&bytes, notes)?;
    let author = text(&raw.rest, "author").or_else(|| text(&raw.rest, "m_author"));
    let name = text(&raw.rest, "name");
    let description = text(&raw.rest, "description");
    let mut plan = if raw.definitions.is_some() || raw.defs.is_some() {
        plan_dx(input, raw, opts, notes)?
    } else {
        plan_mechvibes(input, raw, notes)?
    };
    plan.name = name;
    plan.author = author;
    plan.description = description;
    Ok(plan)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Sprite,
    Files,
}

/// A parsed `defines` key: leading zeros, the code, and whether it is an `-up` (release) key.
fn parse_code(raw: &str) -> Option<(usize, u32, bool)> {
    let raw = raw.trim();
    let (digits, up) = match raw.strip_suffix("-up") {
        Some(d) => (d, true),
        None => (raw, false),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || digits.len() > 9 {
        return None;
    }
    let trimmed = digits.trim_start_matches('0');
    let zeros = digits.len() - trimmed.len();
    let code: u32 = if trimmed.is_empty() { 0 } else { trimmed.parse().ok()? };
    Some((zeros, code, up))
}

/// One candidate sound for a key and action; the lowest `rank` wins, later entries win ties.
struct Candidate {
    key: Key,
    release: bool,
    rank: u8,
    clips: Vec<Clip>,
}

fn plan_mechvibes(
    input: &mut Input,
    raw: RawConfig,
    notes: &mut Notes,
) -> Result<Plan, ImportError> {
    let rest = &raw.rest;
    let version = rest.get("version").and_then(number).map(|v| v as i64).unwrap_or(1);
    if version >= 3 {
        return Err(ImportError::Unsupported(format!(
            "Mechvibes config version {version} is not supported (only versions 1 and 2 exist \
             in released Mechvibes)"
        )));
    }
    let v2 = version == 2;
    let compat = rest.get("compatibility").and_then(Value::as_bool).unwrap_or(false);
    let defines = raw
        .defines
        .ok_or_else(|| ImportError::InvalidConfig("config.json has no \"defines\"".into()))?
        .0;

    let mode = match rest.get("key_define_type").and_then(Value::as_str) {
        Some("single") => Mode::Sprite,
        Some(other) => {
            if !matches!(other, "multi" | "multiple") {
                notes.warn(format!("unknown key_define_type {other:?}; reading it as \"multi\""));
            }
            Mode::Files
        }
        None => {
            let arrays = defines.iter().filter(|(_, v)| v.is_array()).count();
            let strings = defines.iter().filter(|(_, v)| v.is_string()).count();
            if arrays > strings { Mode::Sprite } else { Mode::Files }
        }
    };
    let format = match (v2, compat, mode) {
        (true, _, _) => SourceFormat::MechvibesV2,
        (false, true, m) => SourceFormat::MechvibesPlusPlus { sprite: m == Mode::Sprite },
        (false, false, Mode::Sprite) => SourceFormat::MechvibesV1Sprite,
        (false, false, Mode::Files) => SourceFormat::MechvibesV1Files,
    };

    let sprite = if mode == Mode::Sprite {
        let sound = text(rest, "sound").unwrap_or_default();
        match input.resolve(&sound) {
            Some(file) => Some(file),
            None => {
                notes.missing(&sound);
                return Err(ImportError::NoSounds(format!(
                    "the sprite file {sound:?} named by \"sound\" is not in the pack"
                )));
            }
        }
    } else {
        None
    };

    // Mechvibes++ mouse packs use 1/2/3 and 01/02/03; nothing of a keyboard pack is that small.
    let codes: Vec<(usize, u32, bool)> =
        defines.iter().filter_map(|(k, _)| parse_code(k)).collect();
    if !compat
        && !codes.is_empty()
        && codes.iter().all(|&(_, c, _)| (1..=3).contains(&c))
        && codes.iter().any(|&(z, _, _)| z > 0)
    {
        return Err(ImportError::Unsupported("mouse packs are not supported".into()));
    }
    let evdev = codes.iter().any(|&(z, c, _)| z == 0 && is_strong_evdev_signal(c));
    if evdev {
        notes.warn("key codes look Linux evdev-numbered; codes 85–127 are read as evdev codes");
    }

    let mut seen = HashSet::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    for (raw_key, value) in &defines {
        if !seen.insert(raw_key.as_str()) {
            notes.warn(format!("key {raw_key:?} appears twice in defines; the last one is used"));
        }
        let Some((zeros, code, up)) = parse_code(raw_key) else {
            notes.skip(raw_key, "not a Mechvibes key code");
            continue;
        };
        let mut release = up;
        let mut rank = 0u8;
        if zeros > 0 && code != 0 {
            if !compat {
                notes.skip(raw_key, "leading zero, but the pack is not a Mechvibes++ compat pack");
                continue;
            }
            if up {
                notes.skip(raw_key, "not a Mechvibes key code");
                continue;
            }
            // Mechvibes++: "0N" is the press, "00N" the release.
            release = zeros >= 2;
        } else if compat && !up {
            // In a compat pack a plain "N" is only the press when "0N" is absent.
            rank = 1;
        }
        let key = match resolve_key(code, evdev) {
            Ok((key, extra_rank)) => {
                rank += extra_rank;
                key
            }
            Err(reason) => {
                notes.skip(raw_key, &reason);
                continue;
            }
        };
        let clips = match clips_for(input, value, mode, sprite.as_deref(), v2, raw_key, notes) {
            Some(clips) if !clips.is_empty() => clips,
            _ => continue,
        };
        candidates.push(Candidate { key, release, rank, clips });
    }

    let mut keys = choose(candidates);
    if mode == Mode::Sprite {
        for plan in keys.values_mut() {
            if plan.release.is_empty() && plan.press.len() == 1 {
                plan.split = Some(SplitHint::Detect);
            }
        }
    }

    let mut fallback = KeyPlan::default();
    if v2 {
        for (field, release) in [("sound", false), ("soundup", true)] {
            let Some(name) = text(rest, field) else { continue };
            let clips = expand_files(input, &name, true, notes);
            if release {
                fallback.release = clips;
            } else {
                fallback.press = clips;
            }
        }
    }

    Ok(Plan {
        format,
        name: None,
        author: None,
        description: None,
        keys,
        fallback,
        volume: None,
        pitch: None,
    })
}

/// A libuiohook code → key, with a rank penalty for codes that only fill gaps (aliases 2,
/// evdev 3).
fn resolve_key(code: u32, evdev: bool) -> Result<(Key, u8), String> {
    let evdev_key =
        || keycodes::evdev_name(code).map(|name| keycodes::key_for_name(name).map(|k| (k, 3)));
    if evdev
        && (85..=127).contains(&code)
        && let Some(result) = evdev_key()
    {
        return result;
    }
    match keycodes::lookup(code) {
        Ok(key) => Ok((key, if is_secondary(code) { 2 } else { 0 })),
        Err(reason) if keycodes::code_name(code).is_none() => evdev_key().unwrap_or(Err(reason)),
        Err(reason) => Err(reason),
    }
}

/// The clips a `defines` value names, or `None` (with notes) when it names nothing usable.
fn clips_for(
    input: &Input,
    value: &Value,
    mode: Mode,
    sprite: Option<&str>,
    v2: bool,
    raw_key: &str,
    notes: &mut Notes,
) -> Option<Vec<Clip>> {
    match value {
        Value::Null | Value::Bool(false) => None,
        Value::String(s) if s.trim().is_empty() => None,
        Value::Array(parts) => {
            let Some(file) = sprite else {
                notes.warn(format!("key {raw_key:?}: a [start, length] slice in a multi pack"));
                return None;
            };
            let start = parts.first().and_then(number);
            let length = parts.get(1).and_then(number);
            if parts.iter().take(2).any(Value::is_string) {
                notes.warn(format!("key {raw_key:?}: slice times written as strings"));
            }
            match (start, length) {
                (Some(start), Some(length)) if start >= 0.0 && length > 0.0 => Some(vec![Clip {
                    file: file.to_owned(),
                    range: Some((micros(start), micros(start + length))),
                }]),
                (Some(_), Some(length)) if length <= 0.0 => {
                    notes.warn(format!("key {raw_key:?}: zero-length slice skipped"));
                    None
                }
                _ => {
                    notes.warn(format!("key {raw_key:?}: invalid slice {value}"));
                    None
                }
            }
        }
        Value::String(name) => {
            if mode == Mode::Sprite {
                notes
                    .warn(format!("key {raw_key:?}: a file name in a sprite pack; using the file"));
            }
            Some(expand_files(input, name, v2, notes))
        }
        other => {
            notes.warn(format!("key {raw_key:?}: unexpected value {other}"));
            None
        }
    }
}

/// Resolves a file name (expanding a v2 `{a-b}` range) to whole-file clips. Missing files are
/// noted and left out.
fn expand_files(input: &Input, name: &str, ranges: bool, notes: &mut Notes) -> Vec<Clip> {
    let names = if ranges { expand_range(name) } else { vec![name.to_owned()] };
    let mut clips = Vec::new();
    for n in &names {
        match input.resolve(n) {
            Some(file) => clips.push(Clip { file, range: None }),
            None => notes.missing(n),
        }
    }
    clips.dedup();
    clips
}

/// `GENERIC_R{0-4}.mp3` → `GENERIC_R0.mp3` … `GENERIC_R4.mp3` (the intended inclusive range;
/// Mechvibes itself mis-adds the bounds). Only the first `{…}` is expanded, as in Mechvibes.
fn expand_range(name: &str) -> Vec<String> {
    let Some(open) = name.find('{') else { return vec![name.to_owned()] };
    let Some(close) = name[open..].find('}').map(|i| open + i) else {
        return vec![name.to_owned()];
    };
    let inner = &name[open + 1..close];
    let Some((a, b)) = inner.split_once('-') else { return vec![name.to_owned()] };
    let (Ok(a), Ok(b)) = (a.trim().parse::<u64>(), b.trim().parse::<u64>()) else {
        return vec![name.to_owned()];
    };
    let (lo, hi) = (a.min(b), a.max(b).min(a.min(b) + MAX_RANGE - 1));
    (lo..=hi).map(|i| format!("{}{i}{}", &name[..open], &name[close + 1..])).collect()
}

/// Picks, per key and action, the best-ranked candidate (later entries win ties).
fn choose(candidates: Vec<Candidate>) -> BTreeMap<Key, KeyPlan> {
    let mut best: BTreeMap<(Key, bool), (u8, Vec<Clip>)> = BTreeMap::new();
    for c in candidates {
        let slot = best.entry((c.key, c.release)).or_insert((u8::MAX, Vec::new()));
        if c.rank <= slot.0 {
            *slot = (c.rank, c.clips);
        }
    }
    let mut keys: BTreeMap<Key, KeyPlan> = BTreeMap::new();
    for ((key, release), (_, clips)) in best {
        let plan = keys.entry(key).or_default();
        if release {
            plan.release = clips;
        } else {
            plan.press = clips;
        }
    }
    keys
}

fn plan_dx(
    input: &mut Input,
    raw: RawConfig,
    opts: &ImportOptions,
    notes: &mut Notes,
) -> Result<Plan, ImportError> {
    let rest = &raw.rest;
    if let Some(v) = rest.get("config_version").and_then(number)
        && v > 2.0
    {
        return Err(ImportError::Unsupported(format!(
            "MechvibesDX config_version {v} is newer than this importer understands"
        )));
    }
    let defs = raw.definitions.or(raw.defs).map(|e| e.0).unwrap_or_default();
    let audio_file = text(rest, "audio_file");
    let shared = audio_file.as_deref().and_then(|name| {
        let found = input.resolve(name);
        if found.is_none() {
            notes.missing(name);
        }
        found
    });

    let mut seen = HashSet::new();
    let mut mouse = 0usize;
    let mut candidates = Vec::new();
    for (name, value) in &defs {
        if !seen.insert(name.as_str()) {
            notes.warn(format!("key {name:?} appears twice in definitions; the last one is used"));
        }
        if ["Mouse", "Wheel", "Button"].iter().any(|p| name.starts_with(p)) {
            mouse += 1;
            continue;
        }
        let key = match keycodes::key_for_name(name) {
            Ok(key) => key,
            Err(reason) => {
                notes.skip(name, &reason);
                continue;
            }
        };
        let (timing, own_file) = match value {
            Value::Array(_) => (Some(value), None),
            Value::Object(o) => (o.get("timing"), o.get("audio_file").and_then(Value::as_str)),
            _ => (None, None),
        };
        let file = match own_file {
            Some(own) => match input.resolve(own) {
                Some(f) => f,
                None => {
                    notes.missing(own);
                    continue;
                }
            },
            None => match &shared {
                Some(f) => f.clone(),
                None => {
                    notes.skip(name, "no audio_file to play");
                    continue;
                }
            },
        };
        let pairs: Vec<(f64, f64)> = timing
            .and_then(Value::as_array)
            .map(|pairs| {
                pairs
                    .iter()
                    .filter_map(|p| {
                        let p = p.as_array()?;
                        Some((number(p.first()?)?, number(p.get(1)?)?))
                    })
                    .filter(|&(s, e)| s >= 0.0 && e > s)
                    .collect()
            })
            .unwrap_or_default();
        let clip =
            |(s, e): (f64, f64)| Clip { file: file.clone(), range: Some((micros(s), micros(e))) };
        let mut plan = KeyPlan::default();
        match pairs.as_slice() {
            [] if timing.is_none() && own_file.is_some() => {
                plan.press = vec![Clip { file: file.clone(), range: None }];
            }
            [] => {
                notes.skip(name, "no valid timing");
                continue;
            }
            [press] => plan.press = vec![clip(*press)],
            [press, release, more @ ..] => {
                if !more.is_empty() {
                    notes.warn(format!(
                        "key {name:?}: {} timings; using the first as press and the second as \
                         release",
                        pairs.len()
                    ));
                }
                let contiguous = (press.1 - release.0).abs() <= 1.0;
                if opts.split_release && contiguous && more.is_empty() {
                    plan.press = vec![clip((press.0, release.1))];
                    plan.split = Some(SplitHint::Boundary(micros(press.1)));
                } else {
                    plan.press = vec![clip(*press)];
                    plan.release = vec![clip(*release)];
                }
            }
        }
        candidates.push((key, plan));
    }
    if mouse > 0 && candidates.is_empty() {
        return Err(ImportError::Unsupported("mouse packs are not supported".into()));
    }
    if mouse > 0 {
        notes.warn(format!("{mouse} mouse button definitions skipped"));
    }

    let options = rest.get("options").and_then(Value::as_object);
    let volume = options
        .and_then(|o| o.get("recommended_volume"))
        .and_then(number)
        .map(|v| v.clamp(0.0, 2.0) as f32)
        .filter(|v| (v - 1.0).abs() > 1e-3);
    let pitch = options
        .and_then(|o| o.get("random_pitch"))
        .and_then(Value::as_bool)
        .filter(|&b| b)
        .map(|_| 0.10);
    Ok(Plan {
        format: SourceFormat::MechvibesDx,
        name: None,
        author: None,
        description: None,
        keys: candidates.into_iter().collect(),
        fallback: KeyPlan::default(),
        volume,
        pitch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_parse() {
        assert_eq!(parse_code("30"), Some((0, 30, false)));
        assert_eq!(parse_code("14-up"), Some((0, 14, true)));
        assert_eq!(parse_code("030"), Some((1, 30, false)));
        assert_eq!(parse_code("0030"), Some((2, 30, false)));
        assert_eq!(parse_code("91,91,92"), None);
        assert_eq!(parse_code("KeyA"), None);
        assert_eq!(parse_code(""), None);
    }

    #[test]
    fn ranges_expand() {
        assert_eq!(expand_range("R{0-2}.mp3"), ["R0.mp3", "R1.mp3", "R2.mp3"]);
        assert_eq!(expand_range("R{1-3}.mp3"), ["R1.mp3", "R2.mp3", "R3.mp3"]);
        assert_eq!(expand_range("plain.mp3"), ["plain.mp3"]);
        assert_eq!(expand_range("bad{x}.mp3"), ["bad{x}.mp3"]);
        assert_eq!(expand_range("big{0-1000}.wav").len() as u64, MAX_RANGE);
    }

    #[test]
    fn jsonc_is_tolerated() {
        let text = "{\n // comment\n \"a\": \"x // not a comment\", /* block */ \"b\": [1, 2,],\n}";
        let v: Value = serde_json::from_str(&strip_jsonc(text)).unwrap();
        assert_eq!(v["a"], "x // not a comment");
        assert_eq!(v["b"], serde_json::json!([1, 2]));
    }

    #[test]
    fn duplicate_keys_are_kept_in_order() {
        let mut notes = Notes::default();
        let raw =
            parse_config(br#"{"defines": {"30": "a.wav", "30": "b.wav"}}"#, &mut notes).unwrap();
        let entries = raw.defines.unwrap().0;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].1, "b.wav");
    }

    #[test]
    fn evdev_and_alias_ranks() {
        assert!(matches!(resolve_key(57416, false), Ok((Key::ArrowUp, 0))));
        assert!(matches!(resolve_key(61000, false), Ok((Key::ArrowUp, 2))));
        assert!(matches!(resolve_key(103, false), Ok((Key::F20, 0))));
        assert!(matches!(resolve_key(103, true), Ok((Key::ArrowUp, 3))));
        assert!(matches!(resolve_key(97, false), Ok((Key::ControlRight, 3))));
        assert!(resolve_key(57378, false).is_err());
        assert!(resolve_key(90, true).is_err());
    }
}
