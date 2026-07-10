// Port of internal/store/scratchpads.go. Storage format is a minimal `---`
// frontmatter block + markdown body, hand-parsed exactly like the Go parser
// (no YAML crate) so pads written by the Go binary keep reading.
//
// Every mutating op takes an expected revision; passing -1 skips the guard and
// is intended ONLY for append/append_section (documented optional guard). The
// adapters enforce "required" for everything else.
//
// Deliberate quirks preserved from Go (see CLAUDE.md): save/load_from_file are
// not path-sandboxed, and heading parsing is NOT fenced-code-block aware (a
// `# comment` inside a code fence reads as a heading).
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::errors::{Error, Result};
use super::ids::new_id;
use super::lock::{atomic_write, with_file_lock};
use super::project::Project;
use super::todos::{has_all_tags, now, page};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Scratchpad {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "tags")]
    pub tags: Vec<String>,
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "revision")]
    pub revision: i64,
    #[serde(rename = "created")]
    pub created: String,
    #[serde(rename = "updated")]
    pub updated: String,
    /// Attribution: who created / last mutated this. Empty on pads written
    /// before attribution shipped — never backfilled, so render() omits the
    /// line when empty (keeps old pads byte-identical on rewrite).
    #[serde(rename = "created_by", default)]
    pub created_by: String,
    #[serde(rename = "updated_by", default)]
    pub updated_by: String,
    #[serde(rename = "content")]
    pub content: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EditTarget {
    /// "section" or "line_range" (Go field name: Type).
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "section_heading")]
    pub section_heading: String,
    #[serde(rename = "offset")]
    pub offset: i64,
    #[serde(rename = "limit")]
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Match {
    #[serde(rename = "line")]
    pub line: i64,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "context")]
    pub context: String,
}

/// Parses a scratchpad file: a minimal `---` frontmatter block followed by the
/// markdown body. ponytail: hand-rolled parser for a fixed 7-field header,
/// avoids a YAML dependency; widen only if the header schema grows. Unlike the
/// Go version this can't fail (Go's error return was always nil).
fn parse_pad(b: &[u8]) -> Scratchpad {
    let text = String::from_utf8_lossy(b);
    let mut s = Scratchpad {
        status: "active".to_string(),
        ..Default::default()
    };
    let Some(rest) = text.strip_prefix("---\n") else {
        s.content = text.into_owned();
        return s;
    };
    let Some(end) = rest.find("\n---\n") else {
        s.content = text.into_owned();
        return s;
    };
    let header = &rest[..end];
    s.content = rest[end + 5..].to_string();
    for line in header.split('\n') {
        let Some((k, v)) = line.split_once(": ") else {
            continue;
        };
        match k {
            "id" => s.id = v.to_string(),
            "title" => s.title = v.to_string(),
            "status" => s.status = v.to_string(),
            "created" => s.created = v.to_string(),
            "updated" => s.updated = v.to_string(),
            "created_by" => s.created_by = v.to_string(),
            "updated_by" => s.updated_by = v.to_string(),
            "revision" => s.revision = v.parse().unwrap_or(0), // Go: Atoi error -> 0
            "tags" => s.tags = parse_tag_list(v),
            _ => {}
        }
    }
    s
}

