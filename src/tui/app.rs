//! TUI state + event handling. Rewrite of the Go bubbletea model (model.go)
//! into ratatui idioms: the Go Update/Cmd machinery becomes plain synchronous
//! methods driven by the crossterm event loop in mod.rs, and mouse hit-testing
//! reads the regions the last draw recorded in `Hits` (view.rs) instead of
//! hardcoded column math.
use std::collections::HashSet;
use std::io::Write;
use std::process::{Command, Stdio};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use edtui::{EditorEventHandler, EditorMode, EditorState, Lines};
use ratatui::text::Text;

use crate::plans::{self, Plan};
use crate::store::{Project, Scratchpad, Todo, TodoFilter, TodoUpdate};

use super::markdown;
use super::view::{Hits, MetaSeg};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Todos,
    Scratchpads,
    Plans,
}

impl Tab {
    pub fn idx(self) -> usize {
        match self {
            Tab::Todos => 0,
            Tab::Scratchpads => 1,
            Tab::Plans => 2,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    List,
    Read,
    Confirm,
    Edit,
    DiscardConfirm,
    Filter,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Title,
    Body,
}

pub struct App {
    pub p: Project,
    pub tab: Tab,
    pub mode: Mode,
    pub status: String,
    pub quit: bool,

    pub todos: Vec<Todo>,
    /// Ids of blocked todos, computed once per reload, not per frame. Id-keyed
    /// (not positional) so it survives the `/` filter narrowing `todos`' indices.
    pub blocked: HashSet<String>,
    pub pads: Vec<Scratchpad>,
    pub plans: Vec<Plan>,
    pub cursor: [usize; 3],

    /// Active Plans-tab filter text (empty = no filter).
    pub filter: String,
    /// Todos tab: when set, completed todos are dropped from the list ('c').
    pub hide_completed: bool,

    /// Id awaiting delete confirmation.
    pub pending: String,
    /// Set while a discard-confirm is gating a tab switch.
    pub pending_tab: Option<Tab>,

    // read mode
    pub raw: bool,
    /// Id of the item shown in detail; pins the cursor to it across reloads.
    pub read_id: String,
    pub read_body: String,
    /// Cached render of read_body (markdown or raw), rebuilt on body change.
    pub read_text: Text<'static>,
    pub read_scroll: u16,

    // edit mode
    pub title_ed: EditorState,
    pub body_ed: EditorState,
    pub title_handler: EditorEventHandler,
    pub body_handler: EditorEventHandler,
    pub edit_focus: Focus,
    edit_orig_title: String,
    edit_orig_body: String,
    /// Todo Updated timestamp captured at open (concurrent-edit guard).
    pub edit_updated: String,
    /// Empty = the editor holds a brand-new, unsaved item.
    pub edit_id: String,
    /// Scratchpad revision captured at edit-entry (todos ignore).
    pub edit_rev: i64,
    /// Mode to return to on save/cancel (List or Read).
    pub edit_return: Mode,

    /// Hit-test regions recorded by the last draw (view.rs).
    pub hits: Hits,
}

/// low→medium→high→low; anything unrecognized falls back to medium.
pub fn next_priority(cur: &str) -> &'static str {
    match cur {
        "low" => "medium",
        "medium" => "high",
        "high" => "low",
        _ => "medium",
    }
}

fn new_editor(text: &str, single_line: bool) -> EditorState {
    let mut st = EditorState::new(Lines::from(text));
    st.mode = EditorMode::Insert; // emacs (modeless) bindings live in Insert
    st.set_single_line(single_line);
    st
}

fn editor_text(st: &EditorState) -> String {
    String::from(st.lines.clone())
}

/// Copies s to the system clipboard via pbcopy (macOS-only, like the scripts).
fn clipboard_write(s: &str) -> std::io::Result<()> {
    let mut c = Command::new("pbcopy").stdin(Stdio::piped()).spawn()?;
    c.stdin
        .take()
        .expect("piped stdin")
        .write_all(s.as_bytes())?;
    let status = c.wait()?;
    if !status.success() {
        return Err(std::io::Error::other("pbcopy failed"));
    }
    Ok(())
}

impl App {
    pub fn new(p: Project, initial: Tab) -> App {
        App {
            p,
            tab: initial,
            mode: Mode::List,
            status: String::new(),
            quit: false,
            todos: Vec::new(),
            blocked: HashSet::new(),
            pads: Vec::new(),
            plans: Vec::new(),
            cursor: [0; 3],
            filter: String::new(),
            hide_completed: false,
            pending: String::new(),
            pending_tab: None,
            raw: false,
            read_id: String::new(),
            read_body: String::new(),
            read_text: Text::default(),
            read_scroll: 0,
            title_ed: new_editor("", true),
            body_ed: new_editor("", false),
            title_handler: EditorEventHandler::emacs_mode(),
            body_handler: EditorEventHandler::emacs_mode(),
            edit_focus: Focus::Body,
            edit_orig_title: String::new(),
            edit_orig_body: String::new(),
            edit_updated: String::new(),
            edit_id: String::new(),
            edit_rev: 0,
            edit_return: Mode::List,
            hits: Hits::default(),
        }
    }

