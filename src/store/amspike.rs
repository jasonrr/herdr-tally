//! THROWAWAY spike (Task 1 of automerge-store). Deleted by Task 2.
//!
//! Each `#[test]` empirically pins one automerge 0.11 / autosurgeon 0.13 behavior
//! the feature depends on. Findings are recorded verbatim in the tally build-log
//! scratchpad `build-log:automerge-store`. Do NOT build production code on assumed
//! behavior — assert it here first.

use automerge::transaction::Transactable; // put, put_object, insert_object, splice_text, update_text
use automerge::{ActorId, AutoCommit, ObjType, ROOT, ReadDoc}; // get, length, text, save, load, merge, set_actor
use autosurgeon::{Hydrate, Reconcile, hydrate, hydrate_prop, reconcile, reconcile_prop};

#[derive(Reconcile, Hydrate, Clone, Debug, PartialEq, Default)]
struct MiniTodo {
    title: String,
    done: bool,
}

#[derive(Reconcile, Hydrate, Clone, Debug, PartialEq, Default)]
struct TodosFile {
    todos: Vec<MiniTodo>,
}

// ---------------------------------------------------------------------------
// Finding 1: save/load round-trip + idempotent, union-preserving merge.
//   save(&mut self) -> Vec<u8>
//   AutoCommit::load(&[u8]) -> Result<AutoCommit, AutomergeError>
//   merge(&mut self, other: &mut AutoCommit) -> Result<Vec<ChangeHash>, AutomergeError>
// ---------------------------------------------------------------------------
#[test]
fn f1_roundtrip_and_idempotent_merge() {
    let file = TodosFile {
        todos: vec![
            MiniTodo {
                title: "a".into(),
                done: false,
            },
            MiniTodo {
                title: "b".into(),
                done: true,
            },
        ],
    };

    let mut a = AutoCommit::new();
    reconcile(&mut a, &file).unwrap();

    // round-trip
    let bytes = a.save();
    let loaded = AutoCommit::load(&bytes).unwrap();
    let hydrated: TodosFile = hydrate(&loaded).unwrap();
    assert_eq!(hydrated, file, "save/load round-trip must preserve content");

    // fork C from A's bytes (shared history) and append a DIFFERENT todo additively
    let mut c = AutoCommit::load(&bytes).unwrap();
    c.set_actor(ActorId::from([9u8; 16]));
    let (_, todos_id) = c.get(ROOT, "todos").unwrap().unwrap();
    let idx = c.length(&todos_id);
    let m = c.insert_object(&todos_id, idx, ObjType::Map).unwrap();
    c.put(&m, "title", "c").unwrap();
    c.put(&m, "done", false).unwrap();

    // merge TWICE; second merge must be a no-op (idempotent)
    a.merge(&mut c).unwrap();
    let after1: TodosFile = hydrate(&a).unwrap();
    assert_eq!(after1.todos.len(), 3, "merge must add C's todo");

    a.merge(&mut c).unwrap();
    let after2: TodosFile = hydrate(&a).unwrap();
    assert_eq!(
        after2.todos.len(),
        3,
        "second merge must be idempotent (no dup)"
    );

    let titles: Vec<String> = after2.todos.iter().map(|t| t.title.clone()).collect();
    assert!(titles.contains(&"a".to_string()));
    assert!(titles.contains(&"b".to_string()));
    assert!(titles.contains(&"c".to_string()));
}

