#!/usr/bin/env bash
# Downloads the CC0 recordings of the "tactile" pack from clatterbox's GitHub repository,
# pinned to the commit that added them, with clatterbox's per-file credits.
#
# Usage: fetch.sh DOWNLOAD_DIR
# Files land in DOWNLOAD_DIR/clatterbox-tactile/. Nothing downloaded is ever executed;
# build.py checks every file's size and git blob SHA-1.
set -euo pipefail

dest="${1:?usage: fetch.sh DOWNLOAD_DIR}"
clatterbox_commit=9dca88f4d151e24f243b5a9aafec7a60c809fb48
base="https://raw.githubusercontent.com/zordhalo/clatterbox/${clatterbox_commit}/src-tauri/resources/packs/tactile"

mkdir -p "$dest/clatterbox-tactile"
for f in CREDITS.md pack.toml enter-down-01.mp3 backspace-down-01.mp3 \
         down-01.mp3 down-02.mp3 down-03.mp3 down-04.mp3 down-05.mp3 down-06.mp3 \
         down-07.mp3 down-08.mp3 down-09.mp3 down-10.mp3 down-11.mp3 down-12.mp3; do
  curl -L --fail --silent --show-error -o "$dest/clatterbox-tactile/$f" "$base/$f"
done
shasum -a 256 "$dest"/clatterbox-tactile/*
