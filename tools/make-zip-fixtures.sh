#!/usr/bin/env bash
# Builds tests/data/zip/*.zip with Info-ZIP `zip` 3.0 (black-box producer; its source is never read).
# Content is our own text; the archives are committed as fixtures. Re-run only to regenerate.
set -euo pipefail
cd "$(dirname "$0")/.."
out=tests/data/zip
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cd "$tmp"
mkdir -p docs/sub
printf 'hello, zip\n' > hello.txt
for i in $(seq 1 400); do printf 'line %d: the quick brown fox jumps over the lazy dog\n' "$i"; done > docs/long.txt
printf 'nested\n' > docs/sub/n.txt
printf 'unicode name\n' > 'café-日本.txt'
head -c 3000 /dev/urandom > random.bin
zip -q -X -0 "$OLDPWD/$out/stored.zip" hello.txt docs/long.txt
zip -q -X -9 "$OLDPWD/$out/deflate.zip" hello.txt docs/long.txt docs/sub/n.txt random.bin
zip -q -X -r "$OLDPWD/$out/dirs.zip" docs
zip -q -X -fz "$OLDPWD/$out/zip64.zip" hello.txt docs/long.txt
zip -q -X -UN=UTF8 "$OLDPWD/$out/utf8.zip" 'café-日本.txt'
cat docs/long.txt | zip -q -X - - | cat > "$OLDPWD/$out/stream.zip"   # streaming to a pipe: data descriptor (bit 3), name "-"
zip -q -X "$OLDPWD/$out/comment.zip" hello.txt
printf 'archive comment here' | zip -q -z "$OLDPWD/$out/comment.zip"
cp hello.txt docs/long.txt docs/sub/n.txt random.bin "$OLDPWD/$out/" 2>/dev/null || true
mkdir -p "$OLDPWD/$out/src" && cp -r hello.txt docs random.bin 'café-日本.txt' "$OLDPWD/$out/src/"
rm -f "$OLDPWD/$out/hello.txt" "$OLDPWD/$out/long.txt" "$OLDPWD/$out/n.txt" "$OLDPWD/$out/random.bin"
ls -la "$OLDPWD/$out"
