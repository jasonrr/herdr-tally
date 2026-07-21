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
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::supports_keyboard_enhancement;

use crate::cli::{parse, project_opt};
use crate::store::{GhCli, resolve_project, sync_project};

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

    let project_path = p.path.to_string_lossy().into_owned();
    let mut a = App::new(p, initial);
    a.sync_tx = Some(spawn_sync_worker(project_path, a.sync_status.clone()));
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
    // repro(herdr#1295): per-frame render-vs-flush timings land here so the
    // maintainer can capture the PTY-write stall. Overridable via TALLY_REPRO_LOG.
    let log_path =
        std::env::var("TALLY_REPRO_LOG").unwrap_or_else(|_| "/tmp/tally-herdr-1295.log".into());
    let res = event_loop(&mut terminal, &mut a, &log_path);
    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableBracketedPaste, DisableMouseCapture);
    ratatui::restore();
    eprintln!("[repro herdr#1295] per-frame timings written to {log_path}");
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

/// Background reconcile loop: every 60s (or on nudge) run one sync pass and
/// publish a one-line status. Builds its own Project from the path so the store
/// flock is the only shared state (safe cross-thread). Errors degrade to status.
fn spawn_sync_worker(project_path: String, status: Arc<Mutex<String>>) -> mpsc::Sender<()> {
    let (tx, rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        loop {
            match resolve_project(Some(&project_path)) {
                Ok(mut p) => {
                    // Panic-isolate: a panic inside sync_project must not kill this
                    // thread, or background sync silently stops for the rest of the
                    // TUI session (footer freezes, no signal). Mirrors the MCP
                    // server's catch_unwind discipline (see CLAUDE.md). The next
                    // loop iteration re-resolves its own Project, so this self-heals.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        sync_project(&mut p, &GhCli)
                    }));
                    match result {
                        Ok(rep) => {
                            if let Ok(mut s) = status.lock() {
                                *s = app::summarize(&rep);
                            }
                        }
                        Err(_) => {
                            if let Ok(mut s) = status.lock() {
                                *s = "⚠ sync worker error (recovered)".to_string();
                            }
                        }
                    }
                }
                Err(e) => {
                    if let Ok(mut s) = status.lock() {
                        *s = format!("sync: {e}");
                    }
                }
            }
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    tx
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    a: &mut App,
    log_path: &str,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut last_load = Instant::now();
    let mut log = std::fs::File::create(log_path).ok();
    // What caused the frame we're about to draw (issue's "trigger" column).
    let mut trigger = "init";
    loop {
        // Split Terminal::draw into render (the closure building the back buffer)
        // vs flush (ratatui's write to the child PTY returning) — the render stays
        // microseconds; the flush is where the herdr pane stalls 40-500ms.
        let mut render = Duration::ZERO;
        let t0 = Instant::now();
        terminal.draw(|f| {
            let r = Instant::now();
            view::draw(a, f);
            render = r.elapsed();
        })?;
        let total = t0.elapsed();
        let flush = total.saturating_sub(render);
        if let Some(f) = log.as_mut() {
            let _ = writeln!(
                f,
                "{trigger:<6} total={total:>10.2?}  render={render:>9.2?}  flush={flush:>10.2?}  mode={:?} scroll={}",
                a.mode, a.read_scroll
            );
        }
        let timeout = POLL.saturating_sub(last_load.elapsed());
        if event::poll(timeout)? {
            // NO coalescing here (the 8780dcf workaround is deliberately removed):
            // one input event => one redraw => one flush, so a mouse-wheel burst
            // replays the per-flush stall frame-by-frame. This is the repro.
            match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    trigger = "key";
                    a.on_key(k);
                }
                Event::Mouse(m) => {
                    trigger = "mouse";
                    a.on_mouse(m);
                }
                Event::Paste(s) => {
                    trigger = "paste";
                    a.on_paste(s);
                }
                _ => trigger = "other", // Resize redraws on the next loop pass
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
