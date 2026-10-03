//! One keystroke event (press or release), rendered from the physical model:
//!
//! - excitation: a raised-cosine contact pulse with 1–3 micro-bounces, stabilizer flams and a
//!   band-passed noise burst (friction and micro-collisions);
//! - keycap/stem modes (plastic damping law) driven by the contact pulse;
//! - plate/case "body" modes driven by a longer, smoother pulse (the force that reaches them);
//! - optional layers: click jacket, spring ping, hollow-case cavity, stabilizer rattle;
//! - two early reflections and a short noise tail (desk and case), then HPF, LPF, level, fade.
//!
//! t = 0 is the bottom-out (linear) or the click (clicky): the physical event arrives about as
//! late after the OS key event as TakTak's output latency, so samples start with no silence.

use crate::dsp::{
    Biquad, FS, Rng, add_mode, add_pulse, db_to_gain, fade_out, mix, noise_burst, peak, rms,
    samples,
};
use crate::layout::Voice;
use crate::params::{Action, Class, Mode, PackParams};

/// Per-take randomness, so the takes in a pool (and a key's two takes, which alternate when
/// the app picks variants at random) are different strokes rather than copies.
#[derive(Clone, Copy, Debug)]
pub struct Take {
    /// Striking force spread, ± dB. Harder strokes are also brighter (shorter contact).
    pub velocity_db: f64,
}

