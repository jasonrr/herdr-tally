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
use ratatui::widgets::{Block, Clear, Padding, Paragraph, Widget, Wrap};

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

/// Attribution suffix for the detail meta row: "   · by X, 2m ago", or
/// "   · by X, edited by Y 2m ago" when the creator and last editor differ.
/// Empty for items written before attribution shipped (no created/updated_by).
pub fn attribution(created_by: &str, updated_by: &str, updated: &str, now: u64) -> String {
    let creator = if created_by.is_empty() {
        updated_by
    } else {
        created_by
    };
    if creator.is_empty() {
        return String::new();
    }
    let rel = crate::tui::time::humanize_since(updated, now);
    if !created_by.is_empty() && !updated_by.is_empty() && updated_by != created_by {
        format!("   · by {created_by}, edited by {updated_by} {rel}")
    } else {
        format!("   · by {creator}, {rel}")
    }
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
        Mode::Help => {
            draw_list(app, f, content); // list stays visible behind the overlay
            draw_help(f, content);
        }
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
    let now = crate::tui::time::now_unix();
    let mut rows: Vec<Line> = Vec::new();
    match app.tab {
        Tab::Todos => {
            let vis = app.visible_todos();
            if vis.is_empty() {
                if app.filter.is_empty() {
                    rows.push(Line::from("  No todos yet."));
                } else {
                    rows.push(Line::from(format!("  No todos match /{}", app.filter)));
                }
            }
            for (i, t) in vis.iter().enumerate() {
                let glyph = if t.status == "completed" {
                    "☑"
                } else {
                    "☐"
                };
                let blocked = if app.blocked.contains(&t.id) {
                    " ⛔"
                } else {
                    ""
                };
                let left = format!("{glyph} [{}] {}{blocked}", t.priority, t.title);
                let rel = crate::tui::time::humanize_since(&t.updated, now);
                let right = match &t.lock {
                    Some(l) if !l.owner.is_empty() => format!("🔒 {} {}", l.owner, rel),
                    _ => rel,
                };
                rows.push(right_aligned_row(
                    left,
                    right,
                    list_area.width as usize,
                    i == cursor,
                    cursor_style,
                ));
            }
        }
        Tab::Scratchpads => {
            let vis = app.visible_pads();
            if vis.is_empty() {
                if app.filter.is_empty() {
                    rows.push(Line::from("  No scratchpads yet."));
                } else {
                    rows.push(Line::from(format!(
                        "  No scratchpads match /{}",
                        app.filter
                    )));
                }
            }
            for (i, s) in vis.iter().enumerate() {
                let rel = crate::tui::time::humanize_since(&s.updated, now);
                rows.push(right_aligned_row(
                    format!("• {}", s.title),
                    rel,
                    list_area.width as usize,
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

/// A list row with `left` text and `right` text pinned to the right edge of
/// `width` columns. When they'd collide, `left` is truncated with `…`. The
/// whole row is padded to `width` so the cursor highlight spans edge-to-edge.
fn right_aligned_row(
    left: String,
    right: String,
    width: usize,
    selected: bool,
    cursor_style: Style,
) -> Line<'static> {
    let row = pad_right_aligned(left, &right, width);
    styled_row(row, selected, cursor_style)
}

/// Places `right` at the right edge of `width` columns (char-counted, matching
/// the rest of this file's column math), truncating `left` with `…` when the
/// two would overlap. Empty `right` returns `left` unchanged.
fn pad_right_aligned(left: String, right: &str, width: usize) -> String {
    if right.is_empty() {
        return left;
    }
    let rw = right.chars().count();
    let lw = left.chars().count();
    let avail = width.saturating_sub(rw + 1); // room for right + a one-space gap
    if lw <= avail {
        let pad = width - rw - lw;
        format!("{left}{}{right}", " ".repeat(pad))
    } else {
        // truncate left, leaving room for the ellipsis + gap + right
        let keep = avail.saturating_sub(1);
        let truncated: String = left.chars().take(keep).collect();
        format!("{truncated}… {right}")
    }
}

/// Title text for the detail view: the item pinned by `read_id` (todos/pads)
/// or its abs_path (plans) — resolved against the full lists, not the
/// filtered/cursor-indexed ones, so a mutation that drops the item out of the
/// active filter doesn't blank the view out from under it.
fn read_title(app: &App) -> Option<String> {
    match app.tab {
        Tab::Todos => app
            .todos
            .iter()
            .find(|t| t.id == app.read_id)
            .map(|t| t.title.clone()),
        Tab::Scratchpads => app
            .pads
            .iter()
            .find(|s| s.id == app.read_id)
            .map(|s| s.title.clone()),
        Tab::Plans => app
            .plans
            .iter()
            .find(|d| d.abs_path.to_string_lossy() == app.read_id)
            .map(|d| d.rel_path.clone()),
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
        let now = crate::tui::time::now_unix();
        let meta = app.todos.iter().find(|t| t.id == app.read_id).map(|t| {
            (
                t.status.clone(),
                t.priority.clone(),
                attribution(&t.created_by, &t.updated_by, &t.updated, now),
            )
        });
        if let Some((status, priority, attr)) = meta {
            let meta_area = parts[2];
            let row = format!("{}{attr}", meta_line(&status, &priority));
            f.render_widget(Line::from(row).dim(), meta_area);
            app.hits.meta = Some(MetaHits::new(meta_area.x, meta_area.y, &status, &priority));
        }
    }

    // Pre-wrap the whole doc into an off-screen buffer once (per gen+width), then
    // blit only the visible rows. ratatui's Paragraph re-wraps every line above
    // the scroll offset on each repaint, so scrolling a large doc near the bottom
    // was O(scroll depth) per frame — the stop-motion symptom.
    let w = body_area.width;
    let stale = !matches!(&app.read_cache, Some((g, cw, _)) if *g == app.read_gen && *cw == w);
    if stale && w > 0 {
        let body = Paragraph::new(app.read_text.clone()).wrap(Wrap { trim: false });
        let total = (body.line_count(w) as u16).max(1);
        let mut buf = ratatui::buffer::Buffer::empty(Rect::new(0, 0, w, total));
        body.render(*buf.area(), &mut buf);
        app.read_cache = Some((app.read_gen, w, buf));
    }
    app.hits.body_h = body_area.height;
    let Some((_, _, cache)) = &app.read_cache else {
        return;
    };
    let total = cache.area().height;
    let max_scroll = total.saturating_sub(body_area.height);
    app.read_scroll = app.read_scroll.min(max_scroll);
    let dst = f.buffer_mut();
    for row in 0..body_area.height {
        let src_y = app.read_scroll + row;
        if src_y >= total {
            break;
        }
        for col in 0..w {
            if let (Some(src), Some(dst)) = (
                cache.cell((col, src_y)),
                dst.cell_mut((body_area.x + col, body_area.y + row)),
            ) {
                *dst = src.clone();
            }
        }
    }
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
    let show_meta = app.tab == Tab::Todos;
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
        let status_priority = if app.edit_id.is_empty() {
            Some(("open".to_string(), app.edit_priority.clone()))
        } else {
            let i = app.cursor[Tab::Todos.idx()];
            let v = app.visible_todos();
            v.get(i).map(|t| (t.status.clone(), t.priority.clone()))
        };
        if let Some((status, priority)) = status_priority {
            let meta_area = parts[1];
            f.render_widget(Line::from(meta_line(&status, &priority)).dim(), meta_area);
            app.hits.meta = Some(MetaHits::new(meta_area.x, meta_area.y, &status, &priority));
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
        // Top ~6 commands only; `?` opens the full list (draw_help).
        Mode::List => match app.tab {
            Tab::Todos => "↑↓ · enter · n new · space done · e edit · d del · ? help",
            Tab::Scratchpads => "↑↓ · enter · n new · e edit · d del · ? help",
            Tab::Plans => "↑↓ · enter · / filter · r · ? help",
        },
        Mode::Read => match app.tab {
            Tab::Todos => "space done · p prio · e edit · y id · Y copy · R raw · esc back",
            Tab::Scratchpads => "e edit · y id · Y copy · R raw · esc back",
            Tab::Plans => "y id · Y copy · R raw · esc back",
        },
        Mode::Confirm => "y confirm · n/esc cancel",
        Mode::Edit => match app.tab {
            Tab::Todos => "tab title/body · ctrl+d save · esc discard · ctrl+p prio · ctrl+t done",
            _ => "tab title/body · ctrl+d save · esc discard",
        },
        Mode::DiscardConfirm => "y discard · n/esc keep editing",
        Mode::Filter => "type to filter · enter apply · esc clear",
        Mode::Help => "esc · q · ? — close",
    }
}

/// Full keyboard reference, one (keys, action) pair per row. Grouped by the
/// mode the keys apply in. Kept in sync with the footer/key handlers by hand.
const HELP_ROWS: &[(&str, &str)] = &[
    ("List", ""),
    ("↑↓ j k", "move"),
    ("enter o", "open / read"),
    ("n", "new"),
    ("e", "edit"),
    ("space", "toggle done (todos)"),
    ("p", "cycle priority (todos)"),
    ("c", "hide / show done (todos)"),
    ("d", "delete"),
    ("/", "filter (plans)"),
    ("1 2 3", "switch tab"),
    ("r", "reload"),
    ("q  ctrl+c", "quit"),
    ("", ""),
    ("Read", ""),
    ("space p e", "done · prio · edit"),
    ("y  Y  R", "copy id · copy body · raw"),
    ("ctrl+d/u", "scroll · esc back"),
    ("", ""),
    ("Edit", ""),
    ("tab", "switch title / body"),
    ("ctrl+d / ctrl+enter", "save"),
    ("esc", "discard"),
    ("ctrl+p  ctrl+t", "priority · done (todos)"),
];

/// Floating shortcuts overlay. Centered over `area`; if the pane is too small
/// for the box it fills `area` instead so nothing clips out of view.
fn draw_help(f: &mut Frame, area: Rect) {
    let key_w = HELP_ROWS
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0) as u16;
    let lines: Vec<Line> = HELP_ROWS
        .iter()
        .map(|(k, v)| {
            if v.is_empty() {
                // section header (or blank spacer) — bold, no padding column
                Line::from(Span::from(*k).bold())
            } else {
                Line::from(format!("{k:key_w$}  {v}", key_w = key_w as usize))
            }
        })
        .collect();

    let content_w = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
    let want_w = (content_w + 4).min(area.width); // +2 border +2 padding
    let want_h = (lines.len() as u16 + 2).min(area.height); // +2 border
    let x = area.x + (area.width.saturating_sub(want_w)) / 2;
    let y = area.y + (area.height.saturating_sub(want_h)) / 2;
    let popup = Rect::new(x, y, want_w, want_h);

    f.render_widget(Clear, popup);
    let block = Block::bordered()
        .title(" Shortcuts ")
        .padding(Padding::horizontal(1));
    f.render_widget(Paragraph::new(lines).block(block), popup);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::resolve_project_in;
    use crate::store::testutil::{TempDir, git_repo};

    #[test]
    fn attribution_cases() {
        let now: u64 = 300; // 5m after the epoch below
        let t = "1970-01-01T00:00:00Z";
        // no attribution -> empty
        assert_eq!(attribution("", "", t, now), "");
        // creator == editor -> single "by X"
        let one = attribution("claude", "claude", t, now);
        assert!(
            one.contains("by claude") && !one.contains("edited"),
            "{one:?}"
        );
        // creator != editor -> both shown
        let two = attribution("claude", "jason", t, now);
        assert!(
            two.contains("by claude") && two.contains("edited by jason"),
            "{two:?}"
        );
        // only editor known (pre-attribution creator) -> single "by editor"
        let e = attribution("", "jason", t, now);
        assert!(e.contains("by jason") && !e.contains("edited"), "{e:?}");
    }

    /// Filtering to a high-priority todo, opening it in read mode, then
    /// mutating the field the filter matched on (as `p`/`space` do) must not
    /// blank the detail view just because the item fell out of the filtered
    /// list — the view is pinned to `read_id`, not the filtered cursor.
    #[test]
    fn read_title_survives_item_leaving_filter() {
        let root = TempDir::new();
        let repo = git_repo();
        let p = resolve_project_in(root.path(), Some(&repo.path().to_string_lossy())).unwrap();
        let mut app = App::new(p, Tab::Todos);
        app.p.create_todo("Ship it", "", "high", vec![]).unwrap();
        app.p.create_todo("Other task", "", "low", vec![]).unwrap();
        app.reload();

        app.filter = "high".to_string();
        let high_id = app.visible_todos()[0].id.clone();
        assert_eq!(app.visible_todos().len(), 1, "filter should narrow to one");

        app.enter_read();
        assert_eq!(app.read_id, high_id);

        // Simulate `p` (cycle_priority): the item drops out of the "high" filter.
        app.p
            .update_todo(
                &high_id,
                crate::store::TodoUpdate {
                    priority: Some("low".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        app.reload();
        assert_eq!(
            app.visible_todos().len(),
            0,
            "item should have left the filter"
        );

        assert_eq!(
            read_title(&app),
            Some("Ship it".to_string()),
            "read view must stay resolved by read_id, not the filtered cursor"
        );
    }

    /// The help overlay's centering math must clamp to the pane, not panic,
    /// even when the pane is smaller than the popup it wants to draw.
    /// The read-mode viewport is a blit of a pre-wrapped cache slice; scrolling
    /// must move which document rows land on screen (and not panic on geometry).
    #[test]
    fn read_scroll_blits_the_right_slice() {
        let root = TempDir::new();
        let repo = git_repo();
        let p = resolve_project_in(root.path(), Some(&repo.path().to_string_lossy())).unwrap();
        let mut app = App::new(p, Tab::Todos);
        let body = (0..200)
            .map(|i| format!("L{i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.p.create_todo("Doc", &body, "high", vec![]).unwrap();
        app.reload();
        app.enter_read();
        app.raw = true; // plain lines, no markdown wrapping surprises
        app.rebuild_read_text();

        let dump = |app: &mut App| {
            let backend = ratatui::backend::TestBackend::new(40, 12);
            let mut term = ratatui::Terminal::new(backend).unwrap();
            term.draw(|f| draw(app, f)).unwrap();
            let buf = term.backend().buffer().clone();
            let mut s = String::new();
            for y in 0..buf.area().height {
                for x in 0..buf.area().width {
                    s.push_str(buf.cell((x, y)).unwrap().symbol());
                }
                s.push('\n');
            }
            s
        };

        app.read_scroll = 0;
        let top = dump(&mut app);
        assert!(top.contains("L000"), "top of doc visible at scroll 0");
        assert!(!top.contains("L100"), "far rows not visible at scroll 0");

        app.read_scroll = 100;
        let deep = dump(&mut app);
        assert!(deep.contains("L100"), "scroll moves the viewport down");
        assert!(!deep.contains("L000"), "top rows gone once scrolled down");
    }

    #[test]
    fn help_overlay_renders_at_any_size() {
        let root = TempDir::new();
        let repo = git_repo();
        let p = resolve_project_in(root.path(), Some(&repo.path().to_string_lossy())).unwrap();
        let mut app = App::new(p, Tab::Todos);
        app.p.create_todo("A todo", "", "high", vec![]).unwrap();
        app.reload();
        app.mode = Mode::Help;
        for (w, h) in [(8u16, 4u16), (80, 30)] {
            let backend = ratatui::backend::TestBackend::new(w, h);
            let mut term = ratatui::Terminal::new(backend).unwrap();
            term.draw(|f| draw(&mut app, f)).unwrap(); // panics on bad geometry
        }
    }

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

    #[test]
    fn pad_right_aligned_pads_and_truncates() {
        // fits: right pinned to the edge, one-space min gap, total == width
        let got = pad_right_aligned("todo".to_string(), "3h", 12);
        assert_eq!(got, "todo      3h");
        assert_eq!(got.chars().count(), 12);
        // collides: left truncated with "… " before the right text, total == width
        let got = pad_right_aligned("a very long title".to_string(), "3h", 12);
        assert_eq!(got, "a very l… 3h");
        assert_eq!(got.chars().count(), 12);
        // empty right: left returned unchanged (Plans rows have no time)
        assert_eq!(
            pad_right_aligned("• spec.md".to_string(), "", 12),
            "• spec.md"
        );
    }
}
