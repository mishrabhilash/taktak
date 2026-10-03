//! Synthetic signals for tests: noise floors, keystroke-like transients at known positions,
//! and whole typing sessions with matching key events.

use crate::record::KeyMark;
use crate::signal::{self, Audio, db_amp};
use taktak_core::input::KeyAction;
use taktak_core::key::Key;
use taktak_core::synth::{ClickParams, click};

pub const RATE: u32 = 48_000;
/// Key events precede their sounds by this much in [`session`].
pub const LAG_S: f64 = 0.012;

/// White noise at `rms_db` dBFS RMS (Irwin-Hall approximation of a Gaussian).
pub fn noise(len: usize, rms_db: f32, seed: u64) -> Vec<f32> {
    let mut rng = fastrand::Rng::with_seed(seed);
    let amp = db_amp(rms_db);
    (0..len).map(|_| amp * ((0..4).map(|_| rng.f32()).sum::<f32>() - 2.0) * 3f32.sqrt()).collect()
}

pub fn add(dst: &mut [f32], src: &[f32], at: usize) {
    for (d, s) in dst.iter_mut().skip(at).zip(src) {
        *d += s;
    }
}

pub fn press_click(rate: u32, seed: u64, peak: f32) -> Vec<f32> {
    click(&ClickParams { seed, ..ClickParams::PRESS }, rate, peak).into_vec()
}

/// A press whose low mode rings for a long time, so later sounds land on its tail.
pub fn long_press_click(rate: u32, seed: u64, peak: f32) -> Vec<f32> {
    let mut p = ClickParams { seed, duration_s: 0.15, ..ClickParams::PRESS };
    p.modes[0].2 = 0.030;
    click(&p, rate, peak).into_vec()
}

pub fn release_click(rate: u32, seed: u64, peak: f32) -> Vec<f32> {
    click(&ClickParams { seed, ..ClickParams::RELEASE }, rate, peak).into_vec()
}

pub struct Stroke {
    pub key: Key,
    pub press_peak: f32,
    pub release_peak: Option<f32>,
}

pub fn stroke(key: Key, press_peak: f32, release_peak: Option<f32>) -> Stroke {
    Stroke { key, press_peak, release_peak }
}

/// Strokes 400 ms apart, each key held 110 ms, every key with its own click. Events precede
/// their sounds by [`LAG_S`], as when the input path reports earlier than the mic hears.
/// Returns the audio, the key events and the true sound positions.
pub fn session(strokes: &[Stroke], noise_db: f32) -> (Audio, Vec<KeyMark>, Vec<usize>) {
    let step = signal::frames(RATE, 0.4);
    let hold = signal::frames(RATE, 0.11);
    let mut x = noise(step * (strokes.len() + 1), noise_db, 9);
    let mut marks = Vec::new();
    let mut sounds = Vec::new();
    let lag = LAG_S * RATE as f64;
    for (i, s) in strokes.iter().enumerate() {
        let seed = s.key.index() as u64;
        let at = step / 2 + i * step;
        add(&mut x, &press_click(RATE, seed, s.press_peak), at);
        marks.push(KeyMark { key: s.key, action: KeyAction::Down, frame: at as f64 - lag });
        sounds.push(at);
        if let Some(r) = s.release_peak {
            add(&mut x, &release_click(RATE, seed, r), at + hold);
            sounds.push(at + hold);
        }
        marks.push(KeyMark { key: s.key, action: KeyAction::Up, frame: (at + hold) as f64 - lag });
    }
    (Audio::mono(RATE, x), marks, sounds)
}

/// Five keys from four groups, four strokes each; KeyA's third press is a loud outlier.
pub fn typical_session() -> Vec<Stroke> {
    let mut s = Vec::new();
    for (key, peaks) in [
        (Key::KeyA, [0.40, 0.45, 0.95, 0.42]),
        (Key::KeyS, [0.35, 0.38, 0.36, 0.40]),
        (Key::Space, [0.50, 0.55, 0.52, 0.53]),
        (Key::ShiftLeft, [0.30, 0.32, 0.31, 0.29]),
        (Key::ArrowUp, [0.33, 0.34, 0.30, 0.35]),
    ] {
        for p in peaks {
            s.push(stroke(key, p, Some(p * 0.4)));
        }
    }
    s
}
