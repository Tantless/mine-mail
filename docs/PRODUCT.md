# Mine Mail Product Contracts

This document contains durable user-visible behavior. It complements the
repository-wide constraints in `../AGENTS.md` and the visual rules in
`../DESIGN.md`.

Change these contracts only with user approval. Timing values and exact labels
must be updated here when an intentional product change lands.

## Product boundary

- Mine Mail is a local-first desktop mail client, not a hosted mail service.
- The desktop app connects directly from the user's device to the configured
  provider. Mine Mail does not add a server-side mailbox proxy.
- The Vite demo uses explicit mock data and performs no real mail, credential,
  tray, notification, or operating-system work.
- SQLite is the immediate local source for renderable mailbox state. The server
  remains authoritative for remote messages and system flags once synchronization
  confirms them.

## Accounts and identity

- A user may connect at most three accounts.
- One account is active in the interface at a time. Startup, scheduled, manual,
  and tray synchronization still cover every connected account.
- Account caches, sync cursors, notification baselines, drafts, Outbox items, and
  queued remote mutations are isolated by stable account ID.
- Provider choices shown when connecting a new account are 163, Gmail, and
  custom IMAP/SMTP. Outlook is hidden until its Microsoft OAuth 2.0 / Modern
  Auth path is implemented; the formal interface never shows an unavailable or
  “coming soon” Outlook provider card.
- A legacy local Outlook account record remains readable for cached mail only.
  Mine Mail preserves its metadata and explains that reconnecting is unsupported,
  but it cannot create a new Outlook account or start password-based network work
  for that record.
- Password-based providers use the mailbox address plus a provider-issued
  authorization secret. Gmail uses Google OAuth 2.0 Authorization Code + PKCE in
  the system browser with a random loopback callback, then XOAUTH2 for IMAP and
  SMTP.
- Access and refresh tokens stay in the OS credential store and Rust runtime.
- A missing, expired, or revoked credential stops synchronization and monitoring
  for only that account. Cached mail stays readable.
- The affected account shows a red attention mark in the account switcher and the
  exact Settings status **凭证失效**, with hover/focus help directing the user to
  sign in again or obtain a new authorization credential. Do not repeat
  background-error toasts for this state.
- An account may have a Mine Mail-local remark keyed by stable account ID. A
  non-empty remark is the primary display label wherever account provenance is
  shown; retain the real mailbox address in supporting identity text.

## Account removal

- Account removal is an explicit, confirmed operation and is unavailable while
  that account retains a live compose session.
- Removing a password-based account deletes its OS credential and Mine Mail
  account record. Cached SQLite mail, drafts, and Outbox data remain unless the
  user separately selects **同时删除本地邮件缓存**.
- A Gmail account offers two distinct operations:
  - **仅断开** removes Mine Mail's local connection and OS token while retaining
    the Google authorization grant and local cache;
  - **撤销授权并移除** first asks Google to revoke Mine Mail's OAuth grant, then
    removes the OS token and local account record.
- The optional local-cache deletion removes that account's SQLite mail, drafts,
  managed draft attachments, and Outbox data and is irreversible. It does not
  happen implicitly. Retaining the cache retains every managed attachment still
  referenced by a retained draft or Outbox item.
- A failed or partially completed removal must preserve every recoverable state,
  report exactly what remains, and never present partial cleanup as full success.

## Startup, synchronization, and background lifecycle

- Startup renders cached SQLite state first and starts Rust synchronization
  afterward.
- Immediate synchronization is triggered by startup, explicit in-app refresh,
  tray **刷新**, and supported resume/wake events.
- Runtime capability detection prefers standard IMAP IDLE. Reconnect before the
  server's 29-minute window expires.
- A server that does not advertise IDLE keeps an authenticated connection and
  probes lightweight mailbox counters every 15 seconds while the app is visible
  or every 30 seconds while hidden. Fetch new UIDs only after a detected change.
- A user-selectable 1/3/5-minute interval, default 5 minutes, performs full
  reconciliation for deletions, flags, and recovery. This is separate from the
  lightweight arrival monitor.
- Startup, explicit in-app or tray refresh, and periodic reconciliation operate
  across every network-ready account. Startup and explicit refresh discover
  mailbox roles and perform bounded summary synchronization for Inbox, Sent,
  Drafts, Archive, and Trash when those roles are available; they do not download
  every body or attachment.
- IMAP IDLE and lightweight arrival polling monitor Inbox only. A periodic
  reconciliation always flushes queued mutations; Archive and Trash participate
  when they have been initialized locally or have pending mutations.
