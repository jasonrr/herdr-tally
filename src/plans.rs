// Port of internal/docs/{config.go,list.go}. Filesystem source for the TUI's
// read-only Plans tab: it loads a per-user list of plan directories and
// lists/reads the markdown under them. Stdlib only, mirroring the Go package.
use ignore::Match;
use ignore::WalkBuilder;
use ignore::gitignore::GitignoreBuilder;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Browsed when no plan-paths config exists, relative to the project root.
/// Mirrors Go's `defaultPaths`.
const DEFAULT_PATHS: [&str; 3] = [
    "docs/superpowers/specs",
    "docs/superpowers/plans",
    "docs/solutions",
];

fn defaults() -> Vec<String> {
    DEFAULT_PATHS.iter().map(|s| s.to_string()).collect()
}

/// One markdown file surfaced in the Plans tab. Port of Go's `Doc`.
#[derive(Debug, Clone)]
pub struct Plan {
    /// Path relative to the project root, for display/search (Go `RelPath`).
    pub rel_path: String,
    /// Absolute path, for reading (Go `AbsPath`).
    pub abs_path: PathBuf,
    /// First heading (or first non-empty line), for display/search (Go `Heading`).
    pub heading: String,
    /// For most-recent-first sorting (Go `ModTime`).
    pub mod_time: SystemTime,
}

fn env_nonempty(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// Config directory for the plans-paths file (was Go's configFile dir).
fn config_dir_from(
    plugin_config_dir: Option<String>,
    xdg_config_home: Option<String>,
    home: Option<String>,
) -> Option<PathBuf> {
    if let Some(d) = plugin_config_dir {
        Some(PathBuf::from(d))
    } else if let Some(x) = xdg_config_home {
        Some(PathBuf::from(x).join("tally"))
    } else {
        home.map(|h| PathBuf::from(h).join(".config").join("tally"))
    }
}

fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        env_nonempty("HERDR_PLUGIN_CONFIG_DIR"),
        env_nonempty("XDG_CONFIG_HOME"),
        env_nonempty("HOME"),
    )
}

fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("plan-paths"))
}

/// Editable text shown by the TUI: one configured plan dir per line.
pub fn load_plan_paths_text() -> String {
    let mut s = load_plan_paths().join("\n");
    s.push('\n');
    s
}

/// Persists the TUI-edited plan dirs to `<config>/plan-paths`.
pub fn save_plan_paths(text: &str) -> std::io::Result<()> {
    let path = config_file()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no config directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)
}

/// Loads path list from `<dir>/plan-paths`, falling back to the legacy
/// `<dir>/doc-paths`, then to `defaults()`. Split out for testing.
fn load_plan_paths_from(dir: &Path) -> Vec<String> {
    for name in ["plan-paths", "doc-paths"] {
        let p = dir.join(name);
        if p.exists() {
            return parse_paths(&p);
        }
    }
    defaults()
}

/// Parses a newline-delimited paths file (# comments and blanks ignored),
/// returning `defaults()` when it yields nothing.
fn parse_paths(path: &Path) -> Vec<String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return defaults(),
    };
    let mut paths = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        paths.push(line.to_string());
    }
    if paths.is_empty() { defaults() } else { paths }
}

/// Reads the newline-delimited plan-paths config (falling back to the legacy
/// doc-paths filename) and returns the configured relative dirs, falling back
/// to `defaultPaths` when absent or empty. Port of Go's `LoadDocPaths`.
pub fn load_plan_paths() -> Vec<String> {
    match config_dir() {
        Some(d) => load_plan_paths_from(&d),
        None => defaults(),
    }
}

