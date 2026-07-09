# TUI/UX polish batch — design

Date: 2026-07-09
Status: proposed

Six small-to-medium TUI/store improvements drawn from the tally todo backlog,
batched because they are independent, low-risk, and share the same handful of
files (`src/tui/{app,view,markdown,mod}.rs`, `src/docs.rs`, `src/store/*`).

One larger item — real in-TUI text selection (A3b) — is **out of scope here**
and gets its own spec (see "Deferred" below).

## Guiding facts (from code map, 2026-07-09)

- Store already has the data these features need: `Todo.priority` (String),
  `Todo.updated`/`Scratchpad.updated` (RFC3339), `Todo.lock: Option<Lock>` with
  `lock.owner`. `create_todo(title, body, priority, tags)` already accepts a
  priority — the TUI just passes `""`.
- No relative-time / humanize helper exists anywhere yet.
- The `/` filter exists but is hardcoded to the Docs tab (single shared
  `filter: String`, `visible_docs()` the only filtered accessor).
- "Docs" is a **separate read-only filesystem module** (`src/docs.rs`) over
  `docs/superpowers/specs`, `docs/superpowers/plans`, `docs/solutions`,
  configured by a `doc-paths` file — NOT scratchpads.
- The TUI calls `EnableMouseCapture` (`src/tui/mod.rs:61`), which is why herdr's
  native drag-select is dead in the pane. Native/terminal selection is also
  row-based across the full width, so with left/right stacked panes it rakes in
  adjacent columns — no terminal-level selection can respect pane boundaries.
  This is why in-TUI copy is wanted, and why A3a copies tally's own data rather
  than fighting the terminal.

## Items

### A1 — Markdown tables in read mode (M)
**Where:** `src/tui/markdown.rs:51` (`render`).
**Approach:** pre-pass over `body` before handing to tui-markdown. Detect GFM
table blocks (header row, a `---|---` delimiter row, contiguous body rows),
compute per-column max display width, rewrite each block as space-padded
monospace columns (cells left-padded/truncated to column width; keep a light
`│` separator for readability). Non-table lines pass through untouched. Output
still renders inside the existing scrollable `Paragraph` (`view.rs:316`).
**Test:** unit test in `markdown.rs` — a sample 3-column table in, aligned
columnar text out; a non-table body passes through unchanged.
**Ceiling (ponytail):** alignment markers (`:---:`) are honored for
left/center/right if cheap, else left-align all; nested pipes inside code spans
are not special-cased (documented, matches the existing not-fence-aware quirk).

### A2 — Relative updated time + lock owner, 2-line todo rows (M)
**Where:** new `humanize_since(rfc3339, now) -> String` helper (near `now()` in
`src/store/todos.rs`, or a small `src/tui/time.rs`); render + list-geometry in
`src/tui/view.rs` and `src/tui/app.rs`.
**Approach:** `humanize_since` parses the stored RFC3339 against `now()` and
returns `"just now"`, `"5m"`, `"3h"`, `"2d"`, `"3w"` (coarse buckets, no
external crate).
- **Todo rows become 2 lines** (fixes narrow-pane crowding): line 1 is today's
  `{glyph} [{priority}] {title}{blocked}` (`view.rs:204`); line 2 is an indented,
  dimmed metadata line — `🔒 {owner} · {rel}` when locked, else `{rel}`.
- **Scratchpad rows stay 1 line** with `· {rel}` appended (`view.rs:214`) — one
  short metadata field, title typically short. Docs rows unchanged.
**Geometry (the load-bearing part):** the Todos list now renders 2 terminal
lines per item, so the click-to-select mapping in `mouse_down` and the
visible-rows/scroll math must convert between *item index* and *visual line*
(item i occupies lines `base + 2*i` and `+1`). Cursor movement still moves one
*item* at a time; the highlight (`styled_row`) spans both lines of the selected
item. Only the Todos tab changes height-per-row; Scratchpads/Docs stay 1:1, so
the mapping is per-tab.
**Test:** unit test on `humanize_since` bucket boundaries; an app-level test that
a click at the second visual line of item i still selects item i (guards the
index↔line math).

