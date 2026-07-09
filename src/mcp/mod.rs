// MCP adapter: a thin stdio JSON-RPC 2.0 server over the store, port of
// internal/mcp (server.go + tools.go). Like the Go original, `store` stays the
// single source of truth; this module only marshals JSON to/from store calls.
//
// Transport is newline-delimited JSON-RPC (NOT LSP Content-Length framing),
// `notifications/initialized` gets no response, and a panicking tool is turned
// into an isError result instead of crashing the process (Go used recover; we
// use catch_unwind — see server::safe_dispatch).
mod server;
mod tools;

pub use server::serve_stdio;

use crate::store::{self, Project};

/// A project resolver: maps an optional `--project`/`project` override to a
/// resolved [`Project`]. Production uses `store::resolve_project` (reads
/// XDG_STATE_HOME); tests inject `store::resolve_project_in` over a temp root so
/// they never touch the real store. Mirrors Go's `store.ResolveProject(a.Project)`
/// call inside dispatchTool, lifted to a parameter for test isolation.
pub(crate) type Resolve<'a> = dyn Fn(Option<&str>) -> store::Result<Project> + 'a;
