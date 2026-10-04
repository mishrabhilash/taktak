# TakTak sound pack format (v1)

A **pack** is a folder, or a `.zip` of that folder, containing a `pack.json` manifest and
audio files. This document is the normative spec: the loader, the validator
(`taktak-pack validate`), the Mechvibes importer and the pack maker all implement it.

```
deep-thock/
├── pack.json
├── preview.wav
└── sounds/
    ├── KeyA-press.wav
    ├── KeyA-release.wav
    └── …
```

## pack.json

(The example is a pack as `tools/synth-packs` would generate it. Those synthesized packs are
experimental and not bundled with TakTak.)

```json
{
  "format": 1,
  "id": "deep-thock",
  "name": "Deep Thock",
  "version": "1.0.0",
  "author": "TakTak contributors",
  "license": "CC0-1.0",
  "description": "Lubed linear switches on a heavy aluminium case.",
  "source": "Synthesized by tools/synth-packs",
  "attribution": null,
  "preview": "preview.wav",
  "volume": 1.0,
  "trim_silence": true,
  "variation": { "pitch": 0.03, "volume": 0.10 },
  "groups": {
    "alphanumeric": { "press": ["sounds/alnum-1.wav", "sounds/alnum-2.wav"], "release": ["sounds/alnum-up.wav"] },
    "space":        { "press": ["sounds/space.wav"], "release": ["sounds/space-up.wav"] },
    "other":        { "press": ["sounds/alnum-1.wav"] }
  },
  "keys": {
    "KeyA": { "press": ["sounds/KeyA-press.wav"], "release": ["sounds/KeyA-release.wav"] }
  }
}
```

