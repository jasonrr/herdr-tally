#!/bin/sh
# fetch-or-build.sh — binary half of tally's herdr [[build]] step.
#
# Fast path: download the prebuilt binary matching THIS source's declared version +
# platform from the GitHub release, verify its SHA-256, and install it at bin/tally.
# Match is by VERSION (Cargo.toml), not commit: a checkout ahead of the released tag
# still uses the released, verified binary. Fallback on ANY miss (no asset, network/
# checksum error, unmapped platform, no curl/wget): build from source with cargo.
# Paths/URL are env-overridable (TALLY_*) so the logic is exercised by a hermetic test.
set -u

repo="jasonrr/herdr-tally"

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root="${TALLY_REPO_ROOT:-$(CDPATH= cd -- "$script_dir/.." && pwd)}"
cargo_toml="${TALLY_CARGO_TOML:-$repo_root/Cargo.toml}"
out="${TALLY_OUT:-$repo_root/bin/tally}"
base_url="${TALLY_BASE_URL:-https://github.com/$repo/releases/download}"

have() { command -v "$1" >/dev/null 2>&1; }

# Source build — the original behavior. Source ~/.cargo/env so cargo is found even when
# herdr launched us without ~/.cargo/bin on PATH. `rm -f` before cp gives bin/tally a
# fresh inode (overwriting a signed Mach-O in place SIGKILLs it at exec on macOS).
build_from_source() {
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  if ! have cargo; then
    echo "tally needs Rust to build, but cargo was not found. Install from https://rustup.rs then re-run: herdr plugin install $repo" >&2
    exit 1
  fi
  ( cd "$repo_root" && cargo build --release ) || exit 1
  mkdir -p "$(dirname "$out")"
  rm -f "$out"
  cp "$repo_root/target/release/tally" "$out"
  chmod +x "$out"
  echo "tally: built from source -> $out"
  exit 0
}

fallback() {
  echo "tally: $1 — building from source instead." >&2
  [ -n "${tmpdir:-}" ] && rm -rf "$tmpdir"
  build_from_source
}

download() { # download <url> <dest>
  if have curl; then curl -fsSL -o "$2" "$1"
  elif have wget; then wget -q -O "$2" "$1"
  else return 127; fi
}

sha256_of() {
  if have sha256sum; then sha256sum "$1" | awk '{print $1}'
  elif have shasum; then shasum -a 256 "$1" | awk '{print $1}'
  else return 127; fi
}

# --- resolve target triple ------------------------------------------------------
os=$(uname -s 2>/dev/null || echo unknown)
arch=$(uname -m 2>/dev/null || echo unknown)
triple=""
case "$os" in
  Darwin) case "$arch" in
    arm64|aarch64) triple="aarch64-apple-darwin" ;;
    x86_64|amd64)  triple="x86_64-apple-darwin" ;;
  esac ;;
  Linux) case "$arch" in
    x86_64|amd64) triple="x86_64-unknown-linux-musl" ;;
  esac ;;
esac
[ -n "$triple" ] || fallback "no prebuilt binary for $os/$arch"

# --- declared version -----------------------------------------------------------
version=$(grep -E '^version *= *"' "$cargo_toml" 2>/dev/null | head -n 1 | sed -E 's/^version *= *"([^"]+)".*/\1/')
[ -n "$version" ] || fallback "could not read version from $cargo_toml"

asset="tally-$triple"
tmpdir=$(mktemp -d 2>/dev/null) || fallback "could not create a temp dir"
trap 'rm -rf "$tmpdir"' EXIT

bin_url="$base_url/v$version/$asset"
sums_url="$base_url/v$version/SHA256SUMS"
tmpbin="$tmpdir/$asset"
tmpsums="$tmpdir/SHA256SUMS"

download "$bin_url" "$tmpbin"   || fallback "prebuilt binary not available for v$version ($asset)"
download "$sums_url" "$tmpsums" || fallback "checksums not available for v$version"

# Expected hash = the SHA256SUMS line for our asset (accept two-space or ' *' separator).
expected=$(grep -E "^[0-9a-z]+ [ *]$asset\$" "$tmpsums" 2>/dev/null | awk '{print $1}' | head -n 1)
[ -n "$expected" ] || fallback "no checksum listed for $asset"

actual=$(sha256_of "$tmpbin") || fallback "no sha-256 tool available"
[ "$actual" = "$expected" ] || fallback "checksum mismatch for $asset (expected $expected, got $actual)"

# Verified — install with a fresh inode.
chmod +x "$tmpbin"
mkdir -p "$(dirname "$out")"
rm -f "$out"
mv -f "$tmpbin" "$out" || fallback "could not install the verified binary to $out"
echo "tally: installed prebuilt v$version ($triple), verified SHA-256 -> $out"
exit 0
