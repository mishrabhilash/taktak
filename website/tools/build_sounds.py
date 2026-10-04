#!/usr/bin/env python3
"""Builds the website's sound assets from the bundled packs in ../../packs.

Optional: the output is committed, so the site itself needs no build step. Run it again
after a bundled pack changes:

    python3 website/tools/build_sounds.py [--ffmpeg /opt/homebrew/bin/ffmpeg]

Needs Python 3.9+ and ffmpeg with libmp3lame. Writes, for every pack in packs/:

  website/assets/sounds/<id>/preview.mp3   the pack's own preview.wav, mono MP3
  website/assets/sounds/<id>/keys.mp3      every sound the "type here" box can play, one after
                                           the other with silence between (an audio sprite)
  website/assets/sounds/<id>/keys.json     where each sound starts in keys.mp3, and which sound
                                           each key plays on press and on release
  website/assets/sounds/<id>/LICENSE.txt   copied from the pack, when it has one
  website/index.html                       the pack <option>s and cards, between the
                                           "packs:options" and "packs:cards" markers

and removes the folders in website/assets/sounds/ of packs that are no longer bundled.

Which sound a key plays is resolved exactly like the app does (docs/pack-format.md,
"Resolution" and "Sound sets"): keys[<key>] -> groups[<group>] -> groups.other ->
groups.alphanumeric, and among several files the one picked by the app's fixed hash of the
key name (src-tauri/core/src/audio/mixer.rs, consistent_index), so a key sounds the same on
the website as in the app. Leading silence is trimmed like the app's trim_silence.

Per-key sounds (`keys` in pack.json) are included only for the keys people actually type into
a text box (TYPED_KEYS); other keys fall back to their group, to keep the page light.
"""

import argparse
import array
import html
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

WEB = Path(__file__).resolve().parent.parent
ROOT = WEB.parent
PACKS = ROOT / "packs"
OUT = WEB / "assets" / "sounds"
KEY_RS = ROOT / "src-tauri" / "core" / "src" / "key.rs"

RATE = 44100
PREVIEW_BITRATE = "64k"
KEYS_BITRATE = "64k"
SILENCE_THRESHOLD = 0.00316  # -50 dBFS, as in src-tauri/core/src/pack/decode.rs
PRE_ROLL_SECONDS = 0.0005
LEAD_SECONDS = 0.10  # silence before the first sound in the sprite
GAP_SECONDS = 0.08  # silence after every sound

# Short card text, from the README's pack table (the full descriptions are in packs.json).
BLURBS = {
    "key-press": "Light, crisp ticks; an older board thumping under Space and the modifiers.",
    "linear-red": "Fast typing on red linear switches: short letter clacks and a deeper space bar.",
    "office-classic": "The soft, rounded clack of an everyday office membrane keyboard.",
    "spring-lite": "A compact, lighter buckling-spring board: bright clicks, softer up-strokes.",
    "tactile": "Bright, snappy tactile switches with a separate key-up.",
    "typewriter": "A portable manual typewriter, with a carriage return and bell on Enter.",
    "ui-clicks": "Not a keyboard: crisp interface clicks and a two-note blip on Enter.",
    "vintage-keyboard": "Crisp clacks from a 1986 DOS-era mechanical keyboard.",
}
RECORDED_BY = {
    "key-press": "eklee, qubodup, bluszcz",
    "linear-red": "C40115",
    "office-classic": "unicaegames",
    "spring-lite": "Anonimski",
    "tactile": "StavSounds, alpinemesh, yottasounds",
    "typewriter": "Joseph Sardin (BigSoundBank)",
    "ui-clicks": "Kenney",
    "vintage-keyboard": "jim-ph",
}
# Badge text and class. The badge links to the pack's section of CREDITS.md on GitHub.
LICENSE_LABELS = {
    "MIT": ("MIT", "mit"),
    "CC0-1.0": ("CC0", "cc0"),
    "CC-BY-3.0": ("CC BY 3.0", "ccby"),
    "CC-BY-4.0": ("CC BY 4.0", "ccby"),
}
# CC BY requires a link (or URI) to the license next to the credit; shown as text, not a link,
# so the page links to nothing but GitHub and taktak.tech.
LICENSE_URIS = {
    "CC-BY-3.0": "https://creativecommons.org/licenses/by/3.0/",
    "CC-BY-4.0": "https://creativecommons.org/licenses/by/4.0/",
}
DEFAULT_PACK = "tactile"

