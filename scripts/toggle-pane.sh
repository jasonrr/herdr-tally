#!/usr/bin/env bash
# Open the pane if absent; focus it if present-but-unfocused; close it if focused.
# Mirrors herdr-file-viewer's open-or-focus-or-close pattern. Focus primitive is a
# zoom on/off cycle (herdr has no focus-by-id).
#
# Pane matching: verified live against herdr 0.7.3 — `herdr pane list` carries no
# per-pane command/args field for a plugin-launched pane (grepping for "pane.sh
# $kind" would never match; that string is not present in the JSON at all). What
# IS present is `.label`, which herdr copies from the manifest's [[panes]] `title`
# ("Tally" / "Scratchpads") onto the pane the moment it's opened via
# `plugin pane open`. So we match on label, scoped to this workspace via
# $HERDR_WORKSPACE_ID (herdr sets this for every pane it spawns).
set -euo pipefail
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:$PATH"
kind="${1:?todos|scratchpads}"
# herdr injects HERDR_BIN_PATH for plugin actions (used in the real path). When
# unset (manual runs), resolve `herdr` from PATH before the macOS Homebrew default
# so this works on Linux too.
herdr="${HERDR_BIN_PATH:-$(command -v herdr || echo /opt/homebrew/bin/herdr)}"

case "$kind" in
  todos) title="Tally" ;;
  scratchpads) title="Scratchpads" ;;
  *) echo "usage: toggle-pane.sh todos|scratchpads" >&2; exit 2 ;;
esac

list_args=(pane list)
if [ -n "${HERDR_WORKSPACE_ID:-}" ]; then
  list_args+=(--workspace "$HERDR_WORKSPACE_ID")
fi

match="$("$herdr" "${list_args[@]}" 2>/dev/null \
  | jq -c --arg title "$title" '[.result.panes[]? | select(.label == $title)][0] // empty')" || true

existing="$(printf '%s' "$match" | jq -r 'if . == "" then "" else (.pane_id // "") end' 2>/dev/null || true)"
focused="$(printf '%s' "$match" | jq -r 'if . == "" then "false" else (.focused // false) end' 2>/dev/null || true)"

if [ -z "$existing" ]; then
  # The pane must resolve its project from the repo the user is looking at, not
  # this plugin's root (actions run with cwd = plugin root). Derive the focused
  # pane's cwd from the plugin context and hand it to the pane via --cwd, same
  # source mutation actions use. Falls back to $PWD if context is absent.
  project="$(printf '%s' "${HERDR_PLUGIN_CONTEXT_JSON:-}" \
    | jq -r '.focused_pane_cwd // .workspace_cwd // empty' 2>/dev/null || true)"
  project="${project:-$PWD}"
  "$herdr" plugin pane open --plugin tally --entrypoint "$kind" \
    --placement split --direction right --cwd "$project" --focus
elif [ "$focused" = "true" ]; then
  "$herdr" pane close "$existing"
else
  # focus via zoom on/off; if the zoom cycle fails for any reason, fall back to
  # closing the pane outright so a repeat invocation always changes something.
  "$herdr" pane zoom "$existing" --on >/dev/null 2>&1 || true
  "$herdr" pane zoom "$existing" --off >/dev/null 2>&1 || "$herdr" pane close "$existing"
fi
