//! Sound pack tool for pack authors and CI (see `docs/pack-format.md`).
//!
//!   taktak-pack validate <path>... [--strict]   check packs, decoding every sound at 48 kHz
//!   taktak-pack info <path>                     metadata, sound counts, memory, load time
//!   taktak-pack list [--bundled DIR] [--user DIR]
//!                                               what the app would find in those folders
//!
//! A path is a pack folder or a `.zip`. Exit status: 0 when every pack is valid (for `list`:
//! every candidate found), 1 when one has errors, 64 for a usage error. Only pack contents are
//! printed (key names here come from pack.json), with control characters escaped; nothing is
//! ever read from the keyboard.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};
use taktak_core::input::KeyAction;
use taktak_core::key::Key;
use taktak_core::pack::{
    self, LoadedPack, Manifest, PackError, PackInfo, PackOrigin, PackRegistry, Problem, manifest,
    printable, registry,
};

/// Validation and statistics use one fixed rate, so results do not depend on the machine.
const RATE: u32 = 48_000;

const USAGE: &str = "usage:
  taktak-pack validate <path>... [--strict]
  taktak-pack info <path>
  taktak-pack list [--bundled DIR] [--user DIR]

<path> is a pack folder (containing pack.json) or a .zip.
--strict rejects packs that cannot be bundled or shared (LicenseRef-Personal).
list scans like the app does; --user defaults to the app's user pack folder.";

enum Command {
    Validate { paths: Vec<PathBuf>, strict: bool },
    Info(PathBuf),
    List { bundled: Option<PathBuf>, user: Option<PathBuf> },
    Help,
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else { return Err("missing command".into()) };
    let mut positional = Vec::new();
    let mut strict = false;
    let (mut bundled, mut user) = (None, None);
    while let Some(arg) = args.next() {
        let mut dir = |name: &str, slot: &mut Option<PathBuf>| match args.next() {
            Some(value) if slot.is_none() => {
                *slot = Some(PathBuf::from(value));
                Ok(())
            }
            Some(_) => Err(format!("{name} given more than once")),
            None => Err(format!("{name} needs a folder")),
        };
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(Command::Help),
            Some("--strict") if command == "validate" => strict = true,
            Some("--bundled") if command == "list" => dir("--bundled", &mut bundled)?,
            Some("--user") if command == "list" => dir("--user", &mut user)?,
            Some(option) if option.starts_with('-') => {
                return Err(format!("unknown option {option}"));
            }
            _ => positional.push(PathBuf::from(arg)),
        }
    }
    match command.to_str() {
        Some("-h" | "--help" | "help") => Ok(Command::Help),
        Some("validate") if positional.is_empty() => Err("validate needs at least one path".into()),
        Some("validate") => Ok(Command::Validate { paths: positional, strict }),
        Some("info") => match <[PathBuf; 1]>::try_from(positional) {
            Ok([path]) => Ok(Command::Info(path)),
            Err(_) => Err("info takes exactly one path".into()),
        },
        Some("list") if positional.is_empty() => Ok(Command::List { bundled, user }),
        Some("list") => Err("list takes no paths; use --bundled DIR or --user DIR".into()),
        _ => Err(format!("unknown command {:?}", command.to_string_lossy())),
    }
}

