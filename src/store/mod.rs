// Store: single source of truth, same rule as the Go original. CLI/MCP/TUI
// stay thin adapters over this module.
mod amdoc;
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

/// autosurgeon 0.13 has no `#[autosurgeon(skip)]`; `#[autosurgeon(with =
/// "crate::store::am_skip")]` is the equivalent. `reconcile` is a no-op — it
/// never touches the prop reconciler, so nothing is written and the field is
/// left out of the automerge doc; `hydrate` ignores the doc and returns the
/// field's `Default`, so the never-written key hydrates cleanly instead of
/// erroring as missing. Used for fields carried outside the derived map:
/// `Todo.extra` (an untyped `serde_json::Value` map autosurgeon can't model —
/// doc-resident unknown keys survive reconcile anyway, since it never prunes and
/// list elements diff by `#[key]` identity, not position) and `Scratchpad.content`
/// (written separately as an automerge `Text` child). Caveat: am_skip writes
/// NOTHING, so a struct-resident `extra` reconciled into a FRESH doc is dropped —
/// migration (Task 6) must write those keys explicitly.
pub(crate) mod am_skip {
    use autosurgeon::{HydrateError, Prop, ReadDoc, Reconciler};

    pub(crate) fn reconcile<T, R: Reconciler>(_value: &T, _reconciler: R) -> Result<(), R::Error> {
        Ok(())
    }

    pub(crate) fn hydrate<T: Default, D: ReadDoc>(
        _doc: &D,
        _obj: &automerge::ObjId,
        _prop: Prop<'_>,
    ) -> Result<T, HydrateError> {
        Ok(T::default())
    }
}

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
