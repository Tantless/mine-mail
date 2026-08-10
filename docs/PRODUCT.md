# Mine Mail Product Contracts

This document defines durable user-visible behavior. It complements the
repository-wide architecture and safety rules in `../AGENTS.md`, the visual
system in `../DESIGN.md`, and the untrusted-mail boundary in
`MAIL_RENDERING.md`.

This edition records the approved behavior of the current application. Keep
internal DTO shapes, cache budgets, parser thresholds, and protocol helper
details beside the code and tests. Update this document only when a durable
product decision changes.

## Product boundary

- Mine Mail is a local-first desktop mail client, not a hosted service. The
  desktop app connects from the user's device to the configured mail provider;
  it has no Mine Mail mailbox proxy.
- SQLite is the immediate local source for renderable mailbox state. The server
  remains authoritative for remote messages and system flags after
  synchronization confirms them.
- The Vite build is an explicit mock UI surface. It performs no real mail,
  credential, tray, notification, filesystem, or operating-system work and must
  never become a parallel Web mail runtime.
- Rust owns credentials, IMAP/SMTP, MIME processing, synchronization, drafts,
  Outbox state, notifications, local files, and database access. React calls
  narrow Tauri commands and receives only bounded, typed presentation data.
- React never receives passwords, OAuth tokens, complete RFC822 messages,
  attachment bytes, provider mailbox coordinates, unrestricted paths, or general
  file/network/database access.
- The exact active or newly selected product-data directory is the one complete
  path intentionally shown in React, only in About and the confirmed
  storage-migration flow. Mail, attachment, diagnostics, managed-cache, and
  credential paths remain Rust-only.

## Failures and recovery

- Product-owned error copy is Simplified Chinese. Established names such as IMAP,
  SMTP, OAuth, TLS, Gmail, and Mine Mail may remain in technical form; raw
  provider or backend diagnostics never appear.
- A failure distinguishes correctable input, account authorization, network or
  provider availability, and unexpected Mine Mail processing. It states what
  remained safe and the next action when that matters.
- A failed operation preserves usable cached mail and recoverable local work.
  Retry is offered only when repeating the action is safe.
- Never show credentials, tokens, mailbox content, internal identifiers, database
  details, stack traces, or complete paths outside the explicit storage-location
  flow.
- Routine success is represented by the resulting UI state or nearby status.
  Repeated background failures are not turned into repeated notifications.

## Accounts and identity

- A user may connect at most three accounts. One account is active in the
  interface at a time, while startup, scheduled, manual, and tray synchronization
  cover every network-ready account.
- Account data, synchronization state, notification baselines, drafts, Outbox
  attempts, and queued mutations are isolated by stable account ID.
- Switching accounts remembers each account's current primary destination,
  selected list item, list scroll position, and open-detail state for the current
  session. A new account workspace does not silently select Inbox or the first
  message.
- The connection list contains 163, QQ, Gmail, and custom IMAP/SMTP:
  - 163 and QQ use the provider's fixed address suffix and a provider-generated
    authorization code, not the Web login password. Their forms link to bundled
    offline instructions.
  - Gmail uses Google OAuth 2.0 Authorization Code + PKCE in the system browser
    and XOAUTH2 for mail access. While access remains preview-only, the form shows
    the current allowlisting contact.
  - Custom accounts collect explicit IMAP and SMTP configuration.
- Outlook is not offered until Microsoft OAuth / Modern Auth is supported. A
  legacy Outlook record remains visible and its cached mail remains readable,
  but it cannot reconnect or perform network work.
- Binding a verified account returns without waiting for historical import.
  Synchronization continues in that account's background lifecycle.
- Passwords and OAuth tokens stay in the OS credential store and Rust runtime.
  Provider-issued desktop OAuth client metadata is a separate ignored Rust-only
  build input.
- A missing, expired, or revoked credential stops network work only for the
  affected account. Cached mail stays readable; the account shows one persistent
  reauthentication affordance instead of repeated background-error toasts.
- A local account remark may become the primary display label, but the real
  mailbox address remains available wherever identity must be clear.