fn main() -> ExitCode {
    match parse_args(std::env::args_os().skip(1)) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Validate { paths, strict }) => validate(&paths, strict),
        Ok(Command::Info(path)) => info(&path),
        Ok(Command::List { bundled, user }) => list(bundled, user),
        Err(e) => {
            eprintln!("taktak-pack: {e}\n\n{USAGE}");
            ExitCode::from(64)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// validate

fn validate(paths: &[PathBuf], strict: bool) -> ExitCode {
    let mut failed = 0;
    for path in paths {
        match check(path, strict) {
            Ok((loaded, files)) => {
                let warnings = &loaded.warnings;
                println!(
                    "ok    {}: {} {:?}, {}, {}, {}",
                    path.display(),
                    loaded.info.id,
                    loaded.info.name,
                    loaded.info.license,
                    plural(files, "file"),
                    plural(warnings.len(), "warning")
                );
                print_problems(warnings);
            }
            Err(e) => {
                failed += 1;
                println!("FAIL  {e}");
            }
        }
    }
    if paths.len() > 1 {
        println!("\n{}: {} ok, {failed} failed", plural(paths.len(), "pack"), paths.len() - failed);
    }
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Validation (strict or not) plus a full load, which is what finds undecodable or overlong
/// audio; problems in pack.json and in the audio are reported together. Returns the loaded
/// pack and its number of distinct audio files.
fn check(path: &Path, strict: bool) -> Result<(LoadedPack, usize), PackError> {
    let loaded = pack::load::check_all(path, strict, RATE)?;
    let files = manifest::referenced_files(&pack::load::read_manifest(path)?).len();
    Ok((loaded, files))
}

// ---------------------------------------------------------------------------------------------
// info

fn info(path: &Path) -> ExitCode {
    let started = Instant::now();
    let loaded = match pack::load(path, PackOrigin::User, RATE) {
        Ok(loaded) => loaded,
        Err(e) => {
            println!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let took = started.elapsed();
    let m = match pack::load::read_manifest(path) {
        Ok(m) => m,
        Err(e) => {
            println!("{e}");
            return ExitCode::FAILURE;
        }
    };
    print_info(&loaded.info, &m);
    print_counts(&m);
    print_stats(&loaded, &m, took);
    println!("\n{}", plural(loaded.warnings.len(), "warning"));
    print_problems(&loaded.warnings);
    ExitCode::SUCCESS
}

fn print_info(info: &PackInfo, m: &Manifest) {
    let none = "—";
    let kind = if Path::new(&info.location).is_dir() { "folder" } else { "zip" };
    // Validation rejects control characters in these; escaping is the backstop.
    println!("{} ({})", printable(&info.name), info.id);
    let rows = [
        ("version", info.version.as_deref().unwrap_or(none).to_owned()),
        ("author", info.author.clone()),
        ("license", info.license.clone()),
        ("attribution", info.attribution.as_deref().unwrap_or(none).to_owned()),
        ("description", info.description.as_deref().unwrap_or(none).to_owned()),
        ("source", info.source.as_deref().unwrap_or(none).to_owned()),
        ("location", format!("{} ({kind})", info.location.display())),
        ("preview", manifest::preview_file(m).unwrap_or(none).to_owned()),
    ];
    for (label, value) in rows {
        println!("  {label:<12} {}", printable(&value));
    }
    let variation = m.variation.unwrap_or_default();
    println!(
        "  {:<12} volume {:.2}, trim_silence {}, variation pitch ±{:.1} %, volume ±{:.1} %",
        "settings",
        m.volume.unwrap_or(1.0),
        if m.trim_silence.unwrap_or(true) { "on" } else { "off" },
        100.0 * variation.pitch.unwrap_or(manifest::DEFAULT_PITCH_VARIATION),
        100.0 * variation.volume.unwrap_or(manifest::DEFAULT_VOLUME_VARIATION),
    );
}

/// Files listed per group and per key, as written in pack.json.
fn print_counts(m: &Manifest) {
    for (title, sets) in [("groups", &m.groups), ("keys", &m.keys)] {
        if sets.is_empty() {
            continue;
        }
        println!("\n  {title:<16} press  release");
        for (name, set) in sets {
            let name = printable(name);
            println!("    {name:<14} {:>5}  {:>7}", set.press.len(), set.release.len());
        }
    }
}

fn print_stats(loaded: &LoadedPack, m: &Manifest, took: Duration) {
    let bank = &loaded.bank;
    let covered =
        |action| Key::ALL.iter().filter(|&&k| !bank.map.get(k, action).is_empty()).count();
    let samples: usize = bank.samples.iter().map(|s| s.len()).sum::<usize>()
        + loaded.preview.as_ref().map_or(0, |p| p.len());
    let seconds = samples as f64 / f64::from(RATE);
    let megabytes = (samples * size_of::<f32>()) as f64 / (1024.0 * 1024.0);
    println!();
    println!(
        "  {:<12} {}/{n} keys sound on press, {}/{n} on release",
        "coverage",
        covered(KeyAction::Down),
        covered(KeyAction::Up),
        n = Key::COUNT
    );
    println!(
        "  {:<12} {} distinct, {seconds:.2} s of audio at {} kHz (preview included)",
        "files",
        manifest::referenced_files(m).len(),
        RATE / 1000
    );
    println!("  {:<12} {megabytes:.1} MB as f32 at {} kHz", "memory", RATE / 1000);
    println!("  {:<12} {:.0} ms", "load time", took.as_secs_f64() * 1000.0);
}

// ---------------------------------------------------------------------------------------------
// list

fn list(bundled: Option<PathBuf>, user: Option<PathBuf>) -> ExitCode {
    let user = user.or_else(registry::default_user_dir);
    let show = |dir: &Option<PathBuf>| {
        dir.as_ref().map_or("(none)".into(), |d| printable(&d.display().to_string()).into_owned())
    };
    println!("bundled: {}", show(&bundled));
    println!("user:    {}", show(&user));

    let mut registry = PackRegistry::new(bundled, user);
    registry.scan();
    let entries = registry.entries();
    let visible = registry.packs();

    println!("\npacks ({}):", visible.len());
    for info in &visible {
        let warnings = entries
            .iter()
            .find(|e| e.location == info.location)
            .and_then(|e| e.status.as_ref().ok())
            .map_or(0, |(_, warnings)| warnings.len());
        println!(
            "  {:<24} {:<26} {:<7} {}{}",
            info.id,
            format!("{:?}", info.name),
            origin(info.origin),
            printable(&info.location.display().to_string()),
            if warnings > 0 {
                format!("  ({})", plural(warnings, "warning"))
            } else {
                String::new()
            }
        );
    }

    let hidden: Vec<_> = entries
        .iter()
        .filter_map(|e| e.status.as_ref().ok().map(|(info, _)| (e, info)))
        .filter(|(e, _)| !visible.iter().any(|v| v.location == e.location))
        .collect();
    if !hidden.is_empty() {
        println!("\noverridden by a user pack:");
        for (entry, info) in hidden {
            println!("  {:<24} {}", info.id, printable(&entry.location.display().to_string()));
        }
    }

    let invalid: Vec<&PackError> = entries.iter().filter_map(|e| e.status.as_ref().err()).collect();
    if invalid.is_empty() {
        return ExitCode::SUCCESS;
    }
    println!("\ninvalid ({}):", invalid.len());
    for e in &invalid {
        println!("  {}", e.to_string().replace('\n', "\n  "));
    }
    ExitCode::FAILURE
}

fn origin(origin: PackOrigin) -> &'static str {
    match origin {
        PackOrigin::Bundled => "bundled",
        PackOrigin::User => "user",
    }
}

// ---------------------------------------------------------------------------------------------

fn print_problems(problems: &[Problem]) {
    for p in problems {
        println!("  {p}");
    }
}

fn plural(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_commands() {
        let Ok(Command::Validate { paths, strict }) =
            parse(&["validate", "a", "--strict", "b.zip"])
        else {
            panic!("validate")
        };
        assert_eq!(paths, [PathBuf::from("a"), PathBuf::from("b.zip")]);
        assert!(strict);
        assert!(matches!(parse(&["info", "a"]), Ok(Command::Info(p)) if p == Path::new("a")));
        let Ok(Command::List { bundled, user }) = parse(&["list", "--user", "u"]) else {
            panic!("list")
        };
        assert_eq!((bundled, user), (None, Some(PathBuf::from("u"))));
        assert!(matches!(parse(&["validate", "--help"]), Ok(Command::Help)));
        assert!(matches!(parse(&["help"]), Ok(Command::Help)));
    }

    #[test]
    fn rejects_bad_usage() {
        for (args, needle) in [
            (&[][..], "missing command"),
            (&["validate"], "at least one path"),
            (&["info"], "exactly one path"),
            (&["info", "a", "b"], "exactly one path"),
            (&["info", "a", "--strict"], "unknown option --strict"),
            (&["list", "x"], "takes no paths"),
            (&["list", "--user"], "needs a folder"),
            (&["list", "--user", "a", "--user", "b"], "more than once"),
            (&["frobnicate"], "unknown command"),
        ] {
            let err = parse(args).err().unwrap_or_else(|| panic!("{args:?} parsed"));
            assert!(err.contains(needle), "{args:?}: {err}");
        }
    }
}
