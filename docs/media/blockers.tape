# Static still for the README blockers section: the blocker picker open on the
# blocked todo (⛔ in the list behind it). Re-render with: bash scripts/record-demo.sh
# vhs writes only the last Screenshot per tape, so this tape takes exactly one.
# The .gif output is a throwaway; blockers.png is the artifact.
Output docs/media/_stills.gif
Set Shell "bash"
Set FontSize 15
Set Width 1100
Set Height 500
Set Padding 22

Hide
Type 'export XDG_STATE_HOME="$PWD/.demo/state"; export PATH="$PWD/.demo/bin:$PWD/bin:$PATH"; cd .demo/herd; clear'
Enter
Show
Type "tally tui todos"
Enter
Sleep 2500ms
Down
Down
Sleep 500ms
Type "b"
Sleep 1200ms
Screenshot docs/media/blockers.png
Sleep 300ms
