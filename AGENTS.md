# Mine Mail

Cross-platform desktop mail client. Product decisions in this file are durable; ask the user before changing them.

## Architecture

- Desktop only: Tauri 2 + React + Rust + SQLite. Do not build a parallel Web mail runtime.
- Rust/SQLite own credentials, IMAP/SMTP, synchronization, drafts, outbox, and notification decisions. React only calls narrow Tauri commands and renders local state.
- Never log or return authorization secrets, raw credentials, or complete RFC822 messages to React.
- Preserve offline-first startup: render SQLite immediately, then synchronize in Rust.

## MVP behavior

- Inbox sync runs at startup, tray **刷新**, and manual wake; polling is user-selectable at 1/3/5 minutes and defaults to 5 minutes.
- First historical import establishes a notification baseline. Later unread arrivals notify with sender + subject, never body text.
- Closing the window hides it to the tray while background mode is active. Tray labels are exactly **打开 / 刷新 / 退出**.
- Login autostart is a setting and defaults off.
- Drafts synchronize both ways. Editing reuses one stable draft ID; save locally during editing and upload remotely every five minutes.
- Draft editor writes must carry the SQLite `local_version`; stale edits become conflict copies and stale deletes never remove the newer canonical draft. HTML/attachment drafts remain read-only until that MIME is supported.
- Sending binds exact-recipient confirmation and Outbox state to one draft `local_version`. Preserve newer edits, supersede safe older retry items, and never automatically retry `delivery_unknown` items.
- Import the development account from `password.txt` once into the OS credential store. Keep provider presets (163, Gmail, Outlook) and a custom IMAP/SMTP option; users supply account and authorization secret.

## Verification

- Root Rust: `cargo test`
- React: `cd web && npm test -- --run && npm run build`
- Tauri: `cd web/src-tauri && cargo test && cargo check`
- Network acceptance tests may read/sync the configured 163 mailbox. The approved SMTP acceptance recipient is `1193894851@qq.com`; use an unmistakable test subject.
