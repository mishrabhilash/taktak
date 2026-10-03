//! Take handling shared by both modes: cleaning the recording, cutting one keystroke out of
//! it, rejecting unusable takes and choosing the most consistent ones.

use crate::onset::{self, DetectorConfig, Envelope, Onset};
use crate::signal::{self, Audio};

/// Mic rumble and DC below this are removed from everything that is written.
const CLEAN_HIGHPASS_HZ: f64 = 30.0;
/// Takes are compared on their first 20 ms: the attack is what the ear compares.
const MEASURE_S: f64 = 0.020;
const TAIL_BLOCK_S: f64 = 0.005;
/// A tail has decayed once its 5 ms RMS is within 6 dB of the noise floor.
const TAIL_ABOVE_NOISE_DB: f32 = 6.0;
const TAIL_MARGIN_S: f64 = 0.010;

pub struct Prepared {
    /// DC- and rumble-free audio the takes are cut from, with the source's clip ranges.
    pub clean: Audio,
    pub onsets: Vec<Onset>,
    /// Background noise of `clean`, dBFS RMS.
    pub noise_db: f32,
}

pub fn prepare(audio: &Audio, cfg: &DetectorConfig) -> Prepared {
    let n = audio.samples.len().max(1) as f64;
    let mean = (audio.samples.iter().map(|&s| s as f64).sum::<f64>() / n) as f32;
    let centered: Vec<f32> = audio.samples.iter().map(|&s| s - mean).collect();
    let samples = signal::highpass(&centered, audio.rate, CLEAN_HIGHPASS_HZ);
    let onsets = onset::detect(&samples, audio.rate, cfg);
    let noise_db = Envelope::new(&samples, audio.rate).noise_floor_db();
    let clean = Audio { rate: audio.rate, samples, clipped: audio.clipped.clone() };
    Prepared { clean, onsets, noise_db }
}

#[derive(Clone, Copy, Debug)]
pub struct CutConfig {
    /// Kept before the onset; absorbs refinement error without audible latency.
    pub pre_roll_s: f64,
    /// Space left before the next onset.
    pub next_gap_s: f64,
    pub max_press_s: f64,
    pub max_release_s: f64,
    pub min_len_s: f64,
    pub fade_s: f64,
    /// Minimum peak level above the noise floor.
    pub min_snr_db: f32,
}

impl Default for CutConfig {
    fn default() -> Self {
        CutConfig {
            pre_roll_s: 0.0005,
            next_gap_s: 0.001,
            max_press_s: 0.300,
            max_release_s: 0.200,
            min_len_s: 0.015,
            fade_s: 0.005,
            min_snr_db: 15.0,
        }
    }
}

