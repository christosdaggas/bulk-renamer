#!/usr/bin/env bash
# package-rpm.sh – Build RPM package
# Usage: ./scripts/package-rpm.sh
set -euo pipefail
cd "$(dirname "$0")/.."

echo "📦 Building RPM package..."

# Ensure release build exists
if [[ ! -f target/release/bulk-renamer ]]; then
    echo "Release build not found. Building..."
    cargo build --release
fi

# Generate RPM
cargo generate-rpm

# Create dist directory and move package
mkdir -p dist/rpm
rm -f dist/rpm/*.rpm
mv target/generate-rpm/*.rpm dist/rpm/

echo "✅ RPM package created:"
ls -lh dist/rpm/*.rpm
