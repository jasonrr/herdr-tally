---
name: herdr
description: You run inside herdr, a terminal workspace manager. Use its native primitives for worktrees, panes, and dispatching work to fresh agents instead of rolling your own. Use whenever work needs isolation (a branch/worktree) or you want to hand a task to another agent pane.
---

# herdr

You are inside herdr when `HERDR_ENV=1`. herdr injects `HERDR_WORKSPACE_ID`, `HERDR_PANE_ID`, `HERDR_TAB_ID`, and `HERDR_SOCKET_PATH` into your pane. Locate project tools via PATH.

## Isolation — the herdr way, not `git worktree add`

When work needs a branch/worktree, let herdr create it — it opens a fresh pane in the worktree with the standard layout:

```bash
herdr worktree create --cwd <repo-root> --branch <type>/<name>   # type ∈ feat|fix|chore|deps
```

`--cwd` is required — herdr does not resolve the repo from your shell's cwd, and can pick the wrong workspace without it.

## Dispatch work to a fresh agent

To run a substantial or parallel task in its own pane so this session stays a clean orchestrator:

```bash
herdr worktree create --cwd <repo-root> --branch <type>/<name> --no-focus   # → JSON; read .result.root_pane.pane_id
herdr agent prompt <pane_id> "<task>"        # the pane auto-launches its agent; just prompt it
herdr agent wait <pane_id> --until done      # only when a chained step needs the result
```

After dispatching, step back — the dispatched session talks to the human directly. Don't mirror its output back here unless the human asks, or a chained next step needs its result.

Worktrees are disposable — remove them once the branch is merged or abandoned. Merge via PR; prefer linear history.
