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
use super::todos::now;

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
}
