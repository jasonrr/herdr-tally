// Port of internal/mcp/server.go — the stdio JSON-RPC loop and request handler.
use std::io::{BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ExitCode;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Resolve;
use super::tools;
use crate::store;

/// Go's rpcRequest. `id`/`params` were json.RawMessage; a serde_json::Value
/// captures both (id is echoed back verbatim; params is peeked field-by-field).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RpcRequest {
    jsonrpc: String,
    id: Value,
    method: String,
    params: Value,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

/// Go's rpcResponse. `result`/`error` carry omitempty; the Option + skip mirrors
/// that (a non-nil interface always serialized in Go, so an empty `{}` result is
/// still emitted — hence `ping`/`initialize` set Some(...)).
#[derive(Debug, Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

/// Runs the MCP server over real stdin/stdout with the real store resolver.
/// Go's Serve() returned 0/1; we return ExitCode the same way.
pub fn serve_stdio() -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let resolve = |o: Option<&str>| store::resolve_project(o);
    match serve(stdin.lock(), stdout.lock(), &resolve) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(1),
    }
}

/// The read loop, generic over Read/Write so tests can inject in-memory buffers
/// (Go's serve(in io.Reader, out io.Writer)). read_until has NO line-length cap
/// — the deliberate choice Go made with ReadBytes over Scanner's 64KB limit, so
/// large scratchpads round-trip intact.
fn serve<R: BufRead, W: Write>(mut r: R, mut w: W, resolve: &Resolve) -> std::io::Result<()> {
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        let n = r.read_until(b'\n', &mut line)?;
        if n > 0 {
            // Skip blank/garbage lines exactly like Go: an Unmarshal failure or
            // an empty method yields no response.
            if let Ok(req) = serde_json::from_slice::<RpcRequest>(&line)
                && !req.method.is_empty()
                && let Some(resp) = handle(req, resolve)
            {
                serde_json::to_writer(&mut w, &resp)?;
                w.write_all(b"\n")?;
            }
        }
        if n == 0 {
            return Ok(()); // EOF
        }
    }
}

/// safeDispatch: run a tool inside catch_unwind so a panicking implementation
/// becomes an error result instead of crashing the long-lived stdio process.
/// Generic over the run closure so the dispatch call site (and the panic test)
/// both funnel through the same guard, mirroring Go's recover wrapper.
fn safe_dispatch<F>(name: &str, f: F) -> Result<Value, String>
where
    F: FnOnce() -> store::Result<Value>,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(p) => Err(format!("tool {name} panicked: {}", panic_msg(&p))),
    }
}

fn panic_msg(p: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Port of server.go's handle(): returns None for notifications (no reply).
fn handle(req: RpcRequest, resolve: &Resolve) -> Option<RpcResponse> {
    let mut resp = RpcResponse {
        jsonrpc: "2.0",
        id: req.id,
        result: None,
        error: None,
    };
    match req.method.as_str() {
        "notifications/initialized" => return None, // notification: no response
        "initialize" => {
            let pv = req
                .params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("2025-06-18");
            resp.result = Some(json!({
                "protocolVersion": pv,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "herdr-notes", "version": "0.1.0"},
            }));
        }
        "ping" => resp.result = Some(json!({})),
        "tools/list" => resp.result = Some(json!({"tools": tools::tool_defs()})),
        "tools/call" => {
            let name = req
                .params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let arguments = req.params.get("arguments").cloned().unwrap_or(Value::Null);
            match safe_dispatch(&name, || tools::dispatch_tool(resolve, &name, &arguments)) {
                Ok(v) => {
                    // Go marshaled the result with 2-space indent into the text
                    // block; to_string_pretty matches.
                    let text = serde_json::to_string_pretty(&v).unwrap_or_default();
                    resp.result = Some(json!({
                        "content": [{"type": "text", "text": text}]
                    }));
                }
                Err(msg) => {
                    resp.result = Some(json!({
                        "isError": true,
                        "content": [{"type": "text", "text": msg}]
                    }));
                }
            }
        }
        _ => {
            resp.error = Some(RpcError {
                code: -32601,
                message: format!("method not found: {}", req.method),
            });
        }
    }
    Some(resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::{TempDir, git_repo};
    use std::io::Cursor;

    // Go's runServer(t, in): drive the loop over an in-memory reader/writer.
    // A temp git repo + temp store root back the resolver so no real store is
    // touched (Go used chdir + XDG_STATE_HOME; env is process-global and Rust
    // tests run in parallel, so we inject the resolver instead).
    fn run_server(input: &str) -> String {
        let repo = git_repo();
        let root = TempDir::new();
        let repo_str = repo.path().to_string_lossy().into_owned();
        let resolve =
            |o: Option<&str>| store::resolve_project_in(root.path(), o.or(Some(&repo_str)));
        let mut out: Vec<u8> = Vec::new();
        serve(Cursor::new(input.as_bytes()), &mut out, &resolve).expect("serve");
        String::from_utf8(out).unwrap()
    }

    fn non_empty(s: &str) -> Vec<&str> {
        s.split('\n').filter(|l| !l.trim().is_empty()).collect()
    }

    // Port of TestInitializeAndToolsList: verifies the stdio transport and the
    // initialize/tools-list handshake; the notification yields no response.
    #[test]
    fn test_initialize_and_tools_list() {
        let input = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ]
        .join("\n")
            + "\n";

        let out = run_server(&input);
        let lines = non_empty(&out);
        assert_eq!(
            lines.len(),
            2,
            "want 2 responses, got {}: {out:?}",
            lines.len()
        );

        let init: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "herdr-notes");
        assert!(
            init["result"]["protocolVersion"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            "protocolVersion empty: {}",
            lines[0]
        );

        let list: Value = serde_json::from_str(lines[1]).unwrap();
        assert!(
            list["result"]["tools"].as_array().is_some(),
            "tools/list did not decode to an array: {}",
            lines[1]
        );
    }

    // Port of TestSafeDispatchRecoversPanic: a panicking tool run becomes an
    // error instead of crashing the process.
    #[test]
    fn test_safe_dispatch_recovers_panic() {
        let err = safe_dispatch("__panic_test__", || -> store::Result<Value> {
            panic!("boom")
        })
        .expect_err("expected safe_dispatch to turn the panic into an error");
        assert!(
            err.contains("panicked"),
            "expected error to mention the panic, got {err:?}"
        );
    }

    // A notification (no id) must produce no response line even when it is the
    // only input — the transport-level invariant behind server.go.
    #[test]
    fn test_notification_only_yields_no_output() {
        let out = run_server("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n");
        assert!(
            non_empty(&out).is_empty(),
            "notification produced output: {out:?}"
        );
    }

    // Unknown method -> JSON-RPC error object (-32601), like the default arm.
    #[test]
    fn test_unknown_method_errors() {
        let out = run_server("{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"bogus\"}\n");
        let v: Value = serde_json::from_str(non_empty(&out)[0]).unwrap();
        assert_eq!(v["error"]["code"], -32601);
    }
}
