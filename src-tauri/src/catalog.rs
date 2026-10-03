//! The pack list the UI shows, and which pack plays: fallback selection, the user-facing error
//! when the selected pack cannot play, and how the active pack reacts to a batch of hot-reload
//! events. Pure functions over registry data, tested without files or audio.

use crate::state::{DEFAULT_PACK_ID, InvalidPack, PackSummary, RETIRED_PACK_IDS};
use std::fs;
use std::path::{Path, PathBuf};
use taktak_core::pack::registry::PackEntry;
use taktak_core::pack::{self, Manifest, PackError, PackInfo, Problem, RegistryEvent, Severity};

/// How many packs one load tries before giving up on packs: the selected one, the default,
/// then the first other one by name. After that the built-in click plays.
pub const MAX_CANDIDATES: usize = 3;

/// What the UI says when no pack plays.
pub const BUILT_IN: &str = "the built-in click";

/// Pack traits the UI shows that [`PackInfo`] does not carry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Features {
    pub has_release: bool,
    pub per_key: bool,
}

impl Features {
    pub fn of(manifest: &Manifest) -> Features {
        let mut sets = manifest.groups.values().chain(manifest.keys.values());
        Features {
            has_release: sets.any(|set| !set.release.is_empty()),
            per_key: !manifest.keys.is_empty(),
        }
    }
}

/// A pack's [`Features`], from its pack.json. Nothing if it cannot be read (it was valid
/// moments ago when the registry inspected it; the next scan will report what changed).
pub fn read_features(location: &Path) -> Features {
    pack::load::read_manifest(location).map(|m| Features::of(&m)).unwrap_or_default()
}

/// `"<location>: <message>"`, control characters escaped.
pub fn describe(problem: &Problem) -> String {
    format!("{}: {}", pack::printable(&problem.location), pack::printable(&problem.message))
}

/// `"error: <location>: <message>"` (or `warning: …`), control characters escaped.
pub fn problem_line(problem: &Problem) -> String {
    let severity = match problem.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    format!("{severity}: {}", describe(problem))
}

/// The UI's pack list (in `packs`' order, which the registry sorts by name) and invalid-pack
/// list, from the registry's usable packs and all its candidates.
pub fn summarize(
    packs: &[PackInfo],
    entries: &[PackEntry],
    features: impl Fn(&Path) -> Features,
) -> (Vec<PackSummary>, Vec<InvalidPack>) {
    let summaries = packs
        .iter()
        .map(|info| {
            let warnings = entries
                .iter()
                .find(|e| e.location == info.location)
                .and_then(|e| e.status.as_ref().ok())
                .map(|(_, warnings)| warnings.iter().map(describe).collect())
                .unwrap_or_default();
            let Features { has_release, per_key } = features(&info.location);
            PackSummary {
                id: info.id.clone(),
                name: info.name.clone(),
                author: info.author.clone(),
                license: info.license.clone(),
                description: info.description.clone(),
                attribution: info.attribution.clone(),
                origin: info.origin.into(),
                has_release,
                per_key,
                warnings,
            }
        })
        .collect();
    let invalid = entries
        .iter()
        .filter_map(|e| e.status.as_ref().err())
        .map(|err| InvalidPack {
            location: pack::printable(&err.pack.display().to_string()).into_owned(),
            problems: err.problems.iter().map(problem_line).collect(),
        })
        .collect();
    (summaries, invalid)
}

/// The packs to try, in order, for `selected`: itself if installed, then the default pack,
/// then the first other pack by name (at most [`MAX_CANDIDATES`]). Empty → built-in click.
pub fn candidates<'a>(selected: &str, packs: &'a [PackInfo]) -> Vec<&'a PackInfo> {
    let mut out: Vec<&PackInfo> = Vec::with_capacity(MAX_CANDIDATES);
    let wanted = [selected, DEFAULT_PACK_ID];
    let ordered =
        wanted.iter().filter_map(|id| packs.iter().find(|p| p.id == *id)).chain(packs.iter());
    for pack in ordered {
        if out.len() == MAX_CANDIDATES {
            break;
        }
        if !out.iter().any(|p| p.id == pack.id) {
            out.push(pack);
        }
    }
    out
}

