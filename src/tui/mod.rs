//! TUI adapter: ratatui + crossterm rewrite of the Go bubbletea panes. Entry
//! is `tally tui <todos|scratchpads> [--project PATH]`; the Plans tab is
//! reachable from either via key 3 or a tab click. Mouse is first-class:
//! SGR capture is enabled and every mode handles wheel + click (the whole
//! point of the Rust port).
mod app;
mod markdown;
mod time;
mod view;

use std::io::stdout;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::supports_keyboard_enhancement;

use crate::cli::{parse, project_opt};
use crate::store::resolve_project;

use app::{App, Tab};

/// How often the store is re-read while idle (the Go tickCmd cadence).
const POLL: Duration = Duration::from_secs(2);

pub fn run(args: &[String]) -> ExitCode {
    let usage = "usage: tally tui <todos|scratchpads> [--project PATH]";
    let Some(kind) = args.first() else {
        eprintln!("{usage}");
        return ExitCode::from(2);
    };
    let initial = match kind.as_str() {
        "todos" => Tab::Todos,
        "scratchpads" => Tab::Scratchpads,
        _ => {
            eprintln!("unknown tui kind: {kind}");
            return ExitCode::from(2);
        }
    };
    let parsed = match parse(&args[1..], &[], &["project"], &[]) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let project = parsed.str("project", "");
    let mut p = match resolve_project(project_opt(&project)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    // The TUI is driven by you; attribute its writes to "you" rather than the
    // agent/env default.
    p.actor = "you".to_string();

    let mut a = App::new(p, initial);
    a.reload();
    let mut terminal = ratatui::init(); // altscreen + raw mode + panic hook
    let _ = execute!(stdout(), EnableMouseCapture, EnableBracketedPaste);
    // Kitty protocol lets us tell Ctrl+Enter apart from plain Enter (the save
    // alias). Only push where the terminal supports it; on the rest Ctrl+D
    // stays the universal save key. Track whether we pushed so we don't pop a
    // flag we never set.
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    let res = event_loop(&mut terminal, &mut a);
    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableBracketedPaste, DisableMouseCapture);
    ratatui::restore();
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, a: &mut App) -> std::io::Result<()> {
    let mut last_load = Instant::now();
    loop {
        terminal.draw(|f| view::draw(a, f))?;
        let timeout = POLL.saturating_sub(last_load.elapsed());
        if event::poll(timeout)? {
            // Coalesce: drain every input already buffered before redrawing, so a
            // burst of mouse-wheel ticks (which arrive far faster than a herdr
            // pane can flush a full-viewport repaint) collapses into ONE flush
            // instead of one stop-motion frame per tick. One event still behaves
            // exactly as before.
            loop {
                match event::read()? {
                    Event::Key(k) if k.kind != KeyEventKind::Release => a.on_key(k),
                    Event::Mouse(m) => a.on_mouse(m),
                    Event::Paste(s) => a.on_paste(s),
                    _ => {} // Resize redraws on the next loop pass
                }
                if a.quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if last_load.elapsed() >= POLL {
            a.reload();
            last_load = Instant::now();
        }
        if a.quit {
            return Ok(());
        }
    }
}
