# Credits

TakTak's own code is MIT-licensed. The sound packs it ships in [`packs/`](packs/) carry their
own licenses, listed below with the credit each one asks for. Every pack folder also has a
`SOURCES.md` with the full provenance of every file (source page, direct download, author,
license proof, SHA-256 of the original and of the file as shipped, and exactly how each file
was derived), and a `LICENSE.txt` when the license requires its notice to travel with copies.
The `attribution` field of each pack's `pack.json` holds its credit line, for display with the
pack.

All licenses below were checked on 2026-10-02. Every pack can be rebuilt from the original
downloads with the scripts in [`tools/pack-sources/`](tools/pack-sources/) (or, for the
synthesized packs, [`tools/synth-packs/`](tools/synth-packs/)).

The original authors do not endorse TakTak or its packs. Product names that appear below in
the titles and descriptions of the original recordings (IBM, Keychron, Corsair, Cherry and
others) are trademarks of their owners. They are used only to identify the recorded hardware
and are never part of a pack's name.

**Changes common to the recorded packs.** TakTak made these packs by cutting single
keystrokes out of the original recordings and processing them: mixed to mono where the source
is stereo, high-passed at 25-30 Hz to remove DC and rumble, leading silence cut (at most 0.5 ms
kept before the attack), tails faded out to silence, one level change per pack, per source or
per group of sounds, and saved as 16-bit WAV. Each pack also sets a playback `volume` so that
all packs play at about the same loudness (see "Loudness" in each `SOURCES.md`), and each
includes a short `preview.wav` mixed from its own sounds. Pack-specific changes are listed per
pack.

## Synthesized packs (TakTak's own work)

| Pack | id | License |
|---|---|---|
| Deep Thock | `deep-thock` | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) |
| Crisp Clack | `crisp-clack` | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) |
| Blue Click | `blue-click` | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) |

These three packs are procedurally synthesized by TakTak's own generator,
[`tools/synth-packs`](tools/synth-packs/), from physical models. No recordings or other
third-party audio were used at any stage. The TakTak contributors dedicate them to the public
domain under CC0 1.0; no credit is required. Provenance and the generator seeds:
[deep-thock](packs/deep-thock/SOURCES.md), [crisp-clack](packs/crisp-clack/SOURCES.md),
[blue-click](packs/blue-click/SOURCES.md).

## Recorded packs

### Buckling Spring (`buckling-spring`)

- **License:** MIT. Copyright (c) 2016 - Ico Doornekamp. The full license text is in
  [`packs/buckling-spring/LICENSE.txt`](packs/buckling-spring/LICENSE.txt), which ships with
  the pack.
