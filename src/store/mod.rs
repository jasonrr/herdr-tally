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
// public `Todo::github` field. In this binary crate the non-test code never *names*
// them — it only calls `Gh` methods and reads the field — so `unused_imports` flags
// these re-exports: a false positive for intentional public API. Consumed by tests
// and by any external embedder of the `store` module.
#[allow(unused_imports)]
pub use sync::{IssueSnapshot, IssueState};
#[allow(unused_imports)]
pub use todos::GithubLink;
pub use todos::{Todo, TodoFilter, TodoUpdate};
