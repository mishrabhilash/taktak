#!/usr/bin/env bash
# Rebuilds packs/spring-lite from its Wikimedia Commons CC0 recording.
#   ./fetch.sh DOWNLOAD_DIR              (downloads IBM_M2_sound.ogg and checks its SHA-256)
#   ./build.sh DOWNLOAD_DIR [--reselect]
# Needs python3 with numpy, and ffmpeg. The slicer is springslice.py; the exact cut list is
# selection.json, reused unless --reselect is given, so a rebuild is byte for byte.
set -euo pipefail
downloads="${1:?usage: build.sh DOWNLOAD_DIR [--reselect]}"
shift
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
python3 "$here/springslice.py" build --downloads "$downloads" \
  --out "$root/packs/spring-lite" --selection "$here/selection.json" "$@"
