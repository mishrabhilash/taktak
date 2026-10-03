//! Pack maker: `record` (microphone + key listener) and `slice` (an existing WAV file) turn
//! keyboard recordings you have the right to use into a TakTak sound pack.
//!
//! Privacy: what was typed, and in which order or rhythm, is never printed or written. Output
//! is per-key samples, counts and a preview with a made-up rhythm. The live status line
//! (elapsed time, keystroke count, input level) is drawn only when stderr is a terminal, where
//! each refresh overwrites the last; redirected stderr gets none of it.

mod assemble;
mod capture;
mod cli;
mod onset;
mod output;
mod record;
mod signal;
mod slice;
mod takes;
#[cfg(test)]
mod testutil;
mod wav;

use cli::Args;
use output::PackMeta;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
pack-maker: make a TakTak sound pack from keyboard recordings you have the right to use.

USAGE
  pack-maker record --out DIR --id ID --name NAME --author AUTHOR --license SPDX
                    [--seconds N] [--takes N] [--device NAME] [PACK OPTIONS]
      Records your keyboard through a microphone while listening for key presses, then
      cuts one sample per key press and release.
        --seconds N     stop after N seconds (default: Ctrl+C or Escape 3 times quickly)
        --takes N       best takes kept per key and action, 1-10 (default 3); by default
                        the app plays one of them per key, the rest with random variants on
        --device NAME   input device (default: the system microphone)

  pack-maker slice --input FILE.wav --out DIR --id ID --name NAME --author AUTHOR
                   --license SPDX [--threshold-db DB] [--min-gap-ms MS] [PACK OPTIONS]
      Finds keystrokes in an existing recording, tells presses from releases, and keeps
      the most consistent ones as one pool used for every key.
        --threshold-db DB   ignore sounds quieter than this, dBFS (default -40)
        --min-gap-ms MS     sounds closer than this are one keystroke (default 25)

PACK OPTIONS
  --version TEXT        pack version (default 1.0.0)
  --description TEXT    one or two sentences about the keyboard
  --attribution TEXT    credit line (required by CC-BY licenses; defaults to NAME by AUTHOR)
  --source TEXT         where the recordings came from
  --force               write into a non-empty DIR (files with the same names are replaced)

LICENSES
  CC0-1.0 CC-BY-3.0 CC-BY-4.0 MIT 0BSD Unlicense Apache-2.0 BSD-2-Clause BSD-3-Clause ISC,
  or LicenseRef-Personal for a pack you keep to yourself.

Only record keyboards and use recordings you have the right to share under that license.
";

