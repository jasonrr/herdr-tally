//! Port of internal/cli/scratchpads.go. Same shape as todos.rs plus the CLI's
//! half of the revision guard: every mutating op REQUIRES --expected-revision
//! except append/append-section (where -1 opts out). The store enforces the
//! guard too; this is a second, adapter-level line of defense (Go parity).
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use super::{body_from, fail, parse, print_json, project_opt, render, resolve};
use crate::store::{EditTarget, Scratchpad};

const BOOL_FLAGS: &[&str] = &[
    "json",
    "include-archived",
    "newline",
    "case-sensitive",
    "confirm",
];
const VALUE_FLAGS: &[&str] = &[
    "project",
    "name",
    "content",
    "content-file",
    "mode",
    "section-heading",
    "heading",
    "query",
    "scope",
    "path",
    "target",
    "tag",
    "expected-revision",
    "offset",
    "limit",
    "context-lines",
    "lines",
];
const INT_FLAGS: &[&str] = &[
    "expected-revision",
    "offset",
    "limit",
    "context-lines",
    "lines",
];

const ID_TAKING: &[&str] = &[
    "read",
    "update",
    "append",
    "append-section",
    "edit",
    "rename",
    "find",
    "tail",
    "clear",
    "archive",
    "unarchive",
    "delete",
    "save-to-file",
];

/// Go's `revisionRequired` set — the mutating ops that MUST be guarded (append
/// and append-section are deliberately absent: they accept the -1 opt-out).
const REVISION_REQUIRED: &[&str] = &[
    "update",
    "rename",
    "edit",
    "clear",
    "archive",
    "unarchive",
    "delete",
];

#[derive(Serialize)]
struct ReadOut<'a> {
    scratchpad: &'a Scratchpad,
    text: &'a str,
}
#[derive(Serialize)]
struct ListOut<'a> {
    scratchpads: &'a [Scratchpad],
}
#[derive(Serialize)]
struct TailOut<'a> {
    text: &'a str,
    total_lines: i64,
}