- **Credit:** Buckling-spring keyboard samples by Ico Doornekamp (bucklespring), MIT License.
- **Original work:** the `wav/` samples of [bucklespring](https://github.com/zevv/bucklespring)
  by Ico Doornekamp, recorded on his IBM Model M space saver, at commit
  [`63b3d5b`](https://github.com/zevv/bucklespring/tree/63b3d5b8ea84b426b466146ced5cbf85fe3bc0cb/wav),
  the last commit whose license is MIT
  ([license at that commit](https://github.com/zevv/bucklespring/blob/63b3d5b8ea84b426b466146ced5cbf85fe3bc0cb/LICENSE)).
  Later bucklespring commits are GPL-2.0 and are not used.
- **Changes:** each key's press and release sample trimmed (no leading silence, tail faded
  out), high-passed at 25 Hz, one gain for the whole pack, mono 16-bit at the original 44.1 kHz
  (bucklespring's stereo panning is not reproduced); played at a pack volume of 0.96.
- **Provenance:** [`packs/buckling-spring/SOURCES.md`](packs/buckling-spring/SOURCES.md).

### Key Press (`key-press`)

- **License:** [CC BY 3.0](https://creativecommons.org/licenses/by/3.0/) (SPDX `CC-BY-3.0`).
  The credits, the list of changes and the full legal code are in
  [`packs/key-press/LICENSE.txt`](packs/key-press/LICENSE.txt), which ships with the pack.
- **Credit:** "Single Key Press Sounds" by eklee, qubodup
  (https://opengameart.org/content/single-key-press-sounds) and "Mechanical keyboard sound" by
  bluszcz (https://opengameart.org/content/mechanical-keyboard-sound), both CC BY 3.0
  (https://creativecommons.org/licenses/by/3.0/). Sliced, mixed to mono and level-adjusted by
  TakTak contributors.
- **Original works:**
  - ["Single Key Press Sounds"](https://opengameart.org/content/single-key-press-sounds),
    recordings by eklee, single sounds extracted by qubodup (OpenGameArt, 2012). Notice given
    by the work: "Copyright 2012 eklee | Copyright 2012 qubodup extraction of single sounds |
    License: CC Attribution 3.0".
  - ["Mechanical keyboard sound"](https://opengameart.org/content/mechanical-keyboard-sound)
    by bluszcz (OpenGameArt, 2016).
- **Changes (TakTak contributors, 2026-10-02):** this pack is an adaptation of both works.
  From "Single Key Press Sounds", 11 of the 14 takes were mixed to mono and high-passed at
  30 Hz. Seven of them (Mac01, Mac02, Mac04-Mac07 and Old01) were split at the key-up into a
  press and a release sound. The other four (Mac03, Old02, Old03, Old04) have only a faint
  release, so they were kept whole as press sounds. From "Mechanical keyboard sound", six
  single keystrokes were cut out of the two typing clips, high-passed and resampled from 44.1
  to 48 kHz. All were trimmed and faded, given one gain of -5.92 dB and saved as 16-bit, 48 kHz
  mono WAV. The pack plays at a volume of 1.14 (+1.1 dB). `preview.wav` is a typing phrase
  mixed from these sounds.
- **Provenance:** [`packs/key-press/SOURCES.md`](packs/key-press/SOURCES.md).

### Linear Red (`linear-red`)

- **License:** [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) (SPDX `CC-BY-4.0`,
  [legal code](https://creativecommons.org/licenses/by/4.0/legalcode)). The credit, the license
  link and the list of changes are in
  [`packs/linear-red/LICENSE.txt`](packs/linear-red/LICENSE.txt), which ships with the pack.
- **Credit:** Typing on Keychron V1 Ultra (Red Linear Switch) by C40115, CC BY 4.0, via
  Wikimedia Commons.
- **Original work:** ["Typing on Keychron V1 Ultra (Red Linear Switch).wav"](https://commons.wikimedia.org/wiki/File:Typing_on_Keychron_V1_Ultra_(Red_Linear_Switch).wav)
  by C40115 (own work, Wikimedia Commons, 2026): 17.7 s of fast typing on red linear switches,
  recorded with a laptop's built-in microphone.
- **Changes (TakTak contributors, 2026-10-02):** this pack is made of modified excerpts of the
  recording, not the recording itself. The two identical channels were mixed to mono and
  high-passed at 30 Hz; 24 short excerpts (13 letter keystrokes, 7 space-bar key-downs and 4
  space-bar key-ups) were cut out, each starting 0.5 ms before its keystroke and ending with a
  short fade-out; every excerpt got a gain of +7.51 dB and the space-bar excerpts a further
  -5.35 dB; saved as 16-bit, 48 kHz mono WAV. Which strokes are key-downs and which key-ups was
  inferred from the timing. The pack plays at a volume of 0.94 (-0.5 dB). `preview.wav` is a
  typing phrase mixed from these excerpts.
- **Disclaimer:** the original work is offered as-is, and so is this adaptation; see sections 5
  and 6 of the [license](https://creativecommons.org/licenses/by/4.0/legalcode).
- **Provenance:** [`packs/linear-red/SOURCES.md`](packs/linear-red/SOURCES.md).

### Office Classic (`office-classic`)

- **License:** [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). No credit is
  required; it is given as a courtesy.
- **Credit:** Keyboard sounds by unicaegames (CC0).
- **Original work:** ["Keyboard Soundpack #1 [Typing and Single Keystrokes]"](https://opengameart.org/content/keyboard-soundpack-1-typing-and-single-keystrokes)
  by unicaegames (OpenGameArt), recorded on a Cherry KC 1000 membrane keyboard. 31 of its 32
  single keystrokes are used; the other one is a second copy of one of them.
- **Changes:** each keystroke split into its press and its release, trimmed and faded, one gain
  for the whole pack, 16-bit mono at the original 44.1 kHz; played at a pack volume of 0.94.
- **Provenance:** [`packs/office-classic/SOURCES.md`](packs/office-classic/SOURCES.md).

### Spring Lite (`spring-lite`)

- **License:** [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). No credit is
  required; it is given as a courtesy.
- **Credit:** "IBM M2 sound.ogg" by Anonimski, CC0 1.0, via Wikimedia Commons.
- **Original work:** ["IBM M2 sound.ogg"](https://commons.wikimedia.org/wiki/File:IBM_M2_sound.ogg)
  by Anonimski (own work, Wikimedia Commons, 2014): typing "www.wikipedia.org" and Enter on an
  IBM M2 buckling-spring keyboard.
- **Changes:** single presses and releases cut out of the recording, mixed to mono, one gain for
  the whole pack, 16-bit at the original 44.1 kHz; played at a pack volume of 0.74.
- **Provenance:** [`packs/spring-lite/SOURCES.md`](packs/spring-lite/SOURCES.md).

### Tactile (`tactile`)

- **License:** [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). No credit is
  required; it is given as a courtesy.
- **Credit:** Recordings by StavSounds, alpinemesh and yottasounds (Freesound, CC0 1.0), via
  clatterbox.
- **Original works (Freesound):**
  - ten recordings by StavSounds, "Keyboard_Tactile_2" to "_8", "_12", "_14" and "_15", from
    the Freesound pack [42151](https://freesound.org/people/StavSounds/packs/42151/) (each one
    linked in `SOURCES.md`, which also says why two other takes are not used);
  - ["Enter Key Press Mechanical Keyboard"](https://freesound.org/people/alpinemesh/sounds/627647/)
    by alpinemesh (an Enter key of a Corsair K70 RGB);
  - ["Computer Keyboard - single key - type 2.wav"](https://freesound.org/people/yottasounds/sounds/380141/)
    by yottasounds.

  The files were taken from the unchanged Freesound previews that
  [clatterbox](https://github.com/zordhalo/clatterbox/tree/9dca88f4d151e24f243b5a9aafec7a60c809fb48/src-tauri/resources/packs/tactile)
  ships with per-file credits (clatterbox's code is Apache-2.0; none of its code is used).
- **Changes:** each take split into its key-down and key-up, trimmed and faded; each
  StavSounds take levelled with its own gain (its key-down brought to within 3 dB of the median
  take's loudness, its key-up kept from going above that band, the take's balance kept), then
  one gain per source plus a headroom trim, 16-bit mono at the source rate; played at a pack
  volume of 0.98.
- **Provenance:** [`packs/tactile/SOURCES.md`](packs/tactile/SOURCES.md).

### Typewriter (`typewriter`)

- **License:** [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/)
  ([BigSoundBank licenses](https://bigsoundbank.com/licenses.html)). No credit is required; it
  is given as a courtesy.
- **Credit:** Typewriter recordings by Joseph Sardin (BigSoundBank.com), CC0.
- **Original works (BigSoundBank, by Joseph Sardin, recorded on a Hermes Precisa 305):**
  ["Typewriter #5"](https://bigsoundbank.com/typewriter-5-s2838.html),
  ["Typewriter #6"](https://bigsoundbank.com/typewriter-6-s2839.html),
  ["Typewriter #7"](https://bigsoundbank.com/typewriter-7-s2840.html),
  ["Typewriter #8"](https://bigsoundbank.com/typewriter-8-s2841.html),
  ["Typewriter, Key"](https://bigsoundbank.com/typewriter-key-s2842.html) and
  ["Typewriter, space"](https://bigsoundbank.com/typewriter-space-s2843.html).
- **Changes:** type-bar strikes, space-bar strokes, shift thuds and carriage returns (some with
  the margin bell) cut out of the takes, high-passed at 25 Hz, one gain per group of sounds,
  16-bit mono at the original 48 kHz; played at a pack volume of 1.29.
- **Provenance:** [`packs/typewriter/SOURCES.md`](packs/typewriter/SOURCES.md).

### UI Clicks (`ui-clicks`)

- **License:** [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). No credit is
  required; it is given as a courtesy.
- **Credit:** UI sounds by Kenney (kenney.nl), CC0.
- **Original works:** ["UI Audio"](https://kenney.nl/assets/ui-audio) and
  ["Interface Sounds"](https://kenney.nl/assets/interface-sounds) by Kenney (Kenney Vleugels,
  https://kenney.nl).
- **Changes:** 24 of the sounds chosen as key sounds (three switch sounds split into an "on" and
  an "off" click), mixed to mono, trimmed and faded, one gain per group of keys (Enter's set by
  loudness, with its own gain for its key-up clicks), 16-bit at the original 44.1 kHz; played
  at a pack volume of 0.86.
- **Provenance:** [`packs/ui-clicks/SOURCES.md`](packs/ui-clicks/SOURCES.md).

### Vintage (`vintage-keyboard`)

- **License:** [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). No credit is
  required; it is given as a courtesy.
- **Credit:** Recordings by jim-ph (Freesound, CC0 1.0), via Tickeys.
- **Original work:** jim-ph's Freesound pack ["vintage keyboard"](https://freesound.org/people/jim-ph/packs/12363/)
  (five keystrokes of a 1986 DOS-era mechanical keyboard, sounds 194795-194799). The files were
  taken from the unchanged copies, with Freesound's own `license.txt`, that the MIT-licensed
  [Tickeys](https://github.com/yingDev/Tickeys/tree/028083403ab75eae752446ce30cffb3b22b49eab/Tickeys.app/Contents/Resources/data/mechanical)
  app ships (none of its code is used).
- **Changes:** each keystroke split into its key-down and key-up, trimmed and faded, one gain
  for the whole pack, 16-bit mono at the original 44.1 kHz; played at a pack volume of 0.88.
- **Provenance:** [`packs/vintage-keyboard/SOURCES.md`](packs/vintage-keyboard/SOURCES.md).

## Third-party code

TakTak's Rust code uses third-party crates (for example `cpal`, `symphonia`, `rubato`, `rtrb`,
`notify`, `serde` and `zip`), each under its own license, mostly MIT and/or Apache-2.0. The
exact set and versions are pinned in [`Cargo.lock`](Cargo.lock). A complete list of crate
licenses and notices, generated from `cargo metadata`, will be added in a later milestone and
shipped with the app. Until then, `cargo metadata --format-version 1` lists every crate with its
license field.

The pack-source build scripts use Python, numpy and ffmpeg on the developer's machine only;
none of them is part of the app.
