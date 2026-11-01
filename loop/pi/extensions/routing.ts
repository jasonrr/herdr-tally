import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { readFileSync } from "node:fs";
import { join } from "node:path";

// Single copy of the routing block, shared with hooks/session-start.sh.
// Read at import so a wrong path fails loudly at load, not silently per turn.
// Assumes pi loads this file in place (it does today); a bundler would break the relative path.
const DEV_LOOP_BLOCK = readFileSync(new URL("../../hooks/routing.md", import.meta.url), "utf8").trimEnd();

// Mirrors hooks/session-start.sh: .claude/dev-loop.md, else the older
// .claude/tally-dev-loop.md; a `routing: on` line, matched exactly (no CRLF).
function routingEnabled(cwd: string): boolean {
  for (const name of ["dev-loop.md", "tally-dev-loop.md"]) {
    try {
      const cfg = readFileSync(join(cwd, ".claude", name), "utf8");
      return cfg.split("\n").some((line) => line === "routing: on");
    } catch {}
  }
  return false;
}

export default function (pi: ExtensionAPI) {
  pi.on("before_agent_start", async (event, ctx) => {
    // Injected every turn (not once per session): pi resets systemPrompt to
    // base before each call, so a once-per-session flag would only ever fire
    // on turn 1. Content-guarded instead, so re-injection is a no-op if pi
    // ever starts accumulating systemPrompt across turns.
    if (!routingEnabled(ctx.cwd)) return;
    if (event.systemPrompt.includes(DEV_LOOP_BLOCK)) return;
    // Framework-free test marker: greppable on stderr in -p mode. Only there —
    // in the interactive TUI, stderr paints over the "Working" line every turn.
    if (!ctx.hasUI) console.error("[loop-routing] dev-loop routing injected");
    return { systemPrompt: `${event.systemPrompt}\n\n${DEV_LOOP_BLOCK}` };
  });
}
