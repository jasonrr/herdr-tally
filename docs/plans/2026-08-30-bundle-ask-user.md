# plan:bundle-ask-user

**Goal:** Bundle `pi-ask-user` with Tally via a conditional bridge so every supported install path provides `ask_user` without colliding with standalone installs.
**Design:** s_dl1u2j9b1q9s1 (approved)
**Branch:** chore/bundle-ask-user

**Whole-feature verify (run from repo root):**
```bash
npm install --omit=dev && node -e 'const p=require("./package.json");if(p.dependencies["pi-ask-user"]!=="0.14.0"||!p.bundledDependencies.includes("pi-ask-user"))process.exit(1);console.log("manifest OK")' && sh tests/install.test.sh && sh tests/pi-ask-user.test.sh && printf '' | pi --mode rpc --no-extensions -e pi/extensions/ask-user-bridge.ts --no-session 2>&1 | grep -q '\[tally-ask-user\] bundled ask_user registered' && printf '' | pi --mode rpc --no-extensions -e node_modules/pi-ask-user/index.ts -e pi/extensions/ask-user-bridge.ts --no-session 2>&1 | grep -q '\[tally-ask-user\] standalone ask_user present; bundled copy inactive' && echo "feature OK"
```
(The two RPC startup probes close stdin immediately, so they exercise extension startup without a model request, provider login, or network call. `--no-extensions` prevents globally installed copies from making either branch pass accidentally.)

## Verified facts (this planning session)

- `pi-ask-user@0.14.0` package.json: entry `./index.ts`, skills dir `./skills`, **no `main`/`exports`** → resolve the package via `createRequire(...).resolve("pi-ask-user/package.json")`, never bare `"pi-ask-user"`.
- Skill is `skills/ask-user/SKILL.md`, frontmatter name `ask-user`.
- packages.md: git installs run `npm install` (deps land automatically); **local-path installs load in place, no install step** → install.sh must install deps for the local case.
- skills.md: same-name skills from different locations warn and keep the first → gate the bundled skill on bridge activation.
- Lifecycle (extensions.md): factories all run at load, then `session_start`, then `resources_discover`. So a `session_start` check of `pi.getAllTools()` sees a standalone copy's `ask_user` regardless of extension load order, and `activated` is already set when `resources_discover` fires.
- On `/reload` extensions are torn down and rebound fresh → per-instance `activated` flag is correct, no stale-state handling needed.
- A live RPC startup probe against `pi-ask-user@0.14.0` confirmed that dynamic `import(pathToFileURL(index.ts).href)` registers `ask_user` synchronously (`pi.getAllTools()` immediately returned it).
- A live two-extension RPC startup probe confirmed that a later extension's `session_start` handler sees `ask_user` registered by the standalone extension factory.
- `extensions.md` documents `ctx.ui.notify` and the `resources_discover` `{ skillPaths }` response, but the bridge uses `console.error` on load failure so headless runs see it too.

## Patterns followed

- `pi/extensions/routing.ts` — house extension shape (default-export factory, `console.error("[tally-…] …")` marker as greppable test seam, explanatory header comment).
- `scripts/install.sh` — best-effort phases, `TALLY_*` env seams (`TALLY_PI=-` skips), never fatal, print manual command on failure.
- `tests/install.test.sh` — hermetic shell test style: `set -eu`, `mktemp -d`+trap, stub binaries on PATH logging argv, `check()` helper, `exit $fail`.
- docs/plans/2026-08-13-pi-package.md Task 1 — `pi -e … -p -a --no-session … | grep` marker verify.
- Prior art s_dkm86rogch7s1: test runtime behavior, not advertisement → the bridge smoke test greps a runtime marker, not source text.

---

## Task 1 — manifest dependency + lockfile + gitignore

**Exists because:** without `dependencies`/`bundledDependencies` the git-install `npm install` has nothing to fetch and the bridge has nothing to load. The dumb alternative (static nested `pi.extensions`/`pi.skills` paths) collides with standalone installs — rejected in the design.
**Model:** haiku
**Files:** edit `package.json`; create `package-lock.json` (generated); edit `.gitignore`.
**Interfaces:** declares and locks `pi-ask-user@0.14.0`; Task 2's verify (`npm install --omit=dev`) and Task 3's installer phase materialize `node_modules/pi-ask-user/` with `index.ts` and `skills/ask-user/`.
**Steps:**
1. In `package.json` add top-level keys (keep existing keys untouched):
   ```json
   "dependencies": { "pi-ask-user": "0.14.0" },
   "bundledDependencies": ["pi-ask-user"]
   ```
2. Run `npm install --package-lock-only` and commit the generated `package-lock.json` (deterministic installs for git clones).
3. Append `node_modules/` to `.gitignore` (create the file with that single line if absent — check first; the repo may already have one).
**Verify:**
```bash
node -e 'const p=require("./package.json");if(p.dependencies["pi-ask-user"]!=="0.14.0"||!p.bundledDependencies.includes("pi-ask-user"))process.exit(1);console.log("manifest OK")' && npm install --package-lock-only && grep -q '"node_modules/pi-ask-user"' package-lock.json && echo "T1 OK"
```

