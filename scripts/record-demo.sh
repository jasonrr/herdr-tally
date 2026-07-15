#!/usr/bin/env bash
# Render every README demo from scratch: reseed the throwaway store, then run
# each vhs tape. Reseeds before each tape so all recordings share identical
# state (the two-way tape mutates the store). Needs vhs + tmux + a built
# bin/tally. Outputs: docs/media/{hero,two-way}.gif and docs/media/todos.png
set -euo pipefail
cd "$(dirname "$0")/.."

[ -x bin/tally ] || { echo "build bin/tally first (see CLAUDE.md)"; exit 1; }
command -v vhs  >/dev/null || { echo "need vhs: brew install vhs"; exit 1; }
command -v tmux >/dev/null || { echo "need tmux: brew install tmux"; exit 1; }

for tape in hero two-way stills; do
  echo "== recording $tape =="
  tmux kill-server 2>/dev/null || true
  bash scripts/demo-seed.sh >/dev/null
  vhs "docs/media/$tape.tape"
done

rm -f docs/media/_stills.gif
echo "done → docs/media/{hero,two-way}.gif, docs/media/todos.png"
