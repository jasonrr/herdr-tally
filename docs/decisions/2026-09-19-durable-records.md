# Design: durable dev-loop records — learnings, decisions, no build log (2026-09-19)

## Problem
- Two pads named `learnings` exist. "Create it if missing" let an agent miss the first one.
- The build log duplicates the task todos (what is done). Its only unique content is deviations from the plan, and those are design decisions buried in a pad nobody reads.
- The design pad never reaches git. Only the plan is materialized (`docs/plans/`).
- No skill archives anything. Pads pile up.

## Chosen approach (human picked: git files; delete build log)
1. **Learnings → `docs/learnings.md`**, one file. Same format as today: `pattern → consequence` bullets under `## <slug> (PR #<n>, YYYY-MM-DD)`. A path cannot duplicate. build / review-branch / debug append to the file in the worktree; it rides the PR. The `##` headings are the index.
2. **Decisions → `docs/decisions/YYYY-MM-DD-<slug>.md`**, one file per feature. Build materializes the `design` pad to this path at the same step that materializes the plan (absolute path — see learnings on `scratchpad_save_to_file`). review-branch appends a `## Deviations` section at the gate: each place the build left the plan, and why.
3. **Build log deleted.** On task completion, build adds one comment to the task todo: commit hash + any deviation. Resume reads todo status. Dispatcher watches todos only. review-branch collects the deviation comments into the decision file.
4. **Archive at the gate.** After the PR is opened and the decision file is written, review-branch archives the `design` and `plan:<slug>` pads. Git holds the durable copy.
5. **Plan's research pass** reads `docs/learnings.md` and `docs/decisions/` instead of `learnings`/`build-log:` pads.
6. **One-time migration in this repo:** merge the two `learnings` pads into `docs/learnings.md`; archive both pads and the five old build-log pads.

All changes are skill text (`skills/{plan,build,review-branch,debug}/SKILL.md`). No Rust, no MCP tool changes.

## Cut, and why
- Hand-kept index at the top of learnings — headings already are the index; a second list drifts.
- One running decisions file — parallel branches would conflict; per-feature files cannot.
- ADR template / numbering — the design pad's shape (problem, approach, cuts) is already the record.
- Backfilling decision files for past features — old design pads stay readable as archived pads.
- New MCP tools or tags — nothing breaks without them.

## Open questions
- Add `docs/decisions` and `docs/learnings.md` to `plan-paths` so the Plans tab shows them? Cheap; decide in plan.
- Parallel branches both appending to `docs/learnings.md` can conflict. Append-only, trivial to resolve; accept.
- `/tally:debug` that ends with no commit leaves an uncommitted learnings edit. Say "commit it with the fix, or on its own" in the skill.

## Deviations

- Task 1 and Task 2 edits were applied by a controller script (exact, uniqueness-asserted string swaps from the plan) instead of a sonnet implementer, because both were verbatim copy jobs. Task 1's reviewer was replaced by a mechanical diff of pad bullets vs file bullets (identical). Commits 8ddbe08, d6b2cef.
- review-branch step 2 had no source for the decision file's date; a later-day review would create a duplicate file. Pinned to the plan file's basename. Commit 595dad7.
- review-branch step 2's "append only if no `## Deviations` yet" guard (from 595dad7) dropped new deviations on a re-run; replaced with "add only missing bullets". build's resume rule for `in_progress` todos was reworded (no comment exists yet; check `git log` + `git status` since lock time). Commit 1d0e22b.
- This build logged learnings under its own new rules, so `docs/learnings.md` has more than the plan's pinned 18 bullets / 5 headings. The migrated sections match 18 / 5; the plan file is left as written. Commits 4e8db5d, 534fcb1.
- Branch reviewers ran through the Agent tool, not herdr panes (doc-only diff).
