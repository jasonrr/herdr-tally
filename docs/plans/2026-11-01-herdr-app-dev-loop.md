# Plan: dev loop on the Herdr desktop app (hc + herdr-app)

- **Goal:** /tally:plan dispatch, /tally:build report-back and /tally:review-branch reviewer panes work in the new Herdr desktop app (`hc` herdr-core 0.1.16 + `herdr-app`) AND still work in old herdr 0.9.5 (`herdr` CLI).
- **Design:** scratchpad `s_dmjxvktwrla81`
- **Branch:** `feat/herdr-app-dev-loop` — /tally:build creates/enters it per its own skill (EnterWorktree when started on main).
- **Whole-feature verify:**
  ```bash
  for f in skills/plan/SKILL.md skills/build/SKILL.md skills/review-branch/SKILL.md; do
    printf '%s new=%s old=%s\n' "$f" "$(grep -o 'HC_TERMINAL_ID' "$f" | wc -l | tr -d ' ')" "$(grep -o 'herdr agent prompt\|HERDR_ENV' "$f" | wc -l | tr -d ' ')"
  done   # every file: new>=1 AND old>=1
  git diff main -- skills | grep '^+' | grep -c 'scratchpad_\|todo_'   # expect 0
  diff <(grep -o 'hc send <dispatcher-terminal> .*' skills/plan/SKILL.md | head -1) <(grep -o 'hc send <dispatcher-terminal> .*' skills/build/SKILL.md | head -1)   # expect empty
  ```
  Plus Task 4's live smoke test.

## Facts verified live (2026-11-01, herdr-core 0.1.16) — do not re-derive
- Inside an app terminal: `$HC_TERMINAL_ID` = its address (e.g. `t5`). `HERDR_ENV` and `HERDR_PANE_ID` are EMPTY there. In old herdr panes `HERDR_ENV=1`.
- Caller kind: `hc ls --json | jq -r --arg t "$HC_TERMINAL_ID" '.terminals[] | select(.address==$t) | .agent'` → e.g. `claude-code`, `zsh`.
- Own space id: `hc get "$HC_TERMINAL_ID" --json | jq -r '.labels["herdr.space"] // empty'`.
- `herdr-app do space.new --name N --folder DIR --argv '[...]' --task "T"` prints `{"space":"sp…","tab":"tb…","terminal":"t5"}`. Args: name, folder, tab, argv, task, machine.
- `hc send <terminal> "<text>"` pastes + Enter into ANY terminal; it refuses a `blocked` target (non-zero exit) unless `--force`.
- `herdr-app do space.close --id <space-id> --confirmed true` → `{"ok":true}`; ALL its terminals end.
- A space.new terminal has no hc parent; it outlives the dispatcher, and hc sends no automatic notes about it.
- `hc run --bg --name X --cwd DIR [--label herdr.space=<id>] <argv…>` starts a background child (shown as a new tab in that space when labelled; hidden otherwise). Children end when the parent ends. Names: 1-32 chars of a-z 0-9 - _, unique among live terminals.
- `hc wait --children all` = until no child is working; "one that hasn't reported yet counts as working". Returns with a `reason`; `timeout` (default 180 s) is a check-in — wait again. `hc wait <t> --output X` ends when X is printed after the wait starts (for programs).
- `hc read [--lines N] <t>` prints screen + N history lines. `hc kill <t>` ends a terminal's program.

## Rules every task follows
- **Detection, in this order:** `$HC_TERMINAL_ID` non-empty → desktop app (even if `HERDR_ENV` is also set). Else `HERDR_ENV=1` → old herdr. Else → no multiplexer.
- **Old-herdr text stays byte-identical.** Every edit is an inserted new line/paragraph, except the ONE condition edit in Task 1 step 2. Nothing else existing is changed or deleted.
- **Mux steps stay storage-neutral** (dev-loop may leave tally later): new text names "the plan id", never tally tool names (`scratchpad_*`, `todo_*`).
- **Brief goes in a file in the checkout** (learnings.md:53: the worktree guard refuses prompt text containing "git"; a file outside the project dir can trigger a read-permission prompt that blocks a fresh agent): `<checkout>/.tally-brief.md`; the prompt/`--task` is only `Read .tally-brief.md in this folder, delete it, then follow it.` Deleting it first keeps it out of any commit.

---

