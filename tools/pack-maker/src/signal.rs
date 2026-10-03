//! Mono audio buffers and the small DSP toolkit the pack maker needs.

use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};
use std::ops::Range;

/// A source channel at or above this magnitude counts as clipped.
pub const CLIP_LEVEL: f32 = 0.999;

/// Rate of written samples when the source is neither 44.1 nor 48 kHz.
pub const OUTPUT_RATE: u32 = 48_000;

/// Mono audio, plus where the (possibly multichannel) source clipped.
#[derive(Clone, Debug, Default)]
pub struct Audio {
    pub rate: u32,
    pub samples: Vec<f32>,
    /// Frame ranges in which any source channel reached [`CLIP_LEVEL`], in order.
    pub clipped: Vec<Range<usize>>,
}

impl Audio {
    /// Wraps mono samples, detecting clipped runs.
    #[cfg(test)]
    pub fn mono(rate: u32, samples: Vec<f32>) -> Audio {
        let mut runs = ClipRuns::default();
        let mut clipped: Vec<_> = samples
            .iter()
            .enumerate()
            .filter_map(|(i, s)| runs.feed(i, s.abs() >= CLIP_LEVEL))
            .collect();
        clipped.extend(runs.finish(samples.len()));
        Audio { rate, samples, clipped }
    }

    pub fn frames(&self, secs: f64) -> usize {
        frames(self.rate, secs)
    }

    pub fn is_clipped(&self, r: Range<usize>) -> bool {
        let i = self.clipped.partition_point(|c| c.end <= r.start);
        self.clipped.get(i).is_some_and(|c| c.start < r.end)
    }
}

pub fn frames(rate: u32, secs: f64) -> usize {
    (secs * rate as f64).round().max(0.0) as usize
}

/// Mean of one interleaved frame, and whether any channel clipped.
pub fn mix_frame(frame: &[f32]) -> (f32, bool) {
    let mut sum = 0.0;
    let mut clip = false;
    for &s in frame {
        sum += s;
        clip |= s.abs() >= CLIP_LEVEL;
    }
    (sum / frame.len().max(1) as f32, clip)
}

/// Turns per-frame clip flags into ranges, across buffer boundaries.
#[derive(Default)]
pub struct ClipRuns {
    start: Option<usize>,
}

impl ClipRuns {
    /// Feeds frame `i`; returns a finished run when clipping stops.
    pub fn feed(&mut self, i: usize, clipped: bool) -> Option<Range<usize>> {
        match (self.start, clipped) {
            (None, true) => {
                self.start = Some(i);
                None
            }
            (Some(s), false) => {
                self.start = None;
                Some(s..i)
            }
            _ => None,
        }
    }

    pub fn finish(&mut self, end: usize) -> Option<Range<usize>> {
        self.start.take().map(|s| s..end)
    }
}

/// 2nd-order Butterworth high-pass (RBJ cookbook), transposed direct form II in f64.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    pub fn highpass(rate: u32, cutoff_hz: f64) -> Biquad {
        let w0 = TAU * cutoff_hz.min(0.45 * rate as f64) / rate as f64;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let b0 = (1.0 + cos) / 2.0 / a0;
        Biquad { b: [b0, -2.0 * b0, b0], a: [-2.0 * cos / a0, (1.0 - alpha) / a0], z: [0.0; 2] }
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y as f32
    }
}

pub fn highpass(x: &[f32], rate: u32, cutoff_hz: f64) -> Vec<f32> {
    let mut f = Biquad::highpass(rate, cutoff_hz);
    x.iter().map(|&s| f.process(s)).collect()
}

/// Amplitude to dBFS, floored at -160 dB so silence stays finite.
pub fn amp_db(a: f32) -> f32 {
    20.0 * a.max(1e-8).log10()
}

/// Mean-square power to dB, floored at -160 dB.
pub fn power_db(p: f64) -> f32 {
    (10.0 * p.max(1e-16).log10()) as f32
}

pub fn db_amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

pub fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|&s| s as f64 * s as f64).sum::<f64>() / x.len() as f64).sqrt() as f32
}

/// Nearest-rank percentile, `p` in `[0, 1]`.
pub fn percentile(values: &[f32], p: f64) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(f32::total_cmp);
    let i = (p.clamp(0.0, 1.0) * (v.len() - 1) as f64).round() as usize;
    Some(v[i])
}

pub fn median(values: &[f32]) -> Option<f32> {
    percentile(values, 0.5)
}

/// Raised-cosine fade to exactly zero over the last `n` samples.
pub fn fade_out(x: &mut [f32], n: usize) {
    let n = n.min(x.len());
    let start = x.len() - n;
    for (i, s) in x[start..].iter_mut().enumerate() {
        *s *= (0.5 * (1.0 + (PI * (i + 1) as f64 / n as f64).cos())) as f32;
    }
}