/// Where a saved selection moves when TakTak starts: to [`DEFAULT_PACK_ID`] if `selected` is a
/// pack earlier versions bundled ([`RETIRED_PACK_IDS`]) and no installed pack has that id (a
/// user pack of that id keeps the selection). `None` keeps it. Without this, users whose old
/// default was retired would see "not installed" on every start.
pub fn migrate_retired(selected: &str, packs: &[PackInfo]) -> Option<&'static str> {
    (RETIRED_PACK_IDS.contains(&selected) && !packs.iter().any(|p| p.id == selected))
        .then_some(DEFAULT_PACK_ID)
}

/// The first error of `err`, for a one-line message (control characters escaped).
pub fn first_error(err: &PackError) -> String {
    let line = err.errors().next().map(describe).unwrap_or_else(|| "unknown problem".to_owned());
    line.trim_end_matches('.').to_owned()
}

/// How the UI names the selected pack when it is not among the valid packs but one of the
/// invalid ones is it (it broke on disk, or was broken when TakTak started): `None` if no
/// invalid entry is the selected pack. An entry is it when it sits where the pack was last seen
/// (`known`: location and name), when its pack.json still names the selected id (`read` gives
/// a location's manifest id and name, if pack.json parses), or when its folder or zip is named
/// after the id.
pub fn broken_label(
    selected: &str,
    entries: &[PackEntry],
    known: Option<(&Path, &str)>,
    read: impl Fn(&Path) -> Option<(String, String)>,
) -> Option<String> {
    let quoted = || format!("The pack “{}”", pack::printable(selected));
    entries.iter().filter(|e| e.status.is_err()).find_map(|entry| {
        if let Some((location, name)) = known
            && entry.location == location
        {
            return Some(name.to_owned());
        }
        if let Some((id, name)) = read(&entry.location) {
            return (id == selected).then_some(name);
        }
        (entry.location.file_stem().is_some_and(|stem| stem == selected)).then(quoted)
    })
}

/// A broken pack's manifest id and name, for [`broken_label`]: nothing if pack.json cannot be
/// read or parsed.
pub fn read_identity(location: &Path) -> Option<(String, String)> {
    pack::load::read_manifest(location).ok().map(|m| (m.id, m.name))
}

/// What `activePackError` says after a load: `None` when `selected` plays; otherwise why it
/// does not and what plays instead. `broken` is [`broken_label`]'s answer for `selected`.
pub fn active_pack_error(
    selected: &str,
    packs: &[PackInfo],
    playing: Option<&PackInfo>,
    failures: &[(PackInfo, PackError)],
    broken: Option<&str>,
) -> Option<String> {
    if playing.is_some_and(|p| p.id == selected) {
        return None;
    }
    let instead = playing.map_or(BUILT_IN, |p| p.name.as_str());
    if let Some((info, err)) = failures.iter().find(|(info, _)| info.id == selected) {
        let why = first_error(err);
        return Some(format!(
            "{} could not be loaded: {why}. Playing {instead} instead.",
            info.name
        ));
    }
    if !packs.iter().any(|p| p.id == selected)
        && let Some(label) = broken
    {
        return Some(format!(
            "{label} has errors (see the invalid packs). Playing {instead} instead."
        ));
    }
    if packs.is_empty() {
        return Some(format!("No sound packs found. Playing {BUILT_IN} instead."));
    }
    Some(format!(
        "The pack “{}” is not installed. Playing {instead} instead.",
        pack::printable(selected)
    ))
}

/// What `activePackError` says while the selected pack is broken on disk and its last good
/// bank keeps playing.
pub fn stale_message(name: &str) -> String {
    format!(
        "{name} has errors (see the invalid packs), so it was not reloaded. Its last working \
         version keeps playing."
    )
}

/// What plays and why, as far as hot reload is concerned.
#[derive(Clone, Copy, Debug)]
pub struct ActiveView<'a> {
    /// `settings.packId`.
    pub selected: &'a str,
    /// The pack whose bank is in the engine: id and location. `None` = built-in click (or
    /// nothing loaded yet).
    pub playing: Option<(&'a str, &'a Path)>,
    /// Set while the selected pack is invalid on disk and its last good bank keeps playing.
    pub stale: Option<&'a Path>,
}

/// What to do about a batch of registry events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reaction {
    /// Nothing that plays is affected.
    Keep,
    /// Run the selection again ([`candidates`]) and load the result.
    Reload,
    /// The selected pack broke on disk: keep playing its old bank, and say so.
    KeepOldBank { location: PathBuf },
}

