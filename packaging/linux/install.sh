#!/bin/sh
# Adds splice9000 to this account's app menu, with its icon. Nothing
# outside your home folder is touched.
#
#   ./install.sh            add it (again, after moving this folder)
#   ./install.sh --remove   take it out of the menu
set -eu

here=$(cd "$(dirname "$0")" && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
apps="$data/applications"
icons="$data/icons/hicolor/256x256/apps"

refresh() {
  command -v update-desktop-database >/dev/null && update-desktop-database -q "$apps" || true
  command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
}

if [ "${1:-}" = "--remove" ]; then
  rm -f "$apps/splice9000.desktop" "$icons/splice9000.png"
  refresh
  echo "splice9000 is out of the app menu. (This folder is untouched; delete it to remove the app.)"
  exit 0
fi

[ -x "$here/splice9000" ] || { echo "splice9000 isn't next to install.sh" >&2; exit 1; }
mkdir -p "$apps" "$icons"
cp "$here/splice9000.png" "$icons/splice9000.png"
# The path goes in quotes, escaped as a desktop file wants: \ " ` $ take
# a backslash (written twice, since the file's own strings unescape one),
# and % is doubled.
exec_path=$(printf '%s' "$here/splice9000" | sed -e 's/[\\"`$]/\\\\&/g' -e 's/%/%%/g')
SPLICE_EXEC="Exec=\"$exec_path\"" awk '/^Exec=/ { print ENVIRON["SPLICE_EXEC"]; next } { print }' \
  "$here/splice9000.desktop" > "$apps/splice9000.desktop"
chmod 644 "$apps/splice9000.desktop"
refresh
echo "splice9000 is in the app menu. Run ./install.sh again if you move this folder."