fn parse_tag_list(v: &str) -> Vec<String> {
    let v = v.trim_matches(|c| c == '[' || c == ']').trim();
    if v.is_empty() {
        return Vec::new();
    }
    v.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

impl Scratchpad {
    fn render(&self) -> String {
        // created_by/updated_by are omitted when empty so a pad written before
        // attribution round-trips byte-for-byte (see the Go migration test).
        let attr = |k: &str, v: &str| {
            if v.is_empty() {
                String::new()
            } else {
                format!("{k}: {v}\n")
            }
        };
        format!(
            "---\nid: {}\ntitle: {}\ntags: [{}]\nstatus: {}\nrevision: {}\ncreated: {}\nupdated: {}\n{}{}---\n{}",
            self.id,
            self.title,
            self.tags.join(", "),
            self.status,
            self.revision,
            self.created,
            self.updated,
            attr("created_by", &self.created_by),
            attr("updated_by", &self.updated_by),
            self.content
        )
    }
}

fn first_h1(content: &str) -> &str {
    for line in content.split('\n') {
        if let Some(rest) = line.strip_prefix("# ") {
            return rest.trim();
        }
    }
    ""
}

/// Returns Some((text, depth)) when line is `#...# text` (spaces-indented ok,
/// space after the hashes required); Go returned ("", 0) for non-headings.
fn heading_text(line: &str) -> Option<(String, usize)> {
    let t = line.trim_start_matches(' ');
    let b = t.as_bytes();
    let mut n = 0;
    while n < b.len() && b[n] == b'#' {
        n += 1;
    }
    if n == 0 || n >= b.len() || b[n] != b' ' {
        return None;
    }
    Some((t[n + 1..].trim().to_string(), n))
}

fn norm_heading(h: &str) -> String {
    h.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn headings_of(content: &str) -> String {
    content
        .split('\n')
        .filter(|line| heading_text(line).is_some())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Returns the body under a heading (case-insensitive, whitespace-normalized),
/// up to the next heading of the same-or-shallower depth.
fn section_of(content: &str, heading: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    let want = norm_heading(heading);
    let mut found = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some((h, d)) = heading_text(line)
            && !h.is_empty()
            && norm_heading(&h) == want
        {
            found = Some((i, d));
            break;
        }
    }
    let Some((start, depth)) = found else {
        return String::new();
    };
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate().skip(start) {
        if i > start
            && let Some((_, d)) = heading_text(line)
            && d <= depth
        {
            break;
        }
        out.push(*line);
    }
    out.join("\n")
}

fn line_window(text: &str, offset: i64, limit: i64) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    page(lines, offset, limit).join("\n")
}

fn first_line(s: &str) -> &str {
    s.split('\n').next().unwrap_or(s)
}

impl Project {
    /// The on-disk path of a scratchpad, for external editors (TUI `$EDITOR`).
    pub fn pad_path(&self, id: &str) -> PathBuf {
        self.scratch_dir().join(format!("{id}.md"))
    }

    fn read_pad(&self, id: &str) -> Result<Scratchpad> {
        match std::fs::read(self.pad_path(id)) {
            Ok(b) => Ok(parse_pad(&b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::NotFound),
            Err(e) => Err(e.into()),
        }
    }

    /// Loads, checks revision (exp_rev < 0 skips), applies f, bumps revision, saves.
    fn mutate_pad(
        &self,
        id: &str,
        exp_rev: i64,
        f: impl FnOnce(&mut Scratchpad) -> Result<()>,
    ) -> Result<Scratchpad> {
        let path = self.pad_path(id);
        with_file_lock(&path, || {
            let mut s = self.read_pad(id)?;
            if exp_rev >= 0 && s.revision != exp_rev {
                return Err(Error::RevisionMismatch);
            }
            f(&mut s)?;
            s.revision += 1;
            s.updated = now();
            s.updated_by = self.actor.clone();
            atomic_write(&path, s.render().as_bytes())?;
            Ok(s)
        })
    }

    pub fn create_scratchpad(
        &self,
        name: &str,
        content: &str,
        tags: Vec<String>,
    ) -> Result<Scratchpad> {
        let name = if name.is_empty() {
            first_h1(content)
        } else {
            name
        };
        let s = Scratchpad {
            id: new_id("s_"),
            title: name.to_string(),
            tags,
            status: "active".to_string(),
            revision: 1,
            created: now(),
            updated: now(),
            created_by: self.actor.clone(),
            updated_by: self.actor.clone(),
            content: content.to_string(),
        };
        let path = self.pad_path(&s.id);
        with_file_lock(&path, || atomic_write(&path, s.render().as_bytes()))?;
        Ok(s)
    }

    /// mode: "content" | "headings" | "section" | anything else = full.
    /// Returns the pad plus the extracted text view.
    pub fn read_scratchpad(
        &self,
        id: &str,
        mode: &str,
        section_heading: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Scratchpad, String)> {
        let s = self.read_pad(id)?;
        let mut text = match mode {
            "headings" => headings_of(&s.content),
            "section" => section_of(&s.content, section_heading),
            _ => s.content.clone(), // "content" and default "full" are identical
        };
        if offset > 0 || limit > 0 {
            text = line_window(&text, offset, limit);
        }
        Ok((s, text))
    }

    /// Listing omits pad bodies (content is cleared on each result).
    pub fn list_scratchpads(
        &self,
        tags: &[String],
        query: &str,
        include_archived: bool,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Scratchpad>> {
        // Go's os.ReadDir returns name-sorted entries; sort to match so paging
        // over equal Updated stamps stays deterministic.
        let mut names: Vec<String> = std::fs::read_dir(self.scratch_dir())?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".md"))
            .collect();
        names.sort();
        let mut out = Vec::new();
        for name in &names {
            let id = name.strip_suffix(".md").unwrap_or(name);
            let Ok(mut s) = self.read_pad(id) else {
                continue;
            };
            if s.status == "archived" && !include_archived {
                continue;
            }
            if !tags.is_empty() && !has_all_tags(&s.tags, tags) {
                continue;
            }
            if !query.is_empty() {
                let hay = format!("{} {}", s.title, s.content).to_lowercase();
                if !hay.contains(&query.to_lowercase()) {
                    continue;
                }
            }
            s.content = String::new(); // list omits body
            out.push(s);
        }
        out.sort_by(|a, b| b.updated.cmp(&a.updated));
        Ok(page(out, offset, limit))
    }

    pub fn update_scratchpad(
        &self,
        id: &str,
        exp_rev: i64,
        name: Option<&str>,
        content: Option<&str>,
        tags: Option<Vec<String>>,
    ) -> Result<Scratchpad> {
        self.mutate_pad(id, exp_rev, |s| {
            if let Some(v) = name {
                s.title = v.to_string();
            }
            if let Some(v) = content {
                s.content = v.to_string();
            }
            if let Some(v) = tags {
                s.tags = v;
            }
            Ok(())
        })
    }

    pub fn rename_scratchpad(&self, id: &str, name: &str, exp_rev: i64) -> Result<Scratchpad> {
        self.mutate_pad(id, exp_rev, |s| {
            s.title = name.to_string();
            Ok(())
        })
    }

    fn set_pad_status(&self, id: &str, status: &str, exp_rev: i64) -> Result<Scratchpad> {
        self.mutate_pad(id, exp_rev, |s| {
            s.status = status.to_string();
            Ok(())
        })
    }

    pub fn archive_scratchpad(&self, id: &str, exp_rev: i64) -> Result<Scratchpad> {
        self.set_pad_status(id, "archived", exp_rev)
    }

    pub fn unarchive_scratchpad(&self, id: &str, exp_rev: i64) -> Result<Scratchpad> {
        self.set_pad_status(id, "active", exp_rev)
    }

    pub fn delete_scratchpad(&self, id: &str, exp_rev: i64) -> Result<()> {
        let path = self.pad_path(id);
        with_file_lock(&path, || {
            let s = self.read_pad(id)?;
            if exp_rev >= 0 && s.revision != exp_rev {
                return Err(Error::RevisionMismatch);
            }
            std::fs::remove_file(&path)?;
            Ok(())
        })
    }

    /// exp_rev -1 skips the guard here by design (append is the documented
    /// optional-guard op).
    pub fn append_scratchpad(
        &self,
        id: &str,
        content: &str,
        exp_rev: i64,
        newline: bool,
    ) -> Result<Scratchpad> {
        self.mutate_pad(id, exp_rev, |s| {
            if newline && !s.content.is_empty() && !s.content.ends_with('\n') {
                s.content.push('\n');
            }
            s.content.push_str(content);
            Ok(())
        })
    }

    pub fn append_section(
        &self,
        id: &str,
        heading: &str,
        content: &str,
        exp_rev: i64,
    ) -> Result<Scratchpad> {
        self.mutate_pad(id, exp_rev, |s| {
            let sec = section_of(&s.content, heading);
            if sec.is_empty() {
                return Err(Error::NotFound);
            }
            let new_sec = format!("{}\n{}", sec.trim_end_matches('\n'), content);
            s.content = s.content.replacen(&sec, &new_sec, 1);
            Ok(())
        })
    }

    pub fn edit_scratchpad(
        &self,
        id: &str,
        target: EditTarget,
        content: &str,
        exp_rev: i64,
    ) -> Result<Scratchpad> {
        self.mutate_pad(id, exp_rev, |s| {
            match target.kind.as_str() {
                "section" => {
                    let sec = section_of(&s.content, &target.section_heading);
                    if sec.is_empty() {
                        return Err(Error::NotFound);
                    }
                    // Preserve the heading unless the replacement starts with one.
                    let repl = if heading_text(content).is_some() {
                        content.to_string()
                    } else {
                        format!("{}\n{}", first_line(&sec), content)
                    };
                    s.content = s.content.replacen(&sec, &repl, 1);
                }
                "line_range" => {
                    let lines: Vec<&str> = s.content.split('\n').collect();
                    // Bounds check ordered to be overflow-proof: `limit > n - offset`
                    // instead of `offset + limit > n` (see the huge-offset test).
                    let n = lines.len() as i64;
                    if target.offset < 0
                        || target.limit < 0
                        || target.offset > n
                        || target.limit > n - target.offset
                    {
                        return Err(Error::Other("line range out of bounds".to_string()));
                    }
                    let off = target.offset as usize;
                    let end = (target.offset + target.limit) as usize;
                    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
                    out.extend(&lines[..off]);
                    out.extend(content.split('\n'));
                    out.extend(&lines[end..]);
                    s.content = out.join("\n");
                }
                _ => {
                    return Err(Error::Other(format!(
                        "unknown edit target {:?}",
                        target.kind
                    )));
                }
            }
            Ok(())
        })
    }

    pub fn find_in_scratchpad(
        &self,
        id: &str,
        query: &str,
        scope: &str,
        case_sensitive: bool,
        context_lines: i64,
    ) -> Result<Vec<Match>> {
        let s = self.read_pad(id)?;
        let needle = if case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        let lines: Vec<&str> = s.content.split('\n').collect();
        let mut out = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if scope == "headings" && heading_text(line).is_none() {
                continue;
            }
            let hay = if case_sensitive {
                (*line).to_string()
            } else {
                line.to_lowercase()
            };
            if !hay.contains(&needle) {
                continue;
            }
            let len = lines.len() as i64;
            let lo = (i as i64 - context_lines).clamp(0, len) as usize;
            let hi = (i as i64 + context_lines + 1).clamp(lo as i64, len) as usize;
            out.push(Match {
                line: i as i64,
                text: (*line).to_string(),
                context: lines[lo..hi].join("\n"),
            });
        }
        Ok(out)
    }

    /// Returns (last `lines` lines, total line count). lines <= 0 returns only
    /// the count. Trailing newlines don't count as a line (Go TrimRight).
    pub fn tail_scratchpad(&self, id: &str, lines: i64) -> Result<(String, i64)> {
        let s = self.read_pad(id)?;
        let trimmed = s.content.trim_end_matches('\n');
        let all: Vec<&str> = trimmed.split('\n').collect();
        let total = all.len() as i64;
        if lines <= 0 {
            return Ok((String::new(), total));
        }
        let start = if lines < total {
            (total - lines) as usize
        } else {
            0
        };
        Ok((all[start..].join("\n"), total))
    }

    pub fn clear_scratchpad(&self, id: &str, exp_rev: i64) -> Result<Scratchpad> {
        self.mutate_pad(id, exp_rev, |s| {
            s.content = String::new();
            Ok(())
        })
    }

    pub fn scratchpad_tags(&self) -> Result<Vec<String>> {
        let pads = self.list_scratchpads(&[], "", true, 0, 0)?;
        let set: std::collections::BTreeSet<&String> = pads.iter().flat_map(|s| &s.tags).collect();
        Ok(set.into_iter().cloned().collect())
    }

    /// Writes clean markdown (H1 title + body, no frontmatter). Relative paths
    /// resolve against the project root. NOT path-sandboxed — deliberate; this
    /// is a local single-user tool.
    pub fn save_scratchpad_to_file(&self, id: &str, path: &str) -> Result<()> {
        let s = self.read_pad(id)?;
        let body = if first_h1(&s.content).is_empty() {
            format!("# {}\n\n{}", s.title, s.content)
        } else {
            s.content
        };
        std::fs::write(self.resolve_external(path), body)?;
        Ok(())
    }

    /// Creates a scratchpad from a markdown file (first H1 -> title, falling
    /// back to the file name minus ".md"). NOT path-sandboxed — deliberate.
    pub fn load_scratchpad_from_file(&self, path: &str) -> Result<Scratchpad> {
        let full = self.resolve_external(path);
        let b = std::fs::read(&full)?;
        let content = String::from_utf8_lossy(&b).into_owned();
        let mut name = first_h1(&content).to_string();
        if name.is_empty() {
            let base = full
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            name = base.strip_suffix(".md").unwrap_or(&base).to_string();
        }
        self.create_scratchpad(&name, &content, Vec::new())
    }

    fn resolve_external(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.path.join(p)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::{TempDir, new_project};
    use crate::store::{Error, resolve_project_in};

    #[test]
    fn test_create_read_scratchpad() {
        let p = new_project();
        let s = p
            .create_scratchpad("Plan", "# Plan\n\nstep one\n", vec!["x".into()])
            .unwrap();
        assert_eq!(s.revision, 1);
        assert_eq!(s.status, "active");
        let (got, _) = p.read_scratchpad(&s.id, "full", "", 0, 0).unwrap();
        assert!(got.content.contains("step one"));
    }

    #[test]
    fn test_read_headings_and_section() {
        let p = new_project();
        let s = p
            .create_scratchpad("Doc", "# Doc\n\n## A\naaa\n\n## B\nbbb\n", Vec::new())
            .unwrap();
        let (_, headings) = p.read_scratchpad(&s.id, "headings", "", 0, 0).unwrap();
        assert!(
            headings.contains("## A") && headings.contains("## B"),
            "headings: {headings:?}"
        );
        let (_, sec) = p.read_scratchpad(&s.id, "section", "A", 0, 0).unwrap();
        assert!(
            sec.contains("aaa") && !sec.contains("bbb"),
            "section: {sec:?}"
        );
    }

    #[test]
    fn test_update_revision_guard() {
        let p = new_project();
        let s = p.create_scratchpad("x", "# x\nbody\n", Vec::new()).unwrap();
        let err = p
            .update_scratchpad(&s.id, 99, None, Some("# x\nnew\n"), None)
            .unwrap_err();
        assert!(
            matches!(err, Error::RevisionMismatch),
            "want RevisionMismatch, got {err}"
        );
        let up = p
            .update_scratchpad(&s.id, 1, None, Some("# x\nnew\n"), None)
            .unwrap();
        assert_eq!(up.revision, 2);
    }

    #[test]
    fn test_archive_hides_from_list() {
        let p = new_project();
        let s = p.create_scratchpad("x", "# x\n", Vec::new()).unwrap();
        p.archive_scratchpad(&s.id, 1).unwrap();
        let active = p.list_scratchpads(&[], "", false, 0, 0).unwrap();
        assert!(active.is_empty(), "archived should be hidden: {active:?}");
        let all = p.list_scratchpads(&[], "", true, 0, 0).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_append_and_section() {
        let p = new_project();
        let s = p
            .create_scratchpad("x", "# x\n\n## Notes\nfirst\n", Vec::new())
            .unwrap();
        let ap = p.append_scratchpad(&s.id, "tail", 1, true).unwrap();
        assert!(
            ap.content.trim_end_matches('\n').ends_with("tail"),
            "append: {:?}",
            ap.content
        );
        let sec = p
            .append_section(&s.id, "Notes", "second", ap.revision)
            .unwrap();
        let idx = sec.content.find("first").expect("first missing");
        assert!(
            sec.content[idx..].contains("second"),
            "append-section: {:?}",
            sec.content
        );
    }

    #[test]
    fn test_edit_line_range() {
        let p = new_project();
        let s = p.create_scratchpad("x", "a\nb\nc\n", Vec::new()).unwrap();
        let up = p
            .edit_scratchpad(
                &s.id,
                EditTarget {
                    kind: "line_range".into(),
                    offset: 1,
                    limit: 1,
                    ..Default::default()
                },
                "B",
                1,
            )
            .unwrap();
        assert_eq!(up.content, "a\nB\nc\n");
    }

    #[test]
    fn test_find_and_tail() {
        let p = new_project();
        let s = p
            .create_scratchpad("x", "# x\nalpha\nbeta\nalpha\n", Vec::new())
            .unwrap();
        let m = p
            .find_in_scratchpad(&s.id, "alpha", "content", false, 0)
            .unwrap();
        assert_eq!(m.len(), 2, "find: {m:?}");
        let (tail, n) = p.tail_scratchpad(&s.id, 2).unwrap();
        assert!(n >= 2 && tail.contains("alpha"), "tail: {tail:?} n={n}");
    }

    #[test]
    fn test_save_load_file() {
        let p = new_project();
        let s = p
            .create_scratchpad("Title", "# Title\nbody\n", Vec::new())
            .unwrap();
        let dir = TempDir::new();
        let dst = dir.path().join("out.md");
        let dst = dst.to_string_lossy();
        p.save_scratchpad_to_file(&s.id, &dst).unwrap();
        let b = std::fs::read_to_string(dst.as_ref()).unwrap();
        assert!(
            b.starts_with("# Title"),
            "saved file should lead with H1: {b:?}"
        );
        let loaded = p.load_scratchpad_from_file(&dst).unwrap();
        assert_eq!(loaded.title, "Title");
    }

    #[test]
    fn test_tail_negative_lines_no_panic() {
        let p = new_project();
        let s = p
            .create_scratchpad("x", "a\nb\nc\nd\n", Vec::new())
            .unwrap();
        let (tail, total) = p.tail_scratchpad(&s.id, -1).unwrap();
        assert_eq!(tail, "");
        assert_eq!(total, 4);
    }

    #[test]
    fn test_edit_negative_limit_errors() {
        let p = new_project();
        let s = p.create_scratchpad("x", "a\nb\nc\n", Vec::new()).unwrap();
        let t = |offset, limit| EditTarget {
            kind: "line_range".into(),
            offset,
            limit,
            ..Default::default()
        };
        assert!(p.edit_scratchpad(&s.id, t(2, -1), "X", s.revision).is_err());
        let (got, _) = p.read_scratchpad(&s.id, "full", "", 0, 0).unwrap();
        assert_eq!(got.content, s.content, "pad should not be modified");
        assert!(p.edit_scratchpad(&s.id, t(0, -5), "X", s.revision).is_err());
    }

    #[test]
    fn test_clear_scratchpad() {
        let p = new_project();
        let s = p
            .create_scratchpad("x", "some content\nmore\n", Vec::new())
            .unwrap();
        let cleared = p.clear_scratchpad(&s.id, s.revision).unwrap();
        assert_eq!(cleared.content, "");
        assert_eq!(cleared.revision, s.revision + 1);
    }

    #[test]
    fn test_scratchpad_tags() {
        let p = new_project();
        p.create_scratchpad("a", "# a\n", vec!["foo".into(), "bar".into()])
            .unwrap();
        p.create_scratchpad("b", "# b\n", vec!["bar".into(), "baz".into()])
            .unwrap();
        let tags = p.scratchpad_tags().unwrap();
        assert_eq!(tags, vec!["bar", "baz", "foo"]);
    }

    #[test]
    fn test_edit_line_range_huge_offset_no_panic() {
        let p = new_project();
        let s = p.create_scratchpad("x", "a\nb\nc\n", Vec::new()).unwrap();
        // offset+limit must not be allowed to integer-overflow past the bounds
        // check: i64::MAX+1 wraps to a large-negative end, which would defeat
        // a naive "end > len(lines)" guard and panic on the slice below it.
        let target = EditTarget {
            kind: "line_range".into(),
            offset: i64::MAX,
            limit: 1,
            ..Default::default()
        };
        assert!(
            p.edit_scratchpad(&s.id, target, "X", s.revision).is_err(),
            "expected error for huge offset (overflow bypass)"
        );
    }

    #[test]
    fn test_headings_of_requires_space_after_hash() {
        let p = new_project();
        let s = p
            .create_scratchpad("x", "#nospace\n## Real\nbody\n", Vec::new())
            .unwrap();
        let (_, headings) = p.read_scratchpad(&s.id, "headings", "", 0, 0).unwrap();
        assert!(
            !headings.contains("#nospace"),
            "headings should exclude {headings:?}"
        );
        assert!(
            headings.contains("## Real"),
            "headings should include \"## Real\": {headings:?}"
        );
    }

    #[test]
    fn test_save_load_relative_path() {
        let p = new_project();
        let s = p
            .create_scratchpad("Handoff", "# Handoff\nbody\n", Vec::new())
            .unwrap();
        p.save_scratchpad_to_file(&s.id, "handoff.md").unwrap();
        let want_path = p.path.join("handoff.md");
        assert!(want_path.exists(), "expected file at {want_path:?}");
        let loaded = p.load_scratchpad_from_file("handoff.md").unwrap();
        assert_eq!(loaded.title, "Handoff");
        assert!(loaded.content.contains("body"));
    }

    #[test]
    fn test_pad_path_matches_file() {
        let root = TempDir::new();
        let dir = TempDir::new(); // non-git dir, like the Go test's bare t.TempDir()
        let p = resolve_project_in(root.path(), Some(&dir.path().to_string_lossy())).unwrap();
        let s = p.create_scratchpad("x", "hi", Vec::new()).unwrap();
        assert!(p.pad_path(&s.id).exists(), "pad_path not on disk");
    }

    // Migration guard: a pad byte-for-byte as the Go render() wrote it must
    // parse to the same fields, and the Rust render() must reproduce it.
    #[test]
    fn test_parses_go_written_frontmatter() {
        let go_pad = "---\nid: s_go1\ntitle: My Pad\ntags: [a, b]\nstatus: active\nrevision: 3\ncreated: 2026-01-02T03:04:05Z\nupdated: 2026-01-02T03:04:06Z\n---\n# My Pad\n\nbody\n";
        let s = parse_pad(go_pad.as_bytes());
        assert_eq!(s.id, "s_go1");
        assert_eq!(s.title, "My Pad");
        assert_eq!(s.tags, vec!["a", "b"]);
        assert_eq!(s.status, "active");
        assert_eq!(s.revision, 3);
        assert_eq!(s.created, "2026-01-02T03:04:05Z");
        assert_eq!(s.updated, "2026-01-02T03:04:06Z");
        assert_eq!(s.content, "# My Pad\n\nbody\n");
        assert_eq!(s.render(), go_pad, "render must round-trip Go's format");
    }

    #[test]
    fn test_scratchpad_attribution_roundtrip() {
        let mut tp = new_project();
        tp.p.actor = "claude".to_string();
        let s = tp
            .create_scratchpad("x", "# x\nbody\n", Vec::new())
            .unwrap();
        assert_eq!(s.created_by, "claude");
        assert_eq!(s.updated_by, "claude");
        // Persisted to frontmatter and read back.
        let (got, _) = tp.read_scratchpad(&s.id, "full", "", 0, 0).unwrap();
        assert_eq!(got.created_by, "claude");
        // A different actor's mutation stamps updated_by, leaves created_by.
        tp.p.actor = "jason".to_string();
        let up = tp
            .append_scratchpad(&s.id, "more", s.revision, true)
            .unwrap();
        assert_eq!(up.created_by, "claude");
        assert_eq!(up.updated_by, "jason");
    }

    // Omitempty: a pad with no actor renders WITHOUT the created_by/updated_by
    // lines, so pads written before attribution round-trip byte-for-byte.
    #[test]
    fn test_render_omits_empty_attribution() {
        let s = parse_pad(
            b"---\nid: s_x\ntitle: x\ntags: []\nstatus: active\nrevision: 1\ncreated: 2026-01-01T00:00:00Z\nupdated: 2026-01-01T00:00:00Z\n---\nbody",
        );
        assert_eq!(s.created_by, "");
        let out = s.render();
        assert!(
            !out.contains("created_by") && !out.contains("updated_by"),
            "empty attribution must not render lines: {out:?}"
        );
    }

    // Go parsePad quirks: no frontmatter -> whole text is content with status
    // "active"; empty tag list "[]" -> empty vec.
    #[test]
    fn test_parse_pad_no_frontmatter_and_empty_tags() {
        let s = parse_pad(b"just text\n");
        assert_eq!(s.content, "just text\n");
        assert_eq!(s.status, "active");
        assert!(s.tags.is_empty());

        let s = parse_pad(b"---\nid: s_x\ntags: []\n---\nbody");
        assert_eq!(s.id, "s_x");
        assert!(s.tags.is_empty());
        assert_eq!(s.content, "body");
    }
}
