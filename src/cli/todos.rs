//! Port of internal/cli/todos.go. Thin adapter: id-first arg split, parse the
//! rest as flags, call one store method, emit. The id must come BEFORE flags
//! for id-taking subcommands (`todos update <id> --status x`) — that grammar is
//! documented and agents depend on it.
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use super::{body_from, fail, parse, print_json, project_opt, render, resolve};
use crate::store::{Todo, TodoFilter, TodoUpdate};

const BOOL_FLAGS: &[&str] = &["json", "release-lock"];
const VALUE_FLAGS: &[&str] = &[
    "project",
    "title",
    "body",
    "body-file",
    "priority",
    "status",
    "sort",
    "query",
    "completed",
    "is-blocked",
    "tag",
    "blocker",
    "github",
    "offset",
    "limit",
];
const INT_FLAGS: &[&str] = &["offset", "limit"];

/// The Go `idTaking` set: subcommands whose leading positional is an id.
const ID_TAKING: &[&str] = &[
    "get",
    "update",
    "delete",
    "complete",
    "incomplete",
    "add-tag",
    "remove-tag",
    "set-blockers",
    "add-blocker",
    "remove-blocker",
    "lock",
    "unlock",
];

#[derive(Serialize)]
struct TodoListOut<'a> {
    todos: &'a [Todo],
}

