---
name: plan
description: Turn an approved design into an executable plan — authored as docs/plans/YYYY-MM-DD-<slug>.md and moved into the build's worktree; in herdr or the desktop app, dispatches the build into its own worktree. Use after /loop:brainstorm, once a design file exists.
---

# Plan

Write a plan a fresh agent with zero context can execute. Every ambiguity you leave becomes a wrong guess by an implementer that never saw this conversation.

A design is required input: an uncommitted `docs/decisions/YYYY-MM-DD-<slug>.md` (`git status --porcelain docs/decisions` lists it) or one the user names. If none exists, run /loop:brainstorm first — do not write a plan from a cold request.

## The plan (file)

Author the plan as `$ROOT/docs/plans/YYYY-MM-DD-<slug>.md` in the main checkout (ROOT as defined at the end of this paragraph — not your cwd, which may be another worktree) — today's date, the design's `<slug>` — uncommitted (`mkdir -p "$ROOT/docs/plans"`). The date in the name is fixed now; every later skill uses the name as written, never today's date. Header: goal, design file path, branch name (`<type>/<slug>`, type ∈ feat|fix|chore), and the verification command for the whole feature. Every task is a heading that is exactly `## Task N` (no trailing spaces, never inside a code fence), with `**<title>**` on the next line — the build's PR checklist links to the `#task-n` anchors GitHub makes from those headings. /loop:build commits this file and the design file as its first commit in the worktree, so they reach `main` with the PR. In every bullet below, `ROOT=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")` is the main checkout — set it in each bullet that uses it.

## Research before tasks

Dispatch one Explore subagent before writing any task: for each major task area, find the nearest existing in-repo pattern to follow; read `docs/learnings.md` and skim `docs/decisions/` for prior art and past failures. Cite what you follow in the plan ("follows the pattern in src/…"). A novel approach where a pattern exists, uncited, is a plan defect. A fact verified for one harness, tool, or caller does not transfer to another — each named consumer of a config key or interface needs its own citation.

## Tasks

Size each task so it is independently implementable and reviewable — a reviewer could approve it while rejecting its neighbor. Each task states:

- **Exists because:** one line naming what breaks without it, or why the dumb alternative fails. If you can't write that line, the task shouldn't exist — cut it now, not after it's built. A wasted task costs a full implement+review cycle.
- **Model:** `haiku` only for mechanical transcription where the plan already contains the complete code; `sonnet` for everything else (the default); `opus` only when the task is itself architectural. /loop:build dispatches with exactly this.
- **Files** touched, and **interfaces** consumed from / produced for neighboring tasks.
- **Steps, test-first:** the failing test to write, then the minimal implementation. Include complete code where you already know it. Placeholders — "TBD", "add appropriate error handling", "similar to task 2" — are plan defects; resolve them here.
- **Verify:** the exact command that proves this task done (usually the scoped test invocation) — /loop:build runs it after the implementer reports.

## Ledger

There is no separate ledger to create. /loop:build opens a draft PR whose body is a checklist, one line per task, linking to the `## Task N` anchors; ticked boxes are the live state you and the user both watch. Ordering is the order of the tasks in the file — write them in dependency order.

## Before handing off

First reread the plan yourself for placeholders and interface mismatches. Then dispatch **two** fresh `sonnet` subagents in parallel, each given only the plan file path. Every finding either returns is a plan defect — fix them all before handing off.

1. **Ambiguity (zero-context implementer):** "List every place you would have to guess — missing file paths, undefined interfaces, placeholder steps, decisions the plan assumes you know."
2. **Correctness/invariants (adversary):** brief it with the repo's lens charters — `docs/review-lenses.md` when it exists, else the `## Lenses` section of `.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md`) — when either is present, else correctness + invariants. "This plan is confident and may be confidently wrong. For each task: name every documented invariant the task touches, and trace one concrete failure sequence — state/input → wrong behavior — through the plan's own code. Review the plan's code as shipped code; the implementer will transcribe it verbatim." Severity definitions and the probe-log rule are /loop:review-branch's: zero findings is legal only with a probe log naming which invariants were checked per task.

The ambiguity pass cannot find a plan that is unambiguous and wrong, and that is where the expensive defects live — a design error in the plan's own code reaches the implementer as an instruction not to deviate.

Then hand off to the build:

- **In herdr (`HERDR_ENV=1`) or the Herdr desktop app (`$HC_TERMINAL_ID` set):** one prompt carrying two questions (use AskUserQuestion when available). First: dispatch to a new worktree / build inline here / defer. Second, "builder model?" — offer the repo's configured default first, with `(default)` in its label, then the other two of `opus` / `sonnet` / `fable`. Read the default from `.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md` when only that exists) with `grep '^builder-model:'`, falling back to `opus` when the key or the file is absent. One prompt, two questions — never a second round trip. The model answer applies only to the Dispatch branch; on Inline or Defer, ignore it.
  - **Dispatch, in the Herdr desktop app** (`$HC_TERMINAL_ID` set — this wins even if `HERDR_ENV` is also set; then skip the next bullet). The app has no worktree command: git makes the checkout, the app opens a space on it. Model comes from the builder-model answer; family from your kind: `hc ls --json | jq -r --arg t "$HC_TERMINAL_ID" '.terminals[] | select(.address==$t) | .agent'`. **claude-code** → argv `["claude","--model","<chosen-builder-model>"]`; **pi** → `["pi","--provider","<PI_PROVIDER>","--model","<PI_MODEL>"]`, both non-empty; **any other kind** → do not dispatch; say why and offer Inline. Main checkout: `ROOT=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")`. Then `git -C "$ROOT" fetch origin main` and `git -C "$ROOT" worktree add "$ROOT/.claude/worktrees/<slug>" --no-track -b <type>/<slug> origin/main`. If the path or branch already exists, stop and ask the human — never reuse or delete it. Move the two files in — move, not copy; a copy left behind blocks the `git pull` that later brings the committed file to main: `WT="$ROOT/.claude/worktrees/<slug>"; mkdir -p "$WT/docs/plans" "$WT/docs/decisions"; mv "$ROOT/docs/plans/<plan-file>" "$WT/docs/plans/"; [ -f "$ROOT/docs/decisions/<design-file>" ] && mv "$ROOT/docs/decisions/<design-file>" "$WT/docs/decisions/"`. If the plan `mv` fails, stop and report — do not dispatch a build on a missing plan. Write the brief to `$ROOT/.claude/worktrees/<slug>/.tally-brief.md`: checkout path, branch, the plan file path inside the checkout (`docs/plans/<plan-file>`), the whole-feature verify command, "run /loop:build on `docs/plans/<plan-file>` through completion", and the desktop-app report block from **Dispatch contract** with your `$HC_TERMINAL_ID` filled in. Then `herdr-app do space.new --name "<title>" --folder "$ROOT/.claude/worktrees/<slug>" --argv '<argv-json>' --task "Read .tally-brief.md in this folder, delete it, then follow it."`. It prints `{"space":…,"tab":…,"terminal":…}`; keep `space` and `terminal`. Write the `.claude/dispatch-log.md` line, then step back — say so; the space's agent owns it and talks to the user directly (no polling; see **Dispatch contract**).
  - **Dispatch:** preserve the caller's CLI *family*; take the model from the answer to the builder-model question above, never from the caller's session. Before creating the worktree, read the current kind from `herdr agent get "$HERDR_PANE_ID"` at `.result.agent.agent`; for Pi also retain `PI_PROVIDER` + `PI_MODEL`. Run `herdr worktree create --cwd <repo-root> --branch <type>/<slug> --label "<title>" --no-focus`; read `.result.root_pane.pane_id` (workspace at `.result.workspace.workspace_id`, checkout at `.result.workspace.worktree.checkout_path`). Set `ROOT=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")` and `WT=<checkout_path>`, then move the two files in exactly as the desktop bullet does (`mkdir -p` + `mv`, never `cp`). Check whether an agent is actually running first (`herdr agent get <pane-id>`); only if so, exit it with `herdr agent send-keys <pane-id> ctrl+d` — on a bare shell ctrl+d closes the pane instead. Confirm the pane is back at a shell, then `herdr agent start build-<slug> --kind <caller-kind> --pane <pane-id>`, appending an agent-arg tail chosen by kind: **claude** → `-- --model <chosen-builder-model>`; **pi** → `-- --provider "$PI_PROVIDER" --model "$PI_MODEL"`, requiring both non-empty; **any other kind** → no tail, it launches bare as today. `herdr agent start` accepts the tail after `--` (`[-- [AGENT_ARG]...]`, verified live against herdr 0.9.1). Then `herdr agent prompt <pane-id> "<brief>"` with a self-contained brief: repo path (the worktree checkout), branch, the plan file path inside the checkout, the whole-feature verify command, and one instruction — "run /loop:build on `docs/plans/<plan-file>` through completion." Build owns the rest (first commit + draft PR, implement task-by-task, open the PR, run review-branch, push fixes). No session history in the brief. The brief also carries the return address and report line from **Dispatch contract** below — a dispatch without one is the bug this contract exists to prevent. Then write the `.claude/dispatch-log.md` line (below) and **step back** — say so; the space's agent owns it and talks to the user directly (no polling — see **Dispatch contract** below).
  - **Inline:** `EnterWorktree` (it creates the worktree and branch; give it the `<type>/<slug>` name as its argument), set `ROOT` as above and `WT` to the new worktree's path, move the two files in exactly as the desktop bullet does, then proceed to /loop:build in this session. `EnterWorktree` may not use the name verbatim as the branch — read the real one with `git branch --show-current` and use that wherever a step says `<type>/<slug>`.
  - **Defer:** stop; the plan and design files stay uncommitted in the main checkout — say so. Do not `git stash` them (the stash stack is shared across worktrees); leave them, or commit them on a branch if the user asks.
- **Outside herdr:** `EnterWorktree`, set `ROOT` and `WT`, move the two files in the same way, then /loop:build inline.

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

