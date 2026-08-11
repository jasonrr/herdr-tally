---
name: setup
description: One-time, consent-gated setup of the tally dev loop for this repo — asks before enabling session routing, proposes repo-specific reviewer lenses, then writes .claude/tally-dev-loop.md. Use when the user asks to set up, enable, or configure the dev loop, or when another tally skill suggested it.
---

# Setup

You are configuring, never assuming. This skill writes exactly one file — `.claude/tally-dev-loop.md` at the repo root — and only after the user approves its exact content. Never touch `.claude/settings.json`, global config, or anything else. Every question is one AskUserQuestion.

## 1. Current state

If `.claude/tally-dev-loop.md` already exists, show it and ask what to change — routing on/off, lens edits — then apply just that and stop. The full flow below is for first-time setup.

## 2. Routing — ask first

Show the user this exact rule — the block the hook injects (kept in sync with `hooks/session-start.sh` by hand):

    <dev-loop>
    This project uses the tally dev loop. Standing routing — the human should never have to ask:
    - Feature or non-trivial change requested → run /tally:brainstorm before writing any code.
    - Bug, failing test, or unexpected behavior → run /tally:debug before proposing any fix.
    - Approved design (tally scratchpad tagged design) or any multi-step implementation → run /tally:plan before building.
    - A plan doc with plan:<slug> todos exists → execute it with /tally:build.
    - Branch complete, or a merge/PR is requested → run /tally:review-branch first.
    Trivial one-file changes and pure questions are exempt. When this rule triggers a skill, say so in one line.
    </dev-loop>

And the mechanism in one sentence: the tally plugin's SessionStart hook emits this into every session in this repo, only while this file says `routing: on`; deleting the file or setting `routing: off` disarms it. Then ask: enable routing? Decline → the file gets `routing: off`; they can still want lenses.

## 3. Reviewer lenses — scan, propose, confirm

Read the repo before proposing: CLAUDE.md (especially invariants / gotchas / "do not fix" sections), migration dirs or persisted formats, auth and input-parsing surfaces, protocol surfaces (MCP, APIs), UI/TUI code. Propose 2–4 lenses, one line each — `name: charter`, where the charter says what to attack and what counts as p1 for that lens. Always include `correctness` and `simplicity` (simplicity findings cap at p2). Add an `invariants` lens whenever the repo documents frozen contracts — its charter IS that list, compressed. Confirm with AskUserQuestion (multiSelect, user can edit via Other).

## 4. Write and recap

Show the complete file content, get one final yes, then write `.claude/tally-dev-loop.md`:

    # dev loop
    routing: on

    ## Lenses
    - correctness: ...
    - invariants: ...
    - simplicity: ...

Recap in three lines: what was written and where; that `rm .claude/tally-dev-loop.md` (or `routing: off`) undoes everything; that the tally TUI footer now reflects this state. Suggest committing the file so the whole team gets the same loop.
