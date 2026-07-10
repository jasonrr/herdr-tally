---
name: tally
description: "Project todos & scratchpads shared with the human in herdr. Use to record plans, handoffs, blockers, and working context that must outlive this session. CLI: `tally todos …` / `tally scratchpads …` / `tally comments …` (project inferred from cwd)."
---

# tally — agent skill

You share a project's **todos** and **scratchpads** with the human. Both are
project-scoped (inferred from the current git repo) and render live in herdr panes,
so anything you write, the human sees immediately.

## When to use

- **Scratchpad** — before executing a multi-step task, write the plan as a scratchpad.
  Update it as you go. Use it for handoffs ("where I left off"), snippets, and context
  too large for a todo.
- **Todo** — one per follow-up, blocker, or piece of work you can't finish now. Mark
  `in_progress` while working it, `complete` when done.
- **Lock** a todo (`tally todos lock <id>`) while you're actively editing the work
  it describes, so the human and other agents know it's taken. Unlock or complete when done.
- **Comment** — leave a margin note on a todo, scratchpad, or plan to flag a decision,
  a blocker, or context for whoever picks the item up next. Read what's accrued with
  `tally comments recent` (across everything) or `tally comments targets` (which items
  carry notes).

## Todos

```bash
tally todos create --title "Rotate refresh tokens" --priority high --tag auth
tally todos list --status open --json
tally todos update <id> --status in_progress
tally todos add-blocker <id> --blocker <other-id>   # can't start until <other-id> done
tally todos complete <id>
```
`--json` gives machine-readable output. `list` filters: `--status`, `--priority`,
`--is-blocked true`, `--query`, `--tag` (repeatable), `--sort priority`.

## Scratchpads

```bash
tally scratchpads create --name "Auth refactor plan" --content-file -   # reads stdin
tally scratchpads read <id> --mode headings          # outline first, then...
tally scratchpads read <id> --mode section --section-heading "Step 2"
tally scratchpads append-section <id> --heading "Progress" --content "done X" --expected-revision <r>
```
Scratchpad writes are **revision-guarded**: `read` returns the current `revision`;
pass it as `--expected-revision` on your next write. If you get a revision-mismatch,
re-read and retry — someone else edited it. Prefer `append`/`append-section`/`edit`
over rewriting the whole pad.

## Comments

```bash
tally comments add t_abc --body "hold off — waiting on the auth PR"     # target: t_… | s_… | plan path
tally comments add s_xyz --body "spike looks good" --section "Phase 2"  # anchor to a heading
tally comments list t_abc --json
tally comments recent --since 2h --json        # newest-first across ALL targets (default 24h)
tally comments targets --json                  # every item with notes: count + latest snippet
tally comments delete c_123
```
A comment attaches to a **target**: a todo (`t_…`), a scratchpad (`s_…`), or a plan
file (its repo-relative path). `recent` defaults to the last 24h — narrow it with
`--since 30m`/`2h`/`1d`, filter with `--author`, and widen past notes to auto-logged
status changes with `--include-events`. `targets` groups by item so you can see which
ones have unread context. Both views are notes-only by default.

## Etiquette

- Keep todo titles short; put detail in the body or a linked scratchpad.
- A comment is a note in the margin, not a state change — use a todo's `status`/`lock`
  for state, comments for the *why*.
- Don't delete the human's todos/scratchpads; archive scratchpads instead.
- Complete todos you finish — a stale open list is worse than none.
