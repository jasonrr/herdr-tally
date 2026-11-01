# Design: dev loop leaves tally (2026-11-01)

Status: APPROVED 2026-11-01 by the human — option A (git files + draft PR ledger).

## Problem
The dev-loop skills (brainstorm, plan, build, review-branch, debug, setup) and the tally store are separate concerns, but the loop requires tally for every handoff. The loop should work in any repo with `gh`, with no tally binary, MCP server, or TUI.

## Answer to "can native gh/git objects replace tally here?"
Yes. Every tally use in the loop crosses one of three boundaries: session→session, main→worktree, or pre→post compaction. Git and GitHub already cover each:

| Today (tally) | Job | Native replacement |
|---|---|---|
| `design` scratchpad | brainstorm → plan handoff; becomes docs/decisions on build | **Write `docs/decisions/YYYY-MM-DD-<slug>.md` directly.** Brainstorm writes it in the main checkout (uncommitted). Plan reads the file. |
| `plan:<slug>` scratchpad | plan → build across the worktree boundary; becomes docs/plans on build | **Write `docs/plans/YYYY-MM-DD-<slug>.md` directly.** Plan copies both files into the new worktree before dispatch (plan already creates the worktree in both herdr and desktop paths); inline build via EnterWorktree does the same copy. Build's first commit is these two files, pushed, then `gh pr create --draft`. |
| `plan:<slug>` todos + blockers | live ledger, task order, resume | **Task list in the draft PR body** — one line per task, title only, linking to its section in the committed plan file: `- [ ] T3 — [title](docs/plans/YYYY-MM-DD-<slug>.md#task-3)`. The full task (steps, code, verify) stays in the plan file; the body stays ~10 lines. Order = list order (blockers were always linear). Build ticks a box per task with `gh pr edit --body`. Resume = read the body; unticked = not done. GitHub renders progress. |
| `todo_lock` / `in_progress` | "who has it" in the TUI | **Cut.** One builder per PR. The draft PR is the "in progress" signal. |
| per-task `comment_add` (commit / deviation / parked) | feeds `## Deviations` in the decision record | **Append to the `## Deviations` section of the decisions file as each task lands**, in the task's commit. Review-branch no longer collects comments. |
| `dispatch-log` scratchpad | what is still out, survives compaction (same machine only) | **Local gitignored file `.claude/dispatch-log.md`** in the main checkout, one line per dispatch, same fields. Never needed cross-machine. |
| review findings todos (p1 fixed; p2/p3 backlog) | tracking | **p1s**: fixed in the PR as today; verdicts in one PR comment (`gh pr comment`). **p2/p3**: `gh issue create --label p2|p3 --body …` with the PR URL in the body. |
| brainstorm deferred questions, debug follow-ups | backlog | `gh issue create`. |
| debug "ruled out" scratchpad | notes inside one investigation | **Ephemeral**: a `docs/learnings.md` bullet at the end, as debug already does; no store. |
| `learnings` scratchpads | legacy prior art | Already migrating to `docs/learnings.md`. The split ships with a one-time note: run the old migration once from tally before uninstalling; the new skills read only the file. |
| Plans / Scratchpads TUI tabs | human visibility in flight | **The draft PR is the page you watch.** Tally's Plans tab still shows merged `docs/plans/*.md` — unchanged, it just isn't required. |

**Defer branch of plan:** the two files stay uncommitted in the main checkout. Plan says so and offers `git stash`/commit-to-a-branch. (Cut: parking them in an issue — a rare path; files on disk are enough.)

**Requirement the new loop adds:** a GitHub remote and `gh auth`. Setup checks `gh auth status` and says so. No fallback for non-GitHub repos (cut: nobody here has one).

## Open choice (human's)
A. **Git files + draft PR ledger** (above) — recommended. Fewest new objects; no labels or issues until there is something to follow up.
B. **Issues for everything** — `design` issue, `plan` issue with task-list body, comments per task, PR closes both. Visible before a branch exists; Defer is free; two issues + label setup per repo.
C. Keep tally in-flight — does not meet the goal.

## Packaging
- New plugin **`loop`** (skills `/loop:brainstorm` … ). Contents: `skills/{brainstorm,plan,build,review-branch,debug,setup}`, `hooks/`, `pi/extensions/routing.ts`, `.claude-plugin/plugin.json`, `package.json` (pi). No binary, no build step.
- Lives in its own repo (`jasonrr/herdr-loop`), with its own `.claude-plugin/marketplace.json` (same shape as tally's). Alternative: second plugin entry in tally's marketplace pointing at that repo — decide at plan time, both are 10 lines.
- Config file renamed `.claude/dev-loop.md`; hook/skills read it, falling back to `.claude/tally-dev-loop.md` (one `||`). Routing block text drops the words "tally scratchpad"/"tally todos" (3 hand-synced copies → make the hook and routing.ts both read `skills/setup/routing.md`; one copy).
- tally repo: delete the six skills + hooks + routing.ts, drop "dev loop" from its plugin.json description, keep `pi/skills/tally` and everything store-side. tally keeps working as a plain store/TUI. Global `~/.claude/CLAUDE.md` wording `/tally:plan`→`/loop:plan`.
- Version: loop starts at 0.1.0; tally bumps minor (skills removed).

## Cut, and why
- Sub-issues or a GitHub Project per plan: PR checkboxes already render progress; nothing breaks without them.
- Todo locks: single builder per PR.
- Per-task issue comments: the decisions file is the record and is already committed.
- Non-GitHub fallback: no such repo in use.
- Migrating existing tally scratchpads/todos into issues: the five open designs/plans in this store are done or stale; `tally dump` keeps them readable.

## Open questions → todos
- Does `gh pr edit --body` race with GitHub's own checkbox toggles if the human ticks one? (Build is the only writer by contract; verify once live.)
- Plan is written on main before any branch exists: confirm `git worktree add` + copy two untracked files is clean in both herdr (`herdr worktree create`) and desktop paths.
