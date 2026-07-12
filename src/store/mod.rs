// Store: single source of truth, same rule as the Go original. CLI/MCP/TUI
// stay thin adapters over this module.
mod comments;
mod errors;
mod ids;
mod lock;
mod project;
mod scratchpads;
mod sync;
mod todos;

#[cfg(test)]
pub(crate) mod testutil;

pub use comments::{Comment, CommentSummary};
pub use errors::{Error, Result};
pub use project::{Project, resolve_project, resolve_project_in};
pub use scratchpads::{EditTarget, Scratchpad};
pub(crate) use scratchpads::{norm_heading, parse_headings};
pub use sync::{Gh, GhCli, SyncReport, sync_project};
pub use todos::{GithubLink, Todo, TodoFilter, TodoUpdate};
