# Comments Retrieval Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Read comments across *all* targets — "recent comments" (time window) and "which items have comments" (per-target summary) — from both the CLI and MCP.

**Architecture:** One new store read primitive (`all_comments`) with two query methods (`recent_comments`, `comment_summaries`) built on it. Recency is a lexicographic string compare against an RFC3339 cutoff (no date crate). CLI and MCP are thin adapters. TUI is untouched.

**Tech Stack:** Rust (edition 2024, rustc 1.94), serde/serde_json, stdlib only. No new dependencies.

## Global Constraints

- **No new dependencies.** Timestamp math is dependency-free (string compare + a `SystemTime`-derived clock already in `todos.rs`).
- **On-disk shape and `Comment` struct are frozen** (comments-design deviation #8) — no `unix` field, no schema churn.
- **Store is the single source of truth**; `cli`/`mcp` call store methods and do not reimplement logic. If CLI and MCP disagree, that's a bug.
- **Notes-only by default** for both views (matches badge semantics); `recent` widens to events via a flag.
- **Reads only** — no revision guards, consistent with all comment ops.
- Build after changes: `cargo build --release && mkdir -p bin && rm -f bin/tally && cp target/release/tally bin/tally` (the `rm -f` before `cp` is load-bearing on macOS). Test: `cargo test`. Lint: `cargo clippy && cargo fmt --check`.

### Two deliberate deviations from the design doc (intentional, flagged here)

1. **Title resolution lives in the store, not the adapter** (design decision #2 said adapter). Reason: both CLI and MCP need the identical resolver; a single `Project::resolve_target_label` keeps it DRY and it's a pure read. The glyph/label *formatting* is trivial and shared.
2. **`recent_comments` takes a precomputed cutoff string; the window→cutoff conversion is a separate `Project::recency_cutoff(window)`** wrapper over a private `duration_cutoff(now, window)`. Adapters call `recency_cutoff` (so they never import the clock) then `recent_comments`. The design's free-function `duration_cutoff(now, window)` still exists and is unit-tested with an injected `now`.

---

### Task 1: RFC3339 ↔ epoch helpers in the store clock

`duration_cutoff` needs to turn the current time into a cutoff string `N` seconds in the past. `todos.rs` already owns the clock (`now()` → `format_rfc3339(secs)`). Add the inverse (`epoch_from_rfc3339`) and its supporting `days_from_civil` (Howard Hinnant's companion to the existing `civil_from_days`), and widen `format_rfc3339`'s visibility so `comments.rs` can reuse it. This keeps the timestamp format single-sourced.

**Files:**
- Modify: `src/store/todos.rs` (near `format_rfc3339`/`civil_from_days`, ~line 123-146)

**Interfaces:**
- Produces:
  - `pub(crate) fn format_rfc3339(secs: u64) -> String` (visibility change only)
  - `pub(crate) fn epoch_from_rfc3339(s: &str) -> Option<u64>` — parses the exact `YYYY-MM-DDTHH:MM:SSZ` shape `now()` emits; `None` on any other shape.
  - `fn days_from_civil(y: i64, m: u32, d: u32) -> i64` (private helper)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `src/store/todos.rs` (create the module if none exists there; if one exists, append):

```rust
#[test]
fn test_rfc3339_epoch_roundtrip() {
    // Known value: 2026-07-10T12:00:00Z
    let e = epoch_from_rfc3339("2026-07-10T12:00:00Z").unwrap();
    assert_eq!(format_rfc3339(e), "2026-07-10T12:00:00Z");
    // now() round-trips through the epoch parser
    let n = now();
    assert_eq!(format_rfc3339(epoch_from_rfc3339(&n).unwrap()), n);
    // malformed inputs are rejected, not panicked on
    assert_eq!(epoch_from_rfc3339("2026-07-10"), None);
    assert_eq!(epoch_from_rfc3339(""), None);
    assert_eq!(epoch_from_rfc3339("2026-07-10T12:00:00+00:00"), None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib store::todos::tests::test_rfc3339_epoch_roundtrip`
Expected: FAIL — `epoch_from_rfc3339` not found (won't compile).

- [ ] **Step 3: Write the implementation**

In `src/store/todos.rs`, change `fn format_rfc3339` to `pub(crate) fn format_rfc3339`, and add after `civil_from_days`:

```rust
// Inverse of civil_from_days: (y, m, d) -> days since 1970-01-01.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Parse the fixed-width "YYYY-MM-DDTHH:MM:SSZ" that now() emits back to unix
/// seconds. None for any other shape (callers degrade to "all time").
pub(crate) fn epoch_from_rfc3339(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[19] != b'Z' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)? as u32, n(8..10)? as u32);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3_600 + mi * 60 + se;
    u64::try_from(secs).ok()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib store::todos::tests::test_rfc3339_epoch_roundtrip`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/store/todos.rs
git commit -m "feat(store): rfc3339->epoch parser for recency cutoffs"
```

---

### Task 2: Store — `all_comments`, cutoff math, `recent_comments`

The recency read path. `all_comments` is the primitive both queries share. `recent_comments` filters by cutoff/author/kind and returns newest-first.

**Files:**
- Modify: `src/store/comments.rs`

**Interfaces:**
- Consumes: `now`, `format_rfc3339`, `epoch_from_rfc3339` from `super::todos` (Task 1).
- Produces:
  - `pub(crate) fn all_comments(&self) -> Result<Vec<Comment>>`
  - `pub fn recent_comments(&self, cutoff: &str, author: Option<&str>, include_events: bool) -> Result<Vec<Comment>>` — newest-first.
  - `pub fn recency_cutoff(&self, window: &str) -> String` — window (`30m`/`2h`/`1d`) → RFC3339 cutoff; `""` (all time) on malformed/empty.
  - `fn duration_cutoff(now: &str, window: &str) -> String` (private, unit-tested)

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `src/store/comments.rs`:

```rust
#[test]
fn test_duration_cutoff() {
    let now = "2026-07-10T12:00:00Z";
    assert_eq!(duration_cutoff(now, "2h"), "2026-07-10T10:00:00Z");
    assert_eq!(duration_cutoff(now, "30m"), "2026-07-10T11:30:00Z");
    assert_eq!(duration_cutoff(now, "1d"), "2026-07-09T12:00:00Z");
    assert_eq!(duration_cutoff(now, "90s"), "2026-07-10T11:58:30Z");
    // malformed / empty -> "all time" (empty string), never a panic
    assert_eq!(duration_cutoff(now, "xyz"), "");
    assert_eq!(duration_cutoff(now, ""), "");
    assert_eq!(duration_cutoff(now, "2"), "");
}

#[test]
fn test_recent_comments_cutoff_author_events() {
    let mut tp = new_project();
    tp.p.actor = "jason".to_string();
    tp.add_comment("t_a", "", "fresh note").unwrap(); // created = now()
    // Seed a backdated note + an event directly (tests module can reach the
    // private load/save on the store).
    let mut cf = tp.load_comments().unwrap();
    cf.comments.push(Comment {
        id: "c_old".into(), target: "t_a".into(), section: String::new(),
        author: "ana".into(), created: "2000-01-01T00:00:00Z".into(),
        kind: "note".into(), text: "old note".into(),
    });
    cf.comments.push(Comment {
        id: "c_ev".into(), target: "t_a".into(), section: String::new(),
        author: "jason".into(), created: "2026-07-10T12:00:00Z".into(),
        kind: "event".into(), text: "marked done".into(),
    });
    tp.save_comments(&cf).unwrap();

    // cutoff excludes the 2000 note, keeps the fresh note; events off by default
    let cutoff = "2020-01-01T00:00:00Z";
    let r = tp.recent_comments(cutoff, None, false).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].text, "fresh note");

    // include_events widens; boundary is inclusive (event created == cutoff)
    let r = tp.recent_comments("2026-07-10T12:00:00Z", None, true).unwrap();
    assert!(r.iter().any(|c| c.text == "marked done"));

    // author filter
    let r = tp.recent_comments("", Some("ana"), false).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].text, "old note");

    // newest-first: empty cutoff (all time), notes only -> [fresh, old]
    let r = tp.recent_comments("", None, false).unwrap();
    assert_eq!(r.first().unwrap().text, "fresh note");
    assert_eq!(r.last().unwrap().text, "old note");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib store::comments::tests::test_duration_cutoff store::comments::tests::test_recent_comments_cutoff_author_events`
Expected: FAIL — `duration_cutoff` / `recent_comments` not found.

- [ ] **Step 3: Write the implementation**

In `src/store/comments.rs`, change the import at the top:

```rust
use super::todos::{epoch_from_rfc3339, format_rfc3339, now};
```

Add these methods inside `impl Project` (next to `list_comments`):

```rust
/// Every comment in the store, file order (chronological). The primitive
/// the recent/summary queries share.
pub(crate) fn all_comments(&self) -> Result<Vec<Comment>> {
    Ok(self.load_comments()?.comments)
}

/// Comments at or after `cutoff` (an RFC3339 string; "" = all time),
/// optionally filtered by author, notes-only unless include_events.
/// Newest-first.
pub fn recent_comments(
    &self,
    cutoff: &str,
    author: Option<&str>,
    include_events: bool,
) -> Result<Vec<Comment>> {
    let mut v: Vec<Comment> = self
        .all_comments()?
        .into_iter()
        .filter(|c| c.created.as_str() >= cutoff)
        .filter(|c| include_events || c.kind == "note")
        .filter(|c| author.map_or(true, |a| c.author == a))
        .collect();
    v.reverse(); // file order is chronological -> newest-first
    Ok(v)
}

/// Window ("30m"/"2h"/"1d") -> RFC3339 cutoff string via the store clock.
pub fn recency_cutoff(&self, window: &str) -> String {
    duration_cutoff(&now(), window)
}
```

Add these free functions at module scope (below the `impl Project` block, above `#[cfg(test)]`):

```rust
/// N{s,m,h,d} -> seconds. None on any other shape.
fn parse_window(w: &str) -> Option<u64> {
    let w = w.trim();
    let (num, unit) = w.split_at(w.len().checked_sub(1)?);
    let n: u64 = num.parse().ok()?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => return None,
    };
    n.checked_mul(mult)
}

/// `now` minus `window`, as an RFC3339 cutoff string. Malformed window (or
/// unparseable now) -> "" so a typo degrades to "all time", never an error.
fn duration_cutoff(now: &str, window: &str) -> String {
    match (parse_window(window), epoch_from_rfc3339(now)) {
        (Some(w), Some(base)) => format_rfc3339(base.saturating_sub(w)),
        _ => String::new(),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib store::comments::tests::test_duration_cutoff store::comments::tests::test_recent_comments_cutoff_author_events`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/store/comments.rs
git commit -m "feat(store): all_comments + recent_comments with string-compare recency"
```

---

### Task 3: Store — `comment_summaries` + `CommentSummary` + target labels

The per-target view and the shared target→label resolver (used by both adapters). `read_pad` becomes `pub(crate)` so the resolver can look up a pad title from `comments.rs`.

**Files:**
- Modify: `src/store/comments.rs` (struct + two methods)
- Modify: `src/store/scratchpads.rs:248` (visibility of `read_pad`)
- Modify: `src/store/mod.rs:14` (export `CommentSummary`)

**Interfaces:**
- Consumes: `all_comments` (Task 2), `get_todo` (`src/store/todos.rs:214`), `read_pad` (`src/store/scratchpads.rs:248`).
- Produces:
  - `pub struct CommentSummary { pub target: String, pub count: usize, pub latest: String, pub created: String }`
  - `pub fn comment_summaries(&self) -> Result<Vec<CommentSummary>>` — notes-only, one per target, newest-commented target first.
  - `pub fn resolve_target_label(&self, target: &str) -> String` — `t_…`→`☐/☑ title`, `s_…`→`• title`, else the raw target (plan rel_path or unresolvable).

- [ ] **Step 1: Write the failing test**

Append to `mod tests` in `src/store/comments.rs`:

```rust
#[test]
fn test_comment_summaries_and_labels() {
    let tp = new_project();
    // Two targets: a real todo and a real pad; comment on each.
    let t = tp.create_todo("Fix auth", "", "", Vec::new()).unwrap();
    let s = tp.create_scratchpad("Design notes", "# H\nbody", Vec::new()).unwrap();
    tp.add_comment(&t.id, "", "first").unwrap();
    tp.add_comment(&s.id, "", "hold off").unwrap();
    tp.add_comment(&t.id, "", "second").unwrap(); // latest note on the todo
    tp.add_comment_event(&t.id, "marked done").unwrap(); // event: ignored by summaries

    let sums = tp.comment_summaries().unwrap();
    assert_eq!(sums.len(), 2);
    // newest-commented target first == the todo (its 2nd note is most recent)
    assert_eq!(sums[0].target, t.id);
    assert_eq!(sums[0].count, 2); // notes only, event excluded
    assert_eq!(sums[0].latest, "second");

    // labels
    assert_eq!(tp.resolve_target_label(&t.id), format!("☐ Fix auth"));
    assert_eq!(tp.resolve_target_label(&s.id), format!("• Design notes"));
    assert_eq!(tp.resolve_target_label("docs/plan.md"), "docs/plan.md");
    assert_eq!(tp.resolve_target_label("t_gone"), "t_gone"); // unresolvable
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib store::comments::tests::test_comment_summaries_and_labels`
Expected: FAIL — `comment_summaries` / `resolve_target_label` not found.

- [ ] **Step 3: Write the implementation**

In `src/store/scratchpads.rs:248`, change `fn read_pad` to `pub(crate) fn read_pad`.

In `src/store/comments.rs`, add the struct above `impl Project` (after `CommentsFile`):

```rust
/// One row of the per-target comment view: note count + the most recent
/// note's text (snippet) and timestamp (for ordering).
#[derive(Debug, Clone, Serialize)]
pub struct CommentSummary {
    pub target: String,
    pub count: usize,
    pub latest: String,
    pub created: String,
}
```

Add these methods inside `impl Project`:

```rust
/// One row per target that has notes: count + most-recent note snippet.
/// Notes only (events don't accrue badges). Newest-commented target first.
pub fn comment_summaries(&self) -> Result<Vec<CommentSummary>> {
    // target -> (count, latest_text, latest_created). File order is
    // chronological, so ">=" keeps the last note seen as the latest.
    let mut by: HashMap<String, (usize, String, String)> = HashMap::new();
    for c in self.all_comments()? {
        if c.kind != "note" {
            continue;
        }
        let e = by.entry(c.target).or_insert((0, String::new(), String::new()));
        e.0 += 1;
        if c.created >= e.2 {
            e.1 = c.text;
            e.2 = c.created;
        }
    }
    let mut out: Vec<CommentSummary> = by
        .into_iter()
        .map(|(target, (count, latest, created))| CommentSummary { target, count, latest, created })
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created)); // newest target first
    Ok(out)
}

