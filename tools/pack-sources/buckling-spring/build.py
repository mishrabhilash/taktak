#!/usr/bin/env python3
"""Builds the TakTak "Buckling Spring" pack (id buckling-spring) from the bucklespring samples.

Source: bucklespring by Ico Doornekamp, wav/ at commit 63b3d5b8ea84b426b466146ced5cbf85fe3bc0cb,
the last commit whose LICENSE is MIT. Fetch the inputs with fetch.sh, then:

    python3 tools/pack-sources/buckling-spring/build.py --src DOWNLOAD_DIR [--out packs/buckling-spring]

The source names its files "%02x-%d.wav": the hex Linux evdev key code (rec.c reads
/dev/input/event*; scan-linux.c turns X11 keycodes back into evdev codes with "key -= 8") and
the event value, 1 = press and 0 = release (scan-linux.c calls play(key, 1) on KeyPress).

Only the Python 3.8+ standard library is used, and the build is deterministic: the same inputs
always give byte-identical outputs.
"""

import argparse
import array
import hashlib
import io
import json
import math
import random
import re
import statistics
import sys
import wave
from pathlib import Path

REPO = "zevv/bucklespring"
COMMIT = "63b3d5b8ea84b426b466146ced5cbf85fe3bc0cb"
SOURCE_PAGE = f"https://github.com/{REPO}/tree/{COMMIT}/wav"
RAW_BASE = f"https://raw.githubusercontent.com/{REPO}/{COMMIT}/"
LICENSE_PAGE = f"https://github.com/{REPO}/blob/{COMMIT}/LICENSE"
DATE_CHECKED = "2026-10-02"

PACK_ID = "buckling-spring"
# Playback level, written to pack.json as "volume"; it changes no audio file. It matches the
# pack's typing loudness to the synthesized packs (REFERENCE_LK), so switching packs does not
# jump in volume. Measured on the built pack with
#     cargo run -p synth-packs --release -- loudness packs/buckling-spring
# (tools/synth-packs/src/loudness.rs): LK_AS_BUILT at volume 1.0. Re-measure whenever the
# processing below changes.
VOLUME = 0.96
LK_AS_BUILT = -25.47
REFERENCE_LK = -25.8
PACK = {
    "format": 1,
    "id": PACK_ID,
    "name": "Buckling Spring",
    "version": "1.0.0",
    "author": "Ico Doornekamp, packaged by TakTak contributors",
    "license": "MIT",
    "description": (
        "The ping and clack of a buckling-spring keyboard, every key sampled on the way down "
        "and on the way back up, recorded by Ico Doornekamp on his IBM Model M space saver."
    ),
    "source": SOURCE_PAGE,
    "attribution": "Buckling-spring keyboard samples by Ico Doornekamp (bucklespring), MIT License",
    "preview": "preview.wav",
    "volume": VOLUME,
    "trim_silence": True,
    # One natural take per key, so a little runtime variation keeps repeats alive.
    "variation": {"pitch": 0.015, "volume": 0.06},
}

# Processing parameters (documented in SOURCES.md).
DC_HPF_HZ = 25.0  # 2nd-order Butterworth high-pass, after removing the file mean
# Tail detection only (the audio keeps everything above DC_HPF_HZ): a 4th-order high-pass, so
# the sub-audible rumble that some takes carry cannot hold a tail open.
DETECT_HPF_HZ = 100.0
RUMBLE_SHARE = 0.75  # a take "rumbles" when more of its energy after 100 ms is below that
FRAME_S = 0.001  # envelope frame
FLOOR_PERCENTILE = 0.10  # noise floor = this percentile of the frame RMS values
ONSET_FLOOR_X = 5.0  # onset: first sample >= max(5 x floor RMS, peak - 40 dB)
ONSET_REL_DB = -40.0
PREROLL_S = 0.0005  # kept before the onset, faded in from zero
TAIL_FLOOR_X = 2.0  # tail: first 20 ms stretch with frame RMS < 2 x floor (+6 dB)
TAIL_HOLD_S = 0.020
FADE_OUT_S = 0.008  # half-cosine fade to exact zero, laid over the start of the quiet stretch
MAX_LEN_S = 0.400
TARGET_MEDIAN_PRESS_PEAK_DBFS = -6.0
CEILING_DBFS = -0.5  # preview mix
# The mixer clips hard at 0 dBFS and the loader resamples to the device rate, so the loudest
# sample's true peak, raised by the maximum random volume variation, must stay below this.
MAX_TRUE_PEAK_DBFS = -1.0
TP_OVERSAMPLE = 4  # true peak: band-limited 4x interpolation between samples
TP_HALF_TAPS = 32  # Hann-windowed sinc, 64 taps
SIZE_BUDGET = 3_000_000  # bytes on disk for the whole pack
TAIL_REL_STEPS = [None, -60.0, -54.0, -48.0, -42.0, -36.0]  # tighter tails only if over budget
LOADER_SILENCE = 10 ** (-50 / 20)  # the loader's leading-silence threshold

