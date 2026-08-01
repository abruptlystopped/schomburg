#!/bin/zsh
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/dist/Schomburg.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cd "$root"
cargo build -p schomburg-host --release
cd "$root/apps/schomburg-macos"
swift build -c release
cp "$root/apps/schomburg-macos/.build/release/SchomburgMacOS" "$app/Contents/MacOS/Schomburg"
cp "$root/target/release/schomburg-host" "$app/Contents/MacOS/schomburg-host"
cp "$root/apps/schomburg-macos/Resources/Info.plist" "$app/Contents/Info.plist"
echo "$app"
