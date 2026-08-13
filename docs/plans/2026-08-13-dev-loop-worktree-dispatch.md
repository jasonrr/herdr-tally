# Dev-loop worktree dispatch — plan+build run in per-feature worktree spaces

- **Goal:** substantial work runs in its own nested herdr worktree space per feature/bugfix, root reserved for brainstorm/plan/debug/one-offs + dispatch. `/tally:plan` authors the plan as a `plan:<slug>` scratchpad and, on confirm, dispatches `/tally:build` into a fresh worktree; build materializes the plan file, implements, opens a PR, then review-branch runs against the PR.
- **Design:** tally scratchpad `s_dko40eke9gbky` ("Design: dev-loop worktree dispatch", tags `design dev-loop worktree`). Read it in full before implementing.
- **Branch:** `feat/dev-loop-worktree-dispatch`
- **Whole-feature verify:** `cargo test && cargo clippy && cargo fmt --check` (regression backstop — no Rust changes, must stay green) + the per-task consistency reads. These are agent-instruction (prose) edits; verification is primarily a careful read against the shared contracts below, not a unit test.

This is a **meta change**: it edits the very skills that run the loop. It is itself being executed through the CURRENT skills, so this plan is a `docs/plans/` file (the old way) and the new scratchpad-plan behavior can't be dogfooded until it ships.

---

## Shared contract A — the `plan:<slug>` scratchpad (Tasks 1 & 2; do not deviate)

Plan authors the plan as a tally **scratchpad**, not a `docs/plans/*.md` file:
- Tag: `plan:<slug>` (same slug as the branch). Title: the feature goal.
- Body: the same task structure the current plan doc uses (per-task Exists-because / Model / Files / Steps test-first / Verify), plus a header (goal, design scratchpad id, branch, whole-feature verify command).
- One tally **todo per task**, tagged `plan:<slug>`, body pointing at the scratchpad section (not a file section). Ordering via `todo_add_blocker`. Todos keep pointing at the scratchpad even after build materializes the file — do NOT repoint them (the scratchpad persists; archival is deferred).

Build's first act **materializes** this scratchpad to `docs/plans/YYYY-MM-DD-<slug>.md` in the worktree and commits it with the feature, so it reaches main + the Plans tab through the PR. Rationale (from the design): scratchpads are the only artifact that crosses the worktree boundary (store is keyed to `--git-common-dir` = main tree; a `docs/plans` file written in a worktree is invisible to the Plans tab until merged). Do NOT change `src/plans.rs` — it stays main-tree-only.

## Shared contract B — herdr dispatch commands (Task 1; verified against `herdr` 0.8.0 `--help`, not the skill doc — per learnings)

- `herdr worktree create --cwd <repo-root> --branch <type>/<slug> --label "<title>" --no-focus` → JSON; new worktree workspace, root pane at `.result.root_pane.pane_id`, workspace at `.result.workspace_id`, checkout at `.result.worktree.path`. `<type>` ∈ `feat|fix|chore`, inferred from the design. `--cwd <repo-root>` nests the space under the parent project (verified live).
- `herdr agent prompt <pane-id> "<brief>"` → submits the brief to the agent that auto-launches in the worktree's root pane. (The pane auto-launches `claude` within seconds; do NOT `herdr agent start` — it fails `agent_pane_busy`.)
- After dispatch, **step back** — do NOT `herdr agent wait` + read the pane to mirror its progress (global CLAUDE.md). The dispatched agent talks to the user directly; progress rides tally todos + the `build-log:<slug>` scratchpad in the shared store. Root re-engages only on user request.

## Shared contract C — the build dispatch brief (Task 1; self-contained, no session history)

The brief handed to `herdr agent prompt` contains exactly:
- Repo path (the worktree checkout path from `.result.worktree.path`), branch name.
- The `plan:<slug>` scratchpad **id** (build reads the plan from it — the only thing crossing the boundary).
- Whole-feature verify command.
- The worktree sequence (contract D).
- Commit + PR instructions: commit with explicit paths (`git add <paths>`, never `-A`), no AI references in messages; `gh pr create` (let `gh` detect the remote — do NOT hardcode `jasonrr`/`jasonrosoff`, per learnings).
- **Forbid bare `git stash`** — the stash stack is shared across all worktrees/sessions; a stray stash/pop can eat another session's WIP (per learnings).

## Shared contract D — the worktree sequence (Tasks 2 & 3)

