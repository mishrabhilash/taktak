#!/usr/bin/env python3
"""Rebuilds packs/ui-clicks from Kenney's "UI Audio" and "Interface Sounds" zips (CC0).

    tools/pack-sources/ui-clicks/fetch.sh DOWNLOADS
    python3 tools/pack-sources/ui-clicks/build.py --downloads DOWNLOADS --out packs/ui-clicks

DOWNLOADS/kenney/ must hold kenney_ui-audio.zip and kenney_interface-sounds.zip (fetch.sh
checks their SHA-256). Only the .ogg files named in PICKS are read, straight from the zips;
nothing is extracted to disk and nothing from the zips is executed.

Needs ffmpeg (decoding, the mono mix and the 30 Hz high-pass) and numpy. Built with
ffmpeg 9.0.2, numpy 2.4.2 and Python 3.14.

Processing, in order (nothing else is done to the audio: no EQ, pitch or time changes):

1. Decode the Ogg Vorbis file with ffmpeg at its own rate (44.1 kHz for every file used).
   Stereo files are mixed to mono as (L+R)/2. A 2-pole 30 Hz high-pass removes DC.
2. Find the part to keep. A "click" is the whole file, or for the two-click switch files
   (an "on" click followed by a softer "off" click) the part before or after the quiet gap
   between them (SPLIT_MS, a point in that gap). Within that part:
   - attack: the first sample at or above -26 dB of the part's peak, and at or above
     -50 dBFS once the gain of step 4 is applied (the loader's leading-silence threshold;
     this only moves the start of the quietest file, modifier-release-02). Anything quieter
     before it (a faint lead-in or silence) is dropped, so every sound starts at once;
   - end: 2 ms into the first 10 ms (after the peak) in which every 1 ms window's RMS is
     below -45 dB of the peak, or the end of the part, and at most the group's length cap
     (MAX_MS; every "off" click uses the "release" cap).
3. Cut: the file starts 0.5 ms (22 samples) before the attack, with a raised-cosine fade-in
   over those 22 samples (zeros are used where the source has no samples before the attack),
   and ends with a raised-cosine fade-out (8 ms, or a third of the length if shorter) to
   exactly zero.
4. Level. Kenney peak-normalized each file, so levels are set per group: one gain per group
   brings the median peak of that group's presses to its target (TARGET_DB), or, for Enter,
   the loudness of its presses to a set distance below the letters' (LOUDNESS_OFFSET_DB):
   its two-note blips are sustained tones, which a peak target makes far louder than the
   short clicks of the other groups. Loudness is measured as synth-packs measures typing
   loudness: the K-weighted (ITU-R BS.1770) energy of each press's first 100 ms, power-
   averaged over the group. Within a group the files keep their level relative to each
   other. The "off" clicks take the gain of the group that plays them: the letters' releases
   get the letters' gain, the same gain as the "on" click of their own file, so they stay as
   much softer as they are in the source. Enter, Backspace and the modifiers are quieter
   than the letters, so each gets its own copy of the three off clicks at its own gain;
   without them those keys would fall back to the letters' releases, and a key-up would be
   louder than its key-down. For a group levelled by loudness, that gain is the letters'
   gain moved by the group's loudness offset, so its key-up sits as far below its key-down
   as the letters' key-ups below theirs (the group's own gain would bury the clicks under a
   sustained tone's level). Space is within 1.5 dB of the letters and uses their releases.
   The build stops if any group's presses end up more than MAX_OVER_LETTERS_DB louder than
   the letters'.
5. Write 16-bit PCM WAV, 44.1 kHz, mono (rounded, no dither); render preview.wav; write
   pack.json, whose "volume" (VOLUME) matches the pack's typing loudness to the other
   bundled packs.
"""

import argparse
import hashlib
import json
import random
import subprocess
import wave
import zipfile
from pathlib import Path

import numpy as np

SR = 44_100
PRE_ROLL = 22  # 0.5 ms
TRIM_LEVEL = 10 ** (-50 / 20)  # the loader's leading-silence threshold

ZIPS = {
    "ui": ("kenney/kenney_ui-audio.zip", "946fc23a63d535d693eb31b2eabb80c8c28d6351e2186b344ceb71b2cb1d5eb6"),
    "if": (
        "kenney/kenney_interface-sounds.zip",
        "f2193d072726d6758a5f7871b2dcc54dcce0d5c35c6f0a62f92549b327c81232",
    ),
}

