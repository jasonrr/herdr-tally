---
name: build
description: Execute a plan task-by-task with fresh subagents — TDD, per-task review, tally todos as the ledger. Runs inside its own worktree from a plan:<slug> scratchpad, opens a PR, then review. Use after /tally:plan (usually dispatched into a worktree), or directly on any plan:<slug> todos.
---

# Build

You are the controller: dispatch, verify, adjudicate. Subagents implement. Work on a branch or worktree, never main. Usually `/tally:plan` has already dispatched you into a fresh worktree — if you are in a linked worktree (`.git` is a file) or on a non-main `<type>/<slug>` branch, work in place. Only when invoked directly on `main` in the main checkout do you isolate yourself — `EnterWorktree` onto a `<type>/<slug>` branch and run the loop in-session, in herdr or not. Do NOT `herdr worktree create` here: that spawns a whole workspace whose auto-launched agent you would then orphan by working inline. The herdr worktree *space* is `/tally:plan`'s dispatch hand-off, not something build re-creates.

Input: the `plan:<slug>` scratchpad **id** (from your dispatch brief) and its `plan:<slug>` tally todos. If neither is in context, `scratchpad_find` the most recent `plan:`-tagged scratchpad, `todo_list` its tag, and confirm with the user before starting.

**First, materialize the plan and the design.**

1. Write the `plan:<slug>` scratchpad to `docs/plans/YYYY-MM-DD-<slug>.md` (the scratchpad's creation date, the branch slug).
2. Write the `design` scratchpad the plan's header names to `docs/decisions/YYYY-MM-DD-<slug>.md` — same basename as the plan file. That file is the feature's decision record; /tally:review-branch appends the build's deviations to it. If the plan names no design scratchpad, skip this step.

Both steps: pass `scratchpad_save_to_file` an **absolute** path under `git rev-parse --show-toplevel`, and `mkdir -p` the directory first — the tool creates no parent directories, and a relative path resolves against the MCP server's cwd and lands in the main checkout, not this worktree. Commit both files with explicit paths — they ride the feature's PR to main, and the plan shows in the Plans tab on merge. In-flight they stayed scratchpads because a `docs/` file in a worktree is invisible to the main checkout until merged.

**Then seed the dev-loop config.** `.claude/tally-dev-loop.md` (routing + review lenses) is gitignored, so a fresh worktree checkout lacks it and the per-task review below would silently fall back to default lenses. If it's absent here but present in the main worktree, copy it in — a no-op when it already exists or the repo has no dev loop: `[ -f .claude/tally-dev-loop.md ] || { root="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"; [ -f "$root/.claude/tally-dev-loop.md" ] && mkdir -p .claude && cp "$root/.claude/tally-dev-loop.md" .claude/tally-dev-loop.md; }` (the copy stays gitignored — it never enters the PR).

## Per task, in blocker order, one at a time

1. **Cut check.** Reread the task's "Exists because" line against the code as it stands now. If it no longer holds, or a dumber path has appeared since planning, surface that to the user instead of building — plans go stale as earlier tasks land, and a wasted task costs its full implement+review cycle.
2. **Claim.** `todo_lock` + status `in_progress`, so the user's TUI shows who has it.
3. **Implement.** Dispatch a fresh subagent with an explicit `model` — the tier the plan names, `sonnet` if unstated. Never omit the model: an omitted model inherits the session's, usually the most expensive. Implementers dispatch through the Agent tool (cheap, model-pinned, serialized on one tree) — never a herdr pane; the workspace-level dispatch that `/tally:plan` already did is the only pane hand-off in the loop. The brief is self-contained (repo path, the task's text with its code and interfaces, verification commands, commit instructions) — never session history, never the whole plan. The brief includes:
   - TDD: write the failing test first, run it, confirm it fails for the right reason, then minimal code to green. No production code without a failing test. A bug found mid-task gets a failing test reproducing it before the fix.
   - Hygiene during implementation, not review: types, lint clean, no dead code, match surrounding style.
   - Commit with explicit paths (`git add <paths>`, never `-A`); never a bare `git stash` (the stash stack is shared across all worktrees/sessions — a stray stash/pop eats another session's WIP); no AI references in messages.
   - Stop conditions: the live code contradicts the brief, verification fails twice after a fix attempt, or the work needs files outside those named — report back rather than improvise.
4. **Verify yourself.** Run the task's verification command and read the diff. The subagent's report is a claim, not evidence.
5. **Review.** Dispatch a fresh reviewer subagent (`sonnet`) with the diff, the task brief, and the severity definitions from /tally:review-branch (p1 = data loss, invariant violation, security, mainline breakage; p2 = edge wrongness, missing I/O error handling; p3 = hygiene) — plus the lens charters from `.claude/tally-dev-loop.md` if it exists. Every finding must be a concrete failure scenario (input/state → wrong behavior); verify each to CONFIRMED (reproduced/traced) or PLAUSIBLE (couldn't refute), and drop a finding only by showing it's wrong. Fix rounds go back to the implementer, capped at 3; past the cap you adjudicate each open finding yourself — fix it, or park it with a ruling recorded as a tally comment on the todo. Nothing is dropped silently.
6. **Record.** `todo_complete`, plus one `comment_add` on the todo: `commit <hash> — deviation: <what left the plan and why, or none> — parked: <todo ids or none>`. The todos are the ledger — keep no separate log scratchpad. A resumed session reads `todo_list` for `plan:<slug>` and re-dispatches nothing that is `completed`; a todo still `in_progress` has no comment yet, so treat it as unfinished: check `git log` and `git status` for work since its lock time, and dispatch again only for what is missing. The deviation comments are what /tally:review-branch collects into the decision record. **If the task surprised you** — a plan defect, a tool or classifier refusal, a verify command that could not pass at this task, a finding you had to park — also append one `pattern → consequence` bullet to `docs/learnings.md` in this worktree, under a dated heading for this work: `## <slug> (PR #<n>, YYYY-MM-DD)`, blank line before the heading, one bullet per line. If the file is missing, create it with a `# Learnings` H1 and first copy in every bullet from every `learnings`-tagged tally scratchpad, oldest scratchpad first; archive a scratchpad only after all its bullets are in the committed file. The PR doesn't exist yet mid-build — write `PR #pending` and reuse that one heading for every surprise in this build; review-branch fills the number in. Commit the file with an explicit path. The todo comment says what happened here; learnings says what the next planner should not repeat.

One implementer at a time; parallel implementers on one tree conflict.

When every task is complete, open a PR (`gh pr create`, let gh detect the remote), then run /tally:review-branch against it. Review fixes land on the PR as follow-up commits.

**Last action: report to your dispatcher.** If your brief named a dispatcher pane, your final act
on **every** exit path — all tasks done, blocked awaiting the operator, stopped by a stop
condition, or abandoned — is the one-line report the brief specified:

    herdr agent prompt <dispatcher-pane-id> "<slug>: <done|blocked|failed> — PR #<n> <url> | suite <N> OK | parked: <ids or none> | needs human: <one line or none>"

Send it once, last, after the PR and review-branch — **once total**, not once per skill: if
/tally:review-branch already sent a `blocked` report when it stopped at its gate, that was the
report, and you send nothing more. Exiting without any report is the failure this contract
exists to prevent: a silent exit looks exactly like a build still running. If your brief named
no dispatcher pane you were invoked directly — no report, everything else unchanged.