# Linux evdev KEY_* codes (include/uapi/linux/input-event-codes.h) -> KeyboardEvent.code.
EVDEV = {
    0x01: ("KEY_ESC", "Escape"),
    0x02: ("KEY_1", "Digit1"), 0x03: ("KEY_2", "Digit2"), 0x04: ("KEY_3", "Digit3"),
    0x05: ("KEY_4", "Digit4"), 0x06: ("KEY_5", "Digit5"), 0x07: ("KEY_6", "Digit6"),
    0x08: ("KEY_7", "Digit7"), 0x09: ("KEY_8", "Digit8"), 0x0A: ("KEY_9", "Digit9"),
    0x0B: ("KEY_0", "Digit0"),
    0x0C: ("KEY_MINUS", "Minus"), 0x0D: ("KEY_EQUAL", "Equal"),
    0x0E: ("KEY_BACKSPACE", "Backspace"), 0x0F: ("KEY_TAB", "Tab"),
    0x10: ("KEY_Q", "KeyQ"), 0x11: ("KEY_W", "KeyW"), 0x12: ("KEY_E", "KeyE"),
    0x13: ("KEY_R", "KeyR"), 0x14: ("KEY_T", "KeyT"), 0x15: ("KEY_Y", "KeyY"),
    0x16: ("KEY_U", "KeyU"), 0x17: ("KEY_I", "KeyI"), 0x18: ("KEY_O", "KeyO"),
    0x19: ("KEY_P", "KeyP"),
    0x1A: ("KEY_LEFTBRACE", "BracketLeft"), 0x1B: ("KEY_RIGHTBRACE", "BracketRight"),
    0x1C: ("KEY_ENTER", "Enter"), 0x1D: ("KEY_LEFTCTRL", "ControlLeft"),
    0x1E: ("KEY_A", "KeyA"), 0x1F: ("KEY_S", "KeyS"), 0x20: ("KEY_D", "KeyD"),
    0x21: ("KEY_F", "KeyF"), 0x22: ("KEY_G", "KeyG"), 0x23: ("KEY_H", "KeyH"),
    0x24: ("KEY_J", "KeyJ"), 0x25: ("KEY_K", "KeyK"), 0x26: ("KEY_L", "KeyL"),
    0x27: ("KEY_SEMICOLON", "Semicolon"), 0x28: ("KEY_APOSTROPHE", "Quote"),
    0x29: ("KEY_GRAVE", "Backquote"), 0x2A: ("KEY_LEFTSHIFT", "ShiftLeft"),
    0x2B: ("KEY_BACKSLASH", "Backslash"),
    0x2C: ("KEY_Z", "KeyZ"), 0x2D: ("KEY_X", "KeyX"), 0x2E: ("KEY_C", "KeyC"),
    0x2F: ("KEY_V", "KeyV"), 0x30: ("KEY_B", "KeyB"), 0x31: ("KEY_N", "KeyN"),
    0x32: ("KEY_M", "KeyM"),
    0x33: ("KEY_COMMA", "Comma"), 0x34: ("KEY_DOT", "Period"), 0x35: ("KEY_SLASH", "Slash"),
    0x36: ("KEY_RIGHTSHIFT", "ShiftRight"), 0x37: ("KEY_KPASTERISK", "NumpadMultiply"),
    0x38: ("KEY_LEFTALT", "AltLeft"), 0x39: ("KEY_SPACE", "Space"),
    0x3A: ("KEY_CAPSLOCK", "CapsLock"),
    0x3B: ("KEY_F1", "F1"), 0x3C: ("KEY_F2", "F2"), 0x3D: ("KEY_F3", "F3"),
    0x3E: ("KEY_F4", "F4"), 0x3F: ("KEY_F5", "F5"), 0x40: ("KEY_F6", "F6"),
    0x41: ("KEY_F7", "F7"), 0x42: ("KEY_F8", "F8"), 0x43: ("KEY_F9", "F9"),
    0x44: ("KEY_F10", "F10"),
    0x45: ("KEY_NUMLOCK", "NumLock"), 0x46: ("KEY_SCROLLLOCK", "ScrollLock"),
    0x47: ("KEY_KP7", "Numpad7"), 0x48: ("KEY_KP8", "Numpad8"), 0x49: ("KEY_KP9", "Numpad9"),
    0x4A: ("KEY_KPMINUS", "NumpadSubtract"),
    0x4B: ("KEY_KP4", "Numpad4"), 0x4C: ("KEY_KP5", "Numpad5"), 0x4D: ("KEY_KP6", "Numpad6"),
    0x4E: ("KEY_KPPLUS", "NumpadAdd"),
    0x4F: ("KEY_KP1", "Numpad1"), 0x50: ("KEY_KP2", "Numpad2"), 0x51: ("KEY_KP3", "Numpad3"),
    0x52: ("KEY_KP0", "Numpad0"), 0x53: ("KEY_KPDOT", "NumpadDecimal"),
    0x56: ("KEY_102ND", "IntlBackslash"),
    0x57: ("KEY_F11", "F11"), 0x58: ("KEY_F12", "F12"),
    0x60: ("KEY_KPENTER", "NumpadEnter"), 0x61: ("KEY_RIGHTCTRL", "ControlRight"),
    0x62: ("KEY_KPSLASH", "NumpadDivide"), 0x63: ("KEY_SYSRQ", "PrintScreen"),
    0x64: ("KEY_RIGHTALT", "AltRight"),
    0x66: ("KEY_HOME", "Home"), 0x67: ("KEY_UP", "ArrowUp"), 0x68: ("KEY_PAGEUP", "PageUp"),
    0x69: ("KEY_LEFT", "ArrowLeft"), 0x6A: ("KEY_RIGHT", "ArrowRight"),
    0x6B: ("KEY_END", "End"), 0x6C: ("KEY_DOWN", "ArrowDown"),
    0x6D: ("KEY_PAGEDOWN", "PageDown"), 0x6E: ("KEY_INSERT", "Insert"),
    0x6F: ("KEY_DELETE", "Delete"),
    0x75: ("KEY_KPEQUAL", "NumpadEqual"), 0x77: ("KEY_PAUSE", "Pause"),
    0x7D: ("KEY_LEFTMETA", "MetaLeft"), 0x7E: ("KEY_RIGHTMETA", "MetaRight"),
    0x7F: ("KEY_COMPOSE", "ContextMenu"),
}

