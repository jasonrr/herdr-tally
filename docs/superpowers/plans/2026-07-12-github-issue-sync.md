# GitHub Issue Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Opt-in, per-todo sync between tally todos and GitHub issues — title/body push one way (tally authoritative), comments both ways, complete/close both ways — driven by a headless `tally sync` pass and a TUI timer, transported by the `gh` CLI.

**Architecture:** All logic lives in the store crate. A pure `plan_actions` decision function turns a (todo, link, comments, issue-snapshot) tuple into a list of `Action`s; `sync_project` executes them against a `Gh` trait (real impl shells out to `gh`; tests use a fake). CLI, MCP, and TUI are thin drivers. `gh` is never a hard dependency: if it's missing or unauthed, store operations are unaffected.

**Tech Stack:** Rust (stdlib + existing deps only — `serde`, `serde_json`, `libc`, `std::process::Command`). No new crates. `gh` CLI and `git` invoked as subprocesses.

## Global Constraints

- **MCP tool count stays 38.** No new MCP tool. Opt-in is one new optional string param (`github`) on the existing `todo_update`. Tool names/schemas are agent-facing contracts.
- **Store key format is frozen** (`<base>-<sha1(abspath)[:8]>`). This feature touches no path/key code; do not modify `src/store/project.rs::project_key` or its golden test.
- **Unsynced todos serialize byte-identical to today.** The new `github` field uses `skip_serializing_if = "Option::is_none"`; a todo with no link must not gain a `"github"` key. Non-GitHub comments must not gain a `"github_comment_id"` key (`skip_serializing_if`).
- **Todos are not revision-guarded.** The per-file flock is the only concurrency ceiling. `sync_project` relies on it for cross-process safety; do not add revision guards to todos.
- **CLI surface is id-first**, mirroring MCP. `todos update <id> --github on|off`. Arg parsing is hand-rolled — do not adopt clap.
- **`todo_update` empty-string-means-unchanged quirk is preserved** (CLAUDE.md invariant). `github: ""` (or absent) = unchanged.
- **serde field names are pinned** with `#[serde(rename)]` to the on-disk JSON tags.
- **macOS-only.** Consistent with the existing manifest; no cross-platform branching required. `libc::kill` is fine.
- **Build/test after any change:**
  ```
  cargo build --release && mkdir -p bin && rm -f bin/tally && cp target/release/tally bin/tally
  cargo test
  cargo clippy && cargo fmt --check
  ```
  The `rm -f` before `cp` is load-bearing on macOS (stale code-signature cache → `Killed: 9`).

**Data-flow reference (settled by the spec — do not relitigate):**
- Title/body: tally → GH only. GH edits are clobbered on next push (accepted lossy case).
- State: if `todo.updated > link.last_pushed`, tally wins (push close/reopen); else GH wins (pull complete/reopen).
- Comments: both ways. Echo prevention is by comment id — a pulled comment carries its GH id and author `gh:<login>` and is never pushed back; a pushed comment stores the GH id it became and is never re-imported.
- `number == 0` means "sync requested, issue not yet created." `paused == true` means "unticked — link kept, sync skipped."

---

### Task 1: Data model — `GithubLink` on Todo, `github_comment_id` on Comment

**Files:**
- Modify: `src/store/todos.rs` (add `GithubLink` struct near `Lock`; add `github` field to `Todo`; add `github: None` to the `create_todo` literal)
- Modify: `src/store/comments.rs` (add `github_comment_id` field to `Comment` + `is_zero` helper; add `github_comment_id: 0` to every `Comment { .. }` literal the compiler flags)
- Modify: `src/store/mod.rs` (re-export `GithubLink`)
- Test: inline `#[cfg(test)]` in both files

**Interfaces:**
- Produces:
  ```rust
  // todos.rs
  pub struct GithubLink {
      pub repo: String,               // "owner/name"
      pub number: i64,                // 0 = sync requested, issue not yet created
      pub last_pushed: String,        // RFC3339; push when todo.updated > this
      pub last_comment_pull: String,  // RFC3339; pull GH comments created >= this
      pub paused: bool,               // true = unticked; link kept, sync skipped
  }
  // Todo gains:  pub github: Option<GithubLink>
  // Comment gains:  pub github_comment_id: i64
  ```

- [ ] **Step 1: Write the failing tests**

In `src/store/todos.rs` `mod tests`:
```rust
#[test]
fn test_unsynced_todo_serializes_without_github_key() {
    let t = Todo::default();
    let js = serde_json::to_string(&t).unwrap();
    assert!(!js.contains("github"), "unsynced todo must omit github: {js}");
}

#[test]
fn test_todo_with_github_link_roundtrips() {
    let mut t = Todo::default();
    t.github = Some(GithubLink {
        repo: "owner/name".into(),
        number: 42,
        last_pushed: "2026-07-12T00:00:00Z".into(),
        last_comment_pull: "2026-07-12T00:00:00Z".into(),
        paused: false,
    });
    let js = serde_json::to_string(&t).unwrap();
    assert!(js.contains(r#""github""#) && js.contains(r#""number":42"#), "{js}");
    let back: Todo = serde_json::from_str(&js).unwrap();
    assert_eq!(back.github, t.github);
}
```

In `src/store/comments.rs` `mod tests`:
```rust
#[test]
fn test_non_github_comment_omits_id_key() {
    let c = Comment::default();
    let js = serde_json::to_string(&c).unwrap();
    assert!(!js.contains("github_comment_id"), "non-GH comment must omit the field: {js}");
}

#[test]
fn test_github_comment_id_roundtrips() {
    let mut c = Comment::default();
    c.github_comment_id = 12345;
    let js = serde_json::to_string(&c).unwrap();
    assert!(js.contains(r#""github_comment_id":12345"#), "{js}");
    let back: Comment = serde_json::from_str(&js).unwrap();
    assert_eq!(back.github_comment_id, 12345);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p tally --lib store::todos::tests::test_todo_with_github_link_roundtrips store::comments::tests::test_github_comment_id_roundtrips 2>&1 | tail -20`
Expected: FAIL — `GithubLink` / `github` / `github_comment_id` do not exist (compile error).

- [ ] **Step 3: Add `GithubLink` and the `Todo` field**

In `src/store/todos.rs`, after the `Lock` struct (around line 30), add:
```rust
/// Opt-in GitHub sync link for a single todo. Absent for unsynced todos.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GithubLink {
    #[serde(rename = "repo")]
    pub repo: String,
    #[serde(rename = "number")]
    pub number: i64,
    #[serde(rename = "last_pushed")]
    pub last_pushed: String,
    #[serde(rename = "last_comment_pull")]
    pub last_comment_pull: String,
    #[serde(rename = "paused")]
    pub paused: bool,
}
```
Add the field to `Todo` (after `updated_by`, before the closing brace at ~line 63):
```rust
    /// Opt-in GitHub sync link; None for unsynced todos so existing stores load
    /// unchanged AND unsynced todos serialize byte-identical to today.
    #[serde(rename = "github", default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GithubLink>,
```
Add `github: None,` to the `Todo { .. }` literal in `create_todo` (after `updated_by: self.actor.clone(),`).

- [ ] **Step 4: Add the `Comment` field**

In `src/store/comments.rs`, add the field to `Comment` (after `text`, ~line 36):
```rust
    /// GitHub echo-prevention. 0 = none. On a pulled comment, the GH comment id
    /// (never re-imported, never pushed back). On a pushed comment, the id of the
    /// GH comment it became. Absent (skipped) on non-GitHub comments.
    #[serde(rename = "github_comment_id", default, skip_serializing_if = "is_zero")]
    pub github_comment_id: i64,
```
Add the helper near `norm_target` (~line 60):
```rust
fn is_zero(n: &i64) -> bool {
    *n == 0
}
```
Add `github_comment_id: 0,` to the `Comment { .. }` literal in `add_comment_kind`, and to every `Comment { .. }` literal in `mod tests` that the compiler flags (the seeded-comment tests around lines 442, 451, 517, 540).

- [ ] **Step 5: Re-export `GithubLink`**

In `src/store/mod.rs`, extend the todos re-export:
```rust
pub use todos::{GithubLink, Todo, TodoFilter, TodoUpdate};
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p tally --lib store:: 2>&1 | tail -20`
Expected: PASS, including the pre-existing `test_reads_go_written_todos_json` (Go fixture has no `github` key → `github` stays `None`) and all comment tests.

- [ ] **Step 7: Commit**

```bash
git add src/store/todos.rs src/store/comments.rs src/store/mod.rs
git commit -m "feat(store): add opt-in github link to todos and echo id to comments"
```

---

### Task 2: Origin repo resolution

**Files:**
- Create: `src/store/sync.rs` (module skeleton + `parse_repo`)
- Modify: `src/store/mod.rs` (add `mod sync;`)
- Modify: `src/store/project.rs` (add `Project::origin_repo`)

**Interfaces:**
- Produces:
  ```rust
  // sync.rs
  pub(crate) fn parse_repo(url: &str) -> Option<String>;   // git url -> "owner/name"
  // project.rs
  impl Project { pub(crate) fn origin_repo(&self) -> Option<String>; }
  ```

- [ ] **Step 1: Write the failing test**

Create `src/store/sync.rs` with only the test module for now:
```rust
//! GitHub issue sync — decision engine, executor, and the `gh` boundary.
//! All logic lives here (store is the single source of truth). CLI/MCP/TUI drive.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_repo_forms() {
        assert_eq!(parse_repo("git@github.com:owner/name.git").as_deref(), Some("owner/name"));
        assert_eq!(parse_repo("https://github.com/owner/name.git").as_deref(), Some("owner/name"));
        assert_eq!(parse_repo("https://github.com/owner/name").as_deref(), Some("owner/name"));
        assert_eq!(parse_repo("ssh://git@github.com/owner/name.git").as_deref(), Some("owner/name"));
        assert_eq!(parse_repo("  https://github.com/owner/name.git\n").as_deref(), Some("owner/name"));
        assert_eq!(parse_repo("not-a-url"), None);
        assert_eq!(parse_repo(""), None);
    }
}
```

- [ ] **Step 2: Wire the module and run to verify it fails**

In `src/store/mod.rs`, add after `mod scratchpads;`:
```rust
mod sync;
```
Run: `cargo test -p tally --lib store::sync::tests::test_parse_repo_forms 2>&1 | tail -20`
Expected: FAIL — `parse_repo` not found (compile error).

- [ ] **Step 3: Implement `parse_repo`**