The dispatched agent runs, in order:
1. `/tally:build` for `plan:<slug>` (scratchpad id from the brief) — first materialize + commit `docs/plans/YYYY-MM-DD-<slug>.md` from the scratchpad, then implement task-by-task (TDD, per-task review) against the shared `plan:<slug>` todos.
2. On completion of ALL tasks → **open a PR** (`gh pr create`).
3. `/tally:review-branch` runs against the open PR branch.
4. Review fixes land on the PR as follow-up commits (commit + push to the PR branch).

---

## Task 1 — plan/SKILL.md: author plan as scratchpad, dispatch build on confirm

**Exists because:** without this, the plan stays a `docs/plans` file (invisible across the worktree boundary) and build never gets dispatched to a space — the two failures the design fixes.

**Model:** `sonnet` (prose edit requiring integration with the skill's existing voice and structure).

**Files:** `skills/plan/SKILL.md` (only).

**Steps:**
1. **Frontmatter `description:` (line 3):** rewrite so it no longer says "a plan doc in docs/plans/" — e.g. "…an executable plan as a `plan:<slug>` scratchpad (surfaced beside the live todos) plus one tally todo per task; in herdr, dispatches the build into its own worktree." This string shows in skill listings/routing, so it must match the new behavior.
2. Under "## The plan doc" (rename to "## The plan (scratchpad)"): replace "Write to `docs/plans/YYYY-MM-DD-<slug>.md`…" with authoring a **`plan:<slug>` scratchpad** per contract A — same header + task structure, but as a scratchpad via `scratchpad_write` tagged `plan:<slug>`. Note that build materializes it to a committed `docs/plans/` file in the worktree, and that in-flight plans live in the Scratchpads tab (Plans tab shows shipped plans).
3. Under "## Tally ledger": todos' bodies point at the **scratchpad section**, not a file section. Otherwise unchanged (one todo per task, `plan:<slug>`, blockers for ordering).
4. Rewrite "## Before handing off": keep the self-reread + zero-context-implementer subagent check (its input is now the plan scratchpad id, not a doc path). Then **replace** the closing "ask the user: execute via /tally:build … or inline" with the **confirm-gated dispatch**:
   - In herdr (`HERDR_ENV=1`): one `AskUserQuestion` — dispatch to a new worktree / build inline here / defer. On **dispatch**: run contracts B + C — `herdr worktree create …`, read `.result.root_pane.pane_id`, `herdr agent prompt <pane-id> "<brief>"`, then **step back** (state you're stepping back and that the space's agent owns it now). On **inline**: proceed to `/tally:build` in this session (current behavior). On **defer**: stop; the `plan:<slug>` scratchpad + todos persist for later.
   - Outside herdr: `EnterWorktree` then inline `/tally:build`, as today.
5. Confirm no remaining instruction tells PLAN to write a `docs/plans/*.md` file.

**Verify (occurrence counts, per learnings — `grep -o … | wc -l`, never `grep -c`):**
- `grep -oF 'plan:<slug>' skills/plan/SKILL.md | wc -l` ≥ 2 (scratchpad tag + todo tag).
- `grep -oF 'worktree create' skills/plan/SKILL.md | wc -l` ≥ 1 and `grep -oF -- '--no-focus' skills/plan/SKILL.md | wc -l` ≥ 1 (dispatch present).
- `grep -oiF 'docs/plans' skills/plan/SKILL.md | wc -l` = 1 (the sole surviving mention is the "build materializes to docs/plans" note; the frontmatter and the "## The plan" section no longer point plan at a file). Reviewer reads to confirm that one hit is the build-materialize note, not a plan-writes-it instruction.
- Read the whole file: frontmatter description updated; the herdr commands match contract B (`.result.` prefix); the confirm-gate has all three branches; non-herdr fallback present.

## Task 2 — build/SKILL.md: scratchpad input, materialize file, PR→review→fixes, no double-worktree

**Exists because:** build currently creates its own worktree and runs the loop in-place (illusory isolation) and has no PR/materialize step — it must instead run *inside* the dispatched space, produce the committed plan file, and drive PR→review→fixes.

**Model:** `sonnet`.

**Files:** `skills/build/SKILL.md` (only).

**Steps:**
1. **Frontmatter `description:` (line 3):** update the stale "on any existing plan doc that has tally todos" phrasing to reflect the scratchpad input (e.g. "…from a `plan:<slug>` scratchpad; runs inside its worktree, opens a PR, then review").
2. Line 8 (controller/worktree paragraph): **drop the unconditional `herdr worktree create`.** Replace with a context guard: if already in a dispatched worktree (`.git` is a file / on a non-main `<type>/<slug>` branch), proceed in place; only if invoked directly on `main` in the main checkout, create/`EnterWorktree` first (preserve the non-herdr `EnterWorktree` path). Keep "never main."
3. State the input: the `plan:<slug>` scratchpad **id** (from the dispatch brief, or resolved via `todo_list`/`scratchpad_find` on the tag when invoked directly). Replace references to "a plan doc under `docs/plans/`" accordingly.
4. Add a **first step before the per-task loop**: materialize the `plan:<slug>` scratchpad to `docs/plans/YYYY-MM-DD-<slug>.md` in the worktree and commit it (explicit path). Per contract A. Use the plan scratchpad's creation date for `YYYY-MM-DD` and the branch slug for `<slug>`.
5. In the per-task implement brief (step 3), add the **forbid-bare-`git stash`** stop-condition (contract C, per learnings), if not already implied.
6. Replace the closing "When every task is complete, run /tally:review-branch" with contract D's tail: on completion → `gh pr create` (let gh detect the remote) → `/tally:review-branch` against the PR → review fixes committed + pushed to the PR branch.

**Verify (occurrence counts):**
- `grep -oiF 'scratchpad' skills/build/SKILL.md | wc -l` ≥ 1 (reads plan from scratchpad).
- `grep -oF 'gh pr create' skills/build/SKILL.md | wc -l` ≥ 1 (PR step present).
- `grep -oF 'worktree create' skills/build/SKILL.md | wc -l` ≥ 1 (a guarded fallback remains) — reviewer reads to confirm every `worktree create` occurrence is inside the "invoked directly on main" guard, with no unconditional one left. (A literal `= 0` is WRONG here: step 2 keeps a guarded invocation.)
- Read the whole file: frontmatter updated; materialize-first step present; PR→review→fixes ordering matches contract D; `git stash` forbidden; "never main" preserved; per-task TDD loop otherwise unchanged.

## Task 3 — review-branch/SKILL.md: run against an open PR, fixes push to the PR

**Exists because:** review-branch currently assumes no PR exists (its Gate offers "push + PR" as a closing option and its description says "before any merge or PR"). In the new sequence the PR is already open when review runs, so its closing options and fix-handling are stale.

**Model:** `sonnet`.

**Files:** `skills/review-branch/SKILL.md` (only).

**Steps:**
1. Description + intro: reflect that review runs against an **already-open PR** (after build opens it), reviewing `merge-base..HEAD` = the PR diff. Adjust "before any merge or PR" → "before merge."
2. "## Verdicts": the consolidated p1 fix dispatch commits **and pushes to the PR branch** (follow-up commits), then one scoped re-review — per contract D.
3. "## Gate": replace the closing options "merge, push + PR, or leave the branch" with the PR-exists options — e.g. **mark the PR ready / merge it / leave it for the user** (the branch is already pushed + PR'd). Keep everything else (fresh full test + lint, `learnings` scratchpad line per surprise, the `/tally:setup` offer if `.claude/tally-dev-loop.md` is absent).

**Verify (occurrence counts):**
- `grep -oiF 'push + PR' skills/review-branch/SKILL.md | wc -l` = 0 (stale "review-then-PR" option removed).
- `grep -oF 'the PR' skills/review-branch/SKILL.md | wc -l` ≥ 1 (PR-exists framing present; not the collision-prone bare "PR" substring).
- Read the whole file: fixes commit + push to the PR; closing options assume the PR already exists (mark-ready / merge / leave); "before any merge or PR" → "before merge"; the herdr reviewer-pane block and severity/verdict machinery are otherwise unchanged.

---

## Cuts / not in this plan (cut test applied)

- **Routing block / setup edit — CUT.** The five routing triggers are unchanged by this design (approved-design→plan, plan+todos→build, branch-complete→review all still hold); dispatch is internal to plan/build. The block is triplicated byte-identical across `hooks/session-start.sh`, `pi/extensions/routing.ts`, `setup/SKILL.md`; rewording "plan doc"→"plan" risks a sync divergence (per learnings) for zero behavior change. Not worth it.
- **`src/plans.rs` per-worktree resolution — REJECTED** in design (fights the main-keyed invariant). No Rust changes at all.
- **Plugin-pane auto-attach on `worktree.created`** (`on-worktree.sh`) — out of scope; toggle actions already target a worktree correctly.
- **Archiving shipped `plan:<slug>` scratchpads** — deferred by user; keep them for now.
