#!/usr/bin/env bash
# publish-flatpak.sh — push a release; GitHub Actions builds and publishes it
#
# Usage:
#   ./publish-flatpak.sh 0.1.0
#
# What this script does NOT do (Claude's job, done before running this):
#   - Write the CHANGELOG entry / metainfo release note
#   - Bump the version / commit / tag
#
# What this script DOES do:
#   1. Verify the version you pass matches pereplyot-ui-gtk/Cargo.toml (sanity check)
#   2. Push main and the version tag to GitHub
#
# Pushing the tag triggers .github/workflows/release-flatpak.yml, which builds the
# flatpak, exports it into the public repo, GPG-signs it, and pushes. Watch it at:
#   https://github.com/calstfrancis/pereplyot/actions/workflows/release-flatpak.yml
#
# Needs CI to have already passed for this commit — release-flatpak.yml checks this
# itself and refuses to publish otherwise. If GitHub Actions is down or you need to debug
# the build locally, use publish-flatpak-local.sh instead.

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <version>   e.g.  $0 0.1.0"
  exit 1
fi
VERSION="$1"

CARGO_VERSION=$(grep '^version' pereplyot-ui-gtk/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
if [[ "$CARGO_VERSION" != "$VERSION" ]]; then
  echo "ERROR: pereplyot-ui-gtk/Cargo.toml says '$CARGO_VERSION', but you passed '$VERSION'."
  echo "Did you forget the version bump? (Ask Claude to do the version bump + docs first.)"
  exit 1
fi

echo "==> Publishing Pereplyot $VERSION"
git push origin main
git push origin "v$VERSION"

echo ""
echo "Done! GitHub Actions is building and publishing $VERSION now:"
echo "  https://github.com/calstfrancis/pereplyot/actions/workflows/release-flatpak.yml"
