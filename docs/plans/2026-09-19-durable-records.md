# Plan: durable dev-loop records (plan:durable-records)

- **Goal:** learnings live in one git file, each feature gets a decision record in git, the build-log scratchpad is gone, finished pads get archived.
- **Design scratchpad id:** `s_dlja001vfgbs1`
- **Branch:** `chore/durable-records`
- **Scope:** skill text + docs only. No Rust, no MCP tool changes, no version bump (releases are their own commit).
- **Whole-feature verify** (run from the worktree root, expect `OK`). It is Task 1's verify plus Task 2's verify; each task's own verify is a subset and does not depend on the other task:

```bash
! grep -rn 'build-log' skills/ \
&& [ "$(grep -l 'docs/learnings.md' skills/build/SKILL.md skills/review-branch/SKILL.md skills/debug/SKILL.md skills/plan/SKILL.md | wc -l | tr -d ' ')" = 4 ] \
&& [ "$(grep -l 'docs/decisions/' skills/build/SKILL.md skills/review-branch/SKILL.md skills/plan/SKILL.md | wc -l | tr -d ' ')" = 3 ] \
&& [ "$(grep -c '^- ' docs/learnings.md)" = 18 ] && [ "$(grep -c '^## ' docs/learnings.md)" = 5 ] \
&& ! grep -n '\.- ' docs/learnings.md && echo OK
```

## Controller note — this build dogfoods the new rules

The installed `/tally:build` and `/tally:review-branch` still carry the OLD text. For this build, the controller follows the NEW text from Task 2 wherever they differ:

- Do NOT create a `build-log:durable-records` scratchpad. Record each task as a comment on its todo (Task 2, edit B).
- Surprises go to `docs/learnings.md` in the worktree (exists after Task 1), not to a `learnings` pad.
- At setup, also materialize design pad `s_dlja001vfgbs1` to `<worktree-root>/docs/decisions/2026-09-19-durable-records.md` (absolute path, `mkdir -p` first — `scratchpad_save_to_file` is a plain `fs::write`, `src/store/scratchpads.rs:624-632`, and makes no parent dirs) and commit it with the plan file.
- At the review gate, follow the NEW Gate text (Task 2, edit C), including `## Deviations` and archiving this plan pad and the design pad after the push.
- `scratchpad_archive` requires `expected_revision` (`src/mcp/tools.rs:289-294`). Read each pad for its current revision right before archiving it.

## Research — patterns followed

- Materializing a pad to a committed docs file: follows `skills/build/SKILL.md:12` ("First, materialize the plan").
- `scratchpad_save_to_file` relative-path trap: `learnings` pad `s_dl1wl8navqlsi`, section dispatch-contract — a relative path resolves against the MCP server cwd and lands in the MAIN checkout. Hence absolute paths everywhere below.
- Verify greps must not be satisfiable by reshaping text: `learnings` pad `s_dkm86rogch7s1`, first bullet. The greps here check presence per file and exact counts on one-bullet-per-line content.
- Heading handoff `PR #pending` → real number: kept exactly as `skills/build/SKILL.md:27` and `skills/review-branch/SKILL.md:41` do today.
- Archived pads drop out of default `scratchpad_list`/`scratchpad_find` (`src/store/scratchpads.rs:356`). So every archive in this plan happens only after the content is committed to git.
- The Plans tab reads user-set `plan-paths` (`src/plans.rs:14,68`, default `docs/plans`). Showing `docs/decisions` there is a user TUI setting, not code. Cut from this plan.

## Task 1 — migrate this repo's learnings to `docs/learnings.md`

