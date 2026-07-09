//! Read-mode markdown rendering: tui-markdown with a glamour-"dark"-like
//! style sheet (pink headings, warm inline code, cyan links), matching the
//! look the Go TUI got from glamour's "dark" theme.
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use tui_markdown::{Options, StyleSheet};

#[derive(Clone)]
struct GlamourDark;

impl StyleSheet for GlamourDark {
    fn heading(&self, level: u8) -> Style {
        let pink = Color::Rgb(0xff, 0x87, 0xd7);
        match level {
            1 => Style::new()
                .fg(Color::Rgb(0x1d, 0x1d, 0x1d))
                .bg(pink)
                .add_modifier(Modifier::BOLD),
            2 => Style::new().fg(pink).add_modifier(Modifier::BOLD),
            _ => Style::new().fg(pink),
        }
    }

    fn code(&self) -> Style {
        Style::new()
            .fg(Color::Rgb(0xff, 0x5f, 0x87))
            .bg(Color::Rgb(0x2a, 0x2a, 0x2a))
    }

    fn link(&self) -> Style {
        Style::new()
            .fg(Color::Cyan)
            .add_modifier(Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::Gray).add_modifier(Modifier::ITALIC)
    }

    fn heading_meta(&self) -> Style {
        Style::new().fg(Color::DarkGray)
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::DarkGray)
    }
}

/// Renders markdown to an owned Text so the app can cache it (tui-markdown
/// returns a Text borrowing the input string).
///
/// `neutralize_task_lists` runs first: tui-markdown 0.3.8 panics rendering loose
/// GFM task lists (the shape every plan/spec doc uses), which would kill the
/// pane — rewriting the markers to plain checkbox glyphs sidesteps the bug.
pub fn render(body: &str) -> Text<'static> {
    let body = neutralize_task_lists(&reformat_tables(body));
    owned(tui_markdown::from_str_with_options(
        &body,
        &Options::new(GlamourDark),
    ))
}