# Codes in the snapshot that are not evdev codes of a real key on the recorded keyboard.
SKIP = {
    0x5B: (
        "not an evdev code here: 0x5b is the Windows key's PC scan code (set 1) that "
        "bucklespring sees on win32 (commit 688a522, \"Added windows key on win32 (0x5b)\"); "
        "as an evdev code it would be KEY_HIRAGANA. The file is byte-identical to 64-*.wav "
        "(Right Alt), which MetaLeft (evdev 0x7d) already uses."
    ),
}

GROUP_OF = {}
for _k in ("Backquote Minus Equal BracketLeft BracketRight Backslash Semicolon Quote Comma "
           "Period Slash IntlBackslash IntlRo IntlYen").split():
    GROUP_OF[_k] = "alphanumeric"
for _c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ":
    GROUP_OF["Key" + _c] = "alphanumeric"
for _d in "0123456789":
    GROUP_OF["Digit" + _d] = "alphanumeric"
GROUP_OF.update({"Space": "space", "Enter": "enter", "NumpadEnter": "enter",
                 "Backspace": "backspace", "Delete": "backspace"})
for _k in ("ShiftLeft ShiftRight ControlLeft ControlRight AltLeft AltRight MetaLeft MetaRight "
           "CapsLock Fn").split():
    GROUP_OF[_k] = "modifiers"
GROUP_ORDER = ["alphanumeric", "space", "enter", "backspace", "modifiers", "other"]


def group_of(code_name):
    return GROUP_OF.get(code_name, "other")


# Group fallbacks for keys the source has no sample of (IntlBackslash, numpad, MetaRight, …).
# They reuse per-key files, so they cost no disk space.
FALLBACK_GROUPS = {
    "alphanumeric": ["KeyA", "KeyS", "KeyD", "KeyF", "KeyJ", "KeyK", "KeyL"],
    "enter": ["Enter"],
    # bucklespring itself plays the Right Alt sample for the Windows/Meta key.
    "modifiers": ["AltLeft", "AltRight"],
}

PREVIEW_KEYS = ["KeyI", "KeyT", "Space", "KeyI", "KeyS", "Space", "KeyO", "KeyK", "Enter"]
PREVIEW_SEED = 1986
PREVIEW_MAX_S = 1.95


def db_to_gain(db):
    return 10 ** (db / 20)


def dbfs(v):
    return 20 * math.log10(v) if v > 0 else float("-inf")


def _tp_kernels():
    """Windowed-sinc taps for the points k/TP_OVERSAMPLE (k = 1 .. TP_OVERSAMPLE-1) after a
    sample, applied to the samples at offsets -TP_HALF_TAPS+1 .. TP_HALF_TAPS."""
    h, kernels = TP_HALF_TAPS, []
    for k in range(1, TP_OVERSAMPLE):
        frac = k / TP_OVERSAMPLE
        taps = []
        for m in range(-h + 1, h + 1):
            d = frac - m
            taps.append(math.sin(math.pi * d) / (math.pi * d)
                        * 0.5 * (1 + math.cos(math.pi * d / h)))
        kernels.append(taps)
    return kernels


TP_KERNELS = _tp_kernels()


def true_peak(x):
    """Peak of x including the peaks between samples that resampling can reach (silence is
    assumed around x, as when it plays). Only the neighbourhoods of the local maxima at or
    above half the sample peak are interpolated: that is where an intersample peak can top
    the sample peak."""
    if not x:
        return 0.0
    h = TP_HALF_TAPS
    peak = max(abs(v) for v in x)
    best = peak
    padded = [0.0] * h + list(x) + [0.0] * h  # padded[i + h] == x[i]
    for i, v in enumerate(x):
        a = abs(v)
        if a < 0.5 * peak or a < abs(padded[i + h - 1]) or a < abs(padded[i + h + 1]):
            continue
        for base in (i - 1, i):  # the points between base and base + 1
            window = padded[base + 1:base + 2 * h + 1]
            for taps in TP_KERNELS:
                best = max(best, abs(sum(t * w for t, w in zip(taps, window))))
    return best


def full_scale_runs(vals):
    """Samples at digital full scale in the source (the recording clipped there): the count
    and the longest run of consecutive ones."""
    count = run = longest = 0
    for v in vals:
        if abs(v) >= 32767 / 32768:
            count += 1
            run += 1
            longest = max(longest, run)
        else:
            run = 0
    return count, longest


def git_blob_sha1(data):
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


def read_wav(data, name):
    """Returns (rate, mono samples in [-1, 1), channels, bits) of a WAV file's bytes."""
    with wave.open(io.BytesIO(data), "rb") as w:
        channels, width, rate = w.getnchannels(), w.getsampwidth(), w.getframerate()
        raw = w.readframes(w.getnframes())
    if width == 1:
        vals = [(b - 128) / 128.0 for b in raw]
    elif width == 2:
        a = array.array("h")
        a.frombytes(raw)
        if sys.byteorder == "big":
            a.byteswap()
        vals = [v / 32768.0 for v in a]
    elif width == 3:
        vals = [int.from_bytes(raw[i:i + 3], "little", signed=True) / 8388608.0
                for i in range(0, len(raw), 3)]
    elif width == 4:
        a = array.array("i")
        a.frombytes(raw)
        if sys.byteorder == "big":
            a.byteswap()
        vals = [v / 2147483648.0 for v in a]
    else:
        raise ValueError(f"{name}: unsupported sample width {width}")
    if channels > 1:
        vals = [sum(vals[i:i + channels]) / channels for i in range(0, len(vals), channels)]
    return rate, vals, channels, width * 8


