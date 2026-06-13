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

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
RELEASE="5"
RPM_TOPDIR="$(mktemp -d /tmp/bulk-renamer-rpmbuild.XXXXXX)"
SPEC_FILE="$RPM_TOPDIR/SPECS/bulk-renamer.spec"
SOURCES="$RPM_TOPDIR/SOURCES"
trap 'rm -rf "$RPM_TOPDIR"' EXIT

mkdir -p "$RPM_TOPDIR"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}

# Stage sources (mirrors speech-to-text packaging: Source0..N)
cp "target/release/bulk-renamer" "$SOURCES/bulk-renamer"
cp "data/com.chrisdaggas.bulk-renamer.desktop" "$SOURCES/"
cp "data/icons/hicolor/scalable/apps/com.chrisdaggas.bulk-renamer.svg" "$SOURCES/"
cp "data/icons/symbolic/apps/com.chrisdaggas.bulk-renamer-symbolic.svg" "$SOURCES/"

cat > "$SPEC_FILE" <<EOF
Name:           bulk-renamer
Version:        $VERSION
Release:        $RELEASE%{?dist}
Summary:        A GNOME-native bulk file renaming application
License:        MIT
URL:            https://github.com/christosdaggas/bulk-renamer

# We use a pre-built binary, no source archive needed
Source0:        bulk-renamer
Source1:        com.chrisdaggas.bulk-renamer.desktop
Source2:        com.chrisdaggas.bulk-renamer.svg
Source3:        com.chrisdaggas.bulk-renamer-symbolic.svg

BuildArch:      x86_64

Requires:       gtk4
Requires:       libadwaita

%description
Bulk Renamer is a GTK4/libadwaita application for safely renaming files in
batches with rule-based previews, undo support, presets, and CSV import.

%install
# Binary
install -Dm755 "%{SOURCE0}" "%{buildroot}%{_bindir}/bulk-renamer"

# Desktop file
install -Dm644 "%{SOURCE1}" "%{buildroot}%{_datadir}/applications/com.chrisdaggas.bulk-renamer.desktop"

# Icons
install -Dm644 "%{SOURCE2}" "%{buildroot}%{_datadir}/icons/hicolor/scalable/apps/com.chrisdaggas.bulk-renamer.svg"
install -Dm644 "%{SOURCE3}" "%{buildroot}%{_datadir}/icons/hicolor/symbolic/apps/com.chrisdaggas.bulk-renamer-symbolic.svg"

%files
%{_bindir}/bulk-renamer
%{_datadir}/applications/com.chrisdaggas.bulk-renamer.desktop
%{_datadir}/icons/hicolor/scalable/apps/com.chrisdaggas.bulk-renamer.svg
%{_datadir}/icons/hicolor/symbolic/apps/com.chrisdaggas.bulk-renamer-symbolic.svg

%post
/usr/bin/update-desktop-database &>/dev/null || :
/usr/bin/gtk-update-icon-cache %{_datadir}/icons/hicolor &>/dev/null || :

%postun
/usr/bin/update-desktop-database &>/dev/null || :
/usr/bin/gtk-update-icon-cache %{_datadir}/icons/hicolor &>/dev/null || :

%changelog
* Sun May 24 2026 Christos A. Daggas <chris@daggas.com> - $VERSION-$RELEASE
- Mirror speech-to-text spec layout exactly (Source0..N, no metainfo,
  no docdir/licensedir, no explicit Requires(post), no debug/strip globals)
EOF

rpmbuild \
    --define "_topdir $RPM_TOPDIR" \
    -bb "$SPEC_FILE"

# Create dist directory and move package
mkdir -p dist/rpm
rm -f dist/rpm/*.rpm
mv "$RPM_TOPDIR"/RPMS/*/*.rpm dist/rpm/

echo "✅ RPM package created:"
ls -lh dist/rpm/*.rpm
