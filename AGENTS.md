# Mine Mail — Agent Contract

Mine Mail is a cross-platform desktop mail client built with Tauri 2, React, Rust,
and SQLite. This file contains repository-wide rules for coding agents. Keep it
short and durable.

## Source of truth

- Read this file before changing the repository.
- Read `DESIGN.md` before changing any visible UI, interaction, theme, copy
  hierarchy, asset, or layout. It is the only visual specification.
- Read the relevant section of `docs/PRODUCT.md` before changing user-visible
  behavior, synchronization, accounts, drafts, sending, notifications, contacts,
  or identity rules.
- Read `docs/MAIL_RENDERING.md` before changing MIME parsing, HTML sanitization,
  body render modes, remote images, or reply-history parsing.
- `README.md` is the human setup guide. `docs/RELEASE.md` is a mutable release
  checklist, not a product specification.
- Do not create nested `AGENTS.md` files that repeat these rules. Add a scoped
  instruction only when a subproject has a genuinely different build or safety
  constraint.

When documents conflict, this file controls agent behavior; the domain document
controls its named domain. Existing code or an old screenshot does not silently
override a documented decision. Durable product, architecture, or design changes
require the user's approval and an update to the corresponding canonical document.

## Architecture and safety invariants

- The product is a desktop application. The Vite browser build is only an
  explicit, no-network UI demo and test surface; never create a parallel Web mail
  runtime.
- Rust and SQLite own credentials, IMAP/SMTP, MIME processing, synchronization,
  drafts, Outbox state, and notification decisions. React calls narrow Tauri
  commands and renders local state.
- Preserve offline-first startup: render cached SQLite state immediately, then
  synchronize in Rust without replacing usable content with loading placeholders.
- Keep account data, caches, notification baselines, queued mutations, and
  synchronization state scoped by stable account ID.
- Never expose authorization secrets, passwords, OAuth tokens, complete RFC822
  messages, or unrestricted database/file/network access to React. The exact
  active or user-selected product-data directory is the sole complete-path
  exception, limited to the About and confirmed storage-migration flow required
  by `docs/PRODUCT.md`; mail, attachment, diagnostic, and managed-cache paths
  remain Rust-only.
- Never write mailbox addresses, subjects, message bodies, raw HTML/RFC822,
  credentials, tokens, or complete local paths to logs. Keep errors useful but
  privacy-safe.
- End-user mailbox passwords, authorization secrets, and OAuth access/refresh
  tokens belong in the OS credential store and Rust runtime, not SQLite,
  frontend state, repository files, or build configuration.
- Provider-issued desktop OAuth client metadata is a separate, ignored Rust-only
  build input. Never treat it as an end-user credential, expose it to React or
  logs, or commit production configuration.
- Treat mail content as untrusted. Sanitize in Rust, preserve low-confidence
  content, and isolate sender-controlled complex HTML without scripts.
- Operations that can be retried must be idempotent or explicitly model an
  uncertain outcome. Never turn an unknown SMTP result into an automatic resend.

## Product invariants

- Support at most three connected accounts. The interface has one active account,
  while startup, scheduled, tray, and manual synchronization cover every account.
- A missing, expired, or revoked credential stops network work only for that
  account; its cached mail remains readable and the UI offers reauthentication
  without repeated background-error notifications.
- Message stars are the IMAP `\Flagged` system flag. Local changes are immediate
  and remain queued until the server confirms the requested state.
- Draft edits use a stable draft ID plus SQLite `local_version`. Stale writes
  create conflict copies, stale deletes cannot remove a newer draft, and sending
  binds recipient confirmation and Outbox state to one exact version.
- First historical import establishes the notification baseline. Later unread
  arrivals may notify with sender identity/address, subject, and receiving account
  identity/address, but never body text.
- Contacts, favorites, remarks, account remarks, and avatar overrides are
  Mine Mail-local metadata. IMAP does not own them. A local remark or avatar
  override wins over sender/provider presentation without hiding the real address
  where identity must be clear.
- Never query a remote avatar service. Runtime avatar assets come from local
  overrides, the built-in known-domain map, or initials.

Detailed behavior belongs in `docs/PRODUCT.md`; do not expand this section with
screen-specific copy, timing constants, pixel values, or speculative future plans.

## Working rules

- Inspect the working tree before editing. Preserve user changes and unrelated
  work; do not reformat or rewrite files outside the task.
- Prefer existing Rust boundaries, React components, semantic tokens, and tests.
  Fix shared primitives instead of copying a nearby implementation.
- Keep Tauri commands narrow, typed, and privacy-safe. Validate inputs again at
  the Rust boundary even when React already validates them.
- Schema and persistent-state changes need migrations, backward-compatible reads
  where practical, and failure-path tests.
- User-visible failures must preserve recoverable local state and explain the next
  action. Do not use browser-native `alert`, `confirm`, `prompt`, validation
  bubbles, or native select styling in product UI.
- A durable behavior or design change is incomplete until its canonical document
  and relevant tests change with the implementation.
- Do not commit dated QA journals, AI discussion notes, generated comparison
  boards, screenshots, absolute local paths, or alternate “final” design files.
  Keep temporary evidence in the OS temporary directory and retain durable
  conclusions only in canonical docs and tests.

## Git commits

- Read `contributing.md` before creating a commit.
- Every agent-authored commit subject must exactly match
  `<type>: <中文说明>`, including amended, reverted, and merge commits.
- Everything after the colon must form a Chinese phrase. English is allowed only
  for an indivisible technical proper noun, protocol name, file name, code
  symbol, or version number embedded in an otherwise Chinese description.
  English sentences, clauses, and action phrases are forbidden.
- Use only the commit types defined in `contributing.md`: `feat`, `fix`, `ui`,
  `perf`, `refactor`, `docs`, `test`, `ci`, `build`, `chore`, `release`, and
  `revert`.
- Use a lowercase type, an ASCII colon, and one space. Never add a scope:
  `ui: 调整邮件列表头像比例` is valid; `fix(ui): align mail-list avatar
  proportions` is invalid.
- Existing English commit history is not a template and does not override these
  rules. Do not copy an English task title, branch name, or previous subject into
  a new commit message.
- Keep each commit focused on one primary intent. Before committing, inspect the
  staged diff and exclude unrelated user or agent changes.
- After committing, inspect the actual subject with `git log -1 --format=%s`. If
  it does not satisfy the Chinese format, amend it before handing off.

## Verification

Run the smallest relevant checks while iterating, then the applicable project
checks before handoff:

- Rust core: `cargo test`
- React: `cd web && npm test -- --run && npm run build`
- Tauri: `cd web/src-tauri && cargo test && cargo check`
- Documentation-only changes: `git diff --check` plus repository link/path checks

Tests that connect to a real mailbox, mutate server state, send mail, publish,
sign, or release are never part of the default verification set. Run them only
when the user explicitly requests that external action and supplies the intended
private test configuration.
