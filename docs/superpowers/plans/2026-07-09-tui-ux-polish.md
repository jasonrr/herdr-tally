# TUI/UX Polish Batch — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land six independent TUI/UX improvements from the tally backlog — Docs→Plans rename, cross-tab `/` filter, markdown-table rendering, 2-line todo rows with relative time + lock owner, priority-at-creation, and one-key content copy.

**Architecture:** Pure adapter-layer work — no store schema or on-disk changes; all needed data already exists on `Todo`/`Scratchpad`. Changes live in `src/tui/*`, `src/docs.rs`→`src/plans.rs`, and one new `src/tui/time.rs`. The store stays untouched.

**Tech Stack:** Rust, ratatui, tui-markdown 0.3.8, crossterm. Tests are `cargo test` unit/integration tests using the existing `store::testutil` (`TempDir`, `git_repo`, `resolve_project_in`) harness.

## Global Constraints

- **macOS only**; clipboard is `pbcopy` (no new deps for clipboard).
- **No new crates** — humanize + table formatting are hand-rolled stdlib, matching the codebase's `now()`/`civil_from_days` precedent.
- **Store key format, MCP tool names, CLI surface are frozen** — this plan touches none of them.
- **Rebuild after building** for the pane binary: `cargo build --release && mkdir -p bin && rm -f bin/tally && cp target/release/tally bin/tally` (the `rm -f` is load-bearing on macOS).
- **Gate every task** on `cargo test && cargo clippy && cargo fmt --check`.
- Cursor semantics: **`app.cursor[tab]` indexes the tab's _visible_ (post-filter) list.** When the filter is empty, visible == full, so behavior is unchanged.

---

## Task 1: Docs → Plans rename

Full concept rename. The Rust compiler is the coverage tool here — a missed reference fails to build. New behavior (config-key fallback) gets a real test.

**Files:**
- Rename: `src/docs.rs` → `src/plans.rs`
- Modify: the module declaration (`grep -rn "mod docs" src` — likely `src/main.rs` or `src/lib.rs`)
- Modify: `src/tui/app.rs` (`Tab::Docs`, `app.docs`, `visible_docs`, `docs::`), `src/tui/view.rs` (label, empty-states, footer, `read_title`), `src/tui/mod.rs:2` doc-comment
- Test: `src/plans.rs` `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `plans::list(root, paths) -> Vec<Plan>`, `plans::read(&Path) -> io::Result<String>`, `plans::load_plan_paths() -> Vec<String>`, struct `Plan { rel_path, abs_path, heading, mod_time }`, `Tab::Plans`.
- Consumed by: every later task that reads `visible_plans()` / `Tab::Plans` in match arms.

- [ ] **Step 1: Add the failing config-fallback test**

In `src/plans.rs` tests (after rename in Step 3, but write it now against the target name), add:

```rust
#[test]
fn load_plan_paths_prefers_plan_paths_then_falls_back_to_doc_paths() {
    let dir = TempDir::new();
    // legacy file only -> used as fallback
    fs::write(dir.path().join("doc-paths"), "docs/legacy\n").unwrap();
    let got = load_plan_paths_from(dir.path());
    assert_eq!(got, vec!["docs/legacy"]);
    // when both exist, plan-paths wins
    fs::write(dir.path().join("plan-paths"), "docs/new\n").unwrap();
    let got = load_plan_paths_from(dir.path());
    assert_eq!(got, vec!["docs/new"]);
}
```

- [ ] **Step 2: Run it, expect a compile failure**

Run: `cargo test load_plan_paths_prefers 2>&1 | head`
Expected: FAIL — `cannot find function load_plan_paths_from` / `plans` module missing.

- [ ] **Step 3: Do the rename**

```bash
git mv src/docs.rs src/plans.rs
```

In the module-declaration file (`grep -rn "mod docs" src`): `mod docs;` → `mod plans;`.

In `src/plans.rs`:
- Rename `pub struct Doc` → `pub struct Plan` and its doc-comments ("One markdown file surfaced in the Plans tab. Port of Go's `Doc`.").
- `pub fn list(...) -> Vec<Doc>` → `-> Vec<Plan>` (and the `out: Vec<Doc>` local).
- Replace the config resolver so it returns the **directory** and tries both filenames:

```rust
/// Config directory for the plans-paths file (was Go's configFile dir).
fn config_dir_from(
    plugin_config_dir: Option<String>,
    xdg_config_home: Option<String>,
    home: Option<String>,
) -> Option<PathBuf> {
    if let Some(d) = plugin_config_dir {
        Some(PathBuf::from(d))
    } else if let Some(x) = xdg_config_home {
        Some(PathBuf::from(x).join("tally"))
    } else {
        home.map(|h| PathBuf::from(h).join(".config").join("tally"))
    }
}

fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        env_nonempty("HERDR_PLUGIN_CONFIG_DIR"),
        env_nonempty("XDG_CONFIG_HOME"),
        env_nonempty("HOME"),
    )
}

/// Loads path list from `<dir>/plan-paths`, falling back to the legacy
/// `<dir>/doc-paths`, then to `defaults()`. Split out for testing.
fn load_plan_paths_from(dir: &Path) -> Vec<String> {
    for name in ["plan-paths", "doc-paths"] {
        let p = dir.join(name);
        if p.exists() {
            return parse_paths(&p);
        }
    }
    defaults()
}

/// Parses a newline-delimited paths file (# comments and blanks ignored),
/// returning `defaults()` when it yields nothing.
fn parse_paths(path: &Path) -> Vec<String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return defaults(),
    };
    let mut paths = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        paths.push(line.to_string());
    }
    if paths.is_empty() { defaults() } else { paths }
}

pub fn load_plan_paths() -> Vec<String> {
    match config_dir() {
        Some(d) => load_plan_paths_from(&d),
        None => defaults(),
    }
}
```

Delete the old `config_file`/`config_file_from`/`load_doc_paths`/`load_doc_paths_from`. Update the three existing `config_file_*` tests to the new `config_dir_from` name and `<dir>` (drop the trailing `/doc-paths` — they now assert the directory: `Some(PathBuf::from("/plug"))`, `Some(PathBuf::from("/x/tally"))`, `Some(PathBuf::from("/h/.config/tally"))`). Rename the `load_doc_paths_*` tests' calls to `load_plan_paths_from(dir.path())` (they pass a dir, not a file path now — the two "defaults" tests just point at an empty temp dir).

In `src/tui/app.rs`:
- `use crate::docs::{self, Doc};` → `use crate::plans::{self, Plan};`
- `Tab::Docs` → `Tab::Plans` everywhere (enum variant, `idx()`, all match arms).
- field `pub docs: Vec<Doc>` → `pub plans: Vec<Plan>`; `App::new` initializer.
- `visible_docs` → `visible_plans` (fn + call sites); `self.docs` → `self.plans`; `docs::list(&self.p.path, &docs::load_doc_paths())` → `plans::list(&self.p.path, &plans::load_plan_paths())`; `docs::read` → `plans::read`.

In `src/tui/view.rs`:
- `TAB_LABELS` `"3 Docs"` → `"3 Plans"`.
- `Tab::Docs` → `Tab::Plans` (in `tab_at`, `draw_list`, `read_title`, `draw_read`, `footer`).
- Empty-states (`draw_list`): `"  No plans found. Configure paths in $HERDR_PLUGIN_CONFIG_DIR/plan-paths."` and `"  No plans match /{}"`.
- Footer: `Tab::Plans => "↑↓ move · enter read · / filter · 1·2·3 · r · q"`.

In `src/tui/mod.rs:2` doc-comment: "Docs tab" → "Plans tab".

- [ ] **Step 4: Build + test**

Run: `cargo build 2>&1 | tail -20` then `cargo test 2>&1 | tail -20`
Expected: builds clean (no dangling `Doc`/`docs::`/`Tab::Docs`); all tests pass including `load_plan_paths_prefers...`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(tui): rename Docs concept to Plans

Tab, module (docs.rs -> plans.rs), struct Doc -> Plan, and config key
doc-paths -> plan-paths with a fallback to the legacy filename so existing
configs keep working. Default dirs unchanged.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_017PreR3aCUq3dL3PfAnvJ6d
EOF
)"
```

---

## Task 2: Extend the `/` filter to Todos & Scratchpads

Route cursor-addressed reads through per-tab visible sets, ungate `/`, and make `blocked` id-keyed so it survives filtering.

**Files:**
- Modify: `src/tui/app.rs` (`blocked` field + `reload`, `visible_todos`/`visible_pads`, `count`, `selected_id`, `pin_cursor_to`, `toggle_status`, `cycle_priority`, `enter_read`, `key_list` `/` guard, `key_filter` cursor-reset)
- Modify: `src/tui/view.rs` (`draw_list` Todos/Scratchpads arms, `read_title`)
- Test: `src/tui/app.rs` tests

**Interfaces:**
- Produces: `visible_todos(&self) -> Vec<&Todo>`, `visible_pads(&self) -> Vec<&Scratchpad>`, `blocked: std::collections::HashSet<String>` (blocked todo ids). Task 4 renders 2-line rows over `visible_todos()`.
- Consumes: `Tab::Plans`, `visible_plans()` from Task 1.

- [ ] **Step 1: Failing tests for cross-tab filtering**

Add to `src/tui/app.rs` tests (mirror the existing harness — see how other tests build an `App` via `resolve_project_in` + `git_repo`):

