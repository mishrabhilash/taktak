//! The real-time mixer: turns key events into sounds through a [`SoundBank`] and plays them on
//! a fixed polyphonic voice pool, plus one separate preview voice.
//!
//! Runs inside the audio callback, so it never allocates, frees, locks or does I/O. Whatever
//! it lets go of (old banks, old preview clips) is handed back to the caller to drop elsewhere.
//! Kept separate from cpal so it can be unit-tested without an audio device.

use super::bank::SoundBank;
use crate::input::{KeyAction, KeyEvent};
use crate::key::Key;

/// One key event forwarded from the input thread. Which sound it makes, and how it is
/// humanized, is decided by the mixer.
#[derive(Clone, Copy, Debug)]
pub struct Trigger {
    pub key: Key,
    pub action: KeyAction,
    /// OS event time and hook receive time, for latency measurement (0 = unmeasured).
    pub event_ns: u64,
    pub received_ns: u64,
}

impl From<KeyEvent> for Trigger {
    fn from(e: KeyEvent) -> Trigger {
        Trigger { key: e.key, action: e.action, event_ns: e.event_ns, received_ns: e.received_ns }
    }
}

pub const MAX_VOICES: usize = 32;

/// Ceiling for every gain setting and for each voice's final gain, so a bad setting or bank
/// can never push inf/NaN into the mix.
pub const MAX_GAIN: f32 = 4.0;
const MIN_RATE: f32 = 0.25;
const MAX_RATE: f32 = 4.0;

/// Default share of the pack's `variation` that each keystroke gets (see
/// [`Mixer::set_humanize`]): enough that fast repeats of one key do not sound copied, little
/// enough that the key keeps its sound.
pub const DEFAULT_HUMANIZE: f32 = 0.25;

/// How a key picks among its candidate samples, when its sound set lists several files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VariantMode {
    /// Every keystroke of a key plays the same candidate, chosen by a fixed hash of the key:
    /// a key keeps its sound, and different keys spread over the pool. A key's release comes
    /// from the same position as its press, so equal-sized press and release pools recorded as
    /// matched takes stay paired.
    #[default]
    Consistent,
    /// A uniformly random candidate on every keystroke.
    Random,
}

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    sample: u32,
    pos: f64,
    /// Playback rate; 1.03 = 3% higher pitch (and 3% shorter).
    rate: f64,
    gain: f32,
    /// Monotonic start counter; the smallest active one is stolen when the pool is full.
    serial: u64,
}

pub struct Mixer {
    bank: Box<SoundBank>,
    voices: [Voice; MAX_VOICES],
    serial: u64,
    /// "Click to hear" clip, played at rate 1 outside the voice pool. A finished clip stays
    /// here (silent) until the next `set_preview`/`stop_preview` hands it back.
    preview: Option<Box<[f32]>>,
    preview_pos: usize,
    master_gain: f32,
    press_gain: f32,
    release_gain: f32,
    variant_mode: VariantMode,
    /// Share of the bank's variation applied per keystroke, in `0..=1`.
    humanize: f32,
    rng: fastrand::Rng,
}

impl Mixer {
    /// A mixer with an entropy-seeded RNG. Call off the audio thread.
    pub fn new(bank: Box<SoundBank>) -> Mixer {
        Mixer::with_rng(bank, fastrand::Rng::new())
    }

    /// A mixer whose random variant choice and humanization are reproducible.
    pub fn with_seed(bank: Box<SoundBank>, seed: u64) -> Mixer {
        Mixer::with_rng(bank, fastrand::Rng::with_seed(seed))
    }

    fn with_rng(bank: Box<SoundBank>, rng: fastrand::Rng) -> Mixer {
        Mixer {
            bank,
            voices: [Voice::default(); MAX_VOICES],
            serial: 0,
            preview: None,
            preview_pos: 0,
            master_gain: 1.0,
            press_gain: 1.0,
            release_gain: 1.0,
            variant_mode: VariantMode::default(),
            humanize: DEFAULT_HUMANIZE,
            rng,
        }
    }

