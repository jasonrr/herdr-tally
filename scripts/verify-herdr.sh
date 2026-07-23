#!/usr/bin/env bash
# Verify tally's herdr integration against the running herdr server — run this
# after a herdr version bump to confirm the plugin still works.
#
# Two modes:
#   scripts/verify-herdr.sh            verify against the CURRENTLY running server
#   scripts/verify-herdr.sh --bounce   restart the server first (loads the newly
#                                       installed binary), then verify. The restart
#                                       SIGHUPs every pane — including yours — so run
#                                       this mode DETACHED and read the log after:
#                                         nohup bash scripts/verify-herdr.sh --bounce \
#                                           >/tmp/verify-herdr.log 2>&1 &
#
# What it checks (the only herdr surfaces tally depends on):
#   1. running server version == disk binary version (`herdr --version`)
#   2. tally still linked+enabled
#   3. `plugin pane open` opens a "Tally" pane
#   4. `pane list` (JSON by default — NEVER pass --json, 0.7.4 rejects it) exposes
#      .label / .pane_id / .focused  ← the contract toggle-pane.sh matches on
#   5. pane repaints when the store changes (create todo -> wait output)
#   6. `pane zoom --on/--off` (the focus primitive; herdr has no focus-by-id)
# Everything runs in a throwaway workspace that is closed at the end.
set -uo pipefail
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:$PATH"

HERDR="${HERDR_BIN_PATH:-$(command -v herdr || echo /opt/homebrew/bin/herdr)}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
TALLY="$REPO/bin/tally"
MARK="verify-herdr-marker"
PASS=1
say()   { printf '%s\n' "$*"; }
check() { if [ "$2" -eq 0 ]; then say "PASS  $1"; else say "FAIL  $1"; PASS=0; fi; }

say "=== tally vs herdr verification ==="
[ -x "$TALLY" ] || { say "FAIL  bin/tally missing — build it first (cargo build --release && cp ...)"; exit 1; }

# --bounce: restart the launchd-keepalive server so a freshly installed binary loads.
if [ "${1:-}" = "--bounce" ]; then
  say "-- restarting herdr server (keepalive respawns it) --"
  "$HERDR" server stop >/dev/null 2>&1
  for i in $(seq 1 60); do
    sleep 1
    st="$("$HERDR" status server 2>/dev/null)"
    [ "$(printf '%s' "$st" | awk -F': *' '/^status:/{print $2}')" = "running" ] && break
  done
fi

DISK_VER="$("$HERDR" --version 2>/dev/null | awk '{print $NF}')"
SRV="$("$HERDR" status server 2>/dev/null)"
SRV_STATUS="$(printf '%s' "$SRV" | awk -F': *' '/^status:/{print $2}')"
SRV_VER="$(printf '%s' "$SRV" | awk -F': *' '/^version:/{print $2}')"
say "disk binary: ${DISK_VER:-?}   running server: ${SRV_STATUS:-?} ${SRV_VER:-?}"
if [ "$SRV_STATUS" != "running" ] || [ "$SRV_VER" != "$DISK_VER" ]; then
  say "FAIL  running server ($SRV_VER) != disk binary ($DISK_VER)."
  say "      Re-run with --bounce (DETACHED — it drops your pane) to load the new binary."
  exit 1
fi
check "server running on disk-binary version ($DISK_VER)" 0

# 2. tally linked+enabled?
"$HERDR" plugin list --json 2>/dev/null \
  | jq -e '.result.plugins[]? | select(.plugin_id=="tally" and .enabled==true)' >/dev/null 2>&1
check "tally plugin linked+enabled" $?

# 3. throwaway workspace (--focus so the plugin pane opens INTO it). Parse the
#    workspace_id from the CREATE response — .result.workspace.workspace_id — not
#    from `workspace list` (which doesn't reliably surface it by label).
WS="$("$HERDR" workspace create --focus --label tally-verify 2>/dev/null \
  | jq -r '.result.workspace.workspace_id // empty')"
say "throwaway workspace: ${WS:-<none>}"
[ -n "$WS" ]; check "created throwaway workspace" $?

# 4. open the todos pane exactly as toggle-pane.sh does.
"$HERDR" plugin pane open --plugin tally --entrypoint todos \
  --placement split --direction right --cwd "$REPO" --focus >/dev/null 2>&1
sleep 2

# 5. pane list (default JSON, NO --json flag) — the toggle-pane.sh contract.
match="$("$HERDR" pane list --workspace "$WS" 2>/dev/null \
  | jq -c '[.result.panes[]? | select(.label=="Tally")][0] // empty')"
PANE="$(printf '%s' "$match" | jq -r '.pane_id // empty')"
hasfocus="$(printf '%s' "$match" | jq -e 'has("focused")' >/dev/null 2>&1 && echo 1 || echo 0)"
[ -n "$PANE" ]; check "pane list exposes .label==\"Tally\" with .pane_id" $?
[ "$hasfocus" = "1" ]; check "pane list exposes .focused" $?

# 6. live repaint + zoom primitive.
if [ -n "$PANE" ]; then
  ( cd "$REPO" && "$TALLY" todos create --title "$MARK" ) >/dev/null 2>&1
  "$HERDR" pane wait-output "$PANE" --match "$MARK" --source recent --timeout 8000 >/dev/null 2>&1
  check "pane repaints on store change (create todo -> wait output)" $?
  "$HERDR" pane zoom "$PANE" --on  >/dev/null 2>&1 && "$HERDR" pane zoom "$PANE" --off >/dev/null 2>&1
  check "pane zoom --on/--off (focus primitive)" $?
else
  check "pane repaints on store change" 1
  check "pane zoom --on/--off" 1
fi

# cleanup: close the workspace (drops its panes) and delete the marker todo.
[ -n "$WS" ] && "$HERDR" workspace close "$WS" >/dev/null 2>&1
( cd "$REPO" && "$TALLY" todos list --json 2>/dev/null \
  | jq -r --arg m "$MARK" '.todos[]? | select((.title//"")==$m) | .id' \
  | while read -r id; do [ -n "$id" ] && "$TALLY" todos delete "$id" >/dev/null 2>&1; done ) || true

if [ "$PASS" -eq 1 ]; then say ""; say "RESULT: PASS — tally works against herdr $DISK_VER"; exit 0
else say ""; say "RESULT: FAIL — see per-check lines above"; exit 1; fi
