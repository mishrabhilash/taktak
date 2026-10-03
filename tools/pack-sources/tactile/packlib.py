"""Audio helpers shared by the TakTak pack-source build scripts.

Needs python3 with numpy, and ffmpeg/ffprobe on PATH (or in /opt/homebrew/bin). Nothing here
touches the network: the build scripts only read files that fetch.sh already downloaded.

Processing is deliberately minimal, so that each pack keeps the character of its recordings:

1. decode with ffmpeg to float, keep the source rate, average the channels to mono;
2. high-pass at 30 Hz (2nd-order Butterworth), which also removes DC;
3. split a recorded tap (key-down, then the key-up a little later in the same take) into
   its two strokes at the quietest point between them, when the build asks for it;
4. cut the leading silence, keeping 0.5 ms before the onset with a raised-cosine fade-in;
5. cut the tail where it has decayed into the noise floor, capped at a maximum length, with
   a raised-cosine fade-out that ends on an exact zero;
6. one gain per recording source (one for the whole pack when there is a single source):
   no per-file normalization, so the take-to-take variation is preserved (a build whose
   takes were recorded far apart may level each take's loudness, measured with k_loudness,
   into a band around the median take, keeping the variation within it); then, if needed,
   one trim for the whole pack so that the loudest file keeps 1 dB of true-peak headroom
   even at the top of the runtime's random volume variation (the mixer clips at 0 dBFS);
7. write 16-bit PCM WAV, mono, at the source's own sample rate.

This file is identical in every tools/pack-sources/<id>/ folder that uses it, so that each
pack can be rebuilt on its own.
"""

from __future__ import annotations

import hashlib
import json
import math
import shutil
import subprocess
import wave
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

FFMPEG = shutil.which("ffmpeg") or "/opt/homebrew/bin/ffmpeg"
FFPROBE = shutil.which("ffprobe") or "/opt/homebrew/bin/ffprobe"

# The loader's silence threshold (-50 dBFS) and pre-roll, from src-tauri/core/src/pack/decode.rs.
RUNTIME_SILENCE = 0.003_162_3
PRE_ROLL_S = 0.0005
# How long before its attack a key-up split from a tap starts (see split_tap).
RELEASE_LEAD_S = 0.005

HIGHPASS_HZ = 30.0
TARGET_MEDIAN_PEAK_DBFS = -6.0
CEILING_DBFS = -0.5
# The mixer clips hard at 0 dBFS and the loader resamples to the device rate, so the loudest
# file's true peak, raised by the pack's maximum random volume variation, must stay below this.
MAX_TRUE_PEAK_DBFS = -1.0
TRUE_PEAK_OVERSAMPLE = 4
# Typing loudness every bundled pack is matched to with its pack.json "volume": the level of
# the synthesized packs, as measured by `cargo run -p synth-packs --release -- loudness`.
REFERENCE_LK = -25.8
# One keystroke's loudness is measured over this fixed window from its start, as synth-packs
# does (tools/synth-packs/src/analysis.rs, k_energy).
LOUDNESS_WINDOW_S = 0.100
# ITU-R BS.1770 K-weighting as analogue prototypes (the parameters libebur128 uses): at 48 kHz
# they give BS.1770's published coefficients, and at any other rate the same response, so a
# file is measured at its own rate. Stage 1 is a high shelf (f0, gain dB, Q, shelf exponent),
# stage 2 the RLB high-pass (f0, Q).
K_SHELF = (1681.974450955533, 3.999843853973347, 0.7071752369554196, 0.4996667741545416)
K_HIGHPASS = (38.13547087602444, 0.5003270373238773)


def db(x: float) -> float:
    return 20.0 * math.log10(max(x, 1e-12))


def undb(d: float) -> float:
    return 10.0 ** (d / 20.0)


# ---------------------------------------------------------------- provenance


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git_blob_sha1(path: Path) -> str:
    """The SHA-1 GitHub shows for a file (`git hash-object`), to check a download."""
    data = path.read_bytes()
    h = hashlib.sha1(b"blob %d\0" % len(data))
    h.update(data)
    return h.hexdigest()


def verify_download(path: Path, size: int, blob_sha1: str) -> list[str]:
    """Problems with a downloaded file, or an empty list when it is the expected blob."""
    if not path.is_file():
        return [f"{path}: missing (run fetch.sh first)"]
    problems = []
    actual_size = path.stat().st_size
    if actual_size != size:
        problems.append(f"{path.name}: {actual_size} bytes, expected {size}")
    actual_sha = git_blob_sha1(path)
    if actual_sha != blob_sha1:
        problems.append(f"{path.name}: git blob {actual_sha}, expected {blob_sha1}")
    return problems


