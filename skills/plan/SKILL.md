---
name: plan
description: Turn an approved design into an executable plan — a plan doc in docs/plans/ (surfaced in tally's Plans tab) plus one tally todo per task. Use after /tally:brainstorm, once a design scratchpad exists.
---

# Plan

Write a plan a fresh agent with zero context can execute. Every ambiguity you leave becomes a wrong guess by an implementer that never saw this conversation.

A design is required input. If no `design`-tagged scratchpad is in context, `scratchpad_find` one and confirm it with the user; if none exists, run /tally:brainstorm first — do not write a plan from a cold request.

## The plan doc

Write to `docs/plans/YYYY-MM-DD-<slug>.md` — tally surfaces this directory in its Plans tab, so the user reads it beside the live todos. Header: goal, design scratchpad id, branch name, and the verification command for the whole feature. If the design scratchpad isn't already in context, find it with `scratchpad_find` (tag `design`, most recent) and confirm with the user it's the right one.

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

Create one tally todo per task, tagged `plan:<slug>`, body pointing at the plan doc section. Encode ordering with blockers (`todo_add_blocker`). The plan doc is the narrative; todo state is the live ledger you and the user both watch.

## Before handing off

First reread the plan yourself for placeholders and interface mismatches. Then dispatch one fresh `sonnet` subagent whose only input is the plan doc path: "You are the zero-context implementer. List every place you would have to guess — missing file paths, undefined interfaces, placeholder steps, decisions the plan assumes you know." Every guess it returns is a plan defect: fix it in the doc before creating the tally todos. Then ask the user: execute via /tally:build (fresh subagent per task) or inline in this session.