// ---------------------------------------------------------------------------
// Finding 2 (THE key finding): write ONE root key without pruning siblings.
//   Mechanism: reconcile_prop(doc, ROOT, "todos", value) touches only "todos".
//   reconcile_prop<D: Doc, R: Reconcile, O: AsRef<ObjId>, P: Into<Prop>>(
//       doc, obj, prop, value) -> Result<(), ReconcileError>
// ---------------------------------------------------------------------------
#[test]
fn f2_subproperty_write_preserves_sibling() {
    let mut doc = AutoCommit::new();
    reconcile_prop(
        &mut doc,
        ROOT,
        "todos",
        vec![MiniTodo {
            title: "t1".into(),
            done: false,
        }],
    )
    .unwrap();
    doc.put(ROOT, "comments", "keepme").unwrap();

    // Rewrite ONLY todos with entirely different content.
    reconcile_prop(
        &mut doc,
        ROOT,
        "todos",
        vec![
            MiniTodo {
                title: "t2".into(),
                done: true,
            },
            MiniTodo {
                title: "t3".into(),
                done: false,
            },
        ],
    )
    .unwrap();

    // Sibling "comments" survives untouched.
    let (val, _) = doc.get(ROOT, "comments").unwrap().unwrap();
    assert_eq!(
        val.to_str(),
        Some("keepme"),
        "reconcile_prop on todos must not disturb comments"
    );
    let todos: Vec<MiniTodo> = hydrate_prop(&doc, ROOT, "todos").unwrap();
    assert_eq!(todos.len(), 2);
    assert_eq!(todos[0].title, "t2");
}

#[derive(Reconcile, Hydrate, Debug)]
struct TodosOnlyRoot {
    todos: Vec<MiniTodo>,
}

// Companion to finding 2: does a WHOLE-ROOT reconcile of a struct lacking a
// `comments` field prune the sibling? (Derived struct Reconcile puts its own
// fields and does NOT call retain, so the sibling SURVIVES — recorded so the
// feature never assumes root-reconcile is destructive.)
#[test]
fn f2b_root_reconcile_does_not_prune_sibling() {
    let mut doc = AutoCommit::new();
    reconcile_prop(
        &mut doc,
        ROOT,
        "todos",
        vec![MiniTodo {
            title: "x".into(),
            done: false,
        }],
    )
    .unwrap();
    doc.put(ROOT, "comments", "sibling").unwrap();

    reconcile(
        &mut doc,
        TodosOnlyRoot {
            todos: vec![MiniTodo {
                title: "y".into(),
                done: false,
            }],
        },
    )
    .unwrap();

    let comments = doc.get(ROOT, "comments").unwrap();
    assert!(
        comments.is_some(),
        "whole-root struct reconcile unexpectedly pruned unmodeled sibling `comments`"
    );
    let todos: Vec<MiniTodo> = hydrate_prop(&doc, ROOT, "todos").unwrap();
    assert_eq!(todos[0].title, "y", "todos should be updated by reconcile");
}

// ---------------------------------------------------------------------------
// Finding 3: unknown map key — reconcile a struct{title} onto a map that also
// has x_unknown. Derived struct Reconcile does not retain/prune → SURVIVES.
// ---------------------------------------------------------------------------
#[derive(Reconcile, Hydrate, Debug)]
struct TitleOnly {
    title: String,
}

#[test]
fn f3_unknown_key_survives() {
    let mut doc = AutoCommit::new();
    let item = doc.put_object(ROOT, "item", ObjType::Map).unwrap();
    doc.put(&item, "title", "orig").unwrap();
    doc.put(&item, "x_unknown", "extra").unwrap();

    // Reconcile a struct with ONLY `title` into that same child map.
    reconcile_prop(
        &mut doc,
        ROOT,
        "item",
        TitleOnly {
            title: "updated".into(),
        },
    )
    .unwrap();

    let (_, item_id) = doc.get(ROOT, "item").unwrap().unwrap();
    let x = doc.get(&item_id, "x_unknown").unwrap();
    assert!(
        x.is_some(),
        "x_unknown must SURVIVE (derived Reconcile does not prune unknown keys)"
    );
    assert_eq!(x.unwrap().0.to_str(), Some("extra"));
    let (tval, _) = doc.get(&item_id, "title").unwrap().unwrap();
    assert_eq!(tval.to_str(), Some("updated"));
}

