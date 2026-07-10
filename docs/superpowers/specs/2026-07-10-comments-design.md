# Comments on todos, scratchpads & plans — design

**Todo:** `t_dju9dqh8jmy03` — "Add the ability to comment on todos scratchpads
and docs." (Group 3 #7, builds on attribution `72a10d4`.)

## Purpose

Give you and your agents a lightweight, async back-and-forth channel on a
specific item: leave a note ("hold off, waiting on the spike"), get one back
("done, but the API was flaky"). Item-level by default; optionally anchored to a
section for review-style annotations. The same timeline doubles as a free
lightweight activity log for a chosen few auto-logged events.

Three target kinds, all heading-structured, all already surfaced in the app:

- **Todos** — store JSON, id `t_…`
- **Scratchpads** — store markdown + frontmatter, id `s_…`
- **Plans** — read-only markdown files under configured dirs
  (`docs/superpowers/specs`, `.../plans`, `docs/solutions`), addressed by
  **rel_path**, already listed/rendered on TUI tab 3 via `src/plans.rs`.

Plans are where the most back-and-forth happens, so they are a first-class
target, not an afterthought.

## Decisions (settled in brainstorm)

- **Item-level is the default.** Section anchoring is optional, never required.
- **Anchor to a section (heading), not a line.** Edit-stable and TUI-friendly;
  line numbers shift on every edit and a wrapped TUI has no stable gutter.
