# tally install / distribution — design

**Todo:** `t_dju970chs1541` — Install / distribution story for official-plugin status
**Date:** 2026-07-11
**Verified against:** herdr 0.7.3, `claude` CLI (`~/.local/bin/claude`), reference plugin `herdr-file-viewer` v1.8.0

## Goal / success criterion

`herdr plugin install jasonrosoff/herdr-tally` on a clean machine (macOS or Linux,
**no Rust toolchain**) yields, with **zero manual steps**:

1. Working `Tally` / `Scratchpads` panes (prebuilt binary at `bin/tally`).
2. The tally stdio MCP server registered with Claude Code.
3. The `tally` agent skill installed at `~/.claude/skills/tally/SKILL.md`.

## Facts this design rests on (verified, not assumed)

- **No herdr marketplace/registry.** herdr installs plugins straight from GitHub:
  `herdr plugin install <owner>/<repo>`. "Official-plugin status" = this command
  works cleanly. There is no submission process.
- **`herdr plugin install` runs the manifest `[[build]]` step at install time.**
  (Contrast: `herdr plugin link` does NOT — see CLAUDE.md.) `[[build]]` is therefore
  the single hook every install action hangs off. The `reviewr` plugin already uses
  its `[[build]]` as a general `install.sh`.
- **herdr manifests have no concept of MCP servers or Claude Code skills.** The only
  sections are `[[build]] / [[panes]] / [[actions]] / [[events]]`. MCP registration
  and skill install must happen inside the `[[build]]` step.
- **The plugin root is version-hashed** (`~/.config/herdr/plugins/github/herdr-tally-<hash>/`)
  and changes on upgrade. But `[[build]]` runs on *every* install, so re-running the
  wiring each time (pointing at the current `$HERDR_PLUGIN_ROOT`) sidesteps stale paths.
- **`claude mcp add <name>` is NOT idempotent** — it refuses when the name exists
  ("already exists"). The build hook must `remove || true` then `add`.
- **`SKILL.md` is self-contained** (no sibling-file references) → a single-file copy
  to `~/.claude/skills/tally/SKILL.md` is sufficient. `~/.claude/skills/` is the
  personal-skill load path and already exists.
- **The reference plugin `herdr-file-viewer` v1.8.0 already ships the binary/release
  pattern** the todo asked for, cross-platform, with a hermetically-tested
  `fetch-or-build.sh`. A/B below are near-copy-paste from it.

## Workstreams

### A. Build / release pipeline (low risk — copy the template)

- **`.github/workflows/release.yml`** (adapted from file-viewer):
  - Trigger: push tag `v*`.
  - Per-platform build matrix: `aarch64-apple-darwin`, `x86_64-apple-darwin`,
    `x86_64-unknown-linux-musl` (Linux uses musl; install `musl-tools`).
  - **Version guard:** fail the release if the tag (minus `v`) ≠ `Cargo.toml` version
    ≠ `herdr-plugin.toml` version. Never publish a release whose assets/manifest
    disagree with the source the install step clones.
  - Stage each binary as `tally-<triple>` + a `.sha256`; publish job flattens them,
    concatenates into `SHA256SUMS`, writes `COMMIT` (= `$GITHUB_SHA`), and
    `gh release create`/`upload --clobber`.
  - Release procedure: `git tag vX.Y.Z && git push --tags`.
- **`scripts/fetch-or-build.sh`** — the binary half of the `[[build]]` step:
  - Resolve target triple from `uname -s`/`-m`. Read declared version from `Cargo.toml`.
  - Download `tally-<triple>` + `SHA256SUMS` from
    `https://github.com/jasonrosoff/herdr-tally/releases/download/v<version>/`,
    verify SHA-256, `chmod +x`, `mv -f` into `bin/tally` (fresh inode).
  - **Version-only match** (not commit-exact): a checkout ahead of the released tag
    still uses the released, SHA-verified binary. A version with no release 404s → source.
  - **Fallback** on any miss (no asset, network/checksum error, unmapped platform,
    no curl/wget): source `~/.cargo/env`, `cargo build --release`, then
    `rm -f bin/tally && cp target/release/tally bin/tally`. The `rm -f` before `cp`
    preserves the macOS fresh-inode fix (overwriting a signed Mach-O in place →
    `Killed: 9` at exec). Clear message if `cargo` absent.
  - Env-overridable paths/URL (`TALLY_REPO_ROOT` / `TALLY_CARGO_TOML` / `TALLY_OUT`
    / `TALLY_BASE_URL`) so the logic can be exercised by a hermetic test.

