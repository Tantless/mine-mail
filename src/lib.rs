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
