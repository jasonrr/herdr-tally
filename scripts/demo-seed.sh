#!/usr/bin/env bash
# Seed an isolated, throwaway tally store for recording the README demos.
# Reproducible: wipes and rebuilds the demo store + project on every run, and
# never touches your real store (points XDG_STATE_HOME at .demo/state).
#
#   TALLY_BIN     tally binary            (default: ./bin/tally)
#   DEMO_HOME     XDG_STATE_HOME to use   (default: ./.demo/state)
#   DEMO_PROJECT  throwaway project repo  (default: ./.demo/herd)
#
# The demo project gets a fake `origin` so a todo can carry a GitHub link (the
# ⇅ glyph) WITHOUT ever calling `gh`. Don't run `tally sync` against it — that
# would try to create real issues on acme/herd.
set -euo pipefail

TALLY="${TALLY_BIN:-$PWD/bin/tally}"
DEMO_HOME="${DEMO_HOME:-$PWD/.demo/state}"
DEMO_PROJECT="${DEMO_PROJECT:-$PWD/.demo/herd}"

DEMO_BIN="$(dirname "$DEMO_HOME")/bin"

rm -rf "$DEMO_HOME" "$DEMO_PROJECT" "$DEMO_BIN"
mkdir -p "$DEMO_HOME" "$DEMO_PROJECT" "$DEMO_BIN"
export XDG_STATE_HOME="$DEMO_HOME"

git -C "$DEMO_PROJECT" init -q
git -C "$DEMO_PROJECT" remote add origin git@github.com:acme/herd.git

# Offline `gh` shim so the TUI's background sync worker shows a clean "↕ synced"
# footer without any network call or a real issue on acme/herd. Put $DEMO_BIN
# first on PATH when recording. Every verb the reconcile touches succeeds benignly.
cat > "$DEMO_BIN/gh" <<'SHIM'
#!/usr/bin/env bash
case "$1 $2" in
  "auth status")  exit 0 ;;
  "issue create") echo "https://github.com/acme/herd/issues/42" ;;
  "issue view")   echo '{"state":"OPEN"}' ;;
  "issue edit"|"issue close"|"issue reopen") exit 0 ;;
  "api"*)
    case "$*" in
      *POST*) echo '{"id":999}' ;;   # comment create
      *)      echo '[]' ;;           # comment/event list
    esac ;;
  *) exit 0 ;;
esac
SHIM
chmod +x "$DEMO_BIN/gh"

# Two authors, so the two-way ledger reads clearly: `jason` (human) and `agent`.
# Run from inside the project dir so tally derives the project from cwd — the
# same way the TUI and a real agent session do.
human() { ( cd "$DEMO_PROJECT" && HERDR_NOTES_OWNER=jason "$TALLY" "$@" ); }
agent() { ( cd "$DEMO_PROJECT" && HERDR_NOTES_OWNER=agent "$TALLY" "$@" ); }
id_of() { jq -r '.id'; }

# --- Todos ---------------------------------------------------------------
auth=$(agent todos create --title "Rotate refresh tokens on reuse" --priority p1 --tag auth --json | id_of)
agent todos update "$auth" --status in_progress >/dev/null
agent todos update "$auth" --github on >/dev/null          # ⇅ (link only; no gh call)

bill=$(human todos create --title "Double-charge when retrying a failed payment" --priority p0 --tag billing --json | id_of)

rate=$(agent todos create --title "Emit rate-limit headers on the public API" --priority p2 --tag api --json | id_of)
agent todos complete "$rate" >/dev/null

human todos create --title "Dark-mode contrast fails WCAG on the plan tab" --priority p2 --tag ui >/dev/null

flake=$(agent todos create --title "Flaky: auth_test races on token refresh" --priority p1 --tag auth --tag test --json | id_of)
agent todos add-blocker "$flake" --blocker "$auth" >/dev/null   # can't fix until refresh lands

# --- Comments (the margin thread) ---------------------------------------
human comments add "$auth" --body "Cap reuse at one grace window — see the RFC in the scratchpad." >/dev/null
agent comments add "$auth" --body "Done. Grace window is 10s; added a test for the race." >/dev/null
human comments add "$bill" --body "Repro: retry within 3s double-fires the charge. p0." >/dev/null

# --- Scratchpads ---------------------------------------------------------
agent scratchpads create --name "Auth refactor — plan" --content-file - >/dev/null <<'EOF'
# Auth refactor — plan

## Approach
Move refresh-token rotation behind a single grace window so a reused token is
accepted once, then hard-revoked. One code path for web + mobile.

## Progress
- [x] Grace-window store field
- [x] Rotation on reuse
- [ ] Backfill existing sessions
- [ ] Metrics: reuse-after-revoke counter

## Open questions
- Do we revoke the whole family on a second reuse, or just the branch?
- 10s grace enough for slow mobile networks?
EOF

human scratchpads create --name "Where I left off" --content-file - >/dev/null <<'EOF'
# Where I left off

Handed the auth work to the agent. Refresh rotation is in and tested; next is
the session backfill (see the plan). Billing double-charge is the p0 — repro is
in its comments. Don't touch the plan tab styling yet, contrast pass is queued.
EOF

# --- Plans (read-only tab; read straight from disk, not the store) ----------
mkdir -p "$DEMO_PROJECT/docs/superpowers/plans"
cat > "$DEMO_PROJECT/docs/superpowers/plans/auth-refactor.md" <<'EOF'
# Refresh-token rotation — implementation plan

## Goal
One grace window for token reuse across web and mobile. A reused token is
accepted once, then the whole family is hard-revoked.

## Steps
1. Add a `grace_until` field to the session store → verify: migration round-trips.
2. Rotate on reuse; revoke the family on a second reuse → verify: `auth_test` passes.
3. Backfill live sessions with a null grace window → verify: no forced logouts.
4. Emit a `reuse_after_revoke` counter → verify: metric shows in the dashboard.

## Risks
- Slow mobile networks may exceed a 10s grace window — measure p99 before shipping.
EOF

echo "seeded → store=$DEMO_HOME project=$DEMO_PROJECT bin=$DEMO_BIN"
( cd "$DEMO_PROJECT" && "$TALLY" todos list )
