#!/usr/bin/env python3
"""Slices Anonimski's CC0 Wikimedia Commons recording "IBM M2 sound.ogg" into packs/spring-lite.

Usage:
  springslice.py build --downloads DIR --out PACK_DIR [--selection FILE] [--reselect]
  springslice.py stats PACK_DIR

DIR holds IBM_M2_sound.ogg (fetch.sh downloads it). `build` decodes the OGG with ffmpeg, finds
every keystroke, splits presses from releases and writes pack.json, sounds/*.wav, preview.wav
and SOURCES.md. The cut list (sample ranges in the decoded source, the pack gain and each
file's role) is stored in --selection. If that file exists, it is reused as-is so a rebuild is
exact; --reselect runs the automatic selection again and overwrites it.

Processing, in order (nothing else is done to the audio):
  1. ffmpeg decode, stereo -> mono (L+R)/2, 2-pole high-pass at 30 Hz (removes DC and rumble).
     The source rate (44.1 kHz) is kept.
  2. Onsets: 1 ms energy frames of a 1 kHz high-passed copy (detection only); a frame that is
     12 dB above the 10th-percentile floor and 9 dB above the quietest of the previous 8 frames
     starts a stroke, at least 25 ms after the previous one.
  3. Each stroke starts 0.5 ms (22 samples) before its main attack: the first sample reaching
     -20 dB of the stroke's peak (and twice the background peak just before it). The 0.5 ms
     pre-roll gets a raised-cosine fade-in.
  4. Each stroke ends at the earliest of: 1 ms before the next comparable stroke, the point
     where its 2 ms envelope rises 6 dB again more than 25 ms after the attack (another sound
     starting; a buckling-spring stroke's own second transient comes sooner), the point where
     it decays to 3 dB above the noise floor, or 400 ms (1.5 s for Enter). A raised-cosine
     fade-out of up to 8 ms ends on an exact zero.
  5. One gain for the whole pack: the median press peaks at -6 dBFS. No per-file
     normalization; the loudest file stays below -1 dBFS.
  6. 16-bit PCM WAV, mono, 44.1 kHz (rounded, no dither).
  7. pack.json plays the pack at VOLUME, which matches its typing loudness to the other
     bundled packs (measured with `synth-packs loudness`, see VOLUME below).

Which strokes are kept (see clean()): peak at least 20 dB (releases: 15 dB) over the RMS of
the 10 ms before it and 8 dB over the peak of the 5 ms before it; either decays into the noise
floor on its own (20 ms or longer) or is at least 45 ms long (35 ms for a release) and 18 dB
down when cut; peak within -9..+5 dB of the median stroke of its kind. Press or release: see
select_press_release().

Not used: Raymangold22's three CC0 Commons takes ("Typing - Model M 1986.ogg", "Typing - Model F
122 1984.ogg", "Typing - Model M13 1999.ogg"). They were sliced into packs once and rejected
on 2026-10-02 because their strokes overlap. They are 12 s of continuous fast typing (about
220 onsets each). Measured on 2 ms frames, for strokes within -9..+5 dB of the median: no
stroke stands more than 26-29 dB above its local background (10th percentile within 250 ms;
median 16-18 dB), and 90-99 % start less than 18 dB above the sound just before them (median
8-12 dB). None of the three has a stroke that is both 30 dB over its background and 18 dB over
what precedes it; this M2 take has 11.
Re-checked in the 2026-10-02 audit (the packs were spring-1986, spring-1999 and
capacitive-spring): select_press_release() and clean() run on each take, then two isolation
measures per kept press, relative to its loudest 2 ms within 10 ms of the attack: the RMS of the
10 ms before the attack, and the quietest 5 ms of the kept tail. This M2 take has 13 presses at
-22 dB or lower on both; the 1986, 1999 and 1984 takes have 0, 0 and 2 (medians -16..-21 dB:
other keystrokes ring under every stroke). Their median time between onsets is 46-50 ms; here
it is 80 ms. So none of them reaches 6 clean press variants.
"""

import argparse
import hashlib
import json
import math
import os
import random
import subprocess
import sys
import wave

import numpy as np

