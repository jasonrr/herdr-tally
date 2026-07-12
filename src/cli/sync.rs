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
