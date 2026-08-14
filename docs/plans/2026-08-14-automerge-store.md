# Plan: automerge-backed store (plan:automerge-store)

- **Goal:** replace tally's JSON/markdown store backend with an embedded automerge (CRDT)
  document persisted as ONE full-doc snapshot file per machine
  (`automerge/<actor-hex>.automerge`), so the store merges losslessly across machines over
  a dumb synced folder while staying local-only by default. Ships with a one-time
  migration and a `tally dump` command so nothing is lost and cat-ability survives.
- **Design:** scratchpad `s_dkonyc138bbk16` (tag `design`, rev 3). Read in full first.
- **Branch:** `feat/automerge-store`
- **Whole-feature verify:** `cargo test && cargo clippy && cargo fmt --check` (+ CLAUDE.md
  build: rebuild `bin/tally` with `rm -f` before `cp`).

## Shared context (all tasks; do not deviate)

### Storage model = per-machine snapshot
Each machine owns exactly one file `automerge/<our-actor-hex>.automerge`; it is the SOLE
writer of that file. Every mutation: flock -> load+merge every `automerge/*.automerge`
into one `AutoCommit` -> hydrate the touched entity to structs -> [existing method logic
mutates structs] -> reconcile back into the doc -> `doc.save()` -> `atomic_write` to OUR
file only. Load = merge all files (any order; idempotent). NO per-change files, NO
compaction, NO snapshot/incremental split.

### Root doc schema (PINNED — every task uses exactly this)
Root map has exactly three keys:
- `todos` — a Map reconciled from `TodosFile` = `{ revision: Int, todos: List<Map> }`.
- `comments` — a Map reconciled from `CommentsFile` = `{ comments: List<Map> }`.
- `scratchpads` — a Map keyed by pad id; each value is a Map of the pad's metadata
  scalars PLUS a `content` child of `ObjType::Text` (the body, char-mergeable).
`revision` lives ONLY inside the `todos` map (from `TodosFile.revision`). There is NO
top-level revision. Reconciling one entity MUST NOT prune the other two root keys —
Task 2's spike (T1) determines the exact autosurgeon call (`reconcile`/`hydrate` at a
sub-property, or low-level automerge get/put) that writes one root key without touching
siblings; the entity helpers below are implemented on top of that finding.

### The seam = named helpers (no invented names; owners in parens)
Generic doc infra — Task 2: `with_doc`, `load_doc`, `save_doc`, `machine_actor`,
`ensure_root`. Entity doc-mapping helpers, each hydrating/reconciling ONLY its root key
without pruning siblings:
- todos/comments — Task 4: `load_todos_file(doc)->TodosFile`,
  `save_todos_file(doc,&TodosFile)`, `load_comments_file(doc)->CommentsFile`,
  `save_comments_file(doc,&CommentsFile)`.
- scratchpads — Task 5: `load_pad(doc,id)->Option<Scratchpad>`, `save_pad(doc,&Scratchpad)`
  (body via `update_text`), `pad_ids(doc)->Vec<String>`, `remove_pad(doc,id)`.
Tasks 4-7 call these; they never invent ad-hoc doc access.

### Identity (machine + repo)
- **Repo disambiguation is the existing frozen store key** — unchanged. Each repo =
  `projects/<base>-<sha1(abspath)[:8]>/` (abspath = git main-worktree root via
  `--git-common-dir`, so worktrees share one store). `automerge/` lives INSIDE that keyed
  dir, so every repo has its own doc + its own snapshot files. Nothing here changes it.
