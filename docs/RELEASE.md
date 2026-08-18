# Mine Mail Release Gate

This is the living checklist for the next public release. It contains open
release decisions and gates only. Completed investigations and historical QA
belong in Git history.

Verify every item against the release commit, provider console, signing account,
CI configuration, and a clean target device before checking it.

## Release scope and ownership

- [ ] Choose the channel: invited beta, public beta, or stable.
- [ ] Declare supported operating systems, architectures, providers, and known
  limits consistently in the app, README, installer, and release notes.
- [ ] Confirm the publisher, OAuth brand, website, signing identity, support
  contact, and privacy/data-deletion pages identify the same release owner.
- [ ] Assign owners for signing keys, OAuth review, security reports, release
  approval, rollback, and user support.

## Provider, privacy, and security

- [ ] Separate development/test OAuth configuration from the production project
  and complete any required provider verification.
- [ ] Test account login, refresh, revocation, password changes, expired
  credentials, reauthentication, and removal for every supported provider.
- [ ] Review published privacy, terms, support, and data-deletion pages against
  the shipped defaults and data flow.
- [ ] Document local persistent data, retention/deletion controls, and the
  decision that the SQLite mail cache is not whole-database encrypted.
- [ ] Verify OS credential storage and local file permissions on every supported
  platform.
- [ ] Security-review MIME parsing, sanitizer/iframe/CSP boundaries, remote-image
  policy, safe links, TLS, draft conflicts, managed attachments, Outbox
  idempotency, and uncertain delivery.
- [ ] Scan representative logs and final artifacts for credentials, tokens,
  addresses, subjects, bodies, RFC822 source, private configuration, and complete
  local paths.
- [ ] Review dependency advisories and licenses; produce third-party notices and
  an SBOM or explicitly accept the remaining risk.

## Build, signing, and updates

- [ ] Pin and review release workflow dependencies and apply least-privilege
  permissions.
- [ ] Protect production signing and publishing with an independently approved
  environment.
- [ ] Make version consistency, tests, build/check, signing, signature
  verification, malware scanning, hashes, updater metadata, and draft-release
  creation one fail-closed pipeline.
- [ ] Sign and timestamp every shipped executable, installer, uninstaller, update
  payload, and bootstrap component with the durable publisher identity.
- [ ] Keep beta and stable update channels separate.
- [ ] Verify updater manifests use version-pinned public artifacts and point to
  the dedicated Windows updater payload, not the recommended first-install
  package.
- [ ] Test signed update success plus interruption, offline, invalid signature,
  disk-full, migration, skipped-version, rollback, and emergency withdrawal.
- [ ] Publish expected publisher identities and artifact SHA-256 values.

## Native-platform acceptance

- [ ] Windows: validate the declared minimum version and architecture, install,
  custom location, upgrade, uninstall, autostart, shortcuts, updater, non-admin
  use, Chinese paths/usernames, insufficient disk, and recovery.
- [ ] macOS: complete Developer ID signing, notarization, stapling, Apple Silicon
  testing on the declared minimum version, Keychain, tray, notification,
  autostart, sleep/wake, updater, and uninstall checks.
- [ ] Linux: validate DEB and AppImage on the declared x64 distributions,
  including package signing, secret storage, WebKitGTK, tray, notification,
  autostart, sleep/wake, updater, and uninstall behavior.
- [ ] Publish only platforms that passed their complete signing and native-device
  gate; keep other builds internal.

### Attachment Save As matrix

- [ ] Exercise Save As on the supported local and SMB filesystems for each
  platform. Verify collision suffixes, concurrent same-name saves, exact decoded
  bytes, and that existing files never change.
- [ ] Exercise permission loss, disk-full, removed media, cancellation, and
  interruption. Confirm no partial final name remains.
- [ ] Review the residual path-based race in
  `tempfile::NamedTempFile::persist_noclobber`. Mine Mail rejects a selected
  directory whose final component is a symlink or Windows reparse point, but it
  does not yet anchor staging and publication to an opened directory handle.
  Block release if the chosen threat model requires protection from hostile
  same-user directory replacement.

## Implementation stabilization gates

These are unresolved implementation findings from the pre-release whole-chain
review. The identifiers are stable discussion handles. An item remains open
until its behavior and scope have been agreed, the implementation has changed,
and the stated evidence has been verified. The close condition describes what
must be proven; it does not prescribe a particular refactor.

