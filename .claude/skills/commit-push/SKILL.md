---
name: commit-push
description: Verify, commit with a conventional message, push to master, and check exact-head CI
disable-model-invocation: true
---

1. Run the `/verify` steps; stop if anything fails.
2. Review `git status` and `git diff`. Stage only relevant files (never `.serena/`).
3. Commit with a lowercase conventional message `type(scope): imperative`, types `fix|test|docs|style|diagnostics`, scope a crate area (`source`) or TODO item ID (`src-003`). Match `git log` style.
4. `git push origin master`. If the push result is ambiguous or times out, re-read the remote head (`git fetch && git rev-parse origin/master`) before doing anything else; never blindly retry.
5. Check CI for the exact pushed commit SHA (`gh run list --commit <sha>`) and report its status. Do not claim a TODO item is complete without committed qualification evidence from exact-head CI.

$ARGUMENTS, if given, is the intended commit message or scope.