/// Human label for a comment target: t_… -> "☐/☑ <todo title>",
/// s_… -> "• <pad title>", anything else (plan rel_path or an
/// unresolvable id) -> the raw target string.
pub fn resolve_target_label(&self, target: &str) -> String {
    if target.starts_with("t_") {
        if let Ok(t) = self.get_todo(target) {
            let glyph = if t.status == "completed" { "☑" } else { "☐" };
            return format!("{glyph} {}", t.title);
        }
    } else if target.starts_with("s_") {
        if let Ok(s) = self.read_pad(target) {
            return format!("• {}", s.title);
        }
    }
    target.to_string()
}
```

In `src/store/mod.rs:14`, extend the comments export:

```rust
pub use comments::{Comment, CommentSummary};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib store::comments::tests::test_comment_summaries_and_labels`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/store/comments.rs src/store/scratchpads.rs src/store/mod.rs
git commit -m "feat(store): comment_summaries + shared target label resolver"
```

---

### Task 4: CLI — `recent` and `targets` subcommands

Two new subcommands on the existing `comments` noun. Neither takes a positional. `recent` is a newest-first flat list, each row prefixed with a resolved target label; `targets` is the grouped per-target view.

**Files:**
- Modify: `src/cli/comments.rs`
- Modify: `src/cli/render.rs` (two render fns)

