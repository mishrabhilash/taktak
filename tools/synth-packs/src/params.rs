//! The three experimental (not bundled) packs as synthesis parameters. Values come from the calibrated table in
//! the M2 acoustics research (keyboard-acoustics.md §6): mode lists are Hz : gain (τ ms),
//! gains are linear amplitudes for a unit-area excitation.

use crate::dsp::plastic_tau;

#[derive(Clone, Copy, Debug)]
pub struct Mode {
    pub f: f64,
    pub g: f64,
    /// Amplitude e-folding time in ms; `None` follows the plastic damping law.
    pub tau_ms: Option<f64>,
}

const fn m(f: f64, g: f64, tau_ms: f64) -> Mode {
    Mode { f, g, tau_ms: Some(tau_ms) }
}

/// A keycap/stem mode whose decay follows Ren et al.'s plastic law.
const fn pl(f: f64, g: f64) -> Mode {
    Mode { f, g, tau_ms: None }
}

impl Mode {
    /// Decay in seconds at the (possibly shifted) frequency `f`.
    pub fn tau_s(&self, f: f64, plastic_mult: f64) -> f64 {
        match self.tau_ms {
            Some(ms) => ms * 1e-3,
            None => plastic_tau(f, plastic_mult),
        }
    }
}

/// Band-passed noise burst: friction and micro-collisions, or a hollow case's cavity.
#[derive(Clone, Copy, Debug)]
pub struct Noise {
    pub fc: f64,
    pub q: f64,
    pub g: f64,
    pub tau_ms: f64,
}

/// A click-jacket event: a very short, hard pulse into its own bright modes, followed
/// `delay_ms` later by the bottom-out (press) or top-out (release).
#[derive(Clone, Copy, Debug)]
pub struct Click {
    pub tau_ms: f64,
    pub gain: f64,
    pub delay_ms: f64,
    pub modes: &'static [Mode],
    pub noise: Noise,
}

#[derive(Clone, Copy, Debug)]
pub struct ActionParams {
    pub level_db: f64,
    pub length_ms: f64,
    pub contact_ms: f64,
    /// Number of impact peaks (1 = no bounce), inclusive range.
    pub bounces: (u32, u32),
    pub cap_fmult: f64,
    pub decay_mult: f64,
    pub body_g: f64,
    pub ping_g: f64,
    pub cavity_g: f64,
    pub noise: Noise,
    pub click: Option<Click>,
}

/// One delayed stabilizer sub-impact: time range in ms and relative gain.
#[derive(Clone, Copy, Debug)]
pub struct Flam {
    pub lo_ms: f64,
    pub hi_ms: f64,
    pub g: f64,
}

/// Stabilizer wire ticks after the impact.
#[derive(Clone, Copy, Debug)]
pub struct Rattle {
    pub count: (u32, u32),
    pub t_ms: (f64, f64),
    pub g: f64,
    pub modes: &'static [Mode],
}

#[derive(Clone, Copy, Debug)]
pub struct Tail {
    pub g: f64,
    pub t60_ms: f64,
    pub lp: f64,
}

/// Which kind of key a sample is for. Finer than `KeyGroup`: shifts have stabilizers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Alnum,
    Space,
    Enter,
    Backspace,
    Modifier,
    Shift,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Press,
    Release,
}

/// Key-class modifiers applied on top of the press/release parameters (§6.3).
#[derive(Clone, Copy, Debug)]
pub struct ClassParams {
    pub level_db: f64,
    pub cap_fmult: f64,
    pub body_fmult: f64,
    pub decay_mult: f64,
    pub tc_mult: f64,
    pub len_mult: f64,
    pub bar: &'static [Mode],
    pub flam_press: &'static [Flam],
    pub flam_release: &'static [Flam],
    pub rattle: Option<Rattle>,
}

const PLAIN: ClassParams = ClassParams {
    level_db: 0.0,
    cap_fmult: 1.0,
    body_fmult: 1.0,
    decay_mult: 1.0,
    tc_mult: 1.0,
    len_mult: 1.0,
    bar: &[],
    flam_press: &[],
    flam_release: &[],
    rattle: None,
};