- Opening Archive or Trash paints its SQLite summaries first and then performs a
  bounded foreground synchronization for that role. Mine Mail never empties
  Trash automatically; provider retention policy remains authoritative.
- A background refresh must not replace already rendered messages, contacts, or
  correspondence with a loading placeholder.
- When background mode is enabled, closing the main window hides it to the tray.
  Tray labels are exactly **打开 / 刷新 / 退出**.
- Login autostart is one setting and defaults off.

## Mail list, bodies, and remote content

- Inbox summaries contain only bounded list/preview data, never raw RFC822,
  complete HTML, or an unrestricted body payload.
- Cached address metadata retains Bcc only when the parsed RFC822 source contains
  an actual `Bcc` header. Legacy rows and messages without that header use an
  empty Bcc list; Mine Mail never infers it from the receiving account,
  `Delivered-To`, another transport header, or an SMTP envelope.
- Synchronization derives bounded list previews without requiring the user to
  select each message. Preview readiness is independent from full-body caching.
- The primary sidebar shows numeric badges only for unread Inbox messages and
  Outbox items not yet confirmed sent. Zero counts are omitted. Starred,
  Contacts, Sent, Drafts, Archive, and Trash never show numeric badges.
- Selecting a message paints its cached preview immediately and hydrates the body
  silently. A selected message uses a foreground fetch lane that outranks queued
  prefetch work; queued neighbors are promoted, and an identical in-flight
  download is shared instead of requested twice.
- Returning a mailbox page schedules its uncached bodies as background candidates
  in visible order. The default 50-message page may cache at most 16 MiB and skips
  individual messages larger than 2 MiB; synchronization may additionally
  schedule the 20 most recent bounded candidates within 8 MiB. Loading another
  page cancels page work that has not started. These are opportunistic caches,
  not a guarantee that every listed body is downloaded.
- Full-body cache payloads have a 512 MiB device budget shared evenly across
  connected accounts. Least-recently-used bodies are evicted first while their
  list summaries and bounded previews remain available; selecting an evicted
  message downloads it again. Drafts, Outbox state, and explicitly managed
  attachment files are outside this eviction policy.
- Inbox, Sent, Archive, and Trash use opaque keyset pagination. The default page
  contains 50 messages and a caller may request at most 100. A cursor is bound to
  the account, mailbox role, UIDVALIDITY epoch, stable sort position, remote
  before-UID position, and search fingerprint; it cannot be reused for another
  account, folder, epoch, or query.
- A page distinguishes more SQLite history, more possible server history, an
  unavailable offline history request, and a confirmed end. Approaching the
  bottom automatically requests the next page of at most 50 messages. Loading
  uses one compact bottom line; completion with the appended count or failure is
  shown for two seconds and then removed. There is no manual load-more control or
  persistent end/empty-state explanation.
- New arrivals and background refreshes are merged by exact mailbox and UID
  without invalidating an older keyset cursor, duplicating rows, changing the
  current selection, or moving the visible scroll position. A UIDVALIDITY change
  invalidates cursors for that mailbox and starts a fresh bounded listing.
- Search is local-only in this version. It queries every synchronized SQLite
  summary in the active account and current folder, not only the React page, and
  matches subject, From, To, Cc, and bounded preview. Bcc is deliberately
  excluded. It does not search
  inconsistently cached full bodies or issue a remote IMAP search. The interface
  identifies the scope as **搜索已同步邮件**, and search results use the same
  keyset pagination.
- MIME parsing, HTML classification, sanitization, iframe isolation, reply
  segmentation, attachment extraction, forwarding, and remote-image boundaries
  follow `MAIL_RENDERING.md`.
- Remote-image policy is user-selectable: automatic, ask, or blocked. The default
  is automatic.
- The setting includes nearby help explaining that automatic requests can reveal
  open time, IP/device information, or activate tracking pixels.

## Stars and remote mutations

- A message star is the standard IMAP `\Flagged` system flag, not Mine Mail-only
  metadata.
- Star and unstar update SQLite immediately and persist offline.
- Remote Inbox and Sent messages synchronize `\Flagged` in both directions.
- A queued local mutation remains until the server confirms the requested final
  flag state. A transient failure cannot silently discard it.
- The **已收藏** workspace shows the complete starred aggregate without
  **全部 / 未读** filter tabs.
- Contact **收藏** is a separate local organization feature and must not be
  implemented with IMAP `\Flagged`.

## Mailbox roles, message actions, and convergence

- Mailbox roles are account-scoped and map to provider mailbox names only in
  Rust/SQLite. React works with semantic Inbox, Sent, Drafts, Archive, and Trash
  roles and does not guess localized IMAP names.
