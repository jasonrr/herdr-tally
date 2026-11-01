---
name: build
description: Execute a plan task-by-task with fresh subagents — TDD, per-task review, the draft PR checklist as the ledger. Runs inside its own worktree from a docs/plans/*.md file, opens the draft PR first, then review. Use after /loop:plan (usually dispatched into a worktree), or directly on a plan file with unticked boxes.
---

# Build

You are the controller: dispatch, verify, adjudicate. Subagents implement. Work on a branch or worktree, never main. Usually `/loop:plan` has already dispatched you into a fresh worktree — if you are in a linked worktree (`.git` is a file) or on a non-main `<type>/<slug>` branch, work in place. Only when invoked directly on `main` in the main checkout do you isolate yourself — `EnterWorktree` onto a `<type>/<slug>` branch and run the loop in-session, in herdr or not. When you isolate yourself this way and the plan file is still uncommitted in the main checkout, move it (and its design file) into the new worktree first: `ROOT=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")`, `WT=$(git rev-parse --show-toplevel)`, then `mkdir -p "$WT/docs/plans" "$WT/docs/decisions"; mv "$ROOT/docs/plans/<plan-file>" "$WT/docs/plans/"; [ -f "$ROOT/docs/decisions/<design-file>" ] && mv "$ROOT/docs/decisions/<design-file>" "$WT/docs/decisions/"` — move, not copy. `EnterWorktree` may not use the name verbatim as the branch — read the real one with `git branch --show-current`. If the plan `mv` fails, stop and report. Do NOT `herdr worktree create` here: that spawns a whole workspace whose auto-launched agent you would then orphan by working inline. The herdr worktree *space* is `/loop:plan`'s dispatch hand-off, not something build re-creates.

In the Herdr desktop app (`$HC_TERMINAL_ID` set) the same holds: work in place when dispatched; on `main`, `EnterWorktree` — never `herdr-app do space.new` from build.

Input: the plan file path from your brief — `docs/plans/YYYY-MM-DD-<slug>.md` in this checkout; the design file is `docs/decisions/` with the same basename, if it exists. If no path is in context, look for an uncommitted plan (`git status --porcelain docs/plans`, in this checkout or the main one) or the one named on the first line of the current branch's draft PR body (`gh pr view --json body -q .body | head -1`), and confirm with the user before starting.

**First commit, then the draft PR.** If `git status --porcelain docs/plans docs/decisions` is non-empty: `git add docs/plans/<plan-file> docs/decisions/<design-file>` (omit the design if there is none), commit as `plan: <slug>`, and `git push -u origin <type>/<slug>`. Then, if `gh pr view` reports no PR for this branch, create it as a draft whose body is the checklist — one line per `## Task N` heading in the plan, linking to its anchor:
```bash
BASE=$(gh repo view --json url -q .url); BR=$(git branch --show-current); PLAN=docs/plans/<plan-file>; BODY=$(mktemp)
{ echo "Plan: $PLAN"; echo; grep -n '^## Task [0-9][0-9]*[[:space:]]*$' "$PLAN" | while IFS=: read -r ln _; do n=$(sed -n "${ln}p" "$PLAN" | sed 's/## Task //; s/[[:space:]]*$//'); title=$(sed -n "$((ln+1))p" "$PLAN" | sed 's/^\*\*//; s/\*\*[[:space:]]*$//'); echo "- [ ] T$n — $title ($BASE/blob/$BR/$PLAN#task-$n)"; done; } > "$BODY"
TITLE='<plan goal, one line — single quotes, no quote characters inside>'
gh pr create --draft --title "$TITLE" --body-file "$BODY"
```
(`gh pr create` has no `--title-file`; the single-quoted variable keeps backticks and `$` in the goal from executing.) The checklist is the ledger: `gh pr view --json body -q .body | grep '^- \[ \]'` lists what is not done. A resumed session starts from the first unticked task after checking `git log`/`git status` for partial work on it.

**Then seed the dev-loop config.** `.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md` when only that exists; routing, `builder-model`, and — in a repo with no committed `docs/review-lenses.md` — the review lenses) is gitignored, so a fresh worktree checkout lacks it: this worktree's sessions would run with routing disarmed, and in a repo without the committed file the per-task review below would silently fall back to default lenses. If it's absent here but present in the main worktree, copy it in — a no-op when it already exists or the repo has no dev loop: `for f in dev-loop.md tally-dev-loop.md; do [ -f ".claude/$f" ] && break; root="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"; [ -f "$root/.claude/$f" ] && mkdir -p .claude && cp "$root/.claude/$f" ".claude/$f" && break; done` (the copy stays gitignored — it never enters the PR).

## Per task, in plan order, one at a time

1. **Cut check.** Reread the task's "Exists because" line against the code as it stands now. If it no longer holds, or a dumber path has appeared since planning, surface that to the user instead of building — plans go stale as earlier tasks land, and a wasted task costs its full implement+review cycle.
2. **Implement.** Dispatch a fresh subagent with an explicit `model` — the tier the plan names, `sonnet` if unstated. Never omit the model: an omitted model inherits the session's, usually the most expensive. Implementers dispatch through the Agent tool (cheap, model-pinned, serialized on one tree) — never a herdr pane; the workspace-level dispatch that `/loop:plan` already did is the only pane hand-off in the loop. The brief is self-contained (repo path, the task's text with its code and interfaces, verification commands, commit instructions) — never session history, never the whole plan. The brief includes:
   - TDD: write the failing test first, run it, confirm it fails for the right reason, then minimal code to green. No production code without a failing test. A bug found mid-task gets a failing test reproducing it before the fix.
   - Hygiene during implementation, not review: types, lint clean, no dead code, match surrounding style.
   - Commit with explicit paths (`git add <paths>`, never `-A`); never a bare `git stash` (the stash stack is shared across all worktrees/sessions — a stray stash/pop eats another session's WIP); no AI references in messages.
   - Stop conditions: the live code contradicts the brief, verification fails twice after a fix attempt, or the work needs files outside those named — report back rather than improvise.
3. **Verify yourself.** Run the task's verification command and read the diff. The subagent's report is a claim, not evidence.
4. **Review.** Dispatch a fresh reviewer subagent (`sonnet`) with the diff, the task brief, and the severity definitions from /loop:review-branch (p1 = data loss, invariant violation, security, mainline breakage; p2 = edge wrongness, missing I/O error handling; p3 = hygiene) — plus the repo's lens charters — `docs/review-lenses.md` when it exists, else the `## Lenses` section of `.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md`) — when either is present. Every finding must be a concrete failure scenario (input/state → wrong behavior); verify each to CONFIRMED (reproduced/traced) or PLAUSIBLE (couldn't refute), and drop a finding only by showing it's wrong. Fix rounds go back to the implementer, capped at 3; past the cap you adjudicate each open finding yourself — fix it, or park it as a GitHub issue — `gh issue create --title '[p<sev>] <one line>' --body-file -` with a heredoc body: the scenario, your ruling (why not fixed here), and `From PR #<n>` — and put the issue URL in this task's Deviations line. Nothing is dropped silently.
5. **Record.** Tick the task's box: `N=<n>; b=$(gh pr view --json body -q .body) && [ -n "$b" ] && printf '%s\n' "$b" | sed "s/^- \[ \] T$N /- [x] T$N /" | gh pr edit --body-file -` (the guard keeps a failed read from blanking the body). Then append one bullet to the `## Deviations` section of `docs/decisions/<design-file>` (create the section at the end of the file if missing; if the plan named no design file, put the same line in the task's commit message body instead): `- T<n> commit <hash> — deviation: <what left the plan and why, or none> — parked: <issue urls or none>`. Commit that file with an explicit path and `git push`. The checklist plus this section are the whole ledger — keep no other log. A resumed session reads the checklist; an unticked task with commits already on the branch is unfinished, not undone: check `git log` and `git status`, and dispatch again only for what is missing. **If the task surprised you** — a plan defect, a tool or classifier refusal, a verify command that could not pass at this task, a finding you had to park — also append one `pattern → consequence` bullet to `docs/learnings.md` in this worktree, under a dated heading for this work: `## <slug> (PR #<n>, YYYY-MM-DD)`, blank line before the heading, one bullet per line (the PR number is known from the start — never use a placeholder). If the file is missing, create it with a `# Learnings` H1. Commit the file with an explicit path. The Deviations line says what happened here; learnings says what the next planner should not repeat.

