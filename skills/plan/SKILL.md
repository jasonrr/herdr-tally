---
name: plan
description: Turn an approved design into an executable plan — authored as a plan:<slug> scratchpad (surfaced beside the live todos) plus one tally todo per task; in herdr, dispatches the build into its own worktree. Use after /tally:brainstorm, once a design scratchpad exists.
---

# Plan

Write a plan a fresh agent with zero context can execute. Every ambiguity you leave becomes a wrong guess by an implementer that never saw this conversation.

A design is required input. If no `design`-tagged scratchpad is in context, `scratchpad_find` one and confirm it with the user; if none exists, run /tally:brainstorm first — do not write a plan from a cold request.

## The plan (scratchpad)

Author the plan as a **`plan:<slug>` scratchpad** (`scratchpad_write`, tag `plan:<slug>`) — not a `docs/plans/` file. It is the narrative a fresh agent executes, and the shared store carries it across the worktree boundary the build runs in (a `docs/plans` file written in a worktree is invisible to the Plans tab until it merges to main). Header: goal, design scratchpad id, branch name (`<type>/<slug>`, type ∈ feat|fix|chore), and the verification command for the whole feature. `/tally:build` materializes this scratchpad to a committed `docs/plans/YYYY-MM-DD-<slug>.md` in the worktree, so it reaches the Plans tab on merge; in-flight it lives in the Scratchpads tab.

## Research before tasks

Dispatch one Explore subagent before writing any task: for each major task area, find the nearest existing in-repo pattern to follow; skim `learnings`- and `build-log:`-tagged tally scratchpads for prior art and past failures. Cite what you follow in the plan ("follows the pattern in src/…"). A novel approach where a pattern exists, uncited, is a plan defect.

## Tasks

Size each task so it is independently implementable and reviewable — a reviewer could approve it while rejecting its neighbor. Each task states:

- **Exists because:** one line naming what breaks without it, or why the dumb alternative fails. If you can't write that line, the task shouldn't exist — cut it now, not after it's built. A wasted task costs a full implement+review cycle.
- **Model:** `haiku` only for mechanical transcription where the plan already contains the complete code; `sonnet` for everything else (the default); `opus` only when the task is itself architectural. /tally:build dispatches with exactly this.
- **Files** touched, and **interfaces** consumed from / produced for neighboring tasks.
- **Steps, test-first:** the failing test to write, then the minimal implementation. Include complete code where you already know it. Placeholders — "TBD", "add appropriate error handling", "similar to task 2" — are plan defects; resolve them here.
- **Verify:** the exact command that proves this task done (usually the scoped test invocation) — /tally:build runs it after the implementer reports.

## Tally ledger

Create one tally todo per task, tagged `plan:<slug>`, body pointing at the scratchpad section. Encode ordering with blockers (`todo_add_blocker`). The scratchpad is the narrative; todo state is the live ledger you and the user both watch.

## Before handing off

First reread the plan yourself for placeholders and interface mismatches. Then dispatch one fresh `sonnet` subagent whose only input is the plan scratchpad id: "You are the zero-context implementer. List every place you would have to guess — missing file paths, undefined interfaces, placeholder steps, decisions the plan assumes you know." Every guess it returns is a plan defect: fix it before creating the tally todos.

Then hand off to the build:

- **In herdr (`HERDR_ENV=1`):** one AskUserQuestion — dispatch to a new worktree / build inline here / defer.
  - **Dispatch:** `herdr worktree create --cwd <repo-root> --branch <type>/<slug> --label "<title>" --no-focus`; read `.result.root_pane.pane_id` (workspace at `.result.workspace_id`, checkout at `.result.worktree.path`). Then `herdr agent prompt <pane-id> "<brief>"` with a self-contained brief: repo path (the worktree checkout), branch, the `plan:<slug>` scratchpad **id**, the whole-feature verify command, and one instruction — "run /tally:build for `plan:<slug>` through completion." Build owns the rest (materialize the plan file, implement task-by-task, open the PR, run review-branch, push fixes). No session history in the brief. Then **step back** — say so; the space's agent owns it and talks to the user directly. Don't `herdr agent wait` on it or mirror the pane; progress rides the tally todos + `build-log:<slug>` scratchpad.
  - **Inline:** proceed to /tally:build in this session.
  - **Defer:** stop; the `plan:<slug>` scratchpad + todos persist for later.
- **Outside herdr:** `EnterWorktree`, then /tally:build inline.
