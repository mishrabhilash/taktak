//! The app's state and the control thread that drives `taktak-core`.
//!
//! - [`Shared`] holds the [`AppState`] behind a mutex that is only ever held for a moment
//!   (never across I/O, decoding or device work). Commands, the tray menu, the hotkey and the
//!   macOS observers (frontmost app, screen lock, session) change it with [`Shared::update`],
//!   which recomputes the derived fields ([`derive`]: `autoMute`, `ruleBlocked`, `playing`,
//!   `onboarding.offer`) and the input gate, queues a settings save, wakes the control thread
//!   and broadcasts the new state.
//! - The control thread (`taktak-control`) owns the [`Engine`] (`Send`, not `Sync`), the key
//!   listener, the pack registry and its watcher.
//! - The output stream and the key listener run only while a key press can make a sound
//!   (sounds on, not muted, not auto-muted, not rule-blocked for [`rules::CLOSE_AFTER`] or
//!   longer, Input Monitoring granted: [`keys_can_sound`]) or a preview plays.
//!   An open stream keeps the audio device awake, which costs `coreaudiod` 5–9 % of a core even
//!   in silence. While they are closed, the playing pack stays decoded at the default device's
//!   rate, so opening again takes only the ~0.1 s the device needs.
//! - It sleeps until a message arrives or a poll is due: 4 Hz while the engine runs (stream
//!   faults and reroutes → reopen on the new default device, xruns, hook re-enables, latency
//!   samples, garbage), every 2 s for Input Monitoring while it is missing (10 s once granted),
//!   every 2 s for an output device while one is needed and none opens, and every 5 s for the
//!   default device while the output is closed.
//! - Packs are decoded on `taktak-loader` ([`Loader`]); the control thread swaps the result in.
//! - It also watches the default output device ([`DeviceWatch`]) for the `outputChanged`
//!   auto-mute: every device it opens or looks at while the output is closed.
//!
//! Nothing here sees which keys are pressed: the hook forwards events straight to the audio
//! callback, and latency samples are timings only.

use crate::automute::{self, DeviceWatch};
use crate::catalog::{self, ActiveView, Reaction};
use crate::input::{self, Permission as PermissionPoll};
use crate::loader::{BankJob, BankLoaded, Done, Loader, PreviewJob, PreviewLoaded};
use crate::rules::{self, RuleBlock};
use crate::settings::{self, PersistHandle, Persister};
use crate::state::{AppState, AudioState, AudioStatus, LatencyReport, Permission, Settings};
use crate::windows;
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use taktak_core::audio::{
    self as core_audio, DeviceInfo, Engine, EngineConfig, SoundBank, StreamFault, TriggerSender,
};
use taktak_core::input::{InputError, Listener};
use taktak_core::latency::{LatencySample, Report};
use taktak_core::pack::registry::{self, Watcher};
use taktak_core::pack::{self, PackError, PackInfo, PackRegistry, RegistryEvent};

/// Engine poll interval while the engine runs (4 Hz).
const POLL: Duration = Duration::from_millis(250);
/// Wait after a stream fault or reroute before reopening, so the OS can settle on the new
/// default device.
const REBUILD_DELAY: Duration = Duration::from_millis(500);
/// Retry interval while no output device can be opened.
const RETRY: Duration = Duration::from_secs(2);
/// After the key listener failed to start, wait this long before trying again (with a new
/// engine: a failed start uses up the engine's trigger sender).
const HOOK_RETRY: Duration = Duration::from_secs(30);
/// Requested audio buffer, in frames (the core's low-latency default).
const BUFFER_FRAMES: u32 = 64;
/// How long closing the output waits for the audio callback to hand back the bank it plays,
/// so opening again needs no decode.
const STOP_TIMEOUT: Duration = Duration::from_millis(200);
/// The same after a stream fault: a dead stream never answers, though one the backend only
/// invalidated may still play.
const FAULT_STOP_TIMEOUT: Duration = Duration::from_millis(50);
/// While the output is closed, how often to look at the default output device again: the
/// device Settings shows, and the rate packs are decoded at.
const IDLE_REFRESH: Duration = Duration::from_secs(5);
/// A preview keeps the output open this long after its clip ends.
const PREVIEW_TAIL: Duration = Duration::from_millis(500);
/// How long the output stays open for a preview whose clip is still being decoded.
const PREVIEW_DECODE: Duration = Duration::from_secs(10);
/// Preview clips kept decoded, by pack and rate.
const PREVIEW_CACHE: usize = 4;
/// `get_latency` answers once this many presses were measured since the window opened.
const MIN_LATENCY_SAMPLES: usize = 5;
/// Latency samples kept (the most recent).
const MAX_LATENCY_SAMPLES: usize = 1000;
/// How often xruns are logged at most.
const XRUN_LOG_INTERVAL: Duration = Duration::from_secs(10);

/// Why a preview cannot play: no output device opens.
pub const NO_OUTPUT: &str = "No sound output is available right now.";
/// The `audio.message` while no output device can be found.
const NO_OUTPUT_RETRYING: &str = "No audio output is available — retrying…";

/// What the control thread is asked to do.
pub enum Msg {
    /// Settings, `enabled` or `muted` changed: apply the levels, load the selected pack if it
    /// changed, and open or close the output and the listener.
    Sync,
    /// Load the selected pack again even though it is selected already (`set_pack` retry).
    LoadSelected,
    /// Play pack `id`'s preview clip; the outcome goes back on `reply` ([`PreviewWait`]).
    Preview {
        id: String,
        reply: Sender<Result<(), String>>,
    },
    StopPreview,
    /// A batch of hot-reload events from the pack watcher.
    Registry(Vec<RegistryEvent>),
    Loader(Done),
    Shutdown,
}

/// Waits for the control thread to start a preview, or to say why it cannot.
pub struct PreviewWait(Receiver<Result<(), String>>);

impl PreviewWait {
    /// `Ok` once the clip plays, or when it was stopped or replaced by another preview before it
    /// could; still decoding after `timeout` also counts as `Ok` (it plays when ready). `Err`
    /// says why it cannot play, for the user.
    pub fn wait(self, timeout: Duration) -> Result<(), String> {
        match self.0.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                log::debug!("preview still loading after {timeout:?}; it plays when ready");
                Ok(())
            }
            Err(RecvTimeoutError::Disconnected) => Ok(()),
        }
    }
}

/// Whether a key press can make a sound, so the output stream and the key listener should run:
/// sounds may play (on, not muted, not auto-muted, and not blocked by a per-app rule for
/// [`rules::CLOSE_AFTER`] or longer), the platform has a key listener, Input Monitoring is
/// granted, and the listener is not waiting to be retried after it failed to start.
pub fn keys_can_sound(
    gate_open: bool,
    hook_supported: bool,
    granted: bool,
    retry_pending: bool,
) -> bool {
    gate_open && hook_supported && granted && !retry_pending
}

/// The `permission` the UI shows. A listener that failed to start counts as denied until one
/// starts: no sound plays, and allowing TakTak again in System Settings is what may help.
pub fn permission_state(hook_supported: bool, granted: bool, hook_failed: bool) -> Permission {
    if !hook_supported {
        Permission::Unknown
    } else if granted && !hook_failed {
        Permission::Granted
    } else {
        Permission::Denied
    }
}

/// `onboarding.relaunchSuggested`: the platform reports the permission as granted (macOS:
/// Input Monitoring; Windows and Linux X11 have none to grant), but the key listener still could
/// not start (a relaunch usually fixes that). Never while the Linux `input` group is what is
/// missing ([`input_group_needed`]): restarting TakTak would not help there.
pub fn relaunch_suggested(
    hook_supported: bool,
    granted: bool,
    hook_failed: bool,
    input_group: bool,
) -> bool {
    hook_supported && granted && hook_failed && !input_group
}

/// `onboarding.inputGroupNeeded` (M5): on Linux (`linux`), the listener has to read the keyboard
/// devices (Wayland, or `TAKTAK_INPUT=evdev`) and cannot: `has_permission` says they are not
/// readable (`!granted`), or the listener was refused (`refused`: the X11 backend failed and
/// the evdev fallback could not open the devices).
pub fn input_group_needed(linux: bool, hook_supported: bool, granted: bool, refused: bool) -> bool {
    linux && hook_supported && (!granted || refused)
}

/// `onboarding.permissionRequired`: macOS with the key listener on.
pub fn permission_required(listen: bool) -> bool {
    cfg!(target_os = "macos") && listen
}

/// What a failed key listener start means for later attempts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookFailure {
    /// No listener on this platform: never try again.
    Unsupported,
    /// The OS refused it although permission looked granted: count it as denied, retry later.
    Refused,
    /// Anything else (the tap could not be created, its thread did not start): retry later.
    Failed,
}

impl HookFailure {
    pub fn of(error: &InputError) -> HookFailure {
        match error {
            InputError::Unsupported(_) => HookFailure::Unsupported,
            InputError::PermissionDenied => HookFailure::Refused,
            InputError::Platform(_) => HookFailure::Failed,
        }
    }
}

