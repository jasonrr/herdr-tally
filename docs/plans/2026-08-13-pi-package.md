# Plan: pi package for tally

**Goal:** Ship tally (store + dev-loop) as a **pi package** (earendil-works/pi harness, runs in herdr), mirroring the Claude Code plugin. pi is anti-MCP, so the store is reached via the existing `tally` CLI; the 6 dev-loop skills are shared; two pi-only skills + a routing extension + a manifest are new. Zero new Rust.

**Design scratchpad:** `s_dknx9i20crqw1` (tagged `design`, `pi`). Prior art: `s_dk0umfsenxd41` (pi-without-herdr planning — herdr-skill source), `s_djvy38aknk681` (install/distribution research).

**Branch:** `feat/pi-package`

**Whole-feature verification:**
```bash
# 1. package installs & is discovered
cd "$PWD" && pi install "$PWD" && pi list 2>/dev/null | grep -qi tally && echo "package OK"
# 2. routing extension fires only when the repo opted in (marker on stderr)
printf 'routing: on\n' | head   # this repo already has .claude/tally-dev-loop.md with routing:on
pi -e pi/extensions/routing.ts -p -a --no-session --provider anthropic --model haiku --thinking off "reply with OK" 2>&1 \
  | grep -q '\[tally-routing\] dev-loop routing injected' && echo "routing OK"
```

## Verified facts (do not re-litigate)
- pi skill format == Claude's: `name` + `description` YAML frontmatter, `SKILL.md`, invoked `/skill:<name>`.
- pi ExtensionAPI: `pi.on("before_agent_start", async (event, ctx) => ...)` — `event.systemPrompt` is the current system prompt; return `{ systemPrompt }` to replace it. `ctx.cwd` is the project dir. `node:fs` / `node:path` imports work. The factory runs once per session; use a closure flag, reset it in `session_start` for branch-safety. Import: `import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";`
- **herdr env in an agent pane (measured):** `HERDR_ENV=1`, `HERDR_PANE_ID`, `HERDR_SOCKET_PATH`, `HERDR_TAB_ID`, `HERDR_WORKSPACE_ID` present. `HERDR_PLUGIN_ROOT` / `HERDR_PLUGIN_CONTEXT_JSON` **absent** (plugin-command only). pi inherits parent env → these reach pi's bash tool (proven live in pane w15:p30). `tally` is on PATH (repo `bin/`).
- `tally` CLI subcommands: `tally todos <list|get|create|update|delete|complete|…>`, `tally scratchpads <list|read|create|update|append|append-section|edit|find|…>`, `tally comments <add|list|delete|recent|targets>`. `tally scratchpads find <id> <query>` greps *within one pad* — it is NOT a tag filter; list-by-tag is `tally scratchpads list` (tags print in `[brackets]`).
- The `<dev-loop>` block is canonically in `hooks/session-start.sh`, hand-mirrored in `skills/setup/SKILL.md:18-26`. This plan adds a third copy in the extension. Keep them byte-identical.

## Parity findings (why Tasks 6–7 exist)
The pi package lives entirely on the CLI, so any MCP tool with no CLI verb is a landmine. Audited all 38 MCP tools against the CLI verbs. Four had no CLI verb; on inspection:
- `todo_transfer`, `scratchpad_transfer` — **phase-2 stubs**: their MCP `run` returns `Err("…is phase 2")` (`src/mcp/tools.rs:220,312`), desc "Not supported cross-project in v1." Non-functional on both sides. → allowlist, don't build error-stub CLI verbs.
- `scratchpad_add_tags`, `scratchpad_remove_tags` — **real, but implemented inline in the MCP adapter** (`src/mcp/tools.rs:249-270`) as read-modify-write over `read_scratchpad` + `update_scratchpad` (both already on the CLI). The CLI can *emulate* via `update --tag <full set> --expected-revision`, but has no dedicated verb. → close for real by lifting the logic into a store method both adapters call (upholds the "logic lives in store, adapters are thin" invariant), then add CLI verbs.

