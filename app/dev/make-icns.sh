#!/bin/sh
# Makes src-tauri/icons/icon.icns from icon.png with the tools macOS ships (sips, iconutil). Run from app/.
set -eu
src=src-tauri/icons/icon.png
set_dir=$(mktemp -d)/icon.iconset
mkdir -p "$set_dir"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$src" --out "$set_dir/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$src" --out "$set_dir/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set_dir" -o src-tauri/icons/icon.icns
echo "wrote src-tauri/icons/icon.icns"
