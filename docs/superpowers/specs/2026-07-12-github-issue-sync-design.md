# GitHub Issue Sync — Design

2026-07-12

## Summary

Opt-in, per-todo sync between tally todos and GitHub issues. Title and body
push one way (tally → GH, tally authoritative). Comments flow both ways.
Complete/close syncs both ways. Nothing else (priority, tags, blockers) ever
leaves tally. Transport is the `gh` CLI via `process::Command`. The reconcile
engine lives in the store crate and is driven by the long-running TUI on a
timer, plus a one-shot `tally sync` CLI subcommand for headless use. MCP gets
no new machinery — agents mutate todos as today and the loop picks changes up.

## Data model

`Todo` gains one optional field; absent for unsynced todos so existing stores
load unchanged:

```rust
#[serde(rename = "github", default, skip_serializing_if = "Option::is_none")]
pub github: Option<GithubLink>,

pub struct GithubLink {
    pub repo: String,              // "owner/name", captured from origin at link time
    pub number: i64,               // 0 = sync requested, issue not yet created
    pub last_pushed: String,       // RFC3339; push when todo.updated > last_pushed
    pub last_comment_pull: String, // RFC3339; pull GH comments created after this
    pub paused: bool,              // true = unticked; link kept, sync skipped
}
```

Comments gain echo-prevention fields (both serde-default, absent on existing
comments):

- `github_comment_id: i64` (0 = none). On a comment pulled from GH, the GH
  comment id — never re-imported, never pushed back. On a tally comment that
  has been pushed, the id of the GH comment it became.
- Pulled comments set `author` to `gh:<github-login>`.

## Reconcile pass

`sync_project(store) -> SyncReport`, in a new `src/store/sync.rs`. Per synced
todo (link present and not paused), in order, best-effort — a network/gh failure on one todo is recorded in
the report and skipped; the next tick retries:

1. **Create**: `number == 0` → `gh issue create --repo <repo> --title --body`,
   parse the issue number from output, store it, set `last_pushed = now`.
2. **Push title/body/state**: `todo.updated > last_pushed` →
   `gh issue edit` title/body. If todo is completed and issue open →
   `gh issue close`; if todo reopened and issue closed → `gh issue reopen`.
   Set `last_pushed = now`.
3. **Pull close**: issue closed on GH while todo not completed → complete the
   todo with attribution `gh:<closer-login>`. (GH-side reopen likewise reopens
   the todo.)
4. **Pull comments**: fetch issue comments created after `last_comment_pull`
   (`gh api repos/<repo>/issues/<n>/comments`); import any whose id isn't
   already present, author `gh:<login>`. Advance `last_comment_pull`.
5. **Push comments**: tally comments on this todo with `github_comment_id == 0`
   and author not `gh:*` → `gh api` create comment, store resulting id.

Title/body conflicts don't exist by construction: tally is authoritative, GH
edits are overwritten on the next push. Known lossy case, accepted: a body
rewritten on GH is clobbered.

State conflicts (completed in tally AND closed on GH between ticks) converge
trivially — both sides agree.

Issue deleted or repo access lost → the todo keeps its link, the report notes
the error each tick. No automatic unlink.

## Drivers

- **TUI**: run `sync_project` on a timer (60s) and immediately after a local
  mutation of a synced todo. Runs on a background thread; results applied via
  the normal store API (flock makes this safe). One-line sync status in the
  footer (last sync time / error count).
- **CLI**: `tally sync` runs one reconcile pass and prints the report.
- **MCP**: nothing. No new tools; the 38 names stay frozen.

## Opt-in surface ("the box tick")

- **TUI**: keybind on the selected todo toggles sync. Ticking on sets
  `github = Some(GithubLink { repo: <origin>, number: 0, .. })`; issue is
  created on the next reconcile.
- **CLI**: `todos update <id> --github on|off`.
- **MCP**: one new optional string param on the existing `todo_update` tool:
  `github: "on" | "off"`, absent or empty string = unchanged (consistent with
  the existing empty-string quirk).
- **Untick** (`off`): clears sync (stops pushing/pulling) but keeps
  `repo`/`number` so re-ticking relinks the same issue instead of creating a
  duplicate. Never deletes or closes anything GH-side. Concretely: `off` sets a
  `paused: bool` on the link rather than dropping it.

Repo is resolved at link time: `git remote get-url origin` from the project
root, parsed to `owner/name`. No origin → link fails with a clear error.

## Failure posture

- `gh` missing or unauthed (`gh auth status` fails) → sync disabled, one-line
  notice in the TUI footer / `tally sync` output. Store operations never fail
  because of GitHub.
- All GH calls have a timeout (subprocess kill after ~30s) so a hung network
  can't wedge the TUI's sync thread.

## Testing

Logic lives in the store, so tests do too:

- Unit-test the reconcile decision function (given todo + issue snapshot →
  list of actions) with a faked GH boundary — the `gh` invocation sits behind
  a small trait so tests never shell out.
- Round-trip serde tests: todos/comments with and without the new fields;
  golden check that an unsynced todo serializes byte-identical to today.
- Echo-prevention: pulled comment is not pushed back; pushed comment is not
  re-imported.
- One live smoke test behind `#[ignore]` for manual runs against a scratch
  repo.

## Out of scope

- Priority/tags/blockers/labels/assignees/milestones — never sync.
- GH-side title/body edits flowing back.
- Webhooks, daemons, or any always-on process beyond the TUI.
- Multi-repo or non-origin targets (the link captures origin; override can
  come later if ever needed).
