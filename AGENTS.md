# Mine Mail

Cross-platform desktop mail client. Product decisions in this file are durable; ask the user before changing them.

## Architecture

- Desktop only: Tauri 2 + React + Rust + SQLite. Do not build a parallel Web mail runtime.
- Rust/SQLite own credentials, IMAP/SMTP, synchronization, drafts, outbox, and notification decisions. React only calls narrow Tauri commands and renders local state.
- Never log or return authorization secrets, raw credentials, or complete RFC822 messages to React.
- Preserve offline-first startup: render SQLite immediately, then synchronize in Rust.
- Inbox summaries must not carry raw RFC822 or full HTML. Paint a local preview immediately, hydrate the selected body silently, and prefetch recent bounded-size bodies after sync.
- Keep one reader scrollbar. Simple HTML with a readable text alternative uses the native themed reader; complex sender-designed HTML stays sanitized and isolated in a no-script iframe whose height is owned by the outer reader.
- The HTML boundary is deterministic: wrapper-only HTML with a readable text alternative stays plain regardless of redundant wrapper count or depth. Native semantic HTML strips sender styles and layout attributes and is limited to 32 KiB of structural markup, 100 elements, depth 10, and 3 images; a small DOM without class/id styling hooks may discard an otherwise unused style block. One unnested signature table may use the native reader when it has at most 4 rows, 8 cells, no merged cells, no style block, and no background or positioning layout; it uses a depth limit of 24. Other tables, styled DOM, media/forms, and anything outside those limits use the no-script iframe.

## MVP behavior

- Inbox sync runs at startup, tray **刷新**, and manual wake; polling is user-selectable at 1/3/5 minutes and defaults to 5 minutes.
- First historical import establishes a notification baseline. Later unread arrivals notify with sender + subject, never body text.
- Closing the window hides it to the tray while background mode is active. Tray labels are exactly **打开 / 刷新 / 退出**.
- Login autostart is a setting and defaults off.
- Remote images are user-selectable as automatic/ask/blocked and default to automatic loading. The setting includes a nearby help affordance explaining the privacy risk of automatic remote requests.
- Drafts synchronize both ways. Editing reuses one stable draft ID; save locally during editing and upload remotely every five minutes.
- Closing or pressing Escape never forces a draft save. For a new compose session, close removes any recovery draft created by that session; for an existing draft, close leaves the previously persisted draft intact. Minimize keeps the editing session active.
- Draft editor writes must carry the SQLite `local_version`; stale edits become conflict copies and stale deletes never remove the newer canonical draft. HTML/attachment drafts remain read-only until that MIME is supported.
- Sending binds exact-recipient confirmation and Outbox state to one draft `local_version`. Preserve newer edits, supersede safe older retry items, and never automatically retry `delivery_unknown` items.
- Import the development account from `password.txt` once into the OS credential store. Keep provider presets (163, Gmail, Outlook) and a custom IMAP/SMTP option; users supply account and authorization secret.

## Visual baseline

- The approved MVP material is the layered frosted treatment in `web/design/references/mine-mail-frosted-material-reference.png`: one continuous painterly wallpaper, quieter glass for the message list, more atmospheric glass for the reader, and a theme-tinted compose control. All themes inherit the shared material structure and only tune semantic tokens.
- The native window chrome is visually fused into the app shell: keep a transparent draggable top safe area with the platform window controls in their standard positions, but no separate titlebar surface, divider, title, or duplicate logo. The single Mine Mail brand lives at the top of the left navigation.
- Reader reply and forward actions use the active theme accent as emphasized buttons. Their shared action row spans the native reading width, with reply anchored left and forward anchored right.
- Routine backend health and cache state are not persistent main-interface chrome. The sidebar identifies the configured account without connection labels or status dots, and the mail list has no cache/sync footer; surface only progress tied to an explicit user action and failures that require attention.
- The compose window uses that same layered glass system for its shell, fields, editor, footer, and controls. It has no visible title bar; only minimize and close remain at the top-right. The floating surface can be dragged and resized from every edge/corner, remembers the user's last normal position and size across messages and app restarts, and stays within the visible app bounds.
- Compose address and subject inputs use inset rounded focus surfaces with visible spacing from the grouped field shell. The icon-only copy-recipient toggle expands and collapses CC/BCC without clearing their values.
- A minimized composer is a 340 × 44 subject-only glass bar at the bottom center. It removes the compose backdrop blur and all status/action chrome; clicking the bar restores the exact pre-minimize geometry. An empty subject is shown as **新邮件**.

## Verification

- Root Rust: `cargo test`
- React: `cd web && npm test -- --run && npm run build`
- Tauri: `cd web/src-tauri && cargo test && cargo check`
- Network acceptance tests may read/sync the configured 163 mailbox. The approved SMTP acceptance recipient is `1193894851@qq.com`; use an unmistakable test subject.
