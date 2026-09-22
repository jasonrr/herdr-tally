// Port of internal/store/project.go. The store key MUST stay byte-compatible
// with the Go binary — `<base>-<sha1(abspath)[:8]>` — so this binary finds the
// data the Go version wrote. See test_project_key_matches_go_store.
use std::path::{Path, PathBuf};
use std::process::Command;

use super::errors::{Error, Result};
use super::todos::now;

pub struct Project {
    pub path: PathBuf,
    pub name: String,
    pub dir: PathBuf,
    /// Who is making changes, stamped into created_by/updated_by. Defaults to
    /// $HERDR_NOTES_OWNER (the same identity the lock uses) or "agent"; the TUI
    /// overrides it to "you" after resolve.
    pub actor: String,
}

/// The default attribution identity: $HERDR_NOTES_OWNER, else "agent". Same
/// resolution the lock owner uses (CLI owner_name / MCP or_agent), reused so
/// "who edited" and "who locked" never disagree.
fn default_actor() -> String {
    match std::env::var("HERDR_NOTES_OWNER") {
        Ok(a) if !a.is_empty() => a,
        _ => "agent".to_string(),
    }
}

/// The store root: `$XDG_STATE_HOME/tally`, else `~/.local/state/tally`. The
/// single resolver — `tally store link/status` operates on exactly this path.
pub fn store_root() -> PathBuf {
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
        // ENOENT here means the process's cwd was deleted out from under it
        // (e.g. a herdr worktree removed while its stdio MCP server kept
        // running). The bare "No such file or directory" reads as "no store",
        // so name the real cause and the fix instead.
        None => std::env::current_dir().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Other(
                    "tally's working directory no longer exists (was it a deleted worktree?); \
                     reconnect the tally MCP server, or pass an absolute `project` path"
                        .into(),
                )
            } else {
                Error::Io(e)
            }
        })?,
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
    let store_dir = store_root.join("projects").join(&key);
    // A missing override with no store yet is a typo'd path: creating one would
    // land the write where nobody looks. A store that already exists stays
    // reachable after its dir is deleted (TUI sync re-resolving a removed worktree).
    if override_dir.is_some() && !dir.is_dir() && !store_dir.is_dir() {
        return Err(Error::Other(format!(
            "project path {} is not an existing directory",
            dir.display()
        )));
    }
    let dir = store_dir;
    std::fs::create_dir_all(dir.join("scratchpads"))?;
    let p = Project {
        path: abs,
        name,
        dir,
        actor: default_actor(),
    };
    p.write_project_json();
    Ok(p)
}

/// exists() that does NOT follow symlinks — a dangling link still counts as
/// "something is there", so link never clobbers.
fn path_present(p: &Path) -> bool {
    p.symlink_metadata().is_ok()
}

/// Describe the store root: symlinked (and where to), a plain dir, or absent.
/// `root` is passed in so tests don't have to touch XDG_STATE_HOME.
pub fn store_status(root: &Path) -> String {
    match std::fs::read_link(root) {
        Ok(t) => format!(
            "store root: {}\n  symlink -> {}",
            root.display(),
            t.display()
        ),
        Err(_) if path_present(root) => {
            format!(
                "store root: {}\n  not a symlink (local directory)",
                root.display()
            )
        }
        Err(_) => format!("store root: {}\n  does not exist yet", root.display()),
    }
}

