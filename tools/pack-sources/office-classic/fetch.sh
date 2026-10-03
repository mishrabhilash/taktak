#!/usr/bin/env bash
# Downloads the source recordings for packs/office-classic and checks their SHA-256.
#
#   tools/pack-sources/office-classic/fetch.sh [DOWNLOAD_DIR]
#
# Source: "Keyboard Soundpack #1 [Typing and Single Keystrokes]" by unicaegames, CC0-1.0,
# https://opengameart.org/content/keyboard-soundpack-1-typing-and-single-keystrokes
# No account is needed. Nothing downloaded is executed.
set -euo pipefail

dir="${1:-downloads/unicaegames}"
url="https://opengameart.org/sites/default/files/unicae_games_keyboard_soundpack_1_0.zip"
file="$dir/unicae_games_keyboard_soundpack_1_0.zip"
sha256="935eae2fa5c3742eacdd38c4ea0e9047f3887faa0701996498492c256db1b351"

mkdir -p "$dir"
if [[ ! -f "$file" ]]; then
    curl -L --fail -sS -o "$file.part" "$url"
    mv "$file.part" "$file"
fi

actual="$(shasum -a 256 "$file" | cut -d' ' -f1)"
if [[ "$actual" != "$sha256" ]]; then
    echo "error: $file has SHA-256 $actual, expected $sha256" >&2
    echo "The upstream zip changed. Re-check its license and update SOURCES.md before rebuilding." >&2
    exit 1
fi
echo "ok: $file"
echo "next: python3 tools/pack-sources/office-classic/build.py \"$file\" packs/office-classic"
