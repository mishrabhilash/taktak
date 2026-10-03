//! Engine spike: global key listener → audio engine (a sound pack, or synthesized press and
//! release clicks), with live latency reporting.
//!
//!   taktak-spike                 type anywhere; prints latency every 25 presses
//!   taktak-spike --buffer 128    request a 128-frame audio buffer (default 64; 0 = device default)
//!   taktak-spike --synthetic 40  no keyboard needed: fires 40 timed press/release pairs and
//!                                measures the queue + output stages only
//!   taktak-spike --pack DIR|ZIP  play a sound pack (press and release) instead of the
//!                                synthesized clicks; combines with --synthetic
//!   taktak-spike --pack DIR|ZIP --preview
//!                                play the pack's preview clip once and exit
//!   taktak-spike --random-variants
//!                                pick a random sample per keystroke instead of keeping one
//!                                per key
//!   taktak-spike --humanize 0.5  share of the pack's pitch and volume variation each
//!                                keystroke gets, 0 to 1 (default 0.25)
//!   taktak-spike --check-permission
//!
//! Output never contains key identities, only timings (and the pack's own metadata).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use taktak_core::audio::{DEFAULT_HUMANIZE, Engine, EngineConfig, SoundBank, Trigger, VariantMode};
use taktak_core::input::{self, KeyAction};
use taktak_core::key::Key;
use taktak_core::latency::{LatencySample, Report};
use taktak_core::pack::{self, LoadedPack, PackError, PackOrigin};
use taktak_core::{clock, synth};

struct Args {
    buffer: Option<u32>,
    report_every: usize,
    synthetic: Option<usize>,
    check_permission: bool,
    pack: Option<PathBuf>,
    preview: bool,
    variant_mode: VariantMode,
    humanize: f32,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        buffer: Some(64),
        report_every: 25,
        synthetic: None,
        check_permission: false,
        pack: None,
        preview: false,
        variant_mode: VariantMode::Consistent,
        humanize: DEFAULT_HUMANIZE,
    };
    let mut it = std::env::args_os().skip(1);
    while let Some(arg) = it.next() {
        let mut num = |name: &str| -> Result<usize, String> {
            it.next()
                .and_then(|v| v.to_str()?.parse().ok())
                .ok_or_else(|| format!("{name} needs a number"))
        };
        match arg.to_str().unwrap_or_default() {
            "--buffer" => a.buffer = Some(num("--buffer")? as u32).filter(|&b| b > 0),
            "--report-every" => a.report_every = num("--report-every")?.max(1),
            "--synthetic" => a.synthetic = Some(num("--synthetic")?),
            "--check-permission" => a.check_permission = true,
            // Taken as given, so paths that are not valid UTF-8 still work.
            "--pack" => {
                let path = it.next().ok_or("--pack needs a pack folder or .zip")?;
                a.pack = Some(PathBuf::from(path));
            }
            "--preview" => a.preview = true,
            "--random-variants" => a.variant_mode = VariantMode::Random,
            "--humanize" => {
                a.humanize = it
                    .next()
                    .and_then(|v| v.to_str()?.parse().ok())
                    .filter(|h| (0.0..=1.0).contains(h))
                    .ok_or("--humanize needs a number from 0 to 1")?;
            }
            "-h" | "--help" => {
                println!(
                    "usage: taktak-spike [--buffer N] [--report-every N] [--synthetic N] \
                     [--pack DIR|ZIP [--preview]] [--random-variants] [--humanize 0..1] \
                     [--check-permission]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument {}", arg.to_string_lossy())),
        }
    }
    if a.preview && a.pack.is_none() {
        return Err("--preview needs --pack".into());
    }
    Ok(a)
}

struct StderrLogger;
impl log::Log for StderrLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{}] {}", r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Ctrl+C ends the run with a final summary instead of killing the process mid-report.
#[cfg(unix)]
fn install_sigint_handler() {
    extern "C" fn on_sigint(_: i32) {
        INTERRUPTED.store(true, Ordering::Relaxed);
    }
    unsafe extern "C" {
        fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
    }
    const SIGINT: i32 = 2;
    unsafe { signal(SIGINT, on_sigint) };
}

#[cfg(not(unix))]
fn install_sigint_handler() {}

