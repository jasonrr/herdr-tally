# tally — hero tour. Re-render with: bash scripts/record-demo.sh
# Assumes `bash scripts/demo-seed.sh` has populated .demo/ (record-demo.sh does it).
Output docs/media/hero.gif
Set Shell "bash"
Set FontSize 15
Set Width 1100
Set Height 640
Set Padding 22

Hide
Type 'export XDG_STATE_HOME="$PWD/.demo/state"; export PATH="$PWD/.demo/bin:$PWD/bin:$PATH"; cd .demo/herd; clear'
Enter
Show
Type "tally tui todos"
Enter
Sleep 2s

# Glance down the list — priorities, the ⇅ synced todo, a ⛔ blocker.
Down
Sleep 700ms
Down
Sleep 900ms
Up

# Open the auth todo to show its margin thread (human + agent comments).
Sleep 600ms
Enter
Sleep 3s
Escape
Sleep 800ms

# Filter to just the auth work.
Type "/"
Sleep 400ms
Type "auth"
Sleep 1800ms
Escape
Sleep 700ms

# Scratchpads tab — open the plan in read mode (rendered markdown).
Type "2"
Sleep 1500ms
Enter
Sleep 3s
Escape
Sleep 800ms

# Plans tab — read-only, straight off disk.
Type "3"
Sleep 1500ms
Enter
Sleep 3s
Escape
Sleep 800ms

# Back to todos.
Type "1"
Sleep 1800ms
