//! Keystroke onset detection.
//!
//! Two high-passed bands are watched: a broadband one (above hum and rumble) and a click
//! band. Each has a 1 ms energy envelope that is compared with an adaptive noise floor and
//! with its own recent minimum (a keystroke is a fast rise). The click band catches a bright
//! release landing on the low-frequency tail of its press, which the broadband envelope
//! barely notices. Each detection is then refined to the first sample of the transient,
//! because any silence left in front of a sample becomes playback latency.

use crate::signal::{self, power_db};

pub const FRAME_S: f64 = 0.001;
/// Lower edge of the click band: switch and keycap transients are broadband, the ringing
/// that follows them mostly is not.
const CLICK_BAND_HZ: f64 = 2_000.0;
/// Noise-floor blocks: 200 frames (200 ms).
const FLOOR_BLOCK: usize = 200;
const FLOOR_PERCENTILE: f64 = 0.10;
/// Refinement threshold above the noise floor (about 4 sigma for Gaussian noise).
const REFINE_MARGIN_DB: f32 = 12.0;
/// Refinement threshold above the background just before the rise, e.g. a decaying tail.
const BACKGROUND_MARGIN_DB: f32 = 6.0;
/// Quiet gap tolerated inside a transient (zero crossings of its first cycles).
const MAX_GAP_S: f64 = 0.0003;
/// Frames after the onset over which its level is measured.
const LEVEL_FRAMES: usize = 10;