### A3a — Structured content copy (S)
**Where:** `src/tui/app.rs` — existing `yank()` (`:853`, bound to `y`) +
`clipboard_write()` (`:127`, already shells to pbcopy).
**Approach:** add a keystroke that copies the current item's **content** (not its
id) to pbcopy: in read mode, copy `read_body`; from a list row, copy the
resolved item's logical text (scratchpad/doc body, or a todo's `title` + `body`).
Keep `y` = copy id; add `Y` = copy content (both advertised in the footer).
Sidesteps the terminal entirely — copies tally's own data, so no column-trash.
**Test:** the existing mouse/key test harness in `app.rs` — assert `Y` populates
the clipboard sink with the body, `y` still yields the id. (Abstract the pbcopy
call behind a testable sink if not already.)

### B1 — Priority at creation (S)
**Where:** `src/tui/app.rs` (`begin_edit_new` `:741`, cycle guards `:407,:416`,
`save_new` `:838`) + `src/tui/view.rs:344` (meta-row `show_meta` gate).
**Approach:** show the status/priority meta row in create mode (currently gated
to edit), ungate `cycle_priority()` for new items, and pass the selected
priority into `create_todo(title, body, priority, tags)` instead of `""`. Default
remains `medium` when untouched.
**Test:** app-level test — create a todo, cycle priority to `high` before save,
assert the persisted todo has `priority == "high"`.

### B2 — Extend `/` filter to Todos & Scratchpads (M)
**Where:** `src/tui/app.rs` (`/` guard `:312`, cursor-reset `:469`,
`visible_docs()` `:222`, accessors `:235-283`) + `src/tui/view.rs` list-render
arms (`:188-239`) and `read_title` (`view.rs:272`).
**Approach:** keep the single shared `filter` string, cleared on tab switch
(today's behavior — no per-tab state). Ungate `/` for all three tabs;
un-hardcode the cursor-reset from `Tab::Docs`. Add `visible_todos()` and
`visible_pads()` beside `visible_docs()`; route `count / selected_id /
pin_cursor_to / read_title` and the list-render arms through the visible-set for
the active tab. **Search across metadata:** todos match filter substring against
title + tags + status + priority; scratchpads against title + tags; docs
unchanged (rel_path + heading).
**Test:** app-level tests — set filter on Todos tab, assert `count()` and
`selected_id()` reflect only matching rows; same for Scratchpads; switching tabs
clears the filter.

### C1 — Docs → Plans rename (M, mechanical)
**Where:** `src/docs.rs` → `src/plans.rs`; `src/tui/app.rs` (`Tab::Docs`,
`app.docs`, `docs::list/read`), `src/tui/view.rs:15` label + `:224-228`
empty-states + footer, `src/tui/mod.rs:2` doc-comment.
**Approach:** full concept rename. `Tab::Docs`→`Tab::Plans`; tab label
`"3 Docs"`→`"3 Plans"`; empty-state and footer strings; module and its symbols
(`docs::` → `plans::`). Config key `doc-paths` → `plan-paths`, **with fallback**:
read `plan-paths`, else fall back to an existing `doc-paths` file, so the user's
orphaned config (and anyone else's) keeps working with no migration step.
Default dirs unchanged — specs/plans/solutions are all planning artifacts.
**Test:** update existing `docs.rs` tests to the new names; add one asserting
`plan-paths` is preferred and `doc-paths` still loads as fallback.
**Non-goal:** narrowing the default dirs to plans-only (explicitly rejected).

## Deferred — A3b: real in-TUI text selection (own spec)

Keyboard/mouse-driven sub-range selection with a highlight rendered over the
wrapped read view, coords mapped through the scroll offset, copy-selection →
pbcopy. This is the genuine "better select handling" goal that motivated the
Rust port; the Go design punted on it ("Shift+drag falls through to the
terminal", `interactive-tui-panes-design.md:179`). It has its own design surface
(selection state, highlight rendering, coord math, wrap interaction) and will
get its own brainstorm/spec. Tracked as a dedicated todo.

## Suggested order

C1 (rename, low-risk, touches the `Tab` enum the others lean on) → B2 (filter) →
A1 (tables) → A2 (time/lock) → B1 (priority) → A3a (content copy). Each lands
independently with its own tests.

## Testing strategy

Every item leaves at least one runnable check (unit test in the touched module
or an app-level TUI test using the existing harness in `app.rs`). Full suite:
`cargo test`; `cargo clippy && cargo fmt --check`. Rebuild for the pane binary
per CLAUDE.md (`… && rm -f bin/tally && cp target/release/tally bin/tally`).

## Out of scope

- A3b in-TUI text selection (own spec, above).
- Any store schema / on-disk field changes (all data already present).
- MCP tool changes (frozen surface).
- Scratchpad locks (they have none; A2 lock display is todos-only).
