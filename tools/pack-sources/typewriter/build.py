#!/usr/bin/env python3
"""Rebuild packs/typewriter from Joseph Sardin's CC0 BigSoundBank typewriter recordings.

    tools/pack-sources/typewriter/fetch.sh DOWNLOADS
    python3 tools/pack-sources/typewriter/build.py DOWNLOADS packs/typewriter

Needs python3 with numpy, and ffmpeg (on PATH, or FFMPEG=/path/to/ffmpeg). Nothing here touches
the network: it only reads the FLAC files that fetch.sh downloaded into DOWNLOADS/bigsoundbank.
The output is deterministic: the same downloads give byte-identical files.

All sources are mono, 48 kHz / 24-bit recordings of one portable manual typewriter (a Hermes
Precisa 305) made with the same Neumann KM184 microphone and Sound Devices MixPre-3 recorder.
Four of them are typing takes; the other two are a single key and a single space-bar stroke.
Nothing is synthesized. What the script does (all constants are below):

 1. Decodes each FLAC exactly (24-bit integers to float) and applies a 25 Hz 2nd-order
    Butterworth high-pass, which also removes DC. No resampling: the sources are 48 kHz mono.
 2. Finds every type-bar strike in the takes and keeps the cleanly isolated ones: nothing from
    the previous stroke rings into them, no other key sound starts within 175 ms of the strike,
    the tail has decayed by at least 42 dB where the next key sound begins, no margin bell
    rings, and the stroke is not inside a carriage return or next to an edit in the take. Each
    is cut from 0.5 ms before the strike transient (the quiet key travel before it is left out,
    so the sound starts with the strike) to the start of the next sound, the noise floor or
    420 ms, whichever is first, so the strike keeps its own type-bar return and escapement
    clicks. Of those, the most typical 85% are kept and KEY_VARIANTS mutually different ones
    are chosen (farthest-point sampling on the spectrum), plus the single "Typewriter, Key".
 3. Space: the single "Typewriter, Space" shot plus space-bar strokes found in the takes (no
    type bar, a press and a second click about 0.1 s later, 0.3 s or more after the last
    letter of a word, spectrally close to the single space shot).
 4. Modifiers: the shift thuds. In the takes a capital letter is a low thud (the shift
    pressed: the type basket moves), the strike, and a second thud when the shift is released.
    The thuds before the capital are the press sounds and the thuds after it the release sounds.
 5. Enter: carriage returns from the takes (lever and line-space ratchet, the carriage sliding,
    the thump at the margin stop), each from its first click to the end of the thump's decay.
    Three more Enter variants ring the margin bell first: a bell recorded in a take, with the
    carriage return that followed it in that take mixed in 0.40 s after the bell started (the
    typist's pause shortened so that the sound stays under 1.5 s; the bell's tail is faded out
    under the return).
 6. Gains: one gain for the type-bar strikes (median strike peak -6 dBFS, so the variation
    between strokes is kept); one for the take space strokes, one for the single space shot,
    one for all shift thuds (press and release together) and one for all Enter files, each set
    so the group's median loudness (loudest 50 ms RMS) sits a fixed number of dB below the
    strikes'. On the machine the space bar, shift and carriage are much softer than a type bar
    hitting the platen; the offsets keep that order but make them audible.
 7. Writes 16-bit 48 kHz mono WAVs (raised-cosine 0.5 ms fade-in, fade-out ending on an exact
    zero), preview.wav (under 1.95 s of typing ending with Enter), pack.json and SOURCES.md.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import shutil
import subprocess
import sys
import wave

import numpy as np

SR = 48000
FFMPEG = os.environ.get("FFMPEG") or shutil.which("ffmpeg") or "/opt/homebrew/bin/ffmpeg"
CHECKED = "2026-10-02"
LICENSE_PAGE = "https://bigsoundbank.com/licenses.html"
CC0_DEED = "https://creativecommons.org/publicdomain/zero/1.0/"

# BigSoundBank sound number -> (page URL, title on the page, SHA-256 of the FLAC download).
# Every page says "License CC0 (public domain)", names Joseph SARDIN as the author, and lists
# 48,000 Hz / 24 bits, Monophonic, SoundDevices MixPre-3 + Neumann KM184.
SOURCES = {
    "2838": ("https://bigsoundbank.com/typewriter-5-s2838.html", "Typewriter #5",
             "575a649462582a918e52892dfdf961703cb63b6a617c734bf8fab039acaf8f6d"),
    "2839": ("https://bigsoundbank.com/typewriter-6-s2839.html", "Typewriter #6",
             "bf794a52f5ef6d540c29c568293f0679330ae4e5331cbbccf0f37ca0beed3ddb"),
    "2840": ("https://bigsoundbank.com/typewriter-7-s2840.html", "Typewriter #7",
             "1ddad1613c6eaae41388c3177826aef5af3036e9e473034243b147b9c730eafe"),
    "2841": ("https://bigsoundbank.com/typewriter-8-s2841.html", "Typewriter #8",
             "aae3475fe07a3f06f021bcce837f68cd4cc435a8301a5bc18bd719331bcd4961"),
    "2842": ("https://bigsoundbank.com/typewriter-key-s2842.html", "Typewriter, Key",
             "eef2c09a89af32f3415cbab19608426c34534b1e4eba837cdc66af72fbadb809"),
    "2843": ("https://bigsoundbank.com/typewriter-space-s2843.html", "Typewriter, space",
             "ccc16e0382b934b20c8d92a5d4ea2a505b0073b0b30e20b23e5522439f1fa772"),
}
TAKES = ["2838", "2839", "2840", "2841"]


def direct_url(n: str) -> str:
    return f"https://bigsoundbank.com/UPLOAD/flac/{n}.flac"


HIGHPASS_HZ = 25.0

# Event detection (source levels, dBFS; the takes' loudest strikes are peak-limited at 0 dBFS).
HOP = SR // 2000                 # 0.5 ms envelope hop
STRIKE_DB = -15.0                # envelope peaks above this are type-bar strikes
EVENT_MIN_DB = -45.0             # quieter events are ignored
STRIKE_SEP_MS = 25.0             # a strike is the loudest point within +-25 ms
RISE_WIN_MS = 2.0                # onset detector: 2 ms RMS ...
RISE_BASE_MS = 20.0              # ... rising RISE_DB above its minimum over the previous 20 ms
RISE_DB = 10.0
RISE_ABOVE_FLOOR_DB = 12.0       # ... and at least this far above the take's noise floor
EDIT_GAP_DB = -85.0              # 10 ms RMS below this: an edit (silence spliced into a take)
EDIT_GUARD_S = 0.05

# Cutting.
PREROLL = SR // 2000             # 0.5 ms before the onset, faded in
STRIKE_LEAD_MS = 3.0             # strike onset: first sample <= 3 ms before the peak ...
STRIKE_ONSET_REL_DB = -18.0      # ... within 18 dB of it
EVENT_LEAD_MS = 30.0             # space / thud / bell onset: <= 30 ms before the peak ...
EVENT_ONSET_REL_DB = -30.0       # ... within 30 dB of it
OWN_STRIKE_MS = 175.0            # a strike's own type-bar return and escapement clicks
OWN_SPACE_MS = 250.0             # a space stroke's press, second click and carriage step
OWN_THUD_MS = 150.0            # the shift basket's own rattle after the thud
NEXT_GUARD_MS = 3.0              # stop this long before the next sound's onset
KEY_TRAVEL_MS = 70.0             # a key's travel is heard up to ~60 ms before its strike
TAIL_RISE_DB = 8.0               # 5 ms RMS this far above the tail's running minimum: a new sound
KEY_MAX_MS = 420.0
SPACE_MAX_MS = 420.0
THUD_MAX_MS = 320.0
TAIL_ABOVE_FLOOR_DB = 6.0        # the tail ends where the 5 ms RMS falls below floor + 6 dB ...
TAIL_BELOW_PEAK_DB = 60.0        # ... or peak - 60 dB
FADE_OUT_MS = 20.0
SOFT_FADE_OUT_MS = 40.0          # space and shift sounds: their tails are cut closer to the peak
ENTER_FADE_OUT_MS = 40.0
ABS_ONSET = 0.003_162_3 * 1.02   # the loader's -50 dBFS silence threshold, plus margin

# Strike selection.
KEY_MIN_MS = 190.0
KEY_END_BELOW_PEAK_DB = 48.0     # the cut must fall where the tail has decayed this far ...
KEY_LAST_BELOW_PEAK_DB = 40.0    # ... with no click louder than this in its last 40 ms
KEY_PRE_BELOW_PEAK_DB = 30.0     # RMS of the 15 ms before the cut, relative to the strike peak
KEY_PREV_STRIKE_MS = 180.0
OWN_EVENT_MAX_REL_DB = -12.0     # a louder event inside the own window is another key
BELL_TONAL_DB = 15.0             # bell partial (6.9 kHz) this far above its neighbourhood
KEY_VARIANTS = 40                # plus the single "Typewriter, Key" shot
OUTLIER_PCT = 85

# Space strokes in the takes.
SPACE_LEVEL_DB = (-40.0, -18.0)  # source peak range
SPACE_MAX_DIST = 11.0            # spectral distance to the single space shot's press
SPACE_PREV_STRIKE_MS = 280.0
SPACE_NEXT_STRIKE_MS = 200.0
SPACE_SECOND_MS = (60.0, 160.0)  # the second click follows within this window
SPACE_MIN_MS = 200.0
SPACE_END_BELOW_PEAK_DB = 32.0
SPACE_VARIANTS = 10

# Shift thuds: low-frequency share (50-600 Hz vs above) of the first 30 ms.
THUD_LF_PRESS_DB = 6.0
THUD_LF_RELEASE_DB = 3.5
THUD_LEVEL_DB = (-40.0, -15.0)
SHIFT_TO_STRIKE_MS = (120.0, 500.0)
STRIKE_TO_RELEASE_MS = (120.0, 450.0)
THUD_MIN_MS = 120.0

# Bell and carriage-return regions (seconds), found by ear and on the spectrogram: no key,
# space or thud is taken from these.
EXCLUDE = {
    "2838": [(26.05, 28.02), (52.52, 54.38), (62.70, 65.40), (85.20, 88.10)],
    "2839": [],
    "2840": [(9.75, 11.93), (29.35, 31.73)],
    "2841": [(6.20, 8.28), (24.95, 26.86)],
}

# Enter. Carriage return: (take, first click, end), located on the spectrogram. The onset is the
# strike-style onset of the loudest sample within +-5 ms of the first click (the lever being
# hit, 30-50 ms before the line-space ratchet); the end is just before the next unrelated
# sound, after the margin-stop thump has decayed into the noise floor.
CARRIAGE_RETURNS = {
    "R1": ("2838", 27.215, 28.000),
    "R2": ("2838", 53.506, 54.345),
    "R3": ("2840", 11.025, 11.915),
    "R4": ("2840", 30.614, 31.715),
    "R5": ("2841", 7.331, 8.265),
    "R6": ("2841", 25.945, 26.855),
}
# Margin bells that ring cleanly after a strike: (take, search from). Each is followed in its
# take by the carriage return named in ENTER.
BELLS = {
    "B1": ("2838", 26.110),
    "B2": ("2838", 52.575),
    "B3": ("2841", 24.980),
}
BELL_KEEP_S = 0.85               # bell part: 0.85 s from its onset ...
BELL_FADE_S = 0.30               # ... the last 0.30 s faded out
BELL_TO_RETURN_S = 0.40          # the carriage return starts 0.40 s after the bell
ENTER_MAX_S = 1.5
ENTER = [  # pack file -> (bell or None, carriage return)
    ("enter-return-1", None, "R1"),
    ("enter-return-2", None, "R2"),
    ("enter-return-3", None, "R3"),
    ("enter-return-4", None, "R4"),
    ("enter-return-5", None, "R5"),
    ("enter-return-6", None, "R6"),
    ("enter-bell-1", "B1", "R1"),
    ("enter-bell-2", "B2", "R2"),
    ("enter-bell-3", "B3", "R6"),
]

# Levels.
TARGET_MEDIAN_KEY_PEAK_DB = -6.0
LOUDNESS_WIN_MS = 50.0
BELOW_KEYS_DB = {"space": 8.0, "modifier": 9.0, "enter": 5.0}
CEILING_DB = -1.0
VARIATION = {"pitch": 0.015, "volume": 0.06}
# Playback level, written to pack.json as "volume"; it changes no audio file. Measured on the
# built pack with
#     cargo run -p synth-packs --release -- loudness packs/typewriter
# (tools/synth-packs/src/loudness.rs): typing reads LK_AS_BUILT at volume 1.0, and matching
# TakTak's reference typing level (REFERENCE_LK) would take VOLUME_FOR_REFERENCE. The loudest file's true
# peak (TRUE_PEAK_DBFS, 4x oversampled) must stay at or below MAX_TRUE_PEAK_DBFS at the top of
# the volume variation, as the mixer clips hard at 0 dBFS, which allows VOLUME: typing ends
# 0.4 dB below the reference, within the 0.5 dB tolerance. Recorded in SOURCES.md; re-measure
# whenever the processing here changes.
VOLUME = 1.29
LK_AS_BUILT = -28.41
REFERENCE_LK = -25.8
VOLUME_FOR_REFERENCE = 1.35
TRUE_PEAK_DBFS = -3.77
MAX_TRUE_PEAK_DBFS = -1.0

PREVIEW_SEED = 305
PREVIEW_MAX_S = 1.95

MASK64 = (1 << 64) - 1


# ------------------------------------------------------------------------------------ basics


def db(a):
    return 20.0 * np.log10(np.maximum(a, 1e-12))


def undb(d: float) -> float:
    return 10.0 ** (d / 20.0)


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def decode(path: str) -> np.ndarray:
    """The FLAC's samples as float64 in [-1, 1): exact, no resampling or dithering."""
    raw = subprocess.run(
        [FFMPEG, "-v", "error", "-nostdin", "-i", path, "-map", "0:a:0",
         "-f", "s32le", "-acodec", "pcm_s32le", "-"],
        check=True, capture_output=True).stdout
    return np.frombuffer(raw, dtype="<i4").astype(np.float64) / 2147483648.0


