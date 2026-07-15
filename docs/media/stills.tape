# Static still for inline use in the README (the todos list — note the ⇅ synced
# glyph and the "↕ synced" footer). Re-render with: bash scripts/record-demo.sh
# vhs writes only the last Screenshot per tape, so this tape takes exactly one.
# The .gif output is a throwaway; todos.png is the artifact.
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
Screenshot docs/media/todos.png
Sleep 300ms
