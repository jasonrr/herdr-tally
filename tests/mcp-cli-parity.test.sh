#!/bin/sh
# Guards the "thin adapters" invariant: every MCP tool must have a CLI verb, since
# the tally pi package ships CLI-only. Queries the live tools/list from `tally mcp`,
# maps each tool name to its expected CLI verb (namespace-scoped bare-subcommand
# usage string), and fails loudly if any tool has no CLI equivalent.
#
# ALLOWLIST — phase-2 stubs, non-functional in MCP too (src/mcp/tools.rs returns
# Err("...is phase 2") for these, so no CLI verb is expected or possible):
#   todo_transfer, scratchpad_transfer
#
# NAME-MAP — same capability, different verb spelling:
#   scratchpad_write  -> satisfied by CLI verb `create` OR `update`
#   *_tags_list       -> satisfied by CLI verb `tags`
set -eu
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$here/.." && pwd)
fail=0

# Build fresh so the test reflects current source, not a possibly-stale bin/tally.
# Run target/release/tally directly (not bin/tally) to sidestep the macOS
# code-signature stale-inode issue documented in CLAUDE.md.
(cd "$repo_root" && cargo build --release --quiet)
BIN="$repo_root/target/release/tally"

# ===== Step 1: authoritative MCP tool list (expect 38) ===========================
tools=$(printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"parity-test","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  | "$BIN" mcp 2>/dev/null \
  | jq -r 'select(.id==2) | .result.tools[].name')

tool_count=$(printf '%s\n' "$tools" | grep -c . || true)
if [ "$tool_count" -eq 0 ]; then
  echo "FAIL - MCP handshake yielded 0 tools (framing broken?)"
  exit 1
fi
if [ "$tool_count" -ne 38 ]; then
  echo "FAIL - expected 38 MCP tools, got $tool_count"
  fail=1
fi

# ===== Step 2: CLI verbs per namespace ============================================
# Bare `tally <namespace>` prints "error: usage: tally <ns> <a|b|c>" to stderr.
verbs_of() {  # $1 = namespace (todos|scratchpads|comments)
  "$BIN" "$1" 2>&1 | sed -n 's/.*<\(.*\)>.*/\1/p' | tr '|' '\n'
}
todo_verbs=$(verbs_of todos)
scratchpad_verbs=$(verbs_of scratchpads)
comment_verbs=$(verbs_of comments)

has_verb() {  # $1 = verb, $2 = newline-separated verb list
  printf '%s\n' "$2" | grep -qx -- "$1"
}

# ===== Step 3: map each tool -> expected CLI verb, assert ========================
# ALLOWLIST — see file header for why these two are exempt.
allowlist="todo_transfer scratchpad_transfer"

missing=0
for tool in $tools; do
  case " $allowlist " in
    *" $tool "*) continue ;;
  esac

  case "$tool" in
    todo_*) ns=todos; verbs="$todo_verbs"; rest=${tool#todo_} ;;
    scratchpad_*) ns=scratchpads; verbs="$scratchpad_verbs"; rest=${tool#scratchpad_} ;;
    comment_*) ns=comments; verbs="$comment_verbs"; rest=${tool#comment_} ;;
    *)
      echo "MISSING CLI verb for $tool (unrecognized namespace prefix)"
      missing=1
      continue
      ;;
  esac

  # NAME-MAP: scratchpad_write is satisfied by CLI create OR update.
  if [ "$tool" = "scratchpad_write" ]; then
    if has_verb create "$verbs" || has_verb update "$verbs"; then continue; fi
    echo "MISSING CLI verb for $tool (expected \`$ns create\` or \`$ns update\`)"
    missing=1
    continue
  fi

  # NAME-MAP: any *_tags_list is satisfied by CLI verb "tags".
  case "$tool" in
    *_tags_list)
      if has_verb tags "$verbs"; then continue; fi
      echo "MISSING CLI verb for $tool (expected \`$ns tags\`)"
      missing=1
      continue
      ;;
  esac

  verb=$(printf '%s' "$rest" | tr '_' '-')
  if has_verb "$verb" "$verbs"; then continue; fi
  echo "MISSING CLI verb for $tool (expected \`$ns $verb\`)"
  missing=1
done

if [ "$missing" -ne 0 ] || [ "$fail" -ne 0 ]; then
  exit 1
fi

echo "parity OK ($tool_count tools, 2 allowlisted stubs)"
exit 0