# ---------------------------------------------------------------- decoding


def probe(path: Path) -> dict:
    out = subprocess.run(
        [FFPROBE, "-v", "error", "-select_streams", "a:0",
         "-show_entries", "stream=codec_name,sample_rate,channels,duration",
         "-of", "json", str(path)],
        check=True, capture_output=True, text=True,
    ).stdout
    stream = json.loads(out)["streams"][0]
    return {
        "codec": stream.get("codec_name", "?"),
        "rate": int(stream["sample_rate"]),
        "channels": int(stream["channels"]),
        "duration": float(stream.get("duration", "nan")),
    }


def decode_mono(path: Path, rate: int) -> np.ndarray:
    """Decode the first audio stream to float64 mono at `rate` (ffmpeg resamples if needed).

    Channels are averaged. ffmpeg removes the MP3 encoder delay when the file has a LAME/Xing
    header, so MP3 sources stay time-aligned.
    """
    channels = probe(path)["channels"]
    raw = subprocess.run(
        [FFMPEG, "-v", "error", "-nostdin", "-i", str(path), "-map", "0:a:0",
         "-f", "f32le", "-acodec", "pcm_f32le", "-ar", str(rate), "-"],
        check=True, capture_output=True,
    ).stdout
    x = np.frombuffer(raw, dtype="<f4").astype(np.float64)
    return x.reshape(-1, channels).mean(axis=1)


# ---------------------------------------------------------------- processing


def highpass(x: np.ndarray, rate: int, fc: float = HIGHPASS_HZ) -> np.ndarray:
    """2nd-order Butterworth high-pass (RBJ biquad), started in its DC steady state.

    Starting as if x[0] had always been present means a DC offset produces no step at t=0.
    """
    w0 = 2.0 * math.pi * fc / rate
    alpha = math.sin(w0) / (2.0 * math.sqrt(0.5))
    cosw = math.cos(w0)
    a0 = 1.0 + alpha
    b0 = (1.0 + cosw) / 2.0 / a0
    b1 = -(1.0 + cosw) / a0
    b2 = b0
    a1 = -2.0 * cosw / a0
    a2 = (1.0 - alpha) / a0
    y = np.empty_like(x)
    x1 = x2 = float(x[0]) if len(x) else 0.0
    y1 = y2 = 0.0
    for i, xi in enumerate(x.tolist()):
        yi = b0 * xi + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        y[i] = yi
        x2, x1 = x1, xi
        y2, y1 = y1, yi
    return y


def window_rms(x: np.ndarray, n: int) -> np.ndarray:
    """RMS of every window x[i:i+n] (i = 0 .. len-n)."""
    n = max(1, min(n, len(x)))
    c = np.concatenate(([0.0], np.cumsum(x * x)))
    return np.sqrt(np.maximum(c[n:] - c[:-n], 0.0) / n)


def noise_floor(x: np.ndarray, rate: int) -> float:
    """The 5th percentile of the 5 ms window RMS (at least 1e-7)."""
    return max(float(np.percentile(window_rms(x, round(0.005 * rate)), 5)), 1e-7)


def raised_cosine(n: int) -> np.ndarray:
    """n samples rising from 0 (exclusive of 1) towards 1: sin^2 ramp."""
    if n <= 0:
        return np.zeros(0)
    t = np.arange(n) / n
    return np.sin(0.5 * math.pi * t) ** 2


@dataclass
class Cut:
    """Where a sample was cut from its decoded source (sample indices at `rate`)."""

    rate: int
    start: int
    onset: int
    end: int
    fade_in: int
    fade_out: int
    noise_floor_dbfs: float
    capped: bool

    def describe(self) -> str:
        ms = lambda n: f"{1000.0 * n / self.rate:.1f} ms"  # noqa: E731
        text = (f"kept {ms(self.start)}–{ms(self.end)} of the decoded source "
                f"(onset {ms(self.onset)}; {ms(self.fade_in)} fade-in, "
                f"{ms(self.fade_out)} fade-out to zero")
        if self.capped:
            text += "; length capped"
        return text + ")"


