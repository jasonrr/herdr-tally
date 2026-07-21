# Plan-paths Globs — Design

2026-07-21

## Summary

Turn the Plans tab's `plan-paths` config from a list of directory prefixes into
a **gitignore-in-reverse include list**. Each line is a gitignore-style glob;
a markdown file is surfaced in Plans if it matches an include pattern and isn't
re-excluded by a later `!` pattern. This lets a user pick *some but not all*
folders under the repo without enumerating every one (`docs/*-plans/`,
`**/design/*.md`, `!docs/archive/**`), which the current recursive-directory-walk
can't express.

Semantics and matching come from the `ignore` crate (ripgrep's). We do **not**
hand-roll glob matching — its `overrides` module is purpose-built for exactly
an include-list (whitelist) with `!` exclusions, i.e. reverse gitignore.

## Decisions (settled)

- **Use the `ignore` crate, not a hand-rolled matcher.** Familiar,
  upstream-maintained gitignore semantics; the only cost is binary size, which
  is acceptable. This is a deliberate, documented exception to the project's
  stdlib-only lean (see CLAUDE.md) — the semantics are too fiddly to own.
- **Full gitignore anchoring semantics.** A pattern containing a `/` is anchored
  to the repo root; a bare single-segment name (`notes`) matches at any depth.
  Because git anchors every slash-containing pattern, **every existing
  `plan-paths` entry keeps matching exactly what it did** (all defaults and all
  realistic user entries contain slashes). Bare names become *more* inclusive.
  Net effect on existing configs: strictly ≥ today's set, never less.

## The reverse-gitignore rule

A `plan-paths` line is a gitignore pattern, but its *sense is inverted*:

| Pattern            | gitignore means | here means |
|--------------------|-----------------|------------|
| `docs/plans`       | ignore it       | **include** it |
| `!docs/plans/wip`  | un-ignore it    | **exclude** it |
| (no match)         | keep (tracked)  | **exclude** (not surfaced) |

Last match wins (gitignore ordering). `#` comments and blank lines are ignored,
same as today. A file must also end in `.md` (case-insensitive) to surface —
the glob selects *which* files, the `.md` gate is kept as a final filter so a
broad `docs/**` doesn't pull in every non-markdown file.

### Examples

```
# everything under docs/, except the archive
docs/**
!docs/archive/**

# only design docs, wherever they live
**/design/*.md

# a subset of sibling folders without listing each
docs/*-plans/
```

## Implementation

One file changes: `src/plans.rs`. Public surface is unchanged —
`list(root, paths) -> Vec<Plan>`, `load_plan_paths*`, `save_plan_paths`,
`read`, `Plan` all keep their signatures. Only `list`'s internals are rewritten;
the recursive `walk()` and the `.md` suffix check move inside the new path.

Primary approach — `ignore::overrides` drives both selection *and* traversal:

```rust
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;

pub fn list(root: &Path, paths: &[String]) -> Vec<Plan> {
    let mut ob = OverrideBuilder::new(root);
    for line in paths {
        // Override globs already use whitelist (include) sense; `!foo` excludes.
        // Skip lines the builder rejects rather than aborting the whole list.
        let _ = ob.add(line);
    }
    let overrides = match ob.build() {
        Ok(o) => o,
        Err(_) => return Vec::new(), // malformed set => empty, not a panic
    };

    let mut out = Vec::new();
    let walk = WalkBuilder::new(root)
        .overrides(overrides) // prunes non-matching dirs during traversal
        .hidden(true)         // skip dotfiles/dirs
        .git_ignore(true)     // never descend .gitignore'd trees (node_modules, target)
        .parents(false)       // don't read .gitignore above the repo root
        .build();
    for entry in walk.filter_map(Result::ok) {
        let p = entry.path();
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        if !p.file_name().and_then(|n| n.to_str())
            .map(|n| n.to_lowercase().ends_with(".md")).unwrap_or(false) {
            continue;
        }
        // ...build Plan { rel_path, abs_path, heading, mod_time } as today...
        out.push(plan_from(p, root));
    }
    out.sort_by(|a, b| b.mod_time.cmp(&a.mod_time)); // most-recent-first, unchanged
    out
}
```

`WalkBuilder` rooted at the project confines traversal to the repo and honors
`.gitignore`, so the **whole-drive walk that commit `8ea8ad0` guarded against is
now structurally impossible** — no path can escape the root, and `.git`,
`node_modules`, `target` etc. are pruned for free. The lexical `is_under_root`
guard is therefore removed; malformed lines (absolute, `..`) simply match
nothing.

### Fallback if override anchoring surprises us

`ignore::overrides` is built on the same Gitignore engine, so anchoring should
match git exactly. If a spike shows a corner where whitelist-override anchoring
diverges from plain gitignore, fall back to: traverse with a bare `WalkBuilder`
and select each file via `ignore::gitignore::Gitignore` built from the same
lines, using `matched_path_or_any_parents(path, is_dir)` (so a matched *parent
directory* includes its files) and inverting `Match`: `Ignore => include`,
`Whitelist => exclude`, `None => exclude`. Same crate, ~15 more lines. Decide
during the spike; don't build both.

## Backward compatibility

- **Defaults unchanged.** No config file → the three default paths
  (`docs/superpowers/specs`, `docs/superpowers/plans`, `docs/solutions`) become
  three anchored include globs; each includes everything beneath it, identical
  to today.
- **Existing files unchanged in effect.** Every default and realistic entry
  contains a `/`, so it stays root-anchored. A hypothetical bare entry (`notes`)
  starts matching at any depth — more inclusive, which is the accepted direction.
- **One new subtraction path:** files inside a `.gitignore`'d directory no longer
  surface even if a pattern would match them. Plans are tracked docs in practice,
  so this is intended and documented, not a regression to preserve. No toggle
  until someone asks (YAGNI).

## Tests (extend `plans.rs` `#[cfg(test)]`)

Reuse the existing `TempDir` + `write_at` harness. New cases:

1. `glob_star_selects_subset` — `docs/*-plans/` matches `docs/a-plans/x.md` and
   `docs/b-plans/y.md` but not `docs/notes/z.md`.
2. `glob_double_star_any_depth` — `**/design/*.md` matches nested `design` dirs.
3. `negation_excludes` — `docs/**` + `!docs/archive/**` surfaces `docs/a.md`,
   drops `docs/archive/old.md`.
4. `bare_dir_still_recursive_and_backcompat` — `docs/plans` (no wildcard) still
   pulls every `.md` under it (parent-match), proving existing configs hold.
5. `respects_gitignore` — a `.md` under a `.gitignore`'d dir is not surfaced.
6. `non_md_filtered` — `docs/**` does not surface `docs/readme.txt`.
7. Keep `list_collects_configured_markdown_sorted_by_mtime` and
   `read_returns_contents` green as-is (regression guard on ordering/reading).

The old `list_skips_paths_outside_root` is retired — its concern (whole-drive
walk from `/` or `..`) is now handled by `WalkBuilder`'s root confinement;
replace it with a case asserting an absolute/`..` line matches nothing.

## Out of scope

- Per-directory nested `.plan-paths` files (gitignore's cascading). One flat
  config, as today.
- Case-insensitive glob matching (git is case-sensitive; the `.md` gate stays
  case-insensitive). Add `OverrideBuilder`/`GitignoreBuilder::case_insensitive`
  only on request.
- Any change to the CLI/MCP surface — Plans is a TUI-only, read-only tab.
- Editing the `plan-paths` UX in the TUI beyond what already exists (it's a
  free-text pad; glob lines are just text).

## Docs to update on implement

- `README.md` Plans blurb (the `plan-paths` paragraph) — describe glob/reverse-
  gitignore syntax with one example.
- `Cargo.toml` — add `ignore = "0.4"`; note in CLAUDE.md's crate-decisions list
  that `ignore` owns plan-path glob matching (the one non-stdlib exception here).
