// Port of internal/mcp/tools.go — the 33 Solo-identical tools plus 5 comment_*
// tools (38 total). Each tool is a
// declarative {name, description, inputSchema} plus a `run` fn that calls
// exactly ONE store method, keeping the adapter mechanical (logic lives in
// store). Tool names/descriptions/schemas are byte-parity with the Go registry
// so agent prompts port over unchanged.
use serde::Deserialize;
use serde_json::{Value, json};

use super::Resolve;
use crate::store::{EditTarget, Error, Project, Result, TodoFilter, TodoUpdate};

/// The permissive union of every tool's parameters (Go's `args`). serde
/// `default` fills omitted fields; field names already match the Go JSON tags
/// (all snake_case), so no per-field rename is needed. `tags` and the pointer
/// fields stay Option so the nil-vs-empty distinction Go relied on survives
/// (a missing `tags` leaves tags unchanged; `"tags":[]` clears them).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Args {
    project: String,
    // todos
    id: String,
    title: String,
    body: String,
    priority: String,
    status: String,
    tags: Option<Vec<String>>,
    tag: String,
    blocker: String,
    blockers: Vec<String>,
    github: String,
    query: String,
    sort: String,
    completed: Option<bool>,
    is_blocked: Option<bool>,
    offset: i64,
    limit: i64,
    // scratchpads
    name: String,
    content: String,
    mode: String,
    section_heading: String,
    heading: String,
    scope: String,
    case_sensitive: bool,
    context_lines: i64,
    lines: Option<i64>,
    newline: bool,
    include_archived: bool,
    expected_revision: Option<i64>,
    target: EditTarget,
    path: String,
    // coordination
    owner: String,
    pid: i64,
    // comments
    section: String,
    since: String,
    author: String,
    include_events: bool,
}

impl Args {
    /// Go's args.rev(): nil expected_revision means -1 (skip the store guard).
    fn rev(&self) -> i64 {
        self.expected_revision.unwrap_or(-1)
    }

    fn tags(&self) -> Vec<String> {
        self.tags.clone().unwrap_or_default()
    }
}

/// requireRevision: adapter-level guard so required-revision mutations can't
/// silently clobber concurrent edits by omitting expected_revision. The store's
/// -1-means-skip mechanism stays as-is; this rejects the omission up front.
fn require_revision(a: &Args, tool_name: &str) -> Result<()> {
    if a.expected_revision.is_none() {
        return Err(Error::Other(format!(
            "expected_revision is required for {tool_name}"
        )));
    }
    Ok(())
}

fn or_agent(s: &str) -> String {
    if s.is_empty() {
        if let Ok(a) = std::env::var("HERDR_NOTES_OWNER")
            && !a.is_empty()
        {
            return a;
        }
        return "agent".to_string();
    }
    s.to_string()
}

fn or_default<'a>(s: &'a str, d: &'a str) -> &'a str {
    if s.is_empty() { d } else { s }
}

/// Serialize a store value into the JSON result the tool returns.
fn val<T: serde::Serialize>(x: T) -> Result<Value> {
    Ok(serde_json::to_value(x)?)
}

struct Tool {
    name: &'static str,
    desc: &'static str,
    schema: Value,
    run: fn(&Project, &Args) -> Result<Value>,
}

// obj/prop/arr mirror the Go schema builders. An empty required set is passed as
// Value::Null and OMITS the key: JSON Schema requires `required` to be an array
// when present, and emitting `null` makes Claude Code reject the whole tools list
// ("inputSchema.required expected an array but got null"). (Go marshaled a nil
// []string to JSON null; matching that byte-for-byte broke the MCP handshake.)
fn obj(required: Value, props: Value) -> Value {
    if required.is_null() {
        json!({"type": "object", "properties": props})
    } else {
        json!({"type": "object", "required": required, "properties": props})
    }
}
fn prop(typ: &str, desc: &str) -> Value {
    json!({"type": typ, "description": desc})
}
fn arr(desc: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "description": desc})
}
fn req(names: &[&str]) -> Value {
    json!(names)
}

