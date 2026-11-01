<dev-loop>
This project uses the dev loop. Standing routing — the human should never have to ask:
- Feature or non-trivial change requested → run /loop:brainstorm before writing any code.
- Bug, failing test, or unexpected behavior → run /loop:debug before proposing any fix.
- Approved design (an uncommitted docs/decisions/*.md) or any multi-step implementation → run /loop:plan before building.
- A plan file (docs/plans/*.md) named in your brief, or one with an open draft PR → execute it with /loop:build.
- Branch complete, or a merge/PR is requested → run /loop:review-branch first.
Trivial one-file changes and pure questions are exempt. When this rule triggers a skill, say so in one line.
</dev-loop>
