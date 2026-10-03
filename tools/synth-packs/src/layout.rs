//! Where keys sit on the board, and the per-key voicing that follows from it.
//!
//! Each key excites the plate and case at a different point, like hitting a drum in different
//! places (Asonov & Agrawal), so body-mode gains follow plate mode shapes at the key's
//! position. Rows differ in keycap height (taller caps ring lower), and the finger that
//! usually strikes a key sets its typical force. A fixed per-key offset adds the small
//! manufacturing differences between otherwise identical keys.

use crate::dsp::{Rng, hash};
use taktak_core::key::{Key, KeyGroup};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finger {
    Pinky,
    Ring,
    Middle,
    Index,
    Thumb,
    /// Struck by whichever finger is free (edge and navigation keys).
    Any,
}

/// Key centre: `x` in key units from the left edge of an ANSI TKL board, `row` 0 = function
/// row, 1 = number row … 5 = space row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub x: f64,
    pub row: u8,
    pub finger: Finger,
}

const fn at(x: f64, row: u8, finger: Finger) -> Place {
    Place { x, row, finger }
}

/// Board size in key units, used to map positions onto the plate.
const BOARD_W: f64 = 18.25;
const BOARD_ROWS: f64 = 6.0;

/// Positions of every key the generator voices individually, plus a few the group samples
/// borrow. `None` for keys that only ever use group samples.
pub fn place(key: Key) -> Option<Place> {
    use Finger::*;
    use Key::*;
    let p = match key {
        Backquote => at(0.5, 1, Pinky),
        Digit1 => at(1.5, 1, Pinky),
        Digit2 => at(2.5, 1, Ring),
        Digit3 => at(3.5, 1, Middle),
        Digit4 => at(4.5, 1, Index),
        Digit5 => at(5.5, 1, Index),
        Digit6 => at(6.5, 1, Index),
        Digit7 => at(7.5, 1, Index),
        Digit8 => at(8.5, 1, Middle),
        Digit9 => at(9.5, 1, Ring),
        Digit0 => at(10.5, 1, Pinky),
        Minus => at(11.5, 1, Pinky),
        Equal => at(12.5, 1, Pinky),
        IntlYen => at(13.5, 1, Pinky),
        Backspace => at(14.0, 1, Pinky),
        KeyQ => at(2.0, 2, Pinky),
        KeyW => at(3.0, 2, Ring),
        KeyE => at(4.0, 2, Middle),
        KeyR => at(5.0, 2, Index),
        KeyT => at(6.0, 2, Index),
        KeyY => at(7.0, 2, Index),
        KeyU => at(8.0, 2, Index),
        KeyI => at(9.0, 2, Middle),
        KeyO => at(10.0, 2, Ring),
        KeyP => at(11.0, 2, Pinky),
        BracketLeft => at(12.0, 2, Pinky),
        BracketRight => at(13.0, 2, Pinky),
        Backslash => at(14.25, 2, Pinky),
        CapsLock => at(0.875, 3, Pinky),
        KeyA => at(2.25, 3, Pinky),
        KeyS => at(3.25, 3, Ring),
        KeyD => at(4.25, 3, Middle),
        KeyF => at(5.25, 3, Index),
        KeyG => at(6.25, 3, Index),
        KeyH => at(7.25, 3, Index),
        KeyJ => at(8.25, 3, Index),
        KeyK => at(9.25, 3, Middle),
        KeyL => at(10.25, 3, Ring),
        Semicolon => at(11.25, 3, Pinky),
        Quote => at(12.25, 3, Pinky),
        Enter => at(13.875, 3, Pinky),
        ShiftLeft => at(1.125, 4, Pinky),
        IntlBackslash => at(1.75, 4, Pinky),
        KeyZ => at(2.75, 4, Pinky),
        KeyX => at(3.75, 4, Ring),
        KeyC => at(4.75, 4, Middle),
        KeyV => at(5.75, 4, Index),
        KeyB => at(6.75, 4, Index),
        KeyN => at(7.75, 4, Index),
        KeyM => at(8.75, 4, Index),
        Comma => at(9.75, 4, Middle),
        Period => at(10.75, 4, Ring),
        Slash => at(11.75, 4, Pinky),
        IntlRo => at(12.75, 4, Pinky),
        ShiftRight => at(13.625, 4, Pinky),
        ControlLeft => at(0.625, 5, Pinky),
        MetaLeft => at(1.875, 5, Thumb),
        AltLeft => at(3.125, 5, Thumb),
        Space => at(6.875, 5, Thumb),
        F5 => at(7.0, 0, Any),
        PageDown => at(17.75, 2, Any),
        ArrowDown => at(16.75, 5, Any),
        _ => return None,
    };
    Some(p)
}