    /// Scales everything, previews included. Takes effect immediately.
    pub fn set_master_gain(&mut self, gain: f32) {
        self.master_gain = bounded(gain, 0.0, MAX_GAIN);
    }

    /// Scales key-down sounds started from now on.
    pub fn set_press_gain(&mut self, gain: f32) {
        self.press_gain = bounded(gain, 0.0, MAX_GAIN);
    }

    /// Scales key-up sounds started from now on.
    pub fn set_release_gain(&mut self, gain: f32) {
        self.release_gain = bounded(gain, 0.0, MAX_GAIN);
    }

    /// How keys pick among their candidates, for sounds started from now on.
    pub fn set_variant_mode(&mut self, mode: VariantMode) {
        self.variant_mode = mode;
    }

    /// How much of the bank's pitch and volume variation each keystroke gets, for sounds
    /// started from now on: 0 plays every keystroke at exactly rate 1 and the base gain, 1 uses
    /// the pack's full ranges. Bounded to `0..=1` (NaN means 0).
    pub fn set_humanize(&mut self, amount: f32) {
        self.humanize = bounded(amount, 0.0, 1.0);
    }

    /// Installs a new bank and stops all key voices (they index the old one). The preview
    /// keeps playing: it owns its clip, and selecting a pack usually swaps the bank and starts
    /// its preview together. Returns the old bank so the caller can free it off the audio
    /// thread.
    pub fn replace_bank(&mut self, bank: Box<SoundBank>) -> Box<SoundBank> {
        for v in &mut self.voices {
            v.active = false;
        }
        std::mem::replace(&mut self.bank, bank)
    }

    /// Plays `clip` from the start on the preview voice, unaffected by press/release gains
    /// and never stolen by key voices. Returns the clip it replaced, for freeing elsewhere.
    pub fn set_preview(&mut self, clip: Box<[f32]>) -> Option<Box<[f32]>> {
        self.preview_pos = 0;
        self.preview.replace(clip)
    }

    /// Silences the preview voice. Returns its clip, for freeing elsewhere.
    pub fn stop_preview(&mut self) -> Option<Box<[f32]>> {
        self.preview_pos = 0;
        self.preview.take()
    }

    /// Key voices currently playing (the preview is not counted).
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    pub fn preview_playing(&self) -> bool {
        self.preview.as_ref().is_some_and(|c| self.preview_pos < c.len())
    }

    /// Starts the sound for one key event: a candidate from the bank chosen by the variant
    /// mode, with pitch and volume randomized within the bank's variation scaled by the
    /// humanize amount. Returns `false` if the key is silent for this action.
    pub fn start(&mut self, key: Key, action: KeyAction) -> bool {
        let candidates = self.bank.map.get(key, action);
        if candidates.is_empty() {
            return false;
        }
        let index = match self.variant_mode {
            VariantMode::Consistent => consistent_index(key, candidates.len()),
            VariantMode::Random => self.rng.usize(..candidates.len()),
        };
        let sample = candidates[index];
        // Rendering needs two samples to interpolate between; shorter is silent.
        if self.bank.samples.get(sample as usize).is_none_or(|s| s.len() < 2) {
            return false;
        }

        let variation = self.bank.variation;
        let action_gain = match action {
            KeyAction::Down => self.press_gain,
            KeyAction::Up => self.release_gain,
        };
        let amount = self.humanize;
        let rate = 1.0 + variation.pitch * amount * self.jitter();
        let gain = action_gain * self.bank.gain * (1.0 + variation.volume * amount * self.jitter());

        let slot = match self.voices.iter().position(|v| !v.active) {
            Some(i) => i,
            // Steal the oldest voice: by then it is usually deep in its decay tail.
            None => (0..MAX_VOICES).min_by_key(|&i| self.voices[i].serial).unwrap_or(0),
        };
        self.serial += 1;
        self.voices[slot] = Voice {
            active: true,
            sample,
            pos: 0.0,
            rate: f64::from(bounded(rate, MIN_RATE, MAX_RATE)),
            gain: bounded(gain, 0.0, MAX_GAIN),
            serial: self.serial,
        };
        true
    }

