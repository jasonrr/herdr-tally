//! GitHub issue sync — decision engine, executor, and the `gh` boundary.
//! All logic lives here (store is the single source of truth). CLI/MCP/TUI drive.

/// Parse a git remote URL to "owner/name". Handles scp-style (`git@host:o/n.git`),
/// ssh (`ssh://git@host/o/n.git`), and https (`https://host/o/n[.git]`). None on
/// anything that doesn't yield two path segments.
// Unused outside tests until a later task wires the sync engine to it.
pub(crate) fn parse_repo(url: &str) -> Option<String> {
    let u = url.trim();
    let path = if let Some(rest) = u.strip_prefix("git@") {
        // git@github.com:owner/name.git
        rest.split_once(':').map(|(_, p)| p)?
    } else if let Some((_scheme, rest)) = u.split_once("://") {
        // https://github.com/owner/name  |  ssh://git@github.com/owner/name.git
        rest.split_once('/').map(|(_, p)| p)?
    } else {
        return None;
    };
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut segs = path.trim_matches('/').split('/');
    let owner = segs.next().filter(|s| !s.is_empty())?;
    let name = segs.next().filter(|s| !s.is_empty())?;
    Some(format!("{owner}/{name}"))
}

use std::collections::HashSet;

use super::comments::Comment;
use super::todos::{GithubLink, Todo};

#[derive(Debug, Clone, Default, PartialEq)]
pub enum IssueState {
    #[default]
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GhComment {
    pub id: i64,
    pub author: String,
    pub created: String,
    pub body: String,
}

/// One tick's view of a GitHub issue.
#[derive(Debug, Clone, Default)]
pub struct IssueSnapshot {
    pub state: IssueState,
    pub closed_by: String, // GH login of the closer, "" if open/unknown
    pub comments: Vec<GhComment>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    EditIssue,
    CloseIssue,
    ReopenIssue,
    CompleteTodo { by: String },
    ReopenTodo { by: String },
    ImportComment(GhComment),
    PushComment { comment_id: String },
}

/// Pure reconcile decision for an already-created issue (number != 0). Ordering
/// mirrors the spec: state first (tally wins if it changed since last push, else
/// GH wins), then pull comments, then push comments.
pub(crate) fn plan_actions(
    todo: &Todo,
    link: &GithubLink,
    todo_comments: &[Comment],
    snap: &IssueSnapshot,
) -> Vec<Action> {
    let mut acts = Vec::new();
    let completed = todo.status == "completed";
    let issue_closed = snap.state == IssueState::Closed;
    // Empty last_pushed sorts before any RFC3339 stamp, so a never-pushed todo is
    // always "newer" here — exactly the create-then-first-edit path.
    let tally_newer = todo.updated.as_str() > link.last_pushed.as_str();

    if tally_newer {
        acts.push(Action::EditIssue);
        if completed && !issue_closed {
            acts.push(Action::CloseIssue);
        } else if !completed && issue_closed {
            acts.push(Action::ReopenIssue);
        }
    } else if issue_closed && !completed {
        acts.push(Action::CompleteTodo {
            by: snap.closed_by.clone(),
        });
    } else if !issue_closed && completed {
        acts.push(Action::ReopenTodo {
            by: snap.closed_by.clone(),
        });
    }

    // Pull: GH comments at/after last_comment_pull whose id we don't already hold.
    // The id set is the real echo/dup guard (imported comments carry their GH id,
    // and comments we pushed carry theirs), so an inclusive time bound is safe.
    let known: HashSet<i64> = todo_comments
        .iter()
        .map(|c| c.github_comment_id)
        .filter(|&i| i != 0)
        .collect();
    for gc in &snap.comments {
        if gc.created.as_str() >= link.last_comment_pull.as_str() && !known.contains(&gc.id) {
            acts.push(Action::ImportComment(gc.clone()));
        }
    }

    // Push: local human/agent notes never sent to GH. Skip events (auto-logged)
    // and anything pulled from GH (author `gh:*`).
    for c in todo_comments {
        if c.kind == "note" && c.github_comment_id == 0 && !c.author.starts_with("gh:") {
            acts.push(Action::PushComment {
                comment_id: c.id.clone(),
            });
        }
    }
    acts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Comment, GithubLink, Todo};