/// How the active pack reacts to one batch of registry events (`PackRegistry::scan` order).
///
/// - The selected pack was added, updated or moved (a `Removed` + `Added` pair) → reload.
/// - The playing pack went away → reload (fallback), except when it is the selected pack and
///   the same location is now `Invalid` (it broke while being edited) → keep the old bank.
/// - A fallback pack changed, or packs appeared, broke or were fixed or deleted while a
///   fallback or the built-in click plays (something better may be available, and
///   `activePackError` may need to say something else) → reload.
/// - The broken selected pack whose old bank plays was deleted (`InvalidCleared` with no
///   `Added`) → reload.
pub fn react(events: &[RegistryEvent], view: &ActiveView) -> Reaction {
    use RegistryEvent::{Added, Invalid, InvalidCleared, Removed, Updated};
    let selected = view.selected;
    if events.iter().any(|e| matches!(e, Added(i) | Updated(i) if i.id == selected)) {
        return Reaction::Reload;
    }
    match view.playing {
        Some((id, location)) => {
            let gone = events
                .iter()
                .any(|e| matches!(e, Removed { id: r, location: l } if r == id && l == location));
            if gone {
                let broke =
                    events.iter().any(|e| matches!(e, Invalid(err) if err.pack == location));
                if id == selected && broke {
                    return Reaction::KeepOldBank { location: location.to_path_buf() };
                }
                return Reaction::Reload;
            }
            let on_fallback = id != selected;
            if on_fallback
                && events.iter().any(|e| match e {
                    Added(_) | Invalid(_) | InvalidCleared { .. } => true,
                    Updated(i) => i.id == id,
                    Removed { .. } => false,
                })
            {
                return Reaction::Reload;
            }
        }
        None => {
            if events.iter().any(|e| matches!(e, Added(_) | Invalid(_) | InvalidCleared { .. })) {
                return Reaction::Reload;
            }
        }
    }
    if let Some(stale) = view.stale
        && events.iter().any(|e| matches!(e, InvalidCleared { location } if location == stale))
    {
        return Reaction::Reload;
    }
    Reaction::Keep
}

/// The bundled packs folder: `<resource dir>/packs`, or, in debug builds when that has no
/// packs, the repository's `packs/`.
pub fn bundled_dir(resource_dir: Option<&Path>) -> Option<PathBuf> {
    let bundled = resource_dir.map(|dir| dir.join("packs"));
    if bundled.as_deref().is_some_and(has_entries) {
        return bundled;
    }
    #[cfg(debug_assertions)]
    if let Some(repo) = Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(|r| r.join("packs"))
        && has_entries(&repo)
    {
        return Some(repo);
    }
    bundled
}