Priority here controls discussion order, not whether an item is a confirmed
functional defect. Frontend performance items must be reproduced in their named
usage scenario before changing visible behavior or interaction.

### P0 — synchronization and resource amplification

- [x] **SYNC-01 — Establish one owner for mailbox-list refreshes.** A summary
  synchronization emits start, batch-progress, and completion events; React
  reloads the mailbox for every event, while manual synchronization and
  post-delivery reconciliation reload it again after the command returns. In an
  initial import or preview backfill this can turn one synchronization into
  dozens of duplicate list reads and prefetch reschedules.

  **Implemented.** Preserve intentional progressive
  display for the currently visible account and mailbox: after each summary
  batch has been durably persisted (normally ten messages, plus a final partial
  batch), that visible projection may request a list update. Start and zero-item
  progress events update counters only. Non-visible roles and inactive accounts
  are marked dirty and read from SQLite when opened instead of being refreshed
  in the background.

  Route progressive requests through one frontend coordinator keyed by account,
  role, and visible projection. Allow only one read in flight; events arriving
  during that read set one dirty bit, so completion reads the newest snapshot
  rather than queuing obsolete ten-message snapshots. Intermediate reads update
  only list rows and progress. They do not refresh Contacts or Outbox, reload
  other roles, or reschedule unrelated body-prefetch work.

  Emit one terminal success or failure for each account/role operation. Success
  performs one authoritative refresh for final flags, removals, mailbox epoch,
  derived views, and required Contacts/Outbox state. Remove the duplicate generic
  mailbox event and command-return reload when a specialized terminal event owns
  the same change. Failure exits the syncing state without reading the list.
  Opening Archive or Trash still performs the intentional two-stage sequence of
  one immediate cached read followed by one synchronized authoritative read.

  Close only with tests proving that start/zero progress performs no list read,
  persisted batches progressively update only a visible projection with
  single-flight coalescing, an inactive view performs no immediate read, and one
  operation has only one authoritative terminal refresh. Evidence:
  `SUMMARY_BATCH_SIZE` and progress callbacks in
  [`src/backend.rs`](../src/backend.rs), event emission in
  [`web/src-tauri/src/desktop/mod.rs`](../web/src-tauri/src/desktop/mod.rs), and
  `handleMailboxUpdate`, manual synchronization, and
  `reconcileSentAfterDelivery` in [`web/src/App.jsx`](../web/src/App.jsx).

- [x] **DATA-01 — Bound persistent mailbox-page cursor state.** Every
  non-terminal mailbox page currently inserts a new row in
  `message_page_cursors`. Cursors older than 24 hours are deleted, but an
  identical cursor is not reused, a cursor superseded by React remains until
  expiry, and there is no per-account hard limit. This creates short-lived
  garbage and turns otherwise local list reads into repeated SQLite/WAL writes.

  **Implemented.** Keep SQLite cursor storage for now
  because it is the existing privacy-safe bridge between the separate local and
  network `MailBackend` instances. Reuse the existing opaque token when the full
  validated cursor payload is identical. Keep the current 24-hour TTL and add a
  hard limit of 128 cursor rows per account, pruning the oldest rows whenever a
  genuinely new cursor would exceed that limit. Account removal continues to
  clear its cursors through account-scoped cascade behavior.

  React continues to receive only an opaque UUID. Account, role, provider
  mailbox, mailbox epoch, normalized query, Starred scope, keyset boundary, and
  remote-history coordinates remain Rust/SQLite-only and must all match before a
  cursor is accepted. An expired or pruned cursor returns a typed stale-cursor
  result; React keeps already visible rows and rebuilds pagination from the
  current cached first page instead of clearing the list or presenting a network
  failure. Reconsider moving cursors to shared runtime memory only after
  `ARCH-01` decides whether local and network backends remain separate.

  Close with migration and failure-path tests proving identical-payload reuse,
  the 128-row per-account cap, 24-hour expiry, cross-account/role/query/epoch
  rejection, account-removal cleanup, stable pagination across newer arrivals,
  and visible-row preservation after stale-cursor recovery. Evidence:
  `list_mailbox_page_filtered`, `issue_message_cursor`, and cursor validation in
  [`src/database.rs`](../src/database.rs), plus local/network continuation in
  [`web/src-tauri/src/mailbox_api.rs`](../web/src-tauri/src/mailbox_api.rs).