One implementer at a time; parallel implementers on one tree conflict.

When every box is ticked, run /loop:review-branch against the draft PR. Review fixes land on the PR as follow-up commits.

**Last action: report to your dispatcher.** If your brief named a dispatcher pane, your final act
on **every** exit path — all tasks done, blocked awaiting the operator, stopped by a stop
condition, or abandoned — is the one-line report the brief specified:

    herdr agent prompt <dispatcher-pane-id> "<slug>: <done|blocked|failed> — PR #<n> <url> | suite <N> OK | parked: <ids or none> | needs human: <one line or none>"

If your brief came from the Herdr desktop app (it names an hc terminal such as `t5` and says `hc send`), send the same line with:

    hc send <dispatcher-terminal> "<slug>: <done|blocked|failed> — PR #<n> <url> | suite <N> OK | parked: <ids or none> | needs human: <one line or none>"

If it exits non-zero, run `hc wait <dispatcher-terminal> --timeout 100` and send again; repeat up to 3 times, then tell the human the report could not be delivered. Exit 3 (no such terminal) or 11 (it has finished) means the dispatcher is gone: stop retrying and tell the human.

Send it once, last, after the PR and review-branch — **once total**, not once per skill: if
/loop:review-branch already sent a `blocked` report when it stopped at its gate, that was the
report, and you send nothing more. Exiting without any report is the failure this contract
exists to prevent: a silent exit looks exactly like a build still running. If your brief named
no dispatcher pane you were invoked directly — no report, everything else unchanged.