# Split points (ms from the file start) in the files that hold an "on" and an "off" click:
# the "on" part is before it, the "off" part after it. Each is just before the off click's
# main burst, so faint pre-clicks of the off movement (up to 28 ms earlier, 14 dB or more
# below the off click) stay out of the release and it starts at once.
SPLIT_MS = {"Audio/switch33.ogg": 315, "Audio/switch34.ogg": 268, "Audio/switch38.ogg": 195}

# (pack file, group, zip, member, part) — part is "all", "on" or "off".
PICKS = [
    # Letters, digits and punctuation: crisp switch clicks (UI Audio).
    ("click-01.wav", "letters", "ui", "Audio/switch2.ogg", "all"),
    ("click-02.wav", "letters", "ui", "Audio/switch4.ogg", "all"),
    ("click-03.wav", "letters", "ui", "Audio/switch9.ogg", "all"),
    ("click-04.wav", "letters", "ui", "Audio/switch12.ogg", "all"),
    ("click-05.wav", "letters", "ui", "Audio/switch19.ogg", "all"),
    ("click-06.wav", "letters", "ui", "Audio/switch26.ogg", "all"),
    ("click-07.wav", "letters", "ui", "Audio/switch28.ogg", "all"),
    ("click-08.wav", "letters", "ui", "Audio/switch31.ogg", "all"),
    ("click-09.wav", "letters", "ui", "Audio/click4.ogg", "all"),
    ("click-10.wav", "letters", "ui", "Audio/click5.ogg", "all"),
    ("click-11.wav", "letters", "ui", "Audio/switch33.ogg", "on"),
    ("click-12.wav", "letters", "ui", "Audio/switch34.ogg", "on"),
    ("click-13.wav", "letters", "ui", "Audio/switch38.ogg", "on"),
    # Key-up: the softer "off" clicks of the same three switch files.
    ("release-01.wav", "letters", "ui", "Audio/switch33.ogg", "off"),
    ("release-02.wav", "letters", "ui", "Audio/switch34.ogg", "off"),
    ("release-03.wav", "letters", "ui", "Audio/switch38.ogg", "off"),
    # Space: the deeper, duller switch clicks (UI Audio).
    ("space-01.wav", "space", "ui", "Audio/switch10.ogg", "all"),
    ("space-02.wav", "space", "ui", "Audio/switch15.ogg", "all"),
    ("space-03.wav", "space", "ui", "Audio/switch17.ogg", "all"),
    ("space-04.wav", "space", "ui", "Audio/switch30.ogg", "all"),
    # Enter: short rising two-note "toggle" blips (Interface Sounds).
    ("enter-01.wav", "enter", "if", "Audio/toggle_001.ogg", "all"),
    ("enter-02.wav", "enter", "if", "Audio/toggle_002.ogg", "all"),
    # Backspace: soft, rounded clicks (Interface Sounds).
    ("backspace-01.wav", "backspace", "if", "Audio/click_001.ogg", "all"),
    ("backspace-02.wav", "backspace", "if", "Audio/click_003.ogg", "all"),
    # Modifiers: tiny, bright ticks (UI Audio).
    ("modifier-01.wav", "modifiers", "ui", "Audio/switch13.ogg", "all"),
    ("modifier-02.wav", "modifiers", "ui", "Audio/switch14.ogg", "all"),
    ("modifier-03.wav", "modifiers", "ui", "Audio/switch29.ogg", "all"),
    # Key-up for the quieter groups: the same three off clicks at each group's own level.
    ("enter-release-01.wav", "enter", "ui", "Audio/switch33.ogg", "off"),
    ("enter-release-02.wav", "enter", "ui", "Audio/switch34.ogg", "off"),
    ("enter-release-03.wav", "enter", "ui", "Audio/switch38.ogg", "off"),
    ("backspace-release-01.wav", "backspace", "ui", "Audio/switch33.ogg", "off"),
    ("backspace-release-02.wav", "backspace", "ui", "Audio/switch34.ogg", "off"),
    ("backspace-release-03.wav", "backspace", "ui", "Audio/switch38.ogg", "off"),
    ("modifier-release-01.wav", "modifiers", "ui", "Audio/switch33.ogg", "off"),
    ("modifier-release-02.wav", "modifiers", "ui", "Audio/switch34.ogg", "off"),
    ("modifier-release-03.wav", "modifiers", "ui", "Audio/switch38.ogg", "off"),
]

