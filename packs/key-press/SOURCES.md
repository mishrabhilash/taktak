# Key Press: sources and provenance

Pack id `key-press`, license **CC-BY-3.0**. Everything here was checked on **2026-10-02**.
`LICENSE.txt` has the credits, a list of the changes made, and the full CC BY 3.0 legal code.

Credit line (also in `pack.json` → `attribution`):

> "Single Key Press Sounds" by eklee, qubodup (https://opengameart.org/content/single-key-press-sounds)
> and "Mechanical keyboard sound" by bluszcz (https://opengameart.org/content/mechanical-keyboard-sound),
> both CC BY 3.0 (https://creativecommons.org/licenses/by/3.0/). Sliced, mixed to mono and
> level-adjusted by TakTak contributors.

## 1. Original works

### 1.1 "Single Key Press Sounds", by eklee, submitted by qubodup (OpenGameArt, 2012-11-11)

| | |
|---|---|
| Source page | https://opengameart.org/content/single-key-press-sounds |
| Direct download | https://opengameart.org/sites/default/files/eklee-KeyPresses-cc0-opengameart.zip (2,339,648 bytes) |
| SHA-256 of the zip | `45c9b07560f7123ced930584c9b6a6de50551620b1c968b973f495dec21c8987` |
| Authors | eklee (recordings), qubodup (extraction of single sounds) |
| License | CC BY 3.0 (SPDX `CC-BY-3.0`) |
| License proof | The page's "License(s)" field reads **CC-BY 3.0** and links to http://creativecommons.org/licenses/by/3.0/. Its "Attribution Instructions" read "Single Key Press Sounds by eklee, qubodup". The page body (edited 2025-02-28) and `eklee-KeyPresses.txt` inside the zip both give the redistribution notice "Copyright 2012 eklee … \| Copyright 2012 qubodup … extraction of single sounds \| License: CC Attribution 3.0". The zip also includes the CC BY 3.0 legal code as `license.txt`. |
| Name conflict | The zip is named `…-cc0-…`, but the page's license field and the notice inside the zip both say CC BY 3.0. This pack treats the work as **CC-BY-3.0**, the stricter reading. |
| Upstream | qubodup cut the sounds from eklee's Freesound recordings [158057](http://freesound.org/people/eklee/sounds/158057/) and [158058](http://freesound.org/people/eklee/sounds/158058/). Both Freesound pages now return 404. The keyboards are not named. The file names say "Mac" and "Old", and the OGA forum thread [key-press-sounds](https://opengameart.org/forumtopic/key-press-sounds) notes that each take holds both a press and a release. |
| Checked | 2026-10-02: page fetched and saved, license field and notice read, zip downloaded and hashed |

Files from the zip:

| File (in `eklee-KeyPresses-cc0-opengameart/`) | Bytes | SHA-256 | Used? |
|---|---|---|---|
| `flac48000/eklee-KeyPressMac01.flac` | 120091 | `02e5a4702a8fa6994658da70c597e1f1d84247f1eb47fe93ac096408f57b0ad7` | yes |
| `flac48000/eklee-KeyPressMac02.flac` | 110061 | `e7a6af0cb0d3e1c680167381e7bdf35be29509350a755f0ed7979d911250535c` | yes |
| `flac48000/eklee-KeyPressMac03.flac` | 107445 | `b447e2cbe44fe2f24af01c256b91c5b2db077dcc38ad3b7e422a6665e273286f` | yes |
| `flac48000/eklee-KeyPressMac04.flac` | 90974 | `5a4c0d63e314c4e551faf933a38e62654f1631310b9da49cf7a040e6f3edac6f` | yes |
| `flac48000/eklee-KeyPressMac05.flac` | 89004 | `db9644a2c99192ec2624ab68d6de84b185f0c12c91916da23996eeea7a332c0a` | yes |
| `flac48000/eklee-KeyPressMac06.flac` | 77052 | `105376823bb64468fc0d4f4fe734e3cae2f75e76d08bcc5be032bd69e4e55e6e` | yes |
| `flac48000/eklee-KeyPressMac07.flac` | 85030 | `d06cb660ced0e1fb3e5e733bacbf8a89c7b596264fb6616beb5becc1aab4e514` | yes |
| `flac48000/eklee-KeyPressOld01.flac` | 100517 | `a82bd053bb4b22f4777a3e67ae703c549316aa4f2ac87dc01422f50b9696ed6d` | yes |
| `flac48000/eklee-KeyPressOld02.flac` | 99023 | `eb2dcef790dd1787b8e267d47df524cd7810d3586800c9034b262c1e21423970` | yes |
| `flac48000/eklee-KeyPressOld03.flac` | 103261 | `820cba5f224366256cf0251f645190678337990452fb87c1d5bd270617262efb` | yes |
| `flac48000/eklee-KeyPressOld04.flac` | 70365 | `95b03abfe07d072f45351a8143d4ff3c469f967f61a718e1d8ef765f009e6ba4` | yes |
| `flac48000/eklee-KeyPressOld05.flac` | 68753 | `1c12f9f510b1a776ddbaeb9b03d11fd995a31a8722065b30d8eedfc94278bd72` | no: the same take as Old01 (shifted by 461 samples, it matches Old01's mono mix to within −37 dB) |
| `flac48000/eklee-KeyPressOld06.flac` | 66453 | `7138c6872cab60373a53a1234ef732a50fa1ede5972c0707e7dec1f941aa5d3c` | no: a low thump and small ticks start 30–80 ms before the click, which would play as latency |
| `flac48000/eklee-KeyPressOldEffect01.flac` | 151438 | `f4b44f249559b39d0f933d58fcb5e0e6929e410a9ec05ec69a8f58aad32902fd` | no: an effect-processed copy of Old06 |
| `eklee-KeyPresses.txt` | 404 | `e106ce57153173049f342b9ae8700938494a86c002228ef68e0ab2d32b7c5b0e` | notice text, quoted in `LICENSE.txt` |
| `license.txt` | 19466 | `5138a74ac20f2965d97b2a9c35219ca74e24f4a93618b33f0e36e4b3c7873197` | CC BY 3.0 legal code, appended unchanged to `LICENSE.txt` |
| `wav44100/*.wav` | | | no: 44.1 kHz/16-bit copies of the same takes; the 48 kHz/24-bit FLACs were used instead |

### 1.2 "Mechanical keyboard sound", by bluszcz (OpenGameArt, 2016-05-25)

| | |
|---|---|
| Source page | https://opengameart.org/content/mechanical-keyboard-sound |
| Direct downloads | https://opengameart.org/sites/default/files/keyboard01_0.ogg (14,624 bytes), https://opengameart.org/sites/default/files/keyboard02_0.ogg (21,373 bytes) |
| SHA-256 | `keyboard01_0.ogg`: `5917fc7ce559058569bbdd9df9f32c6e07eda3a13a1be3e7a090761a24cee227`; `keyboard02_0.ogg`: `21815899715b18b9c703b0a94dde19aad1ad7561b3a6d95bff088442dcaeefc4` |
| Author | bluszcz |
| License | CC BY 3.0 (SPDX `CC-BY-3.0`) |
| License proof | The page's "License(s)" field reads **CC-BY 3.0** and links to http://creativecommons.org/licenses/by/3.0/. The page gives no special attribution instructions, so the credit follows the OGA FAQ: title, author, license, link. |
| Content | The page describes it as a recording of bluszcz's own mechanical keyboard. There are two short Ogg Vorbis clips (44.1 kHz mono, 0.82 s and 1.32 s) of quick typing, several strokes each. The keyboard model is not named |
| Checked | 2026-10-02: page fetched and saved, license field read, both files downloaded and hashed |

## 2. Processing

The steps are reproducible with `tools/pack-sources/key-press/fetch.sh DOWNLOADS`, then
`python3 tools/pack-sources/key-press/build.py --downloads DOWNLOADS --out packs/key-press`.
The build needs ffmpeg and numpy; it was run with ffmpeg 9.0.2, numpy 2.4.2 and Python 3.14.

1. **Decode.**
   - eklee FLAC (48 kHz, 24-bit, stereo): mixed to mono as (L+R)/2 with ffmpeg
     `pan=mono|c0=0.5*c0+0.5*c1`, then a 2-pole 30 Hz high-pass (`highpass=f=30:poles=2`).
     The pass removes DC and sub-audio rumble.
   - bluszcz OGG (44.1 kHz, mono): the same 30 Hz high-pass, then resampled to 48 kHz
     (`aresample=48000:filter_size=128:phase_shift=10:cutoff=0.97:filter_type=kaiser:kaiser_beta=9`).

   Both filters run over the whole file, before slicing.
2. **Slice.** Each pack file is the sample range listed in §3, in the decoded 48 kHz signal.
   - Every slice starts 24 samples (0.5 ms) before the stroke's onset, with a raised-cosine
     fade-in over those 24 samples. That cuts room noise and finger noise before the stroke
     to under 0.5 ms of lead-in.
   - Every slice ends with a raised-cosine fade-out to exactly zero.
3. **Gain.** One gain for the whole pack, **−5.92 dB**, makes the median press peak −6 dBFS
   (it was −0.08 dBFS before the gain). No file is normalized on its own.
   - qubodup had already peak-normalized each eklee take, so their peaks sit close together.
   - The bluszcz strokes keep their natural differences in level.
4. **Encode.** 16-bit PCM WAV, 48 kHz, mono, rounded with no dither.
5. **Preview.** `preview.wav` (1.89 s) mixes the pack's own files as a typed phrase: 3 letters,
   Space, 3 letters, Enter. Each key gets a press and a release, with 60–120 ms dwell and
   80–200 ms gaps (fixed random seed). Enter uses a letter release, as the runtime does.
6. **Loudness.** `volume` in `pack.json` is **1.14** (+1.1 dB): a playback gain the app
   applies to the whole pack, so no audio file changes. It matches typing loudness across the
   bundled packs, so switching packs does not jump in volume; the target is TakTak's
   reference typing level, −25.8 LK. Measured with
   `cargo run -p synth-packs --release -- loudness packs/key-press`
   (`tools/synth-packs/src/loudness.rs`): the K-weighted (ITU-R BS.1770) energy of the first
   100 ms of the sample each alphanumeric key plays on press by default, power-averaged over
   the keys, on the samples as the app loads them at 48 kHz. The pack reads −26.8 LK as built and −25.7 LK at
   this volume. The loudest true peak, at the top of the ±6 % volume variation, becomes
   −2.5 dBFS (the headroom rule is at most −1 dBFS, as the mixer clips hard at 0 dBFS).
   `build.py` writes the volume (`VOLUME`), so a rebuild keeps it.

**How the cut points were chosen.** They are frozen in `build.py` as sample indices. They
were picked once, as follows:

- **Press onset:** the first sample at or above −26 dB relative to the take's peak, within
  the 30 ms before its loudest high-frequency (above 800 Hz) transient.
- **Release onset:** the loudest 1 ms high-frequency peak 60–300 ms after the press. It was
  accepted as a release only if it was at least −25 dB relative to the press and stood at
  least 25 dB above the median level of that window. The onset is the first sample within
  12 dB of that peak.
  - Takes with an accepted release were split there. The press fades out over the 6 ms
    (12 ms for Old01, which cuts through a 60 Hz ring) before the release onset, and the
    release slice starts at that point.
  - Takes without a clear release (Mac03, Old02, Old03, Old04) keep the whole stroke. Any
    release in them is faint (at least 25 dB below the press) and stays inside the press sound.
- **Tail end:** where the 5 ms RMS stays below the larger of twice the noise floor and
  −56 dB relative to the take's peak for 15 ms. Presses are capped at 400 ms and releases
  at 250 ms. The tail then gets an 8 ms fade.
  - Mac01's release was cut by hand at 236 ms, just before an unrelated tick at 239 ms.
- **bluszcz strokes:** the typing overlaps, so each stroke runs from its main attack to the
  next stroke's onset, with a 6 ms fade, or to its natural decay, with an 8 ms fade.
  - The main attack is the start of the run of 1 ms blocks, leading up to the stroke's
    peak, that stay within −18 dB of that peak. The slice starts 0.5 ms before it.
  - In these clips a quieter sound comes 6–18 ms before that run, peaking 11–20 dB below
    the click (most likely the neighbouring keystroke in this fast typing). It is left out,
    so that the click plays at once and does not sound doubled. (Re-cut on 2026-10-02: the
    first cut started at the first rise, up to 18 ms before the click.)
  - Six loud strokes were used.
  - The other strokes were left out. They overlap the next stroke within 30–60 ms, are
    7–16 dB quieter, or have a second hit 10 ms in.
  - No releases were taken from these clips, because press and release cannot be told
    apart reliably in this fast typing.

## 3. Per-file derivation

Sample ranges index the decoded 48 kHz signal (step 1); `end` is exclusive. Times are in
ms from the start of the original file. The bluszcz times hold for the 44.1 kHz original too.
Every file also gets the 0.5 ms fade-in, the −5.92 dB pack gain and 16-bit encoding.

| Pack file | Source file | Samples @48 kHz | Time (ms) | Length (ms) | Fade-out (ms) |
|---|---|---|---|---|---|
| `sounds/light-press-01.wav` | `eklee-KeyPressMac01.flac` | 1004–5431 | 20.92–113.15 | 92.2 | 6 |
| `sounds/light-press-02.wav` | `eklee-KeyPressMac02.flac` | 1075–11819 | 22.40–246.23 | 223.8 | 6 |
| `sounds/light-press-03.wav` | `eklee-KeyPressMac03.flac` | 1028–11293 | 21.42–235.27 | 213.9 | 8 |
| `sounds/light-press-04.wav` | `eklee-KeyPressMac04.flac` | 425–5097 | 8.85–106.19 | 97.3 | 6 |
| `sounds/light-press-05.wav` | `eklee-KeyPressMac05.flac` | 254–4526 | 5.29–94.29 | 89.0 | 6 |
| `sounds/light-press-06.wav` | `eklee-KeyPressMac06.flac` | 240–4538 | 5.00–94.54 | 89.5 | 6 |
| `sounds/light-press-07.wav` | `eklee-KeyPressMac07.flac` | 681–4960 | 14.19–103.33 | 89.1 | 6 |
| `sounds/light-release-01.wav` | `eklee-KeyPressMac01.flac` | 5431–11328 | 113.15–236.00 | 122.9 | 8 |
| `sounds/light-release-02.wav` | `eklee-KeyPressMac02.flac` | 11819–15328 | 246.23–319.33 | 73.1 | 8 |
| `sounds/light-release-04.wav` | `eklee-KeyPressMac04.flac` | 5097–11175 | 106.19–232.81 | 126.6 | 8 |
| `sounds/light-release-05.wav` | `eklee-KeyPressMac05.flac` | 4526–12131 | 94.29–252.73 | 158.4 | 8 |
| `sounds/light-release-06.wav` | `eklee-KeyPressMac06.flac` | 4538–7628 | 94.54–158.92 | 64.4 | 8 |
| `sounds/light-release-07.wav` | `eklee-KeyPressMac07.flac` | 4960–13067 | 103.33–272.23 | 168.9 | 8 |
| `sounds/deep-press-01.wav` | `eklee-KeyPressOld01.flac` | 1044–5079 | 21.75–105.81 | 84.1 | 12 |
| `sounds/deep-press-02.wav` | `eklee-KeyPressOld02.flac` | 551–15497 | 11.48–322.85 | 311.4 | 8 |
| `sounds/deep-press-03.wav` | `eklee-KeyPressOld03.flac` | 226–10710 | 4.71–223.12 | 218.4 | 8 |
| `sounds/deep-press-04.wav` | `eklee-KeyPressOld04.flac` | 331–14913 | 6.90–310.69 | 303.8 | 8 |
| `sounds/deep-release-01.wav` | `eklee-KeyPressOld01.flac` | 5079–15568 | 105.81–324.33 | 218.5 | 8 |
| `sounds/clack-01.wav` | `keyboard01_0.ogg` | 696–3574 | 14.50–74.46 | 60.0 | 6 |
| `sounds/clack-02.wav` | `keyboard01_0.ogg` | 21665–24698 | 451.35–514.54 | 63.2 | 6 |
| `sounds/clack-03.wav` | `keyboard02_0.ogg` | 1914–5603 | 39.88–116.73 | 76.9 | 6 |
| `sounds/clack-04.wav` | `keyboard02_0.ogg` | 5993–9880 | 124.85–205.83 | 81.0 | 6 |
| `sounds/clack-05.wav` | `keyboard02_0.ogg` | 16068–19293 | 334.75–401.94 | 67.2 | 6 |
| `sounds/clack-06.wav` | `keyboard02_0.ogg` | 25250–30889 | 526.04–643.52 | 117.5 | 8 |
| `preview.wav` | mix of the files above | — | — | 1888.0 | — |

## 4. Mapping

| Group | Press | Release |
|---|---|---|
| `alphanumeric` | `light-press-01…07` (eklee "Mac" takes) | `light-release-01, 02, 04, 05, 06, 07` |
| `space` | `deep-press-01, 02` (eklee "Old" takes) | `deep-release-01` |
| `backspace` | `deep-press-03, 04` | `deep-release-01` |
| `modifiers` | `deep-press-01…04` | `deep-release-01` |
| `enter` | `clack-01…06` (bluszcz) | none, so the runtime falls back to the `alphanumeric` releases |

Every other key (`other`) falls back to `alphanumeric`. Runtime variation is set to pitch
±0.015 and volume ±0.06.

## 5. Result

| Group | Count | Mean length (ms) | Peak dBFS, median (min..max) | RMS dBFS (mean) | Max leading silence (ms) |
|---|---|---|---|---|---|
| alphanumeric.press | 7 | 127.8 | −5.9 (−6.0..−5.9) | −30.8 | 0.48 |
| alphanumeric.release | 6 | 119.0 | −18.9 (−28.6..−14.4) | −44.2 | 0.46 |
| space/backspace/modifiers.press | 4 | 229.4 | −6.2 (−7.3..−6.1) | −29.9 | 0.38 |
| space/backspace/modifiers.release | 1 | 218.5 | −22.4 | −38.8 | 0.12 |
| enter.press | 6 | 77.6 | −6.0 (−8.1..−5.9) | −26.8 | 0.25 |

Leading silence is the time before the first sample at or above −50 dBFS, the loader's
trim threshold.

## 6. Shipped files

SHA-256 of every audio file as shipped. A rebuild with `build.py`, using the same
ffmpeg and numpy versions, reproduces them byte for byte (checked 2026-10-02).

| File | Bytes | SHA-256 |
|---|---|---|
| `sounds/clack-01.wav` | 5800 | `816311f6481850a9d5df870bfdc4ad0af7bbec4f730a8eecb5505c93513f5fec` |
| `sounds/clack-02.wav` | 6110 | `d7ff015992789e1c1007fea6f286f69c040417e9004834fd5388c8fdd228d9cc` |
| `sounds/clack-03.wav` | 7422 | `ac54c3919de26aa171448ef346b8ae597fc13e5ca7a8074863f37639f873e1fa` |
| `sounds/clack-04.wav` | 7818 | `ed69a7415093183ffe83e89cfef2db50a6d4761a7b6c7919c8c1b1fdd2427dc3` |
| `sounds/clack-05.wav` | 6494 | `6558a55940c844d686e6261c3d5aaefca3da501af4acb56c5bd19ca69bc38873` |
| `sounds/clack-06.wav` | 11322 | `bd902662bb589bd8171ddff66b68039ce31f31adf94ab87584365712f5c79717` |
| `sounds/deep-press-01.wav` | 8114 | `88daa09b8833fe681b7b3d1dd7a0fa7d46b4282b4820ce453ec2fd66be427b89` |
| `sounds/deep-press-02.wav` | 29936 | `787d474bd1b3822ea2c862f0ea92bc24f0df7a07a5f1e4fb4ca23a28cb2886f6` |
| `sounds/deep-press-03.wav` | 21012 | `dc87702e044b4e1a1363be6c3112e1be2e848450ef3761867e8675d34c8738e2` |
| `sounds/deep-press-04.wav` | 29208 | `a4712b218103fb04e6df1c53423315c3b3721257f008c858f2eb660c228d6b90` |
| `sounds/deep-release-01.wav` | 21022 | `67eb5a83a914aca60c4870f0c0ab0fb9fcaa0c7891fe7005c34eb7f391b079c1` |
| `sounds/light-press-01.wav` | 8898 | `bf58d1a77d4e336dbc4e3d006b36baf8b95c71e9c2fc8912e6f7a7f729bd9b1c` |
| `sounds/light-press-02.wav` | 21532 | `96030eaa4c66e84536f98cccbd4c4838098584fd76c6bfdd2bb264999eef08f0` |
| `sounds/light-press-03.wav` | 20574 | `37422e12f074a8a57068b331b46edbcd67994796153f1800c8d493bafe6750cc` |
| `sounds/light-press-04.wav` | 9388 | `d106760615fc415754c7b8653bb1a2b63975e5d2c1b8bb74b17b01e35658d1df` |
| `sounds/light-press-05.wav` | 8588 | `4a56bffb99a2e72900890425a227db71272b352d6c82676108cc83172e71659a` |
| `sounds/light-press-06.wav` | 8640 | `b58c4e8752c7ed5a9bf28f5446d50eb6dcac90fa32032d08851d98c43d360181` |
| `sounds/light-press-07.wav` | 8602 | `04634426be9f656bd98dcd93d0db618617843d152692417b21fea03a2bf3bbc2` |
| `sounds/light-release-01.wav` | 11838 | `6ddaad2f317ce437931150aab900deb555baa68b5be784e991f2f5a8f3fcb148` |
| `sounds/light-release-02.wav` | 7062 | `23880460989221f0139c4d91ba7a53c4a4da92ea76a13036328b2b63d3d80eef` |
| `sounds/light-release-04.wav` | 12200 | `b6f4e8b6491293fa4a9d5d492876473f2e4bfba5994c6e18d45614f51e091926` |
| `sounds/light-release-05.wav` | 15254 | `bd71ffc2ff637247ad94bb2617ce41de1fca0df23d85aa6f5500fa760e8e7749` |
| `sounds/light-release-06.wav` | 6224 | `216e1075cc1a841b6e816216d75ec9c1d57f4b2f0552dfb7954ae04dce4f1991` |
| `sounds/light-release-07.wav` | 16258 | `0bc04043e1a651d0425faa92c9f1c83f55c61caaf4ea27c9682bf7610d9327ce` |
| `preview.wav` | 181290 | `b4d5bc52e83f2554ea454988dccef7f1ab74388d73a227074a3fb8b81589ed2e` |