FFMPEG = os.environ.get("FFMPEG", "ffmpeg")
RATE = 44100
HOP = 44  # ~1 ms detection frames
ENV_HOP = 88  # 2 ms envelope frames
PRE_ROLL = 22  # 0.5 ms at 44.1 kHz
SILENCE = 10 ** (-50 / 20)  # the loader's leading-silence threshold (docs/pack-format.md)

DETECT = dict(floor_pct=10, snr_db=12.0, rise_db=9.0, rise_frames=8, min_gap_frames=25)

PACK_ID = "spring-lite"
SOURCE = dict(
    file="IBM_M2_sound.ogg",
    url_path="1/16/IBM_M2_sound.ogg",
    sha1="3c64c15dd174840d8e4e8b2d5f02a35c6680ec63",
    uploaded="2014-05-17",
    commons_desc=("Sound sample produced by typing on the IBM M2 (Model M) mechanical keyboard. "
                  'Typed text: "www.wikipedia.org[Enter]"'),
    title="IBM M2 sound.ogg",
    author_name="Anonimski",
)
NAME = "Spring Lite"
DESCRIPTION = (
    "An IBM M2, the compact, lighter-built buckling-spring board: bright clicks, "
    "softer up-strokes and a resonant Enter, from a CC0 recording of one typed URL."
)
PREVIEW_SEED = 2014
PREVIEW_TEXT = "the quick fox\n"  # "_" or " " is Space, "\n" is Enter
PREVIEW_MAX_S = 1.95

COMMONS = "https://commons.wikimedia.org/wiki/File:"
API = ("https://commons.wikimedia.org/w/api.php?action=query&format=json&prop=imageinfo"
       "&iiprop=url%7Csize%7Csha1%7Cextmetadata&titles=File:")

# Selection rules for a clean stroke.
MIN_SNR_DB = 20.0  # stroke peak over the RMS of the 10 ms before it
MIN_BG_MARGIN = 2.5  # stroke peak over the peak of the 5 ms before it (8 dB)
MIN_LEN_MS = {"press": 45.0, "release": 35.0}  # for strokes cut before they decay
MIN_COMPLETE_MS = 20.0  # for strokes that decay into the noise floor on their own
STROKE_S = 0.025  # a second transient this soon after the attack is part of the stroke
MAX_END_REL_DB = -18.0  # the cut must come after the stroke decayed this far
PEAK_WINDOW_DB = (-9.0, 5.0)  # kept peaks relative to the median stroke
MAX_LEN_S = 0.400
MAX_ENTER_LEN_S = 1.5
FADE_OUT_MS = 8.0
TARGET_PEAK_DB = -6.0
CEILING_DB = -1.0
# Playback level, written to pack.json as "volume"; it changes no audio file. It matches the
# pack's typing loudness to TakTak's reference typing level (REFERENCE_LK), so switching packs does not
# jump in volume. Measured on the built pack with
#     cargo run -p synth-packs --release -- loudness packs/spring-lite
# (tools/synth-packs/src/loudness.rs): LK_AS_BUILT at volume 1.0. Recorded in SOURCES.md;
# re-measure whenever the processing or the cut list changes.
VOLUME = 0.74
LK_AS_BUILT = -23.32
REFERENCE_LK = -25.8


# ---------------------------------------------------------------- audio io


def decode(path, extra_filter=""):
    """Mono float64 at RATE: (L+R)/2, 2-pole 30 Hz high-pass, plus an optional extra filter."""
    af = "pan=mono|c0=0.5*c0+0.5*c1,highpass=f=30:poles=2"
    if extra_filter:
        af += "," + extra_filter
    cmd = [FFMPEG, "-nostdin", "-loglevel", "error", "-i", path, "-af", af,
           "-ar", str(RATE), "-ac", "1", "-f", "f32le", "-"]
    raw = subprocess.run(cmd, capture_output=True, check=True).stdout
    return np.frombuffer(raw, dtype="<f4").astype(np.float64)


def write_wav(path, samples):
    pcm = np.clip(np.round(samples * 32767.0), -32768, 32767).astype("<i2")
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm.tobytes())