/// Why pack `name`'s preview cannot play: it failed to load (`error`), or it has no sound to
/// preview.
pub fn preview_error(name: &str, error: Option<&PackError>) -> String {
    let name = pack::printable(name);
    match error {
        Some(err) => {
            format!("The preview of {name} could not be loaded: {}.", catalog::first_error(err))
        }
        None => format!("{name} has no sound to preview."),
    }
}

/// The earliest of `times` that is still ahead of `now`. Anything already due was handled
/// just before, or cannot be acted on yet (a hook retry while muted), so waiting for it would
/// only spin.
fn earliest(now: Instant, times: impl IntoIterator<Item = Option<Instant>>) -> Option<Instant> {
    times.into_iter().flatten().filter(|&at| at > now).min()
}

/// Receives every new state with its revision. Called with the state lock held, so calls
/// arrive in revision order; it must return at once (queue the work, do not block).
pub type Notify = Box<dyn Fn(u64, &AppState) + Send + Sync>;
/// Queues a settings save.
pub type Persist = Box<dyn Fn(Settings) + Send + Sync>;

struct Inner {
    state: AppState,
    revision: u64,
}

/// The state every thread reads and changes, and the gate the key hook checks.
pub struct Shared {
    inner: Mutex<Inner>,
    gate: Arc<AtomicBool>,
    /// The gate without the per-app rule: sounds on, not muted, not auto-muted. With
    /// `rule_blocked`, what the control thread needs to open or close the output.
    sounds: AtomicBool,
    /// `AppState::rule_blocked`.
    rule_blocked: AtomicBool,
    /// Set once the control thread has checked Input Monitoring for the first time (or knows it
    /// never will): the onboarding decision at startup waits for it.
    permission_checked: (Mutex<bool>, Condvar),
    latency: Mutex<VecDeque<LatencySample>>,
    /// Whether the output stream is open (diagnostics and the self-test).
    output_open: AtomicBool,
    control: Sender<Msg>,
    persist: Persist,
    notify: Notify,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The gate the key hook checks, for `state` (its derived fields up to date).
pub fn gate_of(state: &AppState) -> bool {
    rules::gate_open(
        state.settings.enabled,
        state.muted,
        state.auto_mute.is_some(),
        state.rule_blocked,
    )
}

/// `playing` as the contract defines it: the gate is open, permission granted, audio ok.
pub fn is_playing(state: &AppState) -> bool {
    gate_of(state) && state.permission == Permission::Granted && state.audio.state == AudioState::Ok
}

/// Recomputes the fields that follow from the others: `autoMute` from its reasons,
/// `frontmostApp` (none where rules are unsupported), `ruleBlocked`, `playing` and
/// `onboarding.offer`.
pub fn derive(state: &mut AppState) {
    state.auto_mute = state.auto_mute_reasons.reason();
    if !state.rules_supported {
        state.frontmost_app = None;
    }
    state.rule_blocked = state.rules_supported
        && rules::blocks(
            &state.settings.app_rule,
            state.frontmost_app.as_ref().map(|a| a.id.as_str()),
        );
    state.playing = is_playing(state);
    state.onboarding.offer = windows::offer_onboarding(
        state.settings.onboarding_done,
        state.onboarding.permission_required,
        state.permission,
    );
}

/// The gate without its per-app rule term (see [`Shared`]).
fn sounds_of(state: &AppState) -> bool {
    rules::gate_open(state.settings.enabled, state.muted, state.auto_mute.is_some(), false)
}

/// The contract's latency report for `samples`, once there are enough of them.
pub fn latency_report(samples: &[LatencySample]) -> Option<LatencyReport> {
    if samples.len() < MIN_LATENCY_SAMPLES {
        return None;
    }
    let r = Report::from_samples(samples);
    Some(LatencyReport {
        count: r.count,
        total_p50_ms: r.total.p50,
        total_p95_ms: r.total.p95,
        total_max_ms: r.total.max,
        input_p50_ms: r.input.p50,
        queue_p50_ms: r.queue.p50,
        output_ms: r.output.p50,
    })
}

impl Shared {
    pub fn new(
        mut state: AppState,
        control: Sender<Msg>,
        persist: Persist,
        notify: Notify,
    ) -> Shared {
        derive(&mut state);
        let gate = gate_of(&state);
        let sounds = sounds_of(&state);
        let rule_blocked = state.rule_blocked;
        Shared {
            inner: Mutex::new(Inner { state, revision: 0 }),
            gate: Arc::new(AtomicBool::new(gate)),
            sounds: AtomicBool::new(sounds),
            rule_blocked: AtomicBool::new(rule_blocked),
            permission_checked: (Mutex::new(false), Condvar::new()),
            latency: Mutex::new(VecDeque::new()),
            output_open: AtomicBool::new(false),
            control,
            persist,
            notify,
        }
    }

    pub fn snapshot(&self) -> AppState {
        lock(&self.inner).state.clone()
    }

    pub fn settings(&self) -> Settings {
        lock(&self.inner).state.settings.clone()
    }

    /// The flag the key hook checks: `enabled && !muted && autoMute === null && !ruleBlocked`.
    pub fn gate(&self) -> &Arc<AtomicBool> {
        &self.gate
    }

    /// The gate without its per-app rule term: sounds on, not muted, not auto-muted.
    pub fn sounds_on(&self) -> bool {
        self.sounds.load(Ordering::Relaxed)
    }

    /// Whether a per-app rule silences the app in front now.
    pub fn rule_blocked(&self) -> bool {
        self.rule_blocked.load(Ordering::Relaxed)
    }

    /// Records that the first Input Monitoring check is done (or will never happen).
    pub fn mark_permission_checked(&self) {
        let (done, changed) = &self.permission_checked;
        *lock(done) = true;
        changed.notify_all();
    }

    /// Waits up to `timeout` for the first Input Monitoring check; `false` on timeout.
    pub fn wait_permission_checked(&self, timeout: Duration) -> bool {
        let (done, changed) = &self.permission_checked;
        let guard = lock(done);
        let (guard, _) = changed
            .wait_timeout_while(guard, timeout, |done| !*done)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }

    /// Applies `change` and returns the new state. See [`Shared::try_update`].
    pub fn update(&self, change: impl FnOnce(&mut AppState)) -> AppState {
        let result: Result<AppState, String> = self.try_update(|s| {
            change(s);
            Ok(())
        });
        result.unwrap_or_else(|_| self.snapshot())
    }

    /// Applies `change`, or nothing if it fails. On success it recomputes the derived fields
    /// ([`derive`]) and the gate; if anything changed it bumps the revision and notifies. If the
    /// settings changed it queues a save, and if they, the gate or either of its parts (sounds,
    /// rule block) changed it tells the control thread (which opens or closes the output).
    pub fn try_update<E>(
        &self,
        change: impl FnOnce(&mut AppState) -> Result<(), E>,
    ) -> Result<AppState, E> {
        self.apply(change, false)
    }

    /// [`Shared::update`] that notifies even when nothing changed: the contract promises a
    /// `state-changed` once a pack load finishes, whatever it changed.
    pub fn announce(&self, change: impl FnOnce(&mut AppState)) -> AppState {
        let result: Result<AppState, String> = self.apply(
            |s| {
                change(s);
                Ok(())
            },
            true,
        );
        result.unwrap_or_else(|_| self.snapshot())
    }

    fn apply<E>(
        &self,
        change: impl FnOnce(&mut AppState) -> Result<(), E>,
        always_notify: bool,
    ) -> Result<AppState, E> {
        let mut inner = lock(&self.inner);
        let before = inner.state.clone();
        if let Err(e) = change(&mut inner.state) {
            inner.state = before;
            return Err(e);
        }
        let state = &mut inner.state;
        derive(state);
        let gate = gate_of(state);
        let sounds = sounds_of(state);
        let blocked = state.rule_blocked;
        let gate_changed = self.gate.swap(gate, Ordering::Relaxed) != gate;
        let sounds_changed = self.sounds.swap(sounds, Ordering::Relaxed) != sounds;
        let blocked_changed = self.rule_blocked.swap(blocked, Ordering::Relaxed) != blocked;
        if inner.state == before && !always_notify {
            return Ok(before);
        }
        let settings_changed = inner.state.settings != before.settings;
        if settings_changed {
            (self.persist)(inner.state.settings.clone());
        }
        if settings_changed || gate_changed || sounds_changed || blocked_changed {
            let _ = self.control.send(Msg::Sync);
        }
        inner.revision += 1;
        (self.notify)(inner.revision, &inner.state);
        Ok(inner.state.clone())
    }

    /// Id of the pack whose sounds play now ([`AppState::playing_pack_id`]); `None` while the
    /// built-in click plays or before the first load finished.
    pub fn now_playing(&self) -> Option<String> {
        lock(&self.inner).state.playing_pack_id.clone()
    }

    /// Whether the output stream is open now. It is closed while nothing can play (see
    /// [`keys_can_sound`]) and no preview plays. For diagnostics and the self-test.
    pub fn output_open(&self) -> bool {
        self.output_open.load(Ordering::Relaxed)
    }

