---
name: setup
description: One-time, consent-gated setup of the tally dev loop for this repo — asks before enabling session routing, proposes repo-specific reviewer lenses, then writes .claude/tally-dev-loop.md. Use when the user asks to set up, enable, or configure the dev loop, or when another tally skill suggested it.
---

# Setup

You are configuring, never assuming. This skill writes exactly one file — `.claude/tally-dev-loop.md` at the repo root — and only after the user approves its exact content. Never touch `.claude/settings.json`, global config, or anything else. Every question is one AskUserQuestion.

## 1. Current state

If `.claude/tally-dev-loop.md` already exists, show it and ask what to change — routing on/off, builder model, lens edits — then apply just that and stop. A file written before the `builder-model` key existed simply lacks it; offer to add it. The full flow below is for first-time setup.

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

## 3. Builder model — set the default

`/tally:plan` dispatches a build controller into its own pane. Launched bare, a Claude-kind
agent takes its model from the user's global `~/.claude/settings.json`, so the controller lands
on whatever tier that names — usually the most expensive one. The controller only dispatches,
verifies and adjudicates; the plan already pins a model per task for the implementers who do
the work.

Ask (one AskUserQuestion) which model build controllers should default to: `opus` (recommended
default), `sonnet` (right when plans pin every task to sonnet or haiku), or `fable`. Write the
answer as `builder-model: <value>`.

Say what the key is, in one line: it is the **default offered at dispatch, not a lock** —
`/tally:plan` shows it prefilled and the human can override it per build. Only Claude- and
Pi-kind panes consume it; other agent kinds launch bare, as today.

## 4. Reviewer lenses — scan, propose, confirm

Read the repo before proposing: CLAUDE.md (especially invariants / gotchas / "do not fix" sections), migration dirs or persisted formats, auth and input-parsing surfaces, protocol surfaces (MCP, APIs), UI/TUI code. Propose 2–4 lenses, one line each — `name: charter`, where the charter says what to attack and what counts as p1 for that lens. Always include `correctness` and `simplicity` (simplicity findings cap at p2). Add an `invariants` lens whenever the repo documents frozen contracts — its charter IS that list, compressed. Confirm with AskUserQuestion (multiSelect, user can edit via Other).

## 5. Write and recap

Show the complete file content, get one final yes, then write `.claude/tally-dev-loop.md`:

    # dev loop
    routing: on
    builder-model: opus

    ## Lenses
    - correctness: ...
    - invariants: ...
    - simplicity: ...

Recap in three lines: what was written and where; that `rm .claude/tally-dev-loop.md` (or `routing: off`) undoes everything; that the tally TUI footer now reflects this state.

Then ask (one AskUserQuestion) whether to gitignore or commit the file:

- **Gitignore (default)** — add `.claude/tally-dev-loop.md` to `.gitignore`. The loop stays personal to this checkout. This is the safe default because committing it means every teammate who *also* has the tally plugin gets the `<dev-loop>` block injected into their sessions on clone — arming routing for them without their own consent, which is exactly what the rest of this setup is careful to ask about first. (For a purely local ignore that doesn't touch a shared `.gitignore`, use `.git/info/exclude` instead.)
- **Commit** — the whole team shares one loop. Choose this only when everyone works in this repo with the tally plugin and wants the same routing + lenses. Teammates without the plugin just see inert markdown.