def write_wav16(path, rate, ints):
    a = array.array("h", ints)
    if sys.byteorder == "big":
        a.byteswap()
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(a.tobytes())


def remove_dc(x, rate):
    """Subtracts the mean, then a causal 2nd-order Butterworth high-pass (no pre-ringing)."""
    mean = sum(x) / len(x)
    w0 = 2 * math.pi * DC_HPF_HZ / rate
    cw, alpha = math.cos(w0), math.sin(w0) / math.sqrt(2)  # Q = 1/sqrt(2)
    a0 = 1 + alpha
    b0, b1, b2 = (1 + cw) / 2 / a0, -(1 + cw) / a0, (1 + cw) / 2 / a0
    a1, a2 = -2 * cw / a0, (1 - alpha) / a0
    out = []
    x1 = x2 = y1 = y2 = 0.0
    for v in x:
        v -= mean
        y = b0 * v + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        x2, x1, y2, y1 = x1, v, y1, y
        out.append(y)
    return out


def highpass2(x, fc, rate):
    """A causal 2nd-order Butterworth high-pass (RBJ biquad, Q = 1/sqrt(2))."""
    w0 = 2 * math.pi * fc / rate
    cw, alpha = math.cos(w0), math.sin(w0) / math.sqrt(2)
    a0 = 1 + alpha
    b0, b1, b2 = (1 + cw) / 2 / a0, -(1 + cw) / a0, (1 + cw) / 2 / a0
    a1, a2 = -2 * cw / a0, (1 - alpha) / a0
    out = []
    x1 = x2 = y1 = y2 = 0.0
    for v in x:
        y = b0 * v + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        x2, x1, y2, y1 = x1, v, y1, y
        out.append(y)
    return out


def detection_band(x, rate):
    """x above DETECT_HPF_HZ (two 2nd-order sections): what the tail is measured on."""
    return highpass2(highpass2(x, DETECT_HPF_HZ, rate), DETECT_HPF_HZ, rate)


def rumbles(x, xd, rate):
    """Whether most of the take's energy after its first 100 ms is below DETECT_HPF_HZ."""
    i = round(0.1 * rate)
    full = sum(v * v for v in x[i:])
    above = sum(v * v for v in xd[i:])
    return full > 0 and above < (1 - RUMBLE_SHARE) * full


def frame_rms(x, flen):
    return [math.sqrt(sum(v * v for v in x[i:i + flen]) / len(x[i:i + flen]))
            for i in range(0, len(x), flen)]


