#!/usr/bin/env bash
# SessionStart hook for the tally plugin. Emits the dev-loop routing rule ONLY
# when this repo opted in via .claude/tally-dev-loop.md with `routing: on` — the file
# /tally:setup writes. Silent no-op everywhere else: consent lives in the repo,
# not in the install.
set -euo pipefail

cfg="${CLAUDE_PROJECT_DIR:-.}/.claude/tally-dev-loop.md"
if [ ! -f "$cfg" ] || ! grep -q '^routing: on$' "$cfg" 2>/dev/null; then
  exit 0
fi

ctx='<dev-loop>\nThis project uses the tally dev loop. Standing routing — the human should never have to ask:\n- Feature or non-trivial change requested → run /tally:brainstorm before writing any code.\n- Bug, failing test, or unexpected behavior → run /tally:debug before proposing any fix.\n- Approved design (tally scratchpad tagged design) or any multi-step implementation → run /tally:plan before building.\n- A plan doc with plan:<slug> todos exists → execute it with /tally:build.\n- Branch complete, or a merge/PR is requested → run /tally:review-branch first.\nTrivial one-file changes and pure questions are exempt. When this rule triggers a skill, say so in one line.\n</dev-loop>'

printf '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"%s"}}\n' "$ctx"
