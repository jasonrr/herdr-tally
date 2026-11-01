#!/usr/bin/env bash
# Runs session-start.sh against temp repos: new name, old name, routing off, no file.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
run() { CLAUDE_PROJECT_DIR="$1" bash "$here/session-start.sh"; }
t="$(mktemp -d)"; trap 'rm -rf "$t"' EXIT
mkdir -p "$t/new/.claude" "$t/old/.claude" "$t/off/.claude" "$t/none"
printf '# dev loop\nrouting: on\n' > "$t/new/.claude/dev-loop.md"
printf '# dev loop\nrouting: on\n' > "$t/old/.claude/tally-dev-loop.md"
printf '# dev loop\nrouting: off\n' > "$t/off/.claude/dev-loop.md"
run "$t/new" | jq -e '.hookSpecificOutput.additionalContext | test("/loop:plan") and test("<dev-loop>") and (test("tally") | not)' >/dev/null
run "$t/old" | jq -e '.hookSpecificOutput.additionalContext | test("/loop:build")' >/dev/null
[ -z "$(run "$t/off")" ]
[ -z "$(run "$t/none")" ]
echo "session-start.test.sh: OK"
