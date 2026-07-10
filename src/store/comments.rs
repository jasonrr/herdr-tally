// Central per-project comment store. One comments.json holds notes and
// auto-logged events for every target — todos (t_…), scratchpads (s_…), and
// plan files (keyed by rel_path). Mirrors the todos store: serde structs, a
// revision-bumping atomic write, and a per-file flock. No revision guard — a
// comment never mutates a target body, so the flock is the only ceiling.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::errors::{Error, Result};
use super::ids::new_id;
use super::lock::{atomic_write, with_file_lock};
use super::project::Project;
use super::todos::{epoch_from_rfc3339, format_rfc3339, now};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Comment {
    #[serde(rename = "id")]
    pub id: String,
    /// t_… | s_… | plan rel_path. Type is inferred from the prefix by adapters.
    #[serde(rename = "target")]
    pub target: String,
    /// Anchored heading text; empty = item-level (the default).
    #[serde(rename = "section")]
    pub section: String,
    #[serde(rename = "author")]
    pub author: String,
    /// RFC3339 like todos, so the TUI's humanize_since works unchanged.
    #[serde(rename = "created")]
    pub created: String,
    /// "note" (human/agent) | "event" (auto-logged).
    #[serde(rename = "kind")]
    pub kind: String,
    #[serde(rename = "text")]
    pub text: String,
}

// On-disk shape is just {"comments":[…]} — no revision counter: comment ops are
// explicitly not revision-guarded, so nothing would read it.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct CommentsFile {
    #[serde(rename = "comments")]
    comments: Vec<Comment>,
}

/// One row of the per-target comment view: note count + the most recent
/// note's text (snippet) and timestamp (for ordering).
#[derive(Debug, Clone, Serialize)]
pub struct CommentSummary {
    pub target: String,
    pub count: usize,
    pub latest: String,
    pub created: String,
}

/// Normalize a target key: drop a leading "./" so "./docs/x.md" and "docs/x.md"
/// are the same target. (Todo/pad ids are unaffected.)
fn norm_target(t: &str) -> &str {
    t.strip_prefix("./").unwrap_or(t)
}