/// Renders one event. Deterministic for a given `rng` state.
pub fn render(
    p: &PackParams,
    class: Class,
    action: Action,
    voice: &Voice,
    take: Take,
    rng: &mut Rng,
) -> Vec<f64> {
    let a = p.action(action);
    let c = p.class(class);
    let len = samples(a.length_ms * c.len_mult);
    let mut out = vec![0.0; len];

    let vel = rng.jitter_db(take.velocity_db);
    // Hertz contact: t_c ∝ v^(−1/5), so a harder stroke has a shorter, brighter impact.
    let hardness = vel.powf(-0.2);

    let mut t0 = 0.0;
    if let Some(click) = &a.click {
        let gain = click.gain * rng.jitter_db(1.5);
        let mut exc = vec![0.0; len];
        add_pulse(&mut exc, click.tau_ms * rng.jitter(0.15), 0.0, gain);
        for mode in click.modes {
            let f = mode.f * rng.jitter(0.02);
            let tau = mode.tau_s(f, 1.0) * rng.jitter(0.1);
            add_mode(&exc, f, tau, mode.g, &mut out);
        }
        let mut nz = noise_burst(rng, len, click.noise.tau_ms, 0.0);
        Biquad::bandpass(click.noise.fc, click.noise.q).process(&mut nz);
        mix(&mut out, &nz, click.noise.g * gain);
        t0 = click.delay_ms * rng.jitter(0.2);
    }

    // Stabilizer stems landing a little after the centre switch.
    let flams: Vec<(f64, f64)> = match action {
        Action::Press => c.flam_press,
        Action::Release => c.flam_release,
    }
    .iter()
    .map(|fl| (t0 + rng.range(fl.lo_ms, fl.hi_ms), fl.g * vel))
    .collect();

    // Keycap/stem excitation: the contact pulse, its bounces and the flams.
    let tc = a.contact_ms * c.tc_mult * hardness * rng.jitter(0.15);
    let mut exc = vec![0.0; len];
    add_pulse(&mut exc, tc, t0, vel);
    let mut tb = t0;
    for k in 1..rng.int(a.bounces.0, a.bounces.1) {
        tb += rng.range(0.4, 1.6);
        add_pulse(&mut exc, tc * 0.8, tb, vel * 0.35f64.powi(k as i32));
    }
    for &(at, g) in &flams {
        add_pulse(&mut exc, tc * 1.2, at, g);
    }

    let cap_f = c.cap_fmult * a.cap_fmult * voice.cap_f;
    let decay = c.decay_mult * a.decay_mult;
    for mode in p.cap {
        let f = mode.f * cap_f * rng.jitter(0.015);
        let tau = mode.tau_s(f, p.cap_damp_mult) * decay * rng.jitter(0.1);
        add_mode(&exc, f, tau, mode.g * rng.jitter_db(1.5), &mut out);
    }
    // A long cap (space bar) has its own bending modes, struck through the stem.
    add_modes(&exc, c.bar, Scale { decay, ..Scale::PLAIN }, rng, &mut out);

    // Plate/case excitation: smoother, because the switch and plate filter the impact.
    let mut bexc = vec![0.0; len];
    add_pulse(&mut bexc, p.body_contact_ms * c.tc_mult, t0, vel);
    for &(at, g) in &flams {
        add_pulse(&mut bexc, p.body_contact_ms, at, g);
    }
    for (i, mode) in p.body.iter().enumerate() {
        let f = mode.f * c.body_fmult * rng.jitter(0.005);
        let tau = mode.tau_s(f, 1.0) * decay * rng.jitter(0.08);
        let g = mode.g * a.body_g * voice.body.get(i).copied().unwrap_or(1.0) * rng.jitter_db(1.0);
        add_mode(&bexc, f, tau, g, &mut out);
    }

    let mut nz = noise_burst(rng, len, a.noise.tau_ms, t0);
    Biquad::bandpass(a.noise.fc * rng.jitter(0.1), a.noise.q).process(&mut nz);
    mix(&mut out, &nz, a.noise.g * vel);

    if let Some(cv) = p.cavity {
        let mut cz = noise_burst(rng, len, cv.tau_ms, t0);
        Biquad::bandpass(cv.fc, cv.q).process(&mut cz);
        mix(&mut out, &cz, cv.g * a.cavity_g * vel);
    }

    if !p.ping.is_empty() {
        let mut pe = vec![0.0; len];
        add_pulse(&mut pe, 0.1, t0, vel);
        // Each switch has its own spring, so each key pings at its own pitch.
        let scale = Scale { f: voice.ping_f, gain: a.ping_g, ..Scale::PLAIN };
        add_modes(&pe, p.ping, scale, rng, &mut out);
    }

    if let Some(r) = c.rattle {
        for _ in 0..rng.int(r.count.0, r.count.1) {
            let mut te = vec![0.0; len];
            let at = t0 + rng.range(r.t_ms.0, r.t_ms.1);
            add_pulse(&mut te, 0.08, at, r.g * rng.jitter_db(3.0));
            add_modes(&te, r.modes, Scale { f_spread: 0.03, ..Scale::PLAIN }, rng, &mut out);
        }
    }

    // Desk and case: two filtered early reflections and a short decaying noise tail.
    let dry = out.clone();
    for &(d_ms, g, lp) in p.reflections {
        let mut refl = dry.clone();
        Biquad::lowpass(lp, 0.707).process(&mut refl);
        let d = samples(d_ms);
        mix(&mut out[d.min(len)..], &refl, g);
    }
    let mut tail = vec![0.0; len];
    let start = samples(t0);
    let k = -6.91 / (p.tail.t60_ms * 1e-3 * FS);
    for (n, s) in tail.iter_mut().enumerate().skip(start) {
        *s = rng.gauss() * (k * (n - start) as f64).exp();
    }
    Biquad::lowpass(p.tail.lp, 0.707).process(&mut tail);
    let reference = rms(&dry[..samples(10.0).min(len)]);
    let tail_peak = peak(&tail).max(1e-12);
    mix(&mut out, &tail, 3.0 * p.tail.g * reference / tail_peak);

    Biquad::highpass(p.hpf, 0.707).process(&mut out);
    Biquad::lowpass(p.lpf, 0.707).process(&mut out);
    let gain = db_to_gain(a.level_db + c.level_db + voice.level_db);
    out.iter_mut().for_each(|s| *s *= gain);
    fade_out(&mut out, samples(p.fade_ms));
    out
}

/// Multipliers for a bank of modes with explicit decays.
#[derive(Clone, Copy, Debug)]
struct Scale {
    f: f64,
    gain: f64,
    decay: f64,
    /// Per-take frequency jitter, ± fraction.
    f_spread: f64,
}

impl Scale {
    const PLAIN: Scale = Scale { f: 1.0, gain: 1.0, decay: 1.0, f_spread: 0.01 };
}