- Archive discovery first uses a selectable IMAP SPECIAL-USE `\Archive` mailbox.
  A provider with documented archive semantics, such as Gmail, uses its dedicated
  provider adapter. Mine Mail does not silently treat a similarly named ordinary
  mailbox as Archive.
- Gmail uses its selectable SPECIAL-USE `\All` mailbox only as the storage
  location for Archive actions. Archive synchronization and history pagination
  use Gmail's `in:archive` provider query while excluding Sent, Drafts, Spam,
  and Trash, so only dedicated archived messages appear instead of the raw
  contents of All Mail or another semantic mailbox.
- Archive remains a neutral folder entry while discovery is pending or creation
  has not been approved; absence of an optional Archive target is not a warning
  or account-health problem. Sidebar entries never expose capability discovery
  or queued-mutation status. Opening Archive first paints any cached summaries
  for a persisted available role; discovery, missing-role, empty, and failure
  states do not render explanatory cards in the list workspace.
- If no archive target exists, the first message Archive action uses a Mine Mail
  confirmation to offer creation of the fixed `Archive` mailbox. Accepting that
  confirmation creates and verifies the role, then continues the exact Archive
  action that prompted setup. The role is persisted only after CREATE followed
  by LIST confirms a selectable mailbox. Canceling creates no mailbox and leaves
  the message unchanged. Creation failure keeps Archive unavailable with a
  recoverable explanation; Archive never degrades into deletion.
- Trash follows the same rule using selectable SPECIAL-USE `\Trash`, with a
  one-time confirmed creation of `Trash` when absent. Canceling or failing that
  setup disables move-to-Trash for the account and never converts it to permanent
  deletion.
- **删除** in Inbox, Sent, or Archive means move to Trash. Only a message already
  in Trash offers permanent deletion, and every permanent deletion requires a
  Mine Mail confirmation. Draft and Outbox deletion keep their separate
  versioned lifecycle.
- Mark read and mark unread set the desired final state of the IMAP `\Seen`
  system flag. They are available for selectable remote mailboxes that permit
  persistent `\Seen` updates; Draft and Outbox do not expose them. Mark unread
  updates the local row immediately and continues synchronization silently. The
  reader does not show its pending state, and a pending automatic mark-read
  operation never disables or relabels the mark-unread action.
- Star, read/unread, Archive, move-to-Trash, and permanent-delete actions update
  SQLite and persist an account-scoped mutation before network work. Moving a
  message removes it from the source view immediately and presents it in the
  destination with a pending state. Restarting or going offline cannot lose that
  intent.
- Every queued mutation has an opaque operation ID, the exact source mailbox and
  UIDVALIDITY, source UID, optional destination role, a monotonic local revision,
  and one of `pending`, `in_flight`, `confirmed`, `needs_attention`, or
  `outcome_unknown`. Repeated actions collapse to the newest desired state, and
  an older network result cannot overwrite a newer local revision.
- Before executing a UID-scoped mutation, Rust reselects the mailbox and checks
  UIDVALIDITY. It never applies the same numeric UID in a different epoch.
  UIDVALIDITY mismatch stops the action, synchronizes the mailbox, and marks the
  operation `needs_attention` unless one unique strong identity match can be
  established. Message-ID by itself is not sufficient for a destructive rebind.
- If the source UID has disappeared or a MOVE/COPY outcome is unknown, Mine Mail
  reconciles source and destination before deciding. A uniquely proven destination
  confirms the action; an ambiguous external move or deletion stops automatic
  retry and explains that another client changed the message. A network timeout
  never causes a blind repeat that could copy or delete twice.
- Permanent deletion may retry only an exact UID in the same UIDVALIDITY epoch.
  Without UIDPLUS, Mine Mail must not use a global EXPUNGE that could remove
  unrelated messages; it may hide the locally confirmed `\Deleted` item while
  waiting for safe server cleanup.

## Attachments

- A fully hydrated received message exposes bounded attachment metadata: an
  opaque stable part ID, original name when present, safe display name, MIME type,
  exact decoded size, and disposition. A list summary never contains attachment
  bytes or raw message data.
- Saving one received attachment opens the platform Save As flow and is completed
  in Rust. React never receives the bytes, complete RFC822 source, an unrestricted
  read/write path, or the complete chosen path.
- Rust removes path separators and control characters, handles platform-reserved
  names and trailing dots/spaces, bounds the file name, and falls back to
  `attachment.bin`. An existing name is not overwritten; Mine Mail selects
  `name (1).ext`, `name (2).ext`, and so on.