    /// Asks the control thread to do something; never blocks.
    pub fn send(&self, msg: Msg) {
        let _ = self.control.send(msg);
    }

    /// Keystroke-to-sound timings since [`Shared::reset_latency`], if enough were measured.
    pub fn latency(&self) -> Option<LatencyReport> {
        latency_report(lock(&self.latency).make_contiguous())
    }

    /// Forgets the timings so far (the settings window just opened).
    pub fn reset_latency(&self) {
        lock(&self.latency).clear();
    }

    fn push_latency(&self, batch: &[LatencySample]) {
        let mut samples = lock(&self.latency);
        samples.extend(batch.iter().copied());
        let excess = samples.len().saturating_sub(MAX_LATENCY_SAMPLES);
        samples.drain(..excess);
    }
}

/// Where the service finds and keeps things.
pub struct Config {
    pub version: String,
    pub settings: Settings,
    pub settings_path: PathBuf,
    pub bundled_dir: Option<PathBuf>,
    pub user_dir: Option<PathBuf>,
    /// Listen to the keyboard (checking permission, never prompting). Off for the self-test.
    pub listen: bool,
}

/// The running service: shared state, the settings writer and the control thread.
pub struct Service {
    shared: Arc<Shared>,
    persister: Persister,
    control: Mutex<Option<JoinHandle<()>>>,
}

impl std::ops::Deref for Service {
    type Target = Shared;
    fn deref(&self) -> &Shared {
        &self.shared
    }
}

impl Service {
    /// Starts the settings writer, the pack loader and the control thread, which scans packs,
    /// opens the audio output and starts listening (once permitted) on its own.
    pub fn start(config: Config, notify: Notify) -> io::Result<Service> {
        let persister = Persister::start(
            config.settings_path.clone(),
            config.settings.clone(),
            settings::DEBOUNCE,
        )?;
        let persist: PersistHandle = persister.handle();
        let (tx, rx) = mpsc::channel();
        let mut initial = AppState::initial(config.version, config.settings);
        initial.user_packs_dir = config.user_dir.as_ref().map(|d| d.display().to_string());
        initial.onboarding.permission_required = permission_required(config.listen);
        let shared =
            Arc::new(Shared::new(initial, tx.clone(), Box::new(move |s| persist.save(s)), notify));

        let loader_tx = tx.clone();
        let loader = Loader::spawn(move |done| {
            let _ = loader_tx.send(Msg::Loader(done));
        })?;
        let registry = PackRegistry::new(config.bundled_dir, config.user_dir);
        let mut control = Control::new(shared.clone(), rx, tx, registry, loader);
        control.hook_supported = config.listen;
        let thread =
            thread::Builder::new().name("taktak-control".into()).spawn(move || control.run())?;
        Ok(Service { shared, persister, control: Mutex::new(Some(thread)) })
    }

    /// The shared state, for observers that outlive no particular command (macOS notifications).
    pub fn handle(&self) -> Arc<Shared> {
        self.shared.clone()
    }

    /// Selects pack `id` (persisted) and loads it off the main thread; another
    /// `state-changed` follows once it plays. Rejects ids that are not installed.
    pub fn set_pack(&self, id: &str) -> Result<AppState, String> {
        let mut already = false;
        let state = self.try_update(|s| {
            if !s.packs.iter().any(|p| p.id == id) {
                return Err(format!("There is no pack “{}”.", pack::printable(id)));
            }
            already = s.settings.pack_id == id;
            s.settings.pack_id = id.to_owned();
            s.active_pack_error = None;
            Ok(())
        })?;
        // A new id reaches the control thread as a settings change; the same id again means
        // "try again" (it may have failed to load).
        if already {
            self.send(Msg::LoadSelected);
        }
        Ok(state)
    }

    /// Plays pack `id`'s preview clip once, without changing the active pack, opening the output
    /// for it if nothing else keeps it open. Rejects ids that are not installed; the returned
    /// [`PreviewWait`] says whether the clip could play (never wait on it on the main thread).
    pub fn preview(&self, id: &str) -> Result<PreviewWait, String> {
        if !self.snapshot().packs.iter().any(|p| p.id == id) {
            return Err(format!("There is no pack “{}”.", pack::printable(id)));
        }
        let (reply, outcome) = mpsc::channel();
        self.send(Msg::Preview { id: id.to_owned(), reply });
        Ok(PreviewWait(outcome))
    }

    /// Stops the preview clip, if one plays.
    pub fn stop_preview(&self) {
        self.send(Msg::StopPreview);
    }

    /// Saves pending settings and stops the control thread (engine, listener, watcher),
    /// waiting at most `timeout` for each.
    pub fn shutdown(&self, timeout: Duration) {
        self.send(Msg::Shutdown);
        if !self.persister.flush(timeout) {
            log::warn!("settings may not have been saved before quitting");
        }
        let Some(thread) = lock(&self.control).take() else { return };
        let deadline = Instant::now() + timeout;
        while !thread.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if thread.is_finished() {
            let _ = thread.join();
        }
    }
}

impl Drop for Service {
    /// Stops the control thread without waiting (see [`Service::shutdown`] to wait), then the
    /// settings writer saves anything pending as it drops.
    fn drop(&mut self) {
        self.shared.send(Msg::Shutdown);
    }
}

/// The output stream and what depends on it. Fields drop in order: the hook (which holds the
/// trigger sender) stops before the engine does.
struct Audio {
    listener: Option<Listener>,
    /// Until a listener takes it (and with it, for good: a later listener needs a new engine).
    sender: Option<TriggerSender>,
    metrics: Option<rtrb::Consumer<LatencySample>>,
    /// A bank the engine could not take yet (its command ring was full); retried every poll.
    pending_bank: Option<SoundBank>,
    engine: Engine,
}

/// Which pack plays and why.
#[derive(Default)]
struct Active {
    /// The selected id the latest load was requested for.
    requested: Option<String>,
    /// Of the latest load request; results of older ones are dropped.
    generation: u64,
    /// The rate of the latest load while it runs.
    pending_rate: Option<u32>,
    /// The pack the bank comes from; `None` = built-in click or nothing yet.
    playing: Option<PackInfo>,
    /// Set while the selected pack is invalid on disk and its old bank keeps playing.
    stale: Option<PathBuf>,
}

/// The playing pack's bank while the output is closed, and the rate it was decoded at.
struct Parked {
    rate: u32,
    bank: SoundBank,
}

/// `parked`'s bank if it was decoded at `rate`; otherwise it stays parked.
fn unpark(parked: &mut Option<Parked>, rate: u32) -> Option<SoundBank> {
    parked.take_if(|p| p.rate == rate).map(|p| p.bank)
}

/// A preview whose clip is being decoded, and who waits for it.
struct PendingPreview {
    id: String,
    reply: Sender<Result<(), String>>,
}

/// Recently decoded preview clips, most recently used last.
#[derive(Default)]
struct PreviewCache {
    entries: Vec<(String, u32, Box<[f32]>)>,
}

impl PreviewCache {
    fn get(&mut self, id: &str, rate: u32) -> Option<Box<[f32]>> {
        let i = self.entries.iter().position(|(i, r, _)| i == id && *r == rate)?;
        let entry = self.entries.remove(i);
        let clip = entry.2.clone();
        self.entries.push(entry);
        Some(clip)
    }

    fn insert(&mut self, id: String, rate: u32, clip: Box<[f32]>) {
        self.entries.retain(|(i, _, _)| *i != id);
        self.entries.push((id, rate, clip));
        let excess = self.entries.len().saturating_sub(PREVIEW_CACHE);
        self.entries.drain(..excess);
    }

