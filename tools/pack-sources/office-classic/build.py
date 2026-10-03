#!/usr/bin/env python3
"""Builds packs/office-classic from unicaegames' CC0 "Keyboard Soundpack #1".

    python3 tools/pack-sources/office-classic/build.py ZIP [PACK_DIR]

ZIP is unicae_games_keyboard_soundpack_1_0.zip (fetch.sh downloads it and checks its SHA-256).
PACK_DIR defaults to packs/office-classic in this repository. Standard library only. The output
is deterministic: the same zip always gives byte-identical files.

Each source file "Single Keys/keypress-NNN.wav" (44.1 kHz, 16-bit, mono, 250 ms) holds one
complete tap on the keyboard: the press, then 40-125 ms later the key coming back up. For each:

1. Remove DC (subtract the mean), then a 2nd-order Butterworth high-pass at 30 Hz.
2. Find the press onset and the release onset (find_events) and split the tap there.
3. press   = 0.5 ms before the press onset .. 0.5 ms before the release onset
   release = 0.5 ms before the release onset .. where it decays into the noise floor
4. One gain for the whole pack: the median press peak goes to -6 dBFS, unless that would push
   any sample above -1 dBFS; then the loudest sample is put at -1 dBFS instead. An onset that
   is still below -50 dBFS after the gain moves to the first sample that reaches it, so that
   no sample has more than 0.5 ms of leading silence.
   Every sample gets a raised-cosine fade-in over its 0.5 ms pre-roll and an 8 ms
   raised-cosine fade-out that ends on an exact zero.
5. Write 16-bit mono WAVs at 44.1 kHz (the source rate, no resampling), preview.wav,
   pack.json (with the playback "volume" VOLUME, see below) and SOURCES.md.

Two of the single keystrokes are one take (SKIPPED): only the first is used. The build stops if
any two keystrokes it uses are that alike (find_duplicates), so the pool never plays one stroke
twice as often as the others. The human-typing and generated-typing takes in the zip are not
used either (see SOURCES.md).
"""

import array
import hashlib
import json
import math
import operator
import random
import statistics
import sys
import wave
import zipfile
from io import BytesIO
from pathlib import Path

ZIP_NAME = "unicae_games_keyboard_soundpack_1_0.zip"
ZIP_SHA256 = "935eae2fa5c3742eacdd38c4ea0e9047f3887faa0701996498492c256db1b351"
ZIP_URL = "https://opengameart.org/sites/default/files/" + ZIP_NAME
PAGE_URL = "https://opengameart.org/content/keyboard-soundpack-1-typing-and-single-keystrokes"
CC0_URL = "https://creativecommons.org/publicdomain/zero/1.0/"
CHECKED = "2026-10-02"
SOURCE_COUNT = 32
# Single keystrokes that are left out, by number, with the take each one repeats. Pack files
# keep the source numbers (press-NN.wav and release-NN.wav come from keypress-0NN.wav), so
# these numbers are simply missing from the pack.
SKIPPED = {21: 16}

SR = 44100
BLOCK = 44  # samples per envelope block (about 1 ms)
PRE_ROLL = round(0.0005 * SR)  # 22 samples kept before each onset
FADE_OUT = round(0.008 * SR)  # 353 samples
MAX_LEN = round(0.400 * SR)  # cap per sample
HP_HZ = 30.0
DETECT_HZ = 150.0  # detection only, never in the output
PRESS_TARGET_DB = -6.0
CEILING_DB = -1.0
SILENCE_THRESHOLD = 0.00316  # -50 dBFS, as in docs/pack-format.md
# Two keystrokes whose waveforms around the press peak correlate at least this much are the
# same recording (distinct taps of this set stay below 0.6; keypress-016 and -021 reach 0.99999).
DUPLICATE_NCC = 0.95
DUP_WINDOW = (round(0.002 * SR), round(0.040 * SR))  # samples before, after the peak
DUP_MAX_LAG = 3
# Playback level, written to pack.json as "volume"; it changes no audio file. It matches the
# pack's typing loudness to the synthesized packs' REFERENCE_LK, so switching packs does not
# jump in volume. Measured on the built pack with
#     cargo run -p synth-packs --release -- loudness packs/office-classic
# (tools/synth-packs/src/loudness.rs): LK_AS_BUILT at volume 1.0. Re-measure whenever the
# processing here changes.
VOLUME = 0.94
LK_AS_BUILT = -25.65
REFERENCE_LK = -25.8


