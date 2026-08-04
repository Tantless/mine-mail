---
name: isolate-worktree
description: Create, integrate, and safely clean a dedicated Git worktree and unique task branch for one Codex session. Use immediately before the first repository write after read-only investigation determines that code, tests, documentation, configuration, generated files, or dependencies must change; use again when the user explicitly asks to merge that session into the base branch or when a verified merged worktree is ready for cleanup. Do not allocate a worktree for read-only diagnosis, explanation, review, or status reporting.
---

# Isolate Worktree

Use the bundled `scripts/worktree.ps1` as the only implementation of Git
allocation, integration, and cleanup. Resolve the script relative to this
`SKILL.md`; do not rewrite its commands ad hoc.

## Choose the phase

- Stay in investigation mode for read-only diagnosis, explanation, review, and
  status reporting. Do not allocate anything.
- Before the first write, formatter, generator, dependency installation, or
  command that may change tracked files, enter implementation mode and allocate
  exactly one task worktree.
- Reuse the worktree already recorded by the session. Never allocate a second
  worktree for the same task.
- Enter integration mode only when the user explicitly asks to merge this
  session's result into the base branch.

## Allocate before writing

1. Choose a short lowercase ASCII task slug such as `fix-imap-timeout`.
2. From the primary checkout, run:

   ```powershell
   powershell -NoProfile -File <skill-dir>\scripts\worktree.ps1 create -TaskSlug <slug>
   ```

   Use `pwsh` instead of `powershell` when that is the available PowerShell
   executable. Pass `-BaseBranch` or `-BranchPrefix` only when the repository
   intentionally uses non-default names.
3. Parse the returned JSON and retain `branch` and `worktree` for the entire
   session. Report both values to the user.
4. Explicitly target the returned worktree for every subsequent file operation
   and command. The primary checkout remains read-only and stays on the base
   branch.
5. If allocation refuses a dirty primary checkout or otherwise fails, stop
   before writing. Do not stash, reset, copy uncommitted changes, switch the
   primary branch, or silently edit the primary checkout.

## Implement and commit

- Inspect and preserve any pre-existing state in the task worktree.
- Keep all task edits, tests, builds, and commits inside that worktree.
- Apply the repository's commit-message and verification rules.
- Hand off the branch and verification result without merging unless the user
  explicitly requests integration.

## Integrate only on request

1. Confirm that the user explicitly requested a merge. Serialize integration;
   never race another session for the base worktree.
2. Ensure the task worktree is clean and every intended change is committed.
3. Choose a merge subject that satisfies the target repository's commit rules,
   then run from outside the task worktree:

   ```powershell
   powershell -NoProfile -File <skill-dir>\scripts\worktree.ps1 integrate -Branch <branch> -MergeMessage <subject>
   ```

4. Run the repository's applicable verification in the primary checkout.
5. If integration or verification fails, keep the task worktree and branch for
   recovery. Report the failure; do not force cleanup.
6. After successful verification, run:

   ```powershell
   powershell -NoProfile -File <skill-dir>\scripts\worktree.ps1 cleanup -Branch <branch>
   ```

The cleanup command proves that the task branch is contained in the base branch
and that both worktrees are clean before it removes the worktree, safely deletes
the local task branch, and prunes stale metadata. It never deletes a remote
branch.

## Reuse in another repository

Copy the complete `isolate-worktree` skill directory into the target
repository's `.agents/skills/` directory. Add a concise `AGENTS.md` rule that
requires `$isolate-worktree` immediately before the first repository write and
forbids integration without an explicit user request. Keep repository-specific
commit and verification rules in that repository's `AGENTS.md`, not in this
skill.