```rust
#[test]
fn filter_narrows_todos_by_metadata() {
    let mut app = test_app_with_todos(&[
        ("Rotate tokens", "auth", "high"),
        ("Fix footer", "ui", "low"),
    ]);
    app.tab = Tab::Todos;
    app.filter = "auth".to_string(); // matches tag on the first only
    assert_eq!(app.count(), 1);
    assert_eq!(app.visible_todos()[0].title, "Rotate tokens");
    app.filter = "low".to_string(); // matches priority on the second
    app.clamp_cursor();
    assert_eq!(app.count(), 1);
    assert_eq!(app.selected_id(), Some(app.visible_todos()[0].id.clone()));
}

#[test]
fn filter_clears_on_tab_switch() {
    let mut app = test_app_with_todos(&[("A", "", "medium")]);
    app.tab = Tab::Todos;
    app.filter = "zzz".to_string();
    app.switch_tab(Tab::Scratchpads);
    assert!(app.filter.is_empty(), "filter must clear when leaving a tab");
}
```

Add a small builder near the other test helpers (follow the existing pattern for creating todos through `app.p.create_todo`):

```rust
fn test_app_with_todos(items: &[(&str, &str, &str)]) -> App {
    let (app, _guard) = /* existing harness that yields an App + tempdir guard */;
    for (title, tag, prio) in items {
        let tags = if tag.is_empty() { vec![] } else { vec![tag.to_string()] };
        app.p.create_todo(title, "", prio, tags).unwrap();
    }
    app.reload();
    app
}
```

> Note to implementer: match the exact harness the file already uses (it constructs `Project` via `resolve_project_in(store_root, Some(dir))` inside a `git_repo`). Reuse it rather than inventing a new one; keep the tempdir guard alive for the test's duration.

- [ ] **Step 2: Run, expect fail**

Run: `cargo test filter_narrows_todos filter_clears_on_tab -- --nocapture 2>&1 | tail`
Expected: FAIL — `visible_todos` not found; `count()` returns 2 not 1.

- [ ] **Step 3: Implement the visible sets + routing**

In `src/tui/app.rs`:

Change the field (near line 61): `pub blocked: Vec<bool>,` → `pub blocked: std::collections::HashSet<String>,` (import `HashSet` at top). Update `App::new` initializer to `HashSet::new()`.

In `reload` (replace the `self.blocked = ...` line): build the set from the filtered todos:

```rust
self.blocked = t
    .iter()
    .filter(|x| self.p.is_blocked(x))
    .map(|x| x.id.clone())
    .collect();
self.todos = t;
```

Add the visible accessors next to `visible_plans`:

```rust
/// Todos after the active filter: case-insensitive substring over
/// title + tags + status + priority. Empty filter -> all.
pub fn visible_todos(&self) -> Vec<&Todo> {
    if self.filter.is_empty() {
        return self.todos.iter().collect();
    }
    let q = self.filter.to_lowercase();
    self.todos
        .iter()
        .filter(|t| {
            t.title.to_lowercase().contains(&q)
                || t.status.to_lowercase().contains(&q)
                || t.priority.to_lowercase().contains(&q)
                || t.tags.iter().any(|g| g.to_lowercase().contains(&q))
        })
        .collect()
}

/// Scratchpads after the active filter: title + tags. Empty filter -> all.
pub fn visible_pads(&self) -> Vec<&Scratchpad> {
    if self.filter.is_empty() {
        return self.pads.iter().collect();
    }
    let q = self.filter.to_lowercase();
    self.pads
        .iter()
        .filter(|s| {
            s.title.to_lowercase().contains(&q)
                || s.tags.iter().any(|g| g.to_lowercase().contains(&q))
        })
        .collect()
}
```

Route the cursor-addressed reads through the visible sets:

- `count`: `Tab::Todos => self.visible_todos().len(), Tab::Scratchpads => self.visible_pads().len(), Tab::Plans => self.visible_plans().len(),`
- `selected_id`: `Tab::Todos => self.visible_todos().get(i).map(|t| t.id.clone())`, `Tab::Scratchpads => self.visible_pads().get(i).map(|s| s.id.clone())`, Plans unchanged shape.
- `pin_cursor_to`: `Tab::Todos => self.visible_todos().iter().position(|t| t.id == id)`, `Tab::Scratchpads => self.visible_pads().iter().position(|s| s.id == id)`, Plans unchanged.
- `toggle_status` (line 615): `let Some(t) = self.visible_todos().get(self.cursor[Tab::Todos.idx()]) else { return };` — then clone `id`/`done` before the mutable store call (the borrow of `self.visible_todos()` must end first: bind `let (id, done) = { let v = self.visible_todos(); let Some(t) = v.get(...) else { return }; (t.id.clone(), t.status == "completed") };`).
- `cycle_priority` (line 630): same pattern — resolve `(id, next)` from `visible_todos()` in a scoped block, then call `update_todo`.
- `enter_read` (line 672): `Tab::Todos => Ok(self.visible_todos()[self.cursor[Tab::Todos.idx()]].body.clone()),` (guard with `.get(...)` to avoid panic if the filter emptied the list — return early to List on None).

