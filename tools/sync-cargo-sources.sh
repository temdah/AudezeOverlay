#!/bin/sh
# Generate offline Flatpak sources using only Python 3.11+ and Cargo.lock.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
exec python3 -I "$root/tools/sync-cargo-sources.py" "$@"