fn add_modes(exc: &[f64], modes: &[Mode], s: Scale, rng: &mut Rng, out: &mut [f64]) {
    for mode in modes {
        let f = mode.f * s.f * rng.jitter(s.f_spread);
        add_mode(exc, f, mode.tau_s(f, 1.0) * s.decay, mode.g * s.gain, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{centroid, crest_db, share_above};
    use crate::dsp::gain_to_db;
    use crate::params::PACKS;

    const CLASSES: [Class; 7] = [
        Class::Alnum,
        Class::Space,
        Class::Enter,
        Class::Backspace,
        Class::Modifier,
        Class::Shift,
        Class::Other,
    ];
    const TAKE: Take = Take { velocity_db: 1.0 };

    fn one(p: &PackParams, class: Class, action: Action, seed: u64) -> Vec<f64> {
        render(p, class, action, &Voice::NEUTRAL, TAKE, &mut Rng::new(seed))
    }

    #[test]
    fn render_is_deterministic_per_seed() {
        for p in PACKS {
            let a = one(p, Class::Space, Action::Press, 11);
            let b = one(p, Class::Space, Action::Press, 11);
            let c = one(p, Class::Space, Action::Press, 12);
            assert_eq!(a, b);
            assert_ne!(a, c);
        }
    }

    #[test]
    fn every_event_is_finite_starts_immediately_and_ends_at_zero() {
        for p in PACKS {
            for class in CLASSES {
                for action in [Action::Press, Action::Release] {
                    for seed in 0..3 {
                        let x = one(p, class, action, seed);
                        assert!(x.iter().all(|s| s.is_finite()), "{} NaN", p.id);
                        assert_eq!(*x.last().unwrap(), 0.0);
                        // Onset: the loader's -50 dBFS threshold (with the sample normalized
                        // to -3 dBFS) is crossed within 0.5 ms.
                        let thr = peak(&x) * 0.00316 / 0.708;
                        let onset = x.iter().position(|s| s.abs() >= thr).unwrap();
                        assert!(onset <= samples(0.5), "{} onset at {onset}", p.id);
                        // No DC.
                        let mean = x.iter().sum::<f64>() / x.len() as f64;
                        assert!(mean.abs() < 1e-3 * peak(&x), "{} DC {mean}", p.id);
                    }
                }
            }
        }
    }

    fn avg(p: &PackParams, action: Action, f: impl Fn(&[f64]) -> f64) -> f64 {
        (0..6).map(|s| f(&one(p, Class::Alnum, action, 100 + s))).sum::<f64>() / 6.0
    }

    #[test]
    fn packs_sound_as_intended() {
        let [thock, clack, click] = PACKS;
        let cen = |x: &[f64]| centroid(x, 60.0);
        let attack = |x: &[f64]| centroid(x, 12.0);
        let hf = |x: &[f64]| share_above(x, 5.0, 4000.0);
        let level = |x: &[f64]| gain_to_db(rms(&x[..samples(60.0).min(x.len())]));
        // Thock is clearly darker than clack.
        assert!(avg(thock, Action::Press, cen) < 0.5 * avg(clack, Action::Press, cen));
        // The click is a strong high-frequency transient: several times more energy above
        // 4 kHz in its first 5 ms than the clack, the brightest attack, the highest crest.
        assert!(avg(click, Action::Press, hf) > 4.0 * avg(clack, Action::Press, hf));
        assert!(avg(click, Action::Press, attack) > 1.2 * avg(clack, Action::Press, attack));
        let crest = |p| avg(p, Action::Press, crest_db);
        assert!(crest(click) > crest(clack) && crest(clack) > crest(thock) + 3.0);
        // Releases are quieter and brighter than presses.
        for p in PACKS {
            let d = avg(p, Action::Release, level) - avg(p, Action::Press, level);
            assert!(d < -3.0, "{}: release {d:.1} dB re press", p.id);
            assert!(avg(p, Action::Release, cen) > avg(p, Action::Press, cen), "{}", p.id);
        }
    }

    #[test]
    fn space_is_longer_and_lower_than_alphanumeric() {
        for p in PACKS {
            let a = one(p, Class::Alnum, Action::Press, 5);
            let s = one(p, Class::Space, Action::Press, 5);
            assert!(s.len() > a.len());
            assert!(centroid(&s, 60.0) < centroid(&a, 60.0), "{}", p.id);
        }
    }
}