**Interfaces:**
- Consumes: `recency_cutoff`, `recent_comments`, `comment_summaries`, `resolve_target_label` (Tasks 2-3); `render::render_recent_comments`, `render::render_comment_summaries`.
- Produces: CLI surface `tally comments recent [--since 24h] [--author X] [--include-events] [--json]` and `tally comments targets [--json]`.

- [ ] **Step 1: Write the failing test**

Append to `mod tests` in `src/cli/comments.rs`:

```rust
#[test]
fn test_recent_and_targets() {
    let root = TempDir::new();
    let repo = git_repo();
    let proj = repo.path().to_string_lossy().into_owned();

    // Seed two comments on one target via the CLI add path.
    for body in ["one", "two"] {
        let add = ["add", "t_z", "--project", proj.as_str(), "--body", body].map(String::from);
        let mut b: Vec<u8> = Vec::new();
        assert_eq!(run(&add, Some(root.path()), &mut b), 0);
    }

    // recent --json returns both, newest-first, under {"comments":[...]}
    let recent = ["recent", "--project", proj.as_str(), "--json"].map(String::from);
    let mut rb: Vec<u8> = Vec::new();
    assert_eq!(run(&recent, Some(root.path()), &mut rb), 0);
    let s = String::from_utf8_lossy(&rb);
    assert!(s.contains("\"comments\""));
    assert!(s.find("\"two\"").unwrap() < s.find("\"one\"").unwrap(), "newest first: {s}");

    // targets --json returns one row for t_z with count 2 and a title field
    let targets = ["targets", "--project", proj.as_str(), "--json"].map(String::from);
    let mut tb: Vec<u8> = Vec::new();
    assert_eq!(run(&targets, Some(root.path()), &mut tb), 0);
    let s = String::from_utf8_lossy(&tb);
    assert!(s.contains("\"target\": \"t_z\""), "targets json: {s}");
    assert!(s.contains("\"count\": 2"), "targets json: {s}");
    assert!(s.contains("\"title\""), "targets json: {s}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib cli::comments::tests::test_recent_and_targets`
