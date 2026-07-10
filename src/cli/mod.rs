//! CLI adapter over the store — port of internal/cli. Thin: parse args (Go
//! stdlib-`flag` grammar, hand-rolled), call exactly one store method, then
//! emit JSON or a glow-style markdown render. All logic lives in the store; if
//! this and the MCP adapter ever disagree about an operation, that's a bug.
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use serde::Serialize;

use crate::store::{Project, resolve_project, resolve_project_in};

mod comments;
mod render;
mod scratchpads;
mod todos;

/// `tally todos …` entry: real store root, stdout.
pub fn todos(args: &[String]) -> ExitCode {
    exit(todos::run(args, None, &mut io::stdout()))
}

/// `tally scratchpads …` entry: real store root, stdout.
pub fn scratchpads(args: &[String]) -> ExitCode {
    exit(scratchpads::run(args, None, &mut io::stdout()))
}

/// `tally comments …` entry: real store root, stdout.
pub fn comments(args: &[String]) -> ExitCode {
    exit(comments::run(args, None, &mut io::stdout()))
}

/// Only main turns a code into a process exit — the run functions return codes
/// (Go's `fail` pattern) so they stay testable.
fn exit(code: i32) -> ExitCode {
    ExitCode::from(code as u8)
}

/// Go's fail(...): "error: <msg>" to stderr, returns exit code 1.
pub(crate) fn fail(msg: &str) -> i32 {
    eprintln!("error: {msg}");
    1
}

/// ResolveProject with an optional injected store root: tests pass a temp dir,
/// production passes None (→ the XDG_STATE_HOME/HOME default computed in the
/// store). Mirrors the Go CLI's single ResolveProject call.
pub(crate) fn resolve(
    project: Option<&str>,
    store_root: Option<&Path>,
) -> crate::store::Result<Project> {
    match store_root {
        Some(r) => resolve_project_in(r, project),
        None => resolve_project(project),
    }
}

/// Port of bodyFrom: `--body-file -` reads stdin, a non-empty path reads that
/// file, otherwise the inline `--body` value is used. Shared with `--content`.
pub(crate) fn body_from(body: &str, body_file: &str) -> io::Result<String> {
    if body_file == "-" {
        let mut s = String::new();
        io::stdin().read_to_string(&mut s)?;
        Ok(s)
    } else if !body_file.is_empty() {
        Ok(String::from_utf8_lossy(&std::fs::read(body_file)?).into_owned())
    } else {
        Ok(body.to_string())
    }
}

/// json.MarshalIndent(v, "", "  ") + Println: pretty JSON, trailing newline.
pub(crate) fn print_json<T: Serialize>(out: &mut dyn Write, v: &T) -> io::Result<()> {
    let s = serde_json::to_string_pretty(v).unwrap_or_default();
    writeln!(out, "{s}")
}

/// Go passed the (possibly empty) --project string straight to ResolveProject,
/// where "" means cwd. Map "" → None to reproduce that.
pub(crate) fn project_opt(project: &str) -> Option<&str> {
    if project.is_empty() {
        None
    } else {
        Some(project)
    }
}

/// Parsed flags. Reproduces the grammar Go's stdlib `flag` accepts: `-x`/`--x`,
/// `-x val`/`--x=val`, bool flags that consume no value (unless `=`), and
/// repeatable value flags collected in order. Parsing stops at the first
/// non-flag token or `--`, exactly like flag.Parse (so an id extracted ahead of
/// the flags is required — see the callers' id-first split).
pub(crate) struct Parsed {
    vals: HashMap<String, Vec<String>>,
    bools: HashMap<String, bool>,
}

impl Parsed {
    /// Last value wins (Go's single-valued string flags), else the default.
    pub(crate) fn str(&self, name: &str, default: &str) -> String {
        self.vals
            .get(name)
            .and_then(|v| v.last())
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }
    /// All values, in order — the repeatable `--tag`/`--blocker` flags.
    pub(crate) fn multi(&self, name: &str) -> Vec<String> {
        self.vals.get(name).cloned().unwrap_or_default()
    }
    pub(crate) fn int(&self, name: &str, default: i64) -> i64 {
        self.vals
            .get(name)
            .and_then(|v| v.last())
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }
    pub(crate) fn boolean(&self, name: &str, default: bool) -> bool {
        self.bools.get(name).copied().unwrap_or(default)
    }
    /// Go's fs.Visit membership: was the flag explicitly provided?
    pub(crate) fn was_set(&self, name: &str) -> bool {
        self.vals.contains_key(name) || self.bools.contains_key(name)
    }
}