- Attachment output is written to a new temporary file in the chosen directory
  and finalized only after the complete extraction succeeds. Cancellation is not
  an error. A failure removes partial output, preserves the open message, and
  offers retry. If full MIME is not cached, saving first hydrates the message and
  reports that requirement accurately.
- Adding the first attachment to a new composition creates a stable draft ID
  before selection. Rust copies each selected file into an account- and
  draft-scoped managed area inside the product-data directory; React retains only
  bounded metadata and an opaque attachment ID.
- Managed attachment blobs are immutable and reference-counted. Adding or removing
  an attachment is an optimistic draft edit that increments `local_version`.
  A conflict copy retains its exact attachment set, and a stale attachment action
  cannot change a newer canonical draft.
- A legacy managed-blob row without a content digest is never accepted through a
  length-only read. On its first MIME read, Rust may hash only the exact direct
  regular file in the active account's managed directory, after validating its
  stored base name and size with a bounded expected-size-plus-one read. SQLite
  binds that digest to the still-`NULL`, unchanged account/blob record with a
  one-time compare-and-set before the bytes are used. A missing, linked/reparse,
  changed, cross-account, or concurrently disagreeing blob remains untrusted and
  returns a recoverable integrity failure; every later read verifies the
  persisted SHA-256.
- Discarding a draft releases only its references. A blob remains while a draft,
  conflict copy, or immutable Outbox item still references it. Temporary and
  unreferenced files are cleaned without touching referenced content.
- Mine Mail-created plain or restricted rich-text drafts with managed ordinary
  attachments are editable. Externally created HTML, multipart, inline-image, or
  attachment-bearing drafts remain read-only until their MIME can be
  round-tripped without loss.

## Drafts and compose lifecycle

- Drafts synchronize in both directions.
- Editing reuses one stable draft ID. The editor saves locally while active and
  uploads the latest eligible version remotely every five minutes.
- Every editor write carries the SQLite `local_version`.
- A stale write becomes a conflict copy rather than overwriting the canonical
  draft. A stale delete never removes a newer canonical version.
- Adding or removing a managed attachment is an editor write and follows the same
  `local_version`, conflict-copy, and stale-delete rules.
- Font, size, emphasis, list, alignment, link, and clear-format edits are part of
  the same exact draft version as the plain authored body. Rust sanitizes the
  owned HTML fragment again at the desktop boundary; the plain body remains the
  interoperability fallback. A formatting command applies to the active range
  (or future input at a collapsed caret), preserves that range/caret after the
  command, and never changes the whole editor or stationery line rhythm merely
  because the toolbar's current font-size value changed. Moving a collapsed
  caret updates the font, size, emphasis, list, and alignment controls from the
  inherited format at that position; a mixed selection reports a mixed font or
  size rather than a stale toolbar value. A collapsed-caret font-size change
  updates both the caret presentation and the stored format used by the next
  input. Italic is persisted as semantic emphasis in the restricted HTML.
- At the start of a paragraph, typing `1.` followed by Space creates a real
  ordered list. Enter creates the next numbered item, and Enter again on an empty
  item exits to an ordinary paragraph. The plain-text fallback emits explicit
  numeric markers so the authored structure remains understandable in clients
  that do not render HTML.
- At the start of an ordinary paragraph, Tab applies a semantic first-line
  indent and remains inside the editor; Shift+Tab removes it. Enter inherits the
  current paragraph's indent for the next paragraph. Plain and lined editing use
  a 2 em stop, approximately two Han characters at the base compose size; grid
  editing renders the same semantic indent as exactly two cells and keeps wrapped
  rows aligned to the grid. The restricted HTML stores only this fixed indent
  token and its canonical `text-indent:2em` representation; legacy Mine Mail
  `4em` values normalize to `2em`, and arbitrary inline styles remain unsupported.
- If the rich editor fails during lazy initialization or lifecycle reconnection,
  the compose surface stays open, preserves the current draft value, and offers
  an in-place retry instead of unmounting the application.
- A draft may select **无**, **横线纸**, or **方格纸**. The compact paper
  control defaults to off (**无**); enabling it restores the last paper type in
  the current compose session and exposes the **横线纸 / 方格纸** choice.
  **仅编辑** stores the selection but sends no paper decoration.
  **随信发送** is available only for a non-empty paper selection and asks Rust
  to wrap the sanitized authored HTML in the selected stationery when it builds
  the exact MIME version. Grid paper visually groups up to three consecutive
  Latin letters or numbers in one cell, centers each Han character in one cell,
  and gives every whitespace or special character an independent cell so an
  authored Space advances by one complete blank cell; these editing decorations
  never enter the authored HTML.
