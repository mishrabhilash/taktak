#!/usr/bin/env python3
"""Rebuilds packs/linear-red from the original Wikimedia Commons recording.

    tools/pack-sources/linear-red/fetch.sh DOWNLOADS
    python3 tools/pack-sources/linear-red/build.py --downloads DOWNLOADS --out packs/linear-red

The source is "Typing on Keychron V1 Ultra (Red Linear Switch).wav" by C40115 (CC BY 4.0):
17.7 s of fast typing (a Monkeytype test) on red linear switches, recorded with a laptop's
built-in microphone. 48 kHz, 16-bit, two identical channels.

Needs ffmpeg (decoding, the mono mix and the 30 Hz high-pass) and numpy. Built with
ffmpeg 9.0.2, numpy 2.4.2 and Python 3.14.

The cut list is frozen in selection.json next to this script, so a rebuild is exact.
`--reselect` runs the automatic selection below again and rewrites selection.json.

Processing, in order (nothing else is done to the audio: no EQ, denoising or compression):

1. Decode with ffmpeg, mix to mono as (L+R)/2 (the two channels are identical) and apply a
   2-pole 30 Hz high-pass (removes DC and sub-audio rumble). 48 kHz is kept.
2. Cut each pack file from the decoded signal with the frozen sample range. Every cut starts
   24 samples (0.5 ms) before the stroke's attack, with a raised-cosine fade-in over those
   24 samples, and ends with a raised-cosine fade-out to exactly zero.
3. Gain. One gain for the whole pack makes the median letter press peak at -6 dBFS. The
   space-bar files (presses and releases alike) get a further shared attenuation so that the
   loudest of them peaks at -1 dBFS; their levels relative to each other are kept.
4. Write 16-bit PCM WAV, 48 kHz, mono (rounded, no dither); render preview.wav; write pack.json,
   whose "volume" (VOLUME) sets the pack's playback level (see VOLUME below).

Selection (--reselect). Analysis only uses a 1 kHz FFT high-passed copy (HF) of step 1's signal:

- Sound starts: in 0.25 ms RMS frames of HF (smoothed over 1 ms), a frame at or above -46 dB
  that is 10 dB above the quietest frame of the previous 4 ms starts a sound.
- Strokes: sounds whose wideband peak within 15 ms reaches -32 dBFS. The attack is the first
  sample reaching -26 dB of the stroke's HF peak, walking back from that peak (at most 15 ms,
  stopping at a 3 ms gap). If the 10 ms before that attack hold a click less than 20 dB below
  the peak, and that click starts at most 12 ms before the peak with 20 dB of quiet before it,
  the file starts at that click instead (a thumb meeting the space bar before it bottoms out).
- Cut end: where the 5 ms RMS stays under the noise floor + 4 dB (the floor is the 10th
  percentile of all 5 ms RMS values, -50.0 dBFS) for 10 ms, or 1 ms before the next sound
  starts, whichever is first (at most 150 ms; files are capped at 120 ms, releases at 80 ms).
- Clean (presses): peak -24 dBFS or louder; HF peak at least 20 dB over the HF peak of the
  10 ms before the attack; loudest HF hit within 12 ms of the attack; cut at least 40 ms long
  and at least 20 dB down at its end; nothing louder than -8 dB of the peak after the first
  15 ms.
- Space bar: its strokes are much darker than the letters (energy in 150-800 Hz over
  2.2-12 kHz in the first 20 ms after the attack). A space press is a stroke at least +4 dB
  dark and -14 dBFS or louder that is followed 40-120 ms later by a quieter stroke (at most
  25 dB down); the loudest such stroke is its release, and a stroke taken as a release is
  never used as a press. This finds 14 pairs (presses +4.7..+13.7 dB dark); of the other 191
  strokes at -24 dBFS or louder, 13 are +3 dB dark or more and are left out as unclear.
- Space releases are kept when their attack is clean (HF peak 20 dB over the 10 ms before,
  loudest hit within 3 ms), the cut is at least 15 ms long and at least 20 dB down at its
  end, and nothing after its first 15 ms is louder than -4 dB of its peak.
- Press or release for the letters: key-down and key-up are about equally loud in this take
  and overlap, and neither level nor timbre separates them. The one usable cue is timing: a
  stroke that follows a much quieter tick (8 dB or more down) by 15-70 ms. That tick is a
  finger meeting a keycap, or the previous key coming up just before this key goes down.
  Of the 14 space pairs, 9 presses show that tick and 4 releases do, so the cue picks
  key-downs about twice as often as key-ups; it is not certain. Only clean letter strokes
  (under +3 dB dark, not a space release) with that tick are kept, as press variants: 13
  of 178 letter strokes. No letter release is used; letter key-up is silent.
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
SPACE_MAX_PEAK_DB = -1.0
# Playback level, written to pack.json as "volume"; it changes no audio file. Measured on the
# built pack with
#     cargo run -p synth-packs --release -- loudness packs/linear-red
# (tools/synth-packs/src/loudness.rs): typing reads -26.6 LK at volume 1.0, so matching the
# synthesized packs' -25.8 LK would take 1.10. But the loudest file (a space press, true peak
# -1.0 dBFS) must stay at or below -1 dBFS at the top of the +6% volume variation, as the
# mixer clips hard at 0 dBFS, which allows at most 0.94: typing ends at -27.1 LK, 1.3 dB
# below the reference. Recorded in SOURCES.md; re-measure whenever the processing changes.
VOLUME = 0.94

SOURCE = "wikimedia-commons/Typing_on_Keychron_V1_Ultra_(Red_Linear_Switch).wav"
SOURCE_SHA256 = "85947c590e3831cf835609c11ceddce64dd5acd34eda08d24714c0bcc054ae4d"
SELECTION = Path(__file__).with_name("selection.json")

MANIFEST = {
    "format": 1,
    "id": "linear-red",
    "name": "Linear Red",
    "version": "1.0.0",
    "author": "C40115 (recording); packaged by TakTak contributors",
    "license": "CC-BY-4.0",
    "description": (
        "Red linear switches on a Keychron V1 Ultra, typed at speed in a typing test and "
        "recorded with a laptop's built-in microphone: short letter clacks with a little room "
        "noise, and a deeper space bar with its own key-up sound. Sliced stroke by stroke from "
        "a CC BY 4.0 recording on Wikimedia Commons."
    ),
    "source": (
        "https://commons.wikimedia.org/wiki/File:"
        "Typing_on_Keychron_V1_Ultra_(Red_Linear_Switch).wav"
    ),
    "attribution": (
        "Typing on Keychron V1 Ultra (Red Linear Switch) by C40115, CC BY 4.0, "
        "via Wikimedia Commons"
    ),
    "preview": "preview.wav",
    "volume": VOLUME,
    "trim_silence": True,
    "variation": {"pitch": 0.015, "volume": 0.06},
}


def decode(ffmpeg, path):
    """Mono float64 at 48 kHz, (L+R)/2, 2-pole 30 Hz high-pass."""
    raw = subprocess.run(
        [
            ffmpeg, "-v", "error", "-i", str(path),
            "-af", "pan=mono|c0=0.5*c0+0.5*c1,highpass=f=30:poles=2",
            "-f", "f32le", "-",
        ],
        capture_output=True,
        check=True,
    ).stdout
    return np.frombuffer(raw, dtype=np.float32).astype(np.float64)


def db(v):
    return 20.0 * np.log10(max(float(v), 1e-12))


def ms(n):
    return int(round(n * SR / 1000.0))


# ---------------------------------------------------------------------------------------------
# Selection (only run with --reselect; the result is frozen in selection.json)


def high_pass_fft(x, cutoff):
    spec = np.fft.rfft(x)
    freqs = np.fft.rfftfreq(len(x), 1.0 / SR)
    return np.fft.irfft(spec * (freqs >= cutoff), len(x))


def sound_starts(hf):
    """Sample positions where a new sound starts (see the module docstring)."""
    hop = 12  # 0.25 ms
    n = len(hf) // hop * hop
    power = (hf[:n].reshape(-1, hop) ** 2).mean(1)
    smooth = 10.0 * np.log10(np.convolve(power, np.ones(4) / 4, "same") + 1e-24)
    starts = []
    i = 16
    while i < len(smooth):
        window = smooth[i - 16 : i]
        if smooth[i] >= -46.0 and smooth[i] - window.min() >= 10.0:
            starts.append((i - 16 + int(np.argmin(window))) * hop)
            i = i + int(np.argmax(smooth[i : i + 12])) + 8
        else:
            i += 1
    return np.array(starts)


def walk_back(hf, peak_pos, threshold, gap, limit):
    """First sample at or above `threshold` before `peak_pos`, stopping at a quiet `gap`."""
    last = peak_pos
    j = peak_pos
    while j > peak_pos - limit:
        j -= 1
        if abs(hf[j]) >= threshold:
            last = j
        elif last - j > gap:
            break
    return last


def margin_db(hf, onset, hf_peak):
    return db(hf_peak) - db(np.abs(hf[onset - ms(10) : onset - PRE_ROLL]).max())


def find_strokes(x):
    hf = high_pass_fft(x, 1000.0)
    starts = sound_starts(hf)
    rms5 = np.sqrt((x[: len(x) // 240 * 240].reshape(-1, 240) ** 2).mean(1))
    noise_db = float(np.percentile(20.0 * np.log10(rms5 + 1e-12), 10))
    nfft = 1024
    freqs = np.fft.rfftfreq(nfft, 1.0 / SR)
    strokes = []
    for s0 in starts:
        window = slice(s0, s0 + ms(15))
        peak_pos = s0 + int(np.argmax(np.abs(x[window])))
        peak = abs(x[peak_pos])
        if db(peak) < -32.0:
            continue
        hf_pos = s0 + int(np.argmax(np.abs(hf[window])))
        hf_peak = abs(hf[hf_pos])
        threshold = hf_peak * 10 ** (-26 / 20)
        attack = walk_back(hf, hf_pos, threshold, ms(3), ms(15))
        if strokes and attack - strokes[-1]["attack"] < ms(15):
            continue
        onset, precursor = attack, False
        if margin_db(hf, attack, hf_peak) < 20.0:
            early = walk_back(hf, hf_pos, threshold, ms(8), ms(14))
            if (
                early < attack
                and hf_pos - early <= ms(12)
                and margin_db(hf, early, hf_peak) >= 20.0
            ):
                onset, precursor = early, True
        later = starts[starts > onset + ms(15)]
        next_start = int(later[0]) if len(later) else len(x)
        decay_end = None
        for q in range(onset + ms(20), min(onset + ms(150), len(x) - 480), 48):
            a = np.sqrt((x[q : q + 240] ** 2).mean())
            b = np.sqrt((x[q + 240 : q + 480] ** 2).mean())
            if db(a) < noise_db + 4 and db(b) < noise_db + 4:
                decay_end = q + 240
                break
        natural = decay_end is not None and decay_end <= next_start - 48
        cut_end = decay_end if natural else min(onset + ms(150), next_start - 48)
        seg = np.pad(x[attack : attack + ms(20)], (0, nfft - ms(20)))
        p = np.abs(np.fft.rfft(seg * np.hanning(nfft))) ** 2
        dark = 10 * np.log10(
            p[(freqs >= 150) & (freqs < 800)].sum() / p[(freqs >= 2200) & (freqs < 12000)].sum()
        )
        after = x[onset + ms(15) : cut_end]
        strokes.append(
            {
                "attack": int(attack),
                "onset": int(onset),
                "peak_db": db(peak),
                "hf_lead_ms": (hf_pos - onset) / SR * 1000,
                "margin_db": margin_db(hf, onset, hf_peak),
                "precursor": precursor,
                "cut_end": int(cut_end),
                "natural_end": bool(natural),
                "cut_ms": (cut_end - onset) / SR * 1000,
                "tail_db": db(np.sqrt((x[cut_end - 240 : cut_end] ** 2).mean())) - db(peak),
                "later_hit_db": (db(np.abs(after).max()) - db(peak)) if len(after) else -99.0,
                "dark_db": float(dark),
            }
        )
    return strokes, noise_db


def select(x):
    strokes, noise_db = find_strokes(x)
    for i, s in enumerate(strokes):
        prev = strokes[i - 1] if i else None
        dt = (s["attack"] - prev["attack"]) / SR * 1000 if prev else 999.0
        s["tick_before"] = bool(prev and 15 <= dt <= 70 and prev["peak_db"] <= s["peak_db"] - 8)

    def clean_press(s):
        return (
            s["peak_db"] >= -24
            and s["margin_db"] >= 20
            and s["cut_ms"] >= 40
            and s["tail_db"] <= -20
            and s["later_hit_db"] <= -8
            and s["hf_lead_ms"] <= 12
        )

    # Space bar first: a dark, loud stroke is a space press and the loudest quieter stroke
    # 40-120 ms after it is its release. A stroke taken as a release is never a press.
    space_presses, space_releases, released = [], [], set()
    for i, s in enumerate(strokes):
        if i in released or s["dark_db"] < 4 or s["peak_db"] < -14:
            continue
        partners = [
            j
            for j in range(i + 1, min(i + 8, len(strokes)))
            if 40 <= (strokes[j]["attack"] - s["attack"]) / SR * 1000 <= 120
            and s["peak_db"] - 25 <= strokes[j]["peak_db"] < s["peak_db"]
        ]
        if not partners:
            continue
        r = max(partners, key=lambda j: strokes[j]["peak_db"])
        released.add(r)
        if clean_press(s):
            space_presses.append(i)
        rs = strokes[r]
        if (
            rs["margin_db"] >= 20
            and rs["hf_lead_ms"] <= 3
            and rs["cut_ms"] >= 15
            and rs["tail_db"] <= -20
            and rs["later_hit_db"] <= -4
        ):
            space_releases.append(r)

    letters = [
        i
        for i, s in enumerate(strokes)
        if i not in released
        and s["dark_db"] < 3
        and not s["precursor"]
        and clean_press(s)
        and s["tick_before"]
    ]

    cuts = []

    def add(prefix, indices, max_ms):
        for n, i in enumerate(indices, 1):
            s = strokes[i]
            end = min(s["cut_end"], s["onset"] + ms(max_ms))
            natural = s["natural_end"] and end == s["cut_end"]
            length = end - (s["onset"] - PRE_ROLL)
            fade = min(ms(8) if natural else ms(6), length // 3)
            cuts.append(
                {
                    "file": f"{prefix}-{n:02d}.wav",
                    "start": s["onset"] - PRE_ROLL,
                    "end": int(end),
                    "fade_out": int(fade),
                    "source_peak_dbfs": round(s["peak_db"], 2),
                    "attack_ms": round(s["onset"] / SR * 1000, 2),
                    "thumb_click_included": s["precursor"],
                }
            )

    add("press", letters, 120)
    add("space-press", space_presses, 120)
    add("space-release", space_releases, 80)
    return {"noise_floor_db": round(noise_db, 2), "strokes_found": len(strokes), "cuts": cuts}


# ---------------------------------------------------------------------------------------------
# Build


def fade(n, rising):
    """Raised cosine over n samples; the end that touches silence is exactly 0."""
    w = 0.5 - 0.5 * np.cos(np.pi * np.arange(n) / (n - 1))
    return w if rising else w[::-1]


def cut(signal, c):
    x = signal[c["start"] : c["end"]].copy()
    x[:PRE_ROLL] *= fade(PRE_ROLL, rising=True)
    x[-c["fade_out"] :] *= fade(c["fade_out"], rising=False)
    return x


def to_pcm16(x):
    return np.clip(np.round(x * 32767.0), -32768, 32767).astype("<i2")


def write_wav(path, x):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(to_pcm16(x).tobytes())


def sounds(names):
    return [f"sounds/{n}" for n in names]


def render_preview(clips, letters, space_p, space_r):
    """About 1.9 s of typing: 4 letters, Space, 3 letters, Enter. Letters have no key-up
    sound; Space and Enter (which uses the space-bar strokes) get their release."""
    rng = random.Random(20261002)
    plan = ["a", "a", "a", "a", "space", "a", "a", "a", "enter"]
    events = []
    t = 0.0
    n = {"a": 0, "space": 0, "enter": 0}
    for kind in plan:
        i = n[kind]
        n[kind] += 1
        dwell = rng.uniform(0.060, 0.100)
        if kind == "a":
            events.append((t, clips[letters[(i * 5 + 2) % len(letters)]]))
        else:
            k = i if kind == "space" else i + 2
            events.append((t, clips[space_p[k % len(space_p)]]))
            events.append((t + dwell, clips[space_r[k % len(space_r)]]))
        t += dwell + rng.uniform(0.070, 0.150)
    length = int(max(start * SR + len(c) for start, c in events)) + 1
    out = np.zeros(length)
    for start, c in events:
        s = int(round(start * SR))
        out[s : s + len(c)] += c
    return out


def print_stats(clips, groups):
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
            f"| {label} | {len(q)} | {dur:.1f} | {np.median(peaks):.1f} "
            f"({min(peaks):.1f}..{max(peaks):.1f}) | {rms:.1f} | {lead:.2f} |"
        )


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--downloads", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--ffmpeg", default="ffmpeg")
    ap.add_argument("--reselect", action="store_true", help="rerun the stroke selection")
    args = ap.parse_args()

    path = args.downloads / SOURCE
    sha = hashlib.sha256(path.read_bytes()).hexdigest()
    print(f"source sha256 {sha}  {SOURCE}")
    if sha != SOURCE_SHA256:
        raise SystemExit(f"unexpected source file (sha256 {sha}, expected {SOURCE_SHA256})")
    signal = decode(args.ffmpeg, path)

    if args.reselect or not SELECTION.exists():
        selection = select(signal)
        SELECTION.write_text(json.dumps(selection, indent=1) + "\n")
        print(f"wrote {SELECTION}")
    selection = json.loads(SELECTION.read_text())
    cuts = selection["cuts"]

    clips = {c["file"]: cut(signal, c) for c in cuts}
    letters = [c["file"] for c in cuts if c["file"].startswith("press-")]
    space_p = [c["file"] for c in cuts if c["file"].startswith("space-press-")]
    space_r = [c["file"] for c in cuts if c["file"].startswith("space-release-")]
    space = space_p + space_r

    median_peak = float(np.median([db(np.abs(clips[n]).max()) for n in letters]))
    gain_db = TARGET_MEDIAN_PRESS_PEAK_DB - median_peak
    space_max = max(db(np.abs(clips[n]).max()) for n in space) + gain_db
    space_db = min(0.0, SPACE_MAX_PEAK_DB - space_max)
    for n in clips:
        g = gain_db + (space_db if n in space else 0.0)
        clips[n] = clips[n] * 10.0 ** (g / 20.0)
    print(
        f"pack gain {gain_db:+.2f} dB (median letter press peak before gain {median_peak:.2f} dBFS), "
        f"space bar {space_db:+.2f} dB more"
    )
    assert max(np.abs(c).max() for c in clips.values()) < 0.95, "clipping"

    sounds_dir = args.out / "sounds"
    sounds_dir.mkdir(parents=True, exist_ok=True)
    for old in sounds_dir.glob("*.wav"):
        old.unlink()
    for name, c in clips.items():
        write_wav(sounds_dir / name, c)

    preview = render_preview(clips, letters, space_p, space_r)
    assert np.abs(preview).max() < 1.0, "preview clips"
    assert len(preview) < 1.95 * SR, "preview must stay under 1.95 s"
    write_wav(args.out / "preview.wav", preview)
    print(f"preview {len(preview) / SR:.2f} s, peak {db(np.abs(preview).max()):.2f} dBFS")

    manifest = dict(MANIFEST)
    manifest["groups"] = {
        "alphanumeric": {"press": sounds(letters)},
        "space": {"press": sounds(space_p), "release": sounds(space_r)},
        "enter": {"press": sounds(space_p), "release": sounds(space_r)},
    }
    (args.out / "pack.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")

    print_stats(
        clips,
        [
            ("alphanumeric.press", letters),
            ("space/enter.press", space_p),
            ("space/enter.release", space_r),
        ],
    )
    print()
    print("| File | Source samples @48 kHz | Attack (s) | Length (ms) | Fade-out (ms) | Source peak (dBFS) |")
    print("|---|---|---|---|---|---|")
    for c in cuts:
        note = " (thumb click included)" if c["thumb_click_included"] else ""
        print(
            f"| `sounds/{c['file']}` | {c['start']}–{c['end']} | {c['attack_ms'] / 1000:.3f}{note} "
            f"| {(c['end'] - c['start']) / SR * 1000:.1f} | {c['fade_out'] / SR * 1000:.1f} "
            f"| {c['source_peak_dbfs']:.1f} |"
        )
    print()
    for path in sorted(sounds_dir.glob("*.wav")) + [args.out / "preview.wav"]:
        sha = hashlib.sha256(path.read_bytes()).hexdigest()
        print(f"| `{path.relative_to(args.out)}` | {path.stat().st_size} | `{sha}` |")


if __name__ == "__main__":
    main()
