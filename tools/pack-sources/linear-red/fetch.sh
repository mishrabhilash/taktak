#!/usr/bin/env bash
# Downloads the original Wikimedia Commons recording used by packs/linear-red and checks its
# SHA-256 against the value recorded in packs/linear-red/SOURCES.md. Nothing downloaded is
# executed.
#
# usage: tools/pack-sources/linear-red/fetch.sh DOWNLOAD_DIR
set -euo pipefail

dl="${1:?usage: fetch.sh DOWNLOAD_DIR}"
dir="$dl/wikimedia-commons"
mkdir -p "$dir"

out="$dir/Typing_on_Keychron_V1_Ultra_(Red_Linear_Switch).wav"
if [ ! -f "$out" ]; then
    curl -L --fail -sS -o "$out" \
        'https://upload.wikimedia.org/wikipedia/commons/2/27/Typing_on_Keychron_V1_Ultra_(Red_Linear_Switch).wav'
fi
echo "85947c590e3831cf835609c11ceddce64dd5acd34eda08d24714c0bcc054ae4d  $out" | shasum -a 256 -c -
echo "ok: source in $dir"