| Field | Required | Rules |
|---|---|---|
| `format` | yes | Integer. Must be `1`. Higher values: error "this pack needs a newer TakTak". |
| `id` | yes | `^[a-z0-9]+(-[a-z0-9]+)*$`, 1–64 chars. Stable identity of the pack (settings refer to it). |
| `name` | yes | 1–64 chars after trimming. |
| `version` | no | Free-form string, ≤ 32 chars. |
| `author` | yes | 1–128 chars. |
| `license` | yes | SPDX identifier, see [Licenses](#licenses). |
| `description` | no | ≤ 500 chars. |
| `source` | no | ≤ 500 chars: where the recordings came from (URL or text). |
| `attribution` | conditional | ≤ 500 chars. **Required** when `license` is a CC-BY variant: the credit line to display. |
| `preview` | no | File path. Played by "click to hear". If absent, the first `press` file of `groups.alphanumeric` (else `groups.other`) is used. |
| `volume` | no | Number in `[0.0, 2.0]`, default `1.0`. Pack-level gain. |
| `trim_silence` | no | Boolean, default `true`. Trim leading silence from every sample at load time (see [Audio](#audio)). |
| `variation` | no | `{ "pitch": 0.0–0.10, "volume": 0.0–0.50 }`, defaults `0.03` / `0.10`. Per-keystroke random ± range at full strength: the app scales both ranges by its humanize setting (0–1, default 0.25, so by default a keystroke varies within a quarter of these ranges). Either key may be omitted. |
| `groups` | no* | Map of group name → sound set. |
| `keys` | no* | Map of key name → sound set. |

Unknown top-level fields, and unknown fields inside sound sets, produce a **warning** (not an
error), so that newer packs still load in older TakTak versions. Unknown fields inside
`variation` are ignored without a warning.

Text fields (`name`, `version`, `author`, `description`, `source`, `attribution`) must not
contain control characters (line breaks, escape sequences). A key that appears twice in the
same JSON object is an error wherever TakTak reads the value (known top-level fields,
`variation.pitch`/`volume`, group and key names, `press`/`release`); duplicates inside fields
TakTak ignores are ignored too.

### Sound sets

```json
{ "press": ["a.wav", "b.wav"], "release": ["c.wav"] }
```

Both arrays are optional, and either may be empty. Each entry is a file path (see
[Paths](#paths)). The same file may appear in several sets.

When an array has more than one file, by default each key consistently uses one of them,
chosen by the key: a key keeps its sound from one keystroke (and one session) to the next,
and different keys spread over the files. A key's release comes from the matching position:
when the press and release arrays a key resolves to have the same length, its release is the
file at the same position as its press, so pools recorded as matched takes (`press-07.wav`
with `release-07.wav`) stay paired. The app may instead be set to pick a random file per
keystroke.

### Group names

`alphanumeric`, `space`, `enter`, `backspace`, `modifiers`, `other`.

| Group | Keys |
|---|---|
| `alphanumeric` | letters, digits, and the punctuation keys `Backquote Minus Equal BracketLeft BracketRight Backslash Semicolon Quote Comma Period Slash IntlBackslash IntlRo IntlYen` |
| `space` | `Space` |
| `enter` | `Enter`, `NumpadEnter` |
| `backspace` | `Backspace`, `Delete` |
| `modifiers` | `ShiftLeft/Right ControlLeft/Right AltLeft/Right MetaLeft/Right CapsLock Fn` |
| `other` | everything else (function keys, arrows, navigation, numpad, media, …) |

### Key names

The web [`KeyboardEvent.code`](https://www.w3.org/TR/uievents-code/) names, case-sensitive:
`KeyA`…`KeyZ`, `Digit0`…`Digit9`, `Space`, `Enter`, `ShiftLeft`, `F1`…`F24`, `ArrowUp`,
`Numpad0`, and so on. The full list is `Key::ALL` in `src-tauri/core/src/key.rs`. They name
**physical positions**, not characters: `KeyQ` is the key left of `W` regardless of keyboard
layout.

### Resolution (which sound a key plays)

Press and release are resolved **independently**, each through this chain. The first
non-empty array wins:

1. `keys[<key>].press` (or `.release`)
2. `groups[<the key's group>]`
3. `groups.other`
4. `groups.alphanumeric`
5. nothing: the key is silent for that action

*Validity rule:* at least one of `groups.alphanumeric.press` or `groups.other.press` must be
non-empty, so that every key makes a sound on press. Release sounds are optional; a pack with
no release sounds at all gets a warning.

## Paths

- Relative to the pack root, with `/` separators.
- Must not be empty, start with `/`, contain `\`, contain a `..` or `.` segment, or contain
  empty segments (`a//b`).
- No segment may start with `.` (hidden files are skipped by hot reload, so a pack must not
  depend on them).
- Names must be valid on every platform: no `:` anywhere (this also rules out drive prefixes
  such as `C:` and NTFS streams such as `a.wav:x`), no `< > " | ? *`, no control characters,
  no segment ending in `.` or a space, and no Windows device names (`CON`, `PRN`, `AUX`,
  `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, with or without an extension; `console.wav` is fine).
- Must end in `.wav`, `.ogg` or `.mp3` (case-insensitive).
- Must refer to an existing file in the pack. Matching is case-sensitive on every platform,
  so that a pack never works on macOS and breaks on Linux.

## Audio

- Formats: WAV (PCM 8/16/24/32-bit int, 32-bit float), Ogg Vorbis, MP3, and nothing else
  (no 64-bit float, A-law or μ-law WAV, no MPEG Layer 1/2). The file's content must match its
  extension: a `.wav` must be a WAV file, and so on.
- Any sample rate from 1 kHz to 1 MHz. A file whose header declares a rate outside that range
  is treated as corrupt (an error). Samples are decoded once at load and resampled to the
  output device rate.
- Stereo or multichannel files are downmixed to mono. Per-key stereo panning is reserved for
  a future format version.
- **Leading silence adds latency.** With `trim_silence` (the default), everything before the
  first sample whose absolute value reaches −50 dBFS (0.00316) is cut, minus a 0.5 ms
  pre-roll. With `trim_silence: false`, more than 5 ms of leading silence produces a warning.
- Each decoded sample must be ≤ 2.0 s long (keystrokes are short; this also bounds memory).
  Longer is an error.
- Limits per pack: each audio file ≤ 10 MB, `pack.json` ≤ 1 MB, ≤ 2,000 distinct audio
  files, total decoded audio ≤ 16 M samples counted at 48 kHz (about 349 s), whatever the
  output device's rate, so a pack is valid or invalid the same way on every machine.
- Loudness is not normalized by the loader. Pack authors should master consistently, and
  `volume` adjusts the whole pack: every key sound and the preview (see [Loudness](#loudness)).

## Loudness

`volume` is a playback gain for the whole pack: the loader keeps the files as they are and the
mixer applies `volume` to every keystroke (on top of the random volume variation); the loader
applies it to the preview clip, so "click to hear" is as loud as typing. Clipping is the pack's
responsibility: the mixer adds all voices and clips hard at 0 dBFS.

To measure a pack the way the bundled packs are matched, from a checkout of the repository:

```
cargo run -p synth-packs --release -- loudness path/to/pack-or-zip... [--target LK] [--check]
```

It loads each pack at 48 kHz like the app and reports, per pack:

- **typing loudness**: what typing sounds like with the default settings. For each
  alphanumeric key, the K-weighted (ITU-R BS.1770) energy of the first 100 ms of the file it
  plays on press (the one it keeps, see [Sound sets](#sound-sets)), averaged as power over
  the keys, in LK, as stored and with the pack's `volume`, and its distance from the target
  (default −25.8 LK, TakTak's reference typing level);
- the **true peak** (4x oversampled) of the loudest file any key can play (with random
  variants on, every file in its arrays), with `volume` and the top of the volume variation
  applied, and the largest `volume` that keeps it at or below −1 dBFS;
- the `volume` that would reach the target, and whether headroom limits it.

`--check` exits with status 1 if a pack is more than 0.5 dB off target although its `volume`
could fix it, or breaks the −1 dBFS headroom rule.

## Zip packs

- The zip contains either `pack.json` at its root, or exactly one top-level folder that
  contains `pack.json`. That folder is then the pack root, and everything outside it (a
  README or license file next to the folder, other folders without `pack.json`) is ignored.
- Two or more top-level folders that contain `pack.json` are an error: one pack per zip.
- macOS metadata (`__MACOSX/`, `.DS_Store`) is always ignored.
- Entries with absolute paths, `..` segments or drive prefixes (a letter and `:`) make the
  zip invalid, wherever they are. Inside the pack folder, entry names must also not contain
  `\` or NUL.
- Entries that share compressed data (overlapping entries, a zip-bomb technique) make the zip
  invalid.
- Limits: ≤ 5,000 entries, total uncompressed size ≤ 200 MB, and no entry larger than 1 MiB
  with a compression ratio above 100:1. Smaller entries are exempt, because short runs of
  silence legitimately compress far better than that. Reading an entry never goes past its
  declared size, so a zip cannot inflate beyond what its directory claims.

## Licenses

`license` must be an SPDX identifier. Allowed, meaning redistributable and MIT-compatible:

`CC0-1.0`, `CC-BY-3.0`, `CC-BY-4.0`, `MIT`, `0BSD`, `Unlicense`, `Apache-2.0`,
`BSD-2-Clause`, `BSD-3-Clause`, `ISC`.

Packs for personal use only may declare `LicenseRef-Personal`. They load locally with a
warning, but `taktak-pack validate --strict` rejects them. Strict mode is used in CI for
bundled packs and will be used for the future community gallery. Any other value (including
`CC-BY-SA-*`, `CC-BY-NC-*` and `CC-BY-ND-*`) is an error.

## Bundled packs

Packs shipped inside TakTak (the repository's `packs/` folder) are redistributed with an app
that may be sold, so they meet stricter rules. CI enforces them in
`src-tauri/core/tests/bundled_packs.rs`, which includes the `taktak-pack validate --strict`
checks, and the loudness rule in `tools/synth-packs` (next to the measurement):

- The license must pass strict validation: one of the allowed licenses above, never
  `LicenseRef-Personal`.
- The folder name equals the pack `id`, and the pack loads with every key making a press
  sound. At most 6 MB on disk per pack.
- `SOURCES.md` in the pack folder records the provenance of every audio file: where it came
  from (URL, or how it was synthesized or recorded), the author, proof of the license (a link
  to the license statement, or how permission was granted), the SHA-256 of the file as
  shipped, and the processing applied (slicing, trimming, resampling, gain). CI checks that
  every audio file appears by its full pack-relative path with its shipped SHA-256 on the
  same line.
- Bundled packs are folders, never zips.
- `LICENSE.txt` in the pack folder when the license requires its notice to travel with copies:
  MIT, BSD-2-Clause, BSD-3-Clause, ISC and Apache-2.0 (the license text), CC-BY-3.0 and
  CC-BY-4.0 (the credit line and a link to the license).
- Typing loudness matches TakTak's reference typing level (a fixed −25.8 LK, see
  [Loudness](#loudness)) within
  0.5 dB, set with `volume` (or, beyond +6 dB, with gain in the pack's build), unless the
  loudest true peak would then exceed −1 dBFS at the top of the volume variation: such a pack
  plays at the largest clean `volume`, and its `SOURCES.md` states the shortfall (test
  `bundled_packs_are_loudness_matched`).
- No third-party trademarks in `name` or `id`, for example IBM, Model M, Model F, Cherry, MX,
  Gateron, Kailh, Keychron, Razer, Logitech, Corsair, Topre, HHKB or Kenney (matched as whole
  words, ignoring case). The hardware may be named factually in `description`
  ("recorded on an IBM Model M").

## Where packs live

| | Bundled (read-only) | User packs (hot-reloaded) |
|---|---|---|
| macOS | `TakTak.app/Contents/Resources/packs` | `~/Library/Application Support/tech.taktak.app/packs` |
| Windows | `<install dir>\packs` | `%APPDATA%\tech.taktak.app\packs` |
| Linux | `/usr/lib/TakTak/packs` (`.deb`); inside the AppImage, `$APPDIR/usr/lib/TakTak/packs` | `~/.local/share/tech.taktak.app/packs` |

Every direct child of a packs directory that is either a folder containing `pack.json` or a
`.zip` file is a pack. Hidden entries (names starting with `.`) and everything else are
ignored. Symbolic links to pack folders or zips are followed, and their targets are watched
too. Adding, changing or removing a pack in the user folder takes effect within about a
second, with no restart; deleting or fixing a broken pack clears its error. A user pack with the same
`id` as a bundled pack overrides it (with a warning). Two user packs with the same `id` is an
error for the second one in path order.

## Errors and warnings

Problems are reported all at once (not one at a time) with a location, for example:

```
packs/my-pack: 2 errors, 1 warning
  error   pack.json:14:9     invalid JSON: expected `,` or `}`
  error   keys.keya          unknown key name "keya" (did you mean "KeyA"?)
  warning release            pack has no release sounds; key-up will be silent
```

A pack with errors never loads. A pack that fails to reload keeps its previous good version
playing, and the error is shown in the UI.

## Validating a pack

```
taktak-pack validate path/to/pack-or-zip... [--strict]
taktak-pack info path/to/pack-or-zip
taktak-pack list [--bundled DIR] [--user DIR]
```

- `validate` checks one or more packs as the loader would, decoding every sound at 48 kHz, and
  prints every error and warning. `--strict` also rejects licenses that cannot be bundled or
  shared (`LicenseRef-Personal`), as CI does for bundled packs.
- `info` prints a pack's metadata and settings, the files per group and per key, which keys
  sound on press and on release, the decoded size and the load time.
- `list` scans folders the way the app does: the packs it would offer (with their origin and
  warnings), bundled packs overridden by a user pack with the same `id`, and invalid
  candidates with their errors. `--user` defaults to the app's user pack folder (see
  [Where packs live](#where-packs-live)); without `--bundled` no bundled folder is scanned.

Exit status: 0 when every pack is valid, 1 when one has errors, 64 for a usage error. Only pack
contents are printed; nothing is ever read from the keyboard.

To compare a pack's loudness with the bundled packs, see [Loudness](#loudness).

## Importing Mechvibes packs

Mechvibes packs carry no license, so TakTak never bundles or shares them. Users can import
packs they already have, for their own use:

```
taktak-import-mechvibes <folder-or-zip>... [--out DIR] [--overwrite] [--no-split-release]
```

(From a checkout: `cargo run -p mechvibes-import --release -- <args>`.) `--out` defaults to the
user pack folder (see [Where packs live](#where-packs-live)), where the running app picks the
new pack up within a second. For each pack the tool prints the new id, the keys mapped, the
key codes skipped, missing and unreadable files, and warnings. Exit status: 0 when every pack
was imported, 1 when one failed, 64 for a usage error. The importer is the `pack::import`
module of `taktak-core`, and the app offers the same thing: **Settings → Sounds → Import
Mechvibes pack…** opens the system's picker for a pack folder or `.zip`, imports into the user
pack folder on a worker thread, shows what was mapped and any notes, and offers to replace an
earlier import of the same pack (the CLI's `--overwrite`). Release splitting is always on
there.

### What can be imported

| Variant | Recognized by | Sounds |
|---|---|---|
| Mechvibes v1, sprite | `key_define_type: "single"` | `defines: {"<code>": [start_ms, length_ms]}` slices of the `sound` file |
| Mechvibes v1, files | `key_define_type: "multi"`, `"multiple"` or any other value; when the field is missing, arrays mean sprite and strings mean files | `defines: {"<code>": "file"}` |
| Mechvibes v2 | `version: 2` | as v1 files, plus `"<code>-up"` release keys, `sound`/`soundup` fallback press/release for undefined keys, and `{a-b}` file ranges (`GENERIC_R{0-4}.mp3` becomes five variants) |
| Mechvibes++ | `compatibility: true` | `"0<code>"` press and `"00<code>"` release; a plain `"<code>"` is the press only when `"0<code>"` is absent |
| MechvibesDX | `definitions` (or `defs`), `config_version` `"2"` | W3C key names, `{"timing": [[start_ms, end_ms], …]}` over `audio_file` (or a per-key `audio_file`): one timing is a press, two are press and release, more are press and release with the rest ignored (warning). `options.recommended_volume` becomes `volume`, `options.random_pitch: true` becomes `variation.pitch` 0.10. A pack MechvibesDX converted from Mechvibes v1 (`config.json.v1.backup` next to `config.json`) is imported from the original config. |

`"<code>-up"` slices in v1 sprite packs (Mechvibes' own CherryMX Black ABS has 48) are used as
release sounds too. Not imported: mouse packs, Mechvibes config versions 3 and up (drafts no
Mechvibes release reads), MechvibesDX `config_version` above 2.

The source is a folder or a `.zip`. Its `config.json` (any letter case, a UTF-8 BOM is fine;
comments and trailing commas are tolerated with a warning) is found by name at any depth: the
shallowest one is the pack root, and two at the same depth are an error ("import them one at
a time"). macOS metadata (`__MACOSX/`, `._*`, `.DS_Store`) is ignored; zips get the same size,
entry-count, compression-ratio and overlap limits as [zip packs](#zip-packs), and entries that
would escape the folder are skipped.

### Keys

- Mechvibes `defines` keys are libuiohook key codes, mapped by their libuiohook meaning with
  the table in `src-tauri/core/src/pack/import/keycodes.rs` (168 codes; `"30"` is `KeyA`,
  `"57416"` is `ArrowUp`). Mechvibes' own per-OS remaps are not replicated: they encode
  position guesses and bugs (on macOS it plays ArrowUp's sound for F13).
- The Windows-only aliases 60999–61011 (dedicated navigation and arrow keys) only fill gaps:
  as in Mechvibes, the standard code's sound wins.
- Packs numbered with Linux evdev codes (Linux-oriented re-packs; recognized by codes such as
  `97`, right Control) have codes 85–127 read as evdev codes, for keys no libuiohook code in the
  pack defines.
- Unknown codes, keys TakTak does not have (media and browser keys), editor artefacts such as
  `"91,91,92"` and Mechvibes++ mouse codes are skipped and listed in the report. When a JSON key
  appears twice, the last one wins (as in Mechvibes), with a warning.
- A key that is silent in Mechvibes (absent or `null`) plays the pack-wide fallback in TakTak,
  because a pack cannot make a key silent (see [Resolution](#resolution-which-sound-a-key-plays)).

### Sounds

- File names are resolved leniently: the exact path, then ignoring case, then a unique file
  with that name anywhere in the pack (`release/ENTER.mp3` that only exists as `ENTER.mp3`),
  then, for a name without an extension, a unique `name.*`. Backslashes count as `/`.
  Missing files are reported and their keys skipped; the import fails only if nothing usable
  is left. (mechvibes.com downloads of "v2" packs contain only `config.json`; import the pack
  folder from the Mechvibes app's `custom` folder instead.)
- Audio is recognized by content, not extension (Mechvibes packs have `.wav` files holding MP3
  and files without an extension), and decoded like pack audio: WAV, Ogg Vorbis and MP3. Other
  formats (AAC/M4A, FLAC, AIFF, Opus) are reported as unreadable.
- Every sound is cut from its file (sprite slices are clamped to the file; zero-length slices
  and slices past the end are dropped with a warning), mixed down to mono, trimmed to start at
  its onset (the first sample at max(noise floor + 20 dB, peak − 35 dB), capped at
  peak − 20 dB, minus the 0.5 ms pre-roll: leading silence is keypress latency, and Mechvibes
  sprite slices often start up to 40 ms early), cut to at most 1.95 s, faded out over 5 ms,
  and written as 16-bit WAV at the source rate (48 kHz when the files have different rates).
  Clips that stay below −50 dBFS are dropped as silent. Identical sounds are stored once.
- **Release splitting** (on by default; `--no-split-release` turns it off). A Mechvibes v1
  sprite slice holds the whole keystroke, and Mechvibes plays all of it on key-down. The
  importer finds the release in it: the largest 2 ms rise of the 1 ms envelope between 35 % and
  90 % of the onset-trimmed slice, if it rises by at least 10 dB, with the cut in the quiet
  valley up to 30 ms before it. The release part is extended by up to 60 ms past the slice (the
  slices clip the release tail), stopping 5 ms before the next slice in the sprite or once the
  sound has decayed to the noise floor. A pack is split only when at least 70 % of its slices
  have a clear release; the others are then cut at 65 % of their length (where the median
  hand-cut release starts). Otherwise every key keeps the whole keystroke on press, as in
  Mechvibes. MechvibesDX packs that cut each keystroke at its midpoint (a contiguous
  press/release pair) are re-split the same way, falling back to their own boundary. On
  Mechvibes' CherryMX Black ABS sprite the cut lands a median 7 ms from the 48 hand-cut
  boundaries of its repository config; the midpoint is a median 32 ms early, inside the press.
- `groups.alphanumeric` is the pack-wide fallback: v2 `sound`/`soundup`, otherwise up to four
  letter keys' press (and release) sounds. Every key with sounds of its own gets a `keys`
  entry, except keys whose sounds equal the fallback.

### The imported pack

- `id`: `mv-` and a slug of the pack name (`mv-cherrymx-blue-abs-keycaps`), with `-2`, `-3`, …
  if a pack or folder in the destination already uses it. Importing the same pack again (same
  id, same `source`) is refused unless `--overwrite` is given, which replaces the earlier
  import.
- `name`: the config's name, trimmed and cut to 64 characters (the full name then goes in
  `description`); the folder name if there is none. `author`: `author` or `m_author`, else
  "Unknown (imported from Mechvibes)". `license`: `LicenseRef-Personal`. `source`: the folder
  or zip file name. `description`: notes the personal-use import, plus the config's own
  description.
- `preview.wav`: 1.9 s of the pack's own sounds on a fixed, made-up rhythm (H E L L O space T A
  K Enter), peak-limited to −1 dBFS.
- The pack is written to a hidden temporary folder in the destination, validated as the loader
  would, and only then moved into place. It loads with the `LicenseRef-Personal` warning (plus
  "no release sounds" for packs that have none), and `taktak-pack validate --strict` rejects it,
  so an imported pack can never be bundled.
