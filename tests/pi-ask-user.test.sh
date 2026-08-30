#!/bin/sh
# Hermetic test for the phase-2a npm-deps block in scripts/install.sh (runs
# before phase 2b `pi install`). Stubs npm/pi/claude and the binary step, then
# asserts: npm is invoked with the right args when deps are missing, npm runs
# before pi, TALLY_NPM=- skips npm entirely, a failing npm still exits 0 with
# a manual-fix hint on stderr, an existing node_modules/pi-ask-user skips npm,
# and a missing npm binary also degrades to a stderr hint + exit 0.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
script="$here/../scripts/install.sh"
fail=0
check() { if [ "$2" = "$3" ]; then echo "ok - $1"; else echo "FAIL - $1: expected [$3] got [$2]"; fail=1; fi; }

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin" "$work/home"

# Minimal manifest so plugin_root has a package.json (npm-deps guard needs it).
printf '{"name":"t","dependencies":{"pi-ask-user":"0.14.0"}}\n' > "$work/package.json"

# stub fetch-or-build: just create bin/tally under the plugin root's OUT
cat > "$work/fob.sh" <<EOF
#!/bin/sh
mkdir -p "$work/pluginbin"; printf 'BIN' > "$work/pluginbin/tally"; exit 0
EOF
chmod +x "$work/fob.sh"

# stub claude: log its argv so phase 2 stays inert and never touches real state
cat > "$work/bin/claude" <<EOF
#!/bin/sh
echo "\$@" >> "$work/claude.log"; exit 0
EOF
chmod +x "$work/bin/claude"

# stub pi: records into a shared ordering log (calls.log) and its own pi.log
cat > "$work/bin/pi" <<EOF
#!/bin/sh
echo "pi \$@" >> "$work/calls.log"
echo "pi \$@" >> "$work/pi.log"
exit 0
EOF
chmod +x "$work/bin/pi"

# stub npm (default: succeeds): records into calls.log and npm.log
write_npm_ok() {
  cat > "$work/bin/npm" <<EOF
#!/bin/sh
echo "npm \$@" >> "$work/calls.log"
echo "npm \$@" >> "$work/npm.log"
exit 0
EOF
  chmod +x "$work/bin/npm"
}
write_npm_fail() {
  cat > "$work/bin/npm" <<EOF
#!/bin/sh
echo "npm \$@" >> "$work/calls.log"
echo "npm \$@" >> "$work/npm.log"
exit 1
EOF
  chmod +x "$work/bin/npm"
}
write_npm_ok

export HOME="$work/home"
export TALLY_FETCH_OR_BUILD="$work/fob.sh"
export TALLY_BIN="$work/pluginbin/tally"
export HERDR_PLUGIN_ROOT="$work"
export TALLY_PI="$work/bin/pi"
export TALLY_NPM="$work/bin/npm"
export TALLY_CLAUDE="$work/bin/claude"  # never touch real global claude MCP config

reset_logs() {
  rm -f "$work/calls.log" "$work/npm.log" "$work/pi.log" "$work/claude.log" "$work/err.log"
}

# ===== Case A: deps missing, npm present -> npm runs, then pi, in order ========
reset_logs
PATH="$work/bin:/usr/bin:/bin" sh "$script" >/dev/null 2>"$work/err.log"; rc=$?
check "case A exit 0" "$rc" "0"
addcount=$(grep -c "install --omit=dev --legacy-peer-deps --prefix" "$work/npm.log" 2>/dev/null || echo 0)
check "case A npm invoked with install --omit=dev --legacy-peer-deps --prefix" "$addcount" "1"
npm_line=$(grep -n "^npm " "$work/calls.log" | head -n1 | cut -d: -f1)
pi_line=$(grep -n "^pi " "$work/calls.log" | head -n1 | cut -d: -f1)
if [ -n "${npm_line:-}" ] && [ -n "${pi_line:-}" ] && [ "$npm_line" -lt "$pi_line" ]; then
  order="before"
else
  order="not-before"
fi
check "case A npm runs before pi" "$order" "before"

# ===== Case B: TALLY_NPM=- -> npm skipped entirely ==============================
reset_logs
PATH="$work/bin:/usr/bin:/bin" TALLY_NPM=- sh "$script" >/dev/null 2>"$work/err.log"; rc=$?
check "case B exit 0" "$rc" "0"
npmlog_exists=$([ -s "$work/npm.log" ] && echo yes || echo no)
check "case B npm never invoked" "$npmlog_exists" "no"

# ===== Case C: npm present but fails -> best-effort, exit 0 + stderr hint ======
reset_logs
write_npm_fail
PATH="$work/bin:/usr/bin:/bin" sh "$script" >/dev/null 2>"$work/err.log"; rc=$?
check "case C exit 0" "$rc" "0"
hint=$(grep -c "could not install pi runtime dependencies" "$work/err.log" 2>/dev/null || echo 0)
check "case C stderr hint on npm failure" "$hint" "1"
write_npm_ok

# ===== Case D: deps present AT PINNED VERSION -> npm not invoked ================
reset_logs
mkdir -p "$work/node_modules/pi-ask-user"
printf '{"name":"pi-ask-user","version":"0.14.0"}\n' > "$work/node_modules/pi-ask-user/package.json"
PATH="$work/bin:/usr/bin:/bin" sh "$script" >/dev/null 2>"$work/err.log"; rc=$?
check "case D exit 0" "$rc" "0"
npmlog_exists_d=$([ -s "$work/npm.log" ] && echo yes || echo no)
check "case D npm not invoked when deps present at pinned version" "$npmlog_exists_d" "no"
rm -rf "$work/node_modules"

# ===== Case F: deps present at STALE version -> npm reinstalls ==================
reset_logs
mkdir -p "$work/node_modules/pi-ask-user"
printf '{"name":"pi-ask-user","version":"0.10.0"}\n' > "$work/node_modules/pi-ask-user/package.json"
PATH="$work/bin:/usr/bin:/bin" sh "$script" >/dev/null 2>"$work/err.log"; rc=$?
check "case F exit 0" "$rc" "0"
upgrade=$(grep -c "install --omit=dev --legacy-peer-deps --prefix" "$work/npm.log" 2>/dev/null || echo 0)
check "case F npm reinstalls on version drift" "$upgrade" "1"
rm -rf "$work/node_modules"

# ===== Case E: npm not found -> best-effort, exit 0 + stderr hint ==============
reset_logs
PATH="$work/bin:/usr/bin:/bin" TALLY_NPM="npm-does-not-exist-xyz" sh "$script" >/dev/null 2>"$work/err.log"; rc=$?
check "case E exit 0" "$rc" "0"
notfound=$(grep -c "npm not found; pi runtime dependency was not installed" "$work/err.log" 2>/dev/null || echo 0)
check "case E stderr hint when npm missing" "$notfound" "1"

exit $fail
