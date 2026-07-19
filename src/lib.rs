//! Reusable local-first mail backend.
//!
//! The CLI in `main.rs` is only an acceptance harness. The future Tauri layer
//! should call [`MailBackend`] directly and must never talk to IMAP/SMTP from
//! the React UI.

mod backend;
mod config;
mod database;
mod error;
mod imap_client;
mod mime;
mod models;
mod smtp_client;

pub use backend::{InboxMonitor, InboxMonitorMode, MailBackend};
pub use config::{AccountConfig, AuthenticationKind, ServerConfig, SmtpSecurity};
pub use error::{MailError, Result};
pub use models::{
    ComposeRequest, ConnectionReport, Draft, DraftDeleteKind, DraftSaveKind, DraftSaveOutcome,
    DraftSyncReport, InboxMessage, MailAddress, OutboxItem, OutboxStatus, ReplyContext, SyncReport,
};

/// Rebuilds the preferred HTML body from the locally cached RFC822 message and
/// resolves safe inline image Content-IDs. The returned HTML is still
/// untrusted mail content and must be sanitized before it crosses a UI
/// boundary.
pub fn render_message_html(message: &InboxMessage) -> Option<String> {
    mime::render_message_html(message)
}

/// Reads only the decoded subject from an immutable Outbox message. This lets
/// desktop callers render sent-mail metadata without exposing the persisted
/// RFC822 payload across the UI boundary or depending on a mutable draft row.
pub fn outbox_subject(item: &OutboxItem) -> Option<String> {
    mime::outbox_subject(&item.raw_rfc822)
}

/// Builds a bounded text preview for an Outbox list row without returning the
/// complete message body or any RFC822 bytes.
pub fn outbox_preview(item: &OutboxItem) -> Option<String> {
    mime::outbox_preview(&item.raw_rfc822)
}

/// Reads the plain-text body from one locally generated Outbox message. This
/// is intended for a narrow, selected-message desktop command.
pub fn outbox_body_text(item: &OutboxItem) -> Option<String> {
    mime::outbox_body_text(&item.raw_rfc822)
}

/// Rebuilds the selected Outbox message's HTML alternative, including safe
/// inline Content-ID images. Desktop callers must still sanitize this value.
pub fn outbox_body_html(item: &OutboxItem) -> Option<String> {
    mime::outbox_body_html(&item.raw_rfc822)
}

/// Reports whether the immutable Outbox message carries standard or Mine Mail
/// reply metadata, without exposing any headers to the UI.
pub fn outbox_has_reply_headers(item: &OutboxItem) -> bool {
    mime::outbox_has_reply_headers(&item.raw_rfc822)
}

/// Reads the stable RFC Message-ID from an immutable Outbox item. It is used
/// only as non-secret metadata when merging the local delivery record with the
/// provider's copy in the Sent mailbox.
pub fn outbox_message_id(item: &OutboxItem) -> Option<String> {
    mime::outbox_message_id(&item.raw_rfc822)
}

/// Reads the RFC Date header from a local Outbox item. Older messages created
/// before Mine Mail added its own Message-ID use this timestamp as one part of
/// a conservative duplicate check.
pub fn outbox_sent_at(item: &OutboxItem) -> Option<String> {
    mime::outbox_sent_at(&item.raw_rfc822)
}
