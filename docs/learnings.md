# Learnings

## dev-loop-v2 (2026-08-11)

- subagent verify commands that count matches must count occurrences (grep -o|wc -l), not lines (grep -c) → an implementer will reshape mandated text to satisfy the letter of a broken check rather than stop and report (seen T4, dev-loop-v2).
- worktree subagent briefs must forbid bare git stash → stash stack is shared across all worktrees/sessions; a subagent stash/pop can eat another session's WIP (seen T8, harmless this time).
- ~/.claude/skills/herdr SKILL.md documents 'herdr wait output/agent-status' but herdr 0.8.0 ships 'herdr pane wait-output <id> --match X' and 'herdr agent wait <id> --until done' → verify herdr invocations against --help, not the skill doc, before baking them into other skills.
- two readers of one config file must share one parser semantics or a constructible input splits them (grep '^x$' vs Rust lines()+trim_end() diverge on CRLF/trailing space; seen dev-loop.md, footer said armed while hook stayed silent).
- manifests/docs that embed the GitHub remote must be checked against 'git remote -v', not memory → shipped jasonrosoff/ where the remote is jasonrr/ (caught at push, fixed 0fe1503).

## pi-package branch (2026-08-13)

- pi's `before_agent_start` resets systemPrompt to base each turn → inject the routing block EVERY turn (content-guarded), not once-per-session; a once-flag makes the block vanish after turn 1. (Unlike Claude's SessionStart additionalContext, which persists.)
- A CLI↔MCP parity test that reads the bare-subcommand USAGE STRING only proves the verb is advertised, not that it dispatches → probe by invoking `tally <ns> <verb>` and failing on the "unknown <ns> subcommand" fallback. Guard-that-checks-advertisement ≠ guard-that-checks-behavior.
- `tally todos|scratchpads create` invoked bare MUTATES the store (creates an empty record) — not a side-effect-free dispatch probe; create-then-delete, or pick a verb that errors on missing args.
- JS `/^x$/m` matches before a bare `\r` (CRLF); POSIX `grep '^x$'` does not. A pi TS extension and a bash hook reading the same config file diverge on CRLF unless the TS side uses exact line-split matching.

## is_blocked perf (2026-08-21)

- Per-item accessor that reloads the whole store → O(N) full automerge hydrations per list render. Project::is_blocked(t) called load_todos() every call; render_todos + TUI refresh called it per todo. On rc-data-tools' 254KB store (~75ms/load) with ~30 blocker-bearing completed todos → ~2.3s per 'todos list --status completed'. --json/dump skip is_blocked so they stayed ~0.1s, masking it. Fix: single-load Project::blocked_ids(), membership check per row. Watch for any other per-item method that re-hydrates inside a render loop.

## bundle-ask-user (2026-08-30)

- bundledDependencies + npm7 auto-installing a *dependency's* peerDependencies → a small bundled dep (pi-ask-user, ~132K) drags its host's core peer tree (~136MB) into node_modules and the lockfile on every install path. pi injects those cores at runtime (separate module roots), so `legacy-peer-deps=true` (committed .npmrc + explicit installer flag) is required to keep the bundle lean. (plan:bundle-ask-user branch review)
- Hermetic install.sh tests must stub/skip EVERY phase that shells out to a real global-state mutator, not just the one under test: install.test.sh set TALLY_FETCH_OR_BUILD + stubbed claude but left TALLY_PI unset → ran real `pi install <scratch-tmp>` against the dev's global pi config. Add a `-` seam per external command (TALLY_NPM=-, TALLY_PI=-; find_claude still lacks one → todo t_dl1waa5o0928c).
- Pi extension startup hooks: raw `console.error` in an interactive TTY bypasses the TUI renderer and can leave Pi unusable → keep smoke-test markers non-TTY-only; use `ctx.ui.notify` for interactive errors.

## dispatch-contract (PR #18, 2026-09-08)

- `scratchpad_append` with content starting on a fresh line still lands the `##` heading directly under the previous bullet → the exact run-on this branch's #17 work exists to prevent, hit while writing these very lines. `newline: true` gives you ONE newline, not a blank line; put an explicit leading blank line in the content, or repair with a full `scratchpad_write`.
- `scratchpad_save_to_file` with a relative path resolves against the MCP server's cwd, not the agent's → in a worktree, `/tally:build`'s "materialize the plan" step wrote `docs/plans/<date>-<slug>.md` into the MAIN checkout instead of the worktree. Pass an absolute path, or move the file after.
- A skill that instructs a *chain* of skills must say who owns a once-only side effect → build's "send the report last" and review-branch's "send a blocked report at the gate" each read correctly alone, but a dispatched build runs both, so the happy path sent the report twice. Name the owner explicitly when two skills in one session can both fire the same action.
- Ordering claims about a CLI need the `--help` read, not the mental model → the cleanup rule ordered `workspace close` before `worktree remove`, but `herdr worktree remove` (0.9.0) takes only `--workspace <ID>` and no path, so closing first leaves no handle and orphans the checkout on disk. Caught in branch review, not planning.
- A plan step written as "copy this text verbatim" can still carry a p1 → two of three review findings on this branch were defects in the plan's own wording, not in the implementer's copy of it. Reviewing the diff against the plan is not the same as reviewing the plan.

## durable-records (PR #20, 2026-09-19)

- A path pattern with a date (`YYYY-MM-DD-<slug>.md`) named in two skills must say in both where the date comes from → build pinned it to the plan file basename, review-branch did not, so a review run on a later day would pick today's date, miss the real decision file, and create a duplicate. Caught in per-task review.
- A whole-feature verify that pins exact counts on a file the build's own rules append to cannot pass once the build follows those rules → the 18-bullet / 5-heading check broke when this build logged its first learning. Pin counts on the migrated part only, or use a floor.
- A guard added in a review fix can create the next bug → "append Deviations only if none yet" (added to stop duplicates) made a second review run drop new deviations. Ask of every guard: what does a legitimate re-run do?
