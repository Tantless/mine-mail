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

pub use backend::MailBackend;
pub use config::{AccountConfig, ServerConfig, SmtpSecurity};
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
