//! `synth-packs loudness`: measures any pack's typing loudness exactly the way the generator
//! matches its own packs ([`crate::pack::match_loudness`]), so that the bundled packs can be
//! set to TakTak's reference typing level ([`REFERENCE_LK`]) with the `volume` field of their
//! pack.json and switching packs does not jump in volume.
//!
//! The pack is loaded with TakTak's own loader at 48 kHz, so the measured samples are the ones
//! the engine plays (decoded, downmixed, leading silence trimmed, resampled). Typing loudness
//! is measured the way the app plays it by default: for each alphanumeric key, the K-weighted
//! (BS.1770) energy of the first 100 ms of the sample that key plays on press (the one
//! [`consistent_index`] gives it), averaged as power over the keys and expressed in LK
//! ([`analysis::k_energy`], [`analysis::k_db`]). The generator's own match averages all of its
//! per-key presses instead (second takes included); the two agree within 0.2 dB on its packs.
//!
//! A louder `volume` must not make the mixer clip (it clips hard at 0 dBFS, and resampling to
//! the device rate can peak between samples), so the same headroom rule as the pack-source
//! builds applies (see `tools/pack-sources/*/packlib.py`): the loudest true peak
//! ([`analysis::true_peak`], 4x oversampled) of any sample a key can play (with random variants
//! on, every candidate), times `volume`, times the top of the random volume variation, stays at
//! or below [`MAX_TRUE_PEAK_DBFS`].

use crate::analysis;
use crate::dsp::{FS, db_to_gain, gain_to_db};
use std::collections::BTreeSet;
use std::path::Path;
use taktak_core::audio::{SoundBank, consistent_index};
use taktak_core::input::KeyAction;
use taktak_core::key::{Key, KeyGroup};
use taktak_core::pack::{self, PackOrigin};

/// TakTak's reference typing level: the effective typing loudness every bundled pack is
/// matched to (with its `volume`), so switching packs does not jump in volume. A fixed
/// constant; it was first set by the experimental synthesized packs, which are no longer
/// bundled.
pub const REFERENCE_LK: f64 = -25.8;
/// How far a bundled pack's effective typing loudness may stray from the reference.
pub const TOLERANCE_DB: f64 = 0.5;
/// The spec's upper bound for `volume` (+6 dB).
pub const MAX_VOLUME: f64 = 2.0;
/// Ceiling for the loudest file's true peak at the pack's `volume` and the top of its random
/// volume variation: 1 dB below the mixer's hard clip, for overlapping strokes.
pub const MAX_TRUE_PEAK_DBFS: f64 = -1.0;
/// Slack on [`MAX_TRUE_PEAK_DBFS`] when judging a pack: true-peak estimators (this one, the
/// pack-source builds' FFT oversampling, the loader's resampler) differ by a few hundredths of
/// a dB, and the builds allow the same.
const TRUE_PEAK_SLACK_DB: f64 = 0.05;

/// How a pack's effective typing loudness relates to the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Within [`TOLERANCE_DB`] of the target, with headroom to spare.
    Matched,
    /// Quieter than the target, at the loudest volume the headroom rule allows: only a change
    /// to the audio itself (limiting, levelling takes) could close the gap.
    HeadroomLimited,
    /// Off target although `volume` (or the build gain) could fix it, or louder than the
    /// headroom rule allows.
    Off,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Matched => "ok",
            Status::HeadroomLimited => "limited",
            Status::Off => "OFF",
        }
    }
}

/// Loudness measurements of one pack.
#[derive(Clone, Debug)]
pub struct Loudness {
    pub id: String,
    /// Distinct samples that alphanumeric keys play on press by default.
    pub presses: usize,
    /// Typing loudness of the samples as stored (at `volume` 1.0), in LK.
    pub raw_lk: f64,
    /// The same for the alphanumeric keys' releases, if any sound.
    pub release_lk: Option<f64>,
    /// The pack's `volume`.
    pub volume: f64,
    /// The pack's random volume variation (± fraction per keystroke).
    pub volume_variation: f64,
    /// Highest true peak of any sample a key can play, at `volume` 1.0, in dBFS.
    pub true_peak_dbfs: f64,
}

impl Loudness {
    /// Typing loudness as played, with the pack's `volume` applied.
    pub fn effective_lk(&self) -> f64 {
        self.raw_lk + gain_to_db(self.volume)
    }

