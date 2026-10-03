#!/usr/bin/env python3
"""Rebuilds packs/key-press from the original OpenGameArt downloads.

    tools/pack-sources/key-press/fetch.sh DOWNLOADS
    python3 tools/pack-sources/key-press/build.py --downloads DOWNLOADS --out packs/key-press

Needs ffmpeg (for decoding, the mono mix, the 30 Hz high-pass and resampling)
and numpy. Built with ffmpeg 9.0.2, numpy 2.4.2 and Python 3.14.

Processing, in order (see packs/key-press/SOURCES.md for the per-file table):

1. Decode. eklee FLAC (48 kHz, 24-bit, stereo) is mixed to mono as (L + R) / 2.
   bluszcz OGG (44.1 kHz, mono) is resampled to 48 kHz. Both get a 2-pole
   30 Hz high-pass (removes DC and sub-audio rumble), applied to the whole file.
2. Slice each output file from its source with the frozen sample ranges in
   SLICES (48 kHz sample indices into the decoded signal). Each slice starts
   0.5 ms (24 samples) before the onset, with a raised-cosine fade-in over those
   24 samples, and ends with a raised-cosine fade-out to exactly zero.
3. One pack-wide gain: the median press peak becomes -6 dBFS.
4. Write 16-bit PCM WAV, 48 kHz, mono; render preview.wav; write pack.json, whose
   "volume" (VOLUME) matches the pack's typing loudness to the other bundled packs.

The ranges were picked once by onset detection (see SOURCES.md) and are frozen
here so that a rebuild is exact.
"""

import argparse
import hashlib
import json
import random
import subprocess
import wave
from pathlib import Path

import numpy as np

SR = 48_000
PRE_ROLL = 24  # 0.5 ms
TARGET_MEDIAN_PRESS_PEAK_DB = -6.0
# Playback level, written to pack.json as "volume"; it changes no audio file. It matches the
# pack's typing loudness to TakTak's reference typing level, -25.8 LK, so switching packs does not jump
# in volume. Measured on the built pack with
#     cargo run -p synth-packs --release -- loudness packs/key-press
# (tools/synth-packs/src/loudness.rs): -26.8 LK at volume 1.0, -25.7 LK at VOLUME. Recorded
# in SOURCES.md; re-measure whenever the processing here changes.
VOLUME = 1.14

EKLEE_DIR = "oga-eklee-key-presses/extracted/eklee-KeyPresses-cc0-opengameart/flac48000"
BLUSZCZ_DIR = "oga-bluszcz-mechanical-keyboard"

SOURCES = {
    "Mac01": f"{EKLEE_DIR}/eklee-KeyPressMac01.flac",
    "Mac02": f"{EKLEE_DIR}/eklee-KeyPressMac02.flac",
    "Mac03": f"{EKLEE_DIR}/eklee-KeyPressMac03.flac",
    "Mac04": f"{EKLEE_DIR}/eklee-KeyPressMac04.flac",
    "Mac05": f"{EKLEE_DIR}/eklee-KeyPressMac05.flac",
    "Mac06": f"{EKLEE_DIR}/eklee-KeyPressMac06.flac",
    "Mac07": f"{EKLEE_DIR}/eklee-KeyPressMac07.flac",
    "Old01": f"{EKLEE_DIR}/eklee-KeyPressOld01.flac",
    "Old02": f"{EKLEE_DIR}/eklee-KeyPressOld02.flac",
    "Old03": f"{EKLEE_DIR}/eklee-KeyPressOld03.flac",
    "Old04": f"{EKLEE_DIR}/eklee-KeyPressOld04.flac",
    "keyboard01": f"{BLUSZCZ_DIR}/keyboard01_0.ogg",
    "keyboard02": f"{BLUSZCZ_DIR}/keyboard02_0.ogg",
}