ALPHANUMERIC_EXTRA = {
    "Backquote", "Minus", "Equal", "BracketLeft", "BracketRight", "Backslash", "Semicolon",
    "Quote", "Comma", "Period", "Slash", "IntlBackslash", "IntlRo", "IntlYen",
}
MODIFIERS = {
    "ShiftLeft", "ShiftRight", "ControlLeft", "ControlRight", "AltLeft", "AltRight",
    "MetaLeft", "MetaRight", "CapsLock", "Fn",
}
TYPED_KEYS = (
    {f"Key{c}" for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ"}
    | {f"Digit{d}" for d in range(10)}
    | (ALPHANUMERIC_EXTRA - {"IntlBackslash", "IntlRo", "IntlYen"})
    | {"Space", "Enter", "Backspace", "Tab", "ShiftLeft", "ShiftRight", "CapsLock",
       "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"}
)


def group_of(code):
    if code.startswith("Key") and len(code) == 4 or code.startswith("Digit") or code in ALPHANUMERIC_EXTRA:
        return "alphanumeric"
    if code == "Space":
        return "space"
    if code in ("Enter", "NumpadEnter"):
        return "enter"
    if code in ("Backspace", "Delete"):
        return "backspace"
    if code in MODIFIERS:
        return "modifiers"
    return "other"


M64 = (1 << 64) - 1


def key_hash(code):
    """SplitMix64 of the 64-bit FNV-1a hash of the key name (mixer.rs KEY_HASHES)."""
    h = 0xCBF29CE484222325
    for b in code.encode():
        h ^= b
        h = (h * 0x100000001B3) & M64
    z = (h + 0x9E3779B97F4A7C15) & M64
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return z ^ (z >> 31)


def all_keys():
    text = KEY_RS.read_text()
    block = text[text.index("keys! {"):]
    block = block[: block.index("\n}\n")]
    return re.findall(r'=>\s*"(\w+)"', block)


def resolve(pack, code, action, use_keys):
    chain = []
    if use_keys:
        chain.append(pack.get("keys", {}).get(code, {}))
    groups = pack.get("groups", {})
    chain += [groups.get(group_of(code), {}), groups.get("other", {}), groups.get("alphanumeric", {})]
    for sound_set in chain:
        files = sound_set.get(action) or []
        if files:
            return files[key_hash(code) % len(files)]
    return None


def decode(ffmpeg, path):
    raw = subprocess.run(
        [ffmpeg, "-v", "error", "-i", str(path), "-ac", "1", "-ar", str(RATE), "-f", "s16le", "-"],
        check=True, capture_output=True,
    ).stdout
    samples = array.array("h")
    samples.frombytes(raw)
    if sys.byteorder == "big":
        samples.byteswap()
    return samples


def trim(samples):
    limit = SILENCE_THRESHOLD * 32768
    first = next((i for i, s in enumerate(samples) if abs(s) >= limit), None)
    if first is None:
        return samples
    cut = max(0, first - round(PRE_ROLL_SECONDS * RATE))
    return samples[cut:]


def align_point(samples, start):
    """Where the page re-aligns the decoded sprite, in case the MP3 decoder shifts it: the
    first sample at or above 10% of the first clip's peak, and that level (0..1)."""
    peak = max((abs(s) for s in samples), default=0)
    limit = max(peak * 0.1, 1)
    first = next(i for i, s in enumerate(samples) if abs(s) >= limit)
    return (start + first) / RATE, limit / 32768


def encode(ffmpeg, src, dst, bitrate):
    subprocess.run(
        [ffmpeg, "-v", "error", "-y", "-i", str(src), "-map_metadata", "-1", "-ac", "1",
         "-ar", str(RATE), "-codec:a", "libmp3lame", "-b:a", bitrate, str(dst)],
        check=True,
    )


def build_pack(ffmpeg, pack_dir, key_names, tmp):
    pack = json.loads((pack_dir / "pack.json").read_text())
    pid = pack["id"]
    out = OUT / pid
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    preview = pack.get("preview") or (
        (pack.get("groups", {}).get("alphanumeric") or pack["groups"]["other"])["press"][0]
    )
    encode(ffmpeg, pack_dir / preview, out / "preview.mp3", PREVIEW_BITRATE)

    files = []  # sprite order
    keymap = {}
    for code in key_names:
        pair = []
        for action in ("press", "release"):
            f = resolve(pack, code, action, use_keys=code in TYPED_KEYS)
            if f is None:
                pair.append(-1)
                continue
            if f not in files:
                files.append(f)
            pair.append(files.index(f))
        keymap[code] = pair

    trim_on = pack.get("trim_silence", True)
    sprite = array.array("h", bytes(2 * round(LEAD_SECONDS * RATE)))
    gap = array.array("h", bytes(2 * round(GAP_SECONDS * RATE)))
    slices = []
    align = None
    for f in files:
        samples = decode(ffmpeg, pack_dir / f)
        if trim_on:
            samples = trim(samples)
        start = len(sprite)
        if align is None:
            align = align_point(samples, start)
        slices.append([round(start / RATE, 5), round(len(samples) / RATE, 5)])
        sprite.extend(samples)
        sprite.extend(gap)

    raw = tmp / f"{pid}.raw"
    data = sprite
    if sys.byteorder == "big":
        data = array.array("h", sprite)
        data.byteswap()
    raw.write_bytes(data.tobytes())
    subprocess.run(
        [ffmpeg, "-v", "error", "-y", "-f", "s16le", "-ar", str(RATE), "-ac", "1", "-i", str(raw),
         "-map_metadata", "-1", "-codec:a", "libmp3lame", "-b:a", KEYS_BITRATE, str(out / "keys.mp3")],
        check=True,
    )
    (out / "keys.json").write_text(
        json.dumps({"align": round(align[0], 5), "alignLevel": round(align[1], 5), "slices": slices, "keys": keymap}, separators=(",", ":"))
        + "\n"
    )

    has_license_file = (pack_dir / "LICENSE.txt").exists()
    if has_license_file:
        shutil.copyfile(pack_dir / "LICENSE.txt", out / "LICENSE.txt")

    label, badge = LICENSE_LABELS[pack["license"]]
    variation = pack.get("variation", {})
    name = pack["name"]
    return {
        "id": pid,
        "name": name,
        "slug": slug(f"{name} ({pid})"),
        "blurb": BLURBS.get(pid, pack.get("description", "")),
        "recordedBy": RECORDED_BY.get(pid, pack["author"]),
        "license": pack["license"],
        "licenseLabel": label,
        "badge": badge,
        "attribution": pack.get("attribution"),
        "source": pack.get("source"),
        "licenseFile": f"assets/sounds/{pid}/LICENSE.txt" if has_license_file else None,
        "volume": pack.get("volume", 1.0),
        "pitch": variation.get("pitch", 0.03),
        "varVolume": variation.get("volume", 0.10),
        "sounds": len(files),
        "default": pid == DEFAULT_PACK,
    }


def slug(heading):
    """GitHub's anchor for a Markdown heading (enough for the CREDITS.md pack headings)."""
    return re.sub(r"[^a-z0-9 -]", "", heading.lower()).replace(" ", "-")


def card(p):
    e = html.escape
    if p["license"] in LICENSE_URIS:
        # CC BY: the full credit line, the license URI and the changes, as the license asks.
        uri = LICENSE_URIS[p["license"]]
        attribution = p["attribution"].rstrip()
        credit = e(attribution if attribution[-1:] in ".)" else attribution + ".")
        if "http" not in attribution and p["source"]:
            credit += f" Source: {e(p['source'])}."
        if uri not in attribution:
            credit += f" License: {e(uri)}."
        credit += " Converted to MP3 for this page."
    elif p["license"].startswith("CC0"):
        credit = f'Recorded by {e(p["recordedBy"])}. CC0: no credit required, given with thanks.'
    else:
        credit = f'Recorded by {e(p["recordedBy"])}.'
    if p["licenseFile"]:
        credit += f' <a href="{p["licenseFile"]}">License text</a>'
    return f"""          <li class="pack" data-pack="{p["id"]}" data-volume="{p["volume"]}" data-pitch="{p["pitch"]}" data-var-volume="{p["varVolume"]}">
            <div class="pack-head">
              <h4 class="pack-name">{e(p["name"])}</h4>
              <a class="badge badge-{p["badge"]}" data-repo="/blob/main/CREDITS.md#{p["slug"]}" href="#credits" title="License: {e(p["licenseLabel"])} (credits)">{e(p["licenseLabel"])}</a>
            </div>
            <p class="pack-blurb">{e(p["blurb"])}</p>
            <p class="pack-credit">{credit}</p>
            <div class="pack-actions">
              <button type="button" class="btn btn-play" data-preview="{p["id"]}" aria-pressed="false"><svg class="icon" aria-hidden="true"><use href="#i-play"/></svg><span class="btn-text">Play</span><span class="visually-hidden"> {e(p["name"])} preview</span></button>
              <button type="button" class="btn btn-ghost" data-try="{p["id"]}">Type with it<span class="visually-hidden">: {e(p["name"])}</span></button>
            </div>
          </li>
"""


def replace_between(text, marker, body, indent):
    start = f"<!-- {marker}:start -->"
    end = f"<!-- {marker}:end -->"
    a = text.index(start) + len(start)
    b = text.index(end)
    return text[:a] + "\n" + body + indent + text[b:]


def write_html(packs):
    index = WEB / "index.html"
    text = index.read_text()
    options = "".join(
        f'                <option value="{p["id"]}"{" selected" if p["default"] else ""}>{html.escape(p["name"])}</option>\n'
        for p in packs
    )
    text = replace_between(text, "packs:options", options, " " * 16)
    text = replace_between(text, "packs:cards", "".join(card(p) for p in packs), " " * 10)
    text = re.sub(r"The \d+ bundled packs", f"The {len(packs)} bundled packs", text)
    index.write_text(text)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--ffmpeg", default=shutil.which("ffmpeg") or "/opt/homebrew/bin/ffmpeg")
    args = parser.parse_args()

    key_names = all_keys()
    pack_dirs = sorted(p for p in PACKS.iterdir() if (p / "pack.json").exists())
    packs = []
    with tempfile.TemporaryDirectory() as tmp:
        for pack_dir in pack_dirs:
            info = build_pack(args.ffmpeg, pack_dir, key_names, Path(tmp))
            packs.append(info)
            print(f"{info['id']:18} {info['sounds']:3} sounds")
    bundled = {p["id"] for p in packs}
    for stale in sorted(d for d in OUT.iterdir() if d.is_dir() and d.name not in bundled):
        shutil.rmtree(stale)
        print(f"{stale.name:18} removed (no longer bundled)")
    packs.sort(key=lambda p: (not p["default"], p["name"]))
    write_html(packs)


if __name__ == "__main__":
    main()