- Connecting another account while compose is open first saves and minimizes the
  source account's session. A local save failure prevents the account switch.

## Account removal

- Account removal is explicit and confirmed. It is unavailable while that account
  has a live compose session.
- Disconnecting removes the Mine Mail account record and its OS credential.
  Cached mail, drafts, and Outbox data remain unless the user separately selects
  **同时删除本地邮件缓存**.
- Gmail offers **仅断开** and **撤销授权并移除**. Revocation is attempted before
  local disconnection; a failed revocation must not be presented as complete
  removal.
- Local-cache deletion is irreversible and account-scoped. It removes that
  account's local mail, drafts, managed attachments, and Outbox data without
  affecting another account.
- Partial failure reports what was and was not removed and preserves all
  recoverable state.

## Startup, synchronization, and background life cycle

- Startup renders cached SQLite state first, then starts Rust synchronization.
  Cached rows are never replaced by a global loading screen.
- Startup, in-app refresh, tray **刷新**, scheduled reconciliation, and supported
  wake/resume events cover all network-ready accounts. Refocusing an already
  running main window performs only a lightweight Inbox update.
- Arrival monitoring prefers IMAP IDLE and otherwise uses a lightweight mailbox
  probe. The user-selectable 1/3/5-minute setting, default 5 minutes, performs
  fuller reconciliation for flags, deletions, queued mutations, and recovery.
- Startup and explicit refresh discover available mailbox roles and synchronize
  bounded summaries. They do not eagerly download every body or attachment.
- Opening Archive or Trash paints cached summaries first and then synchronizes
  that role. Mine Mail never empties Trash automatically.
- When an active folder has no cached rows and its initial read/import is still
  running, show compact nearby loading feedback. A genuinely empty settled list
  remains visually quiet.
- An explicit folder refresh owns its inline progress and bounded completion
  result. Routine background reconciliation does not restart that foreground
  indicator or create a success toast.
- Concurrent work is scoped so one account or mailbox does not unnecessarily
  block another. Account replacement and removal remain exclusive life-cycle
  operations.
- With background mode enabled, closing the main window hides it to the tray.
  Tray actions are **打开 / 刷新 / 退出**.
- Login autostart defaults off. When enabled, system login starts Mine Mail hidden
  in the tray; an ordinary app launch still opens the main window.

## Mail list, search, and reading

- Mail lists contain bounded summaries and previews, never complete HTML or raw
  message source.
- Selecting a message keeps its preview in the list and shows a reader loading
  state until the selected body and final render mode are ready. Preview text is
  never substituted for the opened body.
- If a body is not cached, Rust may fetch the bounded MIME structure and selected
  renderable body sections without downloading ordinary attachment bytes.
  Background prefetch is opportunistic and must not delay a user-selected
  message.
- Cached body payloads may be evicted to enforce a device budget while summaries,
  previews, drafts, Outbox state, and managed outgoing attachments remain.
  Reopening an evicted body fetches it again.
- Inbox, Sent, Archive, Trash, and Starred load bounded pages and append history
  automatically near the list end. Loading another page preserves visible rows,
  selection, and scroll position. There is no manual load-more control or
  persistent end card.
- Each page distinguishes local history, possible remote history, offline
  unavailability, and an authoritative end internally; those distinctions must
  not produce misleading empty or completion claims in the list.
- New arrivals merge by account, mailbox, and server identity without duplicating
  rows or invalidating a usable history cursor. A mailbox identity epoch change
  starts a fresh bounded listing only after safe reconciliation.
- Remote deletion is applied to the cache only after a fresh, consistent server
  view confirms it. An empty, partial, or contradictory snapshot leaves cached
  mail readable and retries later.
- Search is local-only. It covers every synchronized summary in the active
  account and folder and matches subject, From, To, Cc, preview, and any complete
  plain-text body currently in the bounded local body cache. It excludes Bcc and
  remote IMAP search. The interface labels the scope **搜索已同步邮件**.
- Body mode selection, sanitization, isolated HTML, remote content, reply history,
  and attachment parsing follow `MAIL_RENDERING.md`.