def highpass(x: np.ndarray, fc: float = HIGHPASS_HZ) -> np.ndarray:
    """2nd-order Butterworth high-pass (RBJ biquad, Q = 1/sqrt 2), started at rest."""
    w0 = 2.0 * math.pi * fc / SR
    alpha = math.sin(w0) / (2.0 * math.sqrt(0.5))
    cosw = math.cos(w0)
    a0 = 1.0 + alpha
    b0 = (1.0 + cosw) / 2.0 / a0
    b1 = -(1.0 + cosw) / a0
    b2 = b0
    a1 = -2.0 * cosw / a0
    a2 = (1.0 - alpha) / a0
    y = np.empty_like(x)
    x1 = x2 = y1 = y2 = 0.0
    for i, xi in enumerate(x.tolist()):
        yi = b0 * xi + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        y[i] = yi
        x2, x1 = x1, xi
        y2, y1 = y1, yi
    return y


def window_rms(x: np.ndarray, n: int) -> np.ndarray:
    """RMS of x[i:i+n] for every i (length len(x) - n + 1)."""
    c = np.concatenate(([0.0], np.cumsum(x * x)))
    return np.sqrt(np.maximum(c[n:] - c[:-n], 0.0) / n)


def rms_db(x: np.ndarray) -> float:
    return float(10.0 * np.log10(np.mean(x * x) + 1e-20)) if len(x) else -200.0