def cut(x: np.ndarray, rate: int, max_s: float, gain: float,
        fade_out_s: float = 0.008, min_onset: int = 0,
        floor: float | None = None) -> tuple[np.ndarray, Cut]:
    """Trim leading silence and the noise-floor tail, then fade both ends.

    `gain` is the linear gain the caller will apply afterwards (the result is not scaled).
    Onset = first sample at or after `min_onset` that will reach the loader's -50 dBFS
    silence threshold after `gain`, and at least noise floor + 12 dB, so the loader finds at
    most PRE_ROLL_S of leading silence. The noise floor is the 5th percentile of the 5 ms
    window RMS (`noise_floor`), unless the caller passes `floor`: the two strokes split from
    one tap use the whole take's floor, as a key-up's own few ms of tail need not reach it.
    A caller that starts x in the middle of a sound passes min_onset = the pre-roll, so the
    result still fades in from zero over the full pre-roll.
    The end is the last 5 ms window whose RMS is above max(noise floor + 6 dB, peak - 60 dB).
    """
    peak = float(np.max(np.abs(x))) if len(x) else 0.0
    if peak <= 0.0:
        raise ValueError("silent input")
    w5 = max(1, round(0.005 * rate))
    rms5 = window_rms(x, w5)
    floor = noise_floor(x, rate) if floor is None else max(floor, 1e-7)

    # 2% above the threshold so that 16-bit rounding cannot push the onset sample below it.
    on_thr = max(RUNTIME_SILENCE * 1.02 / gain, floor * undb(12.0))
    above = np.nonzero(np.abs(x[min_onset:]) >= on_thr)[0] + min_onset
    onset = int(above[0]) if len(above) else min_onset + int(np.argmax(np.abs(x[min_onset:])))
    pre = round(PRE_ROLL_S * rate)
    start = max(0, onset - pre)

    tail_thr = max(floor * undb(6.0), peak * undb(-60.0))
    loud = np.nonzero(rms5[onset:] > tail_thr)[0]
    end = onset + (int(loud[-1]) + w5 if len(loud) else w5)
    end = min(end, len(x))
    max_len = round(max_s * rate)
    capped = end - start > max_len
    if capped:
        end = start + max_len

    y = x[start:end].copy()
    fin = onset - start
    y[:fin] *= raised_cosine(fin)
    fout = min(round(fade_out_s * rate), max(1, (end - onset) // 4))
    fout = max(fout, min(round(0.005 * rate), len(y) // 2))
    y[len(y) - fout:] *= raised_cosine(fout)[::-1]
    y[-1] = 0.0
    return y, Cut(rate, start, onset, end, fin, fout, db(floor), capped)


@dataclass
class TapSplit:
    """Where a recorded tap divides into its key-down and key-up (sample indices)."""

    rate: int
    down: int  # first sample of the key-down at or above -30 dB re the take's peak
    split: int  # x[:split] is the key-down, x[split:] the key-up
    up_attack: int  # where the key-up's attack starts (20 dB below its peak)
    up_peak: int
    up_re_take_db: float  # key-up peak re the take's peak
    gap_re_up_db: float  # RMS of the quietest 5 ms between the strokes, re the key-up peak
    release_from: int  # the key-up sound starts here: max(split, up_attack - RELEASE_LEAD_S)

    def describe(self) -> str:
        ms = lambda n: f"{1000.0 * n / self.rate:.1f} ms"  # noqa: E731
        text = (f"key-down at {ms(self.down)}, key-up attack at {ms(self.up_attack)} "
                f"({self.up_re_take_db:+.1f} dB re the take's peak); split at {ms(self.split)}, "
                f"in a gap {-self.gap_re_up_db:.0f} dB below the key-up")
        if self.release_from > self.split:
            text += (f"; {ms(self.release_from - self.split)} between the split and the "
                     "key-up's start left out")
        return text


def dwell_text(splits: list[TapSplit]) -> tuple[str, str]:
    """For SOURCES.md: the range of key-down to key-up times, and the most left out before a
    key-up, both in words."""
    dwell = [1000.0 * (s.up_attack - s.down) / s.rate for s in splits]
    left = max(1000.0 * (s.release_from - s.split) / s.rate for s in splits)
    lo, hi = 10 * math.floor(min(dwell) / 10), 10 * math.ceil(max(dwell) / 10)
    return f"{lo:.0f}–{hi:.0f} ms", f"{left:.0f} ms"


def split_tap(x: np.ndarray, rate: int, min_dwell_s: float = 0.060,
              min_gap_db: float = 24.0) -> TapSplit | None:
    """Find the key-up in a take that recorded a whole tap (key-down, then key-up).

    The key-up is the loudest moment at least `min_dwell_s` after the key-down starts; its
    attack starts where, walking back from that peak in 1 ms frames, the level first drops
    20 dB below it. The split is the middle of the quietest 5 ms window between 30 ms after
    the key-down and 1 ms before that attack. Returns None unless that window is at least
    `min_gap_db` (RMS) below the key-up's peak and the key-up is no more than 30 dB below the
    take's peak, i.e. unless the take clearly holds two separate strokes.

    The key-down is x[:split]. The key-up sound starts at release_from, RELEASE_LEAD_S before
    its attack (or at the split, if that is later): a soft noise that some takes have between
    the strokes would otherwise start the key-up sample tens of ms before its click, which
    is latency.
    """
    a = np.abs(x)
    take_peak = peak(x)
    if take_peak <= 0.0:
        return None
    down = int(np.nonzero(a >= take_peak * undb(-30.0))[0][0])
    lo = down + round(min_dwell_s * rate)
    if lo >= len(x):
        return None
    up_peak = lo + int(np.argmax(a[lo:]))
    up_level = float(a[up_peak])
    w = max(1, round(0.001 * rate))
    k = up_peak // w
    while k > 0 and float(np.max(a[k * w:(k + 1) * w])) >= up_level * undb(-20.0):
        k -= 1
    up_attack = (k + 1) * w
    n5 = max(1, round(0.005 * rate))
    rms5 = window_rms(x, n5)
    s0 = down + round(0.030 * rate)
    s1 = up_attack - round(0.001 * rate) - n5
    if s1 <= s0:
        return None
    q = s0 + int(np.argmin(rms5[s0:s1 + 1]))
    gap_re_up = db(float(rms5[q]) / up_level)
    up_re_take = db(up_level / take_peak)
    if gap_re_up > -min_gap_db or up_re_take < -30.0:
        return None
    split = q + n5 // 2
    release_from = max(split, up_attack - round(RELEASE_LEAD_S * rate))
    return TapSplit(rate, down, split, up_attack, up_peak, up_re_take, gap_re_up, release_from)


def peak(x: np.ndarray) -> float:
    return float(np.max(np.abs(x))) if len(x) else 0.0


def true_peak(x: np.ndarray) -> float:
    """Peak of x after band-limited 4x oversampling: what resampling can reach between samples.

    Zero-padded to twice the length first, so that silence is assumed before and after x (as
    when the sample plays) and the two ends do not wrap into each other.
    """
    if not len(x):
        return 0.0
    size = 1 << (2 * len(x) - 1).bit_length()
    k = TRUE_PEAK_OVERSAMPLE
    y = np.fft.irfft(np.fft.rfft(x, size), size * k) * k
    return max(peak(x), float(np.max(np.abs(y[: len(x) * k]))))


def biquad(x: np.ndarray, b0: float, b1: float, b2: float, a1: float, a2: float) -> np.ndarray:
    """Direct-form I biquad from a zero state (a0 normalized to 1)."""
    y = np.empty_like(x)
    x1 = x2 = y1 = y2 = 0.0
    for i, xi in enumerate(x.tolist()):
        yi = b0 * xi + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        y[i] = yi
        x2, x1 = x1, xi
        y2, y1 = y1, yi
    return y


def k_weighting(rate: int) -> list[tuple[float, float, float, float, float]]:
    """The two BS.1770 K-weighting biquads (b0, b1, b2, a1, a2) at `rate`."""
    f0, gain_db, q, exponent = K_SHELF
    k = math.tan(math.pi * f0 / rate)
    vh = undb(gain_db)
    vb = vh ** exponent
    a0 = 1.0 + k / q + k * k
    shelf = ((vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0,
             (vh - vb * k / q + k * k) / a0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0)
    f0, q = K_HIGHPASS
    k = math.tan(math.pi * f0 / rate)
    a0 = 1.0 + k / q + k * k
    highpass_rlb = (1.0, -2.0, 1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0)
    return [shelf, highpass_rlb]


def k_loudness(x: np.ndarray, rate: int) -> float:
    """One keystroke's loudness in LK: the K-weighted (ITU-R BS.1770) mean square of its first
    LOUDNESS_WINDOW_S, always divided by the full window length (a shorter sound counts as
    silence after its end), with BS.1770's -0.691 dB offset.

    This is the measure `synth-packs loudness` applies per sample (analysis.rs, k_energy and
    k_db), here at the file's own rate. On a file that starts 0.5 ms before its onset, as the
    loader trims it, the two agree to a few hundredths of a dB.
    """
    n = round(LOUDNESS_WINDOW_S * rate)
    y = np.asarray(x[:n], dtype=np.float64)
    for coefficients in k_weighting(rate):
        y = biquad(y, *coefficients)
    return -0.691 + 10.0 * math.log10(max(float(np.sum(y * y)) / n, 1e-30))


def power_mean_db(levels_db: list[float]) -> float:
    """Power average of levels given in dB (how synth-packs averages keystroke loudness)."""
    return 10.0 * math.log10(sum(10.0 ** (v / 10.0) for v in levels_db) / len(levels_db))


def loudness_text(pack_id: str, volume: float, lk_as_built: float, volume_variation: float,
                  true_peak: float, wanted: float | None = None) -> str:
    """The SOURCES.md paragraph that records a pack's playback `volume` and how it was chosen.

    `lk_as_built` is the typing loudness at volume 1.0 and `true_peak` the loudest file's true
    peak, both measured on the built pack with `synth-packs loudness`. `wanted` is the volume
    that would reach REFERENCE_LK when headroom allows less.
    """
    text = (
        f"`volume` in `pack.json` is {volume:.2f} ({db(volume):+.1f} dB): a playback gain the "
        "app applies to the whole pack, so no audio file changes. Typing loudness is matched "
        "across the bundled packs so that switching packs does not jump in volume; the "
        f"reference is the synthesized packs' {REFERENCE_LK:.1f} LK. Measured with "
        f"`cargo run -p synth-packs --release -- loudness packs/{pack_id}` "
        "(`tools/synth-packs/src/loudness.rs`): the K-weighted (ITU-R BS.1770) energy of the "
        "first 100 ms of the sample each alphanumeric key plays on press by default, "
        "power-averaged over the keys, on the samples as the app loads them at 48 kHz. The "
        "pack reads "
        f"{lk_as_built:.1f} LK as built"
    )
    peak_at_volume = db(true_peak * volume * (1.0 + volume_variation))
    if wanted is None:
        text += (
            f" and {lk_as_built + db(volume):.1f} LK at this volume; the loudest true peak at "
            f"the top of the volume variation becomes {peak_at_volume:.2f} dBFS."
        )
    else:
        residual = lk_as_built + db(volume) - REFERENCE_LK
        text += (
            f", so the reference would take a volume of {wanted:.2f}. Headroom caps it at "
            f"{volume:.2f}: the loudest file's true peak must stay at or below "
            f"{MAX_TRUE_PEAK_DBFS:.0f} dBFS at the top of the ±{volume_variation:.0%} volume "
            f"variation (it is {peak_at_volume:.2f} dBFS), because the mixer clips hard at "
            f"0 dBFS. Typing therefore plays at {lk_as_built + db(volume):.1f} LK, "
            f"{-residual:.1f} dB below the reference; nothing is limited or re-normalized to "
            "close the gap."
        )
    return text + " `build.py` writes the volume (`VOLUME`), so a rebuild keeps it."


def headroom_trim(signals: list[np.ndarray], volume_variation: float) -> float:
    """One gain factor (at most 1) for a whole pack, from its signals at their final gains.

    The loudest true peak, raised by the runtime's maximum random volume boost
    (1 + volume_variation), ends at or below MAX_TRUE_PEAK_DBFS.
    """
    loudest = max(true_peak(x) for x in signals) * (1.0 + volume_variation)
    return min(1.0, undb(MAX_TRUE_PEAK_DBFS) / loudest)


def to_int16(x: np.ndarray) -> np.ndarray:
    if peak(x) > 1.0:
        raise ValueError("sample would clip")
    return np.round(x * 32767.0).astype("<i2")


def write_wav(path: Path, x: np.ndarray, rate: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(to_int16(x).tobytes())


def read_wav16(path: Path) -> tuple[np.ndarray, int]:
    with wave.open(str(path), "rb") as w:
        assert w.getsampwidth() == 2 and w.getnchannels() == 1, path
        rate = w.getframerate()
        data = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2")
    return data.astype(np.float64) / 32768.0, rate


# ---------------------------------------------------------------- analysis


def normalized_xcorr_max(a: np.ndarray, b: np.ndarray) -> float:
    """Max over lags of the normalized cross-correlation of a and b (1.0 = same waveform)."""
    n = len(a) + len(b) - 1
    size = 1 << (n - 1).bit_length()
    fa = np.fft.rfft(a, size)
    fb = np.fft.rfft(b, size)
    xc = np.fft.irfft(fa * np.conj(fb), size)
    denom = math.sqrt(float(np.dot(a, a)) * float(np.dot(b, b)))
    return float(np.max(np.abs(xc)) / denom) if denom > 0 else 0.0


def spectral_centroid(x: np.ndarray, rate: int) -> float:
    spec = np.abs(np.fft.rfft(x * np.hanning(len(x))))
    freqs = np.fft.rfftfreq(len(x), 1.0 / rate)
    total = float(spec.sum())
    return float((spec * freqs).sum() / total) if total > 0 else 0.0


@dataclass
class GroupStats:
    count: int = 0
    durations_ms: list[float] = field(default_factory=list)
    peaks: list[float] = field(default_factory=list)
    rms: list[float] = field(default_factory=list)
    lead_ms: list[float] = field(default_factory=list)


def leading_silence_ms(x: np.ndarray, rate: int) -> float:
    above = np.nonzero(np.abs(x) >= RUNTIME_SILENCE)[0]
    n = int(above[0]) if len(above) else len(x)
    return 1000.0 * n / rate


def stats_table(pack_dir: Path, sets: dict[str, list[str]]) -> str:
    """Markdown table: per sound set, count, mean duration, peak, true peak, RMS and max
    leading silence.

    `sets` maps a label such as "alphanumeric.press" to the pack-relative files it uses.
    """
    rows = ["| Set | Files | Mean duration (ms) | Peak (dBFS, max) | True peak (dBFS, max) "
            "| RMS (dBFS, mean) | Max leading silence (ms) |",
            "|---|---:|---:|---:|---:|---:|---:|"]
    for label, files in sets.items():
        g = GroupStats()
        true_peaks = []
        for rel in files:
            x, rate = read_wav16(pack_dir / rel)
            g.count += 1
            g.durations_ms.append(1000.0 * len(x) / rate)
            g.peaks.append(peak(x))
            true_peaks.append(true_peak(x))
            g.rms.append(float(np.sqrt(np.mean(x * x))))
            g.lead_ms.append(leading_silence_ms(x, rate))
        if not g.count:
            continue
        rows.append(
            f"| {label} | {g.count} | {np.mean(g.durations_ms):.0f} | {db(max(g.peaks)):.1f} "
            f"| {db(max(true_peaks)):.1f} | {db(float(np.mean(g.rms))):.1f} "
            f"| {max(g.lead_ms):.2f} |"
        )
    return "\n".join(rows)


# ---------------------------------------------------------------- preview


def render_preview(
    rate: int,
    script: list[str],
    sounds: dict[str, list[np.ndarray]],
    seed: int,
    volume_variation: float,
    max_s: float = 1.95,
) -> np.ndarray:
    """About 2 s of human-timed typing, never longer than `max_s` (the loader's limit is 2 s).

    `script` is a list of tokens: "k" for an ordinary key, "space", "enter", "backspace".
    `sounds` maps "<token>.press" / "<token>.release" to candidate samples; missing release
    lists make silent releases, and missing token lists fall back to "k.*".
    Holds are 60–120 ms, presses 80–200 ms apart, each stroke picks a random candidate and gets
    a random gain within ±volume_variation: like the runtime with random variants on and the
    full variation (humanize 1), not its defaults, where each key keeps one candidate and gets
    a quarter of the variation. Random draws come from `seed`; if a draw runs longer than
    `max_s`, the next seed is tried, so the result is deterministic.
    """
    for attempt in range(1000):
        rng = np.random.default_rng(seed + attempt)
        events: list[tuple[int, np.ndarray]] = []
        t = 0.01
        for token in script:
            hold = rng.uniform(0.060, 0.120)
            for action, at in (("press", t), ("release", t + hold)):
                pool = sounds.get(f"{token}.{action}") or sounds.get(f"k.{action}") or []
                if pool:
                    gain = 1.0 + rng.uniform(-volume_variation, volume_variation)
                    events.append((round(at * rate), pool[int(rng.integers(len(pool)))] * gain))
            t += rng.uniform(0.080, 0.200)
        length = max(i + len(x) for i, x in events) + round(0.02 * rate)
        if length <= max_s * rate:
            break
    else:
        raise ValueError("cannot fit the preview script into max_s")
    out = np.zeros(length)
    for i, x in events:
        out[i:i + len(x)] += x
    p = peak(out)
    if p > undb(-1.0):
        out *= undb(-1.0) / p
    out[-1] = 0.0
    return out
