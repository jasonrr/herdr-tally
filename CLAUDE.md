# CLAUDE.md

Rust port of [herdr-notes](../herdr-notes) (Go). Fresh repo so port history doesn't
pollute the log; the Go repo stays the reference implementation until the port ships.

## Why the port

bubbletea's stack put a ceiling on mouse interactivity: a split-read SGR parser bug
leaks garbage runes into edit mode, and bubbles' textarea hides its viewport and
force-follows the cursor (no scroll-without-cursor, no click-to-place, no selection).
crossterm buffers partial sequences; ratatui is immediate-mode with app-owned state;
edtui gives soft wrap + native mouse editing. Both pre-commit spikes passed
(rendering via tui-markdown, editing via edtui).

## Parity invariants — break these and data orphans or agents break

- **Store key is byte-compatible with Go**: `<base>-<sha1(abspath)[:8]>`, project
  root via `git rev-parse --path-format=absolute --git-common-dir` (worktrees share
  one store), symlinks resolved. Golden test in `src/store/project.rs` uses a value
  from the live Go store — never "fix" that test.
- **One-way migration**: this binary must READ everything the Go binary wrote
  (todos.json, scratchpad frontmatter). It need NOT stay Go-writable — the user is
  the only user and cut over one way.
- **MCP tool names Solo-identical** (`todo_*` / `scratchpad_*`, 33 tools) — agent
  prompts depend on them. Newline-delimited JSON-RPC 2.0 over stdio, NOT
  Content-Length framing. `notifications/initialized` gets no response. A panicking
  tool must not kill the server (Go used recover; use Result or catch_unwind).
- **CLI surface unchanged**, including id-first arg order
  (`todos update <id> --status x`) — SKILL.md and agent muscle memory depend on it.
  Hand-roll arg parsing; don't adopt clap conventions.
- **Revision guards (scratchpads only)**: every mutating scratchpad op takes an
  expected revision; `-1` skips ONLY for append/append-section; enforced in BOTH the
  CLI and MCP adapters. Todos are not revision-guarded (Solo parity); flock is their
  only ceiling.
- **Deliberate quirks to preserve, not fix**: MCP `todo_update` treats empty string
  as "unchanged"; `LockTodo` allows lock-stealing; save/load_from_file are not
  path-sandboxed; heading parsing is not code-fence aware.

## Crate decisions (settled by spike — don't relitigate)

- **edtui** for edit mode, NOT tui-textarea: tui-textarea has no soft wrap and pins
  ratatui 0.29. Use `EditorEventHandler::emacs_mode()` (modeless); intercept our
  bindings (Ctrl+D save, Esc discard-confirm) before forwarding.
- **tui-markdown** + custom `StyleSheet` (glamour-dark-like) for read mode. syntect
  highlighting is on by default. Known gap: tables pass through as raw pipe-text —
  needs a follow-up pre-pass; don't block the port on it.
- **sha1_smol** (key parity), **libc** flock (`LOCK_EX`, same BSD flock as Go — the
  two binaries are lock-compatible during transition), **serde/serde_json** with
  `#[serde(rename)]` pinned to the Go field names.
- Everything else stdlib: `process::Command` for git, string ops for frontmatter,
  temp+rename for atomic writes, stdin lines for MCP.

## Architecture (same as Go)

`store` is the single source of truth; `cli`, `mcp`, `tui` are thin adapters calling
store methods. Logic and tests live in store. If CLI and MCP disagree, that's a bug.

## Build, test

```bash
cargo build --release && mkdir -p bin && rm -f bin/herdr-notes && cp target/release/herdr-notes bin/herdr-notes
cargo test
cargo clippy && cargo fmt --check
```

`herdr plugin link` does NOT run the manifest `[[build]]` step — build by hand after
linking. Panes/scripts expect the binary at `bin/herdr-notes` exactly.

**`rm -f` before the `cp` is load-bearing on macOS.** Overwriting the signed
Mach-O at `bin/herdr-notes` in place leaves a stale kernel code-signature cache,
and the binary is then SIGKILLed at exec (`Killed: 9`, exit 137, no output) even
though `codesign -v` reports "valid on disk". A fresh inode (rm then cp) avoids it;
`codesign -f -s - bin/herdr-notes` also fixes an already-broken copy.

## herdr integration gotchas (verified live against herdr 0.7.3 — stack-independent)

- Panes open with cwd = the user's project; actions run with cwd = the plugin root.
  Pane commands must locate the binary via `$HERDR_PLUGIN_ROOT` (see manifest).
- herdr's server runs under launchd with a bare PATH — export
  `/opt/homebrew/bin:/usr/local/bin` in every script/pane command that needs tools.
  Use `$HERDR_BIN_PATH` (fallback `/opt/homebrew/bin/herdr`) for herdr itself.
- Mutation actions derive the project from `$HERDR_PLUGIN_CONTEXT_JSON`
  (`.focused_pane_cwd // .workspace_cwd`), never cwd.
- `herdr pane list` is already JSON, has no per-pane command field — match on
  `.label` scoped to `$HERDR_WORKSPACE_ID` (see scripts/toggle-pane.sh).
- `pane open --placement split` needs a target pane; use `--placement tab` for
  headless/test opens. No focus-by-id; `pane zoom <id> --on/--off` is the focus
  primitive.
- Test panes in a throwaway `herdr workspace create --no-focus` +
  `herdr wait output <pane> --match ... --timeout`, then `workspace close`.

## Port order & status

1. [x] scaffold: repo, deps, dispatch, store key + golden test
2. [x] store: todos.json (+ project.json write on resolve), scratchpads +
       frontmatter, flock + atomic write, revision guards — port Go store tests
3. [x] cli: todos + scratchpads subcommands, `--json`, `--project`, stdin bodies
4. [x] mcp: server loop + 33 tools
5. [x] docs: doc-paths config + listing (Go internal/docs, 150 LOC)
6. [x] tui: ratatui app — tabs/list/read (tui-markdown), edit (edtui), mouse
7. [x] cut over: relink plugin, retire Go binary

Post-port follow-ups now live as todos in the herdr-notes store, not here.

Reference Go sources: `../herdr-notes/internal/{store,cli,mcp,docs,tui}`.

## macOS-only for now

Manifest declares `platforms = ["macos"]`; scripts hardcode Homebrew paths.
