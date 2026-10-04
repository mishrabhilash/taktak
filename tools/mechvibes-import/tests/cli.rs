//! The `taktak-import-mechvibes` binary: report, exit status, `--out`, `--overwrite`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use taktak_core::pack::import::audio::wav_bytes;

fn run(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_taktak-import-mechvibes")).args(args).output().unwrap()
}

fn click(len: usize) -> Vec<u8> {
    let samples: Vec<f32> = (0..len)
        .map(|i| {
            let t = i as f32 / 48_000.0;
            0.8 * (std::f32::consts::TAU * 2000.0 * t).cos() * (-t / 0.02).exp()
        })
        .collect();
    wav_bytes(&samples, 48_000)
}

fn pack(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("config.json"),
        r#"{"name": "CLI Pack", "key_define_type": "multi",
            "defines": {"30": "a.wav", "57": "space.wav", "1": "gone.wav", "91,91,92": "a.wav"}}"#,
    )
    .unwrap();
    fs::write(dir.join("a.wav"), click(2_000)).unwrap();
    fs::write(dir.join("space.wav"), click(3_000)).unwrap();
}

#[test]
fn imports_reports_and_refuses_to_overwrite_silently() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("cli-pack");
    let out = tmp.path().join("packs");
    pack(&src);

    let first = run(&[src.as_os_str(), "--out".as_ref(), out.as_os_str()]);
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(first.status.success(), "{stdout}");
    assert!(stdout.contains("cli-pack -> mv-cli-pack"), "{stdout}");
    assert!(stdout.contains("2 mapped"), "{stdout}");
    assert!(stdout.contains("missing   \"gone.wav\""), "{stdout}");
    assert!(stdout.contains("skipped   \"91,91,92\""), "{stdout}");
    assert!(stdout.contains("LicenseRef-Personal"), "{stdout}");
    assert!(stdout.contains("1 of 1 pack imported"), "{stdout}");
    assert!(out.join("mv-cli-pack/pack.json").is_file());
    assert!(out.join("mv-cli-pack/preview.wav").is_file());

    let again = run(&[src.as_os_str(), "--out".as_ref(), out.as_os_str()]);
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stdout).contains("already imported as mv-cli-pack"));

    let replaced = run(&[
        "--overwrite".as_ref(),
        "--no-split-release".as_ref(),
        src.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
    ]);
    assert!(replaced.status.success());
    assert!(String::from_utf8_lossy(&replaced.stdout).contains("[replaced]"));
}

#[test]
fn usage_errors_exit_64() {
    let none = run(&[]);
    assert_eq!(none.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&none.stderr).contains("usage:"));
    assert_eq!(run(&["x".as_ref(), "--frobnicate".as_ref()]).status.code(), Some(64));
    assert!(run(&["--help".as_ref()]).status.success());
}
