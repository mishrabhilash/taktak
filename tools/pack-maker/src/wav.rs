//! WAV input (PCM 8/16/24/32-bit or 32-bit float, mixed down to mono) and 16-bit mono output.

use crate::signal::{Audio, ClipRuns, mix_frame};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use std::path::Path;

/// Longer recordings should be split; this also bounds memory.
const MAX_INPUT_S: u64 = 30 * 60;

pub fn read(path: &Path) -> Result<Audio, String> {
    let err = |e: hound::Error| format!("{}: {e}", path.display());
    let mut reader = WavReader::open(path).map_err(err)?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err(format!("{}: invalid WAV header", path.display()));
    }
    if reader.duration() as u64 > MAX_INPUT_S * spec.sample_rate as u64 {
        return Err(format!(
            "{}: longer than {} minutes; split it into shorter files",
            path.display(),
            MAX_INPUT_S / 60
        ));
    }
    let channels = spec.channels as usize;
    let frames = reader.duration() as usize;
    match spec.sample_format {
        SampleFormat::Float => mixdown(reader.samples::<f32>(), channels, spec.sample_rate, frames),
        SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            let samples = reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale));
            mixdown(samples, channels, spec.sample_rate, frames)
        }
    }
    .map_err(err)
}

fn mixdown(
    samples: impl Iterator<Item = hound::Result<f32>>,
    channels: usize,
    rate: u32,
    frames: usize,
) -> hound::Result<Audio> {
    let mut mono = Vec::with_capacity(frames);
    let mut frame = Vec::with_capacity(channels);
    let mut runs = ClipRuns::default();
    let mut clipped = Vec::new();
    for s in samples {
        frame.push(s?);
        if frame.len() == channels {
            let (m, clip) = mix_frame(&frame);
            clipped.extend(runs.feed(mono.len(), clip));
            mono.push(m);
            frame.clear();
        }
    }
    clipped.extend(runs.finish(mono.len()));
    Ok(Audio { rate, samples: mono, clipped })
}

/// Writes 16-bit PCM mono.
pub fn write(path: &Path, rate: u32, samples: &[f32]) -> Result<(), String> {
    let err = |e: hound::Error| format!("{}: {e}", path.display());
    let spec = WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).map_err(err)?;
    for &s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).map_err(err)?;
    }
    w.finalize().map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_16_bit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        write(&path, 44_100, &[0.0, 0.5, -0.5, 1.5]).unwrap();
        let a = read(&path).unwrap();
        assert_eq!(a.rate, 44_100);
        assert_eq!(a.samples.len(), 4);
        assert!((a.samples[1] - 0.5).abs() < 1e-4 && (a.samples[2] + 0.5).abs() < 1e-4);
        assert_eq!(a.clipped, vec![3..4], "full scale counts as clipped");
    }

    #[test]
    fn reads_24_bit_stereo_and_float() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 96_000,
            bits_per_sample: 24,
            sample_format: SampleFormat::Int,
        };
        let mut w = WavWriter::create(&path, spec).unwrap();
        for v in [4_194_304i32, 0, -8_388_608, -8_388_608] {
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();
        let a = read(&path).unwrap();
        assert_eq!((a.rate, a.samples.len()), (96_000, 2));
        assert!((a.samples[0] - 0.25).abs() < 1e-6 && (a.samples[1] + 1.0).abs() < 1e-6);
        assert_eq!(a.clipped, vec![1..2]);

        let path = dir.path().join("f.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut w = WavWriter::create(&path, spec).unwrap();
        w.write_sample(0.125f32).unwrap();
        w.finalize().unwrap();
        assert_eq!(read(&path).unwrap().samples, vec![0.125]);
    }

    #[test]
    fn missing_or_bogus_files_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(&dir.path().join("nope.wav")).is_err());
        let bogus = dir.path().join("bogus.wav");
        std::fs::write(&bogus, b"not a wav file at all").unwrap();
        assert!(read(&bogus).is_err());
    }
}