Expected: FAIL — subcommands not handled (`recent`/`targets` hit the `other =>` arm, non-zero exit).

- [ ] **Step 3: Write the CLI implementation**

In `src/cli/comments.rs`, update the imports and flag tables:

```rust
use crate::store::{Comment, CommentSummary, Project};
```

```rust
const BOOL_FLAGS: &[&str] = &["json", "include-events"];
const VALUE_FLAGS: &[&str] = &["project", "body", "section", "since", "author"];
```

Read the new flags alongside the existing ones (after `let as_json = ...`):

```rust
    let since = p.str("since", "24h");
    let author = p.str("author", "");
    let include_events = p.boolean("include-events", false);
```

Add two match arms before `other =>`:

```rust
        "recent" => {
            let cutoff = proj.recency_cutoff(&since);
            let author_opt = if author.is_empty() { None } else { Some(author.as_str()) };
            match proj.recent_comments(&cutoff, author_opt, include_events) {
                Ok(list) => {
                    if as_json {
                        let _ = print_json(out, &CommentListOut { comments: &list });
                    } else {
                        let rows: Vec<(String, &Comment)> = list
                            .iter()
                            .map(|c| (proj.resolve_target_label(&c.target), c))
                            .collect();
                        let _ = render::render_recent_comments(out, &rows);
                    }
                }
                Err(e) => return fail(&e.to_string()),
            }
        }
        "targets" => match proj.comment_summaries() {
            Ok(sums) => {
                if as_json {
                    let rows: Vec<TargetOut> = sums
                        .iter()
                        .map(|s| TargetOut {
                            target: &s.target,
                            count: s.count,
                            latest: &s.latest,
                            created: &s.created,
                            title: proj.resolve_target_label(&s.target),
                        })
                        .collect();
                    let _ = print_json(out, &TargetsOut { targets: &rows });
                } else {
                    let rows: Vec<(String, &CommentSummary)> = sums
                        .iter()
                        .map(|s| (proj.resolve_target_label(&s.target), s))
                        .collect();
                    let _ = render::render_comment_summaries(out, &rows);
                }
            }
            Err(e) => return fail(&e.to_string()),
        },
```

