#!/bin/sh
# Builds the Linux archive from an already built release binary:
#   tools/package-linux.sh [BINARY] [OUTPUT_DIR]
# Defaults: target/release/audeze-overlay and dist/. This is a CI/test artifact;
# releases are still assembled by tools/package.ps1 and the Windows workflow.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
binary=${1:-$root/target/release/audeze-overlay}
out=${2:-$root/dist}
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n 1)
name=Audeze-Overlay-Linux-x86_64
stage=$out/$name

[ -x "$binary" ] || { echo "Build the release binary first: $binary" >&2; exit 1; }

rm -rf "$stage" "$out/$name.tar.gz"
mkdir -p "$stage/icons" "$stage/docs"
install -m755 "$binary" "$stage/audeze-overlay"
strip "$stage/audeze-overlay" 2>/dev/null || true
install -m755 "$root/packaging/linux/install.sh" "$stage/install.sh"
install -m644 "$root/packaging/linux/io.github.temdah.AudezeOverlay.desktop" \
    "$root/packaging/linux/70-audeze-overlay.rules" "$stage/"
cp -r "$root/packaging/linux/icons/." "$stage/icons/"
install -m644 "$root/README.md" "$root/LICENSE" "$root/THIRD-PARTY-NOTICES.md" "$stage/"
install -m644 "$root/docs/LINUX.md" "$stage/docs/"
python3 -I "$root/tools/dependency-notices.py" "$stage/DEPENDENCY-LICENSES.txt"
echo "Audeze Overlay $version for Linux x86_64. Run ./install.sh; see docs/LINUX.md." > "$stage/VERSION.txt"

tar -C "$out" --owner=0 --group=0 --sort=name -czf "$out/$name.tar.gz" "$name"
(cd "$out" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
rm -rf "$stage"
echo "Wrote $out/$name.tar.gz"