In `key_list` (line 312): drop the `if self.tab == Tab::Docs` guard on `/`:
```rust
KeyCode::Char('/') => self.mode = Mode::Filter,
```

In `key_filter` (line 469): un-hardcode the cursor reset to the active tab:
```rust
self.cursor[self.tab.idx()] = 0;
```

In `src/tui/view.rs`:
- `draw_list` Todos arm: iterate `app.visible_todos()` instead of `app.todos`; keep the empty-state (`if app.visible_todos().is_empty()`), and add a filtered empty-state mirroring Plans (`if app.filter.is_empty() { "  No todos yet." } else { format!("  No todos match /{}", app.filter) }`). Replace the blocked lookup `app.blocked.get(i)...` with `app.blocked.contains(&t.id)`.
- `draw_list` Scratchpads arm: iterate `app.visible_pads()`; add the same filtered empty-state.
- `read_title` (lines 275-277): `Tab::Todos => app.visible_todos().get(i).map(|t| t.title.clone())`, `Tab::Scratchpads => app.visible_pads().get(i).map(|s| s.title.clone())`.

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1 | tail -20`
Expected: PASS (new filter tests + all existing). Fix any borrow-checker fallout from the visible-set scoping described above.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
feat(tui): extend the / filter to Todos and Scratchpads

Filter matches across metadata (todos: title/tags/status/priority; pads:
title/tags). Cursor now indexes the visible set on every tab; blocked
becomes an id set so it survives filtering.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_017PreR3aCUq3dL3PfAnvJ6d
EOF
)"
```

---

## Task 3: Markdown tables in read mode

Pre-pass that rewrites GFM tables into monospace-aligned columns before tui-markdown sees them.

**Files:**
- Modify: `src/tui/markdown.rs` (`render` + new `reformat_tables` and helpers + tests)

**Interfaces:**
- Produces: `reformat_tables(body: &str) -> String` (called first in `render`).

- [ ] **Step 1: Failing test**

Add to `src/tui/markdown.rs` a `#[cfg(test)] mod tests`:

```rust
#[test]
fn reformat_tables_aligns_columns() {
    let input = "before\n\n| Name | Qty |\n|---|---:|\n| apples | 3 |\n| pears | 12 |\n\nafter";
    let out = reformat_tables(input);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "before");
    assert_eq!(lines[2], "Name   │ Qty");     // header padded to widest cell
    assert_eq!(lines[3], "──────┼───");        // separator sized to columns
    assert_eq!(lines[4], "apples │ 3");
    assert_eq!(lines[5], "pears  │ 12");
    assert_eq!(*lines.last().unwrap(), "after");
}

#[test]
fn reformat_tables_leaves_nontables_untouched() {
    let input = "# Heading\n\ntext | with a pipe but no delimiter row\n";
    assert_eq!(reformat_tables(input), input.trim_end_matches('\n'));
}
```

- [ ] **Step 2: Run, expect fail**

Run: `cargo test reformat_tables 2>&1 | tail`
Expected: FAIL — `reformat_tables` not found.

- [ ] **Step 3: Implement**

In `src/tui/markdown.rs`, change `render`:

```rust
pub fn render(body: &str) -> Text<'static> {
    let body = reformat_tables(body);
    owned(tui_markdown::from_str_with_options(
        &body,
        &Options::new(GlamourDark),
    ))
}
```

Add:

```rust
fn split_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

/// A GFM delimiter row: every cell is dashes with optional leading/trailing
/// colon (e.g. `---`, `:--`, `--:`, `:-:`).
fn is_delim(line: &str) -> bool {
    if !line.contains('-') {
        return false;
    }
    let cells = split_cells(line);
    !cells.is_empty()
        && cells.iter().all(|c| {
            let c = c.trim();
            !c.is_empty() && c.contains('-') && c.chars().all(|ch| ch == '-' || ch == ':')
        })
}

/// Rewrites GFM table blocks (header row + delimiter row + body rows) as
/// space-padded monospace columns joined by ` │ `, leaving all other lines
/// untouched. ponytail: alignment markers are ignored (all left-aligned);
/// wrapping of tables wider than the pane is left to the Paragraph. Not
/// code-fence aware — same known quirk as the rest of the parser.
pub(crate) fn reformat_tables(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].contains('|') && i + 1 < lines.len() && is_delim(lines[i + 1]) {
            let ncol = split_cells(lines[i]).len();
            let mut block: Vec<Vec<String>> = vec![split_cells(lines[i])];
            let mut j = i + 2;
            while j < lines.len() && lines[j].contains('|') && !lines[j].trim().is_empty() {
                block.push(split_cells(lines[j]));
                j += 1;
            }
            let mut w = vec![0usize; ncol];
            for row in &block {
                for (c, cell) in row.iter().take(ncol).enumerate() {
                    w[c] = w[c].max(cell.chars().count());
                }
            }
            let fmt_row = |row: &[String]| {
                let mut s = String::new();
                for c in 0..ncol {
                    if c > 0 {
                        s.push_str(" │ ");
                    }
                    let cell = row.get(c).map(String::as_str).unwrap_or("");
                    s.push_str(cell);
                    s.push_str(&" ".repeat(w[c].saturating_sub(cell.chars().count())));
                }
                s.trim_end().to_string()
            };
            out.push(fmt_row(&block[0]));
            let mut sep = String::new();
            for c in 0..ncol {
                if c > 0 {
                    sep.push_str("─┼─");
                }
                sep.push_str(&"─".repeat(w[c]));
            }
            out.push(sep);
            for row in &block[1..] {
                out.push(fmt_row(row));
            }
            i = j;
        } else {
            out.push(lines[i].to_string());
            i += 1;
        }
    }
    out.join("\n")
}
```

> The test's expected header `"Name   │ Qty"` assumes trailing padding on the last column is trimmed (`trim_end`), and column widths: col0 = max("Name",6,"apples",5,"pears") = 6, col1 = max("Qty",3,"12") = 3. Verify the expected strings against these widths; adjust the test literals if your padding math differs by a space (the point is columns line up, not the exact literal).

- [ ] **Step 4: Run tests**

Run: `cargo test reformat_tables 2>&1 | tail`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
feat(tui): render markdown tables as aligned columns in read mode

Pre-pass rewrites GFM tables to monospace columns before tui-markdown,
closing the raw-pipe-text gap noted in CLAUDE.md.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_017PreR3aCUq3dL3PfAnvJ6d
EOF
)"
```

---

## Task 4: 2-line todo rows — relative time + lock owner

New `time` module for humanize; `draw_list` renders todos as 2 lines and the hit-test geometry accounts for the row height.

**Files:**
- Create: `src/tui/time.rs`
- Modify: `src/tui/mod.rs` (add `mod time;`)
- Modify: `src/tui/view.rs` (`ListHits` + `list_row_at` + `draw_list` Todos/Scratchpads arms + `styled_row`)
- Test: `src/tui/time.rs`, and update `src/tui/view.rs` `list_row_at_applies_offset_and_len`

**Interfaces:**
- Produces: `time::humanize_since(updated: &str, now_secs: u64) -> String`, `time::now_unix() -> u64`. `ListHits` gains `pub row_h: u16`.
- Consumes: `visible_todos()`/`visible_pads()` from Task 2 (the Todos arm iterates the visible set).

- [ ] **Step 1: Failing test for humanize**

Create `src/tui/time.rs`:

```rust
//! Wall-clock helpers for the TUI: current unix seconds and a coarse
//! "time ago" formatter. Kept out of the store (which only needs RFC3339
//! strings) so the humanize logic is unit-testable without a clock.
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parses the fixed `YYYY-MM-DDTHH:MM:SSZ` shape `store::now()` writes into
/// unix seconds. None on any malformation.
fn rfc3339_to_secs(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T'
        || b[13] != b':' || b[16] != b':' || b[19] != b'Z'
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<i64>().ok());
    let (y, mo, d) = (n(0..4)?, n(5..7)? as u32, n(8..10)? as u32);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let days = days_from_civil(y, mo, d);
    let secs = days * 86_400 + h * 3_600 + mi * 60 + se;
    u64::try_from(secs).ok()
}