Add the JSON output structs next to `CommentListOut`:

```rust
#[derive(Serialize)]
struct TargetOut<'a> {
    target: &'a str,
    count: usize,
    latest: &'a str,
    created: &'a str,
    title: String,
}

#[derive(Serialize)]
struct TargetsOut<'a> {
    targets: &'a [TargetOut<'a>],
}
```

Update the top-level usage string (the empty-args guard) to list the new verbs:

```rust
        return fail("usage: tally comments <add|list|delete|recent|targets>");
```

- [ ] **Step 4: Add the render functions**

In `src/cli/render.rs`, update the import and append two functions:

```rust
use crate::store::{Comment, CommentSummary, Project, Scratchpad, Todo};
```

```rust
/// Newest-first flat list; each row prefixed with its resolved target label.
pub(crate) fn render_recent_comments(
    out: &mut dyn Write,
    rows: &[(String, &Comment)],
) -> io::Result<()> {
    if rows.is_empty() {
        writeln!(out, "_No comments yet._")?;
        return Ok(());
    }
    for (label, c) in rows {
        let anchor = if c.section.is_empty() {
            String::new()
        } else {
            format!(" · {}", c.section)
        };
        writeln!(
            out,
            "- {label} — **{}**{anchor}: {}  \n  <sub>{} · {}</sub>",
            c.author, c.text, c.created, c.id
        )?;
    }
    Ok(())
}

/// Per-target view: "☐ Fix auth · 💬2 · last: "hold off"".
pub(crate) fn render_comment_summaries(
    out: &mut dyn Write,
    rows: &[(String, &CommentSummary)],
) -> io::Result<()> {
    if rows.is_empty() {
        writeln!(out, "_No comments yet._")?;
        return Ok(());
    }
    for (label, s) in rows {
        writeln!(out, "- {label} · 💬{} · last: \"{}\"", s.count, s.latest)?;
    }
    Ok(())
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib cli::comments::tests::test_recent_and_targets`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/cli/comments.rs src/cli/render.rs
git commit -m "feat(cli): comments recent + targets subcommands"
```

---

### Task 5: MCP — `comment_recent` + `comment_targets` tools (36 → 38)

Two new tools reusing the store methods. Adds three `#[serde(default)]` fields to `Args`. A dedicated `author` field (not `owner`, which carries lock-identity semantics).