const META_OPTIONS: [&str; 9] = [
    "--out",
    "--id",
    "--name",
    "--author",
    "--license",
    "--version",
    "--description",
    "--attribution",
    "--source",
];

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let rest: Vec<String> = args.collect();
    if rest.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let result = match command.as_deref() {
        Some("record") => run_record(rest),
        Some("slice") => run_slice(rest),
        None | Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(format!("unknown command {other:?} (expected record or slice)")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn options(extra: &[&'static str]) -> Vec<&'static str> {
    META_OPTIONS.iter().chain(extra).copied().collect()
}

fn pack_meta(a: &Args, default_source: &str) -> Result<PackMeta, String> {
    let mut meta = PackMeta {
        id: a.req("--id")?.to_owned(),
        name: a.req("--name")?.to_owned(),
        author: a.req("--author")?.to_owned(),
        license: a.req("--license")?.to_owned(),
        version: a.opt("--version").unwrap_or("1.0.0").to_owned(),
        description: a.opt("--description").map(str::to_owned),
        attribution: a.opt("--attribution").map(str::to_owned),
        source: a.opt("--source").unwrap_or(default_source).to_owned(),
    };
    for note in meta.check()? {
        eprintln!("note: {note}");
    }
    Ok(meta)
}

fn run_record(args: Vec<String>) -> Result<(), String> {
    let a = Args::parse(args, &options(&["--seconds", "--takes", "--device"]), &["--force"])?;
    let meta = pack_meta(&a, "Recorded with TakTak pack-maker")?;
    let out = PathBuf::from(a.req("--out")?);
    let force = a.flag("--force");
    output::check_out_dir(&out, force)?;
    let seconds = a.num::<f64>("--seconds")?;
    if seconds.is_some_and(|s| !(s > 0.0 && s <= capture::MAX_SECONDS)) {
        return Err(format!("--seconds must be between 0 and {}", capture::MAX_SECONDS));
    }
    let takes = a.num::<usize>("--takes")?.unwrap_or(3);
    if !(1..=10).contains(&takes) {
        return Err("--takes must be between 1 and 10".into());
    }

    let opts = capture::CaptureOptions { seconds, device: a.opt("--device").map(str::to_owned) };
    let captured = capture::record(&opts)?;
    eprintln!(
        "processing {:.1} s of audio...",
        captured.audio.samples.len() as f64 / captured.audio.rate as f64
    );
    let record_opts = record::RecordOptions { takes, ..record::RecordOptions::default() };
    let result = record::process(&captured.audio, &captured.marks, &record_opts);
    // The key events are not needed past this point.
    drop(captured);
    let (built, s) = result?;

    eprintln!(
        "{} key presses and {} releases captured; {} matched to a sound, {} without a clear \
         sound, {} too close to another key, {} clipped, {} too quiet, {} too short",
        s.presses,
        s.releases,
        s.matched,
        s.no_onset,
        s.overlapping,
        s.clipped,
        s.too_quiet,
        s.too_short
    );
    eprintln!(
        "key-to-sound offset {:.1} ms ({})",
        s.latency_ms,
        if s.latency_measured { "measured" } else { "assumed" }
    );
    eprintln!("{} keys: {} press and {} release samples", s.keys, s.press_files, s.release_files);
    finish(&out, &meta, &built, force)
}

fn run_slice(args: Vec<String>) -> Result<(), String> {
    let a =
        Args::parse(args, &options(&["--input", "--threshold-db", "--min-gap-ms"]), &["--force"])?;
    let meta = pack_meta(&a, "Sliced with TakTak pack-maker")?;
    let out = PathBuf::from(a.req("--out")?);
    let force = a.flag("--force");
    output::check_out_dir(&out, force)?;
    let mut opts = slice::SliceOptions::default();
    if let Some(db) = a.num::<f32>("--threshold-db")? {
        if !(-90.0..=0.0).contains(&db) {
            return Err("--threshold-db must be between -90 and 0".into());
        }
        opts.detector.threshold_db = db;
    }
    if let Some(ms) = a.num::<f64>("--min-gap-ms")? {
        if !(1.0..=1000.0).contains(&ms) {
            return Err("--min-gap-ms must be between 1 and 1000".into());
        }
        opts.detector.min_gap_s = ms / 1e3;
    }

    let audio = wav::read(Path::new(a.req("--input")?))?;
    let (built, s) = slice::process(&audio, &opts)?;
    eprintln!(
        "{} keystroke sounds: {} presses, {} releases ({} clipped, {} too quiet, {} too short)",
        s.onsets, s.presses, s.releases, s.clipped, s.too_quiet, s.too_short
    );
    eprintln!("kept {} press and {} release variants", s.press_files, s.release_files);
    finish(&out, &meta, &built, force)
}

fn finish(
    out: &Path,
    meta: &PackMeta,
    built: &assemble::BuiltPack,
    force: bool,
) -> Result<(), String> {
    eprintln!(
        "gain {:+.1} dB, keystrokes {:.0} dB above the background noise",
        built.gain_db, built.snr_db
    );
    for note in &built.notes {
        eprintln!("warning: {note}");
    }
    let written = output::write(out, meta, built, force)?;
    if written.stale > 0 {
        eprintln!(
            "note: sounds/ also holds {} older files this pack does not use; delete them before \
             sharing the pack",
            written.stale
        );
    }
    println!("wrote {} files to {}", written.files, out.display());
    println!("try it: copy the folder into TakTak's packs folder (see docs/pack-format.md)");
    Ok(())
}
