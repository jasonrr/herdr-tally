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

Dispatch one Explore subagent before writing any task: for each major task area, find the nearest existing in-repo pattern to follow; skim `learnings`- and `build-log:`-tagged tally scratchpads for prior art and past failures. Cite what you follow in the plan ("follows the pattern in src/…"). A novel approach where a pattern exists, uncited, is a plan defect. A fact verified for one harness, tool, or caller does not transfer to another — each named consumer of a config key or interface needs its own citation.

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

First reread the plan yourself for placeholders and interface mismatches. Then dispatch **two** fresh `sonnet` subagents in parallel, each given only the plan scratchpad id. Every finding either returns is a plan defect — fix them all before creating the tally todos.

1. **Ambiguity (zero-context implementer):** "List every place you would have to guess — missing file paths, undefined interfaces, placeholder steps, decisions the plan assumes you know."
2. **Correctness/invariants (adversary):** brief it with the repo's lens charters from `.claude/tally-dev-loop.md` when present, else correctness + invariants. "This plan is confident and may be confidently wrong. For each task: name every documented invariant the task touches, and trace one concrete failure sequence — state/input → wrong behavior — through the plan's own code. Review the plan's code as shipped code; the implementer will transcribe it verbatim." Severity definitions and the probe-log rule are /tally:review-branch's: zero findings is legal only with a probe log naming which invariants were checked per task.

The ambiguity pass cannot find a plan that is unambiguous and wrong, and that is where the expensive defects live — a design error in the plan's own code reaches the implementer as an instruction not to deviate.

Then hand off to the build:

- **In herdr (`HERDR_ENV=1`):** one AskUserQuestion carrying two questions. First: dispatch to a new worktree / build inline here / defer. Second, "builder model?" — offer the repo's configured default first, with `(default)` in its label, then the other two of `opus` / `sonnet` / `fable`. Read the default from `.claude/tally-dev-loop.md` (`grep '^builder-model:'`), falling back to `opus` when the key or the file is absent. One prompt, two questions — never a second round trip. The model answer applies only to the Dispatch branch; on Inline or Defer, ignore it.
  - **Dispatch:** preserve the caller's CLI *family*; take the model from the answer to the builder-model question above, never from the caller's session. Before creating the worktree, read the current kind from `herdr agent get "$HERDR_PANE_ID"` at `.result.agent.agent`; for Pi also retain `PI_PROVIDER` + `PI_MODEL`. Run `herdr worktree create --cwd <repo-root> --branch <type>/<slug> --label "<title>" --no-focus`; read `.result.root_pane.pane_id` (workspace at `.result.workspace_id`, checkout at `.result.worktree.path`). Always exit its auto-launched agent with `herdr agent send-keys <pane-id> ctrl+d`, confirm the pane is back at a shell, then `herdr agent start build-<slug> --kind <caller-kind> --pane <pane-id>`, appending an agent-arg tail chosen by kind: **claude** → `-- --model <chosen-builder-model>`; **pi** → `-- --provider "$PI_PROVIDER" --model "$PI_MODEL"`, requiring both non-empty; **any other kind** → no tail, it launches bare as today. `herdr agent start` accepts the tail after `--` (`[-- [AGENT_ARG]...]`, verified live against herdr 0.9.0). Then `herdr agent prompt <pane-id> "<brief>"` with a self-contained brief: repo path (the worktree checkout), branch, the `plan:<slug>` scratchpad **id**, the whole-feature verify command, and one instruction — "run /tally:build for `plan:<slug>` through completion." Build owns the rest (materialize the plan file, implement task-by-task, open the PR, run review-branch, push fixes). No session history in the brief. The brief also carries the return address and report line from **Dispatch contract** below — a dispatch without one is the bug this contract exists to prevent. Then write the `dispatch-log` line (below) and **step back** — say so; the space's agent owns it and talks to the user directly. Don't `herdr agent wait` on it or mirror the pane; progress rides the tally todos + `build-log:<slug>` scratchpad until the child reports.
  - **Inline:** proceed to /tally:build in this session.
  - **Defer:** stop; the `plan:<slug>` scratchpad + todos persist for later.
- **Outside herdr:** `EnterWorktree`, then /tally:build inline.

## Dispatch contract

A dispatched build is fire-and-**report**, not fire-and-forget. You still never poll it: no
`herdr agent wait`, no reading its pane to mirror progress. The child pushes exactly once, at
its final exit. (The worktree-dispatch design rejected *the dispatcher pulling*; this is *the
child pushing*. Don't "fix" it back to silence.)

**Put a return address and the exact report line in the brief.** Append to the dispatch brief,
substituting your own `$HERDR_PANE_ID` for `<dispatcher-pane-id>`:

    Report back when you finish, on every exit path — done, blocked awaiting the operator,
    stopped by a stop condition, or abandoned. Your last action is:
    herdr agent prompt <dispatcher-pane-id> "<slug>: <done|blocked|failed> — PR #<n> <url> | suite <N> OK | parked: <ids or none> | needs human: <one line or none>"

Name `herdr agent prompt` specifically. It is harness-agnostic and works from a background pane,
where `agent wait` and `tab close` can be refused by the auto-mode classifier. A Claude-kind
child may also use its native SendMessage, but the brief names `agent prompt`.

**Write the dispatch-log line before you step back.** Append one line per dispatch to a
`dispatch-log`-tagged tally scratchpad (create it if missing): slug, worktree pane id, workspace
id, worktree path, sent-at, expected report. This is what a resumed or compacted session reads
to know what is still outstanding — the same job `build-log:<slug>` does for tasks inside a
build. Written after stepping back it is too late; a compaction in between loses the dispatch.

**When a report arrives, four things in order:**

1. **Verify the claim.** A report is a claim, not evidence — the same rule build applies to its
   implementers. Check the PR (`gh pr view <n> --json state,mergedAt,statusCheckRollup`) and, if
   it claims merged, that `main` actually contains it.
2. **Append the outcome** to `dispatch-log`: what arrived, what you verified.
3. **Clean up the space you opened** — the rule below.
4. **Tell the user, in one line.** They asked for the work; the report is not for you alone.

**Cleanup rule — destructive, read it before running anything.** You opened the worktree
workspace, so you close it — but only once the branch is merged or explicitly abandoned, and
only the workspace *you* created for this slug (its id is in your `dispatch-log` line). Never
close a workspace you did not open. Never close on a `blocked` report: the operator may still
need that pane. Order: confirm merged or abandoned → `herdr workspace close <workspace-id>` →
`herdr worktree remove` for the checkout. `herdr workspace close` takes a workspace id and
nothing else — verified live against herdr 0.9.0. Its release notes mention a `--group` flag
for closing a primary workspace together with its worktree children; that flag is NOT in the
0.9.0 CLI. Passing it is read as the workspace id and fails with `workspace_not_found`. Close
the child workspace by id; never reach for a group flag. If anything is ambiguous, leave the
space open and say so. An orphaned pane is cheap; a closed pane holding unpushed work is not.


**If a report never arrives,** you may schedule exactly one non-blocking check per dispatch — a
Monitor on the pane's agent status, or a wakeup sized to the plan's expected duration. Never a
blocking `herdr agent wait`, never reading the pane to mirror progress.
