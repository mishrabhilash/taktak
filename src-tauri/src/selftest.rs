//! `taktak --selftest [--allow-no-audio] [--packs <dir>]`: a headless check of the app layer
//! for CI and integration runs. No window, no tray, no keyboard listener and no permission
//! prompt. It scans the bundled packs, opens the default output device, loads every pack at
//! the device rate and swaps each into the engine, checks the input gate, per-app rule
//! evaluation, the auto-mute state machine, the onboarding decision, settings persistence and
//! migration (in a temporary folder), then runs the real [`Service`] (control thread, loader,
//! registry watcher) through pack switches, a preview that opens and closes the output (closed
//! otherwise: nothing can play without a listener), a saved selection of a pack TakTak no longer
//! bundles (moved to the default pack), a hot-reloaded user pack that breaks
//! (and is named as broken by a freshly started service) and is deleted, and a synthetic
//! Mechvibes pack imported into the user packs folder (listed as a personal pack, then
//! recognized as already imported and replaced), and idle sleep with a synthetic keyboard (the
//! output closes when nobody types; a key-down reopens it and plays, its wake latency measured).
//! Prints one line per check with its timing;
//! exit code 0 when nothing failed.
//!
//! `--allow-no-audio` turns "no output device" into skipped playback checks instead of a
//! failure (for machines without audio hardware).

use crate::automute::{self, Event};
use crate::catalog;
use crate::idle::{self, Activity, Waker};
use crate::input::{self, KeySource};
use crate::loader;
use crate::mechvibes;
use crate::rules;
use crate::service::{Config, ControlTx, NO_OUTPUT, Service, Shared};
use crate::settings::{self, Persister};
use crate::state::{
    AppRef, AppRule, AppRuleEntry, AppRuleMode, AppState, AudioState, AutoMute, DEFAULT_PACK_ID,
    MechvibesImport, PackOrigin, Permission, RETIRED_PACK_IDS, Settings, VariantMode,
};
use crate::windows;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use taktak_core::audio::{Engine, EngineConfig, SoundBank};
use taktak_core::input::{KeyAction, KeyEvent};
use taktak_core::key::Key;
use taktak_core::pack::manifest::PERSONAL_LICENSE;
use taktak_core::pack::{PackInfo, PackRegistry};

/// How long a service check waits for the state it expects.
const WAIT: Duration = Duration::from_secs(10);
/// The id the hot-reload checks give their copy of a bundled pack.
const USER_PACK_ID: &str = "selftest-user-pack";

/// Command-line options after `--selftest`.
#[derive(Debug, Default, PartialEq)]
pub struct Options {
    pub allow_no_audio: bool,
    /// Bundled packs folder to use instead of the resource dir's.
    pub packs: Option<PathBuf>,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Options, String> {
        let mut options = Options::default();
        let mut args = args.iter().filter(|a| *a != "--selftest");
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--allow-no-audio" => options.allow_no_audio = true,
                "--packs" => {
                    let dir = args.next().ok_or("--packs needs a folder")?;
                    options.packs = Some(PathBuf::from(dir));
                }
                other => return Err(format!("unknown self-test option {other:?}")),
            }
        }
        Ok(options)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Pass,
    Fail,
    Skip,
}

/// Collects check results and prints them as they come.
struct Report {
    started: Instant,
    passed: usize,
    failed: usize,
    skipped: usize,
}

impl Report {
    fn new() -> Report {
        Report { started: Instant::now(), passed: 0, failed: 0, skipped: 0 }
    }

    fn line(&mut self, outcome: Outcome, name: &str, elapsed: Option<Duration>, detail: &str) {
        let tag = match outcome {
            Outcome::Pass => {
                self.passed += 1;
                "PASS"
            }
            Outcome::Fail => {
                self.failed += 1;
                "FAIL"
            }
            Outcome::Skip => {
                self.skipped += 1;
                "SKIP"
            }
        };
        let time = elapsed.map_or(String::new(), |d| format!("{:.1} ms", d.as_secs_f64() * 1e3));
        println!("  {tag}  {name:<44} {time:>10}  {detail}");
    }

    /// Runs `check`, timing it: `Ok(detail)` passes, `Err(why)` fails.
    fn check<T>(
        &mut self,
        name: &str,
        check: impl FnOnce() -> Result<(T, String), String>,
    ) -> Option<T> {
        let started = Instant::now();
        match check() {
            Ok((value, detail)) => {
                self.line(Outcome::Pass, name, Some(started.elapsed()), &detail);
                Some(value)
            }
            Err(why) => {
                self.line(Outcome::Fail, name, Some(started.elapsed()), &why);
                None
            }
        }
    }

    fn skip(&mut self, name: &str, why: &str) {
        self.line(Outcome::Skip, name, None, why);
    }

    fn summary(&self) -> String {
        format!(
            "{} passed, {} failed, {} skipped in {:.0} ms",
            self.passed,
            self.failed,
            self.skipped,
            self.started.elapsed().as_secs_f64() * 1e3
        )
    }
}

/// Polls `condition` every 10 ms until it holds or `timeout` passes.
fn wait_for(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1e3)
}

/// Runs the self-test and returns the process exit code.
pub fn run(version: &str, resource_dir: Option<PathBuf>, args: &[String]) -> i32 {
    let options = match Options::parse(args) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("taktak --selftest: {e}");
            return 2;
        }
    };
    println!("TakTak {version} self-test");
    let mut report = Report::new();
    let scratch = std::env::temp_dir().join(format!("taktak-selftest-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    if let Err(e) = fs::create_dir_all(&scratch) {
        eprintln!("taktak --selftest: cannot create {}: {e}", scratch.display());
        return 2;
    }

    let bundled = options.packs.clone().or_else(|| catalog::bundled_dir(resource_dir.as_deref()));
    let packs = report.check("scan bundled packs", || scan(bundled.as_deref()));
    let audio = open_audio(&mut report, options.allow_no_audio);
    let playback = audio.is_some();
    let rate = audio.as_ref().map_or(48_000, |(engine, _)| engine.info().sample_rate);
    let mut banks = Vec::new();
    for info in packs.iter().flatten() {
        let name = format!("load {} at {rate} Hz", info.id);
        if let Some(bank) = report.check(&name, || load(info, rate)) {
            banks.push(bank);
        }
    }
    match audio {
        Some((mut engine, _sender)) => {
            report.check("swap every pack into the engine", || swap(&mut engine, banks));
        }
        None => report.skip("swap every pack into the engine", "no audio output"),
    }
    report.check("input gate (enabled and mute)", gate);
    report.check("per-app rules (only, never, unknown app)", app_rules);
    report.check("auto-mute (lock, session, output change)", auto_mute);
    report.check("onboarding decision", onboarding);
    if cfg!(target_os = "macos") {
        report.check("hotkey labels from the keyboard layout", key_labels);
    } else {
        report.skip("hotkey labels from the keyboard layout", "macOS only");
    }
    report.check("settings persist and reload", || persist(&scratch));
    report.check("settings corrupt-file recovery", || recover(&scratch));
    report.check("settings migration (Milestone 3 file)", || migrate(&scratch));

    match (&bundled, &packs) {
        (Some(bundled), Some(packs)) if !packs.is_empty() => {
            service_checks(&mut report, version, bundled, packs, &scratch, playback);
        }
        _ => report.skip("service checks", "no bundled packs"),
    }

    let _ = fs::remove_dir_all(&scratch);
    println!("{}", report.summary());
    i32::from(report.failed > 0)
}