def loudness_db(y: np.ndarray) -> float:
    """Loudest 50 ms RMS, dBFS."""
    n = int(LOUDNESS_WIN_MS / 1000 * SR)
    if len(y) <= n:
        return rms_db(y)
    return float(db(window_rms(y, n).max()))


def band_shape(seg: np.ndarray, bands: int = 12) -> np.ndarray:
    """Spectrum in log-spaced bands (100 Hz-16 kHz), dB, mean removed."""
    spec = np.abs(np.fft.rfft(seg * np.hanning(len(seg)), 8192)) ** 2
    f = np.fft.rfftfreq(8192, 1.0 / SR)
    edges = np.geomspace(100.0, 16000.0, bands + 1)
    b = 10 * np.log10(np.array([spec[(f >= lo) & (f < hi)].sum()
                                for lo, hi in zip(edges, edges[1:])]) + 1e-20)
    return b - b.mean()


def lf_share_db(seg: np.ndarray) -> float:
    spec = np.abs(np.fft.rfft(seg * np.hanning(len(seg)), 8192)) ** 2
    f = np.fft.rfftfreq(8192, 1.0 / SR)
    return float(10 * np.log10(spec[(f > 50) & (f < 600)].sum() / (spec[f >= 600].sum() + 1e-30)
                               + 1e-30))


def bell_tonal_db(seg: np.ndarray) -> float:
    """How far the bell's strongest partial (6.9 kHz) stands above its neighbourhood."""
    spec = np.abs(np.fft.rfft(seg * np.hanning(len(seg)), 16384)) ** 2
    f = np.fft.rfftfreq(16384, 1.0 / SR)
    peak = spec[(f > 6880) & (f < 6930)].max()
    around = spec[((f > 6500) & (f < 6850)) | ((f > 6960) & (f < 7300))].mean()
    return float(10 * np.log10(peak / (around + 1e-30) + 1e-30))


def ms(n: int) -> float:
    return n / SR * 1000.0


def consistent_index(code_name: str, n: int) -> int:
    """Which of `n` candidates the app plays for a key by default: SplitMix64 of the 64-bit
    FNV-1a hash of the key's KeyboardEvent.code name, mod n. A copy of `consistent_index` in
    src-tauri/core/src/audio/mixer.rs; main() checks the values that its test pins."""
    h = 0xCBF2_9CE4_8422_2325
    for b in code_name.encode():
        h = ((h ^ b) * 0x0000_0100_0000_01B3) & MASK64
    z = (h + 0x9E37_79B9_7F4A_7C15) & MASK64
    z = ((z ^ (z >> 30)) * 0xBF58_476D_1CE4_E5B9) & MASK64
    z = ((z ^ (z >> 27)) * 0x94D0_49BB_1331_11EB) & MASK64
    return (z ^ (z >> 31)) % n


def default_enter(pool: list[str]) -> dict[str, str]:
    """The file each Enter key plays by default (it keeps one; random variants pick per press)."""
    return {key: pool[consistent_index(key, len(pool))] for key in ("Enter", "NumpadEnter")}


def samples(milliseconds: float) -> int:
    return int(round(milliseconds / 1000.0 * SR))


# ------------------------------------------------------------------------------------ takes


class Take:
    """A decoded, high-passed recording with its events."""

    def __init__(self, number: str, x: np.ndarray):
        self.number = number
        self.x = x
        n = len(x) // HOP
        self.env = db(np.abs(x[: n * HOP]).reshape(n, HOP).max(1))
        b = SR // 100
        m = len(x) // b
        r10 = 10 * np.log10((x[: m * b].reshape(m, b) ** 2).mean(1) + 1e-20)
        live = r10[r10 > EDIT_GAP_DB]
        self.floor_db = float(np.percentile(live, 10)) if len(live) else -90.0
        gaps = r10 <= EDIT_GAP_DB
        self.edit_gaps = []
        i = 0
        while i < m:
            if gaps[i]:
                j = i
                while j < m and gaps[j]:
                    j += 1
                if j - i >= 3:
                    self.edit_gaps.append((i * b, j * b))
                i = j
            else:
                i += 1
        self.r2 = db(window_rms(x, samples(RISE_WIN_MS)))
        self.r5 = db(window_rms(x, samples(5.0)))
        self.strikes = self._strikes()
        self.quiet_events = self.events(EVENT_MIN_DB, STRIKE_DB)

    def _strikes(self) -> list[int]:
        e = self.env
        w = int(STRIKE_SEP_MS / 0.5)
        out = []
        for i in np.nonzero(e >= STRIKE_DB)[0]:
            if i < w or i + w >= len(e) or e[i] < e[i - w: i + w + 1].max():
                continue
            if out and (i * HOP - out[-1]) < w * HOP:
                continue
            out.append(i * HOP + int(np.argmax(np.abs(self.x[i * HOP: (i + 1) * HOP]))))
        return out

    def events(self, lo_db: float, hi_db: float, sep_ms: float = 25.0) -> list[int]:
        """Peaks of non-strike events: the loudest point within +-sep_ms, in [lo_db, hi_db)."""
        e = self.env
        w = int(sep_ms / 0.5)
        back = 40
        out = []
        for i in np.nonzero((e >= lo_db) & (e < hi_db))[0]:
            if i < back or i + w >= len(e) or e[i] < e[i - w: i + w + 1].max():
                continue
            if e[i] - e[i - back: i - 10].min() < 12.0:
                continue
            if out and (i * HOP - out[-1]) < w * HOP:
                continue
            out.append(i * HOP + int(np.argmax(np.abs(self.x[i * HOP: (i + 1) * HOP]))))
        return out

    def rising(self, i: int) -> bool:
        """Onset detector: the 2 ms RMS at i is RISE_DB above its minimum over the previous
        20 ms, and clearly above the noise floor."""
        base = samples(RISE_BASE_MS)
        r = self.r2
        if i < base or i >= len(r) or r[i] < self.floor_db + RISE_ABOVE_FLOOR_DB:
            return False
        return bool(r[i] - r[i - base: i - HOP].min() >= RISE_DB)

    def cut_end(self, p: int, peak_db: float, own_ms: float, max_ms: float, nxt: int) -> int:
        """Where a sound peaking at p ends: before the next key's travel (KEY_TRAVEL_MS before
        its strike `nxt`), before the next other event after the sound's own `own_ms`, where the
        tail reaches the noise floor, or after max_ms, whichever is first."""
        own_end = p + samples(own_ms)
        limit = min(nxt - samples(KEY_TRAVEL_MS), p + samples(max_ms))
        for q in self.quiet_events:
            if own_end <= q < limit:
                limit = min(limit, self.event_onset(q) - samples(NEXT_GUARD_MS))
                break
        # anything that starts to rise out of the decaying tail (a key's travel, a hand on the
        # machine) is not part of this sound
        r = self.r5
        low = r[own_end] if own_end < len(r) else 0.0
        for i in range(own_end, min(limit, len(r)), HOP):
            low = min(low, r[i])
            if r[i] >= low + TAIL_RISE_DB and r[i] >= self.floor_db + TAIL_RISE_DB:
                limit = i - samples(NEXT_GUARD_MS)
                break
        return self.tail_end(p, limit, peak_db)

    def event_onset(self, q: int) -> int:
        """Onset of the non-strike event peaking at q: where it rises out of what precedes it.

        The detector fires all along a rising edge, so the onset is the start of the unbroken
        run of detections (gaps of at most 2.5 ms) that leads up to the peak; then the first
        sample from there within 30 dB of the peak. A level threshold alone would start an
        event that rides on the tail of a strike too early.
        """
        a = max(samples(RISE_BASE_MS), q - samples(EVENT_LEAD_MS))
        grid = list(range(a, q + 1, HOP))
        hits = [k for k, i in enumerate(grid) if self.rising(i)]
        if not hits:
            return onset_before_peak(self.x, q, STRIKE_LEAD_MS, STRIKE_ONSET_REL_DB, 1.0)
        k = len(hits) - 1
        while k > 0 and hits[k] - hits[k - 1] <= 5:
            k -= 1
        rise = grid[hits[k]]
        thr = abs(self.x[q]) * undb(EVENT_ONSET_REL_DB)
        idx = np.nonzero(np.abs(self.x[rise: q + 1]) >= thr)[0]
        return rise + int(idx[0]) if len(idx) else q

    def tail_end(self, p: int, limit: int, peak_db: float) -> int:
        thr = max(self.floor_db + TAIL_ABOVE_FLOOR_DB, peak_db - TAIL_BELOW_PEAK_DB)
        w = samples(5.0)
        r = self.r5[p: max(p + 1, limit - w)]
        above = np.nonzero(r >= thr)[0]
        return min(limit, p + int(above[-1]) + w) if len(above) else min(limit, p + w)

    def near_edit(self, a: int, b: int) -> bool:
        g = samples(EDIT_GUARD_S * 1000)
        return any(a - g < e and b + g > s for s, e in self.edit_gaps)

    def excluded(self, a: int, b: int) -> bool:
        return any(a < int(e * SR) and b > int(s * SR) for s, e in EXCLUDE[self.number])


