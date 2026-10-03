# Linear Red: sources and provenance

Pack id `linear-red`, license **CC-BY-4.0**. Everything here was checked on **2026-10-02**.
`LICENSE.txt` has the credit, a link to the license, and the list of changes made.

Credit line (also in `pack.json` → `attribution`):

> Typing on Keychron V1 Ultra (Red Linear Switch) by C40115, CC BY 4.0, via Wikimedia Commons

## 1. Original work

| | |
|---|---|
| Title | Typing on Keychron V1 Ultra (Red Linear Switch).wav |
| Source page | https://commons.wikimedia.org/wiki/File:Typing_on_Keychron_V1_Ultra_(Red_Linear_Switch).wav |
| Direct download | https://upload.wikimedia.org/wikipedia/commons/2/27/Typing_on_Keychron_V1_Ultra_(Red_Linear_Switch).wav (3,406,832 bytes) |
| Author | C40115 ("Own work" per the source page) |
| Date on Commons | 2026-06-05 |
| Commons description | The uploader's own typing during a Monkeytype test on a Keychron V1 Ultra with red linear switches, recorded with the built-in microphone of a 2025 MacBook Air with minimal processing |
| License | Creative Commons Attribution 4.0 International (SPDX `CC-BY-4.0`), https://creativecommons.org/licenses/by/4.0/ |
| License proof | The license section of the source page, and its Commons API record (license "CC BY 4.0", license URL https://creativecommons.org/licenses/by/4.0, artist C40115, credit "Own work"): https://commons.wikimedia.org/w/api.php?action=query&format=json&prop=imageinfo&iiprop=extmetadata%7Csize%7Cmime%7Csha1&titles=File:Typing_on_Keychron_V1_Ultra_(Red_Linear_Switch).wav |
| Format | WAV, PCM 16-bit, 48 kHz, 2 channels (both channels are sample-for-sample identical), 17.72 s |
| SHA-1 (reported by Commons) | `778ae6ee58b79fe05050f910607b970f788ddf4e` (matches the download) |
| SHA-256 of the download | `85947c590e3831cf835609c11ceddce64dd5acd34eda08d24714c0bcc054ae4d` |
| Checked | 2026-10-02: license and author read from the Commons record, file downloaded and hashed |

The keyboard brand appears only in this description and in the credit line the license
requires; it is not part of the pack's name or id.

## 2. Processing

Rebuild with `tools/pack-sources/linear-red/fetch.sh DOWNLOADS`, then
`python3 tools/pack-sources/linear-red/build.py --downloads DOWNLOADS --out packs/linear-red`.
The build needs ffmpeg and numpy; it was run with ffmpeg 9.0.2, numpy 2.4.2 and Python 3.14,
and a rebuild reproduces every shipped file byte for byte (checked 2026-10-02). The cut list
is frozen in `tools/pack-sources/linear-red/selection.json`; `build.py --reselect` recomputes
it with the rules in §3.

1. **Decode.** ffmpeg, mono as (L+R)/2 (`pan=mono|c0=0.5*c0+0.5*c1`; the channels are
   identical, so this is the left channel), then a 2-pole 30 Hz high-pass
   (`highpass=f=30:poles=2`) that removes DC and sub-audio rumble. 48 kHz is kept.
2. **Slice.** Each pack file is the sample range listed in §4 of the decoded signal.
   - It starts 24 samples (0.5 ms) before the stroke's attack, with a raised-cosine
     fade-in over those 24 samples.
   - It ends with a raised-cosine fade-out to exactly zero: 8 ms where the stroke had
     decayed into the room noise, 6 ms where it was cut 1 ms before the next sound (shorter
     for the shortest release, at most a third of its length).
3. **Gain.** One gain for the whole pack, **+7.51 dB**, makes the median letter press peak
   at -6 dBFS (it was -13.51 dBFS). The space-bar files get a further **-5.35 dB**, shared by
   all of them (so +2.16 dB in total), which puts the loudest space press at -1 dBFS.
   No file is normalized on its own, so the natural differences between strokes stay.
4. **Encode.** 16-bit PCM WAV, 48 kHz, mono, rounded with no dither.
5. **Preview.** `preview.wav` (1.66 s) mixes the pack's own files as a typed phrase: 4
   letters, Space, 3 letters, Enter (fixed random seed, 60-100 ms dwell, 70-150 ms between
   keys). Letters have no key-up sound; Space and Enter play a space-bar release.
6. **Loudness.** `volume` in `pack.json` is **0.94** (-0.5 dB): a playback gain the app
   applies to the whole pack, so no audio file changes. Typing loudness is matched across the
   bundled packs so that switching packs does not jump in volume; the reference is the
   synthesized packs' -25.8 LK. Measured with
   `cargo run -p synth-packs --release -- loudness packs/linear-red`
   (`tools/synth-packs/src/loudness.rs`): the K-weighted (ITU-R BS.1770) energy of the first
   100 ms of the sample each alphanumeric key plays on press by default, power-averaged over
   the keys, on the samples as the app loads them at 48 kHz. The pack reads -26.6 LK as built, so the
   reference would take a volume of 1.10. Headroom caps it at 0.94: the loudest file
   (`sounds/space-press-05.wav`, true peak -1.0 dBFS) must stay at or below -1 dBFS at the
   top of the ±6 % volume variation, because the mixer clips hard at 0 dBFS. Typing therefore
   plays at -27.1 LK, **1.3 dB below the reference**; nothing was limited or re-normalized to
   close the gap. `build.py` writes the volume (`VOLUME`), so a rebuild keeps it.

No other processing: no EQ, denoising, compression or time/pitch change. The recording's
low room noise (about -50 dBFS before the gain) is part of every file.

## 3. How the strokes were chosen

The take is fast, continuous typing: about 300 strokes in 16 seconds, often 20-80 ms apart.
Only strokes that stand alone were kept. Detection used a 1 kHz FFT high-passed copy of the
decoded signal (analysis only; it is not what ships).

- **Attack**: the first sample within 26 dB of the stroke's high-frequency peak, walking back
  from that peak. For one space press (`space-press-05`) the file starts at the thumb's
  touch on the bar, 10 ms before it bottoms out, because that touch is part of the stroke.
- **Clean**: at least 20 dB of quiet (high-frequency peak) in the 10 ms before the attack; the
  loudest hit within 12 ms of the attack; at least 40 ms before the next sound (15 ms for a
  space release) and at least 20 dB down when cut; no later hit within -8 dB of the peak
  (-4 dB for a space release); peak -24 dBFS or louder.
- **Space bar**: its strokes are clearly darker than the letters (energy at 150-800 Hz
  against 2.2-12 kHz in the first 20 ms) and come in pairs: a loud key-down, then a quieter
  key-up 40-120 ms later. 14 such pairs were found. Their presses are +4.7..+13.7 dB dark;
  13 of the other 191 strokes at -24 dBFS or louder are +3 dB dark or more and were left out
  of every group as unclear (they may be Backspace or other wide keys).
- **Letters, key-down or key-up**: in this take both are about equally loud and they overlap,
  and neither level nor timbre told them apart. One timing cue helps: a stroke that follows
  a much quieter tick (8 dB or more down) by 15-70 ms. The tick is a finger meeting a keycap,
  or the previous key coming up just before this one goes down. On the 14 space-bar pairs, 9
  presses and 4 releases show it, so the cue picks key-downs about twice as often as key-ups,
  but not always. Only clean letter strokes with the cue were kept (13 of 178), and they are
  used as press variants. **Some of them may be key-ups.** No letter release is shipped, so
  letter key-up is silent.
- Every kept file was also checked by eye on a waveform plot.

## 4. Per-file derivation

Sample ranges index the decoded 48 kHz signal of step 1, `end` exclusive. "Attack" is the
time in the original file. Every file also gets the 0.5 ms fade-in and 16-bit encoding.

| Pack file | Used as | Source samples @48 kHz | Attack (s) | Length (ms) | Fade-out (ms) | Source peak (dBFS) | Gain (dB) |
|---|---|---|---|---|---|---|---|
| `sounds/press-01.wav` | letter press | 75039–77208 | 1.564 | 45.2 | 6.0 | -16.1 | +7.51 |
| `sounds/press-02.wav` | letter press | 143946–147504 | 2.999 | 74.1 | 6.0 | -10.2 | +7.51 |
| `sounds/press-03.wav` | letter press | 158728–162180 | 3.307 | 71.9 | 6.0 | -15.7 | +7.51 |
| `sounds/press-04.wav` | letter press | 166041–168828 | 3.460 | 58.1 | 6.0 | -15.2 | +7.51 |
| `sounds/press-05.wav` | letter press | 179466–182976 | 3.739 | 73.1 | 6.0 | -14.4 | +7.51 |
| `sounds/press-06.wav` | letter press | 243008–245912 | 5.063 | 60.5 | 8.0 | -10.4 | +7.51 |
| `sounds/press-07.wav` | letter press | 274947–277707 | 5.729 | 57.5 | 8.0 | -11.2 | +7.51 |
| `sounds/press-08.wav` | letter press | 307748–310632 | 6.412 | 60.1 | 6.0 | -13.5 | +7.51 |
| `sounds/press-09.wav` | letter press | 365669–368640 | 7.619 | 61.9 | 6.0 | -13.2 | +7.51 |
| `sounds/press-10.wav` | letter press | 380661–383757 | 7.931 | 64.5 | 8.0 | -11.1 | +7.51 |
| `sounds/press-11.wav` | letter press | 426552–428640 | 8.887 | 43.5 | 8.0 | -16.3 | +7.51 |
| `sounds/press-12.wav` | letter press | 718466–722208 | 14.969 | 78.0 | 6.0 | -10.8 | +7.51 |
| `sounds/press-13.wav` | letter press | 738768–741720 | 15.392 | 61.5 | 8.0 | -16.3 | +7.51 |
| `sounds/space-press-01.wav` | space key-down | 136478–138636 | 2.844 | 45.0 | 6.0 | -6.6 | +2.16 |
| `sounds/space-press-02.wav` | space key-down | 211907–213972 | 4.415 | 43.0 | 6.0 | -7.2 | +2.16 |
| `sounds/space-press-03.wav` | space key-down | 252172–255732 | 5.254 | 74.2 | 6.0 | -13.2 | +2.16 |
| `sounds/space-press-04.wav` | space key-down | 517147–520483 | 10.774 | 69.5 | 8.0 | -11.8 | +2.16 |
| `sounds/space-press-05.wav` | space key-down (thumb touch included) | 528205–531528 | 11.005 | 69.2 | 6.0 | -3.2 | +2.16 |
| `sounds/space-press-06.wav` | space key-down | 577891–580608 | 12.040 | 56.6 | 6.0 | -4.1 | +2.16 |
| `sounds/space-press-07.wav` | space key-down | 638123–640344 | 13.295 | 46.3 | 6.0 | -10.1 | +2.16 |
| `sounds/space-release-01.wav` | space key-up (of the 2.844 s press) | 140099–141780 | 2.919 | 35.0 | 6.0 | -14.1 | +2.16 |
| `sounds/space-release-02.wav` | space key-up (of the 4.415 s press) | 215328–216156 | 4.487 | 17.2 | 5.8 | -8.6 | +2.16 |
| `sounds/space-release-03.wav` | space key-up (of a 9.050 s press) | 438114–439428 | 9.128 | 27.4 | 6.0 | -10.7 | +2.16 |
| `sounds/space-release-04.wav` | space key-up (of an 11.563 s press) | 557588–558468 | 11.617 | 18.3 | 6.0 | -11.7 | +2.16 |
| `preview.wav` | mix of the files above | — | — | 1656 | — | — | — |

Space releases 03 and 04 belong to presses that were not clean enough to ship.

## 5. Mapping

| Group | Press | Release |
|---|---|---|
| `alphanumeric` | `press-01…13` | none: letter key-up is silent |
| `space` | `space-press-01…07` | `space-release-01…04` |
| `enter` | `space-press-01…07` | `space-release-01…04` |

The take has no Enter key (a typing test does not use it), so Enter uses the space-bar
strokes, the other long stabilized key on this board. Backspace, the modifiers and every
other key fall back to `alphanumeric`. Runtime variation is pitch ±0.015, volume ±0.06.

## 6. Result

| Group | Count | Mean length (ms) | Peak dBFS, median (min..max) | RMS dBFS (mean) | Max leading silence (ms) |
|---|---|---|---|---|---|
| alphanumeric.press | 13 | 62.3 | -6.0 (-8.8..-2.7) | -27.1 | 0.29 |
| space/enter.press | 7 | 57.7 | -5.0 (-11.0..-1.0) | -27.5 | 0.29 |
| space/enter.release | 4 | 24.5 | -9.1 (-11.9..-6.4) | -23.8 | 0.31 |

Leading silence is the time before the first sample at or above -50 dBFS, the loader's
trim threshold. Total size on disk: about 0.3 MB.

## 7. Shipped files

| File | Bytes | SHA-256 |
|---|---|---|
| `sounds/press-01.wav` | 4382 | `aadb79c504ecda098c8262d1912225d046461d8a3a362db05852c7ec7aa86ea1` |
| `sounds/press-02.wav` | 7160 | `0a6b334c90af790add7a707c76e53ceecf269a76345def07faea17dea5cc6cc3` |
| `sounds/press-03.wav` | 6948 | `ad972acf127634d621888d3291cbfaf5fb36407e23d147eaee5251ba769c792c` |
| `sounds/press-04.wav` | 5618 | `88718cddba2f6b0517c170c29007faab4b4abfb6c8be460a608a96688d4f9963` |
| `sounds/press-05.wav` | 7064 | `49138be1a4d755d2aeba1461f6bb97787032a5340d255324d24657d077f9ded8` |
| `sounds/press-06.wav` | 5852 | `308ec97f6f8a7200b4d0cf84cc9b0e28eeeb609a751e85b0da5474c2fbcf69f2` |
| `sounds/press-07.wav` | 5564 | `880b4c302d38b5e55408ab53ce733498d786fe1d4b5f018ce58b3be987477b6e` |
| `sounds/press-08.wav` | 5812 | `7c65f0c4cb4fac5573b61335979c76d4068505f4fca492413562d1c5f80f3cdd` |
| `sounds/press-09.wav` | 5986 | `f9a75f25f2f0bea33eb121cee6cdde2352f5fe2fcbe81b821d3f6aae067a63bc` |
| `sounds/press-10.wav` | 6236 | `735c49751e5a26506fb797cf2fa99495ed18451942a1b21f7bd70288b7a9a45f` |
| `sounds/press-11.wav` | 4220 | `bebe3811af2c133f18babfbf200f4e4dd9aff28bf86e25e137b4008a9bb88182` |
| `sounds/press-12.wav` | 7528 | `b561e574fa36e87df24f13d69c83122d146e3ac411c6ec95eb6c63f1949d28ad` |
| `sounds/press-13.wav` | 5948 | `2b75ad72de2329fc8278fa7b46f8f85a29bc951fbde18d221b36fcf270e6412a` |
| `sounds/space-press-01.wav` | 4360 | `2a78ad1a8b28b9e6a449d280f77a954cc829f632b2e1ee1865be279e62f00901` |
| `sounds/space-press-02.wav` | 4174 | `f94783be042b7f4e8ba4814c8ad631f6922178037c417d7965cecbdf9553fc7f` |
| `sounds/space-press-03.wav` | 7164 | `43a2347e4151c8db8fdd500b4fe85da243b02648613be3980052fe536ea34312` |
| `sounds/space-press-04.wav` | 6716 | `70d35e3dc0474b931198bced1aea790f422645d581215275cca95fea3e89b76f` |
| `sounds/space-press-05.wav` | 6690 | `6fe3d27f553a020f61ce1db1de2f87085fabf028514af6fc41b87ef4a4affbf9` |
| `sounds/space-press-06.wav` | 5478 | `ab91e6e0bda6959d9cf37217cb39b6aefb631dbfb665c3cc867610995d75950f` |
| `sounds/space-press-07.wav` | 4486 | `749defa8a44af4e50cd66e362fc50cf818ff27ca825bcb049ad31618413fe008` |
| `sounds/space-release-01.wav` | 3406 | `71d1bb688cb4e5a9b55a65792dcd163c231f604b9e6d0b7abca73a0784155401` |
| `sounds/space-release-02.wav` | 1700 | `efd78af9b72354aef2764c86945b145189af7b5dfbaa9a3704fdeceedd156885` |
| `sounds/space-release-03.wav` | 2672 | `6cf8d0b3980b03edf117c03e896a61e6f2504c6c88bd25f7e90fb7161faa2f50` |
| `sounds/space-release-04.wav` | 1804 | `0c54533dd5ce50674628a17d0ac9a7ea6818ab00f36b33e09a3e618512384122` |
| `preview.wav` | 158936 | `e89fce6f0d20818590c76b7217d7fe90e1f79fcf420a323c15d8d8a5ae91d7cc` |
