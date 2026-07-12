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

#[cfg(test)]
mod tests {
    use super::*;

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
