//! Port of internal/cli/render.go — glow-friendly markdown for the human-facing
//! (non-`--json`) list output. Todos are grouped by status; scratchpads are a
//! flat list. Byte-for-byte the same document the Go renderer emitted.
use std::io::{self, Write};

use crate::store::{Comment, CommentSummary, Project, Scratchpad, Todo};

const STATUS_ORDER: [&str; 4] = ["in_progress", "open", "backlog", "completed"];

fn status_label(s: &str) -> &str {
    match s {
        "in_progress" => "In progress",
        "open" => "Open",
        "backlog" => "Backlog",
        "completed" => "Completed",
        _ => s,
    }
}

fn prio_mark(p: &str) -> &str {
    match p {
        "p0" => "🔴",
        "p1" => "🟠",
        "p2" => "🟡",
        "p3" => "⚪",
        _ => "",
    }
}

/// Go's fmt.Sprint on a []string renders as "[a b]".
fn sprint_tags(tags: &[String]) -> String {
    format!("[{}]", tags.join(" "))
}

pub(crate) fn render_todos(out: &mut dyn Write, p: &Project, todos: &[Todo]) -> io::Result<()> {
    writeln!(out, "# {} — todos\n", p.name)?;
    for s in STATUS_ORDER {
        let group: Vec<&Todo> = todos.iter().filter(|t| t.status == s).collect();
        if group.is_empty() {
            continue;
        }
        writeln!(out, "## {} ({})\n", status_label(s), group.len())?;
        for t in group {
            let checkbox = if t.status == "completed" { "x" } else { " " };
            let mut line = format!("- [{}] {} {}", checkbox, prio_mark(&t.priority), t.title);
            if p.is_blocked(t) {
                line.push_str(" ⛔");
            }
            if t.lock.is_some() {
                line.push_str(" 🔒");
            }
            if !t.tags.is_empty() {
                line.push_str("  `");
                line.push_str(&sprint_tags(&t.tags));
                line.push('`');
            }
            writeln!(out, "{line}  \n  <sub>{}</sub>", t.id)?;
        }
        writeln!(out)?;
    }
    if todos.is_empty() {
        writeln!(out, "_No todos yet._")?;
    }
    Ok(())
}

pub(crate) fn render_scratchpads(
    out: &mut dyn Write,
    p: &Project,
    pads: &[Scratchpad],
) -> io::Result<()> {
    writeln!(out, "# {} — scratchpads\n", p.name)?;
    for s in pads {
        let archived = if s.status == "archived" {
            " _(archived)_"
        } else {
            ""
        };
        writeln!(
            out,
            "- **{}**{}  `{}`  \n  <sub>{} · rev {} · {}</sub>",
            s.title,
            archived,
            sprint_tags(&s.tags),
            s.id,
            s.revision,
            s.updated
        )?;
    }
    if pads.is_empty() {
        writeln!(out, "_No scratchpads yet._")?;
    }
    Ok(())
}

pub(crate) fn render_comments(out: &mut dyn Write, comments: &[Comment]) -> io::Result<()> {
    if comments.is_empty() {
        writeln!(out, "_No comments yet._")?;
        return Ok(());
    }
    for c in comments {
        let anchor = if c.section.is_empty() {
            String::new()
        } else {
            format!(" · {}", c.section)
        };
        // events read as log lines; notes read as authored comments
        if c.kind == "event" {
            writeln!(
                out,
                "- ⋯ _{}_ — {}  \n  <sub>{}</sub>",
                c.text, c.author, c.created
            )?;
        } else {
            writeln!(
                out,
                "- **{}**{anchor} — {}  \n  <sub>{} · {}</sub>",
                c.author, c.text, c.created, c.id
            )?;
        }
    }
    Ok(())
}

/// Newest-first flat list; each row prefixed with its resolved target label.
pub(crate) fn render_recent_comments(
    out: &mut dyn Write,
    rows: &[(String, &Comment)],
) -> io::Result<()> {
    if rows.is_empty() {
        writeln!(out, "_No comments yet._")?;
        return Ok(());
    }
    for (label, c) in rows {
        let anchor = if c.section.is_empty() {
            String::new()
        } else {
            format!(" · {}", c.section)
        };
        // events read as log lines; notes read as authored comments
        if c.kind == "event" {
            writeln!(
                out,
                "- {label} — ⋯ _{}_ — {}  \n  <sub>{}</sub>",
                c.text, c.author, c.created
            )?;
        } else {
            writeln!(
                out,
                "- {label} — **{}**{anchor}: {}  \n  <sub>{} · {}</sub>",
                c.author, c.text, c.created, c.id
            )?;
        }
    }
    Ok(())
}

/// Per-target view: "☐ Fix auth · 💬2 · last: "hold off"".
pub(crate) fn render_comment_summaries(
    out: &mut dyn Write,
    rows: &[(String, &CommentSummary)],
) -> io::Result<()> {
    if rows.is_empty() {
        writeln!(out, "_No comments yet._")?;
        return Ok(());
    }
    for (label, s) in rows {
        writeln!(out, "- {label} · 💬{} · last: \"{}\"", s.count, s.latest)?;
    }
    Ok(())
}