def read_wav(path):
    with wave.open(path, "rb") as w:
        assert w.getsampwidth() == 2 and w.getnchannels() == 1, path
        rate = w.getframerate()
        data = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float64)
    return data / 32768.0, rate


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


def db(power):
    return 10.0 * np.log10(np.maximum(power, 1e-20))


def amp_db(a):
    return 20.0 * np.log10(max(float(a), 1e-10))


# ---------------------------------------------------------------- analysis


def frame_db(x, hop):
    n = len(x) // hop
    return db((x[: n * hop].reshape(n, hop) ** 2).mean(axis=1))


def detect_onsets(hf):
    """Detection frames' first sample, from the 1 kHz high-passed signal."""
    e = frame_db(hf, HOP)
    floor = np.percentile(e, DETECT["floor_pct"])
    out, last = [], -(10 ** 9)
    rf = DETECT["rise_frames"]
    for i in range(rf, len(e)):
        if e[i] < floor + DETECT["snr_db"]:
            continue
        if e[i] - e[i - rf:i].min() >= DETECT["rise_db"] and i - last >= DETECT["min_gap_frames"]:
            out.append((i - 1) * HOP)
            last = i
    return out


def measure(x, d):
    """Main attack, peak and background around the detection at sample d."""
    win = x[d:d + int(0.025 * RATE)]
    peak = np.abs(win).max()
    bg = x[max(0, d - int(0.005 * RATE)):d]
    bg_peak = np.abs(bg).max() if len(bg) else 0.0
    thr = max(0.1 * peak, 2.0 * bg_peak)
    hits = np.nonzero(np.abs(win) >= thr)[0]
    attack = d + int(hits[0]) if len(hits) else d
    pre = x[max(0, attack - int(0.010 * RATE)):attack]
    pre_rms = np.sqrt(np.mean(pre ** 2)) if len(pre) else 1e-10
    return dict(det=d, attack=attack, peak=float(peak), bg_peak=float(bg_peak),
                snr=amp_db(peak) - amp_db(pre_rms))


def envelope(x, start, end):
    seg = x[start:end]
    n = len(seg) // ENV_HOP
    if n == 0:
        return np.array([])
    return db((seg[: n * ENV_HOP].reshape(n, ENV_HOP) ** 2).mean(axis=1))