def db(x):
    return 20.0 * math.log10(max(x, 1e-12))


def read_wav(data, name):
    with wave.open(BytesIO(data)) as w:
        if (w.getnchannels(), w.getsampwidth(), w.getframerate()) != (1, 2, SR):
            sys.exit(f"error: {name}: expected 44.1 kHz 16-bit mono")
        pcm = array.array("h", w.readframes(w.getnframes()))
    if sys.byteorder == "big":
        pcm.byteswap()
    return [v / 32768.0 for v in pcm]


def highpass(x, fc):
    """2nd-order Butterworth high-pass (RBJ biquad), causal, zero initial state."""
    w0 = 2.0 * math.pi * fc / SR
    alpha = math.sin(w0) / (2.0 * math.sqrt(0.5))
    c = math.cos(w0)
    a0 = 1.0 + alpha
    b0, b1, b2 = (1.0 + c) / 2.0 / a0, -(1.0 + c) / a0, (1.0 + c) / 2.0 / a0
    a1, a2 = -2.0 * c / a0, (1.0 - alpha) / a0
    y = []
    x1 = x2 = y1 = y2 = 0.0
    for xn in x:
        yn = b0 * xn + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        x2, x1, y2, y1 = x1, xn, y1, yn
        y.append(yn)
    return y


def rms(x):
    return math.sqrt(sum(v * v for v in x) / len(x)) if x else 0.0


def block_db(x):
    n = len(x) // BLOCK
    return [10.0 * math.log10(max(sum(v * v for v in x[k * BLOCK:(k + 1) * BLOCK]) / BLOCK, 1e-14))
            for k in range(n)]


def find_events(name, x):
    """Returns (y, press_onset, release_onset, noise_rms): y is the 30 Hz-filtered file, the
    onsets are sample indices into it and noise_rms is its level before the press.

    Coarse search on a 1 ms energy envelope of a 150 Hz high-passed copy:
    - press: the first block more than 15 dB above the file's noise floor (median of the
      first 4 ms) that rose at least 12 dB within 3 ms;
    - release: from 30 ms after the press, track the envelope's running minimum (3 ms
      smoothing); the first point at least 45 ms after the press that is 8 dB above that
      minimum and 12 dB above the floor is the release; walk back to the valley before it.
    Then refine to the sample: the first sample above 5x the noise RMS (press) or 4x the RMS
    of the decaying press tail just before it (release).
    """
    y = highpass(x, HP_HZ)
    e = block_db(highpass(x, DETECT_HZ))
    floor = statistics.median(e[:4])
    p0 = next((k for k in range(3, len(e)) if e[k] > floor + 15 and e[k] - e[k - 3] >= 12), None)
    if p0 is None:
        sys.exit(f"error: {name}: no keystroke found")
    lin = [10 ** (v / 10) for v in e]
    s = [10 * math.log10(sum(lin[max(k - 1, 0):k + 2]) / len(lin[max(k - 1, 0):k + 2]))
         for k in range(len(lin))]
    r0 = None
    run_min, arg_min = s[p0 + 30], p0 + 30
    for k in range(p0 + 30, len(s) - 5):
        if s[k] < run_min:
            run_min, arg_min = s[k], k
        if k >= p0 + 45 and s[k] - run_min >= 8 and s[k] > floor + 12:
            j = k
            while j > arg_min and s[j - 1] < s[j]:
                j -= 1
            r0 = j
            break
    if r0 is None:
        sys.exit(f"error: {name}: no release found")

    noise = rms(y[:max(BLOCK, (p0 - 2) * BLOCK)])
    lo, hi = max(0, (p0 - 3) * BLOCK), (p0 + 2) * BLOCK
    press = next((n for n in range(lo, hi) if abs(y[n]) > 5 * noise), p0 * BLOCK)
    tail = rms(y[(r0 - 6) * BLOCK:(r0 - 1) * BLOCK])
    lo, hi = (r0 - 2) * BLOCK, (r0 + 4) * BLOCK
    release = next((n for n in range(lo, hi) if abs(y[n]) > 4 * tail), r0 * BLOCK)
    return y, press, release, noise


