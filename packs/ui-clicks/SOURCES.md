# UI Clicks: sources and provenance

Pack id `ui-clicks`, license **CC0-1.0**. Everything here was checked on **2026-10-02**.
CC0 does not require a notice, so the pack has no `LICENSE.txt`. Kenney asks for credit as a
courtesy (not a condition); `pack.json` → `attribution` gives it:

> UI sounds by Kenney (kenney.nl), CC0

This is not a keyboard recording. Every sound is an interface sound that Kenney made for
games and apps; the pack maps them onto keys for fun.

## 1. Original works

Both works are by Kenney (Kenney Vleugels, https://kenney.nl). "Kenney" is the author's name
and trade name. It appears here and in the description as a credit only, and not in the
pack's name or id.

### 1.1 "UI Audio" (the zip's own title: "UI SFX Set")

| | |
|---|---|
| Source page | https://kenney.nl/assets/ui-audio |
| Download | The page's "Download" button: `kenney_ui-audio.zip`, 411,949 bytes. Kenney serves it from a versioned URL under `https://kenney.nl/media/pages/assets/ui-audio/` whose version part changes whenever the page is updated. The exact URL of this copy was not recorded, so the zip is identified by its SHA-256 below. |
| SHA-256 of the zip | `946fc23a63d535d693eb31b2eabb80c8c28d6351e2186b344ceb71b2cb1d5eb6` |
| Author | Kenney Vleugels (Kenney.nl), as named in the zip's `License.txt` |
| License | Creative Commons Zero 1.0 Universal (SPDX `CC0-1.0`), https://creativecommons.org/publicdomain/zero/1.0/ |
| License proof | `License.txt` in the zip (479 bytes, dated 2017-10-28, SHA-256 `4f88ab3c885c87874834441a0d009cea8942f57461d7b870be65cf4e31362073`) names "License (Creative Commons Zero, CC0)" with the link http://creativecommons.org/publicdomain/zero/1.0/. It allows use in personal and commercial projects and says credit is not mandatory. The source page also lists the license as CC0. |
| Contents | 51 Ogg Vorbis sounds in `Audio/` (44.1 kHz, stereo), `Preview.ogg`, `License.txt` and two `.url` shortcuts. The shortcuts and the preview were not opened. |
| Checked | 2026-10-02: zip hashed, `License.txt` read from the zip, every sound in `Audio/` decoded and measured |

### 1.2 "Interface Sounds" (version 1.0)

| | |
|---|---|
| Source page | https://kenney.nl/assets/interface-sounds |
| Download | The page's "Download" button: `kenney_interface-sounds.zip`, 834,536 bytes, from a versioned URL under `https://kenney.nl/media/pages/assets/interface-sounds/`. As above, the exact URL was not recorded and the SHA-256 identifies the zip. |
| SHA-256 of the zip | `f2193d072726d6758a5f7871b2dcc54dcce0d5c35c6f0a62f92549b327c81232` |
| Author | Kenney (www.kenney.nl): "Created/distributed by Kenney" in the zip's `License.txt` |
| License | Creative Commons Zero 1.0 Universal (SPDX `CC0-1.0`), https://creativecommons.org/publicdomain/zero/1.0/ |
| License proof | `License.txt` in the zip (574 bytes, "Creation date: 11-02-2020", SHA-256 `f7966c773bbed0eca6a9c75081c44a178b38eae112724dbb5fdfbd4192d118a9`) names "License: (Creative Commons Zero, CC0)" with the link http://creativecommons.org/publicdomain/zero/1.0/. It allows use in personal, educational and commercial projects and says crediting Kenney is not mandatory. The source page also lists the license as CC0. |
| Contents | 100 Ogg Vorbis sounds in `Audio/` (44.1 kHz, mono or stereo), `License.txt` and two `.url` shortcuts. The shortcuts were not opened. |
| Checked | 2026-10-02: zip hashed, `License.txt` read from the zip, every sound in `Audio/` decoded and measured |

### 1.3 Sounds used

The build reads these members straight from the zips; nothing else in them is used.

| Zip → member | Bytes | Channels | SHA-256 of the member |
|---|---|---|---|
| `kenney_ui-audio.zip` → `Audio/switch2.ogg` | 6042 | 2 | `aa8ad6e4745e87c84c24a0335e0e0f8629ccbbc474ccfd544fe07d7f3a1b2280` |
| `kenney_ui-audio.zip` → `Audio/switch4.ogg` | 6477 | 2 | `687a42c24e5d25be1a256f35a439fe858548fe76d43ccfb17db546618b0d36db` |
| `kenney_ui-audio.zip` → `Audio/switch9.ogg` | 6009 | 2 | `069861740681c528866483e3d528d79034f24d744c4fc79f7045b7d9ed999e26` |
| `kenney_ui-audio.zip` → `Audio/switch10.ogg` | 6456 | 2 | `bf61b3f3af39b2c680e70ee2f791786edd2bca2016ea683947059f079849bbf3` |
| `kenney_ui-audio.zip` → `Audio/switch12.ogg` | 4588 | 2 | `dab0c21eacb6536724d139c27c0d60c932c87171752dc8d24d7a826694377f94` |
| `kenney_ui-audio.zip` → `Audio/switch13.ogg` | 4467 | 2 | `54d1e63b907274bb415a8de7f652194756a8eeab4079a7cf769ec153b93a436a` |
| `kenney_ui-audio.zip` → `Audio/switch14.ogg` | 4522 | 2 | `c640b370738c816eda3d1196d8a8dda7130825b4094f3bb3cbba22a9f6f9b7c9` |
| `kenney_ui-audio.zip` → `Audio/switch15.ogg` | 6325 | 2 | `5a20a9d5365dec8fec1311731540f56429cab38822326133c7ce60fbcf9fd275` |
| `kenney_ui-audio.zip` → `Audio/switch17.ogg` | 6686 | 2 | `d09974d4cf5ccb2d75647e2464cd424e7e3c7a0374e73e8a22577662031a3677` |
| `kenney_ui-audio.zip` → `Audio/switch19.ogg` | 7177 | 2 | `63753f884e4e0d44b0939bee9f048b32e658d996958930363c0579ffa0f810c1` |
| `kenney_ui-audio.zip` → `Audio/switch26.ogg` | 6031 | 2 | `1a628e2fbede99afdb57533fa367f352fede596df18c91deb841a21f8171654a` |
| `kenney_ui-audio.zip` → `Audio/switch28.ogg` | 5916 | 2 | `2f7b627d85ce1530faef5374d3826bfca1ff2823b1b41d5f908448976061e2d4` |
| `kenney_ui-audio.zip` → `Audio/switch29.ogg` | 6769 | 2 | `b2923ed170e963b5e211bf73e75f8eeb41f6b49a46eb52f3b3fc43234c713a16` |
| `kenney_ui-audio.zip` → `Audio/switch30.ogg` | 7151 | 2 | `d220abb5218fccc6d37649e17e7cce2a4e253f474332d9ea2b0f0518264aff5a` |
| `kenney_ui-audio.zip` → `Audio/switch31.ogg` | 8180 | 2 | `bca5f3047e73d28f6619d49a45dee1984340c165da5fa49b239068c2cb9b4be1` |
| `kenney_ui-audio.zip` → `Audio/switch33.ogg` | 8530 | 2 | `977ecead4b27cf9801aed433bfa4bdfcf49444c15990d27d2e472401a1fb2742` |
| `kenney_ui-audio.zip` → `Audio/switch34.ogg` | 7910 | 2 | `6e7943ed133c1b559d009da5028da0a318fee0b1bcfe6e4fa49744ba2cd3975b` |
| `kenney_ui-audio.zip` → `Audio/switch38.ogg` | 7303 | 2 | `d5c164ba622e3c7904eb0f442a81cff9985bebbd8951202348d2a7c8a449309b` |
| `kenney_ui-audio.zip` → `Audio/click4.ogg` | 4558 | 2 | `9c02bbfc872b50eb8750adf4b7c343120f19f5d2f04ab06a7a8948368a77b872` |
| `kenney_ui-audio.zip` → `Audio/click5.ogg` | 4532 | 2 | `52d41e0bb012731bf2a391d53ffb52265ef21589c7701e66b391d5e36a65b293` |
| `kenney_interface-sounds.zip` → `Audio/toggle_001.ogg` | 7369 | 1 | `ca1d2dde5f0b286abac4f070e23edab8f30927c8a533665cdf2ac6492a415e49` |
| `kenney_interface-sounds.zip` → `Audio/toggle_002.ogg` | 8056 | 1 | `1f564f5520cfa0764028ce245ad97599d3ece8f0ab504554c403af92ac72b900` |
| `kenney_interface-sounds.zip` → `Audio/click_001.ogg` | 4876 | 1 | `ccfb7fa0cccdd9faec0eb16033c732b1e308d139d80f799161495d58f7adcdb9` |
| `kenney_interface-sounds.zip` → `Audio/click_003.ogg` | 4371 | 1 | `2fa929138bc3a0f432696588f66eac94b8f6d463905ec8808c6461ce4b054292` |

All are Ogg Vorbis at 44.1 kHz.

## 2. Processing

Rebuild with `tools/pack-sources/ui-clicks/fetch.sh DOWNLOADS` (it checks both zips'
SHA-256; download them by hand from the source pages, see the script), then
`python3 tools/pack-sources/ui-clicks/build.py --downloads DOWNLOADS --out packs/ui-clicks`.
The build needs ffmpeg and numpy; it was run with ffmpeg 9.0.2, numpy 2.4.2 and Python 3.14,
and a rebuild reproduces every shipped file byte for byte (last checked 2026-10-03). It reads the
members from the zips in memory; nothing is extracted and nothing from the zips is executed.

1. **Decode.** ffmpeg decodes each member at its own rate, 44.1 kHz. Stereo members are mixed
   to mono as (L+R)/2 (`pan=mono|c0=0.5*c0+0.5*c1`). Then a 2-pole 30 Hz high-pass
   (`highpass=f=30:poles=2`) removes DC.
2. **Find the sound.** Most pack files use a whole member. Three UI Audio switch files
   (`switch33`, `switch34`, `switch38`) hold two clicks about 160 ms apart; they are split
   at a point in the quiet gap between them (315, 268 and 195 ms from the file start), and
   the first click ("on") and the second ("off") become separate files. Within the part used:
   - **Start**: the first sample at or above -26 dB of the part's peak. Whatever comes
     before it (up to 140 ms of silence or faint lead-in in some switch files) is dropped,
     so every file starts with its click. A file must also reach -50 dBFS, the
     loader's leading-silence threshold, after its gain (step 4). Only the quietest file,
     `modifier-release-02`, was below that at its first -26 dB sample; its start moved
     1.52 ms later, to where it reaches -50 dBFS. The loader would have cut there anyway.
   - **End**: 2 ms into the first stretch of ten 1 ms windows, after the peak, that are all
     below -45 dB of the peak (the sound has died away), or the end of the part. Each file
     is also capped at its group's length: letters 120 ms, Space 150 ms, Enter 250 ms,
     Backspace 120 ms, modifiers 80 ms, releases 120 ms. Only `modifier-release-02` reaches
     its cap, with its tail already more than 45 dB down.
3. **Cut and fade.** Every file starts 22 samples (0.5 ms) before its start point, with a
   raised-cosine fade-in over those 22 samples. Where the click begins in the first 22
   samples of the member (both Enter files, both Backspace files), the missing samples are
   zeros. Every file ends with a raised-cosine fade-out to exactly zero: 8 ms, or a third of
   the file if that is shorter (`backspace-01` 7.8 ms, `backspace-02` 2.5 ms, `modifier-01`
   7.1 ms).
4. **Gain.** Kenney normalized each sound to about full scale, so the levels are set per
   group. One gain per group brings the median peak of the group's presses to a target:
   letters and Space -6 dBFS, Backspace -12 dBFS, modifiers -15 dBFS. Enter is levelled by
   loudness instead, because its two-note blips are sustained tones: at a peak target 4 dB
   under the letters' (-10 dBFS) they would play about 6 dB louder than the letters. Its
   gain puts the loudness of its presses 4 dB below the letters'.
   Loudness is measured as `synth-packs loudness` measures typing loudness: the K-weighted
   (ITU-R BS.1770) energy of each press's first 100 ms, power-averaged over the group
   (`k_loudness` in `build.py`). The gains are letters -4.22 dB, Space -5.44 dB, Enter
   -19.20 dB, Backspace -10.38 dB and modifiers -13.62 dB. Within a group, files keep their
   levels relative to each other. The "off" clicks take the gain of the group that plays
   them. The letters' releases get the letters' gain, the same as their own "on" clicks, so
   they stay 2-9 dB softer than them, as in the source. Enter, Backspace and the modifiers
   each get their own copy of the three "off" clicks. Without those copies they would fall
   back to the letters' releases, and their key-up would be louder than their key-down.
   Backspace's and the modifiers' copies get their group's gain. Enter's get the letters'
   gain moved by Enter's 4 dB loudness offset, -8.22 dB: at Enter's own gain the clicks
   would sit far below the blip, which is much louder than a click of the same peak. So the
   Enter key-ups are as much quieter than the Enter presses (6.3 dB) as the letters' key-ups
   are than the letters', although their short clicks peak higher than the blips. Space is
   within 1.5 dB of the letters and uses the letters' releases. Measured on the files as
   shipped (at volume 1.0), the presses play at: letters -24.5 LK, Space -24.6 LK, Enter
   -28.5 LK, Backspace -35.0 LK and modifiers -35.4 LK. The build stops if any group's
   presses are more than 1.5 dB louder than the letters'.
5. **Encode.** 16-bit PCM WAV, 44.1 kHz (the source rate), mono, rounded with no dither.
6. **Preview.** `preview.wav` (1.76 s) mixes the pack's own files as a typed phrase: 4
   letters, Space, 3 letters, Enter. It uses a fixed random seed, holds each key 60-110 ms
   and leaves 70-150 ms between keys. Every key plays its press and the release it gets at
   runtime.
7. **Loudness.** `volume` in `pack.json` is **0.86** (-1.3 dB): a playback gain the app
   applies to the whole pack, so no audio file changes. It matches typing loudness across the
   bundled packs, so switching packs does not jump in volume; the reference is the
   synthesized packs' -25.8 LK. Measured with
   `cargo run -p synth-packs --release -- loudness packs/ui-clicks`
   (`tools/synth-packs/src/loudness.rs`): the K-weighted (ITU-R BS.1770) energy of the first
   100 ms of the sample each alphanumeric key plays on press by default, power-averaged over
   the keys, on the samples as the app loads them at 48 kHz. The pack reads -24.2 LK as built and -25.5 LK at
   this volume. The loudest true peak, at the top of the ±8 % volume variation, becomes
   -2.6 dBFS (the headroom rule is at most -1 dBFS, as the mixer clips hard at 0 dBFS).
   `build.py` writes the volume (`VOLUME`), so a rebuild keeps it.

No other processing: no EQ, compression, pitch or time change. The loudest file peaks at
-2.2 dBFS and the preview at -4.1 dBFS.

## 3. How the sounds were chosen

Every sound in both zips (151) was decoded and measured: where it starts, how long it lasts,
where its peak is and its spectral centroid. The candidates were also checked on waveform
plots. A sound had to start with its main hit, reaching -6 dB of its peak within 4 ms of
the file start, and fit in 250 ms.

- **Letters, digits and punctuation**: 13 short, bright clicks from UI Audio (10 switch
  sounds, the first clicks of the three two-click switch files, and `click4`, `click5`).
  They are 29-93 ms long, with spectral centroids of 3.0-6.4 kHz (median 4.9 kHz).
- **Key-up**: the second, softer click of `switch33`, `switch34` and `switch38`. Kenney does
  not label the two clicks; the second one is taken as the switch going back. In the
  source it is 4.1, 9.1 and 2.0 dB below the first, and the pack keeps those differences.
  They match the switch clicks used for the presses, three of which are the first clicks
  of the same files. The only other release in the sets, `mouserelease1`, belongs to
  `mouseclick1`, a thin, very high click (centroid about 9-10 kHz) that the pack does not
  use. In `switch34` the second click starts with a smaller click, so `release-02` peaks
  16 ms in.
- **Space**: four lower, duller switch clicks from UI Audio. Their centroids are 2.6-3.6 kHz
  (median 2.8 kHz), with 33-50 % of the energy below 1 kHz; most letter clicks have under
  10 % there.
- **Enter**: `toggle_001` and `toggle_002` from Interface Sounds, short rising two-note
  blips of about 100 ms (1.42 then 1.90 kHz, and 2.11 then 3.19 kHz). They are the most
  confirm-like sounds under 250 ms. The sets'
  `confirmation_001`-`004` are 290-540 ms long, so they were left out. Set 4 dB below the
  letters by loudness (step 4).
- **Backspace**: `click_001` and `click_003` from Interface Sounds, short, soft clicks
  (23 ms and 8 ms; centroids 1.6 and 2.2 kHz, well below the letters), set 6 dB below the
  letters. `back_001`-`004` were left out because they are buzzy trains of 5-9 short
  clicks.
- **Modifiers**: `switch13`, `switch14` and `switch29` from UI Audio, tiny, very bright ticks
  (21-35 ms, centroids 6.7-8.0 kHz), set 9 dB below the letters.

## 4. Per-file derivation

Sample ranges index the decoded 44.1 kHz mono signal of step 1, `end` exclusive. A negative
start means the click begins at the very start of the member, and those pre-roll samples
are zeros. "Part" is the whole member (`all`), or the first (`on`) or second (`off`) click
of a two-click switch file. Every file also gets the 0.5 ms fade-in and 16-bit encoding.

| Pack file | Used as | Zip → member | Part | Source samples @44.1 kHz | Length (ms) | Fade-out (ms) | Gain (dB) | Peak (dBFS) |
|---|---|---|---|---|---|---|---|---|
| `sounds/click-01.wav` | letter press | `kenney_ui-audio.zip` → `Audio/switch2.ogg` | all | 1938–5964 | 91.3 | 8.0 | -4.22 | -6.8 |
| `sounds/click-02.wav` | letter press | `kenney_ui-audio.zip` → `Audio/switch4.ogg` | all | 6161–10275 | 93.3 | 8.0 | -4.22 | -7.0 |
| `sounds/click-03.wav` | letter press | `kenney_ui-audio.zip` → `Audio/switch9.ogg` | all | 3616–7378 | 85.3 | 8.0 | -4.22 | -4.1 |
| `sounds/click-04.wav` | letter press | `kenney_ui-audio.zip` → `Audio/switch12.ogg` | all | 172–2234 | 46.8 | 8.0 | -4.22 | -6.3 |
| `sounds/click-05.wav` | letter press | `kenney_ui-audio.zip` → `Audio/switch19.ogg` | all | 5280–8206 | 66.3 | 8.0 | -4.22 | -5.7 |
| `sounds/click-06.wav` | letter press | `kenney_ui-audio.zip` → `Audio/switch26.ogg` | all | 2362–4936 | 58.4 | 8.0 | -4.22 | -4.0 |
| `sounds/click-07.wav` | letter press | `kenney_ui-audio.zip` → `Audio/switch28.ogg` | all | 2623–4933 | 52.4 | 8.0 | -4.22 | -5.3 |
| `sounds/click-08.wav` | letter press | `kenney_ui-audio.zip` → `Audio/switch31.ogg` | all | 6547–10221 | 83.3 | 8.0 | -4.22 | -2.2 |
| `sounds/click-09.wav` | letter press | `kenney_ui-audio.zip` → `Audio/click4.ogg` | all | 218–1664 | 32.8 | 8.0 | -4.22 | -6.0 |
| `sounds/click-10.wav` | letter press | `kenney_ui-audio.zip` → `Audio/click5.ogg` | all | 260–1536 | 28.9 | 8.0 | -4.22 | -9.7 |
| `sounds/click-11.wav` | letter press | `kenney_ui-audio.zip` → `Audio/switch33.ogg` | on | 7185–9759 | 58.4 | 8.0 | -4.22 | -6.4 |
| `sounds/click-12.wav` | letter press | `kenney_ui-audio.zip` → `Audio/switch34.ogg` | on | 4914–8236 | 75.3 | 8.0 | -4.22 | -5.9 |
| `sounds/click-13.wav` | letter press | `kenney_ui-audio.zip` → `Audio/switch38.ogg` | on | 1572–5598 | 91.3 | 8.0 | -4.22 | -8.5 |
| `sounds/release-01.wav` | letter and Space key-up | `kenney_ui-audio.zip` → `Audio/switch33.ogg` | off | 14099–17597 | 79.3 | 8.0 | -4.22 | -10.5 |
| `sounds/release-02.wav` | letter and Space key-up | `kenney_ui-audio.zip` → `Audio/switch34.ogg` | off | 11857–16851 | 113.2 | 8.0 | -4.22 | -15.0 |
| `sounds/release-03.wav` | letter and Space key-up | `kenney_ui-audio.zip` → `Audio/switch38.ogg` | off | 8718–12260 | 80.3 | 8.0 | -4.22 | -10.5 |
| `sounds/space-01.wav` | Space press | `kenney_ui-audio.zip` → `Audio/switch10.ogg` | all | 5012–8290 | 74.3 | 8.0 | -5.44 | -7.0 |
| `sounds/space-02.wav` | Space press | `kenney_ui-audio.zip` → `Audio/switch15.ogg` | all | 3595–6565 | 67.3 | 8.0 | -5.44 | -6.0 |
| `sounds/space-03.wav` | Space press | `kenney_ui-audio.zip` → `Audio/switch17.ogg` | all | 4528–6794 | 51.4 | 8.0 | -5.44 | -6.0 |
| `sounds/space-04.wav` | Space press | `kenney_ui-audio.zip` → `Audio/switch30.ogg` | all | 7444–10942 | 79.3 | 8.0 | -5.44 | -5.6 |
| `sounds/enter-01.wav` | Enter press | `kenney_interface-sounds.zip` → `Audio/toggle_001.ogg` | all | -20–4402 | 100.3 | 8.0 | -19.20 | -20.1 |
| `sounds/enter-02.wav` | Enter press | `kenney_interface-sounds.zip` → `Audio/toggle_002.ogg` | all | -20–4314 | 98.3 | 8.0 | -19.20 | -20.2 |
| `sounds/enter-release-01.wav` | Enter key-up | `kenney_ui-audio.zip` → `Audio/switch33.ogg` | off | 14099–17597 | 79.3 | 8.0 | -8.22 | -14.5 |
| `sounds/enter-release-02.wav` | Enter key-up | `kenney_ui-audio.zip` → `Audio/switch34.ogg` | off | 11857–16851 | 113.2 | 8.0 | -8.22 | -19.0 |
| `sounds/enter-release-03.wav` | Enter key-up | `kenney_ui-audio.zip` → `Audio/switch38.ogg` | off | 8718–12260 | 80.3 | 8.0 | -8.22 | -14.5 |
| `sounds/backspace-01.wav` | Backspace press | `kenney_interface-sounds.zip` → `Audio/click_001.ogg` | all | -22–1012 | 23.4 | 7.8 | -10.38 | -12.1 |
| `sounds/backspace-02.wav` | Backspace press | `kenney_interface-sounds.zip` → `Audio/click_003.ogg` | all | -21–314 | 7.6 | 2.5 | -10.38 | -11.9 |
| `sounds/backspace-release-01.wav` | Backspace key-up | `kenney_ui-audio.zip` → `Audio/switch33.ogg` | off | 14099–17597 | 79.3 | 8.0 | -10.38 | -16.7 |
| `sounds/backspace-release-02.wav` | Backspace key-up | `kenney_ui-audio.zip` → `Audio/switch34.ogg` | off | 11857–16851 | 113.2 | 8.0 | -10.38 | -21.2 |
| `sounds/backspace-release-03.wav` | Backspace key-up | `kenney_ui-audio.zip` → `Audio/switch38.ogg` | off | 8718–12260 | 80.3 | 8.0 | -10.38 | -16.7 |
| `sounds/modifier-01.wav` | modifier press | `kenney_ui-audio.zip` → `Audio/switch13.ogg` | all | 148–1090 | 21.4 | 7.1 | -13.62 | -15.9 |
| `sounds/modifier-02.wav` | modifier press | `kenney_ui-audio.zip` → `Audio/switch14.ogg` | all | 170–1536 | 31.0 | 8.0 | -13.62 | -14.6 |
| `sounds/modifier-03.wav` | modifier press | `kenney_ui-audio.zip` → `Audio/switch29.ogg` | all | 4911–6473 | 35.4 | 8.0 | -13.62 | -15.0 |
| `sounds/modifier-release-01.wav` | modifier key-up | `kenney_ui-audio.zip` → `Audio/switch33.ogg` | off | 14099–17597 | 79.3 | 8.0 | -13.62 | -19.9 |
| `sounds/modifier-release-02.wav` | modifier key-up | `kenney_ui-audio.zip` → `Audio/switch34.ogg` | off | 11924–17238 | 120.5 | 8.0 | -13.62 | -24.4 |
| `sounds/modifier-release-03.wav` | modifier key-up | `kenney_ui-audio.zip` → `Audio/switch38.ogg` | off | 8718–12260 | 80.3 | 8.0 | -13.62 | -19.9 |
| `preview.wav` | mix of the files above | — | — | — | 1762 | — | — | -4.1 |

## 5. Mapping

| Group | Press | Release |
|---|---|---|
| `alphanumeric` | `click-01…13` | `release-01…03` |
| `space` | `space-01…04` | none in the group: falls back to `alphanumeric` (`release-01…03`) |
| `enter` | `enter-01…02` | `enter-release-01…03` |
| `backspace` | `backspace-01…02` | `backspace-release-01…03` |
| `modifiers` | `modifier-01…03` | `modifier-release-01…03` |

Every other key (`other`: arrows, function keys, navigation, numpad and so on) falls back to
`alphanumeric`. Runtime variation is pitch ±0.02, volume ±0.08.

## 6. Result

| Group | Count | Mean length (ms) | Peak dBFS, median (min..max) | RMS dBFS (mean) | Max leading silence (ms) |
|---|---|---|---|---|---|
| alphanumeric.press | 13 | 66.4 | -6.0 (-9.7..-2.2) | -25.8 | 0.27 |
| alphanumeric.release | 3 | 91.0 | -10.5 (-15.0..-10.5) | -31.9 | 0.32 |
| space.press | 4 | 68.1 | -6.0 (-7.0..-5.6) | -25.1 | 0.23 |
| enter.press | 2 | 99.3 | -20.1 (-20.2..-20.1) | -30.9 | 0.50 |
| enter.release | 3 | 91.0 | -14.5 (-19.0..-14.5) | -35.9 | 0.41 |
| backspace.press | 2 | 15.5 | -12.0 (-12.1..-11.9) | -28.0 | 0.50 |
| backspace.release | 3 | 91.0 | -16.7 (-21.2..-16.7) | -38.0 | 0.43 |
| modifiers.press | 3 | 29.3 | -15.0 (-15.9..-14.6) | -33.3 | 0.39 |
| modifiers.release | 3 | 93.4 | -19.9 (-24.4..-19.9) | -41.3 | 0.50 |

Leading silence is the time before the first sample at or above -50 dBFS, the loader's
trim threshold; 0.50 ms is the 22-sample pre-roll. Every file ends on an exact zero.
Total size on disk: about 0.4 MB.

## 7. Shipped files

| File | Bytes | SHA-256 |
|---|---|---|
| `sounds/backspace-01.wav` | 2112 | `f2343dbdbf047f58e984525bd1b833bc19668626212a5da01cd1cb3019c9dc98` |
| `sounds/backspace-02.wav` | 714 | `52e5a9958d0f320290a7ebe13bff7f67a4d676dcc97fec07bc923acbceeb66d6` |
| `sounds/backspace-release-01.wav` | 7040 | `651204f9c5c7b298e535cd337cdb0c50d577bea8995eca9fbe3600dadbaec0c2` |
| `sounds/backspace-release-02.wav` | 10032 | `86e633b98992f68ad2a4a1a51e8cb5b3a9059c9a2e307dab3a197b0583a82c7b` |
| `sounds/backspace-release-03.wav` | 7128 | `ba1ba5b4083e4b6953cb323ccb9f1a656261b41da2d6bab0e9f769b41d5cfd0e` |
| `sounds/click-01.wav` | 8096 | `d39e6c91d4ef7f15ba0e343ca7a7c11bc3759b703ece6b74330a326349092974` |
| `sounds/click-02.wav` | 8272 | `bc1ec137db4c56139975987b11a26aed1b033abfee90b121f1ae5999864036da` |
| `sounds/click-03.wav` | 7568 | `51246a1a25ad42d99905bc34a5afbeb57b4f9529119a9872803cc3ac6acaec9c` |
| `sounds/click-04.wav` | 4168 | `9c777df28551a997d678f688d34bf73670d9191385392450a72eba4f0338c619` |
| `sounds/click-05.wav` | 5896 | `70d24916232432cee3bf3091a4dafa4d3934a59eaad9efde3a414884bcd2eaed` |
| `sounds/click-06.wav` | 5192 | `c9616a0d5b33a4d4ea758958ee9ddbfcedd307f5d993cc2512e64e6a7260e43c` |
| `sounds/click-07.wav` | 4664 | `b3c544222c5456b18b4d27946b7efa92f99bce8fe2257299fb9d58ffd24a6abf` |
| `sounds/click-08.wav` | 7392 | `cc5d9404de100cac3efecec9ba97ca53e9e4b407005ee7e211be5f3338ad5a41` |
| `sounds/click-09.wav` | 2936 | `912730418fa5dd574eb1a24c6959bfbf2d7af3b1b8da5c46972567c7cbd04e18` |
| `sounds/click-10.wav` | 2596 | `7bcac44552cc324930c34d4959569db1192574b7573ac8ca586d2feb9a097a11` |
| `sounds/click-11.wav` | 5192 | `32508aba6e75d47b24376d4f717862b6f0ad76c8f0fb406fa80ad6798db63f99` |
| `sounds/click-12.wav` | 6688 | `d8347106080184ddf8a44444fca75b5ea06750a0d35a676a25c87edbb1ef7efa` |
| `sounds/click-13.wav` | 8096 | `41ff2ac9c5284d83914451614e6b669220f1a98e926556e02c4969a6a3abbaed` |
| `sounds/enter-01.wav` | 8888 | `29072065991aa156313185a2d45d89983b536f37a365b53a4849582cd6942019` |
| `sounds/enter-02.wav` | 8712 | `00c81f593c7ecaf9a9b6b4dc4a0dab3f37bf700101fa74c8fd0c4671dce3cdbf` |
| `sounds/enter-release-01.wav` | 7040 | `b6faaa3a6f1c85883b9e97741c75f3ee3a6b5bf79bdbd942e5f1b38ef24f8aad` |
| `sounds/enter-release-02.wav` | 10032 | `3c9e449eb3c1f253cbc4c46fe4e2f49fa016401b5fa276b9a019eb595094294f` |
| `sounds/enter-release-03.wav` | 7128 | `6bfc1fa54501effa8140f6aefeb1a1b4d7965687507ee930a61f62aa30222760` |
| `sounds/modifier-01.wav` | 1928 | `20a9eef237c03dcc0254c2e09e0b83e7e402fe630ec5d0cbabdd1027587cce18` |
| `sounds/modifier-02.wav` | 2776 | `651c605ca51898cfad7bc9a0f670b2029e845928b4e19b146056b5f179af2817` |
| `sounds/modifier-03.wav` | 3168 | `a8bfb0c976050556acadd0055eb854c53626212fdc454fb47c226c1a26391721` |
| `sounds/modifier-release-01.wav` | 7040 | `ccda426ab6b0792826cdd6b8c21e8153a26690d71371090ed724f8c584690f75` |
| `sounds/modifier-release-02.wav` | 10672 | `155b134254e843c4aaade395d7acd509a3782df44ce4a3d11b5844c74c30ea52` |
| `sounds/modifier-release-03.wav` | 7128 | `93787e616a18bf03436ee288bf788435351d79c1c2fe086d27bd4ff3a139869e` |
| `sounds/release-01.wav` | 7040 | `3680accc0ccc1cf465f75647e3bd8f8ba856a5e0f6744bfd1a8abf11f73d52a5` |
| `sounds/release-02.wav` | 10032 | `822f51e122467181d870252c0c3d5cc0d1d107961a30c43666e6cf43fb38613c` |
| `sounds/release-03.wav` | 7128 | `b152de2f8533ab1c04a06b9d819cb5999f668aeed8b6af515d27631f14ca1ba4` |
| `sounds/space-01.wav` | 6600 | `197e55d2170900d730e3077f5436663e6e31b8d11c529178a0fb730efde31c45` |
| `sounds/space-02.wav` | 5984 | `1bbccf42becf4bc1ac0333c8df3659af342edba48a75d6600dcccc75713401e8` |
| `sounds/space-03.wav` | 4576 | `d7071d2460e0403cb50b4920f0b957fcf239d52a32fcfddb5d433e21b997781e` |
| `sounds/space-04.wav` | 7040 | `c89614b45f905be8ec381377b3aa28fb754712e36d971d5dad96f02128c9bc3b` |
| `preview.wav` | 155468 | `515a26c6bc67427d70921a1c6380a84a7fd0be5afe505c580750d20ade1326ee` |
