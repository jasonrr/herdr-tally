# Design: dev loop on the Herdr desktop app (hc + herdr-app)

## Problem
The dev-loop skills drive the old `herdr` 0.9.5 CLI: `herdr worktree create`, `agent start/prompt/wait`, `pane split/run/read`. The new Herdr desktop app splits that into `hc` (herdr-core 0.1.16: terminals, send, wait, read) and `herdr-app` (spaces/tabs/panes the person sees). `hc` has no worktrees. Skills must work in BOTH (human's call).

## Detection (one rule, all skills)
- `$HC_TERMINAL_ID` set → new app. Else `HERDR_ENV=1` → old herdr. Else → no multiplexer (existing non-herdr paths).
- New-app check first: the new app may also export HERDR_* vars.

## Approach: second branch in the skill text, no shim
Each herdr step in skills/plan, skills/build, skills/review-branch gets an hc branch next to it.

| Step | old herdr | new app |
|---|---|---|
| caller kind | `herdr agent get $HERDR_PANE_ID` `.result.agent.agent` | `hc get $HC_TERMINAL_ID` `.agent` (claude-code / pi) |
| worktree + agent + brief (plan Dispatch) | `herdr worktree create` → `agent start` → `agent prompt` | `git worktree add <path> -b <type>/<slug>` then `herdr-app do space.new --name "<title>" --folder <checkout> --argv '["claude","--model","<m>"]' --task "<brief>"` (pi: `["pi","--provider",…,"--model",…]`) |
| return address in brief | dispatcher pane id | dispatcher `$HC_TERMINAL_ID` |
| child report (build) | `herdr agent prompt <pane> "<line>"` | `hc send <terminal> "<line>"` (same report line) |
| reviewer panes (review-branch) | tab + `pane split` + `pane run` + `agent prompt` + `agent wait` + `pane read` | `hc run --bg --name review-<lens> --cwd <repo> --label herdr.space=<id> claude --model sonnet "<brief>"` per lens, `hc wait --children all`, `hc read --lines 100 review-<lens>` |
| cleanup | `herdr worktree remove` → `workspace close` | `git worktree remove <path>` → `herdr-app do space.close` |

Worktree path for new app: same place herdr used, or `.claude/worktrees/<slug>` (pick one in plan; verify global bootstrap hook fires, it keys off linked worktree only).

## Cut
- Shim script (`scripts/mux.sh`) abstracting both CLIs. Cut test: nothing breaks without it; two branches of prose are cheaper than a new process. Add if a third backend shows up.
- Polling/mirroring child progress — already banned by the dispatch contract; same in new app.
- TUI pane port (herdr-plugin.toml → herdr-app mod). Out of scope by human's call; todo filed.
- hooks/session-start.sh: no herdr refs, no change.

## Open questions (verify live during build, not design blockers)
- Does a space.new terminal get `$HC_TERMINAL_ID`, and does `hc send` to a non-child terminal deliver as a prompt? (Expected yes per `hc --skill`.)
- `space.close` confirm flag name (`herdr-app commands`).
- Does `space.new` return the space id for the dispatch-log line?
- verify-herdr.sh: add an hc section or a sibling script — decide in plan.
- User's global ~/.claude/CLAUDE.md says "use `herdr worktree create`" — needs a new-app line; human edits their own file.

## Future: dev-loop may split from tally (human, 2026-11-01)
Human is considering moving dev-loop out of tally, with ephemeral tracking by default and gh objects (issues for todos) where state must persist. Effect on this design: none to the approach. One rule for the build: keep the herdr/hc steps free of tally references. The brief carries "the plan id", not "the tally scratchpad id" wording baked into the mux steps. Then the multiplexer layer moves over unchanged and only the storage layer gets rewritten.

## Deviations

- Plan dispatch bullet: states step-back inline (the plan text said both "skip the next bullet" and "as in the next bullet"); `git worktree add --no-track`; app cleanup recomputes `ROOT`, says it replaces the herdr commands, and names the clean/unpushed check. Task-1 review. d22199a
- `hc send` retry: `hc wait <dispatcher> --timeout 100`, up to 3 times, then tell the human — same wording in plan and build (Bash tool default timeout is 120 s; hc wait default 180 s). 8757e53
- Reviewers in the desktop app no longer `hc send` a "done" ping (it pastes a stray prompt into the busy controller; `hc wait --children all` already covers it); names are `rv-<you>-r<round>-<n>` (names are global across live terminals); `--` before the claude argv; bounded wait. Task-3 review. 8757e53
- Task 4 smoke and doc inserts run by the controller directly (verbatim plan text plus a live test; no implementer or reviewer). 56cb0df
- Branch review fixes: retry stops on `hc send` exit 3/11; unpushed-commits gate requires exit 0; `branch -d` refusal → leave it, never `-D`; reviewer rerun and cleanup do `hc kill` + `hc rm`; exact END OF REVIEW match trims indent/bullets; wait loop asks the human after 6 timeouts; `.gitignore` covers `.claude/worktrees/`, `.tally-brief.md`, `.review-*.md`. f4e9c5d
- Parked: `hc send` retry may not actually wait when the dispatcher is blocked (PLAUSIBLE p2, todo t_dmk00vsxoeogd).