    fn forget(&mut self, id: &str) {
        self.entries.retain(|(i, _, _)| i != id);
    }
}

/// Logs buffer underruns, at most every [`XRUN_LOG_INTERVAL`].
#[derive(Default)]
struct XrunLog {
    pending: u32,
    logged_at: Option<Instant>,
}

impl XrunLog {
    fn add(&mut self, xruns: u32, now: Instant) {
        self.pending = self.pending.saturating_add(xruns);
        if self.pending > 0 && self.logged_at.is_none_or(|t| now - t >= XRUN_LOG_INTERVAL) {
            log::warn!("audio: {} buffer underrun(s)", self.pending);
            self.pending = 0;
            self.logged_at = Some(now);
        }
    }
}

/// The engine's levels and variation from the settings.
fn apply_settings(engine: &Engine, s: &Settings) {
    engine.set_master_gain(settings::master_gain(s.master_volume));
    engine.set_press_gain(settings::unit(s.press_volume) as f32);
    engine.set_release_gain(settings::unit(s.release_volume) as f32);
    engine.set_variant_mode(s.variant_mode.into());
    engine.set_humanize(settings::unit(s.humanize) as f32);
}

/// `audio` for a device that works: the one the engine opened, or while the output is closed,
/// the one it would open.
fn device_status(info: &DeviceInfo) -> AudioStatus {
    AudioStatus {
        device: (!info.name.is_empty()).then(|| info.name.clone()),
        sample_rate: Some(info.sample_rate),
        buffer_frames: info.buffer_frames,
        state: AudioState::Ok,
        message: None,
    }
}

/// `audio` while no output device can be found.
fn no_output_status() -> AudioStatus {
    AudioStatus {
        state: AudioState::Fault,
        message: Some(NO_OUTPUT_RETRYING.to_owned()),
        ..AudioStatus::default()
    }
}

/// The control thread's state; see the module docs.
struct Control {
    shared: Arc<Shared>,
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
    registry: Arc<Mutex<PackRegistry>>,
    watcher: Option<Watcher>,
    loader: Loader,
    audio: Option<Audio>,
    next_poll: Instant,
    /// The rate packs are decoded at: the engine's, or while the output is closed, the default
    /// device's. `None` until an output device was found.
    rate: Option<u32>,
    /// The playing pack's bank while the output is closed.
    parked: Option<Parked>,
    /// Earliest (re)open of the output after a fault or a failed open, while one is needed.
    rebuild_at: Option<Instant>,
    /// When to reopen the running output on the new default device (the system rerouted it).
    reroute_at: Option<Instant>,
    /// While the output is closed: when to look at the default device again.
    idle_refresh_at: Option<Instant>,
    /// The output stays open until then for a preview.
    preview_until: Option<Instant>,
    /// The last look at the default device found none (logged once).
    output_missing: bool,
    permission: PermissionPoll,
    /// `false` once the platform said it has no key listener (not macOS yet), or when the
    /// service was started without listening.
    hook_supported: bool,
    /// Set while a key listener that failed to start waits to be tried again (not before then).
    hook_retry_at: Option<Instant>,
    /// The last listener start was refused for lack of permission (until one starts).
    hook_refused: bool,
    /// How long a per-app rule has silenced the app in front: the output closes once it is
    /// [`rules::CLOSE_AFTER`].
    rule_block: RuleBlock,
    /// The default output device as last seen, for the `outputChanged` auto-mute.
    devices: DeviceWatch,
    active: Active,
    previews: PreviewCache,
    /// The preview the user asked for last, while it is being decoded.
    pending_preview: Option<PendingPreview>,
    xruns: XrunLog,
}

impl Control {
    fn new(
        shared: Arc<Shared>,
        rx: Receiver<Msg>,
        tx: Sender<Msg>,
        registry: PackRegistry,
        loader: Loader,
    ) -> Control {
        Control {
            shared,
            rx,
            tx,
            registry: Arc::new(Mutex::new(registry)),
            watcher: None,
            loader,
            audio: None,
            next_poll: Instant::now(),
            rate: None,
            parked: None,
            rebuild_at: None,
            reroute_at: None,
            idle_refresh_at: None,
            preview_until: None,
            output_missing: false,
            permission: PermissionPoll::new(),
            hook_supported: true,
            hook_retry_at: None,
            hook_refused: false,
            rule_block: RuleBlock::default(),
            devices: DeviceWatch::default(),
            active: Active::default(),
            previews: PreviewCache::default(),
            pending_preview: None,
            xruns: XrunLog::default(),
        }
    }