def onset_before_peak(x: np.ndarray, p: int, lead_ms: float, rel_db: float, gain: float) -> int:
    """First sample in [p - lead, p] within rel_db of x[p] and audible after `gain`."""
    a = max(0, p - samples(lead_ms))
    thr = max(abs(x[p]) * undb(rel_db), ABS_ONSET / gain)
    idx = np.nonzero(np.abs(x[a: p + 1]) >= thr)[0]
    return a + int(idx[0]) if len(idx) else p


def audible_from(x: np.ndarray, i: int, gain: float) -> int:
    """The first sample at or after i that reaches the loader's threshold after `gain`."""
    idx = np.nonzero(np.abs(x[i:]) * gain >= ABS_ONSET)[0]
    return i + int(idx[0])


def cut(x: np.ndarray, onset: int, end: int, fade_out_ms: float = FADE_OUT_MS) -> np.ndarray:
    """x[onset - 0.5 ms : end], faded in over the 0.5 ms pre-roll and out to an exact zero."""
    start = onset - PREROLL
    y = x[start:end].copy()
    n = np.arange(PREROLL)
    y[:PREROLL] *= np.sin(0.5 * np.pi * n / PREROLL) ** 2
    fo = min(samples(fade_out_ms), len(y) - PREROLL)
    n = np.arange(1, fo + 1)
    y[-fo:] *= np.cos(0.5 * np.pi * n / fo) ** 2
    y[-1] = 0.0
    return y


# ------------------------------------------------------------------------------- selection


def strike_candidates(t: Take) -> tuple[list[dict], dict]:
    x = t.x
    reasons: dict[str, int] = {}

    def reject(why: str) -> None:
        reasons[why] = reasons.get(why, 0) + 1

    out = []
    for k, p in enumerate(t.strikes):
        peak_db = float(db(abs(x[p])))
        onset = onset_before_peak(x, p, STRIKE_LEAD_MS, STRIKE_ONSET_REL_DB, 1.0)
        start = onset - PREROLL
        prev = t.strikes[k - 1] if k else None
        nxt = t.strikes[k + 1] if k + 1 < len(t.strikes) else len(x)
        if prev is not None and ms(p - prev) < KEY_PREV_STRIKE_MS:
            reject("previous strike < 180 ms before")
            continue
        own_end = p + samples(OWN_STRIKE_MS)
        own = [q for q in t.quiet_events if p < q < own_end]
        if (any(db(abs(x[q])) - peak_db > OWN_EVENT_MAX_REL_DB for q in own)
                or nxt - samples(KEY_TRAVEL_MS) < own_end):
            reject("another key inside the stroke's own 175 ms")
            continue
        end = t.cut_end(p, peak_db, OWN_STRIKE_MS, KEY_MAX_MS, nxt)
        if t.excluded(start, end):
            reject("inside a bell or carriage-return region")
            continue
        if t.near_edit(start, end):
            reject("next to an edit in the take")
            continue
        if ms(end - start) < KEY_MIN_MS:
            reject("shorter than 190 ms before the next sound")
            continue
        end_level = rms_db(x[end - samples(10.0): end])
        last = float(db(window_rms(x[end - samples(40.0): end], samples(5.0)).max()))
        if end_level > peak_db - KEY_END_BELOW_PEAK_DB or last > peak_db - KEY_LAST_BELOW_PEAK_DB:
            reject("tail not decayed where the next sound starts")
            continue
        if rms_db(x[start - samples(15.0): start]) > peak_db - KEY_PRE_BELOW_PEAK_DB:
            reject("previous sound still ringing")
            continue
        if bell_tonal_db(x[p + samples(20.0): p + samples(250.0)]) > BELL_TONAL_DB:
            reject("margin bell ringing")
            continue
        a = np.abs(x[start:end]) >= 0.999
        if np.any(a[2:] & a[1:-1] & a[:-2]):
            reject("clipped")
            continue
        out.append(dict(take=t.number, p=p, onset=onset, end=end, peak_db=peak_db,
                        shape=band_shape(x[p - samples(2.0): p + samples(40.0)])))
    return out, reasons


def select_diverse(cands: list[dict], count: int, feature) -> list[dict]:
    """Drop the least typical (1 - OUTLIER_PCT), then farthest-point sampling from the medoid."""
    feats = np.array([feature(c) for c in cands])
    med = np.median(feats, 0)
    dist = np.linalg.norm(feats - med, axis=1)
    typical = np.nonzero(dist <= np.percentile(dist, OUTLIER_PCT))[0]
    count = min(count, len(typical))
    chosen = [int(typical[np.argmin(dist[typical])])]
    mind = np.linalg.norm(feats[typical] - feats[chosen[0]], axis=1)
    while len(chosen) < count:
        j = int(np.argmax(mind))
        chosen.append(int(typical[j]))
        mind = np.minimum(mind, np.linalg.norm(feats[typical] - feats[typical[j]], axis=1))
    picked = [cands[j] for j in chosen]
    picked.sort(key=lambda c: (c["take"], c["p"]))
    return picked


