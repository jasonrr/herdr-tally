# CLAUDE.md

**tally** — project-scoped todos & scratchpads for you and your agents, shipped as a
herdr plugin. One store behind three thin adapters: a CLI, an MCP server (38 tools),
and a ratatui TUI.

## Architecture

`store` is the single source of truth; `cli`, `mcp`, `tui` are thin adapters calling
store methods. Logic and tests live in store. If CLI and MCP disagree, that's a bug.

## Invariants — break these and data orphans or agents break

The store is an embedded automerge (CRDT) doc, persisted as one full-doc snapshot
file per machine at `projects/<key>/automerge/<machine-hex>.automerge` — each
machine is the sole writer of its own file; loading a project merges every
machine's file.

- **Store key format is frozen**: `<base>-<sha1(abspath)[:8]>`, project root via
  `git rev-parse --path-format=absolute --git-common-dir` (worktrees share one store),
  symlinks resolved. The golden test in `src/store/project.rs` pins it — the store is
  keyed by this, so any change orphans existing data. Never "fix" that test. The
  `automerge/` dir lives inside that keyed project dir, so this key/layout freeze
  still holds.
- **MCP tool names are fixed** (`todo_*` / `scratchpad_*` / `comment_*`, 38 tools) — agent prompts
  depend on them. Newline-delimited JSON-RPC 2.0 over stdio, NOT Content-Length
  framing. `notifications/initialized` gets no response. A panicking tool must not
  kill the server (return `Result` / `catch_unwind`).
- **CLI surface is id-first** (`todos update <id> --status x`), mirroring the MCP
  tools — non-MCP callers depend on it. Arg parsing is hand-rolled; don't adopt clap
  conventions.
- **Revision guards (scratchpads only)**: every mutating scratchpad op takes an
  expected revision; `-1` skips ONLY for append/append-section; enforced in BOTH the
  CLI and MCP adapters. Todos are not revision-guarded; flock is their only ceiling.
  Cross-machine, these guards soften to advisory — the CRDT subsumes their
  conflict-prevention job — but the `RevisionMismatch`/`ConcurrentEdit` API contract
  is preserved.
- **Deliberate quirks to preserve, not fix**: MCP `todo_update` treats empty string
  as "unchanged"; `LockTodo` allows lock-stealing; save/load_from_file are not
  path-sandboxed; heading parsing is not code-fence aware.

## Crate decisions (settled by spike — don't relitigate)

- **edtui** for edit mode, NOT tui-textarea: tui-textarea has no soft wrap and pins
  ratatui 0.29. Use `EditorEventHandler::emacs_mode()` (modeless); intercept our
  bindings (Ctrl+D save, Esc discard-confirm) before forwarding.
- **tui-markdown** + custom `StyleSheet` (glamour-dark-like) for read mode. syntect
  highlighting is on by default. Known gap: tables pass through as raw pipe-text —
  needs a follow-up pre-pass.
- **automerge 0.11 + autosurgeon 0.13** is the store backend — autosurgeon
  reconciles/hydrates Rust structs into the per-machine doc. **sha1_smol** (store
  key), **libc** flock (`LOCK_EX`). **serde/serde_json** now serve legacy-JSON
  migration parsing and `tally dump` output, with `#[serde(rename)]` still pinned to
  the legacy on-disk field names the migration reader expects.
- **ignore** owns plan-path glob matching — the one non-stdlib exception here.
  `plan-paths` lines are reverse-gitignore globs; `src/plans.rs::list` builds an
  `ignore::gitignore::Gitignore` and inverts its verdict per file
  (`matched_path_or_any_parents`, `Ignore => include`) so a bare-dir line still
  parent-matches every `.md` beneath it. Pure `overrides` don't parent-match — the
  design's documented fallback, taken during implementation.
- Everything else stdlib: `process::Command` for git, string ops for frontmatter,
  temp+rename for atomic writes, stdin lines for MCP.

## Build, test

```bash
cargo build --release && mkdir -p bin && rm -f bin/tally && cp target/release/tally bin/tally
pkill -f 'bin/tally tui'; pgrep -fl 'bin/tally mcp'  # kill stale panes; RECONNECT any mcp listed
cargo test
cargo clippy && cargo fmt --check
```

`herdr plugin link` does NOT run the manifest `[[build]]` step — build by hand after
linking. Panes/scripts expect the binary at `bin/tally` exactly.

The store is now a binary automerge blob, not a text file — `cat todos.json` no
longer works. Inspect it with `tally dump [--json]`.

**Line 2 is not optional.** The rebuild above swaps `bin/tally` to a fresh inode, but
already-running TUI panes and MCP servers keep executing the OLD image. This used to
mean silent data loss (a whole-file write from stale code dropped fields it didn't
know — how a linked todo's `github` block once vanished); autosurgeon's reconcile
never prunes unmodeled map keys and preserves `Todo.extra`, so a stale image now
loads+merges and its snapshot write merges rather than clobbers — that footgun is
largely defused. The remaining reason to care: you're just running stale code. Kill
stale panes yourself and reconnect any MCP session `pgrep` lists (don't `pkill`
those — they're stdio children of live agent sessions). Panes built after that fix
also nag in their own footer.

**`rm -f` before the `cp` is load-bearing on macOS.** Overwriting the signed
Mach-O at `bin/tally` in place leaves a stale kernel code-signature cache,
and the binary is then SIGKILLed at exec (`Killed: 9`, exit 137, no output) even
though `codesign -v` reports "valid on disk". A fresh inode (rm then cp) avoids it;
`codesign -f -s - bin/tally` also fixes an already-broken copy.

## herdr integration gotchas (verified live against herdr 0.8.0)

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
  `herdr pane wait-output <pane> --match ... --timeout`, then `workspace close`.
- **On a herdr version bump, run `scripts/verify-herdr.sh`** (PASS/FAIL over every
  surface tally uses; `--bounce` restarts the server first, run detached). It's what
  re-verifies the pin above — see the `verify-herdr-version` skill. Gotcha it encodes:
  `pane list` emits JSON by default and *rejects* `--json`.

## macOS-only for now

Manifest declares `platforms = ["macos"]`; scripts hardcode Homebrew paths.

Follow-ups live as todos in the tally store, not here.
