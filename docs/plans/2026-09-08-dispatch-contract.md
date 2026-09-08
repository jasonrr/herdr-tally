# Plan: dispatch contract, builder model, build-time learnings (plan:dispatch-contract)

**Goal:** close GitHub issues #15, #16, #17 — one branch, one PR. Give the dev loop a
configured builder model, a return path from every dispatched build, and learnings written
while the build is happening instead of only at the end.

**Design scratchpad:** `s_dla2gkws9goo1` — read it first. It records the deliberate changes to
the issues as written (the hook change is cut; #16's "pane-cleanup rule" does not exist and must
be written here) and reconciles #16 with a prior design decision it looks like it reverses.

**Branch:** `feat/dispatch-contract`

**Whole-feature verify:**

```sh
# 1. The two copies of the report line are byte-identical (one contract, two files)
diff <(grep -F 'herdr agent prompt <dispatcher-pane-id>' skills/plan/SKILL.md) \
     <(grep -F 'herdr agent prompt <dispatcher-pane-id>' skills/build/SKILL.md) && echo "report line in sync"
# 2. Nothing in the repo regressed
cargo test && cargo clippy && cargo fmt --check
```

**Prose only.** No Rust, no store change, no MCP tool change, no `hooks/session-start.sh` change.
Do not touch the `<dev-loop>` block — it is duplicated by hand in `hooks/session-start.sh` and
`skills/setup/SKILL.md` §2, and this work is scoped to avoid that trap entirely.

**Two editing notes for whoever executes this.**


*Editing this pad:* several steps below quote literal markdown headings inside fenced blocks.
tally's heading parser is not code-fence aware, so a section-targeted `scratchpad_edit` against
this plan truncates at the first fenced `##` and leaks the old tail back. Use a full
`scratchpad_write`, or target line ranges. (Found the hard way while writing it.)

*Editing the skill files:* steps 1e, 1f, 2a and 2b each quote a sentence formatted here as its
own blockquote, but in `skills/plan/SKILL.md` all four live inside **one long run-on line**
(the `**Dispatch:**` bullet, currently line 46). They are distinct, non-overlapping substrings,
so four sequential exact-string `Edit` calls work — just don't expect to find four separate
lines. Same shape in `skills/review-branch/SKILL.md`: steps 2e and 3b target different
sentences of the one `## Gate` paragraph.

---

## Task 1 — builder model: default in config, confirmed at dispatch (#15)

**Exists because:** a dispatched build controller launched bare takes its model from the user's
global `~/.claude/settings.json`. On 2026-09-08 three build controllers came up on Fable 5.1 —
the frontier tier — to do work that is only dispatch, verify, adjudicate. The dumb alternative
(hardcode `--model opus` in the skill) removes the user's ability to pick, and `sonnet` is the
right answer for a plan that pins every task to sonnet/haiku.

**UX shape — decided 2026-09-08, differs from #15 as filed.** The config key is a **default**,
not the final word. `/tally:setup` sets it once; `/tally:plan` offers it at dispatch time,
prefilled, inside the AskUserQuestion it *already* asks — so the human confirms or overrides the
tier for this specific build, at the moment they can see what the plan actually contains. No
extra prompt: the existing dispatch question gains a second question in the same call.

**Model:** `sonnet`

**Files:** `skills/setup/SKILL.md`, `skills/plan/SKILL.md`

**Interfaces produced:** the config key `builder-model: <value>` on its own line in
`.claude/tally-dev-loop.md`, read by `grep '^builder-model:'`. Task 2 edits the same Dispatch
bullet in `skills/plan/SKILL.md`, so do this task first.

### Steps

**1a. `skills/setup/SKILL.md` §1 — let an existing file gain the key.** Replace:

> If `.claude/tally-dev-loop.md` already exists, show it and ask what to change — routing on/off, lens edits — then apply just that and stop. The full flow below is for first-time setup.

with:

> If `.claude/tally-dev-loop.md` already exists, show it and ask what to change — routing on/off, builder model, lens edits — then apply just that and stop. A file written before the `builder-model` key existed simply lacks it; offer to add it. The full flow below is for first-time setup.