- [x] **CONTACT-01 — Eliminate all-account, all-message contact rescans from
  routine refresh paths.** `list_contacts` currently rebuilds activity for every
  configured account by loading every cached message header and aggregating it in
  Rust. Inbox/Sent invalidation can invoke this work even when Contacts is not
  visible, and AI turn preparation repeats it before contacting the provider.
  **Implemented.** Contact activity is rebuildable
  derived data maintained incrementally from `message_contact_emails`: add an
  account-and-normalized-email keyed activity summary plus a persistent dirty-email
  set. Message/contact membership changes mark only the affected addresses dirty;
  bounded maintenance then authoritatively recomputes those addresses rather than
  applying fragile count deltas. A full rebuild is reserved for migration and
  explicit repair. The active account reads its complete activity summary, while
  other accounts contribute activity only for app-wide favorite addresses instead
  of rebuilding their complete directories. Frontend mailbox changes invalidate
  contact data lazily and only load it when its consumer needs it; AI preparation
  reads the same summary rather than scanning messages. Preserve the existing
  semantics for excluding Bcc and the account's own address, deduplicating one
  address per message, selecting the newest non-empty display name, applying local
  remarks, exposing the real address, cross-account favorites, deletion fallback,
  and moves without double-counting. Close with migration/repair coverage and
  tests proving routine reads scale with affected contacts rather than all cached
  messages. Evidence: `list_contacts` and `prepare_ai_execution_context` in
  [`web/src-tauri/src/lib.rs`](../web/src-tauri/src/lib.rs),
  `list_contact_activity` in [`src/backend.rs`](../src/backend.rs), and
  `list_contact_source_messages` plus `message_contact_emails` in
  [`src/database.rs`](../src/database.rs).

- [x] **MUTATION-01 — Coalesce queued message-state network work.** Each seen,
  flagged, move, archive, trash, or delete action can spawn its own flush, emit
  one or two mailbox-update events, and enqueue a `message_mutation` full-sync
  request. Seen/flagged flushes open and select an IMAP mailbox before proving
  that pending work still exists, so rapid triage can create several serialized
  connections after an earlier worker has already drained the queue.
  **Implemented.** Per-action flush tasks are replaced with
  one asynchronous, single-flight mutation worker per account; accounts remain
  independent. Commands still durably record local intent and update the visible
  state immediately, then only wake the worker. The worker uses a short bounded
  coalescing window of at most roughly 100 ms, proves that work exists before
  connecting, and records one further drain when work arrives while it is active
  instead of spawning another worker. Coalesce idempotent desired-state changes
  such as seen and flagged by mailbox and final state. Preserve ordering and the
  existing durable phase/reconciliation rules for move, archive, trash, and
  delete; an uncertain server outcome must never become a blind retry. Reuse a
  bounded account connection/drain where practical, aggregate affected roles,
  and emit at most one update per changed role after a drain. Do not request a
  generic all-account sync after each successful action. Failures retain durable
  local intent and use bounded, account-targeted recovery; startup, manual,
  scheduled, and network-recovery synchronization wake the owning worker once
  without duplicating flush passes. The coalescing window must never delay the
  local response, an immediately opened destination mailbox must reflect the
  local mutation, remote stale state must not overwrite unconfirmed local intent,
  and closing/restarting during the window must not lose work. Close with tests
  covering rapid actions, work arriving during a drain, no-work wakeups, changed
  and unchanged event counts, failures and uncertain moves, restart recovery,
  protection from stale remote state, and independence between accounts.
  Evidence: the `schedule_*_flush` functions in
  [`web/src-tauri/src/mailbox_api.rs`](../web/src-tauri/src/mailbox_api.rs) and
  `flush_pending_system_flag_mutations` in
  [`src/backend.rs`](../src/backend.rs).