/// Rewrites GFM task-list markers (`- [ ]` / `- [x]`) into plain list items with
/// a checkbox glyph. tui-markdown 0.3.8 panics rendering task-list markers in
/// loose lists (`line.spans.insert(1, ..)` into an empty span vec, lib.rs:469),
/// and plan/spec docs are full of them. Neutralizing the marker means
/// pulldown-cmark never emits the TaskListMarker event that trips the bug, and
/// the item still reads as a checkbox.
fn neutralize_task_lists(body: &str) -> String {
    body.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            for bullet in ['-', '*', '+'] {
                let Some(after) = trimmed
                    .strip_prefix(bullet)
                    .and_then(|r| r.strip_prefix(' '))
                else {
                    continue;
                };
                if let Some(rest) = after.strip_prefix("[ ]") {
                    return format!("{indent}- ☐ {}", rest.trim_start());
                }
                if let Some(rest) = after
                    .strip_prefix("[x]")
                    .or_else(|| after.strip_prefix("[X]"))
                {
                    return format!("{indent}- ☑ {}", rest.trim_start());
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn split_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

/// A GFM delimiter row: every cell is dashes with optional leading/trailing
/// colon (e.g. `---`, `:--`, `--:`, `:-:`).
fn is_delim(line: &str) -> bool {
    if !line.contains('-') {
        return false;
    }
    let cells = split_cells(line);
    !cells.is_empty()
        && cells.iter().all(|c| {
            let c = c.trim();
            !c.is_empty() && c.contains('-') && c.chars().all(|ch| ch == '-' || ch == ':')
        })
}

/// Rewrites GFM table blocks (header row + delimiter row + body rows) as
/// space-padded monospace columns joined by ` │ `, leaving all other lines
/// untouched. ponytail: alignment markers are ignored (all left-aligned);
/// wrapping of tables wider than the pane is left to the Paragraph. Not
/// code-fence aware — same known quirk as the rest of the parser.
pub(crate) fn reformat_tables(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].contains('|') && i + 1 < lines.len() && is_delim(lines[i + 1]) {
            let ncol = split_cells(lines[i]).len();
            let mut block: Vec<Vec<String>> = vec![split_cells(lines[i])];
            let mut j = i + 2;
            while j < lines.len() && lines[j].contains('|') && !lines[j].trim().is_empty() {
                block.push(split_cells(lines[j]));
                j += 1;
            }
            let mut w = vec![0usize; ncol];
            for row in &block {
                for (c, cell) in row.iter().take(ncol).enumerate() {
                    w[c] = w[c].max(cell.chars().count());
                }
            }
            let fmt_row = |row: &[String]| {
                let mut s = String::new();
                for (c, &width) in w.iter().enumerate() {
                    if c > 0 {
                        s.push_str(" │ ");
                    }
                    let cell = row.get(c).map(String::as_str).unwrap_or("");
                    s.push_str(cell);
                    s.push_str(&" ".repeat(width.saturating_sub(cell.chars().count())));
                }
                s.trim_end().to_string()
            };
            out.push(fmt_row(&block[0]));
            let mut sep = String::new();
            for (c, &width) in w.iter().enumerate() {
                if c > 0 {
                    sep.push_str("─┼─");
                }
                sep.push_str(&"─".repeat(width));
            }
            out.push(sep);
            for row in &block[1..] {
                out.push(fmt_row(row));
            }
            i = j;
        } else {
            out.push(lines[i].to_string());
            i += 1;
        }
    }
    out.join("\n")
}

fn owned(t: Text<'_>) -> Text<'static> {
    let lines = t
        .lines
        .into_iter()
        .map(|l| {
            let spans: Vec<Span<'static>> = l
                .spans
                .into_iter()
                .map(|s| Span::styled(s.content.into_owned(), s.style))
                .collect();
            let mut line = Line::from(spans).style(l.style);
            line.alignment = l.alignment;
            line
        })
        .collect::<Vec<_>>();
    let mut out = Text::from(lines).style(t.style);
    out.alignment = t.alignment;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reformat_tables_aligns_columns() {
        let input = "before\n\n| Name | Qty |\n|---|---:|\n| apples | 3 |\n| pears | 12 |\n\nafter";
        let out = reformat_tables(input);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "before");
        assert_eq!(lines[2], "Name   │ Qty"); // header padded to widest cell
        assert_eq!(lines[3], "───────┼────"); // separator sized to columns
        assert_eq!(lines[4], "apples │ 3");
        assert_eq!(lines[5], "pears  │ 12");
        assert_eq!(*lines.last().unwrap(), "after");
    }

    #[test]
    fn reformat_tables_leaves_nontables_untouched() {
        let input = "# Heading\n\ntext | with a pipe but no delimiter row\n";
        assert_eq!(reformat_tables(input), input.trim_end_matches('\n'));
    }

    #[test]
    fn neutralize_task_lists_rewrites_markers() {
        let md = "- [ ] open\n- [x] done\n  - [X] nested done\n* [ ] star bullet\n";
        let out = neutralize_task_lists(md);
        assert!(out.contains("- ☐ open"));
        assert!(out.contains("- ☑ done"));
        assert!(
            out.contains("  - ☑ nested done"),
            "indent preserved: {out:?}"
        );
        assert!(out.contains("- ☐ star bullet"));
        assert!(!out.contains("[ ]") && !out.contains("[x]") && !out.contains("[X]"));
        // non-task lines untouched
        assert_eq!(neutralize_task_lists("- plain\ntext"), "- plain\ntext");
    }

    #[test]
    fn render_does_not_panic_on_loose_task_list() {
        // Loose GFM task list (blank line between items) crashes tui-markdown
        // 0.3.8 raw; render must neutralize so it never panics.
        let md = "- [ ] a\n\n- [x] b\n";
        let r = std::panic::catch_unwind(|| {
            render(md);
        });
        assert!(r.is_ok(), "render must not panic on a loose task list");
    }
}
