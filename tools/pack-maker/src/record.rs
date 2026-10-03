//! Record mode, the pure part: align key events with the onsets the microphone heard, cut
//! per-key takes and plan the pack. The live capture (`capture.rs`) only feeds this.
//!
//! Privacy: key events reach this module in memory only. What leaves it is per-key audio
//! and counts; the order and timing of what was typed are never returned.

use crate::assemble::{self, BuiltPack, Entry, MAX_POOL, Plan, SetRefs};
use crate::onset::{DetectorConfig, Onset};
use crate::signal::{self, Audio};
use crate::takes::{self, CutConfig, Reject, Take};
use std::collections::BTreeMap;
use taktak_core::audio::consistent_index;
use taktak_core::input::KeyAction;
use taktak_core::key::{Key, KeyGroup};

const GROUPS: [KeyGroup; 6] = [
    KeyGroup::Alphanumeric,
    KeyGroup::Space,
    KeyGroup::Enter,
    KeyGroup::Backspace,
    KeyGroup::Modifier,
    KeyGroup::Other,
];

/// A key transition placed on the recording's timeline.
#[derive(Clone, Copy, Debug)]
pub struct KeyMark {
    pub key: Key,
    pub action: KeyAction,
    /// Position in (fractional) frames of the recording.
    pub frame: f64,
}

/// Maps clock-timebase nanoseconds to recording frames, from per-buffer anchors: the index
/// of each captured buffer's first frame and that frame's capture time.
pub struct ClockMap {
    rate: f64,
    anchors: Vec<(u64, u64)>,
}

impl ClockMap {
    /// Anchors within this distance of a timestamp take part in its local fit.
    const FIT_SPAN_NS: u64 = 1_000_000_000;

    pub fn new(rate: u32) -> ClockMap {
        ClockMap { rate: rate as f64, anchors: Vec::new() }
    }

    /// Adds an anchor; anchors must arrive in frame order.
    pub fn push(&mut self, frame: u64, ns: u64) {
        self.anchors.push((frame, ns));
    }