def space_candidates(t: Take, ref_shape: np.ndarray) -> list[dict]:
    x = t.x
    out = []
    events = t.quiet_events
    for q in events:
        level = float(db(abs(x[q])))
        if not SPACE_LEVEL_DB[0] <= level < SPACE_LEVEL_DB[1]:
            continue
        prev = [s for s in t.strikes if s < q]
        nxt = [s for s in t.strikes if s > q]
        if prev and ms(q - prev[-1]) < SPACE_PREV_STRIKE_MS:
            continue
        if nxt and ms(nxt[0] - q) < SPACE_NEXT_STRIKE_MS:
            continue
        onset = t.event_onset(q)
        shape = band_shape(x[q - samples(2.0): q + samples(28.0)])
        if np.linalg.norm(shape - ref_shape) > SPACE_MAX_DIST:
            continue
        if lf_share_db(x[onset: onset + samples(30.0)]) >= THUD_LF_RELEASE_DB:
            continue
        second = [r for r in events
                  if SPACE_SECOND_MS[0] <= ms(r - q) <= SPACE_SECOND_MS[1]]
        if not second:
            continue
        start = onset - PREROLL
        if rms_db(x[start - samples(15.0): start]) > level - 20.0:
            continue
        end = t.cut_end(q, level, OWN_SPACE_MS, SPACE_MAX_MS, nxt[0] if nxt else len(x))
        if t.excluded(start, end) or t.near_edit(start, end):
            continue
        if ms(end - start) < SPACE_MIN_MS:
            continue
        if rms_db(x[end - samples(10.0): end]) > level - SPACE_END_BELOW_PEAK_DB:
            continue
        out.append(dict(take=t.number, p=q, onset=onset, end=end, peak_db=level, shape=shape))
    return out


def shift_pairs(t: Take) -> list[tuple[dict, dict]]:
    """(press thud, release thud) around a single capital-letter strike."""
    x = t.x
    events = t.events(*THUD_LEVEL_DB)
    onsets = {q: t.event_onset(q) for q in events}
    lf = {q: lf_share_db(x[onsets[q]: onsets[q] + samples(30.0)]) for q in events}
    pairs = []
    for q in events:
        if lf[q] < THUD_LF_PRESS_DB:
            continue
        nxt = [s for s in t.strikes if s > q]
        if not nxt or not SHIFT_TO_STRIKE_MS[0] <= ms(nxt[0] - q) <= SHIFT_TO_STRIKE_MS[1]:
            continue
        s = nxt[0]
        after = nxt[1] if len(nxt) > 1 else len(x)
        rel = [r for r in events if s < r < after and lf[r] >= THUD_LF_RELEASE_DB
               and STRIKE_TO_RELEASE_MS[0] <= ms(r - s) <= STRIKE_TO_RELEASE_MS[1]]
        if not rel:
            continue
        thuds = []
        for r, lim in ((q, s), (rel[0], after)):
            level = float(db(abs(x[r])))
            onset = onsets[r]
            end = t.cut_end(r, level, OWN_THUD_MS, THUD_MAX_MS, lim)
            thuds.append(dict(take=t.number, p=r, onset=onset, end=end, peak_db=level))
        press, release = thuds
        if any(t.excluded(c["onset"], c["end"]) or t.near_edit(c["onset"], c["end"])
               or ms(c["end"] - c["onset"]) < THUD_MIN_MS for c in thuds):
            continue
        pairs.append((press, release))
    return pairs


# ------------------------------------------------------------------------------------ output


def to_int16(y: np.ndarray) -> np.ndarray:
    if np.max(np.abs(y)) >= 1.0:
        raise ValueError("sample would clip")
    return np.round(y * 32767.0).astype("<i2")


def write_wav(path: str, y: np.ndarray) -> None:
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(to_int16(y).tobytes())


def read_wav(path: str) -> np.ndarray:
    with wave.open(path, "rb") as w:
        return np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float64) / 32768.0


def tool_versions() -> str:
    """"Python 3.x.y, numpy a.b.c and ffmpeg d.e.f", for SOURCES.md."""
    out = subprocess.run([FFMPEG, "-version"], check=True, capture_output=True, text=True).stdout
    words = out.split()
    ff = words[2] if len(words) > 2 and words[:2] == ["ffmpeg", "version"] else "(unknown)"
    py = ".".join(str(v) for v in sys.version_info[:3])
    return f"Python {py}, numpy {np.__version__} and ffmpeg {ff}"


