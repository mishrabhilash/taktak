# Adding a sound pack

This walks through making a TakTak sound pack, checking it, and trying it in the app. The
format itself is specified in [pack-format.md](pack-format.md); this page is the practical
route. Everything here runs on your machine, offline.

You need a checkout of this repository with Rust installed (see
[CONTRIBUTING.md](../CONTRIBUTING.md#development-setup)) for the tools: `pack-maker` (record or
slice), `taktak-pack` (validate) and `synth-packs loudness` (level-match). Run them from the
repository root.

## 1. Choose where the sounds come from

Only use sounds you have the right to use, under the license you will give the pack:

- **Record your own keyboard** with `pack-maker record` (macOS for now): every key you tap gets
  its own press and release sound. Best result, and the sounds are yours.
- **Slice a recording** you made, or one licensed CC0, CC BY or similar, with
  `pack-maker slice`: it finds the keystrokes in a WAV file and builds a pack from them.
- **Assemble by hand** from short audio files you already have: write `pack.json` yourself
  (step 3).
- **Convert a Mechvibes pack** for your own use with the Mechvibes importer (see the
  [README](../README.md#import-mechvibes-packs)). Such packs are `LicenseRef-Personal` unless
  the original author's license says otherwise.

## 2. Record or slice with pack-maker

Record (a quiet room, the microphone 20–30 cm from the keyboard; tap each key 3–5 times,
slowly):

```sh
cargo run -p pack-maker --release -- record --out my-board --id my-board \
    --name "My Board" --author "Your Name" --license CC0-1.0
```

Stop with Ctrl+C, or Escape three times. macOS asks for **Input Monitoring** (to know which
key made each sound) and **Microphone** for the terminal app you run it from. pack-maker shows
only the elapsed time, the number of keystrokes and the input level, never which keys, and
writes nothing about what or how you typed: just per-key samples.

Or slice an existing recording:

```sh
cargo run -p pack-maker --release -- slice --input typing.wav --out my-board --id my-board \
    --name "My Board" --author "Your Name" --license CC-BY-4.0 \
    --attribution "My Board by Your Name, CC BY 4.0"
```

Either way you get a complete pack folder with `pack.json`, `preview.wav` and `sounds/`. All
options, recording tips and how it works: [tools/pack-maker/README.md](../tools/pack-maker/README.md).
Then continue at step 4.

## 3. Or lay out a pack by hand

```
my-board/
├── pack.json
├── preview.wav            optional: what "click to hear" plays
└── sounds/
    ├── press-1.wav
    ├── press-2.wav
    ├── release-1.wav
    ├── space.wav
    └── enter.wav
```

A minimal `pack.json`:

```json
{
  "format": 1,
  "id": "my-board",
  "name": "My Board",
  "version": "1.0.0",
  "author": "Your Name",
  "license": "CC0-1.0",
  "description": "My keyboard, recorded on my desk.",
  "preview": "preview.wav",
  "groups": {
    "alphanumeric": {
      "press": ["sounds/press-1.wav", "sounds/press-2.wav"],
      "release": ["sounds/release-1.wav"]
    },
    "space": { "press": ["sounds/space.wav"] },
    "enter": { "press": ["sounds/enter.wav"] }
  }
}
```

The rules that matter most:

- `id` is lowercase letters, digits and single hyphens (`my-board`), and is how settings refer
  to the pack. The folder name should match it.
- Every key must make a sound on press: `groups.alphanumeric.press` or `groups.other.press`
  must list at least one file. Keys without their own entry fall back to their group, then to
  `other`, then to `alphanumeric`.
- Per-key sounds go under `keys`, by physical position (`"KeyA"`, `"Space"`, `"ShiftLeft"`,
  the web `KeyboardEvent.code` names).
- Paths are relative, with `/`, and match file names exactly (case included).
- Audio: WAV, Ogg Vorbis or MP3, at most 2 s per file. Cut the silence before each keystroke
  (leading silence is latency; `trim_silence`, on by default, trims it at load time anyway).
- A CC BY license needs an `attribution` line.

Groups, key names, `variation`, `volume` and every limit: [pack-format.md](pack-format.md).

## 4. Validate it

```sh
cargo run -p taktak-core --release --bin taktak-pack -- validate my-board
cargo run -p taktak-core --release --bin taktak-pack -- info my-board
```

`validate` loads the pack exactly as the app does, decoding every sound, and prints every
error and warning with its location (for example `keys.keya  unknown key name "keya" (did you
mean "KeyA"?)`). It exits 0 when the pack is valid. `info` shows which keys sound on press and
release, the memory it takes and the load time. Add `--strict` to also reject licenses that
can't be shared (`LicenseRef-Personal`), as CI does for bundled packs.

## 5. Match the loudness

So that switching packs doesn't jump in volume, measure the pack against TakTak's reference
typing level (−25.8 LK):

```sh
cargo run -p synth-packs --release -- loudness my-board
```

Write the `volume for ref` it prints into `pack.json` as `"volume"` (0.0–2.0), and run it
again: `status` should read `ok` (within 0.5 dB) or `limited` (as loud as it can go without
clipping). `--check` exits 1 when a pack is off. The columns are explained in
[tools/synth-packs/README.md](../tools/synth-packs/README.md#measuring-any-packs-loudness).

## 6. Try it in TakTak

Copy the folder (or a `.zip` of it) into your user packs folder:

| OS | User packs folder |
|---|---|
| macOS | `~/Library/Application Support/tech.taktak.app/packs/` |
| Windows | `%APPDATA%\tech.taktak.app\packs\` |
| Linux | `~/.local/share/tech.taktak.app/packs/` |

It appears in the tray popover's pack list within about a second, no restart needed. Edit
files in place and TakTak reloads the pack; if an edit breaks it, Settings lists the errors
and the last good version keeps playing. A user pack with the same `id` as a bundled pack
replaces it. To check what the app would see: `taktak-pack list --user <folder>`.

## 7. Share it

Zip the pack folder (`pack.json` at the zip's root, or inside one top-level folder) and share
the `.zip`; anyone can drop it into their user packs folder. Include a license that allows
sharing, and credit the recordings you used.

## 8. Propose it for bundling

A pack shipped inside TakTak has to meet stricter rules: an allowed license, a `SOURCES.md`
recording the provenance and SHA-256 of every file, a `LICENSE.txt` where the license asks
for one, no trademarks in its name, matched loudness, at most 6 MB, and a credit in
`CREDITS.md`. They are listed in
[CONTRIBUTING.md § Sound packs](../CONTRIBUTING.md#sound-packs) and specified in
[pack-format.md § Bundled packs](pack-format.md#bundled-packs). Put the pack in `packs/<id>/`
and run `cargo test -p taktak-core --test bundled_packs` and
`cargo run -p synth-packs --release -- loudness --check packs/<id>` before opening a pull
request.