### B. Portability (low risk)

- Manifest `platforms = ["macos", "linux"]`. **Windows deferred to a later version**
  — file-viewer shows Windows is a real lift (unspawnable relative pane commands,
  `-windows`-suffixed action ids, absolute-path launcher scripts). Out of scope for v1.
- One `[[build]]` entry gated `platforms = ["linux", "macos"]`, `/bin/sh` command.
- Widen the hardcoded `/opt/homebrew/bin:/usr/local/bin` PATH prepends (pane commands
  in the manifest + `scripts/*.sh`) to a harmless cross-platform superset that also
  includes `/usr/bin:/bin`. The binary is already located absolutely via
  `$HERDR_PLUGIN_ROOT`; the homebrew prefix is a macOS-launchd workaround and is a
  no-op prepend on Linux.

### C. MCP registration (new — auto-wired, best-effort)

- The `[[build]]` step (after the binary is in place) runs:
  ```sh
  claude mcp remove --scope user tally 2>/dev/null || true
  claude mcp add --scope user tally -- "$HERDR_PLUGIN_ROOT/bin/tally" mcp
  ```
- **`--scope user`** (global): tally infers the project from cwd, so one registration
  serves every project. remove-then-add is re-run-safe on upgrades and re-points at
  the current version-hashed root each install.
- **Locate `claude` robustly** under herdr's bare launchd PATH: probe
  `$HOME/.local/bin`, `/opt/homebrew/bin`, then PATH. If not found → print the exact
  manual `claude mcp add …` command and **continue (exit 0)**. MCP wiring must never
  abort the install; the binary + panes are the critical path.

### D. Skill install (new — auto-wired, best-effort)

- Same build step: `mkdir -p ~/.claude/skills/tally && cp SKILL.md
  ~/.claude/skills/tally/SKILL.md` (overwrite each install → always current).
- Copy, not symlink: no dangling link across the uninstall/reinstall gap, and no
  dependence on the version-hashed root staying put.

### Structure

- `[[build]]` command becomes `["/bin/sh", "scripts/install.sh"]`.
- `scripts/install.sh` orchestrates: call `scripts/fetch-or-build.sh` (keeps the
  binary logic as file-viewer's separately-testable unit), then do the C + D wiring
  inline (both best-effort, warn-and-continue). Binary concerns and integration
  concerns stay cleanly separable.

## Risks / mitigations

- **Auto-wiring runs under herdr's launchd env at install** (reduced PATH). Mitigation:
  best-effort-with-fallback for both C and D — worst case the user runs two printed
  commands and the install still succeeds.
- **MCP server runtime env**: `tally mcp` shells out to `git rev-parse`. Claude Code
  spawns the server with its own (normal) env, so `git` is on PATH — not the reduced
  launchd env. No special handling needed.

## Explicitly out of scope (v1)

- Windows support.
- Any herdr "registry submission" (none exists).
- Auto-update / version-bump UX beyond "re-link/reinstall re-runs the build hook".
  (Manifest edits still require a re-link + pane reopen per the CLAUDE.md gotcha —
  documented, not automated here.)

## Verification (definition of done)

1. `release.yml` produces a GitHub Release with all three triples + `SHA256SUMS` +
   `COMMIT`, and the version guard fails a mismatched tag. (CI + one real tag.)
2. `fetch-or-build.sh` hermetic test: stub `uname`/`curl`/`cargo`/`git`, assert
   prebuilt-download path and each fallback path.
3. On a clean checkout (no `bin/`), the `[[build]]` step installs `bin/tally`,
   registers the MCP server (`claude mcp get tally` succeeds), and installs the skill
   (`~/.claude/skills/tally/SKILL.md` present). Verified via a throwaway
   `herdr plugin link` + build on macOS; Linux verified in CI or a container.
4. With `claude` unavailable on PATH, the build still exits 0 and prints the manual
   command; `bin/tally` is present.
