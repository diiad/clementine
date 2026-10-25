#!/bin/sh
# Download the static ffmpeg build bundled with the macOS app (Apple silicon)
# into app/src-tauri/binaries/, where `tauri.ffmpeg.conf.json` picks it up as
# a sidecar. Pinned and checksummed; bump both together.
set -eu

URL="https://ffmpeg.martin-riedl.de/download/macos/arm64/1789931890_9.0.2/ffmpeg.zip"
SHA256="c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924"
VERSION="9.0.2"

root=$(cd "$(dirname "$0")/.." && pwd)
dest="$root/app/src-tauri/binaries"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -fsSL -o "$tmp/ffmpeg.zip" "$URL"
echo "$SHA256  $tmp/ffmpeg.zip" | shasum -a 256 -c -
unzip -q "$tmp/ffmpeg.zip" -d "$tmp/out"
mkdir -p "$dest"
install -m 755 "$tmp/out/ffmpeg" "$dest/ffmpeg-aarch64-apple-darwin"
# ffmpeg is GPL: ship its license and say where the source is.
curl -fsSL -o "$dest/FFMPEG-LICENSE.txt" "https://www.gnu.org/licenses/gpl-3.0.txt"
cat > "$dest/FFMPEG-NOTICE.txt" <<NOTICE
Clémentine bundles ffmpeg $VERSION (https://ffmpeg.org), a separate program
licensed under the GNU GPL version 3 (FFMPEG-LICENSE.txt). This build comes
from $URL (https://ffmpeg.martin-riedl.de). Its source code is available at
https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz
NOTICE
echo "$dest/ffmpeg-aarch64-apple-darwin"
