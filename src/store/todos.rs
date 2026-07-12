// Port of internal/store/todos.go. Field names are pinned with #[serde(rename)]
// to the exact Go JSON tags so this binary reads todos.json files the Go binary
// wrote (one-way migration: we must read Go's output; keeping the names on the
// write side too so downstream --json consumers see no change).
//
// Todos are NOT revision-guarded (Solo parity) — the per-file flock is the
// concurrency ceiling. The optional expected_updated guard on update_todo is
// the one exception, and it is opt-in.
use std::collections::{BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize};

use super::errors::{Error, Result};
use super::ids::new_id;
use super::lock::{atomic_write, with_file_lock};
use super::project::Project;

/// Advisory lock breadcrumb on a todo. lock_todo overwrites unconditionally —
/// lock-stealing is deliberate (coordination, not a security lock).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Lock {
    #[serde(rename = "owner")]
    pub owner: String,
    #[serde(rename = "pid")]
    pub pid: i64,
    #[serde(rename = "at")]
    pub at: String,
}

/// Opt-in GitHub sync link for a single todo. Absent for unsynced todos.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GithubLink {
    #[serde(rename = "repo")]
    pub repo: String,
    #[serde(rename = "number")]
    pub number: i64,
    #[serde(rename = "last_pushed")]
    pub last_pushed: String,
    #[serde(rename = "last_comment_pull")]
    pub last_comment_pull: String,
    #[serde(rename = "paused")]
    pub paused: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Todo {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "body")]
    pub body: String,
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "priority")]
    pub priority: String,
    #[serde(rename = "tags", deserialize_with = "null_default")]
    pub tags: Vec<String>,
    #[serde(rename = "blockers", deserialize_with = "null_default")]
    pub blockers: Vec<String>,
    #[serde(rename = "lock")]
    pub lock: Option<Lock>,
    #[serde(rename = "created")]
    pub created: String,
    #[serde(rename = "updated")]
    pub updated: String,
    #[serde(rename = "completed")]
    pub completed: Option<String>,
    /// Attribution: who created / last mutated this. Empty on todos written
    /// before attribution shipped (serde default) — never backfilled.
    #[serde(rename = "created_by", default)]
    pub created_by: String,
    #[serde(rename = "updated_by", default)]
    pub updated_by: String,
    /// Opt-in GitHub sync link; None for unsynced todos so existing stores load
    /// unchanged AND unsynced todos serialize byte-identical to today.
    #[serde(rename = "github", default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GithubLink>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct TodosFile {
    #[serde(rename = "revision")]
    revision: i64,
    #[serde(rename = "todos", deserialize_with = "null_default")]
    todos: Vec<Todo>,
}

/// Go's json.Unmarshal turns JSON null into a nil slice, which the store then
/// treats as empty; serde would reject null for Vec, so map null -> Default.
fn null_default<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Clone, Default)]
pub struct TodoFilter {
    pub status: String,
    pub completed: Option<bool>,
    pub is_blocked: Option<bool>,
    pub priority: String,
    pub query: String,
    pub tags: Vec<String>,
    /// priority|created-desc|created-asc|updated-desc|updated-asc|completed-desc|completed-asc
    pub sort: String,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, Default)]
pub struct TodoUpdate {
    pub title: Option<String>,
    pub body: Option<String>,
    pub priority: Option<String>,
    pub status: Option<String>,
    /// Replaces the whole tag list (add_todo_tag preserves existing tags).
    pub tags: Option<Vec<String>>,
    /// When set, makes the update conditional: if the todo's updated timestamp
    /// no longer equals it, the update fails with ConcurrentEdit and nothing
    /// is written. None skips the check (the default for MCP/CLI — todos stay
    /// unguarded there, Solo parity).
    pub expected_updated: Option<String>,
}

/// RFC 3339 UTC at second precision, same shape Go's
/// time.Now().UTC().Format(time.RFC3339) produced ("2026-07-09T18:00:00Z").
pub(crate) fn now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_rfc3339(secs)
}