fn scan(bundled: Option<&Path>) -> Result<(Vec<PackInfo>, String), String> {
    let dir = bundled.ok_or("no bundled packs folder found")?;
    let mut registry = PackRegistry::new(Some(dir.to_path_buf()), None);
    registry.scan();
    let packs = registry.packs();
    let invalid: Vec<String> = registry
        .entries()
        .into_iter()
        .filter_map(|e| e.status.err())
        .map(|e| e.to_string())
        .collect();
    if !invalid.is_empty() {
        return Err(format!("invalid bundled pack(s): {}", invalid.join("; ")));
    }
    if !packs.iter().any(|p| p.id == DEFAULT_PACK_ID) {
        return Err(format!(
            "the default pack {DEFAULT_PACK_ID} is missing from {}",
            dir.display()
        ));
    }
    if let Some(p) = packs.iter().find(|p| RETIRED_PACK_IDS.contains(&p.id.as_str())) {
        return Err(format!(
            "the bundled pack {} is listed in RETIRED_PACK_IDS: remove it from that list",
            p.id
        ));
    }
    let detail = format!("{} packs in {}", packs.len(), dir.display());
    Ok((packs, detail))
}

fn open_audio(
    report: &mut Report,
    allow_no_audio: bool,
) -> Option<(Engine, taktak_core::audio::TriggerSender)> {
    let started = Instant::now();
    match Engine::start(EngineConfig::default(), |_| SoundBank::default()) {
        Ok((engine, sender)) => {
            let info = engine.info();
            let buffer = info.buffer_frames.map_or("device default".to_owned(), |f| f.to_string());
            let detail = format!("“{}”, {} Hz, buffer {buffer}", info.name, info.sample_rate);
            report.line(
                Outcome::Pass,
                "open the default audio output",
                Some(started.elapsed()),
                &detail,
            );
            Some((engine, sender))
        }
        Err(e) if allow_no_audio => {
            report.skip("open the default audio output", &format!("{e} (allowed)"));
            None
        }
        Err(e) => {
            report.line(
                Outcome::Fail,
                "open the default audio output",
                Some(started.elapsed()),
                &e.to_string(),
            );
            None
        }
    }
}

fn load(info: &PackInfo, rate: u32) -> Result<(SoundBank, String), String> {
    let loaded = loader::load_pack(info, rate).map_err(|e| e.to_string())?;
    let bank = loaded.bank;
    if bank.samples.is_empty() {
        return Err("the pack has no sounds".into());
    }
    let frames: usize = bank.samples.iter().map(|s| s.len()).sum();
    let detail = format!(
        "{} sounds, {:.1} s of audio, preview {}",
        bank.samples.len(),
        frames as f64 / f64::from(rate),
        if loaded.preview.is_some() { "yes" } else { "no" }
    );
    Ok((bank, detail))
}

fn swap(engine: &mut Engine, banks: Vec<SoundBank>) -> Result<((), String), String> {
    let count = banks.len();
    for bank in banks {
        if engine.replace_bank(bank).is_err() {
            return Err(
                "the engine's command ring is full (the audio callback is not running)".into()
            );
        }
        // Let the callback take it and hand the old bank back.
        thread::sleep(Duration::from_millis(15));
        engine.collect_garbage();
    }
    let (click, _) = loader::builtin_bank(engine.info().sample_rate);
    engine.set_master_gain(0.0);
    engine.preview(click.samples[0].clone()).map_err(|_| "the preview was refused")?;
    if !engine.stop_preview() {
        return Err("stopping the preview was refused".into());
    }
    thread::sleep(Duration::from_millis(20));
    engine.collect_garbage();
    if let Some(fault) = engine.take_stream_fault() {
        return Err(format!("the stream reported {fault:?}"));
    }
    Ok(((), format!("{count} swaps, preview played and stopped, no stream fault")))
}

/// A [`Shared`] like the service's, without a control thread, persistence or events.
fn bare_shared(settings: Settings) -> Shared {
    let (tx, _rx) = mpsc::channel();
    let tx = ControlTx::new(tx, Arc::new(Waker::default()));
    Shared::new(AppState::initial("selftest", settings), tx, Box::new(|_| {}), Box::new(|_, _| {}))
}

/// The gate the key hook checks, driven through the same [`Shared`] the commands use.
fn gate() -> Result<((), String), String> {
    let shared = bare_shared(Settings::default());
    let activity = Activity::new(Arc::new(Waker::default()));
    let sent = AtomicUsize::new(0);
    let press = || {
        let event =
            KeyEvent { key: Key::KeyA, action: KeyAction::Down, event_ns: 0, received_ns: 0 };
        input::forward(shared.gate(), &activity, event, |_| {
            sent.fetch_add(1, Ordering::Relaxed);
            true
        })
    };
    let cases =
        [(true, false, true), (true, true, false), (false, false, false), (false, true, false)];
    for (enabled, muted, sounds) in cases {
        let state = shared.update(|s| {
            s.settings.enabled = enabled;
            s.muted = muted;
        });
        if press() != sounds {
            return Err(format!("enabled={enabled} muted={muted}: expected sound={sounds}"));
        }
        if state.playing {
            return Err("playing without permission or audio".into());
        }
    }
    shared.update(|s| {
        s.settings.enabled = true;
        s.muted = false;
        s.permission = Permission::Granted;
        s.audio.state = AudioState::Ok;
    });
    if !shared.snapshot().playing {
        return Err("not playing with everything on".into());
    }
    Ok(((), format!("4 combinations, {} event(s) forwarded", sent.load(Ordering::Relaxed))))
}