pub(crate) fn run(args: &[String], store_root: Option<&Path>, out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        return fail(
            "usage: tally scratchpads <list|read|create|update|append|append-section|edit|rename|find|tail|clear|archive|unarchive|delete|save-to-file|load-from-file|tags>",
        );
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
    let name = p.str("name", "");
    let content = p.str("content", "");
    let content_file = p.str("content-file", "");
    let mode = p.str("mode", "full");
    let section_heading = p.str("section-heading", "");
    let heading = p.str("heading", "");
    let query = p.str("query", "");
    let scope = p.str("scope", "all");
    let path = p.str("path", "");
    let target = p.str("target", "");
    let tags = p.multi("tag");
    let as_json = p.boolean("json", false);
    let include_archived = p.boolean("include-archived", false);
    let newline = p.boolean("newline", false);
    let case_sensitive = p.boolean("case-sensitive", false);
    let confirm = p.boolean("confirm", false);
    let expected_revision = p.int("expected-revision", -1);
    let offset = p.int("offset", 0);
    let limit = p.int("limit", 0);
    let context_lines = p.int("context-lines", 0);
    let lines = p.int("lines", 10);

    let proj = match resolve(project_opt(&project), store_root) {
        Ok(p) => p,
        Err(e) => return fail(&e.to_string()),
    };
    let body = match body_from(&content, &content_file) {
        Ok(b) => b,
        Err(e) => return fail(&e.to_string()),
    };

    if REVISION_REQUIRED.contains(&sub) && expected_revision < 0 {
        return fail(&format!("--expected-revision is required for {sub}"));
    }

    match sub {
        "create" => match proj.create_scratchpad(&name, &body, tags) {
            Ok(s) => {
                let _ = print_json(out, &s);
            }
            Err(e) => return fail(&e.to_string()),
        },
        "read" => match proj.read_scratchpad(&id, &mode, &section_heading, offset, limit) {
            Ok((s, text)) => {
                if as_json {
                    let _ = print_json(
                        out,
                        &ReadOut {
                            scratchpad: &s,
                            text: &text,
                        },
                    );
                } else {
                    let _ = writeln!(out, "{text}");
                    // Only the human-facing full read gets the comments block;
                    // content/section/headings modes stay raw for machine callers.
                    if mode == "full"
                        && let Ok(cs) = proj.list_comments(&id)
                        && !cs.is_empty()
                    {
                        let _ = writeln!(out, "\n---\n## Comments\n");
                        let _ = render::render_comments(out, &cs);
                    }
                }
            }
            Err(e) => return fail(&e.to_string()),
        },
        "list" => match proj.list_scratchpads(&tags, &query, include_archived, offset, limit) {
            Ok(list) => {
                if as_json {
                    let _ = print_json(out, &ListOut { scratchpads: &list });
                } else {
                    let _ = render::render_scratchpads(out, &proj, &list);
                }
            }
            Err(e) => return fail(&e.to_string()),
        },
        "update" => {
            let name_p = if p.was_set("name") {
                Some(name.as_str())
            } else {
                None
            };
            let body_p = if p.was_set("content") || p.was_set("content-file") {
                Some(body.as_str())
            } else {
                None
            };
            let tags_p = if p.was_set("tag") {
                Some(tags.clone())
            } else {
                None
            };
            match proj.update_scratchpad(&id, expected_revision, name_p, body_p, tags_p) {
                Ok(s) => {
                    let _ = print_json(out, &s);
                }
                Err(e) => return fail(&e.to_string()),
            }
        }
        "append" => match proj.append_scratchpad(&id, &body, expected_revision, newline) {
            Ok(s) => {
                let _ = print_json(out, &s);
            }
            Err(e) => return fail(&e.to_string()),
        },
        "append-section" => match proj.append_section(&id, &heading, &body, expected_revision) {
            Ok(s) => {
                let _ = print_json(out, &s);
            }
            Err(e) => return fail(&e.to_string()),
        },
        "edit" => {
            let et: EditTarget = match serde_json::from_str(&target) {
                Ok(t) => t,
                Err(e) => return fail(&format!("bad --target json: {e}")),
            };
            match proj.edit_scratchpad(&id, et, &body, expected_revision) {
                Ok(s) => {
                    let _ = print_json(out, &s);
                }
                Err(e) => return fail(&e.to_string()),
            }
        }
        "rename" => match proj.rename_scratchpad(&id, &name, expected_revision) {
            Ok(s) => {
                let _ = print_json(out, &s);
            }
            Err(e) => return fail(&e.to_string()),
        },
        "find" => {
            match proj.find_in_scratchpad(&id, &query, &scope, case_sensitive, context_lines) {
                Ok(m) => {
                    let _ = print_json(out, &m);
                }
                Err(e) => return fail(&e.to_string()),
            }
        }
        "tail" => match proj.tail_scratchpad(&id, lines) {
            Ok((text, total)) => {
                if as_json {
                    let _ = print_json(
                        out,
                        &TailOut {
                            text: &text,
                            total_lines: total,
                        },
                    );
                } else {
                    let _ = writeln!(out, "{text}");
                }
            }
            Err(e) => return fail(&e.to_string()),
        },
        "clear" => match proj.clear_scratchpad(&id, expected_revision) {
            Ok(s) => {
                let _ = print_json(out, &s);
            }
            Err(e) => return fail(&e.to_string()),
        },
        "archive" => match proj.archive_scratchpad(&id, expected_revision) {
            Ok(s) => {
                let _ = print_json(out, &s);
            }
            Err(e) => return fail(&e.to_string()),
        },
        "unarchive" => match proj.unarchive_scratchpad(&id, expected_revision) {
            Ok(s) => {
                let _ = print_json(out, &s);
            }
            Err(e) => return fail(&e.to_string()),
        },
        "delete" => {
            if !confirm {
                return fail("delete requires --confirm");
            }
            if let Err(e) = proj.delete_scratchpad(&id, expected_revision) {
                return fail(&e.to_string());
            }
        }
        "save-to-file" => {
            if let Err(e) = proj.save_scratchpad_to_file(&id, &path) {
                return fail(&e.to_string());
            }
        }
        "load-from-file" => match proj.load_scratchpad_from_file(&path) {
            Ok(s) => {
                let _ = print_json(out, &s);
            }
            Err(e) => return fail(&e.to_string()),
        },
        "tags" => match proj.scratchpad_tags() {
            Ok(list) => {
                let _ = print_json(out, &list);
            }
            Err(e) => return fail(&e.to_string()),
        },
        other => return fail(&format!("unknown scratchpads subcommand: {other}")),
    }
    0
}
