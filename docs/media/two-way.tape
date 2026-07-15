# tally — the two-way ledger. Left: your TUI. Right: an agent session writing to
# the same store. The TUI re-reads every 2s, so the agent's edits land live.
# Re-render with: bash scripts/record-demo.sh
Output docs/media/two-way.gif
Set Shell "bash"
Set FontSize 14
Set Width 1320
Set Height 540
Set Padding 18

Hide
Type 'export XDG_STATE_HOME="$PWD/.demo/state"; export PATH="$PWD/.demo/bin:$PWD/bin:$PATH"; cd .demo/herd'
Enter
Type 'export bill=$(tally todos list --json | jq -r ".todos[] | select(.priority==\"p0\") | .id")'
Enter
Type "clear"
Enter
Type "tmux kill-server 2>/dev/null; true"
Enter
Type "tmux new-session \; set -g status off \; split-window -h \; select-pane -t 0"
Enter
Sleep 1s
Show

# Left pane: the human's TUI.
Type "tally tui todos"
Enter
Sleep 3s

# Right pane: an agent session on the same store.
Ctrl+b
Right
Sleep 500ms
Type "# agent session — same project, same ledger"
Enter
Sleep 800ms
Type "tally comments recent --since 1h"
Enter
Sleep 3s

# The agent files a follow-up todo — watch it appear in the TUI on the left.
Type 'tally todos create --title "Add idempotency key to the charge endpoint" --priority p1 --tag billing'
Enter
Sleep 3500ms

# ...then claims the p0 billing bug and leaves a note. The TUI's 💬 count ticks up.
Type 'tally comments add $bill --body "Repro confirmed — retry within 3s double-fires. Taking this."'
Enter
Sleep 3500ms
Sleep 1s
