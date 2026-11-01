#!/usr/bin/env bash
# SessionStart hook for the loop plugin. Emits the dev-loop routing block ONLY
# when this repo opted in via .claude/dev-loop.md (or the older
# .claude/tally-dev-loop.md) containing `routing: on` — the file /loop:setup
# writes. Silent no-op everywhere else: consent lives in the repo, not the install.
set -euo pipefail

dir="${CLAUDE_PROJECT_DIR:-.}/.claude"
cfg="$dir/dev-loop.md"
[ -f "$cfg" ] || cfg="$dir/tally-dev-loop.md"
if [ ! -f "$cfg" ] || ! grep -q '^routing: on$' "$cfg" 2>/dev/null; then
  exit 0
fi

# routing.md is the single copy of the block; JSON-escape it for additionalContext.
ctx="$(awk '{ gsub(/\r/, ""); gsub(/\\/, "\\\\"); gsub(/"/, "\\\""); gsub(/\t/, "\\t"); printf "%s\\n", $0 }' "$(dirname "$0")/routing.md")"
printf '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"%s"}}\n' "$ctx"