/// Raised-cosine fade from exactly zero over the first `n` samples.
pub fn fade_in(x: &mut [f32], n: usize) {
    let n = n.min(x.len());
    for (i, s) in x[..n].iter_mut().enumerate() {
        *s *= (0.5 * (1.0 - (PI * i as f64 / n as f64).cos())) as f32;
    }
}

/// Band-limited resampling with a Blackman-windowed sinc (24 zero crossings per side).
/// Offline and only applied to short takes, so clarity beats speed here.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    const ZEROS: f64 = 24.0;
    let ratio = to as f64 / from as f64;
    // Cutoff relative to the input Nyquist; a little headroom keeps the transition band
    // below the output Nyquist.
    let cutoff = ratio.min(1.0) * 0.95;
    let half = ZEROS / cutoff;
    let out_len = (x.len() as f64 * ratio).round() as usize;
    (0..out_len)
        .map(|m| {
            let t = m as f64 / ratio;
            let lo = (t - half).ceil().max(0.0) as usize;
            let hi = ((t + half).floor() as usize).min(x.len() - 1);
            let mut acc = 0.0;
            for (k, &s) in x.iter().enumerate().take(hi + 1).skip(lo) {
                let d = t - k as f64;
                let u = d / half;
                let window = 0.42 + 0.5 * (PI * u).cos() + 0.08 * (TAU * u).cos();
                let arg = PI * cutoff * d;
                let sinc = if arg.abs() < 1e-9 { 1.0 } else { arg.sin() / arg };
                acc += s as f64 * cutoff * sinc * window;
            }
            acc as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, secs: f64, amp: f32) -> Vec<f32> {
        (0..frames(rate, secs))
            .map(|n| amp * (TAU * freq * n as f64 / rate as f64).sin() as f32)
            .collect()
    }

    fn zero_crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
    }

    #[test]
    fn clip_runs_and_lookup() {
        let mut x = vec![0.0; 100];
        x[10] = 1.0;
        x[11] = -1.0;
        x[50] = 0.9995;
        x[99] = 1.0;
        let a = Audio::mono(48_000, x);
        assert_eq!(a.clipped, vec![10..12, 50..51, 99..100]);
        assert!(a.is_clipped(0..11));
        assert!(!a.is_clipped(12..50));
        assert!(a.is_clipped(40..60));
        assert!(a.is_clipped(99..120));
    }

    #[test]
    fn highpass_removes_dc_and_rumble_but_keeps_clicks() {
        let rate = 48_000;
        let dc: Vec<f32> = vec![0.5; rate as usize];
        assert!(peak(&highpass(&dc, rate, 30.0)[24_000..]) < 1e-3);
        let low = sine(20.0, rate, 1.0, 0.5);
        assert!(rms(&highpass(&low, rate, 150.0)[24_000..]) < 0.5 * 0.71 * 0.03);
        let high = sine(3000.0, rate, 1.0, 0.5);
        let out = highpass(&high, rate, 150.0);
        assert!((rms(&out[24_000..]) / rms(&high[24_000..]) - 1.0).abs() < 0.01);
    }

    #[test]
    fn fades_reach_zero() {
        let mut x = vec![1.0; 100];
        fade_out(&mut x, 10);
        fade_in(&mut x, 10);
        assert_eq!(x[99], 0.0);
        assert_eq!(x[0], 0.0);
        assert!(x[50] == 1.0 && x[89] == 1.0 && x[10] == 1.0);
        assert!(x[95] > 0.0 && x[95] < 1.0);
    }

    #[test]
    fn percentiles() {
        let v = [5.0, 1.0, 3.0, 2.0, 4.0];
        assert_eq!(median(&v), Some(3.0));
        assert_eq!(percentile(&v, 0.0), Some(1.0));
        assert_eq!(percentile(&v, 1.0), Some(5.0));
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn resample_preserves_frequency_and_level() {
        for (from, to) in [(96_000, 48_000), (22_050, 48_000), (32_000, 48_000)] {
            let x = sine(1000.0, from, 0.5, 0.5);
            let y = resample(&x, from, to);
            assert_eq!(y.len(), frames(to, 0.5));
            let mid = &y[y.len() / 4..3 * y.len() / 4];
            let secs = mid.len() as f64 / to as f64;
            let freq = zero_crossings(mid) as f64 / 2.0 / secs;
            assert!((freq - 1000.0).abs() < 5.0, "{from}->{to}: {freq} Hz");
            assert!((rms(mid) / (0.5 * FRAC_1_SQRT_2 as f32) - 1.0).abs() < 0.01, "{from}->{to}");
        }
    }

    #[test]
    fn downsampling_rejects_content_above_the_new_nyquist() {
        let x = sine(30_000.0, 96_000, 0.5, 0.5);
        let y = resample(&x, 96_000, 48_000);
        assert!(rms(&y[2_000..y.len() - 2_000]) < 0.005);
    }
}
