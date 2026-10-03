//! Discovers packs in the bundled and user folders, resolves id overrides, and hot-reloads
//! the user folder when it changes.

use super::{PackError, PackInfo, PackOrigin, Problem};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Signature of the function used to inspect a candidate pack. Production code uses
/// `|p, o| load::inspect(p, o, false)`; tests inject fakes.
pub type Inspector =
    Box<dyn Fn(&Path, PackOrigin) -> Result<(PackInfo, Vec<Problem>), PackError> + Send>;

type Inspection = Result<(PackInfo, Vec<Problem>), PackError>;

#[derive(Clone, Debug)]
pub enum RegistryEvent {
    /// A new valid pack appeared (or became valid).
    Added(PackInfo),
    /// A known pack's files changed (content or metadata); reload it if it is active.
    Updated(PackInfo),
    /// A pack disappeared or stopped being valid. `id` is its last known id.
    Removed { id: String, location: PathBuf },
    /// A candidate failed to inspect (also emitted when a previously valid pack breaks,
    /// after `Removed`, and again whenever its problems change). The UI shows these, keyed by
    /// `PackError::pack`.
    Invalid(PackError),
    /// A candidate last reported `Invalid` is no longer invalid: it was deleted, or it is
    /// valid now (an `Added` for it follows). The UI drops the error it shows for `location`.
    InvalidCleared { location: PathBuf },
}

/// One candidate found on disk, valid or not.
#[derive(Clone, Debug)]
pub struct PackEntry {
    pub location: PathBuf,
    pub origin: PackOrigin,
    /// `Ok(info, warnings)` or the reason it is unusable. Losing an id collision is an error.
    pub status: Result<(PackInfo, Vec<Problem>), PackError>,
}

struct Candidate {
    location: PathBuf,
    origin: PackOrigin,
    /// Fingerprint of the files on disk; the inspector only reruns when it changes.
    signature: u64,
    /// The inspector's verdict for `signature`, before id collisions are resolved.
    inspected: Inspection,
    /// `inspected` after collision resolution: what [`PackRegistry::entries`] reports.
    status: Inspection,
    /// Whether [`PackRegistry::packs`] lists it (valid, and neither a duplicate nor
    /// overridden by a user pack).
    visible: bool,
}

impl Candidate {
    fn visible_info(&self) -> Option<&PackInfo> {
        match &self.status {
            Ok((info, _)) if self.visible => Some(info),
            _ => None,
        }
    }
}

pub struct PackRegistry {
    bundled_dir: Option<PathBuf>,
    user_dir: Option<PathBuf>,
    inspector: Inspector,
    /// Result of the last scan, in path order.
    candidates: Vec<Candidate>,
}

impl PackRegistry {
    pub fn new(bundled_dir: Option<PathBuf>, user_dir: Option<PathBuf>) -> PackRegistry {
        PackRegistry::with_inspector(
            bundled_dir,
            user_dir,
            Box::new(|p, o| super::load::inspect(p, o, false)),
        )
    }

    pub fn with_inspector(
        bundled_dir: Option<PathBuf>,
        user_dir: Option<PathBuf>,
        inspector: Inspector,
    ) -> PackRegistry {
        PackRegistry { bundled_dir, user_dir, inspector, candidates: Vec::new() }
    }

    pub fn user_dir(&self) -> Option<&Path> {
        self.user_dir.as_deref()
    }

    /// Full rescan of both folders. Returns what changed since the previous scan
    /// (everything is `Added`/`Invalid` on the first scan).
    ///
    /// Events come in four runs: `Removed`, then `InvalidCleared` (both in previous path
    /// order), then `Added`/`Updated`, then `Invalid` (both in current path order). When the
    /// pack visible under an id moves to another location (an override appears or goes away,
    /// a duplicate takes over), that is `Removed` for the old location followed by `Added` for
    /// the new one. Every `Invalid` is eventually followed by another `Invalid` for the same
    /// location (new problems) or by `InvalidCleared`, so the UI never keeps a stale error.
    pub fn scan(&mut self) -> Vec<RegistryEvent> {
        let mut found = Vec::new();
        if let Some(dir) = &self.bundled_dir {
            discover(dir, PackOrigin::Bundled, &mut found);
        }
        if let Some(dir) = &self.user_dir {
            discover(dir, PackOrigin::User, &mut found);
        }

        let previous: HashMap<&Path, &Candidate> =
            self.candidates.iter().map(|c| (c.location.as_path(), c)).collect();
        let mut current: Vec<Candidate> = found
            .into_iter()
            .map(|(location, origin)| {
                // Fingerprint before inspecting: if files change mid-inspection, the next
                // scan sees a newer fingerprint and inspects again.
                let signature = signature(&location);
                let inspected = match previous.get(location.as_path()) {
                    Some(prev) if prev.signature == signature && prev.origin == origin => {
                        prev.inspected.clone()
                    }
                    _ => (self.inspector)(&location, origin),
                };
                Candidate {
                    status: inspected.clone(),
                    location,
                    origin,
                    signature,
                    inspected,
                    visible: false,
                }
            })
            .collect();

        resolve_ids(&mut current);
        let events = diff(&self.candidates, &current);
        self.candidates = current;
        events
    }

    /// Usable packs after override resolution, sorted by name (case-insensitive) then id.
    pub fn packs(&self) -> Vec<PackInfo> {
        let mut packs: Vec<PackInfo> =
            self.candidates.iter().filter_map(Candidate::visible_info).cloned().collect();
        packs.sort_by_cached_key(|p| (p.name.to_lowercase(), p.id.clone()));
        packs
    }

