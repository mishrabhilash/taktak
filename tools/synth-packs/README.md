# synth-packs

Generates TakTak's three bundled [sound packs](../../docs/pack-format.md) by physically
informed synthesis. No recordings are involved, so the packs are our own work. The generated
packs are released under **CC0-1.0**, and the tool itself is MIT like the rest of TakTak.

| Pack | Models | Character |
|---|---|---|
| `deep-thock` "Deep Thock" | lubed linear switches, thick PBT caps, heavy case with foam | low, round, muted |
| `crisp-clack` "Crisp Clack" | ABS caps, light and slightly hollow case, unlubed springs | bright, sharp, snappy |
| `blue-click` "Blue Click" | click-jacket switches (MX Blue style), ABS caps | sharp click, then bottom-out; softer up-click |

## Regenerating

From the repository root:

```
cargo run -p synth-packs --release -- --out packs
```

This writes `packs/<id>/` for each pack and takes a few seconds. An existing folder is replaced
only if its `pack.json` says it came from synth-packs; anything else is left alone. Before
writing, the generator removes the files it owns (`sounds/*.wav`, `pack.json`, `preview.wav`,
`SOURCES.md`), so renamed samples leave nothing stale behind. Never edit these files by hand:
change the generator and regenerate.

Every random choice comes from a SplitMix64 generator seeded with the 64-bit FNV-1a hash of a
fixed label: `<id>/<file>` for each sample (for example `deep-thock/sounds/KeyA-press.wav`),
`<id>/voice/<key>` for a key's fixed voicing offsets, and `<id>/preview` for the preview.
Nothing depends on the clock, the machine or the output folder, so a re-run rewrites
byte-identical files on the same platform. Results could differ in the last bit on another OS
or toolchain if its `libm` rounds differently.

## What a pack contains

- **Per-key samples** for all 50 alphanumeric keys (letters, digits, punctuation, the three
  `Intl*` keys) and both Shift keys: one press and one release each, plus a second press take
  for the eight most frequent English letters (`E T A O I N S R`). By default each of these
  keys always plays one of its two takes (chosen by the key); the other is heard only with
  random variants on, when the two alternate.
- **Group samples** for all six groups: 3 press and 3 release takes each. For `alphanumeric`
  these are the average key, a fallback for keys without their own entry. Modifiers and
  `other` (function, arrow and navigation keys) are voiced at a few different board positions.
- `preview.wav`: about 2 s of the pack typing "taktak is fun" and Enter, with human timing
  (60–120 ms holds, 80–200 ms between keys), played the way the app plays it by default: each
  key plays the take the app gives it, with the pack's pitch and volume variation times the
  default humanize amount (0.25).
- `pack.json`: format 1, `trim_silence: true`, and a small runtime `variation` (pitch
  0.015–0.02, volume 0.08). The variety between keys is baked into their own samples (board
  position, row, finger, fixed offsets), so the runtime only needs to keep fast repeats of one
  key from sounding copied; by default the app applies a quarter of these ranges.