fn has_entries(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::PackOrigin;
    use taktak_core::pack::PackOrigin as CoreOrigin;

    fn info(id: &str, name: &str) -> PackInfo {
        PackInfo {
            id: id.into(),
            name: name.into(),
            version: None,
            author: "A".into(),
            license: "CC0-1.0".into(),
            description: None,
            source: None,
            attribution: None,
            location: PathBuf::from(format!("/packs/{id}")),
            origin: CoreOrigin::Bundled,
        }
    }

    fn at(mut pack: PackInfo, location: &str) -> PackInfo {
        pack.location = PathBuf::from(location);
        pack
    }

    fn ids(packs: &[&PackInfo]) -> Vec<String> {
        packs.iter().map(|p| p.id.clone()).collect()
    }

    fn broken(location: &str, message: &str) -> PackError {
        PackError::new(
            location,
            vec![
                Problem::warning("pack.json", "unknown field \"x\""),
                Problem::error("sounds/a.wav", format!("{message}.")),
            ],
        )
    }

    #[test]
    fn candidates_follow_the_fallback_order() {
        let packs = [info("a-board", "A Board"), info("buckling-spring", "Buckling Spring")];
        assert_eq!(ids(&candidates("a-board", &packs)), ["a-board", "buckling-spring"]);
        assert_eq!(ids(&candidates("buckling-spring", &packs)), ["buckling-spring", "a-board"]);
        // A missing pack falls back to the default, then the rest by name.
        assert_eq!(ids(&candidates("gone", &packs)), ["buckling-spring", "a-board"]);
        let no_default = [info("a", "A"), info("b", "B"), info("c", "C"), info("d", "D")];
        assert_eq!(ids(&candidates("gone", &no_default)), ["a", "b", "c"]);
        assert_eq!(ids(&candidates("c", &no_default)), ["c", "a", "b"]);
        assert!(candidates("buckling-spring", &[]).is_empty());
    }

    #[test]
    fn retired_packs_migrate_to_the_default_unless_installed() {
        let packs = [info("buckling-spring", "Buckling Spring"), info("mine", "Mine")];
        for retired in RETIRED_PACK_IDS {
            assert_eq!(migrate_retired(retired, &packs), Some(DEFAULT_PACK_ID), "{retired}");
            assert_eq!(migrate_retired(retired, &[]), Some(DEFAULT_PACK_ID), "{retired}");
            // Installed as a user pack: the user's choice stands.
            let user = [PackInfo { origin: CoreOrigin::User, ..info(retired, "Mine") }];
            assert_eq!(migrate_retired(retired, &user), None, "{retired}");
        }
        // Other missing packs keep their selection (and their "not installed" message).
        assert_eq!(migrate_retired("gone", &packs), None);
        assert_eq!(migrate_retired("mine", &packs), None);
        assert_eq!(migrate_retired(DEFAULT_PACK_ID, &packs), None);
    }

    #[test]
    fn active_pack_error_explains_what_plays() {
        let spring = info("buckling-spring", "Buckling Spring");
        let mine = info("my-board", "My Board");
        let packs = [spring.clone(), mine.clone()];
        assert_eq!(active_pack_error("my-board", &packs, Some(&mine), &[], None), None);

        let failures = [(mine.clone(), broken("/packs/my-board", "unsupported format"))];
        assert_eq!(
            active_pack_error("my-board", &packs, Some(&spring), &failures, None).unwrap(),
            "My Board could not be loaded: sounds/a.wav: unsupported format. Playing Buckling Spring \
             instead."
        );
        assert_eq!(
            active_pack_error("my-board", &packs, None, &failures, None).unwrap(),
            "My Board could not be loaded: sounds/a.wav: unsupported format. Playing the \
             built-in click instead."
        );
        assert_eq!(
            active_pack_error("gone\n", &packs, Some(&spring), &[], None).unwrap(),
            "The pack “gone\\n” is not installed. Playing Buckling Spring instead."
        );
        assert_eq!(
            active_pack_error("buckling-spring", &[], None, &[], None).unwrap(),
            "No sound packs found. Playing the built-in click instead."
        );
        // Broken on disk: listed under the invalid packs, so not "not installed".
        let only_spring = [spring.clone()];
        assert_eq!(
            active_pack_error("my-board", &only_spring, Some(&spring), &[], Some("My Board"))
                .unwrap(),
            "My Board has errors (see the invalid packs). Playing Buckling Spring instead."
        );
        assert_eq!(
            active_pack_error("my-board", &[], None, &[], Some("My Board")).unwrap(),
            "My Board has errors (see the invalid packs). Playing the built-in click instead."
        );
        // A valid pack of that id wins over a broken copy elsewhere.
        assert_eq!(active_pack_error("my-board", &packs, Some(&mine), &[], Some("x")), None);
    }

    #[test]
    fn broken_label_finds_the_selected_pack_among_invalid_ones() {
        let invalid = |location: &str| PackEntry {
            location: PathBuf::from(location),
            origin: CoreOrigin::User,
            status: Err(broken(location, "x")),
        };
        let valid = PackEntry {
            location: PathBuf::from("/u/fine"),
            origin: CoreOrigin::User,
            status: Ok((at(info("fine", "Fine"), "/u/fine"), vec![])),
        };
        let entries = [valid, invalid("/u/a"), invalid("/u/my-board.zip"), invalid("/u/b")];
        // pack.json of /u/b still parses and names the pack.
        let read = |p: &Path| (p == Path::new("/u/b")).then(|| ("mine".into(), "Mine".into()));
        assert_eq!(broken_label("mine", &entries, None, read).as_deref(), Some("Mine"));
        // Unreadable pack.json: matched by folder or zip name.
        assert_eq!(
            broken_label("my-board", &entries, None, read).as_deref(),
            Some("The pack “my-board”")
        );
        // Where it was last seen, whatever pack.json says now.
        let known = Some((Path::new("/u/a"), "Old Name"));
        assert_eq!(broken_label("zzz", &entries, known, read).as_deref(), Some("Old Name"));
        // A parsed manifest with another id is another pack, whatever the folder is called.
        let other = |_: &Path| Some(("else".to_owned(), "Else".to_owned()));
        assert_eq!(broken_label("my-board", &entries, None, other), None);
        assert_eq!(broken_label("fine", &entries, None, read), None, "valid packs never count");
        assert_eq!(broken_label("nothing", &entries, None, read), None);
    }

    #[test]
    fn summaries_carry_warnings_features_and_invalid_packs() {
        let user = PackInfo { origin: CoreOrigin::User, ..info("mine", "Mine") };
        let packs = [info("buckling-spring", "Buckling Spring"), user.clone()];
        let entries = vec![
            PackEntry {
                location: packs[0].location.clone(),
                origin: CoreOrigin::Bundled,
                status: Ok((packs[0].clone(), vec![])),
            },
            PackEntry {
                location: user.location.clone(),
                origin: CoreOrigin::User,
                status: Ok((user.clone(), vec![Problem::warning("license", "personal\x1b[2J")])),
            },
            PackEntry {
                location: PathBuf::from("/user/broken.zip"),
                origin: CoreOrigin::User,
                status: Err(broken("/user/broken.zip", "file not found")),
            },
        ];
        let features =
            |p: &Path| Features { has_release: p.ends_with("buckling-spring"), per_key: false };
        let (summaries, invalid) = summarize(&packs, &entries, features);
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, "buckling-spring");
        assert!(summaries[0].has_release && summaries[0].warnings.is_empty());
        assert_eq!(summaries[1].origin, PackOrigin::User);
        assert!(!summaries[1].has_release);
        assert_eq!(summaries[1].warnings, ["license: personal\\u{1b}[2J"]);
        assert_eq!(invalid.len(), 1);
        assert_eq!(invalid[0].location, "/user/broken.zip");
        assert_eq!(
            invalid[0].problems,
            ["warning: pack.json: unknown field \"x\"", "error: sounds/a.wav: file not found."]
        );
    }

    #[test]
    fn features_come_from_the_manifest() {
        let manifest: Manifest = serde_json::from_value(serde_json::json!({
            "format": 1, "id": "x", "name": "X", "author": "A", "license": "CC0-1.0",
            "groups": { "alphanumeric": { "press": ["a.wav"] } },
        }))
        .unwrap();
        assert_eq!(Features::of(&manifest), Features { has_release: false, per_key: false });
        let manifest: Manifest = serde_json::from_value(serde_json::json!({
            "format": 1, "id": "x", "name": "X", "author": "A", "license": "CC0-1.0",
            "groups": { "alphanumeric": { "press": ["a.wav"] } },
            "keys": { "space": { "press": ["s.wav"], "release": ["r.wav"] } },
        }))
        .unwrap();
        assert_eq!(Features::of(&manifest), Features { has_release: true, per_key: true });
        assert_eq!(read_features(Path::new("/nonexistent/pack")), Features::default());
    }

    fn view<'a>(
        selected: &'a str,
        playing: Option<(&'a str, &'a str)>,
        stale: Option<&'a str>,
    ) -> ActiveView<'a> {
        ActiveView {
            selected,
            playing: playing.map(|(id, location)| (id, Path::new(location))),
            stale: stale.map(Path::new),
        }
    }

    fn removed(id: &str, location: &str) -> RegistryEvent {
        RegistryEvent::Removed { id: id.into(), location: location.into() }
    }

    #[test]
    fn unrelated_events_keep_the_active_pack() {
        let v = view("mine", Some(("mine", "/u/mine")), None);
        let events = [
            RegistryEvent::Added(info("other", "Other")),
            RegistryEvent::Updated(info("buckling-spring", "Buckling Spring")),
            removed("old", "/u/old"),
            RegistryEvent::Invalid(broken("/u/bad", "x")),
            RegistryEvent::InvalidCleared { location: "/u/bad2".into() },
        ];
        assert_eq!(react(&events, &v), Reaction::Keep);
        assert_eq!(react(&[], &v), Reaction::Keep);
    }

    #[test]
    fn selected_pack_changes_reload_it() {
        let v = view("mine", Some(("mine", "/u/mine")), None);
        // Files changed.
        let events = [RegistryEvent::Updated(at(info("mine", "Mine"), "/u/mine"))];
        assert_eq!(react(&events, &v), Reaction::Reload);
        // Moved, or replaced by an override: Removed + Added for the same id.
        let events =
            [removed("mine", "/u/mine"), RegistryEvent::Added(at(info("mine", "M"), "/x"))];
        assert_eq!(react(&events, &v), Reaction::Reload);
        // Came back while a fallback plays.
        let v = view("mine", Some(("buckling-spring", "/b/buckling-spring")), None);
        let events = [RegistryEvent::Added(at(info("mine", "Mine"), "/u/mine"))];
        assert_eq!(react(&events, &v), Reaction::Reload);
    }

    #[test]
    fn a_broken_active_pack_keeps_its_old_bank() {
        let v = view("mine", Some(("mine", "/u/mine")), None);
        let events = [removed("mine", "/u/mine"), RegistryEvent::Invalid(broken("/u/mine", "x"))];
        assert_eq!(
            react(&events, &v),
            Reaction::KeepOldBank { location: PathBuf::from("/u/mine") }
        );
        // While stale: new problems change nothing...
        let stale = view("mine", Some(("mine", "/u/mine")), Some("/u/mine"));
        let events = [RegistryEvent::Invalid(broken("/u/mine", "y"))];
        assert_eq!(react(&events, &stale), Reaction::Keep);
        // ...fixed: it is valid again and reloads...
        let events = [
            RegistryEvent::InvalidCleared { location: "/u/mine".into() },
            RegistryEvent::Added(at(info("mine", "Mine"), "/u/mine")),
        ];
        assert_eq!(react(&events, &stale), Reaction::Reload);
        // ...deleted: fall back.
        let events = [RegistryEvent::InvalidCleared { location: "/u/mine".into() }];
        assert_eq!(react(&events, &stale), Reaction::Reload);
    }

    #[test]
    fn a_removed_active_pack_falls_back() {
        let v = view("mine", Some(("mine", "/u/mine")), None);
        assert_eq!(react(&[removed("mine", "/u/mine")], &v), Reaction::Reload);
        // A broken *other* location does not count as this pack breaking.
        let events = [removed("mine", "/u/mine"), RegistryEvent::Invalid(broken("/u/else", "x"))];
        assert_eq!(react(&events, &v), Reaction::Reload);
        // The fallback that plays went away.
        let v = view("gone", Some(("buckling-spring", "/b/buckling-spring")), None);
        assert_eq!(
            react(&[removed("buckling-spring", "/b/buckling-spring")], &v),
            Reaction::Reload
        );
    }

    #[test]
    fn fallbacks_reconsider_when_packs_change() {
        let v = view("gone", Some(("a-board", "/b/a-board")), None);
        let events = [RegistryEvent::Updated(at(info("a-board", "A"), "/b/a-board"))];
        assert_eq!(react(&events, &v), Reaction::Reload);
        assert_eq!(
            react(&[RegistryEvent::Added(info("buckling-spring", "D"))], &v),
            Reaction::Reload
        );
        // Built-in click: anything new is better.
        let v = view("gone", None, None);
        assert_eq!(react(&[RegistryEvent::Added(info("x", "X"))], &v), Reaction::Reload);
        assert_eq!(react(&[removed("y", "/u/y")], &v), Reaction::Keep);
    }

    #[test]
    fn fallbacks_reload_when_invalid_packs_change() {
        // The broken selected pack was deleted or fixed (or another broke): the message may
        // change from "has errors" to "not installed", or the pack may load again.
        let cleared = RegistryEvent::InvalidCleared { location: "/u/mine".into() };
        let invalid = RegistryEvent::Invalid(broken("/u/mine", "x"));
        for v in [
            view("mine", Some(("buckling-spring", "/b/buckling-spring")), None),
            view("mine", None, None),
        ] {
            assert_eq!(react(std::slice::from_ref(&cleared), &v), Reaction::Reload);
            assert_eq!(react(std::slice::from_ref(&invalid), &v), Reaction::Reload);
        }
        // The selected pack itself plays: problems elsewhere change nothing.
        let v = view("mine", Some(("mine", "/u/mine")), None);
        assert_eq!(react(&[cleared, invalid], &v), Reaction::Keep);
    }

    #[test]
    fn bundled_dir_prefers_the_resource_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("packs/buckling-spring")).unwrap();
        assert_eq!(bundled_dir(Some(dir.path())), Some(dir.path().join("packs")));
        // Debug builds fall back to the repository's packs when the resource dir has none.
        let empty = tempfile::tempdir().unwrap();
        let found = bundled_dir(Some(empty.path())).unwrap();
        assert!(found.join("buckling-spring").join("pack.json").is_file(), "{}", found.display());
    }
}