def find_end(x, ev, nxt_det, noise_db, max_len_s):
    """End sample (exclusive) of the stroke starting at ev['attack']."""
    start = ev["attack"] - PRE_ROLL
    limit = min(len(x), start + int(max_len_s * RATE))
    if nxt_det is not None:
        limit = min(limit, nxt_det - int(0.001 * RATE))
    env = envelope(x, ev["attack"], limit)
    reason = "next" if nxt_det is not None and limit == nxt_det - int(0.001 * RATE) else "max"
    if len(env) == 0:
        return limit, reason, 0.0
    # The stroke's own peak frame (a release can swell for 20+ ms).
    head = max(1, min(len(env), int(0.030 * RATE) // ENV_HOP))
    pk = int(np.argmax(env[:head]))
    smooth = np.convolve(env, np.ones(3) / 3, mode="same")
    # A buckling-spring stroke is two transients (spring buckle, then bottom-out or top-out)
    # a few ms apart, so a second rise within STROKE_S belongs to the same stroke.
    first = max(pk + 1, int(STROKE_S * RATE) // ENV_HOP)
    run_min = smooth[min(first, len(smooth) - 1)]
    end = limit
    for j in range(pk + 1, len(env)):
        if j >= first:
            run_min = min(run_min, smooth[j])
            if smooth[j] >= run_min + 6.0 and env[j] >= env[pk] - 30.0:
                end = ev["attack"] + j * ENV_HOP - int(0.001 * RATE)
                reason = "re-rise"
                break
        if j > pk + 2 and j + 1 < len(env) and max(env[j], env[j + 1]) < noise_db + 3.0:
            end = ev["attack"] + (j + 1) * ENV_HOP
            reason = "noise"
            break
    end = min(end, limit)
    # Level where the cut falls, relative to the stroke's peak frame.
    jend = max(0, min(len(env) - 1, (end - ev["attack"]) // ENV_HOP - 1))
    return end, reason, float(env[jend] - env[pk])


def analyze(src):
    x = decode(src)
    hf = decode(src, "highpass=f=1000:poles=2,highpass=f=1000:poles=2")
    dets = detect_onsets(hf)
    events = [measure(x, d) for d in dets]
    noise_db = float(np.percentile(frame_db(x, ENV_HOP), 10))
    return x, events, noise_db


def next_comparable(events, k):
    """Detection sample of the next onset that is not a much quieter blip (-15 dB or more)."""
    for e in events[k + 1:]:
        if amp_db(e["peak"]) >= amp_db(events[k]["peak"]) - 15.0:
            return e["det"]
    return None


def two_means_threshold(values):
    v = np.sort(np.asarray(values))
    lo, hi = v[0], v[-1]
    for _ in range(100):
        t = (lo + hi) / 2
        a, b = v[v < t], v[v >= t]
        nlo, nhi = (a.mean() if len(a) else lo), (b.mean() if len(b) else hi)
        if abs(nlo - lo) < 1e-9 and abs(nhi - hi) < 1e-9:
            break
        lo, hi = nlo, nhi
    return (lo + hi) / 2


def candidate(x, events, k, noise_db, action, max_len_s=MAX_LEN_S):
    ev = events[k]
    end, reason, end_rel = find_end(x, ev, next_comparable(events, k), noise_db, max_len_s)
    start = ev["attack"] - PRE_ROLL
    head = x[ev["attack"]:ev["attack"] + int(STROKE_S * RATE)]
    delay = int(np.argmax(np.abs(head))) / RATE * 1000 if len(head) else 0.0
    return dict(k=k, start=int(start), end=int(end), action=action, peak_delay_ms=round(delay, 1),
                src_peak_db=round(amp_db(ev["peak"]), 2), snr_db=round(ev["snr"], 2),
                bg_margin=round(ev["peak"] / max(ev["bg_peak"], 1e-10), 2),
                len_ms=round((end - start) / RATE * 1000, 1), end_reason=reason,
                end_rel_db=round(end_rel, 1))


def clean(c, median_db, min_snr=MIN_SNR_DB):
    """A clean stroke starts well above its background and either decays into the noise
    floor on its own (complete) or is cut late enough that it has mostly decayed."""
    rel = c["src_peak_db"] - median_db
    complete = c["end_reason"] == "noise" and c["len_ms"] >= MIN_COMPLETE_MS
    long_enough = c["len_ms"] >= MIN_LEN_MS[c["action"]] and c["end_rel_db"] <= MAX_END_REL_DB
    return (c["snr_db"] >= min_snr and c["bg_margin"] >= MIN_BG_MARGIN
            and (complete or long_enough)
            and PEAK_WINDOW_DB[0] <= rel <= PEAK_WINDOW_DB[1])


def select_press_release(x, events, noise_db):
    """Slow typing with clear pairs. Strokes are split by the level of their attack (first
    15 ms) into two clusters: the louder ones are presses, the quieter ones are releases,
    counted against the keys still held down (a release needs an earlier press). The last
    press is Enter: the Commons description gives the typed text as "www.wikipedia.org[Enter]"."""
    hit = int(0.015 * RATE)
    level = {k: amp_db(np.abs(x[e["attack"]:e["attack"] + hit]).max()) for k, e in enumerate(events)}
    strong = [k for k, e in enumerate(events) if e["snr"] >= 15.0]
    loudest = max(level[k] for k in strong)
    strong = [k for k in strong if level[k] >= loudest - 20.0]  # drop small rattles
    thr = two_means_threshold([level[k] for k in strong])
    roles = {}
    held = 0
    for k in strong:
        if level[k] >= thr:
            roles[k] = "press"
            held += 1
        elif held > 0:
            roles[k] = "release"
            held -= 1
    presses = [k for k, r in roles.items() if r == "press"]
    enter = max(presses)
    enter_rel = next((k for k, r in roles.items() if r == "release" and k > enter), None)
    press_db = float(np.median([amp_db(events[k]["peak"]) for k in presses]))
    rel_list = [k for k, r in roles.items() if r == "release"]
    rel_db = float(np.median([amp_db(events[k]["peak"]) for k in rel_list])) if rel_list else press_db

    files = []

    def add(k, action, group, name, median_db, min_snr, max_len_s=MAX_LEN_S, force=False):
        c = candidate(x, events, k, noise_db, action, max_len_s)
        if force or clean(c, median_db, min_snr):
            files.append(dict(c, name=name, group=group))
            return True
        return False

    add(enter, "press", "enter", "enter-press.wav", press_db, MIN_SNR_DB, MAX_ENTER_LEN_S, force=True)
    if enter_rel is not None:
        add(enter_rel, "release", "enter", "enter-release.wav", rel_db, 15.0, MAX_ENTER_LEN_S, force=True)
    n = 0
    for k in sorted(presses):
        if k != enter and add(k, "press", "alphanumeric", f"press-{n + 1:02d}.wav", press_db, MIN_SNR_DB):
            n += 1
    n = 0
    for k in sorted(rel_list):
        if k != enter_rel and add(k, "release", "alphanumeric", f"release-{n + 1:02d}.wav", rel_db, 15.0):
            n += 1
    return files, dict(onsets=len(events), threshold_db=round(float(thr), 2), presses=len(presses),
                       releases=len(rel_list), enter_onset_s=events[enter]["attack"] / RATE)


# ---------------------------------------------------------------- cutting


def cut(x, f, gain):
    seg = x[f["start"]:f["end"]].copy() * gain
    fin = min(PRE_ROLL, len(seg))
    seg[:fin] *= 0.5 - 0.5 * np.cos(np.pi * np.arange(fin) / fin)
    fout = min(int(FADE_OUT_MS / 1000 * RATE), len(seg) // 4)
    if fout > 1:
        seg[-fout:] *= 0.5 + 0.5 * np.cos(np.pi * np.arange(1, fout + 1) / fout)
    seg[-1] = 0.0
    return seg


def pack_gain(x, files):
    peaks = [np.abs(x[f["start"]:f["end"]]).max() for f in files]
    press = [p for p, f in zip(peaks, files) if f["action"] == "press" and f["group"] == "alphanumeric"]
    g_db = TARGET_PEAK_DB - amp_db(np.median(press))
    g_db = min(g_db, CEILING_DB - amp_db(max(peaks)))
    return round(g_db, 2)


def render_preview(sounds, files):
    """A short phrase typed with the pack's own samples (press, then release, per key), ending
    with Enter. Groups the pack lacks fall back to alphanumeric, as in the app. 60-120 ms dwell,
    70-170 ms from one key-down to the next (plus 50 ms after Space), fixed seed."""
    rng = random.Random(PREVIEW_SEED)
    by = {}
    for f in files:
        by.setdefault((f["group"], f["action"]), []).append(sounds[f["name"]])
    alnum_p = by.get(("alphanumeric", "press"), [])
    alnum_r = by.get(("alphanumeric", "release"), [])
    out = np.zeros(int(2.3 * RATE))
    t = 0.02
    last_end = 0
    for ch in PREVIEW_TEXT:
        space = ch in " _"
        group = "space" if space else "enter" if ch == "\n" else "alphanumeric"
        press = by.get((group, "press"), alnum_p)
        release = by.get((group, "release"), alnum_r)
        dwell = rng.uniform(0.060, 0.120)
        for pool, at in ((press, t), (release, t + dwell)):
            if not pool:
                continue
            s = rng.choice(pool)
            i = int(at * RATE)
            if i + len(s) > len(out):
                out = np.concatenate([out, np.zeros(i + len(s) - len(out))])
            out[i:i + len(s)] += s
            last_end = max(last_end, i + len(s))
        t += rng.uniform(0.070, 0.170) + (0.05 if space else 0.0)
    out = out[:last_end + int(0.02 * RATE)]
    if len(out) > PREVIEW_MAX_S * RATE:
        sys.exit(f"error: preview is {len(out) / RATE:.3f} s, over {PREVIEW_MAX_S} s; shorten PREVIEW_TEXT")
    peak = np.abs(out).max()
    if peak > 10 ** (CEILING_DB / 20):
        out *= 10 ** (CEILING_DB / 20) / peak
    out[-1] = 0.0
    return out


# ---------------------------------------------------------------- pack writing


def write_pack(files, out_dir, x, gain_db):
    sounds_dir = os.path.join(out_dir, "sounds")
    os.makedirs(sounds_dir, exist_ok=True)
    for old in os.listdir(sounds_dir):
        if old.endswith(".wav"):
            os.remove(os.path.join(sounds_dir, old))
    g = 10 ** (gain_db / 20)
    sounds = {}
    for f in files:
        s = cut(x, f, g)
        sounds[f["name"]] = s
        write_wav(os.path.join(sounds_dir, f["name"]), s)
    preview = render_preview(sounds, files)
    write_wav(os.path.join(out_dir, "preview.wav"), preview)

    groups = {}
    for f in files:
        groups.setdefault(f["group"], {}).setdefault(f["action"], []).append("sounds/" + f["name"])
    order = ["alphanumeric", "space", "enter", "backspace", "modifiers", "other"]
    groups = {k: {a: groups[k][a] for a in ("press", "release") if a in groups[k]}
              for k in order if k in groups}
    manifest = {
        "format": 1,
        "id": PACK_ID,
        "name": NAME,
        "version": "1.0.0",
        "author": f"{SOURCE['author_name']} (recording); packaged by TakTak contributors",
        "license": "CC0-1.0",
        "description": DESCRIPTION,
        "source": COMMONS + SOURCE["file"],
        "attribution": f"“{SOURCE['title']}” by {SOURCE['author_name']}, CC0 1.0, via Wikimedia Commons",
        "preview": "preview.wav",
        "volume": VOLUME,
        "trim_silence": True,
        "variation": {"pitch": 0.015, "volume": 0.06},
        "groups": groups,
    }
    with open(os.path.join(out_dir, "pack.json"), "w", encoding="utf-8") as fh:
        json.dump(manifest, fh, indent=2, ensure_ascii=False)
        fh.write("\n")
    return len(preview)


def build(args):
    src = os.path.join(args.downloads, SOURCE["file"])
    digest = sha256(src)
    x, events, noise_db = analyze(src)
    if args.selection and os.path.exists(args.selection) and not args.reselect:
        with open(args.selection) as fh:
            sel = json.load(fh)
        if sel["source_sha256"] != digest:
            sys.exit(f"{src}: SHA-256 {digest} does not match the selection's {sel['source_sha256']}")
        files, gain_db, info = sel["files"], sel["gain_db"], sel.get("info", {})
    else:
        files, info = select_press_release(x, events, noise_db)
        gain_db = pack_gain(x, files)
        if args.selection:
            os.makedirs(os.path.dirname(os.path.abspath(args.selection)), exist_ok=True)
            with open(args.selection, "w") as fh:
                json.dump(dict(pack=PACK_ID, source=SOURCE["file"], source_sha256=digest, rate=RATE,
                               decode="ffmpeg: pan=mono|c0=0.5*c0+0.5*c1,highpass=f=30:poles=2",
                               gain_db=gain_db, info=info, files=files), fh, indent=1)
                fh.write("\n")
    preview_len = write_pack(files, args.out, x, gain_db)
    write_sources(args.out, digest, os.path.getsize(src), gain_db, info, files, preview_len)
    print(f"{PACK_ID}: {len(events)} onsets, {len(files)} files, gain {gain_db:+.2f} dB, "
          f"preview {preview_len / RATE:.3f} s, info {info}")
    print_stats(args.out)


def stats_table(pack_dir):
    """Markdown table: per group and action, count, mean length, peaks, RMS, leading silence."""
    with open(os.path.join(pack_dir, "pack.json")) as fh:
        m = json.load(fh)
    rows = ["| group / action | count | mean ms | median peak dBFS | max peak dBFS | mean RMS dBFS "
            "| max leading silence ms |", "|---|---|---|---|---|---|---|"]
    for g, sets in m["groups"].items():
        for action, paths in sets.items():
            durs, peaks, rmss, leads = [], [], [], []
            for p in paths:
                smp, rate = read_wav(os.path.join(pack_dir, p))
                durs.append(len(smp) / rate * 1000)
                peaks.append(amp_db(np.abs(smp).max()))
                rmss.append(amp_db(np.sqrt(np.mean(smp ** 2))))
                hits = np.nonzero(np.abs(smp) >= SILENCE)[0]
                leads.append((hits[0] if len(hits) else len(smp)) / rate * 1000)
            rows.append(f"| {g} {action} | {len(paths)} | {np.mean(durs):.0f} | {np.median(peaks):.1f} | "
                        f"{max(peaks):.1f} | {np.mean(rmss):.1f} | {max(leads):.2f} |")
    return rows


def print_stats(pack_dir):
    print("\n".join(stats_table(pack_dir)))
    total = sum(os.path.getsize(os.path.join(dp, f)) for dp, _, fs in os.walk(pack_dir) for f in fs)
    print(f"total size on disk: {total} bytes")


def write_sources(out_dir, digest, size, gain_db, info, files, preview_len):
    page = COMMONS + SOURCE["file"]
    url = "https://upload.wikimedia.org/wikipedia/commons/" + SOURCE["url_path"]
    typed = PREVIEW_TEXT.replace("\n", "").replace("_", " ")
    lines = [
        f"# Sources: {NAME} (`{PACK_ID}`)",
        "",
        "Every sound in this pack is cut from one recording on Wikimedia Commons that its author "
        "dedicated to the public domain under CC0 1.0. TakTak's cuts, `pack.json` and "
        "`preview.wav` are also released under CC0-1.0. Crediting the author is not required "
        "but is done in `attribution`.",
        "",
        "## Original recording",
        "",
        "| | |",
        "|---|---|",
        f"| Title | {SOURCE['title']} |",
        f"| Source page | {page} |",
        f"| Direct download | {url} |",
        f"| Author | {SOURCE['author_name']} (\"Own work\" per the source page) |",
        f"| Uploaded | {SOURCE['uploaded']} |",
        f"| Commons description | {SOURCE['commons_desc']} |",
        "| License | CC0 1.0 Universal Public Domain Dedication (SPDX `CC0-1.0`), "
        "https://creativecommons.org/publicdomain/zero/1.0/ |",
        f"| License proof | The source page's license section, and the Commons API record "
        f"(extmetadata `LicenseShortName` \"CC0\", `UsageTerms` \"Creative Commons Zero, Public "
        f"Domain Dedication\", `AttributionRequired` \"false\", `Credit` \"Own work\", artist "
        f"{SOURCE['author_name']}): {API}{SOURCE['file']} |",
        "| Date checked | 2026-10-02 |",
        f"| Format | Ogg Vorbis, stereo, 44.1 kHz, {size} bytes |",
        f"| SHA-1 (reported by Commons) | `{SOURCE['sha1']}` |",
        f"| SHA-256 of the download | `{digest}` |",
        "",
        "## How the files were made",
        "",
        f"Rebuild: `tools/pack-sources/{PACK_ID}/fetch.sh DIR`, then "
        f"`tools/pack-sources/{PACK_ID}/build.sh DIR`, where DIR holds `{SOURCE['file']}`. The "
        f"script is `tools/pack-sources/{PACK_ID}/springslice.py`; the exact cut list is "
        f"`tools/pack-sources/{PACK_ID}/selection.json`. It needs python3 with numpy, and ffmpeg.",
        "",
        "1. Decode with ffmpeg; downmix to mono as (L+R)/2; 2-pole high-pass at 30 Hz "
        "(removes DC). The source rate, 44.1 kHz, is kept.",
        "2. Find stroke onsets (1 ms energy frames of a 1 kHz high-passed copy, used for "
        "detection only).",
        "3. Start each file 0.5 ms (22 samples) before the stroke's attack, with a raised-cosine "
        "fade-in over those 22 samples.",
        "4. End it where the stroke decays to 3 dB above the recording's noise floor, or 1 ms "
        "before the next sound starts, whichever is first (at most 400 ms; 1.5 s for Enter), "
        "with a raised-cosine fade-out of up to 8 ms to an exact zero.",
        f"5. Apply one gain to every file: **{gain_db:+.2f} dB**, so that the median press "
        "peaks at -6 dBFS. No per-file normalization and no other processing (no EQ, no "
        "denoising, no compression).",
        "6. Write 16-bit PCM mono WAV at 44.1 kHz.",
        f"7. Loudness: `volume` in `pack.json` is **{VOLUME:.2f}** "
        f"({20 * math.log10(VOLUME):+.1f} dB), a playback gain the app applies to the whole "
        "pack, so no audio file changes. It matches typing loudness across the bundled packs, "
        "so switching packs does not jump in volume; the target is TakTak's reference typing level, "
        f"{REFERENCE_LK:.1f} LK. Measured with `cargo run -p synth-packs --release -- loudness "
        f"packs/{PACK_ID}` (`tools/synth-packs/src/loudness.rs`): the K-weighted (ITU-R "
        "BS.1770) energy of the first 100 ms of the sample each alphanumeric key plays on "
        "press by default, power-averaged over the keys, on the samples as the app loads them "
        "at 48 kHz. The "
        f"pack reads {LK_AS_BUILT:.1f} LK as built and "
        f"{LK_AS_BUILT + 20 * math.log10(VOLUME):.1f} LK at this volume. The build writes "
        "the volume (`VOLUME`), so a rebuild keeps it.",
        "",
        "Only clean, isolated strokes are kept: peak at least 20 dB over the 10 ms before it "
        "(15 dB for releases), decayed or at least 45 ms long when cut, and within -9..+5 dB "
        "of the median stroke of its kind; the Enter press and release are always kept.",
        "",
        "**Press or release.** This take is slow, with clear pairs: each loud stroke is followed "
        "80-140 ms later by a quieter one. Strokes were split by the peak of their first 15 ms "
        f"into two level clusters (2-means; threshold {info.get('threshold_db')} dBFS in the "
        "source): the louder ones are presses, the quieter ones releases, each release needing "
        "a key still held down. The Commons description gives the typed text as "
        "\"www.wikipedia.org[Enter]\", so the last press and the release after it are the "
        "`enter` group. Space and the other keys use the `alphanumeric` pool. Every release "
        "file is a recorded stroke that this split classed as a key-up; none is synthesized.",
        "",
        "### Files",
        "",
        "Source range: sample indices `[start, end)` in the decoded 44.1 kHz mono stream "
        "(step 1).",
        "",
        "| File | Group / action | Source range | Start (s) | Length (ms) | Source peak (dBFS) "
        "| SHA-256 as shipped |",
        "|---|---|---|---|---|---|---|",
    ]
    for f in sorted(files, key=lambda f: f["name"]):
        shipped = sha256(os.path.join(out_dir, "sounds", f["name"]))
        lines.append(f"| `sounds/{f['name']}` | {f['group']} {f['action']} | {f['start']}-{f['end']} | "
                     f"{f['start'] / RATE:.3f} | {(f['end'] - f['start']) / RATE * 1000:.1f} | "
                     f"{f['src_peak_db']:.1f} | `{shipped}` |")
    lines += [
        "",
        "`preview.wav` (SHA-256 `" + sha256(os.path.join(out_dir, "preview.wav")) + "`, "
        f"{preview_len / RATE * 1000:.1f} ms) types \"{typed}\" and Enter with the files above: "
        "a press and a release per key, 60-120 ms dwell, 70-170 ms from one key-down to the "
        "next (50 ms more after Space), with a fixed random seed. It is mixed by addition; if "
        "the mix peaked above -1 dBFS it would be scaled down to -1 dBFS.",
        "",
        "## Stats",
        "",
        *stats_table(out_dir),
        "",
    ]
    with open(os.path.join(out_dir, "SOURCES.md"), "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("--downloads", required=True)
    b.add_argument("--out", required=True)
    b.add_argument("--selection")
    b.add_argument("--reselect", action="store_true")
    s = sub.add_parser("stats")
    s.add_argument("pack_dir")
    args = ap.parse_args()
    if args.cmd == "build":
        build(args)
    else:
        print_stats(args.pack_dir)


if __name__ == "__main__":
    main()