    /// Every candidate, including invalid ones, in path order (bundled first).
    pub fn entries(&self) -> Vec<PackEntry> {
        self.candidates
            .iter()
            .map(|c| PackEntry {
                location: c.location.clone(),
                origin: c.origin,
                status: c.status.clone(),
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<PackInfo> {
        self.candidates.iter().filter_map(Candidate::visible_info).find(|p| p.id == id).cloned()
    }

    /// Where the user-folder candidates that are symbolic links point, when that is outside
    /// the user folder, with whether each target is a folder. File-system notifications do
    /// not follow links, so [`watch`] watches these as well.
    fn link_targets(&self) -> Vec<(PathBuf, bool)> {
        let Some(user_dir) = &self.user_dir else { return Vec::new() };
        let user_root = fs::canonicalize(user_dir).ok();
        let mut targets: Vec<(PathBuf, bool)> = self
            .candidates
            .iter()
            .filter(|c| c.origin == PackOrigin::User)
            .filter(|c| fs::symlink_metadata(&c.location).is_ok_and(|m| m.file_type().is_symlink()))
            .filter_map(|c| fs::canonicalize(&c.location).ok())
            .filter(|target| !user_root.as_ref().is_some_and(|root| target.starts_with(root)))
            .map(|target| {
                let is_dir = target.is_dir();
                (target, is_dir)
            })
            .collect();
        targets.sort();
        targets.dedup();
        targets
    }
}

fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

/// Appends the pack candidates directly inside `dir`, sorted by file name. A missing folder
/// simply has no packs.
fn discover(dir: &Path, origin: PackOrigin, out: &mut Vec<(PathBuf, PackOrigin)>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return,
        Err(e) => {
            log::warn!("cannot read pack folder {}: {e}", dir.display());
            return;
        }
    };
    let mut found: Vec<(OsString, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if is_hidden(&name) {
            continue;
        }
        let path = entry.path();
        // Follows symlinks, so a linked pack folder or zip counts too.
        let Ok(meta) = fs::metadata(&path) else { continue };
        let is_pack = if meta.is_dir() {
            path.join("pack.json").is_file()
        } else {
            meta.is_file()
                && Path::new(&name).extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        };
        if is_pack {
            found.push((name, path));
        }
    }
    found.sort();
    out.extend(found.into_iter().map(|(_, path)| (path, origin)));
}

/// Changes whenever a non-hidden file under the candidate is added, removed, renamed,
/// resized or touched. Metadata only, so it is cheap enough to recompute on every scan.
fn signature(location: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    match fs::metadata(location) {
        Err(_) => hasher.write_u8(0),
        Ok(meta) if !meta.is_dir() => {
            hasher.write_u8(1);
            (meta.len(), meta.modified().ok()).hash(&mut hasher);
        }
        Ok(_) => {
            hasher.write_u8(2);
            let mut files = Vec::new();
            let mut pending = vec![PathBuf::new()];
            while let Some(rel) = pending.pop() {
                let Ok(dir) = fs::read_dir(location.join(&rel)) else { continue };
                for entry in dir.flatten() {
                    let name = entry.file_name();
                    if is_hidden(&name) {
                        continue;
                    }
                    let rel = rel.join(&name);
                    let Ok(file_type) = entry.file_type() else { continue };
                    if file_type.is_dir() {
                        pending.push(rel);
                    } else if let Ok(meta) = fs::metadata(entry.path()) {
                        // Symlinks are fingerprinted by their target but never descended
                        // into, so link cycles cannot hang a scan.
                        files.push((rel, meta.len(), meta.modified().ok()));
                    }
                }
            }
            files.sort();
            files.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Turns id collisions into statuses. Path order decides within a folder; a valid user pack
/// hides a bundled pack with the same id.
fn resolve_ids(candidates: &mut [Candidate]) {
    let mut bundled: HashMap<String, usize> = HashMap::new();
    let mut user: HashMap<String, usize> = HashMap::new();
    for i in 0..candidates.len() {
        let Ok((info, warnings)) = &candidates[i].status else { continue };
        let id = info.id.clone();
        let origin = candidates[i].origin;
        let winners = match origin {
            PackOrigin::Bundled => &mut bundled,
            PackOrigin::User => &mut user,
        };
        if let Some(&winner) = winners.get(&id) {
            let mut problems = vec![Problem::error(
                "id",
                format!(
                    "duplicate id \"{id}\": already used by {}",
                    candidates[winner].location.display()
                ),
            )];
            problems.extend(warnings.iter().cloned());
            candidates[i].status = Err(PackError::new(&candidates[i].location, problems));
            continue;
        }
        winners.insert(id.clone(), i);
        candidates[i].visible = true;
        if origin == PackOrigin::User
            && let Some(&hidden) = bundled.get(&id)
        {
            candidates[hidden].visible = false;
            if let Ok((_, warnings)) = &mut candidates[i].status {
                warnings.push(Problem::warning("id", format!("overrides bundled pack \"{id}\"")));
            }
        }
    }
}

/// Events that turn the `old` scan result into the `new` one; see [`PackRegistry::scan`].
fn diff(old: &[Candidate], new: &[Candidate]) -> Vec<RegistryEvent> {
    fn visible_by_id(candidates: &[Candidate]) -> HashMap<&str, usize> {
        candidates
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.visible_info().map(|info| (info.id.as_str(), i)))
            .collect()
    }
    let old_visible = visible_by_id(old);
    let new_visible = visible_by_id(new);
    let mut events = Vec::new();

    for c in old {
        let Some(info) = c.visible_info() else { continue };
        let still_visible =
            new_visible.get(info.id.as_str()).is_some_and(|&n| new[n].location == c.location);
        if !still_visible {
            events
                .push(RegistryEvent::Removed { id: info.id.clone(), location: c.location.clone() });
        }
    }

    let new_by_location: HashMap<&Path, &Candidate> =
        new.iter().map(|c| (c.location.as_path(), c)).collect();
    for c in old {
        let still_invalid =
            new_by_location.get(c.location.as_path()).is_some_and(|n| n.status.is_err());
        if c.status.is_err() && !still_invalid {
            events.push(RegistryEvent::InvalidCleared { location: c.location.clone() });
        }
    }

    for c in new {
        let Some(info) = c.visible_info() else { continue };
        match old_visible.get(info.id.as_str()).map(|&o| &old[o]) {
            Some(o) if o.location == c.location => {
                if o.signature != c.signature || o.visible_info() != Some(info) {
                    events.push(RegistryEvent::Updated(info.clone()));
                }
            }
            _ => events.push(RegistryEvent::Added(info.clone())),
        }
    }

    let old_by_location: HashMap<&Path, &Candidate> =
        old.iter().map(|c| (c.location.as_path(), c)).collect();
    for c in new {
        let Err(err) = &c.status else { continue };
        let reported = old_by_location.get(c.location.as_path()).is_some_and(|o| {
            o.signature == c.signature
                && matches!(&o.status, Err(old_err) if old_err.problems == err.problems)
        });
        if !reported {
            events.push(RegistryEvent::Invalid(err.clone()));
        }
    }
    events
}

/// `<platform data dir>/tech.taktak.app/packs`, matching Tauri's app data dir.
pub fn default_user_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("tech.taktak.app").join("packs"))
}

/// How long the folder must be quiet before a rescan.
const DEBOUNCE_QUIET: Duration = Duration::from_millis(300);
/// Upper bound on the wait during continuous writes, so a long copy still shows progress.
const DEBOUNCE_MAX: Duration = Duration::from_secs(2);

enum WatchMsg {
    Changed,
    Stop,
}

/// Keeps the user folder watched; dropping it stops watching.
pub struct Watcher {
    stop: Sender<WatchMsg>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Watcher {
    /// Stops the debounce thread, which ends filesystem notifications as it exits. Waits for a
    /// rescan or `on_events` call in progress, so do not drop it while holding the registry
    /// lock.
    fn drop(&mut self) {
        let _ = self.stop.send(WatchMsg::Stop);
        if let Some(thread) = self.thread.take()
            && thread.thread().id() != thread::current().id()
        {
            let _ = thread.join();
        }
    }
}

/// The filesystem watch: the user folder, plus the targets of linked packs (see
/// [`PackRegistry::link_targets`]), which change as packs are linked and unlinked.
struct FsWatch {
    notify: notify::RecommendedWatcher,
    links: HashMap<PathBuf, bool>,
}

impl FsWatch {
    /// Watches the targets that are new and stops watching the ones that are gone.
    fn sync_links(&mut self, targets: Vec<(PathBuf, bool)>) {
        use notify::Watcher as _;
        let wanted: HashMap<PathBuf, bool> = targets.into_iter().collect();
        self.links.retain(|target, is_dir| {
            let keep = wanted.get(target) == Some(&*is_dir);
            if !keep {
                let _ = self.notify.unwatch(target);
            }
            keep
        });
        for (target, is_dir) in wanted {
            if self.links.contains_key(&target) {
                continue;
            }
            let mode = if is_dir {
                notify::RecursiveMode::Recursive
            } else {
                notify::RecursiveMode::NonRecursive
            };
            match self.notify.watch(&target, mode) {
                Ok(()) => {
                    self.links.insert(target, is_dir);
                }
                Err(e) => log::warn!("cannot watch linked pack {}: {e}", target.display()),
            }
        }
    }
}

/// Watches the registry's user folder (creating it if missing), and where linked packs in it
/// point. Filesystem events are debounced (~300 ms of quiet), the registry is rescanned under
/// the lock, and non-empty event lists are passed to `on_events` on the watcher's thread.
///
/// `on_events` runs after the lock is released, so it may lock the registry itself.
pub fn watch(
    registry: Arc<Mutex<PackRegistry>>,
    on_events: impl FnMut(Vec<RegistryEvent>) + Send + 'static,
) -> Result<Watcher, String> {
    use notify::Watcher as _;

    let (dir, targets) = {
        let registry = registry.lock().unwrap_or_else(PoisonError::into_inner);
        let dir = registry.user_dir().map(Path::to_path_buf);
        (dir.ok_or("no user pack folder is configured")?, registry.link_targets())
    };
    fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create pack folder {}: {e}", dir.display()))?;

    let (tx, rx) = mpsc::channel();
    let fs_tx = tx.clone();
    let mut notify_watcher =
        notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Err(e) = &res {
                log::warn!("pack folder watch error: {e}");
            }
            if needs_rescan(&res) {
                let _ = fs_tx.send(WatchMsg::Changed);
            }
        })
        .map_err(|e| format!("cannot watch pack folder {}: {e}", dir.display()))?;
    notify_watcher
        .watch(&dir, notify::RecursiveMode::Recursive)
        .map_err(|e| format!("cannot watch pack folder {}: {e}", dir.display()))?;
    let mut fs_watch = FsWatch { notify: notify_watcher, links: HashMap::new() };
    fs_watch.sync_links(targets);

    let thread = thread::Builder::new()
        .name("taktak-packs-watch".into())
        .spawn(move || debounce_and_scan(rx, registry, fs_watch, on_events))
        .map_err(|e| format!("cannot start the pack watcher thread: {e}"))?;
    Ok(Watcher { stop: tx, thread: Some(thread) })
}

/// Opening and reading files changes nothing, and inotify reports our own scans' reads, so
/// reacting to access events would make every scan trigger the next one. Errors do rescan:
/// they may mean events were lost.
fn needs_rescan(res: &notify::Result<notify::Event>) -> bool {
    !matches!(res, Ok(event) if event.kind.is_access())
}

/// Owns `fs_watch`, so notifications end when this returns.
fn debounce_and_scan(
    rx: Receiver<WatchMsg>,
    registry: Arc<Mutex<PackRegistry>>,
    mut fs_watch: FsWatch,
    mut on_events: impl FnMut(Vec<RegistryEvent>),
) {
    // Blocking here keeps an idle watcher at zero CPU.
    while let Ok(WatchMsg::Changed) = rx.recv() {
        let first = Instant::now();
        loop {
            let left = DEBOUNCE_MAX.saturating_sub(first.elapsed());
            if left.is_zero() {
                break;
            }
            match rx.recv_timeout(DEBOUNCE_QUIET.min(left)) {
                Ok(WatchMsg::Changed) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Ok(WatchMsg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        let (events, targets) = {
            let mut registry = registry.lock().unwrap_or_else(PoisonError::into_inner);
            let events = registry.scan();
            (events, registry.link_targets())
        };
        fs_watch.sync_links(targets);
        if !events.is_empty() {
            on_events(events);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::Severity;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    /// Reads `{"id": …, "name": …}` from `pack.json`, or from the file itself for zips, so
    /// registry behaviour can be tested without real manifests or audio.
    fn fake_inspect(path: &Path, origin: PackOrigin) -> Inspection {
        let fail = |msg: String| PackError::single(path, Problem::error("pack.json", msg));
        let manifest = if path.is_dir() { path.join("pack.json") } else { path.to_path_buf() };
        let text = fs::read_to_string(manifest).map_err(|e| fail(e.to_string()))?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| fail(format!("invalid JSON: {e}")))?;
        let id = json["id"].as_str().ok_or_else(|| fail("missing id".into()))?;
        let name = json["name"].as_str().unwrap_or(id);
        let info = PackInfo {
            id: id.into(),
            name: name.into(),
            version: json["version"].as_str().map(Into::into),
            author: "Test".into(),
            license: "CC0-1.0".into(),
            description: None,
            source: None,
            attribution: None,
            location: path.to_path_buf(),
            origin,
        };
        Ok((info, Vec::new()))
    }

    fn counting_inspector() -> (Inspector, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let inspector: Inspector = Box::new(move |p, o| {
            counter.fetch_add(1, Ordering::SeqCst);
            fake_inspect(p, o)
        });
        (inspector, calls)
    }

    struct Fixture {
        _tmp: TempDir,
        bundled: PathBuf,
        user: PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            let tmp = tempfile::tempdir().unwrap();
            let bundled = tmp.path().join("bundled");
            let user = tmp.path().join("user");
            fs::create_dir_all(&bundled).unwrap();
            fs::create_dir_all(&user).unwrap();
            Fixture { _tmp: tmp, bundled, user }
        }

        fn registry(&self) -> PackRegistry {
            PackRegistry::with_inspector(
                Some(self.bundled.clone()),
                Some(self.user.clone()),
                Box::new(fake_inspect),
            )
        }
    }

    fn manifest(id: &str, name: &str) -> String {
        format!(r#"{{"id": "{id}", "name": "{name}"}}"#)
    }

    fn write_pack(dir: &Path, folder: &str, id: &str, name: &str) -> PathBuf {
        let root = dir.join(folder);
        fs::create_dir_all(root.join("sounds")).unwrap();
        fs::write(root.join("pack.json"), manifest(id, name)).unwrap();
        fs::write(root.join("sounds/a.wav"), b"RIFF").unwrap();
        root
    }

    fn write_broken(dir: &Path, folder: &str) -> PathBuf {
        let root = dir.join(folder);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("pack.json"), "{ not json").unwrap();
        root
    }

    /// Compact, comparable form of an event list.
    fn summary(events: &[RegistryEvent]) -> Vec<String> {
        let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
        events
            .iter()
            .map(|e| match e {
                RegistryEvent::Added(i) => format!("added {} {}", i.id, name(&i.location)),
                RegistryEvent::Updated(i) => format!("updated {} {}", i.id, name(&i.location)),
                RegistryEvent::Removed { id, location } => {
                    format!("removed {id} {}", name(location))
                }
                RegistryEvent::Invalid(err) => format!("invalid {}", name(&err.pack)),
                RegistryEvent::InvalidCleared { location } => format!("cleared {}", name(location)),
            })
            .collect()
    }

    fn ids(packs: &[PackInfo]) -> Vec<&str> {
        packs.iter().map(|p| p.id.as_str()).collect()
    }

    fn entry_names(reg: &PackRegistry) -> Vec<String> {
        reg.entries()
            .iter()
            .map(|e| e.location.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn first_scan_reports_valid_and_broken_packs() {
        let fx = Fixture::new();
        write_pack(&fx.bundled, "deep-thock", "deep-thock", "Deep Thock");
        write_pack(&fx.user, "mine", "mine", "Mine");
        write_broken(&fx.user, "broken");
        let mut reg = fx.registry();
        assert!(reg.packs().is_empty() && reg.entries().is_empty());

        let events = reg.scan();
        assert_eq!(
            summary(&events),
            ["added deep-thock deep-thock", "added mine mine", "invalid broken"]
        );
        let RegistryEvent::Invalid(err) = &events[2] else { unreachable!() };
        assert!(err.errors().any(|p| p.message.contains("invalid JSON")));
        assert_eq!(ids(&reg.packs()), ["deep-thock", "mine"]);
        assert_eq!(reg.get("mine").unwrap().origin, PackOrigin::User);
        assert_eq!(reg.get("deep-thock").unwrap().origin, PackOrigin::Bundled);
        assert!(reg.get("broken").is_none());
        assert_eq!(reg.entries().len(), 3);

        assert!(reg.scan().is_empty(), "nothing changed");
    }

    #[test]
    fn add_modify_and_remove_between_scans() {
        let fx = Fixture::new();
        let a = write_pack(&fx.user, "a", "a", "A");
        let mut reg = fx.registry();
        reg.scan();

        let b = write_pack(&fx.user, "b", "b", "B");
        assert_eq!(summary(&reg.scan()), ["added b b"]);

        // A new sound file changes the content but not the metadata.
        fs::write(a.join("sounds/extra.wav"), b"RIFF....").unwrap();
        assert_eq!(summary(&reg.scan()), ["updated a a"]);

        fs::write(b.join("pack.json"), manifest("b", "Bee, renamed")).unwrap();
        let events = reg.scan();
        assert_eq!(summary(&events), ["updated b b"]);
        let RegistryEvent::Updated(info) = &events[0] else { unreachable!() };
        assert_eq!(info.name, "Bee, renamed");
        assert_eq!(reg.get("b").unwrap().name, "Bee, renamed");

        fs::remove_dir_all(&a).unwrap();
        let events = reg.scan();
        assert_eq!(summary(&events), ["removed a a"]);
        let RegistryEvent::Removed { location, .. } = &events[0] else { unreachable!() };
        assert_eq!(location, &a);
        assert_eq!(ids(&reg.packs()), ["b"]);
        assert!(reg.scan().is_empty());
    }

    #[test]
    fn changing_the_id_removes_the_old_id_and_adds_the_new_one() {
        let fx = Fixture::new();
        let a = write_pack(&fx.user, "a", "old-id", "A");
        let mut reg = fx.registry();
        reg.scan();
        fs::write(a.join("pack.json"), manifest("new-id", "A, renamed")).unwrap();
        assert_eq!(summary(&reg.scan()), ["removed old-id a", "added new-id a"]);
        assert_eq!(ids(&reg.packs()), ["new-id"]);
    }

    #[test]
    fn pack_that_breaks_is_removed_then_invalid_and_comes_back_when_fixed() {
        let fx = Fixture::new();
        let a = write_pack(&fx.user, "a", "a", "A");
        let mut reg = fx.registry();
        reg.scan();

        fs::write(a.join("pack.json"), "{ \"id\": ").unwrap();
        assert_eq!(summary(&reg.scan()), ["removed a a", "invalid a"]);
        assert!(reg.packs().is_empty());
        assert!(reg.entries()[0].status.is_err());
        assert!(reg.scan().is_empty(), "an unchanged broken pack is reported once");

        // Still broken, but changed: reported again so the UI shows the latest problems.
        fs::write(a.join("pack.json"), "{ \"id\": 1 ").unwrap();
        assert_eq!(summary(&reg.scan()), ["invalid a"]);

        fs::write(a.join("pack.json"), manifest("a", "A")).unwrap();
        assert_eq!(summary(&reg.scan()), ["cleared a", "added a a"]);
        assert!(reg.entries()[0].status.is_ok());
    }

    #[test]
    fn deleting_an_invalid_pack_clears_its_error() {
        let fx = Fixture::new();
        write_pack(&fx.user, "a-first", "same", "First");
        let loser = write_pack(&fx.user, "b-second", "same", "Second");
        let broken = write_broken(&fx.user, "broken");
        let mut reg = fx.registry();
        assert_eq!(
            summary(&reg.scan()),
            ["added same a-first", "invalid b-second", "invalid broken"]
        );

        fs::remove_dir_all(&broken).unwrap();
        let events = reg.scan();
        assert_eq!(summary(&events), ["cleared broken"]);
        let RegistryEvent::InvalidCleared { location } = &events[0] else { unreachable!() };
        assert_eq!(location, &broken);
        assert_eq!(entry_names(&reg), ["a-first", "b-second"]);

        // A duplicate that loses its collision is invalid too; deleting it clears that.
        fs::remove_dir_all(&loser).unwrap();
        assert_eq!(summary(&reg.scan()), ["cleared b-second"]);
        assert_eq!(entry_names(&reg), ["a-first"]);
        assert!(reg.scan().is_empty());
    }

    #[test]
    fn watcher_reports_a_deleted_invalid_pack() {
        let fx = Fixture::new();
        let broken = write_broken(&fx.user, "broken");
        let reg = Arc::new(Mutex::new(fx.registry()));
        assert_eq!(summary(&reg.lock().unwrap().scan()), ["invalid broken"]);
        let (tx, rx) = mpsc::channel();
        let _watcher = watch(Arc::clone(&reg), move |events| {
            let _ = tx.send(events);
        })
        .unwrap();
        fs::remove_dir_all(&broken).unwrap();
        wait_for(
            &rx,
            "InvalidCleared",
            |e| matches!(e, RegistryEvent::InvalidCleared { location } if location == &broken),
        );
        assert!(reg.lock().unwrap().entries().is_empty());
    }

    #[test]
    fn zips_count_and_other_entries_are_ignored() {
        let fx = Fixture::new();
        fs::write(fx.user.join("zipped.zip"), manifest("zipped", "Zipped")).unwrap();
        fs::write(fx.user.join("SHOUTY.ZIP"), manifest("shouty", "Shouty")).unwrap();
        write_pack(&fx.user, "folder", "folder", "Folder");
        // Ignored: hidden entries, folders without pack.json, other files, a zip-named folder
        // without pack.json, and pack.json directly in the packs folder.
        write_pack(&fx.user, ".hidden", "hidden", "Hidden");
        fs::write(fx.user.join(".hidden.zip"), manifest("hidden-zip", "Hidden zip")).unwrap();
        fs::create_dir_all(fx.user.join("no-manifest/sounds")).unwrap();
        fs::write(fx.user.join("no-manifest/sounds/a.wav"), b"RIFF").unwrap();
        fs::create_dir_all(fx.user.join("folder.zip")).unwrap();
        fs::write(fx.user.join("notes.txt"), "hello").unwrap();
        fs::write(fx.user.join("pack.json"), manifest("loose", "Loose")).unwrap();
        fs::write(fx.user.join(".DS_Store"), "junk").unwrap();

        let mut reg = fx.registry();
        let events = reg.scan();
        assert_eq!(
            summary(&events),
            ["added shouty SHOUTY.ZIP", "added folder folder", "added zipped zipped.zip"]
        );
        assert_eq!(entry_names(&reg), ["SHOUTY.ZIP", "folder", "zipped.zip"]);

        // Touching ignored entries changes nothing.
        fs::write(fx.user.join("notes.txt"), "hello again").unwrap();
        fs::write(fx.user.join(".DS_Store"), "more junk").unwrap();
        assert!(reg.scan().is_empty());

        fs::write(fx.user.join("zipped.zip"), manifest("zipped", "Zipped v2")).unwrap();
        assert_eq!(summary(&reg.scan()), ["updated zipped zipped.zip"]);
        fs::remove_file(fx.user.join("zipped.zip")).unwrap();
        assert_eq!(summary(&reg.scan()), ["removed zipped zipped.zip"]);
    }

    #[test]
    fn hidden_files_inside_a_pack_do_not_count_as_changes() {
        let fx = Fixture::new();
        let a = write_pack(&fx.user, "a", "a", "A");
        let (inspector, calls) = counting_inspector();
        let mut reg = PackRegistry::with_inspector(None, Some(fx.user.clone()), inspector);
        reg.scan();
        fs::write(a.join(".DS_Store"), "junk").unwrap();
        fs::create_dir_all(a.join(".git")).unwrap();
        fs::write(a.join(".git/HEAD"), "ref").unwrap();
        fs::write(a.join("sounds/.a.wav.swp"), "swap").unwrap();
        assert!(reg.scan().is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn user_pack_overrides_bundled_pack_with_a_warning() {
        let fx = Fixture::new();
        let bundled = write_pack(&fx.bundled, "deep-thock", "deep-thock", "Deep Thock");
        let mut reg = fx.registry();
        assert_eq!(summary(&reg.scan()), ["added deep-thock deep-thock"]);

        let user = write_pack(&fx.user, "my-thock", "deep-thock", "My Thock");
        assert_eq!(
            summary(&reg.scan()),
            ["removed deep-thock deep-thock", "added deep-thock my-thock"]
        );
        let packs = reg.packs();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].name, "My Thock");
        assert_eq!(packs[0].origin, PackOrigin::User);
        assert_eq!(reg.get("deep-thock").unwrap().location, user);

        let entries = reg.entries();
        assert_eq!(entries.len(), 2);
        let (bundled_info, bundled_warnings) = entries[0].status.as_ref().unwrap();
        assert_eq!(bundled_info.location, bundled);
        assert!(bundled_warnings.is_empty());
        let (_, user_warnings) = entries[1].status.as_ref().unwrap();
        assert_eq!(user_warnings.len(), 1);
        assert_eq!(user_warnings[0].severity, Severity::Warning);
        assert!(user_warnings[0].message.contains("overrides bundled pack \"deep-thock\""));

        assert!(reg.scan().is_empty(), "override is stable");

        fs::remove_dir_all(&user).unwrap();
        assert_eq!(
            summary(&reg.scan()),
            ["removed deep-thock my-thock", "added deep-thock deep-thock"]
        );
        assert_eq!(reg.get("deep-thock").unwrap().origin, PackOrigin::Bundled);
    }

    #[test]
    fn broken_user_pack_does_not_override() {
        let fx = Fixture::new();
        write_pack(&fx.bundled, "deep-thock", "deep-thock", "Deep Thock");
        write_broken(&fx.user, "deep-thock");
        let mut reg = fx.registry();
        assert_eq!(summary(&reg.scan()), ["added deep-thock deep-thock", "invalid deep-thock"]);
        assert_eq!(reg.get("deep-thock").unwrap().origin, PackOrigin::Bundled);
    }

    #[test]
    fn duplicate_user_ids_keep_the_first_in_path_order() {
        let fx = Fixture::new();
        let first = write_pack(&fx.user, "a-first", "same", "First");
        write_pack(&fx.user, "b-second", "same", "Second");
        let mut reg = fx.registry();
        let events = reg.scan();
        assert_eq!(summary(&events), ["added same a-first", "invalid b-second"]);
        let RegistryEvent::Invalid(err) = &events[1] else { unreachable!() };
        let error = err.errors().next().unwrap();
        assert!(error.message.contains("duplicate id \"same\""), "{error}");
        assert!(error.message.contains(&first.display().to_string()), "{error}");
        assert_eq!(reg.get("same").unwrap().name, "First");
        assert!(reg.entries()[1].status.is_err());
        assert!(reg.scan().is_empty(), "the duplicate error is reported once");

        // The loser takes over once the winner goes away.
        fs::remove_dir_all(&first).unwrap();
        assert_eq!(
            summary(&reg.scan()),
            ["removed same a-first", "cleared b-second", "added same b-second"]
        );
        assert_eq!(reg.get("same").unwrap().name, "Second");
        assert!(reg.entries()[0].status.is_ok());
    }

    #[test]
    fn newly_added_duplicate_is_invalid_and_winner_is_untouched() {
        let fx = Fixture::new();
        write_pack(&fx.user, "b", "same", "B");
        let mut reg = fx.registry();
        reg.scan();
        // "a" sorts first, so it wins and "b" now loses the collision.
        write_pack(&fx.user, "a", "same", "A");
        assert_eq!(summary(&reg.scan()), ["removed same b", "added same a", "invalid b"]);
    }

    #[test]
    fn duplicate_bundled_ids_and_override_of_the_winner() {
        let fx = Fixture::new();
        write_pack(&fx.bundled, "one", "dup", "One");
        write_pack(&fx.bundled, "two", "dup", "Two");
        write_pack(&fx.user, "mine", "dup", "Mine");
        let mut reg = fx.registry();
        assert_eq!(summary(&reg.scan()), ["added dup mine", "invalid two"]);
        let entries = reg.entries();
        assert!(entries[0].status.is_ok() && entries[1].status.is_err());
        assert_eq!(ids(&reg.packs()), ["dup"]);
        assert_eq!(reg.get("dup").unwrap().name, "Mine");
    }

    #[test]
    fn ordering_of_packs_and_entries() {
        let fx = Fixture::new();
        write_pack(&fx.bundled, "z-bundled", "zebra", "zebra");
        write_pack(&fx.bundled, "a-bundled", "alpha", "Alpha");
        write_pack(&fx.user, "c-user", "beta-2", "beta");
        write_pack(&fx.user, "b-user", "beta-1", "Beta");
        write_broken(&fx.user, "a-broken");
        let mut reg = fx.registry();
        reg.scan();
        assert_eq!(
            entry_names(&reg),
            ["a-bundled", "z-bundled", "a-broken", "b-user", "c-user"],
            "bundled first, then user, each sorted by file name"
        );
        let origins: Vec<PackOrigin> = reg.entries().iter().map(|e| e.origin).collect();
        assert_eq!(origins[..2], [PackOrigin::Bundled; 2]);
        assert_eq!(origins[2..], [PackOrigin::User; 3]);
        assert_eq!(
            ids(&reg.packs()),
            ["alpha", "beta-1", "beta-2", "zebra"],
            "by name ignoring case, then id"
        );
    }

    #[test]
    fn inspector_runs_only_for_new_or_changed_candidates() {
        let fx = Fixture::new();
        let a = write_pack(&fx.bundled, "a", "a", "A");
        write_pack(&fx.user, "b", "b", "B");
        write_broken(&fx.user, "c");
        let (inspector, calls) = counting_inspector();
        let mut reg = PackRegistry::with_inspector(
            Some(fx.bundled.clone()),
            Some(fx.user.clone()),
            inspector,
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "construction does not inspect");

        reg.scan();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        reg.scan();
        reg.scan();
        assert_eq!(calls.load(Ordering::SeqCst), 3, "unchanged candidates are cached");

        fs::write(a.join("sounds/a.wav"), b"RIFF and more").unwrap();
        reg.scan();
        assert_eq!(calls.load(Ordering::SeqCst), 4);

        write_pack(&fx.user, "d", "d", "D");
        reg.scan();
        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn missing_or_unset_folders_have_no_packs() {
        let fx = Fixture::new();
        let mut reg = PackRegistry::with_inspector(
            Some(fx.bundled.join("nope")),
            Some(fx.user.join("nope")),
            Box::new(fake_inspect),
        );
        assert!(reg.scan().is_empty());
        let mut reg = PackRegistry::with_inspector(None, None, Box::new(fake_inspect));
        assert!(reg.scan().is_empty() && reg.packs().is_empty() && reg.user_dir().is_none());
    }

    #[test]
    fn new_does_not_inspect_anything() {
        let fx = Fixture::new();
        write_pack(&fx.user, "a", "a", "A");
        let reg = PackRegistry::new(Some(fx.bundled.clone()), Some(fx.user.clone()));
        assert_eq!(reg.user_dir(), Some(fx.user.as_path()));
        assert!(reg.packs().is_empty() && reg.entries().is_empty() && reg.get("a").is_none());
    }

    #[test]
    fn default_user_dir_is_the_tauri_app_data_dir() {
        if let Some(dir) = default_user_dir() {
            assert!(dir.ends_with("tech.taktak.app/packs"), "{}", dir.display());
            assert_eq!(dir.parent().unwrap().parent(), dirs::data_dir().as_deref());
        }
    }

    #[test]
    fn watch_requires_a_user_folder() {
        let reg = PackRegistry::with_inspector(None, None, Box::new(fake_inspect));
        assert!(watch(Arc::new(Mutex::new(reg)), |_| {}).is_err());
    }

    #[test]
    fn reads_do_not_trigger_rescans() {
        use notify::event::{AccessKind, AccessMode, CreateKind, DataChange, ModifyKind};
        use notify::{Event, EventKind};
        let event = |kind| Ok(Event::new(kind));
        assert!(!needs_rescan(&event(EventKind::Access(AccessKind::Open(AccessMode::Read)))));
        assert!(!needs_rescan(&event(EventKind::Access(AccessKind::Close(AccessMode::Write)))));
        assert!(needs_rescan(&event(EventKind::Create(CreateKind::Folder))));
        assert!(needs_rescan(&event(EventKind::Modify(ModifyKind::Data(DataChange::Content)))));
        assert!(needs_rescan(&event(EventKind::Any)));
        assert!(needs_rescan(&Err(notify::Error::generic("events dropped"))));
    }

    /// Receives event batches until one contains an event matching `want`.
    fn wait_for(
        rx: &Receiver<Vec<RegistryEvent>>,
        what: &str,
        want: impl Fn(&RegistryEvent) -> bool,
    ) -> RegistryEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(batch) => {
                    if let Some(event) = batch.into_iter().find(&want) {
                        return event;
                    }
                }
                Err(_) => panic!("no {what} event within 5 s"),
            }
        }
    }

    #[test]
    fn watcher_reports_changes_in_the_user_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("created-by-watch");
        let reg = Arc::new(Mutex::new(PackRegistry::with_inspector(
            None,
            Some(user.clone()),
            Box::new(fake_inspect),
        )));
        assert!(reg.lock().unwrap().scan().is_empty());

        let (tx, rx) = mpsc::channel();
        let watcher = watch(Arc::clone(&reg), move |events| {
            let _ = tx.send(events);
        })
        .unwrap();
        assert!(user.is_dir(), "watch creates the user folder");

        let pack = write_pack(&user, "live", "live", "Live");
        wait_for(&rx, "Added", |e| matches!(e, RegistryEvent::Added(i) if i.id == "live"));
        assert_eq!(ids(&reg.lock().unwrap().packs()), ["live"]);

        fs::write(pack.join("pack.json"), manifest("live", "Live, edited")).unwrap();
        let updated = wait_for(
            &rx,
            "Updated",
            |e| matches!(e, RegistryEvent::Updated(i) if i.name == "Live, edited"),
        );
        let RegistryEvent::Updated(info) = updated else { unreachable!() };
        assert_eq!(info.id, "live");

        fs::remove_dir_all(&pack).unwrap();
        wait_for(
            &rx,
            "Removed",
            |e| matches!(e, RegistryEvent::Removed { id, .. } if id == "live"),
        );
        assert!(reg.lock().unwrap().packs().is_empty());

        let start = Instant::now();
        drop(watcher);
        let took = start.elapsed();
        assert!(took < Duration::from_millis(500), "dropping the watcher took {took:?}");
    }

    #[cfg(unix)]
    #[test]
    fn watcher_follows_linked_packs() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let (user, dev) = (tmp.path().join("user"), tmp.path().join("dev"));
        fs::create_dir_all(&user).unwrap();
        let linked = write_pack(&dev, "linked", "linked", "Linked");
        let zipped = dev.join("zipped.zip");
        fs::write(&zipped, manifest("zipped", "Zipped")).unwrap();
        symlink(&linked, user.join("linked")).unwrap();
        symlink(&zipped, user.join("zipped.zip")).unwrap();
        // A link into the user folder itself needs no watch of its own.
        let plain = write_pack(&user, "plain", "plain", "Plain");
        symlink(&plain, user.join("alias")).unwrap();

        let reg = Arc::new(Mutex::new(PackRegistry::with_inspector(
            None,
            Some(user.clone()),
            Box::new(fake_inspect),
        )));
        reg.lock().unwrap().scan();
        let targets = reg.lock().unwrap().link_targets();
        let canonical = |p: &Path| fs::canonicalize(p).unwrap();
        assert_eq!(targets, [(canonical(&linked), true), (canonical(&zipped), false)]);

        let (tx, rx) = mpsc::channel();
        let _watcher = watch(Arc::clone(&reg), move |events| {
            let _ = tx.send(events);
        })
        .unwrap();

        // Edits in the link targets, which are outside the watched user folder.
        fs::write(linked.join("pack.json"), manifest("linked", "Linked, edited")).unwrap();
        wait_for(
            &rx,
            "Updated",
            |e| matches!(e, RegistryEvent::Updated(i) if i.name == "Linked, edited"),
        );
        fs::write(&zipped, manifest("zipped", "Zipped, edited")).unwrap();
        wait_for(
            &rx,
            "Updated",
            |e| matches!(e, RegistryEvent::Updated(i) if i.name == "Zipped, edited"),
        );

        // A link made while watching is followed too.
        let later = write_pack(&dev, "later", "later", "Later");
        symlink(&later, user.join("later")).unwrap();
        wait_for(&rx, "Added", |e| matches!(e, RegistryEvent::Added(i) if i.id == "later"));
        fs::write(later.join("pack.json"), manifest("later", "Later, edited")).unwrap();
        wait_for(
            &rx,
            "Updated",
            |e| matches!(e, RegistryEvent::Updated(i) if i.name == "Later, edited"),
        );
    }

    #[test]
    fn dropping_an_idle_watcher_is_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = PackRegistry::with_inspector(
            None,
            Some(tmp.path().to_path_buf()),
            Box::new(fake_inspect),
        );
        let watcher = watch(Arc::new(Mutex::new(reg)), |_| {}).unwrap();
        // Mid-debounce, too: a pending change must not delay shutdown.
        write_pack(tmp.path(), "a", "a", "A");
        thread::sleep(Duration::from_millis(50));
        let start = Instant::now();
        drop(watcher);
        let took = start.elapsed();
        assert!(took < Duration::from_millis(500), "dropping the watcher took {took:?}");
    }
}
