mod cli;
mod mcp;
mod plans;
mod store;
mod tui;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("todos") => cli::todos(&args[1..]),
        Some("scratchpads") => cli::scratchpads(&args[1..]),
        Some("comments") => cli::comments(&args[1..]),
        Some("sync") => cli::sync(&args[1..]),
        Some("mcp") => mcp::serve_stdio(),
        Some("tui") => tui::run(&args[1..]),
        _ => {
            eprintln!("usage: tally <todos|scratchpads|comments|sync|mcp|tui> ...");
            ExitCode::from(2)
        }
    }
}