At the top of `src/store/sync.rs` (above the test module):
```rust
/// Parse a git remote URL to "owner/name". Handles scp-style (`git@host:o/n.git`),
/// ssh (`ssh://git@host/o/n.git`), and https (`https://host/o/n[.git]`). None on
/// anything that doesn't yield two path segments.
pub(crate) fn parse_repo(url: &str) -> Option<String> {
    let u = url.trim();
    let path = if let Some(rest) = u.strip_prefix("git@") {
        // git@github.com:owner/name.git
        rest.split_once(':').map(|(_, p)| p)?
    } else if let Some((_scheme, rest)) = u.split_once("://") {
        // https://github.com/owner/name  |  ssh://git@github.com/owner/name.git
        rest.split_once('/').map(|(_, p)| p)?
    } else {
        return None;
    };
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut segs = path.trim_matches('/').split('/');
    let owner = segs.next().filter(|s| !s.is_empty())?;
    let name = segs.next().filter(|s| !s.is_empty())?;
    Some(format!("{owner}/{name}"))
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p tally --lib store::sync::tests::test_parse_repo_forms 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Add `Project::origin_repo` with a test**

In `src/store/project.rs`, inside `impl Project` (after `comments_path`), add:
```rust
    /// The GitHub "owner/name" from this project's `origin` remote, or None
    /// (no remote / not parseable). Uses the same `git` helper as project root.
    pub(crate) fn origin_repo(&self) -> Option<String> {
        let url = git(&self.path, &["remote", "get-url", "origin"])?;
        super::sync::parse_repo(&url)
    }
```
Add to `src/store/project.rs` `mod tests`:
```rust
    #[test]
    fn test_origin_repo_reads_remote() {
        if !git_available() {
            eprintln!("skipping: git not on PATH");
            return;
        }
        let root = TempDir::new();
        let dir = TempDir::new();
        init_repo(dir.path());
        let out = Command::new("git")
            .arg("-C").arg(dir.path())
            .args(["remote", "add", "origin", "git@github.com:owner/name.git"])
            .output().unwrap();
        assert!(out.status.success());
        let p = resolve_project_in(root.path(), Some(&dir.path().to_string_lossy())).unwrap();
        assert_eq!(p.origin_repo().as_deref(), Some("owner/name"));
    }

    #[test]
    fn test_origin_repo_none_without_remote() {
        if !git_available() {
            eprintln!("skipping: git not on PATH");
            return;
        }
        let root = TempDir::new();
        let dir = TempDir::new();
        init_repo(dir.path());
        let p = resolve_project_in(root.path(), Some(&dir.path().to_string_lossy())).unwrap();
        assert_eq!(p.origin_repo(), None);
    }
```

- [ ] **Step 6: Run to verify it passes**

Run: `cargo test -p tally --lib store::project::tests::test_origin_repo 2>&1 | tail -20`
Expected: PASS (both).

- [ ] **Step 7: Commit**

```bash
git add src/store/sync.rs src/store/mod.rs src/store/project.rs
git commit -m "feat(store): resolve github owner/name from origin remote"
```

---

### Task 3: `set_github` opt-in toggle store method

**Files:**
- Modify: `src/store/todos.rs` (add `set_github`)
- Test: inline in `src/store/todos.rs`

**Interfaces:**
- Consumes: `GithubLink` (Task 1), `Project::origin_repo` (Task 2), `edit_todo_raw`.
- Produces: `impl Project { pub fn set_github(&self, id: &str, on: bool) -> Result<Todo>; }`
  - `on=true`, no link, origin present → create `GithubLink { repo, number: 0, paused: false, .. }`.
  - `on=true`, no link, no origin → `Err(Error::Other("no git origin remote; cannot link to GitHub"))`.
  - `on=true`, existing link → `paused = false` (re-tick; reuses same repo/number, no duplicate issue).
  - `on=false`, existing link → `paused = true` (keeps repo/number).
  - `on=false`, no link → no-op, returns the todo unchanged.

- [ ] **Step 1: Write the failing test**

In `src/store/todos.rs` `mod tests`:
```rust
#[test]
fn test_set_github_toggle() {
    // new_project() creates a git repo but no origin remote.
    let p = new_project();
    let td = p.create_todo("sync me", "", "", Vec::new()).unwrap();

    // No origin remote yet -> linking fails, leaves the todo unlinked.
    assert!(p.set_github(&td.id, true).is_err());
    assert!(p.get_todo(&td.id).unwrap().github.is_none());

    // Add an origin, then link.
    let out = std::process::Command::new("git")
        .arg("-C").arg(&p.path)
        .args(["remote", "add", "origin", "git@github.com:owner/name.git"])
        .output().unwrap();
    assert!(out.status.success());

    let linked = p.set_github(&td.id, true).unwrap();
    let link = linked.github.unwrap();
    assert_eq!(link.repo, "owner/name");
    assert_eq!(link.number, 0);
    assert!(!link.paused);

    // Untick pauses but keeps repo/number.
    let paused = p.set_github(&td.id, false).unwrap();
    let link = paused.github.unwrap();
    assert!(link.paused);
    assert_eq!(link.repo, "owner/name");

    // Re-tick clears paused, same link (no new issue requested).
    let retick = p.set_github(&td.id, true).unwrap();
    assert!(!retick.github.unwrap().paused);
}
```
Note: `new_project()` derefs to `Project`, and `Project.path` is the repo working tree, so `git -C p.path remote add` targets the right repo.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p tally --lib store::todos::tests::test_set_github_toggle 2>&1 | tail -20`
Expected: FAIL — `set_github` not found.

- [ ] **Step 3: Implement `set_github`**

In `src/store/todos.rs`, inside `impl Project` (after `update_todo`), add:
```rust
    /// The opt-in toggle behind the box-tick. `on` links (or un-pauses) the todo;
    /// `off` pauses without dropping repo/number so re-ticking relinks the same
    /// issue. Resolving the origin is only required when creating a fresh link.
    pub fn set_github(&self, id: &str, on: bool) -> Result<Todo> {
        // Resolve origin up front (edit closure can't re-borrow self). Only the
        // fresh-link branch consumes it; re-tick/off ignore it.
        let origin = if on { self.origin_repo() } else { None };
        self.edit_todo_raw(id, |t| {
            match (&mut t.github, on) {
                (Some(link), true) => link.paused = false,
                (Some(link), false) => link.paused = true,
                (None, true) => {
                    let repo = origin.clone().ok_or_else(|| {
                        Error::Other("no git origin remote; cannot link to GitHub".to_string())
                    })?;
                    t.github = Some(GithubLink {
                        repo,
                        number: 0,
                        last_pushed: String::new(),
                        last_comment_pull: String::new(),
                        paused: false,
                    });
                }
                (None, false) => {} // no-op: nothing to unlink
            }
            Ok(())
        })
    }
```
(`GithubLink` lives in this same module, so name it directly — no `super::todos::` prefix.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p tally --lib store::todos::tests::test_set_github_toggle 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/store/todos.rs
git commit -m "feat(store): set_github opt-in toggle (link/pause/re-tick)"
```

---

### Task 4: Sync writeback store methods

Internal (`pub(crate)`) methods `sync_project` uses to persist results without going through the user-edit path.

**Files:**
- Modify: `src/store/todos.rs` (`update_github_link`)
- Modify: `src/store/comments.rs` (`import_github_comment`, `set_comment_github_id`)
- Test: inline in both files

**Interfaces:**
- Produces:
  ```rust
  // todos.rs
  impl Project { pub(crate) fn update_github_link(&self, id: &str, link: GithubLink) -> Result<()>; }
  // comments.rs
  impl Project {
      pub(crate) fn import_github_comment(&self, target: &str, author: &str, created: &str, gh_id: i64, text: &str) -> Result<Comment>;
      pub(crate) fn set_comment_github_id(&self, comment_id: &str, gh_id: i64) -> Result<()>;
  }
  ```
  `update_github_link` deliberately bypasses `edit_todo_raw` — it must NOT bump `updated`/`updated_by` or log an event, or sync's own writeback would look like a user edit and re-trigger a push next tick. It also **merges**: it writes `number`/`last_pushed`/`last_comment_pull` but preserves the currently-stored `paused`, so a user un-ticking (via CLI/MCP/TUI) mid-pass is not silently re-enabled by the end-of-pass writeback. (Todos have no revision guard; flock is per-mutation, so this read-preserve is the only thing keeping the un-tick contract.)

- [ ] **Step 1: Write the failing tests**

In `src/store/todos.rs` `mod tests`:
```rust
#[test]
fn test_update_github_link_does_not_bump_updated() {
    let p = new_project();
    let td = p.create_todo("x", "", "", Vec::new()).unwrap();
    let before = td.updated.clone();
    p.update_github_link(&td.id, GithubLink {
        repo: "o/n".into(), number: 7, last_pushed: "t".into(),
        last_comment_pull: String::new(), paused: false,
    }).unwrap();
    let got = p.get_todo(&td.id).unwrap();
    assert_eq!(got.github.unwrap().number, 7);
    assert_eq!(got.updated, before, "sync writeback must not bump updated");
}

#[test]
fn test_update_github_link_preserves_paused() {
    // A concurrent un-tick (paused=true) must survive sync's own end-of-pass
    // writeback, which carries paused=false from the pre-pass link clone.
    let p = new_project();
    let out = std::process::Command::new("git")
        .arg("-C").arg(&p.path)
        .args(["remote", "add", "origin", "git@github.com:o/n.git"])
        .output().unwrap();
    assert!(out.status.success());
    let td = p.create_todo("x", "", "", Vec::new()).unwrap();
    p.set_github(&td.id, true).unwrap();   // link, paused=false
    p.set_github(&td.id, false).unwrap();  // user un-ticks -> paused=true stored
    // Sync writeback arrives with a stale paused=false clone.
    p.update_github_link(&td.id, GithubLink {
        repo: "o/n".into(), number: 7, last_pushed: "t".into(),
        last_comment_pull: String::new(), paused: false,
    }).unwrap();
    let link = p.get_todo(&td.id).unwrap().github.unwrap();
    assert_eq!(link.number, 7, "sync fields still applied");
    assert!(link.paused, "concurrent un-tick must be preserved");
}
```

In `src/store/comments.rs` `mod tests`:
```rust
#[test]
fn test_import_and_set_github_comment_id() {
    let tp = new_project();
    let c = tp.import_github_comment("t_x", "gh:octocat", "2026-07-12T00:00:00Z", 999, "hi from GH").unwrap();
    assert_eq!(c.author, "gh:octocat");
    assert_eq!(c.github_comment_id, 999);
    assert_eq!(c.kind, "note");
    assert_eq!(c.created, "2026-07-12T00:00:00Z");
    let listed = tp.list_comments("t_x").unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].github_comment_id, 999);

    let local = tp.add_comment("t_x", "", "local note").unwrap();
    tp.set_comment_github_id(&local.id, 12345).unwrap();
    let found = tp.list_comments("t_x").unwrap().into_iter().find(|x| x.id == local.id).unwrap();
    assert_eq!(found.github_comment_id, 12345);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p tally --lib test_update_github_link_does_not_bump_updated test_update_github_link_preserves_paused test_import_and_set_github_comment_id 2>&1 | tail -20`
Expected: FAIL — methods not found.

- [ ] **Step 3: Implement `update_github_link`**

In `src/store/todos.rs`, inside `impl Project` (after `set_github`):
```rust
    /// Persist a link's fields (number/timestamps) WITHOUT touching updated/
    /// updated_by or logging an event — this is sync's own writeback, not a user
    /// edit, so it must not re-trigger the `updated > last_pushed` push rule.
    pub(crate) fn update_github_link(&self, id: &str, mut link: GithubLink) -> Result<()> {
        self.mutate_todos(|tf| {
            let t = tf.find_mut(id).ok_or(Error::NotFound)?;
            // Merge, don't clobber: a concurrent un-tick set paused=true after this
            // pass cloned the link, so preserve the stored paused rather than the
            // (stale) paused carried in `link`.
            if let Some(existing) = &t.github {
                link.paused = existing.paused;
            }
            t.github = Some(link);
            Ok(())
        })
    }
```

- [ ] **Step 4: Implement the comment methods**

In `src/store/comments.rs`, inside `impl Project` (after `add_comment_event`):
```rust
    /// Insert a comment pulled from GitHub: preserves the GH author (`gh:<login>`),
    /// the GH creation timestamp, and the GH comment id (echo prevention).
    pub(crate) fn import_github_comment(
        &self, target: &str, author: &str, created: &str, gh_id: i64, text: &str,
    ) -> Result<Comment> {
        let c = Comment {
            id: new_id("c_"),
            target: norm_target(target).to_string(),
            section: String::new(),
            author: author.to_string(),
            created: created.to_string(),
            kind: "note".to_string(),
            text: text.to_string(),
            github_comment_id: gh_id,
        };
        let cp = c.clone();
        self.mutate_comments(|cf| {
            cf.comments.push(cp);
            Ok(())
        })?;
        Ok(c)
    }

    /// Stamp the GH comment id onto a local comment we just pushed (echo prevention).
    pub(crate) fn set_comment_github_id(&self, comment_id: &str, gh_id: i64) -> Result<()> {
        self.mutate_comments(|cf| {
            let c = cf.comments.iter_mut().find(|c| c.id == comment_id).ok_or(Error::NotFound)?;
            c.github_comment_id = gh_id;
            Ok(())
        })
    }
```

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p tally --lib test_update_github_link_does_not_bump_updated test_update_github_link_preserves_paused test_import_and_set_github_comment_id 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/store/todos.rs src/store/comments.rs
git commit -m "feat(store): sync writeback helpers (link, import/id comments)"
```

---

### Task 5: `plan_actions` — the pure reconcile decision function

The heart of the engine, with no I/O so it's exhaustively unit-testable.

**Files:**
- Modify: `src/store/sync.rs` (types + `plan_actions` + tests)

**Interfaces:**
- Consumes: `Todo`, `GithubLink`, `Comment` from the store.
- Produces:
  ```rust
  #[derive(Debug, Clone, Default, PartialEq)]
  pub enum IssueState { #[default] Open, Closed }

  #[derive(Debug, Clone, PartialEq)]
  pub struct GhComment { pub id: i64, pub author: String, pub created: String, pub body: String }

  #[derive(Debug, Clone, Default)]
  pub struct IssueSnapshot { pub state: IssueState, pub closed_by: String, pub comments: Vec<GhComment> }

  #[derive(Debug, Clone, PartialEq)]
  pub(crate) enum Action {
      EditIssue,                          // push title+body
      CloseIssue,
      ReopenIssue,
      CompleteTodo { by: String },        // pull close; `by` = raw GH login ("" if unknown)
      ReopenTodo { by: String },          // pull reopen
      ImportComment(GhComment),
      PushComment { comment_id: String },
  }

  pub(crate) fn plan_actions(
      todo: &Todo, link: &GithubLink, todo_comments: &[Comment], snap: &IssueSnapshot,
  ) -> Vec<Action>;
  ```
  Covers the `number != 0` case only. Create (`number == 0`) is handled in the executor before any snapshot exists (Task 6).

- [ ] **Step 1: Write the failing tests**

In `src/store/sync.rs` `mod tests`, add imports and tests:
```rust
    use crate::store::{Comment, GithubLink, Todo};

    fn linked(number: i64, last_pushed: &str, last_comment_pull: &str) -> GithubLink {
        GithubLink {
            repo: "o/n".into(), number,
            last_pushed: last_pushed.into(),
            last_comment_pull: last_comment_pull.into(),
            paused: false,
        }
    }
    fn todo_at(status: &str, updated: &str) -> Todo {
        let mut t = Todo::default();
        t.id = "t_1".into();
        t.title = "T".into();
        t.status = status.into();
        t.updated = updated.into();
        t
    }
    fn note(id: &str, author: &str, gh_id: i64) -> Comment {
        let mut c = Comment::default();
        c.id = id.into();
        c.author = author.into();
        c.kind = "note".into();
        c.github_comment_id = gh_id;
        c.text = "body".into();
        c
    }

    #[test]
    fn test_plan_push_title_body_when_tally_newer() {
        let t = todo_at("open", "2026-07-12T02:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "");
        let snap = IssueSnapshot::default(); // Open, no comments
        let acts = plan_actions(&t, &link, &[], &snap);
        assert_eq!(acts, vec![Action::EditIssue]);
    }

    #[test]
    fn test_plan_push_close_when_completed_and_issue_open() {
        let t = todo_at("completed", "2026-07-12T02:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "");
        let snap = IssueSnapshot::default();
        let acts = plan_actions(&t, &link, &[], &snap);
        assert_eq!(acts, vec![Action::EditIssue, Action::CloseIssue]);
    }

    #[test]
    fn test_plan_pull_close_when_gh_wins() {
        // tally NOT newer than last_pushed -> GH state wins.
        let t = todo_at("open", "2026-07-12T01:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", ""); // updated == last_pushed => not newer
        let snap = IssueSnapshot { state: IssueState::Closed, closed_by: "octocat".into(), comments: vec![] };
        let acts = plan_actions(&t, &link, &[], &snap);
        assert_eq!(acts, vec![Action::CompleteTodo { by: "octocat".into() }]);
    }

    #[test]
    fn test_plan_import_new_comment_but_not_echo() {
        let t = todo_at("open", "2026-07-12T01:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "2026-07-12T00:00:00Z");
        // gc 100 is new; gc 200 is one we already pushed (present as a local id) -> skip.
        let snap = IssueSnapshot {
            state: IssueState::Open, closed_by: String::new(),
            comments: vec![
                GhComment { id: 100, author: "octocat".into(), created: "2026-07-12T01:30:00Z".into(), body: "new".into() },
                GhComment { id: 200, author: "me".into(),      created: "2026-07-12T01:31:00Z".into(), body: "echo".into() },
            ],
        };
        let local = vec![note("c_local", "you", 200)]; // github_comment_id 200 => already known
        let acts = plan_actions(&t, &link, &local, &snap);
        assert_eq!(acts, vec![
            Action::ImportComment(snap.comments[0].clone()),
        ]);
    }

    #[test]
    fn test_plan_push_local_note_but_not_pulled_or_event() {
        let t = todo_at("open", "2026-07-12T01:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "2026-07-12T00:00:00Z");
        let snap = IssueSnapshot::default();
        let mut event = note("c_ev", "you", 0);
        event.kind = "event".into(); // auto-logged status event: must NOT push
        let local = vec![
            note("c_push", "you", 0),        // local note, never on GH -> push
            note("c_pulled", "gh:octocat", 100), // pulled from GH -> never push back
            event,
        ];
        let acts = plan_actions(&t, &link, &local, &snap);
        assert_eq!(acts, vec![Action::PushComment { comment_id: "c_push".into() }]);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p tally --lib store::sync::tests::test_plan 2>&1 | tail -20`
Expected: FAIL — types/`plan_actions` not found.

- [ ] **Step 3: Implement the types and `plan_actions`**

In `src/store/sync.rs`, above the test module (below `parse_repo`), add:
```rust
use std::collections::HashSet;

use super::comments::Comment;
use super::todos::{GithubLink, Todo};

#[derive(Debug, Clone, Default, PartialEq)]
pub enum IssueState {
    #[default]
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GhComment {
    pub id: i64,
    pub author: String,
    pub created: String,
    pub body: String,
}

/// One tick's view of a GitHub issue.
#[derive(Debug, Clone, Default)]
pub struct IssueSnapshot {
    pub state: IssueState,
    pub closed_by: String, // GH login of the closer, "" if open/unknown
    pub comments: Vec<GhComment>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    EditIssue,
    CloseIssue,
    ReopenIssue,
    CompleteTodo { by: String },
    ReopenTodo { by: String },
    ImportComment(GhComment),
    PushComment { comment_id: String },
}

/// Pure reconcile decision for an already-created issue (number != 0). Ordering
/// mirrors the spec: state first (tally wins if it changed since last push, else
/// GH wins), then pull comments, then push comments.
pub(crate) fn plan_actions(
    todo: &Todo,
    link: &GithubLink,
    todo_comments: &[Comment],
    snap: &IssueSnapshot,
) -> Vec<Action> {
    let mut acts = Vec::new();
    let completed = todo.status == "completed";
    let issue_closed = snap.state == IssueState::Closed;
    // Empty last_pushed sorts before any RFC3339 stamp, so a never-pushed todo is
    // always "newer" here — exactly the create-then-first-edit path.
    let tally_newer = todo.updated.as_str() > link.last_pushed.as_str();

    if tally_newer {
        acts.push(Action::EditIssue);
        if completed && !issue_closed {
            acts.push(Action::CloseIssue);
        } else if !completed && issue_closed {
            acts.push(Action::ReopenIssue);
        }
    } else if issue_closed && !completed {
        acts.push(Action::CompleteTodo { by: snap.closed_by.clone() });
    } else if !issue_closed && completed {
        acts.push(Action::ReopenTodo { by: snap.closed_by.clone() });
    }

    // Pull: GH comments at/after last_comment_pull whose id we don't already hold.
    // The id set is the real echo/dup guard (imported comments carry their GH id,
    // and comments we pushed carry theirs), so an inclusive time bound is safe.
    let known: HashSet<i64> = todo_comments
        .iter()
        .map(|c| c.github_comment_id)
        .filter(|&i| i != 0)
        .collect();
    for gc in &snap.comments {
        if gc.created.as_str() >= link.last_comment_pull.as_str() && !known.contains(&gc.id) {
            acts.push(Action::ImportComment(gc.clone()));
        }
    }

    // Push: local human/agent notes never sent to GH. Skip events (auto-logged)
    // and anything pulled from GH (author `gh:*`).
    for c in todo_comments {
        if c.kind == "note" && c.github_comment_id == 0 && !c.author.starts_with("gh:") {
            acts.push(Action::PushComment { comment_id: c.id.clone() });
        }
    }
    acts
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p tally --lib store::sync::tests 2>&1 | tail -20`
Expected: PASS (parse_repo + all plan_* tests).

- [ ] **Step 5: Commit**

```bash
git add src/store/sync.rs
git commit -m "feat(sync): pure plan_actions reconcile decision + tests"
```

---

### Task 6: `sync_project` executor + `Gh` trait + `SyncReport`

Wires `plan_actions` to the `Gh` boundary and the store, per synced todo, best-effort.

**Files:**
- Modify: `src/store/sync.rs` (`Gh` trait, `SyncReport`, `sync_project`, `sync_one`, helpers, `FakeGh` test + tests)
- Modify: `src/store/mod.rs` (re-export `Gh`, `SyncReport`, `sync_project`)

**Interfaces:**
- Consumes: `plan_actions`, `IssueSnapshot`, `GhComment`, `Action`, all store methods from Tasks 3–4.
- Produces:
  ```rust
  pub trait Gh {
      fn auth_ok(&self) -> bool;
      fn create_issue(&self, repo: &str, title: &str, body: &str) -> Result<i64>;
      fn edit_issue(&self, repo: &str, number: i64, title: &str, body: &str) -> Result<()>;
      fn close_issue(&self, repo: &str, number: i64) -> Result<()>;
      fn reopen_issue(&self, repo: &str, number: i64) -> Result<()>;
      fn view_issue(&self, repo: &str, number: i64) -> Result<IssueSnapshot>;
      fn create_comment(&self, repo: &str, number: i64, body: &str) -> Result<i64>;
  }

  #[derive(Debug, Default, serde::Serialize)]
  pub struct SyncReport {
      pub gh_available: bool,
      pub checked: usize,
      pub created: usize,
      pub pushed: usize,           // title/body pushes
      pub state_changes: usize,    // close/reopen either direction
      pub pulled_comments: usize,
      pub pushed_comments: usize,
      pub errors: Vec<String>,     // "t_xxx: <error>"
  }

  pub fn sync_project(p: &mut Project, gh: &dyn Gh) -> SyncReport;
  ```

- [ ] **Step 1: Write the failing test (with a fake `Gh`)**

In `src/store/sync.rs` `mod tests`, add:
```rust
    use std::cell::RefCell;
    use crate::store::testutil::new_project;

    /// A scripted GH boundary. Records mutating calls; serves one snapshot.
    struct FakeGh {
        snapshot: IssueSnapshot,
        next_issue: i64,
        next_comment: i64,
        edits: RefCell<Vec<String>>,
    }
    impl FakeGh {
        fn new(snapshot: IssueSnapshot) -> Self {
            FakeGh { snapshot, next_issue: 7, next_comment: 500, edits: RefCell::new(vec![]) }
        }
    }
    impl Gh for FakeGh {
        fn auth_ok(&self) -> bool { true }
        fn create_issue(&self, _r: &str, _t: &str, _b: &str) -> crate::store::Result<i64> {
            self.edits.borrow_mut().push("create".into());
            Ok(self.next_issue)
        }
        fn edit_issue(&self, _r: &str, n: i64, _t: &str, _b: &str) -> crate::store::Result<()> {
            self.edits.borrow_mut().push(format!("edit {n}")); Ok(())
        }
        fn close_issue(&self, _r: &str, n: i64) -> crate::store::Result<()> {
            self.edits.borrow_mut().push(format!("close {n}")); Ok(())
        }
        fn reopen_issue(&self, _r: &str, n: i64) -> crate::store::Result<()> {
            self.edits.borrow_mut().push(format!("reopen {n}")); Ok(())
        }
        fn view_issue(&self, _r: &str, _n: i64) -> crate::store::Result<IssueSnapshot> {
            Ok(self.snapshot.clone())
        }
        fn create_comment(&self, _r: &str, _n: i64, _b: &str) -> crate::store::Result<i64> {
            self.edits.borrow_mut().push("comment".into());
            Ok(self.next_comment)
        }
    }

    fn link_todo(p: &crate::store::Project) -> String {
        // origin so set_github can create a link
        let out = std::process::Command::new("git")
            .arg("-C").arg(&p.path)
            .args(["remote", "add", "origin", "git@github.com:o/n.git"])
            .output().unwrap();
        assert!(out.status.success());
        let td = p.create_todo("issue", "body", "", Vec::new()).unwrap();
        p.set_github(&td.id, true).unwrap();
        td.id
    }

    #[test]
    fn test_sync_creates_issue_on_first_pass() {
        let mut tp = new_project();
        let id = link_todo(&tp.p);
        let rep = sync_project(&mut tp.p, &FakeGh::new(IssueSnapshot::default()));
        assert!(rep.gh_available);
        assert_eq!(rep.created, 1);
        let link = tp.get_todo(&id).unwrap().github.unwrap();
        assert_eq!(link.number, 7);
        assert!(!link.last_pushed.is_empty());
    }

    #[test]
    fn test_sync_gh_unavailable_is_soft() {
        struct DeadGh;
        impl Gh for DeadGh {
            fn auth_ok(&self) -> bool { false }
            fn create_issue(&self, _:&str,_:&str,_:&str)->crate::store::Result<i64>{unreachable!()}
            fn edit_issue(&self,_:&str,_:i64,_:&str,_:&str)->crate::store::Result<()>{unreachable!()}
            fn close_issue(&self,_:&str,_:i64)->crate::store::Result<()>{unreachable!()}
            fn reopen_issue(&self,_:&str,_:i64)->crate::store::Result<()>{unreachable!()}
            fn view_issue(&self,_:&str,_:i64)->crate::store::Result<IssueSnapshot>{unreachable!()}
            fn create_comment(&self,_:&str,_:i64,_:&str)->crate::store::Result<i64>{unreachable!()}
        }
        // Needs an ACTIVE linked todo — sync_project only reaches auth_ok() when
        // there is work to do (perf gate). With no link it returns clean/quiet.
        let mut tp = new_project();
        link_todo(&tp.p);
        let rep = sync_project(&mut tp.p, &DeadGh);
        assert!(!rep.gh_available);
        assert_eq!(rep.checked, 0);
        assert_eq!(rep.errors.len(), 1);
    }

    #[test]
    fn test_sync_no_links_is_quiet_and_skips_gh() {
        // No synced todos: never call gh, report is empty/quiet (not an error).
        struct PanicGh;
        impl Gh for PanicGh {
            fn auth_ok(&self) -> bool { panic!("must not check auth with no links") }
            fn create_issue(&self,_:&str,_:&str,_:&str)->crate::store::Result<i64>{unreachable!()}
            fn edit_issue(&self,_:&str,_:i64,_:&str,_:&str)->crate::store::Result<()>{unreachable!()}
            fn close_issue(&self,_:&str,_:i64)->crate::store::Result<()>{unreachable!()}
            fn reopen_issue(&self,_:&str,_:i64)->crate::store::Result<()>{unreachable!()}
            fn view_issue(&self,_:&str,_:i64)->crate::store::Result<IssueSnapshot>{unreachable!()}
            fn create_comment(&self,_:&str,_:i64,_:&str)->crate::store::Result<i64>{unreachable!()}
        }
        let mut tp = new_project();
        tp.p.create_todo("unlinked", "", "", Vec::new()).unwrap();
        let rep = sync_project(&mut tp.p, &PanicGh);
        assert!(!rep.gh_available);
        assert_eq!(rep.checked, 0);
        assert!(rep.errors.is_empty());
    }

    #[test]
    fn test_sync_pull_close_completes_todo_with_attribution() {
        let mut tp = new_project();
        let id = link_todo(&tp.p);
        // First pass: create the issue (number 7, last_pushed=now).
        sync_project(&mut tp.p, &FakeGh::new(IssueSnapshot::default()));
        // Second pass: issue is Closed on GH, todo still open, tally not newer.
        let snap = IssueSnapshot { state: IssueState::Closed, closed_by: "octocat".into(), comments: vec![] };
        let rep = sync_project(&mut tp.p, &FakeGh::new(snap));
        assert_eq!(rep.state_changes, 1);
        let td = tp.get_todo(&id).unwrap();
        assert_eq!(td.status, "completed");
        assert_eq!(td.updated_by, "gh:octocat");
    }

    #[test]
    fn test_sync_imports_and_pushes_comments() {
        let mut tp = new_project();
        let id = link_todo(&tp.p);
        sync_project(&mut tp.p, &FakeGh::new(IssueSnapshot::default())); // create
        // A local human note to push, and a GH comment to import.
        tp.p.add_comment(&id, "", "please look").unwrap();
        let snap = IssueSnapshot {
            state: IssueState::Open, closed_by: String::new(),
            comments: vec![GhComment { id: 100, author: "octocat".into(), created: "2026-07-12T09:00:00Z".into(), body: "on it".into() }],
        };
        let rep = sync_project(&mut tp.p, &FakeGh::new(snap));
        assert_eq!(rep.pulled_comments, 1);
        assert_eq!(rep.pushed_comments, 1);
        let comments = tp.list_comments(&id).unwrap();
        // pulled comment present with gh author + id; local note now carries id 500.
        assert!(comments.iter().any(|c| c.author == "gh:octocat" && c.github_comment_id == 100));
        assert!(comments.iter().any(|c| c.text == "please look" && c.github_comment_id == 500));
    }
```
Helper note: `new_project()` holds its own temp store + repo. All tests above call `sync_project(&mut tp.p, ..)` directly on that same project (mutable borrow); there is no `new_project_from` helper to build.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p tally --lib store::sync::tests::test_sync 2>&1 | tail -20`
Expected: FAIL — `Gh`, `SyncReport`, `sync_project` not found.

- [ ] **Step 3: Implement the executor**

In `src/store/sync.rs`, above the test module, add:
```rust
use super::errors::Result;
use super::todos::now;
use super::{Project, TodoFilter};

pub trait Gh {
    fn auth_ok(&self) -> bool;
    fn create_issue(&self, repo: &str, title: &str, body: &str) -> Result<i64>;
    fn edit_issue(&self, repo: &str, number: i64, title: &str, body: &str) -> Result<()>;
    fn close_issue(&self, repo: &str, number: i64) -> Result<()>;
    fn reopen_issue(&self, repo: &str, number: i64) -> Result<()>;
    fn view_issue(&self, repo: &str, number: i64) -> Result<IssueSnapshot>;
    fn create_comment(&self, repo: &str, number: i64, body: &str) -> Result<i64>;
}

#[derive(Debug, Default, serde::Serialize)]
pub struct SyncReport {
    pub gh_available: bool,
    pub checked: usize,
    pub created: usize,
    pub pushed: usize,
    pub state_changes: usize,
    pub pulled_comments: usize,
    pub pushed_comments: usize,
    pub errors: Vec<String>,
}

/// `gh:<login>`, or bare `gh` when the login is unknown.
fn gh_actor(login: &str) -> String {
    if login.is_empty() { "gh".to_string() } else { format!("gh:{login}") }
}

/// Run `f` with the project's actor temporarily set to `actor`, so a store
/// mutation is attributed to (e.g.) `gh:octocat` and restored afterward.
fn with_actor<T>(p: &mut Project, actor: &str, f: impl FnOnce(&Project) -> Result<T>) -> Result<T> {
    let saved = std::mem::replace(&mut p.actor, actor.to_string());
    let r = f(p);
    p.actor = saved;
    r
}

/// One reconcile pass over every synced, un-paused todo. Best-effort: a per-todo
/// gh/network failure is recorded and skipped; the next tick retries.
pub fn sync_project(p: &mut Project, gh: &dyn Gh) -> SyncReport {
    let mut rep = SyncReport::default();
    // List first, then gate on active links BEFORE shelling out to `gh auth
    // status`. Most tally users have no synced todos; they must not pay a `gh`
    // subprocess every 60s (TUI worker) / every nudge just to learn there's
    // nothing to do. gh_available stays false in that case → "nothing to sync".
    let todos = match p.list_todos(TodoFilter::default()) {
        Ok(t) => t,
        Err(e) => {
            rep.errors.push(format!("list todos: {e}"));
            return rep;
        }
    };
    let active: Vec<Todo> = todos
        .into_iter()
        .filter(|t| t.github.as_ref().is_some_and(|l| !l.paused))
        .collect();
    if active.is_empty() {
        return rep; // nothing linked; don't touch gh
    }
    if !gh.auth_ok() {
        rep.errors.push("gh unavailable or not authenticated".to_string());
        return rep;
    }
    rep.gh_available = true;
    for t in active {
        let link = t.github.clone().expect("filtered to Some above");
        rep.checked += 1;
        if let Err(e) = sync_one(p, gh, &t, link, &mut rep) {
            rep.errors.push(format!("{}: {e}", t.id));
        }
    }
    rep
}

fn sync_one(
    p: &mut Project,
    gh: &dyn Gh,
    todo: &Todo,
    mut link: GithubLink,
    rep: &mut SyncReport,
) -> Result<()> {
    // Capture the pass watermark BEFORE any gh read. A user edit or a fresh GH
    // comment that lands DURING this pass then has updated/created > pass_start,
    // so stamping pass_start (not a post-pass now()) never buries it — next tick's
    // `updated > last_pushed` / `created >= last_comment_pull` still fires. Using a
    // post-pass now() would silently skip anything that arrived mid-pass.
    let pass_start = now();

    // Create: no issue yet. One pass creates it; the next reconciles state/comments.
    if link.number == 0 {
        link.number = gh.create_issue(&link.repo, &todo.title, &todo.body)?;
        link.last_pushed = pass_start;
        p.update_github_link(&todo.id, link)?;
        rep.created += 1;
        return Ok(());
    }

    let snap = gh.view_issue(&link.repo, link.number)?;
    let comments = p.list_comments(&todo.id)?;
    let actions = plan_actions(todo, &link, &comments, &snap);

    let mut pushed_state = false;  // we pushed to GH (edit/close/reopen)
    let mut pulled_state = false;  // GH won: we bumped the todo (complete/reopen)
    for act in actions {
        match act {
            Action::EditIssue => {
                gh.edit_issue(&link.repo, link.number, &todo.title, &todo.body)?;
                pushed_state = true;
                rep.pushed += 1;
            }
            Action::CloseIssue => {
                gh.close_issue(&link.repo, link.number)?;
                pushed_state = true;
                rep.state_changes += 1;
            }
            Action::ReopenIssue => {
                gh.reopen_issue(&link.repo, link.number)?;
                pushed_state = true;
                rep.state_changes += 1;
            }
            Action::CompleteTodo { by } => {
                let actor = gh_actor(&by);
                let id = todo.id.clone();
                with_actor(p, &actor, |p| p.complete_todo(&id, false))?;
                pulled_state = true;
                rep.state_changes += 1;
            }
            Action::ReopenTodo { by } => {
                let actor = gh_actor(&by);
                let id = todo.id.clone();
                with_actor(p, &actor, |p| p.incomplete_todo(&id, false))?;
                pulled_state = true;
                rep.state_changes += 1;
            }
            Action::ImportComment(gc) => {
                p.import_github_comment(&todo.id, &gh_actor(&gc.author), &gc.created, gc.id, &gc.body)?;
                rep.pulled_comments += 1;
            }
            Action::PushComment { comment_id } => {
                if let Some(c) = comments.iter().find(|c| c.id == comment_id) {
                    let gid = gh.create_comment(&link.repo, link.number, &c.text)?;
                    p.set_comment_github_id(&comment_id, gid)?;
                    rep.pushed_comments += 1;
                }
            }
        }
    }

    // last_pushed policy (push and pull state changes are mutually exclusive —
    // plan_actions takes the tally-wins OR the GH-wins branch, never both):
    //  - pulled a close/reopen → complete_todo/incomplete_todo just bumped the
    //    todo's `updated` to the pull moment. Set last_pushed to that RE-READ
    //    value so the pull-bump doesn't read as a user edit next tick (a fresh
    //    now() would be < updated on a fast pass and re-trigger a spurious push).
    //  - pushed a state change → advance to pass_start (a mid-pass user edit has
    //    updated > pass_start, so it still pushes next tick).
    if pulled_state {
        link.last_pushed = p.get_todo(&todo.id)?.updated;
    } else if pushed_state {
        link.last_pushed = pass_start;
    }
    // We've now seen every GH comment created up to pass_start; the id-known set
    // guards echoes/dups, so an inclusive pass_start bound loses nothing.
    link.last_comment_pull = pass_start;
    p.update_github_link(&todo.id, link)?;
    Ok(())
}
```

- [ ] **Step 4: Re-export from the store**

In `src/store/mod.rs`, add:
```rust
pub use sync::{Gh, SyncReport, sync_project};
```

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p tally --lib store::sync::tests 2>&1 | tail -30`
Expected: PASS (all sync + plan + parse tests).

- [ ] **Step 6: Commit**

```bash
git add src/store/sync.rs src/store/mod.rs
git commit -m "feat(sync): sync_project executor over the Gh boundary + SyncReport"
```

---

### Task 7: `GhCli` — the real `gh` boundary

Shells out to `gh`, each call bounded by a 30s watchdog. Not unit-tested (I/O); a `#[ignore]` live smoke test documents the round trip.

**Files:**
- Modify: `src/store/sync.rs` (`GhCli` struct + `run` helper + JSON parsing + `#[ignore]` test)
- Modify: `src/store/mod.rs` (re-export `GhCli`)

**Interfaces:**
- Consumes: `Gh` trait (Task 6), `IssueSnapshot`, `GhComment`, `IssueState`.
- Produces: `pub struct GhCli;` implementing `Gh`.
- `gh` commands used (all with `--repo <repo>`):
  - auth: `gh auth status`
  - create: `gh issue create --repo R --title T --body B` → stdout is the issue URL; number = last path segment.
  - edit: `gh issue edit N --repo R --title T --body B`
  - close: `gh issue close N --repo R`
  - reopen: `gh issue reopen N --repo R`
  - view state: `gh issue view N --repo R --json state` → `{"state":"OPEN"|"CLOSED"}`
  - view comments: `gh api repos/R/issues/N/comments` → `[{"id","user":{"login"},"created_at","body"}]`
  - view closer (best-effort): `gh api repos/R/issues/N/events` → last `event=="closed"`'s `actor.login`
  - create comment: `gh api --method POST repos/R/issues/N/comments -f body=@-` (body on stdin) → `{"id":N}`

- [ ] **Step 1: Write the ignored smoke test + a `run`-timeout unit test**

In `src/store/sync.rs` `mod tests`:
```rust
    #[test]
    fn test_run_smoke_no_panic() {
        // `gh` may or may not be installed in CI; either a clean stdout or a spawn
        // error is fine. This only exercises the run() plumbing (spawn, watchdog
        // wiring, pipe drain) and asserts it neither panics nor hangs.
        let _ = super::run(&["--version"], None);
    }

    #[ignore = "live: needs gh auth + a scratch repo; run manually"]
    #[test]
    fn test_ghcli_live_roundtrip() {
        // Set TALLY_SCRATCH_REPO=owner/name to a throwaway repo you own.
        let repo = std::env::var("TALLY_SCRATCH_REPO").expect("set TALLY_SCRATCH_REPO");
        let gh = GhCli;
        assert!(gh.auth_ok(), "gh not authed");
        let n = gh.create_issue(&repo, "tally smoke", "body").unwrap();
        gh.edit_issue(&repo, n, "tally smoke edited", "body2").unwrap();
        let cid = gh.create_comment(&repo, n, "hello from tally").unwrap();
        assert!(cid > 0);
        let snap = gh.view_issue(&repo, n).unwrap();
        assert!(snap.comments.iter().any(|c| c.id == cid));
        gh.close_issue(&repo, n).unwrap();
        let snap = gh.view_issue(&repo, n).unwrap();
        assert_eq!(snap.state, IssueState::Closed);
    }
```

- [ ] **Step 2: Run to verify it fails to compile**

Run: `cargo test -p tally --lib store::sync::tests::test_run_smoke_no_panic 2>&1 | tail -20`
Expected: FAIL — `run` and `GhCli` not found.

- [ ] **Step 3: Implement `run` (subprocess + 30s watchdog) and `GhCli`**

In `src/store/sync.rs`, above the test module, add:
```rust
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

/// Run `gh <args>` with a 30s ceiling, optionally feeding `stdin`. Drains stdout
/// on the calling thread (so a large comment list can't deadlock the pipe) while
/// a watchdog thread SIGKILLs the child if it overruns.
// ponytail: 30s hard ceiling via libc::kill watchdog; a hung gh can't wedge the
// TUI's sync thread. Bump the constant if long-running gh calls ever appear.
fn run(args: &[&str], stdin: Option<&str>) -> Result<Vec<u8>> {
    let mut cmd = Command::new("gh");
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn()?;
    // Spawn the watchdog BEFORE writing stdin so the 30s ceiling also covers a
    // stuck write, and so every path below reaches wait_with_output (which reaps
    // the child). A `?` on the stdin write here would drop the child un-waited and
    // leak a zombie when gh dies early (e.g. BrokenPipe on bad auth before it reads
    // stdin).
    let pid = child.id() as libc::pid_t;
    let done = Arc::new(AtomicBool::new(false));
    let watch_done = done.clone();
    let watch = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !watch_done.load(Ordering::Relaxed) {
            if Instant::now() >= deadline {
                unsafe { libc::kill(pid, libc::SIGKILL) };
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    });
    if let (Some(s), Some(mut w)) = (stdin, child.stdin.take()) {
        // Best-effort: if gh already closed stdin (erroring out), ignore the broken
        // pipe — its exit status/stderr below is the real signal. drop(w) = EOF.
        let _ = w.write_all(s.as_bytes());
    }
    let out = child.wait_with_output()?;
    done.store(true, Ordering::Relaxed);
    let killed = watch.join().unwrap_or(false);
    if killed {
        return Err(Error::Other(format!("gh {args:?} timed out")));
    }
    if !out.status.success() {
        return Err(Error::Other(format!(
            "gh {args:?}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(out.stdout)
}

/// The real GitHub boundary: shells out to `gh`.
pub struct GhCli;

impl Gh for GhCli {
    fn auth_ok(&self) -> bool {
        run(&["auth", "status"], None).is_ok()
    }

    fn create_issue(&self, repo: &str, title: &str, body: &str) -> Result<i64> {
        let out = run(
            &["issue", "create", "--repo", repo, "--title", title, "--body", body],
            None,
        )?;
        let url = String::from_utf8_lossy(&out);
        // Last non-empty line is the issue URL: .../issues/<n>
        let n = url
            .split_whitespace()
            .rev()
            .find_map(|tok| tok.rsplit('/').next().and_then(|s| s.parse::<i64>().ok()))
            .ok_or_else(|| Error::Other(format!("could not parse issue number from: {url}")))?;
        Ok(n)
    }

    fn edit_issue(&self, repo: &str, number: i64, title: &str, body: &str) -> Result<()> {
        run(
            &[
                "issue", "edit", &number.to_string(), "--repo", repo,
                "--title", title, "--body", body,
            ],
            None,
        )?;
        Ok(())
    }

    fn close_issue(&self, repo: &str, number: i64) -> Result<()> {
        run(&["issue", "close", &number.to_string(), "--repo", repo], None)?;
        Ok(())
    }

    fn reopen_issue(&self, repo: &str, number: i64) -> Result<()> {
        run(&["issue", "reopen", &number.to_string(), "--repo", repo], None)?;
        Ok(())
    }

    fn view_issue(&self, repo: &str, number: i64) -> Result<IssueSnapshot> {
        let state_out = run(
            &["issue", "view", &number.to_string(), "--repo", repo, "--json", "state"],
            None,
        )?;
        let sv: Value = serde_json::from_slice(&state_out)?;
        let state = match sv.get("state").and_then(Value::as_str) {
            Some("CLOSED") => IssueState::Closed,
            _ => IssueState::Open,
        };

        let cpath = format!("repos/{repo}/issues/{number}/comments");
        let comments_out = run(&["api", &cpath], None)?;
        let cv: Value = serde_json::from_slice(&comments_out)?;
        let comments = cv
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        Some(GhComment {
                            id: c.get("id")?.as_i64()?,
                            author: c.get("user")?.get("login")?.as_str()?.to_string(),
                            created: c.get("created_at")?.as_str()?.to_string(),
                            body: c.get("body").and_then(Value::as_str).unwrap_or("").to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Closer login is best-effort; degrade to "" (→ bare `gh` attribution).
        let closed_by = if state == IssueState::Closed {
            closer_login(repo, number).unwrap_or_default()
        } else {
            String::new()
        };

        Ok(IssueSnapshot { state, closed_by, comments })
    }

    fn create_comment(&self, repo: &str, number: i64, body: &str) -> Result<i64> {
        let path = format!("repos/{repo}/issues/{number}/comments");
        // body on stdin via -f body=@- avoids arg-length/escaping issues.
        let out = run(&["api", "--method", "POST", &path, "-f", "body=@-"], Some(body))?;
        let v: Value = serde_json::from_slice(&out)?;
        v.get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| Error::Other("gh comment create returned no id".to_string()))
    }
}

/// Best-effort GH login of whoever last closed the issue (issue events API).
fn closer_login(repo: &str, number: i64) -> Option<String> {
    let path = format!("repos/{repo}/issues/{number}/events");
    let out = run(&["api", &path], None).ok()?;
    let v: Value = serde_json::from_slice(&out).ok()?;
    v.as_array()?
        .iter()
        .filter(|e| e.get("event").and_then(Value::as_str) == Some("closed"))
        .next_back()
        .and_then(|e| e.get("actor")?.get("login")?.as_str().map(str::to_string))
}
```
Add `use super::errors::Error;` to the existing `use super::errors::Result;` line (make it `use super::errors::{Error, Result};`).

- [ ] **Step 4: Re-export `GhCli`**

In `src/store/mod.rs`:
```rust
pub use sync::{Gh, GhCli, SyncReport, sync_project};
```

- [ ] **Step 5: Run tests + clippy**

Run: `cargo test -p tally --lib store::sync 2>&1 | tail -20 && cargo clippy 2>&1 | tail -15`
Expected: PASS (ignored live test skipped); no clippy errors.

- [ ] **Step 6: Commit**

```bash
git add src/store/sync.rs src/store/mod.rs
git commit -m "feat(sync): GhCli real boundary with 30s subprocess watchdog"
```

---

### Task 8: CLI `tally sync`

**Files:**
- Create: `src/cli/sync.rs`
- Modify: `src/cli/mod.rs` (add `mod sync;` + `pub fn sync(..)` entry)
- Modify: `src/main.rs` (dispatch `Some("sync")`; update usage string)
- Test: inline in `src/cli/sync.rs`

**Interfaces:**
- Consumes: `crate::store::{GhCli, sync_project, SyncReport, resolve_project*}`.
- Produces: `pub fn run(args: &[String], store_root: Option<&Path>, gh: &dyn Gh, out: &mut dyn Write) -> i32` and a thin `crate::cli::sync(args)` wrapper.
- Flags: `--project <path>`, `--json`.

- [ ] **Step 1: Write the failing test (with a fake Gh)**

Create `src/cli/sync.rs`:
```rust
//! `tally sync`: one reconcile pass, printing a SyncReport. Thin adapter over
//! store::sync_project — the CLI just picks the Gh boundary and formats output.
use std::io::Write;
use std::path::Path;

use super::{fail, print_json, project_opt, resolve};
use crate::store::{Gh, sync_project};

const BOOL_FLAGS: &[&str] = &["json"];
const VALUE_FLAGS: &[&str] = &["project"];

pub(crate) fn run(
    args: &[String],
    store_root: Option<&Path>,
    gh: &dyn Gh,
    out: &mut dyn Write,
) -> i32 {
    let p = match super::parse(args, BOOL_FLAGS, VALUE_FLAGS, &[]) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let project = p.str("project", "");
    let as_json = p.boolean("json", false);
    let mut proj = match resolve(project_opt(&project), store_root) {
        Ok(p) => p,
        Err(e) => return fail(&e.to_string()),
    };
    let rep = sync_project(&mut proj, gh);
    if as_json {
        let _ = print_json(out, &rep);
    } else if !rep.gh_available && rep.errors.is_empty() {
        // No active links: sync never touched gh. Don't cry "unavailable".
        let _ = writeln!(out, "nothing to sync (no linked todos)");
    } else if !rep.gh_available {
        let _ = writeln!(out, "sync skipped: gh unavailable or not authenticated");
        for e in &rep.errors {
            let _ = writeln!(out, "  ! {e}");
        }
    } else {
        let _ = writeln!(
            out,
            "synced {} todo(s): {} created, {} pushed, {} state change(s), {} comment(s) in, {} out{}",
            rep.checked, rep.created, rep.pushed, rep.state_changes,
            rep.pulled_comments, rep.pushed_comments,
            if rep.errors.is_empty() { String::new() } else { format!(", {} error(s)", rep.errors.len()) },
        );
        for e in &rep.errors {
            let _ = writeln!(out, "  ! {e}");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use crate::store::testutil::{TempDir, git_repo};
    use crate::store::{Gh, IssueSnapshot, Result};

    struct OkGh;
    impl Gh for OkGh {
        fn auth_ok(&self) -> bool { true }
        fn create_issue(&self, _:&str,_:&str,_:&str)->Result<i64>{ Ok(1) }
        fn edit_issue(&self,_:&str,_:i64,_:&str,_:&str)->Result<()>{ Ok(()) }
        fn close_issue(&self,_:&str,_:i64)->Result<()>{ Ok(()) }
        fn reopen_issue(&self,_:&str,_:i64)->Result<()>{ Ok(()) }
        fn view_issue(&self,_:&str,_:i64)->Result<IssueSnapshot>{ Ok(IssueSnapshot::default()) }
        fn create_comment(&self,_:&str,_:i64,_:&str)->Result<i64>{ Ok(1) }
    }

    #[test]
    fn sync_reports_json_when_no_synced_todos() {
        let root = TempDir::new();
        let repo = git_repo();
        let args = vec![
            "--project".to_string(), repo.path().to_string_lossy().into_owned(),
            "--json".to_string(),
        ];
        let mut buf = Vec::new();
        let code = super::run(&args, Some(root.path()), &OkGh, &mut buf);
        assert_eq!(code, 0);
        let out = String::from_utf8(buf).unwrap();
        // No linked todos → sync gates out before touching gh, so gh_available
        // stays false and nothing is checked. (OkGh's auth_ok is never called.)
        assert!(out.contains(r#""gh_available":false"#), "{out}");
        assert!(out.contains(r#""checked":0"#), "{out}");
    }

    #[test]
    fn sync_human_says_nothing_to_sync_when_no_links() {
        let root = TempDir::new();
        let repo = git_repo();
        let args = vec![
            "--project".to_string(), repo.path().to_string_lossy().into_owned(),
        ];
        let mut buf = Vec::new();
        let code = super::run(&args, Some(root.path()), &OkGh, &mut buf);
        assert_eq!(code, 0);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("nothing to sync"), "{out}");
    }
}
```
This test needs `IssueSnapshot` re-exported from the store — add it to `mod.rs` in Step 3.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p tally --lib cli::sync 2>&1 | tail -20`
Expected: FAIL — module not wired; `IssueSnapshot` not exported.

- [ ] **Step 3: Wire the module, exports, and dispatch**

In `src/store/mod.rs`, extend the sync re-export so adapters/tests can name the snapshot type:
```rust
pub use sync::{Gh, GhCli, IssueSnapshot, IssueState, SyncReport, sync_project};
```
In `src/cli/mod.rs`, add `mod sync;` (with the others) and an entry alongside `todos`/`comments`:
```rust
/// `tally sync …` entry: real store root, real gh boundary, stdout.
pub fn sync(args: &[String]) -> ExitCode {
    exit(sync::run(args, None, &crate::store::GhCli, &mut io::stdout()))
}
```
In `src/main.rs`, add the dispatch arm and update usage:
```rust
        Some("sync") => cli::sync(&args[1..]),
```
and change the usage line to:
```rust
            eprintln!("usage: tally <todos|scratchpads|comments|sync|mcp|tui> ...");
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p tally --lib cli::sync 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli/sync.rs src/cli/mod.rs src/main.rs src/store/mod.rs
git commit -m "feat(cli): tally sync runs one reconcile pass and prints a report"
```

---

### Task 9: Opt-in surface on `todos update` (CLI + MCP)

The box-tick for non-TUI callers. CLI and MCP mirror each other (invariant).

**Files:**
- Modify: `src/cli/todos.rs` (add `github` value flag; handle in `update`)
- Modify: `src/mcp/tools.rs` (add `github` to `Args` + `todo_update` schema + run)
- Test: inline in both

**Interfaces:**
- Consumes: `Project::set_github` (Task 3).
- CLI: `todos update <id> --github on|off`. `off`/`on` validated; anything else → error.
- MCP: `todo_update` gains optional `github: "on"|"off"`. Empty/absent = unchanged (matches the empty-string quirk). Invalid value → error. Returns the resulting todo.

**Deliberate behavior change (decided, not accidental):** the `has_fields` guard means a *fieldless* `todos update <id>` / `todo_update {id}` (no title/body/priority/status/tags, no `github`) now **falls through to a no-op fetch+emit and does NOT bump `updated`** — today `edit_todo_raw` stamps `updated` unconditionally on any update call. This is required (a pure `--github` toggle must not look like a user edit and re-trigger a push) and is the desirable behavior anyway (a no-content update shouldn't move the clock). Flagged here so it's an owned decision. No other update path changes: any real field update still bumps `updated` exactly as before.

- [ ] **Step 1: Write the failing tests**

In `src/cli/mod.rs` `mod tests` (reuses the `Cli` harness):
```rust
    #[test]
    fn todos_github_toggle_via_update() {
        let cli = Cli::new();
        // add an origin so linking can resolve a repo
        let out = std::process::Command::new("git")
            .arg("-C").arg(cli.repo.path())
            .args(["remote", "add", "origin", "git@github.com:owner/name.git"])
            .output().unwrap();
        assert!(out.status.success());

        cli.todos(&["create", "--title", "Sync me"]);
        let (_, out) = cli.todos(&["list", "--json"]);
        let listed: TodoList = serde_json::from_str(&out).unwrap();
        let id = listed.todos[0].id.clone();

        assert_eq!(cli.todos(&["update", &id, "--github", "on"]).0, 0);
        let (_, out) = cli.todos(&["get", &id, "--json"]);
        let got: crate::store::Todo = serde_json::from_str(&out).unwrap();
        assert_eq!(got.github.as_ref().unwrap().repo, "owner/name");
        assert!(!got.github.as_ref().unwrap().paused);

        assert_eq!(cli.todos(&["update", &id, "--github", "off"]).0, 0);
        let (_, out) = cli.todos(&["get", &id, "--json"]);
        let got: crate::store::Todo = serde_json::from_str(&out).unwrap();
        assert!(got.github.unwrap().paused);

        // bogus value is rejected
        assert_ne!(cli.todos(&["update", &id, "--github", "maybe"]).0, 0);
    }
```

In `src/mcp/tools.rs` `mod tests`:
```rust
    #[test]
    fn test_todo_update_github_param() {
        let e = Env::new();
        // origin on the temp repo so linking resolves
        let out = std::process::Command::new("git")
            .arg("-C").arg(e._repo.path())
            .args(["remote", "add", "origin", "git@github.com:owner/name.git"])
            .output().unwrap();
        assert!(out.status.success());

        let created = e.call("todo_create", r#"{"title":"x"}"#).unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        let on = e.call("todo_update", &format!(r#"{{"id":"{id}","github":"on"}}"#)).unwrap();
        assert_eq!(on["github"]["repo"].as_str(), Some("owner/name"));
        assert_eq!(on["github"]["paused"].as_bool(), Some(false));
        let off = e.call("todo_update", &format!(r#"{{"id":"{id}","github":"off"}}"#)).unwrap();
        assert_eq!(off["github"]["paused"].as_bool(), Some(true));
        // bogus value errors
        assert!(e.call("todo_update", &format!(r#"{{"id":"{id}","github":"maybe"}}"#)).is_err());
    }
```
Note: `Env` holds `_repo`; make it reachable in the test by referencing `e._repo` (the field exists on the struct in this module's tests).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p tally --lib todos_github_toggle_via_update test_todo_update_github_param 2>&1 | tail -20`
Expected: FAIL — `--github` undefined flag / `github` arg absent.

- [ ] **Step 3: Implement the CLI flag**

In `src/cli/todos.rs`, add `"github"` to `VALUE_FLAGS` (after `"blocker"`). Read it alongside the other flags:
```rust
    let github = p.str("github", "");
```
Replace the `"update"` match arm body so a `--github` toggle is applied after any field update and the final todo is emitted:
```rust
        "update" => {
            let b = match body_from(&body, &body_file) {
                Ok(b) => b,
                Err(e) => return fail(&e.to_string()),
            };
            let mut u = TodoUpdate::default();
            if p.was_set("title") { u.title = Some(title); }
            if p.was_set("priority") { u.priority = Some(priority); }
            if p.was_set("status") { u.status = Some(status); }
            if p.was_set("body") || p.was_set("body-file") { u.body = Some(b); }
            if p.was_set("tag") { u.tags = Some(tags); }

            // Apply field updates only when some field was actually set, so a
            // pure --github toggle doesn't write an empty (updated-bumping) update.
            let has_fields = p.was_set("title") || p.was_set("priority")
                || p.was_set("status") || p.was_set("body") || p.was_set("body-file")
                || p.was_set("tag");
            let mut td = if has_fields {
                match proj.update_todo(&id, u) {
                    Ok(td) => Some(td),
                    Err(e) => return fail(&e.to_string()),
                }
            } else {
                None
            };
            if p.was_set("github") {
                let on = match github.as_str() {
                    "on" => true,
                    "off" => false,
                    other => return fail(&format!("--github must be on|off, got {other:?}")),
                };
                match proj.set_github(&id, on) {
                    Ok(t) => td = Some(t),
                    Err(e) => return fail(&e.to_string()),
                }
            }
            match td {
                Some(t) => emit_todo(out, &t),
                None => {
                    // Neither fields nor --github: fall back to a no-op fetch+emit.
                    match proj.get_todo(&id) {
                        Ok(t) => emit_todo(out, &t),
                        Err(e) => return fail(&e.to_string()),
                    }
                }
            }
        }
```

- [ ] **Step 4: Implement the MCP param**

In `src/mcp/tools.rs`, add to `struct Args` (near the todo fields):
```rust
    github: String,
```
In the `todo_update` tool, extend the schema `properties` with `"github": prop("string", "on|off (opt-in sync); empty = unchanged")` and update `run`:
```rust
        Tool { name: "todo_update", desc: "Update provided todo fields; omitted fields preserved.",
            schema: obj(req(&["id"]), json!({"id": prop("string", ""), "title": prop("string", ""), "body": prop("string", ""), "priority": prop("string", ""), "status": prop("string", ""), "tags": arr(""), "github": prop("string", "on|off (opt-in sync); empty = unchanged")})),
            run: |p, a| {
                let mut u = TodoUpdate::default();
                if !a.title.is_empty() { u.title = Some(a.title.clone()); }
                if !a.body.is_empty() { u.body = Some(a.body.clone()); }
                if !a.priority.is_empty() { u.priority = Some(a.priority.clone()); }
                if !a.status.is_empty() { u.status = Some(a.status.clone()); }
                if let Some(t) = &a.tags { u.tags = Some(t.clone()); }
                let has_fields = u.title.is_some() || u.body.is_some() || u.priority.is_some()
                    || u.status.is_some() || u.tags.is_some();
                let mut td = if has_fields { Some(p.update_todo(&a.id, u)?) } else { None };
                // Empty string = unchanged (consistent with the field quirk above).
                if !a.github.is_empty() {
                    let on = match a.github.as_str() {
                        "on" => true,
                        "off" => false,
                        other => return Err(Error::Other(format!("github must be on|off, got {other:?}"))),
                    };
                    td = Some(p.set_github(&a.id, on)?);
                }
                match td {
                    Some(t) => val(t),
                    None => val(p.get_todo(&a.id)?),
                }
            } },
```

- [ ] **Step 5: Run tests, tool-count guard, and clippy**

Run: `cargo test -p tally --lib todos_github_toggle_via_update test_todo_update_github_param test_tool_defs_count 2>&1 | tail -20`
Expected: PASS — including `test_tool_defs_count` (still exactly 38 tools; no tool added).

- [ ] **Step 6: Commit**

```bash
git add src/cli/todos.rs src/mcp/tools.rs
git commit -m "feat(adapters): opt-in github sync via todos update --github / todo_update github"
```

---

### Task 10: TUI driver — keybind, background sync, footer status

Independent convenience layer over the now-complete engine. Tasks 1–9 already deliver working headless sync; this makes the TUI a live driver.

**Files:**
- Modify: `src/tui/app.rs` (add `sync_status`/`sync_tx` fields; `nudge_sync`; `toggle_github`; call `nudge_sync` after todo mutations; add `G` key)
- Modify: `src/tui/mod.rs` (spawn the sync worker; hand the app its channel + shared status)
- Modify: `src/tui/view.rs` (render the sync status line in `draw_footer`; add a `G` hint)
- Test: inline unit test for `summarize`

**Interfaces:**
- Consumes: `crate::store::{GhCli, sync_project, resolve_project, SyncReport, Project}`.
- Produces (in `app.rs`):
  ```rust
  pub sync_status: std::sync::Arc<std::sync::Mutex<String>>,
  pub sync_tx: Option<std::sync::mpsc::Sender<()>>,
  fn nudge_sync(&self);                 // non-blocking wake of the worker
  fn toggle_github(&mut self);          // 'G' on the selected/open todo
  pub(super) fn summarize(rep: &SyncReport) -> String;  // footer text
  ```
  Worker (in `mod.rs`): `fn spawn_sync_worker(project_path: String, status: Arc<Mutex<String>>) -> mpsc::Sender<()>`.

- [ ] **Step 1: Write the failing test for `summarize`**

In `src/tui/app.rs` `mod tests` (add one if none exists at the bottom of the file):
```rust
#[cfg(test)]
mod tests {
    use super::summarize;
    use crate::store::SyncReport;

    #[test]
    fn test_summarize() {
        // Nothing to sync (no links) → quiet footer.
        let rep = SyncReport::default();
        assert!(summarize(&rep).is_empty(), "quiet: {:?}", summarize(&rep));
        // gh genuinely unavailable: there was work, auth failed → error recorded.
        let mut rep = SyncReport::default();
        rep.errors.push("gh unavailable or not authenticated".into());
        assert!(summarize(&rep).contains("gh"), "unavailable: {}", summarize(&rep));
        // Live and synced.
        let mut rep = SyncReport::default();
        rep.gh_available = true;
        rep.checked = 3;
        assert!(summarize(&rep).contains('3'));
        rep.errors.push("t_x: boom".into());
        assert!(summarize(&rep).contains("1"), "should note error count: {}", summarize(&rep));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p tally --lib tui::app::tests::test_summarize 2>&1 | tail -20`
Expected: FAIL — `summarize` not found.

- [ ] **Step 3: Add `summarize` + the App fields**

In `src/tui/app.rs`, add the free function near the top (after the imports/`next_priority`):
```rust
/// One-line sync status for the footer.
pub(super) fn summarize(rep: &crate::store::SyncReport) -> String {
    // Nothing linked: sync gated out before touching gh (gh_available false, no
    // error). Keep the footer quiet rather than alarming every non-user with
    // "gh unavailable" every 60s.
    if !rep.gh_available && rep.errors.is_empty() {
        return String::new();
    }
    if !rep.gh_available {
        return "⚠ gh unavailable".to_string();
    }
    let errs = if rep.errors.is_empty() {
        String::new()
    } else {
        format!(" · {} err", rep.errors.len())
    };
    format!("↕ {} synced{errs}", rep.checked)
}
```
Add fields to `pub struct App` (after `hits`):
```rust
    /// Shared with the background sync worker; read by the footer.
    pub sync_status: std::sync::Arc<std::sync::Mutex<String>>,
    /// Nudge channel to wake the worker after a local mutation. None in tests.
    pub sync_tx: Option<std::sync::mpsc::Sender<()>>,
```
Initialize them in `App::new`'s literal (after `hits: Hits::default(),`):
```rust
            sync_status: std::sync::Arc::new(std::sync::Mutex::new(String::new())),
            sync_tx: None,
```

- [ ] **Step 4: Run to verify `summarize` passes**

Run: `cargo test -p tally --lib tui::app::tests::test_summarize 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Add `nudge_sync`, `toggle_github`, and the key binding**

In `src/tui/app.rs`, inside `impl App`, add:
```rust
    /// Wake the sync worker now (best-effort; ignored if the channel is gone).
    fn nudge_sync(&self) {
        if let Some(tx) = &self.sync_tx {
            let _ = tx.send(());
        }
    }

    /// 'G' on the selected/open todo: flip GitHub sync on/off. Off = pause (keeps
    /// the link). On a synced-but-paused todo, re-enables. Todos tab only.
    fn toggle_github(&mut self) {
        if self.tab != Tab::Todos {
            return;
        }
        let Some(id) = self.selected_id() else { return };
        let currently_on = self
            .todos
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.github.as_ref())
            .is_some_and(|l| !l.paused);
        match self.p.set_github(&id, !currently_on) {
            Ok(_) => {
                self.status = if currently_on {
                    "GitHub sync paused".to_string()
                } else {
                    "GitHub sync on".to_string()
                };
                self.reload();
                self.nudge_sync();
            }
            Err(e) => self.status = format!("sync toggle failed: {e}"),
        }
    }
```
In `on_key`, add a `'G'` arm in the Todos List/Read handling (alongside the existing todo actions — mirror how `'C'` (comment) is dispatched):
```rust
            KeyCode::Char('G') => self.toggle_github(),
```
Then add `self.nudge_sync();` at the end of the existing todo-mutation paths so a synced todo pushes promptly: in `toggle_status` (after the store call succeeds), after a successful edit save of a todo, and after the priority cycle. (Locate these by the existing `self.reload()` calls in those handlers and add `self.nudge_sync();` right after.)

- [ ] **Step 6: Spawn the worker and wire the channel**

In `src/tui/mod.rs`, add imports:
```rust
use std::sync::{Arc, Mutex};
use std::sync::mpsc;

use crate::store::{GhCli, resolve_project as resolve_project_again, sync_project};
```
(You can reuse the existing `resolve_project` import; the alias just avoids a name clash if you prefer — otherwise call `crate::store::resolve_project` directly in the worker.)

Add the worker function:
```rust
/// Background reconcile loop: every 60s (or on nudge) run one sync pass and
/// publish a one-line status. Builds its own Project from the path so the store
/// flock is the only shared state (safe cross-thread). Errors degrade to status.
fn spawn_sync_worker(project_path: String, status: Arc<Mutex<String>>) -> mpsc::Sender<()> {
    let (tx, rx) = mpsc::channel::<()>();
    std::thread::spawn(move || loop {
        match resolve_project(Some(&project_path)) {
            Ok(mut p) => {
                let rep = sync_project(&mut p, &GhCli);
                if let Ok(mut s) = status.lock() {
                    *s = app::summarize(&rep);
                }
            }
            Err(e) => {
                if let Ok(mut s) = status.lock() {
                    *s = format!("sync: {e}");
                }
            }
        }
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    });
    tx
}
```
In `run`, after `let mut a = App::new(p, initial);` and before `a.reload();`, wire the worker to the app (capture the project path from the resolved project BEFORE `App::new` moves it — read `p.path` first):
```rust
    let project_path = p.path.to_string_lossy().into_owned();
    // (p is moved into App::new below)
```
Reorder so `project_path` is captured before the move, then after `App::new`:
```rust
    let sync_status = a.sync_status.clone();
    a.sync_tx = Some(spawn_sync_worker(project_path, sync_status));
```
(`resolve_project` is already imported in this file; drop the alias import if unused.)

- [ ] **Step 7: Render the sync status in the footer**

In `src/tui/view.rs`, change `draw_footer` to append the shared sync status to the hints line (right side), reading the mutex:
```rust
fn draw_footer(app: &App, f: &mut Frame, area: Rect) {
    let hints = footer(app);
    let sync = app.sync_status.lock().map(|s| s.clone()).unwrap_or_default();
    let first = if sync.is_empty() {
        Line::from(hints).dim()
    } else {
        Line::from(vec![
            Span::from(hints).dim(),
            Span::from("    "),
            Span::from(sync).dim(),
        ])
    };
    let mut lines = vec![first];
    if !app.status.is_empty() {
        lines.push(Line::from(app.status.clone()));
    }
    f.render_widget(Paragraph::new(lines), area);
}
```
Add a `G` entry to the Todos read/list footer hint strings and the `HELP_ROWS` table (e.g. `("G", "toggle GitHub sync (todos)")`).

- [ ] **Step 8: Build the binary, run the full suite + clippy/fmt**

Run:
```
cargo build --release && mkdir -p bin && rm -f bin/tally && cp target/release/tally bin/tally
cargo test 2>&1 | tail -20
cargo clippy 2>&1 | tail -15 && cargo fmt --check
```
Expected: build succeeds; all tests pass; clippy clean; fmt clean.

- [ ] **Step 9: Manual smoke (optional, needs `gh` + a scratch repo)**

In a repo whose `origin` you own: `bin/tally tui todos`, select a todo, press `G`, and confirm the footer shows `↕ 1 synced` within ~60s and a GitHub issue appears. Then `bin/tally sync --json` and confirm the report.

- [ ] **Step 10: Commit**

```bash
git add src/tui/app.rs src/tui/mod.rs src/tui/view.rs
git commit -m "feat(tui): G toggles github sync; background reconcile + footer status"
```

---

## Self-Review

**Spec coverage:**
- Data model (`GithubLink`, comment echo fields) → Task 1. ✅
- Reconcile pass steps 1–5 (create, push title/body/state, pull close, pull comments, push comments) → `plan_actions` (Task 5) + `sync_one` (Task 6). ✅
- Title/body-authoritative, state conflict convergence → `plan_actions` tally-wins/GH-wins branch. ✅
- Issue-deleted/access-lost → per-todo error recorded, no unlink (best-effort loop in `sync_project`). ✅
- Drivers: TUI timer+nudge+footer (Task 10), CLI `tally sync` (Task 8), MCP unchanged tool count (Task 9). ✅
- Opt-in surface: TUI `G` (Task 10), CLI `--github` (Task 9), MCP `github` param (Task 9); untick = `paused` keeping repo/number (Task 3). ✅
- Repo from origin, no-origin error (Task 2/3). ✅
- Failure posture: `auth_ok` gate, 30s subprocess watchdog, store never fails on GitHub (Task 6/7). ✅
- Testing: pure decision tests, round-trip/golden serde, echo-prevention, `#[ignore]` live smoke (Tasks 1/5/6/7). ✅

**Type consistency:** `GithubLink` fields, `Action` variants, `Gh` trait signatures, and `SyncReport` fields are used identically across Tasks 5–10. `set_github`/`update_github_link`/`import_github_comment`/`set_comment_github_id` signatures match their call sites in `sync_one`.

**Placeholder scan:** no TBD/"handle edge cases"/"similar to Task N" — every code step is complete. The one deliberately-manual piece is the `#[ignore]` live test (Task 7) and optional manual smoke (Task 10 Step 9), both explicitly scoped.

**Known simplifications (accepted, per spec / ponytail):**
- Footer shows counts, not a wall-clock timestamp (no clock plumbing). Add if asked.
- Comment time-bound is inclusive (`>=`); the id-known set is the real echo/dup guard, so a same-second boundary comment is never missed nor duplicated.
- Closer login is best-effort (events API); unknown → bare `gh` attribution.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-12-github-issue-sync.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

Note: Tasks 1–9 are a complete, working headless feature on their own; Task 10 (TUI) is an independent driver you can defer or drop.

Which approach?
