#!/usr/bin/env bash
# Download the CC0 source recordings used by packs/typewriter and verify their SHA-256.
#
#   tools/pack-sources/typewriter/fetch.sh DOWNLOAD_DIR
#
# Every file is by Joseph Sardin, BigSoundBank.com, CC0 1.0
# (https://bigsoundbank.com/licenses.html). The FLACs are the site's lossless
# downloads (48 kHz / 24-bit, Hermes Precisa 305, Neumann KM184 into a
# Sound Devices MixPre-3). No account or login is needed.
set -euo pipefail

dir=${1:?usage: fetch.sh DOWNLOAD_DIR}
mkdir -p "$dir/bigsoundbank"

# sound-number  sha256
files=(
  "2838 575a649462582a918e52892dfdf961703cb63b6a617c734bf8fab039acaf8f6d"
  "2839 bf794a52f5ef6d540c29c568293f0679330ae4e5331cbbccf0f37ca0beed3ddb"
  "2840 1ddad1613c6eaae41388c3177826aef5af3036e9e473034243b147b9c730eafe"
  "2841 aae3475fe07a3f06f021bcce837f68cd4cc435a8301a5bc18bd719331bcd4961"
  "2842 eef2c09a89af32f3415cbab19608426c34534b1e4eba837cdc66af72fbadb809"
  "2843 ccc16e0382b934b20c8d92a5d4ea2a505b0073b0b30e20b23e5522439f1fa772"
)

for entry in "${files[@]}"; do
  read -r n sum <<<"$entry"
  out="$dir/bigsoundbank/$n.flac"
  if [[ ! -f $out ]]; then
    curl -L --fail --silent --show-error -o "$out" "https://bigsoundbank.com/UPLOAD/flac/$n.flac"
  fi
  got=$(shasum -a 256 "$out" | cut -d' ' -f1)
  if [[ $got != "$sum" ]]; then
    echo "SHA-256 mismatch for $out: got $got, want $sum" >&2
    exit 1
  fi
  echo "ok  $out"
done