impl CutConfig {
    pub fn max_len_s(&self, press: bool) -> f64 {
        if press { self.max_press_s } else { self.max_release_s }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    Clipped,
    TooQuiet,
    TooShort,
}

#[derive(Clone, Debug)]
pub struct Take {
    pub samples: Vec<f32>,
    pub peak: f32,
    /// RMS of the first 20 ms after the onset, dBFS.
    pub loudness_db: f32,
    /// Mean-square slope over mean-square level of the first 20 ms (a spectral-centroid proxy).
    pub brightness: f32,
}

impl Take {
    pub fn peak_db(&self) -> f32 {
        signal::amp_db(self.peak)
    }
}

/// Cuts the keystroke at `onset`: from the pre-roll to whichever comes first of the next
/// onset (less a small gap), `limit`, the maximum length, or the decay into the noise.
pub fn cut(
    p: &Prepared,
    onset: usize,
    next_onset: Option<usize>,
    limit: Option<usize>,
    press: bool,
    cfg: &CutConfig,
) -> Result<Take, Reject> {
    let a = &p.clean;
    let start = onset.saturating_sub(a.frames(cfg.pre_roll_s));
    let mut end = (onset + a.frames(cfg.max_len_s(press))).min(a.samples.len());
    if let Some(next) = next_onset {
        end = end.min(next.saturating_sub(a.frames(cfg.next_gap_s)));
    }
    if let Some(l) = limit {
        end = end.min(l);
    }
    let min_len = a.frames(cfg.min_len_s);
    if end < onset + min_len {
        return Err(Reject::TooShort);
    }
    if a.is_clipped(start..end) {
        return Err(Reject::Clipped);
    }
    let body = &a.samples[onset..end];
    if signal::amp_db(signal::peak(body)) < p.noise_db + cfg.min_snr_db {
        return Err(Reject::TooQuiet);
    }
    let end = onset + decayed_len(body, a.rate, p.noise_db).max(min_len);

    let mut samples = a.samples[start..end].to_vec();
    signal::fade_in(&mut samples, (onset - start) / 2);
    let fade = a.frames(cfg.fade_s).min(samples.len() / 2);
    signal::fade_out(&mut samples, fade);
    let attack = &a.samples[onset..(onset + a.frames(MEASURE_S)).min(end)];
    Ok(Take {
        peak: signal::peak(&samples),
        loudness_db: signal::amp_db(signal::rms(attack)),
        brightness: brightness(attack),
        samples,
    })
}

/// Length up to the last 5 ms block still clearly above the noise, plus a short margin.
/// Noise-only tails would add hiss every time many keys ring at once.
fn decayed_len(body: &[f32], rate: u32, noise_db: f32) -> usize {
    let block = signal::frames(rate, TAIL_BLOCK_S).max(1);
    let threshold = signal::db_amp(noise_db + TAIL_ABOVE_NOISE_DB);
    let last = body.chunks(block).rposition(|b| signal::rms(b) > threshold);
    last.map_or(body.len(), |b| {
        ((b + 1) * block + signal::frames(rate, TAIL_MARGIN_S)).min(body.len())
    })
}

pub fn brightness(x: &[f32]) -> f32 {
    let energy: f64 = x.iter().map(|&s| s as f64 * s as f64).sum();
    let slope: f64 = x.windows(2).map(|w| (w[1] as f64 - w[0] as f64).powi(2)).sum();
    if energy > 0.0 { (slope / energy) as f32 } else { 0.0 }
}

/// Indices of the (up to) `n` values closest to their median, closest first.
pub fn closest_to_median(values: &[f32], n: usize) -> Vec<usize> {
    let Some(m) = signal::median(values) else { return Vec::new() };
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| (values[a] - m).abs().total_cmp(&(values[b] - m).abs()));
    idx.truncate(n);
    idx
}