- Only Mine Mail-owned restricted rich drafts are editable. Private compose
  metadata and authored-boundary markers distinguish them from arbitrary
  sender-created HTML; missing or malformed ownership markers keep the draft
  read-only.
- **保存并最小化** is a local stabilization action, not a close action. When the
  current session has authored content, Mine Mail saves its exact local draft
  version before showing the standard minimized compose bar. A new untouched
  empty session minimizes without creating a draft. A local save failure leaves
  the expanded editor open.
- Closing the composer or pressing Escape never forces a save:
  - a new compose session removes any recovery draft created only by that session;
  - closing an existing draft leaves its previously persisted version intact;
  - minimizing keeps the current editing session alive.
- Each stable account ID owns at most one live compose session. Before switching
  accounts, Mine Mail saves any edited content to that account's local draft
  store and retains the session's in-memory editing state. The destination
  account has an independent compose session and sender identity. Returning to
  the source account restores its session as the minimized compose bar. A local
  save failure leaves the current account and editor in place instead of risking
  cross-account or unsaved state. Activating **写信** while the active account's
  session is minimized restores that session instead of creating another one.
- Compose window geometry and its visual behavior follow `../DESIGN.md`.

## Replies and forwarding

- Reply and forward preparation require a fully hydrated local message. A list
  preview is never treated as the complete quoted body; preparation hydrates the
  message first when network access is required and fails recoverably when that
  cannot be done.
- Forwarding is prepared in Rust as a stable draft with a structured immutable
  context containing original subject, From, To, Cc, time, trustworthy plain-text
  body, an optional Rust-sanitized HTML alternative, and opaque attachment
  references. It never invents or reveals Bcc.
- Reply preparation likewise excludes cached Bcc from both the authored
  recipients and immutable reply context.
- The compose editor contains only the user's new text. React does not concatenate
  identity headers, original HTML, or quoted content; Rust assembles the final
  plain and safe HTML MIME alternatives.
- A formatted authored body always sends a trustworthy `text/plain` alternative.
  The HTML alternative contains only Rust-sanitized editor formatting and, when
  explicitly selected, the stationery wrapper. It contains no remote image,
  script, form, or sender-controlled style payload.
- A forward includes all ordinary named original attachments by default and does
  not silently replace the original message with an `.eml` file. If any attachment
  cannot be prepared safely, preparation fails without omitting it. The reader
  remains intact without a body-level failure card, and the Forward control
  returns to a retryable state. The frontend does not offer an attachment-less
  fallback.
- Inline resources remain subject to the sanitizer and are not silently promoted
  to ordinary downloadable or forwarded attachments.

## Sending and Outbox

- Sending always presents exact-recipient confirmation.
- Recipient confirmation, draft state, and the created Outbox item bind to one
  exact `local_version`, including its exact managed attachment set.
- After confirmation, Rust constructs and persists one immutable MIME message and
  envelope in Outbox. Later body, recipient, or attachment edits cannot alter that
  attempt.
- Every newly created Outbox item also persists the exact authored To, Cc, and
  Bcc grouping from that immutable draft version alongside the flat SMTP envelope
  recipient list. Restart, recovery, and retry preserve both forms unchanged.
  Legacy Outbox items expose grouping as unavailable; Mine Mail never reconstructs
  it from the flat envelope or RFC822 headers.
- A send attempt preserves any newer editor changes.
- A newer safe attempt may supersede an older retryable Outbox item for the same
  version.
- Distinguish confirmed success, confirmed failure, retryable failure, and
  `delivery_unknown`.
- Never automatically retry `delivery_unknown`; the user must decide whether to
  risk a duplicate.
- An ambiguous attempt exposes only two explicit decisions. After checking the
  provider, the user may confirm that the message was delivered; Mine Mail then
  atomically marks that exact Outbox attempt sent and consumes only its exact
  draft version. This decision does not invent an SMTP delivery timestamp:
  `sent_at` remains absent unless it was already known, while the immutable MIME
  Date and Outbox creation time remain available for display fallback.
  Alternatively, the user may explicitly acknowledge the duplicate-delivery
  risk and request one manual retry.
- A duplicate-risk retry reuses the exact persisted RFC822 bytes and SMTP
  envelope; it never rebuilds MIME from the editable draft. Each decision is
  bound to the reviewed attempt generation so concurrent or repeated submission
  cannot create an extra retry. If the manual retry also ends in
  `delivery_unknown`, it remains blocked until the user reviews the new
  generation and decides again.

## Desktop mail API boundary

- React calls narrow Tauri commands and treats every mailbox cursor, message
  operation ID, deletion plan ID, attachment ID, and draft ID as opaque. Rust
  validates the active/configured account again and derives concrete mailbox names,
  UIDVALIDITY, UIDs, MIME parts, and managed paths.
