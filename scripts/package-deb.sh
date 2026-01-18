#!/usr/bin/env bash
# package-deb.sh – Build Debian package
# Usage: ./scripts/package-deb.sh
set -euo pipefail
cd "$(dirname "$0")/.."

echo "📦 Building DEB package..."

# Ensure release build exists
if [[ ! -f target/release/bulk-renamer ]]; then
    echo "Release build not found. Building..."
    cargo build --release
fi

# Generate DEB
cargo deb

# Create dist directory and move package
mkdir -p dist/deb
rm -f dist/deb/*.deb
mv target/debian/*.deb dist/deb/

echo "✅ DEB package created:"
ls -lh dist/deb/*.deb