#[derive(Clone, Copy, Debug)]
pub struct PackParams {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub variation_pitch: f32,
    pub variation_volume: f32,
    pub fade_ms: f64,
    pub hpf: f64,
    pub lpf: f64,
    pub cap: &'static [Mode],
    pub cap_damp_mult: f64,
    pub body: &'static [Mode],
    pub body_contact_ms: f64,
    pub cavity: Option<Noise>,
    pub ping: &'static [Mode],
    /// Early reflections: (delay ms, gain, low-pass Hz).
    pub reflections: &'static [(f64, f64, f64)],
    pub tail: Tail,
    pub press: ActionParams,
    pub release: ActionParams,
    /// Level offsets in dB for space, enter and backspace (they differ by build).
    pub big_key_db: [f64; 3],
    pub space_bar: &'static [Mode],
    pub space_rattle: Rattle,
    pub stab_rattle: Rattle,
}

const SPACE_FLAM: &[Flam] =
    &[Flam { lo_ms: 0.3, hi_ms: 1.5, g: 0.5 }, Flam { lo_ms: 0.6, hi_ms: 2.5, g: 0.35 }];
const SPACE_FLAM_RELEASE: &[Flam] = &[Flam { lo_ms: 0.2, hi_ms: 1.0, g: 0.4 }];
const ENTER_FLAM: &[Flam] = &[Flam { lo_ms: 0.2, hi_ms: 0.8, g: 0.4 }];
const BACKSPACE_FLAM: &[Flam] = &[Flam { lo_ms: 0.2, hi_ms: 0.7, g: 0.35 }];
const SPACE_TICK: &[Mode] = &[m(4800.0, 1.0, 4.0), m(7300.0, 0.6, 2.5)];
const STAB_TICK: &[Mode] = &[m(5200.0, 1.0, 4.0), m(7900.0, 0.6, 2.5)];

impl PackParams {
    pub fn action(&self, a: Action) -> &ActionParams {
        match a {
            Action::Press => &self.press,
            Action::Release => &self.release,
        }
    }

    pub fn class(&self, c: Class) -> ClassParams {
        let [space_db, enter_db, backspace_db] = self.big_key_db;
        match c {
            Class::Alnum => PLAIN,
            Class::Space => ClassParams {
                level_db: space_db,
                cap_fmult: 0.85,
                body_fmult: 0.9,
                decay_mult: 1.35,
                tc_mult: 1.4,
                len_mult: 1.4,
                bar: self.space_bar,
                flam_press: SPACE_FLAM,
                flam_release: SPACE_FLAM_RELEASE,
                rattle: Some(self.space_rattle),
            },
            Class::Enter => ClassParams {
                level_db: enter_db,
                cap_fmult: 0.92,
                decay_mult: 1.15,
                tc_mult: 1.2,
                len_mult: 1.15,
                flam_press: ENTER_FLAM,
                rattle: Some(self.stab_rattle),
                ..PLAIN
            },
            Class::Backspace => ClassParams {
                level_db: backspace_db,
                cap_fmult: 0.93,
                decay_mult: 1.12,
                tc_mult: 1.15,
                len_mult: 1.1,
                flam_press: BACKSPACE_FLAM,
                rattle: Some(self.stab_rattle),
                ..PLAIN
            },
            Class::Modifier => {
                ClassParams { level_db: -0.5, cap_fmult: 0.96, tc_mult: 1.05, ..PLAIN }
            }
            // A 2.25–2.75u cap on a stabilizer: heavier and lower, wire ticks, no audible flam.
            Class::Shift => ClassParams {
                level_db: -0.5,
                cap_fmult: 0.94,
                decay_mult: 1.08,
                tc_mult: 1.1,
                len_mult: 1.05,
                rattle: Some(self.stab_rattle),
                ..PLAIN
            },
            Class::Other => ClassParams { level_db: -1.0, cap_fmult: 1.03, ..PLAIN },
        }
    }
}