- [x] **AUTH-01 — Keep cached-message opening independent of unrelated OAuth
  accounts.** Previously, `fetch_mailbox_message` refreshed every Google backend
  before checking whether the selected message body was already complete in
  SQLite, so opening a cached 163/QQ message could wait for keyring or token
  refresh work belonging to another account. **Implemented and verified.** The
  owning account's SQLite body is now the first read path:
  when its cached body is complete, it returns immediately without consulting the
  credential store, refreshing OAuth, or opening a network connection, including
  when the owning Google authorization is expired or revoked. The path preserves
  existing cached MIME/attachment indexing, inline-image repair, sanitization,
  and rendering behavior. Only a missing or incomplete body refreshes credentials,
  and then only for the owning account via the account-scoped refresh API before
  attempting its network backend; unrelated Google accounts do not enter the
  reader critical path. Network failure retains any usable local state and
  returns the existing safe, actionable failure. Tests prove that cached
  non-Google and Google messages remain readable when unrelated credentials or
  the owning authorization are unavailable, and that a cache miss refreshes only
  its owning account. Evidence: `fetch_mailbox_message` in
  [`web/src-tauri/src/mailbox_api.rs`](../web/src-tauri/src/mailbox_api.rs),
  `refresh_oauth_backends` in
  [`web/src-tauri/src/account.rs`](../web/src-tauri/src/account.rs), and the
  cached-body fast path in [`src/backend.rs`](../src/backend.rs).

- [x] **DATA-02 — Persist fetched summary batches in bounded transactions.** IMAP
  summary fetches were batched, but each returned message called an upsert that
  opened its own SQLite connection and autocommitted several statements; Gmail
  identity persistence added another per-message operation. Initial import and
  preview backfill could therefore create hundreds of connections and commits.
  **Implemented and verified.** One repository batch API is now shared by
  ordinary incremental synchronization, initial history import, preview
  backfill, and Gmail History reconciliation. The existing network batch bound
  remains unchanged (normally ten summaries), while each returned batch is
  persisted through one SQLite connection and one immediate
  transaction. That transaction upserts the mailbox once as needed, all message
  summaries, Gmail stable-ID mappings, and related derived-index work; prepared
  statements are reused within the connection. Stable public identities and
  already-fetched bodies and attachment data remain preserved, while pending
  local Seen and Flagged intent is merged into every incoming server summary.
  The commit completes before batch progress is emitted, so a reported
  ten-message batch is fully readable. Any row or related mapping failure rolls
  the whole batch back instead of exposing a partially persisted success.
  Transactions are capped at 50 summaries and do not encompass a complete
  mailbox; network and
  full-versus-incremental synchronization semantics are unchanged. Tests cover
  bounded connection/commit counts, complete rollback on an injected
  mid-batch failure, Gmail mapping/rebinding, preservation of cached bodies and
  pending flags, derived-index consistency, and progress only after commit.
  Evidence: `fetch_and_cache_summaries` and
  `fetch_and_cache_required_summaries` in
  [`src/backend.rs`](../src/backend.rs), and
  `upsert_message_summary_batch` in
  [`src/database.rs`](../src/database.rs).

- [x] **SYNC-02 — Preserve explicit full-Inbox refresh semantics when an
  incremental sync is already running.** The desktop Inbox single-flight was
  keyed only by account, not by synchronization strength. A focus/IDLE
  incremental sync could therefore cause a user-triggered full refresh to join
  and return the incremental report without completing deletion and older-flag
  reconciliation. **Implemented and verified.** Each per-account Inbox flight is
  now mode-aware with ordered strength (`incremental` less than `full
  reconciliation`). An incremental request may join either running mode, and any
  request may join a running full reconciliation. When a full request arrives
  during an incremental run, the flight records one pending full pass instead of
  cancelling partially persisted work; after the incremental run settles,
  exactly one full reconciliation executes for all accumulated stronger
  requests. The full caller waits through that pass and receives its full result,
  never an incremental result presented as success. Multiple stronger requests
  coalesce, accounts remain independent, and an incremental failure does not
  satisfy or discard an already-requested full attempt. SYNC-01 progress and
  terminal events remain owned by each actual pass. Deterministic race tests
  cover incremental followed by one or many full requests, full followed by
  weaker requests, failure propagation, exact pass counts, requested-strength
  result matching, and account isolation. Evidence: `coordinate_inbox_sync`,
  `perform_inbox_mailbox_sync`, and `sync_inbox_with_operation` in
  [`web/src-tauri/src/desktop/mod.rs`](../web/src-tauri/src/desktop/mod.rs).