- **Central comments store, not embedded.** External plan files can't embed
  (tally doesn't own their schema), so one path/id-keyed store is the only
  mechanism spanning all three targets. (This reverses an earlier embed
  leaning, which was premised on only two store-owned targets.)
- **No plan registry.** Plans stay filesystem-discovered by `plans::list()` and
  keyed by rel_path. "Which plans have comments" is computed, not stored.
  Tradeoff: renaming a plan file orphans its comments (no rename-following).
- **`tally init` CLAUDE.md block is a *sibling* spec, sequenced after this** —
  documents the final surface (todos + scratchpads + plans + comments) in an
  idempotent managed block. Out of scope here.

## Data model

A single central comments store per project: `comments.json` in the per-project
store directory (`<store_root>/projects/<key>/comments.json`, alongside the
existing `todos.json` — `key` is the frozen `<base>-<sha1>`), guarded by its own
`flock` (`LOCK_EX`), mirroring the todos file. Flat list:

```rust
struct Comment {
    id: String,            // "c_…" via existing store::ids::new_id("c_")
    target: String,        // "t_…" | "s_…" | plan rel_path (docs/.../foo.md)
    section: Option<String>, // heading text; None = item-level (default)
    author: String,        // Project.actor — reuses attribution identity
    created: i64,          // unix seconds
    kind: CommentKind,     // Note | Event
    text: String,
}

enum CommentKind { Note, Event }  // serde rename: "note" / "event"
```

- **Target-type inference** is by string prefix: `t_` → todo, `s_` →
  scratchpad, otherwise → plan rel_path. (Hand-rolled, id-first, matches the
  existing CLI convention.)
- On-disk file shape: `{ "comments": [ …Comment… ] }`. `serde` with
  `#[serde(rename)]` pinned to field names, consistent with the rest of the
  store. `kind`/`section` use serde defaults so future fields stay
  migration-safe.
- **No revision guard.** Comment ops don't mutate a pad body or bump its
  revision — consistent with "todos are not revision-guarded." The store flock
  is the only concurrency ceiling.

## Behavior

### Ops (store methods; adapters are thin)

- `add_comment(target, section: Option<String>, text) -> Comment` — stamps
  `author = actor`, `created = now`, `kind = Note`, fresh `c_…` id.
- `list_comments(target) -> Vec<Comment>` — chronological (created asc).
- `delete_comment(comment_id) -> Result<()>`.

v1 excludes edit, reply threads, resolve/unresolve.

### Section anchoring

- When `section` is provided, validate the heading exists in the target body at
  add-time (reuse existing heading parsing). Reject if absent, so you can't
  anchor to a typo.
- If a heading later disappears (edit/rename), the comment is **not lost** — it
  renders under a "detached" group instead of its section group.
- Todos are effectively item-level (short title/body); `section` is allowed but
  rarely used.

### Auto-logged events (the "free activity log")

- On **todo status change** and **todo completion**, the store appends a
  `kind = Event` comment ("status: open → in progress", "marked done") authored
  by the acting `actor`. Deliberately partial — tag/blocker/body edits stay
  silent ("doesn't have to capture everything").
- Plans are read-only (tally never writes them) → no auto-events, notes only.
- Scratchpad edits are **not** auto-logged in v1 (revision already tracks pad
  edits; can add later).
- Events interleave with notes chronologically in the same timeline, rendered
  distinctly (e.g. dimmed / no author-as-speaker styling).

### Cascade & lifecycle

- **Todo/scratchpad delete** removes that target's comments.
- **`todo_transfer`** moves the todo's comments to the destination project's
  comments store (so a transferred todo keeps its history).
- **Plan-file rename/move** orphans comments (accepted; `comments prune` is a
  named follow-on, not v1).

## Surfaces (thin adapters over the store)

### CLI — new top-level `comments` noun, target-first

```
tally comments add <target> [--section "<heading>"] "<text>"
tally comments list <target>
tally comments delete <comment-id>
```

`<target>` = `t_…` | `s_…` | plan rel_path. Comments also render inline at the
foot of `todos get` and `scratchpads read` output.

### MCP — 3 new tools (→ 36 total)

`comment_add`, `comment_list`, `comment_delete`. Newline-delimited JSON-RPC 2.0
as with the rest; a panicking tool must not kill the server. `author` defaults
to the actor (the same `or_agent` identity locks use). The frozen `todo_*` /
`scratchpad_*` names are untouched; these are additive.

### TUI — one reusable comment component, three call sites

A single render component: `author · relative-time · text`, grouped by section
plus a "whole item" group, matching the approved mockup:

```
┌─ my-plan ────────────── rev 7 ─┐
│ ## Phase 1   💬2                 │
│ Ship minifier behind a flag.    │
│─ comments · Phase 1 ───────────│
│ jason ·2m  hold off, spike first│
│ claude·1m  ack, flag stays off  │
│─ comments · (whole pad) ──── 1 │
│ jason ·5m  who owns phase 2?    │
└────────────────────────────────┘
```

Reused in **three** places:

- **Todo detail** — item-level group only.
- **Scratchpad read** — section badges + threads.
- **Plans reader (tab 3)** — same, built on the existing read-only renderer;
  section badges + threads, keyed by rel_path.

Keybindings / indicators:

- **`C`** (shift-c) — add a comment. (`c` is already bound on the Todos tab;
  `C` is free on all three tabs.) Two-step, so a human can anchor from the TUI
  (not just agents via `--section`):
  1. **Anchor picker** — a small select popup listing `(whole item)` on top plus
     the body's headings (from the existing heading parser). Skipped entirely
     when the body has no headings (typical todo) → straight to step 2 as
     item-level. The reader stays a plain scroll; no section cursor is tracked.
  2. **Input** — the existing `edtui` widget, titled with the chosen anchor;
     Ctrl+D saves, Esc cancels.

  ```
  ┌ Ship minifier — new comment ─┐   ┌ new comment · Phase 1 ───┐
  │ Anchor to:                   │   │ hold off, spike first_   │
  │ » (whole pad)                │ → │ Ctrl+D save · Esc cancel │
  │   ## Phase 1                 │   └──────────────────────────┘
  │   ## Phase 2                 │
  │ j/k pick · Enter · Esc       │
  └──────────────────────────────┘
  ```
- **`x`** — delete the selected comment (`j/k` move the selection within the
  thread). Auto-events (`⋯` rows) are not selectable/deletable.
- List views show a **`💬N`** count per item/plan that has comments.
- Reuse relative-time formatting from the attribution work (`by you, 2m ago`).

## Non-goals (v1)

Comment editing, reply threads, resolve/unresolve, a plan registry, orphan
pruning, scratchpad-edit auto-events. Each layers on later without reworking the
central store.

## Test anchors

- Store round-trip: add/list/delete across all three target types.
- Section validation: anchor to missing heading rejected; heading removal →
  comment renders detached, not dropped.
- Auto-event: todo status change and completion each append one `Event`;
  tag/blocker edits append none.
- Cascade: todo delete removes comments; `todo_transfer` moves them.
- Golden project-key test in `src/store/project.rs` stays untouched.

## Follow-ons (separate todos)

- `tally init` — idempotent CLAUDE.md managed block (sibling spec, next).
- `comments prune` — drop comments whose plan rel_path no longer exists.
- Comment edit / threads / resolve if the async channel proves it needs them.
