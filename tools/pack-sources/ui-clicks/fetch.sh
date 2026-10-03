#!/usr/bin/env bash
# Checks the two Kenney zips used by packs/ui-clicks against the SHA-256 values recorded in
# packs/ui-clicks/SOURCES.md. Kenney's download links carry a version hash that changes
# when a page is updated, so this script does not download them: get both zips with the
# "Download" button on
#   https://kenney.nl/assets/ui-audio          (kenney_ui-audio.zip)
#   https://kenney.nl/assets/interface-sounds  (kenney_interface-sounds.zip)
# and put them in DOWNLOAD_DIR/kenney/. A newer upload with a different hash may still hold
# the same sounds; build.py checks the hash and stops on a mismatch.
# Nothing in the zips is executed or extracted; build.py reads the .ogg members it needs.
#
# usage: tools/pack-sources/ui-clicks/fetch.sh DOWNLOAD_DIR
set -euo pipefail

dl="${1:?usage: fetch.sh DOWNLOAD_DIR}"
dir="$dl/kenney"
for f in kenney_ui-audio.zip kenney_interface-sounds.zip; do
    if [ ! -f "$dir/$f" ]; then
        echo "missing $dir/$f: download it from the Kenney asset page (see the top of this script)" >&2
        exit 1
    fi
done
shasum -a 256 -c - <<SUMS
946fc23a63d535d693eb31b2eabb80c8c28d6351e2186b344ceb71b2cb1d5eb6  $dir/kenney_ui-audio.zip
f2193d072726d6758a5f7871b2dcc54dcce0d5c35c6f0a62f92549b327c81232  $dir/kenney_interface-sounds.zip
SUMS
echo "ok: sources in $dir"
