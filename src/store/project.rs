// Port of internal/store/project.go. The store key MUST stay byte-compatible
// with the Go binary — `<base>-<sha1(abspath)[:8]>` — so this binary finds the
// data the Go version wrote. See test_project_key_matches_go_store.
use std::path::{Path, PathBuf};
use std::process::Command;

use super::errors::Result;
use super::todos::now;

pub struct Project {
    pub path: PathBuf,
    pub name: String,
    pub dir: PathBuf,
}

fn store_root() -> PathBuf {
    // Data lives under `tally/` (renamed from the original `herdr-notes/`; the live
    // dir was moved to match). The store *key* below (project_key) is unaffected —
    // it hashes the project path, not the app name — so the Go golden test still holds.
    if let Ok(x) = std::env::var("XDG_STATE_HOME")
        && !x.is_empty()
    {
        return PathBuf::from(x).join("tally");
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".local/state/tally")
}

pub fn project_key(abs: &str) -> String {
    let name = Path::new(abs)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let hex = sha1_smol::Sha1::from(abs.as_bytes()).digest().to_string();
    format!("{name}-{}", &hex[..8])
}

/// Finds the project for `override_dir` or, if None, cwd. The project root is
/// the MAIN working tree for the dir's repository (keyed on
/// `git rev-parse --git-common-dir`, shared across worktrees), falling back to
/// the dir itself when not in a repo. Reads XDG_STATE_HOME for the store root
/// (identical CLI behavior to Go).
pub fn resolve_project(override_dir: Option<&str>) -> Result<Project> {
    resolve_project_in(&store_root(), override_dir)
}

