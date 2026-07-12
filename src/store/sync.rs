//! GitHub issue sync — decision engine, executor, and the `gh` boundary.
//! All logic lives here (store is the single source of truth). CLI/MCP/TUI drive.

/// Parse a git remote URL to "owner/name". Handles scp-style (`git@host:o/n.git`),
/// ssh (`ssh://git@host/o/n.git`), and https (`https://host/o/n[.git]`). None on
/// anything that doesn't yield two path segments.
// Unused outside tests until a later task wires the sync engine to it.
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
        acts.push(Action::CompleteTodo {
            by: snap.closed_by.clone(),
        });
    } else if !issue_closed && completed {
        acts.push(Action::ReopenTodo {
            by: snap.closed_by.clone(),
        });
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
            acts.push(Action::PushComment {
                comment_id: c.id.clone(),
            });
        }
    }
    acts
}

use super::errors::{Error, Result};
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
    if login.is_empty() {
        "gh".to_string()
    } else {
        format!("gh:{login}")
    }
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
        rep.errors
            .push("gh unavailable or not authenticated".to_string());
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

    let mut pushed_state = false; // we pushed to GH (edit/close/reopen)
    let mut pulled_state = false; // GH won: we bumped the todo (complete/reopen)
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
                p.import_github_comment(
                    &todo.id,
                    &gh_actor(&gc.author),
                    &gc.created,
                    gc.id,
                    &gc.body,
                )?;
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
        link.last_pushed = pass_start.clone();
    }
    // We've now seen every GH comment created up to pass_start; the id-known set
    // guards echoes/dups, so an inclusive pass_start bound loses nothing.
    link.last_comment_pull = pass_start;
    p.update_github_link(&todo.id, link)?;
    Ok(())
}

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    if let (Some(s), Some(mut w)) = (stdin, child.stdin.take()) {
        w.write_all(s.as_bytes())?; // drop(w) closes the pipe
    }
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
            &[
                "issue", "create", "--repo", repo, "--title", title, "--body", body,
            ],
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
                "issue",
                "edit",
                &number.to_string(),
                "--repo",
                repo,
                "--title",
                title,
                "--body",
                body,
            ],
            None,
        )?;
        Ok(())
    }

    fn close_issue(&self, repo: &str, number: i64) -> Result<()> {
        run(
            &["issue", "close", &number.to_string(), "--repo", repo],
            None,
        )?;
        Ok(())
    }

    fn reopen_issue(&self, repo: &str, number: i64) -> Result<()> {
        run(
            &["issue", "reopen", &number.to_string(), "--repo", repo],
            None,
        )?;
        Ok(())
    }

    fn view_issue(&self, repo: &str, number: i64) -> Result<IssueSnapshot> {
        let state_out = run(
            &[
                "issue",
                "view",
                &number.to_string(),
                "--repo",
                repo,
                "--json",
                "state",
            ],
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
                            body: c
                                .get("body")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
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

        Ok(IssueSnapshot {
            state,
            closed_by,
            comments,
        })
    }

    fn create_comment(&self, repo: &str, number: i64, body: &str) -> Result<i64> {
        let path = format!("repos/{repo}/issues/{number}/comments");
        // body on stdin via -f body=@- avoids arg-length/escaping issues.
        let out = run(
            &["api", "--method", "POST", &path, "-f", "body=@-"],
            Some(body),
        )?;
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
        .rfind(|e| e.get("event").and_then(Value::as_str) == Some("closed"))
        .and_then(|e| e.get("actor")?.get("login")?.as_str().map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::new_project;
    use crate::store::{Comment, GithubLink, Todo};
    use std::cell::RefCell;

    /// A scripted GH boundary. Records mutating calls; serves one snapshot.
    struct FakeGh {
        snapshot: IssueSnapshot,
        next_issue: i64,
        next_comment: i64,
        edits: RefCell<Vec<String>>,
    }
    impl FakeGh {
        fn new(snapshot: IssueSnapshot) -> Self {
            FakeGh {
                snapshot,
                next_issue: 7,
                next_comment: 500,
                edits: RefCell::new(vec![]),
            }
        }
    }
    impl Gh for FakeGh {
        fn auth_ok(&self) -> bool {
            true
        }
        fn create_issue(&self, _r: &str, _t: &str, _b: &str) -> crate::store::Result<i64> {
            self.edits.borrow_mut().push("create".into());
            Ok(self.next_issue)
        }
        fn edit_issue(&self, _r: &str, n: i64, _t: &str, _b: &str) -> crate::store::Result<()> {
            self.edits.borrow_mut().push(format!("edit {n}"));
            Ok(())
        }
        fn close_issue(&self, _r: &str, n: i64) -> crate::store::Result<()> {
            self.edits.borrow_mut().push(format!("close {n}"));
            Ok(())
        }
        fn reopen_issue(&self, _r: &str, n: i64) -> crate::store::Result<()> {
            self.edits.borrow_mut().push(format!("reopen {n}"));
            Ok(())
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
            .arg("-C")
            .arg(&p.path)
            .args(["remote", "add", "origin", "git@github.com:o/n.git"])
            .output()
            .unwrap();
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
            fn auth_ok(&self) -> bool {
                false
            }
            fn create_issue(&self, _: &str, _: &str, _: &str) -> crate::store::Result<i64> {
                unreachable!()
            }
            fn edit_issue(&self, _: &str, _: i64, _: &str, _: &str) -> crate::store::Result<()> {
                unreachable!()
            }
            fn close_issue(&self, _: &str, _: i64) -> crate::store::Result<()> {
                unreachable!()
            }
            fn reopen_issue(&self, _: &str, _: i64) -> crate::store::Result<()> {
                unreachable!()
            }
            fn view_issue(&self, _: &str, _: i64) -> crate::store::Result<IssueSnapshot> {
                unreachable!()
            }
            fn create_comment(&self, _: &str, _: i64, _: &str) -> crate::store::Result<i64> {
                unreachable!()
            }
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
            fn auth_ok(&self) -> bool {
                panic!("must not check auth with no links")
            }
            fn create_issue(&self, _: &str, _: &str, _: &str) -> crate::store::Result<i64> {
                unreachable!()
            }
            fn edit_issue(&self, _: &str, _: i64, _: &str, _: &str) -> crate::store::Result<()> {
                unreachable!()
            }
            fn close_issue(&self, _: &str, _: i64) -> crate::store::Result<()> {
                unreachable!()
            }
            fn reopen_issue(&self, _: &str, _: i64) -> crate::store::Result<()> {
                unreachable!()
            }
            fn view_issue(&self, _: &str, _: i64) -> crate::store::Result<IssueSnapshot> {
                unreachable!()
            }
            fn create_comment(&self, _: &str, _: i64, _: &str) -> crate::store::Result<i64> {
                unreachable!()
            }
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
        let snap = IssueSnapshot {
            state: IssueState::Closed,
            closed_by: "octocat".into(),
            comments: vec![],
        };
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
            state: IssueState::Open,
            closed_by: String::new(),
            comments: vec![GhComment {
                id: 100,
                author: "octocat".into(),
                created: "2026-07-12T09:00:00Z".into(),
                body: "on it".into(),
            }],
        };
        let rep = sync_project(&mut tp.p, &FakeGh::new(snap));
        assert_eq!(rep.pulled_comments, 1);
        assert_eq!(rep.pushed_comments, 1);
        let comments = tp.list_comments(&id).unwrap();
        // pulled comment present with gh author + id; local note now carries id 500.
        assert!(
            comments
                .iter()
                .any(|c| c.author == "gh:octocat" && c.github_comment_id == 100)
        );
        assert!(
            comments
                .iter()
                .any(|c| c.text == "please look" && c.github_comment_id == 500)
        );
    }

    fn linked(number: i64, last_pushed: &str, last_comment_pull: &str) -> GithubLink {
        GithubLink {
            repo: "o/n".into(),
            number,
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
        let snap = IssueSnapshot {
            state: IssueState::Closed,
            closed_by: "octocat".into(),
            comments: vec![],
        };
        let acts = plan_actions(&t, &link, &[], &snap);
        assert_eq!(
            acts,
            vec![Action::CompleteTodo {
                by: "octocat".into()
            }]
        );
    }

    #[test]
    fn test_plan_import_new_comment_but_not_echo() {
        let t = todo_at("open", "2026-07-12T01:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "2026-07-12T00:00:00Z");
        // gc 100 is new; gc 200 is one we already pushed (present as a local id) -> skip.
        let snap = IssueSnapshot {
            state: IssueState::Open,
            closed_by: String::new(),
            comments: vec![
                GhComment {
                    id: 100,
                    author: "octocat".into(),
                    created: "2026-07-12T01:30:00Z".into(),
                    body: "new".into(),
                },
                GhComment {
                    id: 200,
                    author: "me".into(),
                    created: "2026-07-12T01:31:00Z".into(),
                    body: "echo".into(),
                },
            ],
        };
        let local = vec![note("c_local", "you", 200)]; // github_comment_id 200 => already known
        let acts = plan_actions(&t, &link, &local, &snap);
        assert_eq!(acts, vec![Action::ImportComment(snap.comments[0].clone())]);
    }

    #[test]
    fn test_plan_push_local_note_but_not_pulled_or_event() {
        let t = todo_at("open", "2026-07-12T01:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "2026-07-12T00:00:00Z");
        let snap = IssueSnapshot::default();
        let mut event = note("c_ev", "you", 0);
        event.kind = "event".into(); // auto-logged status event: must NOT push
        let local = vec![
            note("c_push", "you", 0),            // local note, never on GH -> push
            note("c_pulled", "gh:octocat", 100), // pulled from GH -> never push back
            event,
        ];
        let acts = plan_actions(&t, &link, &local, &snap);
        assert_eq!(
            acts,
            vec![Action::PushComment {
                comment_id: "c_push".into()
            }]
        );
    }

    #[test]
    fn test_plan_reopen_issue_when_tally_newer_and_issue_closed() {
        // tally is newer (edited after last push) and todo is open, but the GH
        // issue is closed -> push an edit then reopen the issue.
        let t = todo_at("open", "2026-07-12T02:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "");
        let snap = IssueSnapshot {
            state: IssueState::Closed,
            closed_by: String::new(),
            comments: vec![],
        };
        let acts = plan_actions(&t, &link, &[], &snap);
        assert_eq!(acts, vec![Action::EditIssue, Action::ReopenIssue]);
    }

    #[test]
    fn test_plan_reopen_todo_when_gh_reopened() {
        // tally NOT newer (updated == last_pushed) so GH wins; issue is open but
        // the todo is completed -> pull a reopen onto the todo.
        let t = todo_at("completed", "2026-07-12T01:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "");
        let snap = IssueSnapshot::default(); // Open, closed_by ""
        let acts = plan_actions(&t, &link, &[], &snap);
        assert_eq!(acts, vec![Action::ReopenTodo { by: String::new() }]);
    }

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
        gh.edit_issue(&repo, n, "tally smoke edited", "body2")
            .unwrap();
        let cid = gh.create_comment(&repo, n, "hello from tally").unwrap();
        assert!(cid > 0);
        let snap = gh.view_issue(&repo, n).unwrap();
        assert!(snap.comments.iter().any(|c| c.id == cid));
        gh.close_issue(&repo, n).unwrap();
        let snap = gh.view_issue(&repo, n).unwrap();
        assert_eq!(snap.state, IssueState::Closed);
    }

    #[test]
    fn test_parse_repo_forms() {
        assert_eq!(
            parse_repo("git@github.com:owner/name.git").as_deref(),
            Some("owner/name")
        );
        assert_eq!(
            parse_repo("https://github.com/owner/name.git").as_deref(),
            Some("owner/name")
        );
        assert_eq!(
            parse_repo("https://github.com/owner/name").as_deref(),
            Some("owner/name")
        );
        assert_eq!(
            parse_repo("ssh://git@github.com/owner/name.git").as_deref(),
            Some("owner/name")
        );
        assert_eq!(
            parse_repo("  https://github.com/owner/name.git\n").as_deref(),
            Some("owner/name")
        );
        assert_eq!(parse_repo("not-a-url"), None);
        assert_eq!(parse_repo(""), None);
    }
}