/// Lubed linears, thick PBT caps, a heavy aluminium case with foam.
pub const DEEP_THOCK: PackParams = PackParams {
    id: "deep-thock",
    name: "Deep Thock",
    description: "Lubed linear switches under thick PBT caps in a heavy, foam-filled case: \
                  low, round and muted.",
    variation_pitch: 0.015,
    variation_volume: 0.08,
    fade_ms: 12.0,
    hpf: 70.0,
    lpf: 9000.0,
    cap: &[pl(1350.0, 1.25), pl(2000.0, 1.0), pl(2850.0, 0.55), pl(4000.0, 0.2)],
    cap_damp_mult: 0.7,
    body: &[
        m(160.0, 0.25, 28.0),
        m(240.0, 0.4, 32.0),
        m(360.0, 0.5, 26.0),
        m(520.0, 0.4, 20.0),
        m(780.0, 0.25, 14.0),
        m(1150.0, 0.15, 9.0),
    ],
    body_contact_ms: 1.6,
    cavity: None,
    ping: &[],
    reflections: &[(1.2, 0.35, 2500.0), (2.9, 0.2, 1800.0)],
    tail: Tail { g: 0.1, t60_ms: 60.0, lp: 2500.0 },
    press: ActionParams {
        level_db: 0.0,
        length_ms: 170.0,
        contact_ms: 0.45,
        bounces: (1, 2),
        cap_fmult: 1.0,
        decay_mult: 1.0,
        body_g: 1.0,
        ping_g: 1.0,
        cavity_g: 1.0,
        noise: Noise { fc: 1200.0, q: 0.8, g: 0.22, tau_ms: 0.8 },
        click: None,
    },
    release: ActionParams {
        level_db: -10.0,
        length_ms: 110.0,
        contact_ms: 0.35,
        bounces: (1, 1),
        cap_fmult: 1.08,
        decay_mult: 0.8,
        body_g: 0.35,
        ping_g: 1.0,
        cavity_g: 1.0,
        noise: Noise { fc: 1500.0, q: 0.8, g: 0.12, tau_ms: 0.6 },
        click: None,
    },
    big_key_db: [0.5, 0.8, 0.5],
    space_bar: &[m(800.0, 0.25, 20.0), m(2200.0, 0.1, 8.0)],
    space_rattle: Rattle { count: (0, 1), t_ms: (2.0, 14.0), g: 0.015, modes: SPACE_TICK },
    stab_rattle: Rattle { count: (0, 1), t_ms: (2.0, 10.0), g: 0.012, modes: STAB_TICK },
};

/// ABS caps, a lighter, slightly hollow case, unlubed springs.
pub const CRISP_CLACK: PackParams = PackParams {
    id: "crisp-clack",
    name: "Crisp Clack",
    description: "Thin ABS caps on a light, slightly hollow case: bright, sharp and snappy.",
    variation_pitch: 0.02,
    variation_volume: 0.08,
    fade_ms: 10.0,
    hpf: 90.0,
    lpf: 13000.0,
    cap: &[pl(2100.0, 1.0), pl(3000.0, 1.0), pl(4100.0, 0.75), pl(5500.0, 0.45), pl(7200.0, 0.2)],
    cap_damp_mult: 1.0,
    body: &[
        m(330.0, 0.1, 16.0),
        m(560.0, 0.14, 13.0),
        m(900.0, 0.16, 10.0),
        m(1400.0, 0.12, 8.0),
        m(2000.0, 0.08, 6.0),
    ],
    body_contact_ms: 1.0,
    cavity: Some(Noise { fc: 1250.0, q: 5.0, g: 0.1, tau_ms: 8.0 }),
    ping: &[m(4100.0, 0.02, 60.0), m(6300.0, 0.012, 40.0)],
    reflections: &[(1.2, 0.35, 4000.0), (2.9, 0.2, 3000.0)],
    tail: Tail { g: 0.1, t60_ms: 70.0, lp: 4500.0 },
    press: ActionParams {
        level_db: 0.0,
        length_ms: 130.0,
        contact_ms: 0.3,
        bounces: (1, 3),
        cap_fmult: 1.0,
        decay_mult: 1.0,
        body_g: 1.0,
        ping_g: 1.0,
        cavity_g: 1.0,
        noise: Noise { fc: 3200.0, q: 0.8, g: 0.45, tau_ms: 0.45 },
        click: None,
    },
    release: ActionParams {
        level_db: -6.0,
        length_ms: 100.0,
        contact_ms: 0.25,
        bounces: (1, 2),
        cap_fmult: 1.1,
        decay_mult: 0.85,
        body_g: 0.4,
        ping_g: 1.3,
        cavity_g: 0.5,
        noise: Noise { fc: 3800.0, q: 0.8, g: 0.22, tau_ms: 0.35 },
        click: None,
    },
    big_key_db: [-0.5, 0.5, 0.3],
    space_bar: &[m(1000.0, 0.25, 16.0), m(2750.0, 0.12, 7.0), m(5400.0, 0.05, 3.0)],
    space_rattle: Rattle { count: (1, 2), t_ms: (2.0, 14.0), g: 0.05, modes: SPACE_TICK },
    stab_rattle: Rattle { count: (0, 2), t_ms: (2.0, 10.0), g: 0.04, modes: STAB_TICK },
};

