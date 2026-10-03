//! Objective measurements of the generated samples, so reviewers who cannot listen can check
//! that the packs differ as intended. Definitions match the research report's §6.5.

use crate::dsp::{Biquad, FS, db_to_gain, gain_to_db, peak, rms, samples};
use std::f64::consts::{PI, TAU};

const FFT_SIZE: usize = 1 << 14;
/// True peak: 4x oversampling with a Hann-windowed sinc of this many taps on each side.
const TP_OVERSAMPLE: usize = 4;
const TP_HALF_TAPS: usize = 32;
/// Only samples within this far of the sample peak (and their neighbours) are interpolated:
/// the overs between quieter samples cannot come near it.
const TP_SEARCH_DB: f64 = -12.0;

/// In-place iterative radix-2 FFT; `re.len()` must be a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -TAU / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (w_re, w_im) = ((ang * k as f64).cos(), (ang * k as f64).sin());
                let (a, b) = (start + k, start + k + len / 2);
                let t_re = re[b] * w_re - im[b] * w_im;
                let t_im = re[b] * w_im + im[b] * w_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
            }
        }
        len <<= 1;
    }
}

/// Power spectrum of the first `window_ms` (Hann window, zero-padded), with bin frequencies.
fn power_spectrum(x: &[f64], window_ms: f64) -> Vec<(f64, f64)> {
    let n = samples(window_ms).min(FFT_SIZE);
    let mut re = vec![0.0; FFT_SIZE];
    let mut im = vec![0.0; FFT_SIZE];
    for (i, r) in re.iter_mut().enumerate().take(n.min(x.len())) {
        let w = 0.5 - 0.5 * (TAU * i as f64 / (n - 1) as f64).cos();
        *r = x[i] * w;
    }
    fft(&mut re, &mut im);
    (0..=FFT_SIZE / 2)
        .map(|k| (k as f64 * FS / FFT_SIZE as f64, re[k] * re[k] + im[k] * im[k]))
        .collect()
}

/// Power-spectrum centroid over the first `window_ms`, in Hz.
pub fn centroid(x: &[f64], window_ms: f64) -> f64 {
    let s = power_spectrum(x, window_ms);
    let total: f64 = s.iter().map(|b| b.1).sum();
    s.iter().map(|b| b.0 * b.1).sum::<f64>() / total.max(1e-30)
}

/// Share of the first `window_ms`'s power above `hz`.
pub fn share_above(x: &[f64], window_ms: f64, hz: f64) -> f64 {
    let s = power_spectrum(x, window_ms);
    let total: f64 = s.iter().map(|b| b.1).sum();
    s.iter().filter(|b| b.0 >= hz).map(|b| b.1).sum::<f64>() / total.max(1e-30)
}

/// Peak over RMS of the first 60 ms, in dB. Higher means a sharper, more impulsive sound.
pub fn crest_db(x: &[f64]) -> f64 {
    gain_to_db(peak(x) / rms(&x[..samples(60.0).min(x.len())]).max(1e-12))
}

/// K-weighted (ITU-R BS.1770) mean-square energy over a fixed 100 ms from the onset, as a
/// power ratio. A loudness proxy for one keystroke: fixed length, so a longer tail counts.
pub fn k_energy(x: &[f64]) -> f64 {
    let n = samples(100.0);
    let mut y = x[..n.min(x.len())].to_vec();
    for bq in Biquad::k_weighting() {
        bq.process(&mut y);
    }
    y.iter().map(|s| s * s).sum::<f64>() / n as f64
}

/// Power ratio to "LUFS-like" dB (BS.1770's −0.691 offset).
pub fn k_db(energy: f64) -> f64 {
    -0.691 + 10.0 * energy.max(1e-30).log10()
}

/// True peak (ITU-R BS.1770 Annex 2): the largest magnitude of `x` and of the band-limited
/// signal between its samples, found by 4x oversampling. It is what resampling (the loader's,
/// to the device rate) can reach. Silence is assumed before and after `x`, as when it plays.
pub fn true_peak(x: &[f64]) -> f64 {
    let sample_peak = peak(x);
    let floor = sample_peak * db_to_gain(TP_SEARCH_DB);
    let half = TP_HALF_TAPS as isize;
    // One filter per fractional position k/4 between two samples: tap j weighs x[n + j].
    let phases: Vec<Vec<f64>> = (1..TP_OVERSAMPLE)
        .map(|k| {
            let frac = k as f64 / TP_OVERSAMPLE as f64;
            (1 - half..=half)
                .map(|j| {
                    let t = j as f64 - frac;
                    let sinc = (PI * t).sin() / (PI * t);
                    sinc * (0.5 + 0.5 * (PI * t / half as f64).cos())
                })
                .collect()
        })
        .collect();
    let mut best = sample_peak;
    for n in 0..x.len() {
        if x[n].abs() < floor && x.get(n + 1).is_none_or(|v| v.abs() < floor) {
            continue;
        }
        for taps in &phases {
            let first = n as isize + 1 - half;
            let y: f64 = taps
                .iter()
                .enumerate()
                .filter_map(|(i, c)| {
                    x.get(usize::try_from(first + i as isize).ok()?).map(|v| v * c)
                })
                .sum();
            best = best.max(y.abs());
        }
    }
    best
}

/// Time until the 1 ms peak envelope stays below `db` re the peak, in ms.
pub fn decay_ms(x: &[f64], db: f64) -> f64 {
    let thr = peak(x) * 10f64.powf(db / 20.0);
    let w = samples(1.0);
    let last = x.chunks(w).rposition(|c| peak(c) > thr).map_or(0, |i| i + 1);
    last as f64
}

