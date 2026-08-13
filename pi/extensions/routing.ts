import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { readFileSync } from "node:fs";
import { join } from "node:path";

// The tally dev-loop routing block. CANONICAL COPY: hooks/session-start.sh
// (the Claude SessionStart hook); also mirrored in skills/setup/SKILL.md.
// Keep all copies byte-identical when the routing rules change.
// ponytail: 3 hand-synced copies (hook, setup skill, here). A shared data
// file read by both bash and TS would kill the drift — not worth the
// machinery until the block actually churns.
const DEV_LOOP_BLOCK = `<dev-loop>
This project uses the tally dev loop. Standing routing — the human should never have to ask:
- Feature or non-trivial change requested → run /tally:brainstorm before writing any code.
- Bug, failing test, or unexpected behavior → run /tally:debug before proposing any fix.
- Approved design (tally scratchpad tagged design) or any multi-step implementation → run /tally:plan before building.
- A plan doc with plan:<slug> todos exists → execute it with /tally:build.
- Branch complete, or a merge/PR is requested → run /tally:review-branch first.
Trivial one-file changes and pure questions are exempt. When this rule triggers a skill, say so in one line.
</dev-loop>`;

// Mirrors hooks/session-start.sh: inject only when this repo opted in via
// .claude/tally-dev-loop.md containing a `routing: on` line. Missing file or
// missing line → routing off, silently (same as the hook's `exit 0`).
function routingEnabled(cwd: string): boolean {
  try {
    const cfg = readFileSync(join(cwd, ".claude", "tally-dev-loop.md"), "utf8");
    return /^routing: on$/m.test(cfg);
  } catch {
    return false;
  }
}

export default function (pi: ExtensionAPI) {
  let injected = false; // once per session; reset for branch-safety

  pi.on("session_start", async () => {
    injected = false;
  });

  pi.on("before_agent_start", async (event, ctx) => {
    if (injected) return;
    if (!routingEnabled(ctx.cwd)) return;
    injected = true;
    // Framework-free test marker: greppable on stderr in -p mode.
    console.error("[tally-routing] dev-loop routing injected");
    return { systemPrompt: `${event.systemPrompt}\n\n${DEV_LOOP_BLOCK}` };
  });
}