- Every message exposed through this boundary uses an account-bound, randomly
  generated opaque `public_id` that remains stable across restarts and ordinary
  upserts. SQLite row IDs never cross into React; deleting a mailbox epoch after
  a UIDVALIDITY change and importing it again creates new message identities.
- `InboxMessage.bcc` is an address list sourced only from an actual cached
  RFC822 `Bcc` header. `OutboxItem.recipient_groups` is either
  `{ to, cc, bcc }` for a newly authored immutable send or absent for a legacy
  row; callers must not derive missing groups from `recipients` or MIME.
- Resolving `delivery_unknown` accepts an opaque Outbox ID, the reviewed
  `attempts` generation, and exactly `confirm_delivered` or `retry_once`.
  `retry_once` additionally requires `acknowledge_duplicate_risk: true`; Rust
  validates the account, state, generation, and acknowledgement again before
  changing Outbox state or entering SMTP.
- Stable mailbox and mutation enums use these exact snake-case values:
  - `MailboxCapabilityStatus` is `discovery_pending`, `available`,
    `needs_creation_confirmation`, or `unavailable`;
  - `MailboxCapabilityUnavailableReason` is `create_not_supported`,
    `create_failed`, `created_mailbox_not_selectable`, or
    `provider_unsupported`; it is present only for `unavailable`;
  - `RemoteHistoryState` is `not_checked`, `may_have_more`, `offline`,
    `complete`, or `unavailable`;
  - `MutationStatus` is `pending`, `in_flight`, `confirmed`,
    `needs_attention`, or `outcome_unknown`.
- `MailboxCapability` is
  `{ role, status, display_name?, unavailable_reason?, retryable }`.
  `discovery_pending` means no authoritative online discovery has completed;
  `needs_creation_confirmation` means the role is absent and the UI may offer
  its one-time creation confirmation. `retryable` is true only when repeating
  discovery or role creation can change the result.
- `MessagePage` is
  `{ items, next_cursor?, has_more_local, remote_history_state, end_reached }`.
  `not_checked` is used while more SQLite rows remain; `may_have_more` means a
  bounded server history request can continue; `offline` means the server could
  not be checked; `complete` is authoritative end-of-history; and `unavailable`
  means the role cannot supply remote history. `end_reached` is true only when
  `has_more_local` is false and `remote_history_state` is `complete`.
- `MessageMutationReceipt` is
  `{ operation_id, local_revision, status: MutationStatus, source_role,
  destination_role? }`. React must render `status` directly and never derive it
  from an error string.
- `AttachmentMeta` is
  `{ id, original_name?, safe_display_name, mime_type, size_bytes,
  disposition }`.
- `DraftAttachmentMeta` is
  `{ id, name, mime_type, size_bytes, source_attachment_id? }`. The optional
  source ID identifies an ordinary attachment imported from a forwarded message;
  a file chosen by the user has no source ID.
- Every compose-facing `DraftDto` includes
  `{ id, local_version, format: ComposeFormat,
  attachments: DraftAttachmentMeta[], forward_context?: ForwardContext }` in
  addition to its recipients, subject, authored plain body, status, and
  timestamps. Provider mailbox names and UIDs, the owning account ID, and raw
  RFC822 content remain inside Rust and are not serialized in `DraftDto`.
  `ComposeFormat` is
  `{ body_html?, stationery: none | lined | grid, send_stationery }`.
- `DraftAttachmentMutationKind` is `saved`, `conflict_copy`, `stale`, or
  `canceled`. `DraftAttachmentMutationOutcome` is
  `{ kind, draft: DraftDto, canonical?: DraftDto }`:
  - `saved` returns the updated canonical draft with its new `local_version` and
    complete attachment set;
  - `conflict_copy` returns the new conflict copy as `draft` and the unchanged
    latest canonical draft as `canonical`;
  - `stale` performs no mutation and returns the latest canonical draft as
    `draft`;
  - `canceled` imports no file, does not increment `local_version`, and returns
    the latest canonical draft as `draft`.
- Adding selected files with a stale expected version follows the existing draft
  save rule and creates a conflict copy so the new bytes are not lost. Removing
  an attachment with a stale expected version follows stale-delete safety,
  removes nothing, and returns `stale`.
- `AttachmentSaveStatus` is `saved`, `canceled`, or `error`.
  `AttachmentSaveResult` is
  `{ status, file_name?, error_kind?, retryable }`. `saved` exposes only the
  final base file name, never its directory or complete path; `canceled` has no
  file or error fields. `AttachmentSaveErrorKind` is `message_unavailable`,
  `attachment_not_found`, `permission_denied`, `disk_full`, or `write_failed`.
  After account and opaque-ID validation, every picker, extraction, and write
  outcome uses this DTO; cancellation and expected operational failures are not
  encoded as thrown strings.
