# pack-maker

Makes a [TakTak sound pack](../../docs/pack-format.md) from keyboard recordings **you have the
right to use**: your own keyboard recorded through a microphone, or a WAV file you made yourself
or that is licensed CC0 or similar.

```
cargo run -p pack-maker --release -- record --out my-board --id my-board \
    --name "My Board" --author "Your Name" --license CC0-1.0

cargo run -p pack-maker --release -- slice --input typing.wav --out my-board --id my-board \
    --name "My Board" --author "Your Name" --license CC-BY-4.0
```

## Rights

Only record keyboards, and only use recordings, that you are allowed to share under the license
you give with `--license`. pack-maker records a microphone. It does not capture audio that
other apps or websites play: output-only devices and well-known loopback devices (BlackHole,
Soundflower, "Stereo Mix", PulseAudio monitors, …) are refused.

Allowed licenses: `CC0-1.0 CC-BY-3.0 CC-BY-4.0 MIT 0BSD Unlicense Apache-2.0 BSD-2-Clause
BSD-3-Clause ISC`. Use `LicenseRef-Personal` for a pack you only keep for yourself. CC-BY packs
need a credit line: pass `--attribution`, or `NAME by AUTHOR` is used (also when the one given
is blank).

## `record`: microphone + keyboard

```
pack-maker record --out DIR --id ID --name NAME --author AUTHOR --license SPDX
                  [--seconds N] [--takes N] [--device NAME]
                  [--version V] [--description TEXT] [--attribution TEXT] [--source TEXT] [--force]
```

The microphone and a global key listener run together. Each key press and release is matched
with the sound the microphone heard at that moment, so every key gets its own samples.

- `--seconds N` stops after N seconds. Otherwise press **Ctrl+C**, or **Escape 3 times within
  1.5 s** (those three presses are not part of the pack). The limit is 10 minutes.
- `--takes N` (default 3, at most 10) is how many takes are kept per key and action: the ones
  closest to that key's typical loudness. By default the app plays one of them for each key,
  every time (chosen by the key, not always take 1); the others are heard only with random
  variants on. `--takes 1` keeps just the most typical take.
- `--device NAME` picks an input device by name (exact, or a unique part of it). The default is
  the system microphone. An unknown name lists the available devices.

While recording, a status line shows the elapsed time, the **number** of keystrokes and the input
level. It never shows which keys, and it is drawn only when stderr is a terminal. Characters typed into the terminal are neither echoed nor kept,
and are discarded when recording ends, so the shell never sees them.

### Permissions (macOS)

- **Input Monitoring**, to know which key made each sound. Grant it to the app you run
  pack-maker from (Terminal, iTerm, VS Code, …) in System Settings > Privacy & Security >
  Input Monitoring, then quit and reopen that app.
- **Microphone**, asked for the first time recording starts. If the level meter stays empty,
  allow the same app in System Settings > Privacy & Security > Microphone and run again.

Recording needs the TakTak key listener, which exists on macOS only for now. `slice` works on
every platform.

### Recording tips

- Use a quiet room: no fan, music or voices. pack-maker warns when keystrokes are less than
  20 dB above the background noise.
- Put the microphone 20–30 cm from the keyboard, pointing at it.
- Type **each key slowly, 3–5 times**, about half a second apart, so that the sounds don't
  overlap. Keys pressed together, or too quickly, are skipped.
- Don't hold keys down. Release sounds are part of the pack.
- Watch the meter. `CLIP!` means too loud: lower the input gain or move the microphone back.
  Clipped takes are rejected.
- Cover the keys you want sounds for. Keys you don't record fall back to group sounds
  (alphanumeric, space, enter, backspace, modifiers, other), so recording a few letters plus
  Space, Enter, Backspace and Shift already gives a complete pack.

## `slice`: an existing recording

```
pack-maker slice --input FILE.wav --out DIR --id ID --name NAME --author AUTHOR --license SPDX
                 [--threshold-db -40] [--min-gap-ms 25] [pack options as above]
```

