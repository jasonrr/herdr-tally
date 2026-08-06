---
name: plan
description: Turn an approved design into an executable plan — a plan doc in docs/plans/ (surfaced in tally's Plans tab) plus one tally todo per task. Use after /brainstorm, or whenever a multi-step implementation needs writing down before building.
---

# Plan

Write a plan a fresh agent with zero context can execute. Every ambiguity you leave becomes a wrong guess by an implementer that never saw this conversation.

## The plan doc

Write to `docs/plans/YYYY-MM-DD-<slug>.md` — tally surfaces this directory in its Plans tab, so the user reads it beside the live todos. Header: goal, design scratchpad id, branch name, and the verification command for the whole feature. If the design scratchpad isn't already in context, find it with `scratchpad_find` (tag `design`, most recent) and confirm with the user it's the right one.

## Tasks

Size each task so it is independently implementable and reviewable — a reviewer could approve it while rejecting its neighbor. Each task states:

- **Exists because:** one line naming what breaks without it, or why the dumb alternative fails. If you can't write that line, the task shouldn't exist — cut it now, not after it's built. A wasted task costs a full implement+review cycle.
- **Model:** `haiku` only for mechanical transcription where the plan already contains the complete code; `sonnet` for everything else (the default); `opus` only when the task is itself architectural. /build dispatches with exactly this.
- **Files** touched, and **interfaces** consumed from / produced for neighboring tasks.
- **Steps, test-first:** the failing test to write, then the minimal implementation. Include complete code where you already know it. Placeholders — "TBD", "add appropriate error handling", "similar to task 2" — are plan defects; resolve them here.
- **Verify:** the exact command that proves this task done (usually the scoped test invocation) — /build runs it after the implementer reports.

## Tally ledger

Create one tally todo per task, tagged `plan:<slug>`, body pointing at the plan doc section. Encode ordering with blockers (`todo_add_blocker`). The plan doc is the narrative; todo state is the live ledger you and the user both watch.

## Before handing off

Reread the plan as the zero-context implementer: scan for placeholders, check interfaces line up between tasks, confirm every decision from the design made it in. Then ask the user: execute via /build (fresh subagent per task) or inline in this session.