/// Move the store root into `target` (as `<target>/tally`) and leave a symlink
/// at the old location, so a file-sync folder holds the real data. Refuses if
/// the root is already a symlink or `<target>/tally` exists — never clobbers.
/// Returns the new real path.
pub fn link_store(root: &Path, target: &Path) -> Result<PathBuf> {
    if !target.is_dir() {
        return Err(Error::Other(format!(
            "target is not an existing directory: {}",
            target.display()
        )));
    }
    // A relative target string (e.g. "./sync") would otherwise be written
    // verbatim into the symlink, which resolves it relative to the symlink's
    // OWN directory, not the caller's cwd — silently bricking the store.
    // Canonicalize so the symlink always points at an absolute, `..`-free path.
    let target = target.canonicalize()?;
    if let Ok(dest) = std::fs::read_link(root) {
        return Err(Error::Other(format!(
            "store root {} is already a symlink to {} — nothing to do",
            root.display(),
            dest.display()
        )));
    }
    let dest = target.join("tally");
    if path_present(&dest) {
        return Err(Error::Other(format!(
            "{} already exists — refusing to clobber it",
            dest.display()
        )));
    }
    if path_present(root) {
        // Same-filesystem move only: a recursive copier is out of scope, so a
        // cross-device rename becomes a clear do-it-by-hand error.
        std::fs::rename(root, &dest).map_err(|e| {
            Error::Other(format!(
                "could not move {} to {} ({e}); if they're on different filesystems, \
                 move it by hand (`mv {} {}`) and then `ln -s {} {}`",
                root.display(),
                dest.display(),
                root.display(),
                dest.display(),
                dest.display(),
                root.display()
            ))
        })?;
    } else {
        std::fs::create_dir_all(&dest)?;
    }
    if let Some(parent) = root.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Err(e) = std::os::unix::fs::symlink(&dest, root) {
        // Never leave the store moved but unlinked: put it back.
        let _ = std::fs::rename(&dest, root);
        return Err(e.into());
    }
    Ok(dest)
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
    /// The legacy whole-file JSON path. No longer read/written by the todos
    /// store (that now goes through the automerge doc) — read once by
    /// `migrate_if_needed` as the pre-automerge source, then kept as a backup.
    pub(crate) fn todos_path(&self) -> PathBuf {
        self.dir.join("todos.json")
    }

    /// The directory holding this project's per-machine automerge snapshots
    /// (`automerge/<machine_hex>.automerge`), one file per machine.
    pub(crate) fn am_dir(&self) -> PathBuf {
        self.dir.join("automerge")
    }

    /// The legacy whole-file JSON path. No longer read/written by the
    /// comments store — read once by `migrate_if_needed`, then kept as a backup.
    pub(crate) fn comments_path(&self) -> PathBuf {
        self.dir.join("comments.json")
    }

    /// The GitHub "owner/name" from this project's `origin` remote, or None
    /// (no remote / not parseable). Uses the same `git` helper as project root.
    // Unused outside tests until a later task wires the sync engine to it.
    pub(crate) fn origin_repo(&self) -> Option<String> {
        let url = git(&self.path, &["remote", "get-url", "origin"])?;
        super::sync::parse_repo(&url)
    }

    /// The legacy scratchpad markdown directory. Read once by
    /// `migrate_if_needed` (each `*.md` parsed into the doc), then kept as backups.
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

    // A typo'd project path must error, not key a phantom store off the bogus path.
    #[test]
    fn test_missing_override_dir_errors() {
        let root = TempDir::new();
        let missing = root.path().join("no-such-repo");
        let err = resolve_project_in(root.path(), Some(&missing.to_string_lossy()))
            .err()
            .expect("missing dir must not resolve");
        assert!(err.to_string().contains("no-such-repo"), "{err}");
        assert!(
            !root.path().join("projects").exists(),
            "created a store anyway"
        );
    }

    // ...but a dir deleted after its store exists (TUI sync re-resolving a removed
    // worktree) must still reach that store.
    #[test]
    fn test_deleted_dir_with_existing_store_resolves() {
        let root = TempDir::new();
        let gone = TempDir::new();
        let path = gone.path().canonicalize().unwrap();
        let path = path.to_string_lossy().into_owned();
        let before = resolve_project_in(root.path(), Some(&path)).unwrap();
        drop(gone);
        let after = resolve_project_in(root.path(), Some(&path)).unwrap();
        assert_eq!(before.dir, after.dir);
    }

    // link/status take the root as an argument precisely so these can run in
    // parallel without touching XDG_STATE_HOME.
    #[test]
    fn test_link_store_moves_and_symlinks() {
        let root_parent = TempDir::new();
        let root = root_parent.path().join("tally");
        let repo = git_repo();
        let p = resolve_project_in(&root, Some(&repo.path().to_string_lossy())).unwrap();
        let t = p
            .create_todo("Survives the move", "", "", Vec::new())
            .unwrap();

        let target = TempDir::new();
        let dest = link_store(&root, target.path()).unwrap();
        // dest is canonicalized (e.g. macOS /var -> /private/var), so compare
        // against the canonical form of the target, not its literal path.
        assert_eq!(dest, target.path().canonicalize().unwrap().join("tally"));
        assert_eq!(std::fs::read_link(&root).unwrap(), dest);
        assert!(dest.join("projects").is_dir(), "data not moved");

        // still readable through the link
        let p2 = resolve_project_in(&root, Some(&repo.path().to_string_lossy())).unwrap();
        assert_eq!(p2.get_todo(&t.id).unwrap().title, "Survives the move");
        assert!(store_status(&root).contains("symlink ->"));
    }

    #[test]
    fn test_link_store_creates_when_root_absent() {
        let root_parent = TempDir::new();
        let root = root_parent.path().join("tally");
        assert!(store_status(&root).contains("does not exist yet"));
        let target = TempDir::new();
        let dest = link_store(&root, target.path()).unwrap();
        assert!(dest.is_dir());
        assert_eq!(std::fs::read_link(&root).unwrap(), dest);
    }

    #[test]
    fn test_link_store_refuses_when_already_linked() {
        let root_parent = TempDir::new();
        let root = root_parent.path().join("tally");
        let target = TempDir::new();
        link_store(&root, target.path()).unwrap();
        let other = TempDir::new();
        let err = link_store(&root, other.path()).unwrap_err().to_string();
        assert!(err.contains("already a symlink"), "err: {err}");
        assert!(
            !other.path().join("tally").exists(),
            "second target written"
        );
    }

    #[test]
    fn test_link_store_refuses_when_target_exists() {
        let root_parent = TempDir::new();
        let root = root_parent.path().join("tally");
        std::fs::create_dir_all(root.join("projects")).unwrap();
        let target = TempDir::new();
        std::fs::create_dir_all(target.path().join("tally")).unwrap();
        let err = link_store(&root, target.path()).unwrap_err().to_string();
        assert!(err.contains("refusing to clobber"), "err: {err}");
        assert!(root.join("projects").is_dir(), "root must be untouched");
    }

    // `tally store link ./sync` (a relative target) must not write a relative
    // dest into the symlink — resolved relative to the symlink's OWN dir, not
    // the cwd, that would brick the store. Build a relative-looking path with
    // `..` (no process-wide chdir, since tests run in parallel) and assert the
    // written symlink target is absolute and `..`-free, plus data survives.
    #[test]
    fn test_link_store_canonicalizes_relative_target() {
        let root_parent = TempDir::new();
        let root = root_parent.path().join("tally");
        let repo = git_repo();
        let p = resolve_project_in(&root, Some(&repo.path().to_string_lossy())).unwrap();
        let t = p
            .create_todo("Survives a relative link", "", "", Vec::new())
            .unwrap();

        let target_parent = TempDir::new();
        let real_target = target_parent.path().join("sync");
        std::fs::create_dir_all(&real_target).unwrap();
        // <abs-temp>/sync/sibling/../  == <abs-temp>/sync, but as a string it
        // contains ".." and is not the canonical form.
        std::fs::create_dir_all(real_target.join("sibling")).unwrap();
        let relative_ish = real_target.join("sibling").join("..");

        let dest = link_store(&root, &relative_ish).unwrap();
        assert_eq!(dest, real_target.canonicalize().unwrap().join("tally"));

        let link_target = std::fs::read_link(&root).unwrap();
        assert!(
            link_target.is_absolute(),
            "symlink target must be absolute: {}",
            link_target.display()
        );
        assert!(
            !link_target.components().any(|c| c.as_os_str() == ".."),
            "symlink target must not contain '..': {}",
            link_target.display()
        );

        // still readable through the link
        let p2 = resolve_project_in(&root, Some(&repo.path().to_string_lossy())).unwrap();
        assert_eq!(
            p2.get_todo(&t.id).unwrap().title,
            "Survives a relative link"
        );
    }

    #[test]
    fn test_link_store_refuses_missing_target() {
        let root_parent = TempDir::new();
        let root = root_parent.path().join("tally");
        let err = link_store(&root, &root_parent.path().join("nope"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not an existing directory"), "err: {err}");
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

    #[test]
    fn test_origin_repo_reads_remote() {
        if !git_available() {
            eprintln!("skipping: git not on PATH");
            return;
        }
        let root = TempDir::new();
        let dir = TempDir::new();
        init_repo(dir.path());
        let out = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["remote", "add", "origin", "git@github.com:owner/name.git"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let p = resolve_project_in(root.path(), Some(&dir.path().to_string_lossy())).unwrap();
        assert_eq!(p.origin_repo().as_deref(), Some("owner/name"));
    }

    #[test]
    fn test_origin_repo_none_without_remote() {
        if !git_available() {
            eprintln!("skipping: git not on PATH");
            return;
        }
        let root = TempDir::new();
        let dir = TempDir::new();
        init_repo(dir.path());
        let p = resolve_project_in(root.path(), Some(&dir.path().to_string_lossy())).unwrap();
        assert_eq!(p.origin_repo(), None);
    }
}
