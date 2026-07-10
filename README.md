```
      ┌─────────────────────────────╮
      │  t a l l y                  │
      │                             │   ╭── the herder's little book ──╮
      │   ▌▌▌▌╱  sheep out ....  20  │   │  one gate, one ledger,       │
      │   ▌▌▌▌╱  sheep in  ....  20  │   │  a stroke for every head     │
      │   ▌▌▌ ╱  strays    ....   4  │   │  that passes through.        │
      │                             │   ╰──────────────────────────────╯
      ╰─────────────────────────────╯
```

# tally

**Project-scoped todos & scratchpads for you and your coding agents** — shipped as a
[herdr](https://herdr.dev) plugin.

## Why "tally"?

A drover walking a flock through a gate doesn't trust memory. They carry a
**tally book** — a pocket ledger, four strokes and a slash — and mark every head
that passes. The count in the book *is* the truth about the herd.

**herdr** herds your agents. **tally** is the book you keep on them: the shared
ledger of what's done, what's blocked, and where you left off — so the work
survives any single session and both of you are looking at the same page.

## What it does

You and your agents share two things, both scoped to the current project (inferred
from the git repo you're in):

- **Todos** — one per follow-up, blocker, or piece of work. Priorities, tags,
  blockers, and a lock so whoever's editing the work can claim it.
- **Scratchpads** — longer-lived working context: the plan before a multi-step
  task, a handoff ("here's where I left off"), snippets too big for a todo.

Anything an agent writes, you see immediately in a live herdr pane — and anything
you jot down, the agent reads back. It's a two-way ledger, not a one-way log.

One store, three thin adapters over it:

| Adapter | For | Surface |
|---------|-----|---------|
| **CLI** | you, at the terminal | `tally todos …` / `tally scratchpads …` |
| **MCP** | your agents | 33 `todo_*` / `scratchpad_*` tools over stdio |
| **TUI** | the herdr pane | `tally tui todos` / `tally tui scratchpads` |

The `store` is the single source of truth; the adapters just call into it. Data
lives under `~/.local/state/tally/`, keyed by project path — worktrees of the same
repo share one store.

## Setup

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
```

Scratchpad writes take an expected revision — `read` returns the current one, you
pass it on the next write, and a mismatch means someone else edited it (re-read and
retry). Prefer `append` / `append-section` / `edit` over rewriting the whole pad.

The TUI drives the same store: filter with `/`, edit in place, `Y` to copy an
item, `?` for help.

## For agents

The [`SKILL.md`](SKILL.md) at the repo root tells an agent when to reach for a
todo vs. a scratchpad, and the etiquette (short titles, complete what you finish,
don't delete the human's items). If you run agents in herdr, they'll pick it up.

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
