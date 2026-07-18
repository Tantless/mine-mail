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
    DraftSyncReport, InboxMessage, MailAddress, OutboxItem, OutboxStatus, SyncReport,
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