Everything else maps 1:1 (modulo name shape: `todo_X` → `todos X`, `_`→`-`; and `scratchpad_write` ↔ CLI `create`/`update`). The dev-loop's load-bearing path (create a scratchpad tagged `design`) already works: `scratchpads create --tag design`.

---

## Task 1 — pi routing extension (`pi/extensions/routing.ts`)

**Exists because:** the "full" version's whole point — auto-enforce the dev loop in pi, mirroring the Claude `SessionStart` hook. Without it, pi users get the skills but must invoke every `/skill:*` by hand. This is the only piece not yet proven in practice, so it goes first.

**Model:** sonnet
**Files:** create `pi/extensions/routing.ts`
**Consumes:** pi ExtensionAPI; reads `<ctx.cwd>/.claude/tally-dev-loop.md`.
**Produces:** the loadable extension at `pi/extensions/routing.ts` (path referenced by Task 2's manifest).

**Steps:**
1. Write the file verbatim:

```typescript
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { readFileSync } from "node:fs";
import { join } from "node:path";

// The tally dev-loop routing block. CANONICAL COPY: hooks/session-start.sh
// (the Claude SessionStart hook); also mirrored in skills/setup/SKILL.md.
// Keep all copies byte-identical when the routing rules change.
// ponytail: 3 hand-synced copies (hook, setup skill, here). A shared data
// file read by both bash and TS would kill the drift — not worth the
// machinery until the block actually churns.
const DEV_LOOP_BLOCK = `<dev-loop>
This project uses the tally dev loop. Standing routing — the human should never have to ask:
- Feature or non-trivial change requested → run /tally:brainstorm before writing any code.
- Bug, failing test, or unexpected behavior → run /tally:debug before proposing any fix.
- Approved design (tally scratchpad tagged design) or any multi-step implementation → run /tally:plan before building.
- A plan doc with plan:<slug> todos exists → execute it with /tally:build.
- Branch complete, or a merge/PR is requested → run /tally:review-branch first.
Trivial one-file changes and pure questions are exempt. When this rule triggers a skill, say so in one line.
</dev-loop>`;

// Mirrors hooks/session-start.sh: inject only when this repo opted in via
// .claude/tally-dev-loop.md containing a `routing: on` line. Missing file or
// missing line → routing off, silently (same as the hook's `exit 0`).
function routingEnabled(cwd: string): boolean {
  try {
    const cfg = readFileSync(join(cwd, ".claude", "tally-dev-loop.md"), "utf8");
    return /^routing: on$/m.test(cfg);
  } catch {
    return false;
  }
}

export default function (pi: ExtensionAPI) {
  let injected = false; // once per session; reset for branch-safety

  pi.on("session_start", async () => {
    injected = false;
  });

  pi.on("before_agent_start", async (event, ctx) => {
    if (injected) return;
    if (!routingEnabled(ctx.cwd)) return;
    injected = true;
    // Framework-free test marker: greppable on stderr in -p mode.
    console.error("[tally-routing] dev-loop routing injected");
    return { systemPrompt: `${event.systemPrompt}\n\n${DEV_LOOP_BLOCK}` };
  });
}
```

2. Confirm `ctx.cwd` and the `{ systemPrompt }` return are the actual pi contract for this pi version (0.84.1) — if the field names differ, adjust and note in the build-log. (This is the spike; the marker log lets you see the branch fire regardless.)

**Verify (the ponytail check — two runs, opt-in on vs off):**
```bash
# ON: this repo has .claude/tally-dev-loop.md with 'routing: on' → marker appears
pi -e pi/extensions/routing.ts -p -a --no-session --provider anthropic --model haiku --thinking off "reply OK" 2>&1 \
  | grep -q '\[tally-routing\] dev-loop routing injected' && echo "ON: injected (correct)"
# OFF: a temp dir with no config → marker absent
d=$(mktemp -d); (cd "$d" && pi -e "$OLDPWD/pi/extensions/routing.ts" -p -a --no-session --provider anthropic --model haiku --thinking off "reply OK" 2>&1) \
  | grep -q '\[tally-routing\]' && echo "OFF: LEAKED (bug)" || echo "OFF: silent (correct)"
```

---

## Task 2 — pi package manifest (`package.json`)

**Exists because:** without a `pi`-keyed `package.json` at repo root, pi does not discover the skills/extension as an installable package; this is what `pi install <repo>` wires up. The dumb alternative (dropping files in `~/.pi/agent/skills`) isn't shippable or versioned.

**Model:** sonnet
**Files:** create `/package.json` (repo root — none exists today; harmless in a Rust repo, no build/deps).
**Consumes:** the `pi/extensions/` dir from Task 1 (so the glob resolves to something).
**Produces:** the installable package; `pi.skills` globs `skills/` (the 6 shared dev-loop skills) + `pi/skills/` (the 2 pi-only skills from Task 3).

**Steps:**
1. Write verbatim:

```json
{
  "name": "tally",
  "version": "0.1.12",
  "description": "Project-scoped todos, scratchpads & plans, plus the tally dev loop — as a pi package.",
  "keywords": ["pi-package"],
  "license": "MIT",
  "repository": "https://github.com/jasonrr/herdr-tally",
  "pi": {
    "skills": ["skills", "pi/skills"],
    "extensions": ["pi/extensions"]
  }
}
```

2. Note in build-log: the 6 shared skills load in pi under bare names (`/skill:brainstorm`, not `/tally:brainstorm` — pi has no plugin namespace). Acceptable. Bare package name `tally` is fine for git/local install; publishing to npm later would need a scope (`@jasonrr/tally`).

**Verify:**
```bash
node -e "const p=require('./package.json'); if(!p.keywords.includes('pi-package')) throw 'missing keyword'; if(!p.pi.skills.includes('skills')||!p.pi.skills.includes('pi/skills')) throw 'skills glob wrong'; if(!p.pi.extensions.includes('pi/extensions')) throw 'ext glob wrong'; console.log('manifest OK')"
```

---

## Task 3 — the two pi-only skills (`pi/skills/tally/SKILL.md`, `pi/skills/herdr/SKILL.md`)

**Exists because:** pi is anti-MCP — the `tally` skill is how a pi agent touches the store at all (the "CLI tool with README" pi wants). The `herdr` skill is pi's equivalent of the global CLAUDE.md herdr section — without it pi rolls its own `git worktree add` instead of herdr's native flow. Both mirror the house style of `skills/setup/SKILL.md` / `skills/debug/SKILL.md` (2 frontmatter fields, H1, punchy opening line, `##` sections, ~30-50 lines, terse imperative).

**Model:** sonnet
**Files:** create `pi/skills/tally/SKILL.md` and `pi/skills/herdr/SKILL.md`

**Steps:**
1. `pi/skills/tally/SKILL.md` verbatim:

```markdown
---
name: tally
description: Read and write the project's tally store — todos, scratchpads, and comments — through the `tally` CLI. Use whenever you need to record a plan, track a follow-up, leave a margin note, or check shared state with the human and other agents.
---

# Tally store

The store is shared memory for you, the human, and other agents — reach for it instead of scratch files. Todos for discrete follow-ups, scratchpads for plans and handoffs, comments for the *why*.

`tally` is on your PATH — call it directly. Every subcommand lists its verbs if you run it bare (`tally todos`); add `--json` on reads you want to parse.

## Todos — one discrete follow-up each, id-first

```bash
tally todos create --title "Rotate refresh tokens" --priority p1 --tag auth   # p0 critical … p3 low
tally todos list --status open --json
tally todos update <id> --status in_progress          # open | in_progress | completed
tally todos add-blocker <id> --blocker <other-id>     # <id> can't start until <other-id> completes
tally todos complete <id>
```

## Scratchpads — plans & handoffs, revision-guarded

Read before you write: `read` returns a `revision`; pass it back as `--expected-revision`. A mismatch means someone else edited it — re-read and retry. Prefer `append` / `append-section` / `edit` over rewriting the whole pad.

```bash
tally scratchpads create --name "Auth refactor plan" --content-file -    # reads stdin
tally scratchpads read <id> --mode headings                             # outline first
tally scratchpads append-section <id> --heading "Progress" --content "did X" --expected-revision <r>
tally scratchpads list                                                  # ids + titles + [tags] — filter by eye
```

## Comments — margin notes, the why not the state

```bash
tally comments add <id> --body "hold off — waiting on the auth PR"
tally comments add docs/plans/auth.md --body "step 3 is done"   # target a plan by its path
tally comments recent --since 2h
```

Don't delete the human's items — archive scratchpads, complete the todos you finish.
```

2. `pi/skills/herdr/SKILL.md` verbatim (source: global CLAUDE.md herdr section + `s_dk0umfsenxd41`; uses only the measured env vars):

```markdown
---
name: herdr
description: You run inside herdr, a terminal workspace manager. Use its native primitives for worktrees, panes, and dispatching work to fresh agents instead of rolling your own. Use whenever work needs isolation (a branch/worktree) or you want to hand a task to another agent pane.
---

# herdr

You are inside herdr when `HERDR_ENV=1`. herdr injects `HERDR_WORKSPACE_ID`, `HERDR_PANE_ID`, `HERDR_TAB_ID`, and `HERDR_SOCKET_PATH` into your pane. Locate project tools via PATH.

## Isolation — the herdr way, not `git worktree add`

When work needs a branch/worktree, let herdr create it — it opens a fresh pane in the worktree with the standard layout:

```bash
herdr worktree create --cwd <repo-root> --branch <type>/<name>   # type ∈ feat|fix|chore|deps
```

`--cwd` is required — herdr does not resolve the repo from your shell's cwd, and can pick the wrong workspace without it.

## Dispatch work to a fresh agent

To run a substantial or parallel task in its own pane so this session stays a clean orchestrator:

```bash
herdr worktree create --cwd <repo-root> --branch <type>/<name> --no-focus   # → JSON; read .result.root_pane.pane_id
herdr agent prompt <pane_id> "<task>"        # the pane auto-launches its agent; just prompt it
herdr agent wait <pane_id> --until done      # only when a chained step needs the result
```

After dispatching, step back — the dispatched session talks to the human directly. Don't mirror its output back here unless the human asks, or a chained next step needs its result.

Worktrees are disposable — remove them once the branch is merged or abandoned. Merge via PR; prefer linear history.
```

**Verify:**
```bash
test -f pi/skills/tally/SKILL.md && test -f pi/skills/herdr/SKILL.md
head -1 pi/skills/tally/SKILL.md | grep -q '^---$'          # frontmatter present
grep -q 'HERDR_ENV' pi/skills/herdr/SKILL.md                 # detects herdr correctly
! grep -q 'HERDR_PLUGIN_ROOT' pi/skills/herdr/SKILL.md       # design bug guard: never reference the absent var
! grep -q 'find --tag' pi/skills/tally/SKILL.md              # never propagate the non-existent flag
echo "skills OK"
```

---

## Task 4 — neutralize the MCP-tool reference in `skills/brainstorm/SKILL.md`

**Exists because:** `brainstorm` is a *shared* skill — pi loads it too via the manifest. Naming `ToolSearch`/`mcp__tally__` (Claude-only) misdirects a pi agent that has neither. The "how to write a scratchpad" belongs to the per-agent CLI/MCP skill, not the shared body.

**Model:** haiku (exact single-line edit, no judgment)
**Files:** edit `skills/brainstorm/SKILL.md` (line 20)

**Steps:** delete exactly one parenthetical from line 20 — do NOT retype the whole line (it continues with two more sentences: "Keep it under a page … File a tally todo …"). Remove this substring, including its trailing space:
```
(ToolSearch `mcp__tally__` if the tools aren't loaded) 
```
Result: the line begins `Write the design to a tally scratchpad tagged \`design\`: …` and the rest of the line is untouched.

**Verify:**
```bash
! grep -rq 'mcp__tally__\|ToolSearch' skills/ && echo "shared skills are agent-neutral"
```

---

## Task 5 — register the pi package on install (`scripts/install.sh`)

**Exists because:** without it, every herdr user must `pi install` by hand; this mirrors the script's existing best-effort Claude MCP-registration phase so both agents get wired on `herdr plugin install`. pi is MCP-averse, so the pi equivalent of "add the MCP server" is "install the package."

**Model:** sonnet
**Files:** edit `scripts/install.sh` — insert a new phase after Phase 2 (MCP registration, ends ~line 70), before Phase 3 (CLAUDE.md block).
**Consumes:** `$plugin_root` (defined at `scripts/install.sh:13`) and Task 2's `package.json`. Insertion point: after Phase 2 (MCP registration ends ~line 70), before Phase 3 (~line 72). Also bump the header comment (`scripts/install.sh:3-6`) from "Three phases" to "Four phases" so it stays accurate.

**Steps:**
1. Insert, matching the script's `printf`/best-effort style and `/bin/sh` syntax:

```sh
# --- Phase 2b: register the pi package (best-effort) ---------------------
# pi is MCP-averse — no server to add. Instead install this repo as a pi
# package so pi sessions discover the tally skills + routing extension.
# User-global (no -l) so every project's pi sees it. Never fatal.
if command -v pi >/dev/null 2>&1; then
  if pi install "$plugin_root" >/dev/null 2>&1; then
    printf 'tally: registered pi package.\n'
  else
    printf 'tally: could not auto-register the pi package. Run manually:\n'
    printf '  pi install "%s"\n' "$plugin_root"
  fi
fi
```

2. **Resolve the distribution open question here (live):** confirm `pi install "$plugin_root"` on a dir carrying the `pi` manifest (a) succeeds, (b) is re-run-safe (idempotent — install.sh runs on every re-link). Test: `pi install "$PWD"` twice, then `pi list | grep -i tally`. If global-install-of-a-local-path misbehaves, degrade to printing the manual command only (drop the auto-attempt) and record the finding in the build-log — do NOT make install fail. Note whether a git source (`pi install git:github.com/jasonrr/herdr-tally`) is the better shipping form.

**Verify:**
```bash
sh -n scripts/install.sh && echo "syntax OK"        # don't run the whole script (rebuilds the binary)
pi install "$PWD" >/dev/null 2>&1 && pi install "$PWD" >/dev/null 2>&1 && pi list 2>/dev/null | grep -qi tally && echo "pi install idempotent + discovered"
```

---

## Task 6 — close the scratchpad tag-mutation parity gap (store + both adapters)

**Exists because:** `scratchpad_add_tags`/`remove_tags` are real MCP capabilities with no CLI verb — a landmine for pi, which is CLI-only. Closing it via a shared store method (not another inline adapter copy) also fixes an existing invariant violation: that read-modify-write currently lives in the MCP adapter, not the store.

**Model:** sonnet
**Files:** `src/store/scratchpads.rs` (new methods + tests), `src/mcp/tools.rs` (call the new methods), `src/cli/scratchpads.rs` (new verbs + usage string)
**Consumes:** existing `read_scratchpad`, `update_scratchpad` store methods.
**Produces:** store methods `add_scratchpad_tags` / `remove_scratchpad_tags` used by both adapters; CLI verbs `scratchpads add-tags` / `remove-tags`.

**Steps (test-first):**
1. In `src/store/scratchpads.rs`, add two store-level unit tests near `test_scratchpad_tags` (~line 814): `test_add_scratchpad_tags_no_dupe` (add `["a","b"]` then `["b","c"]` → tags end `[…,a,b,c]`, no dup `b`; revision bumps each call) and `test_remove_scratchpad_tags` (remove a present + an absent tag → only present one drops). Assert a wrong `expected_revision` errors (revision guard holds).
2. Add the methods (mirror the MCP inline logic verbatim so behavior is identical; match `update_scratchpad`'s return type):
```rust
/// Add tags in one revision bump, idempotent (no duplicates). Store-level
/// home for logic both the CLI and MCP adapters share.
pub fn add_scratchpad_tags(&self, id: &str, add: &[String], expected_revision: i64) -> Result<Scratchpad> {
    let (s, _) = self.read_scratchpad(id, "full", "", 0, 0)?;
    let mut merged = s.tags.clone();
    for t in add {
        if !merged.contains(t) { merged.push(t.clone()); }
    }
    self.update_scratchpad(id, expected_revision, None, None, Some(merged))
}

/// Remove tags in one revision bump. Absent tags are a no-op.
pub fn remove_scratchpad_tags(&self, id: &str, drop: &[String], expected_revision: i64) -> Result<Scratchpad> {
    let (s, _) = self.read_scratchpad(id, "full", "", 0, 0)?;
    let keep: Vec<String> = s.tags.into_iter().filter(|t| !drop.contains(t)).collect();
    self.update_scratchpad(id, expected_revision, None, None, Some(keep))
}
```
(If `read_scratchpad`/`update_scratchpad` signatures differ from the calls at `src/mcp/tools.rs:253,260`, match those call sites — they are the ground truth.)
3. Refactor MCP to call the store (removes the inline duplication). Replace the `run` closures at `src/mcp/tools.rs:251-260` and `264-269`:
```rust
run: |p, a| { require_revision(a, "scratchpad_add_tags")?; val(p.add_scratchpad_tags(&a.id, &a.tags(), a.rev())?) } },
// …
run: |p, a| { require_revision(a, "scratchpad_remove_tags")?; val(p.remove_scratchpad_tags(&a.id, &a.tags(), a.rev())?) } },
```
Keep the existing MCP test `test_scratchpad_add_tags_no_duplicate` (tools.rs:620) green — it now exercises the store path.
4. Add CLI verbs in `src/cli/scratchpads.rs`. The handler already parses `id`, `tags = p.multi("tag")`, and `expected_revision`. **Three wiring points — all required, or the verbs silently misbehave:**
   - **`ID_TAKING`** (`~:47-61`): add `"add-tags"`, `"remove-tags"`. Without this the id-positional isn't parsed, `id` stays `""`, and the read hits "not found".
   - **`REVISION_REQUIRED`** (`~:65-73`): add `"add-tags"`, `"remove-tags"`. `expected_revision` defaults to `-1` (`:128`), and `mutate_pad` skips the guard when `exp_rev < 0` (`src/store/scratchpads.rs:256,266`) — omit this and a missing `--expected-revision` mutates UNGUARDED (silent-data defect). This enforces the revision-guard invariant.
   - **Dispatch arms** beside `"update"` (`~:205`), using the file's real idiom (`print_json(out, &s)` returns `io::Result`; errors go through `fail(&e.to_string())` — there is no `print_json(&s)`/`die`):
```rust
"add-tags" => match proj.add_scratchpad_tags(&id, &tags, expected_revision) {
    Ok(s) => { let _ = print_json(out, &s); }
    Err(e) => return fail(&e.to_string()),
},
"remove-tags" => match proj.remove_scratchpad_tags(&id, &tags, expected_revision) {
    Ok(s) => { let _ = print_json(out, &s); }
    Err(e) => return fail(&e.to_string()),
},
```
   - Add `add-tags|remove-tags` to the usage string at line 93.

**Verify:**
```bash
cargo test scratchpad_tags && cargo test add_scratchpad_tags && cargo test remove_scratchpad_tags
cargo build --release && rm -f bin/tally && cp target/release/tally bin/tally
r=$(bin/tally scratchpads create --name paritytest --content-file - <<<'x' | grep -o '"revision":[0-9]*' | head -1 | grep -o '[0-9]*'); \
  id=$(bin/tally scratchpads list | grep paritytest | grep -o 's_[a-z0-9]*' | head -1); \
  bin/tally scratchpads add-tags "$id" --tag foo --expected-revision "$r" && \
  bin/tally scratchpads list | grep paritytest | grep -q foo && echo "add-tags verb works"; \
  bin/tally scratchpads delete "$id"
```

---

## Task 7 — MCP↔CLI parity guard test (`tests/mcp-cli-parity.test.sh`)

**Exists because:** nothing today stops a 39th MCP tool from shipping with no CLI verb — the exact landmine that motivated this. A test makes the "thin adapters" invariant enforceable in CI. Follows the existing shell-test pattern in `tests/` (`install.test.sh`, `fetch-or-build.test.sh`).

**Model:** sonnet
**Files:** create `tests/mcp-cli-parity.test.sh` (executable)
**Consumes:** the running `bin/tally mcp` server (authoritative tool list) and the CLI usage strings. Depends on Task 6 (so the tag verbs exist and the test passes green).

**Steps:**
1. Get the authoritative tool list from the server, not source grep: pipe an `initialize` + `notifications/initialized` + `tools/list` JSON-RPC sequence (newline-delimited, per the MCP invariant) into `bin/tally mcp`, parse `.result.tools[].name` with **`jq`** (present at `/opt/homebrew/bin/jq`; do NOT use `python3` — it resolves to a project venv binary that's absent in a clean herdr pane). Expect 38.
2. Get CLI verbs: for `todos`/`scratchpads`/`comments`, run the bare subcommand; the usage goes to **stderr** (`fail` → `eprintln!`, `src/cli/mod.rs:53`) prefixed like `error: usage: tally scratchpads <list|read|…>`. Strip everything up to the first `<`, take the substring between `<` and `>`, split on `|`.
3. Map each tool → expected CLI verb: strip the `todo_`/`scratchpad_`/`comment_` prefix, `_`→`-`, namespace = plural. Assert the verb exists in that namespace's set. Two explicit, documented exceptions (fail the build if the *reason* changes):
   - **Allowlist (phase-2 stubs, non-functional in MCP too):** `todo_transfer`, `scratchpad_transfer`.
   - **Name-map:** `scratchpad_write` → satisfied by CLI `create` **or** `update`; `*_tags_list` → satisfied by CLI `tags`.
4. Print each unmatched tool and `exit 1` if any tool outside the allowlist/name-map lacks a verb. On success print `parity OK (38 tools, 2 allowlisted stubs)`.
5. Add a one-line comment block at the top naming the allowlist and *why* each entry is exempt, so the next person sees the boundary.

**Verify:**
```bash
chmod +x tests/mcp-cli-parity.test.sh && sh tests/mcp-cli-parity.test.sh && echo "parity guard passes"
# negative check: temporarily add a fake tool name to the expected set inside the test → it must exit 1
```

---

## Ordering
- **Task 1** first (spike; no blockers).
- **Task 2** blocked by Task 1 (extensions dir must exist for the glob).
- **Task 3** — no blockers (independent content).
- **Task 4** — no blockers (haiku, one line).
- **Task 5** blocked by Task 2 (manifest must exist for `pi install`).
- **Task 6** — no blockers (Rust; independent of the pi-package files).
- **Task 7** blocked by Task 6 (guard must pass green once the tag verbs exist).

## Residual risks (not blockers)
- pi 0.84.1 field names for `before_agent_start` (`event.systemPrompt`, `{ systemPrompt }` return, `ctx.cwd`) are from docs, not this exact version — Task 1's marker surfaces the truth on first run.
- Whether `{ systemPrompt }` genuinely reaches the model (vs. a message inject) is asserted only indirectly by the marker; if routing behavior looks off in practice, revisit with a `context`-event approach.
