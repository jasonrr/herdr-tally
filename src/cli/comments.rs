use std::io::Write;
use std::path::Path;

use serde::Serialize;

use super::render;
use super::{fail, parse, print_json, project_opt, resolve};
use crate::store::Comment;

const BOOL_FLAGS: &[&str] = &["json"];
const VALUE_FLAGS: &[&str] = &["project", "body", "section"];
const INT_FLAGS: &[&str] = &[];
/// All three subcommands take a leading positional (target, or comment id).
const ID_TAKING: &[&str] = &["add", "list", "delete"];

pub(crate) fn run(args: &[String], store_root: Option<&Path>, out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        return fail("usage: tally comments <add|list|delete>");
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
        other => return fail(&format!("unknown comments subcommand: {other}")),
    }
    0
}

#[derive(Serialize)]
struct CommentListOut<'a> {
    comments: &'a [Comment],
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
}