/// Inverse of the store's civil_from_days (Howard Hinnant).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Coarse "time ago": "just now", "5m", "3h", "2d", "3w". Empty string when
/// the timestamp can't be parsed (render as nothing).
pub fn humanize_since(updated: &str, now_secs: u64) -> String {
    let Some(then) = rfc3339_to_secs(updated) else {
        return String::new();
    };
    let d = now_secs.saturating_sub(then);
    match d {
        0..=59 => "just now".to_string(),
        60..=3_599 => format!("{}m", d / 60),
        3_600..=86_399 => format!("{}h", d / 3_600),
        86_400..=604_799 => format!("{}d", d / 86_400),
        _ => format!("{}w", d / 604_800),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_buckets() {
        let base = rfc3339_to_secs("2026-07-09T12:00:00Z").unwrap();
        assert_eq!(humanize_since("2026-07-09T12:00:00Z", base), "just now");
        assert_eq!(humanize_since("2026-07-09T11:55:00Z", base), "5m");
        assert_eq!(humanize_since("2026-07-09T09:00:00Z", base), "3h");
        assert_eq!(humanize_since("2026-07-07T12:00:00Z", base), "2d");
        assert_eq!(humanize_since("2026-06-18T12:00:00Z", base), "3w");
        assert_eq!(humanize_since("garbage", base), "");
    }

    #[test]
    fn roundtrips_against_store_epoch() {
        // 2026-07-09T00:00:00Z is a known day boundary; sanity-check parse.
        assert_eq!(rfc3339_to_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_to_secs("1970-01-01T00:01:00Z"), Some(60));
    }
}
```

- [ ] **Step 2: Wire the module + run, expect fail first**

Add `mod time;` to `src/tui/mod.rs`.
Run: `cargo test -p tally time:: 2>&1 | tail` (or `cargo test humanize_buckets`)
Expected: at first (before Step 1 file exists) FAIL; after adding the file, this step's test PASSES on its own. (This task's list-geometry change is tested in Step 4.)

- [ ] **Step 3: 2-line rows + geometry**

In `src/tui/view.rs`:

Extend `ListHits`:
```rust
pub struct ListHits {
    pub area: Rect,
    pub offset: usize, // first visible ITEM index
    pub len: usize,
    pub row_h: u16,    // terminal lines per item (2 for Todos, else 1)
}
```

Update `list_row_at` to map visual rows to item index by row height:
```rust
pub fn list_row_at(&self, x: u16, y: u16) -> Option<usize> {
    let l = self.list.as_ref()?;
    if !l.area.contains(Position::new(x, y)) {
        return None;
    }
    let rh = l.row_h.max(1);
    let i = ((y - l.area.y) / rh) as usize + l.offset;
    if i < l.len { Some(i) } else { None }
}
```

In `draw_list`, compute `row_h` and build rows. Replace the Todos arm (now iterating the visible set from Task 2) so each todo pushes two lines:

```rust
let now = crate::tui::time::now_unix();
// ... inside `Tab::Todos =>` after the empty-state check:
for (i, t) in app.visible_todos().iter().enumerate() {
    let glyph = if t.status == "completed" { "☑" } else { "☐" };
    let blocked = if app.blocked.contains(&t.id) { " ⛔" } else { "" };
    let line1 = format!("{glyph} [{}] {}{blocked}", t.priority, t.title);
    let rel = crate::tui::time::humanize_since(&t.updated, now);
    let meta = match &t.lock {
        Some(l) if !l.owner.is_empty() => format!("    🔒 {} · {}", l.owner, rel),
        _ => format!("    {}", rel),
    };
    let sel = i == cursor;
    rows.push(styled_row(line1, sel, cursor_style));
    rows.push(styled_meta(meta, sel, cursor_style)); // dim + optional highlight
}
```

Add a dim metadata-line helper beside `styled_row`:
```rust
fn styled_meta(row: String, selected: bool, cursor_style: Style) -> Line<'static> {
    let line = Line::from(row).dim();
    if selected { line.style(cursor_style) } else { line }
}
```

Scratchpads arm: append relative time inline (1-line): `format!("• {}  {}", s.title, crate::tui::time::humanize_since(&s.updated, now))`.

Replace the offset/scroll block so it works in item units with a per-tab row height:
```rust
let len = app.count();
let row_h: u16 = if app.tab == Tab::Todos { 2 } else { 1 };
let h_items = (list_area.height / row_h.max(1)) as usize;
let offset = if h_items > 0 && len > 0 && cursor >= h_items {
    cursor - h_items + 1
} else {
    0
};
app.hits.list = Some(ListHits {
    area: list_area,
    offset,
    len,
    row_h,
});
// scroll is in terminal lines:
f.render_widget(
    Paragraph::new(rows).scroll(((offset as u16) * row_h, 0)),
    list_area,
);
```

(Delete the old `scroll((offset as u16, 0))` render call — there is now one render at the end.)

- [ ] **Step 4: Update + add geometry tests**

Update the existing `list_row_at_applies_offset_and_len` to set `row_h: 1` in its `ListHits` literal (keeps current assertions valid). Add a 2-line case:

```rust
#[test]
fn list_row_at_maps_two_line_rows() {
    let h = Hits {
        list: Some(ListHits {
            area: Rect::new(0, 2, 80, 6), // 3 items tall at row_h 2
            offset: 0,
            len: 5,
            row_h: 2,
        }),
        ..Hits::default()
    };
    assert_eq!(h.list_row_at(10, 2), Some(0)); // line 1 of item 0
    assert_eq!(h.list_row_at(10, 3), Some(0)); // line 2 of item 0 still selects 0
    assert_eq!(h.list_row_at(10, 4), Some(1)); // item 1
    assert_eq!(h.list_row_at(10, 7), Some(2)); // item 2 second line
}
```

Run: `cargo test 2>&1 | tail -20`
Expected: PASS (time tests + geometry tests + existing). Click-to-select on either line of a todo lands the right item.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
feat(tui): 2-line todo rows with relative time and lock owner

Todos render title on line 1, a dimmed "🔒 owner · 5m" metadata line on
line 2; scratchpads get inline relative time. List hit-test geometry now
maps clicks through a per-tab row height. New tui::time humanize helper.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_017PreR3aCUq3dL3PfAnvJ6d
EOF
)"
```