pub(crate) fn format_rfc3339(secs: u64) -> String {
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

// Howard Hinnant's civil_from_days: days since 1970-01-01 -> (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// Inverse of civil_from_days: (y, m, d) -> days since 1970-01-01.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Parse the fixed-width "YYYY-MM-DDTHH:MM:SSZ" that now() emits back to unix
/// seconds. None for any other shape (callers degrade to "all time").
pub(crate) fn epoch_from_rfc3339(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)? as u32, n(8..10)? as u32);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3_600 + mi * 60 + se;
    u64::try_from(secs).ok()
}

impl TodosFile {
    fn find_mut(&mut self, id: &str) -> Option<&mut Todo> {
        self.todos.iter_mut().find(|t| t.id == id)
    }
}

impl Project {
    fn load_todos(&self) -> Result<TodosFile> {
        let b = match std::fs::read(self.todos_path()) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(TodosFile::default()),
            Err(e) => return Err(e.into()),
        };
        Ok(serde_json::from_slice(&b)?)
    }

    fn save_todos(&self, tf: &mut TodosFile) -> Result<()> {
        tf.revision += 1;
        let b = serde_json::to_vec_pretty(tf)?;
        atomic_write(&self.todos_path(), &b)
    }

    /// Loads, applies f, saves — all under the file lock.
    fn mutate_todos(&self, f: impl FnOnce(&mut TodosFile) -> Result<()>) -> Result<()> {
        with_file_lock(&self.todos_path(), || {
            let mut tf = self.load_todos()?;
            f(&mut tf)?;
            self.save_todos(&mut tf)
        })
    }

    pub fn create_todo(
        &self,
        title: &str,
        body: &str,
        priority: &str,
        tags: Vec<String>,
    ) -> Result<Todo> {
        let priority = if priority.is_empty() {
            "medium".to_string()
        } else {
            normalize_priority(priority)?
        };
        let td = Todo {
            id: new_id("t_"),
            title: title.to_string(),
            body: body.to_string(),
            status: "open".to_string(),
            priority,
            tags,
            blockers: Vec::new(),
            lock: None,
            created: now(),
            updated: now(),
            completed: None,
            created_by: self.actor.clone(),
            updated_by: self.actor.clone(),
            github: None,
        };
        let cp = td.clone();
        self.mutate_todos(|tf| {
            tf.todos.push(cp);
            Ok(())
        })?;
        Ok(td)
    }

    pub fn get_todo(&self, id: &str) -> Result<Todo> {
        let tf = self.load_todos()?;
        tf.todos
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .ok_or(Error::NotFound)
    }

    pub fn delete_todo(&self, id: &str) -> Result<()> {
        self.mutate_todos(|tf| {
            let before = tf.todos.len();
            tf.todos.retain(|t| t.id != id);
            if tf.todos.len() == before {
                return Err(Error::NotFound);
            }
            // Cascade: strip the deleted id from every remaining todo's blockers
            // so dependents don't stay blocked on a todo that no longer exists.
            for t in &mut tf.todos {
                t.blockers.retain(|b| b != id);
            }
            Ok(())
        })?;
        // Cascade comments (separate file/lock, acquired after the todos lock
        // is released — same one-lock-per-file discipline as the store).
        self.delete_comments_for_target(id)
    }

    fn set_complete(&self, id: &str, complete: bool, release_lock: bool) -> Result<Todo> {
        self.edit_todo_raw(id, |t| {
            if complete {
                t.status = "completed".to_string();
                t.completed = Some(now());
            } else {
                t.status = "open".to_string();
                t.completed = None;
            }
            if release_lock {
                t.lock = None;
            }
            Ok(())
        })
    }

    pub fn complete_todo(&self, id: &str, release_lock: bool) -> Result<Todo> {
        self.set_complete(id, true, release_lock)
    }

    pub fn incomplete_todo(&self, id: &str, release_lock: bool) -> Result<Todo> {
        self.set_complete(id, false, release_lock)
    }

    pub fn is_blocked(&self, t: &Todo) -> bool {
        if t.blockers.is_empty() {
            return false;
        }
        match self.load_todos() {
            Ok(tf) => blocked_against(t, &tf.todos),
            Err(_) => false,
        }
    }

    pub fn list_todos(&self, f: TodoFilter) -> Result<Vec<Todo>> {
        let tf = self.load_todos()?;
        let mut out: Vec<Todo> = Vec::new();
        for t in &tf.todos {
            if !f.status.is_empty() && t.status != f.status {
                continue;
            }
            if let Some(want) = f.completed
                && (t.status == "completed") != want
            {
                continue;
            }
            if !f.priority.is_empty() && t.priority != f.priority {
                continue;
            }
            if let Some(want) = f.is_blocked
                && blocked_against(t, &tf.todos) != want
            {
                continue;
            }
            if !f.query.is_empty() {
                let hay = format!("{} {}", t.title, t.body).to_lowercase();
                if !hay.contains(&f.query.to_lowercase()) {
                    continue;
                }
            }
            if !f.tags.is_empty() && !has_all_tags(&t.tags, &f.tags) {
                continue;
            }
            out.push(t.clone());
        }
        sort_todos(&mut out, &f.sort);
        Ok(page(out, f.offset, f.limit))
    }

    pub fn update_todo(&self, id: &str, u: TodoUpdate) -> Result<Todo> {
        self.edit_todo_raw(id, |t| {
            if let Some(exp) = &u.expected_updated
                && t.updated != *exp
            {
                return Err(Error::ConcurrentEdit);
            }
            if let Some(v) = u.title {
                t.title = v;
            }
            if let Some(v) = u.body {
                t.body = v;
            }
            if let Some(v) = u.priority {
                t.priority = normalize_priority(&v)?;
            }
            if let Some(v) = u.status {
                t.status = normalize_status(&v)?;
            }
            if let Some(v) = u.tags {
                t.tags = v;
            }
            Ok(())
        })
    }

    /// Go's editTodo: find, apply f, stamp Updated, return the changed todo.
    fn edit_todo_raw(&self, id: &str, f: impl FnOnce(&mut Todo) -> Result<()>) -> Result<Todo> {
        let mut out = None;
        let mut transition: Option<(String, String)> = None;
        self.mutate_todos(|tf| {
            let t = tf.find_mut(id).ok_or(Error::NotFound)?;
            let before = t.status.clone();
            f(t)?;
            t.updated = now();
            t.updated_by = self.actor.clone();
            if t.status != before {
                transition = Some((before, t.status.clone()));
            }
            out = Some(t.clone());
            Ok(())
        })?;
        if let Some((from, to)) = transition {
            let text = if to == "completed" {
                "marked done".to_string()
            } else if from == "completed" {
                format!("reopened ({from} → {to})")
            } else {
                format!("status: {from} → {to}")
            };
            // Best-effort: the status change already committed; a failed event
            // write must not turn a successful mutation into an error.
            let _ = self.add_comment_event(id, &text);
        }
        out.ok_or(Error::NotFound) // unreachable: mutate succeeded => out is set
    }

    pub fn add_todo_tag(&self, id: &str, tag: &str) -> Result<Todo> {
        self.edit_todo_raw(id, |t| {
            if !t.tags.iter().any(|x| x == tag) {
                t.tags.push(tag.to_string());
            }
            Ok(())
        })
    }

    pub fn remove_todo_tag(&self, id: &str, tag: &str) -> Result<Todo> {
        self.edit_todo_raw(id, |t| {
            t.tags.retain(|x| x != tag);
            Ok(())
        })
    }

    pub fn set_blockers(&self, id: &str, blockers: Vec<String>) -> Result<Todo> {
        self.edit_todo_raw(id, |t| {
            t.blockers = blockers;
            Ok(())
        })
    }

    pub fn add_blocker(&self, id: &str, blocker: &str) -> Result<Todo> {
        self.edit_todo_raw(id, |t| {
            if !t.blockers.iter().any(|x| x == blocker) {
                t.blockers.push(blocker.to_string());
            }
            Ok(())
        })
    }

    pub fn remove_blocker(&self, id: &str, blocker: &str) -> Result<Todo> {
        self.edit_todo_raw(id, |t| {
            t.blockers.retain(|x| x != blocker);
            Ok(())
        })
    }

    pub fn lock_todo(&self, id: &str, owner: &str, pid: i64) -> Result<Todo> {
        self.edit_todo_raw(id, |t| {
            t.lock = Some(Lock {
                owner: owner.to_string(),
                pid,
                at: now(),
            });
            Ok(())
        })
    }

    pub fn unlock_todo(&self, id: &str, owner: &str) -> Result<Todo> {
        self.edit_todo_raw(id, |t| {
            if let Some(l) = &t.lock
                && l.owner != owner
            {
                return Err(Error::Other(format!("lock owned by {}", l.owner)));
            }
            t.lock = None;
            Ok(())
        })
    }

    /// The opt-in toggle behind the box-tick. `on` links (or un-pauses) the todo;
    /// `off` pauses without dropping repo/number so re-ticking relinks the same
    /// issue. Resolving the origin is only required when creating a fresh link.
    pub fn set_github(&self, id: &str, on: bool) -> Result<Todo> {
        // Resolve origin up front (edit closure can't re-borrow self). Only the
        // fresh-link branch consumes it; re-tick/off ignore it.
        let origin = if on { self.origin_repo() } else { None };
        self.edit_todo_raw(id, |t| {
            match (&mut t.github, on) {
                (Some(link), true) => link.paused = false,
                (Some(link), false) => link.paused = true,
                (None, true) => {
                    let repo = origin.clone().ok_or_else(|| {
                        Error::Other("no git origin remote; cannot link to GitHub".to_string())
                    })?;
                    t.github = Some(GithubLink {
                        repo,
                        number: 0,
                        last_pushed: String::new(),
                        last_comment_pull: String::new(),
                        paused: false,
                    });
                }
                (None, false) => {} // no-op: nothing to unlink
            }
            Ok(())
        })
    }

    /// Persist a link's fields (number/timestamps) WITHOUT touching updated/
    /// updated_by or logging an event — this is sync's own writeback, not a user
    /// edit, so it must not re-trigger the `updated > last_pushed` push rule.
    pub(crate) fn update_github_link(&self, id: &str, mut link: GithubLink) -> Result<()> {
        self.mutate_todos(|tf| {
            let t = tf.find_mut(id).ok_or(Error::NotFound)?;
            // Merge, don't clobber: a concurrent un-tick set paused=true after this
            // pass cloned the link, so preserve the stored paused rather than the
            // (stale) paused carried in `link`.
            if let Some(existing) = &t.github {
                link.paused = existing.paused;
            }
            t.github = Some(link);
            Ok(())
        })
    }

    pub fn todo_tags(&self) -> Result<Vec<String>> {
        let tf = self.load_todos()?;
        let set: BTreeSet<&String> = tf.todos.iter().flat_map(|t| &t.tags).collect();
        Ok(set.into_iter().cloned().collect())
    }
}

