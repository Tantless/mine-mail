---
name: release-mine-mail
description: Release a Mine Mail version from main by checking the repository, synchronizing canonical version fields, running release verification, creating a compliant release commit and annotated Git tag, atomically pushing main and the tag, and verifying remote refs. Use when the user explicitly asks to publish or release Mine Mail, bump to a vX.Y.Z version and push/tag it, or invokes $release-mine-mail. Do not use to rewrite an existing release/tag, publish another repository, or merely prepare changes without pushing.
---

# Release Mine Mail

Publish one explicit Mine Mail version without force-pushing, rewriting tags, or
mixing unrelated work.

## Required input and authority

- Require an explicit target matching `vX.Y.Z` or `vX.Y.Z-prerelease`.
- Treat an explicit request to release or invocation of this skill with a target
  version as authority to edit the version, commit, tag, and push.
- Treat that explicit target-version release as the sole exception to the
  repository's worktree-isolation rule. Work directly on a clean primary `main`
  and do not invoke `$isolate-worktree`. A check-only or preparation request is
  not an exception and remains read-only until publication is explicitly
  authorized.
- If the user asks only to check or prepare a release, stop before the first
  unrequested commit, tag, or push.
- Read repository `AGENTS.md`, `contributing.md`, `.github/workflows/release.yml`,
  and the relevant open items in `docs/RELEASE.md` before changing files.
- Work only on Mine Mail's `main` branch and `origin` remote. Never force-push,
  move/delete an existing tag, rewrite history, or open a PR for this workflow.
- Serialize this workflow with every other integration or publication. Never
  modify, merge, remove, or clean another session's branch or worktree.

## Workflow

### 1. Preflight the repository

1. Inspect `git status -sb`, the current branch, `origin`, recent commits, local
   tags, and `git worktree list --porcelain`.
2. List local `codex/*` branches not contained in `main`. Never touch them. If
   any exist, require confirmation that they are intentionally excluded unless
   the release request already explicitly says to publish current `main` as-is.
3. Require a clean, understood primary worktree. If changes already exist,
   continue only when they are entirely part of this exact release and safe to
   preserve; otherwise stop and ask which changes belong.
4. Run `git fetch origin --prune --tags`.
5. Compare `main` with `origin/main`.
   - Fast-forward a clean behind-only branch with `git merge --ff-only
     origin/main`.
   - Allow an ahead-only branch after inspecting and reporting the commits that
     the release push will include.
   - Stop on divergence. Do not rebase, merge, reset, or force-push implicitly.
6. Confirm the target tag is absent locally and remotely. Stop if it exists.
7. Confirm the target version is greater than the current canonical version.

### 2. Set the version once

From the repository root, run:

```text
node .agents/skills/release-mine-mail/scripts/set-version.mjs vX.Y.Z
```

The script updates and cross-checks only these release-owned files:

- `Cargo.toml`
- `Cargo.lock`
- `web/src-tauri/Cargo.toml`
- `web/src-tauri/Cargo.lock`
- `web/src-tauri/tauri.conf.json`
- `installer/windows/src/installerState.js`
- `installer/windows/src/installerState.test.js`

Run `git diff --check` and inspect the complete diff. Require exactly the
expected seven files and version-only changes. Use the script with `--check`
afterward when an extra consistency check is useful:

```text
node .agents/skills/release-mine-mail/scripts/set-version.mjs vX.Y.Z --check
```

### 3. Verify the exact release tree

Run all repository release checks after the version change:

```text
cargo test
cd web && npm test -- --run && npm run build
cd web/src-tauri && cargo test && cargo check
cd installer/windows && npm test
```

Adapt command separators to the active shell. Independent checks may run in
parallel when their output and exit status remain attributable. If any check
fails, do not commit, tag, or push; preserve the version diff and report the
failure.

### 4. Commit the release

1. Stage only the seven expected version files with explicit paths.
2. Inspect `git diff --cached --check`, the staged stat, names, and full staged
   diff.
3. Commit with the exact subject:

```text
release: 发布 vX.Y.Z
```

4. Run `git log -1 --format=%s` and ensure the actual subject satisfies
   `contributing.md`.

### 5. Create and publish the tag

1. Fetch `origin` again and stop if `origin/main` advanced incompatibly.
2. Create an annotated tag on `HEAD`:

```text
git tag -a vX.Y.Z -m "release: 发布 vX.Y.Z"
```

3. Verify the tag peels to the release commit with `git rev-list -n 1
   vX.Y.Z` and compare it with `git rev-parse HEAD`.
4. Atomically publish the branch and tag:

```text
git push --atomic origin main refs/tags/vX.Y.Z
```

If the push fails, keep the local release commit and tag intact, do not force or
rewrite anything, and report the exact remote error.

### 6. Verify and report

1. Confirm `git status -sb` is clean and tracks `origin/main`.
2. Use `git ls-remote` to confirm remote `main`, the annotated tag object, and
   the peeled tag commit. Require remote `main` and the peeled tag to equal the
   local release commit.
3. If existing GitHub access can read Actions, confirm the tag-triggered Release
   workflow started and report its state. Do not require `gh`, install tools, or
   block a successful Git/tag publication solely because workflow status access
   is unavailable.
4. Report the version, commit hash and subject, branch/tag push result, checks
   run, clean status, and whether the Release workflow was observed. Do not claim
   release artifacts are published until CI confirms that outcome.
