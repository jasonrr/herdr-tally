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
// read/write, migration, adapters) call.

use automerge::transaction::Transactable;
use automerge::{ActorId, AutoCommit, ObjType, ROOT, ReadDoc, ScalarValue};
use autosurgeon::{hydrate_prop, reconcile_prop};

use super::comments::CommentsFile;
use super::errors::{Error, Result};
use super::lock::{atomic_write, with_file_lock};
use super::project::Project;
use super::scratchpads::{Scratchpad, parse_pad};
use super::todos::TodosFile;

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
        self.migrate_if_needed()?;

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

    /// One-time JSON->automerge migration, run as the FIRST line of `load_doc`.
    /// On the first load after the backend swap it builds the doc from the
    /// CURRENT legacy JSON/markdown state (one snapshot, not from history) and
    /// renames the legacy files to `*.migrated` backups.
    ///
    /// Convergence (R2): the migration authors ALL inserts under the FIXED
    /// genesis actor (no `set_actor`), so two machines migrating byte-identical
    /// synced legacy files emit identical change hashes that automerge dedups on
    /// merge — no duplicated todos/pads. If content differs, ops differ and they
    /// correctly union.
    pub(crate) fn migrate_if_needed(&self) -> Result<()> {
        // am_dir exists => already migrated, or a fresh doc was created. Cheap
        // path: no lock, no disk scan.
        if self.am_dir().exists() {
            return Ok(());
        }

        let todos_p = self.todos_path();
        let comments_p = self.comments_path();
        let scratch_p = self.scratch_dir();

        // Legacy scratchpad markdown, SORTED for a deterministic insert order
        // (R2): two machines must emit the same op sequence to converge.
        let mut pad_files: Vec<std::path::PathBuf> = if scratch_p.exists() {
            std::fs::read_dir(&scratch_p)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "md"))
                .collect()
        } else {
            Vec::new()
        };
        pad_files.sort();

        // Brand-new user: no legacy sources at all — `load_doc` genesises an
        // empty doc, nothing to migrate. (`scratch_dir` always exists — created
        // by resolve_project — so gate on *.md presence, not the bare dir.)
        if !todos_p.exists() && !comments_p.exists() && pad_files.is_empty() {
            return Ok(());
        }

        // Guard the migration on `migrate.lock` — a sibling of the pre-existing
        // `self.dir/migrate`, which exists before am_dir and is DISTINCT from
        // `am_dir/lock`. That avoids flock re-entrancy (load_doc runs both on
        // bare read paths and inside with_doc, which already holds am_dir/lock)
        // and closes the TOCTOU race where two processes both migrate and one
        // reads a legacy file the other already renamed. Deadlock-free: writers
        // take am_dir/lock then migrate.lock; readers take only migrate.lock.
        with_file_lock(&self.dir.join("migrate"), || {
            // Double-checked locking: another process may have migrated between
            // the pre-lock check and acquiring the lock.
            if self.am_dir().exists() {
                return Ok(());
            }

            // Genesis containers under the FIXED genesis actor; ensure_root
            // leaves that actor active, and we deliberately do NOT set the
            // machine actor — every migration insert stays genesis-authored.
            let mut doc = AutoCommit::new();
            self.ensure_root(&mut doc)?;

            if todos_p.exists() {
                let bytes = std::fs::read(&todos_p)?;
                let mut tf: TodosFile = serde_json::from_slice(&bytes)
                    .map_err(|e| Error::Other(format!("migrate {}: {e}", todos_p.display())))?;
                // R3: normalize legacy priorities so the doc never stores
                // "high"/"medium"/"low" (Task 7's raw `dump` would show them).
                for t in &mut tf.todos {
                    t.priority = crate::store::todos::migrate_legacy_priority(&t.priority);
                }
                save_todos_file(&mut doc, &tf)?;
                // R4: reconcile skips `Todo.extra`, so into a FRESH doc those
                // cross-version keys would vanish — write them explicitly.
                preserve_todo_extras(&mut doc, &tf)?;
            }
            if comments_p.exists() {
                let bytes = std::fs::read(&comments_p)?;
                let cf: CommentsFile = serde_json::from_slice(&bytes)
                    .map_err(|e| Error::Other(format!("migrate {}: {e}", comments_p.display())))?;
                save_comments_file(&mut doc, &cf)?;
            }
            for path in &pad_files {
                let bytes = std::fs::read(path)?;
                save_pad(&mut doc, &parse_pad(&bytes))?;
            }

            // Writes <machine_hex>.automerge, creating am_dir — this gates every
            // future run (the am_dir().exists() checks above).
            self.save_doc(&mut doc)?;

            // R5: keep each legacy source as a `*.migrated` backup (the recovery
            // path), never delete. Tolerate a NotFound (concurrent rename).
            rename_migrated(&todos_p)?;
            rename_migrated(&comments_p)?;
            for path in &pad_files {
                rename_migrated(path)?;
            }
            Ok(())
        })
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