    fn linked(number: i64, last_pushed: &str, last_comment_pull: &str) -> GithubLink {
        GithubLink {
            repo: "o/n".into(),
            number,
            last_pushed: last_pushed.into(),
            last_comment_pull: last_comment_pull.into(),
            paused: false,
        }
    }
    fn todo_at(status: &str, updated: &str) -> Todo {
        let mut t = Todo::default();
        t.id = "t_1".into();
        t.title = "T".into();
        t.status = status.into();
        t.updated = updated.into();
        t
    }
    fn note(id: &str, author: &str, gh_id: i64) -> Comment {
        let mut c = Comment::default();
        c.id = id.into();
        c.author = author.into();
        c.kind = "note".into();
        c.github_comment_id = gh_id;
        c.text = "body".into();
        c
    }

    #[test]
    fn test_plan_push_title_body_when_tally_newer() {
        let t = todo_at("open", "2026-07-12T02:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "");
        let snap = IssueSnapshot::default(); // Open, no comments
        let acts = plan_actions(&t, &link, &[], &snap);
        assert_eq!(acts, vec![Action::EditIssue]);
    }

    #[test]
    fn test_plan_push_close_when_completed_and_issue_open() {
        let t = todo_at("completed", "2026-07-12T02:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "");
        let snap = IssueSnapshot::default();
        let acts = plan_actions(&t, &link, &[], &snap);
        assert_eq!(acts, vec![Action::EditIssue, Action::CloseIssue]);
    }

    #[test]
    fn test_plan_pull_close_when_gh_wins() {
        // tally NOT newer than last_pushed -> GH state wins.
        let t = todo_at("open", "2026-07-12T01:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", ""); // updated == last_pushed => not newer
        let snap = IssueSnapshot {
            state: IssueState::Closed,
            closed_by: "octocat".into(),
            comments: vec![],
        };
        let acts = plan_actions(&t, &link, &[], &snap);
        assert_eq!(
            acts,
            vec![Action::CompleteTodo {
                by: "octocat".into()
            }]
        );
    }

    #[test]
    fn test_plan_import_new_comment_but_not_echo() {
        let t = todo_at("open", "2026-07-12T01:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "2026-07-12T00:00:00Z");
        // gc 100 is new; gc 200 is one we already pushed (present as a local id) -> skip.
        let snap = IssueSnapshot {
            state: IssueState::Open,
            closed_by: String::new(),
            comments: vec![
                GhComment {
                    id: 100,
                    author: "octocat".into(),
                    created: "2026-07-12T01:30:00Z".into(),
                    body: "new".into(),
                },
                GhComment {
                    id: 200,
                    author: "me".into(),
                    created: "2026-07-12T01:31:00Z".into(),
                    body: "echo".into(),
                },
            ],
        };
        let local = vec![note("c_local", "you", 200)]; // github_comment_id 200 => already known
        let acts = plan_actions(&t, &link, &local, &snap);
        assert_eq!(acts, vec![Action::ImportComment(snap.comments[0].clone())]);
    }

    #[test]
    fn test_plan_push_local_note_but_not_pulled_or_event() {
        let t = todo_at("open", "2026-07-12T01:00:00Z");
        let link = linked(5, "2026-07-12T01:00:00Z", "2026-07-12T00:00:00Z");
        let snap = IssueSnapshot::default();
        let mut event = note("c_ev", "you", 0);
        event.kind = "event".into(); // auto-logged status event: must NOT push
        let local = vec![
            note("c_push", "you", 0),            // local note, never on GH -> push
            note("c_pulled", "gh:octocat", 100), // pulled from GH -> never push back
            event,
        ];
        let acts = plan_actions(&t, &link, &local, &snap);
        assert_eq!(
            acts,
            vec![Action::PushComment {
                comment_id: "c_push".into()
            }]
        );
    }

    #[test]
    fn test_parse_repo_forms() {
        assert_eq!(
            parse_repo("git@github.com:owner/name.git").as_deref(),
            Some("owner/name")
        );
        assert_eq!(
            parse_repo("https://github.com/owner/name.git").as_deref(),
            Some("owner/name")
        );
        assert_eq!(
            parse_repo("https://github.com/owner/name").as_deref(),
            Some("owner/name")
        );
        assert_eq!(
            parse_repo("ssh://git@github.com/owner/name.git").as_deref(),
            Some("owner/name")
        );
        assert_eq!(
            parse_repo("  https://github.com/owner/name.git\n").as_deref(),
            Some("owner/name")
        );
        assert_eq!(parse_repo("not-a-url"), None);
        assert_eq!(parse_repo(""), None);
    }
}
