//! DSP building blocks: seeded noise, contact pulses, modal resonators and RBJ biquads.
//! Everything is f64 and allocation-light; the generator runs offline.

use std::f64::consts::{PI, TAU};

/// Output sample rate of every generated file.
pub const FS: f64 = 48_000.0;

/// Milliseconds to whole samples (rounded).
pub fn samples(ms: f64) -> usize {
    (ms.max(0.0) * 1e-3 * FS).round() as usize
}

pub fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

pub fn gain_to_db(g: f64) -> f64 {
    20.0 * g.max(1e-12).log10()
}

/// 64-bit FNV-1a. Used for seeds because `std`'s hashers are not stable across releases.
pub fn hash(label: &str) -> u64 {
    label
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// SplitMix64: tiny, fast and plenty for noise and jitter. Seeded per file, so every output
/// is reproducible on its own.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn from_label(label: &str) -> Rng {
        Rng(hash(label))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform in [lo, hi).
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.uniform()
    }

    /// Uniform integer in [lo, hi].
    pub fn int(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % u64::from(hi - lo + 1)) as u32
    }

    /// Standard normal (Box–Muller).
    pub fn gauss(&mut self) -> f64 {
        let u1 = 1.0 - self.uniform();
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos()
    }

    /// Multiplier 1 ± `spread` (uniform).
    pub fn jitter(&mut self, spread: f64) -> f64 {
        1.0 + self.range(-spread, spread)
    }

    /// Gain of ± `db` decibels (uniform in dB).
    pub fn jitter_db(&mut self, db: f64) -> f64 {
        db_to_gain(self.range(-db, db))
    }
}

/// Adds a raised-cosine contact force 1 − cos(2πk/M) (van den Doel et al. 2001) lasting
/// `dur_ms` from `at_ms`, scaled to an area of `amp` so low modes ring at about their gain.
pub fn add_pulse(buf: &mut [f64], dur_ms: f64, at_ms: f64, amp: f64) {
    let m = samples(dur_ms).max(2);
    let start = samples(at_ms);
    // The sum of 1 − cos(2πk/m) over one period is exactly m.
    let scale = amp / m as f64;
    for (k, s) in buf.iter_mut().skip(start).take(m).enumerate() {
        *s += scale * (1.0 - (TAU * k as f64 / m as f64).cos());
    }
}

/// White Gaussian noise under an exponential envelope e^(−t/τ) that starts at `at_ms`.
pub fn noise_burst(rng: &mut Rng, len: usize, tau_ms: f64, at_ms: f64) -> Vec<f64> {
    let mut out = vec![0.0; len];
    let start = samples(at_ms);
    let k = -1.0 / (tau_ms * 1e-3 * FS);
    // e^(−40) is far below 16-bit resolution; stop drawing there.
    let stop = (start + (40.0 / -k) as usize).min(len);
    for (n, s) in out.iter_mut().enumerate().take(stop).skip(start) {
        *s = rng.gauss() * (k * (n - start) as f64).exp();
    }
    out
}

/// Adds `gain` × one damped-sine mode driven by `x` (J. O. Smith's two-pole resonator,
/// impulse-invariant form). Its impulse response is r^(n−1)·sin(nθ): it starts at zero (no
/// onset click) and peaks at about `gain` for a unit-area excitation. τ is the amplitude
/// e-folding time; T60 = 6.91·τ.
pub fn add_mode(x: &[f64], f: f64, tau_s: f64, gain: f64, out: &mut [f64]) {
    if !(f > 0.0 && f < 0.48 * FS && tau_s > 0.0) {
        return;
    }
    let th = TAU * f / FS;
    let r = (-1.0 / (tau_s * FS)).exp();
    let (a1, a2, b1) = (2.0 * r * th.cos(), r * r, gain * th.sin());
    let (mut y1, mut y2, mut x1) = (0.0, 0.0, 0.0);
    for (o, &xn) in out.iter_mut().zip(x) {
        let y = a1 * y1 - a2 * y2 + b1 * x1;
        y2 = y1;
        y1 = y;
        x1 = xn;
        *o += y;
    }
}

/// Ren et al. 2013 "plastic" Rayleigh damping, d = (α + βω²)/2, as an e-folding time in seconds.
pub fn plastic_tau(f: f64, mult: f64) -> f64 {
    let w = TAU * f;
    mult / ((52.627 + 8.7753e-7 * w * w) / 2.0)
}