---

## Task 5: Priority at creation

New-item priority is held in buffer state (the persisted-todo cycle can't apply to an unsaved item) and passed to `create_todo`.

**Files:**
- Modify: `src/tui/app.rs` (`edit_priority` field, `begin_edit_new`, `key_edit` ctrl+p, `mouse_down`/`meta_click` for new, `save_new`)
- Modify: `src/tui/view.rs` (`draw_edit` meta row shown for new todos)
- Test: `src/tui/app.rs`

**Interfaces:**
- Produces: `App.edit_priority: String`. Consumes `next_priority` (existing, app.rs:107).

- [ ] **Step 1: Failing test**

```rust
#[test]
fn new_todo_saves_chosen_priority() {
    let mut app = test_app_with_todos(&[]);
    app.tab = Tab::Todos;
    app.begin_edit_new();
    // type a title into the title editor (reuse the harness's editor-typing helper)
    set_editor_text(&mut app.title_ed, "Ship it");
    app.cycle_new_priority(); // medium -> high
    app.save_edit();
    let todos = app.p.list_todos(Default::default()).unwrap();
    let t = todos.iter().find(|t| t.title == "Ship it").unwrap();
    assert_eq!(t.priority, "high");
}
```

> `set_editor_text` — if the test module lacks a helper, set the editor via `app.title_ed = super::new_editor("Ship it", true);` (mirror `begin_edit_new`). `cycle_new_priority` is added in Step 3.

- [ ] **Step 2: Run, expect fail**

Run: `cargo test new_todo_saves_chosen_priority 2>&1 | tail`
Expected: FAIL — `cycle_new_priority` not found; default save would be `medium`.

- [ ] **Step 3: Implement**

In `src/tui/app.rs`:

Add field (near `edit_id`): `pub edit_priority: String,` and initialize in `App::new` to `String::new()`.

In `begin_edit_new` (line 741), add: `self.edit_priority = "medium".to_string();`.

Add the local cycle:
```rust
/// Cycles the pending priority for a not-yet-saved todo (no store write).
pub fn cycle_new_priority(&mut self) {
    self.edit_priority = next_priority(&self.edit_priority).to_string();
}
```

In `key_edit` ctrl+p (line 405-414), handle the new-item case:
```rust
KeyCode::Char('p') if ctrl => {
    if self.tab == Tab::Todos {
        if self.edit_id.is_empty() {
            self.cycle_new_priority();
        } else {
            self.cycle_priority();
            self.refresh_edit_updated();
            self.reload();
        }
    }
    return;
}
```

In `save_new` (line 838): `self.p.create_todo(title, body, &self.edit_priority, Vec::new())`.

In `mouse_down` Edit arm (line 546-552) — allow the meta priority click for new todos too:
```rust
if self.tab == Tab::Todos && self.meta_click_edit(m.column, m.row) {
    return;
}
```
Add `meta_click_edit` (routes priority clicks to the right cycle depending on new vs existing; status toggle stays existing-only since a new item has no status yet):
```rust
fn meta_click_edit(&mut self, x: u16, y: u16) -> bool {
    use crate::tui::view::MetaSeg;
    match self.hits.meta_seg_at(x, y) {
        Some(MetaSeg::Priority) if self.edit_id.is_empty() => {
            self.cycle_new_priority();
            true
        }
        _ if !self.edit_id.is_empty() => {
            let hit = self.meta_click(x, y);
            if hit {
                self.refresh_edit_updated();
            }
            hit
        }
        _ => false,
    }
}
```

In `src/tui/view.rs` `draw_edit`:
- `let show_meta = app.tab == Tab::Todos;` (drop the `&& !app.edit_id.is_empty()`).
- In the meta render block, branch on new vs existing:
```rust
if show_meta {
    let (status, priority) = if app.edit_id.is_empty() {
        ("open".to_string(), app.edit_priority.clone())
    } else {
        let i = app.cursor[Tab::Todos.idx()];
        match app.todos.get(i) {
            Some(t) => (t.status.clone(), t.priority.clone()),
            None => ("open".to_string(), app.edit_priority.clone()),
        }
    };
    let meta_area = parts[1];
    f.render_widget(Line::from(meta_line(&status, &priority)).dim(), meta_area);
    app.hits.meta = Some(MetaHits::new(meta_area.x, meta_area.y, &status, &priority));
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
feat(tui): set priority when creating a todo

New items carry a buffer-held priority (default medium), cycled with
ctrl+p or a meta-row click and passed to create_todo.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_017PreR3aCUq3dL3PfAnvJ6d
EOF
)"
```

---

## Task 6: One-key content copy (A3a)

`Y` copies the viewed item's content to the clipboard; `y` still copies the id. Testable seam via a target function (pbcopy itself can't run headless).

