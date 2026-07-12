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
// Public sync vocabulary. `IssueState`/`IssueSnapshot` are the `Gh` trait's return
// types (anyone implementing `Gh` needs them) and `GithubLink` is the type of the
// public `Todo::github` field. Named only by tests today (and kept public as the
// store's flat API surface); the non-test binary reaches them only through `Gh`
// methods and the `Todo::github` field, so `unused_imports` false-positives.
#[allow(unused_imports)]
pub use sync::{IssueSnapshot, IssueState};
#[allow(unused_imports)]
pub use todos::GithubLink;
pub use todos::{Todo, TodoFilter, TodoUpdate};
