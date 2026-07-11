#!/bin/sh
# install.sh — tally's herdr [[build]] step. Runs on every `herdr plugin install`
# and re-link. Three phases:
#   1. fetch-or-build the binary (CRITICAL — aborts the install on failure).
#   2. register the tally MCP server with Claude Code (best-effort).
#   3. install the tally agent skill (best-effort).
# Best-effort = a failure prints a manual-fix command and we still exit 0, so the
# binary + panes always install even when `claude`/$HOME wiring can't complete.
set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
plugin_root="${HERDR_PLUGIN_ROOT:-$repo_root}"
bin="${TALLY_BIN:-$plugin_root/bin/tally}"
skill_src="${TALLY_SKILL:-$plugin_root/SKILL.md}"

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

# --- 3. agent skill (best-effort) -----------------------------------------------
skill_dir="$HOME/.claude/skills/tally"
if mkdir -p "$skill_dir" 2>/dev/null && cp "$skill_src" "$skill_dir/SKILL.md" 2>/dev/null; then
  echo "tally: installed agent skill -> $skill_dir/SKILL.md"
else
  echo "tally: could not install the agent skill. Copy it with:" >&2
  echo "  mkdir -p \"$skill_dir\" && cp \"$skill_src\" \"$skill_dir/SKILL.md\"" >&2
fi

exit 0