    /// The (fractional) frame captured at `ns`. A least-squares line through nearby anchors
    /// smooths callback jitter; an implausible slope falls back to the nominal rate.
    pub fn frame_at(&self, ns: u64) -> Option<f64> {
        let last = self.anchors.len().checked_sub(1)?;
        let lo = self.anchors.partition_point(|a| a.1 < ns.saturating_sub(Self::FIT_SPAN_NS));
        let hi = self.anchors.partition_point(|a| a.1 <= ns.saturating_add(Self::FIT_SPAN_NS));
        let near = if hi > lo {
            &self.anchors[lo..hi]
        } else {
            let i = self.anchors.partition_point(|a| a.1 < ns).min(last);
            &self.anchors[i..=i]
        };
        // x: seconds relative to `ns`, y: frames; the fitted line's intercept is the answer.
        let n = near.len() as f64;
        let pts = near.iter().map(|&(f, t)| ((t as f64 - ns as f64) * 1e-9, f as f64));
        let (mx, my) = pts.clone().fold((0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n));
        let (sxx, sxy) =
            pts.fold((0.0, 0.0), |a, p| (a.0 + (p.0 - mx).powi(2), a.1 + (p.0 - mx) * (p.1 - my)));
        let slope = if sxx > 0.0 { sxy / sxx } else { self.rate };
        let slope = if (slope / self.rate - 1.0).abs() <= 0.05 { slope } else { self.rate };
        Some(my - slope * mx)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RecordOptions {
    /// Best takes kept per key and action.
    pub takes: usize,
    /// Onset search window around each event, after latency correction.
    pub window_before_s: f64,
    pub window_after_s: f64,
    /// Fixed event-to-sound offset; `None` measures it from the recording.
    pub latency_s: Option<f64>,
    /// Events closer than this to another event cannot be told apart.
    pub min_separation_s: f64,
    pub detector: DetectorConfig,
    pub cut: CutConfig,
}

impl Default for RecordOptions {
    fn default() -> Self {
        RecordOptions {
            takes: 3,
            window_before_s: 0.030,
            window_after_s: 0.080,
            latency_s: None,
            min_separation_s: 0.025,
            detector: DetectorConfig { threshold_db: -55.0, ..DetectorConfig::default() },
            cut: CutConfig::default(),
        }
    }
}

/// Counts only, never which keys.
#[derive(Clone, Debug, Default)]
pub struct RecordStats {
    pub presses: usize,
    pub releases: usize,
    pub matched: usize,
    pub no_onset: usize,
    pub overlapping: usize,
    pub clipped: usize,
    pub too_quiet: usize,
    pub too_short: usize,
    /// Keys with at least one kept press.
    pub keys: usize,
    pub press_files: usize,
    pub release_files: usize,
    /// Event-to-sound offset used, and whether it was measured from the recording.
    pub latency_ms: f64,
    pub latency_measured: bool,
}

pub fn process(
    audio: &Audio,
    marks: &[KeyMark],
    opts: &RecordOptions,
) -> Result<(BuiltPack, RecordStats), String> {
    let p = takes::prepare(audio, &opts.detector);
    let rate = audio.rate as f64;
    let mut stats = RecordStats {
        presses: marks.iter().filter(|m| m.action == KeyAction::Down).count(),
        releases: marks.iter().filter(|m| m.action == KeyAction::Up).count(),
        ..RecordStats::default()
    };

    let measured = match opts.latency_s {
        Some(_) => None,
        None => estimate_lag(marks, &p.onsets, rate),
    };
    let lag = opts.latency_s.map(|s| s * rate).or(measured).unwrap_or(0.0);
    stats.latency_ms = lag / rate * 1e3;
    stats.latency_measured = measured.is_some();

    let mut sorted = marks.to_vec();
    sorted.sort_by(|a, b| a.frame.total_cmp(&b.frame));
    let assigned = assign(&sorted, &p.onsets, lag, rate, opts, &mut stats);

    // Keystrokes whose sound was not located still bound their neighbours' takes.
    let unlocated: Vec<f64> = sorted
        .iter()
        .zip(&assigned)
        .filter(|(_, a)| a.is_none())
        .map(|(m, _)| m.frame + lag - opts.window_before_s * rate)
        .collect();

    let mut found: BTreeMap<(Key, bool), Vec<Take>> = BTreeMap::new();
    for (m, a) in sorted.iter().zip(&assigned) {
        let Some(oi) = *a else { continue };
        stats.matched += 1;
        let onset = p.onsets[oi].pos;
        let next = p.onsets.get(oi + 1).map(|o| o.pos);
        let limit = unlocated.iter().find(|&&f| f > onset as f64).map(|&f| f as usize);
        let press = m.action == KeyAction::Down;
        match takes::cut(&p, onset, next, limit, press, &opts.cut) {
            Ok(t) => found.entry((m.key, press)).or_default().push(t),
            Err(Reject::Clipped) => stats.clipped += 1,
            Err(Reject::TooQuiet) => stats.too_quiet += 1,
            Err(Reject::TooShort) => stats.too_short += 1,
        }
    }
    let plan = plan(found, opts.takes, audio.rate, p.noise_db, &mut stats);
    if stats.press_files == 0 {
        return Err(no_takes_message(&stats));
    }
    Ok((assemble::assemble(plan)?, stats))
}

/// This recording's typical event-to-sound offset: the median distance from each press to
/// its nearest onset, if at least five presses agree to within 15 ms (median deviation).
fn estimate_lag(marks: &[KeyMark], onsets: &[Onset], rate: f64) -> Option<f64> {
    let (before, after) = (0.150 * rate, 0.300 * rate);
    let lags: Vec<f32> = marks
        .iter()
        .filter(|m| m.action == KeyAction::Down)
        .filter_map(|m| {
            nearest(onsets, m.frame, m.frame - before, m.frame + after)
                .map(|i| (onsets[i].pos as f64 - m.frame) as f32)
        })
        .collect();
    if lags.len() < 5 {
        return None;
    }
    let median = signal::median(&lags)?;
    let deviation: Vec<f32> = lags.iter().map(|l| (l - median).abs()).collect();
    (signal::median(&deviation)? as f64 <= 0.015 * rate).then_some(median as f64)
}

/// Index of the onset nearest to `target` within `[lo, hi]`.
fn nearest(onsets: &[Onset], target: f64, lo: f64, hi: f64) -> Option<usize> {
    let start = onsets.partition_point(|o| (o.pos as f64) < lo);
    onsets[start..]
        .iter()
        .take_while(|o| o.pos as f64 <= hi)
        .enumerate()
        .min_by(|a, b| (a.1.pos as f64 - target).abs().total_cmp(&(b.1.pos as f64 - target).abs()))
        .map(|(i, _)| start + i)
}

/// For each mark (sorted by frame), the onset that is its sound, when that is unambiguous.
fn assign(
    marks: &[KeyMark],
    onsets: &[Onset],
    lag: f64,
    rate: f64,
    opts: &RecordOptions,
    stats: &mut RecordStats,
) -> Vec<Option<usize>> {
    let sep = opts.min_separation_s * rate;
    let mut out: Vec<Option<usize>> = marks
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let crowded = (i > 0 && m.frame - marks[i - 1].frame < sep)
                || marks.get(i + 1).is_some_and(|n| n.frame - m.frame < sep);
            if crowded {
                stats.overlapping += 1;
                return None;
            }
            let expected = m.frame + lag;
            let lo = expected - opts.window_before_s * rate;
            let hi = expected + opts.window_after_s * rate;
            let found = nearest(onsets, expected, lo, hi);
            if found.is_none() {
                stats.no_onset += 1;
            }
            found
        })
        .collect();

