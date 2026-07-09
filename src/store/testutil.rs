// Test-only helpers. The Go tests isolated the store with
// t.TempDir() + t.Setenv("XDG_STATE_HOME", ...); env vars are process-global
// and Rust tests run in parallel, so instead the temp store root is passed
// straight into resolve_project_in and no env is touched.
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use super::Project;

/// A unique temp dir, removed on drop (the Rust stand-in for Go's t.TempDir).
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "herdr-notes-rs-test-{}-{nanos}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Go's gitRepo(t): a fresh temp dir with `git init` run in it.
pub fn git_repo() -> TempDir {
    let d = TempDir::new();
    let out = Command::new("git")
        .arg("-C")
        .arg(d.path())
        .arg("init")
        .output()
        .expect("git not on PATH");
    assert!(
        out.status.success(),
        "git init: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    d
}

/// Go's newProject(t): a project in a fresh git repo, stored under a fresh
/// throwaway root. Holds the TempDir guards so both dirs outlive the project.
pub struct TestProject {
    pub p: Project,
    _root: TempDir,
    _repo: TempDir,
}

impl std::ops::Deref for TestProject {
    type Target = Project;
    fn deref(&self) -> &Project {
        &self.p
    }
}

pub fn new_project() -> TestProject {
    let root = TempDir::new();
    let repo = git_repo();
    let p = super::resolve_project_in(root.path(), Some(&repo.path().to_string_lossy())).unwrap();
    TestProject {
        p,
        _root: root,
        _repo: repo,
    }
}