- Remote-image policy is **自动加载 / 每次询问 / 始终阻止** and defaults to
  automatic.
  Nearby help explains the open-time, IP/device, and tracking-pixel privacy risk.
- Links use safe schemes and open through the desktop-owned path; sender content
  cannot navigate or script the application.
- Incoming mail opened from Inbox, Starred, Archive, Trash, or contact history
  may be translated through the configured AI Provider. Translation is an
  explicit per-message action and does not alter the cached message, server
  content, search index, reply source, or forwarding source. A completed result
  remains a reader-only in-memory alternative that can be switched between
  **原文** and **译文** throughout the current application run. Switching mail
  does not cancel or discard an active or completed translation. Sent, Draft,
  and Outbox content does not expose this action.

## Folders, stars, and message actions

- A mail star is the IMAP `\Flagged` system flag. Local star changes are immediate
  and persist offline until the server confirms the desired state.
- **已收藏** merges starred Inbox, Sent, and Archive mail and has no
  **全部 / 未读** tabs. Unstarring keeps the row in the current visit so an
  accidental action can be reversed; leaving or explicitly refreshing rebuilds
  the view.
- Contact **收藏** is separate Mine Mail-local metadata and never maps to
  `\Flagged`.
- Mailbox roles are account-scoped and resolved in Rust. React never receives or
  supplies raw provider mailbox names.
- If Archive is missing, the first explicit Archive navigation/action asks the
  user to assign one eligible existing server folder. Mine Mail does not create
  an Archive mailbox. Cancellation leaves the current message and workspace
  unchanged.
- If Trash is missing, the first explicit Trash navigation/action creates or
  verifies the provider's Trash role without a confirmation dialog. A failure
  leaves the message unchanged and never turns the action into permanent
  deletion.
- **删除** in Inbox, Sent, or Archive means move to Trash. Only a message already
  in Trash offers permanent deletion, and every permanent deletion is confirmed.
- Archive and Trash offer **移到收件箱**. This moves to Inbox without claiming to
  restore an unknown original mailbox.
- Mark read/unread changes the IMAP `\Seen` state. It updates the row immediately
  and synchronizes without a reader-body banner.
- Star, read/unread, Archive, move-to-Inbox, move-to-Trash, and permanent-delete
  intentions are written locally before network work. Restarting or going
  offline cannot silently lose them.
- Retried mutations are account- and mailbox-epoch-bound. An ambiguous move,
  copy, or deletion is reconciled before retry; Mine Mail never blindly repeats
  an action that could duplicate or delete the wrong message.

## Attachments, replies, and forwarding

- Received attachment metadata is bounded and uses an opaque part identity. The
  reader shows a safe name, type, and exact or clearly approximate size.
- Saving opens the platform Save As flow and runs in Rust. React receives neither
  attachment bytes nor the selected complete path.
- Names are sanitized and bounded. Existing files are never overwritten; a
  numeric suffix is chosen instead. A temporary sibling is published only after
  the selected attachment has been decoded completely.
- When the complete RFC822 message is absent, Rust validates the current MIME
  structure and downloads only the selected attachment part. It does not hydrate
  unrelated attachments or replace an already readable body.
- Cancellation is not an error. A failed save removes partial output, preserves
  the reader, and remains retryable when safe.
- Selecting an outgoing file imports an immutable copy into account- and
  draft-scoped managed storage. React retains bounded metadata and an opaque ID,
  not the user's original path.
- Managed attachments participate in draft version checks. A stale add produces
  a conflict copy; a stale removal cannot change the newer canonical draft.
  Referenced blobs remain while owned by a draft, conflict copy, or immutable
  Outbox item.
- Reply and forward preparation requires a fully hydrated source; list preview is
  never treated as quoted content.
- Reply excludes Bcc from both new recipients and quoted context.
- Forwarding preserves structured original identity, a trustworthy plain body,
  optional sanitized HTML, and every ordinary named attachment. Preparation fails
  as a whole if a requested attachment cannot be staged. The current UI does not
  offer a silent or automatic attachment-less fallback.
- The user's authored text remains separate from immutable reply/forward context.
  React never concatenates raw sender HTML or identity headers into the editor;
  Rust assembles final MIME.

