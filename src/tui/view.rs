//! Rendering + mouse hit-test geometry. Every draw records the regions it
//! painted into `Hits`, and app.rs resolves clicks against that — hit ranges
//! are derived from the rendered strings/layout, never hardcoded column math
//! (the Go tabAtX had to be kept in sync with tabBar by hand; here they can't
//! desync).
use edtui::{EditorTheme, EditorView};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use super::app::{App, Focus, Mode, Tab};

const TAB_LABELS: [&str; 3] = ["1 Todos", "2 Scratchpads", "3 Plans"];
const TAB_PREFIX: &str = "  ";
const TAB_GAP: &str = "    ";

/// Column ranges (start, exclusive end) of the three tab labels on the tab
/// bar row, derived from the label strings themselves.
pub fn tab_ranges() -> [(u16, u16); 3] {
    let mut out = [(0u16, 0u16); 3];
    let mut x = TAB_PREFIX.chars().count() as u16;
    for (i, label) in TAB_LABELS.iter().enumerate() {
        out[i] = (x, x + label.chars().count() as u16);
        x = out[i].1 + TAB_GAP.chars().count() as u16;
    }
    out
}

/// The compact metadata row for a todo's detail/edit view. Only status and
/// priority appear — the fields the TUI can actually change.
pub fn meta_line(status: &str, priority: &str) -> String {
    format!("○ {status}   ‖ {priority}")
}

/// Segment boundaries within meta_line, as char columns relative to its start:
/// (status_end, prio_start, prio_end). Derived from the same strings the row
/// renders, so the click targets can't drift from the pixels.
pub fn meta_segments(status: &str, priority: &str) -> (u16, u16, u16) {
    let status_end = ("○ ".chars().count() + status.chars().count()) as u16;
    let prio_start = status_end + "   ".chars().count() as u16;
    let prio_end = prio_start + ("‖ ".chars().count() + priority.chars().count()) as u16;
    (status_end, prio_start, prio_end)
}

/// Hit-test regions recorded by the last draw.
#[derive(Default)]
pub struct Hits {
    pub tab_row: u16,
    pub tabs: [(u16, u16); 3],
    pub list: Option<ListHits>,
    pub meta: Option<MetaHits>,
    pub title_card: Option<Rect>,
    pub body_card: Option<Rect>,
    /// Height of the read-mode body viewport (for page scrolling).
    pub body_h: u16,
}

pub struct ListHits {
    pub area: Rect,
    /// First visible row index (cursor-follow scrolling).
    pub offset: usize,
    pub len: usize,
}

pub struct MetaHits {
    pub x: u16,
    pub row: u16,
    status_end: u16,
    prio_start: u16,
    prio_end: u16,
}