/// resolve_project with an explicit store root. Tests pass a temp dir here
/// instead of mutating XDG_STATE_HOME (env is process-global and Rust tests
/// run in parallel; the Go tests could get away with t.Setenv).
pub fn resolve_project_in(store_root: &Path, override_dir: Option<&str>) -> Result<Project> {
    let dir = match override_dir {
        Some(d) => PathBuf::from(d),
        None => std::env::current_dir()?,
    };
    let mut abs = std::path::absolute(&dir)?;
    if let Some(root) = git_project_root(&abs) {
        abs = root;
    }
    if let Ok(resolved) = abs.canonicalize() {
        abs = resolved;
    }
    let abs_str = abs.to_string_lossy().into_owned();
    let key = project_key(&abs_str);
    let name = Path::new(&abs_str)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = store_root.join("projects").join(&key);
    std::fs::create_dir_all(dir.join("scratchpads"))?;
    let p = Project {
        path: abs,
        name,
        dir,
    };
    p.write_project_json();
    Ok(p)
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn git_project_root(dir: &Path) -> Option<PathBuf> {
    if let Some(common) = git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) {
        let common = PathBuf::from(common);
        if common.file_name().is_some_and(|n| n == ".git") {
            return common.parent().map(Path::to_path_buf); // /repo/.git -> /repo
        }
        return Some(common); // bare/unusual layout: use the common dir itself
    }
    git(dir, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

impl Project {
    pub(crate) fn todos_path(&self) -> PathBuf {
        self.dir.join("todos.json")
    }

    pub(crate) fn scratch_dir(&self) -> PathBuf {
        self.dir.join("scratchpads")
    }

    /// Per-project TUI preferences (hide-completed, later per-tab filters).
    /// TUI-only state, so it lives outside the todo/scratchpad domain files.
    pub(crate) fn ui_state_path(&self) -> PathBuf {
        self.dir.join("ui.json")
    }

    /// Port of writeProjectJSON: a best-effort breadcrumb written once on
    /// first resolve; errors deliberately ignored (Go discarded them too).
    fn write_project_json(&self) {
        let path = self.dir.join("project.json");
        if path.exists() {
            return;
        }
        let v = serde_json::json!({
            "created": now(),
            "name": self.name,
            "path": self.path.to_string_lossy(),
        });
        if let Ok(s) = serde_json::to_string_pretty(&v) {
            let _ = std::fs::write(path, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::{TempDir, git_repo};

    // Golden value taken from the LIVE Go store: the Go binary keyed
    // /Users/jasonrosoff/Code/herdr-notes to herdr-notes-d0fcfa32. If this
    // test breaks, existing project data orphans — do not "fix" the test.
    #[test]
    fn test_project_key_matches_go_store() {
        assert_eq!(
            project_key("/Users/jasonrosoff/Code/herdr-notes"),
            "herdr-notes-d0fcfa32"
        );
    }

    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    // Go's initRepo(t): init plus an empty commit (worktree add needs one).
    fn init_repo(dir: &Path) {
        let cmds: [&[&str]; 2] = [
            &["init"],
            &[
                "-c",
                "user.email=t@example.com",
                "-c",
                "user.name=t",
                "commit",
                "--allow-empty",
                "-m",
                "init",
            ],
        ];
        for args in cmds {
            let out = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    #[test]
    fn test_resolve_project_uses_git_root() {
        let repo = git_repo();
        let sub = repo.path().join("a/b");
        std::fs::create_dir_all(&sub).unwrap();
        let root = TempDir::new();

        let p = resolve_project_in(root.path(), Some(&sub.to_string_lossy())).unwrap();
        let real_repo = repo.path().canonicalize().unwrap();
        assert_eq!(p.path, real_repo);
        assert_eq!(p.name, real_repo.file_name().unwrap().to_string_lossy());
        let key = p.dir.file_name().unwrap().to_string_lossy();
        assert!(
            key.starts_with(&format!("{}-", p.name)) && key.len() == p.name.len() + 1 + 8,
            "store dir = {key:?}, want <name>-<8hex>"
        );
        assert!(p.dir.exists(), "Dir not created");
        assert!(
            p.dir.join("project.json").exists(),
            "project.json not written"
        );
    }

    #[test]
    fn test_resolve_project_unifies_worktrees() {
        if !git_available() {
            eprintln!("skipping: git not on PATH");
            return;
        }
        let root = TempDir::new();
        let main = TempDir::new();
        init_repo(main.path());
        let wt_parent = TempDir::new();
        let wt = wt_parent.path().join("wt");
        let out = Command::new("git")
            .arg("-C")
            .arg(main.path())
            .arg("worktree")
            .arg("add")
            .arg(&wt)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "worktree add: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let p_main = resolve_project_in(root.path(), Some(&main.path().to_string_lossy())).unwrap();
        let p_wt = resolve_project_in(root.path(), Some(&wt.to_string_lossy())).unwrap();
        assert_eq!(
            p_main.dir, p_wt.dir,
            "worktree resolved to a different store"
        );
    }

    #[test]
    fn test_resolve_project_plain_repo_root() {
        if !git_available() {
            eprintln!("skipping: git not on PATH");
            return;
        }
        let root = TempDir::new();
        let dir = TempDir::new();
        init_repo(dir.path());
        let p = resolve_project_in(root.path(), Some(&dir.path().to_string_lossy())).unwrap();
        assert_eq!(p.path, dir.path().canonicalize().unwrap());
    }

    #[test]
    fn test_resolve_project_subdir_resolves_to_repo_root() {
        if !git_available() {
            eprintln!("skipping: git not on PATH");
            return;
        }
        let root = TempDir::new();
        let dir = TempDir::new();
        init_repo(dir.path());
        let sub = dir.path().join("pkg/inner");
        std::fs::create_dir_all(&sub).unwrap();
        // an explicit --project pointing at a subdir
        let p = resolve_project_in(root.path(), Some(&sub.to_string_lossy())).unwrap();
        assert_eq!(p.path, dir.path().canonicalize().unwrap());
    }

    #[test]
    fn test_resolve_project_non_git_falls_back_to_dir() {
        let root = TempDir::new();
        let dir = TempDir::new(); // no git init
        let p = resolve_project_in(root.path(), Some(&dir.path().to_string_lossy())).unwrap();
        assert_eq!(p.path, dir.path().canonicalize().unwrap());
    }
}