/// Normalized second-order IIR section, Direct Form I.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    fn rbj(f0: f64, q: f64, b: [f64; 3]) -> Biquad {
        let w0 = TAU * f0 / FS;
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        Biquad {
            b0: b[0] / a0,
            b1: b[1] / a0,
            b2: b[2] / a0,
            a1: -2.0 * w0.cos() / a0,
            a2: (1.0 - alpha) / a0,
        }
    }

    /// RBJ band-pass with 0 dB gain at `f0`.
    pub fn bandpass(f0: f64, q: f64) -> Biquad {
        let alpha = (TAU * f0 / FS).sin() / (2.0 * q);
        Biquad::rbj(f0, q, [alpha, 0.0, -alpha])
    }

    pub fn lowpass(f0: f64, q: f64) -> Biquad {
        let c = (TAU * f0 / FS).cos();
        Biquad::rbj(f0, q, [(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0])
    }

    pub fn highpass(f0: f64, q: f64) -> Biquad {
        let c = (TAU * f0 / FS).cos();
        Biquad::rbj(f0, q, [(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0])
    }

    /// The two ITU-R BS.1770 K-weighting stages (pre-filter shelf, RLB high-pass) at 48 kHz.
    pub fn k_weighting() -> [Biquad; 2] {
        [
            Biquad {
                b0: 1.535_124_859_586_97,
                b1: -2.691_696_189_406_38,
                b2: 1.198_392_810_852_85,
                a1: -1.690_659_293_182_41,
                a2: 0.732_480_774_215_85,
            },
            Biquad {
                b0: 1.0,
                b1: -2.0,
                b2: 1.0,
                a1: -1.990_047_454_833_98,
                a2: 0.990_072_250_366_21,
            },
        ]
    }

    pub fn process(&self, x: &mut [f64]) {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        for s in x.iter_mut() {
            let xn = *s;
            let y = self.b0 * xn + self.b1 * x1 + self.b2 * x2 - self.a1 * y1 - self.a2 * y2;
            x2 = x1;
            x1 = xn;
            y2 = y1;
            y1 = y;
            *s = y;
        }
    }

    /// Magnitude response at `f`.
    #[cfg(test)]
    pub fn gain_at(&self, f: f64) -> f64 {
        let w = TAU * f / FS;
        let z1 = (w.cos(), -w.sin());
        let z2 = ((2.0 * w).cos(), -(2.0 * w).sin());
        let num = (self.b0 + self.b1 * z1.0 + self.b2 * z2.0, self.b1 * z1.1 + self.b2 * z2.1);
        let den = (1.0 + self.a1 * z1.0 + self.a2 * z2.0, self.a1 * z1.1 + self.a2 * z2.1);
        (num.0.hypot(num.1)) / (den.0.hypot(den.1))
    }
}

/// `dst += gain · src`, over the shorter of the two.
pub fn mix(dst: &mut [f64], src: &[f64], gain: f64) {
    for (d, s) in dst.iter_mut().zip(src) {
        *d += gain * s;
    }
}

/// Raised-cosine fade over the last `n` samples, ending at exactly zero.
pub fn fade_out(x: &mut [f64], n: usize) {
    let len = x.len();
    let n = n.min(len);
    if n == 0 {
        return;
    }
    for (i, s) in x[len - n..].iter_mut().enumerate() {
        let t = (i + 1) as f64 / n as f64;
        *s *= 0.5 * (1.0 + (PI * t).cos());
    }
}

pub fn peak(x: &[f64]) -> f64 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

pub fn rms(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|s| s * s).sum::<f64>() / x.len() as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse_response(f: f64, tau_s: f64, len: usize) -> Vec<f64> {
        let mut x = vec![0.0; len];
        x[0] = 1.0;
        let mut y = vec![0.0; len];
        add_mode(&x, f, tau_s, 1.0, &mut y);
        y
    }

    /// |DFT| of `x` at frequency `f`.
    fn dft_mag(x: &[f64], f: f64) -> f64 {
        let w = TAU * f / FS;
        let (re, im) = x.iter().enumerate().fold((0.0, 0.0), |(re, im), (n, s)| {
            (re + s * (w * n as f64).cos(), im - s * (w * n as f64).sin())
        });
        re.hypot(im)
    }

    #[test]
    fn mode_response_peaks_at_its_frequency() {
        for &f in &[300.0, 2000.0, 6500.0] {
            let h = impulse_response(f, 0.02, 4800);
            let (best, _) = (0..=200)
                .map(|i| f * (0.8 + 0.4 * i as f64 / 200.0))
                .map(|g| (g, dft_mag(&h, g)))
                .fold((0.0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
            assert!((best - f).abs() / f < 0.01, "peak at {best} Hz for a {f} Hz mode");
        }
    }

    #[test]
    fn mode_starts_at_zero_and_decays_with_tau() {
        let tau = 0.010;
        let h = impulse_response(1000.0, tau, 4800);
        assert_eq!(h[0], 0.0);
        // One period is 48 samples; compare the envelope one τ apart.
        let env = |at: usize| peak(&h[at..at + 48]);
        let ratio = env(48 + samples(tau * 1e3)) / env(48);
        assert!((ratio - (-1.0f64).exp()).abs() < 0.03, "decay ratio {ratio}");
        assert!(peak(&h) <= 1.0 && peak(&h) > 0.9);
    }

    #[test]
    fn pulse_has_the_requested_area_and_starts_at_zero() {
        let mut buf = vec![0.0; 200];
        add_pulse(&mut buf, 0.45, 1.0, 0.7);
        assert!((buf.iter().sum::<f64>() - 0.7).abs() < 1e-12);
        assert_eq!(buf[samples(1.0)], 0.0);
        assert!(buf[..samples(1.0)].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn noise_burst_decays() {
        let mut rng = Rng::new(7);
        let n = noise_burst(&mut rng, 4800, 1.0, 0.0);
        let early = rms(&n[..48]);
        let late = rms(&n[samples(5.0)..samples(6.0)]);
        assert!(late < early * 0.05, "early {early}, late {late}");
        assert!(n.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn biquads_have_the_expected_shape() {
        let bp = Biquad::bandpass(3000.0, 0.8);
        assert!((bp.gain_at(3000.0) - 1.0).abs() < 1e-9);
        assert!(bp.gain_at(300.0) < 0.2 && bp.gain_at(20000.0) < 0.5);
        let lp = Biquad::lowpass(2000.0, 0.707);
        assert!((lp.gain_at(10.0) - 1.0).abs() < 1e-3 && lp.gain_at(12000.0) < 0.05);
        let hp = Biquad::highpass(80.0, 0.707);
        assert!(hp.gain_at(10.0) < 0.02 && (hp.gain_at(5000.0) - 1.0).abs() < 1e-3);
        // K-weighting: about +4 dB above 2 kHz, about 0 dB at 1 kHz relative to its shelf.
        let [s, r] = Biquad::k_weighting();
        let k = |f: f64| gain_to_db(s.gain_at(f) * r.gain_at(f));
        assert!((k(1000.0) - 0.7).abs() < 0.5, "K(1 kHz) = {}", k(1000.0));
        assert!((k(8000.0) - 4.0).abs() < 0.5, "K(8 kHz) = {}", k(8000.0));
    }

    #[test]
    fn plastic_damping_matches_the_research_table() {
        // keyboard-acoustics.md §3.2: 33 ms at 500 Hz, 10.5 ms at 2 kHz, 3.3 ms at 4 kHz.
        for (f, ms) in [(500.0, 33.0), (2000.0, 10.5), (4000.0, 3.3)] {
            let got = plastic_tau(f, 1.0) * 1e3;
            assert!((got - ms).abs() / ms < 0.05, "{f} Hz: {got} ms");
        }
    }

    #[test]
    fn rng_is_deterministic_and_in_range() {
        let mut a = Rng::from_label("deep-thock/KeyA-press");
        let mut b = Rng::from_label("deep-thock/KeyA-press");
        let mut c = Rng::from_label("deep-thock/KeyB-press");
        let xa: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let xb: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        let xc: Vec<u64> = (0..8).map(|_| c.next_u64()).collect();
        assert_eq!(xa, xb);
        assert_ne!(xa, xc);
        let mut r = Rng::new(1);
        let (mut sum, mut sq) = (0.0, 0.0);
        for _ in 0..20_000 {
            let u = r.range(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&u));
            assert!((4..=6).contains(&r.int(4, 6)));
            let g = r.gauss();
            sum += g;
            sq += g * g;
        }
        assert!((sum / 20_000.0).abs() < 0.03 && (sq / 20_000.0 - 1.0).abs() < 0.05);
    }

    #[test]
    fn fade_ends_at_exact_zero() {
        let mut x = vec![1.0; 100];
        fade_out(&mut x, 40);
        assert_eq!(x[99], 0.0);
        assert_eq!(x[59], 1.0);
        assert!(x[60] < 1.0 && x[60] > 0.99);
        assert!(x.windows(2).skip(60).all(|w| w[1] <= w[0]));
    }
}