## Task 1 — plan/SKILL.md: desktop-app hand-off, dispatch contract, cleanup
- **Exists because:** plan's Dispatch is the only path that creates a worktree + agent; in the app `herdr worktree create` / `agent start` do not exist, so dispatch can't happen.
- **Model:** sonnet
- **Files:** `skills/plan/SKILL.md`
- **Produces:** the report line Tasks 2 and 3 copy byte-for-byte:
  `hc send <dispatcher-terminal> "<slug>: <done|blocked|failed> — PR #<n> <url> | suite <N> OK | parked: <ids or none> | needs human: <one line or none>"`

**Steps**
1. Failing check: `grep -c 'HC_TERMINAL_ID' skills/plan/SKILL.md` → 0.
2. The ONE edit to an existing line: in the line starting `- **In herdr (\`HERDR_ENV=1\`):** one prompt carrying two questions`, replace the bold lead `**In herdr (\`HERDR_ENV=1\`):**` with `**In herdr (\`HERDR_ENV=1\`) or the Herdr desktop app (\`$HC_TERMINAL_ID\` set):**`. Rest of the line unchanged.
3. Insert a new sub-bullet BEFORE the existing `  - **Dispatch:** …` bullet (same 2-space indent), verbatim:

```markdown
  - **Dispatch, in the Herdr desktop app** (`$HC_TERMINAL_ID` set — this wins even if `HERDR_ENV` is also set; then skip the next bullet). The app has no worktree command: git makes the checkout, the app opens a space on it. Model comes from the builder-model answer; family from your kind: `hc ls --json | jq -r --arg t "$HC_TERMINAL_ID" '.terminals[] | select(.address==$t) | .agent'`. **claude-code** → argv `["claude","--model","<chosen-builder-model>"]`; **pi** → `["pi","--provider","<PI_PROVIDER>","--model","<PI_MODEL>"]`, both non-empty; **any other kind** → do not dispatch; say why and offer Inline. Main checkout: `ROOT=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")`. Then `git -C "$ROOT" fetch origin main` and `git -C "$ROOT" worktree add "$ROOT/.claude/worktrees/<slug>" -b <type>/<slug> origin/main`. If the path or branch already exists, stop and ask the human — never reuse or delete it. Write the brief to `$ROOT/.claude/worktrees/<slug>/.tally-brief.md`: checkout path, branch, the plan id, the whole-feature verify command, "run /tally:build for `plan:<slug>` through completion", and the desktop-app report block from **Dispatch contract** with your `$HC_TERMINAL_ID` filled in. Then `herdr-app do space.new --name "<title>" --folder "$ROOT/.claude/worktrees/<slug>" --argv '<argv-json>' --task "Read .tally-brief.md in this folder, delete it, then follow it."`. It prints `{"space":…,"tab":…,"terminal":…}`; keep `space` and `terminal`. The space starts the agent itself — no ctrl+d step. Write the `dispatch-log` line, then step back, as in the next bullet.
```
4. In **Dispatch contract**, insert a new paragraph after the paragraph ending "…but the brief names `agent prompt`." (that sentence wraps across two lines; insert after the second), verbatim:

```markdown
**In the Herdr desktop app** the return address is your `$HC_TERMINAL_ID`. Append this block to the brief instead of the one above (the child has no `herdr` CLI):

    Report back when you finish, on every exit path — done, blocked awaiting the operator,
    stopped by a stop condition, or abandoned. Your last action is:
    hc send <dispatcher-terminal> "<slug>: <done|blocked|failed> — PR #<n> <url> | suite <N> OK | parked: <ids or none> | needs human: <one line or none>"
    If hc send exits non-zero (the dispatcher is busy at a dialog), run `hc wait <dispatcher-terminal>` and send once more.

Same rules: pushed once, last; you never `hc wait` on the child or `hc read` its screen.
```
5. Insert a new line right after the `dispatch-log` paragraph (the one containing "sent-at, expected report."), as its own paragraph: `In the desktop app, log the space id and terminal from \`space.new\` in place of the workspace and pane ids.`
6. Insert a new paragraph right after the cleanup paragraph that ends "An orphaned pane is cheap; a closed pane holding unpushed work is not." (leave it intact), verbatim:

```markdown
**Cleanup in the Herdr desktop app.** Same gates, in this order: (1) confirm merged or abandoned — never on `blocked`/`failed`; (2) confirm the checkout is clean with no unpushed commits; (3) only then `herdr-app do space.close --id <space-id> --confirmed true`, using the space id from your `dispatch-log` line — it ends every terminal in that space, so if the human opened others there, ask first; (4) `git -C "$ROOT" worktree remove "$ROOT/.claude/worktrees/<slug>"` (never `--force`), and `git -C "$ROOT" branch -d <type>/<slug>` once merged. Close before remove so no agent is still running in the checkout; the path is the handle, so nothing orphans. If anything is ambiguous, stop before step 3.
```
7. Insert a new line right after the "If a report never arrives" paragraph, as its own paragraph: `In the desktop app, that one check is \`hc get <terminal>\` plus \`hc read --lines 40 <terminal>\`, once — then tell the human what you saw.`