- `ForwardContext` is
  `{ source_message_id, original_subject, from?, to, cc, sent_at?,
  quoted_text, quoted_html?, quoted_render_mode?,
  source_attachments: AttachmentMeta[] }`. It is persisted inside the draft and
  immutable; authored or attachment edits cannot replace its original identity,
  body, or source-attachment inventory.
- `ForwardWarning` is `html_downgraded`,
  `inline_resources_not_forwarded`, or `attachments_omitted_by_user`.
  `PreparedForward` is `{ draft: DraftDto, warnings: ForwardWarning[] }`; the
  complete current staged attachment set is `draft.attachments`. On an initial
  attachment-bearing preparation, its `source_attachment_id` values correspond
  one for one with `draft.forward_context.source_attachments`. Later explicit
  add/remove edits change only `draft.attachments`; the immutable source inventory
  remains available for accurate context.
- Expected preparation failures use
  `ForwardPreparationErrorKind` values `message_unavailable`,
  `body_unavailable`, `attachment_unavailable`, `attachment_stage_failed`, or
  `source_changed`. The typed error is
  `{ kind, failed_attachment_ids, retry_without_attachments_allowed }`.
  Attachment-related failures set the final field true; they never return a
  partially prepared draft. An explicit `include_attachments = false` success
  has an empty draft attachment set and the
  `attachments_omitted_by_user` warning.
- `ForwardPreparationOutcomeKind` is `prepared` or `error`.
  `ForwardPreparationOutcome` is the discriminated union
  `{ kind: "prepared", prepared: PreparedForward }` or
  `{ kind: "error", error: ForwardPreparationError }`. Expected hydration,
  source-change, extraction, and staging failures use this union instead of
  requiring React to inspect a command-error string.
- Mailbox discovery, listing, and history use
  `get_mailbox_capabilities(account_id)`,
  `list_mailbox_page(account_id, role, cursor?, page_size, query?)`,
  `load_older_mailbox_page(account_id, role, cursor, page_size, query?)`, and
  `sync_mailbox(account_id, role)`.
- After the user accepts the one-time creation confirmation, the UI calls
  `create_mailbox_role(account_id, role) -> MailboxCapability`. This command
  accepts only `archive` or `trash`, chooses the canonical `Archive` or `Trash`
  name in Rust, performs LIST before and after CREATE, and returns `available`
  only for the verified selectable role. It accepts no arbitrary mailbox name
  and is idempotent when another client or repeated command already created the
  role. Expected discovery, CREATE, and selectability failures return the
  corresponding capability status/reason; only invalid account or role input
  rejects the command outside that DTO.
- Message actions use `set_message_seen(message_id, seen)`,
  `archive_message(message_id)`, and `move_message_to_trash(message_id)`.
  Permanent deletion is two-step:
  `prepare_permanent_delete(message_id)` returns a short-lived, single-use plan
  bound to that account, message, and action, then
  `confirm_permanent_delete(plan_id)` consumes it.
- Attachment and forwarding commands are
  `save_message_attachment(message_id, attachment_id) ->
  AttachmentSaveResult`,
  `create_compose_draft() -> DraftDto`,
  `add_draft_attachments(draft_id, expected_local_version) ->
  DraftAttachmentMutationOutcome`,
  `remove_draft_attachment(draft_id, attachment_id,
  expected_local_version) -> DraftAttachmentMutationOutcome`, and
  `prepare_forward(message_id, include_attachments) ->
  ForwardPreparationOutcome`.
  A newly created compose draft has `local_version = 1` and an empty attachment
  set.
- Platform open/save dialogs and managed-file access run inside the Tauri/Rust
  boundary. The React `mailApi` mirrors these operations one for one; it must not
  add a generic file reader/writer, arbitrary mailbox mutation, raw IMAP command,
  or unrestricted network API.

## Notifications

- Binding an account starts a fresh notification baseline. The first historical
  import after binding, including re-adding an account whose local cache was
  retained, establishes that baseline and does not generate arrival alerts.
- Successful binding is reflected by the connected-account state and does not
  produce a separate lower-right success toast.
- Later unread arrivals may show:
  - resolved sender display identity and sender address;
  - subject;
  - receiving account remark/identity and address.
- Notifications never show body text, HTML, attachment content, remote images,
  or any recipient header, including Bcc.
