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
            rep.checked,
            rep.created,
            rep.pushed,
            rep.state_changes,
            rep.pulled_comments,
            rep.pushed_comments,
            if rep.errors.is_empty() {
                String::new()
            } else {
                format!(", {} error(s)", rep.errors.len())
            },
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
        fn auth_ok(&self) -> bool {
            true
        }
        fn create_issue(&self, _: &str, _: &str, _: &str) -> Result<i64> {
            Ok(1)
        }
        fn edit_issue(&self, _: &str, _: i64, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn close_issue(&self, _: &str, _: i64) -> Result<()> {
            Ok(())
        }
        fn reopen_issue(&self, _: &str, _: i64) -> Result<()> {
            Ok(())
        }
        fn view_issue(&self, _: &str, _: i64) -> Result<IssueSnapshot> {
            Ok(IssueSnapshot::default())
        }
        fn create_comment(&self, _: &str, _: i64, _: &str) -> Result<i64> {
            Ok(1)
        }
    }

    struct DeadGh;
    impl Gh for DeadGh {
        fn auth_ok(&self) -> bool {
            false
        }
        fn create_issue(&self, _: &str, _: &str, _: &str) -> Result<i64> {
            unreachable!()
        }
        fn edit_issue(&self, _: &str, _: i64, _: &str, _: &str) -> Result<()> {
            unreachable!()
        }
        fn close_issue(&self, _: &str, _: i64) -> Result<()> {
            unreachable!()
        }
        fn reopen_issue(&self, _: &str, _: i64) -> Result<()> {
            unreachable!()
        }
        fn view_issue(&self, _: &str, _: i64) -> Result<IssueSnapshot> {
            unreachable!()
        }
        fn create_comment(&self, _: &str, _: i64, _: &str) -> Result<i64> {
            unreachable!()
        }
    }

    /// A git repo holding one todo linked to GH, so sync has work to do and the
    /// CLI reaches the gh-unavailable / full-summary branches.
    fn repo_with_linked_todo(root: &TempDir) -> TempDir {
        let repo = git_repo();
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["remote", "add", "origin", "git@github.com:o/n.git"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let p = crate::store::resolve_project_in(root.path(), Some(&repo.path().to_string_lossy()))
            .unwrap();
        let td = p.create_todo("issue", "body", "", Vec::new()).unwrap();
        p.set_github(&td.id, true).unwrap();
        repo
    }

    #[test]
    fn sync_human_reports_gh_unavailable_with_error_lines() {
        let root = TempDir::new();
        let repo = repo_with_linked_todo(&root);
        let args = vec![
            "--project".to_string(),
            repo.path().to_string_lossy().into_owned(),
        ];
        let mut buf = Vec::new();
        let code = super::run(&args, Some(root.path()), &DeadGh, &mut buf);
        assert_eq!(code, 0);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("sync skipped: gh unavailable"), "{out}");
        assert!(out.contains("  ! gh unavailable"), "{out}");
        assert!(!out.contains("nothing to sync"), "{out}");
    }

    #[test]
    fn sync_human_prints_full_summary_when_gh_available() {
        let root = TempDir::new();
        let repo = repo_with_linked_todo(&root);
        let args = vec![
            "--project".to_string(),
            repo.path().to_string_lossy().into_owned(),
        ];
        let mut buf = Vec::new();
        let code = super::run(&args, Some(root.path()), &OkGh, &mut buf);
        assert_eq!(code, 0);
        let out = String::from_utf8(buf).unwrap();
        // First pass creates the issue and returns early: 1 checked, 1 created.
        assert!(
            out.contains(
                "synced 1 todo(s): 1 created, 0 pushed, 0 state change(s), 0 comment(s) in, 0 out\n"
            ),
            "{out}"
        );
        assert!(!out.contains("error(s)"), "{out}");
    }

    #[test]
    fn sync_reports_json_when_no_synced_todos() {
        let root = TempDir::new();
        let repo = git_repo();
        let args = vec![
            "--project".to_string(),
            repo.path().to_string_lossy().into_owned(),
            "--json".to_string(),
        ];
        let mut buf = Vec::new();
        let code = super::run(&args, Some(root.path()), &OkGh, &mut buf);
        assert_eq!(code, 0);
        let out = String::from_utf8(buf).unwrap();
        // No linked todos → sync gates out before touching gh, so gh_available
        // stays false and nothing is checked. (OkGh's auth_ok is never called.)
        assert!(out.contains(r#""gh_available": false"#), "{out}");
        assert!(out.contains(r#""checked": 0"#), "{out}");
    }

    #[test]
    fn sync_human_says_nothing_to_sync_when_no_links() {
        let root = TempDir::new();
        let repo = git_repo();
        let args = vec![
            "--project".to_string(),
            repo.path().to_string_lossy().into_owned(),
        ];
        let mut buf = Vec::new();
        let code = super::run(&args, Some(root.path()), &OkGh, &mut buf);
        assert_eq!(code, 0);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("nothing to sync"), "{out}");
    }
}