## Task 2 — conditional bridge extension

**Exists because:** the `pi.extensions` glob loads everything under `pi/extensions` unconditionally, so the "only if no standalone copy" logic must live inside an extension. Without it, users who followed AGENTS.md and installed `pi-ask-user` standalone get duplicate tool/skill registrations.
**Model:** sonnet
**Files:** create `pi/extensions/ask-user-bridge.ts`.
**Interfaces:** consumes Task 1's `node_modules/pi-ask-user/` layout; consumes pi ExtensionAPI `pi.getAllTools()`, `pi.on("session_start")`, `pi.on("resources_discover")` (return `{ skillPaths }`). Produces: registered `ask_user` tool + `ask-user` skill when activated; nothing when a standalone copy exists.
**Steps:**
1. Use the verified loader path directly: resolve `pi-ask-user/package.json` from the bridge's repo-local module root, then `await import(pathToFileURL(join(dir, "index.ts")).href)` and call its default export. The planning-session RPC probe proved this loads 0.14.0 and makes `ask_user` immediately visible in `pi.getAllTools()`.
2. Write `pi/extensions/ask-user-bridge.ts` (follows the routing.ts comment/marker style):
   ```typescript
   import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
   import { createRequire } from "node:module";
   import { dirname, join } from "node:path";
   import { pathToFileURL } from "node:url";

   // Conditional bridge to the bundled pi-ask-user (package.json dependencies +
   // bundledDependencies). Registers the bundled ask_user tool and contributes
   // its ask-user skill ONLY when no other ask_user is loaded — a standalone
   // `pi install npm:pi-ask-user` copy wins, so the two never collide.
   // "[tally-ask-user] …" markers are the greppable test seam (same as routing.ts).
   // ponytail: per-session flag, no standalone-copy change watch; if a user
   // installs/removes pi-ask-user mid-session, /reload re-evaluates.

   function bundledPackageDir(): string | null {
     try {
       // No main/exports in pi-ask-user's package.json — resolve package.json
       // and walk up rather than resolving the bare package name.
       return dirname(createRequire(import.meta.url).resolve("pi-ask-user/package.json"));
     } catch {
       return null; // deps not installed (local-path install without npm install)
     }
   }

   export default function (pi: ExtensionAPI) {
     let activated = false;

     pi.on("session_start", async (_event, ctx) => {
       if (activated) return;
       if (pi.getAllTools().some((t) => t.name === "ask_user")) {
         console.error("[tally-ask-user] standalone ask_user present; bundled copy inactive");
         return;
       }
       const dir = bundledPackageDir();
       if (!dir) return; // silent: skills fall back to numbered options, as today
       try {
         const mod = await import(pathToFileURL(join(dir, "index.ts")).href);
         mod.default(pi);
         activated = true;
         console.error("[tally-ask-user] bundled ask_user registered");
       } catch (err) {
         console.error(`[tally-ask-user] bundled ask_user failed to load: ${err}`);
       }
     });

     pi.on("resources_discover", async () => {
       if (!activated) return undefined;
       const dir = bundledPackageDir();
       return dir ? { skillPaths: [join(dir, "skills")] } : undefined;
     });
   }
   ```
3. Test both branches with extension discovery disabled so the user's global package settings cannot affect the result.
**Verify:**
```bash
npm install --omit=dev && printf '' | pi --mode rpc --no-extensions -e pi/extensions/ask-user-bridge.ts --no-session 2>&1 | grep -q '\[tally-ask-user\] bundled ask_user registered' && printf '' | pi --mode rpc --no-extensions -e node_modules/pi-ask-user/index.ts -e pi/extensions/ask-user-bridge.ts --no-session 2>&1 | grep -q '\[tally-ask-user\] standalone ask_user present; bundled copy inactive' && echo "T2 OK"
```

## Task 3 — install.sh: npm-deps phase for local-path installs

**Exists because:** `pi install <local-path>` (what `herdr plugin install` does in phase 2b) never runs `npm install`, so `node_modules/pi-ask-user` would be missing and the bridge silently no-ops for exactly the users the design promised to cover. Git installs don't need this (pi runs `npm install` itself).
**Model:** sonnet
**Files:** edit `scripts/install.sh` (new phase 2a, header comment "Four phases" → "Five phases" list updated).
**Interfaces:** consumes Task 1's `package.json`; produces `node_modules/` in `$plugin_root` before phase 2b's `pi install`. Adds env seam `TALLY_NPM` (mirrors `TALLY_PI`: default `npm`, `-` skips) consumed by Task 4.
**Steps:**
1. Insert phase 2a immediately before the phase-2b block, following the existing seam/best-effort pattern:
   ```sh
   # --- 2a. pi runtime dependencies (best-effort) -------------------------------
   # Local-path pi installs load the package in place and never run npm install,
   # so the bundled pi-ask-user bridge (pi/extensions/ask-user-bridge.ts) would
   # find no node_modules. Git installs run npm install themselves; this covers
   # the local path. Never fatal — the bridge no-ops silently without the dep.
   # Test seam mirroring TALLY_PI: override the npm binary, or TALLY_NPM=- to skip.
   npm_bin="${TALLY_NPM:-npm}"
   if [ "$npm_bin" != "-" ] && [ -f "$plugin_root/package.json" ] && [ ! -d "$plugin_root/node_modules/pi-ask-user" ] && command -v "$npm_bin" >/dev/null 2>&1; then
     if "$npm_bin" install --omit=dev --prefix "$plugin_root" >/dev/null 2>&1; then
       echo "tally: installed pi runtime dependencies -> $plugin_root/node_modules"
     else
       echo "tally: could not install pi runtime dependencies. Run:" >&2
       echo "  $npm_bin install --omit=dev --prefix \"$plugin_root\"" >&2
     fi
   elif [ "$npm_bin" != "-" ] && [ ! -d "$plugin_root/node_modules/pi-ask-user" ]; then
     echo "tally: npm not found; pi runtime dependency was not installed. Run:" >&2
     echo "  npm install --omit=dev --prefix \"$plugin_root\"" >&2
   fi
   ```