    /// Uniform in `[-1, 1]`.
    fn jitter(&mut self) -> f32 {
        self.rng.f32_inclusive() * 2.0 - 1.0
    }

    /// Mixes all active voices and the preview into `out` (interleaved, `channels` wide),
    /// overwriting it.
    pub fn render(&mut self, out: &mut [f32], channels: usize) {
        out.fill(0.0);
        let channels = channels.max(1);
        let frames = out.len() / channels;

        for v in self.voices.iter_mut().filter(|v| v.active) {
            // `start` guarantees the sample exists with len >= 2, and `replace_bank` stops
            // every voice before the samples it indexes go away.
            let data = &self.bank.samples[v.sample as usize];
            let last = data.len() - 1;
            let mut pos = v.pos;
            for frame in 0..frames {
                let i = pos as usize;
                if i >= last {
                    v.active = false;
                    break;
                }
                let frac = (pos - i as f64) as f32;
                let s = (data[i] + (data[i + 1] - data[i]) * frac) * v.gain;
                for c in &mut out[frame * channels..(frame + 1) * channels] {
                    *c += s;
                }
                pos += v.rate;
            }
            v.pos = pos;
        }

        if let Some(clip) = &self.preview {
            let rest = &clip[self.preview_pos..];
            for (frame, &s) in out.chunks_exact_mut(channels).zip(rest) {
                for c in frame {
                    *c += s;
                }
            }
            self.preview_pos += rest.len().min(frames);
        }

        let g = self.master_gain;
        for s in out.iter_mut() {
            // NaN (from a non-finite sample in a broken pack) must not reach the device.
            let x = *s * g;
            *s = if x.is_nan() { 0.0 } else { x.clamp(-1.0, 1.0) };
        }
    }
}

/// The candidate `key` always plays in [`VariantMode::Consistent`], out of `len` (0 when `len`
/// is 0). A fixed hash of the key's name, the same on every run, platform and build, so a key
/// sounds the same tomorrow. Press and release use the same hash: in equal-sized pools they
/// pick the same position. Allocation-free (audio-thread safe).
pub fn consistent_index(key: Key, len: usize) -> usize {
    (KEY_HASHES[key.index()] % len.max(1) as u64) as usize
}

/// Each key's consistent-choice hash, indexed by [`Key::index`]: SplitMix64 of the 64-bit
/// FNV-1a hash of the key's `KeyboardEvent.code` name ([`Key::code_name`]). It depends on the
/// name only, never on where the key sits in the `Key` enum, so adding or reordering keys
/// changes no key's sound.
const KEY_HASHES: [u64; Key::COUNT] = {
    let mut table = [0; Key::COUNT];
    let mut i = 0;
    while i < Key::COUNT {
        table[i] = splitmix64(fnv1a64(Key::ALL[i].code_name().as_bytes()));
        i += 1;
    }
    table
};