/// Surfaces the `.md` files under `root` selected by `paths`, sorted
/// most-recently-modified first. Each line in `paths` is a gitignore-style glob
/// with its sense inverted (reverse gitignore): a match *includes* the file, a
/// later `!` pattern *excludes* it, no match means not surfaced. We build a
/// `Gitignore` from the same lines and invert its verdict per file
/// (`Ignore => include`, `Whitelist => exclude`, `None => exclude`) using
/// `matched_path_or_any_parents`, so a matched *parent directory* (a bare
/// `docs/plans` or `docs/*-plans/`) includes every `.md` beneath it — the
/// back-compat behavior overrides alone don't give. The walk is rooted at
/// `root` and honors `.gitignore`, so it can't escape the repo or descend
/// `node_modules`/`target`/`.git`.
pub fn list(root: &Path, paths: &[String]) -> Vec<Plan> {
    let mut gb = GitignoreBuilder::new(root);
    for line in paths {
        // Normal gitignore sense here; we invert the verdict below. Skip lines
        // the builder rejects (malformed globs) rather than aborting the list.
        let _ = gb.add_line(None, line);
    }
    let selector = match gb.build() {
        Ok(g) => g,
        Err(_) => return Vec::new(), // malformed set => empty, not a panic
    };

    let mut out: Vec<Plan> = Vec::new();
    let walk = WalkBuilder::new(root)
        .hidden(true) // skip dotfiles/dirs
        .git_ignore(true) // never descend .gitignore'd trees (node_modules, target)
        .require_git(false) // honor .gitignore even outside a checked-out repo
        .git_global(false) // stay hermetic: ignore the user's global gitignore
        .parents(false) // don't read .gitignore above the repo root
        .build();
    for entry in walk.filter_map(Result::ok) {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let p = entry.path();
        // Reverse gitignore: an Ignore verdict (pattern matched) means include.
        if !matches!(
            selector.matched_path_or_any_parents(p, false),
            Match::Ignore(_)
        ) {
            continue;
        }
        let is_md = p
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_lowercase().ends_with(".md"))
            .unwrap_or(false);
        if !is_md {
            continue;
        }
        let mod_time = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let rel_path = p
            .strip_prefix(root)
            .map(|r| r.to_string_lossy().into_owned())
            .unwrap_or_else(|_| p.to_string_lossy().into_owned());
        let heading = first_heading(p);
        out.push(Plan {
            rel_path,
            abs_path: p.to_path_buf(),
            heading,
            mod_time,
        });
    }
    // Most-recent-first (Go: sort.SliceStable with ModTime.After).
    out.sort_by(|a, b| b.mod_time.cmp(&a.mod_time));
    out
}

/// Returns a file's contents. Port of Go's `Read`.
pub fn read(abs_path: &Path) -> std::io::Result<String> {
    fs::read_to_string(abs_path)
}