def leading_silence_ms(y: np.ndarray) -> float:
    idx = np.nonzero(np.abs(y) >= 0.003_162_3)[0]
    return ms(int(idx[0])) if len(idx) else ms(len(y))


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("downloads", help="directory given to fetch.sh")
    ap.add_argument("out", help="pack directory to (re)write, e.g. packs/typewriter")
    args = ap.parse_args()

    golden = [consistent_index(k, n) for k, n in (("KeyA", 31), ("Enter", 9), ("NumpadEnter", 9))]
    if golden != [23, 1, 7]:
        sys.exit(f"consistent_index gives {golden}, not the app's [23, 1, 7]")

    # Decode and check the downloads.
    takes: dict[str, Take] = {}
    for n, (_, _, want) in SOURCES.items():
        path = os.path.join(args.downloads, "bigsoundbank", f"{n}.flac")
        if not os.path.isfile(path):
            sys.exit(f"{path}: missing; run fetch.sh first")
        got = sha256_file(path)
        if got != want:
            sys.exit(f"{path}: SHA-256 {got}, expected {want}; run fetch.sh")
        takes[n] = Take(n, highpass(decode(path)))

    # Type-bar strikes.
    cands = []
    reasons: dict[str, int] = {}
    for n in TAKES:
        c, r = strike_candidates(takes[n])
        cands += c
        for k, v in r.items():
            reasons[k] = reasons.get(k, 0) + v
    n_strikes = sum(len(takes[n].strikes) for n in TAKES)
    keys = select_diverse(cands, KEY_VARIANTS,
                          lambda c: np.concatenate([c["shape"], [0.5 * c["peak_db"]]]))

    single = takes["2842"]
    sp = int(np.argmax(np.abs(single.x)))
    s_onset = onset_before_peak(single.x, sp, STRIKE_LEAD_MS, STRIKE_ONSET_REL_DB, 1.0)
    s_end = single.tail_end(sp, sp + samples(KEY_MAX_MS), float(db(abs(single.x[sp]))))
    key_single = dict(take="2842", p=sp, onset=s_onset, end=s_end,
                      peak_db=float(db(abs(single.x[sp]))))

    # Space strokes: the single shot, and strokes in the takes that resemble its press.
    space = takes["2843"]
    qp = int(np.argmax(np.abs(space.x[: samples(150.0)])))
    q_onset = space.event_onset(qp)
    ref_shape = band_shape(space.x[qp - samples(2.0): qp + samples(28.0)])
    q_end = space.tail_end(qp, qp + samples(SPACE_MAX_MS), float(db(abs(space.x[qp]))))
    space_single = dict(take="2843", p=qp, onset=q_onset, end=q_end,
                        peak_db=float(db(abs(space.x[qp]))))
    space_cands = []
    for n in TAKES:
        space_cands += space_candidates(takes[n], ref_shape)
    spaces = select_diverse(space_cands, SPACE_VARIANTS,
                            lambda c: np.concatenate([c["shape"], [0.5 * c["peak_db"]]]))

    # Shift thuds.
    pairs = []
    for n in TAKES:
        pairs += shift_pairs(takes[n])

    # Enter: carriage returns and bells.
    returns = {}
    for name, (n, t_from, t_end) in CARRIAGE_RETURNS.items():
        t = takes[n]
        a = int(t_from * SR) - samples(5.0)
        p = a + int(np.argmax(np.abs(t.x[a: a + samples(10.0)])))
        onset = onset_before_peak(t.x, p, STRIKE_LEAD_MS, STRIKE_ONSET_REL_DB, 1.0)
        returns[name] = dict(take=n, onset=onset, end=int(t_end * SR))
    bells = {}
    for name, (n, t_from) in BELLS.items():
        t = takes[n]
        a = int(t_from * SR)
        p = a + int(np.argmax(np.abs(t.x[a: a + samples(60.0)])))
        onset = t.event_onset(p)
        bells[name] = dict(take=n, onset=onset, end=onset + int(BELL_KEEP_S * SR))

    # ------------------------------------------------------------------ levels
    def raw(c: dict, fade_ms: float = FADE_OUT_MS) -> np.ndarray:
        return cut(takes[c["take"]].x, c["onset"], c["end"], fade_ms)

    key_gain = undb(TARGET_MEDIAN_KEY_PEAK_DB - float(np.median(
        [c["peak_db"] for c in keys + [key_single]])))
    key_loud = float(np.median([loudness_db(raw(c)) for c in keys + [key_single]])) + db(key_gain)

    def group_gain(cs: list[dict], target_below: float, fade_ms: float = FADE_OUT_MS) -> float:
        loud = float(np.median([loudness_db(raw(c, fade_ms)) for c in cs]))
        return undb(key_loud - target_below - loud)

    space_gain = group_gain(spaces, BELOW_KEYS_DB["space"], SOFT_FADE_OUT_MS)
    space_single_gain = group_gain([space_single], BELOW_KEYS_DB["space"], SOFT_FADE_OUT_MS)
    thuds = [p for p, _ in pairs] + [r for _, r in pairs]
    thud_gain = group_gain(thuds, BELOW_KEYS_DB["modifier"], SOFT_FADE_OUT_MS)

    def enter_raw(bell: str | None, ret: str, gain: float) -> tuple[np.ndarray, str]:
        r = returns[ret]
        t = takes[r["take"]]
        onset = audible_from(t.x, r["onset"], gain)
        ry = cut(t.x, onset, r["end"], ENTER_FADE_OUT_MS)
        how = (f"carriage return {(onset - PREROLL) / SR:.4f}-{r['end'] / SR:.4f} s of "
               f"{r['take']}")
        if bell is None:
            return ry, how
        b = bells[bell]
        bt = takes[b["take"]]
        bon = audible_from(bt.x, b["onset"], gain)
        by = cut(bt.x, bon, b["end"], BELL_FADE_S * 1000)
        off = int(BELL_TO_RETURN_S * SR)
        y = np.zeros(max(len(by), off + len(ry)))
        y[: len(by)] += by
        y[off: off + len(ry)] += ry
        how = (f"margin bell {(bon - PREROLL) / SR:.4f}-{b['end'] / SR:.4f} s of {b['take']} "
               f"(last {BELL_FADE_S * 1000:.0f} ms faded out), with the {how} mixed in at "
               f"+{BELL_TO_RETURN_S:.2f} s")
        return y, how

    enter_loud = float(np.median([loudness_db(enter_raw(b, r, 1.0)[0]) for _, b, r in ENTER]))
    enter_gain = undb(key_loud - BELOW_KEYS_DB["enter"] - enter_loud)

    # ------------------------------------------------------------------ cut with final gains
    sounds: dict[str, np.ndarray] = {}
    derivation: list[tuple[str, str, str, float]] = []  # file, source, how, gain dB

    def add(name: str, c: dict, gain: float, fade_ms: float = FADE_OUT_MS, what: str = "") -> None:
        t = takes[c["take"]]
        onset = c["onset"]
        if abs(t.x[onset]) * gain < ABS_ONSET:
            onset = audible_from(t.x, onset, gain)
        y = cut(t.x, onset, c["end"], fade_ms) * gain
        sounds[name] = y
        how = (f"{what}{(onset - PREROLL) / SR:.4f}-{c['end'] / SR:.4f} s "
               f"(onset {onset / SR:.4f} s)")
        derivation.append((name, c["take"], how, float(db(gain))))

    add("key-01", key_single, key_gain, what="strike ")
    for k, c in enumerate(keys, start=2):
        add(f"key-{k:02d}", c, key_gain, what="strike ")
    add("space-01", space_single, space_single_gain, SOFT_FADE_OUT_MS, what="space bar ")
    for k, c in enumerate(spaces, start=2):
        add(f"space-{k:02d}", c, space_gain, SOFT_FADE_OUT_MS, what="space bar ")
    for k, (press, release) in enumerate(pairs, start=1):
        add(f"shift-down-{k}", press, thud_gain, SOFT_FADE_OUT_MS, what="shift pressed ")
        add(f"shift-up-{k}", release, thud_gain, SOFT_FADE_OUT_MS, what="shift released ")
    for name, bell, ret in ENTER:
        y, how = enter_raw(bell, ret, enter_gain)
        sounds[name] = y * enter_gain
        src = returns[ret]["take"] if bell is None else bells[bell]["take"]
        derivation.append((name, src, how, float(db(enter_gain))))

    # Checks.
    for name, y in sounds.items():
        if db(np.abs(y).max()) > CEILING_DB:
            sys.exit(f"{name}: peak {db(np.abs(y).max()):.2f} dBFS is above {CEILING_DB} dBFS")
        if leading_silence_ms(y) > 0.5 + 1e-9:
            sys.exit(f"{name}: {leading_silence_ms(y):.2f} ms of leading silence")
        if y[-1] != 0.0:
            sys.exit(f"{name}: does not end on zero")
        limit = ENTER_MAX_S if name.startswith("enter-") else 0.5
        if len(y) / SR > limit:
            sys.exit(f"{name}: {len(y) / SR:.3f} s is longer than {limit} s")

    # ------------------------------------------------------------------ preview
    preview, preview_events = render_preview(sounds)

    # ------------------------------------------------------------------ write
    os.makedirs(args.out, exist_ok=True)
    snd_dir = os.path.join(args.out, "sounds")
    os.makedirs(snd_dir, exist_ok=True)
    for f in sorted(os.listdir(snd_dir)):
        if f.endswith(".wav"):
            os.remove(os.path.join(snd_dir, f))
    for name in sorted(sounds):
        write_wav(os.path.join(snd_dir, name + ".wav"), sounds[name])
    write_wav(os.path.join(args.out, "preview.wav"), preview)

    def files(prefix: str) -> list[str]:
        return [f"sounds/{n}.wav" for n in sorted(sounds) if n.startswith(prefix)]

    # The description promises the bell on Enter and a plain return on the numpad's Enter, which
    # is what each key keeps by default from the shared pool.
    enter = default_enter(files("enter-"))
    if "/enter-bell-" not in enter["Enter"] or "/enter-bell-" in enter["NumpadEnter"]:
        sys.exit(f"Enter plays {enter['Enter']} and NumpadEnter {enter['NumpadEnter']} by default; "
                 "the description says Enter rings the bell and NumpadEnter does not")

    manifest = {
        "format": 1,
        "id": "typewriter",
        "name": "Typewriter",
        "version": "1.0.0",
        "author": "Joseph Sardin (BigSoundBank.com); packaged by TakTak contributors",
        "license": "CC0-1.0",
        "description": (
            "A portable manual typewriter, recorded on a Hermes Precisa 305: crisp type-bar "
            "strikes with their own type-bar return and escapement clicks, a soft space bar, "
            "the shift basket's thud on modifier press and release, and a carriage return on "
            "Enter, with the margin bell ringing first (the numpad's Enter returns without it)."),
        "source": "BigSoundBank (CC0): " + " ".join(SOURCES[n][0] for n in SOURCES),
        "attribution": "Typewriter recordings by Joseph Sardin (BigSoundBank.com), CC0",
        "preview": "preview.wav",
        "volume": VOLUME,
        "trim_silence": True,
        "variation": VARIATION,
        "groups": {
            "alphanumeric": {"press": files("key-")},
            "space": {"press": files("space-")},
            "enter": {"press": files("enter-")},
            "backspace": {"press": files("space-")},
            "modifiers": {"press": files("shift-down-"), "release": files("shift-up-")},
        },
    }
    with open(os.path.join(args.out, "pack.json"), "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")

    gains = dict(key=db(key_gain), space=db(space_gain), space_single=db(space_single_gain),
                 thud=db(thud_gain), enter=db(enter_gain))
    write_sources_md(args.out, manifest, derivation, gains, dict(
        strikes=n_strikes, qualifying=len(cands), reasons=reasons,
        space_qualifying=len(space_cands), pairs=len(pairs)), preview_events, len(preview))

    print(f"strikes {n_strikes}, qualifying {len(cands)}, keys {len(keys) + 1}; "
          f"space strokes qualifying {len(space_cands)}, used {len(spaces) + 1}; "
          f"shift pairs {len(pairs)}; enter {len(ENTER)}")
    print("rejections: " + ", ".join(f"{k}: {v}" for k, v in sorted(reasons.items())))
    print("gains dB: " + ", ".join(f"{k} {v:+.2f}" for k, v in gains.items()))
    print(f"preview {len(preview) / SR:.3f} s")