- [x] **SYNC-03 — Prevent one slow account from unnecessarily blocking other
  accounts.** The background loop executes one request at a time and
  `perform_sync_all` processes accounts sequentially, although Inbox, Sent, and
  Drafts run concurrently inside one account. With three accounts, a slow or
  offline secondary account can delay later work and the completion of a manual
  all-account refresh. **Implemented and verified.** The background loop is now
  a non-blocking intake and coalescing dispatcher: one pending full-account pass
  supersedes redundant scheduled Inbox and Draft work, while incremental Inbox
  notifications run through the existing strength-aware account coordinator.
  Full, periodic Inbox, and Draft batches use independent account pipelines with
  an application-wide two-account network cap and active-account-first admission.
  Each owning pipeline performs its credential refresh, mutation drain, core
  roles, and optional roles; one account's ordinary failure or bounded timeout is
  aggregated only after the other admitted pipelines settle and never cancels
  them. Explicit all-account refresh therefore retains a deterministic aggregate
  result while account events remain available as each pipeline progresses.
  Per-account lifecycle read access now blocks removal only for its owning
  account; removal obtains that account's exclusive access, rechecks prevent
  queued work from acting on a removed account, and unrelated account pipelines
  remain available. Tests cover request coalescing and priority, the two-account
  cap, a stalled secondary beside ready work, deterministic partial-failure
  aggregation, active-account ordering, per-account batch single-flight, Inbox
  strength joining, and target-only removal waiting. Evidence:
  `start_background_loop`, `run_bounded_account_pipelines`,
  `perform_sync_all`, and account lifecycle coordination in
  [`web/src-tauri/src/desktop/mod.rs`](../web/src-tauri/src/desktop/mod.rs), plus
  account removal in [`web/src-tauri/src/lib.rs`](../web/src-tauri/src/lib.rs).

### P1 — architecture and scaling risks

- [ ] **DRAFT-01 — Avoid downloading every complete remote draft on each
  periodic reconciliation.** The five-minute draft cycle searches every
  undeleted UID and fetches `BODY.PEEK[]` for every draft, including attachment
  bytes, even when nothing changed. Close only after unchanged drafts have a
  bounded metadata-only cost and new or externally changed MIME drafts still
  import completely with the existing identity and conflict guarantees.
  Evidence: `DRAFT_SYNC_INTERVAL` and draft scheduling in
  [`web/src-tauri/src/desktop/mod.rs`](../web/src-tauri/src/desktop/mod.rs),
  `fetch_draft_snapshot` in
  [`src/imap_client.rs`](../src/imap_client.rs), and
  `sync_drafts_with_progress` in [`src/backend.rs`](../src/backend.rs).

- [ ] **PREFETCH-01 — Scope body prefetch to the visible or explicitly likely
  mailbox.** Startup loads every available role for every inactive account. Each
  page response replaces one account-wide `current_page` prefetch generation, so
  a late Archive or Trash response can become the active prefetch set and compete
  with the Inbox the user is reading. Close only after inactive-account and
  non-visible-role work is demonstrably opportunistic, cannot displace selected
  or visible messages, and stays within a documented per-account/network budget.
  Evidence: `loadAccountView` and `prefetchAccountViews` in
  [`web/src/App.jsx`](../web/src/App.jsx), `page_with_prefetch` in
  [`web/src-tauri/src/mailbox_api.rs`](../web/src-tauri/src/mailbox_api.rs), and
  `schedule_page_body_prefetch` in [`src/backend.rs`](../src/backend.rs).

- [ ] **ARCH-01 — Decide whether local and network access require two complete
  `MailBackend` instances.** Every configured account keeps `local` and optional
  `network` backends pointing to the same database. Both own a Repository,
  attachment store, IMAP gates and sessions, prefetch queue, SMTP gate, and other
  runtime state; startup and OAuth refresh reopen the network instance. Close
  after either sharing the local persistence service/runtime state or documenting
  and verifying why duplicated initialization and state are required. Evidence:
  `BackendAccountSlots`, `AccountRuntime::open`, and backend open helpers in
  [`web/src-tauri/src/account.rs`](../web/src-tauri/src/account.rs), and
  `MailBackend::open` in [`src/backend.rs`](../src/backend.rs).