#[derive(Clone, Copy, Debug)]
pub struct DetectorConfig {
    /// Minimum 1 ms RMS level of the broadband signal, dBFS.
    pub threshold_db: f32,
    /// Minimum level above the local noise floor.
    pub snr_db: f32,
    /// Minimum rise above the quietest frame of the preceding `rise_window_s`.
    pub rise_db: f32,
    pub rise_window_s: f64,
    /// A detection closer than this to the previous onset belongs to it.
    pub min_gap_s: f64,
    /// Lower edge of the broadband signal.
    pub highpass_hz: f64,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        DetectorConfig {
            threshold_db: -50.0,
            snr_db: 12.0,
            rise_db: 9.0,
            rise_window_s: 0.008,
            min_gap_s: 0.025,
            highpass_hz: 150.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Onset {
    /// Index of the first sample of the transient.
    pub pos: usize,
    /// Loudest broadband 1 ms level within 10 ms of the onset, dBFS.
    pub level_db: f32,
}

/// Short-term energy: one value per `hop`-sample (1 ms) frame, in dB.
pub struct Envelope {
    pub hop: usize,
    pub db: Vec<f32>,
}

impl Envelope {
    pub fn new(x: &[f32], rate: u32) -> Envelope {
        let hop = signal::frames(rate, FRAME_S).max(1);
        let db = x
            .chunks_exact(hop)
            .map(|c| power_db(c.iter().map(|&s| s as f64 * s as f64).sum::<f64>() / hop as f64))
            .collect();
        Envelope { hop, db }
    }

    /// Level of the quietest 10 % of frames: the background noise, dBFS RMS.
    pub fn noise_floor_db(&self) -> f32 {
        signal::percentile(&self.db, FLOOR_PERCENTILE).unwrap_or(-160.0)
    }

    /// Per-frame noise floor that follows slow changes in the background: each 200 ms
    /// block's 10th percentile, lowered to a neighbour's where that is quieter, so a block
    /// crowded with keystrokes does not raise its own floor.
    pub fn adaptive_floor(&self) -> Vec<f32> {
        let blocks: Vec<f32> = self
            .db
            .chunks(FLOOR_BLOCK)
            .map(|b| signal::percentile(b, FLOOR_PERCENTILE).unwrap_or(-160.0))
            .collect();
        (0..self.db.len())
            .map(|i| {
                let b = i / FLOOR_BLOCK;
                let hi = (b + 1).min(blocks.len() - 1);
                blocks[b.saturating_sub(1)..=hi].iter().copied().fold(f32::INFINITY, f32::min)
            })
            .collect()
    }
}

/// One high-passed view of the signal.
struct Band {
    hp: Vec<f32>,
    env: Envelope,
    floor: Vec<f32>,
}

impl Band {
    fn new(x: &[f32], rate: u32, cutoff_hz: f64) -> Band {
        let hp = signal::highpass(x, rate, cutoff_hz);
        let env = Envelope::new(&hp, rate);
        let floor = env.adaptive_floor();
        Band { hp, env, floor }
    }

    /// If frame `i` rises out of the noise and out of the preceding `rise_frames`, returns
    /// the quietest preceding frame and its level.
    fn rise(&self, i: usize, rise_frames: usize, cfg: &DetectorConfig) -> Option<(usize, f32)> {
        let level = self.env.db[i];
        if level < self.floor[i] + cfg.snr_db {
            return None;
        }
        let from = i.saturating_sub(rise_frames);
        let (quiet, background) = self.env.db[from..i]
            .iter()
            .enumerate()
            .fold((from, f32::INFINITY), |m, (k, &v)| if v < m.1 { (from + k, v) } else { m });
        (level - background >= cfg.rise_db).then_some((quiet, background))
    }
}

/// Finds keystroke onsets in mono audio, in order.
pub fn detect(x: &[f32], rate: u32, cfg: &DetectorConfig) -> Vec<Onset> {
    let bands = [Band::new(x, rate, cfg.highpass_hz), Band::new(x, rate, CLICK_BAND_HZ)];
    let broadband = &bands[0].env;
    let hop = broadband.hop;
    let rise_frames = ((cfg.rise_window_s / FRAME_S).round() as usize).max(1);
    let min_gap = signal::frames(rate, cfg.min_gap_s);
    let max_gap = signal::frames(rate, MAX_GAP_S).max(1);

    let mut onsets: Vec<Onset> = Vec::new();
    for i in 1..broadband.db.len() {
        if broadband.db[i] < cfg.threshold_db {
            continue;
        }
        let Some((band, (quiet, background))) =
            bands.iter().find_map(|b| b.rise(i, rise_frames, cfg).map(|r| (b, r)))
        else {
            continue;
        };
        let threshold = signal::db_amp(
            (band.floor[i] + REFINE_MARGIN_DB).max(background + BACKGROUND_MARGIN_DB),
        );
        let search_from = (quiet * hop).max(onsets.last().map_or(0, |o| o.pos + 1));
        let pos = refine(&band.hp, i * hop, hop, search_from, threshold, max_gap);
        if onsets.last().is_some_and(|o| pos < o.pos + min_gap) {
            continue;
        }
        let level_db = broadband.db[i..(i + LEVEL_FRAMES).min(broadband.db.len())]
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        onsets.push(Onset { pos, level_db });
    }
    onsets
}

/// Walks back from the frame where the energy rose to the start of the contiguous run of
/// above-threshold samples (gaps up to `max_gap` allowed) that leads into it.
fn refine(
    hp: &[f32],
    frame_start: usize,
    hop: usize,
    search_from: usize,
    threshold: f32,
    max_gap: usize,
) -> usize {
    let frame = &hp[frame_start..(frame_start + hop).min(hp.len())];
    let first = frame.iter().position(|s| s.abs() > threshold).unwrap_or_else(|| {
        frame.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).map_or(0, |m| m.0)
    });
    let mut onset = frame_start + first;
    let mut gap = 0;
    for j in (search_from..onset).rev() {
        if hp[j].abs() > threshold {
            onset = j;
            gap = 0;
        } else {
            gap += 1;
            if gap > max_gap {
                break;
            }
        }
    }
    onset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    const RATE: u32 = 48_000;
    /// ±0.5 ms.
    const TOLERANCE: usize = 24;

    fn assert_near(found: &[Onset], expected: &[usize]) {
        assert_eq!(
            found.len(),
            expected.len(),
            "found {:?}",
            found.iter().map(|o| o.pos).collect::<Vec<_>>()
        );
        for (o, &e) in found.iter().zip(expected) {
            assert!(o.pos.abs_diff(e) <= TOLERANCE, "onset {} vs expected {e}", o.pos);
        }
    }

    #[test]
    fn clicks_in_noise_are_found_to_the_sample() {
        let mut x = noise(RATE as usize * 2, -60.0, 1);
        let at = [4_000, 30_000, 31_000 + 4_800, 70_000, 90_000];
        for (n, &p) in at.iter().enumerate() {
            add(&mut x, &press_click(RATE, n as u64, 0.5), p);
        }
        let found = detect(&x, RATE, &DetectorConfig::default());
        assert_near(&found, &at);
        for (o, &e) in found.iter().zip(&at) {
            // In practice refinement lands within a few samples.
            assert!(o.pos.abs_diff(e) <= 4, "onset {} vs {e}", o.pos);
            assert!(o.level_db > -20.0);
        }
    }

    #[test]
    fn release_inside_a_decaying_press_is_a_separate_onset() {
        let mut x = noise(RATE as usize, -65.0, 2);
        let press = 10_000;
        let release = press + signal::frames(RATE, 0.045);
        add(&mut x, &long_press_click(RATE, 3, 0.6), press);
        add(&mut x, &release_click(RATE, 3, 0.2), release);
        let found = detect(&x, RATE, &DetectorConfig::default());
        assert_near(&found, &[press, release]);
        assert!(found[1].level_db < found[0].level_db);
    }

    #[test]
    fn noise_and_hum_alone_produce_nothing() {
        let mut x = noise(RATE as usize * 3, -50.0, 3);
        for (n, s) in x.iter_mut().enumerate() {
            *s += 0.05 * (std::f32::consts::TAU * 60.0 * n as f32 / RATE as f32).sin();
        }
        assert!(detect(&x, RATE, &DetectorConfig::default()).is_empty());
    }

    #[test]
    fn min_gap_merges_and_threshold_filters() {
        let mut x = noise(RATE as usize, -70.0, 4);
        let second = 5_000 + signal::frames(RATE, 0.015);
        add(&mut x, &press_click(RATE, 1, 0.1), 5_000);
        add(&mut x, &press_click(RATE, 2, 0.6), second);
        add(&mut x, &press_click(RATE, 3, 0.02), 30_000);
        let found = detect(&x, RATE, &DetectorConfig::default());
        assert_near(&found, &[5_000, 30_000]);
        let strict = DetectorConfig { threshold_db: -30.0, ..DetectorConfig::default() };
        assert_near(&detect(&x, RATE, &strict), &[5_000]);
        let loose = DetectorConfig { min_gap_s: 0.005, ..DetectorConfig::default() };
        assert_near(&detect(&x, RATE, &loose), &[5_000, second, 30_000]);
    }

    #[test]
    fn works_on_digital_silence_and_other_rates() {
        for rate in [44_100, 96_000] {
            let mut x = vec![0.0; rate as usize];
            let at = [1_000, rate as usize / 2];
            for &p in &at {
                add(&mut x, &press_click(rate, 7, 0.5), p);
            }
            let found = detect(&x, rate, &DetectorConfig::default());
            assert_eq!(found.len(), 2);
            for (o, &e) in found.iter().zip(&at) {
                assert!(o.pos.abs_diff(e) <= signal::frames(rate, 0.0005));
            }
        }
    }

    #[test]
    fn noise_floor_tracks_the_background() {
        let mut x = noise(RATE as usize * 2, -60.0, 5);
        for p in (2_000..90_000).step_by(6_000) {
            add(&mut x, &press_click(RATE, p as u64, 0.5), p);
        }
        let env = Envelope::new(&x, RATE);
        assert!((env.noise_floor_db() + 60.0).abs() < 3.0, "{}", env.noise_floor_db());
        let floor = env.adaptive_floor();
        assert!(floor.iter().all(|f| (f + 60.0).abs() < 4.0));
    }
}