#[rustfmt::skip]
fn registry() -> Vec<Tool> {
    vec![
        // ─── todos ───────────────────────────────────────────────
        Tool { name: "todo_create", desc: "Create a todo in the project.",
            schema: obj(req(&["title"]), json!({"title": prop("string", ""), "body": prop("string", ""), "priority": prop("string", "high|medium|low"), "tags": arr("")})),
            run: |p, a| val(p.create_todo(&a.title, &a.body, &a.priority, a.tags())?) },
        Tool { name: "todo_list", desc: "List todos with optional filters and sort.",
            schema: obj(Value::Null, json!({"status": prop("string", ""), "completed": prop("boolean", ""), "is_blocked": prop("boolean", ""), "priority": prop("string", ""), "query": prop("string", ""), "tags": arr(""), "sort": prop("string", ""), "offset": prop("integer", ""), "limit": prop("integer", "")})),
            run: |p, a| val(p.list_todos(TodoFilter { status: a.status.clone(), completed: a.completed, is_blocked: a.is_blocked, priority: a.priority.clone(), query: a.query.clone(), tags: a.tags(), sort: a.sort.clone(), offset: a.offset, limit: a.limit })?) },
        Tool { name: "todo_get", desc: "Read one todo by id.",
            schema: obj(req(&["id"]), json!({"id": prop("string", "")})),
            run: |p, a| val(p.get_todo(&a.id)?) },
        Tool { name: "todo_update", desc: "Update provided todo fields; omitted fields preserved.",
            schema: obj(req(&["id"]), json!({"id": prop("string", ""), "title": prop("string", ""), "body": prop("string", ""), "priority": prop("string", ""), "status": prop("string", ""), "tags": arr(""), "github": prop("string", "on|off (opt-in sync); empty = unchanged")})),
            run: |p, a| {
                let mut u = TodoUpdate::default();
                // Deliberate quirk: empty string means "unchanged" (can't clear a
                // field to "" via MCP). Preserve exactly — see CLAUDE.md.
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
        Tool { name: "todo_delete", desc: "Delete a todo.",
            schema: obj(req(&["id"]), json!({"id": prop("string", "")})),
            run: |p, a| { p.delete_todo(&a.id)?; Ok(Value::Null) } },
        Tool { name: "todo_complete", desc: "Mark a todo complete (or incomplete via status).",
            schema: obj(req(&["id"]), json!({"id": prop("string", ""), "completed": prop("boolean", "false → reopen")})),
            run: |p, a| {
                if let Some(false) = a.completed {
                    val(p.incomplete_todo(&a.id, true)?)
                } else {
                    val(p.complete_todo(&a.id, true)?)
                }
            } },
        Tool { name: "todo_add_tag", desc: "Add one tag without replacing others.",
            schema: obj(req(&["id", "tag"]), json!({"id": prop("string", ""), "tag": prop("string", "")})),
            run: |p, a| val(p.add_todo_tag(&a.id, &a.tag)?) },
        Tool { name: "todo_remove_tag", desc: "Remove one tag.",
            schema: obj(req(&["id", "tag"]), json!({"id": prop("string", ""), "tag": prop("string", "")})),
            run: |p, a| val(p.remove_todo_tag(&a.id, &a.tag)?) },
        Tool { name: "todo_tags_list", desc: "List distinct todo tags.",
            schema: obj(Value::Null, json!({})),
            run: |p, _a| val(p.todo_tags()?) },
        Tool { name: "todo_set_blockers", desc: "Replace the full blocker list.",
            schema: obj(req(&["id"]), json!({"id": prop("string", ""), "blockers": arr("")})),
            run: |p, a| val(p.set_blockers(&a.id, a.blockers.clone())?) },
        Tool { name: "todo_add_blocker", desc: "Add one blocker.",
            schema: obj(req(&["id", "blocker"]), json!({"id": prop("string", ""), "blocker": prop("string", "")})),
            run: |p, a| val(p.add_blocker(&a.id, &a.blocker)?) },
        Tool { name: "todo_remove_blocker", desc: "Remove one blocker.",
            schema: obj(req(&["id", "blocker"]), json!({"id": prop("string", ""), "blocker": prop("string", "")})),
            run: |p, a| val(p.remove_blocker(&a.id, &a.blocker)?) },
        Tool { name: "todo_lock", desc: "Lock a todo for coordinated editing.",
            schema: obj(req(&["id"]), json!({"id": prop("string", ""), "owner": prop("string", "")})),
            run: |p, a| val(p.lock_todo(&a.id, &or_agent(&a.owner), a.pid)?) },
        Tool { name: "todo_unlock", desc: "Release a lock you own.",
            schema: obj(req(&["id"]), json!({"id": prop("string", ""), "owner": prop("string", "")})),
            run: |p, a| val(p.unlock_todo(&a.id, &or_agent(&a.owner))?) },
        Tool { name: "todo_transfer", desc: "Not supported cross-project in v1.",
            schema: obj(req(&["id"]), json!({"id": prop("string", "")})),
            run: |_p, _a| Err(Error::Other("todo_transfer is phase 2".to_string())) },

        // ─── scratchpads ─────────────────────────────────────────
        Tool { name: "scratchpad_list", desc: "List scratchpads.",
            schema: obj(Value::Null, json!({"query": prop("string", ""), "tags": arr(""), "include_archived": prop("boolean", ""), "offset": prop("integer", ""), "limit": prop("integer", "")})),
            run: |p, a| val(p.list_scratchpads(&a.tags(), &a.query, a.include_archived, a.offset, a.limit)?) },
        Tool { name: "scratchpad_read", desc: "Read a scratchpad (full/content/headings/section) with revision.",
            schema: obj(req(&["id"]), json!({"id": prop("string", ""), "mode": prop("string", "full|content|headings|section"), "section_heading": prop("string", ""), "offset": prop("integer", ""), "limit": prop("integer", "")})),
            run: |p, a| {
                let (s, text) = p.read_scratchpad(&a.id, or_default(&a.mode, "full"), &a.section_heading, a.offset, a.limit)?;
                Ok(json!({"scratchpad": serde_json::to_value(s)?, "text": text}))
            } },
        Tool { name: "scratchpad_write", desc: "Create a scratchpad, or replace an existing one's content/tags at a revision.",
            schema: obj(req(&["content"]), json!({"id": prop("string", "omit to create"), "content": prop("string", ""), "tags": arr(""), "expected_revision": prop("integer", "required when id given")})),
            run: |p, a| {
                if a.id.is_empty() {
                    return val(p.create_scratchpad(&a.name, &a.content, a.tags())?);
                }
                require_revision(a, "scratchpad_write")?;
                val(p.update_scratchpad(&a.id, a.rev(), None, Some(&a.content), a.tags.clone())?)
            } },
        Tool { name: "scratchpad_rename", desc: "Rename at an expected revision.",
            schema: obj(req(&["id", "name", "expected_revision"]), json!({"id": prop("string", ""), "name": prop("string", ""), "expected_revision": prop("integer", "")})),
            run: |p, a| {
                require_revision(a, "scratchpad_rename")?;
                val(p.rename_scratchpad(&a.id, &a.name, a.rev())?)
            } },
        Tool { name: "scratchpad_add_tags", desc: "Add tags in one revision bump.",
            schema: obj(req(&["id", "tags", "expected_revision"]), json!({"id": prop("string", ""), "tags": arr(""), "expected_revision": prop("integer", "")})),
            run: |p, a| {
                require_revision(a, "scratchpad_add_tags")?;
                let (s, _) = p.read_scratchpad(&a.id, "full", "", 0, 0)?;
                let mut merged = s.tags.clone();
                for t in a.tags() {
                    if !merged.contains(&t) {
                        merged.push(t);
                    }
                }
                val(p.update_scratchpad(&a.id, a.rev(), None, None, Some(merged))?)
            } },
        Tool { name: "scratchpad_remove_tags", desc: "Remove tags in one revision bump.",
            schema: obj(req(&["id", "tags", "expected_revision"]), json!({"id": prop("string", ""), "tags": arr(""), "expected_revision": prop("integer", "")})),
            run: |p, a| {
                require_revision(a, "scratchpad_remove_tags")?;
                let (s, _) = p.read_scratchpad(&a.id, "full", "", 0, 0)?;
                let drop = a.tags();
                let keep: Vec<String> = s.tags.into_iter().filter(|t| !drop.contains(t)).collect();
                val(p.update_scratchpad(&a.id, a.rev(), None, None, Some(keep))?)
            } },
        Tool { name: "scratchpad_append", desc: "Append content to the end.",
            schema: obj(req(&["id", "content"]), json!({"id": prop("string", ""), "content": prop("string", ""), "expected_revision": prop("integer", "optional guard"), "newline": prop("boolean", "")})),
            run: |p, a| val(p.append_scratchpad(&a.id, &a.content, a.rev(), a.newline)?) },
        Tool { name: "scratchpad_append_section", desc: "Append under an existing heading.",
            schema: obj(req(&["id", "heading", "content"]), json!({"id": prop("string", ""), "heading": prop("string", ""), "content": prop("string", ""), "expected_revision": prop("integer", "")})),
            run: |p, a| val(p.append_section(&a.id, &a.heading, &a.content, a.rev())?) },
        Tool { name: "scratchpad_edit", desc: "Replace a section or a line range.",
            schema: obj(req(&["id", "target", "content", "expected_revision"]), json!({"id": prop("string", ""), "target": prop("object", r#"{"type":"section","section_heading":".."} or {"type":"line_range","offset":0,"limit":1}"#), "content": prop("string", ""), "expected_revision": prop("integer", "")})),
            run: |p, a| {
                require_revision(a, "scratchpad_edit")?;
                val(p.edit_scratchpad(&a.id, a.target.clone(), &a.content, a.rev())?)
            } },
        Tool { name: "scratchpad_find", desc: "Search a scratchpad for a literal substring.",
            schema: obj(req(&["id", "query"]), json!({"id": prop("string", ""), "query": prop("string", ""), "scope": prop("string", "headings|content|all"), "case_sensitive": prop("boolean", ""), "context_lines": prop("integer", "")})),
            run: |p, a| val(p.find_in_scratchpad(&a.id, &a.query, or_default(&a.scope, "all"), a.case_sensitive, a.context_lines)?) },
        Tool { name: "scratchpad_tail", desc: "Return the last N lines with total line count.",
            schema: obj(req(&["id"]), json!({"id": prop("string", ""), "lines": prop("integer", "default 10; 0 = metadata only")})),
            run: |p, a| {
                let n = a.lines.unwrap_or(10);
                let (text, total) = p.tail_scratchpad(&a.id, n)?;
                Ok(json!({"text": text, "total_lines": total}))
            } },
        Tool { name: "scratchpad_clear", desc: "Erase content.",
            schema: obj(req(&["id", "expected_revision"]), json!({"id": prop("string", ""), "expected_revision": prop("integer", "")})),
            run: |p, a| {
                require_revision(a, "scratchpad_clear")?;
                val(p.clear_scratchpad(&a.id, a.rev())?)
            } },
        Tool { name: "scratchpad_archive", desc: "Archive (hide without deleting).",
            schema: obj(req(&["id", "expected_revision"]), json!({"id": prop("string", ""), "expected_revision": prop("integer", "")})),
            run: |p, a| {
                require_revision(a, "scratchpad_archive")?;
                val(p.archive_scratchpad(&a.id, a.rev())?)
            } },
        Tool { name: "scratchpad_delete", desc: "Delete permanently.",
            schema: obj(req(&["id", "expected_revision"]), json!({"id": prop("string", ""), "expected_revision": prop("integer", "")})),
            run: |p, a| {
                require_revision(a, "scratchpad_delete")?;
                p.delete_scratchpad(&a.id, a.rev())?;
                Ok(Value::Null)
            } },
        Tool { name: "scratchpad_transfer", desc: "Not supported cross-project in v1.",
            schema: obj(req(&["id"]), json!({"id": prop("string", "")})),
            run: |_p, _a| Err(Error::Other("scratchpad_transfer is phase 2".to_string())) },
        Tool { name: "scratchpad_tags_list", desc: "List distinct scratchpad tags.",
            schema: obj(Value::Null, json!({})),
            run: |p, _a| val(p.scratchpad_tags()?) },
        Tool { name: "scratchpad_save_to_file", desc: "Write a scratchpad to a filesystem path.",
            schema: obj(req(&["id", "path"]), json!({"id": prop("string", ""), "path": prop("string", "")})),
            run: |p, a| { p.save_scratchpad_to_file(&a.id, &a.path)?; Ok(Value::Null) } },
        Tool { name: "scratchpad_load_from_file", desc: "Load a scratchpad from a file.",
            schema: obj(req(&["path"]), json!({"path": prop("string", "")})),
            run: |p, a| val(p.load_scratchpad_from_file(&a.path)?) },
        // ─── comments ────────────────────────────────────────────
        Tool { name: "comment_add", desc: "Add a comment to a todo (t_…), scratchpad (s_…), or plan (rel_path). section is an optional heading to anchor to; empty = whole item.",
            schema: obj(req(&["id", "body"]), json!({"id": prop("string", "target: t_… | s_… | plan rel_path"), "body": prop("string", "comment text"), "section": prop("string", "heading to anchor to; empty = whole item")})),
            run: |p, a| val(p.add_comment(&a.id, &a.section, &a.body)?) },
        Tool { name: "comment_list", desc: "List comments on a todo, scratchpad, or plan.",
            schema: obj(req(&["id"]), json!({"id": prop("string", "target: t_… | s_… | plan rel_path")})),
            run: |p, a| val(p.list_comments(&a.id)?) },
        Tool { name: "comment_delete", desc: "Delete a comment by its id.",
            schema: obj(req(&["id"]), json!({"id": prop("string", "comment id (c_…)")})),
            run: |p, a| { p.delete_comment(&a.id)?; Ok(Value::Null) } },
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
    ]
}

/// tools/list payload: the declarative slice of the registry (Go's toolDefs()).
pub(super) fn tool_defs() -> Value {
    Value::Array(
        registry()
            .into_iter()
            .map(|t| json!({"name": t.name, "description": t.desc, "inputSchema": t.schema}))
            .collect(),
    )
}

/// Port of dispatchTool: parse args, resolve the project, run the one tool.
pub(super) fn dispatch_tool(resolve: &Resolve, name: &str, args: &Value) -> Result<Value> {
    let reg = registry();
    let t = reg
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| Error::Other(format!("unknown tool: {name}")))?;
    let a: Args = if args.is_null() {
        Args::default()
    } else {
        serde_json::from_value(args.clone())
            .map_err(|e| Error::Other(format!("bad arguments: {e}")))?
    };
    let proj = if a.project.is_empty() {
        None
    } else {
        Some(a.project.as_str())
    };
    let p = resolve(proj)?;
    (t.run)(&p, &a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store;
    use crate::store::testutil::{TempDir, git_repo};

    // JSON Schema requires `required` to be an array when present. An empty
    // required set must OMIT the key, never emit `null` — a null makes Claude
    // Code reject the entire tools list ("inputSchema.required expected an
    // array but got null"), so an installed tally MCP exposes no tools.
    #[test]
    fn no_tool_emits_null_required() {
        let defs = tool_defs();
        for t in defs.as_array().expect("tool_defs is an array") {
            let name = t["name"].as_str().unwrap_or("<unnamed>");
            if let Some(req) = t["inputSchema"].get("required") {
                assert!(
                    req.is_array(),
                    "tool {name}: inputSchema.required must be an array when present, got {req}"
                );
            }
        }
    }

    // A resolver over a fresh temp git repo + temp store root. Holds the guards
    // so both dirs outlive every dispatch call in a test. Go used chdir +
    // XDG_STATE_HOME (mcpRepo); we inject the resolver to stay parallel-safe.
    struct Env {
        _repo: TempDir,
        _root: TempDir,
        repo_str: String,
        root: std::path::PathBuf,
    }

    impl Env {
        fn new() -> Env {
            let repo = git_repo();
            let root = TempDir::new();
            let repo_str = repo.path().to_string_lossy().into_owned();
            let root_path = root.path().to_path_buf();
            Env {
                _repo: repo,
                _root: root,
                repo_str,
                root: root_path,
            }
        }

        fn call(&self, name: &str, raw: &str) -> Result<Value> {
            let resolve =
                |o: Option<&str>| store::resolve_project_in(&self.root, o.or(Some(&self.repo_str)));
            let v: Value = serde_json::from_str(raw).unwrap();
            dispatch_tool(&resolve, name, &v)
        }
    }

    // Port of TestDispatchTodoCreateThenList.
    #[test]
    fn test_dispatch_todo_create_then_list() {
        let e = Env::new();
        e.call("todo_create", r#"{"title":"via mcp","priority":"high"}"#)
            .unwrap();
        let res = e.call("todo_list", r#"{"status":"open"}"#).unwrap();
        let b = serde_json::to_string(&res).unwrap();
        assert!(b.contains("via mcp"), "list: {b}");
    }

    #[test]
    fn test_todo_update_github_param() {
        let e = Env::new();
        // origin on the temp repo so linking resolves
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(e._repo.path())
            .args(["remote", "add", "origin", "git@github.com:owner/name.git"])
            .output()
            .unwrap();
        assert!(out.status.success());

        let created = e.call("todo_create", r#"{"title":"x"}"#).unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        let on = e
            .call("todo_update", &format!(r#"{{"id":"{id}","github":"on"}}"#))
            .unwrap();
        assert_eq!(on["github"]["repo"].as_str(), Some("owner/name"));
        assert_eq!(on["github"]["paused"].as_bool(), Some(false));
        let off = e
            .call("todo_update", &format!(r#"{{"id":"{id}","github":"off"}}"#))
            .unwrap();
        assert_eq!(off["github"]["paused"].as_bool(), Some(true));
        // bogus value errors
        assert!(
            e.call(
                "todo_update",
                &format!(r#"{{"id":"{id}","github":"maybe"}}"#)
            )
            .is_err()
        );
    }

    // Port of TestDispatchScratchpadWriteRevisionGuard.
    #[test]
    fn test_dispatch_scratchpad_write_revision_guard() {
        let e = Env::new();
        let res = e
            .call("scratchpad_write", "{\"content\":\"# hi\\nbody\"}")
            .unwrap();
        let id = res["id"].as_str().expect("write returned no id");
        assert_eq!(
            res["revision"].as_i64(),
            Some(1),
            "want revision 1 after create"
        );

        let wrong =
            format!(r#"{{"id":"{id}","content":"should not apply","expected_revision":99}}"#);
        assert!(
            e.call("scratchpad_write", &wrong).is_err(),
            "wrong expected_revision should error"
        );

        let right = format!(r#"{{"id":"{id}","content":"updated body","expected_revision":1}}"#);
        let res2 = e.call("scratchpad_write", &right).unwrap();
        assert_eq!(
            res2["revision"].as_i64(),
            Some(2),
            "want revision bumped to 2"
        );
    }

    #[test]
    fn test_dispatch_comment_add_list_delete() {
        let e = Env::new();
        let add = e
            .call(
                "comment_add",
                r#"{"id":"t_x","body":"hi","section":"Phase 1"}"#,
            )
            .unwrap();
        assert_eq!(add["section"].as_str(), Some("Phase 1"));
        assert_eq!(add["text"].as_str(), Some("hi"));
        assert_eq!(add["target"].as_str(), Some("t_x"));
        let cid = add["id"].as_str().unwrap().to_string();
        let list = e.call("comment_list", r#"{"id":"t_x"}"#).unwrap();
        assert!(serde_json::to_string(&list).unwrap().contains("hi"));
        e.call("comment_delete", &format!(r#"{{"id":"{cid}"}}"#))
            .unwrap();
        let empty = e.call("comment_list", r#"{"id":"t_x"}"#).unwrap();
        assert_eq!(empty.as_array().map(|a| a.len()), Some(0));
    }

    // Port of TestToolDefsCount — plus the exact tool count the port targets.
    #[test]
    fn test_tool_defs_count() {
        let defs = tool_defs();
        let n = defs.as_array().unwrap().len();
        assert!(n >= 30, "want the full tool set, got {n}");
        assert_eq!(
            n, 38,
            "expected exactly 38 tools (todo_* + scratchpad_* + comment_*), got {n}"
        );
    }

    // Port of TestUnknownTool.
    #[test]
    fn test_unknown_tool() {
        let e = Env::new();
        assert!(e.call("nope", "{}").is_err(), "unknown tool should error");
    }

    // Port of TestScratchpadTailMetadataOnly.
    #[test]
    fn test_scratchpad_tail_metadata_only() {
        let e = Env::new();
        let res = e
            .call(
                "scratchpad_write",
                "{\"content\":\"line1\\nline2\\nline3\\nline4\\nline5\"}",
            )
            .unwrap();
        let id = res["id"].as_str().unwrap().to_string();

        let meta = e
            .call("scratchpad_tail", &format!(r#"{{"id":"{id}","lines":0}}"#))
            .unwrap();
        assert_eq!(
            meta["text"].as_str(),
            Some(""),
            "want empty text for lines:0"
        );
        assert_eq!(meta["total_lines"].as_i64(), Some(5), "want total_lines 5");

        let full = e
            .call("scratchpad_tail", &format!(r#"{{"id":"{id}"}}"#))
            .unwrap();
        assert!(
            !full["text"].as_str().unwrap().is_empty(),
            "want non-empty text when lines is omitted"
        );
    }

    // Port of TestScratchpadEditRequiresExpectedRevision.
    #[test]
    fn test_scratchpad_edit_requires_expected_revision() {
        let e = Env::new();
        let res = e
            .call("scratchpad_write", "{\"content\":\"a\\nb\\nc\\n\"}")
            .unwrap();
        let id = res["id"].as_str().unwrap().to_string();

        let no_rev = format!(
            r#"{{"id":"{id}","target":{{"type":"line_range","offset":1,"limit":1}},"content":"B"}}"#
        );
        assert!(
            e.call("scratchpad_edit", &no_rev).is_err(),
            "edit without expected_revision should error"
        );

        let ok = format!(
            r#"{{"id":"{id}","target":{{"type":"line_range","offset":1,"limit":1}},"content":"B","expected_revision":1}}"#
        );
        e.call("scratchpad_edit", &ok)
            .expect("edit with correct expected_revision");
    }

    // Port of TestScratchpadAddTagsNoDuplicate.
    #[test]
    fn test_scratchpad_add_tags_no_duplicate() {
        let e = Env::new();
        let res = e
            .call("scratchpad_write", r#"{"content":"hi","tags":["a"]}"#)
            .unwrap();
        let id = res["id"].as_str().unwrap().to_string();

        e.call(
            "scratchpad_add_tags",
            &format!(r#"{{"id":"{id}","tags":["a","b"],"expected_revision":1}}"#),
        )
        .unwrap();

        let read = e
            .call("scratchpad_read", &format!(r#"{{"id":"{id}"}}"#))
            .unwrap();
        let tags = read["scratchpad"]["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2, "want tags [a b], got {tags:?}");
        assert_eq!(tags[0], "a");
        assert_eq!(tags[1], "b");
    }

    #[test]
    fn test_dispatch_comment_recent() {
        let e = Env::new();
        e.call("comment_add", r#"{"id":"t_x","body":"one"}"#)
            .unwrap();
        e.call("comment_add", r#"{"id":"t_y","body":"two"}"#)
            .unwrap();
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

    // The todo_update empty-string-means-unchanged quirk (CLAUDE.md invariant).
    #[test]
    fn test_todo_update_empty_string_preserves_field() {
        let e = Env::new();
        let created = e
            .call("todo_create", r#"{"title":"keep me","body":"orig"}"#)
            .unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        // Update only the body; empty title must NOT clear the title.
        let updated = e
            .call("todo_update", &format!(r#"{{"id":"{id}","body":"new"}}"#))
            .unwrap();
        assert_eq!(updated["title"], "keep me", "empty title cleared the field");
        assert_eq!(updated["body"], "new");
    }
}
