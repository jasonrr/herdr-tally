---
name: herdr-notes
description: "Project todos & scratchpads shared with the human in herdr. Use to record plans, handoffs, blockers, and working context that must outlive this session. CLI: `herdr-notes todos …` / `herdr-notes scratchpads …` (project inferred from cwd)."
---

# herdr-notes — agent skill

You share a project's **todos** and **scratchpads** with the human. Both are
project-scoped (inferred from the current git repo) and render live in herdr panes,
so anything you write, the human sees immediately.

## When to use

- **Scratchpad** — before executing a multi-step task, write the plan as a scratchpad.
  Update it as you go. Use it for handoffs ("where I left off"), snippets, and context
  too large for a todo.
- **Todo** — one per follow-up, blocker, or piece of work you can't finish now. Mark
  `in_progress` while working it, `complete` when done.
- **Lock** a todo (`herdr-notes todos lock <id>`) while you're actively editing the work
  it describes, so the human and other agents know it's taken. Unlock or complete when done.

## Todos

```bash
herdr-notes todos create --title "Rotate refresh tokens" --priority high --tag auth
herdr-notes todos list --status open --json
herdr-notes todos update <id> --status in_progress
herdr-notes todos add-blocker <id> --blocker <other-id>   # can't start until <other-id> done
herdr-notes todos complete <id>
```
`--json` gives machine-readable output. `list` filters: `--status`, `--priority`,
`--is-blocked true`, `--query`, `--tag` (repeatable), `--sort priority`.

## Scratchpads

```bash
herdr-notes scratchpads create --name "Auth refactor plan" --content-file -   # reads stdin
herdr-notes scratchpads read <id> --mode headings          # outline first, then...
herdr-notes scratchpads read <id> --mode section --section-heading "Step 2"
herdr-notes scratchpads append-section <id> --heading "Progress" --content "done X" --expected-revision <r>
```
Scratchpad writes are **revision-guarded**: `read` returns the current `revision`;
pass it as `--expected-revision` on your next write. If you get a revision-mismatch,
re-read and retry — someone else edited it. Prefer `append`/`append-section`/`edit`
over rewriting the whole pad.

## Etiquette

- Keep todo titles short; put detail in the body or a linked scratchpad.
- Don't delete the human's todos/scratchpads; archive scratchpads instead.
- Complete todos you finish — a stale open list is worse than none.