/// The 64-bit FNV-1a hash (Fowler, Noll and Vo).
const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// The SplitMix64 output function (Steele, Lea and Flood 2014): spreads FNV-1a's weak low bits
/// over all 64 bits, so that small pools split evenly.
const fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// `x` limited to `lo..=hi`, with NaN mapped to `lo` (unlike `clamp`, which keeps NaN).
fn bounded(x: f32, lo: f32, hi: f32) -> f32 {
    if x >= lo { x.min(hi) } else { lo }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{SoundMap, Variation};

    const FLAT: Variation = Variation { pitch: 0.0, volume: 0.0 };

    fn clip(len: usize, value: f32) -> Box<[f32]> {
        vec![value; len].into_boxed_slice()
    }

    /// Every key plays a constant 0.5 of `len` samples on press, without humanization.
    fn uniform(len: usize) -> Box<SoundBank> {
        let mut b = SoundBank::uniform(clip(len, 0.5), None);
        b.variation = FLAT;
        Box::new(b)
    }

    fn bank(samples: Vec<Box<[f32]>>, map: SoundMap, variation: Variation) -> Box<SoundBank> {
        Box::new(SoundBank { samples, map, gain: 1.0, variation })
    }

    fn mixer(bank: Box<SoundBank>) -> Mixer {
        Mixer::with_seed(bank, 7)
    }

    fn press(m: &mut Mixer) -> bool {
        m.start(Key::KeyA, KeyAction::Down)
    }

    /// The most recently started voice.
    fn newest(m: &Mixer) -> Voice {
        *m.voices.iter().filter(|v| v.active).max_by_key(|v| v.serial).unwrap()
    }

    fn render(m: &mut Mixer, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames];
        m.render(&mut out, 1);
        out
    }

    #[test]
    fn trigger_from_key_event_keeps_everything() {
        let e = KeyEvent { key: Key::Space, action: KeyAction::Up, event_ns: 5, received_ns: 9 };
        let t = Trigger::from(e);
        assert!(t.key == Key::Space && t.action == KeyAction::Up);
        assert_eq!((t.event_ns, t.received_ns), (5, 9));
    }

    #[test]
    fn renders_until_sample_ends_on_every_channel() {
        let mut m = mixer(uniform(10));
        assert!(press(&mut m));
        let mut out = vec![0.0; 32];
        m.render(&mut out, 2);
        assert_eq!(&out[..18], &[0.5; 18]); // 9 frames (last sample is interpolation guard)
        assert!(out[18..].iter().all(|&s| s == 0.0));
        assert_eq!(m.active_voices(), 0);
    }

    #[test]
    fn interpolates_linearly_between_samples() {
        let mut map = SoundMap::new();
        map.set(Key::KeyA, KeyAction::Down, &[0]);
        let mut m = mixer(bank(vec![vec![0.0, 1.0, 0.0, -1.0, 0.0].into()], map, FLAT));
        assert!(press(&mut m));
        m.voices[0].rate = 0.5;
        assert_eq!(render(&mut m, 10), [0.0, 0.5, 1.0, 0.5, 0.0, -0.5, -1.0, -0.5, 0.0, 0.0]);
        assert_eq!(m.active_voices(), 0);
    }

    #[test]
    fn rate_changes_duration() {
        let mut m = mixer(uniform(101));
        assert!(press(&mut m));
        m.voices[0].rate = 2.0;
        assert_eq!(render(&mut m, 100).iter().filter(|&&s| s != 0.0).count(), 50);
    }

    #[test]
    fn voices_continue_across_buffers() {
        let mut m = mixer(uniform(21));
        assert!(press(&mut m));
        assert_eq!(render(&mut m, 8), [0.5; 8]);
        assert_eq!(render(&mut m, 8), [0.5; 8]);
        assert_eq!(&render(&mut m, 8)[..], &[0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn overlapping_voices_sum_and_clip() {
        let mut m = mixer(uniform(1000));
        for _ in 0..3 {
            press(&mut m);
        }
        assert_eq!(render(&mut m, 4), [1.0; 4]); // 1.5 clipped
        m.set_master_gain(0.5);
        assert_eq!(render(&mut m, 4), [0.75; 4]);
    }

    #[test]
    fn fast_typing_never_exceeds_pool_and_steals_oldest() {
        let mut m = mixer(uniform(100_000));
        for _ in 0..(MAX_VOICES * 3) {
            assert!(press(&mut m));
        }
        assert_eq!(m.active_voices(), MAX_VOICES);
        let min_serial = m.voices.iter().map(|v| v.serial).min().unwrap();
        assert_eq!(min_serial, (MAX_VOICES * 2 + 1) as u64);
    }

    #[test]
    fn resolves_press_and_release_per_key() {
        let mut map = SoundMap::new();
        map.set(Key::KeyA, KeyAction::Down, &[0]);
        map.set(Key::KeyA, KeyAction::Up, &[1]);
        map.set(Key::Space, KeyAction::Down, &[2]);
        let samples = vec![clip(50, 0.1), clip(50, 0.2), clip(50, 0.3)];
        let mut m = mixer(bank(samples, map, FLAT));

        let cases = [(Key::KeyA, KeyAction::Down, 0), (Key::KeyA, KeyAction::Up, 1)];
        for (key, action, sample) in cases.into_iter().chain([(Key::Space, KeyAction::Down, 2)]) {
            assert!(m.start(key, action));
            assert_eq!(newest(&m).sample, sample);
        }
        assert!(!m.start(Key::Space, KeyAction::Up));
        assert!(!m.start(Key::KeyB, KeyAction::Down));
        assert_eq!(m.active_voices(), 3);
        let out = render(&mut m, 4);
        assert!(out.iter().all(|&s| (s - 0.6).abs() < 1e-6));
    }

    #[test]
    fn silent_without_playable_candidates() {
        let mut m = mixer(Box::default());
        assert!(!press(&mut m));
        assert!(!m.start(Key::KeyA, KeyAction::Up));

        // Candidates pointing at a missing sample, an empty one or a one-sample one.
        let mut map = SoundMap::new();
        map.set(Key::KeyA, KeyAction::Down, &[7]);
        map.set(Key::KeyB, KeyAction::Down, &[0]);
        map.set(Key::KeyC, KeyAction::Down, &[1]);
        let mut m = mixer(bank(vec![clip(0, 0.5), clip(1, 0.5)], map, FLAT));
        for key in [Key::KeyA, Key::KeyB, Key::KeyC] {
            assert!(!m.start(key, KeyAction::Down));
        }
        assert_eq!(m.active_voices(), 0);
        assert_eq!(render(&mut m, 8), [0.0; 8]);
    }

    fn variants_bank() -> Box<SoundBank> {
        let mut map = SoundMap::new();
        map.set(Key::KeyA, KeyAction::Down, &[0, 1, 2, 3]);
        let samples = (0..4).map(|_| clip(10_000, 0.1)).collect();
        bank(samples, map, Variation::default())
    }

    fn choices(seed: u64, n: usize) -> Vec<u32> {
        let mut m = Mixer::with_seed(variants_bank(), seed);
        m.set_variant_mode(VariantMode::Random);
        (0..n)
            .map(|_| {
                assert!(press(&mut m));
                newest(&m).sample
            })
            .collect()
    }

    #[test]
    fn defaults_are_consistent_variants_and_light_humanization() {
        let m = Mixer::new(uniform(10));
        assert_eq!((m.variant_mode, m.humanize), (VariantMode::Consistent, DEFAULT_HUMANIZE));
        assert_eq!(VariantMode::default(), VariantMode::Consistent);
    }

    #[test]
    fn random_variant_choice_covers_all_candidates_and_is_reproducible() {
        let picks = choices(42, 400);
        for sample in 0..4 {
            let n = picks.iter().filter(|&&s| s == sample).count();
            assert!(n >= 50, "sample {sample} picked {n} times out of 400");
        }
        assert_eq!(picks, choices(42, 400));
        assert_ne!(picks, choices(43, 400));
    }

    /// Every key presses one of `n_press` samples (ids `0..n_press`) and releases one of
    /// `n_release` (ids from `n_press` on), with the pack's default variation.
    fn pool_bank(n_press: u32, n_release: u32) -> Box<SoundBank> {
        let mut map = SoundMap::new();
        let press: Vec<u32> = (0..n_press).collect();
        let release: Vec<u32> = (n_press..n_press + n_release).collect();
        for &key in Key::ALL {
            map.set(key, KeyAction::Down, &press);
            map.set(key, KeyAction::Up, &release);
        }
        let samples = (0..n_press + n_release).map(|_| clip(10_000, 0.1)).collect();
        bank(samples, map, Variation::default())
    }

    /// The sample `key` plays for `action` in a fresh consistent-mode mixer.
    fn consistent_pick(m: &mut Mixer, key: Key, action: KeyAction) -> u32 {
        assert!(m.start(key, action));
        newest(m).sample
    }

    #[test]
    fn consistent_mode_keeps_each_key_on_one_sample() {
        let mut m = mixer(pool_bank(5, 5));
        for key in [Key::KeyA, Key::Space, Key::Enter, Key::F5] {
            let first = consistent_pick(&mut m, key, KeyAction::Down);
            for _ in 0..200 {
                assert_eq!(consistent_pick(&mut m, key, KeyAction::Down), first);
            }
        }
    }

    #[test]
    fn consistent_mode_without_humanize_repeats_exactly() {
        let mut b = pool_bank(5, 5);
        b.gain = 0.8;
        b.variation = Variation { pitch: 0.05, volume: 0.2 };
        let mut m = mixer(b);
        m.set_humanize(0.0);
        m.set_press_gain(0.5);
        assert!(press(&mut m));
        let first = newest(&m);
        assert_eq!((first.rate, first.gain), (1.0, 0.5 * 0.8));
        for _ in 0..200 {
            assert!(press(&mut m));
            let v = newest(&m);
            assert_eq!((v.sample, v.rate, v.gain), (first.sample, first.rate, first.gain));
        }
    }

    #[test]
    fn consistent_mode_does_not_depend_on_the_rng() {
        let picks = |seed| {
            let mut m = Mixer::with_seed(pool_bank(7, 7), seed);
            Key::ALL
                .iter()
                .map(|&k| consistent_pick(&mut m, k, KeyAction::Down))
                .collect::<Vec<_>>()
        };
        assert_eq!(picks(1), picks(2));
    }

    #[test]
    fn different_keys_spread_over_the_whole_pool() {
        let mut m = mixer(pool_bank(5, 0));
        let mut counts = [0usize; 5];
        for &key in Key::ALL {
            counts[consistent_pick(&mut m, key, KeyAction::Down) as usize] += 1;
        }
        let fair = Key::COUNT / 5;
        for (sample, &n) in counts.iter().enumerate() {
            assert!(n >= fair / 2, "sample {sample} played by {n} of {} keys", Key::COUNT);
        }
    }

    #[test]
    fn consistent_mode_keeps_paired_takes_together() {
        // Equal pools (office-classic 31/31, tactile 10/10): press take i releases with take i.
        for n in [10, 31] {
            let mut m = mixer(pool_bank(n, n));
            for &key in Key::ALL {
                let press = consistent_pick(&mut m, key, KeyAction::Down);
                let release = consistent_pick(&mut m, key, KeyAction::Up);
                assert_eq!(release - n, press);
            }
        }
        // Unequal pools still give every key one fixed release.
        let mut m = mixer(pool_bank(7, 3));
        for &key in Key::ALL {
            let release = consistent_pick(&mut m, key, KeyAction::Up);
            assert!((7..10).contains(&release));
            assert_eq!(consistent_pick(&mut m, key, KeyAction::Up), release);
        }
    }

    #[test]
    fn consistent_choice_is_a_fixed_function_of_the_key_name() {
        // Published FNV-1a and SplitMix64 test values, so neither hash can drift.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
        assert_eq!(splitmix64(0), 0xe220_a839_7b1d_cdaf);
        assert_eq!(splitmix64(0x9e37_79b9_7f4a_7c15), 0x6e78_9e6a_a1b9_65f4);
        for &key in Key::ALL {
            let h = splitmix64(fnv1a64(key.code_name().as_bytes()));
            assert_eq!(consistent_index(key, 31) as u64, h % 31);
            assert_eq!(consistent_index(key, 1), 0);
            assert_eq!(consistent_index(key, 0), 0);
        }
        // Golden picks: these are the sounds users hear, so a change here changes every pack.
        // Pool sizes 2 (per-key takes), 9 (typewriter's enter) and 31 (office-classic).
        let golden = [
            (Key::KeyA, [1, 6, 23]),
            (Key::Space, [0, 3, 12]),
            (Key::Enter, [0, 1, 29]),
            (Key::NumpadEnter, [1, 7, 27]),
        ];
        for (key, picks) in golden {
            let got = [2, 9, 31].map(|len| consistent_index(key, len));
            assert_eq!(got, picks, "{}", key.code_name());
        }
    }

    /// Lowest and highest rate and gain over 1000 presses with the given humanize amount, on a
    /// bank with pitch variation 0.05 and volume variation 0.2.
    fn humanized_ranges(humanize: f32) -> ((f64, f64), (f64, f64)) {
        let mut map = SoundMap::new();
        map.set(Key::KeyA, KeyAction::Down, &[0]);
        let mut m = mixer(bank(vec![clip(100, 0.5)], map, Variation { pitch: 0.05, volume: 0.2 }));
        m.set_humanize(humanize);
        let (mut rates, mut gains) = (Vec::new(), Vec::new());
        for _ in 0..1000 {
            assert!(press(&mut m));
            rates.push(newest(&m).rate);
            gains.push(f64::from(newest(&m).gain));
        }
        let range =
            |v: &[f64]| v.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        (range(&rates), range(&gains))
    }

    #[test]
    fn full_humanization_spans_the_pack_variation() {
        let ((lo, hi), (glo, ghi)) = humanized_ranges(1.0);
        assert!(lo >= 0.95 - 1e-6 && hi <= 1.05 + 1e-6, "rate {lo}..{hi}");
        assert!(lo < 0.96 && hi > 1.04, "rate barely varies: {lo}..{hi}");
        assert!(glo >= 0.8 - 1e-6 && ghi <= 1.2 + 1e-6, "gain {glo}..{ghi}");
        assert!(glo < 0.82 && ghi > 1.18, "gain barely varies: {glo}..{ghi}");
    }

    #[test]
    fn default_humanization_spans_a_quarter_of_the_pack_variation() {
        let ((lo, hi), (glo, ghi)) = humanized_ranges(DEFAULT_HUMANIZE);
        assert!(lo >= 0.9875 - 1e-6 && hi <= 1.0125 + 1e-6, "rate {lo}..{hi}");
        assert!(lo < 0.9885 && hi > 1.0115, "rate barely varies: {lo}..{hi}");
        assert!(glo >= 0.95 - 1e-6 && ghi <= 1.05 + 1e-6, "gain {glo}..{ghi}");
        assert!(glo < 0.955 && ghi > 1.045, "gain barely varies: {glo}..{ghi}");
    }

    #[test]
    fn no_humanization_plays_the_base_rate_and_gain_in_either_mode() {
        for mode in [VariantMode::Consistent, VariantMode::Random] {
            let mut m = mixer(pool_bank(4, 0));
            m.set_variant_mode(mode);
            m.set_humanize(0.0);
            for _ in 0..100 {
                assert!(press(&mut m));
                assert_eq!((newest(&m).rate, newest(&m).gain), (1.0, 1.0));
            }
        }
    }

    #[test]
    fn press_release_and_pack_gains_multiply() {
        let b = SoundBank {
            gain: 1.5,
            variation: FLAT,
            ..SoundBank::uniform(clip(100, 0.5), Some(clip(100, 0.5)))
        };
        let mut m = mixer(Box::new(b));
        m.set_press_gain(0.5);
        m.set_release_gain(0.25);
        assert!(m.start(Key::KeyA, KeyAction::Down));
        assert_eq!(newest(&m).gain, 0.75);
        assert_eq!(render(&mut m, 2), [0.375; 2]);
        assert!(m.start(Key::KeyA, KeyAction::Up));
        assert_eq!(newest(&m).gain, 0.375);
        assert_eq!(render(&mut m, 2), [0.375 + 0.1875; 2]);
    }

    #[test]
    fn settings_are_bounded_and_nan_safe() {
        let mut m = mixer(uniform(100));
        m.set_master_gain(f32::NAN);
        assert_eq!(m.master_gain, 0.0);
        m.set_master_gain(-1.0);
        assert_eq!(m.master_gain, 0.0);
        m.set_press_gain(f32::INFINITY);
        assert_eq!(m.press_gain, MAX_GAIN);
        for (amount, bounded) in [(f32::NAN, 0.0), (-0.5, 0.0), (0.5, 0.5), (2.0, 1.0)] {
            m.set_humanize(amount);
            assert_eq!(m.humanize, bounded);
        }
        m.set_humanize(f32::INFINITY);
        assert_eq!(m.humanize, 1.0);

        let mut b = SoundBank::uniform(clip(100, 0.5), None);
        b.gain = f32::NAN;
        b.variation = Variation { pitch: f32::NAN, volume: f32::NAN };
        let mut m = mixer(Box::new(b));
        assert!(press(&mut m));
        let v = newest(&m);
        assert_eq!((v.gain, v.rate), (0.0, f64::from(MIN_RATE)));
        assert!(render(&mut m, 8).iter().all(|s| s.is_finite()));
    }

    #[test]
    fn non_finite_samples_never_reach_the_output() {
        let samples = vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.5, 0.5].into();
        let mut m =
            mixer(Box::new(SoundBank { variation: FLAT, ..SoundBank::uniform(samples, None) }));
        assert!(press(&mut m));
        assert_eq!(render(&mut m, 4), [0.0, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn preview_is_independent_of_key_voices_and_never_stolen() {
        let mut m = mixer(uniform(100_000));
        m.set_press_gain(0.0);
        m.set_release_gain(0.0);
        assert!(m.set_preview(clip(1000, 0.25)).is_none());
        for _ in 0..(MAX_VOICES * 3) {
            assert!(press(&mut m));
        }
        assert_eq!(m.active_voices(), MAX_VOICES);
        assert!(m.preview_playing());
        // Key voices are muted by their press gain; the preview is not.
        assert_eq!(render(&mut m, 4), [0.25; 4]);
        m.set_master_gain(0.5);
        assert_eq!(render(&mut m, 4), [0.125; 4]);
    }

    #[test]
    fn preview_plays_once_to_the_end_and_restarts_when_replaced() {
        let mut m = mixer(uniform(100));
        m.set_preview(clip(10, 0.25));
        let mut out = vec![0.0; 16];
        m.render(&mut out, 2);
        assert_eq!(out, [0.25; 16]);
        assert_eq!(&render(&mut m, 4)[..], &[0.25, 0.25, 0.0, 0.0]);
        assert!(!m.preview_playing());
        assert_eq!(render(&mut m, 4), [0.0; 4]);

        let old = m.set_preview(clip(3, 0.5)).unwrap();
        assert_eq!(old.len(), 10);
        assert_eq!(&render(&mut m, 4)[..], &[0.5, 0.5, 0.5, 0.0]);
    }

    #[test]
    fn stop_preview_returns_the_clip_and_silences() {
        let mut m = mixer(uniform(100));
        assert!(m.stop_preview().is_none());
        m.set_preview(clip(100, 0.25));
        render(&mut m, 4);
        assert_eq!(m.stop_preview().unwrap().len(), 100);
        assert!(!m.preview_playing());
        assert_eq!(render(&mut m, 4), [0.0; 4]);
    }

    #[test]
    fn replace_bank_returns_old_bank_and_stops_key_voices_but_not_the_preview() {
        let mut m = mixer(uniform(100));
        m.set_preview(clip(100, 0.25));
        assert!(press(&mut m));
        let old = m.replace_bank(uniform(5));
        assert_eq!(old.samples[0].len(), 100);
        assert_eq!(m.active_voices(), 0);
        assert!(m.preview_playing());
        assert_eq!(render(&mut m, 4), [0.25; 4]);
        // The new bank plays.
        assert!(press(&mut m));
        assert_eq!(&render(&mut m, 6)[..], &[0.75, 0.75, 0.75, 0.75, 0.25, 0.25]);
    }
}
