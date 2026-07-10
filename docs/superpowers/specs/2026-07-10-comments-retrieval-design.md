# Comments Retrieval Design

**Goal:** Read comments across *all* targets, not just one — so "get my recent
comments" and "which items did I leave comments on" both work, from the CLI and
from an agent mid-conversation (MCP).

**Context:** The comments feature (spec `2026-07-10-comments-design.md`, shipped on
`feat/comments-impl`) can only list comments for a single known target
(`list_comments(target)`). Both new asks require reading the whole store and
filtering. TUI badges already surface per-item counts, so the TUI is untouched.

## Architecture

One new store read primitive — `all_comments()` — with two query helpers built
on it. CLI and MCP are thin adapters, same discipline as the rest of the store.
No new dependencies.

**Timestamp handling (dependency-free):** `Comment.created` is fixed-width UTC
RFC3339 (`2026-07-10T17:15:11Z`). For that format, lexicographic string `>=` is
chronological order, so recency filtering is a string compare against a cutoff —
no `chrono`, no date parsing. The adapter converts a window like `30m`/`2h`/`1d`
into a cutoff string; the store just compares.

## Deliberate decisions

1. **Notes-only by default** for both views, matching the badge semantics
   (comments-design deviation #7 — auto-events don't accrue badges). `recent`
   takes `--include-events` / `include_events` to widen to the full log.
2. **Title resolution lives in the adapter, not the store.** Joining a target
   back to its todo/pad/plan and picking a display glyph is a display concern.
   The store returns raw `(target, count, latest)`; the adapter labels it.
3. **String-compare recency, no unix field added.** Keeps the on-disk shape and
   `Comment` struct frozen (comments-design deviation #8 — no schema churn).
4. **Default window `24h`.** A sensible "recent" for a working session; override
   with `--since`.
5. **No new revision guards** — these are reads. Consistent with all comment ops.

## Components

### Store — `src/store/comments.rs`

- `pub(crate) fn all_comments(&self) -> Result<Vec<Comment>>` — returns
  `load_comments()?.comments`. The primitive both queries need.
- `pub fn recent_comments(&self, since: &str, author: Option<&str>, include_events: bool) -> Result<Vec<Comment>>`
  — `since` is an already-computed RFC3339 cutoff string. Filters
  `all_comments` to `c.created >= since`, `author` when `Some`, and
  `c.kind == "note"` unless `include_events`. Returns **newest-first**
  (reverse of file/chronological order).
- `pub fn comment_summaries(&self) -> Result<Vec<CommentSummary>>` — one pass over
  `all_comments`, notes-only. Per target: note count + the most recent note's
  text (snippet). Order: by latest-comment timestamp, newest target first.
  `CommentSummary { target: String, count: usize, latest: String, created: String }`.
- `fn duration_cutoff(now: &str, window: &str) -> Result<String>` — the only new
  logic: parse `Ns`/`Nm`/`Nh`/`Nd` into seconds, subtract from now, return an
  RFC3339 cutoff string. Built on the same clock `now()` uses (whatever
  `todos::now` uses to format — reuse its mechanism so formats match exactly and
  the string compare is valid). Lives in the store so the format is single-sourced
  with `now()`.

Empty/malformed `--since` → treat as "all time" (cutoff = empty string, which
`>=` always satisfies) rather than erroring, so a typo degrades to "show
everything" instead of a failure.

### CLI — `src/cli/comments.rs`, `src/cli/render.rs`

Two new subcommands on the existing `comments` noun (id-first parsing unchanged;
these take no positional):

- `tally comments recent [--since 30m] [--author you] [--include-events] [--json]`
  — default `--since 24h`. Newest-first flat list. Each row prefixed with a
  resolved target label (from `label_for`) so you see *where* each lives. Non-JSON
  reuses a target-prefixed variant of `render_comments`; `--json` emits
  `{"comments":[…]}`.
- `tally comments targets [--json]` — the resolved items view, grouped by tab:
  `☐ Fix auth · 💬2 · last: "hold off"`. `--json` emits the raw summaries plus
  resolved title.

- `fn label_for(proj: &Project, target: &str) -> String` — `t_…` → `get_todo`
  glyph+title (☐/☑ by status), `s_…` → `• <pad title>` (via a pad lookup), else
  the plan `rel_path` verbatim. A missing/deleted target falls back to the raw
  target string (a comment can outlive nothing — cascade deletes it — but a
  plan file removed on disk still keys by rel_path). Shared by both renders.

- `render::render_comment_summaries(out, &[(label, summary)])` — the grouped
  targets render.

### MCP — `src/mcp/tools.rs`

Two new tools (36 → 38). Reuses `Args`; adds three `#[serde(default)]` fields:
`since: String`, `author: String`, `include_events: bool`. (`Args` has `owner`
but no `author`, and `owner` carries lock-identity semantics, so a dedicated
`author` field is correct — don't overload `owner`.) Empty `author` → `None`
(all authors).

- `comment_recent` — args `since` (default `"24h"` when empty), `author`,
  `include_events`. The adapter computes the cutoff via `duration_cutoff(now(),
  since)` then calls `recent_comments`. Returns the comment array.
- `comment_targets` — no args. Returns summaries each augmented with the resolved
  `title` (so the agent needn't re-resolve).

Bump the count assert 36 → 38 and reword. Add a dispatch round-trip test
(`comment_recent` after adding two comments at least one older than the window;
`comment_targets` after commenting on two targets).

## Data flow

```
CLI recent  → parse --since → duration_cutoff(now, window) → recent_comments → render
CLI targets → comment_summaries → label_for per row → render_comment_summaries
MCP recent  → duration_cutoff(now, args.since|"24h") → recent_comments → JSON array
MCP targets → comment_summaries → label_for per row → JSON (summary + title)
```

## Error handling

- Bad `--since` → "all time" (see store note), never an error.
- `label_for` on an unresolvable target → raw target string, never an error.
- Reads only; no flock contention beyond the existing per-file lock in
  `load_comments`.

## Testing

- **Store:** `all_comments` returns everything; `recent_comments` respects the
  cutoff boundary (a comment exactly at / just before cutoff), the `author`
  filter, and `include_events`; `comment_summaries` counts notes-only with the
  right latest snippet and newest-target ordering; `duration_cutoff` parses
  `30m`/`2h`/`1d` and yields a string that string-compares correctly against
  `now()`.
- **CLI:** `recent` and `targets` round-trip over the throwaway-store harness.
- **MCP:** dispatch test for both tools; count assert 36 → 38.

## Non-goals

- No TUI changes (badges already cover per-item counts).
- No cross-project aggregation (single project store, as today).
- No full-text search over comment bodies (`recent`/`targets` are time/target
  views, not search).
- No pagination — a project's comment volume is small; add if it ever isn't.