    fn run(mut self) {
        self.scan();
        self.migrate_retired_selection();
        self.watch();
        if self.hook_supported {
            self.poll_permission(Instant::now());
        }
        self.rule_block.observe(self.shared.rule_blocked(), Instant::now());
        self.reconcile(Instant::now());
        // After the first listener start too: a listener macOS refuses although the permission
        // looks granted also needs the onboarding (its troubleshooting).
        self.shared.mark_permission_checked();
        loop {
            let msg = match self.next_wake(Instant::now()) {
                Some(at) => self.rx.recv_timeout(at.saturating_duration_since(Instant::now())),
                None => self.rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match msg {
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(msg) => self.handle(msg, Instant::now()),
                Err(RecvTimeoutError::Timeout) => {}
            }
            let now = Instant::now();
            self.rule_block.observe(self.shared.rule_blocked(), now);
            self.tick(now);
            self.reconcile(now);
        }
        log::debug!("control thread stopped");
    }

    /// The earliest poll that is due, if any; `None` sleeps until a message arrives.
    fn next_wake(&self, now: Instant) -> Option<Instant> {
        earliest(
            now,
            [
                self.audio.as_ref().map(|_| self.next_poll),
                self.rebuild_at,
                self.reroute_at,
                self.idle_refresh_at,
                self.preview_until,
                self.hook_retry_at,
                self.hook_supported.then(|| self.permission.next_check()),
                // Once, to close the output when a rule block has lasted long enough.
                self.audio.as_ref().and(self.rule_block.settles_at()),
            ],
        )
    }

    fn tick(&mut self, now: Instant) {
        if self.audio.is_some() && now >= self.next_poll {
            self.poll_audio(now);
            self.next_poll = now + POLL;
        }
        if self.reroute_at.is_some_and(|at| now >= at) {
            self.reroute_at = None;
            if self.audio.is_some() {
                self.restart_audio(now);
            }
        }
        if self.hook_supported && self.permission.due(now) {
            self.poll_permission(now);
        }
        if self.idle_refresh_at.is_some_and(|at| now >= at) {
            self.idle_refresh_at = None;
            if self.audio.is_none() && !self.needs_output(now) {
                self.refresh_idle(now);
            }
        }
    }

    fn handle(&mut self, msg: Msg, now: Instant) {
        match msg {
            Msg::Sync => self.sync(),
            Msg::LoadSelected => self.request_bank(),
            Msg::Preview { id, reply } => self.preview(id, reply, now),
            Msg::StopPreview => self.stop_preview(),
            Msg::Registry(events) => self.on_registry(&events),
            Msg::Loader(Done::Bank(done)) => self.on_bank(*done),
            Msg::Loader(Done::Preview(done)) => self.on_preview(done, now),
            Msg::Shutdown => {}
        }
    }

    // --- what runs ----------------------------------------------------------------------------

    /// Whether a key press can make a sound now ([`keys_can_sound`]). A rule block keeps the
    /// output and the listener for [`rules::CLOSE_AFTER`] (the gate silences keys at once).
    fn keys_wanted(&self, now: Instant) -> bool {
        keys_can_sound(
            self.shared.sounds_on() && !self.rule_block.settled(now),
            self.hook_supported,
            self.permission.granted(),
            self.hook_retry_at.is_some_and(|at| now < at),
        )
    }

    /// Whether the output stream should be open: for key sounds or a preview.
    fn needs_output(&self, now: Instant) -> bool {
        self.keys_wanted(now) || self.preview_until.is_some_and(|until| now < until)
    }

    /// Opens or closes the output and the key listener for what is needed now.
    fn reconcile(&mut self, now: Instant) {
        if self.preview_until.is_some_and(|until| now >= until) {
            self.preview_until = None;
        }
        if self.audio.is_none()
            && self.needs_output(now)
            && self.rebuild_at.is_none_or(|at| now >= at)
        {
            self.start_audio(now);
        }
        if self.audio.is_some() {
            if self.keys_wanted(now) {
                self.ensure_listener(now);
            } else if let Some(audio) = &mut self.audio
                && audio.listener.take().is_some()
            {
                log::info!("stopped listening to the keyboard");
            }
        }
        if self.needs_output(now) {
            return;
        }
        self.rebuild_at = None;
        if self.audio.is_some() {
            self.stop_audio(now);
        } else if self.idle_refresh_at.is_none() {
            self.refresh_idle(now);
        }
    }

    // --- packs ------------------------------------------------------------------------------

    fn packs(&self) -> Vec<PackInfo> {
        lock(&self.registry).packs()
    }

    fn scan(&mut self) {
        let started = Instant::now();
        let events = lock(&self.registry).scan();
        for event in &events {
            if let RegistryEvent::Invalid(err) = event {
                log::warn!("invalid pack: {err}");
            }
        }
        self.publish_packs();
        let found = lock(&self.registry).packs().len();
        log::info!(
            "found {found} sound pack(s) in {:.1} ms",
            started.elapsed().as_secs_f64() * 1e3
        );
    }

    /// Moves a saved selection of a pack TakTak no longer bundles to the default pack
    /// ([`catalog::migrate_retired`]), persisted, before the first load: no "not installed"
    /// banner for users whose old default went away.
    fn migrate_retired_selection(&mut self) {
        let selected = self.shared.settings().pack_id;
        if let Some(id) = catalog::migrate_retired(&selected, &self.packs()) {
            log::info!("pack \"{selected}\" is no longer bundled; selecting \"{id}\" instead");
            self.shared.update(|s| s.settings.pack_id = id.to_owned());
        }
    }

    fn watch(&mut self) {
        let tx = self.tx.clone();
        match registry::watch(self.registry.clone(), move |events| {
            let _ = tx.send(Msg::Registry(events));
        }) {
            Ok(watcher) => self.watcher = Some(watcher),
            Err(e) => log::warn!("pack hot reload is off: {e}"),
        }
    }

    /// Rebuilds the UI's pack lists from the registry.
    fn publish_packs(&mut self) {
        let (packs, entries) = {
            let registry = lock(&self.registry);
            (registry.packs(), registry.entries())
        };
        let (summaries, invalid) = catalog::summarize(&packs, &entries, catalog::read_features);
        self.shared.update(|s| {
            s.packs = summaries;
            s.invalid_packs = invalid;
        });
    }

    fn on_registry(&mut self, events: &[RegistryEvent]) {
        for event in events {
            match event {
                RegistryEvent::Added(info) | RegistryEvent::Updated(info) => {
                    self.previews.forget(&info.id);
                }
                RegistryEvent::Removed { id, .. } => self.previews.forget(id),
                RegistryEvent::Invalid(err) => log::warn!("invalid pack: {err}"),
                RegistryEvent::InvalidCleared { .. } => {}
            }
        }
        self.publish_packs();
        let selected = self.shared.settings().pack_id;
        let reaction = catalog::react(
            events,
            &ActiveView {
                selected: &selected,
                playing: self
                    .active
                    .playing
                    .as_ref()
                    .map(|p| (p.id.as_str(), p.location.as_path())),
                stale: self.active.stale.as_deref(),
            },
        );
        match reaction {
            Reaction::Keep => {}
            Reaction::Reload => self.request_bank(),
            Reaction::KeepOldBank { location } => {
                let name = self.active.playing.as_ref().map_or(selected, |p| p.name.clone());
                log::warn!(
                    "{} broke on disk; keeping its last working version",
                    pack::printable(&location.display().to_string())
                );
                self.active.stale = Some(location);
                self.shared.update(|s| s.active_pack_error = Some(catalog::stale_message(&name)));
            }
        }
    }

    /// Asks the loader for the selected pack (or its fallback) at the current rate. Before any
    /// output device was found, only remembers the request; finding one loads it.
    fn request_bank(&mut self) {
        let selected = self.shared.settings().pack_id;
        self.active.requested = Some(selected.clone());
        self.active.stale = None;
        let Some(rate) = self.rate else { return };
        let packs = self.packs();
        let candidates = catalog::candidates(&selected, &packs).into_iter().cloned().collect();
        self.active.generation += 1;
        self.active.pending_rate = Some(rate);
        self.loader.load_bank(BankJob { generation: self.active.generation, rate, candidates });
    }

    fn on_bank(&mut self, done: BankLoaded) {
        if done.generation != self.active.generation {
            return;
        }
        self.active.pending_rate = None;
        if self.rate != Some(done.rate) {
            // Decoded for a device that is gone; the latest request must be served at the
            // current rate.
            self.request_bank();
            return;
        }
        for (info, err) in &done.failures {
            log::warn!("pack \"{}\" failed to load: {err}", pack::printable(&info.id));
        }
        let selected = self.active.requested.clone().unwrap_or_default();
        let packs = self.packs();
        let broken = if packs.iter().any(|p| p.id == selected) {
            None
        } else {
            let entries = lock(&self.registry).entries();
            let known = (self.active.playing.as_ref())
                .filter(|p| p.id == selected)
                .map(|p| (p.location.as_path(), p.name.as_str()));
            catalog::broken_label(&selected, &entries, known, catalog::read_identity)
        };
        let error = catalog::active_pack_error(
            &selected,
            &packs,
            done.playing.as_ref(),
            &done.failures,
            broken.as_deref(),
        );
        if let (Some(info), Some(preview)) = (&done.playing, done.preview) {
            self.previews.insert(info.id.clone(), done.rate, preview);
        }
        log::info!(
            "playing {} at {} Hz (loaded in {:.1} ms)",
            done.playing.as_ref().map_or(catalog::BUILT_IN.into(), |p| pack::printable(&p.name)),
            done.rate,
            done.elapsed.as_secs_f64() * 1e3
        );
        match &mut self.audio {
            Some(audio) => audio.pending_bank = audio.engine.replace_bank(done.bank).err(),
            None => self.parked = Some(Parked { rate: done.rate, bank: done.bank }),
        }
        let playing_id = done.playing.as_ref().map(|p| p.id.clone());
        self.active.playing = done.playing;
        self.shared.announce(|s| {
            s.active_pack_error = error;
            s.playing_pack_id = playing_id;
        });
    }

    /// Plays `id`'s preview, opening the output for it if needed; answers on `reply` once the
    /// clip plays or cannot.
    fn preview(&mut self, id: String, reply: Sender<Result<(), String>>, now: Instant) {
        // A preview still being decoded is replaced (its waiter hears "stopped").
        self.pending_preview = None;
        if self.audio.is_none() {
            self.preview_until = Some(now + PREVIEW_DECODE);
            self.start_audio(now);
        }
        let Some(rate) = self.audio.as_ref().map(|a| a.engine.info().sample_rate) else {
            self.preview_until = None;
            let _ = reply.send(Err(NO_OUTPUT.to_owned()));
            return;
        };
        if let Some(clip) = self.previews.get(&id, rate) {
            let _ = reply.send(self.play_preview(clip, now));
            return;
        }
        let Some(info) = lock(&self.registry).get(&id) else {
            let _ = reply.send(Err(format!("There is no pack “{}”.", pack::printable(&id))));
            return;
        };
        self.preview_until = self.preview_until.max(Some(now + PREVIEW_DECODE));
        self.pending_preview = Some(PendingPreview { id, reply });
        self.loader.load_preview(PreviewJob { info, rate });
    }

    /// Hands `clip` (at the engine's rate) to the engine and keeps the output open until it
    /// has played.
    fn play_preview(&mut self, clip: Box<[f32]>, now: Instant) -> Result<(), String> {
        let Some(audio) = &mut self.audio else { return Err(NO_OUTPUT.to_owned()) };
        let rate = audio.engine.info().sample_rate.max(1);
        let length = Duration::from_secs_f64(clip.len() as f64 / f64::from(rate));
        if audio.engine.preview(clip).is_err() {
            log::warn!("the audio engine is not taking commands; preview dropped");
            return Err("The sound output is busy. Try again in a moment.".to_owned());
        }
        self.preview_until = Some(now + length + PREVIEW_TAIL);
        Ok(())
    }

    fn on_preview(&mut self, done: PreviewLoaded, now: Instant) {
        let pending = self.pending_preview.take_if(|p| p.id == done.id);
        if let Some(clip) = &done.clip {
            self.previews.insert(done.id.clone(), done.rate, clip.clone());
        }
        let Some(PendingPreview { reply, .. }) = pending else { return };
        let playable =
            self.audio.as_ref().is_some_and(|a| a.engine.info().sample_rate == done.rate);
        let result = match done.clip {
            Some(clip) if playable => self.play_preview(clip, now),
            Some(_) => Err("The sound output changed while the preview loaded. Try again.".into()),
            None => {
                let name = lock(&self.registry).get(&done.id).map_or(done.id, |info| info.name);
                Err(preview_error(&name, done.error.as_ref()))
            }
        };
        if result.is_err() {
            self.preview_until = None;
        }
        let _ = reply.send(result);
    }

    fn stop_preview(&mut self) {
        self.pending_preview = None;
        self.preview_until = None;
        if let Some(audio) = &mut self.audio {
            audio.engine.stop_preview();
        }
    }

    /// Applies the levels and loads the selected pack if it changed.
    fn sync(&mut self) {
        let settings = self.shared.settings();
        if let Some(audio) = &self.audio {
            apply_settings(&audio.engine, &settings);
        }
        if self.active.requested.as_deref() != Some(settings.pack_id.as_str()) {
            self.request_bank();
        }
    }

    // --- audio ------------------------------------------------------------------------------

    /// Opens the default output device with the parked bank if it was decoded at the device's
    /// rate (otherwise silence, and the pack is decoded again at that rate).
    fn start_audio(&mut self, now: Instant) {
        self.idle_refresh_at = None;
        let settings = self.shared.settings();
        let config = EngineConfig {
            buffer_frames: Some(BUFFER_FRAMES),
            measure_latency: true,
            variant_mode: settings.variant_mode.into(),
            humanize: settings::unit(settings.humanize) as f32,
        };
        let mut parked = self.parked.take();
        let had_bank = parked.is_some();
        let started = Engine::start(config, |rate| unpark(&mut parked, rate).unwrap_or_default());
        let (mut engine, sender) = match started {
            Ok(started) => started,
            Err(e) => {
                self.parked = parked;
                log::warn!("cannot open the audio output: {e}");
                self.rebuild_at = Some(now + RETRY);
                self.shared.update(|s| s.audio = no_output_status());
                return;
            }
        };
        // A bank still parked was decoded at another rate.
        let reused = had_bank && parked.is_none();
        apply_settings(&engine, &settings);
        let metrics = engine.take_metrics();
        let info = engine.info().clone();
        log::info!(
            "audio output: {} at {} Hz, buffer {}",
            pack::printable(&info.name),
            info.sample_rate,
            info.buffer_frames.map_or("device default".to_owned(), |f| f.to_string())
        );
        self.rate = Some(info.sample_rate);
        self.audio = Some(Audio {
            listener: None,
            sender: Some(sender),
            metrics,
            pending_bank: None,
            engine,
        });
        self.shared.output_open.store(true, Ordering::Relaxed);
        self.rebuild_at = None;
        self.output_missing = false;
        self.next_poll = now + POLL;
        self.shared.update(|s| s.audio = device_status(&info));
        self.saw_device(&info.name);
        if !reused && self.active.pending_rate != Some(info.sample_rate) {
            self.request_bank();
        }
    }

    /// Records the default output device (opened, or looked at while the output is closed):
    /// another device than the last one sets the `outputChanged` auto-mute if it is armed.
    fn saw_device(&mut self, name: &str) {
        if self.devices.see(Some(name)) {
            log::info!("the default output device changed");
            self.shared.update(automute::output_changed);
        }
    }

    /// Closes the listener and the output, parking the bank they played (if the callback hands
    /// it back within `timeout`) for the next open.
    fn close_audio(&mut self, timeout: Duration) {
        let Some(mut audio) = self.audio.take() else { return };
        audio.listener = None;
        let rate = audio.engine.info().sample_rate;
        let bank = match audio.pending_bank.take() {
            // Newer than the bank the engine plays.
            Some(bank) => Some(bank),
            None => audio.engine.stop(timeout),
        };
        if let Some(bank) = bank {
            self.parked = Some(Parked { rate, bank });
        }
        self.shared.output_open.store(false, Ordering::Relaxed);
    }

    /// Closes the output because nothing needs it.
    fn stop_audio(&mut self, now: Instant) {
        self.close_audio(STOP_TIMEOUT);
        self.pending_preview = None;
        log::info!("audio output closed until something can play");
        self.refresh_idle(now);
    }

    /// Opens the output again: on the new default device, or for a new trigger sender.
    fn restart_audio(&mut self, now: Instant) {
        self.close_audio(STOP_TIMEOUT);
        self.start_audio(now);
    }

    /// While the output is closed: shows the default output device (the one the next open
    /// would use) and keeps the playing pack decoded at its rate.
    fn refresh_idle(&mut self, now: Instant) {
        self.idle_refresh_at = Some(now + IDLE_REFRESH);
        let status = match core_audio::default_output(Some(BUFFER_FRAMES)) {
            Ok(info) => {
                self.output_missing = false;
                self.saw_device(&info.name);
                if self.rate != Some(info.sample_rate) {
                    // The device the next open would use runs at another rate: decode for it now
                    // (a broken pack's last working version cannot be decoded again).
                    self.rate = Some(info.sample_rate);
                    self.parked.take_if(|p| p.rate != info.sample_rate);
                    self.request_bank();
                }
                device_status(&info)
            }
            Err(e) => {
                if !self.output_missing {
                    log::warn!("no audio output device: {e}");
                    self.output_missing = true;
                }
                no_output_status()
            }
        };
        self.shared.update(|s| s.audio = status);
    }

    fn poll_audio(&mut self, now: Instant) {
        let Some(audio) = &mut self.audio else { return };
        audio.engine.collect_garbage();
        if let Some(fault) = audio.engine.take_stream_fault() {
            self.on_fault(fault, now);
            return;
        }
        if audio.engine.take_rerouted() && self.reroute_at.is_none() {
            // The stream plays on, but on a device with its own rate and buffer size.
            log::info!("the system moved the audio output to another device; reopening it there");
            self.reroute_at = Some(now + REBUILD_DELAY);
        }
        self.xruns.add(audio.engine.take_xruns(), now);
        if let Some(n) = audio.listener.as_ref().map(Listener::take_reenabled).filter(|&n| n > 0) {
            log::warn!("the system paused the keyboard hook {n} time(s); it was re-enabled");
        }
        if let Some(metrics) = &mut audio.metrics
            && !metrics.is_empty()
        {
            let batch: Vec<LatencySample> = std::iter::from_fn(|| metrics.pop().ok()).collect();
            self.shared.push_latency(&batch);
        }
        if let Some(bank) = audio.pending_bank.take() {
            audio.pending_bank = audio.engine.replace_bank(bank).err();
        }
    }

    /// The stream died: close it (and the hook feeding it) and reopen shortly.
    fn on_fault(&mut self, fault: StreamFault, now: Instant) {
        log::warn!("audio output stopped ({fault:?}); reopening the default output device");
        self.close_audio(FAULT_STOP_TIMEOUT);
        self.pending_preview = None;
        self.reroute_at = None;
        self.rebuild_at = Some(now + REBUILD_DELAY);
        let message = match fault {
            StreamFault::DeviceGone => "Output device disconnected — reconnecting…",
            StreamFault::Invalidated | StreamFault::Failed => {
                "Audio output stopped — reconnecting…"
            }
        };
        self.shared.update(|s| {
            s.audio = AudioStatus {
                state: AudioState::Fault,
                message: Some(message.to_owned()),
                ..AudioStatus::default()
            }
        });
    }

    // --- input ------------------------------------------------------------------------------

    /// Checks Input Monitoring (never prompts: `open_permission_settings` asks macOS to list
    /// TakTak). The first check is logged either way, later changes when they happen.
    fn poll_permission(&mut self, now: Instant) {
        let first = self.permission.first();
        let was = self.permission.granted();
        let granted = self.permission.check(now, taktak_core::input::has_permission);
        let said = if granted { "granted" } else { "not granted" };
        if first && cfg!(target_os = "macos") {
            log::info!("Input Monitoring: {said}");
        } else if !first && granted != was {
            log::info!("Input Monitoring {said}");
        }
        self.publish_permission();
    }

    /// Starts the key listener on the running engine if none runs. A listener takes the
    /// engine's trigger sender for good, so a second one needs a new engine.
    fn ensure_listener(&mut self, now: Instant) {
        if self.audio.as_ref().is_none_or(|audio| audio.listener.is_some()) {
            return;
        }
        if self.audio.as_ref().is_some_and(|audio| audio.sender.is_none()) {
            log::debug!("reopening the audio output for a new keyboard hook");
            self.restart_audio(now);
        }
        let Some(sender) = self.audio.as_mut().and_then(|audio| audio.sender.take()) else {
            return;
        };
        match input::start_listener(self.shared.gate().clone(), sender) {
            Ok(listener) => {
                log::info!("listening to the keyboard");
                if let Some(audio) = &mut self.audio {
                    audio.listener = Some(listener);
                }
                self.hook_retry_at = None;
                self.hook_refused = false;
            }
            Err(e) => self.on_hook_error(&e, now),
        }
        self.publish_permission();
    }

    fn on_hook_error(&mut self, error: &InputError, now: Instant) {
        let retry = HOOK_RETRY.as_secs();
        match HookFailure::of(error) {
            HookFailure::Unsupported => {
                log::warn!("keyboard listening is unavailable: {error}");
                self.hook_supported = false;
            }
            HookFailure::Refused => {
                log::warn!(
                    "the keyboard hook was refused although permission looked granted; trying \
                     again in {retry} s"
                );
                self.permission.set(now, false);
                self.hook_retry_at = Some(now + HOOK_RETRY);
                self.hook_refused = true;
            }
            HookFailure::Failed => {
                log::warn!("{error}; trying again in {retry} s");
                self.hook_retry_at = Some(now + HOOK_RETRY);
            }
        }
    }

    fn publish_permission(&self) {
        let (supported, granted, failed) =
            (self.hook_supported, self.permission.granted(), self.hook_retry_at.is_some());
        let permission = permission_state(supported, granted, failed);
        let input_group =
            input_group_needed(cfg!(target_os = "linux"), supported, granted, self.hook_refused);
        let relaunch = relaunch_suggested(supported, granted, failed, input_group);
        self.shared.update(|s| {
            s.permission = permission;
            s.onboarding.relaunch_suggested = relaunch;
            s.onboarding.input_group_needed = input_group;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automute::Event;
    use crate::state::{
        AppRef, AppRule, AppRuleEntry, AppRuleMode, AutoMute, PackOrigin, PackSummary,
    };

    struct Rig {
        shared: Shared,
        control: Receiver<Msg>,
        saved: Arc<Mutex<Vec<Settings>>>,
        notified: Arc<Mutex<Vec<(u64, AppState)>>>,
    }

    fn rig() -> Rig {
        let (tx, control) = mpsc::channel();
        let saved = Arc::new(Mutex::new(Vec::new()));
        let notified = Arc::new(Mutex::new(Vec::new()));
        let (s, n) = (saved.clone(), notified.clone());
        let mut state = AppState::initial("0.1.0", Settings::default());
        state.packs.push(PackSummary {
            id: "buckling-spring".into(),
            name: "Buckling Spring".into(),
            author: "A".into(),
            license: "CC0-1.0".into(),
            description: None,
            attribution: None,
            origin: PackOrigin::Bundled,
            has_release: true,
            per_key: false,
            warnings: vec![],
        });
        let shared = Shared::new(
            state,
            tx,
            Box::new(move |settings| s.lock().unwrap().push(settings)),
            Box::new(move |rev, state| n.lock().unwrap().push((rev, state.clone()))),
        );
        Rig { shared, control, saved, notified }
    }

    fn ready(shared: &Shared) {
        shared.update(|s| {
            s.permission = Permission::Granted;
            s.audio.state = AudioState::Ok;
        });
    }

    #[test]
    fn playing_needs_everything_ok() {
        let r = rig();
        assert!(!r.shared.snapshot().playing, "permission and audio unknown at first");
        ready(&r.shared);
        assert!(r.shared.snapshot().playing);
        assert!(!r.shared.update(|s| s.muted = true).playing);
        assert!(r.shared.update(|s| s.muted = false).playing);
        assert!(!r.shared.update(|s| s.settings.enabled = false).playing);
        assert!(r.shared.update(|s| s.settings.enabled = true).playing);
        assert!(!r.shared.update(|s| s.audio.state = AudioState::Fault).playing);
        r.shared.update(|s| s.audio.state = AudioState::Ok);
        assert!(!r.shared.update(|s| s.permission = Permission::Denied).playing);
    }

    fn slack() -> AppRef {
        AppRef { id: "com.tinyspeck.slackmacgap".into(), name: "Slack".into() }
    }

    fn never_slack() -> AppRule {
        AppRule {
            mode: AppRuleMode::Never,
            apps: vec![AppRuleEntry { id: slack().id, name: slack().name }],
        }
    }

    #[test]
    fn rules_block_only_where_supported_and_close_the_gate() {
        let r = rig();
        ready(&r.shared);
        let gate = r.shared.gate().clone();
        // Not supported (yet): the frontmost app is dropped and nothing is blocked.
        let state = r.shared.update(|s| {
            s.settings.app_rule = never_slack();
            s.frontmost_app = Some(slack());
        });
        assert_eq!(state.frontmost_app, None);
        assert!(!state.rule_blocked && state.playing);
        // Supported: Slack in front is silent; the hook's gate closes, permission or not.
        let state = r.shared.update(|s| {
            s.rules_supported = true;
            s.frontmost_app = Some(slack());
        });
        assert!(state.rule_blocked && !state.playing);
        assert!(!gate.load(Ordering::Relaxed) && r.shared.rule_blocked());
        assert!(r.shared.sounds_on(), "the rule is not part of the sounds term");
        assert!(!state.muted && state.auto_mute.is_none(), "rules never touch the mute");
        // Another app in front: sounds again.
        let state = r.shared.update(|s| {
            s.frontmost_app = Some(AppRef { id: "com.apple.Safari".into(), name: "Safari".into() })
        });
        assert!(!state.rule_blocked && state.playing && gate.load(Ordering::Relaxed));
        // "only" with nobody known in front: silent.
        let state = r.shared.update(|s| {
            s.settings.app_rule.mode = AppRuleMode::Only;
            s.frontmost_app = None;
        });
        assert!(state.rule_blocked && !gate.load(Ordering::Relaxed));
    }

    #[test]
    fn auto_mute_closes_the_gate_without_touching_the_mute() {
        let r = rig();
        ready(&r.shared);
        let gate = r.shared.gate().clone();
        let state = r.shared.update(|s| s.auto_mute_reasons.apply(Event::ScreenLocked));
        assert_eq!(state.auto_mute, Some(AutoMute::ScreenLocked));
        assert!(!state.playing && !state.muted && state.settings.enabled);
        assert!(!gate.load(Ordering::Relaxed) && !r.shared.sounds_on());
        r.shared.update(|s| s.settings.mute_on_output_change = true);
        let state = r.shared.update(automute::output_changed);
        assert_eq!(state.auto_mute, Some(AutoMute::ScreenLocked), "the stronger reason shows");
        let state = r.shared.update(|s| s.auto_mute_reasons.apply(Event::ScreenUnlocked));
        assert_eq!(state.auto_mute, Some(AutoMute::OutputChanged));
        assert!(automute::effective_mute(&state) && !gate.load(Ordering::Relaxed));
        let state = r.shared.update(|s| automute::set_muted(s, false));
        assert_eq!(state.auto_mute, None);
        assert!(state.playing && gate.load(Ordering::Relaxed));
    }

    #[test]
    fn rule_blocks_and_auto_mutes_wake_the_control_thread() {
        let r = rig();
        r.shared.update(|s| {
            s.rules_supported = true;
            s.settings.app_rule = never_slack();
        });
        while r.control.try_recv().is_ok() {}
        r.shared.update(|s| s.frontmost_app = Some(slack()));
        assert!(matches!(r.control.try_recv(), Ok(Msg::Sync)), "the block started");
        r.shared.update(|s| s.frontmost_app = None);
        assert!(matches!(r.control.try_recv(), Ok(Msg::Sync)), "the block ended");
        // Muted: the gate stays closed, but the block's start still matters for its 5 s.
        r.shared.update(|s| s.muted = true);
        assert!(matches!(r.control.try_recv(), Ok(Msg::Sync)));
        r.shared.update(|s| s.frontmost_app = Some(slack()));
        assert!(matches!(r.control.try_recv(), Ok(Msg::Sync)));
        // A frontmost change that blocks nothing new: notified only.
        r.shared.update(|s| s.muted = false);
        while r.control.try_recv().is_ok() {}
        r.shared.update(|s| {
            s.frontmost_app = Some(AppRef { id: "a.b".into(), name: "A".into() });
        });
        assert!(matches!(r.control.try_recv(), Ok(Msg::Sync)), "unblocked");
        r.shared.update(|s| {
            s.frontmost_app = Some(AppRef { id: "c.d".into(), name: "C".into() });
        });
        assert!(r.control.try_recv().is_err(), "still unblocked");
        r.shared.update(|s| s.auto_mute_reasons.apply(Event::SessionInactive));
        assert!(matches!(r.control.try_recv(), Ok(Msg::Sync)), "auto-muted");
    }

    #[test]
    fn the_onboarding_offer_is_live() {
        let r = rig();
        assert!(r.shared.snapshot().onboarding.offer, "not done yet");
        let state = r.shared.update(|s| {
            s.settings.onboarding_done = true;
            s.onboarding.permission_required = true;
        });
        assert!(!state.onboarding.offer, "permission unknown so far");
        assert!(r.shared.update(|s| s.permission = Permission::Denied).onboarding.offer);
        assert!(!r.shared.update(|s| s.permission = Permission::Granted).onboarding.offer);
        let state = r.shared.update(|s| {
            s.onboarding.permission_required = false;
            s.permission = Permission::Denied;
        });
        assert!(!state.onboarding.offer, "no permission step on this platform");
        assert!(!permission_required(false));
        assert_eq!(permission_required(true), cfg!(target_os = "macos"));
    }

    #[test]
    fn relaunch_is_suggested_when_the_hook_fails_despite_permission() {
        assert!(relaunch_suggested(true, true, true, false));
        assert!(!relaunch_suggested(true, false, true, false), "not granted: grant it first");
        assert!(!relaunch_suggested(true, true, false, false), "listening");
        assert!(!relaunch_suggested(false, true, true, false), "no listener on this platform");
        assert!(!relaunch_suggested(true, true, true, true), "Linux: the input group is missing");
    }

    #[test]
    fn the_input_group_is_needed_only_on_linux_without_device_access() {
        // Wayland without the group: the devices are not readable.
        assert!(input_group_needed(true, true, false, false));
        // X11 looked fine, but its backend failed and evdev was refused.
        assert!(input_group_needed(true, true, true, true));
        assert!(!input_group_needed(true, true, true, false), "listening");
        assert!(!input_group_needed(true, false, false, false), "TAKTAK_NO_INPUT");
        assert!(!input_group_needed(false, true, false, true), "macOS: Input Monitoring instead");
    }

    #[test]
    fn the_first_permission_check_can_be_waited_for() {
        let shared = Arc::new(rig().shared);
        assert!(!shared.wait_permission_checked(Duration::from_millis(10)));
        let waiter = {
            let shared = shared.clone();
            thread::spawn(move || shared.wait_permission_checked(Duration::from_secs(10)))
        };
        thread::sleep(Duration::from_millis(20));
        shared.mark_permission_checked();
        assert!(waiter.join().unwrap());
        assert!(shared.wait_permission_checked(Duration::ZERO));
    }

    #[test]
    fn gate_tracks_enabled_and_muted() {
        let r = rig();
        let gate = r.shared.gate().clone();
        // Open before permission or audio: those decide whether a hook or stream exists.
        assert!(gate.load(Ordering::Relaxed));
        r.shared.update(|s| s.muted = true);
        assert!(!gate.load(Ordering::Relaxed));
        r.shared.update(|s| s.settings.enabled = false);
        assert!(!gate.load(Ordering::Relaxed));
        r.shared.update(|s| s.muted = false);
        assert!(!gate.load(Ordering::Relaxed));
        r.shared.update(|s| s.settings.enabled = true);
        assert!(gate.load(Ordering::Relaxed));
    }

    #[test]
    fn settings_changes_are_saved_and_synced_other_changes_only_notified() {
        let r = rig();
        r.shared.update(|s| s.settings.master_volume = 0.5);
        assert_eq!(r.saved.lock().unwrap().len(), 1);
        assert!(matches!(r.control.try_recv(), Ok(Msg::Sync)));

        // Mute is not a setting, but it opens or closes the output: the control thread hears.
        r.shared.update(|s| s.muted = true);
        assert_eq!(r.saved.lock().unwrap().len(), 1, "mute is not a setting");
        assert!(matches!(r.control.try_recv(), Ok(Msg::Sync)));

        // Neither a setting nor the gate: notified only.
        r.shared.update(|s| s.active_pack_error = Some("x".into()));
        assert!(r.control.try_recv().is_err());

        // No change: no notification, no save, no wake-up.
        let before = r.notified.lock().unwrap().len();
        r.shared.update(|s| s.muted = true);
        assert_eq!(r.notified.lock().unwrap().len(), before);
        assert!(r.control.try_recv().is_err());

        let revisions: Vec<u64> = r.notified.lock().unwrap().iter().map(|(rev, _)| *rev).collect();
        assert_eq!(revisions, [1, 2, 3]);
        assert!(r.notified.lock().unwrap()[1].1.muted);
    }

    #[test]
    fn turning_sounds_off_wakes_the_control_thread() {
        let r = rig();
        r.shared.update(|s| s.settings.enabled = false);
        assert!(matches!(r.control.try_recv(), Ok(Msg::Sync)));
        // Muting while sounds are off leaves the gate closed, so nothing needs to change.
        r.shared.update(|s| s.muted = true);
        assert!(r.control.try_recv().is_err());
        r.shared.update(|s| s.settings.enabled = true);
        assert!(matches!(r.control.try_recv(), Ok(Msg::Sync)), "a setting changed");
        r.shared.update(|s| s.muted = false);
        assert!(matches!(r.control.try_recv(), Ok(Msg::Sync)), "the gate opened");
    }

    #[test]
    fn the_output_runs_only_while_a_key_can_sound() {
        // gate open, hook supported, granted, retry pending
        assert!(keys_can_sound(true, true, true, false));
        assert!(!keys_can_sound(false, true, true, false), "sounds off or muted");
        assert!(!keys_can_sound(true, false, true, false), "no listener (TAKTAK_NO_INPUT)");
        assert!(!keys_can_sound(true, true, false, false), "no Input Monitoring");
        assert!(!keys_can_sound(true, true, true, true), "the hook waits to be retried");
    }

    #[test]
    fn a_failed_hook_reads_as_denied_and_is_retried() {
        assert_eq!(permission_state(true, true, false), Permission::Granted);
        assert_eq!(permission_state(true, false, false), Permission::Denied);
        // The tap could not be created although permission is granted: no sound plays, so
        // the UI must not say it is granted (or "unknown", which never retries).
        assert_eq!(permission_state(true, true, true), Permission::Denied);
        assert_eq!(permission_state(false, true, false), Permission::Unknown);

        assert_eq!(HookFailure::of(&InputError::Unsupported("x")), HookFailure::Unsupported);
        assert_eq!(HookFailure::of(&InputError::PermissionDenied), HookFailure::Refused);
        let platform = InputError::Platform("CGEventTapCreate returned null".into());
        assert_eq!(HookFailure::of(&platform), HookFailure::Failed);
    }

    #[test]
    fn waiting_never_spins_on_something_already_due() {
        let now = Instant::now();
        let later = now + Duration::from_secs(2);
        let past = now - Duration::from_millis(1);
        assert_eq!(earliest(now, [Some(later), None, Some(now + POLL)]), Some(now + POLL));
        // A hook retry that is due while muted stays due; it must not wake the thread again.
        assert_eq!(earliest(now, [Some(past), Some(now), Some(later)]), Some(later));
        assert_eq!(earliest(now, [Some(past), None]), None);
    }

    #[test]
    fn a_parked_bank_is_reused_only_at_its_rate() {
        let bank = || SoundBank::uniform(vec![0.5; 4].into_boxed_slice(), None);
        let mut parked = Some(Parked { rate: 44_100, bank: bank() });
        assert!(unpark(&mut parked, 48_000).is_none());
        assert!(parked.is_some(), "kept for its own rate");
        assert_eq!(unpark(&mut parked, 44_100).unwrap().samples[0].len(), 4);
        assert!(parked.is_none());
        assert!(unpark(&mut parked, 44_100).is_none());
    }

    #[test]
    fn preview_failures_are_explained() {
        let err = PackError::single(
            "/u/mine",
            taktak_core::pack::Problem::error("sounds/a.wav", "unsupported format."),
        );
        assert_eq!(
            preview_error("My Board", Some(&err)),
            "The preview of My Board could not be loaded: sounds/a.wav: unsupported format."
        );
        assert_eq!(preview_error("Evil\n", None), "Evil\\n has no sound to preview.");
    }

    #[test]
    fn preview_waits_report_failures_but_not_replacements() {
        let (reply, outcome) = mpsc::channel();
        reply.send(Err(NO_OUTPUT.to_owned())).unwrap();
        assert_eq!(PreviewWait(outcome).wait(Duration::from_secs(1)).unwrap_err(), NO_OUTPUT);
        let (reply, outcome) = mpsc::channel();
        reply.send(Ok(())).unwrap();
        assert!(PreviewWait(outcome).wait(Duration::from_secs(1)).is_ok());
        // Replaced or stopped before it played: the control thread drops the reply.
        let (reply, outcome) = mpsc::channel::<Result<(), String>>();
        drop(reply);
        assert!(PreviewWait(outcome).wait(Duration::from_secs(1)).is_ok());
        // Still decoding: it plays when ready.
        let (_reply, outcome) = mpsc::channel::<Result<(), String>>();
        assert!(PreviewWait(outcome).wait(Duration::from_millis(10)).is_ok());
    }

    #[test]
    fn device_status_names_the_device() {
        let info = DeviceInfo {
            name: "Speakers".into(),
            sample_rate: 44_100,
            channels: 2,
            buffer_frames: Some(64),
        };
        let status = device_status(&info);
        assert_eq!(status.device.as_deref(), Some("Speakers"));
        assert_eq!((status.sample_rate, status.buffer_frames), (Some(44_100), Some(64)));
        assert_eq!(status.state, AudioState::Ok);
        let unnamed = DeviceInfo { name: String::new(), ..info };
        assert_eq!(device_status(&unnamed).device, None);
        assert_eq!(no_output_status().state, AudioState::Fault);
    }

    #[test]
    fn announce_notifies_even_without_a_change() {
        let r = rig();
        r.shared.update(|s| s.active_pack_error = None);
        assert!(r.notified.lock().unwrap().is_empty());
        r.shared.announce(|s| s.active_pack_error = None);
        let notified = r.notified.lock().unwrap();
        assert_eq!(notified.len(), 1);
        assert_eq!(notified[0].0, 1);
        assert!(r.saved.lock().unwrap().is_empty(), "nothing to save");
    }

    #[test]
    fn failed_updates_change_nothing() {
        let r = rig();
        let result: Result<AppState, String> = r.shared.try_update(|s| {
            s.settings.pack_id = "half-set".into();
            Err("nope".into())
        });
        assert_eq!(result.unwrap_err(), "nope");
        assert_eq!(r.shared.settings().pack_id, "buckling-spring");
        assert!(r.notified.lock().unwrap().is_empty());
        assert!(r.saved.lock().unwrap().is_empty());
    }

    #[test]
    fn latency_needs_five_presses_and_resets() {
        let r = rig();
        let sample = |ms: u64| LatencySample {
            input_ns: ms * 100_000,
            queue_ns: ms * 200_000,
            output_ns: 4_000_000,
        };
        r.shared.push_latency(&[sample(1), sample(2), sample(3), sample(4)]);
        assert_eq!(r.shared.latency(), None);
        r.shared.push_latency(&[sample(5)]);
        let report = r.shared.latency().unwrap();
        assert_eq!(report.count, 5);
        assert!((report.output_ms - 4.0).abs() < 1e-9);
        assert!((report.total_p50_ms - (0.3 + 0.6 + 4.0)).abs() < 1e-9);
        r.shared.reset_latency();
        assert_eq!(r.shared.latency(), None);
        // Only the most recent samples are kept.
        let many: Vec<_> = (0..MAX_LATENCY_SAMPLES as u64 + 50).map(sample).collect();
        r.shared.push_latency(&many);
        assert_eq!(r.shared.latency().unwrap().count, MAX_LATENCY_SAMPLES);
    }

    #[test]
    fn preview_cache_keeps_the_most_recent() {
        let mut cache = PreviewCache::default();
        let clip = |v: f32| vec![v; 2].into_boxed_slice();
        for (i, id) in ["a", "b", "c", "d"].into_iter().enumerate() {
            cache.insert(id.into(), 48_000, clip(i as f32));
        }
        assert_eq!(cache.get("a", 48_000).unwrap()[0], 0.0); // now most recent
        assert!(cache.get("a", 44_100).is_none(), "other rate");
        cache.insert("e".into(), 48_000, clip(4.0)); // evicts "b"
        assert!(cache.get("b", 48_000).is_none());
        assert!(cache.get("a", 48_000).is_some());
        cache.forget("a");
        assert!(cache.get("a", 48_000).is_none());
        cache.insert("c".into(), 48_000, clip(9.0)); // replaces
        assert_eq!(cache.get("c", 48_000).unwrap()[0], 9.0);
        assert_eq!(cache.entries.len(), 3);
    }
}