- **Exists because:** two `learnings` pads exist today; without one merged file the new skill text has nothing to append to and the split persists.
- **Model:** sonnet
- **Files:** creates `docs/learnings.md`. Produces for Task 2: nothing (independent).
- **Steps:**
  1. Run the Verify command below; confirm it fails (file missing).
  2. Read both pads: MCP `scratchpad_read` id `s_dkm86rogch7s1` (older) and `s_dl1wl8navqlsi` (newer), mode `content`. CLI fallback: `bin/tally scratchpads read <id>` from the main checkout.
  3. Write `docs/learnings.md` with this exact skeleton. Bullet text is copied verbatim from the pads, **in each pad's original order**; only split run-ons and add headings. The parenthetical lists below identify the bullets, they do not reorder them.
     - Line 1: `# Learnings`
     - `## dev-loop-v2 (2026-08-11)` — the older pad's 5 bullets before its `## pi-package branch` heading (subagent verify commands; bare git stash; herdr SKILL.md vs `--help`; two readers of one config file; manifests vs `git remote -v`).
     - `## pi-package branch (2026-08-13)` — its 4 bullets (`before_agent_start`; parity test; bare `create` mutates; JS `/^x$/m` vs grep).
     - `## is_blocked perf (2026-08-21)` — 1 bullet: the "Per-item accessor that reloads the whole store…" text, which is run onto the end of the JS-regex bullet in the pad.
     - `## bundle-ask-user (2026-08-30)` — the newer pad's 3 bullets before its `## dispatch-contract` heading (bundledDependencies; hermetic install.sh tests; Pi `console.error`).
     - `## dispatch-contract (PR #18, 2026-09-08)` — its 5 bullets, verbatim.
     - The older pad has exactly three run-ons of the form `.- ` (after "harmless this time)", after "hook stayed silent)", after "exact line-split matching"). Split each into two bullets. One blank line before every `##`. Every bullet is one line starting `- `.
  4. Commit: `git add docs/learnings.md && git commit -m "docs: merge learnings pads into docs/learnings.md"`.
  5. **Controller, after the Verify passes on the committed file** (not the implementer): `scratchpad_archive` these eight pads, reading each first for its current `expected_revision` — learnings `s_dkm86rogch7s1`, `s_dl1wl8navqlsi`; old build logs `s_dla2shgtgkwg3`, `s_dl1vurcnihuw3`, `s_dkopkvz86u2g2`, `s_dko4zj8yo6zs15`, `s_dko0l94jj1ege`, `s_dkm7yhcx3q801`. Archive, never delete. Old build-log deviations are NOT backfilled into decision files (cut in design).
- **Verify:**

```bash
[ "$(grep -c '^- ' docs/learnings.md)" = 18 ] && [ "$(grep -c '^## ' docs/learnings.md)" = 5 ] && [ "$(head -1 docs/learnings.md)" = "# Learnings" ] && ! grep -n '\.- ' docs/learnings.md && echo OK
```

## Task 2 — rewrite the skill text

- **Exists because:** the skills are what create the duplicate pads and the build log; without new text every future build repeats the problem.
- **Model:** sonnet
- **Files:** `skills/build/SKILL.md`, `skills/review-branch/SKILL.md`, `skills/debug/SKILL.md`, `skills/plan/SKILL.md`. No other file mentions these pads (checked: `grep -rn 'build-log\|learnings' --exclude-dir=target --exclude-dir=.git .` hits only these four plus historical `docs/plans/*`, which stay untouched).
- **Steps:** run the Verify command first and confirm it fails. Then make edits A–E. Each replaces one whole paragraph (A–D) or one phrase (E). Text outside the named spans stays byte-identical. Commit: `git add skills/build/SKILL.md skills/review-branch/SKILL.md skills/debug/SKILL.md skills/plan/SKILL.md && git commit -m "skills: learnings and decisions in git, drop the build log"`.

### Edit A — `skills/build/SKILL.md`, the paragraph starting `**First, materialize the plan.**` (line 12). Replace the whole paragraph with:

```
**First, materialize the plan and the design.** Write the `plan:<slug>` scratchpad to `docs/plans/YYYY-MM-DD-<slug>.md` (the scratchpad's creation date, the branch slug). Then write the `design` scratchpad the plan's header names to `docs/decisions/YYYY-MM-DD-<slug>.md` — same basename as the plan file. That file is the feature's decision record; /tally:review-branch appends the build's deviations to it. If the plan names no design scratchpad, skip the decision file. Pass `scratchpad_save_to_file` an **absolute** path under `git rev-parse --show-toplevel`, and `mkdir -p` the directory first — the tool creates no parent directories, and a relative path resolves against the MCP server's cwd and lands in the main checkout, not this worktree. Commit both files with explicit paths — they ride the feature's PR to main, and the plan shows in the Plans tab on merge. In-flight they stayed scratchpads because a `docs/` file in a worktree is invisible to the main checkout until merged.
```

### Edit B — `skills/build/SKILL.md`, list item `6. **Record.**` (line 27). Replace the whole item with:

```
6. **Record.** `todo_complete`, plus one `comment_add` on the todo: `commit <hash> — deviation: <what left the plan and why, or none> — parked: <todo ids or none>`. The todos are the ledger — keep no separate log scratchpad. A resumed session reads `todo_list` for `plan:<slug>` and re-dispatches nothing that is `completed`; for a todo still `in_progress`, check `git log` for its commit before dispatching again. The deviation comments are what /tally:review-branch collects into the decision record. **If the task surprised you** — a plan defect, a tool or classifier refusal, a verify command that could not pass at this task, a finding you had to park — also append one `pattern → consequence` bullet to `docs/learnings.md` in this worktree, under a dated heading for this work: `## <slug> (PR #<n>, YYYY-MM-DD)`, blank line before the heading, one bullet per line. If the file is missing, create it with a `# Learnings` H1 and first copy in every bullet from every `learnings`-tagged tally scratchpad, oldest scratchpad first; archive a scratchpad only after all its bullets are in the committed file. The PR doesn't exist yet mid-build — write `PR #pending` and reuse that one heading for every surprise in this build; review-branch fills the number in. Commit the file with an explicit path. The todo comment says what happened here; learnings says what the next planner should not repeat.
```

### Edit C — `skills/review-branch/SKILL.md`, the single paragraph under `## Gate` (line 41). Replace the whole paragraph with:

