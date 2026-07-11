#!/bin/sh
# Hermetic test for scripts/fetch-or-build.sh. Stubs uname/curl/sha256sum/cargo via a
# temp bin on PATH and env overrides, so no network or real toolchain is touched.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
script="$here/../scripts/fetch-or-build.sh"
fail=0
check() { if [ "$2" = "$3" ]; then echo "ok - $1"; else echo "FAIL - $1: expected [$3] got [$2]"; fail=1; fi; }

# --- scaffold a throwaway workspace with stub tools -------------------------------
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin" "$work/root" "$work/serve"
printf 'version = "9.9.9"\n' > "$work/root/Cargo.toml"

# stub uname -> mac arm64
cat > "$work/bin/uname" <<'EOF'
#!/bin/sh
case "$1" in -s) echo Darwin ;; -m) echo arm64 ;; *) echo Darwin ;; esac
EOF
# stub curl: copy the local file matching the requested URL's basename from $work/serve
cat > "$work/bin/curl" <<EOF
#!/bin/sh
# args: -fsSL -o DEST URL   (or -o DEST URL)
dest=""; url=""
while [ \$# -gt 0 ]; do case "\$1" in -o) dest="\$2"; shift 2;; -*) shift;; *) url="\$1"; shift;; esac; done
name=\$(basename "\$url")
if [ -f "$work/serve/\$name" ]; then cp "$work/serve/\$name" "\$dest"; exit 0; else exit 22; fi
EOF
# stub sha256sum: deterministic digest = "HASH:" + byte count (enough to match/mismatch)
cat > "$work/bin/sha256sum" <<'EOF'
#!/bin/sh
n=$(wc -c < "$1" | tr -d ' '); echo "hash$n  $1"
EOF
# stub cargo: simulate a source build producing target/release/tally
cat > "$work/bin/cargo" <<EOF
#!/bin/sh
mkdir -p "$work/root/target/release"; printf 'BUILT' > "$work/root/target/release/tally"; exit 0
EOF
chmod +x "$work/bin/"*

export PATH="$work/bin:$PATH"
export TALLY_REPO_ROOT="$work/root"
export TALLY_CARGO_TOML="$work/root/Cargo.toml"
export TALLY_OUT="$work/root/bin/tally"
export TALLY_BASE_URL="file:///$work/serve"   # basename is all the curl stub uses

# ===== Case A: matching prebuilt + correct checksum -> installs prebuilt =========
printf 'PREBUILT-BINARY' > "$work/serve/tally-aarch64-apple-darwin"
# checksum stub yields hash<bytes>; asset is 15 bytes -> "hash15"
printf 'hash15  tally-aarch64-apple-darwin\n' > "$work/serve/SHA256SUMS"
rm -f "$TALLY_OUT"
sh "$script" >/dev/null 2>&1 || true
got=$(cat "$TALLY_OUT" 2>/dev/null || echo MISSING)
check "prebuilt installed on checksum match" "$got" "PREBUILT-BINARY"

# ===== Case B: checksum mismatch -> cargo source fallback =======================
printf 'wronghash  tally-aarch64-apple-darwin\n' > "$work/serve/SHA256SUMS"
rm -f "$TALLY_OUT"
sh "$script" >/dev/null 2>&1 || true
got=$(cat "$TALLY_OUT" 2>/dev/null || echo MISSING)
check "cargo fallback on checksum mismatch" "$got" "BUILT"

# ===== Case C: no prebuilt asset (curl 404) -> cargo source fallback ============
rm -f "$work/serve/tally-aarch64-apple-darwin" "$TALLY_OUT"
sh "$script" >/dev/null 2>&1 || true
got=$(cat "$TALLY_OUT" 2>/dev/null || echo MISSING)
check "cargo fallback when no asset" "$got" "BUILT"

exit $fail
