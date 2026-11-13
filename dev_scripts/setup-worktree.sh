#!/usr/bin/env bash
# Bootstrap a fresh git worktree: copy local env files from the main checkout,
# then install deps for whatever stacks the repo has (root + one level down).
# Run by Orca (orca.yaml scripts.setup) and by the herdr/Claude SessionStart hook.
# Writes .worktree-bootstrapped on success; re-runs are no-ops unless --force.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
MARKER="$ROOT/.worktree-bootstrapped"
[ "${1:-}" = "--force" ] || [ ! -f "$MARKER" ] || { echo "already bootstrapped"; exit 0; }

# Main checkout = first entry of `git worktree list`.
MAIN="${ORCA_ROOT_PATH:-$(git worktree list --porcelain | sed -n '1s/^worktree //p')}"

# 1. Env files (gitignored, so a new worktree lacks them). Never .example.
if [ "$MAIN" != "$ROOT" ]; then
  (cd "$MAIN" && find . -maxdepth 2 -path ./node_modules -prune -o -type f \
      \( -name '.env' -o -name '.env.*' -o -name '.dev.vars' \) ! -name '*.example' -print) |
  while read -r f; do
    [ -e "$ROOT/$f" ] && continue
    mkdir -p "$ROOT/$(dirname "$f")" && cp "$MAIN/$f" "$ROOT/$f" && echo "copied $f"
  done
fi

# 2. Deps, per directory that has a lockfile/manifest.
for dir in "$ROOT" "$ROOT"/*/; do
  dir="${dir%/}"
  case "$dir" in */node_modules|*/.venv) continue ;; esac
  if [ -f "$dir/uv.lock" ]; then (cd "$dir" && uv sync)
  elif [ -f "$dir/requirements.txt" ] && [ ! -f "$dir/pyproject.toml" ]; then
    (cd "$dir" && python3 -m venv .venv && .venv/bin/pip install -q -r requirements.txt)
  elif [ -f "$dir/pyproject.toml" ]; then (cd "$dir" && uv sync)
  fi
  if [ -f "$dir/package-lock.json" ]; then (cd "$dir" && npm ci)
  elif [ -f "$dir/pnpm-lock.yaml" ]; then (cd "$dir" && pnpm install --frozen-lockfile)
  elif [ -f "$dir/bun.lock" ] || [ -f "$dir/bun.lockb" ]; then (cd "$dir" && bun install)
  elif [ -f "$dir/package.json" ]; then (cd "$dir" && npm install)
  fi
done

touch "$MARKER"
echo "worktree bootstrapped"
