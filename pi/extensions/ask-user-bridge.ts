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

  pi.on("session_start", async (_event, _ctx) => {
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