impl Project {
    fn load_comments(&self) -> Result<CommentsFile> {
        let b = match std::fs::read(self.comments_path()) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CommentsFile::default());
            }
            Err(e) => return Err(e.into()),
        };
        Ok(serde_json::from_slice(&b)?)
    }

    fn save_comments(&self, cf: &CommentsFile) -> Result<()> {
        let b = serde_json::to_vec_pretty(cf)?;
        atomic_write(&self.comments_path(), &b)
    }

    fn mutate_comments(&self, f: impl FnOnce(&mut CommentsFile) -> Result<()>) -> Result<()> {
        with_file_lock(&self.comments_path(), || {
            let mut cf = self.load_comments()?;
            f(&mut cf)?;
            self.save_comments(&cf)
        })
    }

    fn add_comment_kind(
        &self,
        target: &str,
        section: &str,
        kind: &str,
        text: &str,
    ) -> Result<Comment> {
        let c = Comment {
            id: new_id("c_"),
            target: norm_target(target).to_string(),
            section: section.to_string(),
            author: self.actor.clone(),
            created: now(),
            kind: kind.to_string(),
            text: text.to_string(),
        };
        let cp = c.clone();
        self.mutate_comments(|cf| {
            cf.comments.push(cp);
            Ok(())
        })?;
        Ok(c)
    }

    /// Add a human/agent note. section "" = item-level (the default).
    pub fn add_comment(&self, target: &str, section: &str, text: &str) -> Result<Comment> {
        self.add_comment_kind(target, section, "note", text)
    }

    /// Append an auto-logged event to a target's timeline.
    pub(crate) fn add_comment_event(&self, target: &str, text: &str) -> Result<Comment> {
        self.add_comment_kind(target, "", "event", text)
    }

    /// Every comment in the store, file order (chronological). The primitive
    /// the recent/summary queries share.
    pub(crate) fn all_comments(&self) -> Result<Vec<Comment>> {
        Ok(self.load_comments()?.comments)
    }

    /// Comments at or after `cutoff` (an RFC3339 string; "" = all time),
    /// optionally filtered by author, notes-only unless include_events.
    /// Newest-first.
    pub fn recent_comments(
        &self,
        cutoff: &str,
        author: Option<&str>,
        include_events: bool,
    ) -> Result<Vec<Comment>> {
        // Reverse file order first (most-recently-appended first) so that when
        // two comments land in the same second — `created` is second-precision —
        // the stable sort below breaks the tie by append order instead of
        // silently falling back to oldest-first.
        let mut v: Vec<Comment> = self
            .all_comments()?
            .into_iter()
            .rev()
            .filter(|c| c.created.as_str() >= cutoff)
            .filter(|c| include_events || c.kind == "note")
            .filter(|c| author.is_none_or(|a| c.author == a))
            .collect();
        // Sort by `created` descending (RFC3339/Z sorts lexically = chronologically).
        // Not just a reverse of file order: appends are chronological in normal
        // operation, but nothing stops a caller from seeding an out-of-order
        // backdated entry (as the tests do), so sort on the actual timestamp.
        v.sort_by(|a, b| b.created.cmp(&a.created));
        Ok(v)
    }

    /// Window ("30m"/"2h"/"1d") -> RFC3339 cutoff string via the store clock.
    pub fn recency_cutoff(&self, window: &str) -> String {
        duration_cutoff(&now(), window)
    }

    /// All comments for a target, oldest first (creation order — appends are
    /// serialized under the file flock, so file order is chronological).
    pub fn list_comments(&self, target: &str) -> Result<Vec<Comment>> {
        let target = norm_target(target);
        Ok(self
            .load_comments()?
            .comments
            .into_iter()
            .filter(|c| c.target == target)
            .collect())
    }

    pub fn delete_comment(&self, comment_id: &str) -> Result<()> {
        self.mutate_comments(|cf| {
            let before = cf.comments.len();
            cf.comments.retain(|c| c.id != comment_id);
            if cf.comments.len() == before {
                return Err(Error::NotFound);
            }
            Ok(())
        })
    }

    /// Cascade helper: drop every comment on a deleted target.
    pub(crate) fn delete_comments_for_target(&self, target: &str) -> Result<()> {
        let target = norm_target(target);
        // Don't create/rewrite comments.json when the target had no comments.
        if !self
            .load_comments()?
            .comments
            .iter()
            .any(|c| c.target == target)
        {
            return Ok(());
        }
        self.mutate_comments(|cf| {
            cf.comments.retain(|c| c.target != target);
            Ok(())
        })
    }

    /// target -> note count over the whole file, for TUI list badges. Events are
    /// excluded so a toggled-status todo doesn't accrue a phantom badge.
    pub fn comment_counts(&self) -> Result<HashMap<String, usize>> {
        let mut m = HashMap::new();
        for c in self.load_comments()?.comments {
            if c.kind == "note" {
                *m.entry(c.target).or_insert(0) += 1;
            }
        }
        Ok(m)
    }

    /// One row per target that has notes: count + most-recent note snippet.
    /// Notes only (events don't accrue badges). Newest-commented target first.
    pub fn comment_summaries(&self) -> Result<Vec<CommentSummary>> {
        // target -> (count, latest_text, latest_created, latest_idx). File
        // order is chronological, so ">=" keeps the last note seen as the
        // latest; `created` has only second resolution, so the file-order
        // index breaks ties deterministically instead of falling back to
        // HashMap iteration order.
        let mut by: HashMap<String, (usize, String, String, usize)> = HashMap::new();
        for (i, c) in self.all_comments()?.into_iter().enumerate() {
            if c.kind != "note" {
                continue;
            }
            let e = by
                .entry(c.target)
                .or_insert((0, String::new(), String::new(), 0));
            e.0 += 1;
            if c.created >= e.2 {
                e.1 = c.text;
                e.2 = c.created;
                e.3 = i;
            }
        }
        let mut out: Vec<(CommentSummary, usize)> = by
            .into_iter()
            .map(|(target, (count, latest, created, idx))| {
                (
                    CommentSummary {
                        target,
                        count,
                        latest,
                        created,
                    },
                    idx,
                )
            })
            .collect();
        // newest target first; ties broken by file order (later = newer)
        out.sort_by(|a, b| b.0.created.cmp(&a.0.created).then(b.1.cmp(&a.1)));
        Ok(out.into_iter().map(|(s, _)| s).collect())
    }

    /// Human label for a comment target: t_… -> "☐/☑ <todo title>",
    /// s_… -> "• <pad title>", anything else (plan rel_path or an
    /// unresolvable id) -> the raw target string.
    pub fn resolve_target_label(&self, target: &str) -> String {
        if target.starts_with("t_") {
            if let Ok(t) = self.get_todo(target) {
                let glyph = if t.status == "completed" {
                    "☑"
                } else {
                    "☐"
                };
                return format!("{glyph} {}", t.title);
            }
        } else if target.starts_with("s_") {
            if let Ok(s) = self.read_pad(target) {
                return format!("• {}", s.title);
            }
        }
        target.to_string()
    }
}