def dot(a, b):
    return sum(map(operator.mul, a, b))


def press_peak_index(y, press_span):
    onset, end = press_span
    return max(range(onset, end), key=lambda n: abs(y[n]))


def press_window(y, press_span):
    """The DUP_WINDOW samples around the press peak (the alignment point for find_duplicates),
    or None if the take is too short around it."""
    k = press_peak_index(y, press_span)
    lo, hi = k - DUP_WINDOW[0] - DUP_MAX_LAG, k + DUP_WINDOW[1] + DUP_MAX_LAG
    return y[lo:hi] if lo >= 0 and hi <= len(y) else None


def similarity(wa, wb):
    """Largest normalized cross-correlation of two press windows within +-DUP_MAX_LAG samples
    (1.0 = the same waveform)."""
    a = wa[DUP_MAX_LAG:len(wa) - DUP_MAX_LAG]
    na = dot(a, a)
    best = 0.0
    for lag in range(2 * DUP_MAX_LAG + 1):
        b = wb[lag:lag + len(a)]
        nb = dot(b, b)
        if na > 0 and nb > 0:
            best = max(best, abs(dot(a, b)) / math.sqrt(na * nb))
    return best


def find_duplicates(windows):
    """Every pair (similarity, i, j) of keys in `windows` (number -> press window) whose
    similarity reaches DUPLICATE_NCC, most alike first."""
    numbers = sorted(windows)
    pairs = []
    for x, i in enumerate(numbers):
        for j in numbers[x + 1:]:
            s = similarity(windows[i], windows[j])
            if s >= DUPLICATE_NCC:
                pairs.append((s, i, j))
    return sorted(pairs, reverse=True)


def decay_end(y, onset, stop, noise):
    """End (exclusive) of the last 1 ms block in [onset, stop) that is 3 dB above the noise."""
    end = None
    k = onset
    while k < stop:
        if rms(y[k:min(k + BLOCK, stop)]) > noise * math.sqrt(2):
            end = min(k + BLOCK, stop)
        k += BLOCK
    return end if end is not None else min(onset + BLOCK, stop)