**1b. `skills/setup/SKILL.md` — insert a new section between §2 (Routing) and §3 (Reviewer
lenses), then renumber the two sections that follow (Reviewer lenses → 4, Write and recap → 5).**
The new section, verbatim:

```markdown
## 3. Builder model — set the default

`/tally:plan` dispatches a build controller into its own pane. Launched bare, a Claude-kind
agent takes its model from the user's global `~/.claude/settings.json`, so the controller lands
on whatever tier that names — usually the most expensive one. The controller only dispatches,
verifies and adjudicates; the plan already pins a model per task for the implementers who do
the work.

Ask (one AskUserQuestion) which model build controllers should default to: `opus` (recommended
default), `sonnet` (right when plans pin every task to sonnet or haiku), or `fable`. Write the
answer as `builder-model: <value>`.

Say what the key is, in one line: it is the **default offered at dispatch, not a lock** —
`/tally:plan` shows it prefilled and the human can override it per build. Only Claude- and
Pi-kind panes consume it; other agent kinds launch bare, as today.
```

**1c. `skills/setup/SKILL.md` — the write template in the (now) §5 "Write and recap".** Replace:

```
    # dev loop
    routing: on
```

with:

```
    # dev loop
    routing: on
    builder-model: opus
```

The `## Lenses` block below it is unchanged.

**1d. `skills/plan/SKILL.md` — the dispatch AskUserQuestion gains a second question.** Replace:

> - **In herdr (`HERDR_ENV=1`):** one AskUserQuestion — dispatch to a new worktree / build inline here / defer.

with:

> - **In herdr (`HERDR_ENV=1`):** one AskUserQuestion carrying two questions. First: dispatch to a new worktree / build inline here / defer. Second, "builder model?" — offer the repo's configured default first, with `(default)` in its label, then the other two of `opus` / `sonnet` / `fable`. Read the default from `.claude/tally-dev-loop.md` (`grep '^builder-model:'`), falling back to `opus` when the key or the file is absent. One prompt, two questions — never a second round trip. The model answer applies only to the Dispatch branch; on Inline or Defer, ignore it.

**1e. `skills/plan/SKILL.md` — the Dispatch bullet's opening sentence.** Replace:

> **Dispatch:** preserve the caller's CLI/model family instead of accepting the worktree's default agent. Before creating it, read the current kind from `herdr agent get "$HERDR_PANE_ID"` at `.result.agent.agent`; for Pi also retain `PI_PROVIDER` + `PI_MODEL`.

with:

> **Dispatch:** preserve the caller's CLI *family*; take the model from the answer to the builder-model question above, never from the caller's session. Before creating the worktree, read the current kind from `herdr agent get "$HERDR_PANE_ID"` at `.result.agent.agent`; for Pi also retain `PI_PROVIDER` + `PI_MODEL`.

**1f. `skills/plan/SKILL.md` — the agent-start sentence.** Replace:

> then `herdr agent start build-<slug> --kind <caller-kind> --pane <pane-id>`; when the caller is Pi, require non-empty `PI_PROVIDER` + `PI_MODEL` and append `-- --provider "$PI_PROVIDER" --model "$PI_MODEL"`.

with:

> then `herdr agent start build-<slug> --kind <caller-kind> --pane <pane-id>`, appending an agent-arg tail chosen by kind: **claude** → `-- --model <chosen-builder-model>`; **pi** → `-- --provider "$PI_PROVIDER" --model "$PI_MODEL"`, requiring both non-empty; **any other kind** → no tail, it launches bare as today. `herdr agent start` accepts the tail after `--` (`[-- [AGENT_ARG]...]`, verified live against herdr 0.9.0).

**Deliberately not done here:** #15 step 3 asks `/tally:build` to read the key when it fans out
into sibling worktrees. `skills/build/SKILL.md` explicitly forbids build from creating worktrees
("Do NOT `herdr worktree create` here"), so there is nothing to read the key for. Cut, not
forgotten. #15 step 3's hook change is cut too — see the design pad.

**Known ceiling, do not try to close it:** `herdr agent get` reports the agent *kind*
(`.result.agent.agent`), not the model it launched with. Nothing can assert after the fact that
`-- --model <x>` took effect. Don't write a check that pretends to.

