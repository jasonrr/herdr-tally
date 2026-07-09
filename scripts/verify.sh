#!/usr/bin/env bash
# End-to-end check against a live herdr (run from inside herdr, HERDR_ENV=1).
set -euo pipefail
cd "$(dirname "$0")/.."
herdr="${HERDR_BIN_PATH:-/opt/homebrew/bin/herdr}"
cargo build --release && mkdir -p bin && rm -f bin/tally && cp target/release/tally bin/tally
"$herdr" plugin link "$PWD" || { "$herdr" plugin unlink tally && "$herdr" plugin link "$PWD"; }
echo "→ invoking toggle-todos (a Todos pane should open on the right)"
"$herdr" plugin action invoke toggle-todos --plugin tally
echo "→ creating a todo via CLI (the pane should repaint within ~1s)"
./bin/tally todos create --title "verify-$(date +%s)"
echo "OK — confirm the new todo appeared in the pane, then invoke toggle-todos again to close it."