    /// The loudest true peak as played at the top of the volume variation, in dBFS.
    pub fn effective_peak_dbfs(&self) -> f64 {
        self.true_peak_dbfs + gain_to_db(self.volume * (1.0 + self.volume_variation))
    }

    /// The `volume` that puts typing at `target_lk`, rounded to two decimals as written in
    /// pack.json. Not limited by headroom or by [`MAX_VOLUME`].
    pub fn volume_for(&self, target_lk: f64) -> f64 {
        round2(db_to_gain(target_lk - self.raw_lk))
    }

    /// The largest two-decimal `volume` that keeps [`MAX_TRUE_PEAK_DBFS`].
    pub fn max_clean_volume(&self) -> f64 {
        let limit =
            db_to_gain(MAX_TRUE_PEAK_DBFS - self.true_peak_dbfs) / (1.0 + self.volume_variation);
        (limit * 100.0 + 1e-9).floor() / 100.0
    }

    /// What to write into pack.json: [`Loudness::volume_for`], unless headroom or the spec's
    /// range allow less.
    pub fn recommended_volume(&self, target_lk: f64) -> f64 {
        self.volume_for(target_lk).min(self.max_clean_volume()).min(MAX_VOLUME)
    }

    pub fn status(&self, target_lk: f64) -> Status {
        if self.effective_peak_dbfs() > MAX_TRUE_PEAK_DBFS + TRUE_PEAK_SLACK_DB {
            return Status::Off;
        }
        let off = self.effective_lk() - target_lk;
        if off.abs() <= TOLERANCE_DB {
            Status::Matched
        } else if off < 0.0 && self.volume + 1e-6 >= self.max_clean_volume() {
            Status::HeadroomLimited
        } else {
            Status::Off
        }
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn to_f64(s: &[f32]) -> Vec<f64> {
    s.iter().map(|&v| f64::from(v)).collect()
}

/// Mean K-weighted energy of `samples`, as LK; `None` for an empty set.
fn typing_lk<'a>(samples: impl Iterator<Item = &'a [f32]>) -> Option<f64> {
    let energies: Vec<f64> = samples.map(|s| analysis::k_energy(&to_f64(s))).collect();
    (!energies.is_empty())
        .then(|| analysis::k_db(energies.iter().sum::<f64>() / energies.len() as f64))
}

/// The sample each alphanumeric key plays for `action` in the default variant mode
/// ([`consistent_index`]), one entry per key that sounds, so a sample two keys share counts
/// twice: typing loudness weights every key the same.
fn alphanumeric_samples(bank: &SoundBank, action: KeyAction) -> Vec<u32> {
    Key::ALL
        .iter()
        .filter(|k| k.group() == KeyGroup::Alphanumeric)
        .filter_map(|&k| {
            let candidates = bank.map.get(k, action);
            (!candidates.is_empty()).then(|| candidates[consistent_index(k, candidates.len())])
        })
        .collect()
}

/// Loads the pack at `path` (a folder or `.zip`) and measures it.
pub fn measure(path: &Path) -> Result<Loudness, String> {
    let loaded = pack::load(path, PackOrigin::User, FS as u32)
        .map_err(|e| e.to_string().trim().to_owned())?;
    let bank = &loaded.bank;
    let lk = |action| {
        let ids = alphanumeric_samples(bank, action);
        let distinct = ids.iter().collect::<BTreeSet<_>>().len();
        (distinct, typing_lk(ids.iter().map(|&id| &*bank.samples[id as usize])))
    };
    let (presses, raw_lk) = lk(KeyAction::Down);
    let raw_lk =
        raw_lk.ok_or_else(|| format!("{}: no alphanumeric press sounds", path.display()))?;
    let true_peak =
        bank.samples.iter().map(|s| analysis::true_peak(&to_f64(s))).fold(0.0, f64::max);
    Ok(Loudness {
        id: loaded.info.id.clone(),
        presses,
        raw_lk,
        release_lk: lk(KeyAction::Up).1,
        volume: f64::from(bank.gain),
        volume_variation: f64::from(bank.variation.volume),
        true_peak_dbfs: gain_to_db(true_peak),
    })
}

pub const TABLE_HEADER: &str = "pack                n   LK raw  volume   LK eff  vs ref  status   \
                                rel-press dB  true peak eff  max clean vol  volume for ref";

pub fn table_row(l: &Loudness, target_lk: f64) -> String {
    let wanted = l.volume_for(target_lk);
    let recommended = l.recommended_volume(target_lk);
    let clean = l.max_clean_volume();
    let residual = |volume: f64| l.raw_lk + gain_to_db(volume) - target_lk;
    let advice = if recommended == wanted {
        format!("{wanted:.2}")
    } else if clean <= MAX_VOLUME {
        format!("{recommended:.2} (headroom-limited: {:+.1} dB from target)", residual(clean))
    } else {
        // The spec's range binds first: the build can add the rest, up to the headroom limit.
        let reachable = wanted.min(clean);
        let short = residual(reachable);
        format!(
            "{MAX_VOLUME:.2} and {:+.1} dB more in the build{}",
            gain_to_db(reachable / MAX_VOLUME),
            if short < -TOLERANCE_DB {
                format!(" (headroom-limited: {short:+.1} dB from target)")
            } else {
                String::new()
            }
        )
    };
    let release = l.release_lk.map_or("-".to_owned(), |r| format!("{:.1}", r - l.raw_lk));
    format!(
        "{:<18} {:>3} {:>8.2} {:>7.2} {:>8.2} {:>+7.2}  {:<7} {:>13} {:>14.2} {:>14.2}  {advice}",
        l.id,
        l.presses,
        l.raw_lk,
        l.volume,
        l.effective_lk(),
        l.effective_lk() - target_lk,
        l.status(target_lk).label(),
        release,
        l.effective_peak_dbfs(),
        clean,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::samples;
    use std::f64::consts::TAU;
    use std::fs;

    /// A one-sample pack: a decaying 1 kHz tone at `amp`, with the given `volume`.
    fn tone_pack(dir: &Path, amp: f64, volume: f64) {
        tone(dir, "a", amp);
        tone_manifest(dir, volume, &["sounds/a.wav"]);
    }

    /// Writes `sounds/<name>.wav`: a decaying 1 kHz tone at `amp`.
    fn tone(dir: &Path, name: &str, amp: f64) {
        fs::create_dir_all(dir.join("sounds")).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let path = dir.join(format!("sounds/{name}.wav"));
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for n in 0..samples(150.0) {
            let t = n as f64 / FS;
            let s = amp * (TAU * 1000.0 * t).sin() * (-t / 0.03).exp();
            w.write_sample((s * 32767.0).round() as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    /// A pack.json whose alphanumeric keys press one of `files`.
    fn tone_manifest(dir: &Path, volume: f64, files: &[&str]) {
        let manifest = serde_json::json!({
            "format": 1, "id": "tone", "name": "Tone", "author": "test", "license": "CC0-1.0",
            "volume": volume, "trim_silence": false, "variation": { "volume": 0.1 },
            "groups": { "alphanumeric": { "press": files } }
        });
        fs::write(dir.join("pack.json"), manifest.to_string()).unwrap();
    }

    #[test]
    fn level_and_volume_shift_the_measurement_in_db() {
        let tmp = tempfile::tempdir().unwrap();
        let (full, half) = (tmp.path().join("full"), tmp.path().join("half"));
        tone_pack(&full, 0.5, 1.0);
        tone_pack(&half, 0.25, 0.5);
        let (a, b) = (measure(&full).unwrap(), measure(&half).unwrap());
        assert_eq!((a.presses, a.release_lk), (1, None));
        assert!((a.raw_lk - b.raw_lk - 6.02).abs() < 0.05, "{} vs {}", a.raw_lk, b.raw_lk);
        assert!((a.effective_lk() - b.effective_lk() - 12.04).abs() < 0.05);
        // A 1 kHz tone has no inter-sample overs to speak of.
        assert!((a.true_peak_dbfs - gain_to_db(0.5)).abs() < 0.1, "{}", a.true_peak_dbfs);
        // The suggested volume lands on the target.
        assert!((a.volume_for(a.raw_lk - 3.0) - 0.71).abs() < 1e-9);
        // Headroom: peak · v · 1.1 ≤ −1 dBFS. The decaying tone peaks a little under 0.5
        // (0.496 in its first cycle), so v ≤ 1.63.
        assert!((a.max_clean_volume() - 1.63).abs() < 1e-9, "{}", a.max_clean_volume());
        assert_eq!(a.recommended_volume(a.raw_lk + 6.0), 1.63);
        assert_eq!(b.recommended_volume(b.raw_lk + 3.0), 1.41);
        // Status: on target; quieter but at the headroom limit; fixable; too hot.
        assert_eq!(a.status(a.raw_lk + 0.4), Status::Matched);
        assert_eq!(a.status(a.raw_lk + 1.0), Status::Off);
        let limited = Loudness { volume: a.max_clean_volume(), ..a.clone() };
        assert_eq!(limited.status(limited.effective_lk() + 2.0), Status::HeadroomLimited);
        let hot = Loudness { volume: 1.8, ..a.clone() };
        assert_eq!(hot.status(hot.effective_lk()), Status::Off);
    }

    /// Typing loudness is what the keys play by default: with a loud and a soft sample in the
    /// pool, each counts once for every alphanumeric key that plays it, not once per file.
    #[test]
    fn typing_weights_each_key_by_the_sample_it_plays_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let alone = |name: &str, amp| {
            let dir = tmp.path().join(name);
            tone(&dir, name, amp);
            tone_manifest(&dir, 1.0, &[&format!("sounds/{name}.wav")]);
            measure(&dir).unwrap().raw_lk
        };
        let (loud_lk, soft_lk) = (alone("loud", 0.5), alone("soft", 0.125));
        let both = tmp.path().join("both");
        tone(&both, "loud", 0.5);
        tone(&both, "soft", 0.125);
        tone_manifest(&both, 1.0, &["sounds/loud.wav", "sounds/soft.wav"]);
        let l = measure(&both).unwrap();
        assert_eq!(l.presses, 2);

        let keys: Vec<Key> =
            Key::ALL.iter().copied().filter(|k| k.group() == KeyGroup::Alphanumeric).collect();
        let n_loud = keys.iter().filter(|&&k| consistent_index(k, 2) == 0).count() as f64;
        let n_soft = keys.len() as f64 - n_loud;
        assert!(n_loud > 0.0 && n_soft > 0.0 && n_loud != n_soft, "{n_loud} loud, {n_soft} soft");
        let power = |lk: f64| 10f64.powf(lk / 10.0);
        let per_key = 10.0
            * ((n_loud * power(loud_lk) + n_soft * power(soft_lk)) / (n_loud + n_soft)).log10();
        let per_file = 10.0 * ((power(loud_lk) + power(soft_lk)) / 2.0).log10();
        assert!((l.raw_lk - per_key).abs() < 0.01, "{} vs {per_key}", l.raw_lk);
        assert!((l.raw_lk - per_file).abs() > 0.1, "{} vs {per_file}", l.raw_lk);
    }

    /// Switching between the packs TakTak ships must not jump in volume: every bundled pack
    /// in `packs/` is matched to [`REFERENCE_LK`], or as close as headroom allows.
    #[test]
    fn bundled_packs_are_loudness_matched() {
        let packs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        let mut dirs: Vec<_> = fs::read_dir(&packs)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.join("pack.json").is_file())
            .collect();
        dirs.sort();
        assert!(!dirs.is_empty(), "no packs in {}", packs.display());
        let off: Vec<String> = dirs
            .iter()
            .map(|d| measure(d).unwrap())
            .filter(|l| l.status(REFERENCE_LK) == Status::Off)
            .map(|l| table_row(&l, REFERENCE_LK))
            .collect();
        assert!(off.is_empty(), "off target:\n{TABLE_HEADER}\n{}", off.join("\n"));
    }

    #[test]
    fn matches_the_generator_measurement_of_the_same_samples() {
        let tmp = tempfile::tempdir().unwrap();
        tone_pack(tmp.path(), 0.5, 1.0);
        let mut r = hound::WavReader::open(tmp.path().join("sounds/a.wav")).unwrap();
        let x: Vec<f64> = r.samples::<i16>().map(|s| f64::from(s.unwrap()) / 32768.0).collect();
        let direct = analysis::k_db(analysis::k_energy(&x));
        let measured = measure(tmp.path()).unwrap().raw_lk;
        assert!((direct - measured).abs() < 0.01, "{direct} vs {measured}");
    }
}
