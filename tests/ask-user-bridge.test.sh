#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

ROOT="$root" NODE_NO_WARNINGS=1 node --input-type=module 2>/dev/null <<'NODE'
import { pathToFileURL } from "node:url";
const bridge = await import(pathToFileURL(`${process.env.ROOT}/pi/extensions/ask-user-bridge.ts`).href);
if (bridge.shouldWriteMarker(true)) throw new Error("interactive TTY must stay silent");
if (!bridge.shouldWriteMarker(false)) throw new Error("non-TTY smoke tests need markers");

let sessionStart;
bridge.default({
  getAllTools: () => [{ name: "ask_user" }],
  on: (name, handler) => { if (name === "session_start") sessionStart = handler; },
});
const errors = [];
const originalError = console.error;
console.error = (message) => errors.push(String(message));
await sessionStart({}, { ui: { notify: () => { throw new Error("standalone path must not notify"); } } });
console.error = originalError;
if (!errors.some((line) => line.includes("standalone ask_user present"))) {
  throw new Error("non-TTY standalone marker missing");
}
NODE

echo "ask-user bridge OK"