fn main() {
    let _ = log::set_logger(&StderrLogger).map(|()| log::set_max_level(log::LevelFilter::Info));
    let args = parse_args().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(64);
    });

    if args.check_permission {
        println!("input monitoring granted: {}", input::has_permission());
        return;
    }

    let needs_keyboard = args.synthetic.is_none() && !args.preview;
    if needs_keyboard && !input::has_permission() && !input::request_permission() {
        eprintln!(
            "TakTak needs Input Monitoring permission to hear your keystrokes (key codes only;\n\
             nothing you type is stored or sent anywhere).\n\n\
             Enable it for the app you're running this from (Terminal, iTerm, VS Code, …):\n  \
             open \"x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent\"\n\
             then quit and reopen that app and run this again."
        );
        std::process::exit(2);
    }

    let config = EngineConfig {
        buffer_frames: args.buffer,
        measure_latency: true,
        variant_mode: args.variant_mode,
        humanize: args.humanize,
    };
    // The pack is loaded inside `start` so that it is decoded once, at the device rate.
    let mut loaded: Option<Result<LoadedPack, PackError>> = None;
    let (mut engine, mut triggers) = Engine::start(config, |rate| match &args.pack {
        Some(path) => {
            let mut result = pack::load(path, PackOrigin::User, rate);
            let bank = result.as_mut().map(|l| std::mem::take(&mut l.bank)).unwrap_or_default();
            loaded = Some(result);
            bank
        }
        None => SoundBank::uniform(
            synth::click(&synth::ClickParams::PRESS, rate, 0.8),
            Some(synth::click(&synth::ClickParams::RELEASE, rate, 0.5)),
        ),
    })
    .unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let loaded = loaded.transpose().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let info = engine.info().clone();
    let period_ms = info.buffer_frames.map(|f| f as f64 * 1000.0 / info.sample_rate as f64);
    println!(
        "output: {} — {} Hz, {} ch, buffer {}",
        info.name,
        info.sample_rate,
        info.channels,
        match (info.buffer_frames, period_ms) {
            (Some(f), Some(ms)) => format!("{f} frames ({ms:.2} ms)"),
            _ => "device default".into(),
        }
    );
    println!(
        "variants: {}, humanize {:.2} of the pack's pitch and volume variation",
        match args.variant_mode {
            VariantMode::Consistent => "consistent (one sample per key)",
            VariantMode::Random => "random (per keystroke)",
        },
        args.humanize
    );
    if let Some(pack) = &loaded {
        println!(
            "pack: {} ({}), {} warning{}",
            pack.info.name,
            pack.info.id,
            pack.warnings.len(),
            if pack.warnings.len() == 1 { "" } else { "s" }
        );
        for warning in &pack.warnings {
            println!("  {warning}");
        }
    }
    if args.preview {
        let Some(clip) = loaded.and_then(|pack| pack.preview) else {
            eprintln!("this pack has no preview clip");
            std::process::exit(1);
        };
        let length = Duration::from_secs_f64(clip.len() as f64 / info.sample_rate as f64);
        println!("playing the preview ({:.2} s)", length.as_secs_f64());
        if engine.preview(clip).is_err() {
            eprintln!("the audio engine is not responding");
            std::process::exit(1);
        }
        // Plus the output latency, so the tail is not cut off when the stream closes.
        std::thread::sleep(length + Duration::from_millis(250));
        return;
    }
    let mut metrics = engine.take_metrics().expect("metrics enabled");

    let mut listener = None;
    let target = match args.synthetic {
        Some(n) => {
            println!(
                "synthetic mode: {n} presses, 120 ms apart, release 60 ms after each press \
                 (input stage not measured)"
            );
            std::thread::spawn(move || {
                // Skip the device's start-up period; in the app the stream is long since running.
                std::thread::sleep(Duration::from_millis(500));
                for action in [KeyAction::Down, KeyAction::Up].into_iter().cycle().take(2 * n) {
                    let now = clock::now_ns();
                    triggers.send(Trigger {
                        key: Key::KeyA,
                        action,
                        event_ns: now,
                        received_ns: now,
                    });
                    std::thread::sleep(Duration::from_millis(60));
                }
            });
            n
        }
        None => {
            // Presses and releases both play; only presses are measured.
            listener = Some(
                input::start(move |ev| {
                    triggers.send(Trigger::from(ev));
                })
                .unwrap_or_else(|e| {
                    eprintln!("{e}");
                    std::process::exit(2);
                }),
            );
            println!("listening — type anywhere. Ctrl+C to quit.");
            usize::MAX
        }
    };

    install_sigint_handler();
    let mut window: Vec<LatencySample> = Vec::new();
    let mut all: Vec<LatencySample> = Vec::new();
    let mut last_seen = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(50));
        // The audio and hook threads only count these; reporting them is our job.
        let xruns = engine.take_xruns();
        if xruns > 0 {
            log::warn!("audio: {xruns} buffer underrun{}", if xruns == 1 { "" } else { "s" });
        }
        if let Some(n) = listener.as_ref().map(input::Listener::take_reenabled).filter(|&n| n > 0) {
            log::warn!("the system disabled the keyboard hook {n} time(s); re-enabled it");
        }
        if let Some(fault) = engine.take_stream_fault() {
            // The app rebuilds the engine here; the spike just stops.
            eprintln!("audio output stopped working ({fault:?}); run again to reopen the device");
            break;
        }
        while let Ok(s) = metrics.pop() {
            window.push(s);
            all.push(s);
            last_seen = Instant::now();
        }
        let done = all.len() >= target
            || INTERRUPTED.load(Ordering::Relaxed)
            || (args.synthetic.is_some() && last_seen.elapsed() > Duration::from_secs(2));
        if window.len() >= args.report_every || (done && !window.is_empty()) {
            println!("\n{}", Report::from_samples(&window));
            window.clear();
        }
        if done {
            break;
        }
    }
    if all.len() > args.report_every {
        println!("\noverall:\n{}", Report::from_samples(&all));
    }
    // Let the last sound finish before tearing the stream down.
    std::thread::sleep(Duration::from_millis(150));
}
