---
name: verify-herdr-version
description: Use when a new herdr version is released or installed and you need to confirm tally's herdr plugin (panes, actions, toggle-pane.sh) still works against it — e.g. "herdr 0.7.x is out, make sure tally still works", after `brew upgrade herdr`, or when checking the CLAUDE.md "verified live against herdr X" pin.
---

# Verify tally against a new herdr version

## What this is
tally is a shipped herdr plugin. When herdr bumps versions, the risk is that a
CLI/JSON contract tally's scripts depend on changed. `scripts/verify-herdr.sh`
exercises every herdr surface tally uses, in a throwaway workspace, and prints a
PASS/FAIL verdict. It self-cleans (closes the workspace, deletes its marker todo).

## Run it

```bash
# Common case: server already running the new version → verify inline (fast).
bash scripts/verify-herdr.sh
```

If it says the running server != the disk binary, the newly installed binary
isn't loaded yet. The launchd service is `keepalive+runatload`, so a restart
reloads it — but the restart **SIGHUPs every pane, including the one you're in.**
Run the bounce mode **detached** and read the log after herdr comes back:

```bash
nohup bash scripts/verify-herdr.sh --bounce > /tmp/verify-herdr.log 2>&1 &
# herdr restarts; your Claude pane drops. Reattach, then: cat /tmp/verify-herdr.log
```

`RESULT: PASS` → tally is fine on this version. On PASS, update the
`verified live against herdr X` pin in `CLAUDE.md`.

## Gotchas this encodes (don't re-learn them)

| Trap | Reality |
|---|---|
| `herdr pane list --json` | **Rejected** (exit 2). `pane list` emits JSON by default; `toggle-pane.sh` relies on that — never add `--json`. |
| Getting the workspace id | Parse `workspace create` → `.result.workspace.workspace_id`. `workspace list` doesn't reliably surface it by label. |
| Matching the pane | `.result.panes[] \| select(.label=="Tally")`. `.label` is null on non-plugin panes (`== "Tally"` skips them correctly); `.pane_id`/`.focused` are always present. |
| "Upgrading" herdr (brew install) | `brew upgrade` relinks the binary; the **running server keeps the old image** until `herdr server stop` (keepalive respawns the new one). Disk-vs-server version skew is normal mid-upgrade. |
| Restarting to load new version | Kills your session. Only `--bounce` mode does it, and only run detached. |
| Focus in herdr | No focus-by-id; `pane zoom --on/--off` is the primitive. |

## What it does NOT cover
The non-pane gotchas in `CLAUDE.md` (cwd derivation from `$HERDR_PLUGIN_CONTEXT_JSON`,
launchd bare-PATH, `worktree.created` events). Nothing in the 0.7.x line has
touched those, but a major herdr bump warrants re-reading that section by hand.