    // One sound cannot belong to two keystrokes: drop every claim on a shared onset.
    let mut claims: BTreeMap<usize, usize> = BTreeMap::new();
    for &o in out.iter().flatten() {
        *claims.entry(o).or_default() += 1;
    }
    for a in &mut out {
        if a.is_some_and(|o| claims[&o] > 1) {
            *a = None;
            stats.overlapping += 1;
        }
    }
    out
}

/// Keeps the best takes per key and action, names them, and builds the group pools.
fn plan(
    found: BTreeMap<(Key, bool), Vec<Take>>,
    per_key: usize,
    rate: u32,
    noise_db: f32,
    stats: &mut RecordStats,
) -> Plan {
    let mut entries = Vec::new();
    let mut keys: BTreeMap<Key, SetRefs> = BTreeMap::new();
    for ((key, press), list) in found {
        let loudness: Vec<f32> = list.iter().map(|t| t.loudness_db).collect();
        let mut slots: Vec<Option<Take>> = list.into_iter().map(Some).collect();
        let set = keys.entry(key).or_default();
        let action = if press { "press" } else { "release" };
        for (n, i) in takes::closest_to_median(&loudness, per_key).into_iter().enumerate() {
            let Some(take) = slots[i].take() else { continue };
            if press { &mut set.press } else { &mut set.release }.push(entries.len());
            let stem = format!("{}-{action}-{}", key.code_name(), n + 1);
            entries.push(Entry { stem, take, press });
        }
    }

    let mut groups: BTreeMap<String, SetRefs> = GROUPS
        .iter()
        .map(|&g| {
            let members: Vec<&SetRefs> =
                keys.iter().filter(|(k, _)| k.group() == g).map(|(_, s)| s).collect();
            let set = SetRefs { press: pool(&members, true), release: pool(&members, false) };
            (g.name().to_owned(), set)
        })
        .collect();
    // Every key must sound on press (docs/pack-format.md). Without alphanumeric or other
    // presses, `other` gets a pool drawn from all keys; releases likewise, for consistency.
    let all: Vec<&SetRefs> = keys.values().collect();
    for press in [true, false] {
        let side = |s: &SetRefs| if press { s.press.is_empty() } else { s.release.is_empty() };
        if side(&groups["alphanumeric"]) && side(&groups["other"]) {
            let p = pool(&all, press);
            if let Some(o) = groups.get_mut("other") {
                *(if press { &mut o.press } else { &mut o.release }) = p;
            }
        }
    }

    // Preview: alphanumeric keys in key order, with a space after the fourth if recorded. Each
    // key plays the take the app gives it by default, so the preview sounds like typing.
    let stroke = |key: Key, s: &SetRefs| {
        let pick = |list: &[usize]| list.get(consistent_index(key, list.len())).copied();
        pick(&s.press).map(|p| (p, pick(&s.release)))
    };
    let mut strokes: Vec<_> = keys
        .iter()
        .filter(|(k, _)| k.group() == KeyGroup::Alphanumeric)
        .filter_map(|(&k, s)| stroke(k, s))
        .collect();
    if let Some(space) = keys.get(&Key::Space).and_then(|s| stroke(Key::Space, s)) {
        strokes.insert(strokes.len().min(4), space);
    }
    if strokes.is_empty() {
        strokes = keys.iter().filter_map(|(&k, s)| stroke(k, s)).collect();
    }

    stats.keys = keys.values().filter(|s| !s.press.is_empty()).count();
    stats.press_files = entries.iter().filter(|e| e.press).count();
    stats.release_files = entries.len() - stats.press_files;
    let keys = keys.into_iter().map(|(k, s)| (k.code_name().to_owned(), s)).collect();
    Plan { rate, entries, keys, groups, strokes, noise_db }
}

