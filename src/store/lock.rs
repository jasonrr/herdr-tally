// Port of internal/store/lock.go: flock-serialized writes + temp-file/rename
// atomic writes. Uses libc flock (LOCK_EX, the same BSD flock Go's
// syscall.Flock wraps) on the SAME sidecar `<path>.lock` files the Go binary
// locks, so the two binaries stay lock-compatible during the transition.
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use super::errors::Result;

/// Runs f while holding an exclusive advisory lock on a sidecar lockfile for
/// path. ponytail: one lock per file, fine at single-user scale.
pub(crate) fn with_file_lock<T>(path: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false) // lockfile content is irrelevant; never clobber it
        .read(true)
        .write(true)
        .open(sibling(path, ".lock"))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let _unlock = Unlock(&file); // Go's deferred LOCK_UN: release even on error
    f()
}

struct Unlock<'a>(&'a File);

impl Drop for Unlock<'_> {
    fn drop(&mut self) {
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// Writes b to path via a temp file + rename (same `<path>.tmp` name as Go).
pub(crate) fn atomic_write(path: &Path, b: &[u8]) -> Result<()> {
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, b)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Go built these paths with string concat (`path + ".lock"`), i.e. the
/// suffix goes after the full filename including its extension.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}