## Drafts and compose

- Drafts synchronize in both directions. One stable draft ID and SQLite
  `local_version` protect each editing session.
- A stale save creates a conflict copy rather than overwriting the canonical
  draft. A stale delete or attachment removal cannot remove newer work.
- Active edits save locally; the latest eligible version is periodically uploaded
  to the provider Drafts mailbox.
- Mine Mail-owned plain and restricted-rich drafts are editable. External HTML,
  multipart, inline-image, or attachment drafts remain read-only when they cannot
  round-trip without loss.
- Formatting, stationery, recipients, subject, body, and managed attachments all
  belong to the same exact draft version. Rust sanitizes authored rich text again
  before persistence or MIME construction; plain text remains the fallback.
- Compose supports the fonts and formatting exposed by the current toolbar,
  semantic ordered lists, links, and the current fixed first-line indent.
- A draft may use no paper, lined paper, or grid paper. Edit-only paper stays a
  local aid; send-with-message adds Mine Mail-owned, sanitized stationery while
  preserving a complete plain alternative.
- **保存并最小化** first stabilizes local authored content. An untouched new
  session may minimize without creating an empty draft; a save failure leaves
  compose open.
- Closing compose or pressing Escape does not force-save:
  - closing a new session removes recovery state created only for that session;
  - closing an existing draft leaves its last persisted version intact;
  - minimizing keeps the live editing session.
- Each account owns at most one live compose session. Account switching saves and
  hides the source session; returning restores it as the minimized compose bar.
  **写信** restores an existing minimized session instead of creating another.
- **默认开启 AI 助理** is a persisted desktop preference that defaults on. It
  determines whether a newly opened compose surface initially expands the right
  assistant panel; the user may still collapse or reopen that panel while
  composing.
- Selecting a different draft while compose is minimized first stabilizes the
  current edit. A failed save prevents the switch.
- Reply/forward context is immutable and separate from the authored editor.
  Forwarded ordinary attachments are visible and removable before sending.
- Compose exposes a dedicated optimization action for the authored body.
  Optional user instructions may request wording or supported rich-text
  formatting. Optimization cannot read or change the subject, sender,
  recipients, attachments, stationery, or quoted source. The request captures
  the body and instructions at click time and runs without locking compose.
  Completion does not modify the live draft: the user reopens the result and
  reviews an editable, pure-text difference between the submitted snapshot and
  AI result. Formatting differences are preserved but not highlighted. Applying
  either side requires an explicit side-named confirmation. Mine Mail backs up
  the then-live body immediately before replacement and exposes one icon-only
  rollback until it is used or replaced by a later optimization application.
  The instruction text, an in-flight request, its completed comparison, and the
  rollback backup remain attached to the live compose session through minimize
  and restore.
  Minimizing the comparison preserves it; permanently closing it requires
  confirmation and discards that result.
- Conversational AI sessions belong to the Mine Mail application rather than one
  draft and persist in a Rust-owned SQLite store. A session may associate with
  every editable draft from which the user sends a prompt. The association uses
  the stable draft ID, or the live compose identity before the first save, appears
  as a subject plus short display-ID pill, and is removed after that draft is sent
  or deleted; session history remains.
- Sending a conversational prompt immediately persists the user message and an
  assistant placeholder. Assistant messages carry `streaming`, `completed`,
  `stopped`, or `failed` state; stopping or failing keeps any received partial
  Markdown. Session and mode switching are locked only while that turn runs,
  while compose, collapse/minimize, and editing the next prompt remain usable.
- The assistant provides **自动**、**邮件生成** and read-only **聊天** modes.
  Optimization remains a separate high-frequency action. Rust selects a hard
  tool allowlist for every mode; prompts alone never grant a capability.
- Tool definitions and Rust execution share one typed argument contract. Invalid
  fields, types, and ranges are rejected without silent coercion; repeated
  contract failures are bounded so a model cannot loop indefinitely while
  guessing tool arguments.
