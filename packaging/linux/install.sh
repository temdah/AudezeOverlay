#!/bin/sh
# Installs Audeze Overlay for the current user (no root needed), or removes it.
#
#   ./install.sh [--prefix DIR] [--binary FILE]   install (default prefix ~/.local)
#   ./install.sh --udev                           also install the udev rule (uses sudo)
#   ./install.sh --uninstall [--prefix DIR]       remove what this script installed
#
# It looks for the program next to this script (a release archive) and then in
# ../../target/release (a source checkout). The udev rule is what lets your user
# open the headset's hidraw node; see docs/LINUX.md.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
id=io.github.temdah.AudezeOverlay
prefix=${HOME}/.local
binary=
udev=no
uninstall=no

while [ $# -gt 0 ]; do
    case $1 in
        --prefix) prefix=$2; shift ;;
        --binary) binary=$2; shift ;;
        --udev) udev=yes ;;
        --uninstall) uninstall=yes ;;
        -h|--help) sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "Unknown option: $1" >&2; exit 2 ;;
    esac
    shift
done

# Desktop entries cannot represent embedded newlines. Require an absolute prefix
# so launching from an unrelated working directory always resolves the same app.
case "$prefix" in
    /*) ;;
    *) echo "--prefix must be an absolute path." >&2; exit 2 ;;
esac
case "$prefix" in
    *'
'*) echo "--prefix cannot contain a newline." >&2; exit 2 ;;
esac

bin_dir=$prefix/bin
share=$prefix/share
autostart=${XDG_CONFIG_HOME:-$HOME/.config}/autostart/$id.desktop

refresh_caches() {
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$share/applications" 2>/dev/null || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 && [ -f "$share/icons/hicolor/index.theme" ] \
        && gtk-update-icon-cache -q "$share/icons/hicolor" 2>/dev/null || true
}

if [ "$uninstall" = yes ]; then
    rm -f "$bin_dir/audeze-overlay" "$share/applications/$id.desktop" "$autostart"
    for size in 48x48 128x128 256x256; do
        rm -f "$share/icons/hicolor/$size/apps/$id.png"
    done
    refresh_caches
    echo "Removed Audeze Overlay from $prefix."
    echo "The udev rule, if installed, is /etc/udev/rules.d/70-audeze-overlay.rules."
    exit 0
fi

if [ -z "$binary" ]; then
    for candidate in "$here/audeze-overlay" "$here/../../target/release/audeze-overlay"; do
        if [ -x "$candidate" ]; then binary=$candidate; break; fi
    done
fi
if [ -z "$binary" ] || [ ! -x "$binary" ]; then
    echo "audeze-overlay was not found. Build it first (cargo build --release) or pass --binary FILE." >&2
    exit 1
fi

install -Dm755 "$binary" "$bin_dir/audeze-overlay"
# The entry names the installed program by its full path so it works even when
# ~/.local/bin is not on the desktop's PATH.
# (Written next to the destination: the source tree may be read-only.)
mkdir -p "$share/applications"
OVERLAY_EXEC_PATH="$bin_dir/audeze-overlay" awk '
    BEGIN {
        path = ENVIRON["OVERLAY_EXEC_PATH"]
        escaped = ""
        for (i = 1; i <= length(path); i++) {
            c = substr(path, i, 1)
            if (c == "\\") escaped = escaped "\\\\\\\\"
            else if (c == "\"" || c == "$" || c == "`") escaped = escaped "\\\\" c
            else if (c == "%") escaped = escaped "%%"
            else escaped = escaped c
        }
    }
    /^Exec=/ { print "Exec=\"" escaped "\""; next }
    { print }
' "$here/$id.desktop" > "$share/applications/$id.desktop"
chmod 644 "$share/applications/$id.desktop"
for size in 48x48 128x128 256x256; do
    install -Dm644 "$here/icons/$size/apps/$id.png" "$share/icons/hicolor/$size/apps/$id.png"
done
refresh_caches
echo "Installed to $bin_dir/audeze-overlay."

if [ "$udev" = yes ]; then
    sudo install -m 0644 "$here/70-audeze-overlay.rules" /etc/udev/rules.d/
    sudo udevadm control --reload
    sudo udevadm trigger --subsystem-match=hidraw --action=change
    echo "Installed the udev rule. Reconnect the dongle if it was plugged in."
elif [ ! -f /etc/udev/rules.d/70-audeze-overlay.rules ]; then
    echo "Note: your user needs access to the headset. Run again with --udev, or see docs/LINUX.md."
fi
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo "Note: $bin_dir is not on your PATH; start it from the application menu." ;;
esac
