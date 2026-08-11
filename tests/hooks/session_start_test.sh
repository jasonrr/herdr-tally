#!/usr/bin/env bash
# Self-check for hooks/session-start.sh: silent without opt-in, valid JSON with it.
set -euo pipefail
cd "$(dirname "$0")/../.."
HOOK=hooks/session-start.sh
fail() { echo "FAIL: $1"; exit 1; }

tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

# 1. no config file -> no output, exit 0
out=$(CLAUDE_PROJECT_DIR="$tmp" bash "$HOOK")
[ -z "$out" ] || fail "expected no output without config, got: $out"

# 2. routing: off -> no output
mkdir -p "$tmp/.claude"
printf '# dev loop\nrouting: off\n' > "$tmp/.claude/tally-dev-loop.md"
out=$(CLAUDE_PROJECT_DIR="$tmp" bash "$HOOK")
[ -z "$out" ] || fail "expected no output with routing: off"

# 3. routing: on -> valid JSON containing the routing rule
printf '# dev loop\nrouting: on\n' > "$tmp/.claude/tally-dev-loop.md"
out=$(CLAUDE_PROJECT_DIR="$tmp" bash "$HOOK")
echo "$out" | python3 -c 'import sys,json; d=json.load(sys.stdin); assert "tally:brainstorm" in d["hookSpecificOutput"]["additionalContext"]' \
  || fail "routing: on must emit valid hook JSON containing the rule"

echo "PASS: session-start hook"
