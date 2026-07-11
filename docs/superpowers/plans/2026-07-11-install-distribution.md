# tally install / distribution — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `herdr plugin install jasonrr/herdr-tally` a zero-manual-step install on macOS/Linux — prebuilt binary, MCP server registered, agent skill installed — with a `cargo build` fallback when no matching release exists.

**Architecture:** herdr runs the manifest `[[build]]` step on every install. Point it at `scripts/install.sh`, which (1) calls `scripts/fetch-or-build.sh` to put a verified prebuilt (or source-built) binary at `bin/tally`, then (2) best-effort registers the MCP server via `claude mcp add` and (3) copies `SKILL.md` into `~/.claude/skills/tally/`. A tag-triggered GitHub Actions release publishes the per-platform prebuilt binaries the fetch step consumes.

**Tech Stack:** POSIX `sh` scripts, GitHub Actions, Rust/cargo (fallback + release build), herdr 0.7.3, `claude` CLI.

**Reference implementation:** `herdr-file-viewer` v1.8.0 at `~/.config/herdr/plugins/github/herdr-file-viewer-c993314e2614/` ships the proven `fetch-or-build.sh` + `release.yml`. Tasks 1 and 4 adapt it (rename → tally, out path → `bin/tally`, repo → `jasonrr/herdr-tally`, drop Windows).

## Global Constraints

- Repo: `jasonrr/herdr-tally`. Binary name: `tally`. MCP server invocation: `tally mcp`.
- Binary MUST land at `bin/tally` (panes run `$HERDR_PLUGIN_ROOT/bin/tally`), NOT `target/release`.
- Platforms v1: macOS + Linux only. **No Windows** (deferred).
- Release asset naming: `tally-<triple>` for triples `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`.
- macOS fresh-inode rule: overwriting the signed `bin/tally` in place → `Killed: 9` at exec. Always `rm -f bin/tally && cp …` (or `mv -f` onto a non-existent target). Never `cp` over an existing `bin/tally`.
- MCP registration scope: `--scope user`. `claude mcp add` is NOT idempotent → always `remove || true` then `add`.
- MCP + skill wiring is **best-effort**: any failure prints a manual-fix message and the script still `exit 0`. Only a binary-install failure aborts (`exit 1`).
- Store key format is frozen; none of this work touches `src/`. Shell tests go in `tests/` (cargo ignores non-`.rs` files there).
- POSIX `sh` only in `install.sh` / `fetch-or-build.sh` (they run under `/bin/sh`); no bashisms.

---

## File Structure

- **Create** `scripts/fetch-or-build.sh` — binary half of the build step: download+verify prebuilt or `cargo build` fallback → `bin/tally`.
- **Create** `tests/fetch-or-build.test.sh` — hermetic test (stubs `uname`/`curl`/`sha256sum`/`cargo`).
- **Create** `scripts/install.sh` — `[[build]]` orchestrator: fetch-or-build, then best-effort MCP + skill wiring.
- **Create** `tests/install.test.sh` — hermetic test (stub `claude`, temp `HOME`, stub fetch-or-build).
- **Create** `.github/workflows/release.yml` — tag-triggered per-platform release with version guard.
- **Modify** `herdr-plugin.toml` — `platforms`, `[[build]]` → `install.sh`, widen pane PATHs.
- **Modify** `scripts/toggle-pane.sh` — widen PATH prepend.
- **Modify** `README.md` — install section + manual-fallback commands.

`scripts/verify.sh` (dev-only live check, macOS) and `scripts/on-worktree.sh` (no-op) are intentionally left unchanged — surgical scope.

---

## Task 1: `fetch-or-build.sh` — verified prebuilt with cargo fallback

**Files:**
- Create: `scripts/fetch-or-build.sh`
- Test: `tests/fetch-or-build.test.sh`

**Interfaces:**
- Consumes: nothing (entry script).
- Produces: an executable at `$TALLY_OUT` (default `<repo_root>/bin/tally`). Honors env overrides `TALLY_REPO_ROOT`, `TALLY_CARGO_TOML`, `TALLY_OUT`, `TALLY_BASE_URL`. Exit 0 on success, non-zero only if the source fallback itself fails (e.g. no cargo).

