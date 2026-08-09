---
name: release-mine-mail
description: Release a Mine Mail version from main by checking the repository, synchronizing canonical versions, running verification, writing Chinese GitHub Release notes with platform-specific download guidance, creating a compliant commit and annotated tag, atomically pushing main and the tag, and verifying remote refs and published artifacts. Use when the user explicitly asks to publish or release Mine Mail, bump to a vX.Y.Z version and push/tag it, or invokes $release-mine-mail. Do not use to rewrite an existing release/tag, publish another repository, or merely prepare changes without pushing.
---

# Release Mine Mail

Publish one explicit Mine Mail version with complete release notes and verified
platform artifacts, without force-pushing, rewriting tags, or mixing unrelated
work.

## Required input and authority

- Require an explicit target matching `vX.Y.Z` or `vX.Y.Z-prerelease`.
- Treat an explicit request to release or invocation of this skill with a target
  version as authority to edit the version, commit, tag, and push.
- If the user asks only to check or prepare a release, stop before the first
  unrequested commit, tag, or push.
- Read repository `AGENTS.md`, `contributing.md`, `.github/workflows/release.yml`,
  and the relevant open items in `docs/RELEASE.md` before changing files.
- Require an authenticated way to read and edit this repository's GitHub Release
  before pushing the tag. Prefer `gh`; an equivalent GitHub API path is valid.
- Work only on Mine Mail's `main` branch and `origin` remote. Never force-push,
  move/delete an existing tag, rewrite history, or open a PR for this workflow.

## Workflow

### 1. Preflight the repository

1. Inspect `git status -sb`, the current branch, `origin`, recent commits, and
   local tags.
2. Require a clean, understood worktree. If changes already exist, continue only
   when they are entirely part of this exact release and safe to preserve;
   otherwise stop and ask which changes belong.
3. Run `git fetch origin --prune --tags`.
4. Compare `main` with `origin/main`.
   - Fast-forward a clean behind-only branch with `git merge --ff-only
     origin/main`.
   - Allow an ahead-only branch after inspecting and reporting the commits that
     the release push will include.
   - Stop on divergence. Do not rebase, merge, reset, or force-push implicitly.
5. Confirm the target tag is absent locally and remotely. Stop if it exists.
6. Confirm the target version is greater than the current canonical version.

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

### 3. Draft the GitHub Release notes

1. Find the previous semantic-version release tag and inspect its range through
   the current release candidate. Summarize user-visible features, fixes, and
   important compatibility changes in concise Simplified Chinese; omit merge
   mechanics, release-only version edits, and internal churn with no user impact.
2. Write the Markdown to a UTF-8 file in the operating-system temporary
   directory, never into the repository. Use this structure:

```text
## 本次更新

- <面向用户的变更>

## 下载与系统要求

- **Windows 11 x64（AMD/Intel 64 位，推荐首次安装）**：`Mine-Mail_X.Y.Z_windows-x64-installer.exe`
- **Windows 11 x64（应用内自动更新专用）**：`Mine-Mail_X.Y.Z_windows-x64-updater.exe`；普通用户无需手动下载或运行
- **macOS 14+ Apple Silicon（首次安装）**：`Mine.Mail_X.Y.Z_aarch64.dmg`
- **macOS 14+ Apple Silicon（应用内自动更新专用）**：`Mine.Mail_X.Y.Z_aarch64.app.tar.gz`；普通用户无需手动下载或解压
- **Linux x64**：Ubuntu/Debian 使用 `Mine.Mail_X.Y.Z_amd64.deb`，其他现代发行版可使用 `Mine.Mail_X.Y.Z_amd64.AppImage`

## 应用内更新

已安装 Mine Mail 的用户可在设置中检查更新。确认后由 Tauri 下载已签名的专用更新包，下载完成后自动安装并重启。
```

3. Replace `X.Y.Z` with the exact target version. Do not claim unsupported
   systems, signing, notarization, or fixes that were not verified.
4. Keep this temporary notes file until the published GitHub Release body has
   been updated and read back successfully.

### 4. Verify the exact release tree

Run all repository release checks after the version change:

```text
cargo test
cd web && npm test -- --run && npm run build && npm run verify:production-bundle
cd web/src-tauri && cargo test && cargo check
cd installer/windows && npm test
```

Adapt command separators to the active shell. Independent checks may run in
parallel when their output and exit status remain attributable. If any check
fails, do not commit, tag, or push; preserve the version diff and report the
failure.

### 5. Commit the release

1. Stage only the seven expected version files with explicit paths.
2. Inspect `git diff --cached --check`, the staged stat, names, and full staged
   diff.
3. Commit with the exact subject:

```text
release: 发布 vX.Y.Z
```

4. Run `git log -1 --format=%s` and ensure the actual subject satisfies
   `contributing.md`.

### 6. Create and publish the tag

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

### 7. Verify artifacts, publish notes, and report

1. Confirm `git status -sb` is clean and tracks `origin/main`.
2. Use `git ls-remote` to confirm remote `main`, the annotated tag object, and
   the peeled tag commit. Require remote `main` and the peeled tag to equal the
   local release commit.
3. Observe the tag-triggered Release workflow. Do not claim publication until it
   succeeds and the GitHub Release is public.
4. Inspect the public asset names. Require both distinctly named Windows files,
   every documented macOS/Linux file, and `latest.json`. Read `latest.json` and
   require both Windows platform entries to reference
   `Mine-Mail_X.Y.Z_windows-x64-updater.exe`, never the branded installer.
5. After CI can no longer overwrite the body, update the GitHub Release with the
   temporary notes file, for example:

```text
gh release edit vX.Y.Z --repo Tantless/mine-mail --notes-file <temporary-notes-file>
```

6. Read the Release body back and require the exact installer and updater names,
   the Windows recommendation, all supported-system labels, and the curated
   change summary. Remove the temporary file after successful verification.
7. Report the version, commit hash and subject, branch/tag push result, checks,
   clean status, workflow result, verified asset matrix, updater target, and
   Release-notes result. Treat missing artifacts, an incorrect updater URL, or
   an unwritten/unverified Release body as an incomplete release.