    /// Reload all three lists from the store (the Go loadCmd + loadedMsg pair,
    /// made synchronous). Pins the cursor to the open item's id so a re-sort
    /// can't silently retarget the detail view; list mode pins to the selected
    /// row's id for the same reason.
    pub fn reload(&mut self) {
        let list_sel = if self.mode == Mode::List {
            self.selected_id()
        } else {
            None
        };
        match self.p.list_todos(TodoFilter {
            sort: "priority".to_string(),
            ..TodoFilter::default()
        }) {
            Ok(t) => {
                let t: Vec<Todo> = if self.hide_completed {
                    t.into_iter().filter(|x| x.status != "completed").collect()
                } else {
                    t
                };
                self.blocked = t
                    .iter()
                    .filter(|x| self.p.is_blocked(x))
                    .map(|x| x.id.clone())
                    .collect();
                self.todos = t;
            }
            Err(e) => self.status = format!("load failed: {e}"),
        }
        match self.p.list_scratchpads(&[], "", false, 0, 0) {
            Ok(s) => self.pads = s,
            Err(e) => self.status = format!("load failed: {e}"),
        }
        self.plans = plans::list(&self.p.path, &plans::load_plan_paths());

        if matches!(self.mode, Mode::Read | Mode::Edit | Mode::DiscardConfirm)
            && !self.read_id.is_empty()
        {
            let id = self.read_id.clone();
            self.pin_cursor_to(&id);
        } else if let Some(id) = list_sel {
            self.pin_cursor_to(&id);
        }
        self.clamp_cursor();
    }

    /// The Plans list after the active filter (case-insensitive substring over
    /// rel_path and heading). The Plans tab indexes this, not self.plans.
    pub fn visible_plans(&self) -> Vec<&Plan> {
        if self.filter.is_empty() {
            return self.plans.iter().collect();
        }
        let q = self.filter.to_lowercase();
        self.plans
            .iter()
            .filter(|d| {
                d.rel_path.to_lowercase().contains(&q) || d.heading.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Todos after the active filter: case-insensitive substring over
    /// title + tags + status + priority. Empty filter -> all.
    pub fn visible_todos(&self) -> Vec<&Todo> {
        if self.filter.is_empty() {
            return self.todos.iter().collect();
        }
        let q = self.filter.to_lowercase();
        self.todos
            .iter()
            .filter(|t| {
                t.title.to_lowercase().contains(&q)
                    || t.status.to_lowercase().contains(&q)
                    || t.priority.to_lowercase().contains(&q)
                    || t.tags.iter().any(|g| g.to_lowercase().contains(&q))
            })
            .collect()
    }

    /// Scratchpads after the active filter: title + tags. Empty filter -> all.
    pub fn visible_pads(&self) -> Vec<&Scratchpad> {
        if self.filter.is_empty() {
            return self.pads.iter().collect();
        }
        let q = self.filter.to_lowercase();
        self.pads
            .iter()
            .filter(|s| {
                s.title.to_lowercase().contains(&q)
                    || s.tags.iter().any(|g| g.to_lowercase().contains(&q))
            })
            .collect()
    }

    pub fn count(&self) -> usize {
        match self.tab {
            Tab::Todos => self.visible_todos().len(),
            Tab::Scratchpads => self.visible_pads().len(),
            Tab::Plans => self.visible_plans().len(),
        }
    }

    pub fn move_cursor(&mut self, d: i64) {
        let n = self.count();
        let i = self.tab.idx();
        if n == 0 {
            self.cursor[i] = 0;
            return;
        }
        self.cursor[i] = (self.cursor[i] as i64 + d).clamp(0, n as i64 - 1) as usize;
    }

    pub fn clamp_cursor(&mut self) {
        self.move_cursor(0);
    }

    /// Re-points the current tab's cursor at the row whose id matches, so the
    /// detail view stays on the same item when a reload re-sorts the list.
    pub fn pin_cursor_to(&mut self, id: &str) {
        let pos = match self.tab {
            Tab::Todos => self.visible_todos().iter().position(|t| t.id == id),
            Tab::Scratchpads => self.visible_pads().iter().position(|s| s.id == id),
            Tab::Plans => self
                .visible_plans()
                .iter()
                .position(|d| d.abs_path.to_string_lossy() == id),
        };
        if let Some(i) = pos {
            self.cursor[self.tab.idx()] = i;
        }
    }

    pub fn selected_id(&self) -> Option<String> {
        let i = self.cursor[self.tab.idx()];
        match self.tab {
            Tab::Todos => self.visible_todos().get(i).map(|t| t.id.clone()),
            Tab::Scratchpads => self.visible_pads().get(i).map(|s| s.id.clone()),
            Tab::Plans => self
                .visible_plans()
                .get(i)
                .map(|d| d.abs_path.to_string_lossy().into_owned()),
        }
    }

    // ---- keyboard ----

    pub fn on_key(&mut self, k: KeyEvent) {
        match self.mode {
            Mode::List => self.key_list(k),
            Mode::Read => self.key_read(k),
            Mode::Confirm => self.key_confirm(k),
            Mode::Edit => self.key_edit(k),
            Mode::DiscardConfirm => self.key_discard_confirm(k),
            Mode::Filter => self.key_filter(k),
        }
    }

    fn key_list(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('c') if self.tab == Tab::Todos => {
                self.hide_completed = !self.hide_completed;
                self.reload();
            }
            KeyCode::Char('1') => self.switch_tab(Tab::Todos),
            KeyCode::Char('2') => self.switch_tab(Tab::Scratchpads),
            KeyCode::Char('3') => self.switch_tab(Tab::Plans),
            KeyCode::Char('j') | KeyCode::Down => self.move_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_cursor(-1),
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('r') => self.reload(),
            KeyCode::Char(' ') if self.tab == Tab::Todos => {
                self.toggle_status();
                self.reload();
            }
            KeyCode::Char('d') if self.tab != Tab::Plans => {
                if let Some(id) = self.selected_id() {
                    self.pending = id;
                    self.mode = Mode::Confirm;
                }
            }
            KeyCode::Enter | KeyCode::Char('o') => {
                if self.selected_id().is_some() {
                    self.enter_read();
                }
            }
            KeyCode::Char('n') if self.tab != Tab::Plans => self.begin_edit_new(),
            KeyCode::Char('e') if self.tab != Tab::Plans => self.begin_edit(),
            KeyCode::Char('p') if self.tab == Tab::Todos => {
                self.cycle_priority();
                self.reload();
            }
            _ => {}
        }
    }

    fn key_read(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.status.clear();
                self.mode = Mode::List;
            }
            KeyCode::Char('R') => {
                self.raw = !self.raw;
                self.rebuild_read_text();
            }
            KeyCode::Char(' ') if self.tab == Tab::Todos => {
                self.toggle_status();
                self.reload();
            }
            KeyCode::Char('p') if self.tab == Tab::Todos => {
                self.cycle_priority();
                self.reload();
            }
            KeyCode::Char('e') | KeyCode::Enter if self.tab != Tab::Plans => self.begin_edit(),
            KeyCode::Char('y') => self.yank(),
            // body scrolling (clamped against the rendered height at draw time)
            KeyCode::Char('j') | KeyCode::Down => self.scroll_read(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_read(-1),
            KeyCode::PageDown => self.scroll_read(self.hits.body_h as i32),
            KeyCode::PageUp => self.scroll_read(-(self.hits.body_h as i32)),
            KeyCode::Char('d') if ctrl => self.scroll_read(self.hits.body_h as i32 / 2),
            KeyCode::Char('u') if ctrl => self.scroll_read(-(self.hits.body_h as i32) / 2),
            _ => {}
        }
    }

    fn key_confirm(&mut self, k: KeyEvent) {
        if k.code == KeyCode::Char('y') {
            self.delete_pending();
            self.mode = Mode::List;
            self.reload();
            return;
        }
        // n, esc, anything else cancels
        self.mode = Mode::List;
        self.pending.clear();
    }

    fn key_edit(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => {
                if self.edit_dirty() {
                    self.mode = Mode::DiscardConfirm;
                } else {
                    self.exit_edit(); // clean: leave immediately
                }
                return;
            }
            KeyCode::Char('d') if ctrl => {
                self.save_edit();
                return;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.edit_focus = match self.edit_focus {
                    Focus::Body => Focus::Title,
                    Focus::Title => Focus::Body,
                };
                return;
            }
            KeyCode::Char('p') if ctrl => {
                // no todo to mutate yet on a new item
                if self.tab == Tab::Todos && !self.edit_id.is_empty() {
                    self.cycle_priority();
                    // own write bumped Updated; don't self-conflict at save
                    self.refresh_edit_updated();
                    self.reload();
                }
                return;
            }
            KeyCode::Char('t') if ctrl => {
                if self.tab == Tab::Todos && !self.edit_id.is_empty() {
                    self.toggle_status();
                    self.refresh_edit_updated();
                    self.reload();
                }
                return;
            }
            _ => {}
        }
        match self.edit_focus {
            Focus::Title => self.title_handler.on_key_event(k, &mut self.title_ed),
            Focus::Body => self.body_handler.on_key_event(k, &mut self.body_ed),
        }
    }

