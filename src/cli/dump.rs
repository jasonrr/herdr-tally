//! `tally dump [--json]`: read the WHOLE store (all todos, all comments, all
//! scratchpads including archived) and print it. Restores the inspectability
//! `cat todos.json` gave before the store became a binary automerge blob —
//! thin adapter: parse flags, call `Project::dump`, serialize.
use std::io::Write;
use std::path::Path;

use super::{fail, parse, print_json, project_opt, resolve};

const BOOL_FLAGS: &[&str] = &["json"];
const VALUE_FLAGS: &[&str] = &["project"];
const INT_FLAGS: &[&str] = &[];

pub(crate) fn run(args: &[String], store_root: Option<&Path>, out: &mut dyn Write) -> i32 {
    let p = match parse(args, BOOL_FLAGS, VALUE_FLAGS, INT_FLAGS) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };

    let project = p.str("project", "");
    let proj = match resolve(project_opt(&project), store_root) {
        Ok(p) => p,
        Err(e) => return fail(&e.to_string()),
    };

    let dump = match proj.dump() {
        Ok(d) => d,
        Err(e) => return fail(&e.to_string()),
    };

    if p.boolean("json", false) {
        if let Err(e) = print_json(out, &dump) {
            return fail(&e.to_string());
        }
        return 0;
    }

    if let Err(e) = write_human(out, &dump) {
        return fail(&e.to_string());
    }
    0
}

/// Terse human summary: three counts, plus todo titles and pad titles so the
/// output is actually useful for eyeballing the store, not just its shape.
fn write_human(out: &mut dyn Write, dump: &crate::store::StoreDump) -> std::io::Result<()> {
    writeln!(out, "todos: {}", dump.todos.len())?;
    for t in &dump.todos {
        writeln!(out, "  [{}] {} ({})", t.status, t.title, t.id)?;
    }
    writeln!(out, "comments: {}", dump.comments.len())?;
    writeln!(out, "scratchpads: {}", dump.scratchpads.len())?;
    for s in &dump.scratchpads {
        writeln!(out, "  {} ({})", s.title, s.id)?;
    }
    Ok(())
}