**Files:**
- Modify: `src/mcp/tools.rs`

**Interfaces:**
- Consumes: `recency_cutoff`, `recent_comments`, `comment_summaries`, `resolve_target_label` (Tasks 2-3).
- Produces: MCP tools `comment_recent` (args `since`, `author`, `include_events`) and `comment_targets` (no args).

- [ ] **Step 1: Update the failing count test and add dispatch tests**

In `src/mcp/tools.rs`, change the count assertion in `test_tool_defs_count`:

```rust
        assert_eq!(
            n, 38,
            "expected exactly 38 tools (todo_* + scratchpad_* + comment_*), got {n}"
        );
```

Append two dispatch tests to `mod tests`:

```rust
    #[test]
    fn test_dispatch_comment_recent() {
        let e = Env::new();
        e.call("comment_add", r#"{"id":"t_x","body":"one"}"#).unwrap();
        e.call("comment_add", r#"{"id":"t_y","body":"two"}"#).unwrap();
        // default window (24h) captures both fresh notes, newest-first
        let r = e.call("comment_recent", "{}").unwrap();
        let arr = r.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["text"].as_str(), Some("two"));
        // author filter narrows to nothing for an unknown author
        let r = e.call("comment_recent", r#"{"author":"nobody"}"#).unwrap();
        assert_eq!(r.as_array().map(|a| a.len()), Some(0));
    }

    #[test]
    fn test_dispatch_comment_targets() {
        let e = Env::new();
        e.call("comment_add", r#"{"id":"t_x","body":"a"}"#).unwrap();
        e.call("comment_add", r#"{"id":"s_y","body":"b"}"#).unwrap();
        let r = e.call("comment_targets", "{}").unwrap();
        let arr = r.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // each row carries target, count, and a resolved title (raw id here,
        // since these targets don't exist as todos/pads)
        assert!(arr.iter().all(|row| row.get("title").is_some()));
        assert!(arr.iter().all(|row| row["count"].as_u64() == Some(1)));
    }
```