pub(crate) fn parse(
    args: &[String],
    bool_flags: &[&str],
    value_flags: &[&str],
    int_flags: &[&str],
) -> Result<Parsed, String> {
    let mut vals: HashMap<String, Vec<String>> = HashMap::new();
    let mut bools: HashMap<String, bool> = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            break; // explicit terminator
        }
        if !a.starts_with('-') || a == "-" {
            break; // first non-flag token: flag.Parse stops here
        }
        let body = a
            .strip_prefix("--")
            .unwrap_or_else(|| a.strip_prefix('-').unwrap());
        let (name, inline) = match body.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (body, None),
        };
        if bool_flags.contains(&name) {
            let v = match inline {
                Some(s) => parse_bool(&s)?,
                None => true, // Go bool flags don't consume the next arg
            };
            bools.insert(name.to_string(), v);
            i += 1;
        } else if value_flags.contains(&name) {
            let v = match inline {
                Some(s) => {
                    i += 1;
                    s
                }
                None => {
                    // Non-bool flags consume the next token even if it looks
                    // like a flag (this is how `--body-file -` gets its "-").
                    if i + 1 >= args.len() {
                        return Err(format!("flag needs an argument: -{name}"));
                    }
                    let s = args[i + 1].clone();
                    i += 2;
                    s
                }
            };
            if int_flags.contains(&name) && v.parse::<i64>().is_err() {
                return Err(format!("invalid value \"{v}\" for flag -{name}"));
            }
            vals.entry(name.to_string()).or_default().push(v);
        } else {
            return Err(format!("flag provided but not defined: -{name}"));
        }
    }
    Ok(Parsed { vals, bools })
}