- [ ] **Step 1: Write the failing test**

Create `tests/fetch-or-build.test.sh`:

```sh
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `sh tests/fetch-or-build.test.sh`
Expected: FAIL — `scripts/fetch-or-build.sh` does not exist yet (`sh: .../fetch-or-build.sh: No such file`), all three checks report `MISSING`.

- [ ] **Step 3: Write `scripts/fetch-or-build.sh`**

```sh
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
expected=$(grep -E "^[0-9a-f]+ [ *]$asset\$" "$tmpsums" 2>/dev/null | awk '{print $1}' | head -n 1)
[ -n "$expected" ] || fallback "no checksum listed for $asset"

actual=$(sha256_of "$tmpbin") || fallback "no sha-256 tool available"
[ "$actual" = "$expected" ] || fallback "checksum mismatch for $asset (expected $expected, got $actual)"

# Verified — install with a fresh inode.
chmod +x "$tmpbin"
mkdir -p "$(dirname "$out")"
mv -f "$tmpbin" "$out" || fallback "could not install the verified binary to $out"
echo "tally: installed prebuilt v$version ($triple), verified SHA-256 -> $out"
exit 0
```

Then `chmod +x scripts/fetch-or-build.sh`.

Note on the test's checksum stub: the stub emits `hash<bytecount>` and the script's `expected` regex is `^[0-9a-f]+ [ *]$asset$` — `hash15` matches `[0-9a-f]+`? No: `h`,`s` are not hex. The regex must accept the stub. Use `^[0-9a-z]+ [ *]$asset$` in the SHA256SUMS grep so the hermetic stub digest matches while real 64-hex digests still match. Apply that widened character class in the script above.

- [ ] **Step 4: Adjust the hash regex, then run the test to verify it passes**

Ensure the script's expected-hash line reads:
```sh
expected=$(grep -E "^[0-9a-z]+ [ *]$asset\$" "$tmpsums" 2>/dev/null | awk '{print $1}' | head -n 1)
```
Run: `chmod +x scripts/fetch-or-build.sh && sh tests/fetch-or-build.test.sh`
Expected: PASS — three `ok -` lines, exit 0.

- [ ] **Step 5: Verify the real source-build path end-to-end (local sanity)**

Run: `rm -f bin/tally && sh scripts/fetch-or-build.sh; ls -l bin/tally`
Expected: prints either `installed prebuilt …` (if a release exists) or `building from source…` then `built from source -> …/bin/tally`; `bin/tally` exists and is executable.

- [ ] **Step 6: Commit**

```bash
git add scripts/fetch-or-build.sh tests/fetch-or-build.test.sh
git commit -m "feat(install): fetch-or-build.sh — verified prebuilt binary with cargo fallback"
```

---

## Task 2: `install.sh` — build-step orchestrator with best-effort MCP + skill wiring

**Files:**
- Create: `scripts/install.sh`
- Test: `tests/install.test.sh`

**Interfaces:**
- Consumes: `scripts/fetch-or-build.sh` from Task 1 (invoked via `$TALLY_FETCH_OR_BUILD`, default `<script_dir>/fetch-or-build.sh`).
- Produces: the `[[build]]` entry point. On success: `bin/tally` present, MCP server registered (best-effort), `~/.claude/skills/tally/SKILL.md` present (best-effort). Exit 1 only if the binary step fails; otherwise exit 0.

- [ ] **Step 1: Write the failing test**

Create `tests/install.test.sh`:

```sh
#!/bin/sh
# Hermetic test for scripts/install.sh. Stubs the binary step (TALLY_FETCH_OR_BUILD),
# a fake `claude` on PATH, and a temp HOME, then asserts MCP registration + skill copy
# happen and that a missing `claude` still exits 0 with a manual-fix message.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
script="$here/../scripts/install.sh"
plugin_root=$(CDPATH= cd -- "$here/.." && pwd)
fail=0
check() { if [ "$2" = "$3" ]; then echo "ok - $1"; else echo "FAIL - $1: expected [$3] got [$2]"; fail=1; fi; }

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin" "$work/home"