- One **桌面通知** setting controls popup delivery whether Mine Mail is active or
  in the background.
- Notification sound enablement and sound preset are separate settings.
- A notification batch shows its exact unread count through 99; larger batches
  display **99+**.
- Clicking a new-mail card opens that cached local message in its owning account.
- Visual treatment and avatar use follow `../DESIGN.md`.

## Contacts, remarks, and avatars

- The **通讯录** workspace derives correspondents from cached message headers.
  There is no separate “saved contact” state.
- Correspondent derivation uses From, To, and Cc only. A cached Bcc header never
  creates a contact, affects contact activity counts, or becomes a lookup key.
- **全部** contains correspondents derived from the active account only.
- **收藏** aggregates local favorites from every connected account and visibly
  identifies the owning account on each row.
- A favorite key is account ID plus normalized email.
- A contact remark is app-wide metadata keyed by normalized email. A non-empty
  remark becomes the primary contact label in contacts, mail list, reader, compose
  suggestions, and notifications.
- Contact detail retains the latest sender-owned mail-header name beneath the
  local remark and keeps the real address visible.
- IMAP never owns contact favorites, remarks, or avatars. Any future provider
  contact integration is a separate opt-in adapter and may not change the
  local-first default implicitly.
- Exact-email avatar override wins over the built-in known-domain map, then falls
  back to initials. Account cards similarly prefer their Mine Mail-local avatar.
- User-selected PNG, JPEG, or WebP avatars are stored locally, scoped to the
  account/contact identity, and limited to 2 MiB.

## Settings behavior

- Settings is an embedded workspace and preferences save immediately.
- There is no global Save/Cancel footer.
- Adding an account is provider-first and drills into the chosen provider form.
- Avatar editing starts by activating the avatar.
- Secondary account actions use the local action menu.
- Routine backend health, cache state, and successful background synchronization
  are not persistent main-interface status chrome. Show action-specific progress
  and failures requiring a user decision.

## Local data location and migration

- On a new Windows installation, Mine Mail stores local product data in the
  writable `Data` directory beside the installed executable. A protected,
  occupied, or unwritable install location falls back to Tauri's per-user local
  application-data directory.
- An existing local-data installation remains at its current location after an
  upgrade. Mine Mail never silently switches away from existing mail, drafts, or
  Outbox state merely because a sibling install `Data` directory becomes
  available.
- A small location pointer and bounded diagnostic logs remain in the operating
  system's per-user application-data directory. Credentials and OAuth tokens
  remain in the OS credential store. Mail databases, contacts and settings
  databases, profile avatars, user assets, and the Windows WebView2 data folder
  use the selected product-data directory.
- **关于 Mine Mail** shows the exact active data directory, total disk use, and
  separate usage for **邮件与本地资料 / 界面与浏览器缓存 / 用户资源 /
  可清理缓存 / 诊断日志 / 其他数据**. Its action label is **更改位置**.
- A user-selected data directory must be an empty, writable, absolute directory
  on a local disk. Mine Mail rejects network shares, protected system locations,
  and a directory that contains or is contained by the current data directory.
- Changing location requires an in-app **迁移本地数据** confirmation. The final
  action is **迁移并重启**. Migration runs before databases and WebView2 open on
  the next launch, verifies copied file sizes and every top-level Mine Mail
  SQLite database, then switches the location pointer. Source product data is
  deleted only after the verified target becomes active.
- A failed migration preserves and reopens the original data location, reports
  the failure in About, and never presents a partial copy as active data.
- If a configured custom disk is missing or unwritable on startup, Mine Mail
  does not silently create empty account data elsewhere. Cached mail remains
  untouched and local mail work stays unavailable until the storage location is
  restored. About shows the configured path and asks the user to reconnect that
  disk before restarting Mine Mail.

## Application updates

- Update checks are user-initiated from Settings. Mine Mail never silently
  downloads or installs an update.
- When a signed update is available, show its bounded version metadata and notes,
  then require a second explicit **下载并安装** confirmation.
- The Tauri updater owns endpoint access, signature verification, download,
  installation, and relaunch. React only presents bounded metadata, progress, and
  the user-confirmed trigger.
- An update failure preserves the installed version and local mail data and must
  not be reported as success.
- The signing private key never belongs in the repository, logs, or release
  artifacts. Committed updater endpoints and public verification keys are
  configuration, not secrets.

## Current unsupported editing/delivery scope

Until implemented and tested, do not imply support for:

- Outlook modern authentication;
- arbitrary sender HTML editing;
- inline-image composition;
- editing externally created HTML, multipart, inline-image, or attachment drafts;
- automatic retry of uncertain SMTP delivery.