def segment(x, xd, rate, tail_rel_db):
    """Finds where the keystroke starts (in x) and where it has decayed into the noise floor
    (in xd, the detection band of x)."""
    flen = max(1, round(rate * FRAME_S))
    rms = frame_rms(x, flen)
    floor = max(sorted(rms)[int(len(rms) * FLOOR_PERCENTILE)], 1e-5)
    peak_i = max(range(len(x)), key=lambda i: abs(x[i]))
    peak = abs(x[peak_i])

    thr_on = max(ONSET_FLOOR_X * floor, peak * db_to_gain(ONSET_REL_DB))
    onset = next(i for i, v in enumerate(x) if abs(v) >= thr_on)
    start = max(0, onset - round(rate * PREROLL_S))

    drms = frame_rms(xd, flen)
    dfloor = max(sorted(drms)[int(len(drms) * FLOOR_PERCENTILE)], 1e-5)
    dpeak = max(abs(v) for v in xd)
    quiet = TAIL_FLOOR_X * dfloor
    if tail_rel_db is not None:
        quiet = max(quiet, dpeak * db_to_gain(tail_rel_db))
    hold = round(TAIL_HOLD_S / FRAME_S)
    quiet_at, run = None, 0
    for f in range(peak_i // flen + 1, len(drms)):
        if drms[f] < quiet:
            run += 1
            if run >= hold:
                quiet_at = f - hold + 1
                break
        else:
            run = 0
    if quiet_at is None and run > 0:
        quiet_at = len(drms) - run  # decays into the floor right at the end of the file

    fade = round(rate * FADE_OUT_S)
    end = len(x) if quiet_at is None else min(len(x), quiet_at * flen + fade)
    capped = end - start > round(rate * MAX_LEN_S)
    if capped:
        end = start + round(rate * MAX_LEN_S)

    # A second event after the cut (e.g. a release caught in a press take) is dropped by the cut.
    later = drms[end // flen + 1:]
    late_event = bool(later) and max(later) > max(10 * dfloor, 0.1 * dpeak)
    return {
        "start": start, "onset": onset, "end": end, "floor": floor, "peak": peak,
        "decayed": quiet_at is not None, "capped": capped, "late_event": late_event,
    }


def render(x, seg, gain, rate):
    y = [v * gain for v in x[seg["start"]:seg["end"]]]
    fade_in = seg["onset"] - seg["start"]
    for i in range(fade_in):
        y[i] *= 0.5 * (1 - math.cos(math.pi * i / fade_in))
    fade = min(round(rate * FADE_OUT_S), len(y))
    for i in range(fade):
        y[len(y) - fade + i] *= 0.5 * (1 + math.cos(math.pi * (i + 1) / fade))
    return [max(-32768, min(32767, round(v * 32768))) for v in y]


def leading_silence_ms(ints, rate):
    thr = LOADER_SILENCE * 32768
    i = next((i for i, v in enumerate(ints) if abs(v) >= thr), len(ints))
    return 1000 * i / rate


def wav_bytes(n):
    return 44 + 2 * n


def preview_events(keys, rate, rendered, key_files):
    rng = random.Random(PREVIEW_SEED)
    events, t = [], 0.0
    for k in keys:
        dwell = rng.uniform(0.060, 0.120)
        events.append((t, key_files[k].get("press")))
        events.append((t + dwell, key_files[k].get("release")))
        t += dwell + rng.uniform(0.080, 0.200)
    total = max(round(when * rate) + len(rendered[f]) for when, f in events if f)
    return events, total


def build_preview(rendered, rate, key_files):
    # Drop keys before the final Enter until the preview fits the format's 2 s sample limit.
    keys = list(PREVIEW_KEYS)
    events, total = preview_events(keys, rate, rendered, key_files)
    while total > PREVIEW_MAX_S * rate and len(keys) > 2:
        del keys[-2]
        events, total = preview_events(keys, rate, rendered, key_files)
    mix = [0.0] * total
    for when, f in events:
        if f is None:
            continue
        off = round(when * rate)
        for i, v in enumerate(rendered[f]):
            mix[off + i] += v
    peak = max(abs(v) for v in mix)
    ceiling = db_to_gain(CEILING_DBFS) * 32768
    scale = min(1.0, ceiling / peak) if peak else 1.0
    return [max(-32768, min(32767, round(v * scale))) for v in mix], scale


def load_inputs(src):
    tree = json.loads((src / "tree.json").read_text())
    if tree.get("sha") != COMMIT:
        sys.exit(f"tree.json is for {tree.get('sha')}, expected {COMMIT}")
    blobs = {e["path"]: e for e in tree["tree"] if e["type"] == "blob"}

    license_bytes = (src / "LICENSE").read_bytes()
    if git_blob_sha1(license_bytes) != blobs["LICENSE"]["sha"]:
        sys.exit("LICENSE does not match the snapshot")
    license_text = license_bytes.decode("utf-8")
    if "The MIT License" not in license_text or "Ico Doornekamp" not in license_text:
        sys.exit("LICENSE at the snapshot is not the expected MIT license")

    files = []
    for path in sorted(p for p in blobs if re.fullmatch(r"wav/[0-9a-f]{2}-[01]\.wav", p)):
        data = (src / path).read_bytes()
        if git_blob_sha1(data) != blobs[path]["sha"]:
            sys.exit(f"{path} does not match the snapshot (git blob SHA-1)")
        code, flag = int(path[4:6], 16), int(path[7])
        files.append({
            "path": path, "code": code, "action": "press" if flag == 1 else "release",
            "bytes": data, "sha256": hashlib.sha256(data).hexdigest(), "git": blobs[path]["sha"],
        })
    return license_bytes, license_text, files


def main():
    here = Path(__file__).resolve().parent
    repo_root = here.parents[2]
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--src", required=True, type=Path, help="directory written by fetch.sh")
    ap.add_argument("--out", type=Path, default=repo_root / "packs" / PACK_ID)
    args = ap.parse_args()

    license_bytes, license_text, files = load_inputs(args.src)

    # Map files to keys. Byte-identical files share one pack file.
    by_hash, mapped, skipped, unknown = {}, [], [], []
    for f in files:
        if f["code"] in SKIP:
            skipped.append((f, SKIP[f["code"]]))
            continue
        if f["code"] not in EVDEV:
            unknown.append(f)
            continue
        f["evdev"], f["key"] = EVDEV[f["code"]]
        f["out"] = f"sounds/{f['key']}-{f['action']}.wav"
        if f["sha256"] in by_hash:
            f["same_as"] = by_hash[f["sha256"]]
        else:
            by_hash[f["sha256"]] = f
        mapped.append(f)
    unique = [f for f in mapped if "same_as" not in f]

    rate = None
    for f in unique:
        r, x, f["channels"], f["bits"] = read_wav(f["bytes"], f["path"])
        if rate is None:
            rate = r
        elif r != rate:
            sys.exit(f"{f['path']}: sample rate {r} differs from {rate}")
        f["orig_len"] = len(x)
        f["clip"] = full_scale_runs(x)
        f["x"] = remove_dc(x, rate)
        f["xd"] = detection_band(f["x"], rate)
        f["rumble"] = rumbles(f["x"], f["xd"], rate)

    preview_budget = wav_bytes(round(3.0 * rate))
    for tail_rel_db in TAIL_REL_STEPS:
        for f in unique:
            f["seg"] = segment(f["x"], f["xd"], rate, tail_rel_db)
        size = sum(wav_bytes(f["seg"]["end"] - f["seg"]["start"]) for f in unique)
        if size + preview_budget + 64_000 <= SIZE_BUDGET:
            break
    else:
        sys.exit(f"pack would be {size} bytes, over the {SIZE_BUDGET}-byte budget")

    press_peaks = [f["seg"]["peak"] for f in unique if f["action"] == "press"]
    target = db_to_gain(TARGET_MEDIAN_PRESS_PEAK_DBFS) / statistics.median(press_peaks)
    # Headroom: the loudest true peak, at the top of the runtime's volume variation.
    boost = 1 + PACK["variation"]["volume"]
    loudest = max(true_peak(f["x"][f["seg"]["start"]:f["seg"]["end"]]) for f in unique)
    ceiling = db_to_gain(MAX_TRUE_PEAK_DBFS) / (loudest * boost)
    gain = min(target, ceiling)

    rendered = {}
    for f in unique:
        rendered[f["out"]] = f["ints"] = render(f["x"], f["seg"], gain, rate)
        f["tp"] = true_peak([v / 32768 for v in f["ints"]])
    headroom = max(f["tp"] for f in unique) * boost
    if headroom > db_to_gain(MAX_TRUE_PEAK_DBFS + 0.05):
        sys.exit(f"loudest sample reaches {dbfs(headroom):.2f} dBFS with the volume variation")

    key_files = {}
    for f in mapped:
        target_file = f["same_as"]["out"] if "same_as" in f else f["out"]
        key_files.setdefault(f["key"], {})[f["action"]] = target_file

    preview, preview_scale = build_preview(rendered, rate, key_files)

    # Write the pack.
    out = args.out
    sounds = out / "sounds"
    sounds.mkdir(parents=True, exist_ok=True)
    keep = {Path(f["out"]).name for f in unique}
    for stale in sounds.glob("*.wav"):
        if stale.name not in keep:
            stale.unlink()
    for f in unique:
        write_wav16(out / f["out"], rate, f["ints"])
    write_wav16(out / "preview.wav", rate, preview)

    manifest = dict(PACK)
    groups = {}
    for group in GROUP_ORDER:
        names = FALLBACK_GROUPS.get(group)
        if not names:
            continue
        sets = {}
        for action in ("press", "release"):
            paths = [key_files[k][action] for k in names if action in key_files.get(k, {})]
            if paths:
                sets[action] = paths
        groups[group] = sets
    manifest["groups"] = groups
    manifest["keys"] = {
        k: {a: [key_files[k][a]] for a in ("press", "release") if a in key_files[k]}
        for k in sorted(key_files)
    }
    (out / "pack.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")

    notice = (
        "The sound samples in this pack are derived from bucklespring by Ico Doornekamp\n"
        f"(https://github.com/{REPO}, commit {COMMIT}),\n"
        "which were distributed under the following license at that commit.\n"
        "The original license text follows unchanged.\n\n"
    )
    (out / "LICENSE.txt").write_bytes(notice.encode("utf-8") + license_bytes)

    stats = stats_table(unique, mapped, rate)
    shipped = {p.relative_to(out).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
               for p in [out / f["out"] for f in unique] + [out / "preview.wav"]}
    (out / "SOURCES.md").write_text(sources_md(
        files, mapped, skipped, unknown, license_bytes, rate, gain, target, ceiling,
        tail_rel_db, preview_scale, stats, unique, headroom, shipped))

    total = sum(p.stat().st_size for p in out.rglob("*") if p.is_file())
    print(f"wrote {out}: {len(unique)} sounds + preview, {len(key_files)} keys, {total} bytes")
    print(f"source: {len(files)} files at {rate} Hz; skipped {len(skipped)}; unknown {len(unknown)}; "
          f"preview {len(preview) / rate:.2f} s")
    print(f"pack gain {dbfs(gain):+.2f} dB "
          f"({'median press peak target' if gain == target else 'limited by the headroom rule'}, "
          f"headroom {dbfs(headroom):.2f} dBFS true peak at +{PACK['variation']['volume']:.0%}); "
          f"tail rule: {'noise floor' if tail_rel_db is None else f'{tail_rel_db} dB below peak'}")
    flags = {k: sum(1 for f in unique if f["seg"][k]) for k in ("capped", "late_event")}
    flags["hard_end"] = sum(1 for f in unique if not f["seg"]["decayed"])
    print(f"flags: {flags}")
    print(stats)
    if total > 6 * 1024 * 1024:
        sys.exit("pack exceeds the 6 MB hard limit")


def stats_table(unique, mapped, rate):
    rows = ["| Group | Action | Files | Mean ms | Peak dBFS | True peak dBFS | Mean RMS dBFS "
            "| Max lead ms |",
            "|---|---|---|---|---|---|---|---|"]
    for group in GROUP_ORDER:
        for action in ("press", "release"):
            fs = [f for f in unique if group_of(f["key"]) == group and f["action"] == action]
            if not fs:
                continue
            durs = [1000 * len(f["ints"]) / rate for f in fs]
            peaks = [max(abs(v) for v in f["ints"]) / 32768 for f in fs]
            rmss = [math.sqrt(sum(v * v for v in f["ints"]) / len(f["ints"])) / 32768 for f in fs]
            lead = [leading_silence_ms(f["ints"], rate) for f in fs]
            rows.append(
                f"| {group} | {action} | {len(fs)} | {statistics.mean(durs):.0f} | "
                f"{dbfs(max(peaks)):.1f} | {dbfs(max(f['tp'] for f in fs)):.1f} | "
                f"{statistics.mean(dbfs(r) for r in rmss):.1f} | {max(lead):.2f} |")
    return "\n".join(rows)


def sources_md(files, mapped, skipped, unknown, license_bytes, rate, gain, target, ceiling,
               tail_rel_db, preview_scale, stats, unique, headroom, shipped):
    ms = lambda n: f"{1000 * n / rate:.1f} ms"  # noqa: E731
    at_attack = sum(1 for f in unique if f["seg"]["onset"] == 0)
    rumbling = sum(1 for f in unique if f["rumble"])
    clipped = [f for f in unique if f["clip"][0]]
    longest_run = max((f["clip"][1] for f in clipped), default=0)
    lines = [
        "# Buckling Spring: sources",
        "",
        f"Every sound in this pack comes from one source, rebuilt by "
        f"`tools/pack-sources/{PACK_ID}/fetch.sh` and `build.py` (deterministic).",
        "",
        "## Source",
        "",
        f"- **Source page:** {SOURCE_PAGE}",
        f"- **Snapshot:** commit `{COMMIT}` (2016-08-29), the last commit whose `LICENSE` is "
        "MIT. `wav/` last changed on 2016-02-06; all four commits that touched it "
        "(`3e97813`, `bbab017`, `4c441a3`, `688a522`) are by Ico Doornekamp.",
        "- **Author / recordist:** Ico Doornekamp (GitHub `zevv`). He recorded every key of his "
        "IBM Model M space saver with his own tool `rec.c` (Linux evdev events, `parec` at "
        "44.1 kHz, 16-bit, mono).",
        "- **License:** `MIT`, \"Copyright (c) 2016 - Ico Doornekamp\", reproduced unchanged in "
        "`LICENSE.txt`.",
        f"- **License proof:** {LICENSE_PAGE} "
        f"(raw: {RAW_BASE}LICENSE, SHA-256 `{hashlib.sha256(license_bytes).hexdigest()}`). "
        "History: MIT added in "
        "[`095feed`](https://github.com/zevv/bucklespring/commit/095feed8d1bd15bd6da83d86315cf40f82887510) "
        "(2016-02-03), replaced by GPL-2.0 in "
        "[`6467158`](https://github.com/zevv/bucklespring/commit/646715891702d0d23a5a21ca72ef887430be2a3a) "
        "(2016-09-16). On the samples' license, the author wrote in "
        "[issue #19](https://github.com/zevv/bucklespring/issues/19) (2016-09-16, after the "
        "switch to GPL) that they are simply part of the project, so the top-level licence "
        "applies. That the MIT grant covered the audio during the MIT period is an inference "
        "from that statement.",
        f"- **Date checked:** {DATE_CHECKED}",
        "- **Do not** take files from any later commit: they are GPL-2.0, and the keypad samples "
        "added later (PR #30, by another contributor) are GPL-only.",
        "",
        "## File naming in the source",
        "",
        "`%02x-%d.wav` = hex **Linux evdev key code** (`KEY_*` in "
        "`include/uapi/linux/input-event-codes.h`) and the event value: **1 = press, "
        "0 = release**. `rec.c` writes `event.code`/`event.value` from `/dev/input/event*`, "
        "and `scan-linux.c` subtracts 8 from the X11 keycode and calls `play(key, 1)` on "
        "KeyPress and `play(key, 0)` on KeyRelease.",
        "",
        "## Processing (every file)",
        "",
        f"1. Decode the 16-bit mono WAV ({rate} Hz, kept). No resampling.",
        f"2. Subtract the file mean, then a causal 2nd-order Butterworth high-pass at "
        f"{DC_HPF_HZ:g} Hz.",
        f"3. Noise floor = {int(FLOOR_PERCENTILE * 100)}th percentile of 1 ms frame RMS. "
        f"Onset = first sample at or above max({ONSET_FLOOR_X:g} x floor, peak "
        f"{ONSET_REL_DB:g} dB). Start = onset - 0.5 ms, with that pre-roll faded in from zero. "
        + (f"All {at_attack}" if at_attack == len(unique) else f"{at_attack} of the {len(unique)}")
        + " originals are already audible at their first sample (the recordings begin at the "
        "attack), so they start there unchanged, with no pre-roll and no fade-in: nothing is "
        "added before the recorded attack, and there is no leading silence.",
        f"4. Tail, measured on a copy of the take high-passed at {DETECT_HPF_HZ:g} Hz (4th "
        f"order; used only to find the tail, the audio keeps everything above {DC_HPF_HZ:g} Hz): "
        f"the first {int(TAIL_HOLD_S * 1000)} ms stretch after the peak whose 1 ms frames all "
        f"stay below {TAIL_FLOOR_X:g} x that copy's floor"
        + ("" if tail_rel_db is None else f" (or {-tail_rel_db:g} dB below its peak)")
        + f". {rumbling} of the takes carry a sub-audible rumble (over "
        f"{RUMBLE_SHARE:.0%} of their energy after the first 100 ms is below "
        f"{DETECT_HPF_HZ:g} Hz, marked \"rumble\" below); measured on the full band, that "
        "rumble can hold a tail open to the end of the file, swells and all. The rule used is "
        "the loosest of the noise floor alone, then "
        + ", ".join(f"{-t:g}" for t in TAIL_REL_STEPS if t is not None)
        + f" dB below the peak, that keeps the pack within {SIZE_BUDGET / 1e6:g} MB. The "
        f"sample ends {int(FADE_OUT_S * 1000)} ms into that stretch, with a half-cosine fade "
        f"to exactly zero over those {int(FADE_OUT_S * 1000)} ms. Samples are capped at "
        f"{int(MAX_LEN_S * 1000)} ms.",
        f"5. One gain for the whole pack: {dbfs(gain):+.2f} dB "
        + ("(median press peak to "
           f"{TARGET_MEDIAN_PRESS_PEAK_DBFS:g} dBFS)" if gain == target else
           "(limited by the headroom rule below; the median-press target was "
           f"{dbfs(target):+.2f} dB)")
        + ". No per-file normalization; no clipping. Headroom: the mixer clips hard at 0 dBFS, "
        "the loader resamples to the device rate (which can peak between samples) and the "
        f"runtime adds up to +{PACK['variation']['volume']:.0%} of random volume, so the "
        "loudest true peak (4x band-limited interpolation) at the top of that variation must "
        f"stay at or below {MAX_TRUE_PEAK_DBFS:g} dBFS. It is {dbfs(headroom):.2f} dBFS.",
        "6. Encode 16-bit PCM WAV, mono. bucklespring pans each key in stereo by its position "
        "(OpenAL); TakTak format 1 is mono, so that panning is not reproduced.",
        f"7. `preview.wav` (SHA-256 `{shipped['preview.wav']}`): the pack's own samples typing "
        f"a short phrase with Enter (seeded random dwell 60-120 ms and gaps 80-200 ms), mixed"
        + ("" if preview_scale >= 1 else f" and scaled by {dbfs(preview_scale):+.2f} dB")
        + ".",
        "",
        "## Loudness",
        "",
        f"`volume` in `pack.json` is {VOLUME:.2f} ({dbfs(VOLUME):+.1f} dB): a playback gain the "
        "app applies to the whole pack, so no audio file changes. It matches typing loudness "
        f"across the bundled packs, so switching packs does not jump in volume; the reference "
        f"is the synthesized packs' {REFERENCE_LK:.1f} LK. Measured with `cargo run -p "
        f"synth-packs --release -- loudness packs/{PACK_ID}` (`tools/synth-packs/src/"
        "loudness.rs`): the K-weighted (ITU-R BS.1770) energy of the first 100 ms of the "
        "sample each alphanumeric key plays on press by default, power-averaged over the keys, "
        f"on the samples as the app loads them at 48 kHz. This pack reads {LK_AS_BUILT:.1f} LK "
        "as built and "
        f"{LK_AS_BUILT + dbfs(VOLUME):.1f} LK at this volume. The loudest true peak at the top "
        f"of the volume variation becomes {dbfs(headroom * VOLUME):.2f} dBFS. `build.py` writes "
        "the volume (`VOLUME`), so a rebuild keeps it.",
        "",
        "## Source clipping",
        "",
        f"{len(clipped)} of the {len(unique)} originals used touch digital full scale "
        f"(+/-32767) in places, so the recording clipped there or came within a hair of it "
        f"(mostly single samples; the longest flat run is {longest_run} samples, "
        f"{1000 * longest_run / rate:.2f} ms). They "
        "are kept as recorded, with no declipping, and marked \"source clips\" below. The pack "
        "gain puts them well below full scale, so the pack adds no clipping of its own.",
        "",
        "## Mapping notes",
        "",
        "- Every key the source has gets its own press and release sample, as in bucklespring.",
        "- `Pause` has only a release sample (`77-0.wav`) in the source, so its press falls back "
        "to the group sounds. No release sound was made up for any key.",
        "- `MetaLeft` (evdev `0x7d`) uses the Right Alt sample, because bucklespring's own "
        "`7d-*.wav` are byte-identical copies of `64-*.wav` (the Model M has no Windows key).",
        "- Keys without a sample (numpad, `MetaRight`, `IntlBackslash`, F13+, media keys) fall "
        "back to group sounds that reuse per-key files: home-row letters (alphanumeric, and "
        "through it every `other` key), Enter (`NumpadEnter`), Left/Right Alt (modifiers).",
        "",
        "## Stats",
        "",
        "Max lead ms = time before the first sample at -50 dBFS, the loader's leading-silence "
        "threshold.",
        "",
        stats,
        "",
        "## Files",
        "",
        "Author of every file: Ico Doornekamp. License: MIT (proof above). Checked "
        f"{DATE_CHECKED}. Direct download: `{RAW_BASE}<original>`.",
        "",
        "Each original links to its direct download. \"SHA-256 as shipped\" is the pack file's "
        "own hash.",
        "",
        "| Original | evdev | Pack file | SHA-256 of the original | Derivation "
        "| SHA-256 as shipped |",
        "|---|---|---|---|---|---|",
    ]
    for f in mapped:
        if "same_as" in f:
            how = f"byte-identical to `{f['same_as']['path']}`; the key uses `{f['same_as']['out']}`"
            out = f"(shares `{f['same_as']['out']}`)"
            pack_file = f["same_as"]["out"]
        else:
            s = f["seg"]
            how = (f"trim {ms(s['start'])} lead, keep {ms(s['end'] - s['start'])} "
                   f"of {ms(f['orig_len'])}"
                   + ("" if s["decayed"] else ", end at file end")
                   + (", capped" if s["capped"] else "")
                   + (", later event cut off" if s["late_event"] else "")
                   + (", rumble" if f["rumble"] else "")
                   + (f", source clips ({f['clip'][0]} samples)" if f["clip"][0] else ""))
            out = f"`{f['out']}`"
            pack_file = f["out"]
        lines.append(f"| [`{f['path']}`]({RAW_BASE}{f['path']}) | `{f['evdev']}` {f['action']} "
                     f"| {out} | `{f['sha256']}` | {how} | `{shipped[pack_file]}` |")
    lines += ["", "## Not used", ""]
    for f, why in skipped:
        lines.append(f"- `{f['path']}` (SHA-256 `{f['sha256']}`): {why}")
    for f in unknown:
        lines.append(f"- `{f['path']}` (SHA-256 `{f['sha256']}`): unknown evdev code "
                     f"0x{f['code']:02x}.")
    lines.append("- Everything outside `wav/` and `LICENSE` (the snapshot also holds "
                 "`buckle.exe` and Windows libraries): never downloaded.")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    main()
