//! Signal processing for imports: onset detection (leading silence is keypress latency),
//! press/release splitting of whole-keystroke clips, fades, the preview mix and 16-bit WAV
//! output. All buffers are mono at one sample rate.

use crate::pack::decode::{MAX_SAMPLE_SECONDS, PRE_ROLL_SECONDS, SILENCE_THRESHOLD};

/// Imported clips stay a little under the loader's 2.0 s limit, so resampling at load can
/// never push one over.
pub const MAX_CLIP_SECONDS: f32 = MAX_SAMPLE_SECONDS - 0.05;
/// Fade at the end of every clip, so a cut never clicks.
pub const FADE_OUT_SECONDS: f32 = 0.005;
/// The preview's length; it is decoded like a sample, so it must stay under 2.0 s.
pub const PREVIEW_SECONDS: f32 = 1.9;

/// A clip whose peak stays below this (−50 dBFS, the loader's silence threshold) holds no
/// keystroke, only room tone.
const SILENT_PEAK: f32 = SILENCE_THRESHOLD;
/// Onset threshold: this far above the noise floor, and no further than [`BELOW_PEAK_DB`]
/// below the clip's peak (but never above peak − [`MAX_BELOW_PEAK_DB`]). A fixed −50 dBFS
/// threshold triggers early on Vorbis pre-echo and on noisy room tone; the relative one does
/// not.
const ABOVE_FLOOR_DB: f32 = 20.0;
const BELOW_PEAK_DB: f32 = 35.0;
const MAX_BELOW_PEAK_DB: f32 = 20.0;

/// Release search window, as fractions of the press-onset-aligned clip. In hand-cut Mechvibes
/// packs the release starts at 43–80 % of the slice; starting at 35 % keeps the detector off
/// the press's own bottom-out transient.
const SEARCH_FROM: f32 = 0.35;
const SEARCH_TO: f32 = 0.90;
/// A release transient must raise the 1 ms envelope by this much within 2 ms; anything less
/// and the clip is kept whole (as Mechvibes plays it).
pub const MIN_RELEASE_RISE_DB: f32 = 10.0;
/// The cut lands at least this long before the detected release onset.
const CUT_LEAD_SECONDS: f32 = 0.0015;
/// The cut goes in the quietest window of this length within this span before the onset.
const VALLEY_WINDOW_SECONDS: f32 = 0.005;
const VALLEY_SEARCH_SECONDS: f32 = 0.030;
/// Neither half may be shorter than this.
const MIN_PART_SECONDS: f32 = 0.012;
/// How far a release cut out of a sprite may extend past the end of its slice (Mechvibes
/// slices clip the release tail), and the gap kept before the next slice.
pub const RELEASE_EXTENSION_SECONDS: f32 = 0.060;
const NEXT_SLICE_GAP_SECONDS: f32 = 0.005;

fn frames(rate: u32, seconds: f32) -> usize {
    (seconds * rate as f32).round() as usize
}