**In the Herdr desktop app** the return address is your `$HC_TERMINAL_ID`. Append this block to the brief instead of the one above (the child has no `herdr` CLI):

    Report back when you finish, on every exit path — done, blocked awaiting the operator,
    stopped by a stop condition, or abandoned. Your last action is:
    hc send <dispatcher-terminal> "<slug>: <done|blocked|failed> — PR #<n> <url> | suite <N> OK | parked: <ids or none> | needs human: <one line or none>"
    If hc send exits non-zero (the dispatcher is busy at a dialog), run `hc wait <dispatcher-terminal> --timeout 100` and send again; repeat up to 3 times, then tell the human the report could not be delivered. Exit 3 (no such terminal) or 11 (it has finished) means the dispatcher is gone: stop retrying and tell the human.

Same rules: pushed once, last; you never `hc wait` on the child or `hc read` its screen.

**Write the dispatch-log line before you step back.** Set `ROOT` as above and append one line per dispatch to `$ROOT/.claude/dispatch-log.md` (gitignored; create it if missing): slug, worktree pane id, workspace
id, worktree path, sent-at, expected report. This is what a resumed or compacted session reads
to know what is still outstanding — the same job the PR checklist does for tasks inside a
build. Written after stepping back it is too late; a compaction in between loses the dispatch.

In the desktop app, log the space id and terminal from `space.new` in place of the workspace and pane ids.

**When a report arrives, four things in order:**

1. **Verify the claim.** A report is a claim, not evidence — the same rule build applies to its
   implementers. Check the PR (`gh pr view <n> --json state,mergedAt,statusCheckRollup`) and, if
   it claims merged, that `main` actually contains it.
2. **Append the outcome** to `$ROOT/.claude/dispatch-log.md` (recompute `ROOT` first): what arrived, what you verified.
3. **Clean up the space you opened** — the rule below.
4. **Tell the user, in one line.** They asked for the work; the report is not for you alone.

**Cleanup rule — destructive, read it before running anything.** You opened the worktree
workspace, so you close it — but only once the branch is merged or explicitly abandoned, and
only the workspace *you* created for this slug (its id is in your `$ROOT/.claude/dispatch-log.md` line). Never
close a workspace you did not open. Never close on a `blocked` report: the operator may still
need that pane, and treat `failed` the same as `blocked` unless the human says the branch is
abandoned. Order: confirm merged or abandoned → confirm the checkout is clean
(`git -C <worktree-path> status --porcelain` empty, and no unpushed commits) →
`herdr worktree remove --workspace <workspace-id>` → `herdr workspace close <workspace-id>`
if it is still open. Remove the checkout **before** closing the workspace: `worktree remove`
takes no path, only `--workspace <ID>`, so once the workspace is closed there is no handle
left to name the checkout and it orphans on disk. `herdr workspace close` takes a workspace
id and nothing else — both verified live against herdr 0.9.1. Its release notes mention a
`--group` flag for closing a primary workspace together with its worktree children; that flag is NOT in the
0.9.1 CLI. Passing it is read as the workspace id and fails with `workspace_not_found`. Close
the child workspace by id; never reach for a group flag. If anything is ambiguous, leave the
space open and say so. An orphaned pane is cheap; a closed pane holding unpushed work is not.

**Cleanup in the Herdr desktop app** — use this instead of the herdr commands above. Recompute `ROOT` first (same command as at dispatch). Same gates, in this order: (1) confirm merged or abandoned — never on `blocked`/`failed`; (2) confirm the checkout is clean with no unpushed commits (`git -C <checkout> status --porcelain` empty, and `git -C <checkout> log origin/<type>/<slug>..HEAD` exits 0 with empty output — an error such as a missing remote branch is a fail, not a pass); (3) only then `herdr-app do space.close --id <space-id> --confirmed true`, using the space id from your `.claude/dispatch-log.md` line — it ends every terminal in that space, so if the human opened others there, ask first; (4) `git -C "$ROOT" worktree remove "$ROOT/.claude/worktrees/<slug>"` (never `--force`), and `git -C "$ROOT" branch -d <type>/<slug>` once merged — if `-d` refuses (e.g. a squash merge), leave the branch and say so; never `-D`. Close before remove so no agent is still running in the checkout; the path is the handle, so nothing orphans. If anything is ambiguous, stop before step 3.


**If a report never arrives,** you may schedule exactly one non-blocking check per dispatch — a
Monitor on the pane's agent status, or a wakeup sized to the plan's expected duration. Still no
polling (see above).

In the desktop app, that one check is `hc get <terminal>` plus `hc read --lines 40 <terminal>`, once — then tell the human what you saw. A plain shell at that terminal means the build *agent* is gone (most likely a reboot: the app brings terminals back as shells, not as the agent). The work is not: the worktree, its commits, and any uncommitted changes are still on disk. Tell the human, and offer to resume it in that shell — for a Claude build, `hc send <terminal> "claude --continue"` (the shell's folder is the worktree, so it picks up that session; the original report-back instruction is in it).