**Files:**
- Modify: `src/tui/app.rs` (`key_read` `Y`, refactor `yank`)
- Modify: `src/tui/view.rs` (footer strings)
- Test: `src/tui/app.rs`

**Interfaces:**
- Produces: `App.yank_id_target()` / `App.yank_content_target()` (the strings that would be copied).

- [ ] **Step 1: Failing test**

```rust
#[test]
fn yank_targets_id_and_content() {
    let mut app = test_app_with_scratchpad("Notes", "line one\nline two");
    app.tab = Tab::Scratchpads;
    app.reload();
    app.enter_read(); // loads read_body
    assert_eq!(app.yank_content_target().as_deref(), Some("line one\nline two"));
    assert_eq!(app.yank_id_target(), app.selected_id());
}
```

> `test_app_with_scratchpad` — build via `app.p.create_scratchpad("Notes", "line one\nline two", vec![])`, then `app.reload()`, cursor at 0. Mirror `test_app_with_todos`.

- [ ] **Step 2: Run, expect fail**

Run: `cargo test yank_targets_id_and_content 2>&1 | tail`
Expected: FAIL — `yank_content_target` not found.

- [ ] **Step 3: Implement**

In `src/tui/app.rs`, refactor `yank` (line 853) into targets + copy:

```rust
fn yank_id_target(&self) -> Option<String> {
    self.selected_id()
}

fn yank_content_target(&self) -> Option<String> {
    if self.read_body.is_empty() {
        None
    } else {
        Some(self.read_body.clone())
    }
}

fn yank(&mut self) {
    let Some(id) = self.yank_id_target() else { return };
    match clipboard_write(&id) {
        Ok(()) => self.status = format!("copied {id} to clipboard"),
        Err(e) => self.status = format!("copy failed: {e}"),
    }
}

fn yank_content(&mut self) {
    let Some(c) = self.yank_content_target() else { return };
    let n = c.len();
    match clipboard_write(&c) {
        Ok(()) => self.status = format!("copied {n} bytes to clipboard"),
        Err(e) => self.status = format!("copy failed: {e}"),
    }
}
```

In `key_read` (line 359), add: `KeyCode::Char('Y') => self.yank_content(),` (keep `'y' => self.yank()`).

In `src/tui/view.rs` `footer`, Read-mode strings: add `Y copy` —
- Todos: `"space done · p prio · e edit · y id · Y copy · R raw · esc back"`
- Scratchpads: `"e edit · y id · Y copy · R raw · esc back"`
- Plans: `"y id · Y copy · R raw · esc back"`

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "$(cat <<'EOF'
feat(tui): Y copies the viewed item's content to the clipboard

y still copies the id; Y copies read-mode body. Sidesteps the terminal
selection column-trash problem in stacked panes by copying tally's own data.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_017PreR3aCUq3dL3PfAnvJ6d
EOF
)"
```

---

## Final verification

- [ ] `cargo test && cargo clippy && cargo fmt --check` all clean.
- [ ] Rebuild the pane binary: `cargo build --release && mkdir -p bin && rm -f bin/tally && cp target/release/tally bin/tally`.
- [ ] Smoke: `./bin/tally tui todos` — todos show 2 lines with time; `/` filters todos and scratchpads across metadata; `3` tab reads "Plans"; open a doc/scratchpad with a markdown table and confirm aligned columns; `Y` copies content; create a todo and set its priority before saving.

## Deferred (own spec, not this plan)

**A3b — real in-TUI text selection**: keyboard/mouse sub-range selection with a highlight rendered over the wrapped read view, coords mapped through the scroll offset, copy-selection → pbcopy. The genuine "better select handling" goal; its own design surface. Tracked as a dedicated todo.

## Self-review notes

- **Spec coverage:** A1→T3, A2→T4, A3a→T6, A3b→deferred (explicit), B1→T5, B2→T2, C1→T1. All six batch items covered.
- **Type consistency:** `visible_todos`/`visible_pads`/`visible_plans` return `Vec<&_>`; `blocked` is `HashSet<String>` used by T2 (build) and T4 (`contains`); `ListHits.row_h` added in T4 and consumed by `list_row_at`; `edit_priority: String` in T5. `Tab::Plans` from T1 used in all later match arms.
- **Ordering dependency:** T1 (rename) before all others so match arms reference `Tab::Plans`; T2 (visible sets + `blocked` as set) before T4 (renders over `visible_todos()`, reads `blocked.contains`).