- [ ] **DATA-03 — Keep complete cached bodies out of mailbox-summary result
  projections.** `MESSAGE_SUMMARY_COLUMNS` and its aliased form select full
  `body_text`, although `MessageSummaryDto` deliberately does not serialize a
  body. Large cached plain bodies are therefore loaded and allocated during
  ordinary page refreshes and then discarded. Close only after summary queries
  retain body availability metadata and search behavior without materializing
  full body content into list DTO construction. Evidence: summary column
  constants and page candidate queries in
  [`src/database.rs`](../src/database.rs), and `MessageSummaryDto` in
  [`web/src-tauri/src/mailbox_api.rs`](../web/src-tauri/src/mailbox_api.rs).

- [ ] **SEARCH-01 — Measure and bound local search over complete cached bodies.**
  Search currently applies `lower(...) LIKE '%query%'` to several JSON/text
  columns and full `body_text` with no full-text index. The cost can approach a
  full scan for each debounced query, and Starred expands work across several
  roles. Close after a representative large-mailbox fixture proves acceptable
  latency and CPU use, or after the search index/query path is changed without
  weakening the documented local-only search scope. Evidence: regular and
  pending page candidate queries in
  [`src/database.rs`](../src/database.rs) and mailbox search dispatch in
  [`web/src/App.jsx`](../web/src/App.jsx).

- [ ] **UI-01 — Profile long-session body retention and compose-time mail-list
  rendering before changing frontend behavior.** `messageBodyCacheRef` is an
  unbounded `Map` cleared mainly during account reconfiguration, while opened
  bodies are also merged into retained account views. Compose body changes
  publish top-level `App` state every 160 ms and `MailList` maps all mounted rows.
  Reproduce with a long-running session, many opened bodies, a deeply scrolled
  list, and active compose typing. Close only with recorded heap/render evidence
  and a bounded result; do not infer a visual or interaction redesign from the
  static finding alone. Evidence: body cache and compose publication in
  [`web/src/App.jsx`](../web/src/App.jsx), and list rendering in
  [`web/src/components/MailList.jsx`](../web/src/components/MailList.jsx).

- [ ] **SETTINGS-01 — Reset synchronization deadlines only for scheduling
  changes.** Every desktop settings update emits `ScheduleChanged`, so changing
  remote images, notifications, AI defaults, poetry, or MCP permissions postpones
  the next Inbox reconciliation by a full poll interval. Close only after
  unrelated settings preserve the existing deadline and poll-interval changes
  still take effect immediately. Evidence: `DesktopRuntime::update_settings` and
  `BackgroundRequest::ScheduleChanged` handling in
  [`web/src-tauri/src/desktop/mod.rs`](../web/src-tauri/src/desktop/mod.rs).

- [ ] **QUALITY-01 — Establish a clean or explicitly accepted strict Clippy
  baseline.** `cargo clippy --all-targets -- -D warnings` currently fails on
  structural warnings including argument count, type complexity, and a large
  enum variant. These warnings are not treated as production defects by
  themselves, but they obscure whether later cleanup introduces new warnings.
  Close after the current warnings are resolved or a narrow, reviewed baseline
  is encoded without globally suppressing future findings.

## Release-candidate acceptance

- [ ] Run all applicable checks from `../AGENTS.md` on the exact release commit.
- [ ] Test clean install, first launch, every supported account type, initial
  synchronization, close-to-tray, and reopen.
- [ ] Test three-account startup/manual/tray/scheduled sync, switching, credential
  failure, notification baselines, and account removal.
- [ ] Test online/offline startup, network loss/recovery, DNS/TLS failure,
  sleep/wake, and system-time changes.
- [ ] Test list history, local search, all mail render modes, remote images,
  reply/forward, attachments, stars, mailbox actions, contacts, remarks, and
  avatars.
- [ ] Test new/existing drafts, periodic remote draft sync, version conflicts,
  read-only MIME, recipient binding, confirmed send failure, uncertain delivery,
  and explicit retry.
- [ ] Test keyboard navigation, visible focus, reduced motion, high DPI, zoom,
  long identity/content, Chinese text, and narrow desktop reflows in every theme.
- [ ] Re-download public artifacts, verify hashes/signatures, install, update, and
  confirm release notes list features, migrations, rollback guidance, and known
  limitations accurately.