/// The current layout's labels for the hotkey key positions (macOS, on the main thread): every
/// letter at least, each a short visible text.
fn key_labels() -> Result<((), String), String> {
    let labels = crate::keylabels::read_now().ok_or("the keyboard layout could not be read")?;
    let letters = labels.keys().filter(|k| k.starts_with("Key")).count();
    if letters < 26 {
        return Err(format!("only {letters} letters labelled"));
    }
    if let Some((key, label)) = labels.iter().find(|(_, l)| l.chars().count() > 4) {
        return Err(format!("{key} has the label {label:?}"));
    }
    let m = labels.get("KeyM").map_or("?", String::as_str);
    // French AZERTY, read without selecting it: the US M position prints ",", Q prints "a".
    let french = crate::keylabels::read_installed("com.apple.keylayout.French")
        .ok_or("the French layout could not be read")?;
    let (fm, fq) = (french.get("KeyM"), french.get("KeyQ"));
    if fm.map(String::as_str) != Some(",") || fq.map(String::as_str) != Some("a") {
        return Err(format!("French: M position {fm:?}, Q position {fq:?}"));
    }
    Ok((
        (),
        format!(
            "current layout: {} keys, the M position prints {m:?}; French: M → \",\", Q → \"a\"",
            labels.len()
        ),
    ))
}

/// Per-app rules through [`Shared`]: which frontmost app is silenced in each mode, and that a
/// block closes the hook's gate.
fn app_rules() -> Result<((), String), String> {
    let shared = bare_shared(Settings::default());
    shared.update(|s| {
        s.permission = Permission::Granted;
        s.audio.state = AudioState::Ok;
        s.rules_supported = true;
        s.settings.app_rule.apps =
            vec![AppRuleEntry { id: "com.tinyspeck.slackmacgap".into(), name: "Slack".into() }];
    });
    let slack = || Some(AppRef { id: "com.tinyspeck.slackmacgap".into(), name: "Slack".into() });
    let safari = || Some(AppRef { id: "com.apple.Safari".into(), name: "Safari".into() });
    let cases = [
        (AppRuleMode::Everywhere, slack(), false),
        (AppRuleMode::Everywhere, None, false),
        (AppRuleMode::Only, slack(), false),
        (AppRuleMode::Only, safari(), true),
        (AppRuleMode::Only, None, true),
        (AppRuleMode::Never, slack(), true),
        (AppRuleMode::Never, safari(), false),
        (AppRuleMode::Never, None, false),
    ];
    for (mode, app, blocked) in cases.clone() {
        let name = app.as_ref().map_or("unknown".to_owned(), |a| a.name.clone());
        let state = shared.update(|s| {
            s.settings.app_rule.mode = mode;
            s.frontmost_app = app;
        });
        let gate = shared.gate().load(Ordering::Relaxed);
        if state.rule_blocked != blocked || gate == blocked || state.playing == blocked {
            return Err(format!(
                "{mode:?} with {name} in front: blocked {}, gate {gate}, playing {}",
                state.rule_blocked, state.playing
            ));
        }
        if state.muted || !state.settings.enabled || state.auto_mute.is_some() {
            return Err("a rule changed the mute or the master switch".into());
        }
    }
    // Where rules are unsupported nothing is blocked, and the frontmost app is not kept.
    let state = shared.update(|s| {
        s.settings.app_rule.mode = AppRuleMode::Never;
        s.rules_supported = false;
        s.frontmost_app = slack();
    });
    if state.rule_blocked || state.frontmost_app.is_some() {
        return Err("rules applied where they are unsupported".into());
    }
    let mut rule = AppRule::default();
    let added = rules::add(&mut rule, " com.apple.Safari ", "Safari");
    let rejected =
        [rules::add(&mut rule, "tech.taktak.app", "TakTak"), rules::add(&mut rule, "a b", "")];
    if added != Ok(true) || rejected != [Err(rules::OWN_APP), Err(rules::NOT_AN_APP)] {
        return Err(format!("add_rule_app checks: {added:?}, {rejected:?}"));
    }
    Ok(((), format!("{} mode × app combinations; unsupported ignores rules", cases.len())))
}

/// The auto-mute reasons through [`Shared`]: lock and session, an armed output change that
/// outlasts a lock and only an unmute clears, and the gate closed meanwhile.
fn auto_mute() -> Result<((), String), String> {
    let shared = bare_shared(Settings { mute_on_output_change: true, ..Settings::default() });
    shared.update(|s| {
        s.permission = Permission::Granted;
        s.audio.state = AudioState::Ok;
    });
    let expect = |state: &AppState, want: Option<AutoMute>, step: &str| {
        let gate = shared.gate().load(Ordering::Relaxed);
        if state.auto_mute != want || gate != want.is_none() || state.playing != want.is_none() {
            return Err(format!("{step}: auto-mute {:?}, gate {gate}", state.auto_mute));
        }
        if state.muted {
            return Err(format!("{step}: auto-mute set the manual mute"));
        }
        Ok(())
    };
    let state = shared.update(|s| s.auto_mute_reasons.apply(Event::ScreenLocked));
    expect(&state, Some(AutoMute::ScreenLocked), "locked")?;
    let state = shared.update(|s| s.auto_mute_reasons.apply(Event::SessionInactive));
    expect(&state, Some(AutoMute::ScreenLocked), "session inactive")?;
    // Headphones unplugged at the lock screen.
    let state = shared.update(automute::output_changed);
    expect(&state, Some(AutoMute::ScreenLocked), "output changed while locked")?;
    let state = shared.update(|s| s.auto_mute_reasons.apply(Event::ScreenUnlocked));
    expect(&state, Some(AutoMute::ScreenLocked), "unlocked, session still inactive")?;
    let state = shared.update(|s| s.auto_mute_reasons.apply(Event::SessionActive));
    expect(&state, Some(AutoMute::OutputChanged), "back, output change pending")?;
    if !automute::effective_mute(&state) {
        return Err("the mute switch does not show the output change".into());
    }
    // The hotkey (or tray) unmutes, clearing it.
    let state = shared.update(automute::toggle_mute);
    expect(&state, None, "unmuted")?;
    // Muted by hand or with the setting off, a device change does not auto-mute.
    shared.update(|s| automute::set_muted(s, true));
    let state = shared.update(automute::output_changed);
    if state.auto_mute.is_some() {
        return Err("an output change auto-muted while muted by hand".into());
    }
    shared.update(|s| {
        automute::set_muted(s, false);
        automute::set_mute_on_output_change(s, false);
    });
    let state = shared.update(automute::output_changed);
    expect(&state, None, "setting off")?;
    let mut watch = automute::DeviceWatch::default();
    let seen = [
        watch.see(Some("Speakers")),
        watch.see(None),
        watch.see(Some("Speakers")),
        watch.see(Some("AirPods")),
    ];
    if seen != [false, false, false, true] {
        return Err(format!("device changes seen as {seen:?}"));
    }
    Ok(((), "lock/session, armed output change, unmute clears; device watch".into()))
}

