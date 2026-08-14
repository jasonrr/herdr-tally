// Per-machine automerge snapshot model. Replaces whole-file-JSON load/save.
//
// Each machine owns exactly ONE file `automerge/<machine_hex>.automerge` under
// the project store dir and is its SOLE writer. Every mutation is:
//   flock -> load+merge every `automerge/*.automerge` into one AutoCommit ->
//   caller mutates -> doc.save() -> atomic_write to OUR file only.
// Load merges all files (order-independent, idempotent — proven in the Task 1
// spike), so each machine's edits reach every other machine on the next load.
//
// The machine identity is the hardware host UUID (`gethostuuid`), computed
// fresh every run — never read or written to a file. That guarantees one owner
// file per machine (no id collision, no persisted-id drift) while doubling as
// the automerge actor so a machine's changes carry a stable authorship.
// This is the storage core; every public item is a seam that Tasks 4–7 (entity
// read/write, migration, adapters) call. Until they land, the non-test bin build
// has no caller for them, so allow dead_code here — the tests below exercise the
// whole surface, and removing it would orphan the infra the later tasks depend on.
#![allow(dead_code)]

use automerge::transaction::Transactable;
use automerge::{ActorId, AutoCommit, ObjType, ROOT, ReadDoc};

use super::errors::{Error, Result};
use super::lock::{atomic_write, with_file_lock};
use super::project::Project;

/// Fixed genesis actor: the three root containers are created under THIS actor
/// with deterministic ops, so every machine's fresh doc emits byte-identical
/// genesis changes (same change hashes) that automerge dedups as shared
/// history. See `ensure_root`.
const GENESIS_ACTOR: [u8; 16] = [0u8; 16];

/// Reads the hardware host UUID (macOS `gethostuuid`) as the machine identity.
/// No file backs this — it is recomputed from hardware every run, which is what
/// keeps the single-owner-file guarantee (a persisted or random id could
/// collide or drift and reintroduce two machines writing one file).
fn gethostuuid_bytes() -> Result<[u8; 16]> {
    let mut buf = [0u8; 16];
    let ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::gethostuuid(buf.as_mut_ptr(), &ts as *const libc::timespec) };
    if rc != 0 {
        return Err(Error::Other("gethostuuid failed".into()));
    }
    Ok(buf)
}

impl Project {
    /// This machine's automerge actor = its hardware host UUID.
    fn machine_actor(&self) -> Result<ActorId> {
        Ok(ActorId::from(gethostuuid_bytes()?))
    }

    /// Hex of the machine actor — the basename of this machine's owner file.
    fn machine_hex(&self) -> Result<String> {
        Ok(self.machine_actor()?.to_hex_string())
    }

    /// Creates the three root containers ONCE, under a FIXED genesis actor with
    /// deterministic ops, so independent machines' fresh docs share the same
    /// container object-ids and merge losslessly.
    ///
    /// Fixed-actor deterministic genesis => identical object-ids across machines
    /// => clean CRDT merge. Creating these containers under the per-machine
    /// actor would make independent first-runs conflict and silently hide one
    /// machine's entire todos/comments/scratchpads container. The caller
    /// (`load_doc`) sets the machine actor AFTER this returns, so all subsequent
    /// real edits use the machine actor, not the genesis one.
    pub(crate) fn ensure_root(&self, doc: &mut AutoCommit) -> Result<()> {
        doc.set_actor(ActorId::from(GENESIS_ACTOR));

        // Order is fixed and each key is created only if absent, so re-running
        // over an already-seeded doc is a no-op (and emits no new changes).
        if doc.get(ROOT, "todos")?.is_none() {
            // On-disk shape of TodosFile { revision: i64, todos: Vec<Todo> }.
            let t = doc.put_object(ROOT, "todos", ObjType::Map)?;
            doc.put(&t, "revision", 0_i64)?;
            doc.put_object(&t, "todos", ObjType::List)?;
        }
        if doc.get(ROOT, "comments")?.is_none() {
            // On-disk shape of CommentsFile { comments: Vec<Comment> }.
            let c = doc.put_object(ROOT, "comments", ObjType::Map)?;
            doc.put_object(&c, "comments", ObjType::List)?;
        }
        if doc.get(ROOT, "scratchpads")?.is_none() {
            // Map keyed by pad id; values added in Task 5.
            doc.put_object(ROOT, "scratchpads", ObjType::Map)?;
        }
        Ok(())
    }

    /// Loads the merged view of every machine's snapshot: read each
    /// `automerge/*.automerge` file and merge them into one doc, run genesis,
    /// then switch to the machine actor for the caller's edits.
    pub(crate) fn load_doc(&self) -> Result<AutoCommit> {
        // Task 6 prepends: self.migrate_if_needed()?;

        let mut files: Vec<std::path::PathBuf> = Vec::new();
        if self.am_dir().exists() {
            for entry in std::fs::read_dir(self.am_dir())? {
                let path = entry?.path();
                if path.extension().is_some_and(|e| e == "automerge") {
                    files.push(path);
                }
            }
        }
        files.sort(); // determinism (merge is order-independent regardless)

        let mut doc = if files.is_empty() {
            AutoCommit::new()
        } else {
            let mut doc = AutoCommit::load(&std::fs::read(&files[0])?)?;
            for f in &files[1..] {
                let mut other = AutoCommit::load(&std::fs::read(f)?)?;
                doc.merge(&mut other)?;
            }
            doc
        };

        self.ensure_root(&mut doc)?;
        doc.set_actor(self.machine_actor()?);
        Ok(doc)
    }