# ------------------------------------------------------------------------------------ preview


def render_preview(sounds: dict[str, np.ndarray]) -> tuple[np.ndarray, list[tuple[str, float]]]:
    """Two short words typed at a steady pace, ending with a belled carriage return.

    Two letters, space, two letters, Enter: 130-180 ms between strokes and 150-200 ms before
    Enter, each stroke at a random gain within the pack's volume variation. Enter is the
    shortest belled variant, so that the whole preview, Enter included, stays under
    PREVIEW_MAX_S. Deterministic: the random draws come from PREVIEW_SEED (the next seed is
    tried if a draw does not fit).
    """
    keys = sorted(n for n in sounds if n.startswith("key-"))
    spaces = sorted(n for n in sounds if n.startswith("space-"))
    bells = sorted((n for n in sounds if n.startswith("enter-bell-")), key=lambda n: len(sounds[n]))
    enter = bells[0]
    budget = PREVIEW_MAX_S - len(sounds[enter]) / SR - 0.005
    for attempt in range(1000):
        rng = np.random.default_rng(PREVIEW_SEED + attempt)
        events = []
        t = 0.0
        for k, token in enumerate(["k", "k", "space", "k", "k"]):
            if k:
                t += float(rng.uniform(0.13, 0.18))
            pool = spaces if token == "space" else keys
            events.append((pool[int(rng.integers(len(pool)))], t))
        t += float(rng.uniform(0.15, 0.20))
        if t <= budget:
            events.append((enter, t))
            break
    else:
        raise ValueError("cannot fit the preview into PREVIEW_MAX_S")
    length = max(int(round(ts * SR)) + len(sounds[n]) for n, ts in events)
    out = np.zeros(length)
    for n, ts in events:
        a = int(round(ts * SR))
        g = 1.0 + float(rng.uniform(-VARIATION["volume"], VARIATION["volume"]))
        out[a: a + len(sounds[n])] += sounds[n] * g
    if db(np.abs(out).max()) > CEILING_DB:
        out *= undb(CEILING_DB) / np.abs(out).max()
    out[-1] = 0.0
    return out, events


# ------------------------------------------------------------------------------------ SOURCES.md