/// The onboarding offer, live through [`Shared`].
fn onboarding() -> Result<((), String), String> {
    let shared = bare_shared(Settings::default());
    let offer = |change: &dyn Fn(&mut AppState)| shared.update(|s| change(s)).onboarding.offer;
    let steps = [
        ("first launch", offer(&|_| {}), true),
        (
            "done, permission unknown",
            offer(&|s| {
                s.settings.onboarding_done = true;
                s.onboarding.permission_required = true;
            }),
            false,
        ),
        ("Input Monitoring missing", offer(&|s| s.permission = Permission::Denied), true),
        ("granted", offer(&|s| s.permission = Permission::Granted), false),
        (
            "no permission step",
            offer(&|s| {
                s.onboarding.permission_required = false;
                s.permission = Permission::Denied;
            }),
            false,
        ),
    ];
    for (step, got, want) in steps {
        if got != want {
            return Err(format!("{step}: offer {got}, expected {want}"));
        }
    }
    if !windows::open_at_startup(false, true) || windows::open_at_startup(false, false) {
        return Err("a relaunch from the onboarding window does not reopen it".into());
    }
    Ok(((), format!("{} steps", steps.len())))
}

fn persist(scratch: &Path) -> Result<((), String), String> {
    let path = scratch.join("persist").join(settings::FILE_NAME);
    let persister = Persister::start(path.clone(), Settings::default(), settings::DEBOUNCE)
        .map_err(|e| e.to_string())?;
    let wanted = Settings {
        enabled: false,
        pack_id: "typewriter".into(),
        master_volume: 0.42,
        variant_mode: VariantMode::Random,
        humanize: 0.0,
        mute_hotkey: None,
        ..Settings::default()
    };
    let handle = persister.handle();
    for i in 0..10 {
        handle.save(Settings { master_volume: f64::from(i) / 10.0, ..wanted.clone() });
    }
    handle.save(wanted.clone());
    if path.exists() {
        return Err("written before the debounce time".into());
    }
    if !persister.flush(Duration::from_secs(2)) {
        return Err("the settings writer did not answer".into());
    }
    let loaded = settings::load(&path);
    if loaded.settings != wanted || loaded.note.is_some() {
        return Err(format!("read back {:?} ({:?})", loaded.settings, loaded.note));
    }
    Ok(((), format!("11 debounced saves → 1 write, read back equal ({})", path.display())))
}

/// A Milestone 3 settings file loads with the new defaults (onboarding counted as done), and
/// `appRule` leniency keeps the good entries of a damaged list.
fn migrate(scratch: &Path) -> Result<((), String), String> {
    let path = scratch.join("migrate").join(settings::FILE_NAME);
    fs::create_dir_all(path.parent().unwrap_or(scratch)).map_err(|e| e.to_string())?;
    let m3 = r#"{"enabled": true, "packId": "typewriter", "masterVolume": 0.5, "pressVolume": 1.0,
        "releaseVolume": 1.0, "variantMode": "random", "humanize": 0.25,
        "muteHotkey": "CommandOrControl+Alt+Shift+M", "launchAtLogin": false}"#;
    fs::write(&path, m3).map_err(|e| e.to_string())?;
    let loaded = settings::load(&path);
    let s = &loaded.settings;
    if !s.onboarding_done
        || s.mute_on_output_change
        || s.app_rule != AppRule::default()
        || s.pack_id != "typewriter"
        || loaded.note.is_some()
    {
        return Err(format!("Milestone 3 file read as {s:?} ({:?})", loaded.note));
    }
    fs::write(
        &path,
        r#"{"onboardingDone": false, "appRule": {"mode": "never", "apps": [
            {"id": " com.apple.Safari ", "name": "Safari", "future": 1}, {"name": "no id"},
            "bare", {"id": "us.zoom.xos"}, {"id": "com.apple.Safari"}]}}"#,
    )
    .map_err(|e| e.to_string())?;
    let loaded = settings::load(&path);
    let want = AppRule {
        mode: AppRuleMode::Never,
        apps: vec![
            AppRuleEntry { id: "com.apple.Safari".into(), name: "Safari".into() },
            AppRuleEntry { id: "us.zoom.xos".into(), name: "us.zoom.xos".into() },
        ],
    };
    if loaded.settings.app_rule != want || loaded.settings.onboarding_done {
        return Err(format!("damaged rule list read as {:?}", loaded.settings.app_rule));
    }
    Ok(((), "new fields defaulted, onboarding done; 2 of 5 rule entries kept".into()))
}

fn recover(scratch: &Path) -> Result<((), String), String> {
    let path = scratch.join("corrupt").join(settings::FILE_NAME);
    fs::create_dir_all(path.parent().unwrap_or(scratch)).map_err(|e| e.to_string())?;
    fs::write(&path, "{ not json").map_err(|e| e.to_string())?;
    let loaded = settings::load(&path);
    if loaded.settings != Settings::default() {
        return Err("a corrupt file did not give the defaults".into());
    }
    if !settings::corrupt_backup_path(&path).exists() {
        return Err("the corrupt file was not kept aside".into());
    }
    Ok(((), "defaults used, corrupt file moved aside".into()))
}

