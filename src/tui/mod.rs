//! TUI adapter: ratatui + crossterm rewrite of the Go bubbletea panes. Entry
//! is `herdr-notes tui <todos|scratchpads> [--project PATH]`; the Docs tab is
//! reachable from either via key 3 or a tab click. Mouse is first-class:
//! SGR capture is enabled and every mode handles wheel + click (the whole
//! point of the Rust port).
mod app;
mod markdown;
mod view;

use std::io::stdout;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyEventKind,
};
use crossterm::execute;

use crate::cli::{parse, project_opt};
use crate::store::resolve_project;

use app::{App, Tab};

/// How often the store is re-read while idle (the Go tickCmd cadence).
const POLL: Duration = Duration::from_secs(2);

pub fn run(args: &[String]) -> ExitCode {
    let usage = "usage: herdr-notes tui <todos|scratchpads> [--project PATH]";
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
    let p = match resolve_project(project_opt(&project)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    let mut a = App::new(p, initial);
    a.reload();
    let mut terminal = ratatui::init(); // altscreen + raw mode + panic hook
    let _ = execute!(stdout(), EnableMouseCapture, EnableBracketedPaste);
    let res = event_loop(&mut terminal, &mut a);
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
            match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => a.on_key(k),
                Event::Mouse(m) => a.on_mouse(m),
                Event::Paste(s) => a.on_paste(s),
                _ => {} // Resize redraws on the next loop pass
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