/// Rename a legacy source to `<path>.migrated`, keeping the original bytes as a
/// backup. A missing source (concurrent rename) is not a failure.
fn rename_migrated(path: &std::path::Path) -> Result<()> {
    let mut dst = path.as_os_str().to_owned();
    dst.push(".migrated");
    match std::fs::rename(path, std::path::PathBuf::from(dst)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// R4: write each todo's non-empty `extra` map (unknown/cross-version keys that
/// reconcile skips) onto its automerge map object, so a FRESH migration doc
/// doesn't silently drop them. Todos are matched by `id` (reconcile inserts in
/// Vec order, but id-matching is robust against any future ordering change).
/// Authored under whatever actor is active — the genesis actor during migration.
fn preserve_todo_extras(doc: &mut AutoCommit, tf: &TodosFile) -> Result<()> {
    let (_, todos_map) = doc
        .get(ROOT, "todos")?
        .ok_or_else(|| Error::Other("todos root missing".into()))?;
    let (_, todos_list) = doc
        .get(&todos_map, "todos")?
        .ok_or_else(|| Error::Other("todos list missing".into()))?;
    for t in &tf.todos {
        if t.extra.is_empty() {
            continue;
        }
        let mut target = None;
        for i in 0..doc.length(&todos_list) {
            let Some((_, elem)) = doc.get(&todos_list, i)? else {
                continue;
            };
            let matches = doc
                .get(&elem, "id")?
                .and_then(|(v, _)| v.to_str().map(|s| s == t.id.as_str()))
                .unwrap_or(false);
            if matches {
                target = Some(elem);
                break;
            }
        }
        let Some(obj) = target else { continue };
        for (k, val) in &t.extra {
            put_json(doc, &obj, k, val)?;
        }
    }
    Ok(())
}

/// A `serde_json::Number` as the closest automerge scalar: integers stay
/// integers (i64, then u64-as-i64), everything else is an f64.
fn json_number(n: &serde_json::Number) -> ScalarValue {
    if let Some(i) = n.as_i64() {
        ScalarValue::Int(i)
    } else if let Some(u) = n.as_u64() {
        ScalarValue::Int(u as i64)
    } else {
        ScalarValue::F64(n.as_f64().unwrap_or(0.0))
    }
}

/// Recursively write a `serde_json::Value` onto a map key, mirroring the JSON
/// shape as automerge scalars/objects. Null is skipped: an absent key hydrates
/// to the same default this binary would produce, and the `.migrated` backup
/// remains the source of truth if a null ever needs to be recovered.
fn put_json(
    doc: &mut AutoCommit,
    obj: &automerge::ObjId,
    key: &str,
    v: &serde_json::Value,
) -> Result<()> {
    use serde_json::Value;
    match v {
        Value::Null => {}
        Value::Bool(b) => {
            doc.put(obj, key, *b)?;
        }
        Value::Number(n) => {
            doc.put(obj, key, json_number(n))?;
        }
        Value::String(s) => {
            doc.put(obj, key, s.as_str())?;
        }
        Value::Array(a) => {
            let list = doc.put_object(obj, key, ObjType::List)?;
            for (i, e) in a.iter().enumerate() {
                insert_json(doc, &list, i, e)?;
            }
        }
        Value::Object(m) => {
            let child = doc.put_object(obj, key, ObjType::Map)?;
            for (k, e) in m {
                put_json(doc, &child, k, e)?;
            }
        }
    }
    Ok(())
}

/// Recursively insert a `serde_json::Value` at a list index. Unlike the map
/// case, a Null is inserted as an explicit `ScalarValue::Null` so that the
/// indices of the following elements stay aligned.
fn insert_json(
    doc: &mut AutoCommit,
    list: &automerge::ObjId,
    idx: usize,
    v: &serde_json::Value,
) -> Result<()> {
    use serde_json::Value;
    match v {
        Value::Null => {
            doc.insert(list, idx, ScalarValue::Null)?;
        }
        Value::Bool(b) => {
            doc.insert(list, idx, *b)?;
        }
        Value::Number(n) => {
            doc.insert(list, idx, json_number(n))?;
        }
        Value::String(s) => {
            doc.insert(list, idx, s.as_str())?;
        }
        Value::Array(a) => {
            let inner = doc.insert_object(list, idx, ObjType::List)?;
            for (i, e) in a.iter().enumerate() {
                insert_json(doc, &inner, i, e)?;
            }
        }
        Value::Object(m) => {
            let inner = doc.insert_object(list, idx, ObjType::Map)?;
            for (k, e) in m {
                put_json(doc, &inner, k, e)?;
            }
        }
    }
    Ok(())
}

/// Hydrate the `todos` root map into a `TodosFile`. `load_doc`'s `ensure_root`
/// guarantees the key is present on a doc it produced, but guard a missing key
/// (e.g. a doc built by hand in a test) as an empty file rather than an error.
pub(crate) fn load_todos_file(doc: &AutoCommit) -> Result<TodosFile> {
    if doc.get(ROOT, "todos")?.is_none() {
        return Ok(TodosFile::default());
    }
    Ok(hydrate_prop(doc, ROOT, "todos")?)
}

/// Reconcile a `TodosFile` into the `todos` root map. Identity-keyed list
/// reconcile (`Todo::id` is `#[key]`) means this does not prune siblings
/// written by another machine and merged in.
pub(crate) fn save_todos_file(doc: &mut AutoCommit, tf: &TodosFile) -> Result<()> {
    reconcile_prop(doc, ROOT, "todos", tf)?;
    Ok(())
}

/// Hydrate the `comments` root map into a `CommentsFile`. Same missing-key
/// guard as `load_todos_file`.
pub(crate) fn load_comments_file(doc: &AutoCommit) -> Result<CommentsFile> {
    if doc.get(ROOT, "comments")?.is_none() {
        return Ok(CommentsFile::default());
    }
    Ok(hydrate_prop(doc, ROOT, "comments")?)
}

/// Reconcile a `CommentsFile` into the `comments` root map.
pub(crate) fn save_comments_file(doc: &mut AutoCommit, cf: &CommentsFile) -> Result<()> {
    reconcile_prop(doc, ROOT, "comments", cf)?;
    Ok(())
}

// --- Scratchpads: a Map keyed by pad id under ROOT->scratchpads. ---------
//
// Each pad's metadata is reconciled as map scalars (the `Scratchpad` derive
// skips `content`), and the pad BODY is a separate automerge `Text` child under
// the `content` key so concurrent edits from two machines merge char-by-char
// instead of one clobbering the other.

/// The `scratchpads` root Map object id. `ensure_root` (run in `load_doc`)
/// guarantees it exists; a missing key means the doc wasn't produced by
/// `load_doc`, which is a bug here.
fn scratchpads_obj(doc: &AutoCommit) -> Result<automerge::ObjId> {
    let (_, o) = doc
        .get(ROOT, "scratchpads")?
        .ok_or_else(|| Error::Other("scratchpads root missing".into()))?;
    Ok(o)
}

/// The pad ids present in the doc (the scratchpads Map's keys).
pub(crate) fn pad_ids(doc: &AutoCommit) -> Result<Vec<String>> {
    let sp = scratchpads_obj(doc)?;
    Ok(doc.keys(&sp).collect()) // map keys = pad ids
}

/// Hydrate a single pad by id, or `None` if absent. Metadata comes from the
/// map scalars (autosurgeon skips `content`, so it hydrates empty); the body is
/// read separately from the `content` Text child.
pub(crate) fn load_pad(doc: &AutoCommit, id: &str) -> Result<Option<Scratchpad>> {
    let sp = scratchpads_obj(doc)?;
    let Some((_, pad_map)) = doc.get(&sp, id)? else {
        return Ok(None);
    };
    let mut s: Scratchpad = hydrate_prop(doc, &sp, id)?; // content skipped -> ""
    if let Some((_, text_obj)) = doc.get(&pad_map, "content")? {
        s.content = doc.text(&text_obj)?;
    }
    Ok(Some(s))
}

/// Write a pad: reconcile its metadata scalars, then write the body into the
/// `content` Text child. On update the existing Text child is REUSED (never
/// re-`put_object`'d) so `update_text` diffs against it and merge history — the
/// char-level CRDT — survives.
pub(crate) fn save_pad(doc: &mut AutoCommit, s: &Scratchpad) -> Result<()> {
    let sp = scratchpads_obj(doc)?;
    reconcile_prop(doc, &sp, s.id.as_str(), s)?; // metadata only; content is skipped
    let (_, pad_map) = doc.get(&sp, s.id.as_str())?.unwrap();
    // Get-or-create the Text child; reusing it on update keeps the CRDT history.
    let text_obj = match doc.get(&pad_map, "content")? {
        Some((_, obj)) => obj,
        None => doc.put_object(&pad_map, "content", ObjType::Text)?,
    };
    doc.update_text(&text_obj, &s.content)?;
    Ok(())
}

/// Delete a pad by removing its map key (drops the metadata and Text child).
pub(crate) fn remove_pad(doc: &mut AutoCommit, id: &str) -> Result<()> {
    let sp = scratchpads_obj(doc)?;
    doc.delete(&sp, id)?;
    Ok(())
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

    // Append ".migrated" to a path (the R5 backup name).
    fn migrated(path: &std::path::Path) -> std::path::PathBuf {
        let mut s = path.as_os_str().to_owned();
        s.push(".migrated");
        std::path::PathBuf::from(s)
    }

    // The core fixture test: seed legacy JSON/markdown directly in the store
    // dir, then the first load_doc migrates it into the automerge doc and backs
    // up the originals. Exercises R3 (priority normalize), R4 (extra preserved),
    // and R5 (.migrated backups).
    #[test]
    fn migrates_legacy_store() {
        let p = new_project();

        // todos.json: one todo, legacy "high" priority + an unknown field.
        let todos_json = r#"{"revision":2,"todos":[
            {"id":"t_mig","title":"Legacy todo","priority":"high","future_field":"keepme"}
        ]}"#;
        std::fs::write(p.todos_path(), todos_json).unwrap();
        // comments.json: one note on that todo.
        let comments_json =
            r#"{"comments":[{"id":"c_mig","target":"t_mig","kind":"note","text":"a note"}]}"#;
        std::fs::write(p.comments_path(), comments_json).unwrap();
        // scratchpads/s_x.md: frontmatter + body parse_pad accepts.
        let pad_md = "---\nid: s_x\ntitle: Legacy Pad\ntags: [a]\nstatus: active\nrevision: 1\ncreated: 2026-01-01T00:00:00Z\nupdated: 2026-01-01T00:00:00Z\n---\n# Legacy Pad\n\npad body here\n";
        std::fs::write(p.scratch_dir().join("s_x.md"), pad_md).unwrap();

        assert!(!p.am_dir().exists(), "precondition: not yet migrated");

        // Triggers migrate_if_needed as its first line.
        p.load_doc().unwrap();

        // R3: high -> p1 (normalized on the way into the doc).
        let t = p.get_todo("t_mig").unwrap();
        assert_eq!(t.priority, "p1", "high must normalize to p1 (R3)");
        assert_eq!(t.title, "Legacy todo");

        // R4: the unknown `future_field` survives onto the todo's automerge map.
        let doc = p.load_doc().unwrap();
        let (_, todos_map) = doc.get(ROOT, "todos").unwrap().unwrap();
        let (_, todos_list) = doc.get(&todos_map, "todos").unwrap().unwrap();
        let (_, elem) = doc.get(&todos_list, 0).unwrap().unwrap();
        let ff = doc.get(&elem, "future_field").unwrap();
        assert_eq!(
            ff.and_then(|(v, _)| v.to_str().map(str::to_string)),
            Some("keepme".to_string()),
            "unknown todo field must survive migration (R4)"
        );

        // Comment migrated.
        let comments = p.list_comments("t_mig").unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "a note");

        // Scratchpad migrated with its body.
        let pad = p.read_pad("s_x").unwrap();
        assert_eq!(pad.title, "Legacy Pad");
        assert!(
            pad.content.contains("pad body here"),
            "pad body: {:?}",
            pad.content
        );

        // R5: legacy files renamed to .migrated backups; originals gone.
        assert!(!p.todos_path().exists(), "todos.json must be renamed away");
        assert!(migrated(&p.todos_path()).exists(), "todos.json.migrated");
        assert!(!p.comments_path().exists());
        assert!(migrated(&p.comments_path()).exists());
        assert!(!p.scratch_dir().join("s_x.md").exists());
        assert!(migrated(&p.scratch_dir().join("s_x.md")).exists());
    }

    // Migration runs exactly once: after it, a fresh todo created via the real
    // path coexists with the migrated one, and a second load_doc neither
    // re-migrates (am_dir exists) nor drops the new todo.
    #[test]
    fn migrates_is_one_shot() {
        let p = new_project();
        let todos_json = r#"{"revision":1,"todos":[{"id":"t_old","title":"old","priority":"p2"}]}"#;
        std::fs::write(p.todos_path(), todos_json).unwrap();

        p.load_doc().unwrap(); // migrates
        assert!(p.am_dir().exists(), "migration must create am_dir");

        p.create_todo("fresh", "", "", Vec::new()).unwrap();

        let doc = p.load_doc().unwrap();
        let mut titles = todo_titles(&doc);
        titles.sort();
        assert_eq!(
            titles,
            vec!["fresh".to_string(), "old".to_string()],
            "migrated + new todo must coexist; no re-migration"
        );
    }

    // R2 convergence: two independent machines migrating the SAME byte-identical
    // todos.json under the genesis actor produce identical change hashes, so
    // merging their snapshots dedups — the todo count stays 1, not 2. A doubled
    // count would mean migration is nondeterministic (STOP condition).
    #[test]
    fn concurrent_migration_no_duplicates() {
        let legacy = r#"{"revision":1,"todos":[{"id":"t_1","title":"same","priority":"p2"}]}"#;

        // Two "machines": separate store roots, identical legacy input.
        let a = new_project();
        let b = new_project();
        std::fs::write(a.todos_path(), legacy).unwrap();
        std::fs::write(b.todos_path(), legacy).unwrap();
        a.load_doc().unwrap(); // migrates independently
        b.load_doc().unwrap(); // migrates independently

        // Merge B's snapshot(s) into A's doc.
        let mut da = a.load_doc().unwrap();
        for entry in std::fs::read_dir(b.am_dir()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|x| x == "automerge") {
                let mut other = AutoCommit::load(&std::fs::read(&path).unwrap()).unwrap();
                da.merge(&mut other).unwrap();
            }
        }

        let titles = todo_titles(&da);
        assert_eq!(
            titles.len(),
            1,
            "genesis-authored migration must dedup on merge, got {titles:?}"
        );
        assert_eq!(titles, vec!["same".to_string()]);
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