# stub fetch-or-build: just create bin/tally under the plugin root's OUT
cat > "$work/fob.sh" <<EOF
#!/bin/sh
mkdir -p "$work/pluginbin"; printf 'BIN' > "$work/pluginbin/tally"; exit 0
EOF
# stub claude: log its argv so we can assert on it
cat > "$work/bin/claude" <<EOF
#!/bin/sh
echo "\$@" >> "$work/claude.log"; exit 0
EOF
chmod +x "$work/fob.sh" "$work/bin/claude"

export HOME="$work/home"
export TALLY_FETCH_OR_BUILD="$work/fob.sh"
export HERDR_PLUGIN_ROOT="$work"          # so bin path = $work/bin/tally ... see note
# Point the binary the MCP command references at our stub location:
export TALLY_BIN="$work/pluginbin/tally"

# ===== Case A: claude present -> registers MCP + installs skill =================
PATH="$work/bin:$PATH" sh "$script" >/dev/null 2>&1 || true
addline=$(grep -c "mcp add --scope user tally -- $work/pluginbin/tally mcp" "$work/claude.log" 2>/dev/null || echo 0)
check "mcp add invoked with user scope + bin path" "$addline" "1"
rmline=$(grep -c "mcp remove --scope user tally" "$work/claude.log" 2>/dev/null || echo 0)
check "mcp remove invoked before add" "$rmline" "1"
skill=$( [ -f "$HOME/.claude/skills/tally/SKILL.md" ] && echo yes || echo no )
check "skill copied into ~/.claude/skills/tally" "$skill" "yes"

# ===== Case B: claude absent -> still exit 0 ====================================
rm -f "$work/claude.log"
PATH="/usr/bin:/bin" HOME="$work/home" sh "$script" >/dev/null 2>&1; rc=$?
check "exit 0 when claude missing" "$rc" "0"

exit $fail
```

Note: the test injects `TALLY_BIN` to pin the MCP-referenced binary path deterministically; `install.sh` must honor `TALLY_BIN` (default: `${HERDR_PLUGIN_ROOT:-<repo_root>}/bin/tally`). The test also relies on `SKILL.md` existing at the plugin root — it does (repo root). Set `TALLY_SKILL="$plugin_root/SKILL.md"` in the test env if you prefer to decouple; the default below reads `${HERDR_PLUGIN_ROOT:-<repo_root>}/SKILL.md`, and since `HERDR_PLUGIN_ROOT=$work` here, add `export TALLY_SKILL="$plugin_root/SKILL.md"` to the test env block.

- [ ] **Step 2: Run the test to verify it fails**

Run: `sh tests/install.test.sh`
Expected: FAIL — `scripts/install.sh` does not exist; checks report `0` / `no`.

- [ ] **Step 3: Write `scripts/install.sh`**

```sh
#!/bin/sh
# install.sh — tally's herdr [[build]] step. Runs on every `herdr plugin install`
# and re-link. Three phases:
#   1. fetch-or-build the binary (CRITICAL — aborts the install on failure).
#   2. register the tally MCP server with Claude Code (best-effort).
#   3. install the tally agent skill (best-effort).
# Best-effort = a failure prints a manual-fix command and we still exit 0, so the
# binary + panes always install even when `claude`/$HOME wiring can't complete.
set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
plugin_root="${HERDR_PLUGIN_ROOT:-$repo_root}"
bin="${TALLY_BIN:-$plugin_root/bin/tally}"
skill_src="${TALLY_SKILL:-$plugin_root/SKILL.md}"

# --- 1. binary (critical path) --------------------------------------------------
fob="${TALLY_FETCH_OR_BUILD:-$script_dir/fetch-or-build.sh}"
if ! sh "$fob"; then
  echo "tally: binary install failed — aborting." >&2
  exit 1
fi