    /// Persists the merged doc to OUR machine's file only (single-owner-file).
    pub(crate) fn save_doc(&self, doc: &mut AutoCommit) -> Result<()> {
        let bytes = doc.save();
        std::fs::create_dir_all(self.am_dir())?;
        atomic_write(
            &self
                .am_dir()
                .join(format!("{}.automerge", self.machine_hex()?)),
            &bytes,
        )
    }

    /// The single mutation choke point: flock, load+merge, run the caller's
    /// mutation, then save to our owner file.
    pub(crate) fn with_doc<T>(&self, f: impl FnOnce(&mut AutoCommit) -> Result<T>) -> Result<T> {
        with_file_lock(&self.am_dir().join("lock"), || {
            let mut d = self.load_doc()?;
            let r = f(&mut d)?;
            self.save_doc(&mut d)?;
            Ok(r)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::new_project;

    // Raw helper: append a todo map with the given title to ROOT->todos->todos
    // List. (Entity helpers arrive in Task 4; here we drive automerge directly.)
    fn push_todo(doc: &mut AutoCommit, title: &str) -> Result<()> {
        let (_, todos_map) = doc.get(ROOT, "todos")?.unwrap();
        let (_, todos_list) = doc.get(&todos_map, "todos")?.unwrap();
        let idx = doc.length(&todos_list);
        let m = doc.insert_object(&todos_list, idx, ObjType::Map)?;
        doc.put(&m, "title", title)?;
        Ok(())
    }

    // Raw helper: collect every todo title from ROOT->todos->todos List.
    fn todo_titles(doc: &AutoCommit) -> Vec<String> {
        let (_, todos_map) = doc.get(ROOT, "todos").unwrap().unwrap();
        let (_, todos_list) = doc.get(&todos_map, "todos").unwrap().unwrap();
        let n = doc.length(&todos_list);
        (0..n)
            .filter_map(|i| {
                let (_, m) = doc.get(&todos_list, i).unwrap()?;
                let (v, _) = doc.get(&m, "title").unwrap()?;
                Some(v.to_str()?.to_string())
            })
            .collect()
    }

    // THE lossless-union test: two INDEPENDENT machines (not a fork of shared
    // bytes) each add a todo; merging their snapshots must keep BOTH. This is
    // only true because both ran the same deterministic `ensure_root` genesis,
    // so their container object-ids match. If genesis were per-machine, one
    // machine's whole todos container would be hidden and a todo would vanish.
    #[test]
    fn two_machines_merge() {
        let p = new_project();

        // Machine A (us): write our own <machine_hex>.automerge via with_doc.
        p.with_doc(|doc| push_todo(doc, "A")).unwrap();

        // Machine B: an independently-created fresh doc, same genesis, its own
        // actor and its own file on disk.
        let mut b = AutoCommit::new();
        p.ensure_root(&mut b).unwrap();
        b.set_actor(ActorId::from([9u8; 16]));
        push_todo(&mut b, "B").unwrap();
        let bytes = b.save();
        std::fs::write(
            p.am_dir()
                .join("00000000000000000000000000000009.automerge"),
            bytes,
        )
        .unwrap();

        // Load = merge both machines' files.
        let doc = p.load_doc().unwrap();
        let mut titles = todo_titles(&doc);
        titles.sort();
        assert_eq!(
            titles,
            vec!["A".to_string(), "B".to_string()],
            "both machines' todos must survive the merge (deterministic genesis)"
        );
    }

    // A fresh project with no automerge dir loads without error and has the
    // genesis containers in place (empty todos List). Proves ensure_root runs
    // on a fresh doc.
    #[test]
    fn empty_is_empty_doc() {
        let p = new_project();
        assert!(!p.am_dir().exists(), "precondition: no automerge dir yet");
        let doc = p.load_doc().unwrap();
        let (_, todos_map) = doc.get(ROOT, "todos").unwrap().unwrap();
        let (_, todos_list) = doc.get(&todos_map, "todos").unwrap().unwrap();
        assert_eq!(doc.length(&todos_list), 0, "fresh todos List must be empty");
    }

    // Two sequential mutations on the same project write to the SAME owner file
    // (same machine_hex), so exactly one *.automerge file exists.
    #[test]
    fn single_owner_file() {
        let p = new_project();
        p.with_doc(|doc| push_todo(doc, "first")).unwrap();
        p.with_doc(|doc| push_todo(doc, "second")).unwrap();

        let count = std::fs::read_dir(p.am_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "automerge"))
            .count();
        assert_eq!(count, 1, "one machine must own exactly one snapshot file");
    }
}
