#!/usr/bin/env bash
# WITHHELD: TakTak does not currently ship this pack. It is withheld pending the confirmation of
# bucklespring's author (Ico Doornekamp) that the MIT-era sample snapshot may be bundled. These
# scripts stay so the pack can be rebuilt once that is confirmed. To bundle it again: build it into
# packs/buckling-spring, remove "buckling-spring" from RETIRED_PACK_IDS (src-tauri/src/state.rs),
# and restore its entries in CREDITS.md, README.md and the website.
#
# Fetches the inputs of the "Buckling Spring" pack (id buckling-spring): only wav/*.wav and
# LICENSE from bucklespring at commit 63b3d5b8ea84b426b466146ced5cbf85fe3bc0cb, the last commit
# whose LICENSE is MIT. Later commits are GPL-2.0, and the keypad samples added later are
# GPL-only, so never fetch from another ref.
#
# Usage: fetch.sh [--dry-run] DOWNLOAD_DIR
#   --dry-run  print the files that would be downloaded (URL and size) and write nothing.
#
# Writes DOWNLOAD_DIR/{tree.json,LICENSE,wav/*.wav,SHA256SUMS}. Every file is checked against
# the git blob SHA-1 in the commit's tree, so the downloads are byte-identical to the snapshot.
# Then run build.py --src DOWNLOAD_DIR. Nothing downloaded is ever executed.
set -euo pipefail

repo="zevv/bucklespring"
commit="63b3d5b8ea84b426b466146ced5cbf85fe3bc0cb"
api="https://api.github.com/repos/$repo/git/trees/$commit?recursive=1"
raw="https://raw.githubusercontent.com/$repo/$commit"

dry_run=0
if [ "${1:-}" = "--dry-run" ]; then
    dry_run=1
    shift
fi
dest="${1:?usage: fetch.sh [--dry-run] DOWNLOAD_DIR}"

# Git blob SHA-1 of a file: sha1("blob <size>\0" + content).
blob_sha() {
    python3 -c 'import hashlib, sys
d = open(sys.argv[1], "rb").read()
print(hashlib.sha1(b"blob %d\0" % len(d) + d).hexdigest())' "$1"
}

tree_json="$(curl -L --fail -sS "$api")"

# "path sha size" for LICENSE and wav/XX-F.wav only; the snapshot also holds an .exe and
# Windows libraries, which are never fetched.
listing="$(printf '%s' "$tree_json" | python3 -c 'import json, re, sys
t = json.load(sys.stdin)
if t.get("truncated"):
    sys.exit("tree listing is truncated")
for e in t["tree"]:
    p = e["path"]
    if e["type"] == "blob" and (p == "LICENSE" or re.fullmatch(r"wav/[0-9a-f]{2}-[01]\.wav", p)):
        print(p, e["sha"], e["size"])')"

count="$(printf '%s\n' "$listing" | wc -l | tr -d ' ')"
bytes="$(printf '%s\n' "$listing" | awk '{ s += $3 } END { print s }')"

if [ "$dry_run" = 1 ]; then
    printf '%s\n' "$listing" | while read -r path sha size; do
        printf '%s/%s\t%s bytes\n' "$raw" "$path" "$size"
    done
    echo "$count files, $bytes bytes"
    exit 0
fi

mkdir -p "$dest/wav"
printf '%s\n' "$tree_json" > "$dest/tree.json"

while read -r path sha size; do
    out="$dest/$path"
    if [ -f "$out" ] && [ "$(blob_sha "$out")" = "$sha" ]; then
        continue
    fi
    curl -L --fail -sS -o "$out.part" "$raw/$path"
    if [ "$(blob_sha "$out.part")" != "$sha" ]; then
        rm -f "$out.part"
        echo "checksum mismatch for $path" >&2
        exit 1
    fi
    mv "$out.part" "$out"
done <<< "$listing"

(cd "$dest" && shasum -a 256 LICENSE wav/*.wav > SHA256SUMS)
echo "fetched $count files ($bytes bytes) into $dest"