# Median peak per group after its gain (dBFS), and the length cap (ms).
TARGET_DB = {"letters": -6.0, "space": -6.0, "backspace": -12.0, "modifiers": -15.0}
# Groups levelled by loudness instead of peak: their presses' loudness (k_loudness, power-
# averaged) this many dB from the letters' presses. Enter's blips are sustained tones with a
# low crest factor: at a peak 4 dB under the letters' they played about 6 dB louder than them.
LOUDNESS_OFFSET_DB = {"enter": -4.0}
# No group's presses may be louder than the letters' by more than this (Space is level with
# them), or the build stops.
MAX_OVER_LETTERS_DB = 1.5
# K-weighting (ITU-R BS.1770) as analogue prototypes, the parameters libebur128 uses: they give
# BS.1770's published 48 kHz coefficients at 48 kHz, and the same response at 44.1 kHz. Stage 1
# is a high shelf (f0, gain dB, Q, shelf exponent), stage 2 the RLB high-pass (f0, Q).
K_SHELF = (1681.974450955533, 3.999843853973347, 0.7071752369554196, 0.4996667741545416)
K_HIGHPASS = (38.13547087602444, 0.5003270373238773)
LOUDNESS_WINDOW_S = 0.100
MAX_MS = {
    "letters": 120, "space": 150, "enter": 250, "backspace": 120, "modifiers": 80, "release": 120,
}

# Playback level, written to pack.json as "volume"; it changes no audio file. It matches the
# pack's typing loudness to TakTak's reference typing level, -25.8 LK, so switching packs does not jump
# in volume. Measured on the built pack with
#     cargo run -p synth-packs --release -- loudness packs/ui-clicks
# (tools/synth-packs/src/loudness.rs): -24.2 LK at volume 1.0, -25.5 LK at VOLUME. Recorded
# in SOURCES.md; re-measure whenever the processing here changes.
VOLUME = 0.86

MANIFEST = {
    "format": 1,
    "id": "ui-clicks",
    "name": "UI Clicks",
    "version": "1.0.0",
    "author": "Kenney (kenney.nl); packaged by TakTak contributors",
    "license": "CC0-1.0",
    "description": (
        "Not a keyboard: crisp interface switch clicks for letters, with softer switch-off "
        "clicks on key-up, a duller click for Space, a rising two-note blip for Enter, a soft "
        "click for Backspace and tiny ticks for the modifiers. Made from Kenney's CC0 "
        "UI Audio and Interface Sounds sets."
    ),
    "source": "https://kenney.nl/assets/ui-audio ; https://kenney.nl/assets/interface-sounds",
    "attribution": "UI sounds by Kenney (kenney.nl), CC0",
    "preview": "preview.wav",
    "volume": VOLUME,
    "trim_silence": True,
    "variation": {"pitch": 0.02, "volume": 0.08},
}


def decode(ffmpeg, data):
    """Mono float64 at the file's own rate (checked to be 44.1 kHz), 30 Hz high-pass."""
    probe = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "stream=sample_rate,channels",
         "-of", "csv=p=0", "-i", "pipe:0"],
        input=data, capture_output=True, check=True, text=False,
    ).stdout.decode().strip()
    rate, channels = (int(v) for v in probe.split(","))
    assert rate == SR, f"unexpected rate {rate}"
    mix = "pan=mono|c0=0.5*c0+0.5*c1," if channels == 2 else ""
    raw = subprocess.run(
        [ffmpeg, "-v", "error", "-i", "pipe:0", "-af", f"{mix}highpass=f=30:poles=2",
         "-f", "f32le", "-"],
        input=data, capture_output=True, check=True,
    ).stdout
    return np.frombuffer(raw, dtype=np.float32).astype(np.float64), channels


def db(v):
    return 20.0 * np.log10(max(float(v), 1e-12))


