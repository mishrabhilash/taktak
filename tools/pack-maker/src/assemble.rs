//! Turns chosen takes into a writable pack: one pack-wide gain, output-rate conversion,
//! sound sets with file paths, and a short preview.

use crate::signal::{self, OUTPUT_RATE};
use crate::takes::Take;
use std::collections::BTreeMap;
use taktak_core::pack::SoundSet;

/// The median press peaks here after normalization.
pub const TARGET_PRESS_PEAK_DB: f32 = -6.0;
/// No sample may peak above this, even if that leaves the median press below target.
pub const MAX_PEAK_DB: f32 = -1.0;
/// Below this keystroke-to-noise ratio the background hiss becomes audible.
pub const MIN_SNR_DB: f32 = 20.0;
/// Upper bound on files in one group's pool.
pub const MAX_POOL: usize = 12;

const PREVIEW_S: f64 = 1.5;
/// Fixed, made-up rhythm: the preview never reflects how anything was actually typed.
const PREVIEW_STEPS_S: [f64; 10] = [0.13, 0.11, 0.15, 0.12, 0.10, 0.16, 0.12, 0.14, 0.11, 0.13];
const PREVIEW_HOLDS_S: [f64; 4] = [0.075, 0.09, 0.07, 0.085];
const PREVIEW_LAST_START_S: f64 = 1.25;

pub struct Entry {
    /// File name without extension, e.g. `KeyA-press-1`.
    pub stem: String,
    pub take: Take,
    pub press: bool,
}

/// A sound set as indices into [`Plan::entries`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetRefs {
    pub press: Vec<usize>,
    pub release: Vec<usize>,
}

impl SetRefs {
    pub fn is_empty(&self) -> bool {
        self.press.is_empty() && self.release.is_empty()
    }
}

pub struct Plan {
    /// Sample rate of the takes.
    pub rate: u32,
    pub entries: Vec<Entry>,
    pub keys: BTreeMap<String, SetRefs>,
    pub groups: BTreeMap<String, SetRefs>,
    /// (press, release) entry pairs to play in the preview, cycled as needed.
    pub strokes: Vec<(usize, Option<usize>)>,
    /// Background noise of the source, dBFS RMS.
    pub noise_db: f32,
}

pub struct Sound {
    /// Path inside the pack, e.g. `sounds/KeyA-press-1.wav`.
    pub path: String,
    pub samples: Vec<f32>,
}

pub struct BuiltPack {
    /// Sample rate of `sounds` and `preview`.
    pub rate: u32,
    pub sounds: Vec<Sound>,
    pub keys: BTreeMap<String, SoundSet>,
    pub groups: BTreeMap<String, SoundSet>,
    pub preview: Vec<f32>,
    pub gain_db: f32,
    /// Median press attack level over the noise floor.
    pub snr_db: f32,
    /// Things the author should know (low SNR, limited gain, …).
    pub notes: Vec<String>,
}

impl std::fmt::Debug for BuiltPack {
    /// A summary only: the full sample data is far too long to print.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltPack")
            .field("rate", &self.rate)
            .field("sounds", &self.sounds.len())
            .field("gain_db", &self.gain_db)
            .field("snr_db", &self.snr_db)
            .finish_non_exhaustive()
    }
}

pub fn assemble(plan: Plan) -> Result<BuiltPack, String> {
    let press_peaks: Vec<f32> =
        plan.entries.iter().filter(|e| e.press).map(|e| e.take.peak_db()).collect();
    let median_peak = signal::median(&press_peaks).ok_or("no usable key-press sounds")?;
    let loudest = plan.entries.iter().map(|e| e.take.peak_db()).fold(f32::MIN, f32::max);
    let mut notes = Vec::new();

    let mut gain_db = TARGET_PRESS_PEAK_DB - median_peak;
    if loudest + gain_db > MAX_PEAK_DB {
        gain_db = MAX_PEAK_DB - loudest;
        notes.push(format!(
            "some takes are much louder than the rest; the median press peaks at {:.1} dBFS \
             instead of {TARGET_PRESS_PEAK_DB} dBFS to avoid clipping",
            median_peak + gain_db
        ));
    }

    let press_loudness: Vec<f32> =
        plan.entries.iter().filter(|e| e.press).map(|e| e.take.loudness_db).collect();
    let snr_db = signal::median(&press_loudness).unwrap_or(plan.noise_db) - plan.noise_db;
    if snr_db < MIN_SNR_DB {
        notes.push(format!(
            "keystrokes are only {snr_db:.0} dB above the background noise (aim for \
             {MIN_SNR_DB:.0}+): record in a quieter room or move the microphone closer"
        ));
    }

    let rate = if plan.rate == 44_100 || plan.rate == 48_000 { plan.rate } else { OUTPUT_RATE };
    let gain = signal::db_amp(gain_db);
    let sounds: Vec<Sound> = plan
        .entries
        .iter()
        .map(|e| {
            let scaled: Vec<f32> = e.take.samples.iter().map(|s| s * gain).collect();
            Sound {
                path: format!("sounds/{}.wav", e.stem),
                samples: signal::resample(&scaled, plan.rate, rate),
            }
        })
        .collect();

    let to_set = |r: &SetRefs| SoundSet {
        press: r.press.iter().map(|&i| sounds[i].path.clone()).collect(),
        release: r.release.iter().map(|&i| sounds[i].path.clone()).collect(),
        ..SoundSet::default()
    };
    let keys = plan.keys.iter().map(|(k, r)| (k.clone(), to_set(r))).collect();
    let groups = plan
        .groups
        .iter()
        .filter(|(_, r)| !r.is_empty())
        .map(|(g, r)| (g.clone(), to_set(r)))
        .collect();
    let preview = preview(&sounds, &plan.strokes, rate);
    Ok(BuiltPack { rate, sounds, keys, groups, preview, gain_db, snr_db, notes })
}