# --- 2. MCP server registration (best-effort) -----------------------------------
find_claude() {
  for c in "$HOME/.local/bin/claude" /opt/homebrew/bin/claude /usr/local/bin/claude; do
    [ -x "$c" ] && { printf '%s\n' "$c"; return 0; }
  done
  command -v claude 2>/dev/null && return 0
  return 1
}
manual_mcp="claude mcp add --scope user tally -- \"$bin\" mcp"
if claude_bin=$(find_claude); then
  "$claude_bin" mcp remove --scope user tally >/dev/null 2>&1 || true
  if "$claude_bin" mcp add --scope user tally -- "$bin" mcp >/dev/null 2>&1; then
    echo "tally: registered MCP server (user scope) -> $bin mcp"
  else
    echo "tally: could not register MCP server automatically. Run:" >&2
    echo "  $manual_mcp" >&2
  fi
else
  echo "tally: 'claude' CLI not found on PATH. Register the MCP server with:" >&2
  echo "  $manual_mcp" >&2
fi

# --- 3. agent skill (best-effort) -----------------------------------------------
skill_dir="$HOME/.claude/skills/tally"
if mkdir -p "$skill_dir" 2>/dev/null && cp "$skill_src" "$skill_dir/SKILL.md" 2>/dev/null; then
  echo "tally: installed agent skill -> $skill_dir/SKILL.md"
else
  echo "tally: could not install the agent skill. Copy it with:" >&2
  echo "  mkdir -p \"$skill_dir\" && cp \"$skill_src\" \"$skill_dir/SKILL.md\"" >&2
fi

exit 0
```

Then `chmod +x scripts/install.sh`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `chmod +x scripts/install.sh && sh tests/install.test.sh`
Expected: PASS — four `ok -` lines, exit 0.

- [ ] **Step 5: Commit**

```bash
git add scripts/install.sh tests/install.test.sh
git commit -m "feat(install): install.sh build-step orchestrator with best-effort MCP + skill wiring"
```

---

## Task 3: Wire the manifest to `install.sh` + portability edits

**Files:**
- Modify: `herdr-plugin.toml`
- Modify: `scripts/toggle-pane.sh`

**Interfaces:**
- Consumes: `scripts/install.sh` (Task 2).
- Produces: an installable manifest whose `[[build]]` runs `install.sh` on macOS + Linux.

- [ ] **Step 1: Update the manifest platforms line**

In `herdr-plugin.toml`, change:
```toml
platforms = ["macos"]
```
to:
```toml
platforms = ["macos", "linux"]
```

- [ ] **Step 2: Replace the `[[build]]` step**

Replace the entire existing `[[build]]` block (the `rm -f`/`cp` inline cargo command and its comment) with:
```toml
[[build]]
platforms = ["linux", "macos"]
# Runs on every `herdr plugin install` / re-link: fetch-or-build the binary into
# bin/tally, then best-effort register the MCP server and install the agent skill.
command = ["/bin/sh", "scripts/install.sh"]
```

- [ ] **Step 3: Widen the pane-command PATH prepends**

In `herdr-plugin.toml`, both `[[panes]]` entries contain:
```
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"
```
Change both to:
```
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:$PATH"
```

- [ ] **Step 4: Widen the PATH prepend in `scripts/toggle-pane.sh`**

Change:
```sh
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"
```
to:
```sh
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:$PATH"
```

- [ ] **Step 5: Verify the binary still builds and the manifest re-links cleanly (macOS, inside herdr)**

Run:
```bash
rm -f bin/tally && sh scripts/install.sh
herdr plugin link "$PWD" || { herdr plugin unlink tally && herdr plugin link "$PWD"; }
claude mcp get tally
```
Expected: `install.sh` prints the binary + MCP + skill lines; `herdr plugin link` succeeds; `claude mcp get tally` shows the server pointing at `…/bin/tally mcp`.

- [ ] **Step 6: Commit**

```bash
git add herdr-plugin.toml scripts/toggle-pane.sh
git commit -m "feat(install): point [[build]] at install.sh; add linux platform + portable PATHs"
```

---

## Task 4: `release.yml` — tag-triggered per-platform release

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: `Cargo.toml` + `herdr-plugin.toml` versions (must equal the tag).
- Produces: a GitHub Release `v<version>` with assets `tally-<triple>` (×3), `SHA256SUMS`, `COMMIT` — the inputs `fetch-or-build.sh` (Task 1) downloads.

- [ ] **Step 1: Create the workflow**

```yaml
name: Release