fn db(amplitude: f32) -> f32 {
    20.0 * (amplitude.max(1e-9)).log10()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// The noise floor: the 10th percentile of the 5 ms RMS windows.
fn noise_floor(samples: &[f32], rate: u32) -> f32 {
    let window = frames(rate, 0.005).max(1);
    let mut levels: Vec<f32> = samples.chunks(window).map(rms).collect();
    if levels.len() < 4 {
        return 0.0;
    }
    levels.sort_by(f32::total_cmp);
    levels[levels.len() / 10]
}

/// The first sample of the sound, or `None` for silence: the first sample reaching
/// max(noise floor + 20 dB, peak − 35 dB).
pub fn onset(samples: &[f32], rate: u32) -> Option<usize> {
    let peak = peak(samples);
    if peak < SILENT_PEAK {
        return None;
    }
    let floor = noise_floor(samples, rate);
    // In a short clip that is loud throughout, the "floor" is the sound itself; the threshold
    // never goes above peak − 20 dB.
    let threshold = (floor * 10f32.powf(ABOVE_FLOOR_DB / 20.0))
        .max(peak * 10f32.powf(-BELOW_PEAK_DB / 20.0))
        .min(peak * 10f32.powf(-MAX_BELOW_PEAK_DB / 20.0));
    samples.iter().position(|s| s.abs() >= threshold)
}

/// Where to start a clip so that it begins at its onset (keeping the 0.5 ms pre-roll the
/// loader keeps), or `None` for silence.
pub fn trimmed_start(samples: &[f32], rate: u32) -> Option<usize> {
    onset(samples, rate).map(|i| i.saturating_sub(frames(rate, PRE_ROLL_SECONDS)))
}

/// For a clip holding a whole keystroke (down-stroke, then up-stroke) that starts at the press
/// onset: where to split it into press and release. The release onset is the largest 2 ms
/// rise of the 1 ms envelope between 35 % and 90 % of the clip; the cut goes in the quiet
/// valley up to 30 ms before it (at least 1.5 ms before). `None` when no rise reaches
/// [`MIN_RELEASE_RISE_DB`] (no distinct release).
///
/// On the official Mechvibes CherryMX Black ABS sprite, against the 48 hand-cut release
/// slices of its repository config, the cut is a median 7 ms from the hand-cut boundary (90 %
/// within 19 ms; one slice has no clear release). A midpoint split, as MechvibesDX does, is a
/// median 32 ms early, inside the press.
pub fn find_release_cut(samples: &[f32], rate: u32) -> Option<usize> {
    let hop = frames(rate, 0.0005).max(1);
    let window = frames(rate, 0.001).max(1);
    let len = samples.len();
    if len < frames(rate, 2.0 * MIN_PART_SECONDS) {
        return None;
    }
    // Envelope frame i covers samples [i*hop, i*hop + window).
    let env: Vec<f32> = (0..len.saturating_sub(window) / hop)
        .map(|i| db(rms(&samples[i * hop..i * hop + window])))
        .collect();
    let step = frames(rate, 0.002).div_ceil(hop).max(1);
    let first = (SEARCH_FROM * len as f32) as usize / hop;
    let last = ((SEARCH_TO * len as f32) as usize / hop).min(env.len().saturating_sub(step + 1));
    let peak_db = db(peak(samples));
    let mut best: Option<(usize, f32)> = None;
    for i in first..last {
        let rise = env[i + step] - env[i];
        // A rise out of noise to a level far below the keystroke is not a release.
        if env[i + step] < peak_db - 45.0 {
            continue;
        }
        if best.is_none_or(|(_, r)| rise > r) {
            best = Some((i, rise));
        }
    }
    let (i, rise) = best?;
    if rise < MIN_RELEASE_RISE_DB {
        return None;
    }
    // The onset is the first frame of the rise that is clearly (6 dB) above where it started.
    let onset_frame = (i..=i + step).find(|&j| env[j] >= env[i] + 6.0).unwrap_or(i + step / 2);
    let onset = onset_frame * hop + window / 2;
    let latest = onset.saturating_sub(frames(rate, CUT_LEAD_SECONDS));
    // Cut in the quiet valley before the release: the quietest 5 ms in the 40 ms before it.
    // Hand-cut packs put the boundary there; the release is trimmed to its onset anyway, so
    // this only keeps the low-level gap (and any faint pre-release tick) out of the press.
    let valley_win = frames(rate, VALLEY_WINDOW_SECONDS).max(1);
    let earliest = latest
        .saturating_sub(frames(rate, VALLEY_SEARCH_SECONDS))
        .max((SEARCH_FROM * len as f32) as usize / 2);
    let mut cut = latest;
    if latest > earliest + valley_win {
        let levels: Vec<(usize, f32)> = (earliest..=latest - valley_win)
            .step_by(hop)
            .map(|pos| (pos, rms(&samples[pos..pos + valley_win])))
            .collect();
        let quietest = levels.iter().map(|&(_, l)| l).fold(f32::MAX, f32::min);
        // The latest of the windows within 1 dB of the quietest: no more press tail lost
        // than needed.
        if let Some(&(pos, _)) = levels.iter().rev().find(|&&(_, l)| l <= quietest * 1.413) {
            cut = pos + valley_win / 2;
        }
    }
    let min_part = frames(rate, MIN_PART_SECONDS);
    (cut >= min_part && cut + min_part <= len).then_some(cut)
}

/// How far a release cut out of `source` may run past `end` (exclusive): up to
/// [`RELEASE_EXTENSION_SECONDS`], never within 5 ms of `next_start` (the next slice in the
/// sprite), and stopping once the 5 ms level is within 6 dB of the noise floor.
pub fn extended_release_end(
    source: &[f32],
    rate: u32,
    end: usize,
    next_start: Option<usize>,
) -> usize {
    let mut limit = (end + frames(rate, RELEASE_EXTENSION_SECONDS)).min(source.len());
    if let Some(next) = next_start {
        limit = limit.min(next.saturating_sub(frames(rate, NEXT_SLICE_GAP_SECONDS)).max(end));
    }
    if limit <= end {
        return end;
    }
    let window = frames(rate, 0.005).max(1);
    let context_start = end.saturating_sub(frames(rate, 0.2));
    let floor = noise_floor(&source[context_start..limit], rate);
    let quiet = floor * 10f32.powf(6.0 / 20.0);
    let mut pos = end;
    while pos + window <= limit {
        if rms(&source[pos..pos + window]) <= quiet {
            break;
        }
        pos += window;
    }
    pos.min(limit)
}

/// Caps a clip at [`MAX_CLIP_SECONDS`], fades in over the pre-roll and out over 5 ms.
/// Returns whether it was truncated.
pub fn finish(samples: &mut Vec<f32>, rate: u32) -> bool {
    let max = frames(rate, MAX_CLIP_SECONDS);
    let truncated = samples.len() > max;
    samples.truncate(max);
    let fade_in = frames(rate, PRE_ROLL_SECONDS).min(samples.len());
    for (i, s) in samples.iter_mut().take(fade_in).enumerate() {
        *s *= i as f32 / fade_in as f32;
    }
    fade_out(samples, frames(rate, FADE_OUT_SECONDS));
    truncated
}

fn fade_out(samples: &mut [f32], len: usize) {
    let len = len.min(samples.len());
    let n = samples.len();
    for (k, s) in samples[n - len..].iter_mut().enumerate() {
        *s *= (len - k) as f32 / (len as f32 + 1.0);
    }
}

/// A preview: each `(press, release)` stroke starts on a fixed, made-up rhythm (it never
/// reflects anybody's typing), releases follow after a short hold. Peak-limited to −1 dBFS,
/// [`PREVIEW_SECONDS`] long, faded out.
pub fn mix_preview(strokes: &[(&[f32], Option<&[f32]>)], rate: u32) -> Vec<f32> {
    const STEPS: [f32; 9] = [0.14, 0.12, 0.16, 0.13, 0.15, 0.17, 0.12, 0.15, 0.13];
    const HOLDS: [f32; 4] = [0.075, 0.09, 0.07, 0.085];
    let mut out = vec![0.0f32; frames(rate, PREVIEW_SECONDS)];
    let mut t = 0.0;
    for (n, (press, release)) in strokes.iter().enumerate() {
        let mut mix = |clip: &[f32], at: f32| {
            for (o, s) in out.iter_mut().skip(frames(rate, at)).zip(clip) {
                *o += s;
            }
        };
        mix(press, t);
        if let Some(release) = release {
            mix(release, t + HOLDS[n % HOLDS.len()]);
        }
        t += STEPS[n % STEPS.len()];
    }
    let limit = 10f32.powf(-1.0 / 20.0);
    let p = peak(&out);
    if p > limit {
        out.iter_mut().for_each(|s| *s *= limit / p);
    }
    fade_out(&mut out, frames(rate, 0.02));
    out
}

/// 16-bit PCM mono WAV.
pub fn wav_bytes(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::pack::decode;

    pub const RATE: u32 = 44_100;

    /// A deterministic noise burst: `ms` long, decaying with time constant `tau_ms`.
    pub fn burst(ms: f32, amp: f32, tau_ms: f32, seed: u32) -> Vec<f32> {
        let mut x = seed.wrapping_mul(2_654_435_761).max(1);
        (0..frames(RATE, ms / 1000.0))
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let noise = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
                amp * noise * (-(i as f32) / (tau_ms / 1000.0 * RATE as f32)).exp()
            })
            .collect()
    }

    /// A synthetic whole keystroke: `lead_ms` of room tone, a press (click + bottom-out),
    /// then a release starting `release_ms` after the press. Returns the samples and the
    /// release onset (in samples from the start).
    pub fn keystroke(lead_ms: f32, release_ms: f32, total_ms: f32, seed: u32) -> (Vec<f32>, usize) {
        let n = frames(RATE, total_ms / 1000.0);
        let mut out: Vec<f32> = burst(total_ms, 0.0008, 1e9, seed ^ 7);
        let mut add = |at_ms: f32, b: Vec<f32>| {
            let at = frames(RATE, at_ms / 1000.0);
            for (o, s) in out.iter_mut().skip(at).zip(b) {
                *o += s;
            }
        };
        add(lead_ms, burst(40.0, 0.6, 6.0, seed));
        add(lead_ms + 12.0, burst(40.0, 0.35, 8.0, seed + 1));
        add(lead_ms + release_ms, burst(50.0, 0.3, 7.0, seed + 2));
        out.truncate(n);
        (out, frames(RATE, (lead_ms + release_ms) / 1000.0))
    }

    #[test]
    fn onset_skips_lead_in_and_room_tone() {
        let (k, _) = keystroke(20.0, 120.0, 220.0, 1);
        let at = onset(&k, RATE).unwrap();
        let expected = frames(RATE, 0.020);
        assert!(at.abs_diff(expected) <= frames(RATE, 0.0005), "{at} vs {expected}");
        assert_eq!(onset(&vec![0.0; 4410], RATE), None);
        assert_eq!(onset(&[0.5, 0.0, 0.0], RATE), Some(0));
        // Short and loud throughout: no quiet part to measure a floor in.
        let loud: Vec<f32> = (0..900).map(|i| 0.8 * (-(i as f32) / 900.0).exp()).collect();
        assert_eq!(onset(&loud, RATE), Some(0));
    }

    #[test]
    fn release_split_finds_the_second_transient() {
        for (seed, release_ms, total_ms) in [(1, 95.0, 190.0), (2, 125.0, 186.0), (3, 80.0, 151.0)]
        {
            let (k, release) = keystroke(0.0, release_ms, total_ms, seed);
            let cut = find_release_cut(&k, RATE).expect("split");
            // In the quiet gap: after the press bursts (which end 52 ms in), before the release.
            assert!(
                cut >= frames(RATE, 0.052) && cut < release,
                "seed {seed}: cut {cut} release {release}"
            );
            assert!(
                release - cut <= frames(RATE, 0.030),
                "seed {seed}: cut {cut} release {release}"
            );
        }
    }

    #[test]
    fn single_transient_clips_are_not_split() {
        let mut k = burst(60.0, 0.6, 6.0, 9);
        k.extend(burst(120.0, 0.0008, 1e9, 10));
        assert_eq!(find_release_cut(&k, RATE), None);
        assert_eq!(find_release_cut(&[0.1; 100], RATE), None);
    }

    #[test]
    fn release_extension_stops_at_quiet_and_at_the_next_slice() {
        let mut src = burst(100.0, 0.5, 30.0, 3);
        src.extend(vec![0.0; frames(RATE, 0.2)]);
        let end = frames(RATE, 0.050);
        let ext = extended_release_end(&src, RATE, end, None);
        assert!(ext > end && ext <= end + frames(RATE, RELEASE_EXTENSION_SECONDS));
        let next = end + frames(RATE, 0.010);
        assert!(extended_release_end(&src, RATE, end, Some(next)) <= next);
        let quiet_at = frames(RATE, 0.150);
        assert_eq!(extended_release_end(&src, RATE, quiet_at, None), quiet_at);
    }

    #[test]
    fn finish_caps_and_fades() {
        let mut long = vec![0.5f32; frames(RATE, 3.0)];
        assert!(finish(&mut long, RATE));
        assert_eq!(long.len(), frames(RATE, MAX_CLIP_SECONDS));
        assert!(long[0].abs() < 1e-6 && long.last().unwrap().abs() < 0.01);
        let mut short = vec![0.5f32; 100];
        assert!(!finish(&mut short, RATE));
        assert_eq!(short.len(), 100);
    }

    #[test]
    fn wav_round_trips_through_the_pack_decoder() {
        let samples: Vec<f32> = (0..441).map(|i| ((i as f32) * 0.05).sin() * 0.5).collect();
        let pcm = decode::decode(wav_bytes(&samples, RATE), "wav").unwrap();
        assert_eq!(pcm.sample_rate, RATE);
        assert_eq!(pcm.samples.len(), samples.len());
        assert!(pcm.samples.iter().zip(&samples).all(|(a, b)| (a - b).abs() < 1e-3));
    }

    #[test]
    fn preview_is_limited_and_short_enough() {
        let press = burst(80.0, 0.9, 10.0, 4);
        let release = burst(60.0, 0.9, 10.0, 5);
        let strokes: Vec<(&[f32], Option<&[f32]>)> =
            (0..10).map(|_| (&press[..], Some(&release[..]))).collect();
        let out = mix_preview(&strokes, RATE);
        assert_eq!(out.len(), frames(RATE, PREVIEW_SECONDS));
        assert!(peak(&out) <= 10f32.powf(-1.0 / 20.0) + 1e-6);
        assert!((PREVIEW_SECONDS as f64) < f64::from(MAX_SAMPLE_SECONDS));
    }
}