- `SOURCES.md`: the provenance record that the [bundled-pack rules](../../docs/pack-format.md#bundled-packs)
  require. It states that the pack is entirely synthesized by this tool (no recordings, no
  third-party audio), the CC0-1.0 dedication, the generator version and seed scheme, how to
  regenerate, the processing applied (pack gain, tail trim, fade, 16-bit rounding), and a table
  of every other file with what it plays for, its synthesis class and board position, its size
  and its SHA-256. It is written last because it lists the others' checksums, and it contains
  no dates or paths, so it is as reproducible as the audio. The SHA-256 is computed in-tree
  (`sha256.rs`, checked against the NIST vectors) rather than through a new dependency.

All audio is 48 kHz, 16-bit mono WAV. Each sample starts sounding within 0.5 ms (leading
silence would add latency) and fades to exactly zero. Each pack is under 2 MB.

## Synthesis model

One keystroke event (press or release) is rendered in `render.rs` from the parameters in
`params.rs`. The values come from the M2 keyboard-acoustics research: timing from acoustic
side-channel papers, damping from Ren et al. 2013, and contact times from Hertz impact theory.

- **t = 0 is the impact.** That means the bottom-out for linear switches, and the click for
  clicky ones. The physical event comes 4–10 ms after the OS key event, about the same as
  TakTak's output latency, so the sample needs no leading silence.
- **Excitation.** A raised-cosine contact pulse (van den Doel et al. 2001) with 1–3
  micro-bounces, plus a band-passed noise burst for friction and micro-collisions. A harder
  stroke is louder and has a shorter, brighter contact (t_c ∝ v^−1/5).
- **Keycap and stem modes.** Damped-sine resonators (J. O. Smith's two-pole form, which starts
  at zero so there is no onset click). Their decay follows a fitted damping law for plastic,
  so high modes die in a few ms and lower ones ring for tens of ms. Several inharmonic modes
  per layer avoid the "sine beep".
- **Plate and case body.** Low modes (160 Hz–2 kHz) driven by a longer, smoother pulse,
  because the switch and plate filter the impact. Thock gets strong low body modes and no
  cavity. Clack gets a light body and a hollow-case cavity (noise through a resonant
  band-pass).
- **Optional layers.**
  - Spring ping (unlubed packs only), at a slightly different pitch for every switch.
  - Click jacket (Blue Click). The click is a very short pulse into its own bright modes, with
    the bottom-out about 5 ms later. On release, a softer up-click comes before the top-out.
  - Stabilizers (space, enter, backspace, shifts): flams, where the stabilizer stems land
    0.2–2.5 ms after the centre switch, and faint wire-rattle ticks. The space bar adds its
    own bending modes.
- **Room.** Two low-passed early reflections and a 60–70 ms noise tail stand in for the desk
  and the case. Then a high-pass (70–90 Hz, so laptop speakers do not get mud), a low-pass,
  the level, and a raised-cosine fade.
- **Release (top-out).** Quieter than the press (measured about −12, −4 and −9 dB for Thock,
  Clack and Click; a hard top-out is louder than a damped one), shorter, and brighter: no
  finger mass drives the plate, and the cap modes sit about 8 % higher.
- **Variation.**
  - Per key (`layout.rs`): body-mode gains follow plate mode shapes at the key's position, like
    hitting a drum in different places. Cap pitch follows row height, level follows the finger
    that usually strikes the key, and each key gets fixed small offsets.
  - Per take: velocity, contact time, mode frequencies, gains and decays, bounce timing and
    noise seeds are all jittered.

### Mastering

Each pack gets one gain for all its files, never per file, so the press/release balance and
the key-class balance survive. The three packs are matched on typing loudness: the mean
K-weighted (BS.1770) energy of all the per-key alphanumeric presses (second takes included)
over 100 ms. The pack whose
loudest press would clip first (Blue Click, the highest crest factor) peaks at −3 dBFS, and
the other two sit lower at the same loudness. Tails are cut once they stay below −60 dBFS.
That level, −25.8 LK, is the reference every other bundled pack is matched to (below). The
`loudness` measure below, which counts only the take each key plays by default, reads the
three packs within 0.2 dB of it.

## Measuring any pack's loudness

```
cargo run -p synth-packs --release -- loudness packs/*            # or any pack folder / .zip
cargo run -p synth-packs --release -- loudness --check packs/*    # exit 1 if a pack is off
cargo run -p synth-packs --release -- loudness --target -24 my-pack
```

`loudness` (`loudness.rs`) loads each pack with TakTak's own loader at 48 kHz, so it measures
the samples exactly as the app plays them (decoded, mono, leading silence trimmed, resampled),
and measures typing the way the app plays it by default: for each alphanumeric key, the
K-weighted energy of the first 100 ms of the sample that key plays on press (the one the app's
consistent variant choice, `taktak_core::audio::consistent_index`, gives it), power-averaged
over the keys, so a sample several keys share counts once per key. Per pack it prints:

| Column | Meaning |
|---|---|
| `n` | distinct samples the alphanumeric keys play on press by default |
| `LK raw` / `LK eff` | typing loudness as stored, and with the pack's `volume` |
| `vs ref` | `LK eff` minus the target (default −25.8 LK) |
| `status` | `ok` within 0.5 dB; `limited` quieter, but already at the largest clean volume; `OFF` fix it |
| `rel-press dB` | the same measure on the alphanumeric keys' releases, relative to the presses |
| `true peak eff` | loudest true peak (4x oversampled) of any sample a key can play (random variants included), times `volume` and the top of the volume variation |
| `max clean vol` | the largest `volume` that keeps that true peak at or below −1 dBFS (the mixer clips hard at 0 dBFS) |
| `volume for ref` | the `volume` to write into pack.json, or how much gain the build must add when +6 dB (`volume` 2.0) is not enough |

The recorded packs' build scripts (`tools/pack-sources/<id>/`) write the `volume` measured this
way, and each pack's `SOURCES.md` records it. After changing a build, rebuild the pack, run
`loudness` on it and update the script's `VOLUME`. The test
`loudness::tests::bundled_packs_are_loudness_matched` fails when a bundled pack is `OFF`.

## Checking the result without listening

Generation prints a table per pack and group with these columns:

- sample count
- mean duration
- L40: time to −40 dB re peak
- peak dBFS
- RMS over the first 60 ms
- K-weighted level
- spectral centroid over the first 60 ms and the first 12 ms
- the share of the first 5 ms above 4 kHz
- crest factor

The packs should separate like this:

- Deep Thock: centroid around 700 Hz, almost nothing above 4 kHz, longest decay.
- Crisp Clack: around 2 kHz, short.
- Blue Click: the brightest attack (around 3 kHz in the first 12 ms), roughly 30 % of its
  onset energy above 4 kHz, and the highest crest factor.

Releases are quieter and brighter than presses in all three.

`cargo test -p synth-packs` covers the DSP blocks (resonator peak and decay, filters, pulse,
noise, damping law, determinism), SHA-256 against the NIST vectors, the rendered events
(finite, immediate onset, zero end, no DC, the intended differences between packs), and the
generated packs. For the packs it checks every alphanumeric key and every group, valid key and
group names, that every file exists and is ≤ 2 s, that files end at zero, the size limit,
loudness matching, that `SOURCES.md` lists every other file with its correct size and SHA-256,
byte-identical regeneration (also into a different folder, and over a hand-edited
`SOURCES.md` or a stale sample), and that folders the generator did not write are left alone.
For `loudness` it checks that level and `volume` shift the measurement by the right number of
dB, that it agrees with the generator's own measure, that it weights each alphanumeric key's
default sample once per key, the true-peak estimate (a quarter-rate
sine at 45° reads 0 dBFS from samples at −3 dBFS), the headroom and status logic, and that
every pack in `packs/` is matched to the reference or headroom-limited.

The bundled-pack gate, `cargo test -p taktak-core --test bundled_packs`, then checks the
regenerated `packs/` against docs/pack-format.md.