- **Verify:** `grep -o 'HC_TERMINAL_ID' skills/plan/SKILL.md | wc -l` ≥ 3; `git diff main -- skills/plan/SKILL.md | grep '^-[^-]' | wc -l` = 1 (only the step-2 line removed/replaced).

## Task 2 — build/SKILL.md: report back with hc send
- **Exists because:** a build dispatched from the app has an hc terminal as its return address; `herdr agent prompt` to it fails, and a silent exit looks like a running build.
- **Model:** sonnet
- **Files:** `skills/build/SKILL.md`
- **Consumes:** Task 1's report line.

**Steps**
1. Failing check: `grep -c 'hc send' skills/build/SKILL.md` → 0.
2. Insert directly after the code block line `    herdr agent prompt <dispatcher-pane-id> "<slug>: …"` under **Last action: report to your dispatcher.**, verbatim:

```markdown
If your brief came from the Herdr desktop app (it names an hc terminal such as `t5` and says `hc send`), send the same line with:

    hc send <dispatcher-terminal> "<slug>: <done|blocked|failed> — PR #<n> <url> | suite <N> OK | parked: <ids or none> | needs human: <one line or none>"

If it exits non-zero, `hc wait <dispatcher-terminal>` and send once more.
```
3. Insert a new paragraph directly after the paragraph containing "Do NOT `herdr worktree create` here" (line 8): `In the Herdr desktop app (\`$HC_TERMINAL_ID\` set) the same holds: work in place when dispatched; on \`main\`, \`EnterWorktree\` — never \`herdr-app do space.new\` from build.`
- **Verify:** `grep -o 'hc send' skills/build/SKILL.md | wc -l` ≥ 1; `grep -o 'HC_TERMINAL_ID' skills/build/SKILL.md | wc -l` ≥ 1; `git diff main -- skills/build/SKILL.md | grep -c '^-[^-]'` = 0; the whole-feature `diff` line is empty.

## Task 3 — review-branch/SKILL.md: reviewers as hc terminals
- **Exists because:** in the app, `herdr tab create / pane split / agent wait` don't exist, so reviewers fall to the Agent path and the human loses the live view.
- **Model:** sonnet
- **Files:** `skills/review-branch/SKILL.md`
- **Consumes:** Task 1's report line.

**Steps**
1. Failing check: `grep -c 'HC_TERMINAL_ID' skills/review-branch/SKILL.md` → 0.
2. Insert a NEW paragraph directly BEFORE the paragraph that starts `In herdr (\`HERDR_ENV=1\`), run reviewers as panes` (leave that paragraph, including its `Outside herdr:` sentence, byte-identical), verbatim:

```markdown
In the Herdr desktop app (`$HC_TERMINAL_ID` set — this wins over `HERDR_ENV`), run reviewers as hc terminals the human can watch. `TREE=$(git rev-parse --show-toplevel)` (the PR branch's checkout — never the main checkout). `SPACE=$(hc get "$HC_TERMINAL_ID" --json | jq -r '.labels["herdr.space"] // empty')`. Number the reviewers 1..N. For each, write its brief to `$TREE/.review-<n>.md`, ending with: "Finish your last message with the line END OF REVIEW, then run `hc send $HC_TERMINAL_ID \"rv-<n> done\"`" (your address filled in). Start it: `hc run --bg --name rv-<n> --cwd "$TREE" --label herdr.space="$SPACE" claude --model sonnet "Read .review-<n>.md in this folder, delete it, then follow it."` — drop the `--label` when `SPACE` is empty (the reviewer runs hidden; still fine). Then `hc wait --children all`; on reason `timeout` or `message`, wait again. If a reviewer is `blocked` (a dialog), `hc read rv-<n>` and tell the human — don't answer it for them. Read each report with `hc read --lines 200 rv-<n>`; it counts only if it contains END OF REVIEW. Then `hc kill rv-<n>` for each, so the next round can reuse the names.
```
3. In the gate paragraph, insert a new sentence directly after `If you were dispatched (your brief named a dispatcher pane) and you end here, that is a \`blocked\` report, not silence — send it before you wait.`: ` If the brief named an hc terminal (desktop app), send it with \`hc send <dispatcher-terminal> "…"\` — the same line /tally:build uses.` (pure insertion; the existing sentence is unchanged — the diff shows that line as -/+ with the old text as a prefix of the new.)
- **Verify:** `grep -o 'HC_TERMINAL_ID' skills/review-branch/SKILL.md | wc -l` ≥ 1; `grep -c 'Outside herdr: Agent-tool fan-out with the same briefs.' skills/review-branch/SKILL.md` = 1; `grep -o 'herdr agent wait' skills/review-branch/SKILL.md | wc -l` = 1.