impl MetaHits {
    pub fn new(x: u16, row: u16, status: &str, priority: &str) -> MetaHits {
        let (status_end, prio_start, prio_end) = meta_segments(status, priority);
        MetaHits {
            x,
            row,
            status_end,
            prio_start,
            prio_end,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetaSeg {
    Status,
    Priority,
}

impl Hits {
    pub fn tab_at(&self, x: u16, y: u16) -> Option<Tab> {
        if y != self.tab_row {
            return None;
        }
        for (i, (s, e)) in self.tabs.iter().enumerate() {
            if x >= *s && x < *e {
                return Some([Tab::Todos, Tab::Scratchpads, Tab::Plans][i]);
            }
        }
        None
    }

    pub fn list_row_at(&self, x: u16, y: u16) -> Option<usize> {
        let l = self.list.as_ref()?;
        if !l.area.contains(Position::new(x, y)) {
            return None;
        }
        let i = (y - l.area.y) as usize + l.offset;
        if i < l.len { Some(i) } else { None }
    }

    pub fn meta_seg_at(&self, x: u16, y: u16) -> Option<MetaSeg> {
        let m = self.meta.as_ref()?;
        if y != m.row || x < m.x {
            return None;
        }
        let rel = x - m.x;
        if rel < m.status_end {
            Some(MetaSeg::Status)
        } else if rel >= m.prio_start && rel < m.prio_end {
            Some(MetaSeg::Priority)
        } else {
            None
        }
    }
}

pub fn draw(app: &mut App, f: &mut Frame) {
    app.hits = Hits::default();
    let footer_h = if app.status.is_empty() { 1 } else { 2 };
    let [tab_area, content, footer_area] = Layout::vertical([
        Constraint::Length(2), // tab bar line + blank line
        Constraint::Min(0),
        Constraint::Length(footer_h),
    ])
    .areas(f.area());

    draw_tab_bar(app, f, tab_area);
    match app.mode {
        Mode::Read => draw_read(app, f, content),
        Mode::Edit | Mode::DiscardConfirm => draw_edit(app, f, content),
        _ => draw_list(app, f, content), // List, Confirm, Filter
    }
    draw_footer(app, f, footer_area);
}

fn draw_tab_bar(app: &mut App, f: &mut Frame, area: Rect) {
    let label = |i: usize, t: Tab| {
        if app.tab == t {
            Span::from(TAB_LABELS[i]).bold().underlined()
        } else {
            Span::from(TAB_LABELS[i]).dim()
        }
    };
    let line = Line::from(vec![
        Span::from(TAB_PREFIX),
        label(0, Tab::Todos),
        Span::from(TAB_GAP),
        label(1, Tab::Scratchpads),
        Span::from(TAB_GAP),
        label(2, Tab::Plans),
    ]);
    f.render_widget(line, area);
    app.hits.tab_row = area.y;
    app.hits.tabs = tab_ranges();
}

fn draw_list(app: &mut App, f: &mut Frame, area: Rect) {
    let mut list_area = area;
    if app.mode == Mode::Filter {
        let [filter_area, rest] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
        f.render_widget(Line::from(format!("  /{}", app.filter)), filter_area);
        list_area = rest;
    }

    let cursor = app.cursor[app.tab.idx()];
    let cursor_style = if app.mode == Mode::List {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new() // no highlight outside list mode (Go rowStyle)
    };
    let mut rows: Vec<Line> = Vec::new();
    match app.tab {
        Tab::Todos => {
            if app.todos.is_empty() {
                rows.push(Line::from("  No todos yet."));
            }
            for (i, t) in app.todos.iter().enumerate() {
                let glyph = if t.status == "completed" {
                    "☑"
                } else {
                    "☐"
                };
                let blocked = if app.blocked.get(i).copied().unwrap_or(false) {
                    " ⛔"
                } else {
                    ""
                };
                let row = format!("{glyph} [{}] {}{blocked}", t.priority, t.title);
                rows.push(styled_row(row, i == cursor, cursor_style));
            }
        }
        Tab::Scratchpads => {
            if app.pads.is_empty() {
                rows.push(Line::from("  No scratchpads yet."));
            }
            for (i, s) in app.pads.iter().enumerate() {
                rows.push(styled_row(
                    format!("• {}", s.title),
                    i == cursor,
                    cursor_style,
                ));
            }
        }
        Tab::Plans => {
            let vis = app.visible_plans();
            if vis.is_empty() {
                if app.filter.is_empty() {
                    rows.push(Line::from(
                        "  No plans found. Configure paths in $HERDR_PLUGIN_CONFIG_DIR/plan-paths.",
                    ));
                } else {
                    rows.push(Line::from(format!("  No plans match /{}", app.filter)));
                }
            }
            for (i, d) in vis.iter().enumerate() {
                rows.push(styled_row(
                    format!("• {}", d.rel_path),
                    i == cursor,
                    cursor_style,
                ));
            }
        }
    }
    let len = app.count();

    // keep the cursor row on screen (the Go pane just overflowed)
    let h = list_area.height as usize;
    let offset = if h > 0 && len > 0 && cursor >= h {
        cursor - h + 1
    } else {
        0
    };
    app.hits.list = Some(ListHits {
        area: list_area,
        offset,
        len,
    });

    if app.mode == Mode::Confirm {
        rows.push(Line::from(""));
        rows.push(Line::from("  Delete selected item? "));
    }
    f.render_widget(Paragraph::new(rows).scroll((offset as u16, 0)), list_area);
}

fn styled_row(row: String, selected: bool, cursor_style: Style) -> Line<'static> {
    if selected {
        Line::from(row).style(cursor_style)
    } else {
        Line::from(row)
    }
}

/// Title text for the detail view: the selected row's title (todos/pads) or
/// rel path (plans). None when the cursor points past the list (item vanished).
fn read_title(app: &App) -> Option<String> {
    let i = app.cursor[app.tab.idx()];
    match app.tab {
        Tab::Todos => app.todos.get(i).map(|t| t.title.clone()),
        Tab::Scratchpads => app.pads.get(i).map(|s| s.title.clone()),
        Tab::Plans => app.visible_plans().get(i).map(|d| d.rel_path.clone()),
    }
}

fn draw_read(app: &mut App, f: &mut Frame, area: Rect) {
    let Some(title) = read_title(app) else {
        return;
    };
    let title_par = Paragraph::new(title).bold().wrap(Wrap { trim: false });
    let title_h = title_par.line_count(area.width).max(1) as u16;

    let is_todo = app.tab == Tab::Todos;
    let mut constraints = vec![Constraint::Length(title_h), Constraint::Length(1)];
    if is_todo {
        constraints.push(Constraint::Length(1)); // meta row
        constraints.push(Constraint::Length(1)); // separator
    }
    constraints.push(Constraint::Min(0)); // body
    let parts = Layout::vertical(constraints).split(area);
    f.render_widget(title_par, parts[0]);

    let body_area = *parts.last().unwrap();
    if is_todo {
        let i = app.cursor[Tab::Todos.idx()];
        if let Some(t) = app.todos.get(i) {
            let meta_area = parts[2];
            f.render_widget(
                Line::from(meta_line(&t.status, &t.priority)).dim(),
                meta_area,
            );
            app.hits.meta = Some(MetaHits::new(
                meta_area.x,
                meta_area.y,
                &t.status,
                &t.priority,
            ));
        }
    }

    let body = Paragraph::new(app.read_text.clone()).wrap(Wrap { trim: false });
    let total = body.line_count(body_area.width) as u16;
    let max_scroll = total.saturating_sub(body_area.height);
    app.read_scroll = app.read_scroll.min(max_scroll);
    app.hits.body_h = body_area.height;
    f.render_widget(body.scroll((app.read_scroll, 0)), body_area);
}

fn card_theme(title: &'static str, focused: bool) -> EditorTheme<'static> {
    let border = if focused {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    let mut theme = EditorTheme::default()
        .base(Style::default()) // terminal default colors, not edtui's black
        .selection_style(Style::default().add_modifier(Modifier::REVERSED))
        .block(Block::bordered().title(title).border_style(border))
        .hide_status_line();
    theme = if focused {
        theme.cursor_style(Style::default().add_modifier(Modifier::REVERSED))
    } else {
        theme.hide_cursor()
    };
    theme
}

fn draw_edit(app: &mut App, f: &mut Frame, area: Rect) {
    let show_meta = app.tab == Tab::Todos && !app.edit_id.is_empty();
    let discard = app.mode == Mode::DiscardConfirm;
    let mut constraints = vec![Constraint::Length(4)]; // title card: 2 rows + border
    if show_meta {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(3)); // body card
    if discard {
        constraints.push(Constraint::Length(2));
    }
    let parts = Layout::vertical(constraints).split(area);
    let title_area = parts[0];
    let body_area = parts[if show_meta { 2 } else { 1 }];

    let title_view = EditorView::new(&mut app.title_ed)
        .theme(card_theme("Title", app.edit_focus == Focus::Title))
        .wrap(true);
    f.render_widget(title_view, title_area);

    if show_meta {
        let i = app.cursor[Tab::Todos.idx()];
        if let Some(t) = app.todos.get(i) {
            let meta_area = parts[1];
            f.render_widget(
                Line::from(meta_line(&t.status, &t.priority)).dim(),
                meta_area,
            );
            app.hits.meta = Some(MetaHits::new(
                meta_area.x,
                meta_area.y,
                &t.status,
                &t.priority,
            ));
        }
    }

    let body_view = EditorView::new(&mut app.body_ed)
        .theme(card_theme("Body", app.edit_focus == Focus::Body))
        .wrap(true);
    f.render_widget(body_view, body_area);

    app.hits.title_card = Some(title_area);
    app.hits.body_card = Some(body_area);

    if discard {
        f.render_widget(
            Paragraph::new("\n  Discard edits? (y/n) "),
            *parts.last().unwrap(),
        );
    }
}

fn draw_footer(app: &App, f: &mut Frame, area: Rect) {
    let mut lines = vec![Line::from(footer(app)).dim()];
    if !app.status.is_empty() {
        lines.push(Line::from(app.status.clone()));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn footer(app: &App) -> &'static str {
    match app.mode {
        Mode::List => match app.tab {
            Tab::Todos if app.hide_completed => {
                "↑↓ · enter read · n new · space done · e edit · p prio · c show done · d del · q"
            }
            Tab::Todos => {
                "↑↓ · enter · n new · space done · e edit · p prio · c hide done · d del · q"
            }
            Tab::Scratchpads => "↑↓ move · enter read · n new · e edit · d del · 1·2·3 · r · q",
            Tab::Plans => "↑↓ move · enter read · / filter · 1·2·3 · r · q",
        },
        Mode::Read => match app.tab {
            Tab::Todos => "space done · p prio · e edit · y yank · R raw · esc back",
            Tab::Scratchpads => "e edit · y yank · R raw · esc back",
            Tab::Plans => "y yank · R raw · esc back",
        },
        Mode::Confirm => "y confirm · n/esc cancel",
        Mode::Edit => match app.tab {
            Tab::Todos => "tab title/body · ctrl+d save · esc discard · ctrl+p prio · ctrl+t done",
            _ => "tab title/body · ctrl+d save · esc discard",
        },
        Mode::DiscardConfirm => "y discard · n/esc keep editing",
        Mode::Filter => "type to filter · enter apply · esc clear",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rendered tab bar as a plain string, built the same way
    /// draw_tab_bar assembles its spans.
    fn tab_bar_text() -> String {
        format!(
            "{TAB_PREFIX}{}{TAB_GAP}{}{TAB_GAP}{}",
            TAB_LABELS[0], TAB_LABELS[1], TAB_LABELS[2]
        )
    }

    #[test]
    fn tab_ranges_match_rendered_labels() {
        let bar: Vec<char> = tab_bar_text().chars().collect();
        for (i, (s, e)) in tab_ranges().iter().enumerate() {
            let got: String = bar[*s as usize..*e as usize].iter().collect();
            assert_eq!(got, TAB_LABELS[i], "range {i} out of sync with the bar");
        }
    }

    #[test]
    fn tab_at_hits_labels_and_misses_gaps() {
        let mut h = Hits {
            tabs: tab_ranges(),
            tab_row: 0,
            ..Hits::default()
        };
        // Go's tabAtX contract: 2..=8 Todos, 13..=25 Scratchpads, 30..=36 Plans
        // ("Plans" is one char longer than "Docs", so the tab's range grows by 1.)
        assert_eq!(h.tab_at(1, 0), None);
        assert_eq!(h.tab_at(2, 0), Some(Tab::Todos));
        assert_eq!(h.tab_at(8, 0), Some(Tab::Todos));
        assert_eq!(h.tab_at(9, 0), None);
        assert_eq!(h.tab_at(13, 0), Some(Tab::Scratchpads));
        assert_eq!(h.tab_at(25, 0), Some(Tab::Scratchpads));
        assert_eq!(h.tab_at(26, 0), None);
        assert_eq!(h.tab_at(30, 0), Some(Tab::Plans));
        assert_eq!(h.tab_at(36, 0), Some(Tab::Plans));
        assert_eq!(h.tab_at(37, 0), None);
        assert_eq!(h.tab_at(2, 1), None, "wrong row");
        h.tab_row = 5;
        assert_eq!(h.tab_at(2, 5), Some(Tab::Todos), "tab row offset respected");
    }

    #[test]
    fn meta_segments_match_rendered_line() {
        let (status, priority) = ("open", "high");
        let line: Vec<char> = meta_line(status, priority).chars().collect();
        let (status_end, prio_start, prio_end) = meta_segments(status, priority);
        let seg: String = line[..status_end as usize].iter().collect();
        assert_eq!(seg, "○ open");
        let seg: String = line[prio_start as usize..prio_end as usize]
            .iter()
            .collect();
        assert_eq!(seg, "‖ high");
        assert_eq!(prio_end as usize, line.len());
    }

    #[test]
    fn meta_seg_at_resolves_segments() {
        let m = MetaHits::new(0, 4, "open", "medium");
        let h = Hits {
            meta: Some(m),
            ..Hits::default()
        };
        assert_eq!(h.meta_seg_at(0, 4), Some(MetaSeg::Status)); // "○"
        assert_eq!(h.meta_seg_at(5, 4), Some(MetaSeg::Status)); // last char of "open"
        assert_eq!(h.meta_seg_at(7, 4), None); // the gap
        assert_eq!(h.meta_seg_at(9, 4), Some(MetaSeg::Priority)); // "‖"
        assert_eq!(h.meta_seg_at(16, 4), Some(MetaSeg::Priority)); // last of "medium"
        assert_eq!(h.meta_seg_at(17, 4), None); // past the end
        assert_eq!(h.meta_seg_at(9, 5), None); // wrong row
    }

    #[test]
    fn list_row_at_applies_offset_and_len() {
        let h = Hits {
            list: Some(ListHits {
                area: Rect::new(0, 2, 80, 5),
                offset: 3,
                len: 7,
            }),
            ..Hits::default()
        };
        assert_eq!(h.list_row_at(10, 2), Some(3)); // first visible row
        assert_eq!(h.list_row_at(10, 5), Some(6)); // last item within len
        assert_eq!(h.list_row_at(10, 6), None); // row 7 ≥ len
        assert_eq!(h.list_row_at(10, 1), None); // above the list
        assert_eq!(h.list_row_at(80, 2), None); // right of the area
    }
}