on:
  push:
    tags: ["v*"]

permissions:
  contents: read

jobs:
  build:
    name: build ${{ matrix.triple }}
    strategy:
      fail-fast: true
      matrix:
        include:
          - os: macos-latest
            triple: aarch64-apple-darwin
          - os: macos-latest
            triple: x86_64-apple-darwin
          - os: ubuntu-latest
            triple: x86_64-unknown-linux-musl
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4

      - name: Verify tag matches Cargo.toml and herdr-plugin.toml versions
        shell: bash
        run: |
          tag="${GITHUB_REF_NAME#v}"
          crate="$(grep -E '^version *= *"' Cargo.toml | head -n1 | sed -E 's/^version *= *"([^"]+)".*/\1/')"
          manifest="$(grep -E '^version *= *"' herdr-plugin.toml | head -n1 | sed -E 's/^version *= *"([^"]+)".*/\1/')"
          if [ "$tag" != "$crate" ]; then
            echo "tag $GITHUB_REF_NAME ($tag) != Cargo.toml version ($crate)" >&2; exit 1
          fi
          if [ "$tag" != "$manifest" ]; then
            echo "tag $GITHUB_REF_NAME ($tag) != herdr-plugin.toml version ($manifest)" >&2; exit 1
          fi

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.triple }}

      - name: Install musl tools
        if: matrix.triple == 'x86_64-unknown-linux-musl'
        run: sudo apt-get update && sudo apt-get install -y musl-tools

      - name: Build
        run: cargo build --release --target ${{ matrix.triple }}

      - name: Stage asset + checksum
        shell: bash
        run: |
          mkdir -p dist
          triple="${{ matrix.triple }}"
          src="target/$triple/release/tally"
          asset="tally-$triple"
          cp "$src" "dist/$asset"
          cd dist
          if command -v sha256sum >/dev/null 2>&1; then
            sha256sum "$asset" > "$asset.sha256"
          else
            shasum -a 256 "$asset" > "$asset.sha256"
          fi

      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.triple }}
          path: dist/*

  publish:
    name: publish release
    needs: build
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: artifacts

      - name: Assemble release assets + SHA256SUMS
        shell: bash
        run: |
          mkdir -p release
          find artifacts -type f -name 'tally-*' ! -name '*.sha256' -exec cp {} release/ \;
          cat artifacts/*/*.sha256 > release/SHA256SUMS
          printf '%s\n' "$GITHUB_SHA" > release/COMMIT
          echo "Publishing:"; ls -l release; echo "--- SHA256SUMS ---"; cat release/SHA256SUMS

      - name: Create-or-update the release and upload assets
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          gh release view "$GITHUB_REF_NAME" --repo "$GITHUB_REPOSITORY" >/dev/null 2>&1 \
            || gh release create "$GITHUB_REF_NAME" --repo "$GITHUB_REPOSITORY" --title "$GITHUB_REF_NAME" --generate-notes
          gh release upload "$GITHUB_REF_NAME" release/* --clobber --repo "$GITHUB_REPOSITORY"
```

- [ ] **Step 2: Validate the version-guard logic locally**

Run (simulating a matching tag against the current `0.1.0` versions):
```bash
GITHUB_REF_NAME=v0.1.0
tag="${GITHUB_REF_NAME#v}"
crate="$(grep -E '^version *= *"' Cargo.toml | head -n1 | sed -E 's/^version *= *"([^"]+)".*/\1/')"
manifest="$(grep -E '^version *= *"' herdr-plugin.toml | head -n1 | sed -E 's/^version *= *"([^"]+)".*/\1/')"
echo "tag=$tag crate=$crate manifest=$manifest"; [ "$tag" = "$crate" ] && [ "$tag" = "$manifest" ] && echo GUARD-OK || echo GUARD-FAIL
```
Expected: `tag=0.1.0 crate=0.1.0 manifest=0.1.0` then `GUARD-OK`.

- [ ] **Step 3: Validate YAML parses**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release.yml')); print('YAML OK')"`
Expected: `YAML OK`.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci(release): tag-triggered per-platform binary release with version guard"
```

> **Out-of-band verification (not a plan step):** the true end-to-end test is one real tag push — `git tag v0.1.0 && git push origin v0.1.0` — after which `fetch-or-build.sh` on a clean machine should download the prebuilt instead of compiling. Do this only when ready to cut the first release.

---

## Task 5: README install section + manual-fallback docs

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: the install behavior from Tasks 2–4.
- Produces: user-facing install instructions and the exact manual commands the best-effort wiring prints on failure.

- [ ] **Step 1: Add an Install section to `README.md`**

Insert after the intro (before the first deep-dive section) a section with this content:

````markdown
## Install

```bash
herdr plugin install jasonrr/herdr-tally
```

This downloads a prebuilt `tally` binary for your platform (macOS arm64/x86_64,
Linux x86_64), verifies its SHA-256, and — best-effort — registers the MCP server
with Claude Code and installs the `tally` agent skill. No Rust toolchain is needed
when a release exists for your platform; otherwise install falls back to building
from source with `cargo` (install Rust from https://rustup.rs).

**Platforms:** macOS and Linux. Windows is not yet supported.

### If the automatic wiring is skipped

The binary and panes always install. If `claude` wasn't on `PATH` at install time,
finish the two best-effort steps manually (the installer prints these too):

```bash
# Register the MCP server (adjust the path to your installed plugin root):
claude mcp add --scope user tally -- "$(herdr plugin config-dir tally >/dev/null 2>&1; echo)$HOME/.config/herdr/plugins/github/herdr-tally-*/bin/tally" mcp