## Task 4 — live smoke test + pin facts
- **Exists because:** hc is 0.1.x and moving; learnings.md:7 shows unverified CLI claims shipped wrong before. The smoke test is the only proof that space.new + brief file + `hc send` work end-to-end.
- **Model:** sonnet. **Blocked by:** Tasks 1–3.
- **Files:** `CLAUDE.md`, `docs/learnings.md`

**Steps**
1. Live smoke. Run from the repo root (a trusted project folder). It opens and closes one space and one hidden terminal.
   ```bash
   NONCE="smoke-$RANDOM$RANDOM"
   hc run --bg --name smoke-home cat >/dev/null                 # plain program: echoes what is sent
   trap 'hc kill smoke-home >/dev/null 2>&1; [ -n "$SP" ] && herdr-app do space.close --id "$SP" --confirmed true >/dev/null' EXIT
   printf 'Run exactly this shell command, then stop: hc send smoke-home "%s"\n' "$NONCE" > .tally-brief.md
   OUT=$(herdr-app do space.new --name tally-smoke --folder "$PWD" --argv '["claude","--model","haiku"]' --task "Read .tally-brief.md in this folder, delete it, then follow it.")
   echo "$OUT"; SP=$(echo "$OUT" | jq -r .space); T=$(echo "$OUT" | jq -r .terminal)
   hc wait smoke-home --output "$NONCE" --timeout 180 && echo SMOKE PASS || { echo SMOKE FAIL; hc read --lines 40 "$T"; }
   rm -f .tally-brief.md
   ```
   PASS = `SMOKE PASS` (the nonce appears only if the child sent it). On FAIL, the printed child screen says why (trust or permission dialog = environment, report it to the human; don't add `--dangerously-*` flags or change settings). Do not commit `.tally-brief.md`.
2. In `CLAUDE.md`, insert after the `## herdr integration gotchas (verified live against herdr 0.9.5)` section (before the next `## `), verbatim:

```markdown
## Herdr desktop app (verified live against herdr-core 0.1.16)

The desktop app replaces the `herdr` CLI with `hc` (terminals: run, send, wait, read) and
`herdr-app` (spaces/tabs the person sees). Dev-loop skills branch on `$HC_TERMINAL_ID`
(set in app terminals; `HERDR_ENV` is empty there) before `HERDR_ENV=1`.
- No worktree command: `git worktree add` + `herdr-app do space.new --folder … --argv '[…]' --task …`
  (prints `{"space","tab","terminal"}`). Close with `space.close --id <sp> --confirmed true`.
- Report-back is `hc send <terminal> "<line>"` — works to any terminal, not only children.
- Briefs go in `.tally-brief.md` inside the checkout (read, deleted, then followed).
- Read `hc --skill` and `herdr-app --skill` before changing these steps; verify flags with `--help`.
```
3. Append to `docs/learnings.md` a new section at the end, verbatim plus the smoke result:

```markdown
## herdr-app-dev-loop (2026-11-01)
- The Herdr desktop app leaves `HERDR_ENV` unset in its terminals, so any `HERDR_ENV=1` gate silently routes app sessions to the no-multiplexer path. Gate on `$HC_TERMINAL_ID` first.
- A smoke test that greps the caller's own screen passes on its own command text. Assert on a nonce sent to a separate `cat` terminal (`hc wait <t> --output <nonce>`).
```
   Then one more bullet: `- Smoke on <date>: PASS` or what stalled.
- **Verify:** smoke prints `SMOKE PASS`; `grep -c 'herdr-core 0.1.16' CLAUDE.md` = 1; `git status --porcelain | grep -c tally-brief` = 0.

## Out of scope (noted, not built)
- `src/tui/app.rs:212` and `scripts/verify.sh` also gate on `HERDR_ENV`; both are TUI/plugin surface (todo `t_dmjxvkwq46tk2`).

## Order
Task 1 → Tasks 2 and 3 (different files) → Task 4.
