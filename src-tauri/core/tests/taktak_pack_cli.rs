//! The `taktak-pack` binary, run as a pack author or CI would.

mod common;

use common::{click, folder, manifest, manifest_with, write_folder};
use std::ffi::OsStr;
use std::process::{Command, Output};

const GOOD: &str = r#""groups": { "alphanumeric": { "press": ["a.wav", "b.wav"] },
                                  "space": { "press": ["a.wav"], "release": ["r.wav"] } }"#;

fn os(s: &str) -> &OsStr {
    OsStr::new(s)
}

/// Exit code and stdout + stderr.
fn run(args: &[&OsStr]) -> (i32, String) {
    let Output { status, stdout, stderr } =
        Command::new(env!("CARGO_BIN_EXE_taktak-pack")).args(args).output().unwrap();
    let text = String::from_utf8(stdout).unwrap() + &String::from_utf8(stderr).unwrap();
    (status.code().unwrap(), text)
}

fn good_files() -> Vec<(&'static str, Vec<u8>)> {
    vec![("a.wav", click(500)), ("b.wav", click(600)), ("r.wav", click(300))]
}

#[test]
fn validate_reports_ok_packs_with_counts() {
    let (_tmp, path) = folder(&manifest(GOOD), &good_files());
    let (code, out) = run(&[os("validate"), path.as_os_str(), os("--strict")]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out.trim(),
        format!("ok    {}: test \"Test pack\", CC0-1.0, 3 files, 0 warnings", path.display())
    );
}

#[test]
fn validate_fails_on_errors_found_only_by_decoding() {
    let mut files = good_files();
    files[1].1 = b"not a wav".to_vec();
    let (_tmp, path) = folder(&manifest(GOOD), &files);
    let (code, out) = run(&[os("validate"), path.as_os_str()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with(&format!("FAIL  {}: 1 error, 0 warnings", path.display())), "{out}");
    assert!(out.contains("b.wav") && out.contains("not a valid WAV file"), "{out}");
}

#[test]
fn validate_checks_every_path_and_strict_rejects_personal_packs() {
    let tmp = tempfile::tempdir().unwrap();
    let personal = write_folder(
        &tmp.path().join("personal"),
        &manifest_with("personal", "LicenseRef-Personal", GOOD),
        &good_files(),
    );
    let missing = write_folder(&tmp.path().join("missing"), &manifest(GOOD), &good_files()[1..]);

    let (code, out) = run(&[os("validate"), personal.as_os_str(), missing.as_os_str()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("ok    ") && out.contains("1 warning"), "{out}");
    assert!(out.contains("license") && out.contains("personal use only"), "{out}");
    assert!(out.contains("groups.alphanumeric.press[0]"), "{out}");
    assert!(out.contains("groups.space.press[0]"), "{out}");
    assert!(out.trim_end().ends_with("2 packs: 1 ok, 1 failed"), "{out}");

    let (code, out) = run(&[os("validate"), os("--strict"), personal.as_os_str()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("FAIL  ") && out.contains("cannot be bundled"), "{out}");
}

#[test]
fn info_shows_counts_and_statistics() {
    let (_tmp, path) = folder(
        &manifest(&format!(r#""keys": {{ "KeyA": {{ "press": ["b.wav"] }} }}, {GOOD}"#)),
        &good_files(),
    );
    let (code, out) = run(&[os("info"), path.as_os_str()]);
    assert_eq!(code, 0, "{out}");
    for needle in [
        "Test pack (test)",
        "license      CC0-1.0",
        "preview      a.wav",
        "alphanumeric       2        0",
        "space              1        1",
        "KeyA               1        0",
        "131/131 keys sound on press, 1/131 on release",
        "3 distinct",
        "memory",
        "load time",
        "0 warnings",
    ] {
        assert!(out.contains(needle), "{needle:?} not in:\n{out}");
    }
}

#[test]
fn list_shows_visible_and_invalid_packs() {
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let user = tmp.path().join("user");
    write_folder(&bundled.join("test"), &manifest(GOOD), &good_files());
    write_folder(&user.join("test"), &manifest(GOOD), &good_files());
    write_folder(&user.join("broken"), &manifest(GOOD), &[]);

    let (code, out) =
        run(&[os("list"), os("--bundled"), bundled.as_os_str(), os("--user"), user.as_os_str()]);
    // An invalid candidate fails the run, so `list` can gate a folder of packs.
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("packs (1):"), "{out}");
    assert!(out.contains(&format!("user    {}", user.join("test").display())), "{out}");
    assert!(out.contains("overridden by a user pack:"), "{out}");
    assert!(out.contains("invalid (1):") && out.contains("groups.alphanumeric.press[0]"), "{out}");

    std::fs::remove_dir_all(user.join("broken")).unwrap();
    let (code, out) =
        run(&[os("list"), os("--bundled"), bundled.as_os_str(), os("--user"), user.as_os_str()]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("invalid"), "{out}");
}

#[test]
fn validate_reports_pack_json_and_audio_problems_in_one_run() {
    let mut files = good_files();
    files[1].1 = b"not a wav".to_vec();
    let (_tmp, path) = folder(&manifest_with("Bad_ID", "CC0-1.0", GOOD), &files);
    let (code, out) = run(&[os("validate"), path.as_os_str()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with(&format!("FAIL  {}: 2 errors, 0 warnings", path.display())), "{out}");
    assert!(out.contains("invalid id \"Bad_ID\""), "{out}");
    assert!(out.contains("b.wav") && out.contains("not a valid WAV file"), "{out}");
}

#[test]
fn hostile_names_cannot_forge_output_lines() {
    let (_tmp, path) =
        folder(&manifest(&format!(r#""x\nok    spoofed\u001b[2J": 1, {GOOD}"#)), &good_files());
    let (code, out) = run(&[os("validate"), path.as_os_str()]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains('\u{1b}'), "{out:?}");
    assert!(out.lines().all(|line| !line.starts_with("ok    spoofed")), "{out}");
    assert!(out.contains("x\\nok    spoofed\\u{1b}[2J"), "{out}");
}

#[test]
fn usage_errors_exit_64() {
    for args in [&[][..], &[os("validate")], &[os("bogus")], &[os("info"), os("--strict")]] {
        let (code, out) = run(args);
        assert_eq!(code, 64, "{args:?}: {out}");
        assert!(out.contains("usage:"), "{out}");
    }
    let (code, out) = run(&[os("--help")]);
    assert_eq!(code, 0);
    assert!(out.contains("taktak-pack validate"));
}