// ---------------------------------------------------------------------------
// Finding 4: hydrate a struct with a `Vec` field from a doc LACKING that key.
// Vec uses the default hydrate_none() => Err(Unexpected::None). So it ERRORS —
// ensure_root MUST pre-create empty containers.
//   hydrate::<TodosFile2>(&doc) -> Result<TodosFile2, HydrateError>
// ---------------------------------------------------------------------------
#[derive(Hydrate, Default, Debug)]
#[allow(dead_code)] // fields populated by derived Hydrate; not read directly in the spike
struct TodosFile2 {
    revision: u64,
    todos: Vec<MiniTodo>,
}

#[test]
fn f4_missing_key_hydrate_errors() {
    let mut doc = AutoCommit::new();
    doc.put(ROOT, "revision", 1u64).unwrap();
    // deliberately no "todos" key

    let res: Result<TodosFile2, _> = hydrate(&doc);
    assert!(
        res.is_err(),
        "hydrate of a Vec field over a missing key must ERROR (not default to empty)"
    );
}

// ---------------------------------------------------------------------------
// Finding 5: fixed 16-byte actor stability + hostuuid hardware id.
//   ActorId::from([u8;16]); ActorId::to_hex_string() -> String;
//   ActorId::try_from(&str); set_actor(&mut self, ActorId) -> &mut Self.
//   libc::gethostuuid(id: *mut u8, timeout: *const libc::timespec) -> c_int
// ---------------------------------------------------------------------------
#[test]
fn f5_fixed_actor_and_hostuuid() {
    let a = ActorId::from([1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    let hex = a.to_hex_string();
    assert_eq!(hex, "0102030405060708090a0b0c0d0e0f10");
    let a2 = ActorId::try_from(hex.as_str()).unwrap();
    assert_eq!(a, a2, "actor hex must round-trip");

    // two mutate -> save -> reload cycles REUSING the same fixed actor
    let mut doc = AutoCommit::new();
    doc.set_actor(a.clone());
    doc.put(ROOT, "n", 1i64).unwrap();
    let b1 = doc.save();

    let mut doc = AutoCommit::load(&b1).unwrap();
    doc.set_actor(a.clone());
    doc.put(ROOT, "n", 2i64).unwrap();
    let b2 = doc.save();

    let doc = AutoCommit::load(&b2).unwrap();
    let (v, _) = doc.get(ROOT, "n").unwrap().unwrap();
    assert_eq!(v.to_i64(), Some(2), "reloaded value must reflect last write");

    // hardware id via gethostuuid (macOS)
    let mut buf = [0u8; 16];
    let ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::gethostuuid(buf.as_mut_ptr(), &ts as *const libc::timespec) };
    assert_eq!(rc, 0, "gethostuuid must return 0");
    assert!(
        buf.iter().any(|&b| b != 0),
        "gethostuuid must fill a non-zero 16-byte id"
    );
    // and it must be usable as a stable actor
    let _hw_actor = ActorId::from(buf);
}

// ---------------------------------------------------------------------------
// Finding 6: char-level CRDT text merge on a shared Text child.
//   put_object(ROOT, "content", ObjType::Text); splice_text/update_text;
//   read back via text(&obj) -> Result<String, AutomergeError>.
// ---------------------------------------------------------------------------
#[test]
fn f6_text_merge() {
    let mut a = AutoCommit::new();
    let content = a.put_object(ROOT, "content", ObjType::Text).unwrap();
    a.splice_text(&content, 0, 0, "b").unwrap();
    let bytes = a.save();

    let mut b = AutoCommit::load(&bytes).unwrap();
    b.set_actor(ActorId::from([7u8; 16]));

    let (_, ac) = a.get(ROOT, "content").unwrap().unwrap();
    let (_, bc) = b.get(ROOT, "content").unwrap().unwrap();

    // concurrent, non-overlapping edits at each end
    a.update_text(&ac, "ab").unwrap(); // insert 'a' at front
    b.update_text(&bc, "bc").unwrap(); // insert 'c' at end

    a.merge(&mut b).unwrap();
    let s = a.text(&ac).unwrap();
    assert_eq!(s.len(), 3, "both concurrent inserts must survive: {s:?}");
    assert!(
        s.contains('a') && s.contains('b') && s.contains('c'),
        "merged text lost an edit: {s:?}"
    );
}
