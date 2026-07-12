---
name: tally
description: "Project todos & scratchpads shared with the human in herdr. Use to record plans, handoffs, blockers, and working context that must outlive this session. Interface: the `tally_*` MCP tools (`todo_*` / `scratchpad_*` / `comment_*`) — ToolSearch to load them; project inferred from cwd."
---

# tally — agent skill

You share a project's **todos**, **scratchpads**, **plans**, and **comments** with
the human. Everything is project-scoped (inferred from the current git repo) and
renders live in herdr panes, so anything you write, the human sees immediately.

## Interface

Use the **`tally_*` MCP tools** — `todo_*`, `scratchpad_*`, `comment_*`. If they're
not already loaded, `ToolSearch` for `mcp__tally__` to pull in the ones you need; the
tool schemas document their own fields, and every create/list call returns the item
`id` you'll pass to later calls. (A `tally` CLI mirrors these tools for scripts, hooks,
and other non-MCP callers — you don't need it here.)

## When to use which

- **Scratchpad** — before executing a multi-step task, write the plan as a scratchpad
  and update it as you go. Also for handoffs ("where I left off"), snippets, and context
  too large for a todo. Writes are **revision-guarded**: a read returns the current
  `revision`; pass it back on your next write, and if you get a mismatch, re-read and
  retry — someone else edited it. Prefer append/append-section/edit over rewriting whole.
- **Todo** — one per follow-up, blocker, or piece of work you can't finish now. Set it
  `in_progress` while working, `completed` when done. Status is only `open` /
  `in_progress` / `completed`; priority only `high` / `medium` / `low` — anything else
  is rejected.
- **Lock** a todo while you're actively editing the work it describes, so the human and
  other agents know it's taken. Unlock or complete when done.
- **Comment** — a margin note on a todo, scratchpad, or plan, flagging a decision or
  context for whoever picks the item up next. It's a note, *not* a state change — use a
  todo's status/lock for state, comments for the *why*. Read what's accrued with the
  recent/targets tools.

## Etiquette

- Keep todo titles short; put detail in the body or a linked scratchpad.
- Don't delete the human's todos/scratchpads; archive scratchpads instead.
- Complete todos you finish — a stale open list is worse than none.