/// The service end to end: control thread, loader, registry and watcher, settings writer.
fn service_checks(
    report: &mut Report,
    version: &str,
    bundled: &Path,
    packs: &[PackInfo],
    scratch: &Path,
    playback: bool,
) {
    let settings_path = scratch.join("service").join(settings::FILE_NAME);
    let user_dir = scratch.join("user-packs");
    let config = Config {
        version: version.to_owned(),
        settings: Settings::default(),
        settings_path: settings_path.clone(),
        bundled_dir: Some(bundled.to_path_buf()),
        user_dir: Some(user_dir.clone()),
        listen: false,
        keys: KeySource::Os,
        idle_unit: idle::MINUTE,
    };
    let Some(service) = report.check("service: start", || {
        let service = Service::start(config, Box::new(|_, _| {})).map_err(|e| e.to_string())?;
        let ready = wait_for(WAIT, || {
            let state = service.snapshot();
            state.packs.len() == packs.len()
                && (!playback
                    || (state.audio.state == AudioState::Ok
                        && service.now_playing().as_deref() == Some(DEFAULT_PACK_ID)))
        });
        let state = service.snapshot();
        if !ready {
            return Err(format!(
                "{} packs listed, audio {:?}, playing {:?}",
                state.packs.len(),
                state.audio.state,
                service.now_playing()
            ));
        }
        // Without a key listener no key can make a sound: the pack is decoded, the stream shut.
        if service.output_open() {
            return Err("the output is open although nothing can play".into());
        }
        let detail = format!(
            "{} packs listed, audio {:?} at {} Hz, permission {:?}, playing {}, output closed",
            state.packs.len(),
            state.audio.state,
            state.audio.sample_rate.unwrap_or_default(),
            state.permission,
            service.now_playing().unwrap_or_else(|| "nothing".into())
        );
        Ok((service, detail))
    }) else {
        return;
    };

    let other = packs.iter().find(|p| p.id != DEFAULT_PACK_ID).map(|p| p.id.clone());
    if let (Some(other), true) = (&other, playback) {
        report.check(&format!("service: switch to {other}"), || {
            service.set_pack(other)?;
            let switched = wait_for(WAIT, || {
                service.now_playing().as_deref() == Some(other.as_str())
                    && service.snapshot().active_pack_error.is_none()
            });
            if !switched {
                return Err(format!("still playing {:?}", service.now_playing()));
            }
            Ok(((), "loaded off the control thread and swapped in".into()))
        });
    } else {
        report.skip("service: switch packs", "no audio output or only one pack");
    }

    report.check("service: reject unknown pack and clamp levels", || {
        if service.set_pack("no-such-pack").is_ok() {
            return Err("an unknown pack was accepted".into());
        }
        let state = service.update(|s| s.settings.master_volume = settings::unit(1.7));
        if state.settings.master_volume != 1.0 {
            return Err(format!("master volume {}", state.settings.master_volume));
        }
        if service.preview("no-such-pack").is_ok() {
            return Err("previewing an unknown pack was accepted".into());
        }
        Ok(((), "set_pack and preview_pack reject it; 1.7 → 1.0".into()))
    });

    if playback {
        report.check("service: preview opens the output while it plays", || preview(&service));
    } else {
        report.check("service: preview without an output says so", || {
            match service.preview(DEFAULT_PACK_ID)?.wait(WAIT) {
                Err(e) if e == NO_OUTPUT => Ok(((), e)),
                other => Err(format!("got {other:?}")),
            }
        });
    }

    report.check("service: mute and enable gate the hook", || {
        let gate = service.gate().clone();
        let mut seen = String::new();
        for (enabled, muted) in [(true, true), (false, false), (true, false)] {
            service.update(|s| {
                s.settings.enabled = enabled;
                s.muted = muted;
            });
            let open = gate.load(Ordering::Relaxed);
            if open != rules::gate_open(enabled, muted, false, false) {
                return Err(format!("enabled={enabled} muted={muted}: gate {open}"));
            }
            let _ = write!(seen, "{}", u8::from(open));
        }
        Ok(((), format!("gate {seen} for (on, muted) (off) (on)")))
    });

    report.check("service: a per-app rule closes the gate", || {
        let gate = service.gate().clone();
        let slack = AppRef { id: "com.tinyspeck.slackmacgap".into(), name: "Slack".into() };
        let state = service.update(|s| {
            s.rules_supported = true;
            s.settings.app_rule = AppRule {
                mode: AppRuleMode::Never,
                apps: vec![AppRuleEntry { id: slack.id.clone(), name: slack.name.clone() }],
            };
            s.frontmost_app = Some(slack);
        });
        let blocked = state.rule_blocked && !gate.load(Ordering::Relaxed);
        let state = service.update(|s| s.frontmost_app = None);
        let reopened = !state.rule_blocked && gate.load(Ordering::Relaxed);
        service.update(|s| {
            s.settings.app_rule = AppRule::default();
            s.rules_supported = false;
        });
        if !blocked || !reopened {
            return Err(format!("blocked {blocked}, reopened {reopened}"));
        }
        Ok(((), "Slack in front silences, leaving it sounds again".into()))
    });

    report.check("service: retired pack selection moves to the default", || {
        retired_selection(bundled, scratch, playback)
    });

    hot_reload_checks(report, &service, bundled, packs, &user_dir, scratch, playback);

    if playback {
        idle_sleep_checks(report, bundled, scratch);
    } else {
        report.skip("service: idle sleep", "no audio output");
    }

    report.check("service: imported Mechvibes pack is listed as personal", || {
        mechvibes_import(&service, &user_dir, scratch)
    });

    report.check("service: settings saved on shutdown", || {
        service.update(|s| s.settings.humanize = 0.5);
        service.shutdown(Duration::from_secs(3));
        let saved = settings::load(&settings_path).settings;
        if saved.humanize != 0.5 || saved.master_volume != 1.0 {
            return Err(format!("saved {saved:?}"));
        }
        Ok(((), format!("pack {}, humanize 0.5", saved.pack_id)))
    });
}