- The built-in assistant calls its configured AI Provider directly from Rust.
  It does not route through the local MCP service. A manually entered API Key
  exists in React only as transient form input until the narrow Tauri command
  consumes it. After a successful save, React shows only a fixed non-secret mask
  derived from `has_stored_api_key`; Rust never returns a key to React, and the
  browser demo remains offline and deterministic.
- Agent configuration supports any number of local Provider instances. Multiple
  instances may share one preset while keeping independent stable IDs, names,
  protocols, base URLs, preferred models, credential sources, discovery caches,
  ordering, and health state. Presets cover custom services, DeepSeek, Kimi,
  OpenAI, Anthropic, Qwen, Xiaomi MiMo, MiniMax, ModelScope, Doubao Seed, GLM,
  and OpenRouter. Every preset exposes only the protocols implemented by both
  that service and Mine Mail: OpenAI Responses, OpenAI Chat Completions, and/or
  Anthropic Messages. **自动** resolves to that instance's preset recommendation;
  an explicit protocol remains explicit. Mine Mail never retries a failed request
  through another protocol or Provider because that could duplicate billed work
  or tool effects. A fresh installation has an empty Provider list and does not
  opt into reading an API Key from the environment.
- Exactly zero or one Provider instance is the default route. The default binds
  one exact instance, protocol, and preferred model, powers reader translation
  and standalone compose optimization, and initializes a newly opened compose
  assistant. Choosing another model in compose overrides only that compose
  assistant. Deleting the default clears it rather than silently choosing a
  replacement. The legacy active Provider becomes one ordered instance and its
  selected model becomes the default during migration.
- The same configured Provider powers reader translation. **AI 翻译语言** is a
  persisted reading preference, defaults to Simplified Chinese, and offers
  common languages under their native display names. It supplies the initial
  language for a message, while the reader's language control is a per-message
  override and never updates the persisted preference. A completed translation
  keeps a language trigger beside **原文 / 译文**; choosing another language
  immediately queues a replacement. The previous translation remains readable
  until the replacement succeeds and remains available if it fails. Translation
  tasks and the latest successful result are kept in a bounded runtime-only
  cache, cleared on application exit. At most two messages translate at once;
  further messages wait in order and duplicate active requests are reused.
  When translation cannot
  start because the Agent Provider, model, or API credential configuration is
  incomplete, the reader keeps the original content visible and shows the
  bottom-right warning **请先前往设置界面完成AGENT配置** instead of an inline
  reader error. A structurally valid partial Provider result is still useful:
  valid numbered translations replace their matching text positions, missing
  positions retain the original text, and the reader reports the completed and
  total translation-unit counts. Duplicate, unknown, malformed, or unsafe positions
  remain a failed translation rather than being guessed into the message. MiMo
  translation additionally disables thinking. Every protocol receives translation
  responses through its internal SSE adapter with a 180-second total limit and a
  45-second between-chunk idle limit. Rust first splits a long plain-text body or
  HTML text node at semantic boundaries into units of at most 800 UTF-8 bytes,
  then creates batches of at most six units and normally 800 bytes. The scheduler
  starts with four requests, raises concurrency to at most six after consecutive
  successes, lowers it after partial or failed batches, and starts the next batch
  whenever any active batch completes. Missing retryable units receive one retry
  in smaller batches of at most two units and normally 400 bytes; an individual
  larger unit remains intact. A failed or timed-out
  unit retains its original text while other valid units remain usable; the reader
  still updates only after Rust validates and merges the available results.
- Provider add/edit is an embedded Settings child flow with local **保存渠道**
  and **保存并测试** actions. Incomplete input remains local. Rust persists
  non-secret instance data in the AI SQLite store, while a manual API Key is
  scoped by stable Provider-instance ID in the OS credential store. Environment-
  key mode ignores any form value and reads the preset's documented variable
  after application startup. React receives only a fixed non-secret credential
  mask state.
- Provider order is persisted and resolves duplicate exact model names in the
  compose catalog: the highest successfully refreshed instance wins. Opening
  compose requests model lists from every configured instance independently and
  in parallel. The catalog contains only structurally valid models returned in
  that refresh; an unavailable, expired, or revoked instance contributes zero
  models, records only instance-local status, and never blocks another instance
  or emits a global error. A turn binds its resolved instance and model before
  starting and never fails over after an error.
