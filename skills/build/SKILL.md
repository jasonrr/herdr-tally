---
name: build
description: Execute a plan task-by-task with fresh subagents — TDD, per-task review, tally todos as the ledger. Runs inside its own worktree from a plan:<slug> scratchpad, opens a PR, then review. Use after /tally:plan (usually dispatched into a worktree), or directly on any plan:<slug> todos.
---

# Build

You are the controller: dispatch, verify, adjudicate. Subagents implement. Work on a branch or worktree, never main. Usually `/tally:plan` has already dispatched you into a fresh worktree — if you are in a linked worktree (`.git` is a file) or on a non-main `<type>/<slug>` branch, work in place. Only when invoked directly on `main` in the main checkout do you create the isolation yourself: in herdr (`HERDR_ENV=1`) `herdr worktree create --cwd <repo-root> --branch <type>/<slug> --no-focus` then run the loop from that worktree; otherwise `EnterWorktree`.

Input: the `plan:<slug>` scratchpad **id** (from your dispatch brief) and its `plan:<slug>` tally todos. If neither is in context, `scratchpad_find` the most recent `plan:`-tagged scratchpad, `todo_list` its tag, and confirm with the user before starting.

**First, materialize the plan.** Write the `plan:<slug>` scratchpad to `docs/plans/YYYY-MM-DD-<slug>.md` (the scratchpad's creation date, the branch slug) and commit it with an explicit path — it rides the feature's PR to main and shows in the Plans tab on merge. In-flight it stayed a scratchpad because a `docs/plans` file in a worktree is invisible to the Plans tab until merged.

## Per task, in blocker order, one at a time

1. **Cut check.** Reread the task's "Exists because" line against the code as it stands now. If it no longer holds, or a dumber path has appeared since planning, surface that to the user instead of building — plans go stale as earlier tasks land, and a wasted task costs its full implement+review cycle.
2. **Claim.** `todo_lock` + status `in_progress`, so the user's TUI shows who has it.
3. **Implement.** Dispatch a fresh subagent with an explicit `model` — the tier the plan names, `sonnet` if unstated. Never omit the model: an omitted model inherits the session's, usually the most expensive. Default dispatch surface is the Agent tool (cheap, model-pinned, serialized on one tree). Dispatch to a herdr pane instead when the task's model is `opus` or the user asked to watch: `herdr pane split <pane-id> --direction right --no-focus` in the worktree tab, `herdr pane run <new-id> "claude --model <tier>"`, `herdr agent prompt <new-id> "<the same self-contained brief>"`, then `herdr agent wait <new-id> --until done --timeout 600000` before verifying. Same brief either way. Never read a pane mid-flight — completion signals ride tally (todo status + comments), not pane-scraping. The brief is self-contained (repo path, the task's text with its code and interfaces, verification commands, commit instructions) — never session history, never the whole plan. The brief includes:
   - TDD: write the failing test first, run it, confirm it fails for the right reason, then minimal code to green. No production code without a failing test. A bug found mid-task gets a failing test reproducing it before the fix.
   - Hygiene during implementation, not review: types, lint clean, no dead code, match surrounding style.
   - Commit with explicit paths (`git add <paths>`, never `-A`); never a bare `git stash` (the stash stack is shared across all worktrees/sessions — a stray stash/pop eats another session's WIP); no AI references in messages.
   - Stop conditions: the live code contradicts the brief, verification fails twice after a fix attempt, or the work needs files outside those named — report back rather than improvise.
4. **Verify yourself.** Run the task's verification command and read the diff. The subagent's report is a claim, not evidence.
5. **Review.** Dispatch a fresh reviewer subagent (`sonnet`) with the diff, the task brief, and the severity definitions from /tally:review-branch (p1 = data loss, invariant violation, security, mainline breakage; p2 = edge wrongness, missing I/O error handling; p3 = hygiene) — plus the lens charters from `.claude/tally-dev-loop.md` if it exists. Every finding must be a concrete failure scenario (input/state → wrong behavior); verify each to CONFIRMED (reproduced/traced) or PLAUSIBLE (couldn't refute), and drop a finding only by showing it's wrong. Fix rounds go back to the implementer, capped at 3; past the cap you adjudicate each open finding yourself — fix it, or park it with a ruling recorded as a tally comment on the todo. Nothing is dropped silently.
6. **Record.** `todo_complete`, plus one ledger line in a `build-log:<slug>` scratchpad: commits, deviations from plan, parked findings. Files survive context compaction — the ledger is what stops a resumed session from re-dispatching finished work.

One implementer at a time; parallel implementers on one tree conflict.

When every task is complete, open a PR (`gh pr create`, let gh detect the remote), then run /tally:review-branch against it. Review fixes land on the PR as follow-up commits.