/// A settings file that selects a pack TakTak no longer bundles (an earlier default) starts on
/// the default pack, silently (no `activePackError`), and the new selection is saved.
fn retired_selection(
    bundled: &Path,
    scratch: &Path,
    playback: bool,
) -> Result<((), String), String> {
    // The most recently retired pack: the previous default.
    let retired = RETIRED_PACK_IDS[RETIRED_PACK_IDS.len() - 1];
    let settings_path = scratch.join("retired").join(settings::FILE_NAME);
    let config = Config {
        version: "selftest".into(),
        settings: Settings { pack_id: retired.into(), ..Settings::default() },
        settings_path: settings_path.clone(),
        bundled_dir: Some(bundled.to_path_buf()),
        user_dir: Some(scratch.join("retired-user-packs")),
        listen: false,
        keys: KeySource::Os,
        idle_unit: idle::MINUTE,
    };
    let started = Service::start(config, Box::new(|_, _| {})).map_err(|e| e.to_string())?;
    let migrated = wait_for(WAIT, || {
        started.settings().pack_id == DEFAULT_PACK_ID
            && (!playback || started.now_playing().as_deref() == Some(DEFAULT_PACK_ID))
    });
    let (state, playing) = (started.snapshot(), started.now_playing());
    started.shutdown(Duration::from_secs(3));
    if !migrated || state.active_pack_error.is_some() {
        return Err(format!(
            "selected {:?}, playing {playing:?}, error {:?}",
            state.settings.pack_id, state.active_pack_error
        ));
    }
    let saved = settings::load(&settings_path).settings.pack_id;
    if saved != DEFAULT_PACK_ID {
        return Err(format!("saved {saved:?}"));
    }
    Ok(((), format!("{retired} → {saved}, no error, saved")))
}

/// The idle time in the idle-sleep checks: [`IDLE_MINUTES`] units of [`IDLE_UNIT`].
const IDLE_UNIT: Duration = Duration::from_millis(100);
const IDLE_MINUTES: u32 = 3;
/// Sleep/wake cycles measured.
const WAKE_CYCLES: usize = 7;

/// Idle sleep end to end with the synthetic keyboard: the real gate, activity, trigger ring and
/// control thread, without an OS hook or permission. The output closes after the idle time while
/// the listener stays; a key-down reopens it and that key press plays (its latency sample is the
/// wake latency); muting while asleep stops the listener; a preview while asleep opens the output
/// and it sleeps again afterwards; `idleSleepMinutes` 0 keeps it open. Silent (master volume 0).
fn idle_sleep_checks(report: &mut Report, bundled: &Path, scratch: &Path) {
    let keys = Arc::new(input::SyntheticKeys::default());
    let config = Config {
        version: "selftest".into(),
        settings: Settings {
            master_volume: 0.0,
            idle_sleep_minutes: IDLE_MINUTES,
            ..Settings::default()
        },
        settings_path: scratch.join("idle").join(settings::FILE_NAME),
        bundled_dir: Some(bundled.to_path_buf()),
        user_dir: Some(scratch.join("idle-user-packs")),
        listen: true,
        keys: KeySource::Synthetic(keys.clone()),
        idle_unit: IDLE_UNIT,
    };
    let idle_after = IDLE_UNIT * IDLE_MINUTES;
    let Some(service) =
        report.check("service: idle sleep closes the output, keeps listening", || {
            let service = Service::start(config, Box::new(|_, _| {})).map_err(|e| e.to_string())?;
            let ready = wait_for(WAIT, || {
                service.output_open()
                    && keys.listening()
                    && service.now_playing().as_deref() == Some(DEFAULT_PACK_ID)
            });
            if !ready {
                return Err(format!(
                    "output open {}, listening {}, playing {:?}",
                    service.output_open(),
                    keys.listening(),
                    service.now_playing()
                ));
            }
            keys.press(KeyAction::Down);
            keys.press(KeyAction::Up);
            let typed = Instant::now();
            if !wait_for(WAIT, || !service.output_open() && service.snapshot().audio_asleep) {
                return Err("the output stayed open without typing".into());
            }
            let slept = typed.elapsed();
            if slept < idle_after {
                return Err(format!("asleep after {} (idle time {})", ms(slept), ms(idle_after)));
            }
            if !keys.listening() || !service.snapshot().playing {
                return Err("asleep, but no longer listening or playing".into());
            }
            let detail = format!(
                "asleep {} after the last key (idle time {}), still listening",
                ms(slept),
                ms(idle_after)
            );
            Ok((service, detail))
        })
    else {
        return;
    };

    report.check("service: a key-down wakes the output and plays", || {
        let mut wake = Vec::new();
        let mut warm = Vec::new();
        for _ in 0..WAKE_CYCLES {
            if !wait_for(WAIT, || !service.output_open() && service.snapshot().audio_asleep) {
                return Err("the output did not go back to sleep".into());
            }
            service.reset_latency();
            let pressed = Instant::now();
            keys.press(KeyAction::Down);
            if !wait_for(WAIT, || service.output_open()) {
                return Err("a key press did not reopen the output".into());
            }
            let opened = pressed.elapsed();
            keys.press(KeyAction::Up);
            if !wait_for(WAIT, || service.last_latency().is_some()) {
                return Err(format!(
                    "the waking key press never played (reopened in {})",
                    ms(opened)
                ));
            }
            wake.extend(service.last_latency());
            // A second press while awake, for comparison.
            service.reset_latency();
            keys.press(KeyAction::Down);
            keys.press(KeyAction::Up);
            if wait_for(WAIT, || service.last_latency().is_some()) {
                warm.extend(service.last_latency());
            }
        }
        let summary = |samples: &[taktak_core::latency::LatencySample]| {
            let mut queue: Vec<f64> = samples.iter().map(|s| s.queue_ns as f64 / 1e6).collect();
            queue.sort_by(f64::total_cmp);
            let output = samples.first().map_or(0.0, |s| s.output_ns as f64 / 1e6);
            let median = queue.get(queue.len() / 2).copied().unwrap_or_default();
            let max = queue.last().copied().unwrap_or_default();
            (median, max, output)
        };
        let (wake_p50, wake_max, output) = summary(&wake);
        let (warm_p50, _, _) = summary(&warm);
        if service.snapshot().audio_asleep {
            return Err("still reported asleep with the output open".into());
        }
        Ok((
            (),
            format!(
                "{WAKE_CYCLES} wakes: key-down → first buffer p50 {wake_p50:.1} ms, max \
                 {wake_max:.1} ms (awake: {warm_p50:.1} ms), + output {output:.1} ms"
            ),
        ))
    });

    report.check("service: muting while asleep stops the listener", || {
        if !wait_for(WAIT, || service.snapshot().audio_asleep) {
            return Err("never asleep".into());
        }
        service.update(|s| s.muted = true);
        if !wait_for(WAIT, || !keys.listening() && !service.snapshot().audio_asleep) {
            return Err("still listening (or asleep) while muted".into());
        }
        if service.output_open() {
            return Err("muting opened the output".into());
        }
        service.update(|s| s.muted = false);
        if !wait_for(WAIT, || service.output_open() && keys.listening()) {
            return Err("unmuting did not reopen the output and the listener".into());
        }
        Ok(((), "listener off while muted; unmuting reopens both at once".into()))
    });

    report.check("service: a preview while asleep opens the output", || {
        if !wait_for(WAIT, || !service.output_open() && service.snapshot().audio_asleep) {
            return Err("never asleep".into());
        }
        service.preview(DEFAULT_PACK_ID)?.wait(WAIT)?;
        if !service.output_open() {
            return Err("the preview played without an open output".into());
        }
        let played = Instant::now();
        if !wait_for(WAIT, || !service.output_open()) {
            return Err("the output stayed open after the preview".into());
        }
        if !keys.listening() || !service.snapshot().audio_asleep {
            return Err("not asleep and listening after the preview".into());
        }
        Ok(((), format!("asleep again {} after the clip started", ms(played.elapsed()))))
    });

    report.check("service: idleSleepMinutes 0 never sleeps", || {
        service.update(|s| s.settings.idle_sleep_minutes = 0);
        if !wait_for(WAIT, || service.output_open() && !service.snapshot().audio_asleep) {
            return Err("turning idle sleep off did not reopen the output".into());
        }
        thread::sleep(idle_after * 2);
        if !service.output_open() {
            return Err("the output closed anyway".into());
        }
        Ok(((), format!("still open {} later", ms(idle_after * 2))))
    });
    service.shutdown(Duration::from_secs(3));
}

