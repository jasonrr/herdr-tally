---
name: review-branch
description: Whole-branch adversarial review before merge — scoped reviewers probe the live tree, findings carry verdicts and defined severities, p1s fixed in one consolidated pass. Use when a feature branch is complete (after /tally:build) or before any merge or PR.
---

# Review branch

One deep pass over `merge-base..HEAD`, sized to the diff. A false finding costs one fix dispatch; a missed p1 ships a bug — calibrate to that asymmetry, not away from it.

## Severity — definitions, not vibes

- **p1 (blocks merge):** data loss or orphaned data, violation of a documented invariant, security (auth, injection, secrets exposure), user-visible breakage on a mainline path.
- **p2:** wrong on an edge the user will eventually hit; missing error handling around I/O; silent failure paths.
- **p3:** hygiene.

If `.claude/dev-loop.md` exists, its lens charters replace the default lenses below, and the repo's documented invariants are the p1 checklist.

## Reviewers

Dispatch 2–3 fresh subagents on `sonnet` — explicit model on every dispatch. Default lenses: **correctness** (always), **invariants** (whenever the repo documents frozen contracts — the charter is that list), **simplicity** (always; its findings cap at p2). Add **security** or **data integrity** only if the diff touches those surfaces.

Each brief is adversarial and live-tree:

- Give the worktree path and the diff path. The diff is a map; the tree is the territory.
- Open the charter with: "Find the inputs and states that break this."
- Required probing, not optional: run the scoped tests; execute the changed surface at least once (CLI, TUI, or MCP as applicable); trace the callers of every changed public function.
- Every finding is a concrete failure scenario — input/state → wrong behavior, file:line, proposed severity per the definitions above. Opinions without a failure scenario are not findings.
- Zero findings is a legal report ONLY with a probe log: what was executed, which edges were tried, which invariants were checked.
- A clean report without a probe log is incomplete — re-dispatch it.

In herdr (`HERDR_ENV=1`), run reviewers as panes so the human can watch the probing live: `herdr tab create --workspace <ws-id> --label review`, one `herdr pane split <pane-id> --direction right --no-focus` per extra reviewer, `herdr pane run <pane-id> "claude --model sonnet"` in each, then `herdr agent prompt <pane-id> "<brief>"`; `herdr agent wait <pane-id> --until done --timeout 600000` and read the final report with `herdr pane read <pane-id> --source recent --lines 100`. Outside herdr: Agent-tool fan-out with the same briefs.

## Verdicts

Verify each finding to a verdict, never to a silent rejection: **CONFIRMED** (you reproduced or traced it yourself) or **PLAUSIBLE** (you tried to refute it and couldn't). File both as tally todos — severity as priority, verdict in a comment. Drop a finding only when you can show it's wrong, and record why as a comment. Fix all p1s in ONE consolidated dispatch on `sonnet` — a fixer per finding costs more than the branch did — then one scoped re-review of that fix.

## Gate

Run the full test suite and lint fresh, in this session, and read the output. Append one line per surprise the review surfaced to a `learnings`-tagged tally scratchpad (`pattern → consequence`; create it if missing). Then present the real options — merge, push + PR, or leave the branch — and wait for the user's choice. The p2/p3 follow-ups live in tally, not in your head. If `.claude/dev-loop.md` doesn't exist, offer /tally:setup once in the closing summary.
