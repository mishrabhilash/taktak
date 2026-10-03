#!/usr/bin/env bash
# Downloads the CC0 Wikimedia Commons recording behind packs/spring-lite ("IBM M2 sound.ogg" by
# Anonimski), plus the Commons API record of its license (extmetadata) as evidence.
#
# Usage: fetch.sh DOWNLOAD_DIR
#
# The file is checked against the SHA-256 recorded in packs/spring-lite/SOURCES.md
# (2026-10-02); its SHA-1 matches the one the Commons API reports. Nothing downloaded is
# ever executed.
set -euo pipefail

dest="${1:?usage: fetch.sh DOWNLOAD_DIR}"
ua="TakTak-pack-sources/1.0 (open-source keyboard sound app)"
api="https://commons.wikimedia.org/w/api.php?action=query&format=json&prop=imageinfo&iiprop=url%7Csize%7Cmime%7Csha1%7Cextmetadata%7Ctimestamp%7Cuser&titles=File:IBM_M2_sound.ogg"
file="IBM_M2_sound.ogg"
sha256="e620684e9b03b62319fb48da6149392cfd4042615e17a891554f8a9f6ac8515e"

mkdir -p "$dest"
curl -L --fail -sS -A "$ua" -o "$dest/commons-api.json" "$api"
if [ ! -f "$dest/$file" ]; then
  curl -L --fail -sS -A "$ua" -o "$dest/$file" "https://upload.wikimedia.org/wikipedia/commons/1/16/$file"
fi
echo "$sha256  $dest/$file" | shasum -a 256 -c -

# Print what Commons says about the file's license today.
python3 - "$dest/commons-api.json" <<'PY'
import json, sys
for p in json.load(open(sys.argv[1]))["query"]["pages"].values():
    ii = p["imageinfo"][0]
    em = ii["extmetadata"]
    print(f'{p["title"]}: {em["LicenseShortName"]["value"]} ({em["LicenseUrl"]["value"]}), '
          f'uploader {ii["user"]}, sha1 {ii["sha1"]}')
PY