- **Machine identity MUST be per-machine and NEVER synced.** Derive it from the macOS
  hardware UUID via `libc::gethostuuid` (16 bytes; `libc` already a dep) — do NOT persist
  it to a file in the store dir (a synced `automerge/actor` file would make machine B read
  machine A's id and both write the same snapshot file, destroying single-owner safety).
  `machine_actor() -> ActorId` = `ActorId::from(gethostuuid_bytes)`; `machine_hex()` = its
  hex. Our snapshot file = `<machine_hex>.automerge`. On load, `doc.set_actor(machine_actor()?)`
  before mutating. This SUPERSEDES the design's "machine-id/uuid persisted at automerge/
  machine-id" wording and the earlier plan's `automerge/actor` file — there is NO persisted
  actor file; identity is computed fresh each run from hardware.
- NOTE: `Project` already has an unrelated field `pub actor: String` (`project.rs:17`) =
  the ATTRIBUTION owner ("agent"/"you") for `created_by`/`updated_by`; do NOT reuse or
  shadow it.

### Invariants kept
- flock (`with_file_lock`, `lock.rs:13`) wraps every mutation; `atomic_write` (`lock.rs:39`)
  writes our snapshot. Store-key dir layout frozen (`project.rs:96`); `automerge/` sits
  under `self.dir` beside the now-legacy json.
- Structs carry BOTH `serde` (legacy-JSON parse in migration) AND `autosurgeon` derives.
- Revision guards stay verbatim in method bodies (scratchpad `revision`+`-1` skip
  `scratchpads.rs:266`; todo `expected_updated` `edit_todo_raw` `todos.rs:378-412`); they
  operate on hydrated structs. Cross-machine they soften to advisory; the
  `RevisionMismatch`/`ConcurrentEdit` API contract is preserved.
- `revision += 1` happens EXACTLY ONCE per todo/pad mutation, in the `mutate_*` wrapper
  after `f` (as today `save_todos` did) — the old bump site is REMOVED, not duplicated.

---

## Task 1 — Add deps + automerge/autosurgeon behavior spike
**Exists because:** the entity helpers' implementation hinges on autosurgeon 0.11/0.13
behaviors not yet verified here; a wrong guess breaks Tasks 2-6 silently. Converts each
unknown into a recorded `build-log:automerge-store` finding the later tasks cite.
**Model:** `opus` (findings shape 4 downstream tasks).
**Files:** `Cargo.toml`; `src/store/amspike.rs` (new `#[cfg(test)]`-only throwaway, registered
`#[cfg(test)] mod amspike;` in `src/store/mod.rs`; deleted in Task 2). Writes a
`build-log:automerge-store` tally scratchpad.
**Steps (test-first — each assertion IS the deliverable; record verdict + exact signature
used in the build-log):**
1. `cargo add automerge@0.11 autosurgeon@0.13`; `cargo build`.
2. `#[test]`s asserting:
   - **round-trip + idempotent merge:** doc A with a `todos` List of 2 maps; `b=A.save()`;
     `AutoCommit::load(&b)` hydrates equal; doc C with a different todo; `A.merge(&mut C)`
     twice — todo count stable (idempotent), union present. (Storage-model proof.)
   - **sub-property write WITHOUT sibling prune (THE key finding):** doc with root keys
     `todos` + `comments`; reconcile/put ONLY `todos`; assert `comments` still present.
     Record the exact call used (autosurgeon `hydrate`/`reconcile` at a prop if such exists,
     else low-level `put_object`/`get`+autosurgeon-on-subobject). This defines every
     `load_/save_*` helper.
   - **unknown-key prune:** map `{title, x_unknown}`; reconcile a `struct{title:String}`
     onto it; assert whether `x_unknown` survives. (Decides Todo.extra + scratchpad
     `content` handling in Task 3/5.)
   - **missing-key hydrate:** hydrate a `TodosFile` from a doc lacking `todos`; assert
     error-vs-default. (Decides `ensure_root`.)
   - **fixed-actor stability + hardware id:** confirm `ActorId::from([u8;16])` builds a
     valid actor and `.to_hex_string()` (or equivalent) round-trips; `set_actor(a)` then two
     sequential mutate+save+reload cycles reusing the SAME fixed `a` — assert no panic,
     coherent history. Separately confirm `libc::gethostuuid(&mut buf, &timespec{0,0})`
     returns 0 and fills 16 nonzero bytes (this is `machine_actor`'s source in Task 2).
   - **text merge:** two docs `update_text` a shared Text differently; merge; both edits
     survive.
3. Write the build-log scratchpad, one line per finding with the exact API signature.
**Verify:** `cargo test store::amspike` and `cargo build`.

## Task 2 — Generic per-machine-snapshot doc infra (`amdoc.rs`)
**Exists because:** the core seam replacing whole-file-json load/save with load-merge-all +
save-our-snapshot. Without it there is no CRDT persistence or lossless sync; the dumb
one-shared-blob alternative silently loses cross-machine edits.
**Model:** `opus` (storage core; concurrency + single-owner-file correctness).
**Files:** `src/store/amdoc.rs` (new); `src/store/mod.rs` (`mod amdoc;`, remove the
`#[cfg(test)] mod amspike;` line + delete `amspike.rs` after absorbing findings);
`src/store/project.rs` (path helper `am_dir()`=`self.dir.join("automerge")`, beside
`todos_path()` `:137`). Reuse `lock.rs::{with_file_lock, atomic_write}`. NO `actor_path`
helper — machine identity is computed from hardware (see Identity), not persisted.
**Interfaces produced (consumed by Tasks 4-7):**
- `pub(crate) fn load_doc(&self) -> Result<AutoCommit>` — first calls `migrate_if_needed`
  (Task 6). Then: absent `am_dir` -> `AutoCommit::new()`; else read every `*.automerge`,
  load first + merge/load_incremental the rest (per T1 union finding). Then
  `set_actor(self.machine_actor()?)` + `ensure_root`.
- `pub(crate) fn save_doc(&self, doc:&mut AutoCommit) -> Result<()>` — `let b=doc.save();
  create_dir_all(am_dir); atomic_write(&am_dir().join(format!("{}.automerge",
  self.machine_hex()?)), &b)`.
- `pub(crate) fn with_doc<T>(&self, f: impl FnOnce(&mut AutoCommit)->Result<T>) ->
  Result<T>` — `with_file_lock(&am_dir().join("lock"), || { let mut d=self.load_doc()?;
  let r=f(&mut d)?; self.save_doc(&mut d)?; Ok(r) })`. Single mutation choke point.
- `fn machine_actor(&self)->Result<ActorId>` = `ActorId::from(gethostuuid_bytes())`;
  `fn machine_hex(&self)->Result<String>` = its hex. `gethostuuid_bytes()` calls
  `libc::gethostuuid(&mut [0u8;16], &timespec{0,0})` (macOS; `libc` already a dep) and
  returns the 16 bytes. NO file read/write — deterministic per machine, so a fully synced
  store dir never makes two machines share an identity (see Identity). If `gethostuuid`
  fails (nonzero return), error out — do NOT fall back to a random/persisted id (that would
  reintroduce the collision).
- `fn ensure_root(doc:&mut AutoCommit)` — create empty `todos`(List)/`comments`(List; the
  containers hold the entity maps)/`scratchpads`(Map) only if T1 shows hydrate needs them
  present; else no-op.
**Steps (test-first, `crate::store::testutil::new_project` `testutil.rs:72`):**
1. `two_machines_merge`: project; `with_doc` puts todo "A" (raw automerge API for this infra
   test — entity helpers arrive in T4); write a SECOND `<other-hex>.automerge` (separate
   AutoCommit with todo "B", saved) into `am_dir`; `load_doc` + a raw read asserts BOTH
   present. Proves single-owner files + load-merge union.
2. `empty_is_empty_doc`: fresh project, `load_doc` on absent `am_dir` returns a doc with an
   empty `todos` container, no error.
3. `single_owner_file`: two sequential `with_doc` mutations produce exactly ONE
   `<our-hex>.automerge` (file count == 1) — same machine hex both times.
**Verify:** `cargo test store::amdoc`.

## Task 3 — autosurgeon derives on store structs + Todo unknown-key preservation
**Exists because:** hydrate/reconcile need the structs annotated; and `Todo.extra`
(`#[serde(flatten)]`, `todos.rs:83-90`) cannot autosurgeon-derive — per T1, if reconcile
prunes unknown keys it would drop fields written by a NEWER tally version on another synced
machine (the exact bug `extra` prevents, now cross-version).
**Model:** `opus` (unknown-key handling is a manual-trait design point).
**Files:** `src/store/todos.rs` (`Todo`,`TodosFile`,`Lock`,`GithubLink`),
`src/store/comments.rs` (`Comment`,`CommentsFile`), `src/store/scratchpads.rs`
(`Scratchpad`). Full field/rename lists at `todos.rs:21-100`, `comments.rs:16-51`,
`scratchpads.rs:22-48`.
**Steps (test-first):**
1. Add `#[derive(autosurgeon::Reconcile, autosurgeon::Hydrate)]` beside serde on
   `Comment`,`CommentsFile`,`Lock`,`GithubLink`,`TodosFile`,`Todo`,`Scratchpad`. On `Todo`
   put `#[autosurgeon(skip)]` on `extra`. On `Scratchpad` put `#[autosurgeon(skip)]` on
   `content` (the body is a Text child written by Task 5's `save_pad`, not by the derive).
2. `Todo.extra` preservation per T1 verdict: if reconcile PRUNES unknown keys, hand-write
   `impl autosurgeon::Reconcile for Todo` reconciling each known field and NEVER deleting
   keys it doesn't own (leave unknown map keys intact); `#[derive(Hydrate)]` still fine
   with `extra` skipped. If reconcile PRESERVES, the `skip` alone suffices — leave a code
   comment citing the build-log finding either way.
3. `reconcile_roundtrip`: for each entity, struct -> reconcile into fresh doc -> hydrate
   back -> assert equal on all known fields.
4. `todo_preserves_unknown_field` (invariant, analog of `test_reads_go_written_todos_json`
   `todos.rs:922`): doc where a todo map has extra key `future_field`; hydrate `Todo`,
   mutate `title`, reconcile back; assert `future_field` still in the doc.
**Verify:** `cargo test store::todos::reconcile store::todos::todo_preserves store::comments::reconcile store::scratchpads::reconcile`.

## Task 4 — todos + comments doc-mapping helpers + rewire
**Exists because:** todos/comments must read/write the doc, not json, or the swap is inert.
(Combined: comments is a trivial analog — same seam, no revision guard — one review covers
both.)
**Model:** `sonnet` (seam swap on Task 2's infra; logic bodies unchanged).
**Files:** `src/store/todos.rs` (`load_todos`/`save_todos`/`mutate_todos` `:215-241`),
`src/store/comments.rs` (`load_comments`/`save_comments`/`mutate_comments` `:74-96`),
`src/store/amdoc.rs` (add the four helpers).
**Interfaces:** consumes `with_doc`/`load_doc` (T2), derives (T3); produces
`load_todos_file`/`save_todos_file`/`load_comments_file`/`save_comments_file` per the T1
sub-property recipe (hydrate/reconcile the `todos`/`comments` root key WITHOUT pruning
siblings), with missing-key -> `Default`.
**Steps (test-first):**
1. Implement the four helpers in `amdoc.rs`.
2. `mutate_todos`: `self.with_doc(|doc| { let mut tf=load_todos_file(doc)?; let
   r=f(&mut tf)?; tf.revision+=1; save_todos_file(doc,&tf)?; Ok(r) })`. Drop `serde_json`
   + `atomic_write` from this path; **remove** the old `tf.revision += 1` in `save_todos`
   (bump now lives here, once).
3. `load_todos`: hydrate via `load_todos_file(&mut self.load_doc()?)`, then apply
   `migrate_legacy_priority` (`todos.rs:222`) post-hydrate. Missing -> empty `TodosFile`.
4. Same shape for comments (no revision; `mutate_comments`). Keep `expected_updated` check
   exactly where it is in the todo update path.
**Verify:** `cargo test store::todos store::comments` (FULL existing suites stay green —
create/get/list/update/complete/lock/blockers/tags; add/list/delete/recent).

## Task 5 — scratchpads doc-mapping (body as Text) + rewire ALL five disk touchpoints
**Exists because:** pad bodies are long prose edited on multiple machines; scalar overwrite
would clobber concurrent edits, so the body must be a Text CRDT. AND create/list/delete
currently hit the filesystem OUTSIDE the mutate_pad seam — they must all move to the doc or
new pads silently keep writing `.md` files and listing returns empty after migration.
**Model:** `opus` (Text handling + touching five methods precisely).
**Files:** `src/store/scratchpads.rs` — `create_scratchpad` (writes via `atomic_write`
~`:301`), `read_pad` `:248`, `mutate_pad` `:257-276`, `list_scratchpads` (`read_dir` ~`:329`),
`delete_scratchpad` (`remove_file` ~`:445`); keep `parse_pad`/`render` only as the `--file`
export/import format. `src/store/amdoc.rs` (the four pad helpers).
**Interfaces:** consumes `with_doc`/`load_doc` (T2), derives (T3); produces `load_pad`,
`save_pad` (metadata via autosurgeon per T1 recipe + `content` via
`doc.update_text(text_obj, &s.content)`; read reads the Text child back into
`s.content: String`), `pad_ids`, `remove_pad`.
**Steps (test-first):**
1. Implement the four pad helpers in `amdoc.rs`.
2. Rewire each method to the helpers, all through `with_doc`/`load_doc`:
   - `create_scratchpad` -> `with_doc(|d| { save_pad(d,&pad); ... })` (no `atomic_write`).
   - `read_pad` -> `load_pad(&mut self.load_doc()?, id)`; missing -> `Error::NotFound`
     (`:248-254`).
   - `mutate_pad` -> `with_doc(|d| { let mut s=load_pad(d,id)?.ok_or(NotFound)?; if exp_rev
     >=0 && s.revision!=exp_rev {return Err(RevisionMismatch)} f(&mut s)?; s.revision+=1;
     s.updated=now(); save_pad(d,&s); Ok(()) })` — guard + bump verbatim from `:266-273`.
   - `list_scratchpads` -> `pad_ids(&doc)` then `load_pad` each; apply the existing
     tag/query/archived/offset/limit filtering to the loaded set (same predicate as today,
     just sourced from the doc not `read_dir`).
   - `delete_scratchpad` -> `with_doc(|d| { guard exp_rev; remove_pad(d,id) })`.
3. Make `parse_pad` `pub(crate)` (Task 6's migration parses legacy `.md` with it).
**Steps test:** `pad_body_merges` (two docs `update_text` same pad differently -> merge ->
both survive) + the FULL existing pad suite stays green (create/read/update/append/
append_section/edit/rename/archive/tags/find/tail/clear).
**Verify:** `cargo test store::scratchpads`.

## Task 6 — One-time JSON->automerge migration on first run
**Exists because:** existing users have `todos.json`/`comments.json`/`scratchpads/*.md`;
without migration their data is invisible after the swap.
**Model:** `sonnet` (follows `migrate_legacy_priority`'s load-path self-heal idiom).
**Files:** `src/store/amdoc.rs` (`migrate_if_needed`, called at the FRONT of `load_doc`).
Reads legacy paths `todos_path()`/`comments_path()`/`scratch_dir()` (`project.rs:137-155`)
via the retained serde structs + `pub(crate) parse_pad`.
**Interfaces:** consumes `save_todos_file`/`save_comments_file` (T4), `save_pad` (T5),
`save_doc` (T2).
**Steps (test-first):**
1. `migrate_if_needed(&self)`: if `am_dir` ABSENT and any legacy file exists -> build a
   fresh `AutoCommit` (`ensure_root`); parse `todos.json` (serde `TodosFile`),
   `comments.json` (serde `CommentsFile`), each `scratchpads/*.md` (`parse_pad`);
   `save_todos_file`/`save_comments_file`/`save_pad` each into the doc; `save_doc`; then
   `rename` each legacy file to `<name>.migrated` (KEEP as backup, do NOT delete). One
   `save()` = one snapshot from CURRENT state (not per-revision history). Presence-based,
   one-shot: after it runs `am_dir` exists so `load_doc` never re-migrates.
2. Call it as the first line of `load_doc`.
3. `migrates_legacy_store` (byte-fixture like `todos.rs:922`): seed a temp project dir with
   a hand-written `todos.json` (incl. `"priority":"high"`), a `comments.json`, one
   `scratchpads/s_x.md`; `load_doc`; assert all entities hydrate (priority `high->p1`),
   `todos.json.migrated` exists, `todos.json` gone.
**Verify:** `cargo test store::amdoc::migrates`.

## Task 7 — `tally dump [--json]` CLI command
**Exists because:** the store is now a binary blob; `cat todos.json` no longer works, so
inspection needs a readable dump (required in-scope, ships WITH the migration).
**Model:** `sonnet` (follows the CLI subcommand pattern exactly).
**Files:** `src/cli/dump.rs` (new); `src/cli/mod.rs` (`mod dump;` `:14-18`; `pub fn
dump(args)` wrapper `:21-43`); `src/main.rs` (dispatch arm + usage `:11-20`). Follows
`src/cli/todos.rs:54-131` and helpers `resolve`/`print_json`/`fail`/`parse`/`project_opt`
(`cli/mod.rs:47-196`).
**Steps (test-first):**
1. `run(args, store_root, out)->i32`: `BOOL_FLAGS=&["json"]`, `VALUE_FLAGS=&["project"]`;
   parse -> `resolve(project_opt(&project), store_root)`; `let mut doc=proj.load_doc()?`;
   read the whole store via helpers — `load_todos_file(&doc)`, `load_comments_file(&doc)`
   (ALL comments across targets, no per-target filter), `pad_ids(&doc)`+`load_pad` each
   (INCLUDING archived). Assemble a `#[derive(Serialize)]` combined struct. `--json` ->
   `print_json(out,&dump)`; default -> short human summary (counts + titles). Errors via
   `fail` (`mod.rs:52`).
2. `mod dump;` + wrapper; `Some("dump") => cli::dump(&args[1..])` in `main.rs` match + usage.
3. `dump_json` CLI test (model `todos_create_list_json` `cli/mod.rs:252`): via `Cli` harness
   seed a todo, run `["dump","--json","--project",<p>]`, assert stdout JSON has the title.
**Verify:** `cargo test cli::dump` and `cargo build`.

## Task 8 — Docs: optional sync setup + CLAUDE.md invariant updates
**Exists because:** sync is opt-in and must be documented (not code); CLAUDE.md's current
JSON-format sentences would misdirect every future agent.
**Model:** `sonnet` (prose; integrate with CLAUDE.md's voice).
**Files:** `CLAUDE.md`; `README.md` (or `docs/`) sync section.
**Steps:**
1. CLAUDE.md: store is now an automerge doc as per-machine snapshot files under
   `projects/<key>/automerge/`; store KEY format + dir layout still frozen; revision guards
   now advisory cross-machine but the API contract holds; `bin/tally` rebuild + stale-pane
   guidance still applies BUT note a stale image now writes a snapshot that MERGES rather
   than clobbers, so the old "silently drops fields" footgun is largely defused — say so.
2. Add "Cross-machine sync (optional)": `mv + ln -s` into a synced folder; two gotchas
   (same abspath -> same store key; iCloud eviction -> `.icloud` placeholder, pin or use
   Syncthing/Dropbox). Explicit: fully optional, works local-only, tally neither detects
   nor manages the sync folder.
3. Point to `tally dump --json` as the inspection path replacing `cat todos.json`.
**Verify:** re-read; `grep -oiF 'todos.json' CLAUDE.md | wc -l` reflects only
historical/migration mentions (no live "store is todos.json" claim); sync section exists,
says "optional". (Docs task — verify is a careful read, not a unit test.)

---

## Ordering (blockers)
T1 -> {T2, T3}; {T2,T3} -> T4; {T2,T3} -> T5; {T4,T5} -> T6; {T4,T5} -> T7; {T6,T7} -> T8.
