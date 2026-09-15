#!/usr/bin/env bash
#
# check-versions.sh — guards against the two version-drift bugs documented in
# ../CLAUDE.md ("App capability matrix" / dev-build workflow):
#
#   1. A pre-release version (e.g. 0.1.0-dev1) must NEVER appear in a metainfo
#      <release> entry. AppStream's version comparison has no concept of
#      pre-release ordering, so it reads "-dev1" as *higher* than the clean
#      "0.1.0" and tools like `flatpak info` then show the wrong Version.
#   2. On a clean release (version has no "-" suffix), the app version must have
#      a matching metainfo <release> entry — catches "tagged a release but forgot
#      to add the metainfo entry."
#
# Pereplyot is a Cargo workspace with no [package] version at the root (like
# Kartoteka/Sputnik) — the sole source of truth is pereplyot-ui-gtk/Cargo.toml
# (no separate CLI crate to keep in sync, unlike Kartoteka's variant of this
# script), so this reads that file directly rather than the workspace root.
#
# Runs on every push/PR via .github/workflows/ci.yml. Also safe to run locally.

set -euo pipefail

# --- locate the metainfo file (ignore build artifacts) ---
METAINFO=$(find . -name '*.metainfo.xml' \
  -not -path '*/.flatpak-builder/*' \
  -not -path '*/build-flatpak/*' \
  -not -path '*/build/*' 2>/dev/null | head -1)
[ -n "$METAINFO" ] || { echo "ERROR: no *.metainfo.xml found"; exit 1; }

APP_TOML="pereplyot-ui-gtk/Cargo.toml"
[ -f "$APP_TOML" ] || { echo "ERROR: $APP_TOML not found"; exit 1; }
APP_VERSION=$(grep -m1 '^version[[:space:]]*=[[:space:]]*"' "$APP_TOML" | sed -E 's/.*"([^"]+)".*/\1/' || true)
[ -n "$APP_VERSION" ] || { echo "ERROR: could not determine app version from $APP_TOML"; exit 1; }

echo "App version : $APP_VERSION ($APP_TOML)"
echo "Metainfo    : $METAINFO"
echo

fail=0

# --- check 1: no *stable* pre-release <release> entry ---
bad=$(grep -oE '<release[^>]*>' "$METAINFO" \
  | grep -E 'version="[^"]*-(dev|rc|alpha|beta|pre)' \
  | grep -v 'type="development"' || true)
if [ -n "$bad" ]; then
  echo "ERROR: metainfo has pre-release <release> entries that are NOT type=\"development\""
  echo "       (AppStream sorts these above the real release — wrong 'Version' in flatpak info):"
  echo "$bad" | sed 's/^/  /'
  fail=1
fi

# --- check 2: a clean release must have a matching metainfo <release> entry ---
case "$APP_VERSION" in
  *-*)
    echo "Dev build ($APP_VERSION) — skipping the metainfo-entry match check."
    ;;
  *)
    if grep -qE "<release[[:space:]]+version=\"$APP_VERSION\"" "$METAINFO"; then
      echo "Clean release $APP_VERSION has a matching <release> entry."
    else
      echo "ERROR: clean release $APP_VERSION has no matching <release> entry in $METAINFO"
      fail=1
    fi
    ;;
esac

echo
if [ "$fail" -eq 0 ]; then
  echo "Version consistency OK."
else
  echo "Version consistency FAILED — see errors above."
  exit 1
fi
