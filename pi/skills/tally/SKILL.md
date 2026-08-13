---
name: tally
description: Read and write the project's tally store — todos, scratchpads, and comments — through the `tally` CLI. Use whenever you need to record a plan, track a follow-up, leave a margin note, or check shared state with the human and other agents.
---

# Tally store

The store is shared memory for you, the human, and other agents — reach for it instead of scratch files. Todos for discrete follow-ups, scratchpads for plans and handoffs, comments for the *why*.

`tally` is on your PATH — call it directly. Every subcommand lists its verbs if you run it bare (`tally todos`); add `--json` on reads you want to parse.

## Todos — one discrete follow-up each, id-first

```bash
tally todos create --title "Rotate refresh tokens" --priority p1 --tag auth   # p0 critical … p3 low
tally todos list --status open --json
tally todos update <id> --status in_progress          # open | in_progress | completed
tally todos add-blocker <id> --blocker <other-id>     # <id> can't start until <other-id> completes
tally todos complete <id>
```

## Scratchpads — plans & handoffs, revision-guarded

Read before you write: `read` returns a `revision`; pass it back as `--expected-revision`. A mismatch means someone else edited it — re-read and retry. Prefer `append` / `append-section` / `edit` over rewriting the whole pad.

```bash
tally scratchpads create --name "Auth refactor plan" --content-file -    # reads stdin
tally scratchpads read <id> --mode headings                             # outline first
tally scratchpads append-section <id> --heading "Progress" --content "did X" --expected-revision <r>
tally scratchpads list                                                  # ids + titles + [tags] — filter by eye
```

## Comments — margin notes, the why not the state

```bash
tally comments add <id> --body "hold off — waiting on the auth PR"
tally comments add docs/plans/auth.md --body "step 3 is done"   # target a plan by its path
tally comments recent --since 2h
```

Don't delete the human's items — archive scratchpads, complete the todos you finish.