- Provider base URLs must use HTTPS, except loopback-only HTTP for local
  development, and cannot contain embedded credentials, query parameters, or
  fragments. Model discovery and connection tests run in Rust with bounded
  responses and privacy-safe logs. An explicit instance test first refreshes the
  model list, then performs one minimal request with the preferred model or first
  returned model, reports that request's Rust-observed latency, and performs a
  best-effort structured-output capability probe whose failure does not turn a
  successful connection test into a failure. Capability profiles are scoped by
  Provider instance configuration, protocol, base URL, and model, cached for
  seven days, and combine
  presets with tested and runtime-observed support for structured output,
  streaming, and reasoning controls. They never contain credentials or mail
  content. A model list alone does not prove those capabilities.
- Except for the user's instruction and visible Session history, draft data is
  not placed in the initial model context. The model must use bounded tools to
  read the current subject, body, sender, recipients, immutable reply/forward
  text, contacts, or attachment metadata. Text attachments may be read by opaque
  ID within a size and type allowlist. PDF, Office, archive, executable, and
  other binary formats are not parsed. Image tools are registered only for a
  Provider/model with implemented multimodal support.
- **邮件生成** may replace recipients, subject, body, supported body formatting,
  and stationery. **聊天** has no write tools. **自动** combines those read and write
  tools according to the user's request. No built-in AI mode can switch the
  draft account, mutate immutable quoted context, manipulate attachments, send
  mail, or operate Outbox.
- Tool writes apply only to a Rust in-memory working copy. A successfully
  validated conversational write becomes one or two read-only proposal cards in
  the assistant message: recipients/Cc/Bcc/subject form one group, and
  body/format/stationery form the other. The user applies each changed group with
  one icon action, without confirmation. Apply replaces only that group and
  stores its then-live value as an undo backup; an old proposal intentionally
  remains applicable even after later edits. Stopped, failed, or invalid turns
  never create a proposal. Independent optimization instead retains its click-time snapshot for
  comparison and never writes until the user selects and confirms one side; the
  live body is backed up immediately before that intentional replacement. The
  short opaque request revision sent to Rust is correlation metadata, not the
  serialized draft body. The user always reviews the result and clicks **发送**
  explicitly.
- **邮件生成**、**聊天** and **自动** use Provider SSE streaming through the
  selected OpenAI Responses, OpenAI Chat Completions, or Anthropic Messages
  adapter. Safe Markdown is
  rendered incrementally beneath an append-only execution trail. Each Provider
  reasoning round and tool call becomes its own ordered step. A Provider may
  supply visible reasoning deltas for the active step; that temporary detail is
  replaced by a bounded completion summary when the step ends and is never saved
  as Session message text. As soon as a streaming adapter recognizes a complete,
  allowed tool name, the live trail ends that reasoning step and shows the tool as
  running while its arguments continue to arrive; validation and execution update
  that same step rather than adding a duplicate. Hidden reasoning, tool arguments,
  and tool results are not exposed. Stop cancels the Provider stream and prevents later tools.
  Standalone optimization remains non-streaming. Optimization, conversational
  Agent turns, and reader translation share the same selected adapter, endpoint,
  authentication policy, size limits, and privacy-safe diagnostics; protocol
  selection cannot make one feature bypass those safeguards.
- Proposal payloads, tool lifecycle metadata, and apply backups expire seven
  days after Session activity. Cleanup runs at startup no more than once per day;
  user and final assistant Markdown remain as plain Session history.
- The canonical built-in tool protocol and per-mode permission matrix live in
  [`toolCalling/TOOLS.md`](toolCalling/TOOLS.md) and
  [`toolCalling/AGENT_MODULES.md`](toolCalling/AGENT_MODULES.md). Provider wire
  protocols and preset recommendations live in
  [`toolCalling/API_PROTOCOLS.md`](toolCalling/API_PROTOCOLS.md).

## Sending and Outbox

