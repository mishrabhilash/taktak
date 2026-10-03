//! Builders for test packs: folders and zips with generated WAVs.

#![allow(dead_code, reason = "each test crate uses a different subset")]

use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

pub const RATE: u32 = 48_000;

/// 16-bit mono WAV: `silence` zero samples, then `len` samples of a decaying 2 kHz click.
pub fn wav(rate: u32, silence: usize, len: usize) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    let mut w = hound::WavWriter::new(&mut cursor, spec).unwrap();
    for _ in 0..silence {
        w.write_sample(0i16).unwrap();
    }
    for i in 0..len {
        let t = i as f32 / rate as f32;
        let s = 0.8 * (std::f32::consts::TAU * 2000.0 * t).cos() * (-t / 0.02).exp();
        w.write_sample((s * 32767.0).round() as i16).unwrap();
    }
    w.finalize().unwrap();
    cursor.into_inner()
}

/// A click of exactly `len` samples at 48 kHz with no leading silence, so its decoded length
/// identifies it.
pub fn click(len: usize) -> Vec<u8> {
    wav(RATE, 0, len)
}

pub fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// `pack.json` text: the required fields (id `test`, license CC0-1.0) plus `fields`, which is
/// spliced in as-is (e.g. `"volume": 0.5, "groups": {…}`).
pub fn manifest(fields: &str) -> String {
    manifest_with("test", "CC0-1.0", fields)
}

pub fn manifest_with(id: &str, license: &str, fields: &str) -> String {
    let fields = if fields.is_empty() { String::new() } else { format!(",\n  {fields}") };
    format!(
        "{{\n  \"format\": 1, \"id\": \"{id}\", \"name\": \"Test pack\", \"author\": \"Tester\",\n  \
         \"license\": \"{license}\"{fields}\n}}"
    )
}

/// Writes a folder pack at `dir` (created if needed).
pub fn write_folder(dir: &Path, manifest: &str, files: &[(&str, Vec<u8>)]) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("pack.json"), manifest).unwrap();
    for (rel, bytes) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    dir.to_path_buf()
}

/// A folder pack in a fresh temp dir; keep the `TempDir` alive while using the path.
pub fn folder(manifest: &str, files: &[(&str, Vec<u8>)]) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_folder(&tmp.path().join("pack"), manifest, files);
    (tmp, path)
}

/// Writes a zip at `path`. Every pack file goes under `prefix` (`""` or `"name/"`);
/// `extra` entries are written verbatim at their own names.
pub fn write_zip(
    path: &Path,
    prefix: &str,
    manifest: &str,
    files: &[(&str, Vec<u8>)],
    extra: &[(&str, &[u8])],
) -> PathBuf {
    let mut w = zip::ZipWriter::new(File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    w.start_file(format!("{prefix}pack.json"), options).unwrap();
    w.write_all(manifest.as_bytes()).unwrap();
    for (rel, bytes) in files {
        w.start_file(format!("{prefix}{rel}"), options).unwrap();
        w.write_all(bytes).unwrap();
    }
    for (name, bytes) in extra {
        w.start_file(*name, options).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap();
    path.to_path_buf()
}