# (output file, source, start sample, end sample (exclusive), fade-out samples)
SLICES = [
    # "light": the eklee "Mac" keyboard. Press = onset up to the release onset.
    ("light-press-01.wav", "Mac01", 1004, 5431, 288),
    ("light-press-02.wav", "Mac02", 1075, 11819, 288),
    ("light-press-03.wav", "Mac03", 1028, 11293, 384),  # no clear release: whole stroke
    ("light-press-04.wav", "Mac04", 425, 5097, 288),
    ("light-press-05.wav", "Mac05", 254, 4526, 288),
    ("light-press-06.wav", "Mac06", 240, 4538, 288),
    ("light-press-07.wav", "Mac07", 681, 4960, 288),
    ("light-release-01.wav", "Mac01", 5431, 11328, 384),  # ends before a stray tick at 239 ms
    ("light-release-02.wav", "Mac02", 11819, 15328, 384),
    ("light-release-04.wav", "Mac04", 5097, 11175, 384),
    ("light-release-05.wav", "Mac05", 4526, 12131, 384),
    ("light-release-06.wav", "Mac06", 4538, 7628, 384),
    ("light-release-07.wav", "Mac07", 4960, 13067, 384),
    # "deep": the eklee "Old" keyboard.
    ("deep-press-01.wav", "Old01", 1044, 5079, 576),  # longer fade: cuts through a 60 Hz ring
    ("deep-press-02.wav", "Old02", 551, 15497, 384),  # no clear release: whole stroke
    ("deep-press-03.wav", "Old03", 226, 10710, 384),  # no clear release: whole stroke
    ("deep-press-04.wav", "Old04", 331, 14913, 384),  # no clear release: whole stroke
    ("deep-release-01.wav", "Old01", 5079, 15568, 384),
    # "clack": single strokes cut from bluszcz's two typing clips. Each starts 0.5 ms before
    # its main attack (the run of 1 ms blocks within -18 dB of the stroke's peak that leads
    # up to it); the quieter sound before that, 6-18 ms of it, is left out, so the click
    # plays at once.
    ("clack-01.wav", "keyboard01", 696, 3574, 288),
    ("clack-02.wav", "keyboard01", 21665, 24698, 288),
    ("clack-03.wav", "keyboard02", 1914, 5603, 288),
    ("clack-04.wav", "keyboard02", 5993, 9880, 288),
    ("clack-05.wav", "keyboard02", 16068, 19293, 288),
    ("clack-06.wav", "keyboard02", 25250, 30889, 384),
]

LIGHT_PRESS = [f"light-press-0{i}.wav" for i in range(1, 8)]
LIGHT_RELEASE = [f"light-release-0{i}.wav" for i in (1, 2, 4, 5, 6, 7)]
DEEP_PRESS = [f"deep-press-0{i}.wav" for i in range(1, 5)]
DEEP_RELEASE = ["deep-release-01.wav"]
CLACK = [f"clack-0{i}.wav" for i in range(1, 7)]
PRESS_FILES = LIGHT_PRESS + DEEP_PRESS + CLACK


def sounds(names):
    return [f"sounds/{n}" for n in names]


MANIFEST = {
    "format": 1,
    "id": "key-press",
    "name": "Key Press",
    "version": "1.0.0",
    "author": "eklee, qubodup and bluszcz; packaged by TakTak contributors",
    "license": "CC-BY-3.0",
    "description": (
        "Real recorded key presses from three keyboards: light, crisp ticks on the letters, "
        "a deeper, older board thumping under Space, Backspace and the modifiers, and a sharp "
        "mechanical clack on Enter."
    ),
    "source": (
        "https://opengameart.org/content/single-key-press-sounds ; "
        "https://opengameart.org/content/mechanical-keyboard-sound"
    ),
    "attribution": (
        '"Single Key Press Sounds" by eklee, qubodup '
        "(https://opengameart.org/content/single-key-press-sounds) and "
        '"Mechanical keyboard sound" by bluszcz '
        "(https://opengameart.org/content/mechanical-keyboard-sound), both CC BY 3.0 "
        "(https://creativecommons.org/licenses/by/3.0/). Sliced, mixed to mono and "
        "level-adjusted by TakTak contributors."
    ),
    "preview": "preview.wav",
    "volume": VOLUME,
    "trim_silence": True,
    "variation": {"pitch": 0.015, "volume": 0.06},
    "groups": {
        "alphanumeric": {"press": sounds(LIGHT_PRESS), "release": sounds(LIGHT_RELEASE)},
        "space": {"press": sounds(DEEP_PRESS[:2]), "release": sounds(DEEP_RELEASE)},
        "backspace": {"press": sounds(DEEP_PRESS[2:]), "release": sounds(DEEP_RELEASE)},
        "modifiers": {"press": sounds(DEEP_PRESS), "release": sounds(DEEP_RELEASE)},
        "enter": {"press": sounds(CLACK)},
    },
}


def decode(ffmpeg, path):
    """Decodes to mono float64 at 48 kHz with a 30 Hz high-pass."""
    if path.suffix == ".flac":
        af = "pan=mono|c0=0.5*c0+0.5*c1,highpass=f=30:poles=2"
    else:
        af = (
            "highpass=f=30:poles=2,"
            "aresample=48000:filter_size=128:phase_shift=10:cutoff=0.97:"
            "filter_type=kaiser:kaiser_beta=9"
        )
    raw = subprocess.run(
        [ffmpeg, "-v", "error", "-i", str(path), "-af", af, "-ac", "1", "-f", "f32le", "-"],
        capture_output=True,
        check=True,
    ).stdout
    return np.frombuffer(raw, dtype=np.float32).astype(np.float64)


def fade(n, rising):
    """Raised cosine over n samples; the end that touches silence is exactly 0."""
    w = 0.5 - 0.5 * np.cos(np.pi * np.arange(n) / (n - 1))
    return w if rising else w[::-1]


def cut(signal, start, end, fade_out):
    x = signal[start:end].copy()
    x[:PRE_ROLL] *= fade(PRE_ROLL, rising=True)
    x[-fade_out:] *= fade(fade_out, rising=False)
    return x