Note: the recency-boundary (comment older than the window is excluded) is covered deterministically by the store unit test in Task 2; the MCP tool cannot backdate a comment through its API, so these dispatch tests verify wiring, ordering, and the author filter.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib mcp::tools::tests::test_dispatch_comment_recent mcp::tools::tests::test_dispatch_comment_targets mcp::tools::tests::test_tool_defs_count`
Expected: FAIL — tools not registered (`unknown tool`), count still 36.

- [ ] **Step 3: Add the Args fields**

In `src/mcp/tools.rs`, extend the `// comments` section of `struct Args`:

```rust
    // comments
    section: String,
    since: String,
    author: String,
    include_events: bool,
```

- [ ] **Step 4: Register the two tools**

In `registry()`, add after the `comment_delete` tool (before the closing `]`):

```rust
        Tool { name: "comment_recent", desc: "List recent comments across all targets, newest first. since is a window like 30m/2h/1d (default 24h); author filters by author; include_events widens beyond notes to auto-logged events.",
            schema: obj(Value::Null, json!({"since": prop("string", "window: 30m|2h|1d (default 24h)"), "author": prop("string", "filter by author; empty = all"), "include_events": prop("boolean", "include auto-logged events (default false)")})),
            run: |p, a| {
                let window = if a.since.is_empty() { "24h" } else { a.since.as_str() };
                let cutoff = p.recency_cutoff(window);
                let author = if a.author.is_empty() { None } else { Some(a.author.as_str()) };
                val(p.recent_comments(&cutoff, author, a.include_events)?)
            } },
        Tool { name: "comment_targets", desc: "List every target that has comments, with note count, latest snippet, and resolved title. Newest-commented target first.",
            schema: obj(Value::Null, json!({})),
            run: |p, _a| {
                let rows: Vec<Value> = p
                    .comment_summaries()?
                    .into_iter()
                    .map(|s| json!({
                        "target": s.target,
                        "count": s.count,
                        "latest": s.latest,
                        "created": s.created,
                        "title": p.resolve_target_label(&s.target),
                    }))
                    .collect();
                Ok(Value::Array(rows))
            } },
```

Update the module doc comment at the top of the file (lines 1-2) to reflect the new count:

```rust
// Port of internal/mcp/tools.go — the 33 Solo-identical tools plus 5 comment_*
// tools (38 total). Each tool is a
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib mcp::tools::tests::test_dispatch_comment_recent mcp::tools::tests::test_dispatch_comment_targets mcp::tools::tests::test_tool_defs_count`
Expected: PASS

- [ ] **Step 6: Full suite, lint, and build**

```bash
cargo test && cargo clippy && cargo fmt --check
cargo build --release && mkdir -p bin && rm -f bin/tally && cp target/release/tally bin/tally
```
Expected: all green; binary rebuilt.

- [ ] **Step 7: Commit**

```bash
git add src/mcp/tools.rs
git commit -m "feat(mcp): comment_recent + comment_targets tools (36 -> 38)"
```

---

## Self-Review notes

- **Spec coverage:** `all_comments` (T2), `recent_comments`/`recency_cutoff`/`duration_cutoff` (T2), `comment_summaries`/`CommentSummary` (T3), `label_for`→`resolve_target_label` (T3, moved to store — see deviation #1), CLI `recent`/`targets` + renders (T4), MCP `comment_recent`/`comment_targets` + count bump + dispatch tests (T5). Default window `24h` (T4/T5). Malformed `--since`→all time (T2). Unresolvable target→raw string (T3).
- **Non-goals honored:** no TUI, no cross-project, no full-text search, no pagination (`recent_comments`/`comment_summaries` take no offset/limit — matches the design's "add if volume ever demands it").
- **Follow-up, out of scope (mention, don't fix):** `SKILL.md` and `CLAUDE.md` still say "33/36 tools" and don't document `comments recent`/`targets`; agents won't discover the new surface until those are updated. Not in the design's scope — raise as a separate docs todo.

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-07-10-comments-retrieval.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

**Which approach?**
