//! Imports Mechvibes, Mechvibes++ and MechvibesDX sound packs into TakTak packs.
//!
//!   taktak-import-mechvibes <folder-or-zip>... [--out DIR] [--overwrite] [--no-split-release]
//!
//! Imported packs are for personal use only (`license: "LicenseRef-Personal"`): Mechvibes
//! packs carry no license, so TakTak never bundles or shares them. `--out` defaults to the
//! app's user pack folder, where the running app picks new packs up within a second.
//!
//! Exit status: 0 when every pack was imported, 1 when one failed, 64 for a usage error. Only
//! pack contents (file names, key names from the config) are printed; nothing is ever read
//! from the keyboard.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use taktak_core::pack::import::{ImportOptions, import_mechvibes};
use taktak_core::pack::{printable, registry};

const USAGE: &str = "usage:
  taktak-import-mechvibes <folder-or-zip>... [--out DIR] [--overwrite] [--no-split-release]

Each argument is a Mechvibes pack folder (containing config.json) or a .zip of one.
--out DIR            where to write the TakTak packs (default: the app's user pack folder)
--overwrite          replace an earlier import of the same pack
--no-split-release   keep whole-keystroke sprite slices as press sounds (no release split)

Imported packs are for personal use only and are marked LicenseRef-Personal.";

#[derive(Debug, PartialEq)]
struct Cli {
    sources: Vec<PathBuf>,
    out: Option<PathBuf>,
    options: ImportOptions,
}

enum Parsed {
    Run(Cli),
    Help,
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Parsed, String> {
    let mut sources = Vec::new();
    let mut out = None;
    let mut options = ImportOptions::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(Parsed::Help),
            Some("--overwrite") => options.overwrite = true,
            Some("--no-split-release") => options.split_release = false,
            Some("--out") => {
                let dir = args.next().ok_or("--out needs a folder")?;
                if out.replace(PathBuf::from(dir)).is_some() {
                    return Err("--out given more than once".into());
                }
            }
            Some(s) if s.starts_with("--out=") => {
                if out.replace(PathBuf::from(&s["--out=".len()..])).is_some() {
                    return Err("--out given more than once".into());
                }
            }
            Some(s) if s.starts_with('-') => return Err(format!("unknown option {s}")),
            _ => sources.push(PathBuf::from(arg)),
        }
    }
    if sources.is_empty() {
        return Err("no pack given".into());
    }
    Ok(Parsed::Run(Cli { sources, out, options }))
}

fn main() -> ExitCode {
    let cli = match parse(std::env::args_os().skip(1)) {
        Ok(Parsed::Run(cli)) => cli,
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(64);
        }
    };
    let Some(out) = cli.out.or_else(registry::default_user_dir) else {
        eprintln!("error: no user pack folder on this system; pass --out DIR");
        return ExitCode::from(64);
    };
    let mut failed = 0;
    for src in &cli.sources {
        match import_mechvibes(src, &out, cli.options) {
            Ok(report) => print!("{report}"),
            Err(e) => {
                failed += 1;
                println!("{}: failed: {e}", printable(&src.display().to_string()));
            }
        }
    }
    let total = cli.sources.len();
    println!(
        "{} of {total} pack{} imported into {}{}",
        total - failed,
        if total == 1 { "" } else { "s" },
        printable(&out.display().to_string()),
        if failed == 0 { "" } else { " (see the failures above)" }
    );
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_sources_and_options() {
        let Ok(Parsed::Run(cli)) =
            parse(args(&["a", "b.zip", "--out", "dir", "--overwrite", "--no-split-release"]))
        else {
            panic!("expected a run");
        };
        assert_eq!(cli.sources, [PathBuf::from("a"), PathBuf::from("b.zip")]);
        assert_eq!(cli.out, Some(PathBuf::from("dir")));
        assert!(cli.options.overwrite && !cli.options.split_release);

        let Ok(Parsed::Run(cli)) = parse(args(&["--out=x", "p"])) else { panic!() };
        assert_eq!(cli.out, Some(PathBuf::from("x")));
        assert_eq!(cli.options, ImportOptions::default());
    }

    #[test]
    fn usage_errors() {
        assert!(parse(args(&[])).is_err());
        assert!(parse(args(&["--out"])).is_err());
        assert!(parse(args(&["p", "--bogus"])).is_err());
        assert!(parse(args(&["p", "--out", "a", "--out", "b"])).is_err());
        assert!(matches!(parse(args(&["--help"])), Ok(Parsed::Help)));
    }
}
