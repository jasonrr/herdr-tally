use std::io::Write;
use std::path::Path;

use serde::Serialize;

use super::render;
use super::{fail, parse, print_json, project_opt, resolve};
use crate::store::{Comment, CommentSummary};

const BOOL_FLAGS: &[&str] = &["json", "include-events"];
const VALUE_FLAGS: &[&str] = &["project", "body", "section", "since", "author"];
const INT_FLAGS: &[&str] = &[];
/// All three subcommands take a leading positional (target, or comment id).
const ID_TAKING: &[&str] = &["add", "list", "delete"];

pub(crate) fn run(args: &[String], store_root: Option<&Path>, out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        return fail("usage: tally comments <add|list|delete|recent|targets>");
    }
    let sub = args[0].as_str();
    let rest = &args[1..];

    let (id, flag_args): (String, &[String]) =
        if ID_TAKING.contains(&sub) && !rest.is_empty() && !rest[0].starts_with('-') {
            (rest[0].clone(), &rest[1..])
        } else {
            (String::new(), rest)
        };

    let p = match parse(flag_args, BOOL_FLAGS, VALUE_FLAGS, INT_FLAGS) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let project = p.str("project", "");
    let body = p.str("body", "");
    let section = p.str("section", "");
    let as_json = p.boolean("json", false);
    let since = p.str("since", "24h");
    let author = p.str("author", "");
    let include_events = p.boolean("include-events", false);

    let proj = match resolve(project_opt(&project), store_root) {
        Ok(p) => p,
        Err(e) => return fail(&e.to_string()),
    };

    match sub {
        "add" => {
            if id.is_empty() {
                return fail(
                    "usage: tally comments add <target> --body <text> [--section <heading>]",
                );
            }
            if body.is_empty() {
                return fail("comment body is required (--body)");
            }
            match proj.add_comment(&id, &section, &body) {
                Ok(c) => {
                    let _ = print_json(out, &c);
                }
                Err(e) => return fail(&e.to_string()),
            }
        }
        "list" => {
            if id.is_empty() {
                return fail("usage: tally comments list <target>");
            }
            match proj.list_comments(&id) {
                Ok(list) => {
                    if as_json {
                        let _ = print_json(out, &CommentListOut { comments: &list });
                    } else {
                        let _ = render::render_comments(out, &list);
                    }
                }
                Err(e) => return fail(&e.to_string()),
            }
        }
        "delete" => {
            if id.is_empty() {
                return fail("usage: tally comments delete <comment-id>");
            }
            if let Err(e) = proj.delete_comment(&id) {
                return fail(&e.to_string());
            }
        }
        "recent" => {
            let cutoff = proj.recency_cutoff(&since);
            let author_opt = if author.is_empty() {
                None
            } else {
                Some(author.as_str())
            };
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
        other => return fail(&format!("unknown comments subcommand: {other}")),
    }
    0
}

#[derive(Serialize)]
struct CommentListOut<'a> {
    comments: &'a [Comment],
}

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

#[cfg(test)]
mod tests {
    use super::run;
    use crate::store::testutil::{TempDir, git_repo};

    // Mirrors the `Cli` harness in cli/mod.rs tests: a throwaway store root +
    // git repo, --project passed explicitly (parallel-safe, no cwd/env).
    #[test]
    fn test_add_then_list() {
        let root = TempDir::new();
        let repo = git_repo();
        let proj = repo.path().to_string_lossy().into_owned();

        // proj.as_str() keeps every element &str — mixing &String with &str
        // literals fails to unify (no deref coercion through array LUB).
        let add = [
            "add",
            "t_cli",
            "--project",
            proj.as_str(),
            "--body",
            "hello",
        ]
        .map(String::from);
        let mut buf: Vec<u8> = Vec::new();
        assert_eq!(run(&add, Some(root.path()), &mut buf), 0);
        assert!(String::from_utf8_lossy(&buf).contains("hello"));

        let list = ["list", "t_cli", "--project", proj.as_str(), "--json"].map(String::from);
        let mut lbuf: Vec<u8> = Vec::new();
        assert_eq!(run(&list, Some(root.path()), &mut lbuf), 0);
        assert!(String::from_utf8_lossy(&lbuf).contains("\"text\": \"hello\""));
    }

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
        assert!(
            s.find("\"two\"").unwrap() < s.find("\"one\"").unwrap(),
            "newest first: {s}"
        );

        // targets --json returns one row for t_z with count 2 and a title field
        let targets = ["targets", "--project", proj.as_str(), "--json"].map(String::from);
        let mut tb: Vec<u8> = Vec::new();
        assert_eq!(run(&targets, Some(root.path()), &mut tb), 0);
        let s = String::from_utf8_lossy(&tb);
        assert!(s.contains("\"target\": \"t_z\""), "targets json: {s}");
        assert!(s.contains("\"count\": 2"), "targets json: {s}");
        assert!(s.contains("\"title\""), "targets json: {s}");
    }
}
