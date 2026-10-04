#!/bin/bash
# Xcode "Run Script" phase for the TakTakKeyboard target.
# Copies <repo>/packs/<pack>/pack.json (+ SOURCES.md for attribution) and only the
# WAVs that pack.json's groups reference into <appex>/pack/. The repo pack stays the
# single source of truth: no audio is duplicated under ios/.
set -euo pipefail

PACK_ID="${1:-tactile}"
SRC="${SRCROOT}/../../packs/${PACK_ID}"
DST="${TARGET_BUILD_DIR}/${UNLOCALIZED_RESOURCES_FOLDER_PATH}/pack"

[ -f "${SRC}/pack.json" ] || { echo "error: ${SRC}/pack.json not found"; exit 1; }

# The pack must be CC0 so it can ship inside an App Store binary without conditions.
if ! grep -Eq '"license"[[:space:]]*:[[:space:]]*"CC0-1.0"' "${SRC}/pack.json"; then
  echo "error: pack '${PACK_ID}' is not CC0-1.0; refusing to bundle it"
  exit 1
fi

rm -rf "${DST}"
mkdir -p "${DST}/sounds"
cp "${SRC}/pack.json" "${DST}/pack.json"
[ -f "${SRC}/SOURCES.md" ] && cp "${SRC}/SOURCES.md" "${DST}/SOURCES.md"

count=0
for rel in $(grep -oE '"sounds/[^"]+\.wav"' "${SRC}/pack.json" | tr -d '"' | sort -u); do
  cp "${SRC}/${rel}" "${DST}/${rel}"
  count=$((count + 1))
done

size_kb=$(du -sk "${DST}" | cut -f1)
if [ "${size_kb}" -ge 2048 ]; then
  echo "error: bundled pack is ${size_kb} KB (limit 2 MB)"
  exit 1
fi
echo "note: bundled pack '${PACK_ID}': ${count} wav files, ${size_kb} KB -> ${DST}"
