---
name: debug
description: Use on any bug, test failure, or unexpected behavior, before proposing fixes — root cause first, one hypothesis at a time. The second loop entry beside /brainstorm; exits into an inline fix or /tally:plan.
---

# Debug

No fix before root cause. Symptom patches come back; time pressure makes guessing tempting and guessing is slower.

## Find the cause

1. Read the whole error — stack trace, line numbers, warnings. Reproduce it reliably before anything else; if you can't, gather more data, don't guess. Check what changed recently (`git diff`, new deps, config).
2. In multi-component paths (CI → build → sign, API → service → DB), instrument each boundary — log what enters and exits — and run once to see *where* it breaks before theorizing about *why*.
3. Trace a bad value backward to where it originates. Fix at the source, not where it surfaced.
4. Find the nearest working example of the same pattern and diff against it line by line — no difference is "too small to matter".
5. Replace any arbitrary sleep/timeout with waiting on the actual condition — timing "fixes" are symptom patches.

## Test one hypothesis

State it: "X is the root cause because Y." Make the smallest change that tests it, one variable at a time. Wrong → new hypothesis, and undo the probe; never stack fixes. Unsure → say so and dig, don't pretend.

**Circuit breaker:** three failed fixes means the architecture is in question, not your luck — stop and take it to the user before attempt four. Each fix revealing a new problem somewhere else is the signature.

## Fix and exit

- Small fix: failing test reproducing the bug first (watch it fail), then the fix, then the full suite. Fresh output before claiming fixed.
- Fix too big for inline (architectural, multi-file): take the diagnosis to /tally:plan — the root-cause writeup is the design input.
- Genuinely environmental/external (rare — most "no root cause" is incomplete investigation): document what you ruled out in a tally scratchpad, add handling (retry, timeout, clear error) and logging for next time.

Follow-ups the investigation turns up go to tally todos, not into this fix. When the root cause would surprise the next session, append one line to a `learnings`-tagged tally scratchpad — `pattern → consequence` (create the scratchpad if missing).