/// Click-jacket switches (MX Blue style), ABS caps, plate mount, stock springs.
pub const BLUE_CLICK: PackParams = PackParams {
    id: "blue-click",
    name: "Blue Click",
    description: "Click-jacket switches: a sharp click halfway down every press, then the \
                  bottom-out, and a softer click on the way back up.",
    variation_pitch: 0.02,
    variation_volume: 0.08,
    fade_ms: 10.0,
    hpf: 90.0,
    lpf: 15000.0,
    cap: &[pl(1900.0, 0.35), pl(2800.0, 0.32), pl(3900.0, 0.24), pl(5200.0, 0.12)],
    cap_damp_mult: 1.0,
    body: &[m(300.0, 0.12, 15.0), m(520.0, 0.15, 12.0), m(850.0, 0.15, 9.0), m(1300.0, 0.1, 7.0)],
    body_contact_ms: 1.0,
    cavity: Some(Noise { fc: 1150.0, q: 4.0, g: 0.06, tau_ms: 7.0 }),
    ping: &[m(4500.0, 0.025, 70.0), m(6900.0, 0.015, 45.0)],
    reflections: &[(1.2, 0.35, 5000.0), (2.9, 0.2, 3500.0)],
    tail: Tail { g: 0.1, t60_ms: 70.0, lp: 5000.0 },
    press: ActionParams {
        level_db: 0.0,
        length_ms: 140.0,
        contact_ms: 0.35,
        bounces: (1, 2),
        cap_fmult: 1.0,
        decay_mult: 1.0,
        body_g: 1.0,
        ping_g: 1.0,
        cavity_g: 1.0,
        noise: Noise { fc: 3000.0, q: 0.8, g: 0.15, tau_ms: 0.45 },
        click: Some(Click {
            tau_ms: 0.08,
            gain: 2.2,
            delay_ms: 5.0,
            modes: &[
                m(3000.0, 0.5, 6.0),
                m(4400.0, 0.45, 5.0),
                m(6000.0, 0.3, 3.5),
                m(8000.0, 0.15, 2.0),
            ],
            noise: Noise { fc: 5000.0, q: 0.9, g: 0.4, tau_ms: 0.7 },
        }),
    },
    release: ActionParams {
        level_db: -4.0,
        length_ms: 100.0,
        contact_ms: 0.28,
        bounces: (1, 1),
        cap_fmult: 1.08,
        decay_mult: 0.85,
        body_g: 0.4,
        ping_g: 1.2,
        cavity_g: 0.5,
        noise: Noise { fc: 3500.0, q: 0.8, g: 0.12, tau_ms: 0.35 },
        click: Some(Click {
            tau_ms: 0.09,
            gain: 1.1,
            delay_ms: 4.0,
            modes: &[m(3200.0, 0.5, 5.0), m(4700.0, 0.45, 4.0), m(6400.0, 0.3, 3.0)],
            noise: Noise { fc: 5200.0, q: 0.9, g: 0.4, tau_ms: 0.5 },
        }),
    },
    big_key_db: [1.0, 0.5, 0.3],
    space_bar: &[m(1000.0, 0.25, 16.0), m(2750.0, 0.12, 7.0), m(5400.0, 0.05, 3.0)],
    space_rattle: Rattle { count: (1, 2), t_ms: (2.0, 14.0), g: 0.05, modes: SPACE_TICK },
    stab_rattle: Rattle { count: (0, 2), t_ms: (2.0, 10.0), g: 0.04, modes: STAB_TICK },
};

pub const PACKS: [&PackParams; 3] = [&DEEP_THOCK, &CRISP_CLACK, &BLUE_CLICK];