pub(crate) fn run(args: &[String], store_root: Option<&Path>, out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        return fail(
            "usage: tally todos <list|get|create|update|delete|complete|incomplete|add-tag|remove-tag|set-blockers|add-blocker|remove-blocker|lock|unlock|tags>",
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
    let title = p.str("title", "");
    let body = p.str("body", "");
    let body_file = p.str("body-file", "");
    let priority = p.str("priority", "");
    let status = p.str("status", "");
    let sort = p.str("sort", "");
    let query = p.str("query", "");
    let completed = p.str("completed", "");
    let is_blocked = p.str("is-blocked", "");
    let tags = p.multi("tag");
    let blockers = p.multi("blocker");
    let github = p.str("github", "");
    let as_json = p.boolean("json", false);
    let release_lock = p.boolean("release-lock", true);
    let offset = p.int("offset", 0);
    let limit = p.int("limit", 0);

    let proj = match resolve(project_opt(&project), store_root) {
        Ok(p) => p,
        Err(e) => return fail(&e.to_string()),
    };

    match sub {
        "create" => {
            let b = match body_from(&body, &body_file) {
                Ok(b) => b,
                Err(e) => return fail(&e.to_string()),
            };
            match proj.create_todo(&title, &b, &priority, tags) {
                Ok(td) => emit_todo(out, &td),
                Err(e) => return fail(&e.to_string()),
            }
        }
        "list" => {
            let f = TodoFilter {
                status,
                completed: parse_bool_ptr(&completed),
                is_blocked: parse_bool_ptr(&is_blocked),
                priority,
                query,
                tags,
                sort,
                offset,
                limit,
            };
            match proj.list_todos(f) {
                Ok(list) => {
                    if as_json {
                        let _ = print_json(out, &TodoListOut { todos: &list });
                    } else {
                        let _ = render::render_todos(out, &proj, &list);
                    }
                }
                Err(e) => return fail(&e.to_string()),
            }
        }
        "get" => match proj.get_todo(&id) {
            Ok(td) => emit_todo(out, &td),
            Err(e) => return fail(&e.to_string()),
        },
        "update" => {
            let b = match body_from(&body, &body_file) {
                Ok(b) => b,
                Err(e) => return fail(&e.to_string()),
            };
            // Validate --github up front so a bad value doesn't half-apply field edits.
            let github_on = if p.was_set("github") {
                match github.as_str() {
                    "on" => Some(true),
                    "off" => Some(false),
                    other => return fail(&format!("--github must be on|off, got {other:?}")),
                }
            } else {
                None
            };

            let mut u = TodoUpdate::default();
            if p.was_set("title") {
                u.title = Some(title);
            }
            if p.was_set("priority") {
                u.priority = Some(priority);
            }
            if p.was_set("status") {
                u.status = Some(status);
            }
            if p.was_set("body") || p.was_set("body-file") {
                u.body = Some(b);
            }
            if p.was_set("tag") {
                u.tags = Some(tags);
            }

            // Apply field updates only when some field was actually set, so a
            // pure --github toggle doesn't write an empty (updated-bumping) update.
            let has_fields = p.was_set("title")
                || p.was_set("priority")
                || p.was_set("status")
                || p.was_set("body")
                || p.was_set("body-file")
                || p.was_set("tag");
            let mut td = if has_fields {
                match proj.update_todo(&id, u) {
                    Ok(td) => Some(td),
                    Err(e) => return fail(&e.to_string()),
                }
            } else {
                None
            };
            if let Some(on) = github_on {
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
        "delete" => {
            if let Err(e) = proj.delete_todo(&id) {
                return fail(&e.to_string());
            }
        }
        "complete" => match proj.complete_todo(&id, release_lock) {
            Ok(td) => emit_todo(out, &td),
            Err(e) => return fail(&e.to_string()),
        },
        "incomplete" => match proj.incomplete_todo(&id, release_lock) {
            Ok(td) => emit_todo(out, &td),
            Err(e) => return fail(&e.to_string()),
        },
        "add-tag" => match proj.add_todo_tag(&id, &first(&tags)) {
            Ok(td) => emit_todo(out, &td),
            Err(e) => return fail(&e.to_string()),
        },
        "remove-tag" => match proj.remove_todo_tag(&id, &first(&tags)) {
            Ok(td) => emit_todo(out, &td),
            Err(e) => return fail(&e.to_string()),
        },
        "set-blockers" => match proj.set_blockers(&id, blockers) {
            Ok(td) => emit_todo(out, &td),
            Err(e) => return fail(&e.to_string()),
        },
        "add-blocker" => match proj.add_blocker(&id, &first(&blockers)) {
            Ok(td) => emit_todo(out, &td),
            Err(e) => return fail(&e.to_string()),
        },
        "remove-blocker" => match proj.remove_blocker(&id, &first(&blockers)) {
            Ok(td) => emit_todo(out, &td),
            Err(e) => return fail(&e.to_string()),
        },
        "lock" => match proj.lock_todo(&id, &owner_name(), std::process::id() as i64) {
            Ok(td) => emit_todo(out, &td),
            Err(e) => return fail(&e.to_string()),
        },
        "unlock" => match proj.unlock_todo(&id, &owner_name()) {
            Ok(td) => emit_todo(out, &td),
            Err(e) => return fail(&e.to_string()),
        },
        "tags" => match proj.todo_tags() {
            Ok(list) => {
                let _ = print_json(out, &list);
            }
            Err(e) => return fail(&e.to_string()),
        },
        other => return fail(&format!("unknown todos subcommand: {other}")),
    }
    0
}

/// Go's emit for a single todo falls through to MarshalIndent on both the JSON
/// and non-JSON branch, so `--json` is a no-op here — always pretty JSON.
fn emit_todo(out: &mut dyn Write, td: &Todo) {
    let _ = print_json(out, td);
}

/// Go parseBoolPtr: "true"/"false" → Some, anything else (incl. "") → None.
fn parse_bool_ptr(s: &str) -> Option<bool> {
    match s {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Go first(multiFlag): the first value, or "" when none were given.
fn first(m: &[String]) -> String {
    m.first().cloned().unwrap_or_default()
}

/// Go ownerName(): $HERDR_NOTES_OWNER or "agent".
fn owner_name() -> String {
    match std::env::var("HERDR_NOTES_OWNER") {
        Ok(v) if !v.is_empty() => v,
        _ => "agent".to_string(),
    }
}
