//! Procedurally synthesized key sounds. Used by the Milestone 1 spike and as a built-in
//! fallback when no pack is available; being generated, they carry no licensing baggage.

/// Shape of a synthesized keystroke.
#[derive(Clone, Copy, Debug)]
pub struct ClickParams {
    pub duration_s: f32,
    /// Broadband "tick" of the switch/stem.
    pub transient_gain: f32,
    pub transient_decay_s: f32,
    /// Resonances of the keycap/plate, (Hz, gain, decay seconds).
    pub modes: [(f32, f32, f32); 3],
    pub seed: u64,
}

impl ClickParams {
    /// A short, fairly deep "thock" for key-down.
    pub const PRESS: ClickParams = ClickParams {
        duration_s: 0.07,
        transient_gain: 0.9,
        transient_decay_s: 0.0012,
        modes: [(310.0, 0.55, 0.012), (1850.0, 0.35, 0.006), (4200.0, 0.2, 0.003)],
        seed: 0x7a6b_7a6b,
    };

    /// A lighter, higher "tack" for key-up.
    pub const RELEASE: ClickParams = ClickParams {
        duration_s: 0.04,
        transient_gain: 0.45,
        transient_decay_s: 0.0008,
        modes: [(520.0, 0.25, 0.006), (2600.0, 0.25, 0.004), (5200.0, 0.15, 0.002)],
        seed: 0x7a6b_0001,
    };
}

/// Renders a mono click at `sample_rate`, peak-normalized to `peak`.
pub fn click(params: &ClickParams, sample_rate: u32, peak: f32) -> Box<[f32]> {
    let sr = sample_rate as f32;
    let len = ((params.duration_s * sr) as usize).max(2);
    let mut rng = fastrand::Rng::with_seed(params.seed);
    let mut lp = 0.0f32;
    let mut out: Vec<f32> = (0..len)
        .map(|n| {
            let t = n as f32 / sr;
            // One-pole low-passed noise keeps the transient from sounding like hiss.
            lp += 0.45 * ((rng.f32() * 2.0 - 1.0) - lp);
            let mut s = params.transient_gain * lp * (-t / params.transient_decay_s).exp();
            for &(freq, gain, decay) in &params.modes {
                s += gain * (std::f32::consts::TAU * freq * t).sin() * (-t / decay).exp();
            }
            s
        })
        .collect();

    // 2 ms fade-out so truncation never clicks.
    let fade = ((0.002 * sr) as usize).min(len);
    for (i, s) in out[len - fade..].iter_mut().enumerate() {
        *s *= 1.0 - i as f32 / fade as f32;
    }
    *out.last_mut().unwrap() = 0.0;

    let max = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if max > 0.0 {
        let g = peak / max;
        out.iter_mut().for_each(|s| *s *= g);
    }
    out.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_is_normalized_deterministic_and_ends_silent() {
        let a = click(&ClickParams::PRESS, 48_000, 0.8);
        let b = click(&ClickParams::PRESS, 48_000, 0.8);
        assert_eq!(a, b);
        assert_eq!(a.len(), (0.07 * 48_000.0) as usize);
        let peak = a.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.8).abs() < 1e-4);
        assert_eq!(*a.last().unwrap(), 0.0);
        assert!(a.iter().all(|s| s.is_finite()));
    }
}
