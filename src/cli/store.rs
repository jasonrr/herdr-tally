//! `tally store <link|status>` — where the store lives on disk. Thin adapter:
//! all logic is `store::{link_store,store_status}`. Operates on the store ROOT
//! only (never a single project dir).
use std::io::Write;
use std::path::{Path, PathBuf};

use super::fail;

pub(crate) fn run(args: &[String], store_root: Option<&Path>, out: &mut dyn Write) -> i32 {
    let root: PathBuf = match store_root {
        Some(r) => r.to_path_buf(),
        None => crate::store::store_root(),
    };
    match args.first().map(String::as_str) {
        Some("status") => {
            let _ = writeln!(out, "{}", crate::store::store_status(&root));
            0
        }
        Some("link") => {
            let Some(target) = args.get(1) else {
                return fail("usage: tally store link <target-dir>");
            };
            match crate::store::link_store(&root, Path::new(target)) {
                Ok(dest) => {
                    let _ = writeln!(
                        out,
                        "moved store to {} and linked {} -> it\n\
                         two things to know:\n  \
                         - the same project must live at the SAME ABSOLUTE PATH on every machine \
                         (the store key is a sha1 of that path)\n  \
                         - iCloud may evict synced files; pin the folder \"keep downloaded\", \
                         or use Syncthing/Dropbox instead",
                        dest.display(),
                        root.display()
                    );
                    0
                }
                Err(e) => fail(&e.to_string()),
            }
        }
        _ => fail("usage: tally store <link <target-dir>|status>"),
    }
}

#[cfg(test)]
mod tests {
    use crate::store::testutil::TempDir;

    #[test]
    fn store_link_then_status() {
        let root_parent = TempDir::new();
        let root = root_parent.path().join("tally");
        let target = TempDir::new();
        let args = ["link".to_string(), target.path().to_string_lossy().into()];
        let mut buf: Vec<u8> = Vec::new();
        assert_eq!(super::run(&args, Some(&root), &mut buf), 0);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("ABSOLUTE PATH") && s.contains("iCloud"), "{s}");

        let mut sbuf: Vec<u8> = Vec::new();
        assert_eq!(
            super::run(&["status".to_string()], Some(&root), &mut sbuf),
            0
        );
        assert!(String::from_utf8(sbuf).unwrap().contains("symlink ->"));

        // second link is refused
        let mut ebuf: Vec<u8> = Vec::new();
        assert_ne!(super::run(&args, Some(&root), &mut ebuf), 0);
    }
}
