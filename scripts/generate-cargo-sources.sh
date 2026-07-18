#!/usr/bin/env bash
#
# generate-cargo-sources.sh
#
# Generates cargo-sources.json in the repository root from Cargo.lock, using
# flatpak-cargo-generator.py from the flatpak-builder-tools project:
#   https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo
#
# The Flatpak manifest (com.chrisdaggas.bulk-renamer.yml) builds with
# `cargo --offline`, so every crate dependency must be vendored ahead of time.
# cargo-sources.json describes those vendored sources for flatpak-builder.
#
# Run this script whenever Cargo.lock changes, then commit the regenerated
# cargo-sources.json alongside it.
#
# Requirements: python3 with the 'aiohttp' and 'toml' modules
#   (e.g. pip install --user aiohttp toml
#    or on Fedora: dnf install python3-aiohttp python3-toml)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GENERATOR="${REPO_ROOT}/scripts/flatpak-cargo-generator.py"
GENERATOR_URL="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py"

cd "${REPO_ROOT}"

if [ ! -f Cargo.lock ]; then
    echo "error: Cargo.lock not found in ${REPO_ROOT}" >&2
    exit 1
fi

# Download the generator if it is not present.
if [ ! -f "${GENERATOR}" ]; then
    echo "Downloading flatpak-cargo-generator.py ..."
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "${GENERATOR}" "${GENERATOR_URL}"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "${GENERATOR}" "${GENERATOR_URL}"
    else
        echo "error: need curl or wget to download ${GENERATOR_URL}" >&2
        exit 1
    fi
fi

# Verify python dependencies early with a clear message.
if ! python3 -c 'import aiohttp, toml' 2>/dev/null; then
    echo "error: python3 modules 'aiohttp' and 'toml' are required." >&2
    echo "       Install with: pip install --user aiohttp toml" >&2
    exit 1
fi

echo "Generating cargo-sources.json from Cargo.lock ..."
python3 "${GENERATOR}" Cargo.lock -o cargo-sources.json

echo "Done: ${REPO_ROOT}/cargo-sources.json"
