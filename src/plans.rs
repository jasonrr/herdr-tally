// Port of internal/docs/{config.go,list.go}. Filesystem source for the TUI's
// read-only Plans tab: it loads a per-user list of plan directories and
// lists/reads the markdown under them. Stdlib only, mirroring the Go package.
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
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

/// Walks each of `paths` (joined to `root`) collecting *.md files, sorted
/// most-recently-modified first. Missing or unreadable dirs are skipped. Port of
/// Go's `List`.
pub fn list(root: &Path, paths: &[String]) -> Vec<Plan> {
    let mut out: Vec<Plan> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for rel in paths {
        // Skip paths that escape the project root. `Path::join` REPLACES the base
        // when `rel` is absolute, so a configured "/" would walk the whole drive;
        // a `..` component would climb out of the project. Only plain relative
        // paths are browsed. (Lexical check — no FS/symlink resolution needed.)
        if !is_under_root(rel) {
            continue;
        }
        let base = root.join(rel);
        walk(&base, root, &mut seen, &mut out);
    }
    // Stable sort, most-recent-first (Go: sort.SliceStable with ModTime.After).
    out.sort_by(|a, b| b.mod_time.cmp(&a.mod_time));
    out
}

/// True only for plain relative paths (no root/prefix, no `..`), i.e. paths that
/// stay under the project root once joined. `.` components are fine.
fn is_under_root(rel: &str) -> bool {
    Path::new(rel)
        .components()
        .all(|c| matches!(c, Component::CurDir | Component::Normal(_)))
}

// Recursive directory walk mirroring Go's filepath.WalkDir: lexical order,
// errors skipped, symlinked dirs not followed (DirEntry file_type does not
// dereference).
fn walk(dir: &Path, root: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<Plan>) {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return, // skip missing/unreadable
    };
    let mut entries: Vec<fs::DirEntry> = read.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let p = entry.path();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            walk(&p, root, seen, out);
            continue;
        }
        if !entry
            .file_name()
            .to_string_lossy()
            .to_lowercase()
            .ends_with(".md")
        {
            continue;
        }
        if !seen.insert(p.clone()) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mod_time = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let rel_path = p
            .strip_prefix(root)
            .map(|r| r.to_string_lossy().into_owned())
            .unwrap_or_else(|_| p.to_string_lossy().into_owned());
        let heading = first_heading(&p);
        out.push(Plan {
            rel_path,
            abs_path: p,
            heading,
            mod_time,
        });
    }
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

    #[test]
    fn list_skips_paths_outside_root() {
        let dir = TempDir::new();
        let root = dir.path();
        write_at(&root.join("docs/plans/a.md"), "# A\n", SystemTime::now());
        // A file outside root that "/" or ".." would otherwise reach.
        write_at(
            &root.join("..").join("outside.md"),
            "# leak\n",
            SystemTime::now(),
        );

        let paths = vec![
            "docs/plans".to_string(),
            "/".to_string(),   // absolute: would walk the whole drive
            "../".to_string(), // climbs out of root
            "docs/../../outside".to_string(),
        ];
        let got = list(root, &paths);
        assert_eq!(
            got.len(),
            1,
            "only the contained path is browsed, got {got:?}"
        );
        assert_eq!(got[0].rel_path, "docs/plans/a.md");
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
