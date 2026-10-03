#!/usr/bin/env bash
# Downloads the original OpenGameArt files used by packs/key-press, checks their
# SHA-256 against the values recorded in packs/key-press/SOURCES.md, and unpacks
# the eklee zip. Nothing downloaded is executed.
#
# usage: tools/pack-sources/key-press/fetch.sh DOWNLOAD_DIR
set -euo pipefail

dl="${1:?usage: fetch.sh DOWNLOAD_DIR}"
eklee="$dl/oga-eklee-key-presses"
bluszcz="$dl/oga-bluszcz-mechanical-keyboard"
mkdir -p "$eklee" "$bluszcz"

fetch() {
    local url="$1" out="$2" sha="$3"
    if [ ! -f "$out" ]; then
        curl -L --fail -sS -o "$out" "$url"
    fi
    echo "$sha  $out" | shasum -a 256 -c -
}

fetch https://opengameart.org/sites/default/files/eklee-KeyPresses-cc0-opengameart.zip \
    "$eklee/eklee-KeyPresses-cc0-opengameart.zip" \
    45c9b07560f7123ced930584c9b6a6de50551620b1c968b973f495dec21c8987
fetch https://opengameart.org/sites/default/files/keyboard01_0.ogg \
    "$bluszcz/keyboard01_0.ogg" \
    5917fc7ce559058569bbdd9df9f32c6e07eda3a13a1be3e7a090761a24cee227
fetch https://opengameart.org/sites/default/files/keyboard02_0.ogg \
    "$bluszcz/keyboard02_0.ogg" \
    21815899715b18b9c703b0a94dde19aad1ad7561b3a6d95bff088442dcaeefc4

# Only the 48 kHz FLAC originals and the text files are needed.
unzip -o -q "$eklee/eklee-KeyPresses-cc0-opengameart.zip" \
    'eklee-KeyPresses-cc0-opengameart/flac48000/*' \
    'eklee-KeyPresses-cc0-opengameart/*.txt' \
    -d "$eklee/extracted"
echo "ok: sources in $dl"
