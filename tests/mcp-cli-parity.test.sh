#!/bin/sh
# Guards the "thin adapters" invariant: every MCP tool must have a CLI verb, since
# the tally pi package ships CLI-only. Queries the live tools/list from `tally mcp`,
# maps each tool name to its expected CLI verb, and for each one PROBES the verb by
# actually invoking the binary (not by string-matching the bare-subcommand usage
# line, which only proves the verb is *mentioned*, not that it dispatches).
#
# The dispatch signal: `tally <ns> <badname>` prints exactly
#   error: unknown <ns> subcommand: <badname>
# A recognized verb never prints that string, even when it errors for other
# reasons (missing args, not-found id, missing --expected-revision, ...). So the
# check is: invoke `tally <ns> <verb>` and assert the "unknown <ns> subcommand"
# string is ABSENT.
#
# Most verbs are safe to invoke bare (arg validation or a not-found lookup fails
# before any mutation). Two are not: `todos create` and `scratchpads create` both
# happily create a real record with all-empty fields when given no args (no
# required-field enforcement at the CLI layer) - confirmed empirically, and it
# mutates the LIVE project store (worktrees share one store, per CLAUDE.md). Those
# two probes create a tagged, clearly-named record and delete it immediately after
# (self-cleaning), so the test has no net effect on store contents.
#
# ALLOWLIST — phase-2 stubs, non-functional in MCP too (src/mcp/tools.rs returns
# Err("...is phase 2") for these). The allowlist is STUB-VERIFIED, not name-only:
# each allowlisted tool gets a live `tools/call` and the test FAILS if the response
# is no longer a "phase 2" error (i.e. the stub graduated to real functionality —
# at that point it needs a real CLI verb like everything else):
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

# ===== Step 2: allowlist is stub-verified, not name-only ==========================
# For each allowlisted tool, actually call it over MCP and confirm it STILL errors
# with "phase 2". If it doesn't, the stub graduated and needs a real CLI verb -
# fail loudly rather than silently accepting a name match.
allowlist="todo_transfer scratchpad_transfer"

for tool in $allowlist; do
  resp=$(printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"parity-test","version":"0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"$tool\",\"arguments\":{}}}" \
    | "$BIN" mcp 2>/dev/null \
    | jq -c 'select(.id==2)')

  is_error=$(printf '%s' "$resp" | jq -r '.result.isError // false')
  text=$(printf '%s' "$resp" | jq -r '.result.content[0].text // ""')

  case "$text" in
    *"phase 2"*)
      if [ "$is_error" != "true" ]; then
        echo "FAIL - $tool mentions \"phase 2\" but isError is not true (response: $resp)"
        fail=1
      fi
      ;;
    *)
      echo "FAIL - allowlisted $tool no longer errors with \"phase 2\" (it graduated - add a CLI verb and drop it from the allowlist). response: $resp"
      fail=1
      ;;
  esac
done

# ===== Step 3: probe that each non-allowlisted tool's CLI verb actually DISPATCHES
# dispatchable NS VERB -> exit 0 if invoking `tally NS VERB` does NOT hit the
# "unknown NS subcommand" fallback arm; exit 1 otherwise. Self-cleans the two
# known-mutating bare invocations (todos/scratchpads create).
dispatchable() {
  ns=$1
  verb=$2
  case "$ns:$verb" in
    todos:create)
      out=$("$BIN" todos create --title "__tally-parity-probe__ (safe to delete)" 2>&1) || true
      id=$(printf '%s' "$out" | jq -r '.id // empty' 2>/dev/null || true)
      [ -n "$id" ] && "$BIN" todos delete "$id" >/dev/null 2>&1
      ;;
    scratchpads:create)
      out=$("$BIN" scratchpads create --name "__tally-parity-probe__ (safe to delete)" 2>&1) || true
      id=$(printf '%s' "$out" | jq -r '.id // empty' 2>/dev/null || true)
      rev=$(printf '%s' "$out" | jq -r '.revision // empty' 2>/dev/null || true)
      [ -n "$id" ] && "$BIN" scratchpads delete "$id" --expected-revision "${rev:-1}" --confirm >/dev/null 2>&1
      ;;
    *)
      out=$("$BIN" "$ns" "$verb" 2>&1 </dev/null) || true
      ;;
  esac
  case "$out" in
    *"unknown $ns subcommand"*) return 1 ;;
    *) return 0 ;;
  esac
}

missing=0
for tool in $tools; do
  case " $allowlist " in
    *" $tool "*) continue ;;
  esac

  case "$tool" in
    todo_*) ns=todos; rest=${tool#todo_} ;;
    scratchpad_*) ns=scratchpads; rest=${tool#scratchpad_} ;;
    comment_*) ns=comments; rest=${tool#comment_} ;;
    *)
      echo "MISSING CLI verb for $tool (unrecognized namespace prefix)"
      missing=1
      continue
      ;;
  esac

  # NAME-MAP: scratchpad_write is satisfied by CLI create OR update.
  if [ "$tool" = "scratchpad_write" ]; then
    if dispatchable scratchpads update || dispatchable scratchpads create; then continue; fi
    echo "MISSING CLI verb for $tool (expected \`$ns create\` or \`$ns update\` to dispatch)"
    missing=1
    continue
  fi

  # NAME-MAP: any *_tags_list is satisfied by CLI verb "tags".
  case "$tool" in
    *_tags_list)
      if dispatchable "$ns" tags; then continue; fi
      echo "MISSING CLI verb for $tool (expected \`$ns tags\` to dispatch)"
      missing=1
      continue
      ;;
  esac

  verb=$(printf '%s' "$rest" | tr '_' '-')
  if dispatchable "$ns" "$verb"; then continue; fi
  echo "MISSING CLI verb for $tool (expected \`$ns $verb\` to dispatch)"
  missing=1
done

if [ "$missing" -ne 0 ] || [ "$fail" -ne 0 ]; then
  exit 1
fi

echo "parity OK ($tool_count tools, 2 allowlisted stubs stub-verified)"
exit 0