fn preview(sounds: &[Sound], strokes: &[(usize, Option<usize>)], rate: u32) -> Vec<f32> {
    let mut out = vec![0.0; signal::frames(rate, PREVIEW_S)];
    let mut mix = |i: usize, t: f64| {
        for (o, s) in out.iter_mut().skip(signal::frames(rate, t)).zip(&sounds[i].samples) {
            *o += s;
        }
    };
    let mut t = 0.0;
    for (n, &(press, release)) in strokes.iter().cycle().enumerate() {
        if t > PREVIEW_LAST_START_S {
            break;
        }
        mix(press, t);
        if let Some(r) = release {
            mix(r, t + PREVIEW_HOLDS_S[n % PREVIEW_HOLDS_S.len()]);
        }
        t += PREVIEW_STEPS_S[n % PREVIEW_STEPS_S.len()];
    }
    let peak = signal::peak(&out);
    let limit = signal::db_amp(MAX_PEAK_DB);
    if peak > limit {
        out.iter_mut().for_each(|s| *s *= limit / peak);
    }
    signal::fade_out(&mut out, signal::frames(rate, 0.02));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(peak: f32, len: usize) -> Take {
        let mut samples = vec![0.0; len];
        samples[len / 4] = peak;
        samples[len / 4 + 1] = -peak * 0.5;
        Take { samples, peak, loudness_db: signal::amp_db(peak) - 10.0, brightness: 0.5 }
    }

    fn plan(rate: u32, press: &[f32], release: &[f32]) -> Plan {
        let mut entries = Vec::new();
        for (i, &p) in press.iter().enumerate() {
            entries.push(Entry { stem: format!("press-{i}"), take: take(p, 4_800), press: true });
        }
        for (i, &p) in release.iter().enumerate() {
            entries.push(Entry {
                stem: format!("release-{i}"),
                take: take(p, 2_400),
                press: false,
            });
        }
        let refs = SetRefs {
            press: (0..press.len()).collect(),
            release: (press.len()..press.len() + release.len()).collect(),
        };
        let strokes = (0..press.len()).map(|i| (i, refs.release.first().copied())).collect();
        Plan {
            rate,
            entries,
            keys: BTreeMap::new(),
            groups: BTreeMap::from([
                ("alphanumeric".to_string(), refs),
                ("space".into(), SetRefs::default()),
            ]),
            strokes,
            noise_db: -70.0,
        }
    }

    #[test]
    fn median_press_peak_lands_at_minus_6_dbfs_and_ratios_are_kept() {
        let built = assemble(plan(48_000, &[0.1, 0.2, 0.15], &[0.05])).unwrap();
        let peak_db = |path: &str| {
            signal::amp_db(signal::peak(
                &built.sounds.iter().find(|s| s.path == path).unwrap().samples,
            ))
        };
        assert!((peak_db("sounds/press-2.wav") - TARGET_PRESS_PEAK_DB).abs() < 0.01);
        // The release keeps its level relative to the presses.
        let rel = peak_db("sounds/release-0.wav") - peak_db("sounds/press-2.wav");
        assert!((rel - signal::amp_db(0.05 / 0.15)).abs() < 0.01);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
        assert_eq!(built.rate, 48_000);
        assert_eq!(built.groups.len(), 1, "empty groups are dropped");
        let g = &built.groups["alphanumeric"];
        assert_eq!(g.press.len(), 3);
        assert_eq!(g.release, vec!["sounds/release-0.wav".to_string()]);
    }

    #[test]
    fn gain_is_capped_so_nothing_clips() {
        let built = assemble(plan(44_100, &[0.1, 0.1, 0.9], &[])).unwrap();
        let loudest = built.sounds.iter().map(|s| signal::peak(&s.samples)).fold(0.0, f32::max);
        assert!((signal::amp_db(loudest) - MAX_PEAK_DB).abs() < 0.01);
        assert_eq!(built.notes.len(), 1);
        assert_eq!(built.rate, 44_100);
    }

    #[test]
    fn low_snr_is_reported() {
        let mut p = plan(48_000, &[0.01, 0.01], &[]);
        p.noise_db = -45.0;
        let built = assemble(p).unwrap();
        assert!(built.snr_db < MIN_SNR_DB);
        assert!(built.notes.iter().any(|n| n.contains("background noise")));
    }

    #[test]
    fn unusual_rates_are_converted_to_48k() {
        let built = assemble(plan(96_000, &[0.5], &[0.2])).unwrap();
        assert_eq!(built.rate, 48_000);
        assert_eq!(built.sounds[0].samples.len(), 2_400);
    }

    #[test]
    fn preview_is_about_one_and_a_half_seconds_and_never_clips() {
        let built = assemble(plan(48_000, &[0.5, 0.6, 0.4], &[0.3])).unwrap();
        assert_eq!(built.preview.len(), 72_000);
        let peak = signal::peak(&built.preview);
        assert!(peak > 0.1 && peak <= signal::db_amp(MAX_PEAK_DB) + 1e-6);
        assert_eq!(*built.preview.last().unwrap(), 0.0);
    }

    #[test]
    fn no_presses_is_an_error() {
        assert!(assemble(plan(48_000, &[], &[0.2])).is_err());
    }
}