- The single **发送** action confirms the visible To, Cc, and Bcc set by binding it
  to the exact current draft version. There is no second confirmation dialog.
- Rust persists one immutable MIME message and envelope in Outbox before the
  background send proceeds. Later edits cannot alter that attempt.
- Once persistence succeeds, compose closes without waiting for SMTP. A failure
  before Outbox creation leaves the saved draft recoverable.
- Outbox distinguishes pending, retryable failure, confirmed rejection,
  confirmed success, and an uncertain delivery outcome.
- Confirmed sent items leave active Outbox immediately and may remain as local
  Sent fallbacks until the provider's exact Sent copy is synchronized.
- Sent reconciliation retires a fallback only on strong identity evidence. It
  never deletes one because subject, recipient, body, or timestamp merely looks
  similar.
- An uncertain SMTP outcome is never automatically retried. After checking the
  provider, the user may explicitly confirm delivery or acknowledge duplicate
  risk and retry the exact persisted message once. A new uncertain result
  requires a new review.
- A manual retry reuses the immutable RFC822 bytes and envelope; it does not
  rebuild from the editable draft.
- Graceful shutdown lets an in-flight SMTP operation record its confirmed or
  uncertain outcome. Crash recovery converts abandoned sending state into a safe
  reviewable outcome rather than an automatic resend.

## Notifications

- The first historical import after account binding establishes a notification
  baseline and produces no arrival alerts, including when a retained cache is
  reconnected.
- Later unread arrivals may show resolved sender identity/address, subject, and
  the receiving account identity/address. They never show body, HTML,
  attachments, remote images, or recipient headers.
- One **桌面通知** setting controls delivery. On Windows, a separate persisted
  **通知方式** choice selects exactly one of **Mine Mail 通知** and **Windows
  通知** for future arrivals. Existing settings databases and new installations
  default to **Mine Mail 通知**; the choice is not shown on other platforms.
- **Mine Mail 通知** uses the existing app-owned lower-right card. **Windows
  通知** uses the operating-system banner and notification center. The two
  surfaces never present the same arrival together, and a failed or
  system-blocked Windows delivery does not silently switch to the Mine Mail
  surface.
- Windows notification delivery is local and depends on Mine Mail remaining
  open or in the tray. It does not use a Mine Mail cloud mailbox proxy or a
  Windows push service. Windows notification, lock-screen, and do-not-disturb
  settings remain authoritative over system presentation.
- Sound enablement and the sound preset are shared by both delivery methods.
  Mine Mail owns playback for its card; Windows owns playback and suppression
  for a Windows notification.
- A notification batch displays its bounded unread count, and clicking a card
  while Mine Mail is running or in the tray opens the cached message in its
  owning account. A Windows notification left after an explicit application
  exit is not required to restore that exact message in the first Windows
  notification milestone.
- Successful account binding is represented by connected-account state rather
  than a separate success toast.

## Contacts, remarks, and avatars

- **通讯录** is derived from cached From, To, and Cc headers. Bcc never creates a
  contact or affects activity counts.
- **全部** is scoped to the active account. **收藏** aggregates local favorites
  from connected accounts and identifies the owning account.
- A favorite is scoped by account and normalized email. A contact remark is
  app-wide metadata keyed by normalized email.
- A local remark may become the primary contact label in lists, reader, compose
  suggestions, and notifications, while the latest sender-owned name and real
  address remain available.
- Favorites, remarks, account remarks, and avatar overrides are Mine Mail-local;
  IMAP does not own them.
- Avatar resolution is exact local override, built-in known-domain map, then
  deterministic initials. Selected PNG, JPEG, or WebP overrides are stored
  locally and bounded in size. Mine Mail never queries a remote avatar service.

## Settings, local data, and diagnostics

- Settings is an embedded workspace. Preferences save immediately and there is no
  global Save/Cancel footer.
- Account setup is provider-first. Avatar editing starts from the avatar; account
  remarks and secondary actions use the local row menu.
- Routine backend health and successful background synchronization are not
  permanent interface chrome. Show only action-specific progress and failures
  that require attention.