/// strconv.ParseBool's accepted forms.
fn parse_bool(s: &str) -> Result<bool, String> {
    match s {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Ok(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Ok(false),
        _ => Err(format!("invalid boolean value {s:?}")),
    }
}

#[cfg(test)]
mod tests {
    use crate::store::testutil::{TempDir, git_repo};

    /// A CLI harness over a fresh git repo + throwaway store root. Mirrors the
    /// Go tests' repo(t) + XDG_STATE_HOME isolation, but injects the store root
    /// and passes --project explicitly instead of touching cwd/env (Rust tests
    /// run in parallel; env and cwd are process-global).
    struct Cli {
        root: TempDir,
        repo: TempDir,
    }

    impl Cli {
        fn new() -> Cli {
            Cli {
                root: TempDir::new(),
                repo: git_repo(),
            }
        }
        fn with_project(&self, argv: &[&str]) -> Vec<String> {
            let mut a: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
            a.push("--project".to_string());
            a.push(self.repo.path().to_string_lossy().into_owned());
            a
        }
        fn todos(&self, argv: &[&str]) -> (i32, String) {
            let args = self.with_project(argv);
            let mut buf = Vec::new();
            let code = super::todos::run(&args, Some(self.root.path()), &mut buf);
            (code, String::from_utf8(buf).unwrap())
        }
        fn scratch(&self, argv: &[&str]) -> (i32, String) {
            let args = self.with_project(argv);
            let mut buf = Vec::new();
            let code = super::scratchpads::run(&args, Some(self.root.path()), &mut buf);
            (code, String::from_utf8(buf).unwrap())
        }
    }

    #[derive(serde::Deserialize)]
    struct TodoList {
        todos: Vec<crate::store::Todo>,
    }

    #[test]
    fn todos_create_list_json() {
        let cli = Cli::new();
        assert_eq!(
            cli.todos(&["create", "--title", "Hello", "--priority", "high"])
                .0,
            0
        );
        let (_, out) = cli.todos(&["list", "--json"]);
        let got: TodoList =
            serde_json::from_str(&out).unwrap_or_else(|e| panic!("json: {e} ({out:?})"));
        assert_eq!(got.todos.len(), 1);
        assert_eq!(got.todos[0].title, "Hello");
    }

    #[test]
    fn todos_list_markdown() {
        let cli = Cli::new();
        cli.todos(&["create", "--title", "Buy milk"]);
        let (_, out) = cli.todos(&["list"]);
        assert!(
            out.contains("Buy milk") && out.contains('#'),
            "markdown: {out:?}"
        );
    }

    // Guards the two behaviors Go's flag.Parse got wrong (stopping at the first
    // positional): flags AFTER the id must apply, and update --tag REPLACES
    // while add-tag ADDs.
    #[test]
    fn todos_tag_routing() {
        let cli = Cli::new();
        assert_eq!(
            cli.todos(&["create", "--title", "Tagged", "--tag", "a", "--tag", "b"])
                .0,
            0
        );
        let (_, out) = cli.todos(&["list", "--json"]);
        let listed: TodoList = serde_json::from_str(&out).unwrap();
        assert_eq!(listed.todos.len(), 1);
        let id = listed.todos[0].id.clone();

        assert_eq!(cli.todos(&["update", &id, "--tag", "c"]).0, 0);
        let (_, out) = cli.todos(&["list", "--json"]);
        let listed: TodoList = serde_json::from_str(&out).unwrap();
        assert_eq!(
            listed.todos[0].tags,
            vec!["c"],
            "update should replace tags"
        );

        assert_eq!(cli.todos(&["add-tag", &id, "--tag", "d"]).0, 0);
        let (_, out) = cli.todos(&["get", &id, "--json"]);
        let got: crate::store::Todo = serde_json::from_str(&out).unwrap();
        assert!(
            got.tags.contains(&"c".to_string()) && got.tags.contains(&"d".to_string()),
            "after add-tag: {:?}",
            got.tags
        );

        assert_eq!(cli.todos(&["remove-tag", &id, "--tag", "c"]).0, 0);
        let (_, out) = cli.todos(&["get", &id, "--json"]);
        let got: crate::store::Todo = serde_json::from_str(&out).unwrap();
        assert!(
            !got.tags.contains(&"c".to_string()),
            "c not removed: {:?}",
            got.tags
        );
        assert!(
            got.tags.contains(&"d".to_string()),
            "d missing: {:?}",
            got.tags
        );
    }

    #[test]
    fn todos_flags_after_id() {
        let cli = Cli::new();
        assert_eq!(cli.todos(&["create", "--title", "Flag order"]).0, 0);
        let (_, out) = cli.todos(&["list", "--json"]);
        let listed: TodoList = serde_json::from_str(&out).unwrap();
        let id = listed.todos[0].id.clone();

        assert_eq!(cli.todos(&["update", &id, "--status", "in_progress"]).0, 0);
        let (_, out) = cli.todos(&["get", &id, "--json"]);
        let got: crate::store::Todo = serde_json::from_str(&out).unwrap();
        assert_eq!(got.status, "in_progress");
    }

    #[test]
    fn scratchpads_create_read_list() {
        let cli = Cli::new();
        let (_, out) = cli.scratch(&[
            "create",
            "--name",
            "Plan",
            "--content",
            "# Plan\nbody",
            "--json",
        ]);
        let s: crate::store::Scratchpad =
            serde_json::from_str(&out).unwrap_or_else(|e| panic!("create json: {e} ({out:?})"));
        let (_, read) = cli.scratch(&["read", &s.id, "--mode", "content"]);
        assert!(read.contains("body"), "read: {read:?}");
        let (_, list) = cli.scratch(&["list", "--json"]);
        assert!(list.contains("Plan"), "list: {list:?}");
    }

    #[test]
    fn scratchpads_revision_guard() {
        let cli = Cli::new();
        let (_, out) = cli.scratch(&["create", "--name", "x", "--content", "# x\n", "--json"]);
        let s: crate::store::Scratchpad = serde_json::from_str(&out).unwrap();
        let (code, _) = cli.scratch(&[
            "append",
            &s.id,
            "--content",
            "more",
            "--expected-revision",
            "99",
        ]);
        assert_ne!(code, 0, "stale revision append should fail");
    }

    #[test]
    fn scratchpads_clear_requires_expected_revision() {
        let cli = Cli::new();
        let (_, out) = cli.scratch(&["create", "--name", "x", "--content", "# x\n", "--json"]);
        let s: crate::store::Scratchpad = serde_json::from_str(&out).unwrap();
        let (code, _) = cli.scratch(&["clear", &s.id]);
        assert_ne!(code, 0, "clear without --expected-revision should fail");
    }
}