/// Returns the first markdown heading text (leading #'s stripped), falling back
/// to the first non-empty line. Port of Go's `firstHeading`.
fn first_heading(path: &Path) -> String {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let mut first_line = String::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if first_line.is_empty() {
            first_line = line.to_string();
        }
        if let Some(stripped) = line.strip_prefix('#') {
            return stripped.trim_start_matches('#').trim().to_string();
        }
    }
    first_line
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    // stdlib has no t.TempDir; make a unique dir under the system temp and
    // remove it on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!(
                "tally-docs-{}-{}-{}",
                std::process::id(),
                nanos,
                n
            ));
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_at(path: &Path, body: &str, mtime: SystemTime) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
        let f = fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(mtime).unwrap();
    }

    // --- config parsing (config_test.go) ---

    #[test]
    fn load_doc_paths_parses_file() {
        let dir = TempDir::new();
        fs::write(
            dir.path().join("doc-paths"),
            "# my docs\ndocs/plans\n\n  docs/notes  \n",
        )
        .unwrap();
        let got = load_plan_paths_from(dir.path());
        assert_eq!(got, vec!["docs/plans", "docs/notes"]);
    }

    #[test]
    fn load_doc_paths_defaults_when_absent() {
        let dir = TempDir::new(); // dir exists, no doc-paths/plan-paths file
        let got = load_plan_paths_from(dir.path());
        assert_eq!(
            got,
            vec![
                "docs/superpowers/specs",
                "docs/superpowers/plans",
                "docs/solutions"
            ]
        );
    }

    #[test]
    fn load_doc_paths_defaults_when_empty() {
        let dir = TempDir::new();
        fs::write(dir.path().join("doc-paths"), "# only comments\n\n").unwrap();
        let got = load_plan_paths_from(dir.path());
        assert_eq!(
            got.len(),
            3,
            "empty config should fall back to 3 defaults, got {got:?}"
        );
    }

    #[test]
    fn config_file_prefers_plugin_config_dir() {
        let got = config_dir_from(
            Some("/plug".to_string()),
            Some("/x".to_string()),
            Some("/h".to_string()),
        );
        assert_eq!(got, Some(PathBuf::from("/plug")));
    }

    #[test]
    fn config_file_xdg_fallback() {
        // No plugin dir: XDG_CONFIG_HOME wins over HOME and gets /tally.
        let got = config_dir_from(None, Some("/x".to_string()), Some("/h".to_string()));
        assert_eq!(got, Some(PathBuf::from("/x/tally")));
    }

    #[test]
    fn config_file_home_fallback() {
        let got = config_dir_from(None, None, Some("/h".to_string()));
        assert_eq!(got, Some(PathBuf::from("/h/.config/tally")));
    }

    // --- listing (list_test.go) ---

    #[test]
    fn list_collects_configured_markdown_sorted_by_mtime() {
        let dir = TempDir::new();
        let root = dir.path();
        let now = SystemTime::now();
        write_at(
            &root.join("docs/specs/old.md"),
            "# Old Spec\nbody",
            now - Duration::from_secs(2 * 3600),
        );
        write_at(
            &root.join("docs/specs/new.md"),
            "# New Spec\nbody",
            now - Duration::from_secs(60),
        );
        write_at(
            &root.join("docs/notes/note.md"),
            "plain first line\n",
            now - Duration::from_secs(30 * 60),
        );
        // noise that must NOT be collected:
        write_at(&root.join("docs/specs/readme.txt"), "not markdown", now);
        write_at(&root.join("other/skip.md"), "# unconfigured dir", now);

        let paths = vec![
            "docs/specs".to_string(),
            "docs/notes".to_string(),
            "docs/missing".to_string(), // does not exist
        ];
        let got = list(root, &paths);
        assert_eq!(got.len(), 3, "want 3 plans, got {got:?}");
        assert_eq!(got[0].rel_path, "docs/specs/new.md", "first (most recent)");
        assert_eq!(got[0].heading, "New Spec");

        let note = got
            .iter()
            .find(|d| d.rel_path == "docs/notes/note.md")
            .unwrap();
        assert_eq!(note.heading, "plain first line");
    }

    fn rel_paths(plans: &[Plan]) -> Vec<String> {
        let mut v: Vec<String> = plans.iter().map(|p| p.rel_path.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn glob_star_selects_subset() {
        let dir = TempDir::new();
        let root = dir.path();
        let now = SystemTime::now();
        write_at(&root.join("docs/a-plans/x.md"), "# X\n", now);
        write_at(&root.join("docs/b-plans/y.md"), "# Y\n", now);
        write_at(&root.join("docs/notes/z.md"), "# Z\n", now);

        let got = list(root, &["docs/*-plans/".to_string()]);
        assert_eq!(
            rel_paths(&got),
            vec!["docs/a-plans/x.md", "docs/b-plans/y.md"]
        );
    }

    #[test]
    fn glob_double_star_any_depth() {
        let dir = TempDir::new();
        let root = dir.path();
        let now = SystemTime::now();
        write_at(&root.join("design/a.md"), "# A\n", now);
        write_at(&root.join("src/ui/design/b.md"), "# B\n", now);
        write_at(&root.join("docs/other/c.md"), "# C\n", now);

        let got = list(root, &["**/design/*.md".to_string()]);
        assert_eq!(rel_paths(&got), vec!["design/a.md", "src/ui/design/b.md"]);
    }

    #[test]
    fn negation_excludes() {
        let dir = TempDir::new();
        let root = dir.path();
        let now = SystemTime::now();
        write_at(&root.join("docs/a.md"), "# A\n", now);
        write_at(&root.join("docs/archive/old.md"), "# Old\n", now);

        let paths = vec!["docs/**".to_string(), "!docs/archive/**".to_string()];
        let got = list(root, &paths);
        assert_eq!(rel_paths(&got), vec!["docs/a.md"]);
    }

    #[test]
    fn bare_dir_still_recursive_and_backcompat() {
        // A no-wildcard entry (like every existing config) still pulls every .md
        // beneath it via parent-match — proving old configs keep working.
        let dir = TempDir::new();
        let root = dir.path();
        let now = SystemTime::now();
        write_at(&root.join("docs/plans/a.md"), "# A\n", now);
        write_at(&root.join("docs/plans/deep/b.md"), "# B\n", now);
        write_at(&root.join("other/c.md"), "# C\n", now);

        let got = list(root, &["docs/plans".to_string()]);
        assert_eq!(
            rel_paths(&got),
            vec!["docs/plans/a.md", "docs/plans/deep/b.md"]
        );
    }

    #[test]
    fn respects_gitignore() {
        let dir = TempDir::new();
        let root = dir.path();
        let now = SystemTime::now();
        fs::write(root.join(".gitignore"), "build/\n").unwrap();
        write_at(&root.join("docs/a.md"), "# A\n", now);
        write_at(&root.join("build/gen.md"), "# Gen\n", now);

        let got = list(root, &["**/*.md".to_string()]);
        assert_eq!(rel_paths(&got), vec!["docs/a.md"]);
    }

    #[test]
    fn non_md_filtered() {
        let dir = TempDir::new();
        let root = dir.path();
        let now = SystemTime::now();
        write_at(&root.join("docs/a.md"), "# A\n", now);
        write_at(&root.join("docs/readme.txt"), "not markdown", now);

        let got = list(root, &["docs/**".to_string()]);
        assert_eq!(rel_paths(&got), vec!["docs/a.md"]);
    }

    #[test]
    fn absolute_and_parent_lines_match_nothing() {
        // Replaces list_skips_paths_outside_root: the walk is root-confined, so
        // absolute / `..` lines simply select nothing instead of leaking.
        let dir = TempDir::new();
        let root = dir.path();
        write_at(&root.join("docs/plans/a.md"), "# A\n", SystemTime::now());
        write_at(
            &root.join("..").join("outside.md"),
            "# leak\n",
            SystemTime::now(),
        );

        let paths = vec![
            "docs/plans".to_string(),
            "/".to_string(),
            "../".to_string(),
            "docs/../../outside".to_string(),
        ];
        let got = list(root, &paths);
        assert_eq!(rel_paths(&got), vec!["docs/plans/a.md"]);
    }

    #[test]
    fn read_returns_contents() {
        let dir = TempDir::new();
        let p = dir.path().join("docs/specs/a.md");
        write_at(&p, "# A\nhello", SystemTime::now());
        let got = read(&p).unwrap();
        assert_eq!(got, "# A\nhello");
    }

    #[test]
    fn load_plan_paths_prefers_plan_paths_then_falls_back_to_doc_paths() {
        let dir = TempDir::new();
        // legacy file only -> used as fallback
        fs::write(dir.path().join("doc-paths"), "docs/legacy\n").unwrap();
        let got = load_plan_paths_from(dir.path());
        assert_eq!(got, vec!["docs/legacy"]);
        // when both exist, plan-paths wins
        fs::write(dir.path().join("plan-paths"), "docs/new\n").unwrap();
        let got = load_plan_paths_from(dir.path());
        assert_eq!(got, vec!["docs/new"]);
    }
}