**Verify:**

```sh
grep -n 'builder-model' skills/setup/SKILL.md skills/plan/SKILL.md   # expect hits in both
! grep -q "caller's CLI/model family" skills/plan/SKILL.md && echo "old wording gone"
grep -n 'two questions' skills/plan/SKILL.md                         # one prompt, not two
grep -n 'Write and recap' skills/setup/SKILL.md                      # renumbered to 5
```

---

## Task 2 — dispatch report-back contract (#16)

**Exists because:** on 2026-09-08 three dispatched builds finished — two merged their own PRs,
one parked its last task — and the dispatching session learned none of it. The human noticed
instead. Todos and `build-log:` are a pull surface; nothing pushes. The dumb alternative
(`herdr agent wait` on each dispatch) blocks the dispatcher and was explicitly rejected by the
worktree-dispatch design.

**Read before writing:** design scratchpad `s_dla2gkws9goo1`, section "Reconciling with the
worktree-dispatch design". The earlier design rejected *the dispatcher pulling*. This adds *the
child pushing*, once, at its exit. Do not soften it back into silence, and do not add a blocking
wait anywhere.

**Model:** `sonnet`

**Files:** `skills/plan/SKILL.md`, `skills/build/SKILL.md`, `skills/review-branch/SKILL.md`

**Interfaces consumed:** task 1's edits to the same Dispatch bullet — apply task 1 first.

**Interfaces produced:** one report-line format string appearing verbatim in BOTH
`skills/plan/SKILL.md` and `skills/build/SKILL.md`. One contract, two files; the whole-feature
verify diffs them. Copy it, do not paraphrase it.

### Steps

**2a. `skills/plan/SKILL.md` — extend the dispatch brief.** In the Dispatch bullet, replace:

> Build owns the rest (materialize the plan file, implement task-by-task, open the PR, run review-branch, push fixes). No session history in the brief.

with:

> Build owns the rest (materialize the plan file, implement task-by-task, open the PR, run review-branch, push fixes). No session history in the brief. The brief also carries the return address and report line from **Dispatch contract** below — a dispatch without one is the bug this contract exists to prevent.

**2b. `skills/plan/SKILL.md` — the step-back sentence.** Replace:

> Then **step back** — say so; the space's agent owns it and talks to the user directly. Don't `herdr agent wait` on it or mirror the pane; progress rides the tally todos + `build-log:<slug>` scratchpad.

with:

> Then write the `dispatch-log` line (below) and **step back** — say so; the space's agent owns it and talks to the user directly. Don't `herdr agent wait` on it or mirror the pane; progress rides the tally todos + `build-log:<slug>` scratchpad until the child reports.

**2c. `skills/plan/SKILL.md` — append this section at the very end of the file**, after the
"**Outside herdr:** `EnterWorktree`, then /tally:build inline." line:

```markdown
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
```

**2d. `skills/build/SKILL.md` — report on every exit path.** After the file's last line
("When every task is complete, open a PR ... Review fixes land on the PR as follow-up commits.")
append:

```markdown
**Last action: report to your dispatcher.** If your brief named a dispatcher pane, your final act
on **every** exit path — all tasks done, blocked awaiting the operator, stopped by a stop
condition, or abandoned — is the one-line report the brief specified:

    herdr agent prompt <dispatcher-pane-id> "<slug>: <done|blocked|failed> — PR #<n> <url> | suite <N> OK | parked: <ids or none> | needs human: <one line or none>"

Send it once, last, after the PR and review-branch. Exiting without it is the failure this
contract exists to prevent: a silent exit looks exactly like a build still running. If your brief
named no dispatcher pane you were invoked directly — no report, everything else unchanged.
```

**2e. `skills/review-branch/SKILL.md` — ending in "awaiting the human" is a report, not
silence.** In the `## Gate` section, replace:

> Then present the real options — the PR is already open, so: mark it ready / merge it / leave it for the user — and wait for the user's choice.

with:

> Then present the real options — the PR is already open, so: mark it ready / merge it / leave it for the user — and wait for the user's choice. If you were dispatched (your brief named a dispatcher pane) and you end here, that is a `blocked` report, not silence — send it before you wait.

**Verify:**

```sh
# one contract, two files, byte-identical
diff <(grep -F 'herdr agent prompt <dispatcher-pane-id>' skills/plan/SKILL.md) \
     <(grep -F 'herdr agent prompt <dispatcher-pane-id>' skills/build/SKILL.md) && echo "in sync"
grep -n 'Dispatch contract' skills/plan/SKILL.md
grep -n 'dispatch-log' skills/plan/SKILL.md
grep -n 'Cleanup rule' skills/plan/SKILL.md
grep -n 'report, not silence' skills/review-branch/SKILL.md
! grep -q 'agent wait <dispatcher' skills/plan/SKILL.md && echo "no blocking wait added"
```

---

## Task 3 — learnings during the build, and a heading shape (#17)

**Exists because:** all learnings from three builds landed only when review-branch ran at the
end, though each build hit surprises worth one line at the moment they happened. Some resurfaced
at review; some now live only in a build-log or a todo comment. Separately, the review-branch
append landed with no heading and ran into the previous section's last bullet. The dumb
alternative (keep relying on review-branch) is exactly what produced the miss.

**Model:** `sonnet`

**Files:** `skills/build/SKILL.md`, `skills/review-branch/SKILL.md`, `skills/debug/SKILL.md`

**Interfaces consumed:** task 2 also edits `skills/build/SKILL.md` and
`skills/review-branch/SKILL.md` — apply task 2 first.

### Steps

**3a. `skills/build/SKILL.md` step 6.** Replace:

> 6. **Record.** `todo_complete`, plus one ledger line in a `build-log:<slug>` scratchpad: commits, deviations from plan, parked findings. Files survive context compaction — the ledger is what stops a resumed session from re-dispatching finished work.

with:

> 6. **Record.** `todo_complete`, plus one ledger line in a `build-log:<slug>` scratchpad: commits, deviations from plan, parked findings. Files survive context compaction — the ledger is what stops a resumed session from re-dispatching finished work. **If the task surprised you** — a plan defect, a tool or classifier refusal, a verify command that could not pass at this task, a finding you had to park — also append one `pattern → consequence` line to the `learnings`-tagged scratchpad (create it if missing), under a dated heading for this work: `## <slug> (PR #<n>, YYYY-MM-DD)`, preceded by a blank line if one isn't already there, one bullet per line. Append bare bullets and they run into the previous section's last bullet. The build-log says what happened here; learnings says what the next planner should not repeat.

**3b. `skills/review-branch/SKILL.md` `## Gate`.** Replace:

> Append one line per surprise the review surfaced to a `learnings`-tagged tally scratchpad (`pattern → consequence`; create it if missing).

with:

> Append one line per surprise the review surfaced to a `learnings`-tagged tally scratchpad (`pattern → consequence`; create it if missing), under a dated heading for this work — `## <slug> (PR #<n>, YYYY-MM-DD)`, preceded by a blank line if one isn't already there, one bullet per line. If the build already opened that heading, append under it rather than opening a second one.

**3c. `skills/debug/SKILL.md`, last section.** Replace:

> When the root cause would surprise the next session, append one line to a `learnings`-tagged tally scratchpad — `pattern → consequence` (create the scratchpad if missing).

with:

> When the root cause would surprise the next session, append one line to a `learnings`-tagged tally scratchpad — `pattern → consequence` (create the scratchpad if missing), under a dated heading — `## <slug or short label> (YYYY-MM-DD)`, preceded by a blank line if one isn't already there, one bullet per line.

**3d.** `skills/plan/SKILL.md`'s "Research before tasks" section needs no edit — it already says
to skim `learnings`- and `build-log:`-tagged scratchpads. This task is what gives that
instruction build-time material to find, which is #17's closing point.

**Verify:**

```sh
grep -c 'YYYY-MM-DD' skills/build/SKILL.md skills/review-branch/SKILL.md skills/debug/SKILL.md  # 1 each
grep -n 'pattern → consequence' skills/build/SKILL.md   # build now carries it too
```