def write_sources_md(out: str, manifest: dict, derivation, gains, counts, preview_events,
                     preview_len) -> None:
    def h(rel: str) -> str:
        return sha256_file(os.path.join(out, rel))

    enter_files = default_enter(manifest["groups"]["enter"]["press"])

    lines = [
        "<!-- generated by tools/pack-sources/typewriter/build.py -->",
        "# Typewriter: sources",
        "",
        "Generated by `tools/pack-sources/typewriter/build.py`. Rebuild with:",
        "",
        "```",
        "tools/pack-sources/typewriter/fetch.sh DOWNLOADS",
        "python3 tools/pack-sources/typewriter/build.py DOWNLOADS packs/typewriter",
        "```",
        "",
        f"These files were built with {tool_versions()}. The build is deterministic: the same "
        "downloads give byte-identical files. It reads only the downloads (it checks their "
        "SHA-256 first) and never touches the network.",
        "",
        "## License",
        "",
        f"Every recording is by Joseph SARDIN (BigSoundBank.com) and is released under **CC0 1.0 "
        f"Universal** ({CC0_DEED}). BigSoundBank's license page ({LICENSE_PAGE}) says all its "
        "sounds may be shared, adapted and used, including commercially, without restrictions "
        "and without asking permission, and that crediting \"Joseph SARDIN - BigSoundBank.com\" "
        "is appreciated but not mandatory. Each sound's own page also states \"License CC0 "
        f"(public domain)\". Both were checked on {CHECKED}. The pack is `CC0-1.0`; the credit "
        "in `pack.json` is a courtesy. CC0 grants no trademark rights, so the pack's name avoids "
        "the typewriter's brand, which appears only in the description to say what was "
        "recorded.",
        "",
        "## Original files",
        "",
        "All six are FLAC, 48 kHz, 24-bit, mono, recorded by Joseph SARDIN on a Hermes Precisa "
        "305 portable manual typewriter with a Neumann KM184 into a Sound Devices MixPre-3 (as "
        "listed on each page). No account or login is needed to download them.",
        "",
        "| Sound | Page | Direct download | Author | License (proof) | Checked | SHA-256 of download |",
        "|---|---|---|---|---|---|---|",
    ]
    for n, (page, title, sha) in SOURCES.items():
        lines.append(f"| {n} \"{title}\" | {page} | {direct_url(n)} | Joseph SARDIN | CC0-1.0 "
                     f"({page}, {LICENSE_PAGE}) | {CHECKED} | `{sha}` |")
    lines += [
        "",
        "## How each file was made",
        "",
        "Every pack file comes from exactly one of the downloads above, except the three "
        "`enter-bell-*` files, which mix two passages of one take. Processing, in order:",
        "",
        "1. The FLAC is decoded exactly (24-bit integers to float, no resampling: the sources "
        "are already 48 kHz mono).",
        f"2. A {HIGHPASS_HZ:.0f} Hz 2nd-order Butterworth high-pass is applied to the whole "
        "recording (removes DC and rumble).",
        "3. The passage listed below is cut out: it starts 0.5 ms before the onset (faded in "
        "over that 0.5 ms with a raised cosine) and ends with a raised-cosine fade-out ending on "
        f"an exact zero ({FADE_OUT_MS:.0f} ms; {ENTER_FADE_OUT_MS:.0f} ms for Enter). For a "
        "type-bar strike the onset is the strike itself (the quiet key travel before it is not "
        "kept, so the sound starts without delay); the cut ends before the next sound in the "
        "take, or where the tail reaches the noise floor, or after 420 ms.",
        "4. One gain per group (listed below), so the differences between strokes are kept.",
        "5. Written as 16-bit PCM WAV, 48 kHz, mono. No compression, EQ, normalization, "
        "pitch or time changes.",
        "",
        "Gains: type-bar strikes "
        f"{gains['key']:+.2f} dB (median strike peak -6 dBFS); take space strokes "
        f"{gains['space']:+.2f} dB and the single space shot {gains['space_single']:+.2f} dB "
        f"(median loudness {BELOW_KEYS_DB['space']:.0f} dB below the strikes); shift thuds "
        f"{gains['thud']:+.2f} dB ({BELOW_KEYS_DB['modifier']:.0f} dB below); Enter "
        f"{gains['enter']:+.2f} dB ({BELOW_KEYS_DB['enter']:.0f} dB below). Loudness is the "
        "loudest 50 ms RMS. On the machine these sounds are much softer than a type bar "
        "hitting the platen; the offsets keep that order and make them audible.",
        "",
        f"Selection: the four takes contain {counts['strikes']} type-bar strikes; "
        f"{counts['qualifying']} are cleanly isolated (rejections: "
        + "; ".join(f"{k} {v}" for k, v in sorted(counts["reasons"].items()))
        + f"). The least typical 15% were dropped and {KEY_VARIANTS} mutually different "
        "strokes chosen from the rest, by spectrum and level. "
        f"{counts['space_qualifying']} space-bar strokes in the takes matched the single space "
        f"shot; {SPACE_VARIANTS} were chosen the same way. The {counts['pairs']} shift pairs are "
        "every capital letter in the takes with a clean thud before and after it. Releases: "
        "only the modifiers have release sounds, because only the shift's release is a "
        "separate sound in the recordings (a thud 0.2-0.25 s after the capital, when the "
        "basket drops back). A type bar's return and escapement clicks follow its strike by "
        "about 0.1 s and stay in the press sound; none is cut out as a release.",
        "",
        "| Pack file | Source | Cut from the high-passed source | Gain | SHA-256 of pack file |",
        "|---|---|---|---:|---|",
    ]
    for name, src, how, g in derivation:
        rel = f"sounds/{name}.wav"
        lines.append(f"| `{rel}` | {src} | {how} | {g:+.2f} dB | `{h(rel)}` |")
    ev = ", ".join(f"`{n}` at {t:.3f} s" for n, t in preview_events)
    lines += [
        "",
        f"`preview.wav` ({preview_len / SR:.3f} s, SHA-256 `{h('preview.wav')}`): rendered from "
        f"the pack's own files: {ev}. Each stroke at a random gain within +-"
        f"{VARIATION['volume'] * 100:.0f}% (fixed seed {PREVIEW_SEED}), summed, peak kept at or "
        f"below {CEILING_DB:.0f} dBFS.",
        "",
        "## Loudness",
        "",
        f"`volume` in `pack.json` is {VOLUME:.2f} ({float(db(VOLUME)):+.1f} dB): a playback gain "
        "the app applies to the whole pack, so no audio file changes. Typing loudness is "
        "matched across the bundled packs so that switching packs does not jump in volume; the "
        f"target is TakTak's reference typing level, {REFERENCE_LK:.1f} LK. Measured with "
        "`cargo run -p synth-packs --release -- loudness packs/typewriter` "
        "(`tools/synth-packs/src/loudness.rs`): the K-weighted (ITU-R BS.1770) energy of the "
        "first 100 ms of the sample each alphanumeric key plays on press by default, "
        "power-averaged over the keys, on the samples as the app loads them at 48 kHz. The "
        "pack reads "
        f"{LK_AS_BUILT:.1f} LK as built, so the reference would take a volume of "
        f"{VOLUME_FOR_REFERENCE:.2f}. Headroom caps it at {VOLUME:.2f}: the loudest file's true "
        f"peak ({TRUE_PEAK_DBFS:.2f} dBFS, 4x oversampled) must stay at or below "
        f"{MAX_TRUE_PEAK_DBFS:.0f} dBFS at the top of the +-{VARIATION['volume'] * 100:.0f}% "
        "volume variation, because the mixer clips hard at 0 dBFS (it reaches "
        f"{TRUE_PEAK_DBFS + float(db(VOLUME * (1 + VARIATION['volume']))):.2f} dBFS). Typing "
        f"plays at {LK_AS_BUILT + float(db(VOLUME)):.1f} LK, "
        f"{REFERENCE_LK - LK_AS_BUILT - float(db(VOLUME)):.1f} dB below the reference (within "
        "the 0.5 dB tolerance). `build.py` writes the volume (`VOLUME`), so a rebuild keeps it.",
        "",
        "## Key mapping",
        "",
        "- `alphanumeric`: the type-bar strikes (`key-*`): each key keeps one of them (with "
        "random variants on, one is picked at random per keystroke). Other keys without a group "
        "of their own (arrows, function keys, ...) fall back to them.",
        "- `space`: the space-bar strokes (`space-*`). `backspace` uses them too: a manual "
        "typewriter's backspace moves the carriage like the space bar and has no type bar.",
        "- `modifiers`: `shift-down-*` on press and `shift-up-*` on release.",
        "- `enter`: six carriage returns (`enter-return-*`) and three that ring the margin bell "
        "first (`enter-bell-*`). Each Enter key keeps one of them: Enter plays `{}`, so it rings "
        "the bell every time, and NumpadEnter plays `{}`, a plain return (with random variants "
        "on, both pick one of the nine at random per keystroke, and about one return in three "
        "rings the bell).".format(*(os.path.basename(f)[:-4] for f in enter_files.values())),
        "",
        "## Not used",
        "",
        "Downloaded for this pack on 2026-10-02 (CC0) but not used:",
        "",
        "- BigSoundBank 2835-2837 \"Typewriter #2-#4\" (same author, machine and license): stereo "
        "files whose channels are only weakly correlated (0.03-0.2) and offset by up to 0.5 ms, "
        "so a mono fold-down would comb-filter; their strokes are also brighter than the mono "
        "takes'. The mono takes have more than enough strokes.",
        "- BigSoundBank 2844 and 2845 \"Typewriter, Bell\": stand-alone bell rings, 2.4-2.6 s "
        "long and leveled on their own (peaks at 0 dBFS). The margin bells inside the takes are "
        "used instead: each comes with the carriage return the typist made after it, at the "
        "level it was recorded at.",
        "- BigSoundBank 1065 \"Typewriter #1\": a different recording setup (Tascam DR-40 and "
        "Sennheiser ME66, 16-bit) and typewriter model not stated.",
        "- OpenGameArt \"Typewriter sounds\" by Cassie-OrbitGames (typewriter1-8.wav, "
        "https://opengameart.org/content/typewriter-sounds, CC0, checked 2026-10-02): phone "
        "recordings of an unnamed typewriter at 22.05 kHz (nothing above 11 kHz; 7 of the 8 "
        "files are stereo). Each 0.28-0.69 s clip holds 6-14 separate mechanical events, and the "
        "loudest strike in each is preceded by 60-160 ms of other mechanism sounds, so no cut "
        "gives a clean stroke that starts on its strike. Picked at random next to the 48 kHz "
        "studio strikes above, they would also sound like a second, duller machine. Because "
        "nothing from them is used, `pack.json` credits only Joseph Sardin.",
        "",
        "## Levels",
        "",
        levels_table(out, manifest),
        "",
    ]
    with open(os.path.join(out, "SOURCES.md"), "w") as f:
        f.write("\n".join(lines))


def levels_table(out: str, manifest: dict) -> str:
    rows = ["| Set | Files | Mean duration (ms) | Peak (dBFS, max) | Loudest 50 ms RMS (dBFS, "
            "median) | Max leading silence (ms) |",
            "|---|---:|---:|---:|---:|---:|"]
    for group, sets in manifest["groups"].items():
        for action, rels in sets.items():
            ys = [read_wav(os.path.join(out, r)) for r in rels]
            rows.append(
                f"| {group}.{action} | {len(ys)} | {np.mean([len(y) for y in ys]) / SR * 1000:.0f} "
                f"| {max(float(db(np.abs(y).max())) for y in ys):.1f} "
                f"| {np.median([loudness_db(y) for y in ys]):.1f} "
                f"| {max(leading_silence_ms(y) for y in ys):.2f} |")
    y = read_wav(os.path.join(out, "preview.wav"))
    rows.append(f"| preview | 1 | {len(y) / SR * 1000:.0f} | {float(db(np.abs(y).max())):.1f} "
                f"| {loudness_db(y):.1f} | {leading_silence_ms(y):.2f} |")
    return "\n".join(rows)


if __name__ == "__main__":
    main()