    fn key_discard_confirm(&mut self, k: KeyEvent) {
        if k.code == KeyCode::Char('y') {
            if let Some(t) = self.pending_tab.take() {
                // discard-confirm was gating a tab switch: land on the new tab's list
                self.tab = t;
                self.status.clear();
                self.mode = Mode::List;
                if t != Tab::Plans {
                    self.filter.clear();
                }
                self.reload();
                return;
            }
            self.exit_edit();
            self.reload();
            return;
        }
        self.pending_tab = None;
        self.mode = Mode::Edit; // n/esc/anything: back to editing
    }

    fn key_filter(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.mode = Mode::List;
                self.clamp_cursor();
            }
            KeyCode::Enter => {
                self.mode = Mode::List; // keep the filter, return to the list
                self.clamp_cursor();
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.clamp_cursor();
            }
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.push(c);
                self.cursor[self.tab.idx()] = 0; // reset selection into the narrowed list
            }
            _ => {}
        }
    }

    pub fn on_paste(&mut self, text: String) {
        if self.mode == Mode::Edit {
            match self.edit_focus {
                Focus::Title => self.title_handler.on_paste_event(text, &mut self.title_ed),
                Focus::Body => self.body_handler.on_paste_event(text, &mut self.body_ed),
            }
        }
    }

    // ---- mouse ----

    pub fn on_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::ScrollDown => match self.mode {
                Mode::List | Mode::Filter => self.move_cursor(1),
                Mode::Read => self.scroll_read(3),
                Mode::Edit => self.forward_mouse(m),
                _ => {}
            },
            MouseEventKind::ScrollUp => match self.mode {
                Mode::List | Mode::Filter => self.move_cursor(-1),
                Mode::Read => self.scroll_read(-3),
                Mode::Edit => self.forward_mouse(m),
                _ => {}
            },
            MouseEventKind::Down(MouseButton::Left) => self.mouse_down(m),
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                if self.mode == Mode::Edit {
                    self.forward_mouse(m);
                }
            }
            _ => {}
        }
    }

    fn mouse_down(&mut self, m: MouseEvent) {
        // tab bar first: clicking a label switches tabs in any mode (a dirty
        // editor is still gated through discard-confirm by switch_tab)
        if let Some(t) = self.hits.tab_at(m.column, m.row) {
            self.switch_tab(t);
            return;
        }
        match self.mode {
            Mode::List | Mode::Filter => {
                if let Some(i) = self.hits.list_row_at(m.column, m.row) {
                    if i >= self.count() {
                        return;
                    }
                    let cur = self.cursor[self.tab.idx()];
                    if self.mode == Mode::List && i == cur {
                        self.enter_read(); // click on the already-selected row opens it
                    } else {
                        self.cursor[self.tab.idx()] = i;
                    }
                }
            }
            Mode::Read => {
                if self.tab == Tab::Todos {
                    self.meta_click(m.column, m.row);
                }
            }
            Mode::Edit => {
                if let Some(r) = self.hits.title_card
                    && r.contains(ratatui::layout::Position::new(m.column, m.row))
                {
                    self.edit_focus = Focus::Title;
                } else if let Some(r) = self.hits.body_card
                    && r.contains(ratatui::layout::Position::new(m.column, m.row))
                {
                    self.edit_focus = Focus::Body;
                }
                if self.tab == Tab::Todos
                    && !self.edit_id.is_empty()
                    && self.meta_click(m.column, m.row)
                {
                    self.refresh_edit_updated();
                    return;
                }
                self.forward_mouse(m); // edtui places the cursor / starts a drag
            }
            _ => {}
        }
    }

    /// Click on the read/edit metadata row: `○ status` toggles done, `‖ prio`
    /// cycles priority. Returns whether a segment was hit.
    fn meta_click(&mut self, x: u16, y: u16) -> bool {
        match self.hits.meta_seg_at(x, y) {
            Some(MetaSeg::Status) => {
                self.toggle_status();
                self.reload();
                true
            }
            Some(MetaSeg::Priority) => {
                self.cycle_priority();
                self.reload();
                true
            }
            None => false,
        }
    }

    /// Forwards a mouse event to both editors; edtui bounds-checks against the
    /// area each was last rendered in, so only the editor under the pointer
    /// reacts.
    fn forward_mouse(&mut self, m: MouseEvent) {
        self.title_handler.on_mouse_event(m, &mut self.title_ed);
        self.body_handler.on_mouse_event(m, &mut self.body_ed);
    }

    fn scroll_read(&mut self, d: i32) {
        // upper clamp happens at draw time, where the wrapped line count is known
        self.read_scroll = (self.read_scroll as i32 + d).max(0) as u16;
    }

    // ---- actions ----

    /// Moves to another tab and lands on its list. From a clean detail or
    /// editor it drops straight to the list; from a dirty editor it routes
    /// through discard-confirm (remembering the target) so an accidental tab
    /// click can't silently discard an in-progress edit.
    pub fn switch_tab(&mut self, t: Tab) {
        if t == self.tab && self.mode == Mode::List {
            return;
        }
        if self.mode == Mode::Edit && self.edit_dirty() {
            self.pending_tab = Some(t);
            self.mode = Mode::DiscardConfirm;
            return;
        }
        self.tab = t;
        self.mode = Mode::List;
        self.status.clear();
        if t != Tab::Plans {
            // a Docs filter shouldn't linger (invisibly) on other tabs or on return
            self.filter.clear();
        }
    }

    fn toggle_status(&mut self) {
        let (id, done) = {
            let v = self.visible_todos();
            let Some(t) = v.get(self.cursor[Tab::Todos.idx()]) else {
                return;
            };
            (t.id.clone(), t.status == "completed")
        };
        let r = if done {
            self.p.incomplete_todo(&id, false)
        } else {
            self.p.complete_todo(&id, false)
        };
        if let Err(e) = r {
            self.status = format!("status change failed: {e}");
        }
    }

    fn cycle_priority(&mut self) {
        let (id, next) = {
            let v = self.visible_todos();
            let Some(t) = v.get(self.cursor[Tab::Todos.idx()]) else {
                return;
            };
            (t.id.clone(), next_priority(&t.priority).to_string())
        };
        if let Err(e) = self.p.update_todo(
            &id,
            TodoUpdate {
                priority: Some(next),
                ..TodoUpdate::default()
            },
        ) {
            self.status = format!("priority change failed: {e}");
        }
    }

    fn delete_pending(&mut self) {
        let id = std::mem::take(&mut self.pending);
        if id.is_empty() {
            return;
        }
        if self.tab == Tab::Todos {
            if let Err(e) = self.p.delete_todo(&id) {
                self.status = format!("delete failed: {e}");
            }
            return;
        }
        // scratchpad: pass the loaded revision (list keeps revision even
        // though content is blank)
        let Some(rev) = self.pads.iter().find(|s| s.id == id).map(|s| s.revision) else {
            self.status = format!("delete failed: no revision for {id}");
            return;
        };
        if let Err(e) = self.p.delete_scratchpad(&id, rev) {
            self.status = format!("delete failed: {e}");
        }
    }

    pub fn enter_read(&mut self) {
        let Some(id) = self.selected_id() else {
            return;
        };
        let body = match self.tab {
            Tab::Todos => match self.visible_todos().get(self.cursor[Tab::Todos.idx()]) {
                Some(t) => Ok(t.body.clone()),
                None => {
                    self.mode = Mode::List;
                    return;
                }
            },
            Tab::Scratchpads => self
                .p
                .read_scratchpad(&id, "full", "", 0, 0)
                .map(|(s, _)| s.content)
                .map_err(|e| e.to_string()),
            Tab::Plans => plans::read(std::path::Path::new(&id)).map_err(|e| e.to_string()),
        };
        match body {
            Ok(b) => {
                self.read_id = id;
                self.read_body = b;
                self.read_scroll = 0;
                self.mode = Mode::Read;
                self.rebuild_read_text();
            }
            Err(e) => {
                self.status = format!("read failed: {e}");
                self.mode = Mode::List;
            }
        }
    }

    pub fn rebuild_read_text(&mut self) {
        self.read_text = if self.raw {
            Text::raw(self.read_body.clone())
        } else {
            markdown::render(&self.read_body)
        };
    }

    /// Opens the unified title+body editor on the selected item.
    pub fn begin_edit(&mut self) {
        let Some(id) = self.selected_id() else {
            return;
        };
        let (title, body) = if self.tab == Tab::Todos {
            let Some(t) = self.todos.iter().find(|t| t.id == id) else {
                return;
            };
            self.edit_updated = t.updated.clone(); // guard token for the save
            (t.title.clone(), t.body.clone())
        } else {
            match self.p.read_scratchpad(&id, "full", "", 0, 0) {
                Ok((s, _)) => {
                    self.edit_rev = s.revision;
                    (s.title, s.content)
                }
                Err(e) => {
                    self.status = format!("open failed: {e}");
                    return;
                }
            }
        };
        self.title_ed = new_editor(&title, true);
        self.body_ed = new_editor(&body, false); // cursor opens at the top
        self.edit_focus = Focus::Body; // body focused by default
        self.edit_orig_title = title;
        self.edit_orig_body = body;
        self.edit_id = id.clone();
        self.read_id = id;
        self.edit_return = self.mode; // return to list or view, wherever we came from
        self.status.clear();
        self.mode = Mode::Edit;
    }

    /// Opens the unified editor on a brand-new, unsaved item. save_edit keys
    /// off the empty edit_id to create rather than update; leaving without
    /// typing persists nothing, so there is no orphan blank item.
    pub fn begin_edit_new(&mut self) {
        self.title_ed = new_editor("", true);
        self.body_ed = new_editor("", false);
        self.edit_focus = Focus::Title; // new items start in the title
        self.edit_orig_title = String::new();
        self.edit_orig_body = String::new();
        self.edit_updated = String::new();
        self.edit_rev = 0;
        self.edit_id = String::new(); // signals "new" to save_edit
        self.read_id = String::new();
        self.edit_return = Mode::List;
        self.status.clear();
        self.mode = Mode::Edit;
    }

    pub fn edit_dirty(&self) -> bool {
        editor_text(&self.title_ed) != self.edit_orig_title
            || editor_text(&self.body_ed) != self.edit_orig_body
    }

    /// Returns to edit_return (list or view) WITHOUT saving, clearing the
    /// transient status line.
    fn exit_edit(&mut self) {
        self.status.clear();
        self.mode = self.edit_return;
    }

    /// Re-reads the edited todo's Updated timestamp after one of our own
    /// instant mutations (priority/status), so the later Ctrl+D save's
    /// concurrent-edit guard doesn't trip on our own write.
    fn refresh_edit_updated(&mut self) {
        if let Ok(t) = self.p.get_todo(&self.edit_id) {
            self.edit_updated = t.updated;
        }
    }

    /// Persists the buffer. Scratchpad saves are revision-guarded; todo saves
    /// are guarded against concurrent edits via expected_updated. On a guard
    /// conflict the editor stays open with the buffer intact.
    pub fn save_edit(&mut self) {
        // collapse newlines/runs of whitespace in the title
        let title = editor_text(&self.title_ed)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let body = editor_text(&self.body_ed);
        if self.edit_id.is_empty() {
            self.save_new(&title, &body);
            return;
        }
        let r = if self.tab == Tab::Todos {
            self.p
                .update_todo(
                    &self.edit_id,
                    TodoUpdate {
                        title: Some(title),
                        body: Some(body.clone()),
                        expected_updated: Some(self.edit_updated.clone()),
                        ..TodoUpdate::default()
                    },
                )
                .map(|_| ())
        } else {
            self.p
                .update_scratchpad(
                    &self.edit_id,
                    self.edit_rev,
                    Some(&title),
                    Some(&body),
                    None,
                )
                .map(|_| ())
        };
        if let Err(e) = r {
            // keep the editor open, buffer intact (revision conflict etc.)
            self.status = format!("save failed: {e}");
            return;
        }
        self.status.clear();
        self.mode = self.edit_return;
        if self.edit_return == Mode::Read {
            // refresh the view's rendered body from what we just saved
            self.read_body = body;
            self.rebuild_read_text();
        }
        self.reload();
    }

    /// Persists a brand-new item created through the unified editor. An empty
    /// title keeps the editor open (a titleless item breaks list rendering);
    /// on success it drops back to the list.
    fn save_new(&mut self, title: &str, body: &str) {
        if title.is_empty() {
            self.status = "title required".to_string();
            return;
        }
        let r = if self.tab == Tab::Todos {
            self.p.create_todo(title, body, "", Vec::new()).map(|_| ())
        } else {
            self.p
                .create_scratchpad(title, body, Vec::new())
                .map(|_| ())
        };
        if let Err(e) = r {
            self.status = format!("create failed: {e}");
            return;
        }
        self.status.clear();
        self.mode = Mode::List;
        self.reload();
    }

    fn yank(&mut self) {
        let Some(id) = self.selected_id() else {
            return;
        };
        match clipboard_write(&id) {
            Ok(()) => self.status = format!("copied {id} to clipboard"),
            Err(e) => self.status = format!("copy failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::resolve_project_in;
    use crate::store::testutil::{TempDir, git_repo};
    use ratatui::layout::Rect;

    struct Fixture {
        app: App,
        root: TempDir,
        repo: TempDir,
    }

    impl Fixture {
        fn new(tab: Tab) -> Fixture {
            let root = TempDir::new();
            let repo = git_repo();
            let p = resolve_project_in(root.path(), Some(&repo.path().to_string_lossy())).unwrap();
            Fixture {
                app: App::new(p, tab),
                root,
                repo,
            }
        }

        /// A second Project over the same store, for assertions/interference.
        fn store(&self) -> Project {
            resolve_project_in(self.root.path(), Some(&self.repo.path().to_string_lossy())).unwrap()
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn type_str(app: &mut App, s: &str) {
        for c in s.chars() {
            app.on_key(key(KeyCode::Char(c)));
        }
    }

    fn doc(rel: &str, heading: &str) -> Plan {
        Plan {
            rel_path: rel.to_string(),
            abs_path: std::path::PathBuf::from(format!("/abs/{rel}")),
            heading: heading.to_string(),
            mod_time: std::time::SystemTime::UNIX_EPOCH,
        }
    }

    /// Builds an App on the Todos tab, pre-loaded with (title, tag, priority)
    /// todos. Mirrors Fixture::new but leaks the tempdirs (via mem::forget)
    /// since the helper's return type must stay a bare App to match the
    /// test bodies verbatim; nothing in those tests touches the store again.
    fn test_app_with_todos(items: &[(&str, &str, &str)]) -> App {
        let root = TempDir::new();
        let repo = git_repo();
        let p = resolve_project_in(root.path(), Some(&repo.path().to_string_lossy())).unwrap();
        let mut app = App::new(p, Tab::Todos);
        for (title, tag, prio) in items {
            let tags = if tag.is_empty() {
                vec![]
            } else {
                vec![tag.to_string()]
            };
            app.p.create_todo(title, "", prio, tags).unwrap();
        }
        app.reload();
        std::mem::forget(root);
        std::mem::forget(repo);
        app
    }

    #[test]
    fn priority_cycle() {
        assert_eq!(next_priority("low"), "medium");
        assert_eq!(next_priority("medium"), "high");
        assert_eq!(next_priority("high"), "low");
        assert_eq!(next_priority("bogus"), "medium"); // unrecognized falls back
    }

    #[test]
    fn docs_filter_matches_relpath_and_heading_case_insensitive() {
        let mut f = Fixture::new(Tab::Plans);
        f.app.plans = vec![
            doc("docs/specs/alpha.md", "Alpha Spec"),
            doc("docs/specs/beta.md", "Beta Spec"),
            doc("docs/notes/gamma.md", "Storage Design"),
        ];
        f.app.filter = "ALPHA".to_string();
        let v: Vec<&str> = f
            .app
            .visible_plans()
            .iter()
            .map(|d| d.rel_path.as_str())
            .collect();
        assert_eq!(v, ["docs/specs/alpha.md"]);

        f.app.filter = "storage".to_string(); // matches heading only
        let v: Vec<&str> = f
            .app
            .visible_plans()
            .iter()
            .map(|d| d.rel_path.as_str())
            .collect();
        assert_eq!(v, ["docs/notes/gamma.md"]);

        f.app.filter = String::new();
        assert_eq!(f.app.visible_plans().len(), 3);
    }

    #[test]
    fn filter_keys_narrow_and_clear() {
        let mut f = Fixture::new(Tab::Plans);
        f.app.plans = vec![doc("a/alpha.md", "A"), doc("b/beta.md", "B")];
        f.app.cursor[Tab::Plans.idx()] = 1;
        f.app.on_key(key(KeyCode::Char('/')));
        assert_eq!(f.app.mode, Mode::Filter);
        type_str(&mut f.app, "beta");
        assert_eq!(f.app.count(), 1);
        assert_eq!(
            f.app.cursor[Tab::Plans.idx()],
            0,
            "typing resets the cursor"
        );
        f.app.on_key(key(KeyCode::Enter));
        assert_eq!(f.app.mode, Mode::List);
        assert_eq!(f.app.filter, "beta", "enter keeps the filter");
        f.app.on_key(key(KeyCode::Char('/')));
        f.app.on_key(key(KeyCode::Esc));
        assert_eq!(f.app.filter, "", "esc clears the filter");
    }

    #[test]
    fn filter_cleared_on_tab_switch_away() {
        let mut f = Fixture::new(Tab::Plans);
        f.app.filter = "x".to_string();
        f.app.switch_tab(Tab::Todos);
        assert_eq!(f.app.filter, "");
    }

    #[test]
    fn filter_narrows_todos_by_metadata() {
        let mut app = test_app_with_todos(&[
            ("Rotate tokens", "auth", "high"),
            ("Fix footer", "ui", "low"),
        ]);
        app.tab = Tab::Todos;
        app.filter = "auth".to_string(); // matches tag on the first only
        assert_eq!(app.count(), 1);
        assert_eq!(app.visible_todos()[0].title, "Rotate tokens");
        app.filter = "low".to_string(); // matches priority on the second
        app.clamp_cursor();
        assert_eq!(app.count(), 1);
        assert_eq!(app.selected_id(), Some(app.visible_todos()[0].id.clone()));
    }

    #[test]
    fn filter_clears_on_tab_switch() {
        let mut app = test_app_with_todos(&[("A", "", "medium")]);
        app.tab = Tab::Todos;
        app.filter = "zzz".to_string();
        app.switch_tab(Tab::Scratchpads);
        assert!(
            app.filter.is_empty(),
            "filter must clear when leaving a tab"
        );
    }

    #[test]
    fn cursor_pins_to_id_across_resort() {
        let mut f = Fixture::new(Tab::Todos);
        let a = f.store().create_todo("a", "", "high", Vec::new()).unwrap();
        let b = f.store().create_todo("b", "", "low", Vec::new()).unwrap();
        f.app.reload();
        // priority sort: high first — a at 0, b at 1
        assert_eq!(f.app.todos[0].id, a.id);
        f.app.cursor[0] = 1;
        f.app.enter_read();
        assert_eq!(f.app.read_id, b.id);
        // interfering write flips the order: b outranks a now
        f.store()
            .update_todo(
                &b.id,
                TodoUpdate {
                    priority: Some("high".to_string()),
                    ..TodoUpdate::default()
                },
            )
            .unwrap();
        f.store()
            .update_todo(
                &a.id,
                TodoUpdate {
                    priority: Some("low".to_string()),
                    ..TodoUpdate::default()
                },
            )
            .unwrap();
        f.app.reload();
        assert_eq!(f.app.todos[0].id, b.id, "list re-sorted");
        assert_eq!(
            f.app.todos[f.app.cursor[0]].id, b.id,
            "cursor followed the open item"
        );
    }

    #[test]
    fn list_mode_cursor_pins_to_selected_id() {
        let mut f = Fixture::new(Tab::Todos);
        let a = f.store().create_todo("a", "", "high", Vec::new()).unwrap();
        let b = f.store().create_todo("b", "", "low", Vec::new()).unwrap();
        f.app.reload();
        f.app.cursor[0] = 1; // select b
        f.store()
            .update_todo(
                &b.id,
                TodoUpdate {
                    priority: Some("high".to_string()),
                    ..TodoUpdate::default()
                },
            )
            .unwrap();
        f.store()
            .update_todo(
                &a.id,
                TodoUpdate {
                    priority: Some("low".to_string()),
                    ..TodoUpdate::default()
                },
            )
            .unwrap();
        f.app.reload();
        assert_eq!(f.app.todos[f.app.cursor[0]].id, b.id);
    }

    #[test]
    fn space_toggles_todo_status() {
        let mut f = Fixture::new(Tab::Todos);
        let t = f.store().create_todo("t", "", "", Vec::new()).unwrap();
        f.app.reload();
        f.app.on_key(key(KeyCode::Char(' ')));
        assert_eq!(f.store().get_todo(&t.id).unwrap().status, "completed");
        f.app.on_key(key(KeyCode::Char(' ')));
        assert_eq!(f.store().get_todo(&t.id).unwrap().status, "open");
    }

    #[test]
    fn delete_with_confirm_and_cancel() {
        let mut f = Fixture::new(Tab::Todos);
        f.store().create_todo("t", "", "", Vec::new()).unwrap();
        f.app.reload();
        f.app.on_key(key(KeyCode::Char('d')));
        assert_eq!(f.app.mode, Mode::Confirm);
        f.app.on_key(key(KeyCode::Esc)); // cancel
        assert_eq!(f.app.mode, Mode::List);
        assert_eq!(f.app.todos.len(), 1);
        f.app.on_key(key(KeyCode::Char('d')));
        f.app.on_key(key(KeyCode::Char('y')));
        assert_eq!(f.app.todos.len(), 0);
        assert!(
            f.store()
                .list_todos(TodoFilter::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn delete_scratchpad_uses_loaded_revision() {
        let mut f = Fixture::new(Tab::Scratchpads);
        f.store()
            .create_scratchpad("pad", "body", Vec::new())
            .unwrap();
        f.app.reload();
        f.app.on_key(key(KeyCode::Char('d')));
        f.app.on_key(key(KeyCode::Char('y')));
        assert!(f.app.status.is_empty(), "delete failed: {}", f.app.status);
        assert!(f.app.pads.is_empty());
    }

    #[test]
    fn create_todo_via_editor() {
        let mut f = Fixture::new(Tab::Todos);
        f.app.reload();
        f.app.on_key(key(KeyCode::Char('n')));
        assert_eq!(f.app.mode, Mode::Edit);
        assert_eq!(f.app.edit_focus, Focus::Title);
        type_str(&mut f.app, "new todo");
        f.app.on_key(key(KeyCode::Tab));
        type_str(&mut f.app, "the body");
        f.app.on_key(ctrl('d'));
        assert_eq!(f.app.mode, Mode::List);
        let todos = f.store().list_todos(TodoFilter::default()).unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].title, "new todo");
        assert_eq!(todos[0].body, "the body");
    }

    #[test]
    fn create_empty_title_rejected() {
        let mut f = Fixture::new(Tab::Todos);
        f.app.reload();
        f.app.on_key(key(KeyCode::Char('n')));
        f.app.on_key(ctrl('d'));
        assert_eq!(f.app.mode, Mode::Edit, "editor stays open");
        assert_eq!(f.app.status, "title required");
        assert!(
            f.store()
                .list_todos(TodoFilter::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn create_esc_discards_no_orphan() {
        let mut f = Fixture::new(Tab::Scratchpads);
        f.app.reload();
        f.app.on_key(key(KeyCode::Char('n')));
        type_str(&mut f.app, "typed");
        f.app.on_key(key(KeyCode::Esc));
        assert_eq!(f.app.mode, Mode::DiscardConfirm);
        f.app.on_key(key(KeyCode::Char('y')));
        assert_eq!(f.app.mode, Mode::List);
        assert!(f.app.pads.is_empty(), "no orphan pad");
    }

    #[test]
    fn edit_clean_esc_exits_without_confirm() {
        let mut f = Fixture::new(Tab::Todos);
        f.store().create_todo("t", "b", "", Vec::new()).unwrap();
        f.app.reload();
        f.app.on_key(key(KeyCode::Char('e')));
        assert_eq!(f.app.mode, Mode::Edit);
        f.app.on_key(key(KeyCode::Esc));
        assert_eq!(f.app.mode, Mode::List);
    }

    #[test]
    fn edit_saves_title_and_body_with_title_collapse() {
        let mut f = Fixture::new(Tab::Scratchpads);
        let s = f
            .store()
            .create_scratchpad("old", "body", Vec::new())
            .unwrap();
        f.app.reload();
        f.app.on_key(key(KeyCode::Char('e')));
        f.app.on_key(key(KeyCode::Tab)); // focus title
        f.app.title_ed = super::new_editor("new  title\nwrapped", true);
        f.app.on_key(ctrl('d'));
        assert_eq!(f.app.mode, Mode::List, "save failed: {}", f.app.status);
        let (got, _) = f.store().read_scratchpad(&s.id, "full", "", 0, 0).unwrap();
        assert_eq!(got.title, "new title wrapped", "whitespace collapsed");
        assert_eq!(got.content, "body");
    }

    #[test]
    fn todo_save_conflict_keeps_buffer_open() {
        let mut f = Fixture::new(Tab::Todos);
        f.store().create_todo("t", "b", "", Vec::new()).unwrap();
        f.app.reload();
        f.app.on_key(key(KeyCode::Char('e')));
        type_str(&mut f.app, "x"); // dirty the body
        // simulate a concurrent write landing after edit-entry (Updated is
        // second-granularity, so force the mismatch directly)
        f.app.edit_updated = "1999-01-01T00:00:00Z".to_string();
        f.app.on_key(ctrl('d'));
        assert_eq!(f.app.mode, Mode::Edit, "editor stays open on conflict");
        assert!(f.app.status.starts_with("save failed:"), "{}", f.app.status);
    }

    #[test]
    fn tab_click_while_editing_dirty_confirms_then_lands_on_new_tab() {
        let mut f = Fixture::new(Tab::Todos);
        f.store().create_todo("t", "b", "", Vec::new()).unwrap();
        f.app.reload();
        f.app.on_key(key(KeyCode::Char('e')));
        type_str(&mut f.app, "dirty");
        f.app.switch_tab(Tab::Scratchpads); // what a tab click resolves to
        assert_eq!(f.app.mode, Mode::DiscardConfirm);
        assert_eq!(f.app.tab, Tab::Todos, "not switched yet");
        f.app.on_key(key(KeyCode::Char('y')));
        assert_eq!(f.app.tab, Tab::Scratchpads);
        assert_eq!(f.app.mode, Mode::List);
    }

    #[test]
    fn tab_click_while_editing_clean_switches_immediately() {
        let mut f = Fixture::new(Tab::Todos);
        f.store().create_todo("t", "b", "", Vec::new()).unwrap();
        f.app.reload();
        f.app.on_key(key(KeyCode::Char('e')));
        f.app.switch_tab(Tab::Plans);
        assert_eq!(f.app.tab, Tab::Plans);
        assert_eq!(f.app.mode, Mode::List);
    }

    #[test]
    fn discard_confirm_n_returns_to_editor() {
        let mut f = Fixture::new(Tab::Todos);
        f.store().create_todo("t", "b", "", Vec::new()).unwrap();
        f.app.reload();
        f.app.on_key(key(KeyCode::Char('e')));
        type_str(&mut f.app, "z");
        f.app.on_key(key(KeyCode::Esc));
        assert_eq!(f.app.mode, Mode::DiscardConfirm);
        f.app.on_key(key(KeyCode::Char('n')));
        assert_eq!(f.app.mode, Mode::Edit, "back to editing, buffer intact");
    }

    #[test]
    fn read_mode_click_meta_segments_toggle_and_cycle() {
        let mut f = Fixture::new(Tab::Todos);
        let t = f
            .store()
            .create_todo("t", "b", "medium", Vec::new())
            .unwrap();
        f.app.reload();
        f.app.enter_read();
        // pretend the last draw put the meta row at y=4, x=0
        f.app.hits.meta = Some(super::super::view::MetaHits::new(0, 4, "open", "medium"));
        f.app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1, // inside "○ open"
            row: 4,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(f.store().get_todo(&t.id).unwrap().status, "completed");
        let (_, prio_start, _) = super::super::view::meta_segments("completed", "medium");
        f.app.hits.meta = Some(super::super::view::MetaHits::new(
            0,
            4,
            "completed",
            "medium",
        ));
        f.app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: prio_start,
            row: 4,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(f.store().get_todo(&t.id).unwrap().priority, "high");
    }

    #[test]
    fn list_click_selects_then_opens() {
        let mut f = Fixture::new(Tab::Todos);
        f.store().create_todo("a", "", "high", Vec::new()).unwrap();
        f.store().create_todo("b", "", "low", Vec::new()).unwrap();
        f.app.reload();
        f.app.hits.list = Some(super::super::view::ListHits {
            area: Rect::new(0, 2, 80, 20),
            offset: 0,
            len: 2,
        });
        let click = |row| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row,
            modifiers: KeyModifiers::NONE,
        };
        f.app.on_mouse(click(3)); // row 1: select
        assert_eq!(f.app.cursor[0], 1);
        assert_eq!(f.app.mode, Mode::List);
        f.app.on_mouse(click(3)); // same row again: open
        assert_eq!(f.app.mode, Mode::Read);
    }

    #[test]
    fn esc_quits_at_list_view() {
        let mut f = Fixture::new(Tab::Todos);
        f.app.on_key(key(KeyCode::Esc));
        assert!(f.app.quit);
    }

    #[test]
    fn c_toggles_hide_completed() {
        let mut f = Fixture::new(Tab::Todos);
        f.store().create_todo("open", "", "", Vec::new()).unwrap();
        let done = f.store().create_todo("done", "", "", Vec::new()).unwrap();
        f.store().complete_todo(&done.id, false).unwrap();
        f.app.reload();
        assert_eq!(f.app.todos.len(), 2);
        f.app.on_key(key(KeyCode::Char('c'))); // hide completed
        assert!(f.app.hide_completed);
        assert_eq!(f.app.todos.len(), 1, "completed dropped");
        assert_eq!(f.app.todos[0].title, "open");
        f.app.on_key(key(KeyCode::Char('c'))); // show again
        assert_eq!(f.app.todos.len(), 2, "completed restored");
    }
}
