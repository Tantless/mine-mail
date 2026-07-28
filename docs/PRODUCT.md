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
- Provider choices are 163, Gmail, Outlook, and custom IMAP/SMTP. Outlook remains
  unavailable until its modern authentication path is implemented.
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
  a composer is open.
- Removing a password-based account deletes its OS credential and Mine Mail
  account record. Cached SQLite mail, drafts, and Outbox data remain unless the
  user separately selects **同时删除本地邮件缓存**.
- A Gmail account offers two distinct operations:
  - **仅断开** removes Mine Mail's local connection and OS token while retaining
    the Google authorization grant and local cache;
  - **撤销授权并移除** first asks Google to revoke Mine Mail's OAuth grant, then
    removes the OS token and local account record.
- The optional local-cache deletion removes that account's SQLite mail, drafts,
  and Outbox data and is irreversible. It does not happen implicitly.
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
- A background refresh must not replace already rendered messages, contacts, or
  correspondence with a loading placeholder.
- When background mode is enabled, closing the main window hides it to the tray.
  Tray labels are exactly **打开 / 刷新 / 退出**.
- Login autostart is one setting and defaults off.

## Mail list, bodies, and remote content

- Inbox summaries contain only bounded list/preview data, never raw RFC822,
  complete HTML, or an unrestricted body payload.
- Synchronization derives bounded list previews without requiring the user to
  select each message. Preview readiness is independent from full-body caching.
- The primary sidebar shows numeric badges only for unread Inbox messages and
  Outbox items not yet confirmed sent. Zero counts are omitted. Starred,
  Contacts, Sent, Drafts, Archive, and Trash never show numeric badges.
- Selecting a message paints its cached preview immediately and hydrates the body
  silently. After synchronization, Rust may prefetch a recent bounded set of
  bounded-size bodies.
- MIME parsing, HTML classification, sanitization, iframe isolation, reply
  segmentation, and remote-image boundaries follow `MAIL_RENDERING.md`.
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
- Contact **收藏** is a separate local organization feature and must not be
  implemented with IMAP `\Flagged`.

## Drafts and compose lifecycle

- Drafts synchronize in both directions.
- Editing reuses one stable draft ID. The editor saves locally while active and
  uploads the latest eligible version remotely every five minutes.
- Every editor write carries the SQLite `local_version`.
- A stale write becomes a conflict copy rather than overwriting the canonical
  draft. A stale delete never removes a newer canonical version.
- HTML or attachment-bearing drafts remain read-only until that MIME can be
  edited safely.
- Font, size, emphasis, list, alignment, link, and clear-format edits are part of
  the same exact draft version as the plain authored body. A formatting command
  applies to the active range or future input at a collapsed caret, preserves the
  range or caret, and never changes the whole editor or stationery line rhythm
  merely because the toolbar font-size value changed.
- Moving a collapsed caret updates the font, size, emphasis, list, and alignment
  controls from the inherited format at that position. A mixed selection reports
  a mixed font or size rather than a stale toolbar value. A collapsed-caret size
  change updates both the caret presentation and the stored format used by the
  next input. Italic is persisted as semantic emphasis in restricted HTML.
- At the start of a paragraph, typing `1.` followed by Space creates a real
  ordered list. Enter creates the next numbered item, and Enter again on an empty
  item exits to an ordinary paragraph. The plain-text fallback emits explicit
  numeric markers for clients that do not render HTML.
- If the rich editor fails during lazy initialization or lifecycle reconnection,
  the compose surface stays open, preserves the current draft value, and offers
  an in-place retry instead of unmounting the application.
- Closing the composer or pressing Escape never forces a save:
  - a new compose session removes any recovery draft created only by that session;
  - closing an existing draft leaves its previously persisted version intact;
  - minimizing keeps the current editing session alive.
- Compose window geometry and its visual behavior follow `../DESIGN.md`.

## Sending and Outbox

- Sending always presents exact-recipient confirmation.
- Recipient confirmation, draft state, and the created Outbox item bind to one
  exact `local_version`.
- A send attempt preserves any newer editor changes.
- A newer safe attempt may supersede an older retryable Outbox item for the same
  version.
- Distinguish confirmed success, confirmed failure, retryable failure, and
  `delivery_unknown`.
- Never automatically retry `delivery_unknown`; the user must decide whether to
  risk a duplicate.

## Notifications

- The first historical import establishes an account notification baseline and
  does not generate arrival alerts.
- Later unread arrivals may show:
  - resolved sender display identity and sender address;
  - subject;
  - receiving account remark/identity and address.
- Notifications never show body text, HTML, attachment content, or remote images.
- One **桌面通知** setting controls popup delivery whether Mine Mail is active or
  in the background.
- Notification sound enablement and sound preset are separate settings.
- Clicking a new-mail card opens that cached local message in its owning account.
- Visual treatment and avatar use follow `../DESIGN.md`.

## Contacts, remarks, and avatars

- The **通讯录** workspace derives correspondents from cached message headers.
  There is no separate “saved contact” state.
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
- rich-text composition or arbitrary sender HTML editing;
- inline-image composition;
- complete attachment upload/download/edit workflows;
- editing HTML/attachment drafts;
- automatic retry of uncertain SMTP delivery.