# Install the agent skill:
mkdir -p ~/.claude/skills/tally && cp <plugin-root>/SKILL.md ~/.claude/skills/tally/SKILL.md
```
````

Note: the plugin root is version-hashed; the installer's printed command contains the
exact absolute path — prefer copying that over the glob above. If the glob resolves to
multiple dirs (older versions present), use the newest.

- [ ] **Step 2: Verify the README renders and the install command is correct**

Run: `grep -n "herdr plugin install jasonrr/herdr-tally" README.md`
Expected: one match. Eyeball the section for correct fencing.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs(readme): document one-command install + manual wiring fallback"
```

---

## Self-Review

**Spec coverage:**
- A (release pipeline) → Task 4 (`release.yml`) + Task 1 (`fetch-or-build.sh`). ✓
- B (portability: linux platform, widened PATHs) → Task 3. ✓
- C (MCP registration, user scope, remove-then-add, robust claude probe, best-effort) → Task 2 (`install.sh`) + Task 5 (manual fallback docs). ✓
- D (skill copy into `~/.claude/skills/tally`, best-effort) → Task 2 + Task 5. ✓
- Structure (`[[build]]` → `install.sh` → `fetch-or-build.sh`) → Tasks 2 + 3. ✓
- Verification DoD #1 (release + guard) → Task 4 steps 2–3 + out-of-band note. #2 (hermetic fetch-or-build test) → Task 1. #3 (clean-checkout install wires all three) → Task 3 step 5. #4 (claude-absent still exit 0) → Task 2 test Case B. ✓
- Out-of-scope (Windows, registry, auto-update) honored — no tasks add them. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full content; `<plugin-root>` and the version-hashed glob in Task 5 are inherent user-path variability, not plan gaps (the installer prints the exact path).

**Type/name consistency:** `bin/tally`, asset `tally-<triple>`, `TALLY_OUT`/`TALLY_REPO_ROOT`/`TALLY_CARGO_TOML`/`TALLY_BASE_URL`/`TALLY_BIN`/`TALLY_SKILL`/`TALLY_FETCH_OR_BUILD`, `claude mcp add --scope user tally -- "$bin" mcp`, `~/.claude/skills/tally/SKILL.md`, repo `jasonrr/herdr-tally` — used consistently across Tasks 1–5. The hash regex widened to `[0-9a-z]+` (Task 1 step 4) so the hermetic stub digest and real 64-hex digests both match.