def biquad(x, b0, b1, b2, a1, a2):
    """Direct-form I biquad from a zero state (a0 normalized to 1)."""
    y = np.empty_like(x)
    x1 = x2 = y1 = y2 = 0.0
    for i, xi in enumerate(x.tolist()):
        yi = b0 * xi + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        y[i] = yi
        x2, x1 = x1, xi
        y2, y1 = y1, yi
    return y


def k_weighting(rate):
    """The two BS.1770 K-weighting biquads (b0, b1, b2, a1, a2) at `rate`."""
    f0, gain_db, q, exponent = K_SHELF
    k = np.tan(np.pi * f0 / rate)
    vh = 10.0 ** (gain_db / 20.0)
    vb = vh ** exponent
    a0 = 1.0 + k / q + k * k
    shelf = ((vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0,
             (vh - vb * k / q + k * k) / a0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0)
    f0, q = K_HIGHPASS
    k = np.tan(np.pi * f0 / rate)
    a0 = 1.0 + k / q + k * k
    return [shelf, (1.0, -2.0, 1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0)]


def k_loudness(x):
    """One sound's loudness in LK, as synth-packs measures it per sample (analysis.rs, k_energy
    and k_db): the K-weighted mean square of its first 100 ms, always divided by the full
    window (a shorter sound counts as silence after its end), with BS.1770's -0.691 dB."""
    n = round(LOUDNESS_WINDOW_S * SR)
    y = np.asarray(x[:n], dtype=np.float64)
    for coefficients in k_weighting(SR):
        y = biquad(y, *coefficients)
    return -0.691 + 10.0 * np.log10(max(float(np.sum(y * y)) / n, 1e-30))


def power_mean_db(levels):
    """Power average of levels in dB (how synth-packs averages keystroke loudness)."""
    return float(10.0 * np.log10(np.mean([10.0 ** (v / 10.0) for v in levels])))


def fade(n, rising):
    """Raised cosine over n samples; the end that touches silence is exactly 0."""
    w = 0.5 - 0.5 * np.cos(np.pi * np.arange(n) / (n - 1))
    return w if rising else w[::-1]


def find_cut(x, member, part, group, floor=0.0):
    """(attack, end) sample indices of the part to keep, in the decoded file. The attack is
    also at least `floor`: -50 dBFS in the file as shipped, i.e. after its gain."""
    lo, hi = 0, len(x)
    if part != "all":
        split = int(SPLIT_MS[member] * SR / 1000)
        lo, hi = (0, split) if part == "on" else (split, len(x))
    seg = np.abs(x[lo:hi])
    peak = seg.max()
    attack = lo + int(np.argmax(seg >= max(peak * 10 ** (-26 / 20), floor)))
    w = SR // 1000
    n = (hi - attack) // w
    quiet = np.sqrt((x[attack : attack + n * w].reshape(n, w) ** 2).mean(1)) < peak * 10 ** (-45 / 20)
    first_peak = (lo + int(np.argmax(seg)) - attack) // w
    end = hi
    for i in range(first_peak, n - 9):
        if quiet[i : i + 10].all():
            end = attack + i * w + 2 * w
            break
    cap = MAX_MS["release" if part == "off" else group]
    end = min(end, hi, attack + cap * SR // 1000)
    return attack, end


def cut(x, attack, end):
    start = attack - PRE_ROLL
    pad = max(0, -start)
    y = np.concatenate([np.zeros(pad), x[max(start, 0) : end]])
    y[:PRE_ROLL] *= fade(PRE_ROLL, rising=True)
    n = min(int(0.008 * SR), len(y) // 3)
    y[-n:] *= fade(n, rising=False)
    return y, n


def to_pcm16(x):
    return np.clip(np.round(x * 32767.0), -32768, 32767).astype("<i2")


def write_wav(path, x):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(to_pcm16(x).tobytes())


def render_preview(clips, by_group, releases):
    """About 1.9 s of typing: 4 letters, Space, 3 letters, Enter; every key gets a press
    and the release it gets at runtime (Space falls back to the letter releases)."""
    rng = random.Random(20261002)
    plan = ["letters"] * 4 + ["space"] + ["letters"] * 3 + ["enter"]
    nxt = {g: 0 for g in by_group}
    events = []
    t = 0.0
    for kind in plan:
        pool = by_group[kind]
        i = nxt[kind]
        nxt[kind] += 1
        dwell = rng.uniform(0.060, 0.110)
        events.append((t, clips[pool[(i * 5 + 1) % len(pool)]]))
        up = releases[kind]
        events.append((t + dwell, clips[up[(i * 2 + len(events)) % len(up)]]))
        t += dwell + rng.uniform(0.070, 0.150)
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

    archives = {}
    for key, (rel, expected) in ZIPS.items():
        path = args.downloads / rel
        sha = hashlib.sha256(path.read_bytes()).hexdigest()
        print(f"source {key} sha256 {sha}  {rel}")
        if sha != expected:
            raise SystemExit(f"{rel}: unexpected sha256 {sha} (expected {expected})")
        archives[key] = zipfile.ZipFile(path)

    rows = []
    clips = {}
    decoded = {}
    for name, group, key, member, part in PICKS:
        if (key, member) not in decoded:
            data = archives[key].read(member)
            decoded[(key, member)] = (decode(args.ffmpeg, data), hashlib.sha256(data).hexdigest())
        (x, channels), member_sha = decoded[(key, member)]
        attack, end = find_cut(x, member, part, group)
        clips[name], fade_out = cut(x, attack, end)
        rows.append(
            {
                "file": name, "group": group, "key": key, "zip": ZIPS[key][0].split("/")[1],
                "member": member,
                "member_sha256": member_sha, "channels": channels, "part": part,
                "attack": attack, "end": end, "fade_out": fade_out,
                "source_peak": db(np.abs(x[attack:end]).max()),
            }
        )

    # Group gains, from the presses. An "off" click takes the gain of the group that plays it.
    def presses(group):
        return [r["file"] for r in rows if r["group"] == group and r["part"] != "off"]

    gains = {}
    for group, target in TARGET_DB.items():
        median = float(np.median([db(np.abs(clips[n]).max()) for n in presses(group)]))
        gains[group] = target - median
        print(f"group {group:10s} gain {gains[group]:+.2f} dB (median source peak {median:.2f} dBFS)")
    letters_lk = power_mean_db([k_loudness(clips[n]) for n in presses("letters")])
    for group, offset in LOUDNESS_OFFSET_DB.items():
        lk = power_mean_db([k_loudness(clips[n]) for n in presses(group)])
        gains[group] = letters_lk + gains["letters"] + offset - lk
        print(f"group {group:10s} gain {gains[group]:+.2f} dB (source loudness {lk:.2f} LK, "
              f"{offset:+.1f} dB from the letters)")
    # The off-click copies of a loudness-levelled group: the letters' gain, moved by the offset.
    release_gains = dict(gains)
    for group, offset in LOUDNESS_OFFSET_DB.items():
        release_gains[group] = gains["letters"] + offset
    for r in rows:
        r["gain_db"] = (release_gains if r["part"] == "off" else gains)[r["group"]]
        gain = 10.0 ** (r["gain_db"] / 20.0)
        # A file that would start below the loader's -50 dBFS trim threshold after its gain
        # starts where it reaches it instead (only the quietest off-click copy does).
        x = decoded[(r["key"], r["member"])][0][0]
        attack, end = find_cut(x, r["member"], r["part"], r["group"], TRIM_LEVEL / gain)
        if (attack, end) != (r["attack"], r["end"]):
            moved = (attack - r["attack"]) / SR * 1000
            print(f"{r['file']}: start moved {moved:.2f} ms to where it reaches -50 dBFS")
            clips[r["file"]], r["fade_out"] = cut(x, attack, end)
            r["attack"], r["end"] = attack, end
            r["source_peak"] = db(np.abs(x[attack:end]).max())
        clips[r["file"]] = clips[r["file"]] * gain
    loudest = max(db(np.abs(c).max()) for c in clips.values())
    assert loudest <= -1.0, f"a file peaks at {loudest:.2f} dBFS"
    # Balance between the groups, on the files as shipped (16-bit).
    def shipped_lk(names):
        return power_mean_db([k_loudness(to_pcm16(clips[n]).astype(np.float64) / 32768.0)
                              for n in names])

    group_lk = {group: shipped_lk(presses(group)) for group in gains}
    print("press loudness per group (LK at volume 1.0, re the letters): " + ", ".join(
        f"{g} {lk:.2f} ({lk - group_lk['letters']:+.2f})" for g, lk in group_lk.items()))
    release_lk = {}
    for group in gains:
        names = [r["file"] for r in rows if r["group"] == group and r["part"] == "off"]
        if names:
            release_lk[group] = shipped_lk(names)
    print("release loudness per group (LK at volume 1.0, re the group's presses): " + ", ".join(
        f"{g} {lk:.2f} ({lk - group_lk[g]:+.2f})" for g, lk in release_lk.items()))
    over = {g: lk - group_lk["letters"] for g, lk in group_lk.items()
            if lk - group_lk["letters"] > MAX_OVER_LETTERS_DB}
    assert not over, f"press groups louder than the letters by more than {MAX_OVER_LETTERS_DB} dB: {over}"

    sounds_dir = args.out / "sounds"
    sounds_dir.mkdir(parents=True, exist_ok=True)
    for old in sounds_dir.glob("*.wav"):
        old.unlink()
    for name, c in clips.items():
        write_wav(sounds_dir / name, c)

    def files(group, part=None):
        return [
            r["file"] for r in rows
            if r["group"] == group and (part is None or (r["part"] == "off") == (part == "off"))
        ]

    letters = files("letters", "on")
    releases = files("letters", "off")
    by_group = {"letters": letters, "space": files("space"), "enter": files("enter", "on")}
    release_by_group = {"letters": releases, "space": releases, "enter": files("enter", "off")}
    preview = render_preview(clips, by_group, release_by_group)
    assert np.abs(preview).max() < 1.0, "preview clips"
    assert len(preview) < 1.95 * SR, "preview must stay under 1.95 s"
    write_wav(args.out / "preview.wav", preview)
    print(f"preview {len(preview) / SR:.2f} s, peak {db(np.abs(preview).max()):.2f} dBFS")

    def sounds(names):
        return [f"sounds/{n}" for n in names]

    manifest = dict(MANIFEST)
    manifest["groups"] = {
        "alphanumeric": {"press": sounds(letters), "release": sounds(releases)},
        "space": {"press": sounds(files("space"))},
        "enter": {"press": sounds(files("enter", "on")), "release": sounds(files("enter", "off"))},
        "backspace": {
            "press": sounds(files("backspace", "on")),
            "release": sounds(files("backspace", "off")),
        },
        "modifiers": {
            "press": sounds(files("modifiers", "on")),
            "release": sounds(files("modifiers", "off")),
        },
    }
    (args.out / "pack.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")

    print()
    print("| Pack file | Source (zip → member) | Part | Source samples @44.1 kHz | Length (ms) | Fade-out (ms) | Gain (dB) | Peak (dBFS) |")
    print("|---|---|---|---|---|---|---|---|")
    for r in rows:
        c = clips[r["file"]]
        print(
            f"| `sounds/{r['file']}` | `{r['zip']}` → `{r['member']}` | {r['part']} "
            f"| {r['attack'] - PRE_ROLL}–{r['end']} | {len(c) / SR * 1000:.1f} "
            f"| {r['fade_out'] / SR * 1000:.1f} | {r['gain_db']:+.2f} | {db(np.abs(c).max()):.1f} |"
        )
    print()
    print("| Member | Channels | SHA-256 of the member |")
    print("|---|---|---|")
    seen = set()
    for r in rows:
        if (r["zip"], r["member"]) in seen:
            continue
        seen.add((r["zip"], r["member"]))
        print(f"| `{r['zip']}` → `{r['member']}` | {r['channels']} | `{r['member_sha256']}` |")

    print_stats(clips, rows)
    print()
    for path in sorted(sounds_dir.glob("*.wav")) + [args.out / "preview.wav"]:
        sha = hashlib.sha256(path.read_bytes()).hexdigest()
        print(f"| `{path.relative_to(args.out)}` | {path.stat().st_size} | `{sha}` |")


def print_stats(clips, rows):
    groups = []
    for group, label in [
        ("letters", "alphanumeric"), ("space", "space"), ("enter", "enter"),
        ("backspace", "backspace"), ("modifiers", "modifiers"),
    ]:
        for action, off in [("press", False), ("release", True)]:
            names = [r["file"] for r in rows if r["group"] == group and (r["part"] == "off") == off]
            if names:
                groups.append((f"{label}.{action}", names))
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


if __name__ == "__main__":
    main()
