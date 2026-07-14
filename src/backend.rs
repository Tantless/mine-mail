use std::{
    collections::{BTreeSet, HashSet},
    path::Path,
};

use chrono::{SecondsFormat, Utc};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    AccountConfig, ComposeRequest, ConnectionReport, Draft, InboxMessage, MailError, OutboxItem,
    OutboxStatus, Result, SyncReport,
    database::{MailboxState, Repository},
    imap_client::ImapConnection,
    mime::{IncomingMetadata, build_draft_message, build_outgoing_message, parse_incoming_message},
    smtp_client::SmtpClient,
};

const INBOX: &str = "INBOX";
const SUMMARY_BATCH_SIZE: usize = 100;
const FLAG_BATCH_SIZE: usize = 250;
const MAX_CACHED_MESSAGE_BYTES: u32 = 50 * 1024 * 1024;

/// Reusable application service for the future Tauri command layer.
///
/// The React UI must call this service through narrowly scoped Tauri commands;
/// it should never receive the authorization password or open IMAP/SMTP itself.
pub struct MailBackend {
    config: AccountConfig,
    repository: Repository,
    imap_gate: Mutex<()>,
    smtp_gate: Mutex<()>,
}

impl MailBackend {
    pub fn open(config: AccountConfig, database_path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            config,
            repository: Repository::open(database_path)?,
            imap_gate: Mutex::new(()),
            smtp_gate: Mutex::new(()),
        })
    }

    pub fn initialize(&self) -> Result<()> {
        self.repository.initialize_account(&self.config)?;
        self.repository.recover_sending_as_delivery_unknown()?;
        Ok(())
    }

    pub async fn check_connections(&self) -> Result<ConnectionReport> {
        let imap_ok = {
            let _guard = self.imap_gate.lock().await;
            match ImapConnection::connect(&self.config).await {
                Ok(connection) => connection.probe().await.is_ok(),
                Err(_) => false,
            }
        };

        let smtp_ok = {
            let _guard = self.smtp_gate.lock().await;
            match SmtpClient::new(&self.config) {
                Ok(client) => client.probe().await.is_ok(),
                Err(_) => false,
            }
        };

        Ok(ConnectionReport { imap_ok, smtp_ok })
    }

    pub async fn list_remote_mailboxes(&self) -> Result<Vec<String>> {
        let _guard = self.imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config).await?;
        let mut names: Vec<String> = connection
            .list_mailboxes()
            .await?
            .into_iter()
            .map(|mailbox| mailbox.name)
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        let _ = connection.logout().await;
        Ok(names)
    }

    /// Synchronize Inbox metadata without downloading message bodies.
    ///
    /// On the first run only the newest `initial_limit` messages are cached.
    /// Later runs fetch new UIDs, reconcile flags and remove locally cached UIDs
    /// that no longer exist on the server.
    pub async fn sync_inbox(&self, initial_limit: usize) -> Result<SyncReport> {
        if initial_limit == 0 {
            return Err(MailError::Validation(
                "initial sync limit must be greater than zero".to_owned(),
            ));
        }

        let _guard = self.imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config).await?;
        let snapshot = connection.select_inbox().await?;

        if snapshot.exists > 0 && snapshot.all_uids.is_empty() {
            return Err(MailError::Imap(
                "server reported Inbox messages but returned an empty UID search; local cache was left unchanged"
                    .to_owned(),
            ));
        }

        let previous_state = self
            .repository
            .mailbox_state(&self.config.account_id, INBOX)?;
        let uid_validity_reset = previous_state
            .as_ref()
            .and_then(|state| state.uid_validity)
            .zip(snapshot.uid_validity)
            .is_some_and(|(local, remote)| local != remote);

        if uid_validity_reset {
            self.repository
                .reset_mailbox(&self.config.account_id, INBOX)?;
        }

        let cached_uids = self
            .repository
            .cached_uids(&self.config.account_id, INBOX)?;
        let remote_uids: HashSet<u32> = snapshot.all_uids.iter().copied().collect();
        let removed =
            self.repository
                .delete_missing_uids(&self.config.account_id, INBOX, &remote_uids)?;

        let previous_highest_uid = if uid_validity_reset {
            None
        } else {
            previous_state.as_ref().and_then(|state| state.highest_uid)
        };

        let mut requested = BTreeSet::new();
        for uid in snapshot.all_uids.iter().rev().take(initial_limit) {
            if !cached_uids.contains(uid) {
                requested.insert(*uid);
            }
        }
        if let Some(highest_uid) = previous_highest_uid {
            for uid in snapshot
                .all_uids
                .iter()
                .copied()
                .filter(|uid| *uid > highest_uid && !cached_uids.contains(uid))
            {
                requested.insert(uid);
            }
        }

        let requested: Vec<u32> = requested.into_iter().collect();
        let mut fetched = 0;
        for batch in requested.chunks(SUMMARY_BATCH_SIZE) {
            for remote in connection.fetch_summaries(batch).await? {
                let message = parse_incoming_message(
                    &remote.raw,
                    IncomingMetadata {
                        account_id: &self.config.account_id,
                        mailbox: INBOX,
                        uid: remote.uid,
                        flags: remote.flags,
                        internal_date: remote.internal_date,
                        size_bytes: remote.size_bytes,
                        synced_at: now(),
                        body_fetched: false,
                    },
                )?;
                self.repository.upsert_message(&message)?;
                fetched += 1;
            }
        }

        let existing_remote_uids: Vec<u32> =
            cached_uids.intersection(&remote_uids).copied().collect();
        let mut updated_flags = 0;
        for batch in existing_remote_uids.chunks(FLAG_BATCH_SIZE) {
            for (uid, flags) in connection.fetch_flags(batch).await? {
                self.repository.update_message_flags(
                    &self.config.account_id,
                    INBOX,
                    uid,
                    &flags,
                )?;
                updated_flags += 1;
            }
        }

        self.repository.upsert_mailbox_state(&MailboxState {
            account_id: self.config.account_id.clone(),
            mailbox: INBOX.to_owned(),
            uid_validity: snapshot.uid_validity,
            uid_next: snapshot.uid_next,
            highest_uid: snapshot.all_uids.last().copied(),
            highest_modseq: snapshot.highest_modseq,
            last_synced_at: Some(now()),
        })?;

        let cached_total = self
            .repository
            .count_messages(&self.config.account_id, INBOX)?;
        let _ = connection.logout().await;

        Ok(SyncReport {
            mailbox: INBOX.to_owned(),
            remote_total: snapshot.exists,
            fetched,
            updated_flags,
            removed,
            cached_total,
            uid_validity_reset,
        })
    }

    pub fn list_inbox(&self, limit: usize) -> Result<Vec<InboxMessage>> {
        if limit == 0 {
            return Err(MailError::Validation(
                "Inbox list limit must be greater than zero".to_owned(),
            ));
        }
        self.repository
            .list_inbox(&self.config.account_id, limit, 0)
    }

    pub async fn fetch_message(&self, uid: u32, force: bool) -> Result<InboxMessage> {
        if uid == 0 {
            return Err(MailError::Validation(
                "message UID must be greater than zero".to_owned(),
            ));
        }

        match self
            .repository
            .get_message_by_uid(&self.config.account_id, INBOX, uid)
        {
            Ok(message) if message.body_fetched && !force => return Ok(message),
            Ok(message) if message.size_bytes > MAX_CACHED_MESSAGE_BYTES => {
                return Err(MailError::Validation(format!(
                    "message UID {uid} exceeds the 50 MiB local cache limit"
                )));
            }
            Ok(_) | Err(MailError::NotFound { .. }) => {}
            Err(error) => return Err(error),
        }

        let _guard = self.imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config).await?;
        connection.select_inbox().await?;
        let remote = connection.fetch_full_message(uid).await?;
        let _ = connection.logout().await;

        if remote.size_bytes > MAX_CACHED_MESSAGE_BYTES {
            return Err(MailError::Validation(format!(
                "message UID {uid} exceeds the 50 MiB local cache limit"
            )));
        }

        let message = parse_incoming_message(
            &remote.raw,
            IncomingMetadata {
                account_id: &self.config.account_id,
                mailbox: INBOX,
                uid: remote.uid,
                flags: remote.flags,
                internal_date: remote.internal_date,
                size_bytes: remote.size_bytes,
                synced_at: now(),
                body_fetched: true,
            },
        )?;
        self.repository.upsert_message(&message)?;
        self.repository
            .get_message_by_uid(&self.config.account_id, INBOX, uid)
    }

    pub fn save_draft(&self, request: ComposeRequest) -> Result<Draft> {
        validate_draft_recipients(&request)?;
        let id = Uuid::now_v7().to_string();
        let timestamp = now();
        let raw_rfc822 = build_draft_message(&self.config.email, &request, &id)?;
        let draft = Draft {
            id,
            account_id: self.config.account_id.clone(),
            to: request.to,
            cc: request.cc,
            bcc: request.bcc,
            subject: request.subject,
            body_text: request.body_text,
            status: "local".to_owned(),
            remote_mailbox: None,
            remote_uid: None,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            raw_rfc822,
        };
        self.repository.save_draft(&draft)?;
        self.repository.get_draft(&draft.id)
    }

    pub fn list_drafts(&self) -> Result<Vec<Draft>> {
        self.repository.list_drafts(&self.config.account_id)
    }

    pub async fn sync_draft(
        &self,
        draft_id: &str,
        mailbox_override: Option<&str>,
    ) -> Result<Draft> {
        let draft = self.repository.get_draft(draft_id)?;
        if draft.status == "sent" {
            return Err(MailError::Validation(
                "a sent draft cannot be uploaded again".to_owned(),
            ));
        }
        if draft.status == "synced" {
            return Ok(draft);
        }

        let _guard = self.imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config).await?;
        let (mailbox, remote_uid) = connection
            .append_draft(&draft.id, &draft.raw_rfc822, mailbox_override)
            .await?;
        let _ = connection.logout().await;

        // async-imap does not expose APPENDUID. The stable private header is
        // searched after APPEND; if the server indexes it later, the mailbox
        // is still marked synced so a manual retry cannot create a duplicate.
        self.repository
            .mark_draft_synced(&draft.id, &mailbox, remote_uid)?;
        self.repository.get_draft(&draft.id)
    }

    pub async fn send_compose(&self, request: ComposeRequest) -> Result<OutboxItem> {
        self.send_request(request, None).await
    }

    pub async fn send_draft(&self, draft_id: &str) -> Result<OutboxItem> {
        let draft = self.repository.get_draft(draft_id)?;
        if draft.status == "sent" {
            return Err(MailError::Validation(
                "this draft has already been sent".to_owned(),
            ));
        }
        self.send_request(draft.compose_request(), Some(draft_id.to_owned()))
            .await
    }

    pub fn list_outbox(&self) -> Result<Vec<OutboxItem>> {
        self.repository.list_outbox(&self.config.account_id)
    }

    async fn send_request(
        &self,
        request: ComposeRequest,
        draft_id: Option<String>,
    ) -> Result<OutboxItem> {
        if let Some(draft_id) = draft_id.as_deref()
            && let Some(existing) = self.repository.get_outbox_by_draft(draft_id)?
        {
            return Err(MailError::Validation(format!(
                "this draft already has an outbox item with status '{}'; it will not be sent again",
                existing.status.as_str()
            )));
        }

        let outgoing = build_outgoing_message(&self.config.email, &request)?;
        let outbox_id = Uuid::now_v7().to_string();
        let queued = OutboxItem {
            id: outbox_id.clone(),
            account_id: self.config.account_id.clone(),
            draft_id: draft_id.clone(),
            recipients: outgoing.recipients,
            status: OutboxStatus::Queued,
            attempts: 0,
            last_error: None,
            created_at: now(),
            sent_at: None,
            raw_rfc822: outgoing.raw_rfc822.clone(),
        };
        self.repository.enqueue_outbox(&queued)?;

        let _guard = self.smtp_gate.lock().await;
        let client = match SmtpClient::new(&self.config) {
            Ok(client) => client,
            Err(error) => {
                self.repository.update_outbox_status(
                    &outbox_id,
                    OutboxStatus::Retryable,
                    Some(&error.to_string()),
                )?;
                return self.repository.get_outbox(&outbox_id);
            }
        };

        self.repository
            .update_outbox_status(&outbox_id, OutboxStatus::Sending, None)?;
        match client
            .send_raw(&outgoing.envelope, &outgoing.raw_rfc822)
            .await
        {
            Ok(()) => {
                if let Some(draft_id) = draft_id.as_deref() {
                    self.repository
                        .mark_outbox_and_draft_sent(&outbox_id, draft_id)?;
                } else {
                    self.repository
                        .update_outbox_status(&outbox_id, OutboxStatus::Sent, None)?;
                }
            }
            Err(failure) => {
                self.repository.update_outbox_status(
                    &outbox_id,
                    failure.status,
                    Some(&failure.safe_reason),
                )?;
            }
        }

        self.repository.get_outbox(&outbox_id)
    }
}

fn validate_draft_recipients(request: &ComposeRequest) -> Result<()> {
    if request
        .all_recipients()
        .any(|address| address.trim().is_empty())
    {
        return Err(MailError::Validation(
            "draft recipient addresses cannot be blank".to_owned(),
        ));
    }
    Ok(())
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::MailBackend;
    use crate::{AccountConfig, ComposeRequest};

    #[test]
    fn saves_an_incomplete_local_draft_without_network() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");

        let saved = backend
            .save_draft(ComposeRequest {
                to: Vec::new(),
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "unfinished".to_owned(),
                body_text: "local text".to_owned(),
            })
            .expect("save draft");

        assert_eq!(saved.status, "local");
        assert_eq!(backend.list_drafts().expect("drafts").len(), 1);
    }
}
