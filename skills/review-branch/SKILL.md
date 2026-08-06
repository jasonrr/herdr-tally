---
name: review-branch
description: Whole-branch review before merge — 2-3 scoped reviewer subagents, findings verified then fixed in one consolidated pass. Use when a feature branch is complete (after /build) or before any merge or PR.
---

# Review branch

One deep pass over `merge-base..HEAD`, sized to the diff — not a fixed fan-out.

## Reviewers

Write the diff to a file. Dispatch 2–3 fresh subagents on `sonnet` — explicit model on every dispatch — each given the diff path, the plan doc path, and one lens:

1. **Correctness and spec fidelity** — does the code do what the plan says; edge cases; whether the tests actually discriminate.
2. **Simplicity** — what can be deleted; abstractions with one caller; the cheaper version that still works.
3. Only if the diff warrants it: **security** (auth, input, secrets paths) or **data integrity** (migrations, persisted formats). Skip otherwise.

Escalate to `opus` only for your own synthesis of a large architectural branch — not for the reviewers.

## Findings

Verify each finding against the code yourself before accepting it — reviewers produce plausible-but-wrong findings, and every false one you forward costs a fix dispatch. File confirmed findings as tally todos (`p1` = fix before merge, `p2`/`p3` = follow-up) with a comment on why.

Fix all p1s in ONE consolidated dispatch on `sonnet` — a fixer per finding costs more than the branch did — then one scoped re-review of that fix.

## Gate

Run the full test suite and lint fresh, in this session, and read the output. Then present the real options — merge, push + PR, or leave the branch — and wait for the user's choice. The p2/p3 follow-ups live in tally, not in your head.
