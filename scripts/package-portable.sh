#!/usr/bin/env sh
set -eu
mkdir -p portable/ClipAnchor/data
cp -R src-tauri/target/release/bundle/* portable/ClipAnchor/ 2>/dev/null || true
echo "Portable package prepared under portable/ClipAnchor"