def cut(y, start, onset, end):
    seg = y[start:end]
    fade_in = onset - start
    for n in range(fade_in):
        seg[n] *= 0.5 * (1 - math.cos(math.pi * n / fade_in))
    fade = min(FADE_OUT, len(seg) // 2)
    for n in range(fade):
        seg[len(seg) - fade + n] *= 0.5 * (1 + math.cos(math.pi * (n + 1) / fade))
    return seg


def to_pcm(samples):
    pcm = array.array("h", (max(-32768, min(32767, round(v * 32768))) for v in samples))
    if sys.byteorder == "big":
        pcm.byteswap()
    return pcm


def write_wav(path, samples):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(to_pcm(samples).tobytes())


def leading_silence_ms(samples):
    first = next((n for n, v in enumerate(samples) if abs(v) >= SILENCE_THRESHOLD), len(samples))
    return first / SR * 1000


def stats_row(label, sounds):
    peaks = [db(max(abs(v) for v in s)) for s in sounds]
    return (f"| {label} | {len(sounds)} | {statistics.mean(len(s) for s in sounds) / SR * 1000:.1f} "
            f"| {max(peaks):.1f} | {statistics.median(peaks):.1f} "
            f"| {statistics.mean(db(rms(s)) for s in sounds):.1f} "
            f"| {max(leading_silence_ms(s) for s in sounds):.2f} |")


def render_preview(presses, releases, numbers):
    """About 2 s of typing from the pack's own samples, with a fixed made-up rhythm.
    numbers[i] is the source number of presses[i] and releases[i]."""
    rng = random.Random(20261002)
    strokes = ["key"] * 3 + ["space"] + ["key"] * 2 + ["space"] + ["key"] + ["enter"]
    press_ids = rng.sample(range(len(presses)), len(strokes))
    release_ids = rng.sample(range(len(releases)), len(strokes))
    events = []
    t = 0.0
    for i, kind in enumerate(strokes):
        if kind == "enter":
            t += 0.06  # a short pause before Enter
        dwell = rng.uniform(0.06, 0.10)
        events.append((round(t * SR), presses[press_ids[i]], press_ids[i], "press"))
        events.append((round((t + dwell) * SR), releases[release_ids[i]], release_ids[i], "release"))
        t += dwell + rng.uniform(0.08, 0.13)
    length = max(at + len(s) for at, s, _, _ in events) + round(0.03 * SR)
    out = [0.0] * length
    for at, s, _, _ in events:
        for n, v in enumerate(s):
            out[at + n] += v
    return out, [(at, f"{action}-{numbers[idx]:02d}") for at, _, idx, action in events]


def main():
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    zip_path = Path(sys.argv[1])
    pack = Path(sys.argv[2]) if len(sys.argv) == 3 else Path(__file__).resolve().parents[3] / "packs" / "office-classic"
    data = zip_path.read_bytes()
    if hashlib.sha256(data).hexdigest() != ZIP_SHA256:
        sys.exit(f"error: {zip_path} is not the expected zip (SHA-256 {ZIP_SHA256})")

    every = {}
    with zipfile.ZipFile(BytesIO(data)) as z:
        for n in range(1, SOURCE_COUNT + 1):
            name = f"Single Keys/keypress-{n:03d}.wav"
            raw = z.read(name)
            x = read_wav(raw, name)
            mean = sum(x) / len(x)
            y, press_onset, release_onset, noise = find_events(name, [v - mean for v in x])
            press_end = decay_end(y, press_onset, release_onset - PRE_ROLL, noise)
            release_end = decay_end(y, release_onset, min(len(y), release_onset + MAX_LEN), noise)
            every[n] = (name, hashlib.sha256(raw).hexdigest(), y, noise,
                        (press_onset, press_end), (release_onset, release_end))

    # Duplicates: a skipped keystroke must still be the take it repeats, and no two keystrokes
    # that are used may be one take.
    windows = {}
    for n, (name, _, y, _, press_span, _) in every.items():
        windows[n] = press_window(y, press_span)
        if windows[n] is None:
            sys.exit(f"error: {name}: too short around its press peak to compare")
    skipped = {}
    for n, same_as in sorted(SKIPPED.items()):
        shift = (press_peak_index(every[n][2], every[n][4])
                 - press_peak_index(every[same_as][2], every[same_as][4]))
        skipped[n] = (same_as, similarity(windows[n], windows[same_as]), shift)
        if skipped[n][1] < DUPLICATE_NCC or same_as in SKIPPED:
            sys.exit(f"error: keypress-{n:03d} is not a repeat of a used keystroke "
                     f"keypress-{same_as:03d}; check SKIPPED")
    numbers = [n for n in every if n not in SKIPPED]
    duplicates = find_duplicates({n: windows[n] for n in numbers})
    if duplicates:
        sys.exit("error: these keystrokes are one take (add one of each pair to SKIPPED):\n"
                 + "\n".join(f"  keypress-{i:03d} and keypress-{j:03d}: correlation {s:.4f}"
                             for s, i, j in duplicates))
    takes = [every[n] for n in numbers]

    def peak(y, span):
        return max(abs(v) for v in y[span[0]:span[1]])

    press_peaks = [peak(t[2], t[4]) for t in takes]
    press_peak = statistics.median(press_peaks)
    press_spread_db = db(max(press_peaks)) - db(min(press_peaks))
    loudest = max(max(peak(t[2], t[4]), peak(t[2], t[5])) for t in takes)
    gain_db = min(PRESS_TARGET_DB - db(press_peak), CEILING_DB - db(loudest))
    gain = 10 ** (gain_db / 20)

    def onset_after_gain(y, span):
        # A soft onset that stays under -50 dBFS after the gain is leading silence (the
        # loader would trim it): start the sample where it reaches -50 dBFS instead.
        onset, end = span
        return next((n for n in range(onset, end) if abs(y[n]) * gain >= SILENCE_THRESHOLD), onset)

    rows = []
    presses, releases = [], []
    for n, (name, sha, y, noise, press_span, release_span) in zip(numbers, takes):
        spans = []
        for span, out in ((press_span, presses), (release_span, releases)):
            onset = onset_after_gain(y, span)
            start = max(0, onset - PRE_ROLL)
            out.append([v * gain for v in cut(y, start, onset, span[1])])
            spans += [start, span[1]]
        rows.append((n, name, sha, *spans, db(noise)))

    sounds = pack / "sounds"
    sounds.mkdir(parents=True, exist_ok=True)
    written = set()
    for n, press, release in zip(numbers, presses, releases):
        for kind, samples in (("press", press), ("release", release)):
            write_wav(sounds / f"{kind}-{n:02d}.wav", samples)
            written.add(f"{kind}-{n:02d}.wav")
    for old in sounds.glob("*.wav"):
        if old.name not in written:  # left by an earlier build, e.g. a now skipped keystroke
            old.unlink()
    preview, preview_events = render_preview(presses, releases, numbers)
    if max(abs(v) for v in preview) > 10 ** (CEILING_DB / 20):
        sys.exit("error: preview would exceed the -1 dBFS ceiling")
    if len(preview) > 1.95 * SR:
        sys.exit("error: preview is longer than 1.95 s (the format allows 2 s per sound)")
    write_wav(pack / "preview.wav", preview)

    manifest = {
        "format": 1,
        "id": "office-classic",
        "name": "Office Classic",
        "version": "1.0.0",
        "author": "unicaegames, packaged by TakTak contributors",
        "license": "CC0-1.0",
        "description": "The soft, rounded clack of an everyday office keyboard, recorded on a "
                       "Cherry KC 1000 rubber-dome (membrane) board, with each key's return "
                       "on release.",
        "source": PAGE_URL,
        "attribution": "Keyboard sounds by unicaegames (CC0)",
        "preview": "preview.wav",
        "volume": VOLUME,
        "trim_silence": True,
        "variation": {"pitch": 0.01, "volume": 0.05},
        "groups": {
            "alphanumeric": {
                "press": [f"sounds/press-{n:02d}.wav" for n in numbers],
                "release": [f"sounds/release-{n:02d}.wav" for n in numbers],
            }
        },
    }
    (pack / "pack.json").write_text(json.dumps(manifest, indent=2) + "\n")

    stats = [
        "| Group | Count | Mean duration ms | Peak dBFS (max) | Peak dBFS (median) | RMS dBFS (mean) | Leading silence ms (max) |",
        "|---|---|---|---|---|---|---|",
        stats_row("alphanumeric.press", presses),
        stats_row("alphanumeric.release", releases),
        stats_row("preview", [preview]),
    ]
    shipped = [(p.relative_to(pack).as_posix(), p.stat().st_size, hashlib.sha256(p.read_bytes()).hexdigest())
               for p in sorted(sounds.glob("*.wav")) + [pack / "preview.wav"]]
    (pack / "SOURCES.md").write_text(sources_md(rows, gain_db, db(press_peak), db(loudest),
                                                press_spread_db, skipped, preview_events,
                                                len(preview), stats, shipped))
    print(f"gain {gain_db:+.2f} dB (median press peak before gain {db(press_peak):.1f} dBFS, "
          f"loudest sample {db(loudest):.1f} dBFS)")
    print("\n".join(stats))


def ms(n):
    return f"{n / SR * 1000:.2f}"


def sources_md(rows, gain_db, press_peak_db, loudest_db, press_spread_db, skipped, preview_events,
               preview_len, stats, shipped):
    table = "\n".join(
        f"| `Single Keys/{name.split('/')[-1]}` | `{sha}` | `press-{n:02d}.wav` | {ps}–{pe} ({ms(ps)}–{ms(pe)} ms) "
        f"| `release-{n:02d}.wav` | {rs}–{re} ({ms(rs)}–{ms(re)} ms) | {noise:.1f} |"
        for n, name, sha, ps, pe, rs, re, noise in rows)
    count = len(rows)
    skipped_text = "".join(
        f"""
- `Single Keys/keypress-{n:03d}.wav`: the same recording as `keypress-{same_as:03d}.wav`, placed {ms(abs(shift))} ms
  {"later" if shift >= 0 else "earlier"} in its file. The two files differ byte for byte, but their waveforms around the
  press peak correlate at {similarity_value:.5f} (distinct keystrokes of this set stay below 0.6), so using
  both would play that one stroke twice as often as any other. `build.py` checks this on
  every build, and stops if two keystrokes it uses correlate at {DUPLICATE_NCC} or more."""
        for n, (same_as, similarity_value, shift) in sorted(skipped.items()))
    timeline = ", ".join(f"{label} @ {ms(at)} ms" for at, label in preview_events)
    shipped_table = "\n".join(f"| `{path}` | {size} | `{sha}` |" for path, size, sha in shipped)
    return f"""# Office Classic: sources

Everything in this pack comes from one CC0 recording set, cut and levelled as described below.
To rebuild it byte for byte (DOWNLOADS is any folder; `fetch.sh` puts the zip in it and checks
its SHA-256):

```
tools/pack-sources/office-classic/fetch.sh DOWNLOADS/unicaegames
python3 tools/pack-sources/office-classic/build.py DOWNLOADS/unicaegames/{ZIP_NAME} packs/office-classic
```

`build.py` uses only the Python standard library (it reads the WAVs straight from the zip)
and writes every file in this folder, including this one.

## Source

| | |
|---|---|
| Work | "Keyboard Soundpack #1 [Typing and Single Keystrokes]" |
| Author | unicaegames |
| Source page | {PAGE_URL} |
| Direct download | {ZIP_URL} |
| Downloaded file | `{ZIP_NAME}`, 8,515,609 bytes |
| SHA-256 of the download | `{ZIP_SHA256}` |
| License | CC0-1.0 (Creative Commons Zero v1.0 Universal) |
| License proof | the source page's License(s) field reads "CC0" and links to {CC0_URL} |
| Date checked | {CHECKED} |
| Recording, per the author | keyboard: Cherry KC 1000 (a rubber-dome / membrane office board); microphone: Shure SM7B; post-processed with iZotope Neutron 2 |

License evidence, checked on {CHECKED}:

- The OpenGameArt page lists the license as CC0 (Creative Commons Public Domain Zero 1.0).
  The page was submitted on 2021-02-12; the author's 2026-01-27 update only fixed a Patreon
  link in the description and readme.
- The zip's `readme.txt` (dated 2026-02-01 inside the zip) says the sounds are free to use
  however you like. It adds no conditions that conflict with CC0.
- CC0 needs no attribution. The pack credits the author anyway: "Keyboard sounds by
  unicaegames (CC0)".

The zip contains 32 single keystrokes (`Single Keys/`), 10 human typing takes
(`Human Typing/`), 7 generated typing takes (`Generated Typing/`), `preview.ogg` and
`readme.txt`. It contains no executables.

## What is used, and why

Only files in `Single Keys/` are used: {count} of its 32. Each is a 250 ms, 44.1 kHz, 16-bit
mono WAV that holds one complete tap: the press, then, 40–125 ms later, the quieter sound of
the key coming back up. The build splits every tap into a press sample and a release sample,
so key-down and key-up each get the sound the keyboard really made. No release is synthesized.

Not used:
{skipped_text}
- `Human Typing/`: continuous typing in which keystrokes overlap, so clean single strokes
  cannot be cut from it without bleed from neighbouring keys.
- `Generated Typing/`: sequences assembled from recorded keystrokes; nothing new.
- `preview.ogg`: lossy, and a sequence.

The source does not say which key each recording is, so there are no per-key, space or Enter
sounds. Every key uses the pool of {count} presses and {count} releases in
`groups.alphanumeric`. Pack files keep the number of their source file, so the numbers of the
keystrokes that are not used are missing.

## Processing

For each source file, in this order (all positions are sample indices at 44,100 Hz into the
original file):

1. Subtract the file's mean (DC), then a 2nd-order Butterworth high-pass at 30 Hz (causal).
2. Find the press onset and the release onset: coarse search on a 1 ms energy envelope of a
   150 Hz high-passed copy (detection only), then refined to the first sample that rises
   clearly above the noise (press) or above the decaying press tail (release).
3. Press sample: from the press onset to 0.5 ms before the release onset, or earlier if the
   press has already decayed into the noise floor.
4. Release sample: from the release onset to the end of the last 1 ms block that is more than
   3 dB above the file's noise floor (measured before the press).
5. One gain for the whole pack: **{gain_db:+.2f} dB**. The median press peak was
   {press_peak_db:.1f} dBFS and the loudest sample in the pack {loudest_db:.1f} dBFS. Bringing
   the median press to -6 dBFS would have clipped the hardest taps, so the gain stops where the
   loudest sample reaches -1 dBFS. There is no per-file normalization: the natural
   differences between soft and hard taps (about {press_spread_db:.0f} dB of press peak level)
   are kept.
6. If an onset is still below -50 dBFS after the gain (a few releases swell in gently), it
   moves to the first sample that reaches -50 dBFS, since anything before that is leading
   silence that the loader would trim anyway. Each sample then starts 0.5 ms (22 samples)
   before its onset.
7. Fades: a raised-cosine fade-in over the 0.5 ms pre-roll, and an 8 ms raised-cosine fade-out
   that ends on an exact zero.
8. Written as 16-bit mono WAV at 44,100 Hz, the source rate (no resampling, no dither).
9. Loudness: `volume` in `pack.json` is **{VOLUME:.2f}** ({db(VOLUME):+.1f} dB), a playback gain the
   app applies to the whole pack, so no audio file changes. It matches typing loudness across
   the bundled packs, so switching packs does not jump in volume; the reference is the
   synthesized packs' {REFERENCE_LK:.1f} LK. Measured with
   `cargo run -p synth-packs --release -- loudness packs/office-classic`
   (`tools/synth-packs/src/loudness.rs`): the K-weighted (ITU-R BS.1770) energy of the first
   100 ms of the sample each alphanumeric key plays on press by default, power-averaged over
   the keys, on the samples as the app loads them at 48 kHz. The pack reads {LK_AS_BUILT:.1f} LK as built and {LK_AS_BUILT + db(VOLUME):.1f} LK
   at this volume. This volume also keeps the loudest true peak (4x oversampled, as resampling
   to the device rate can peak between samples) at or below -1 dBFS at the top of the ±5 %
   volume variation (the mixer clips hard at 0 dBFS). `build.py` writes the volume (`VOLUME`),
   so a rebuild keeps it.

| Original file | SHA-256 of original | Press file | Press slice, samples (ms) | Release file | Release slice, samples (ms) | Noise floor dBFS (RMS) |
|---|---|---|---|---|---|---|
{table}

### preview.wav

Rendered by `build.py` from this pack's own samples after the gain, with a fixed rhythm
(random seed 20261002): 3 keys, space, 2 keys, space, 1 key, a short pause, Enter. Dwell
60–100 ms, 80–130 ms from each release to the next press, mixed by simple addition, 16-bit
mono at 44,100 Hz, {ms(preview_len)} ms long. Since the pack has no dedicated space or Enter
sounds, those strokes use the same pool as every other key.

Timeline: {timeline}.

## Statistics

{chr(10).join(stats)}

Leading silence is measured as in `docs/pack-format.md`: time before the first sample at or
above -50 dBFS.

## Shipped files

SHA-256 of every audio file as shipped. `press-NN.wav` and `release-NN.wav` both come from
`Single Keys/keypress-0NN.wav` (table above); `preview.wav` is mixed from them. A rebuild
with `build.py` from the same zip reproduces them byte for byte, and removes any other WAV in
`sounds/`.

| File | Bytes | SHA-256 |
|---|---|---|
{shipped_table}
"""


if __name__ == "__main__":
    main()
