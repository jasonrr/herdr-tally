```
      ┌─────────────────────────────╮
      │  t a l l y                  │
      │                             │   ╭── the herder's little book ──╮
      │   ▌▌▌▌╱  sheep out ....  20 │   │  one gate, one ledger,       │
      │   ▌▌▌▌╱  sheep in  ....  20 │   │  a stroke for every head     │
      │   ▌▌▌ ╱  strays    ....   4 │   │  that passes through.        │
      │                             │   ╰──────────────────────────────╯
      ╰─────────────────────────────╯
```

# tally

**A shared ledger for you and your coding agents** — so the research, plans, and
follow-ups a project accumulates outlive any single session. Shipped as a
[herdr](https://herdr.dev) plugin.

## Why "tally"?

A drover walking a flock through a gate doesn't trust memory. They carry a
**tally book** — a pocket ledger, four strokes and a slash — and mark every head
that passes. The count in the book *is* the truth about the herd.

**herdr** herds your agents. **tally** is the book you keep on them — the shared
ledger of what's done, what's blocked, and where you left off.

## What it's for

Say you're building a real feature. You have an agent research the approach — where
does that thinking land when the session ends? You turn the research into a spec,
then a plan; plenty of tools write those. You dogfood the result and turn up a dozen
small things: a bug here, a confusing bit of UX there. Where do those go?

Without a shared place, they scatter — buried in a chat transcript that scrolls
away, kept in your head, or in a file the next agent never opens. tally is that
place: one project-scoped ledger you and your agents both write to and read back — a
two-way ledger, not a one-way log. The trail of the work outlives any single
session, and you're both looking at the same page.

You keep four kinds of artifact in it:

- **Todos** — the follow-ups, bugs, and blockers the work turns up, one each, so
  nothing slips. Priorities, tags, blockers, and a lock so whoever's on it can claim it.
- **Scratchpads** — the thinking that outgrows a todo: research results, a plan
  you're about to run, a "here's where I left off" handoff.
- **Plans** — the spec or plan you're executing against. You don't author these in
  tally — plan mode, superpowers, ce and friends already write them to disk; tally
  surfaces that markdown so you can read it and talk over it beside the live todos.
  Point it at whichever dirs hold yours.
- **Comments** — the thread that ties it together: a margin note on any todo,
  scratchpad, or plan ("skip step 3", "blocked on the auth PR"), read back across
  everything with `tally comments recent`.

One store, three thin adapters over it:

| Adapter | For | Surface |
|---------|-----|---------|
| **CLI** | anything that can't speak MCP (scripts, hooks, other agents) | `tally todos …` / `tally scratchpads …` |
| **MCP** | your agents | 38 `todo_*` / `scratchpad_*` / `comment_*` tools over stdio |
| **TUI** | the herdr pane | `tally tui todos` / `tally tui scratchpads` |

The `store` is the single source of truth for todos, scratchpads, and their
comments; plans are read straight from disk. Data lives under
`~/.local/state/tally/`, keyed by project path — worktrees of the same repo share
one store.

## Install

```bash
herdr plugin install jasonrr/herdr-tally
```

This downloads a prebuilt `tally` binary for your platform (macOS arm64/x86_64,
Linux x86_64), verifies its SHA-256, and — best-effort — registers the MCP server
with Claude Code and writes a short tally guidance block into `~/.claude/CLAUDE.md`
(idempotent, marker-delimited) so every session knows to reach for the `tally_*`
tools. No Rust toolchain is needed when a release exists for your platform;
otherwise install falls back to building from source with `cargo` (install Rust
from https://rustup.rs).

**Platforms:** macOS and Linux. Windows is not yet supported.

### If the automatic wiring is skipped

The binary and panes always install. If `claude` wasn't on `PATH` at install time,
finish the two best-effort steps manually — the installer prints the exact commands,
or find the paths yourself:

```bash
# Register the MCP server:
tally_bin=$(ls -d "$HOME"/.config/herdr/plugins/github/herdr-tally-*/bin/tally 2>/dev/null | tail -1)
claude mcp add --scope user tally -- "$tally_bin" mcp
```

The guidance block is optional — tally works without it; it just primes agents to
use the tools. If the installer couldn't write it, re-run the install step or paste
the `tally:start`/`tally:end` block from `scripts/install.sh` into `~/.claude/CLAUDE.md`.

The plugin root is version-hashed; `tail -1` picks the newest if multiple versions
are present. Prefer the exact command the installer printed over these fallbacks.

## Install from source (development)

For local development, or if you just want to build and link the binary by hand —
`herdr plugin install` (above) runs this build step for end users automatically.

macOS only for now (the panes hardcode Homebrew paths). You'll need Rust and
herdr ≥ 0.7.0.

```bash
# 1. Build the binary (herdr's `plugin link` does NOT run the build step for you)
cargo build --release && mkdir -p bin && rm -f bin/tally && cp target/release/tally bin/tally

# 2. Link it as a herdr plugin
herdr plugin link .
```

> **The `rm -f` before the `cp` is load-bearing.** Overwriting the signed binary
> at `bin/tally` in place leaves a stale kernel code-signature cache, and macOS
> then SIGKILLs it at exec (`Killed: 9`). A fresh inode avoids it. Rebuild the same
> way every time you change the code.

Open the panes from herdr (**Todos pane** / **Scratchpads pane** actions), or point
your agent's MCP config at `tally mcp`.

## Quick tour

```bash
# Todos — id-first CLI
tally todos create --title "Rotate refresh tokens" --priority high --tag auth
tally todos list --status open --json
tally todos update <id> --status in_progress
tally todos add-blocker <id> --blocker <other-id>   # can't start until <other-id> is done
tally todos complete <id>

# Scratchpads — revision-guarded, read the outline before the whole thing
tally scratchpads create --name "Auth refactor plan" --content-file -   # reads stdin
tally scratchpads read <id> --mode headings
tally scratchpads append-section <id> --heading "Progress" --content "done X" --expected-revision <r>

# Comments — margin notes on a todo, scratchpad, or plan
tally comments add <id> --body "hold off — waiting on the auth PR"
tally comments add docs/plans/auth.md --body "skip step 3, it's done"   # target a plan by its path
tally comments recent --since 2h            # newest-first across every target (default 24h)
tally comments targets                      # which items carry notes, with a snippet
```

Scratchpad writes take an expected revision — `read` returns the current one, you
pass it on the next write, and a mismatch means someone else edited it (re-read and
retry). Prefer `append` / `append-section` / `edit` over rewriting the whole pad.

The TUI carries the same work across tabs — todos, scratchpads, and a read-only
**Plans** tab (`3`) that browses the plan dirs. Filter with `/`, edit in place,
`Y` to copy an item, `?` for help.

> Plans default to `docs/superpowers/{specs,plans}` and `docs/solutions`. To browse
> other dirs, list them (one per line, relative to the repo root) in a `plan-paths`
> file under your tally config dir (`$XDG_CONFIG_HOME/tally`, else `~/.config/tally`).

## For agents

Install writes a short tally block into `~/.claude/CLAUDE.md` telling an agent to
prefer the `tally_*` MCP tools, when to reach for a todo vs. a scratchpad, and the
etiquette (short titles, complete what you finish, don't delete the human's items).
The MCP tool schemas document the rest; agents `ToolSearch` for `mcp__tally__` to
load them.

## Development

```bash
cargo test
cargo clippy && cargo fmt --check
```

Architecture notes, invariants (the store key format is frozen), and the herdr
integration gotchas live in [`CLAUDE.md`](CLAUDE.md). Read it before changing the
store — the on-disk key hashes the project path, so "fixing" it orphans everyone's
data.

Follow-ups live as todos *in the tally store itself*, not in a file. Naturally.