/// A preview opens the output (closed: nothing else can play without a key listener), plays
/// silently (master volume 0), and the output closes again when it is stopped and when a clip
/// has played to its end.
fn preview(service: &Service) -> Result<((), String), String> {
    let volume = service.settings().master_volume;
    service.update(|s| s.settings.master_volume = 0.0);
    let result = preview_cycles(service);
    service.update(|s| s.settings.master_volume = volume);
    result
}

fn preview_cycles(service: &Service) -> Result<((), String), String> {
    if service.output_open() {
        return Err("the output is open before the preview".into());
    }
    let started = Instant::now();
    service.preview(DEFAULT_PACK_ID)?.wait(WAIT)?;
    let opened = started.elapsed();
    if !service.output_open() {
        return Err("the preview reported playing without an open output".into());
    }
    service.stop_preview();
    if !wait_for(WAIT, || !service.output_open()) {
        return Err("the output stayed open after the preview was stopped".into());
    }
    service.preview(DEFAULT_PACK_ID)?.wait(WAIT)?;
    let played = Instant::now();
    if !wait_for(WAIT, || !service.output_open()) {
        return Err("the output stayed open after the clip ended".into());
    }
    Ok((
        (),
        format!(
            "playing after {}; closed when stopped, and {} after a full clip started",
            ms(opened),
            ms(played.elapsed())
        ),
    ))
}

/// A user pack appears, is selected, breaks on disk (its old bank keeps playing, and a fresh
/// start names it as broken rather than missing) and is deleted (fallback to the default pack).
fn hot_reload_checks(
    report: &mut Report,
    service: &Service,
    bundled: &Path,
    packs: &[PackInfo],
    user_dir: &Path,
    scratch: &Path,
    playback: bool,
) {
    let source = packs
        .iter()
        .find(|p| p.id == "vintage-keyboard")
        .or_else(|| packs.iter().find(|p| p.location.is_dir()));
    let Some(source) = source.filter(|p| p.location.is_dir()) else {
        report.skip("service: hot reload", "no folder pack to copy");
        return;
    };
    let pack_dir = user_dir.join(USER_PACK_ID);
    let added = report.check("service: hot reload lists a new user pack", || {
        let staging = scratch.join("staging").join(USER_PACK_ID);
        copy_dir(&source.location, &staging).map_err(|e| e.to_string())?;
        rewrite_manifest(&staging.join("pack.json"), |m| {
            m["id"] = USER_PACK_ID.into();
            m["name"] = "Self-test Pack".into();
        })?;
        fs::create_dir_all(user_dir).map_err(|e| e.to_string())?;
        fs::rename(&staging, &pack_dir).map_err(|e| e.to_string())?;
        let started = Instant::now();
        let listed = wait_for(WAIT, || {
            service
                .snapshot()
                .packs
                .iter()
                .any(|p| p.id == USER_PACK_ID && p.origin == PackOrigin::User)
        });
        if !listed {
            return Err("the copied pack never appeared".into());
        }
        Ok(((), format!("copy of {} listed after {}", source.id, ms(started.elapsed()))))
    });
    if added.is_none() {
        return;
    }
    if !playback {
        report.skip("service: broken and deleted user pack", "no audio output");
        return;
    }
    let selected = report.check("service: switch to the user pack", || {
        service.set_pack(USER_PACK_ID)?;
        if !wait_for(WAIT, || service.now_playing().as_deref() == Some(USER_PACK_ID)) {
            return Err(format!("playing {:?}", service.now_playing()));
        }
        Ok(((), "playing".into()))
    });
    if selected.is_none() {
        return;
    }
    report.check("service: broken active pack keeps its old bank", || {
        fs::write(pack_dir.join("pack.json"), "{ broken").map_err(|e| e.to_string())?;
        let kept = wait_for(WAIT, || {
            let state = service.snapshot();
            !state.invalid_packs.is_empty()
                && state.active_pack_error.is_some()
                && !state.packs.iter().any(|p| p.id == USER_PACK_ID)
        });
        let state = service.snapshot();
        if !kept {
            return Err(format!(
                "invalid {:?}, error {:?}",
                state.invalid_packs, state.active_pack_error
            ));
        }
        if service.now_playing().as_deref() != Some(USER_PACK_ID) {
            return Err(format!("switched to {:?}", service.now_playing()));
        }
        Ok(((), state.active_pack_error.unwrap_or_default()))
    });
    report.check("service: broken selected pack named as broken on start", || {
        let config = Config {
            version: "selftest".into(),
            settings: Settings { pack_id: USER_PACK_ID.into(), ..Settings::default() },
            settings_path: scratch.join("restart").join(settings::FILE_NAME),
            bundled_dir: Some(bundled.to_path_buf()),
            user_dir: Some(user_dir.to_path_buf()),
            listen: false,
            keys: KeySource::Os,
            idle_unit: idle::MINUTE,
        };
        let restarted = Service::start(config, Box::new(|_, _| {})).map_err(|e| e.to_string())?;
        let named = wait_for(WAIT, || {
            restarted.now_playing().as_deref() == Some(DEFAULT_PACK_ID)
                && restarted.snapshot().active_pack_error.is_some_and(|e| e.contains("has errors"))
        });
        let error = restarted.snapshot().active_pack_error;
        restarted.shutdown(Duration::from_secs(3));
        if !named {
            return Err(format!("playing {:?}, error {error:?}", restarted.now_playing()));
        }
        Ok(((), error.unwrap_or_default()))
    });
    report.check("service: deleted active pack falls back", || {
        fs::remove_dir_all(&pack_dir).map_err(|e| e.to_string())?;
        let fell_back = wait_for(WAIT, || {
            let state = service.snapshot();
            state.invalid_packs.is_empty()
                && service.now_playing().as_deref() == Some(DEFAULT_PACK_ID)
                && state.active_pack_error.as_deref().is_some_and(|e| e.contains("not installed"))
        });
        let state = service.snapshot();
        if !fell_back {
            return Err(format!(
                "playing {:?}, error {:?}, invalid {:?}",
                service.now_playing(),
                state.active_pack_error,
                state.invalid_packs
            ));
        }
        Ok(((), state.active_pack_error.unwrap_or_default()))
    });
}