```
Run the full test suite and lint fresh, in this session, and read the output. Then write the durable record, in the worktree:

1. **Learnings.** Append one `pattern → consequence` bullet per surprise the review surfaced to `docs/learnings.md`, under a dated heading for this work — `## <slug> (PR #<n>, YYYY-MM-DD)`, blank line before the heading, one bullet per line. If the build already opened that heading, append under it rather than opening a second one, and replace its `PR #pending` with the real number — do that replacement even when the review adds no bullets. If the file is missing and there is something to write, create it with a `# Learnings` H1 and first copy in every bullet from every `learnings`-tagged tally scratchpad, oldest scratchpad first; archive a scratchpad only after all its bullets are in the committed file.
2. **Decisions.** If the branch came from a `plan:<slug>`: read the comments on its todos (`todo_list` for the tag, completed included; `comment_list` on each) and append a `## Deviations` section to `docs/decisions/YYYY-MM-DD-<slug>.md` — one bullet per place the build, or this review's fixes, left the plan: what changed, why, commit hash. Write `None.` when there were none. If build skipped the file (the plan named no design scratchpad), create it with a `# <slug>` H1 only when there is a deviation to record.
3. Commit the changed files with explicit paths and push to the PR branch.
4. **Archive.** Only if the branch came from a `plan:<slug>` whose plan file is in this PR, and only once that push succeeds: `scratchpad_archive` that `plan:<slug>` scratchpad and the `design` scratchpad its header names (read each first — archive takes the current revision). Git now holds the durable copy. Never pick a scratchpad by guessing from the branch name; if there is no plan, or the push failed, archive nothing.

Then present the real options — the PR is already open, so: mark it ready / merge it / leave it for the user — and wait for the user's choice. If you were dispatched (your brief named a dispatcher pane) and you end here, that is a `blocked` report, not silence — send it before you wait. The p2/p3 follow-ups live in tally, not in your head. If `.claude/tally-dev-loop.md` doesn't exist, offer /tally:setup once in the closing summary.
```

### Edit D — `skills/debug/SKILL.md`, the last paragraph (line 30, starts `Follow-ups the investigation turns up`). Replace the whole paragraph with:

```
Follow-ups the investigation turns up go to tally todos, not into this fix. When the root cause would surprise the next session, append one `pattern → consequence` bullet to `docs/learnings.md`, under a dated heading — `## <slug or short label> (YYYY-MM-DD)`, blank line before the heading, one bullet per line. If the file is missing, create it with a `# Learnings` H1 and first copy in every bullet from every `learnings`-tagged tally scratchpad, oldest scratchpad first; archive a scratchpad only after all its bullets are in the committed file. Commit it with the fix, or on its own when there is no fix commit.
```

### Edit E — `skills/plan/SKILL.md`, three phrase swaps; nothing else on those lines changes:

1. Line 18. Old: ``skim `learnings`- and `build-log:`-tagged tally scratchpads for prior art and past failures``. New: ``read `docs/learnings.md` and skim `docs/decisions/` for prior art and past failures (in a repo with no `docs/learnings.md` yet, skim its `learnings`-tagged tally scratchpads instead)``.
2. Line 46. Old: ``progress rides the tally todos + `build-log:<slug>` scratchpad until the child reports``. New: ``progress rides the tally todos until the child reports``.
3. Line 72. Old: ``the same job `build-log:<slug>` does for``. New: ``the same job the `plan:<slug>` todos do for``.

- **Verify:**

```bash
! grep -rn 'build-log' skills/ \
&& [ "$(grep -l 'docs/learnings.md' skills/build/SKILL.md skills/review-branch/SKILL.md skills/debug/SKILL.md skills/plan/SKILL.md | wc -l | tr -d ' ')" = 4 ] \
&& [ "$(grep -l 'docs/decisions/' skills/build/SKILL.md skills/review-branch/SKILL.md skills/plan/SKILL.md | wc -l | tr -d ' ')" = 3 ] \
&& grep -q 'PR #pending' skills/build/SKILL.md && grep -q 'PR #pending' skills/review-branch/SKILL.md \
&& grep -q 'comment_add' skills/build/SKILL.md && grep -q 'scratchpad_archive' skills/review-branch/SKILL.md \
&& [ "$(grep -l 'only after all its bullets are in the committed file' skills/build/SKILL.md skills/review-branch/SKILL.md skills/debug/SKILL.md | wc -l | tr -d ' ')" = 3 ] && echo OK
```