/// The only statuses the rest of tally understands: the list filter, blocker
/// checks, and transition messages all key on the literal "completed". Status
/// is free-form on disk, but user input funnels through update_todo → here so a
/// typo like "closed" can't create a ghost that displays as done yet never
/// leaves the open view (only status=="completed" is filtered out). Formatting
/// is normalized (case, surrounding space, space/hyphen → underscore); anything
/// that still isn't canonical is rejected rather than guessed at.
fn normalize_status(raw: &str) -> Result<String> {
    let s = raw.trim().to_lowercase().replace([' ', '-'], "_");
    match s.as_str() {
        "open" | "in_progress" | "completed" => Ok(s),
        _ => Err(Error::Other(format!(
            "invalid status {raw:?}: expected open, in_progress, or completed"
        ))),
    }
}

/// Same story as normalize_status for priority: prio_rank sorts any unknown
/// value alongside "high" (todos.rs), so a typo like "urgent" silently jumps
/// the queue. User input is normalized (case, surrounding space) and rejected
/// unless it's one of the three ranks.
fn normalize_priority(raw: &str) -> Result<String> {
    let s = raw.trim().to_lowercase();
    match s.as_str() {
        "high" | "medium" | "low" => Ok(s),
        _ => Err(Error::Other(format!(
            "invalid priority {raw:?}: expected high, medium, or low"
        ))),
    }
}

