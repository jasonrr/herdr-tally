#!/bin/sh
# install.sh — tally's herdr [[build]] step. Runs on every `herdr plugin install`
# and re-link. Three phases:
#   1. fetch-or-build the binary (CRITICAL — aborts the install on failure).
#   2. register the tally MCP server with Claude Code (best-effort).
#   3. write the tally guidance block into ~/.claude/CLAUDE.md (best-effort).
# Best-effort = a failure prints a manual-fix command and we still exit 0, so the
# binary + panes always install even when `claude`/$HOME wiring can't complete.
set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
plugin_root="${HERDR_PLUGIN_ROOT:-$repo_root}"
bin="${TALLY_BIN:-$plugin_root/bin/tally}"

# --- 1. binary (critical path) --------------------------------------------------
fob="${TALLY_FETCH_OR_BUILD:-$script_dir/fetch-or-build.sh}"
if ! sh "$fob"; then
  echo "tally: binary install failed — aborting." >&2
  exit 1
fi

# --- 2. MCP server registration (best-effort) -----------------------------------
find_claude() {
  for c in "$HOME/.local/bin/claude" /opt/homebrew/bin/claude /usr/local/bin/claude; do
    [ -x "$c" ] && { printf '%s\n' "$c"; return 0; }
  done
  command -v claude 2>/dev/null && return 0
  return 1
}
manual_mcp="claude mcp add --scope user tally -- \"$bin\" mcp"
if claude_bin=$(find_claude); then
  "$claude_bin" mcp remove --scope user tally >/dev/null 2>&1 || true
  if "$claude_bin" mcp add --scope user tally -- "$bin" mcp >/dev/null 2>&1; then
    echo "tally: registered MCP server (user scope) -> $bin mcp"
  else
    echo "tally: could not register MCP server automatically. Run:" >&2
    echo "  $manual_mcp" >&2
  fi
else
  echo "tally: 'claude' CLI not found on PATH. Register the MCP server with:" >&2
  echo "  $manual_mcp" >&2
fi

# --- 3. guidance block in ~/.claude/CLAUDE.md (best-effort) ---------------------
# Primes every Claude Code session (all projects) to use tally correctly, keyed
# by the tally:start/end markers so re-runs replace the block instead of stacking.
md="$HOME/.claude/CLAUDE.md"
if mkdir -p "$HOME/.claude" 2>/dev/null && tmp=$(mktemp 2>/dev/null); then
  # strip any existing tally block (incl. markers), then trailing blank lines
  if [ -f "$md" ]; then
    awk '/<!-- tally:start -->/{s=1} s==0{print} /<!-- tally:end -->/{s=0}' "$md" \
      | awk '{L[NR]=$0} END{n=NR; while(n>0 && L[n]~/^[ \t]*$/)n--; for(i=1;i<=n;i++)print L[i]}' > "$tmp"
  fi
  # separate from existing content, then append the block verbatim (heredoc is
  # literal — safe with the apostrophes and backticks inside)
  [ -s "$tmp" ] && printf '\n' >> "$tmp"
  cat >> "$tmp" <<'BLOCK'
<!-- tally:start -->
## tally — shared todos, scratchpads, plans & comments (live in herdr panes)
When a project has a tally store, prefer the `tally_*` MCP tools (`todo_*` / `scratchpad_*` / `comment_*`; ToolSearch for `mcp__tally__` to load them). They return item ids.
- **Scratchpad**: multi-step plans, handoffs, context too big for a todo. Revision-guarded — a read gives a `revision`; pass it back on the next write, and on a mismatch re-read.
- **Todo**: one discrete follow-up/blocker. status = `open`|`in_progress`|`completed`, priority = `high`|`medium`|`low` (anything else is rejected).
- **Comment**: a margin note (the *why*), not a state change. Lock a todo you're actively editing.
- Don't delete the human's items — archive scratchpads instead. Complete todos you finish.
<!-- tally:end -->
BLOCK
  if mv "$tmp" "$md" 2>/dev/null; then
    echo "tally: wrote guidance block -> $md"
  else
    rm -f "$tmp"
    echo "tally: could not update $md — copy the tally:start/end block from scripts/install.sh into it manually." >&2
  fi
else
  echo "tally: could not update $HOME/.claude/CLAUDE.md (mkdir/mktemp failed)." >&2
fi

exit 0