/// Measurements of one sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    pub dur_ms: f64,
    pub peak: f64,
    pub rms60: f64,
    pub centroid: f64,
    pub attack_centroid: f64,
    pub hf_attack: f64,
    pub crest_db: f64,
    pub k_energy: f64,
    pub l40_ms: f64,
}

pub fn measure(x: &[f64]) -> Metrics {
    Metrics {
        dur_ms: x.len() as f64 * 1e3 / FS,
        peak: peak(x),
        rms60: rms(&x[..samples(60.0).min(x.len())]),
        centroid: centroid(x, 60.0),
        attack_centroid: centroid(x, 12.0),
        hf_attack: share_above(x, 5.0, 4000.0),
        crest_db: crest_db(x),
        k_energy: k_energy(x),
        l40_ms: decay_ms(x, -40.0),
    }
}

/// Summary of a set of samples: means, except the peak (maximum).
#[derive(Clone, Copy, Debug, Default)]
pub struct Summary {
    pub count: usize,
    pub dur_ms: f64,
    pub peak_db: f64,
    pub rms_db: f64,
    pub centroid: f64,
    pub attack_centroid: f64,
    pub hf_attack: f64,
    pub crest_db: f64,
    pub k_db: f64,
    pub l40_ms: f64,
}

pub fn summarize(ms: &[Metrics]) -> Summary {
    let n = ms.len().max(1) as f64;
    let mean = |f: fn(&Metrics) -> f64| ms.iter().map(f).sum::<f64>() / n;
    Summary {
        count: ms.len(),
        dur_ms: mean(|m| m.dur_ms),
        peak_db: gain_to_db(ms.iter().fold(0.0, |a, m| a.max(m.peak))),
        rms_db: 10.0 * mean(|m| m.rms60 * m.rms60).max(1e-30).log10(),
        centroid: mean(|m| m.centroid),
        attack_centroid: mean(|m| m.attack_centroid),
        hf_attack: mean(|m| m.hf_attack),
        crest_db: mean(|m| m.crest_db),
        k_db: k_db(mean(|m| m.k_energy)),
        l40_ms: mean(|m| m.l40_ms),
    }
}

pub const TABLE_HEADER: &str = "group          action   n  dur ms  L40 ms  peak dBFS  RMS60 dBFS  \
                                LK dB  centroid Hz  attack Hz  HF>4k %  crest dB";

pub fn table_row(group: &str, action: &str, s: &Summary) -> String {
    format!(
        "{group:<14} {action:<7} {:>3} {:>7.0} {:>7.0} {:>10.1} {:>11.1} {:>6.1} {:>12.0} {:>10.0} \
         {:>8.1} {:>9.1}",
        s.count,
        s.dur_ms,
        s.l40_ms,
        s.peak_db,
        s.rms_db,
        s.k_db,
        s.centroid,
        s.attack_centroid,
        100.0 * s.hf_attack,
        s.crest_db
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(f: f64, ms: f64) -> Vec<f64> {
        (0..samples(ms)).map(|n| (TAU * f * n as f64 / FS).sin()).collect()
    }

    #[test]
    fn centroid_of_a_sine_is_its_frequency() {
        for f in [500.0, 3000.0] {
            let c = centroid(&sine(f, 80.0), 60.0);
            assert!((c - f).abs() / f < 0.02, "{f} Hz → {c}");
        }
    }

    #[test]
    fn hf_share_and_crest_behave() {
        assert!(share_above(&sine(6000.0, 10.0), 5.0, 4000.0) > 0.95);
        assert!(share_above(&sine(1000.0, 10.0), 5.0, 4000.0) < 0.05);
        assert!((crest_db(&sine(1000.0, 80.0)) - 3.01).abs() < 0.1);
    }

    #[test]
    fn decay_time_of_a_damped_sine() {
        // −40 dB is a factor of 100: e^(−t/τ) = 0.01 at t = τ·ln 100 ≈ 4.6 τ.
        let tau_ms = 10.0;
        let x: Vec<f64> = sine(1000.0, 100.0)
            .iter()
            .enumerate()
            .map(|(n, s)| s * (-(n as f64) / (tau_ms * 1e-3 * FS)).exp())
            .collect();
        let got = decay_ms(&x, -40.0);
        assert!((got - 46.0).abs() <= 1.5, "L40 = {got} ms");
    }

    #[test]
    fn true_peak_finds_the_overs_between_samples() {
        // A quarter-rate sine at 45°: every sample is ±0.707, the waveform peaks at 1.0.
        let x: Vec<f64> = (0..480).map(|n| (TAU * n as f64 / 4.0 + PI / 4.0).sin()).collect();
        assert!((peak(&x) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        assert!((gain_to_db(true_peak(&x))).abs() < 0.1, "{}", true_peak(&x));
        // A slow sine has no overs: the true peak is the sample peak.
        let slow = sine(500.0, 20.0);
        assert!((true_peak(&slow) - peak(&slow)).abs() < 1e-3);
        assert_eq!(true_peak(&[]), 0.0);
    }

    #[test]
    fn k_weighting_of_a_full_scale_1k_sine_is_about_minus_3() {
        // BS.1770: a 0 dBFS 997 Hz sine reads −3.01 LKFS.
        let x = sine(997.0, 400.0);
        let e = k_energy(&x[samples(200.0)..]);
        assert!((k_db(e) + 3.01).abs() < 0.1, "{}", k_db(e));
    }
}