fn blocked_against(t: &Todo, all: &[Todo]) -> bool {
    let status: HashMap<&str, &str> = all
        .iter()
        .map(|x| (x.id.as_str(), x.status.as_str()))
        .collect();
    t.blockers
        .iter()
        .any(|b| status.get(b.as_str()).copied() != Some("completed"))
}

/// Go's prioRank map returned 0 (its zero value) for unknown priorities, so
/// anything unrecognized sorts alongside "high". Preserved.
fn prio_rank(p: &str) -> i32 {
    match p {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 0,
    }
}

fn sort_todos(ts: &mut [Todo], key: &str) {
    match key {
        "priority" => ts.sort_by_key(|t| prio_rank(&t.priority)),
        "created-asc" => ts.sort_by(|a, b| a.created.cmp(&b.created)),
        "updated-desc" => ts.sort_by(|a, b| b.updated.cmp(&a.updated)),
        "updated-asc" => ts.sort_by(|a, b| a.updated.cmp(&b.updated)),
        "completed-desc" => ts.sort_by(|a, b| deref_str(&b.completed).cmp(deref_str(&a.completed))),
        "completed-asc" => ts.sort_by(|a, b| deref_str(&a.completed).cmp(deref_str(&b.completed))),
        _ => ts.sort_by(|a, b| b.created.cmp(&a.created)), // created-desc default
    }
}