/// The first item of each list, then the second of each, and so on, up to `max` items, so a
/// pool drawn from several keys represents all of them.
pub fn round_robin<T: Copy>(lists: &[Vec<T>], max: usize) -> Vec<T> {
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    (0..longest)
        .flat_map(|d| lists.iter().filter_map(move |l| l.get(d).copied()))
        .take(max)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    const RATE: u32 = 48_000;

    fn prepared(x: Vec<f32>) -> Prepared {
        prepare(&Audio::mono(RATE, x), &DetectorConfig::default())
    }

    #[test]
    fn cut_starts_half_a_millisecond_early_and_stops_before_the_next_onset() {
        let mut x = noise(RATE as usize, -60.0, 1);
        add(&mut x, &long_press_click(RATE, 1, 0.5), 10_000);
        add(&mut x, &release_click(RATE, 1, 0.3), 14_000);
        let p = prepared(x);
        assert_eq!(p.onsets.len(), 2);
        let cfg = CutConfig::default();
        let (on, next) = (p.onsets[0].pos, p.onsets[1].pos);
        let take = cut(&p, on, Some(next), None, true, &cfg).unwrap();
        assert_eq!(take.samples.len(), next - 48 - (on - 24));
        assert_eq!(take.samples[0], 0.0);
        assert_eq!(*take.samples.last().unwrap(), 0.0);
        // Leading silence (before the first sample above -50 dBFS) is at most the pre-roll.
        let lead = take.samples.iter().position(|s| s.abs() >= 0.00316).unwrap();
        assert!(lead <= 24 + 4, "{lead}");
    }

    #[test]
    fn cut_respects_max_length_limit_and_decay() {
        let mut x = noise(RATE as usize, -70.0, 2);
        add(&mut x, &long_press_click(RATE, 2, 0.5), 10_000);
        let p = prepared(x);
        let on = p.onsets[0].pos;
        let cfg = CutConfig { max_press_s: 0.05, ..CutConfig::default() };
        let t = cut(&p, on, None, None, true, &cfg).unwrap();
        assert_eq!(t.samples.len(), 24 + 2_400);
        let t = cut(&p, on, None, Some(on + 1_000), true, &cfg).unwrap();
        assert_eq!(t.samples.len(), 24 + 1_000);
        // Unlimited: the 150 ms click has decayed into the noise well before 300 ms.
        let t = cut(&p, on, None, None, true, &CutConfig::default()).unwrap();
        assert!(t.samples.len() < signal::frames(RATE, 0.2), "{}", t.samples.len());
        assert!(t.samples.len() > signal::frames(RATE, 0.05));
        // The rumble filter shifts phases, so the peak moves a little.
        assert!(t.peak > 0.4 && t.peak < 0.55, "{}", t.peak);
        assert!(t.loudness_db < t.peak_db());
    }

    #[test]
    fn rejects() {
        let mut x = noise(RATE as usize, -60.0, 3);
        add(&mut x, &press_click(RATE, 3, 0.5), 10_000);
        add(&mut x, &press_click(RATE, 4, 0.5), 30_000);
        let mut audio = Audio::mono(RATE, x);
        audio.clipped.push(30_100..30_105);
        let p = prepare(&audio, &DetectorConfig::default());
        let cfg = CutConfig::default();
        let (a, b) = (p.onsets[0].pos, p.onsets[1].pos);
        assert!(cut(&p, a, Some(b), None, true, &cfg).is_ok());
        assert_eq!(cut(&p, b, None, None, true, &cfg).unwrap_err(), Reject::Clipped);
        assert_eq!(cut(&p, a, Some(a + 500), None, true, &cfg).unwrap_err(), Reject::TooShort);
        // A stretch of plain noise is not a keystroke.
        assert_eq!(cut(&p, 40_000, None, None, true, &cfg).unwrap_err(), Reject::TooQuiet);
    }

    #[test]
    fn releases_are_brighter_than_presses() {
        let mut x = noise(RATE as usize, -70.0, 4);
        add(&mut x, &press_click(RATE, 5, 0.5), 10_000);
        add(&mut x, &release_click(RATE, 5, 0.3), 30_000);
        let p = prepared(x);
        let cfg = CutConfig::default();
        let press = cut(&p, p.onsets[0].pos, Some(p.onsets[1].pos), None, true, &cfg).unwrap();
        let release = cut(&p, p.onsets[1].pos, None, None, false, &cfg).unwrap();
        assert!(release.brightness > press.brightness);
        assert!(release.loudness_db < press.loudness_db);
    }

    #[test]
    fn selection_helpers() {
        assert_eq!(closest_to_median(&[-10.0, -30.0, -12.5, -11.0, -2.0], 3), vec![3, 0, 2]);
        assert_eq!(closest_to_median(&[1.0], 3), vec![0]);
        assert!(closest_to_median(&[], 3).is_empty());
        let lists = vec![vec![1, 2, 3], vec![10], vec![20, 21]];
        assert_eq!(round_robin(&lists, 12), vec![1, 10, 20, 2, 21, 3]);
        assert_eq!(round_robin(&lists, 4), vec![1, 10, 20, 2]);
    }
}