/// A group pool: each member key's best take in turn, then each one's second best, ….
fn pool(sets: &[&SetRefs], press: bool) -> Vec<usize> {
    let lists: Vec<Vec<usize>> =
        sets.iter().map(|s| if press { s.press.clone() } else { s.release.clone() }).collect();
    takes::round_robin(&lists, MAX_POOL)
}

fn no_takes_message(s: &RecordStats) -> String {
    format!(
        "no usable key-press sounds were captured ({} presses: {} without a clear sound, \
         {} too close to another key, {} clipped, {} too quiet, {} too short).\n\
         Tips: type each key slowly, one at a time; put the microphone 20-30 cm from the \
         keyboard; watch the level meter while typing.",
        s.presses, s.no_onset, s.overlapping, s.clipped, s.too_quiet, s.too_short
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    const RATE: u32 = 48_000;

    fn peak_of(built: &BuiltPack, path: &str) -> f32 {
        signal::peak(&built.sounds.iter().find(|s| s.path == path).unwrap().samples)
    }

    #[test]
    fn preview_plays_the_take_each_key_keeps_by_default() {
        let take =
            |loudness_db| Take { samples: vec![0.1; 480], peak: 0.1, loudness_db, brightness: 0.0 };
        let mut found = BTreeMap::new();
        found.insert((Key::KeyR, true), vec![take(-20.0), take(-21.0), take(-19.0)]);
        found.insert((Key::KeyR, false), vec![take(-30.0), take(-31.0), take(-29.0)]);
        let plan = plan(found, 3, RATE, -70.0, &mut RecordStats::default());
        let set = &plan.keys["KeyR"];
        let i = consistent_index(Key::KeyR, 3);
        assert_ne!(i, 0, "the test needs a key whose default is not its most typical take");
        assert_eq!(plan.strokes, [(set.press[i], Some(set.release[i]))]);
    }

    #[test]
    fn full_session_builds_per_key_sets_and_group_pools() {
        let (audio, marks, _) = session(&typical_session(), -65.0);
        let (built, stats) = process(&audio, &marks, &RecordOptions::default()).unwrap();

        assert_eq!((stats.presses, stats.releases, stats.matched), (20, 20, 40));
        assert_eq!(stats.no_onset + stats.overlapping, 0);
        assert_eq!(stats.clipped + stats.too_quiet + stats.too_short, 0);
        assert!(stats.latency_measured);
        assert!((stats.latency_ms - LAG_S * 1e3).abs() < 0.5, "{}", stats.latency_ms);
        assert_eq!((stats.keys, stats.press_files, stats.release_files), (5, 15, 15));

        let a = &built.keys["KeyA"];
        assert_eq!(
            a.press,
            ["sounds/KeyA-press-1.wav", "sounds/KeyA-press-2.wav", "sounds/KeyA-press-3.wav"]
        );
        assert_eq!(a.release.len(), 3);
        // The 0.95 outlier is not among KeyA's kept takes: their peaks stay within ~1.5 dB.
        let peaks: Vec<f32> = a.press.iter().map(|p| peak_of(&built, p)).collect();
        let spread = signal::amp_db(peaks.iter().copied().fold(0.0, f32::max))
            - signal::amp_db(peaks.iter().copied().fold(1.0, f32::min));
        assert!(spread < 1.5, "{peaks:?}");

        // Releases were paired with releases: quieter than the same key's presses.
        for (name, set) in &built.keys {
            let p = set.press.iter().map(|f| peak_of(&built, f)).fold(0.0, f32::max);
            let r = set.release.iter().map(|f| peak_of(&built, f)).fold(0.0, f32::max);
            assert!(r < p * 0.6, "{name}: release {r} vs press {p}");
        }

        // Pools: per group, alphanumeric interleaves KeyA and KeyS.
        let g = &built.groups;
        assert_eq!(g["alphanumeric"].press.len(), 6);
        assert!(
            g["alphanumeric"].press[0].contains("KeyA")
                && g["alphanumeric"].press[1].contains("KeyS")
        );
        assert_eq!(g["space"].press, built.keys["Space"].press);
        assert_eq!(g["modifiers"].release, built.keys["ShiftLeft"].release);
        assert_eq!(g["other"].press, built.keys["ArrowUp"].press);
        assert!(!g.contains_key("enter") && !g.contains_key("backspace"));

        // Normalization: the median press peaks at -6 dBFS.
        let press_peaks: Vec<f32> = built
            .keys
            .values()
            .flat_map(|s| &s.press)
            .map(|p| signal::amp_db(peak_of(&built, p)))
            .collect();
        assert!((signal::median(&press_peaks).unwrap() + 6.0).abs() < 0.05);

        for s in &built.sounds {
            assert_eq!(*s.samples.last().unwrap(), 0.0, "{} must fade to silence", s.path);
            let lead = s.samples.iter().position(|v| v.abs() >= 0.00316).unwrap();
            assert!(lead <= 24 + 4, "{}: {lead} samples of leading silence", s.path);
        }
    }

    #[test]
    fn crowded_unheard_and_out_of_range_events_are_skipped() {
        let mut strokes = typical_session();
        strokes.truncate(4);
        let (audio, mut marks, _) = session(&strokes, -65.0);
        // Two keys pressed together, 8 ms apart: their sounds cannot be told apart.
        let t = marks[0].frame + signal::frames(RATE, 0.2) as f64;
        marks.push(KeyMark { key: Key::KeyQ, action: KeyAction::Down, frame: t });
        marks.push(KeyMark { key: Key::KeyW, action: KeyAction::Down, frame: t + 384.0 });
        // A key event the microphone did not hear, and one after the recording ended.
        marks.push(KeyMark {
            key: Key::KeyE,
            action: KeyAction::Down,
            frame: marks[2].frame + 9_000.0,
        });
        marks.push(KeyMark { key: Key::KeyR, action: KeyAction::Down, frame: 1e9 });
        let opts = RecordOptions { latency_s: Some(LAG_S), ..RecordOptions::default() };
        let (built, stats) = process(&audio, &marks, &opts).unwrap();
        assert_eq!(stats.overlapping, 2);
        assert_eq!(stats.no_onset, 2);
        assert!(!stats.latency_measured);
        assert_eq!(stats.matched, 8);
        assert_eq!(built.keys.keys().collect::<Vec<_>>(), ["KeyA"]);
    }

    #[test]
    fn without_alphanumeric_keys_other_gets_a_fallback_pool() {
        let strokes: Vec<Stroke> =
            (0..3).map(|i| stroke(Key::Space, 0.4 + i as f32 * 0.02, None)).collect();
        let (audio, marks, _) = session(&strokes, -65.0);
        let (built, stats) = process(&audio, &marks, &RecordOptions::default()).unwrap();
        assert_eq!(stats.release_files, 0);
        assert_eq!(built.groups["other"].press, built.keys["Space"].press);
        assert!(!built.groups.contains_key("alphanumeric"));
    }

    #[test]
    fn nothing_usable_is_an_error_with_advice() {
        let (audio, marks, _) = session(&[stroke(Key::KeyA, 0.4, None)], -65.0);
        let quiet = Audio::mono(RATE, noise(audio.samples.len(), -65.0, 1));
        let err = process(&quiet, &marks, &RecordOptions::default()).unwrap_err();
        assert!(err.contains("no usable key-press sounds"));
    }

    #[test]
    fn output_does_not_depend_on_typing_order() {
        let forward = typical_session();
        let mut reversed = typical_session();
        reversed.reverse();
        let (a1, m1, _) = session(&forward, -90.0);
        let (a2, m2, _) = session(&reversed, -90.0);
        let (b1, _) = process(&a1, &m1, &RecordOptions::default()).unwrap();
        let (b2, _) = process(&a2, &m2, &RecordOptions::default()).unwrap();
        assert_eq!(b1.keys, b2.keys);
        assert_eq!(b1.groups, b2.groups);
        let diff =
            b1.preview.iter().zip(&b2.preview).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
        assert!(diff < 1e-3, "{diff}");
    }

    #[test]
    fn clock_map_smooths_jitter_and_tracks_drift() {
        // 256-frame buffers at 48 kHz on a clock running 50 ppm fast, with ±1 ms jitter.
        let mut map = ClockMap::new(RATE);
        let mut rng = fastrand::Rng::with_seed(3);
        let t0 = 5_000_000_000u64;
        let ns_at = |f: u64| t0 as f64 + f as f64 / RATE as f64 * 1e9 * 1.000_05;
        for b in 0..2_000u64 {
            let f = b * 256;
            let jitter = (rng.f64() * 2.0 - 1.0) * 1e6;
            map.push(f, (ns_at(f) + jitter) as u64);
        }
        for f in [10_000u64, 200_000, 400_000, 500_000] {
            let got = map.frame_at(ns_at(f) as u64).unwrap();
            assert!((got - f as f64).abs() < 0.0002 * RATE as f64, "{f}: {got}");
        }
    }

    #[test]
    fn clock_map_edge_cases() {
        let mut map = ClockMap::new(RATE);
        assert_eq!(map.frame_at(1), None);
        map.push(4_800, 2_000_000_000);
        assert!((map.frame_at(2_100_000_000).unwrap() - 9_600.0).abs() < 1e-6);
        assert!((map.frame_at(1_900_000_000).unwrap() - 0.0).abs() < 1e-6);
        // Exact anchors (CoreAudio host time) map exactly.
        let mut exact = ClockMap::new(RATE);
        for b in 0..100u64 {
            exact.push(b * 512, 1_000_000_000 + b * 512 * 1_000_000_000 / 48_000);
        }
        assert!((exact.frame_at(1_000_000_000 + 250_000_000).unwrap() - 12_000.0).abs() < 0.01);
    }
}