fn deref_str(s: &Option<String>) -> &str {
    s.as_deref().unwrap_or("")
}

pub(crate) fn has_all_tags(have: &[String], want: &[String]) -> bool {
    want.iter().all(|w| have.contains(w))
}

/// Go's page(): negative offset behaves like 0, offset past the end yields
/// empty, limit <= 0 means unlimited.
pub(crate) fn page<T>(mut s: Vec<T>, offset: i64, limit: i64) -> Vec<T> {
    let off = offset.clamp(0, s.len() as i64) as usize;
    s.drain(..off);
    if limit > 0 && (limit as usize) < s.len() {
        s.truncate(limit as usize);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::new_project;

    #[test]
    fn test_create_get_todo() {
        let p = new_project();
        let td = p
            .create_todo("Rotate tokens", "body", "", Vec::new())
            .unwrap();
        assert_eq!(td.status, "open");
        assert_eq!(td.priority, "medium");
        let got = p.get_todo(&td.id).unwrap();
        assert_eq!(got.title, "Rotate tokens");
    }

    #[test]
    fn test_status_normalized_and_rejected() {
        assert_eq!(normalize_status(" Open ").unwrap(), "open");
        assert_eq!(normalize_status("In-Progress").unwrap(), "in_progress");
        assert_eq!(normalize_status("in progress").unwrap(), "in_progress");
        assert_eq!(normalize_status("COMPLETED").unwrap(), "completed");
        // "closed" was the ghost-maker: not canonical, so it's rejected outright.
        assert!(normalize_status("closed").is_err());
        assert!(normalize_status("done").is_err());

        assert_eq!(normalize_priority(" HIGH ").unwrap(), "high");
        assert!(normalize_priority("urgent").is_err());

        let p = new_project();
        let td = p.create_todo("x", "", "", Vec::new()).unwrap();
        let mut u = TodoUpdate::default();
        u.status = Some("closed".into());
        assert!(p.update_todo(&td.id, u).is_err());
        // The rejected update left the stored status untouched.
        assert_eq!(p.get_todo(&td.id).unwrap().status, "open");
        // create rejects a bogus priority outright
        assert!(p.create_todo("y", "", "urgent", Vec::new()).is_err());
    }

    #[test]
    fn test_complete_and_incomplete() {
        let p = new_project();
        let td = p.create_todo("x", "", "high", Vec::new()).unwrap();
        let done = p.complete_todo(&td.id, true).unwrap();
        assert_eq!(done.status, "completed");
        assert!(done.completed.is_some());
        let back = p.incomplete_todo(&td.id, false).unwrap();
        assert_eq!(back.status, "open");
        assert!(back.completed.is_none());
    }

    #[test]
    fn test_delete_todo() {
        let p = new_project();
        let td = p.create_todo("x", "", "", Vec::new()).unwrap();
        p.delete_todo(&td.id).unwrap();
        assert!(matches!(p.get_todo(&td.id), Err(Error::NotFound)));
    }

    #[test]
    fn test_list_filter_and_sort() {
        let p = new_project();
        let a = p.create_todo("a", "", "low", vec!["x".into()]).unwrap();
        let b = p.create_todo("b", "", "high", vec!["y".into()]).unwrap();
        p.complete_todo(&a.id, false).unwrap();

        let open = p
            .list_todos(TodoFilter {
                status: "open".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, b.id);

        let tagged = p
            .list_todos(TodoFilter {
                tags: vec!["x".into()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].id, a.id);

        let by_prio = p
            .list_todos(TodoFilter {
                sort: "priority".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_prio[0].priority, "high");
    }

    #[test]
    fn test_update_replaces_tags_add_preserves() {
        let p = new_project();
        let td = p
            .create_todo("x", "", "", vec!["a".into(), "b".into()])
            .unwrap();
        let up = p
            .update_todo(
                &td.id,
                TodoUpdate {
                    tags: Some(vec!["c".into()]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(up.tags, vec!["c"], "update should replace tags");
        let add = p.add_todo_tag(&td.id, "d").unwrap();
        assert_eq!(add.tags.len(), 2, "add-tag should preserve: {:?}", add.tags);
    }

    #[test]
    fn test_blockers_and_is_blocked() {
        let p = new_project();
        let dep = p.create_todo("dep", "", "", Vec::new()).unwrap();
        let main = p.create_todo("main", "", "", Vec::new()).unwrap();
        p.set_blockers(&main.id, vec![dep.id.clone()]).unwrap();

        let blocked = p
            .list_todos(TodoFilter {
                is_blocked: Some(true),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].id, main.id);

        p.complete_todo(&dep.id, false).unwrap();
        let still = p
            .list_todos(TodoFilter {
                is_blocked: Some(true),
                ..Default::default()
            })
            .unwrap();
        assert!(still.is_empty(), "completing dep should unblock: {still:?}");
    }

    #[test]
    fn test_lock_ownership() {
        let p = new_project();
        let td = p.create_todo("x", "", "", Vec::new()).unwrap();
        p.lock_todo(&td.id, "claude", 42).unwrap();
        assert!(
            p.unlock_todo(&td.id, "other").is_err(),
            "non-owner unlock should fail"
        );
        p.unlock_todo(&td.id, "claude").unwrap();
    }

    #[test]
    fn test_list_negative_offset_no_panic() {
        let p = new_project();
        p.create_todo("a", "", "", Vec::new()).unwrap();
        let got = p
            .list_todos(TodoFilter {
                offset: -1,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(got.len(), 1, "negative offset should behave like 0");
    }

    #[test]
    fn test_delete_todo_cascades_blockers() {
        let p = new_project();
        let dep = p.create_todo("dep", "", "", Vec::new()).unwrap();
        let main = p.create_todo("main", "", "", Vec::new()).unwrap();
        p.set_blockers(&main.id, vec![dep.id.clone()]).unwrap();

        p.delete_todo(&dep.id).unwrap();
        let got = p.get_todo(&main.id).unwrap();
        assert!(
            !got.blockers.contains(&dep.id),
            "expected dep removed from main's blockers, got {:?}",
            got.blockers
        );
        assert!(
            !p.is_blocked(&got),
            "main should no longer be blocked after dep was deleted"
        );
    }

    #[test]
    fn test_update_todo_expected_updated_mismatch_fails() {
        let p = new_project();
        let td = p.create_todo("t", "body", "", Vec::new()).unwrap();
        let stale = format!("{}-stale", td.updated); // guaranteed mismatch
        let err = p
            .update_todo(
                &td.id,
                TodoUpdate {
                    title: Some("clobber".into()),
                    expected_updated: Some(stale),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(
            matches!(err, Error::ConcurrentEdit),
            "err = {err}, want ConcurrentEdit"
        );
        let got = p.get_todo(&td.id).unwrap();
        assert_eq!(got.title, "t", "title should be unchanged");
    }

    #[test]
    fn test_update_todo_expected_updated_match_succeeds() {
        let p = new_project();
        let td = p.create_todo("t", "body", "", Vec::new()).unwrap();
        let got = p
            .update_todo(
                &td.id,
                TodoUpdate {
                    title: Some("new".into()),
                    expected_updated: Some(td.updated.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(got.title, "new");
    }

    #[test]
    fn test_update_todo_none_expected_updated_skips_guard() {
        let p = new_project();
        let td = p.create_todo("t", "body", "", Vec::new()).unwrap();
        p.update_todo(
            &td.id,
            TodoUpdate {
                title: Some("unguarded".into()),
                ..Default::default()
            },
        )
        .expect("None guard must never fail");
    }

    // Migration guard: a todos.json exactly as the Go binary marshals it
    // (MarshalIndent, no omitempty, explicit nulls) must round-trip through
    // the Rust store.
    #[test]
    fn test_reads_go_written_todos_json() {
        let p = new_project();
        let fixture = r#"{
  "revision": 3,
  "todos": [
    {
      "id": "t_go1",
      "title": "From Go",
      "body": "left by the Go binary",
      "status": "open",
      "priority": "medium",
      "tags": ["a", "b"],
      "blockers": [],
      "lock": null,
      "created": "2026-01-02T03:04:05Z",
      "updated": "2026-01-02T03:04:05Z",
      "completed": null
    },
    {
      "id": "t_go2",
      "title": "Locked done",
      "body": "",
      "status": "completed",
      "priority": "high",
      "tags": [],
      "blockers": ["t_go1"],
      "lock": {
        "owner": "claude",
        "pid": 42,
        "at": "2026-01-02T03:04:06Z"
      },
      "created": "2026-01-02T03:04:05Z",
      "updated": "2026-01-02T03:04:07Z",
      "completed": "2026-01-02T03:04:07Z"
    }
  ]
}"#;
        std::fs::write(p.todos_path(), fixture).unwrap();
        let t1 = p.get_todo("t_go1").unwrap();
        assert_eq!(t1.tags, vec!["a", "b"]);
        assert!(t1.lock.is_none());
        let t2 = p.get_todo("t_go2").unwrap();
        assert_eq!(t2.lock.as_ref().map(|l| l.owner.as_str()), Some("claude"));
        assert_eq!(t2.completed.as_deref(), Some("2026-01-02T03:04:07Z"));
        assert_eq!(t2.blockers, vec!["t_go1"]);
        // Attribution fields absent in the Go file default to empty (not backfilled).
        assert_eq!(t1.created_by, "");
        assert_eq!(t1.updated_by, "");
        // And a Rust-side mutation on top of the Go file must not lose data.
        let up = p.add_todo_tag("t_go2", "ported").unwrap();
        assert!(up.tags.contains(&"ported".to_string()));
        assert!(p.get_todo("t_go1").is_ok());
    }

    #[test]
    fn test_todo_attribution_stamped() {
        let mut tp = new_project();
        tp.p.actor = "claude".to_string();
        let td = tp.create_todo("x", "", "", Vec::new()).unwrap();
        assert_eq!(td.created_by, "claude");
        assert_eq!(td.updated_by, "claude");
        // A different actor mutating stamps updated_by but leaves created_by.
        tp.p.actor = "jason".to_string();
        let up = tp.add_todo_tag(&td.id, "t").unwrap();
        assert_eq!(up.created_by, "claude", "creator must not change on edit");
        assert_eq!(
            up.updated_by, "jason",
            "editor should be the mutating actor"
        );
    }

    #[test]
    fn test_format_rfc3339_known_values() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(951_782_400), "2000-02-29T00:00:00Z"); // leap day
        assert_eq!(format_rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn test_unsynced_todo_serializes_without_github_key() {
        let t = Todo::default();
        let js = serde_json::to_string(&t).unwrap();
        assert!(
            !js.contains("github"),
            "unsynced todo must omit github: {js}"
        );
    }

    #[test]
    fn test_todo_with_github_link_roundtrips() {
        let mut t = Todo::default();
        t.github = Some(GithubLink {
            repo: "owner/name".into(),
            number: 42,
            last_pushed: "2026-07-12T00:00:00Z".into(),
            last_comment_pull: "2026-07-12T00:00:00Z".into(),
            paused: false,
        });
        let js = serde_json::to_string(&t).unwrap();
        assert!(
            js.contains(r#""github""#) && js.contains(r#""number":42"#),
            "{js}"
        );
        let back: Todo = serde_json::from_str(&js).unwrap();
        assert_eq!(back.github, t.github);
    }

    #[test]
    fn test_set_github_toggle() {
        // new_project() creates a git repo but no origin remote.
        let p = new_project();
        let td = p.create_todo("sync me", "", "", Vec::new()).unwrap();

        // No origin remote yet -> linking fails, leaves the todo unlinked.
        assert!(p.set_github(&td.id, true).is_err());
        assert!(p.get_todo(&td.id).unwrap().github.is_none());

        // Add an origin, then link.
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&p.path)
            .args(["remote", "add", "origin", "git@github.com:owner/name.git"])
            .output()
            .unwrap();
        assert!(out.status.success());

        let linked = p.set_github(&td.id, true).unwrap();
        let link = linked.github.unwrap();
        assert_eq!(link.repo, "owner/name");
        assert_eq!(link.number, 0);
        assert!(!link.paused);

        // Untick pauses but keeps repo/number.
        let paused = p.set_github(&td.id, false).unwrap();
        let link = paused.github.unwrap();
        assert!(link.paused);
        assert_eq!(link.repo, "owner/name");

        // Re-tick clears paused, same link (no new issue requested).
        let retick = p.set_github(&td.id, true).unwrap();
        assert!(!retick.github.unwrap().paused);
    }

    #[test]
    fn test_rfc3339_epoch_roundtrip() {
        // Known value: 2026-07-10T12:00:00Z
        let e = epoch_from_rfc3339("2026-07-10T12:00:00Z").unwrap();
        assert_eq!(format_rfc3339(e), "2026-07-10T12:00:00Z");
        // now() round-trips through the epoch parser
        let n = now();
        assert_eq!(format_rfc3339(epoch_from_rfc3339(&n).unwrap()), n);
        // malformed inputs are rejected, not panicked on
        assert_eq!(epoch_from_rfc3339("2026-07-10"), None);
        assert_eq!(epoch_from_rfc3339(""), None);
        assert_eq!(epoch_from_rfc3339("2026-07-10T12:00:00+00:00"), None);
        assert_eq!(epoch_from_rfc3339("2026-07-10T12X00Y00Z"), None);
        assert_eq!(epoch_from_rfc3339("2026-07-10T12:00:00X"), None);
    }

    #[test]
    fn test_update_github_link_does_not_bump_updated() {
        let p = new_project();
        let td = p.create_todo("x", "", "", Vec::new()).unwrap();
        let before = td.updated.clone();
        p.update_github_link(
            &td.id,
            GithubLink {
                repo: "o/n".into(),
                number: 7,
                last_pushed: "t".into(),
                last_comment_pull: String::new(),
                paused: false,
            },
        )
        .unwrap();
        let got = p.get_todo(&td.id).unwrap();
        assert_eq!(got.github.unwrap().number, 7);
        assert_eq!(got.updated, before, "sync writeback must not bump updated");
    }

    #[test]
    fn test_update_github_link_preserves_paused() {
        // A concurrent un-tick (paused=true) must survive sync's own end-of-pass
        // writeback, which carries paused=false from the pre-pass link clone.
        let p = new_project();
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&p.path)
            .args(["remote", "add", "origin", "git@github.com:o/n.git"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let td = p.create_todo("x", "", "", Vec::new()).unwrap();
        p.set_github(&td.id, true).unwrap(); // link, paused=false
        p.set_github(&td.id, false).unwrap(); // user un-ticks -> paused=true stored
        // Sync writeback arrives with a stale paused=false clone.
        p.update_github_link(
            &td.id,
            GithubLink {
                repo: "o/n".into(),
                number: 7,
                last_pushed: "t".into(),
                last_comment_pull: String::new(),
                paused: false,
            },
        )
        .unwrap();
        let link = p.get_todo(&td.id).unwrap().github.unwrap();
        assert_eq!(link.number, 7, "sync fields still applied");
        assert!(link.paused, "concurrent un-tick must be preserved");
    }
}