def to_pcm16(x):
    return np.clip(np.round(x * 32767.0), -32768, 32767).astype("<i2")


def write_wav(path, x):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(to_pcm16(x).tobytes())


def db(v):
    return 20.0 * np.log10(max(float(v), 1e-12))


def render_preview(clips):
    """About 2 s of typing (under the 2 s sample limit): 3 letters, space, 3 letters, Enter."""
    rng = random.Random(20261002)
    plan = ["a", "a", "a", "space", "a", "a", "a", "enter"]
    nxt = {"a": 0, "space": 0, "enter": 0}
    pools = {
        "a": (LIGHT_PRESS, LIGHT_RELEASE),
        "space": (DEEP_PRESS[:2], DEEP_RELEASE),
        # Enter has no release of its own; at runtime it falls back to the letters' releases.
        "enter": (CLACK, LIGHT_RELEASE),
    }
    events = []
    t = 0.0
    for kind in plan:
        presses, releases = pools[kind]
        i = nxt[kind]
        nxt[kind] += 1
        dwell = rng.uniform(0.060, 0.120)
        events.append((t, clips[presses[(i * 3) % len(presses)]]))
        events.append((t + dwell, clips[releases[(i * 5) % len(releases)]]))
        t += dwell + rng.uniform(0.080, 0.200)
    length = int(max(start * SR + len(c) for start, c in events)) + 1
    out = np.zeros(length)
    for start, c in events:
        s = int(round(start * SR))
        out[s : s + len(c)] += c
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--downloads", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--ffmpeg", default="ffmpeg")
    args = ap.parse_args()

    decoded = {}
    for key, rel in SOURCES.items():
        path = args.downloads / rel
        decoded[key] = decode(args.ffmpeg, path)
        sha = hashlib.sha256(path.read_bytes()).hexdigest()
        print(f"source {key:10s} sha256 {sha}  {rel}")

    clips = {name: cut(decoded[src], s, e, f) for name, src, s, e, f in SLICES}

    median_peak = np.median([db(np.abs(clips[n]).max()) for n in PRESS_FILES])
    gain_db = TARGET_MEDIAN_PRESS_PEAK_DB - median_peak
    gain = 10.0 ** (gain_db / 20.0)
    clips = {n: c * gain for n, c in clips.items()}
    print(f"pack gain {gain_db:+.2f} dB (median press peak before gain {median_peak:.2f} dBFS)")
    assert max(np.abs(c).max() for c in clips.values()) < 1.0, "clipping"

    sounds_dir = args.out / "sounds"
    sounds_dir.mkdir(parents=True, exist_ok=True)
    for name, c in clips.items():
        write_wav(sounds_dir / name, c)

    preview = render_preview(clips)
    assert np.abs(preview).max() < 1.0, "preview clips"
    assert len(preview) < 1.95 * SR, "preview must stay under the 2 s sample limit"
    write_wav(args.out / "preview.wav", preview)
    print(f"preview {len(preview) / SR:.2f} s, peak {db(np.abs(preview).max()):.2f} dBFS")

    (args.out / "pack.json").write_text(json.dumps(MANIFEST, indent=2, ensure_ascii=False) + "\n")

    print_stats(clips)

    print()
    for path in sorted(sounds_dir.glob("*.wav")) + [args.out / "preview.wav"]:
        sha = hashlib.sha256(path.read_bytes()).hexdigest()
        print(f"| `{path.relative_to(args.out)}` | {path.stat().st_size} | `{sha}` |")


def print_stats(clips):
    """Per group: count, mean duration, peak, RMS and leading silence (to -50 dBFS)."""
    groups = [
        ("alphanumeric.press", LIGHT_PRESS),
        ("alphanumeric.release", LIGHT_RELEASE),
        ("space/backspace/modifiers.press", DEEP_PRESS),
        ("space/backspace/modifiers.release", DEEP_RELEASE),
        ("enter.press", CLACK),
    ]
    print()
    print("| group | count | mean ms | peak dBFS median (min..max) | RMS dBFS (mean) | max leading silence ms |")
    print("|---|---|---|---|---|---|")
    for label, names in groups:
        q = [to_pcm16(clips[n]).astype(np.float64) / 32768.0 for n in names]
        dur = np.mean([len(x) / SR * 1000 for x in q])
        peaks = [db(np.abs(x).max()) for x in q]
        rms = np.mean([db(np.sqrt(np.mean(x * x))) for x in q])
        lead = max(int(np.argmax(np.abs(x) >= 0.00316)) / SR * 1000 for x in q)
        print(
            f"| {label} | {len(q)} | {dur:.1f} | {np.median(peaks):.1f} ({min(peaks):.1f}..{max(peaks):.1f}) "
            f"| {rms:.1f} | {lead:.2f} |"
        )


if __name__ == "__main__":
    main()