- Diagnostics are bounded, rotating, structured local events owned by Rust. They
  cover consequential operational boundaries without recording raw errors,
  addresses, subjects, recipients, bodies, HTML/RFC822, credentials, tokens,
  attachment names, or complete local paths.
- On a new Windows installation, product data normally lives in the writable
  `Data` directory beside the installed executable. If that location cannot be
  used, Mine Mail falls back to its per-user application-data location.
- An upgrade preserves the current data location. Mine Mail never silently
  switches away from existing accounts, mail, drafts, or Outbox data.
- About shows the exact active data directory, total use, and a category
  composition. This is the only ordinary product surface allowed to receive the
  complete active product-data path.
- **更改位置** uses the platform folder picker. The selected directory must be
  empty, writable, absolute, local, and separate from the current data tree.
- **迁移本地数据** with **迁移并重启** schedules a restart migration. Mine Mail
  verifies the copied data before switching the location pointer and deletes the
  source only after the verified target is active.
- A failed migration reopens the original location and never presents a partial
  copy as active. If a configured disk is unavailable at startup, Mine Mail does
  not silently create an empty account store elsewhere.

## Local MCP access

- The MCP controls live in **设置 → Agent 配置**, as the card immediately below
  **模型配置**.
- **开启 MCP** is a persisted parent setting. Turning it on requires one compact
  confirmation; an already enabled persisted setting restarts the service with
  the app without prompting again. Mine Mail must remain open or in the tray.
- The server uses Streamable HTTP at `127.0.0.1:46321/mcp`, never listens on a
  LAN/public interface, and has no connection token. Loopback Host/Origin checks,
  bounded requests, and bounded concurrency reduce exposure to local web pages
  and accidental overload.
- **获取信息** and **发送邮件** are persisted child permissions. They expand
  beneath the parent while it is on and collapse while it is off, preserving
  their saved values; initial defaults are information on and sending off. Every
  tool checks its permission when called.
- Information permission covers safe account listing, bounded synchronization,
  metadata and cached-body search, batched body hydration, selected message
  reading, received-attachment download, read/star state, Archive, Inbox, and
  Trash moves. It never exposes credentials, raw RFC822, or permanent deletion.
- Sending permission covers listing, creating, versioned editing/deleting of
  drafts, managed attachment add/remove, reply/forward draft creation, and exact
  draft-version sending. It never exposes automatic retry for an unknown SMTP
  delivery outcome.
- Tools address one stable account ID explicitly and do not inherit the active UI
  account. Cross-account search expands to explicit account IDs internally.
- Received attachment destinations and outgoing attachment sources are absolute
  local paths supplied by the agent. Rust owns transfer/import and never forwards
  those paths into React or diagnostics; subsequent file access depends on the
  agent client's own permissions.
- Supported clients are Codex, ChatGPT Desktop, Claude Code, OpenClaw, and Hermes.
  Their idempotent setup and discovery-only verification contract lives in
  `MCP.md`.

## Application updates

- Update checks are user-initiated. Mine Mail never silently downloads or
  installs an update.
- An available signed update shows bounded version metadata and notes, then
  requires **下载并安装**.
- After confirmation, dismissing the dialog or leaving Settings minimizes
  progress without cancelling the download. Only the explicit stop control
  cancels before installation begins.
- An active download has no elapsed-time deadline. It continues until complete,
  explicitly stopped, the process exits, or the transport fails.
- The Tauri updater owns endpoint access, signature verification, installation,
  and relaunch. React presents only bounded metadata, progress, and the confirmed
  trigger.
- A confirmed update relaunch is an explicit foreground launch. It opens the
  main window even when the running process originally came from login autostart
  with `--background`; that background argument remains limited to login startup.
- A failed update preserves the installed version and local data and identifies
  the failed stage without exposing raw errors or unrestricted URLs.

## Unsupported scope

Until implemented and tested, Mine Mail does not claim support for:

- Outlook Modern Auth or new Outlook connections;
- arbitrary sender HTML editing;
- inline-image composition;
- lossless editing of external HTML, multipart, inline-image, or unsupported
  attachment drafts;
- automatic retry of uncertain SMTP delivery.
