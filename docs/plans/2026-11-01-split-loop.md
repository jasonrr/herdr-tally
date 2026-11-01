# Plan: split-loop

**Goal:** the six dev-loop skills, the routing hook, and the pi routing extension become a second, store-free plugin named `loop` (`/loop:brainstorm` … `/loop:setup`). Git files, a draft PR checklist, and `gh issue` replace every tally scratchpad/todo/comment the loop used. tally keeps the store, CLI, MCP, TUI, and `pi/skills/tally`.
**Design:** scratchpad `s_dmk13ao0gs941` (approved, option A). Revision 2 of this plan: all findings from the two plan reviews folded in.
**Branch:** `feat/split-loop`
**Packaging decision made here (within the design's stated alternative):** the `loop` plugin lives in this repo under `loop/`, listed as a second plugin in `.claude-plugin/marketplace.json` with `"source": "./loop"`. Precedent: `claude-code-warp` (`./plugins/warp`, ships its own `hooks/hooks.json`), `impeccable` (`./plugin`) in the installed marketplaces. Reason: one PR, one review, no cross-repo build. Moving it to its own repo later is a `git mv` plus one marketplace line. Install: `claude plugin install loop@tally`.
**Whole-feature verify** (save as a script, run from the repo root with `bash`, must exit 0):
```bash
#!/usr/bin/env bash
set -euo pipefail
if grep -rnE 'scratchpad_|todo_(create|list|lock|complete|update|add_blocker)|comment_(add|list)|/tally:|tally todo|tally scratchpad|tally comment' loop/; then echo "tally leak in loop/"; exit 1; fi
test ! -e skills
test ! -e hooks
test ! -e pi/extensions/routing.ts
test ! -e tests/hooks/session_start_test.sh
jq -e '.plugins | map(.name) | index("loop") != null' .claude-plugin/marketplace.json
jq -e '.name == "loop"' loop/.claude-plugin/plugin.json
jq -e '.pi.skills == ["pi/skills","loop/skills"] and .pi.extensions == ["pi/extensions","loop/pi/extensions"]' package.json
grep -q 'loop/.claude-plugin/plugin.json' scripts/release.sh
grep -q 'loop/.claude-plugin/plugin.json' release.toml
bash -n loop/hooks/session-start.sh
bash loop/hooks/session-start.test.sh
node --experimental-strip-types --input-type=module -e 'const m = await import(new URL("loop/pi/extensions/routing.ts", "file://" + process.cwd() + "/")); if (typeof m.default !== "function") process.exit(1)'
for t in tests/*.test.sh; do bash "$t"; done
cargo test --quiet
grep -q '/loop:setup' src/tui/view.rs
```
(`tests/*.test.sh` are the existing shell tests; `install.test.sh` and `pi-ask-user.test.sh` stub external commands — read their headers if one fails for an environment reason and report it rather than "fixing" the test.)

## Research notes (patterns to follow, cite in briefs)
- Tally dependency inventory per skill: in the design's table and this plan's tasks; taken from the files at commit 410307e.
- `docs/learnings.md` rules that bind here: "two readers of one config file must share one parser semantics" (dev-loop-v2) → hook, routing.ts and the TUI keep exact-line matching for `routing: on`; "a path pattern with a date named in two skills must say in both where the date comes from" (durable-records) → every skill that names `docs/plans/YYYY-MM-DD-<slug>.md` says: the date is the one in the plan file's own name, set once by /loop:plan; "bare `git stash` is shared across worktrees" → Defer must not suggest stash; "manifests that embed the GitHub remote must be checked against `git remote -v`" → URLs copied from the existing plugin.json (`jasonrr/herdr-tally`, correct).
- `pi/extensions/` also holds `ask-user-bridge.ts` (uses `createRequire` on `pi-ask-user`; unaffected). Only `routing.ts` moves.
- Node here is v22.23.0; `--experimental-strip-types` loads the TS extension (a harmless ESM-reparse warning prints).
- `.gitignore` already ignores `.claude/tally-dev-loop.md`, `.claude/worktrees/`, `.tally-brief.md`, `.review-*.md`. The brief file keeps its `.tally-brief.md` name (renaming touches `.gitignore`, CLAUDE.md and the plan skill for no behavior change).
- TUI footer: `src/tui/app.rs::devloop_status` (line ~244) reads only `.claude/tally-dev-loop.md`; `src/tui/view.rs:1046` prints `dev loop: not set up · /tally:setup`; tests at `src/tui/app.rs` ~1865–1900 write `tally-dev-loop.md` into a temp dir.
- Release: `scripts/release.sh` lines 35–36 bump `.claude-plugin/plugin.json` and `package.json` with perl; `release.toml` has one `[[pre-release-replacements]]` block per file.
- `tests/hooks/session_start_test.sh` hardcodes `HOOK=hooks/session-start.sh` and asserts `tally:brainstorm`; Task 2's test supersedes it.
- `sed -i ''` is BSD sed; the project is macOS-only (CLAUDE.md), so that is fine.

## Shared text used by several tasks

**Config file lookup (CFG rule).** Wherever a skill, the hook, routing.ts, or the TUI reads the dev-loop config: use `.claude/dev-loop.md`; if it does not exist, use `.claude/tally-dev-loop.md`. Prose form, used verbatim in skills: "`.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md` when only that exists)".

**ROOT (ROOT rule).** The main checkout: `ROOT=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")`. Every bullet below that uses `$ROOT` begins by setting it with this exact command — never assume it is set from another bullet.

**Plan-file conventions (PLAN rule).** The plan is `docs/plans/YYYY-MM-DD-<slug>.md`; date = the day /loop:plan wrote it, never recomputed. Every task is a level-2 heading that is exactly `## Task N` (no trailing spaces, never inside a code fence) with `**<title>**` on the next line. GitHub turns `## Task 3` into the anchor `#task-3`. The design is `docs/decisions/YYYY-MM-DD-<slug>.md` with the same basename.

**Move-in (MOVE rule).** Moving the two uncommitted files from the main checkout into a worktree `WT` — move, not copy; a copy left behind blocks the later `git pull` that brings the committed file to main:
```bash
mkdir -p "$WT/docs/plans" "$WT/docs/decisions"
mv "$ROOT/docs/plans/<plan-file>" "$WT/docs/plans/"
[ -f "$ROOT/docs/decisions/<design-file>" ] && mv "$ROOT/docs/decisions/<design-file>" "$WT/docs/decisions/"
```

**PR checklist (LEDGER rule).** Draft PR body:
```
Plan: docs/plans/<plan-file>

- [ ] T1 — <title> (<BASE>/blob/<type>/<slug>/docs/plans/<plan-file>#task-1)
```
`BASE=$(gh repo view --json url -q .url)`. Tick task N (guarded so a failed read can never blank the body):
```bash
N=<n>; b=$(gh pr view --json body -q .body) && [ -n "$b" ] && printf '%s\n' "$b" | sed "s/^- \[ \] T$N /- [x] T$N /" | gh pr edit --body-file -
```
`gh pr view`/`gh pr edit` with no number act on the current branch's PR. Unticked lines are the not-done list.

**Follow-up issue (ISSUE rule).** Prose never goes inside double quotes on the command line — it carries backticks, `$`, and quotes. Always:
```bash
gh issue create --title '[p2] <one line, no quotes>' --body-file - <<'EOF'
<failure scenario or question>
From: <PR #n url | design docs/decisions/<file>>
EOF
```
Severity in the title prefix; no labels (they would need creating per repo).

---

## Task 1
**Move the loop into `loop/` and register it as a second plugin**

- **Exists because:** without a separate plugin root there is nothing to install as `/loop:*`, and the tally plugin would keep shipping the skills.
- **Model:** sonnet
- **Files:** `skills/` → `loop/skills/`, `hooks/` → `loop/hooks/`, `pi/extensions/routing.ts` → `loop/pi/extensions/routing.ts`, delete `tests/hooks/session_start_test.sh`, new `loop/.claude-plugin/plugin.json`, edit `.claude-plugin/marketplace.json`, `.claude-plugin/plugin.json`, `package.json`, `.gitignore`, `scripts/release.sh`, `release.toml`.
- **Steps:**
  1. Failing checks first — run these; each fails now: `test ! -e skills`; `jq -e '.plugins | map(.name) | index("loop") != null' .claude-plugin/marketplace.json`; `jq -e '.name == "loop"' loop/.claude-plugin/plugin.json`; the `package.json` jq line from the whole-feature verify; `grep -q 'loop/.claude-plugin/plugin.json' scripts/release.sh`.
  2. `mkdir -p loop/pi/extensions && git mv skills loop/skills && git mv hooks loop/hooks && git mv pi/extensions/routing.ts loop/pi/extensions/routing.ts && git rm tests/hooks/session_start_test.sh` (superseded by Task 2's test; `rmdir tests/hooks` if now empty). `pi/extensions/ask-user-bridge.ts` stays.
  3. Write `loop/.claude-plugin/plugin.json`:
     ```json
     {
       "name": "loop",
       "description": "The dev loop: brainstorm → plan → build → review-branch, plus debug and setup. Store-free — designs and plans are committed files, the task ledger is a draft PR checklist, follow-ups are GitHub issues.",
       "version": "0.1.0",
       "author": { "name": "Jason Rosoff" },
       "homepage": "https://github.com/jasonrr/herdr-tally",
       "repository": "https://github.com/jasonrr/herdr-tally",
       "license": "MIT",
       "keywords": ["dev-loop", "skills", "herdr"]
     }
     ```
  4. `.claude-plugin/marketplace.json`: `description` → `"tally — shared todos, scratchpads & plans; loop — the dev loop skills."`; tally entry description → `"Project-scoped todos, scratchpads & plans for you and your agents."`; append to `plugins`:
     ```json
     { "name": "loop", "description": "The dev loop skills (/loop:brainstorm, plan, build, review-branch, debug, setup). No tally store required.", "source": "./loop", "category": "productivity" }
     ```
  5. `.claude-plugin/plugin.json`: description → `"Project-scoped todos, scratchpads & plans for you and your agents."`; remove `"dev-loop"` from `keywords`.
  6. `package.json`: description → `"Project-scoped todos, scratchpads & plans, plus the dev loop — as a pi package."`; `pi.skills` → `["pi/skills", "loop/skills"]`; `pi.extensions` → `["pi/extensions", "loop/pi/extensions"]`.
  7. Slash-command rename only: `grep -rl '/tally:' loop/ | xargs sed -i '' 's#/tally:#/loop:#g'`. (Content about tally objects is rewritten in Tasks 3–6.)
  8. `.gitignore`: under the "Personal dev-loop config" comment add `.claude/dev-loop.md` and `.claude/dispatch-log.md` (keep the old line).
  9. Release bumps both plugins in lockstep (same repo, one tag). `scripts/release.sh`: after line 36 add `NEXT=$next perl -0pi -e 's/("version": ")[^"]+/${1}$ENV{NEXT}/' loop/.claude-plugin/plugin.json`, and extend the comment above to say both plugin.json files. `release.toml`: add a fourth `[[pre-release-replacements]]` block identical to the `.claude-plugin/plugin.json` one with `file = "loop/.claude-plugin/plugin.json"`. Set `loop/.claude-plugin/plugin.json` `version` to the current tally version (`jq -r .version .claude-plugin/plugin.json`) instead of `0.1.0`, so the next release bumps both to the same number.
  10. `git add -A loop .claude-plugin package.json .gitignore scripts/release.sh release.toml` (the `git mv`/`git rm` are already staged; do not name `skills`, `hooks` — they no longer exist). Commit.
- **Verify:** the five step-1 checks pass; `grep -rn '/tally:' loop/` prints nothing; `git status --porcelain` is empty.

## Task 2
**One routing block, read by both the hook and the pi extension; config file renamed with fallback**

- **Exists because:** three hand-synced copies of the routing block already drifted once (routing.ts says so) and all three now need the same rewrite; the consent file's name must stop saying "tally" without breaking repos that have the old file.
- **Model:** sonnet
- **Files:** new `loop/hooks/routing.md`, `loop/hooks/session-start.sh`, new `loop/hooks/session-start.test.sh`, `loop/pi/extensions/routing.ts`.
- **Steps:**
  1. Write `loop/hooks/routing.md` (exact, trailing newline, no tabs or control characters — the hook's escaper handles only `\`, `"`, tab and CR):
     ```
     <dev-loop>
     This project uses the dev loop. Standing routing — the human should never have to ask:
     - Feature or non-trivial change requested → run /loop:brainstorm before writing any code.
     - Bug, failing test, or unexpected behavior → run /loop:debug before proposing any fix.
     - Approved design (an uncommitted docs/decisions/*.md) or any multi-step implementation → run /loop:plan before building.
     - A plan file (docs/plans/*.md) named in your brief, or one with an open draft PR → execute it with /loop:build.
     - Branch complete, or a merge/PR is requested → run /loop:review-branch first.
     Trivial one-file changes and pure questions are exempt. When this rule triggers a skill, say so in one line.
     </dev-loop>
     ```
  2. Test first — `loop/hooks/session-start.test.sh` (exact), `chmod +x`:
     ```bash
     #!/usr/bin/env bash
     # Runs session-start.sh against temp repos: new name, old name, routing off, no file.
     set -euo pipefail
     here="$(cd "$(dirname "$0")" && pwd)"
     run() { CLAUDE_PROJECT_DIR="$1" bash "$here/session-start.sh"; }
     t="$(mktemp -d)"; trap 'rm -rf "$t"' EXIT
     mkdir -p "$t/new/.claude" "$t/old/.claude" "$t/off/.claude" "$t/none"
     printf '# dev loop\nrouting: on\n' > "$t/new/.claude/dev-loop.md"
     printf '# dev loop\nrouting: on\n' > "$t/old/.claude/tally-dev-loop.md"
     printf '# dev loop\nrouting: off\n' > "$t/off/.claude/dev-loop.md"
     run "$t/new" | jq -e '.hookSpecificOutput.additionalContext | test("/loop:plan") and test("<dev-loop>") and (test("tally") | not)' >/dev/null
     run "$t/old" | jq -e '.hookSpecificOutput.additionalContext | test("/loop:build")' >/dev/null
     [ -z "$(run "$t/off")" ]
     [ -z "$(run "$t/none")" ]
     echo "session-start.test.sh: OK"
     ```
     Run it: the `new` case fails (old hook reads only the old name).
  3. Replace `loop/hooks/session-start.sh` with (exact):
     ```bash
     #!/usr/bin/env bash
     # SessionStart hook for the loop plugin. Emits the dev-loop routing block ONLY
     # when this repo opted in via .claude/dev-loop.md (or the older
     # .claude/tally-dev-loop.md) containing `routing: on` — the file /loop:setup
     # writes. Silent no-op everywhere else: consent lives in the repo, not the install.
     set -euo pipefail

     dir="${CLAUDE_PROJECT_DIR:-.}/.claude"
     cfg="$dir/dev-loop.md"
     [ -f "$cfg" ] || cfg="$dir/tally-dev-loop.md"
     if [ ! -f "$cfg" ] || ! grep -q '^routing: on$' "$cfg" 2>/dev/null; then
       exit 0
     fi

     # routing.md is the single copy of the block; JSON-escape it for additionalContext.
     ctx="$(awk '{ gsub(/\r/, ""); gsub(/\\/, "\\\\"); gsub(/"/, "\\\""); gsub(/\t/, "\\t"); printf "%s\\n", $0 }' "$(dirname "$0")/routing.md")"
     printf '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"%s"}}\n' "$ctx"
     ```
  4. `loop/pi/extensions/routing.ts`: delete the `DEV_LOOP_BLOCK` template literal **and the comment block above it**; in their place:
     ```ts
     // Single copy of the routing block, shared with hooks/session-start.sh.
     // Read at import so a wrong path fails loudly at load, not silently per turn.
     // Assumes pi loads this file in place (it does today); a bundler would break the relative path.
     const DEV_LOOP_BLOCK = readFileSync(new URL("../../hooks/routing.md", import.meta.url), "utf8").trimEnd();
     ```
     Replace the `routingEnabled` function **and the comment block above it** with:
     ```ts
     // Mirrors hooks/session-start.sh: .claude/dev-loop.md, else the older
     // .claude/tally-dev-loop.md; a `routing: on` line, matched exactly (no CRLF).
     function routingEnabled(cwd: string): boolean {
       for (const name of ["dev-loop.md", "tally-dev-loop.md"]) {
         try {
           const cfg = readFileSync(join(cwd, ".claude", name), "utf8");
           return cfg.split("\n").some((line) => line === "routing: on");
         } catch {}
       }
       return false;
     }
     ```
     Leave the rest as is; change any remaining "tally dev-loop" in comments to "dev-loop".
  5. Commit `loop/hooks/routing.md loop/hooks/session-start.sh loop/hooks/session-start.test.sh loop/pi/extensions/routing.ts`.
- **Verify:** `bash loop/hooks/session-start.test.sh` prints OK; the whole-feature verify's `node --experimental-strip-types …` line exits 0.

## Task 3
**Rewrite brainstorm, debug, and setup to use files and issues**

- **Exists because:** these three skills still tell the agent to write scratchpads and todos.
- **Model:** sonnet
- **Files:** `loop/skills/brainstorm/SKILL.md`, `loop/skills/debug/SKILL.md`, `loop/skills/setup/SKILL.md`.
- **Steps (read each file in full first; edit only what is named):**
  1. **brainstorm** — frontmatter `description`: "lands an approved design in a tally scratchpad" → "lands an approved design in docs/decisions/". Replace the first paragraph of `## Output` with:
     > Write the design to `docs/decisions/YYYY-MM-DD-<slug>.md` in the main checkout (today's date; `<slug>` is a short kebab-case name you will reuse for the branch). `mkdir -p docs/decisions` first. Do **not** commit it: /loop:plan moves it into the build's worktree, where build commits it as the feature's decision record. Sections: Problem, Chosen approach, Cut and why, Open questions. Keep it under a page — it's a handoff artifact for the next session, not a spec ritual. For any question deferred rather than answered, open an issue:
     > ```bash
     > gh issue create --title '[p3] <question, one line>' --body-file - <<'EOF'
     > Deferred from design docs/decisions/<file>.
     > EOF
     > ```
     Last paragraph: "If `.claude/tally-dev-loop.md` doesn't exist in this repo" → "If neither `.claude/dev-loop.md` nor `.claude/tally-dev-loop.md` exists in this repo".
  2. **debug** — "Fix and exit", third bullet: "document what you ruled out in a tally scratchpad, add handling" → "list what you ruled out in the learnings bullet below, add handling". Last paragraph: "Follow-ups the investigation turns up go to tally todos, not into this fix." → "Follow-ups the investigation turns up become GitHub issues, not part of this fix — `gh issue create --title '[p2] <one line>' --body-file -` with a heredoc body saying what you saw and the symptom you were debugging." Replace "If the file is missing, create it with a `# Learnings` H1 and first copy in every bullet from every `learnings`-tagged tally scratchpad, oldest scratchpad first; archive a scratchpad only after all its bullets are in the committed file." with "If the file is missing, create it with a `# Learnings` H1."
  3. **setup** — frontmatter: "setup of the tally dev loop" → "setup of the dev loop"; "another tally skill" → "another loop skill"; "writes .claude/tally-dev-loop.md" → "writes .claude/dev-loop.md". Body: every `.claude/tally-dev-loop.md` → `.claude/dev-loop.md`. `## 1. Current state`, new first sentence: "If `.claude/tally-dev-loop.md` exists and `.claude/dev-loop.md` does not, offer to `mv` it to the new name (the loop reads both; the new name is just clearer) and then continue with the existing-file flow." New section before `## 2`, titled `## 1b. GitHub — required`, body: "The loop keeps its state in git and GitHub: designs and plans are committed files, the build's ledger is a draft PR, follow-ups are issues. Run `gh auth status` and `git remote get-url origin`; if either fails or the remote is not GitHub, stop and tell the user the loop needs a GitHub remote and a logged-in `gh`." In `## 2`, replace the sentence "Show the user this exact rule — the block the hook injects (kept in sync with `hooks/session-start.sh` by hand):" and the indented block after it with: "Show the user the exact block the hook injects: read `hooks/routing.md` in this plugin — `../../hooks/routing.md` relative to the directory this SKILL.md is in — and print it verbatim. It is the single copy; the hook and the pi extension both read that file." "the tally plugin's SessionStart hook" → "the loop plugin's SessionStart hook". `## 5`: "Recap in three lines" → "Recap in two lines"; delete "; that the tally TUI footer now reflects this state" (end at "undoes everything."). Gitignore bullet: "`*also* has the tally plugin`" → "`*also* has the loop plugin`"; "with the tally plugin" → "with the loop plugin".
  4. Commit the three files with explicit paths.
- **Verify:** `grep -nE 'scratchpad|tally todo|tally scratchpad|TUI footer|tally plugin|tally skill' loop/skills/{brainstorm,debug,setup}/SKILL.md` prints nothing; `grep -c 'dev-loop.md' loop/skills/setup/SKILL.md` ≥ 6.

## Task 4
**Rewrite plan: design file in, plan file out, files moved into the worktree, local dispatch log**

- **Exists because:** plan is the handoff point; if it still writes a scratchpad and passes an id, build has nothing to read.
- **Model:** sonnet
- **Files:** `loop/skills/plan/SKILL.md`.
- **Interfaces produced:** plan file per PLAN rule at `<WT>/docs/plans/<file>` and design at `<WT>/docs/decisions/<file>`; a brief naming the plan path; `$ROOT/.claude/dispatch-log.md`.
- **Steps (read in full; it is long — edit the named passages, leave the herdr/hc mechanics alone; some anchors wrap across lines in the file, match them across the line break):**
  1. Frontmatter `description`: "authored as a plan:<slug> scratchpad (surfaced beside the live todos) plus one tally todo per task; in herdr, dispatches" → "authored as docs/plans/YYYY-MM-DD-<slug>.md and moved into the build's worktree; in herdr or the desktop app, dispatches". "once a design scratchpad exists" → "once a design file exists".
  2. Paragraph "A design is required input…" → "A design is required input: an uncommitted `docs/decisions/YYYY-MM-DD-<slug>.md` (`git status --porcelain docs/decisions` lists it) or one the user names. If none exists, run /loop:brainstorm first — do not write a plan from a cold request."
  3. Replace the `## The plan (scratchpad)` heading and paragraph with `## The plan (file)` and:
     > Author the plan as `docs/plans/YYYY-MM-DD-<slug>.md` in the main checkout — today's date, the design's `<slug>` — uncommitted (`mkdir -p docs/plans`). The date in the name is fixed now; every later skill uses the name as written, never today's date. Header: goal, design file path, branch name (`<type>/<slug>`, type ∈ feat|fix|chore), and the verification command for the whole feature. Every task is a heading that is exactly `## Task N` (no trailing spaces, never inside a code fence), with `**<title>**` on the next line — the build's PR checklist links to the `#task-n` anchors GitHub makes from those headings. /loop:build commits this file and the design file as its first commit in the worktree, so they reach `main` with the PR. In every bullet below, `ROOT=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")` is the main checkout — set it in each bullet that uses it.
  4. `## Research before tasks`: delete " (in a repo with no `docs/learnings.md` yet, skim its `learnings`-tagged tally scratchpads instead)" including the leading space.
  5. Replace `## Tally ledger` heading and paragraph with `## Ledger` and: "There is no separate ledger to create. /loop:build opens a draft PR whose body is a checklist, one line per task, linking to the `## Task N` anchors; ticked boxes are the live state you and the user both watch. Ordering is the order of the tasks in the file — write them in dependency order."
  6. `## Before handing off`: "each given only the plan scratchpad id" → "each given only the plan file path"; "fix them all before creating the tally todos" → "fix them all before handing off".
  7. Builder-model sentence: "Read the default from `.claude/tally-dev-loop.md` (`grep '^builder-model:'`)" → "Read the default from `.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md` when only that exists) with `grep '^builder-model:'`".
  8. **Desktop dispatch bullet:** `ROOT` is already set there. After "If the path or branch already exists, stop and ask the human — never reuse or delete it." insert: "Move the two files in — move, not copy; a copy left behind blocks the `git pull` that later brings the committed file to main: `WT="$ROOT/.claude/worktrees/<slug>"; mkdir -p "$WT/docs/plans" "$WT/docs/decisions"; mv "$ROOT/docs/plans/<plan-file>" "$WT/docs/plans/"; [ -f "$ROOT/docs/decisions/<design-file>" ] && mv "$ROOT/docs/decisions/<design-file>" "$WT/docs/decisions/"`." Brief contents: "the plan id" → "the plan file path inside the checkout (`docs/plans/<plan-file>`)"; "run /loop:build for `plan:<slug>` through completion" → "run /loop:build on `docs/plans/<plan-file>` through completion".
  9. **herdr dispatch bullet:** after "(workspace at `.result.workspace.workspace_id`, checkout at `.result.workspace.worktree.checkout_path`)" insert: "Set `ROOT=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")` and `WT=<checkout_path>`, then move the two files in exactly as the desktop bullet does (`mkdir -p` + `mv`, never `cp`)." Brief: "the `plan:<slug>` scratchpad **id**" → "the plan file path inside the checkout"; "run /loop:build for `plan:<slug>` through completion" → "run /loop:build on `docs/plans/<plan-file>` through completion"; "Build owns the rest (materialize the plan file, implement" → "Build owns the rest (first commit + draft PR, implement".
  10. **Inline bullet:** "proceed to /loop:build in this session." → "`EnterWorktree` (it creates the worktree and branch; give it the `<type>/<slug>` name as its argument), set `ROOT` as above and `WT` to the new worktree's path, move the two files in exactly as the desktop bullet does, then proceed to /loop:build in this session." **Outside herdr bullet:** "`EnterWorktree`, then /loop:build inline." → "`EnterWorktree`, set `ROOT` and `WT`, move the two files in the same way, then /loop:build inline."
  11. **Defer bullet:** "stop; the `plan:<slug>` scratchpad + todos persist for later." → "stop; the plan and design files stay uncommitted in the main checkout — say so. Do not `git stash` them (the stash stack is shared across worktrees); leave them, or commit them on a branch if the user asks."
  12. **Dispatch log:** the sentence "Append one line per dispatch to a `dispatch-log`-tagged tally scratchpad (create it if missing):" (wraps across two lines) → "Set `ROOT` as above and append one line per dispatch to `$ROOT/.claude/dispatch-log.md` (gitignored; create it if missing):". "the same job the `plan:<slug>` todos do for tasks inside a build" (wraps) → "the same job the PR checklist does for tasks inside a build". "**Append the outcome** to `dispatch-log`:" → "**Append the outcome** to `.claude/dispatch-log.md`:". Every remaining "`dispatch-log` line" → "`.claude/dispatch-log.md` line".
  13. Correctness-reviewer brief: "the `## Lenses` section of `.claude/tally-dev-loop.md`" → "the `## Lenses` section of `.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md`)".
  14. Commit the file.
- **Verify:** `grep -nE 'scratchpad|todo|tally' loop/skills/plan/SKILL.md | grep -v -e 'tally-dev-loop.md' -e '.tally-brief.md'` prints nothing; `grep -c 'mv "\$ROOT/docs/plans' loop/skills/plan/SKILL.md` = 1 (desktop bullet; the others refer to it).

## Task 5
**Rewrite build: first commit + draft PR checklist as the ledger; deviations into the decisions file**

- **Exists because:** build is the only writer of the ledger; after Task 4 removes the todos the loop has no task state without this.
- **Model:** sonnet
- **Files:** `loop/skills/build/SKILL.md`.
- **Interfaces consumed:** plan path from the brief (Task 4); PLAN, LEDGER, ISSUE, CFG, ROOT, MOVE rules. **Produced:** draft PR, ticked checklist, `## Deviations` in the decisions file (read by Task 6).
- **Steps (read in full; keep the report-back section untouched):**
  1. Frontmatter `description` → "Execute a plan task-by-task with fresh subagents — TDD, per-task review, the draft PR checklist as the ledger. Runs inside its own worktree from a docs/plans/*.md file, opens the draft PR first, then review. Use after /loop:plan (usually dispatched into a worktree), or directly on a plan file with unticked boxes."
  2. First paragraph: after "`EnterWorktree` onto a `<type>/<slug>` branch and run the loop in-session, in herdr or not." insert: "When you isolate yourself this way and the plan file is still uncommitted in the main checkout, move it (and its design file) into the new worktree first: `ROOT=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")`, `WT=$(git rev-parse --show-toplevel)`, then `mkdir -p "$WT/docs/plans" "$WT/docs/decisions"; mv "$ROOT/docs/plans/<plan-file>" "$WT/docs/plans/"; [ -f "$ROOT/docs/decisions/<design-file>" ] && mv "$ROOT/docs/decisions/<design-file>" "$WT/docs/decisions/"` — move, not copy."
  3. Input paragraph → "Input: the plan file path from your brief — `docs/plans/YYYY-MM-DD-<slug>.md` in this checkout; the design file is `docs/decisions/` with the same basename, if it exists. If no path is in context, look for an uncommitted plan (`git status --porcelain docs/plans`, in this checkout or the main one) or the one named on the first line of the current branch's draft PR body (`gh pr view --json body -q .body | head -1`), and confirm with the user before starting."
  4. Replace the "**First, materialize the plan and the design.**" block (its two numbered steps and the "Both steps:" paragraph) with:
     > **First commit, then the draft PR.** If `git status --porcelain docs/plans docs/decisions` is non-empty: `git add docs/plans/<plan-file> docs/decisions/<design-file>` (omit the design if there is none), commit as `plan: <slug>`, and `git push -u origin <type>/<slug>`. Then, if `gh pr view` reports no PR for this branch, create it as a draft whose body is the checklist — one line per `## Task N` heading in the plan, linking to its anchor:
     > ```bash
     > BASE=$(gh repo view --json url -q .url); BR=$(git branch --show-current); PLAN=docs/plans/<plan-file>; BODY=$(mktemp)
     > { echo "Plan: $PLAN"; echo; grep -n '^## Task [0-9][0-9]*[[:space:]]*$' "$PLAN" | while IFS=: read -r ln _; do n=$(sed -n "${ln}p" "$PLAN" | sed 's/## Task //; s/[[:space:]]*$//'); title=$(sed -n "$((ln+1))p" "$PLAN" | sed 's/^\*\*//; s/\*\*[[:space:]]*$//'); echo "- [ ] T$n — $title ($BASE/blob/$BR/$PLAN#task-$n)"; done; } > "$BODY"
     > TITLE='<plan goal, one line — single quotes, no quote characters inside>'
     > gh pr create --draft --title "$TITLE" --body-file "$BODY"
     > ```
     > (`gh pr create` has no `--title-file`; the single-quoted variable keeps backticks and `$` in the goal from executing.) The checklist is the ledger: `gh pr view --json body -q .body | grep '^- \[ \]'` lists what is not done. A resumed session starts from the first unticked task after checking `git log`/`git status` for partial work on it.
  5. "**Then seed the dev-loop config.**" paragraph: "`.claude/tally-dev-loop.md` (routing, `builder-model`, and" → "`.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md` when only that exists; routing, `builder-model`, and"; replace the one-liner with: `for f in dev-loop.md tally-dev-loop.md; do [ -f ".claude/$f" ] && break; root="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"; [ -f "$root/.claude/$f" ] && mkdir -p .claude && cp "$root/.claude/$f" ".claude/$f" && break; done`.
  6. `## Per task, in blocker order, one at a time` → `## Per task, in plan order, one at a time`. Delete step 2 (**Claim**) and renumber. Review step: "`docs/review-lenses.md` when it exists, else the `## Lenses` section of `.claude/tally-dev-loop.md`" → "`docs/review-lenses.md` when it exists, else the `## Lenses` section of `.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md`)"; "park it with a ruling recorded as a tally comment on the todo" → "park it as a GitHub issue — `gh issue create --title '[p<sev>] <one line>' --body-file -` with a heredoc body: the scenario, your ruling (why not fixed here), and `From PR #<n>` — and put the issue URL in this task's Deviations line".
  7. Replace the **Record** step entirely with:
     > **Record.** Tick the task's box: `N=<n>; b=$(gh pr view --json body -q .body) && [ -n "$b" ] && printf '%s\n' "$b" | sed "s/^- \[ \] T$N /- [x] T$N /" | gh pr edit --body-file -` (the guard keeps a failed read from blanking the body). Then append one bullet to the `## Deviations` section of `docs/decisions/<design-file>` (create the section at the end of the file if missing; if the plan named no design file, put the same line in the task's commit message body instead): `- T<n> commit <hash> — deviation: <what left the plan and why, or none> — parked: <issue urls or none>`. Commit that file with an explicit path and `git push`. The checklist plus this section are the whole ledger — keep no other log. A resumed session reads the checklist; an unticked task with commits already on the branch is unfinished, not undone: check `git log` and `git status`, and dispatch again only for what is missing. **If the task surprised you** — a plan defect, a tool or classifier refusal, a verify command that could not pass at this task, a finding you had to park — also append one `pattern → consequence` bullet to `docs/learnings.md` in this worktree, under a dated heading for this work: `## <slug> (PR #<n>, YYYY-MM-DD)`, blank line before the heading, one bullet per line (the PR number is known from the start — never write `PR #pending`). If the file is missing, create it with a `# Learnings` H1. Commit the file with an explicit path. The Deviations line says what happened here; learnings says what the next planner should not repeat.
  8. "When every task is complete, open a PR (`gh pr create`, let gh detect the remote), then run /loop:review-branch against it." → "When every box is ticked, run /loop:review-branch against the draft PR."
  9. Commit the file.
- **Verify:** `grep -nE 'scratchpad|todo_|comment_add|PR #pending|\*\*Claim' loop/skills/build/SKILL.md` prints nothing; `grep -c 'gh pr create --draft' loop/skills/build/SKILL.md` = 1; the generator from step 4 run with `PLAN=` a temp file containing `## Task 1`/`**a**`/`## Task 10`/`**b**` prints exactly `- [ ] T1 — a (…#task-1)` and `- [ ] T10 — b (…#task-10)`.

## Task 6
**Rewrite review-branch: findings as a PR comment and issues; drop the todo/archive steps**

- **Exists because:** review-branch still files findings as todos and archives scratchpads that no longer exist.
- **Model:** sonnet
- **Files:** `loop/skills/review-branch/SKILL.md`.
- **Steps:**
  1. "Seed the dev-loop config first" paragraph: "`.claude/tally-dev-loop.md` is gitignored and so absent" → "`.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md` when only that exists) is gitignored and so absent".
  2. Lens paragraph: "else the `## Lenses` section of `.claude/tally-dev-loop.md` — in place of" → "else the `## Lenses` section of `.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md`) — in place of".
  3. `## Verdicts`: "File both as tally todos — severity as priority, verdict in a comment. Drop a finding only when you can show it's wrong, and record why as a comment." → "Post all findings as **one** PR comment (`gh pr comment <n> --body-file -` with a heredoc): per finding, severity, verdict, `file:line`, the failure scenario, and — for a dropped finding — why it is wrong. For each p2/p3 you will not fix on this branch, open an issue (`gh issue create --title '[p<sev>] <one line>' --body-file -`, heredoc body: the scenario and `From review of PR #<n> <url>`) and put its URL in the comment."
  4. `## Gate` step 1: "If a heading with that `## <slug> ` prefix already exists (build's version still reads `PR #pending`, not the final number), append under it rather than opening a second one, and replace its `PR #pending` with the real number — do that replacement even when the review adds no bullets." → "If a heading with that `## <slug> ` prefix already exists, append under it rather than opening a second one." Replace "If the file is missing and there is something to write, create it with a `# Learnings` H1 and first copy in every bullet from every `learnings`-tagged tally scratchpad, oldest scratchpad first; archive a scratchpad only after all its bullets are in the committed file." with "If the file is missing and there is something to write, create it with a `# Learnings` H1."
  5. `## Gate` step 2 → "**Decisions.** Build already appended one Deviations bullet per task to `docs/decisions/YYYY-MM-DD-<slug>.md` (same basename as the `docs/plans/*-<slug>.md` file in this branch, never today's date). Add one bullet per place *this review's fixes* left the plan: what changed, why, commit hash. If the section is missing, create `## Deviations` and write `None.` when there is nothing to add; drop a lone `None.` once you add a bullet. If there is no decisions file, create it with a `# <slug>` H1 only when there is a deviation to record."
  6. Delete step 4 (**Archive**) entirely.
  7. Closing paragraph: "The p2/p3 follow-ups live in tally, not in your head." → "The p2/p3 follow-ups are GitHub issues now, not in your head."; "If `.claude/tally-dev-loop.md` doesn't exist, offer /loop:setup once" → "If neither `.claude/dev-loop.md` nor `.claude/tally-dev-loop.md` exists, offer /loop:setup once". (The "mark it ready / merge it / leave it" choice stays the human's — do not add `gh pr ready`.)
  8. Commit the file.
- **Verify:** `grep -nE 'scratchpad|todo_|comment_list|tally todo|Archive|PR #pending' loop/skills/review-branch/SKILL.md` prints nothing; `grep -c 'gh pr comment' loop/skills/review-branch/SKILL.md` = 1.

## Task 7
**TUI footer reads both config names and names `/loop:setup`**

- **Exists because:** after Task 3, setup writes `.claude/dev-loop.md`; the footer would then say "dev loop: not set up · /tally:setup" forever, naming a command that no longer exists.
- **Model:** sonnet
- **Files:** `src/tui/app.rs` (`devloop_status` ~line 244, its doc comments ~187–197, tests ~1865–1900), `src/tui/view.rs:1046`.
- **Steps:**
  1. Failing test first: in the `devloop_status` test block of `src/tui/app.rs`, add a case that writes `# dev loop\nrouting: on\n` to `dir.path().join(".claude/dev-loop.md")` (no tally file) and asserts `devloop_status(dir.path()) == Some((true, 0))`; and a case with both files where `dev-loop.md` says `routing: off` and `tally-dev-loop.md` says `routing: on`, asserting `Some((false, 0))` (new name wins, same as the hook). Run `cargo test devloop` — both fail.
  2. In `devloop_status`, replace the single `read_to_string` line with:
     ```rust
     let text = ["dev-loop.md", "tally-dev-loop.md"]
         .iter()
         .find_map(|n| std::fs::read_to_string(project_root.join(".claude").join(n)).ok())?;
     ```
     Doc comments at ~187 and ~196: "`.claude/tally-dev-loop.md`" → "`.claude/dev-loop.md` (or the older `.claude/tally-dev-loop.md`)"; "/tally:setup" → "/loop:setup".
  3. `src/tui/view.rs:1046`: `"dev loop: not set up · /tally:setup"` → `"dev loop: not set up · /loop:setup"`. Fix any view test that asserts the old string (`grep -rn 'tally:setup' src tests`).
  4. `cargo fmt`, `cargo clippy`, commit `src/tui/app.rs src/tui/view.rs`.
- **Verify:** `cargo test --quiet` passes; `grep -rn 'tally:setup' src tests` prints nothing.

## Task 8
**Docs and install script: say where the loop lives now**

- **Exists because:** README and `scripts/install.sh` tell users the skills come with the tally plugin; after Task 1 that is false.
- **Model:** sonnet
- **Files:** `README.md`, `CLAUDE.md`, `scripts/install.sh`.
- **Steps:**
  1. `README.md` §"Claude Code plugin (dev loop skills)" (~line 171): retitle "Claude Code plugins". Say: `claude plugin install tally@tally` gives the store (MCP + TUI); `claude plugin install loop@tally` gives the dev loop — `/loop:brainstorm`, `/loop:debug`, `/loop:plan`, `/loop:build`, `/loop:review-branch`, `/loop:setup` — which needs only `gh` and a GitHub remote: designs are `docs/decisions/*.md`, plans `docs/plans/*.md`, the build ledger is the draft PR checklist, follow-ups are issues. Consent file is `.claude/dev-loop.md` (the older `.claude/tally-dev-loop.md` still works). Add an "Upgrading from tally ≤ 0.2.x" note (3 lines): the skills moved — install `loop@tally`; until `claude plugin update tally@tally` runs, both plugins inject the routing block (harmless duplicate); if you still have `learnings`-tagged tally scratchpads, copy their bullets into `docs/learnings.md` once by hand (`tally scratchpads list --tag learnings`) — the loop no longer reads them. Fix ~line 55 (`/tally:plan` → `/loop:plan`) and the pi section ~200–205 (dev-loop skills ship in the same pi package from `loop/skills`; the routing extension is `loop/pi/extensions/routing.ts`; both config names accepted).
  2. `CLAUDE.md`: in "Herdr desktop app", add one line after the first paragraph: "The dev loop is its own plugin in `loop/` (`/loop:*`); it does not use the tally store." 
  3. `scripts/install.sh` ~line 114: the comment "so pi sessions discover the tally skills + routing extension" → "so pi sessions discover the tally skill, the loop skills, and the routing extension". No other change; there is no `/tally:` skill text in that file.
  4. Commit the changed files with explicit paths.
- **Verify:** `grep -n '/tally:' README.md CLAUDE.md scripts/install.sh` prints nothing; `grep -c 'loop@tally' README.md` ≥ 2.

## Not in this PR (list in the PR body under "Needs human")
- `claude plugin install loop@tally` on each machine; `claude plugin update tally@tally` after the release (removes the duplicate routing block).
- `~/.claude/CLAUDE.md` (dotfiles): `/tally:plan` and `/tally:build` → `/loop:plan` and `/loop:build` in the Worktrees section.
- Issue #30 (footer treats the dev-loop file as the lens source): Task 7 changes only the file lookup and the command name; the lens-source part of #30 stays open.
