# Sources: Spring Lite (`spring-lite`)

Every sound in this pack is cut from one recording on Wikimedia Commons that its author dedicated to the public domain under CC0 1.0. TakTak's cuts, `pack.json` and `preview.wav` are also released under CC0-1.0. Crediting the author is not required but is done in `attribution`.

## Original recording

| | |
|---|---|
| Title | IBM M2 sound.ogg |
| Source page | https://commons.wikimedia.org/wiki/File:IBM_M2_sound.ogg |
| Direct download | https://upload.wikimedia.org/wikipedia/commons/1/16/IBM_M2_sound.ogg |
| Author | Anonimski ("Own work" per the source page) |
| Uploaded | 2014-05-17 |
| Commons description | Sound sample produced by typing on the IBM M2 (Model M) mechanical keyboard. Typed text: "www.wikipedia.org[Enter]" |
| License | CC0 1.0 Universal Public Domain Dedication (SPDX `CC0-1.0`), https://creativecommons.org/publicdomain/zero/1.0/ |
| License proof | The source page's license section, and the Commons API record (extmetadata `LicenseShortName` "CC0", `UsageTerms` "Creative Commons Zero, Public Domain Dedication", `AttributionRequired` "false", `Credit` "Own work", artist Anonimski): https://commons.wikimedia.org/w/api.php?action=query&format=json&prop=imageinfo&iiprop=url%7Csize%7Csha1%7Cextmetadata&titles=File:IBM_M2_sound.ogg |
| Date checked | 2026-10-02 |
| Format | Ogg Vorbis, stereo, 44.1 kHz, 63414 bytes |
| SHA-1 (reported by Commons) | `3c64c15dd174840d8e4e8b2d5f02a35c6680ec63` |
| SHA-256 of the download | `e620684e9b03b62319fb48da6149392cfd4042615e17a891554f8a9f6ac8515e` |

## How the files were made

Rebuild: `tools/pack-sources/spring-lite/fetch.sh DIR`, then `tools/pack-sources/spring-lite/build.sh DIR`, where DIR holds `IBM_M2_sound.ogg`. The script is `tools/pack-sources/spring-lite/springslice.py`; the exact cut list is `tools/pack-sources/spring-lite/selection.json`. It needs python3 with numpy, and ffmpeg.

1. Decode with ffmpeg; downmix to mono as (L+R)/2; 2-pole high-pass at 30 Hz (removes DC). The source rate, 44.1 kHz, is kept.
2. Find stroke onsets (1 ms energy frames of a 1 kHz high-passed copy, used for detection only).
3. Start each file 0.5 ms (22 samples) before the stroke's attack, with a raised-cosine fade-in over those 22 samples.
4. End it where the stroke decays to 3 dB above the recording's noise floor, or 1 ms before the next sound starts, whichever is first (at most 400 ms; 1.5 s for Enter), with a raised-cosine fade-out of up to 8 ms to an exact zero.
5. Apply one gain to every file: **+0.09 dB**, so that the median press peaks at -6 dBFS. No per-file normalization and no other processing (no EQ, no denoising, no compression).
6. Write 16-bit PCM mono WAV at 44.1 kHz.
7. Loudness: `volume` in `pack.json` is **0.74** (-2.6 dB), a playback gain the app applies to the whole pack, so no audio file changes. It matches typing loudness across the bundled packs, so switching packs does not jump in volume; the reference is the synthesized packs' -25.8 LK. Measured with `cargo run -p synth-packs --release -- loudness packs/spring-lite` (`tools/synth-packs/src/loudness.rs`): the K-weighted (ITU-R BS.1770) energy of the first 100 ms of the sample each alphanumeric key plays on press by default, power-averaged over the keys, on the samples as the app loads them at 48 kHz. The pack reads -23.3 LK as built and -25.9 LK at this volume. The build writes the volume (`VOLUME`), so a rebuild keeps it.

Only clean, isolated strokes are kept: peak at least 20 dB over the 10 ms before it (15 dB for releases), decayed or at least 45 ms long when cut, and within -9..+5 dB of the median stroke of its kind; the Enter press and release are always kept.

