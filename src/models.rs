use serde::{Deserialize, Serialize};

use crate::{MailError, Result};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct MailAddress {
    pub name: Option<String>,
    pub email: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ComposeRequest {
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
}

impl ComposeRequest {
    pub fn validate(&self) -> Result<()> {
        if self.to.is_empty() && self.cc.is_empty() && self.bcc.is_empty() {
            return Err(MailError::Validation(
                "at least one recipient is required".to_owned(),
            ));
        }
        if self
            .to
            .iter()
            .chain(&self.cc)
            .chain(&self.bcc)
            .any(|address| address.trim().is_empty())
        {
            return Err(MailError::Validation(
                "recipient addresses cannot be blank".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn all_recipients(&self) -> impl Iterator<Item = &String> {
        self.to.iter().chain(&self.cc).chain(&self.bcc)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct InboxMessage {
    pub id: i64,
    pub account_id: String,
    pub mailbox: String,
    pub uid: u32,
    pub message_id: Option<String>,
    pub subject: String,
    pub sender: Option<MailAddress>,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    pub sent_at: Option<String>,
    pub internal_date: Option<String>,
    pub flags: Vec<String>,
    pub size_bytes: u32,
    pub preview: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub attachment_names: Vec<String>,
    pub body_fetched: bool,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub raw_rfc822: Vec<u8>,
    pub synced_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Draft {
    pub id: String,
    pub account_id: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub status: String,
    pub remote_mailbox: Option<String>,
    pub remote_uid: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub raw_rfc822: Vec<u8>,
}

impl Draft {
    pub fn compose_request(&self) -> ComposeRequest {
        ComposeRequest {
            to: self.to.clone(),
            cc: self.cc.clone(),
            bcc: self.bcc.clone(),
            subject: self.subject.clone(),
            body_text: self.body_text.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxStatus {
    Queued,
    Sending,
    Sent,
    Retryable,
    Rejected,
    DeliveryUnknown,
}

impl OutboxStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Retryable => "retryable",
            Self::Rejected => "rejected",
            Self::DeliveryUnknown => "delivery_unknown",
        }
    }

    pub(crate) fn from_str(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "sending" => Ok(Self::Sending),
            "sent" => Ok(Self::Sent),
            "retryable" => Ok(Self::Retryable),
            "rejected" => Ok(Self::Rejected),
            "delivery_unknown" => Ok(Self::DeliveryUnknown),
            other => Err(MailError::Database(rusqlite::Error::InvalidParameterName(
                format!("unknown outbox status {other}"),
            ))),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct OutboxItem {
    pub id: String,
    pub account_id: String,
    pub draft_id: Option<String>,
    pub recipients: Vec<String>,
    pub status: OutboxStatus,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub created_at: String,
    pub sent_at: Option<String>,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub raw_rfc822: Vec<u8>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub mailbox: String,
    pub remote_total: u32,
    pub fetched: usize,
    pub updated_flags: usize,
    pub removed: usize,
    pub cached_total: usize,
    pub uid_validity_reset: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct ConnectionReport {
    pub imap_ok: bool,
    pub smtp_ok: bool,
}
