//! What the audio thread plays: decoded samples plus a precomputed key → sample table.
//! Built on the control thread (by `pack::load`), then moved into the mixer in one piece so
//! samples and mapping always change together.

use crate::input::KeyAction;
use crate::key::Key;

/// Per-keystroke random ± ranges at full strength; the mixer scales them by its humanize amount.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Variation {
    pub pitch: f32,
    pub volume: f32,
}

impl Default for Variation {
    fn default() -> Self {
        Variation { pitch: 0.03, volume: 0.10 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Slot {
    start: u32,
    len: u32,
}

/// For every key and action, the candidate sample indices (the mixer picks one by its
/// [`VariantMode`](super::VariantMode)).
#[derive(Clone, Debug, PartialEq)]
pub struct SoundMap {
    press: Vec<Slot>,
    release: Vec<Slot>,
    ids: Vec<u32>,
}

impl Default for SoundMap {
    fn default() -> Self {
        SoundMap {
            press: vec![Slot::default(); Key::COUNT],
            release: vec![Slot::default(); Key::COUNT],
            ids: Vec::new(),
        }
    }
}

impl SoundMap {
    pub fn new() -> SoundMap {
        SoundMap::default()
    }

    /// Sets the candidates for `key`/`action`, replacing any previous ones.
    pub fn set(&mut self, key: Key, action: KeyAction, sample_ids: &[u32]) {
        let slot = Slot { start: self.ids.len() as u32, len: sample_ids.len() as u32 };
        self.ids.extend_from_slice(sample_ids);
        match action {
            KeyAction::Down => self.press[key.index()] = slot,
            KeyAction::Up => self.release[key.index()] = slot,
        }
    }

    /// Candidate sample indices; empty means silent. Allocation-free (audio-thread safe).
    #[inline]
    pub fn get(&self, key: Key, action: KeyAction) -> &[u32] {
        let slot = match action {
            KeyAction::Down => self.press[key.index()],
            KeyAction::Up => self.release[key.index()],
        };
        &self.ids[slot.start as usize..(slot.start + slot.len) as usize]
    }
}

/// Mono f32 samples at the engine's output rate, plus how keys map onto them.
#[derive(Clone, Debug, Default)]
pub struct SoundBank {
    pub samples: Vec<Box<[f32]>>,
    pub map: SoundMap,
    /// Pack-level gain (`volume` in pack.json).
    pub gain: f32,
    pub variation: Variation,
}

impl SoundBank {
    /// Every key plays `press` on key-down and, if given, `release` on key-up.
    /// Used by the spike and as the built-in fallback when no pack loads.
    pub fn uniform(press: Box<[f32]>, release: Option<Box<[f32]>>) -> SoundBank {
        let mut map = SoundMap::new();
        let has_release = release.is_some();
        let mut samples = vec![press];
        samples.extend(release);
        for &key in Key::ALL {
            map.set(key, KeyAction::Down, &[0]);
            if has_release {
                map.set(key, KeyAction::Up, &[1]);
            }
        }
        SoundBank { samples, map, gain: 1.0, variation: Variation::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_set_get_and_replace() {
        let mut m = SoundMap::new();
        assert!(m.get(Key::KeyA, KeyAction::Down).is_empty());
        m.set(Key::KeyA, KeyAction::Down, &[3, 4]);
        m.set(Key::KeyA, KeyAction::Up, &[5]);
        m.set(Key::Space, KeyAction::Down, &[9]);
        assert_eq!(m.get(Key::KeyA, KeyAction::Down), &[3, 4]);
        assert_eq!(m.get(Key::KeyA, KeyAction::Up), &[5]);
        assert_eq!(m.get(Key::Space, KeyAction::Down), &[9]);
        assert!(m.get(Key::Space, KeyAction::Up).is_empty());
        m.set(Key::KeyA, KeyAction::Down, &[7]);
        assert_eq!(m.get(Key::KeyA, KeyAction::Down), &[7]);
    }

    #[test]
    fn uniform_bank() {
        let b = SoundBank::uniform(vec![0.1; 4].into(), None);
        assert_eq!(b.map.get(Key::F12, KeyAction::Down), &[0]);
        assert!(b.map.get(Key::F12, KeyAction::Up).is_empty());
        let b = SoundBank::uniform(vec![0.1; 4].into(), Some(vec![0.2; 2].into()));
        assert_eq!(b.map.get(Key::Enter, KeyAction::Up), &[1]);
    }
}