/// N{s,m,h,d} -> seconds. None on any other shape.
fn parse_window(w: &str) -> Option<u64> {
    let w = w.trim();
    let unit = w.chars().last()?;
    let num = &w[..w.len() - unit.len_utf8()];
    let n: u64 = num.parse().ok()?;
    let mult = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3_600,
        'd' => 86_400,
        _ => return None,
    };
    n.checked_mul(mult)
}

/// `now` minus `window`, as an RFC3339 cutoff string. Malformed window (or
/// unparseable now) -> "" so a typo degrades to "all time", never an error.
fn duration_cutoff(now: &str, window: &str) -> String {
    match (parse_window(window), epoch_from_rfc3339(now)) {
        (Some(w), Some(base)) => format_rfc3339(base.saturating_sub(w)),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::new_project;

    #[test]
    fn test_add_list_delete_roundtrip() {
        let mut tp = new_project();
        tp.p.actor = "jason".to_string();
        let c = tp.add_comment("t_abc", "", "hold off").unwrap();
        assert!(c.id.starts_with("c_"));
        assert_eq!(c.author, "jason");
        assert_eq!(c.kind, "note");
        let list = tp.list_comments("t_abc").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].text, "hold off");
        // other targets are isolated
        assert!(tp.list_comments("s_zzz").unwrap().is_empty());
        tp.delete_comment(&c.id).unwrap();
        assert!(tp.list_comments("t_abc").unwrap().is_empty());
    }

    #[test]
    fn test_delete_missing_is_notfound() {
        let tp = new_project();
        assert!(matches!(tp.delete_comment("c_nope"), Err(Error::NotFound)));
    }

    #[test]
    fn test_section_cascade_and_counts() {
        let tp = new_project();
        tp.add_comment("s_pad", "Phase 1", "spike first").unwrap();
        tp.add_comment("s_pad", "", "whole-pad note").unwrap();
        tp.add_comment("t_x", "", "todo note").unwrap();
        let counts = tp.comment_counts().unwrap();
        assert_eq!(counts.get("s_pad"), Some(&2));
        assert_eq!(counts.get("t_x"), Some(&1));
        tp.delete_comments_for_target("s_pad").unwrap();
        assert!(tp.list_comments("s_pad").unwrap().is_empty());
        assert_eq!(tp.list_comments("t_x").unwrap().len(), 1);
    }

    #[test]
    fn test_todo_delete_cascades_comments() {
        let tp = new_project();
        let t = tp.create_todo("x", "", "", Vec::new()).unwrap();
        tp.add_comment(&t.id, "", "a note").unwrap();
        assert_eq!(tp.list_comments(&t.id).unwrap().len(), 1);
        tp.delete_todo(&t.id).unwrap();
        assert!(tp.list_comments(&t.id).unwrap().is_empty());
    }

    #[test]
    fn test_pad_delete_cascades_only_on_success() {
        let tp = new_project();
        let s = tp
            .create_scratchpad("plan", "# H1\nbody", Vec::new())
            .unwrap();
        tp.add_comment(&s.id, "H1", "anchored").unwrap();
        // A wrong expected-revision must fail AND leave comments intact.
        assert!(tp.delete_scratchpad(&s.id, 999).is_err());
        assert_eq!(tp.list_comments(&s.id).unwrap().len(), 1);
        // The real delete (rev 1, or -1 to skip the guard) cascades.
        tp.delete_scratchpad(&s.id, s.revision).unwrap();
        assert!(tp.list_comments(&s.id).unwrap().is_empty());
    }

    #[test]
    fn test_status_transitions_log_events() {
        use crate::store::TodoUpdate;
        use crate::store::testutil::TestProject;
        let tp = new_project();
        let t = tp.create_todo("x", "", "", Vec::new()).unwrap();
        let events = |tp: &TestProject| -> Vec<String> {
            tp.list_comments(&t.id)
                .unwrap()
                .into_iter()
                .filter(|c| c.kind == "event")
                .map(|c| c.text)
                .collect()
        };
        // create is not a transition
        assert!(events(&tp).is_empty());
        // a tag edit changes nothing about status → no event
        tp.add_todo_tag(&t.id, "a").unwrap();
        assert!(events(&tp).is_empty());
        // status change → one event mentioning the new status
        let u = TodoUpdate {
            status: Some("in progress".to_string()),
            ..TodoUpdate::default()
        };
        tp.update_todo(&t.id, u).unwrap();
        let e = events(&tp);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("in progress"), "got {:?}", e);
        // completion → "marked done"
        tp.complete_todo(&t.id, false).unwrap();
        assert!(events(&tp).iter().any(|x| x == "marked done"));
        // a FAILED update (stale expected_updated → ConcurrentEdit) emits nothing
        let before = events(&tp).len();
        let stale = TodoUpdate {
            status: Some("open".to_string()),
            expected_updated: Some("1999-01-01T00:00:00Z".to_string()),
            ..TodoUpdate::default()
        };
        assert!(tp.update_todo(&t.id, stale).is_err());
        assert_eq!(
            events(&tp).len(),
            before,
            "failed update must not log an event"
        );
    }

    #[test]
    fn test_duration_cutoff() {
        let now = "2026-07-10T12:00:00Z";
        assert_eq!(duration_cutoff(now, "2h"), "2026-07-10T10:00:00Z");
        assert_eq!(duration_cutoff(now, "30m"), "2026-07-10T11:30:00Z");
        assert_eq!(duration_cutoff(now, "1d"), "2026-07-09T12:00:00Z");
        assert_eq!(duration_cutoff(now, "90s"), "2026-07-10T11:58:30Z");
        // malformed / empty -> "all time" (empty string), never a panic
        assert_eq!(duration_cutoff(now, "xyz"), "");
        assert_eq!(duration_cutoff(now, ""), "");
        assert_eq!(duration_cutoff(now, "2"), "");
        assert_eq!(duration_cutoff(now, "5µ"), "");
        assert_eq!(duration_cutoff(now, "€"), "");
    }

    #[test]
    fn test_recent_comments_cutoff_author_events() {
        let mut tp = new_project();
        tp.p.actor = "jason".to_string();
        tp.add_comment("t_a", "", "fresh note").unwrap(); // created = now()
        // Seed a backdated note + an event directly (tests module can reach the
        // private load/save on the store).
        let mut cf = tp.load_comments().unwrap();
        cf.comments.push(Comment {
            id: "c_old".into(),
            target: "t_a".into(),
            section: String::new(),
            author: "ana".into(),
            created: "2000-01-01T00:00:00Z".into(),
            kind: "note".into(),
            text: "old note".into(),
        });
        cf.comments.push(Comment {
            id: "c_ev".into(),
            target: "t_a".into(),
            section: String::new(),
            author: "jason".into(),
            created: "2026-07-10T12:00:00Z".into(),
            kind: "event".into(),
            text: "marked done".into(),
        });
        tp.save_comments(&cf).unwrap();

        // cutoff excludes the 2000 note, keeps the fresh note; events off by default
        let cutoff = "2020-01-01T00:00:00Z";
        let r = tp.recent_comments(cutoff, None, false).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].text, "fresh note");

        // include_events widens; boundary is inclusive (event created == cutoff)
        let r = tp
            .recent_comments("2026-07-10T12:00:00Z", None, true)
            .unwrap();
        assert!(r.iter().any(|c| c.text == "marked done"));

        // author filter
        let r = tp.recent_comments("", Some("ana"), false).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].text, "old note");

        // newest-first: empty cutoff (all time), notes only -> [fresh, old]
        let r = tp.recent_comments("", None, false).unwrap();
        assert_eq!(r.first().unwrap().text, "fresh note");
        assert_eq!(r.last().unwrap().text, "old note");
    }

    #[test]
    fn test_comment_summaries_and_labels() {
        let tp = new_project();
        // Two targets: a real todo and a real pad; comment on each.
        let t = tp.create_todo("Fix auth", "", "", Vec::new()).unwrap();
        let s = tp
            .create_scratchpad("Design notes", "# H\nbody", Vec::new())
            .unwrap();
        tp.add_comment(&t.id, "", "first").unwrap();
        tp.add_comment(&s.id, "", "hold off").unwrap();
        tp.add_comment(&t.id, "", "second").unwrap(); // latest note on the todo
        tp.add_comment_event(&t.id, "marked done").unwrap(); // event: ignored by summaries

        let sums = tp.comment_summaries().unwrap();
        assert_eq!(sums.len(), 2);
        // newest-commented target first == the todo (its 2nd note is most recent)
        assert_eq!(sums[0].target, t.id);
        assert_eq!(sums[0].count, 2); // notes only, event excluded
        assert_eq!(sums[0].latest, "second");

        // labels
        assert_eq!(tp.resolve_target_label(&t.id), format!("☐ Fix auth"));
        assert_eq!(tp.resolve_target_label(&s.id), format!("• Design notes"));
        assert_eq!(tp.resolve_target_label("docs/plan.md"), "docs/plan.md");
        assert_eq!(tp.resolve_target_label("t_gone"), "t_gone"); // unresolvable
    }
}