/// A synthetic click, as a 16-bit mono WAV file: `frames` samples of decaying noise at `rate`.
pub fn click_wav(rate: u32, frames: u32) -> Vec<u8> {
    let data_len = frames * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    let mut seed: u32 = 0x9e37_79b9;
    for i in 0..frames {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let noise = (seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let envelope = (-(i as f32) / (rate as f32 * 0.004)).exp();
        out.extend_from_slice(&((noise * envelope * 20_000.0) as i16).to_le_bytes());
    }
    out
}

/// "Import Mechvibes pack…" end to end, without the picker: a synthetic Mechvibes pack is
/// imported into the service's user packs folder, hot reload lists it as a personal pack (whose
/// license note the UI's badge replaces), a second import is recognized, and an overwrite replaces it.
fn mechvibes_import(
    service: &Service,
    user_dir: &Path,
    scratch: &Path,
) -> Result<((), String), String> {
    const ID: &str = "mv-self-test-mechvibes";
    let src = scratch.join("mechvibes").join("Self-test Mechvibes");
    fs::create_dir_all(&src).map_err(|e| e.to_string())?;
    fs::write(
        src.join("config.json"),
        r#"{"name": "Self-test Mechvibes", "key_define_type": "multi", "defines": {"30": "a.wav", "57": "space.wav"}}"#,
    )
    .map_err(|e| e.to_string())?;
    fs::write(src.join("a.wav"), click_wav(44_100, 2_000)).map_err(|e| e.to_string())?;
    fs::write(src.join("space.wav"), click_wav(44_100, 3_000)).map_err(|e| e.to_string())?;

    let started = Instant::now();
    let pack = match mechvibes::import(&src, user_dir, false)? {
        MechvibesImport::Imported { pack } => pack,
        other => return Err(format!("first import: {other:?}")),
    };
    if pack.id != ID || pack.keys_mapped != 2 {
        return Err(format!("imported {pack:?}"));
    }
    let listed = wait_for(WAIT, || {
        service.snapshot().packs.iter().any(|p| {
            p.id == ID
                && p.origin == PackOrigin::User
                && p.license == PERSONAL_LICENSE
                && !p.warnings.iter().any(|w| w.starts_with("license"))
        })
    });
    if !listed {
        let found = service.snapshot().packs.into_iter().find(|p| p.id == ID);
        return Err(format!(
            "never listed as a personal pack without a license warning: {found:?}"
        ));
    }
    let listed_after = started.elapsed();
    match mechvibes::import(&src, user_dir, false)? {
        MechvibesImport::AlreadyImported { id, .. } if id == ID => {}
        other => return Err(format!("second import: {other:?}")),
    }
    let again = mechvibes::take_pending().ok_or("the overwrite has no source")?;
    match mechvibes::import(&again, user_dir, true)? {
        MechvibesImport::Imported { pack } if pack.replaced => {}
        other => return Err(format!("overwrite: {other:?}")),
    }
    fs::remove_dir_all(user_dir.join(ID)).map_err(|e| e.to_string())?;
    if !wait_for(WAIT, || !service.snapshot().packs.iter().any(|p| p.id == ID)) {
        return Err("the imported pack was not removed from the list".into());
    }
    Ok(((), format!("2 keys, listed after {}, replaced on overwrite", ms(listed_after))))
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

fn rewrite_manifest(path: &Path, edit: impl FnOnce(&mut serde_json::Value)) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut manifest: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    edit(&mut manifest);
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn options_parse() {
        assert_eq!(Options::parse(&args(&["--selftest"])).unwrap(), Options::default());
        let o =
            Options::parse(&args(&["--selftest", "--allow-no-audio", "--packs", "/p"])).unwrap();
        assert!(o.allow_no_audio);
        assert_eq!(o.packs, Some(PathBuf::from("/p")));
        assert!(Options::parse(&args(&["--selftest", "--packs"])).is_err());
        assert!(Options::parse(&args(&["--selftest", "--loud"])).is_err());
    }

    #[test]
    fn checks_that_need_no_device_pass() {
        let dir = tempfile::tempdir().unwrap();
        assert!(gate().is_ok());
        assert_eq!(app_rules().err(), None);
        assert_eq!(auto_mute().err(), None);
        assert_eq!(onboarding().err(), None);
        assert!(persist(dir.path()).is_ok());
        assert!(recover(dir.path()).is_ok());
        assert_eq!(migrate(dir.path()).err(), None);
        let repo_packs = catalog::bundled_dir(None).unwrap();
        let (packs, _) = scan(Some(&repo_packs)).unwrap();
        assert!(packs.iter().any(|p| p.id == DEFAULT_PACK_ID));
        let (bank, _) = load(&packs[0], 48_000).unwrap();
        assert!(!bank.samples.is_empty());
    }
}
