---
name: brainstorm
description: Use before building any feature or non-trivial change — explores the problem, cuts scope, and lands an approved design in a tally scratchpad before any code is written. Triggers on "let's brainstorm", "help me think through", or ambiguous feature requests.
---

# Brainstorm

Goal: an approved design the size of the problem. No code, scaffolding, or file edits until the user approves the design.

## Process

1. Ground yourself in the code first. Read the relevant areas; delegate broad scans to an Explore subagent and keep the judgment here.
2. Ask one question at a time. Use AskUserQuestion with concrete options when the choice is discrete. Stop asking when the next answer wouldn't change the design.
3. Propose 2–3 approaches with a stated recommendation and what each trades away. If only one sane approach exists, say so and skip the menu.
4. Apply the cut test to every element: "what breaks if we don't build this?" No concrete answer means cut it, and say what was cut. Prefer the version that ships less.
5. For any UI/TUI change, present ASCII mockups with realistic data and get sign-off on a mockup, not on prose.

## Output

Write the design to a tally scratchpad (ToolSearch `mcp__tally__` if the tools aren't loaded) tagged `design`: problem, chosen approach, what was cut and why, open questions. Keep it under a page — it's a handoff artifact for the next session, not a spec ritual. File a tally todo for any question deferred rather than answered.

Design approval comes from the user, not from you. Once approved, offer /plan.