**Press or release.** This take is slow, with clear pairs: each loud stroke is followed 80-140 ms later by a quieter one. Strokes were split by the peak of their first 15 ms into two level clusters (2-means; threshold -11.12 dBFS in the source): the louder ones are presses, the quieter ones releases, each release needing a key still held down. The Commons description gives the typed text as "www.wikipedia.org[Enter]", so the last press and the release after it are the `enter` group. Space and the other keys use the `alphanumeric` pool. Every release file is a recorded stroke that this split classed as a key-up; none is synthesized.

### Files

Source range: sample indices `[start, end)` in the decoded 44.1 kHz mono stream (step 1).

| File | Group / action | Source range | Start (s) | Length (ms) | Source peak (dBFS) | SHA-256 as shipped |
|---|---|---|---|---|---|---|
| `sounds/enter-press.wav` | enter press | 166686-172032 | 3.780 | 121.2 | -6.2 | `9eb6e8d311dc6f175f884675727b095cf5d6b663acafe3f493e16c8c7eed9f8c` |
| `sounds/enter-release.wav` | enter release | 172257-175887 | 3.906 | 82.3 | -18.4 | `c0e7782cda65565aaea241b388841939c07e8b07c377397468f2d93c64788e55` |
| `sounds/press-01.wav` | alphanumeric press | 5560-8794 | 0.126 | 73.3 | -5.7 | `c2f159d6c7e343375a95b6ac03606c042f08882e43bbec936fc6f99ef8396660` |
| `sounds/press-02.wav` | alphanumeric press | 13757-18311 | 0.312 | 103.3 | -7.7 | `6924e14e3f357531135cf4de84fed4d2bd60f5518525a5e735884054f973e280` |
| `sounds/press-03.wav` | alphanumeric press | 21967-27401 | 0.498 | 123.2 | -6.2 | `eefbd2a1f1bbd53ea531c1b6e8039ed0d4924f5dae656c67e4a6c0700071d780` |
| `sounds/press-04.wav` | alphanumeric press | 34978-39092 | 0.793 | 93.3 | -6.7 | `d098f1d38f58c9cf834eb5da2bde4c5943b5bce49267cc07a1a491949179905e` |
| `sounds/press-05.wav` | alphanumeric press | 49221-53335 | 1.116 | 93.3 | -5.5 | `fec8a1ba01a14db0b18cf8d425016a9d85103c31cff090b152b6ae1414d78093` |
| `sounds/press-06.wav` | alphanumeric press | 53884-58086 | 1.222 | 95.3 | -2.2 | `565cad6548f335a81ae7314524e8abe1b6d63da08b3eba537cc2c9c1b92dbbb4` |
| `sounds/press-07.wav` | alphanumeric press | 63214-66624 | 1.433 | 77.3 | -6.1 | `87f063fa72cd249637d19650c2af6f0062208a0fef9bd46d830cc798d70076e0` |
| `sounds/press-08.wav` | alphanumeric press | 71934-76136 | 1.631 | 95.3 | -5.1 | `d0e77172305af2c23041e95fa3e2d9b23f762e0739abb7a399a2c9a832a4e3d2` |
| `sounds/press-09.wav` | alphanumeric press | 84333-88711 | 1.912 | 99.3 | -4.3 | `182ef1200085b1665b7dfb1a9ad96d9ec87d73fdead9a7b88a7f3fef98c4c46f` |
| `sounds/press-10.wav` | alphanumeric press | 89361-91627 | 2.026 | 51.4 | -7.5 | `e89b672cf4b282bbd0fb4d405c9c32de662e73ca60277252265a01f0e55569f0` |
| `sounds/press-11.wav` | alphanumeric press | 91851-95613 | 2.083 | 85.3 | -8.2 | `8cfcf7cf6c4e799e0f105a8f0069b2934e40da54cef70fb5a07dc3bfc6bea78b` |
| `sounds/press-12.wav` | alphanumeric press | 110272-114914 | 2.500 | 105.3 | -3.7 | `d1427372e317a68a216fd94e887175b343538b0cc74e6950fc7fcd5f6f3479a2` |
| `sounds/press-13.wav` | alphanumeric press | 137769-142323 | 3.124 | 103.3 | -6.8 | `33330a2b8306486cf21f977c617c71bd572051c8cae8f84f4e3f81ff632c2b4a` |
| `sounds/press-14.wav` | alphanumeric press | 147217-154044 | 3.338 | 154.8 | -6.8 | `3a917cee7883b47bf71bf5528a373040df239a47c659cb2ad49e2612365884a2` |
| `sounds/press-15.wav` | alphanumeric press | 154351-160049 | 3.500 | 129.2 | -2.8 | `ccc34919d9cb463627c086161a6188397ce79183f6d89b50dc732e229df96f79` |
| `sounds/release-01.wav` | alphanumeric release | 9414-13352 | 0.213 | 89.3 | -14.0 | `e15e724574065cbe726761073af9db8bb8889d91d73dec5d92f0a01028ea1cea` |
| `sounds/release-02.wav` | alphanumeric release | 40069-43699 | 0.909 | 82.3 | -18.4 | `ced23ef740a53bb30ccc06ae66afaf48e1d6cfc8ad8306dd9761f45e2b78d8a2` |
| `sounds/release-03.wav` | alphanumeric release | 58380-61526 | 1.324 | 71.3 | -13.3 | `a8b99827ddbae12869eb6fb4752543d5cff85e8a903e117604a57c177c9b06ee` |
| `sounds/release-04.wav` | alphanumeric release | 76358-81924 | 1.731 | 126.2 | -14.9 | `66daad14adace7b40946158d6cb0cd21babda7ecd8810988bcec67a1566f792c` |
| `sounds/release-05.wav` | alphanumeric release | 95956-100510 | 2.176 | 103.3 | -14.1 | `9462c6ced8422aa46d0b3e3da8b7290e372a1a6f9e4836b3a4a7afc279d92ab4` |
| `sounds/release-06.wav` | alphanumeric release | 101906-106020 | 2.311 | 93.3 | -14.2 | `692e6c81809bcd9ef3aa17a80ad639c18f842db1f063b326fb080aadcf833eec` |
| `sounds/release-07.wav` | alphanumeric release | 106312-109898 | 2.411 | 81.3 | -13.1 | `87d5a47fa15626b3bd4488ced294aae36a8e9b7dfe67b8c45f660d1dfb747174` |
| `sounds/release-08.wav` | alphanumeric release | 121495-125257 | 2.755 | 85.3 | -12.2 | `073c406b6147ab6e9470a22cd6413983c83ea8ada7ac2e647a997ad5a46558a6` |
| `sounds/release-09.wav` | alphanumeric release | 142546-146924 | 3.232 | 99.3 | -16.1 | `28e54c14d3fab0c551de07a1943914c89bef3eca72f9c0837d6b312858ea2f65` |
| `sounds/release-10.wav` | alphanumeric release | 160287-165721 | 3.635 | 123.2 | -15.4 | `b24482743da6acd1e1bfbcdc061ab0e0fbfc69ea05371ea20aee917bb3861ac8` |

`preview.wav` (SHA-256 `48dc631f66924dcf6d922ded90e38352e79942f22628b71189dc20673c3227a7`, 1746.4 ms) types "the quick fox" and Enter with the files above: a press and a release per key, 60-120 ms dwell, 70-170 ms from one key-down to the next (50 ms more after Space), with a fixed random seed. It is mixed by addition; if the mix peaked above -1 dBFS it would be scaled down to -1 dBFS.

## Stats

| group / action | count | mean ms | median peak dBFS | max peak dBFS | mean RMS dBFS | max leading silence ms |
|---|---|---|---|---|---|---|
| alphanumeric press | 15 | 99 | -6.0 | -2.1 | -24.7 | 0.25 |
| alphanumeric release | 10 | 95 | -14.1 | -12.1 | -30.4 | 0.20 |
| enter press | 1 | 121 | -6.1 | -6.1 | -23.6 | 0.18 |
| enter release | 1 | 82 | -18.3 | -18.3 | -34.4 | 0.14 |