/// Plate mode (m, n) assigned to each body mode, lowest first. The plate is about 3:1, so the
/// first few modes vary along its length.
const MODE_SHAPES: [(f64, f64); 6] =
    [(1.0, 1.0), (2.0, 1.0), (3.0, 1.0), (1.0, 2.0), (4.0, 1.0), (2.0, 2.0)];
pub const MAX_BODY: usize = MODE_SHAPES.len();

/// Keycap height by row (Cherry/OEM-style sculpt: the number row is tallest, the home row
/// lowest, the bottom rows in between): taller caps are heavier and ring lower.
const ROW_CAP_F: [f64; 6] = [0.985, 0.975, 0.99, 1.0, 0.99, 0.99];

/// Per-key timbre and level, applied on top of the class parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voice {
    pub cap_f: f64,
    pub level_db: f64,
    pub body: [f64; MAX_BODY],
    /// Spring ping frequency: springs differ slightly from switch to switch.
    pub ping_f: f64,
}

impl Voice {
    /// The average key: used for the generic alphanumeric fallback samples.
    pub const NEUTRAL: Voice =
        Voice { cap_f: 1.0, level_db: 0.0, body: [1.0; MAX_BODY], ping_f: 1.0 };
}

fn shape(p: &Place, mode: usize) -> f64 {
    use std::f64::consts::PI;
    let u = 0.03 + 0.94 * p.x / BOARD_W;
    let v = 0.06 + 0.88 * (f64::from(p.row) + 0.5) / BOARD_ROWS;
    let (m, n) = MODE_SHAPES[mode];
    ((m * PI * u).sin() * (n * PI * v).sin()).abs().max(0.3)
}

fn alnum_places() -> impl Iterator<Item = Place> {
    Key::ALL.iter().filter(|k| k.group() == KeyGroup::Alphanumeric).filter_map(|&k| place(k))
}

/// Typical striking force by finger, in dB: pinkies are weakest.
fn finger_db(f: Finger) -> f64 {
    match f {
        Finger::Pinky => -0.6,
        Finger::Ring => -0.25,
        Finger::Middle => 0.0,
        Finger::Index => 0.15,
        Finger::Thumb | Finger::Any => 0.0,
    }
}

/// The voice of the key at `p`. `name` and `pack` seed the fixed per-key offsets.
pub fn voice(pack: &str, name: &str, p: &Place) -> Voice {
    let mut body = [1.0; MAX_BODY];
    for (i, b) in body.iter_mut().enumerate() {
        // Normalized so the average alphanumeric key keeps the calibrated body level.
        let mean =
            alnum_places().map(|q| shape(&q, i)).sum::<f64>() / alnum_places().count() as f64;
        *b = 0.5 + 0.5 * shape(p, i) / mean;
    }
    let mut rng = Rng::new(hash(&format!("{pack}/voice/{name}")));
    let reach_db = if p.row == 1 { -0.3 } else { 0.0 };
    Voice {
        cap_f: ROW_CAP_F[usize::from(p.row).min(5)] * rng.jitter(0.02),
        level_db: finger_db(p.finger) + reach_db + rng.range(-0.5, 0.5),
        body,
        ping_f: rng.jitter(0.04),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_alphanumeric_key_has_a_place() {
        for &k in Key::ALL.iter().filter(|k| k.group() == KeyGroup::Alphanumeric) {
            assert!(place(k).is_some(), "no place for {}", k.code_name());
        }
        for k in [Key::Space, Key::Enter, Key::Backspace, Key::ShiftLeft, Key::ShiftRight] {
            assert!(place(k).is_some());
        }
    }

    #[test]
    fn voices_vary_by_position_within_bounds() {
        let a = voice("p", "KeyQ", &place(Key::KeyQ).unwrap());
        let b = voice("p", "KeyH", &place(Key::KeyH).unwrap());
        assert_ne!(a, b);
        for v in [a, b] {
            assert!((0.94..1.04).contains(&v.cap_f));
            assert!(v.level_db.abs() < 1.5);
            assert!(v.body.iter().all(|g| (0.6..1.6).contains(g)));
            assert!((0.96..1.04).contains(&v.ping_f));
        }
        // Average alphanumeric body gain stays at the calibrated level.
        let keys: Vec<Voice> = Key::ALL
            .iter()
            .filter(|k| k.group() == KeyGroup::Alphanumeric)
            .map(|k| voice("p", k.code_name(), &place(*k).unwrap()))
            .collect();
        for i in 0..MAX_BODY {
            let mean = keys.iter().map(|v| v.body[i]).sum::<f64>() / keys.len() as f64;
            assert!((mean - 1.0).abs() < 1e-9);
        }
    }
}
