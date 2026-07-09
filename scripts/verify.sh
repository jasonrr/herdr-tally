#!/usr/bin/env bash
# End-to-end check against a live herdr (run from inside herdr, HERDR_ENV=1).
set -euo pipefail
cd "$(dirname "$0")/.."
herdr="${HERDR_BIN_PATH:-/opt/homebrew/bin/herdr}"
go build -o bin/herdr-notes .
"$herdr" plugin link "$PWD" || { "$herdr" plugin unlink herdr-notes && "$herdr" plugin link "$PWD"; }
echo "→ invoking toggle-todos (a Todos pane should open on the right)"
"$herdr" plugin action invoke toggle-todos --plugin herdr-notes
echo "→ creating a todo via CLI (the pane should repaint within ~1s)"
./bin/herdr-notes todos create --title "verify-$(date +%s)"
echo "OK — confirm the new todo appeared in the pane, then invoke toggle-todos again to close it."
