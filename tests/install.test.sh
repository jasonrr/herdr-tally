#!/bin/sh
# Hermetic test for scripts/install.sh. Stubs the binary step (TALLY_FETCH_OR_BUILD),
# a fake `claude` on PATH, and a temp HOME, then asserts MCP registration + the
# ~/.claude/CLAUDE.md guidance block (written once, idempotent) happen and that a
# missing `claude` still exits 0.
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

# Seed a pre-existing global CLAUDE.md to prove the block is appended, not clobbering.
mkdir -p "$HOME/.claude"
printf '# my rules\nkeep me\n' > "$HOME/.claude/CLAUDE.md"

# ===== Case A: claude present -> registers MCP + writes guidance block ==========
PATH="$work/bin:$PATH" sh "$script" >/dev/null 2>&1 || true
addline=$(grep -c "mcp add --scope user tally -- $work/pluginbin/tally mcp" "$work/claude.log" 2>/dev/null || echo 0)
check "mcp add invoked with user scope + bin path" "$addline" "1"
rmline=$(grep -c "mcp remove --scope user tally" "$work/claude.log" 2>/dev/null || echo 0)
check "mcp remove invoked before add" "$rmline" "1"
markers=$(grep -c "<!-- tally:start -->" "$HOME/.claude/CLAUDE.md" 2>/dev/null || echo 0)
check "guidance block written to ~/.claude/CLAUDE.md" "$markers" "1"
kept=$(grep -c "keep me" "$HOME/.claude/CLAUDE.md" 2>/dev/null || echo 0)
check "pre-existing CLAUDE.md content preserved" "$kept" "1"

# Idempotent: a second run replaces the block rather than stacking a second copy.
PATH="$work/bin:$PATH" sh "$script" >/dev/null 2>&1 || true
markers2=$(grep -c "<!-- tally:start -->" "$HOME/.claude/CLAUDE.md" 2>/dev/null || echo 0)
check "re-run leaves exactly one block" "$markers2" "1"

# ===== Case B: claude absent -> still exit 0 ====================================
rm -f "$work/claude.log"
PATH="/usr/bin:/bin" HOME="$work/home" sh "$script" >/dev/null 2>&1; rc=$?
check "exit 0 when claude missing" "$rc" "0"

exit $fail
