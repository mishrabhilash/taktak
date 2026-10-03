//! Slice mode: cut an existing recording (no key events) into a group-based pack.
//!
//! Onsets are labelled press or release from timing and sound alone, and the most
//! consistent takes of each become the `alphanumeric` pool. Every other group falls back to
//! it through the resolution chain (docs/pack-format.md), so no per-key sets are written.

use crate::assemble::{self, BuiltPack, Entry, Plan, SetRefs};
use crate::onset::{DetectorConfig, Onset};
use crate::signal;
use crate::signal::Audio;
use crate::takes::{self, CutConfig, Reject, Take};
use std::collections::BTreeMap;

/// Brightness and pairing are judged on the first 20 ms of each onset.
const ATTACK_S: f64 = 0.020;

#[derive(Clone, Copy, Debug)]
pub struct SliceOptions {
    pub detector: DetectorConfig,
    pub cut: CutConfig,
    /// Most variants kept per action.
    pub variants: usize,
    /// A release follows its press by this much.
    pub pair_min_s: f64,
    pub pair_max_s: f64,
}

impl Default for SliceOptions {
    fn default() -> Self {
        SliceOptions {
            detector: DetectorConfig { threshold_db: -40.0, ..DetectorConfig::default() },
            cut: CutConfig::default(),
            variants: 12,
            pair_min_s: 0.040,
            pair_max_s: 0.250,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SliceStats {
    pub onsets: usize,
    pub presses: usize,
    pub releases: usize,
    pub clipped: usize,
    pub too_quiet: usize,
    pub too_short: usize,
    pub press_files: usize,
    pub release_files: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Press,
    Release,
}

/// Labels onsets. A release follows its press by `pair_min_s..=pair_max_s` and is quieter
/// (by 1 dB or more) or brighter without being clearly louder. Everything else, including
/// an isolated onset, is a press.
pub fn classify(onsets: &[Onset], brightness: &[f32], rate: u32, opts: &SliceOptions) -> Vec<Role> {
    let mut roles = vec![Role::Press; onsets.len()];
    let mut i = 0;
    while i + 1 < onsets.len() {
        let (a, b) = (&onsets[i], &onsets[i + 1]);
        let gap = (b.pos - a.pos) as f64 / rate as f64;
        let quieter = b.level_db <= a.level_db - 1.0;
        let brighter = b.level_db <= a.level_db + 3.0 && brightness[i + 1] > brightness[i] * 1.1;
        if (opts.pair_min_s..=opts.pair_max_s).contains(&gap) && (quieter || brighter) {
            roles[i + 1] = Role::Release;
            i += 2;
        } else {
            i += 1;
        }
    }
    roles
}

pub fn process(audio: &Audio, opts: &SliceOptions) -> Result<(BuiltPack, SliceStats), String> {
    let p = takes::prepare(audio, &opts.detector);
    let rate = audio.rate;
    let attack = signal::frames(rate, ATTACK_S);
    let brightness: Vec<f32> = p
        .onsets
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let end = p.onsets.get(i + 1).map_or(o.pos + attack, |n| n.pos).min(o.pos + attack);
            takes::brightness(&p.clean.samples[o.pos..end.min(p.clean.samples.len())])
        })
        .collect();
    let roles = classify(&p.onsets, &brightness, rate, opts);

    let mut stats = SliceStats {
        onsets: p.onsets.len(),
        presses: roles.iter().filter(|r| **r == Role::Press).count(),
        ..SliceStats::default()
    };
    stats.releases = stats.onsets - stats.presses;

    let (mut presses, mut releases) = (Vec::new(), Vec::new());
    for (i, (o, role)) in p.onsets.iter().zip(&roles).enumerate() {
        let press = *role == Role::Press;
        let next = p.onsets.get(i + 1).map(|n| n.pos);
        match takes::cut(&p, o.pos, next, None, press, &opts.cut) {
            Ok(t) if press => presses.push(t),
            Ok(t) => releases.push(t),
            Err(Reject::Clipped) => stats.clipped += 1,
            Err(Reject::TooQuiet) => stats.too_quiet += 1,
            Err(Reject::TooShort) => stats.too_short += 1,
        }
    }
    if presses.is_empty() {
        return Err(format!(
            "no usable keystrokes found ({} onsets; {} clipped, {} too quiet, {} too short). \
             Try a lower --threshold-db, or check that the file is a keyboard recording.",
            stats.onsets, stats.clipped, stats.too_quiet, stats.too_short
        ));
    }

    let mut entries = Vec::new();
    let mut set = SetRefs::default();
    for (press, list) in [(true, presses), (false, releases)] {
        let action = if press { "press" } else { "release" };
        let order = most_consistent(&list, opts.variants);
        let mut slots: Vec<Option<Take>> = list.into_iter().map(Some).collect();
        for (n, i) in order.into_iter().enumerate() {
            let Some(take) = slots[i].take() else { continue };
            if press { &mut set.press } else { &mut set.release }.push(entries.len());
            entries.push(Entry { stem: format!("{action}-{}", n + 1), take, press });
        }
    }
    stats.press_files = set.press.len();
    stats.release_files = set.release.len();

    let strokes = set
        .press
        .iter()
        .enumerate()
        .map(|(n, &p)| (p, (!set.release.is_empty()).then(|| set.release[n % set.release.len()])))
        .collect();
    let plan = Plan {
        rate,
        entries,
        keys: BTreeMap::new(),
        groups: BTreeMap::from([("alphanumeric".to_owned(), set)]),
        strokes,
        noise_db: p.noise_db,
    };
    Ok((assemble::assemble(plan)?, stats))
}

/// Indices of the (up to) `n` takes nearest the typical take, closest first: distance from
/// the median loudness (in 3 dB steps) plus distance from the median brightness (in 25 %
/// steps, on a log scale).
fn most_consistent(list: &[Take], n: usize) -> Vec<usize> {
    let loud: Vec<f32> = list.iter().map(|t| t.loudness_db).collect();
    let bright: Vec<f32> = list.iter().map(|t| t.brightness.max(1e-9).ln()).collect();
    let (Some(ml), Some(mb)) = (signal::median(&loud), signal::median(&bright)) else {
        return Vec::new();
    };
    let distance: Vec<f32> = loud
        .iter()
        .zip(&bright)
        .map(|(l, b)| (l - ml).abs() / 3.0 + (b - mb).abs() / 0.25)
        .collect();
    let mut idx: Vec<usize> = (0..list.len()).collect();
    idx.sort_by(|&a, &b| distance[a].total_cmp(&distance[b]));
    idx.truncate(n);
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    const RATE: u32 = 48_000;

    fn onset(pos: usize, level_db: f32) -> Onset {
        Onset { pos, level_db }
    }

    #[test]
    fn classification_rules() {
        let ms = |m: usize| m * 48;
        let opts = SliceOptions::default();
        let onsets = [
            onset(ms(0), -10.0),    // press
            onset(ms(100), -20.0),  // quieter, 100 ms later: its release
            onset(ms(600), -10.0),  // isolated press
            onset(ms(1200), -10.0), // press …
            onset(ms(1320), -8.0),  // … followed by a louder, duller onset: another press
            onset(ms(2000), -12.0), // press
            onset(ms(2060), -11.0), // slightly louder but brighter: release
            onset(ms(3000), -10.0), // press
            onset(ms(3400), -20.0), // quieter but 400 ms later: a separate press
        ];
        let bright = [0.1, 0.2, 0.1, 0.1, 0.1, 0.1, 0.3, 0.1, 0.2];
        let roles = classify(&onsets, &bright, RATE, &opts);
        use Role::*;
        assert_eq!(roles, [Press, Release, Press, Press, Press, Press, Release, Press, Press]);
    }

    /// Press/release pairs (release 90 ms after, quieter and brighter), 350 ms apart, plus
    /// a few isolated presses.
    fn recording(press_peaks: &[f32], isolated: usize) -> (Audio, Vec<usize>, Vec<usize>) {
        let step = signal::frames(RATE, 0.35);
        let total = press_peaks.len() + isolated + 1;
        let mut x = noise(step * total, -66.0, 11);
        let (mut presses, mut releases) = (Vec::new(), Vec::new());
        for (i, &p) in press_peaks.iter().enumerate() {
            let at = step / 2 + i * step;
            add(&mut x, &press_click(RATE, 1, p), at);
            add(&mut x, &release_click(RATE, 1, p * 0.45), at + signal::frames(RATE, 0.09));
            presses.push(at);
            releases.push(at + signal::frames(RATE, 0.09));
        }
        for j in 0..isolated {
            let at = step / 2 + (press_peaks.len() + j) * step;
            add(&mut x, &press_click(RATE, 1, 0.4), at);
            presses.push(at);
        }
        (Audio::mono(RATE, x), presses, releases)
    }

    #[test]
    fn slices_pairs_and_keeps_the_most_consistent_variants() {
        let mut peaks =
            vec![0.40, 0.42, 0.38, 0.41, 0.39, 0.43, 0.40, 0.37, 0.42, 0.41, 0.40, 0.39, 0.41];
        peaks.extend([0.95, 0.1]);
        let (audio, presses, releases) = recording(&peaks, 3);
        let (built, stats) = process(&audio, &SliceOptions::default()).unwrap();
        assert_eq!(stats.onsets, presses.len() + releases.len());
        assert_eq!((stats.presses, stats.releases), (presses.len(), releases.len()));
        assert_eq!((stats.press_files, stats.release_files), (12, 12));

        let g = &built.groups["alphanumeric"];
        assert_eq!(g.press.len(), 12);
        assert_eq!(g.press[0], "sounds/press-1.wav");
        assert_eq!(g.release[11], "sounds/release-12.wav");
        assert!(built.keys.is_empty());
        // The two outliers (+7 dB, -12 dB) were left out: kept presses are within ~2 dB.
        let peak_db = |path: &String| {
            signal::amp_db(signal::peak(
                &built.sounds.iter().find(|s| &s.path == path).unwrap().samples,
            ))
        };
        let press_db: Vec<f32> = g.press.iter().map(peak_db).collect();
        let (lo, hi) = press_db.iter().fold((0.0f32, -200.0f32), |m, &v| (m.0.min(v), m.1.max(v)));
        assert!(hi - lo < 2.0, "{press_db:?}");
        assert!((signal::median(&press_db).unwrap() + 6.0).abs() < 0.05);
        // Releases stay quieter than presses after the shared gain.
        let release_db: Vec<f32> = g.release.iter().map(peak_db).collect();
        assert!(release_db.iter().all(|&r| r < lo - 3.0), "{release_db:?}");
        assert_eq!(built.preview.len(), signal::frames(built.rate, 1.5));
    }

    #[test]
    fn silence_is_an_error() {
        let audio = Audio::mono(RATE, noise(RATE as usize, -60.0, 1));
        let err = process(&audio, &SliceOptions::default()).unwrap_err();
        assert!(err.contains("--threshold-db"));
    }
}