2. Update the header phase list (add `2a. install pi runtime dependencies (best-effort)`).
**Verify:**
```bash
sh -n scripts/install.sh && echo "T3 OK"
```

## Task 4 — hermetic installer + bridge smoke tests

**Exists because:** the install seam and both duplicate-copy behaviors are exactly the regressions that will silently ship otherwise; prior art (s_dkm86rogch7s1) demands behavior probes, not advertisement checks.
**Model:** sonnet
**Files:** create `tests/pi-ask-user.test.sh`; edit `tests/install.test.sh` to export `TALLY_NPM=-` beside its existing `TALLY_*` test seams, preventing the legacy installer test from invoking real npm.
**Interfaces:** consumes Task 3's `TALLY_NPM` seam and Task 2's markers.
**Steps:**
1. Write `tests/pi-ask-user.test.sh` in the install.test.sh house style (`set -eu`, `mktemp -d`+trap, `check()` helper, `exit $fail`): stub `TALLY_FETCH_OR_BUILD`, stub `claude`, stub `pi` (argv-logging), stub `npm` (argv-logging) as `$work/bin/npm`, temp `HOME`, `HERDR_PLUGIN_ROOT="$work"` with a minimal `$work/package.json` (`{"name":"t","dependencies":{"pi-ask-user":"0.14.0"}}`).
   - Case A (deps missing): run install.sh → npm stub log contains `install --omit=dev --prefix` and the npm invocation precedes the pi invocation (compare log line numbers across the two stub logs via a shared append-only `$work/calls.log` each stub writes to).
   - Case B (`TALLY_NPM=-`): npm stub log untouched, exit 0.
   - Case C (npm fails): stub `exit 1` → install.sh exits 0 and stderr contains `could not install pi runtime dependencies`.
   - Case D (deps present): `mkdir -p "$work/node_modules/pi-ask-user"` → npm not invoked.
2. In `tests/install.test.sh`, add `export TALLY_NPM=-` near `TALLY_FETCH_OR_BUILD`/`TALLY_BIN`; do not let this pre-existing hermetic test invoke real npm. The new sibling test owns phase-2a behavior coverage.
3. Keep bridge runtime coverage in Task 2; this task proves installer ordering, skip, failure, present-dependency, and missing-npm warning behavior. Add a missing-npm case whose stderr contains `npm not found; pi runtime dependency was not installed`.
**Verify:**
```bash
sh tests/pi-ask-user.test.sh && sh tests/install.test.sh && echo "T4 OK"
```

## Task 5 — README pi section note

**Exists because:** README's pi section documents both install paths; users need to know `ask_user` now ships with tally and that a standalone install still takes precedence (otherwise they'll debug "which copy am I running" blind). AGENTS.md's compound-engineering block still says `pi install npm:pi-ask-user` — left untouched: it's owned by pi-compound-engineering, and the bridge makes following it harmless (standalone wins).
**Model:** haiku
**Files:** edit `README.md` (pi section, after line ~163 install block).
**Steps:** insert one paragraph:
```markdown
`ask_user` (from [pi-ask-user](https://github.com/edlsh/pi-ask-user)) ships bundled
with tally — no separate install needed. If you also have `pi-ask-user` installed
on its own, that copy wins and the bundled one stays inactive.
```
**Verify:**
```bash
grep -q 'ships bundled' README.md && echo "T5 OK"
```

---

## Todo ledger (create at build handoff — one todo per task, tag plan:bundle-ask-user)

| Todo | Title | Blocked by |
|------|-------|-----------|
| T1 | manifest dependency + lockfile + gitignore | — |
| T2 | conditional bridge extension | T1 |
| T3 | install.sh npm-deps phase | — |
| T4 | hermetic installer + bridge smoke tests | T2, T3 |
| T5 | README pi section note | — |

## Explicitly not in this plan (cut per design)

- No vendored/copied ask UI code. No static nested `pi.extensions`/`pi.skills` manifest paths. No installer-only dependency. No removal of the numbered-options fallback. No CI wiring (none exists). No AGENTS.md edit (foreign-owned block; bridge neutralizes the collision).
