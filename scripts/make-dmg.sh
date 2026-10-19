#!/bin/sh
# Build a plain drag-to-Applications DMG with hdiutil only.
# `cargo tauri build` makes a prettier one but drives Finder through
# AppleScript, which times out without a GUI session (SSH, locked screen).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
bundle="$root/target/release/bundle"
app=$(find "$bundle/macos" -maxdepth 1 -name '*.app' | head -n 1)
if [ -z "$app" ]; then
  (cd "$root/app/src-tauri" && cargo tauri build --bundles app)
  app=$(find "$bundle/macos" -maxdepth 1 -name '*.app' | head -n 1)
fi

name=$(basename "$app" .app)
version=$(sed -n 's/^ *"version": "\(.*\)",/\1/p' "$root/app/src-tauri/tauri.conf.json")
out="$bundle/dmg/${name}_${version}_$(uname -m).dmg"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
ditto "$app" "$stage/$name.app"
ln -s /Applications "$stage/Applications"

mkdir -p "$bundle/dmg"
rm -f "$out"
hdiutil create -quiet -volname "$name" -srcfolder "$stage" -fs HFS+ -format UDZO "$out"
echo "$out"
