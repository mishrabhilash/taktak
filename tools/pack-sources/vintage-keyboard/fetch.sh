#!/usr/bin/env bash
# Downloads jim-ph's CC0 "vintage keyboard" recordings (Freesound pack 12363) as mirrored,
# with Freesound's own license.txt, in Tickeys' mechanical/ folder on GitHub, pinned to the
# last commit that touched that folder. Tickeys' other sound folders have no stated origin
# and are not used.
#
# Usage: fetch.sh DOWNLOAD_DIR
# Files land in DOWNLOAD_DIR/tickeys-mechanical/. Nothing downloaded is ever executed;
# build.py checks every file's size and git blob SHA-1.
set -euo pipefail

dest="${1:?usage: fetch.sh DOWNLOAD_DIR}"
tickeys_commit=028083403ab75eae752446ce30cffb3b22b49eab
base="https://raw.githubusercontent.com/yingDev/Tickeys/${tickeys_commit}/Tickeys.app/Contents/Resources/data/mechanical"

mkdir -p "$dest/tickeys-mechanical"
for f in license.txt 1.wav 2.wav 3.wav 4.wav 5.wav; do
  curl -L --fail --silent --show-error -o "$dest/tickeys-mechanical/$f" "$base/$f"
done
shasum -a 256 "$dest"/tickeys-mechanical/*