Reads a WAV file (PCM 8/16/24/32-bit or 32-bit float, any rate, any channel count), finds the
keystrokes in it and tells presses from releases. A release follows its press by 40–250 ms and
is quieter or brighter. Any other sound counts as a press. The 12 most consistent presses and
releases become one `alphanumeric` pool, which every other key falls back to.

- `--threshold-db` ignores sounds whose 1 ms level stays below this, in dBFS. Lower it for
  quiet recordings.
- `--min-gap-ms` treats sounds closer together than this as one keystroke.

## What gets written

```
DIR/
├── pack.json              format 1, your metadata, keys + groups
├── preview.wav            ~1.5 s of typing built from the samples
└── sounds/
    ├── KeyA-press-1.wav   (record) one file per kept take
    ├── KeyA-release-1.wav
    ├── press-1.wav        (slice) the pool
    └── release-1.wav
```

- Samples are 16-bit mono WAV at 44.1 or 48 kHz. Other source rates are resampled to 48 kHz
  with a windowed-sinc filter.
- Each sample starts 0.5 ms before its keystroke. Leading silence is playback latency, so
  onsets are located to the sample. Each one ends before the next keystroke, after at most
  300 ms (press) or 200 ms (release), or once it has decayed into the noise. A 5 ms fade-out
  follows.
- One gain is applied to the whole pack so that the median press peaks at −6 dBFS. Press and
  release keep their natural balance. If one take is much louder than the rest, the gain is
  lowered so that it still peaks below about −1 dBFS.
- Mic rumble below 30 Hz and DC offset are removed.
- In `record` mode, `groups` holds pools drawn from the recorded keys, so unrecorded keys
  still sound. The pack always has the fallback press sound that the format requires.
- An existing, non-empty `DIR` is only written to with `--force`. Files with the same names are
  replaced. Other files are kept and reported.

## Privacy

- Key events (which key, down or up, when) exist **only in memory** while pack-maker runs, and
  are discarded once the takes are cut.
- What you typed, in which order, and with what rhythm is **never written to disk or printed**.
  The output holds only per-key samples. Take numbers follow how typical each take is, not
  when it was recorded. The preview uses a fixed, made-up rhythm and a fixed key order, and
  each key in it plays the take the app gives that key by default.
- While recording, a status line shows the elapsed time, the number of keystrokes so far and
  the input level. It is drawn only when stderr is a terminal, where each refresh overwrites the
  last. With stderr redirected to a file or a pipe, it is not printed at all, so no log holds a
  running count. A terminal recorder (`script`, asciinema) still captures it, as a screen
  recording would.
- The set of keys you pressed does show in the pack, because each one gets samples. Don't type
  anything sensitive while recording: just tap keys.
- There is no network code. Nothing is uploaded.

## How it works

1. **Onsets**: two high-passed energy envelopes (above 150 Hz, and a 2 kHz click band) at 1 ms
   resolution. A keystroke is a fast rise well above an adaptive noise floor, which is the 10th
   percentile of 200 ms blocks. Each detection is then refined to the first sample that rises
   above the noise floor (+12 dB) and the decaying background, so that samples start on the
   transient.
2. **Alignment** (`record`): the microphone's capture timestamps and the key events share one
   clock (mach host time on macOS). Elsewhere, an offset is re-established on every audio
   callback. A smoothed map turns event times into sample positions. The typical
   key-to-sound offset is then measured from the recording, and each event takes the nearest
   onset in a window from 30 ms before to 80 ms after. Events that are ambiguous, crowded or
   unheard are skipped.
3. **Takes**: cut, reject clipped, too quiet and too short takes, keep the most consistent ones,
   build the group pools, normalize, write.

All processing is pure and unit-tested on synthetic recordings (`cargo test -p pack-maker`).
Only the thin capture layer (`src/capture.rs`) needs a real microphone.
