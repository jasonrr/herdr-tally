#!/usr/bin/env bash
# Bump tally's release version. Default: patch. Usage: scripts/release.sh [patch|minor|major]
set -euo pipefail
cd "$(dirname "$0")/.."

bump="${1:-patch}"
case "$bump" in
  patch|minor|major) ;;
  -h|--help) echo "usage: scripts/release.sh [patch|minor|major]"; exit 0 ;;
  *) echo "usage: scripts/release.sh [patch|minor|major]" >&2; exit 2 ;;
esac

version=$(grep -E '^version *= *"' Cargo.toml | head -n 1 | sed -E 's/^version *= *"([0-9]+)\.([0-9]+)\.([0-9]+)".*/\1 \2 \3/')
set -- $version
[ "$#" -eq 3 ] || { echo "could not parse Cargo.toml version" >&2; exit 1; }
major=$1 minor=$2 patch=$3

case "$bump" in
  major) major=$((major + 1)); minor=0; patch=0 ;;
  minor) minor=$((minor + 1)); patch=0 ;;
  patch) patch=$((patch + 1)) ;;
esac
next="$major.$minor.$patch"

manifest=$(grep -E '^version *= *"' herdr-plugin.toml | head -n 1 | sed -E 's/^version *= *"([^"]+)".*/\1/')
current=$(grep -E '^version *= *"' Cargo.toml | head -n 1 | sed -E 's/^version *= *"([^"]+)".*/\1/')
[ "$manifest" = "$current" ] || { echo "Cargo.toml ($current) != herdr-plugin.toml ($manifest)" >&2; exit 1; }

NEXT=$next perl -0pi -e 's/(\[package\]\nname = "tally"\nversion = ")[^"]+/${1}$ENV{NEXT}/' Cargo.toml
NEXT=$next perl -0pi -e 's/(^version = ")[^"]+/${1}$ENV{NEXT}/m' herdr-plugin.toml
NEXT=$next perl -0pi -e 's/(name = "tally"\nversion = ")[^"]+/${1}$ENV{NEXT}/' Cargo.lock

cargo build --release
mkdir -p bin
rm -f bin/tally
/bin/cp target/release/tally bin/tally
pkill -f 'bin/tally tui' 2>/dev/null || true

echo "bumped tally $current -> $next and rebuilt bin/tally"
if pgrep -fl 'bin/tally mcp' >/tmp/tally-release-mcp.$$; then
  echo "reconnect MCP servers still running old code:"
  cat /tmp/tally-release-mcp.$$
fi
rm -f /tmp/tally-release-mcp.$$
# Annotated tag (-a) so `git push --follow-tags` actually pushes it; lightweight
# tags are skipped by --follow-tags and get left behind.
echo "next: git diff && git commit -am 'Release v$next' && git tag -a v$next -m 'Release v$next' && git push --follow-tags"
