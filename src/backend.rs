use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::Path,
    time::Duration,
};

use chrono::{SecondsFormat, Utc};
use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

use crate::{
    AccountConfig, ComposeRequest, ConnectionReport, Draft, DraftDeleteKind, DraftSaveKind,
    DraftSaveOutcome, InboxMessage, MailError, OutboxItem, OutboxStatus, Result, SyncReport,
    database::{DraftRecord, MailboxState, Repository},
    imap_client::{ImapConnection, MailboxHint, RemoteMessage},
    mime::{
        IncomingMetadata, build_draft_message_revision, build_outgoing_message,
        parse_draft_message, parse_incoming_message, parse_incoming_summary_or_fallback,
        render_message_html, restore_outbox_envelope,
    },
    models::DraftSyncReport,
    smtp_client::SmtpClient,
};

const INBOX: &str = "INBOX";
const SUMMARY_BATCH_SIZE: usize = 100;
const FLAG_BATCH_SIZE: usize = 250;
const MAX_CACHED_MESSAGE_BYTES: u32 = 50 * 1024 * 1024;
const MAX_LOCAL_DRAFT_CAS_RETRIES: usize = 32;
const BODY_IMAP_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(60);

struct BodyImapSession {
    connection: ImapConnection,
    last_used: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InboxMonitorMode {
    Idle,
    LightweightPoll,
}

/// One authenticated, selected IMAP connection dedicated to detecting Inbox
/// changes. It never writes SQLite and never crosses the Tauri command layer.
pub struct InboxMonitor {
    connection: Option<ImapConnection>,
    mode: InboxMonitorMode,
    last_hint: MailboxHint,
}

impl InboxMonitor {
    pub fn mode(&self) -> InboxMonitorMode {
        self.mode
    }

    /// Wait for one server-pushed IDLE event. The connection is restored with
    /// DONE before returning so a subsequent cycle can safely begin.
    pub async fn wait_for_idle_change(&mut self, duration: Duration) -> Result<bool> {
        if self.mode != InboxMonitorMode::Idle {
            return Err(MailError::Validation(
                "this Inbox monitor does not support IDLE".to_owned(),
            ));
        }
        let connection = self.connection.take().ok_or_else(|| {
            MailError::Imap("the Inbox monitor connection is unavailable".to_owned())
        })?;
        let (connection, changed) = connection.wait_for_idle_change(duration).await?;
        self.connection = Some(connection);
        Ok(changed)
    }

    /// Probe a non-IDLE server over the existing authenticated connection.
    /// NOOP keeps the session healthy; SELECT reads only mailbox counters and
    /// does not enumerate or download messages.
    pub async fn poll_for_change(&mut self) -> Result<bool> {
        if self.mode != InboxMonitorMode::LightweightPoll {
            return Err(MailError::Validation(
                "this Inbox monitor uses IDLE instead of polling".to_owned(),
            ));
        }
        let connection = self.connection.as_mut().ok_or_else(|| {
            MailError::Imap("the Inbox monitor connection is unavailable".to_owned())
        })?;
        connection.noop().await?;
        let next = connection.select_inbox_hint().await?;
        let changed = mailbox_hint_changed(self.last_hint, next);
        self.last_hint = next;
        Ok(changed)
    }
}

#[derive(Clone, Debug)]
struct RemoteDraftCandidate {
    id: String,
    revision: u64,
    uid: u32,
    uid_validity: Option<u32>,
    has_unsupported_content: bool,
    request: ComposeRequest,
    raw_rfc822: Vec<u8>,
    updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConfirmedDraftSnapshot {
    id: String,
    revision: u64,
    local_version: u64,
    request: ComposeRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DraftReconciliation {
    InSync,
    PushLocal,
    PullRemote,
    Conflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InboxUidScope {
    Current,
    NeedsSync,
    Changed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RemoteForkPreservation {
    Inserted,
    AlreadyPreserved,
    IdentityCollision,
}

/// Reusable application service for the future Tauri command layer.
///
/// The React UI must call this service through narrowly scoped Tauri commands;
/// it should never receive the authorization password or open IMAP/SMTP itself.
pub struct MailBackend {
    config: AccountConfig,
    repository: Repository,
    imap_gate: Mutex<()>,
    body_imap: Mutex<Option<BodyImapSession>>,
    smtp_gate: Mutex<()>,
}

impl MailBackend {
    pub fn open(config: AccountConfig, database_path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            config,
            repository: Repository::open(database_path)?,
            imap_gate: Mutex::new(()),
            body_imap: Mutex::new(None),
            smtp_gate: Mutex::new(()),
        })
    }

    pub fn initialize(&self) -> Result<()> {
        self.repository.initialize_account(&self.config)?;
        // Current senders create queued and claim sending inside one SQLite
        // transaction, so a visible queued row can only be a legacy/crashed
        // item from an older lifecycle and is safe to expose for manual retry.
        self.repository.recover_queued_as_retryable()?;
        self.repository.recover_sending_as_delivery_unknown()?;
        Ok(())
    }

    pub async fn connect_inbox_monitor(&self) -> Result<InboxMonitor> {
        let mut connection = ImapConnection::connect(&self.config).await?;
        let last_hint = connection.select_inbox_hint().await?;
        let mode = if connection.supports_idle() {
            InboxMonitorMode::Idle
        } else {
            InboxMonitorMode::LightweightPoll
        };
        Ok(InboxMonitor {
            connection: Some(connection),
            mode,
            last_hint,
        })
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
        self.validate_sync_limit(initial_limit)?;

        let _guard = self.imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config).await?;
        let report = self
            .sync_selected_mailbox(&mut connection, INBOX, initial_limit)
            .await;
        let _ = connection.logout().await;
        report
    }

    /// Synchronize the server-designated Sent mailbox. The discovered mailbox
    /// name is persisted as a role so all later local reads stay offline-first
    /// and do not have to guess provider-specific or localized folder names.
    pub async fn sync_sent(&self, initial_limit: usize) -> Result<SyncReport> {
        self.validate_sync_limit(initial_limit)?;

        let _guard = self.imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config).await?;
        let mailbox = connection.discover_sent_mailbox().await?;
        let report = self
            .sync_selected_mailbox(&mut connection, &mailbox, initial_limit)
            .await;
        let _ = connection.logout().await;
        let report = report?;
        self.repository
            .assign_mailbox_role(&self.config.account_id, "sent", &mailbox)?;
        Ok(report)
    }

    fn validate_sync_limit(&self, initial_limit: usize) -> Result<()> {
        if initial_limit == 0 {
            return Err(MailError::Validation(
                "initial sync limit must be greater than zero".to_owned(),
            ));
        }
        Ok(())
    }

    async fn sync_selected_mailbox(
        &self,
        connection: &mut ImapConnection,
        mailbox: &str,
        initial_limit: usize,
    ) -> Result<SyncReport> {
        let snapshot = connection.select_mailbox(mailbox).await?;

        if snapshot.exists > 0 && snapshot.all_uids.is_empty() {
            return Err(MailError::Imap(
                "server reported mailbox messages but returned an empty UID search; local cache was left unchanged"
                    .to_owned(),
            ));
        }

        let previous_state = self
            .repository
            .mailbox_state(&self.config.account_id, mailbox)?;
        let uid_validity_reset = previous_state
            .as_ref()
            .and_then(|state| state.uid_validity)
            .zip(snapshot.uid_validity)
            .is_some_and(|(local, remote)| local != remote);

        if uid_validity_reset {
            self.repository
                .reset_mailbox(&self.config.account_id, mailbox)?;
        }

        let cached_uids = self
            .repository
            .cached_uids(&self.config.account_id, mailbox)?;
        let remote_uids: HashSet<u32> = snapshot.all_uids.iter().copied().collect();
        let removed =
            self.repository
                .delete_missing_uids(&self.config.account_id, mailbox, &remote_uids)?;

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
                let message = parse_incoming_summary_or_fallback(
                    &remote.raw,
                    IncomingMetadata {
                        account_id: &self.config.account_id,
                        mailbox,
                        uid: remote.uid,
                        flags: remote.flags,
                        internal_date: remote.internal_date,
                        size_bytes: remote.size_bytes,
                        synced_at: now(),
                        body_fetched: false,
                    },
                );
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
                    mailbox,
                    uid,
                    &flags,
                )?;
                updated_flags += 1;
            }
        }

        self.repository.upsert_mailbox_state(&MailboxState {
            account_id: self.config.account_id.clone(),
            mailbox: mailbox.to_owned(),
            uid_validity: snapshot.uid_validity,
            uid_next: snapshot.uid_next,
            highest_uid: snapshot.all_uids.last().copied(),
            highest_modseq: snapshot.highest_modseq,
            last_synced_at: Some(now()),
        })?;

        let cached_total = self
            .repository
            .count_messages(&self.config.account_id, mailbox)?;

        Ok(SyncReport {
            mailbox: mailbox.to_owned(),
            remote_total: snapshot.exists,
            fetched,
            updated_flags,
            removed,
            cached_total,
            uid_validity_reset,
        })
    }

    /// Fetch only UIDs newer than the committed SQLite cursor. Deletions,
    /// historical flag changes, and UIDVALIDITY recovery intentionally remain
    /// the job of the periodic full reconciliation in [`Self::sync_inbox`].
    pub async fn sync_new_inbox(&self, initial_limit: usize) -> Result<SyncReport> {
        if initial_limit == 0 {
            return Err(MailError::Validation(
                "initial sync limit must be greater than zero".to_owned(),
            ));
        }

        let guard = self.imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config).await?;
        let hint = connection.select_inbox_hint().await?;
        let previous_state = self
            .repository
            .mailbox_state(&self.config.account_id, INBOX)?;
        let needs_full_sync = previous_state.as_ref().is_none_or(|state| {
            state.highest_uid.is_none()
                || classify_inbox_uid_scope(state.uid_validity, hint.uid_validity)
                    != InboxUidScope::Current
        });
        if needs_full_sync {
            let _ = connection.logout().await;
            drop(guard);
            return self.sync_inbox(initial_limit).await;
        }

        let previous_state = previous_state.expect("full sync fallback handles a missing cursor");
        let previous_highest_uid = previous_state
            .highest_uid
            .expect("full sync fallback handles a missing highest UID");
        let requested = connection.search_uids_after(previous_highest_uid).await?;
        let mut fetched = 0;
        for batch in requested.chunks(SUMMARY_BATCH_SIZE) {
            for remote in connection.fetch_summaries(batch).await? {
                let message = parse_incoming_summary_or_fallback(
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
                );
                self.repository.upsert_message(&message)?;
                fetched += 1;
            }
        }

        let highest_uid = requested
            .last()
            .copied()
            .unwrap_or(previous_highest_uid)
            .max(previous_highest_uid);
        self.repository.upsert_mailbox_state(&MailboxState {
            account_id: self.config.account_id.clone(),
            mailbox: INBOX.to_owned(),
            uid_validity: hint.uid_validity.or(previous_state.uid_validity),
            uid_next: hint.uid_next,
            highest_uid: Some(highest_uid),
            highest_modseq: previous_state.highest_modseq,
            last_synced_at: Some(now()),
        })?;
        let cached_total = self
            .repository
            .count_messages(&self.config.account_id, INBOX)?;
        let _ = connection.logout().await;

        Ok(SyncReport {
            mailbox: INBOX.to_owned(),
            remote_total: hint.exists,
            fetched,
            updated_flags: 0,
            removed: 0,
            cached_total,
            uid_validity_reset: false,
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

    pub fn list_sent(&self, limit: usize) -> Result<Vec<InboxMessage>> {
        if limit == 0 {
            return Err(MailError::Validation(
                "Sent list limit must be greater than zero".to_owned(),
            ));
        }
        let mailbox = match self
            .repository
            .mailbox_for_role(&self.config.account_id, "sent")
        {
            Ok(mailbox) => mailbox,
            // Before the first successful network sync there is no discovered
            // role yet. An empty local view preserves offline-first startup.
            Err(MailError::NotFound { .. }) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        self.repository
            .list_mailbox(&self.config.account_id, &mailbox, limit, 0)
    }

    pub fn cached_inbox_message(&self, uid: u32) -> Result<InboxMessage> {
        self.cached_mailbox_message(INBOX, uid)
    }

    pub fn cached_sent_message(&self, uid: u32) -> Result<InboxMessage> {
        let mailbox = self
            .repository
            .mailbox_for_role(&self.config.account_id, "sent")?;
        self.cached_mailbox_message(&mailbox, uid)
    }

    /// Resolve the direct parent of a reply from SQLite without opening IMAP.
    /// This supplies display metadata for provider reply templates that carry
    /// only a Message-ID reference around their quoted body.
    pub fn cached_reply_parent(&self, message: &InboxMessage) -> Result<Option<InboxMessage>> {
        for message_id in message
            .in_reply_to
            .iter()
            .chain(message.references.iter().rev())
        {
            if let Some(parent) = self
                .repository
                .find_message_by_message_id(&self.config.account_id, message_id)?
                && parent.id != message.id
            {
                return Ok(Some(parent));
            }
        }
        Ok(None)
    }

    fn cached_mailbox_message(&self, mailbox: &str, uid: u32) -> Result<InboxMessage> {
        let message = self
            .repository
            .get_message_by_uid(&self.config.account_id, mailbox, uid)?;
        if !message.body_fetched {
            return Err(MailError::NotFound {
                entity: "cached message body",
                id: format!("{mailbox}/{uid}"),
            });
        }
        self.repair_cached_inline_images(message)
    }

    /// Older cache rows may predate CID resolution. Rebuild only those HTML
    /// bodies that still contain an inline-image reference, then persist the
    /// repaired body so later opens stay on the fast SQLite path.
    fn repair_cached_inline_images(&self, mut message: InboxMessage) -> Result<InboxMessage> {
        let needs_repair = !message.raw_rfc822.is_empty()
            && message.body_html.as_deref().is_some_and(|html| {
                let lower = html.to_ascii_lowercase();
                lower.contains("<img") && lower.contains("cid:")
            });
        if !needs_repair {
            return Ok(message);
        }

        let Some(rendered) = render_message_html(&message) else {
            return Ok(message);
        };
        if message.body_html.as_deref() == Some(rendered.as_str()) {
            return Ok(message);
        }
        message.body_html = Some(rendered);
        let mailbox = message.mailbox.clone();
        self.repository.upsert_message(&message)?;
        self.repository
            .get_message_by_uid(&self.config.account_id, &mailbox, message.uid)
    }

    pub async fn prefetch_inbox_bodies(
        &self,
        limit: usize,
        max_total_bytes: u64,
        max_message_bytes: u32,
    ) -> Result<usize> {
        self.prefetch_mailbox_bodies(INBOX, limit, max_total_bytes, max_message_bytes)
            .await
    }

    pub async fn prefetch_sent_bodies(
        &self,
        limit: usize,
        max_total_bytes: u64,
        max_message_bytes: u32,
    ) -> Result<usize> {
        let mailbox = self
            .repository
            .mailbox_for_role(&self.config.account_id, "sent")?;
        self.prefetch_mailbox_bodies(&mailbox, limit, max_total_bytes, max_message_bytes)
            .await
    }

    async fn prefetch_mailbox_bodies(
        &self,
        mailbox: &str,
        limit: usize,
        max_total_bytes: u64,
        max_message_bytes: u32,
    ) -> Result<usize> {
        if limit == 0 || max_total_bytes == 0 || max_message_bytes == 0 {
            return Ok(0);
        }
        let candidates = self.repository.mailbox_body_prefetch_candidates(
            &self.config.account_id,
            mailbox,
            limit,
            max_message_bytes,
        )?;
        let mut prefetched = 0;
        let mut total_bytes = 0u64;
        for (uid, size_bytes) in candidates {
            let next_total = total_bytes.saturating_add(u64::from(size_bytes));
            if next_total > max_total_bytes {
                continue;
            }
            if self
                .fetch_mailbox_message(mailbox, uid, false)
                .await
                .is_ok()
            {
                total_bytes = next_total;
                prefetched += 1;
            }
        }
        Ok(prefetched)
    }

    pub async fn fetch_message(&self, uid: u32, force: bool) -> Result<InboxMessage> {
        self.fetch_mailbox_message(INBOX, uid, force).await
    }

    pub async fn fetch_sent_message(&self, uid: u32, force: bool) -> Result<InboxMessage> {
        let mailbox = self
            .repository
            .mailbox_for_role(&self.config.account_id, "sent")?;
        self.fetch_mailbox_message(&mailbox, uid, force).await
    }

    async fn fetch_mailbox_message(
        &self,
        mailbox: &str,
        uid: u32,
        force: bool,
    ) -> Result<InboxMessage> {
        if uid == 0 {
            return Err(MailError::Validation(
                "message UID must be greater than zero".to_owned(),
            ));
        }

        match self
            .repository
            .get_message_by_uid(&self.config.account_id, mailbox, uid)
        {
            Ok(message) if message.body_fetched && !force => {
                return self.repair_cached_inline_images(message);
            }
            Ok(message) if message.size_bytes > MAX_CACHED_MESSAGE_BYTES => {
                return Err(MailError::Validation(format!(
                    "message UID {uid} exceeds the 50 MiB local cache limit"
                )));
            }
            Ok(_) | Err(MailError::NotFound { .. }) => {}
            Err(error) => return Err(error),
        }

        let mut body_imap = self.body_imap.lock().await;
        let connection_is_stale = match body_imap.as_mut() {
            Some(session) if session.last_used.elapsed() >= BODY_IMAP_KEEPALIVE_INTERVAL => {
                session.connection.noop().await.is_err()
            }
            Some(_) => false,
            None => true,
        };
        if connection_is_stale {
            *body_imap = Some(BodyImapSession {
                connection: ImapConnection::connect(&self.config).await?,
                last_used: Instant::now(),
            });
        }

        // A foreground request may have queued behind a prefetch of the same
        // UID. Recheck SQLite after acquiring the body-session actor.
        if !force
            && let Ok(message) =
                self.repository
                    .get_message_by_uid(&self.config.account_id, mailbox, uid)
            && message.body_fetched
        {
            return self.repair_cached_inline_images(message);
        }

        let session = body_imap
            .as_mut()
            .expect("body IMAP session is connected before use");
        let result = async {
            let selected_uid_validity = session
                .connection
                .select_mailbox_for_fetch(mailbox)
                .await?;
            let local_uid_validity = self
                .repository
                .mailbox_state(&self.config.account_id, mailbox)?
                .and_then(|state| state.uid_validity);
            match classify_inbox_uid_scope(local_uid_validity, selected_uid_validity) {
                InboxUidScope::Current => {}
                InboxUidScope::NeedsSync => {
                    return Err(MailError::Validation(
                        "Mailbox must be synchronized before downloading message bodies".to_owned(),
                    ));
                }
                InboxUidScope::Changed => {
                    self.repository
                        .reset_mailbox(&self.config.account_id, mailbox)?;
                    return Err(MailError::Validation(
                        "Mailbox UIDVALIDITY changed; synchronize the mailbox before downloading this message"
                            .to_owned(),
                    ));
                }
            }
            let remote = session.connection.fetch_full_message(uid).await?;

            if remote.size_bytes > MAX_CACHED_MESSAGE_BYTES {
                return Err(MailError::Validation(format!(
                    "message UID {uid} exceeds the 50 MiB local cache limit"
                )));
            }

            let message = parse_incoming_message(
                &remote.raw,
                IncomingMetadata {
                    account_id: &self.config.account_id,
                    mailbox,
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
                .get_message_by_uid(&self.config.account_id, mailbox, uid)
        }
        .await;
        match result {
            Ok(message) => {
                session.last_used = Instant::now();
                Ok(message)
            }
            Err(error) => {
                *body_imap = None;
                Err(error)
            }
        }
    }

    pub fn save_draft(&self, request: ComposeRequest) -> Result<Draft> {
        self.upsert_draft(None, request)
    }

    /// Create a draft or update an existing draft while retaining its stable
    /// identity. Updates increment the private draft revision used by the IMAP
    /// reconciliation algorithm.
    pub fn upsert_draft(&self, draft_id: Option<&str>, request: ComposeRequest) -> Result<Draft> {
        validate_draft_recipients(&request)?;
        match draft_id {
            None => return self.insert_local_draft(&request, "local"),
            Some(id) => {
                for _ in 0..MAX_LOCAL_DRAFT_CAS_RETRIES {
                    let expected = self.repository.get_draft_record(id)?;
                    if expected.draft.account_id != self.config.account_id {
                        return Err(MailError::NotFound {
                            entity: "draft",
                            id: id.to_owned(),
                        });
                    }
                    if expected.draft.status == "sent" {
                        return Err(MailError::Validation(
                            "a sent draft cannot be edited".to_owned(),
                        ));
                    }
                    if expected.draft.has_unsupported_content {
                        return Err(MailError::Validation(
                            "this draft contains HTML, attachments, or other unsupported MIME content and is read-only"
                                .to_owned(),
                        ));
                    }

                    let mut replacement = expected.clone();
                    replacement.revision = expected.revision.checked_add(1).ok_or_else(|| {
                        MailError::Validation("draft revision limit reached".to_owned())
                    })?;
                    replacement.local_version =
                        expected.local_version.checked_add(1).ok_or_else(|| {
                            MailError::Validation("draft local version limit reached".to_owned())
                        })?;
                    replacement.draft.local_version = replacement.local_version;
                    replacement.draft.to = request.to.clone();
                    replacement.draft.cc = request.cc.clone();
                    replacement.draft.bcc = request.bcc.clone();
                    replacement.draft.subject = request.subject.clone();
                    replacement.draft.body_text = request.body_text.clone();
                    replacement.draft.status = "local".to_owned();
                    replacement.draft.updated_at = now();
                    replacement.is_deleted = false;
                    replacement.draft.raw_rfc822 = build_draft_message_revision(
                        &self.config.email,
                        &request,
                        id,
                        replacement.revision,
                    )?;

                    if self
                        .repository
                        .replace_draft_if_unchanged(&expected, &replacement, None)?
                    {
                        return Ok(replacement.draft);
                    }
                }
            }
        }

        Err(MailError::Validation(
            "draft changed too frequently; save it again".to_owned(),
        ))
    }

    /// Save against the exact local row version the caller opened. A stale or
    /// deleted base is never overwritten: the caller's current content is
    /// inserted as a new local conflict copy instead.
    pub fn save_draft_optimistic(
        &self,
        draft_id: Option<&str>,
        expected_local_version: Option<u64>,
        request: ComposeRequest,
    ) -> Result<DraftSaveOutcome> {
        validate_draft_recipients(&request)?;
        match (draft_id, expected_local_version) {
            (None, None) => {
                let draft = self.insert_local_draft(&request, "local")?;
                Ok(DraftSaveOutcome {
                    kind: DraftSaveKind::Saved,
                    draft,
                    canonical: None,
                })
            }
            (None, Some(_)) => Err(MailError::Validation(
                "a new draft cannot have an expected local version".to_owned(),
            )),
            (Some(_), None) => Err(MailError::Validation(
                "an existing draft requires its expected local version".to_owned(),
            )),
            (Some(id), Some(expected_local_version)) => {
                let current = match self.repository.get_draft_record(id) {
                    Ok(record) if record.draft.account_id == self.config.account_id => Some(record),
                    Ok(_) => None,
                    Err(MailError::NotFound { .. }) => None,
                    Err(error) => return Err(error),
                };

                if let Some(expected) = current.as_ref()
                    && !expected.is_deleted
                    && expected.draft.status != "sent"
                    && expected.local_version == expected_local_version
                {
                    if expected.draft.has_unsupported_content {
                        return Err(MailError::Validation(
                            "this draft contains HTML, attachments, or other unsupported MIME content and is read-only"
                                .to_owned(),
                        ));
                    }
                    if expected.draft.compose_request() == request {
                        return Ok(DraftSaveOutcome {
                            kind: DraftSaveKind::Saved,
                            draft: expected.draft.clone(),
                            canonical: None,
                        });
                    }
                    let mut replacement = expected.clone();
                    replacement.revision = expected.revision.checked_add(1).ok_or_else(|| {
                        MailError::Validation("draft revision limit reached".to_owned())
                    })?;
                    replacement.local_version =
                        expected.local_version.checked_add(1).ok_or_else(|| {
                            MailError::Validation("draft local version limit reached".to_owned())
                        })?;
                    replacement.draft.local_version = replacement.local_version;
                    replacement.draft.to = request.to.clone();
                    replacement.draft.cc = request.cc.clone();
                    replacement.draft.bcc = request.bcc.clone();
                    replacement.draft.subject = request.subject.clone();
                    replacement.draft.body_text = request.body_text.clone();
                    replacement.draft.status = "local".to_owned();
                    replacement.draft.updated_at = now();
                    replacement.is_deleted = false;
                    replacement.draft.raw_rfc822 = build_draft_message_revision(
                        &self.config.email,
                        &request,
                        id,
                        replacement.revision,
                    )?;

                    if self
                        .repository
                        .replace_draft_if_unchanged(expected, &replacement, None)?
                    {
                        return Ok(DraftSaveOutcome {
                            kind: DraftSaveKind::Saved,
                            draft: replacement.draft,
                            canonical: None,
                        });
                    }
                }

                let canonical = match self.repository.get_draft_record(id) {
                    Ok(record)
                        if record.draft.account_id == self.config.account_id
                            && !record.is_deleted =>
                    {
                        Some(record.draft)
                    }
                    Ok(_) | Err(MailError::NotFound { .. }) => None,
                    Err(error) => return Err(error),
                };

                // If a stale client happens to contain the exact canonical
                // content, adopting the newer token is lossless and avoids an
                // unnecessary duplicate.
                if let Some(canonical) = canonical.as_ref()
                    && canonical.status != "sent"
                    && canonical.compose_request() == request
                {
                    return Ok(DraftSaveOutcome {
                        kind: DraftSaveKind::Saved,
                        draft: canonical.clone(),
                        canonical: None,
                    });
                }

                let draft = self.insert_local_draft(&request, "conflict")?;
                Ok(DraftSaveOutcome {
                    kind: DraftSaveKind::ConflictCopy,
                    draft,
                    canonical,
                })
            }
        }
    }

    fn insert_local_draft(&self, request: &ComposeRequest, status: &str) -> Result<Draft> {
        // UUID collisions are not expected, but insert-if-absent keeps
        // creation from ever overwriting a concurrently created row.
        for _ in 0..MAX_LOCAL_DRAFT_CAS_RETRIES {
            let timestamp = now();
            let id = Uuid::now_v7().to_string();
            let mut record = DraftRecord {
                draft: Draft {
                    id: id.clone(),
                    local_version: 1,
                    has_unsupported_content: false,
                    account_id: self.config.account_id.clone(),
                    to: request.to.clone(),
                    cc: request.cc.clone(),
                    bcc: request.bcc.clone(),
                    subject: request.subject.clone(),
                    body_text: request.body_text.clone(),
                    status: status.to_owned(),
                    remote_mailbox: None,
                    remote_uid: None,
                    created_at: timestamp.clone(),
                    updated_at: timestamp,
                    raw_rfc822: Vec::new(),
                },
                local_version: 1,
                revision: 1,
                synced_revision: 0,
                remote_uid_validity: None,
                is_deleted: false,
            };
            record.draft.raw_rfc822 =
                build_draft_message_revision(&self.config.email, request, &id, record.revision)?;
            if self.repository.insert_draft_if_absent(&record)? {
                return Ok(record.draft);
            }
        }
        Err(MailError::Validation(
            "could not allocate a unique local draft id".to_owned(),
        ))
    }

    pub fn list_drafts(&self) -> Result<Vec<Draft>> {
        self.repository.list_drafts(&self.config.account_id)
    }

    /// Mark a draft deleted locally. The tombstone is hidden immediately and
    /// propagated safely on the next `sync_drafts` call.
    pub fn delete_draft(&self, draft_id: &str) -> Result<()> {
        let draft = self.repository.get_draft(draft_id)?;
        if draft.status == "sent" {
            return Err(MailError::Validation(
                "a sent draft cannot be deleted as an active draft".to_owned(),
            ));
        }
        self.repository.tombstone_draft(draft_id, &now())
    }

    /// Tombstone only the exact local draft version visible to the editor.
    /// A stale discard closes the editor without deleting a newer canonical.
    pub fn delete_draft_optimistic(
        &self,
        draft_id: &str,
        expected_local_version: u64,
    ) -> Result<DraftDeleteKind> {
        let deleted = self.repository.tombstone_draft_if_local_version(
            &self.config.account_id,
            draft_id,
            expected_local_version,
            &now(),
        )?;
        Ok(if deleted {
            DraftDeleteKind::Deleted
        } else {
            DraftDeleteKind::Stale
        })
    }

    pub async fn sync_draft(
        &self,
        draft_id: &str,
        mailbox_override: Option<&str>,
    ) -> Result<Draft> {
        self.repository.get_draft(draft_id)?;
        self.sync_drafts(mailbox_override).await?;
        self.repository.get_draft(draft_id)
    }

    /// Reconcile every visible remote draft with local SQLite state.
    ///
    /// Mine Mail revisions are identified by stable private headers. Drafts
    /// created by other clients are imported under an identity derived from
    /// UIDVALIDITY and UID; the first local edit upgrades them to a stable Mine
    /// Mail identity. See `DraftSyncReport` for the deterministic conflict and
    /// deletion policy.
    pub async fn sync_drafts(&self, mailbox_override: Option<&str>) -> Result<DraftSyncReport> {
        let _guard = self.imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config).await?;
        let snapshot = connection.fetch_draft_snapshot(mailbox_override).await?;
        let mut report = DraftSyncReport {
            mailbox: snapshot.mailbox.clone(),
            ..DraftSyncReport::default()
        };

        let mut remote_groups: HashMap<String, Vec<RemoteDraftCandidate>> = HashMap::new();
        for remote in snapshot.messages {
            match remote_draft_candidate(remote, snapshot.uid_validity) {
                Ok(candidate) => remote_groups
                    .entry(candidate.id.clone())
                    .or_default()
                    .push(candidate),
                Err(_) => report.skipped += 1,
            }
        }

        let mut local_records: HashMap<String, DraftRecord> = self
            .repository
            .list_draft_records(&self.config.account_id)?
            .into_iter()
            .map(|record| (record.draft.id.clone(), record))
            .collect();

        for (id, mut candidates) in remote_groups {
            candidates.sort_by_key(|candidate| (candidate.revision, candidate.uid));
            let local = local_records.remove(&id);

            // A sent row only proves that exact immutable draft version was
            // consumed. Any later or divergent remote object is a new user
            // draft and must be made visible before its remote UID is retired.
            if let Some(sent) = local
                .as_ref()
                .filter(|record| record.draft.status == "sent")
            {
                let mut cleanup_uids = Vec::new();
                for candidate in &candidates {
                    if draft_record_matches_remote(sent, candidate) {
                        cleanup_uids.push(candidate.uid);
                        continue;
                    }
                    match self.preserve_remote_fork(&id, candidate)? {
                        RemoteForkPreservation::Inserted => {
                            report.pulled += 1;
                            report.conflicts += 1;
                            cleanup_uids.push(candidate.uid);
                        }
                        RemoteForkPreservation::AlreadyPreserved => {
                            cleanup_uids.push(candidate.uid);
                        }
                        RemoteForkPreservation::IdentityCollision => report.skipped += 1,
                    }
                }
                report.deleted_remote += connection.delete_draft_uids(&cleanup_uids).await?;
                continue;
            }

            let canonical = candidates
                .last()
                .cloned()
                .expect("remote draft group is never empty");
            let mut cleanup_uids = Vec::new();
            for candidate in candidates
                .iter()
                .filter(|candidate| candidate.uid != canonical.uid)
            {
                if remote_candidates_equivalent(candidate, &canonical) {
                    cleanup_uids.push(candidate.uid);
                    continue;
                }
                match self.preserve_remote_fork(&id, candidate)? {
                    RemoteForkPreservation::Inserted => {
                        report.pulled += 1;
                        report.conflicts += 1;
                        cleanup_uids.push(candidate.uid);
                    }
                    RemoteForkPreservation::AlreadyPreserved => {
                        cleanup_uids.push(candidate.uid);
                    }
                    RemoteForkPreservation::IdentityCollision => report.skipped += 1,
                }
            }
            let mut safe_replacement_uids = vec![canonical.uid];
            safe_replacement_uids.extend(cleanup_uids.iter().copied());

            let Some(local) = local else {
                let record = self.record_from_remote(
                    &canonical,
                    None,
                    &snapshot.mailbox,
                    snapshot.uid_validity,
                )?;
                if self.repository.insert_draft_if_absent(&record)? {
                    report.pulled += 1;
                    report.deleted_remote += connection.delete_draft_uids(&cleanup_uids).await?;
                } else {
                    // A local draft with this stable id appeared after the
                    // snapshot. Preserve both sides for the next sync.
                    report.skipped += 1;
                }
                continue;
            };

            let reconciliation = classify_draft_reconciliation(&local, &canonical);
            if reconciliation == DraftReconciliation::InSync && !local.is_deleted {
                if self.repository.mark_draft_record_synced_if_unchanged(
                    &local,
                    &snapshot.mailbox,
                    Some(canonical.uid),
                    snapshot.uid_validity,
                )? {
                    report.deleted_remote += connection.delete_draft_uids(&cleanup_uids).await?;
                } else {
                    report.skipped += 1;
                }
                continue;
            }

            if local.is_deleted {
                if matches!(
                    reconciliation,
                    DraftReconciliation::PullRemote | DraftReconciliation::Conflict
                ) {
                    let record = self.record_from_remote(
                        &canonical,
                        Some(&local),
                        &snapshot.mailbox,
                        snapshot.uid_validity,
                    )?;
                    if self
                        .repository
                        .replace_draft_if_unchanged(&local, &record, None)?
                    {
                        report.pulled += 1;
                        report.conflicts += 1;
                        report.deleted_remote +=
                            connection.delete_draft_uids(&cleanup_uids).await?;
                    } else {
                        report.skipped += 1;
                    }
                } else {
                    if self.repository.delete_draft_if_unchanged(&local)? {
                        report.deleted_remote +=
                            connection.delete_draft_uids(&safe_replacement_uids).await?;
                    } else {
                        report.skipped += 1;
                    }
                }
                continue;
            }

            match reconciliation {
                DraftReconciliation::Conflict => {
                    let record = self.record_from_remote(
                        &canonical,
                        Some(&local),
                        &snapshot.mailbox,
                        snapshot.uid_validity,
                    )?;
                    let conflict_copy = self.conflict_copy_record(&local)?;
                    if self.repository.replace_draft_if_unchanged(
                        &local,
                        &record,
                        Some(&conflict_copy),
                    )? {
                        report.pulled += 1;
                        report.conflicts += 1;
                        report.deleted_remote +=
                            connection.delete_draft_uids(&cleanup_uids).await?;
                    } else {
                        report.skipped += 1;
                    }
                }
                DraftReconciliation::PullRemote => {
                    let record = self.record_from_remote(
                        &canonical,
                        Some(&local),
                        &snapshot.mailbox,
                        snapshot.uid_validity,
                    )?;
                    if self
                        .repository
                        .replace_draft_if_unchanged(&local, &record, None)?
                    {
                        report.pulled += 1;
                        report.deleted_remote +=
                            connection.delete_draft_uids(&cleanup_uids).await?;
                    } else {
                        report.skipped += 1;
                    }
                }
                DraftReconciliation::PushLocal => {
                    self.push_draft_record(
                        &mut connection,
                        &snapshot.mailbox,
                        snapshot.uid_validity,
                        &local,
                        &safe_replacement_uids,
                        &mut report,
                    )
                    .await?;
                }
                DraftReconciliation::InSync => {
                    if self.repository.mark_draft_record_synced_if_unchanged(
                        &local,
                        &snapshot.mailbox,
                        Some(canonical.uid),
                        snapshot.uid_validity,
                    )? {
                        report.deleted_remote +=
                            connection.delete_draft_uids(&cleanup_uids).await?;
                    } else {
                        report.skipped += 1;
                    }
                }
            }
        }

        for record in local_records.into_values() {
            if record.draft.status == "sent" || record.draft.status == "conflict" {
                continue;
            }
            if record.is_deleted {
                if !self.repository.delete_draft_if_unchanged(&record)? {
                    report.skipped += 1;
                }
                continue;
            }

            let previously_remote = record.synced_revision > 0
                || record.draft.remote_mailbox.as_deref() == Some(snapshot.mailbox.as_str());
            let local_changed = record.revision > record.synced_revision;
            if previously_remote && !local_changed {
                if self.repository.delete_draft_if_unchanged(&record)? {
                    report.deleted_local += 1;
                } else {
                    report.skipped += 1;
                }
            } else {
                self.push_draft_record(
                    &mut connection,
                    &snapshot.mailbox,
                    snapshot.uid_validity,
                    &record,
                    &[],
                    &mut report,
                )
                .await?;
            }
        }

        let _ = connection.logout().await;
        report.local_total = self.repository.list_drafts(&self.config.account_id)?.len();
        Ok(report)
    }

    async fn push_draft_record(
        &self,
        connection: &mut ImapConnection,
        mailbox: &str,
        uid_validity: Option<u32>,
        record: &DraftRecord,
        old_uids: &[u32],
        report: &mut DraftSyncReport,
    ) -> Result<()> {
        let (remote_uid, removed) = connection
            .append_and_replace_draft(
                mailbox,
                &record.draft.id,
                &record.draft.raw_rfc822,
                old_uids,
            )
            .await?;
        let marked = self.repository.mark_draft_record_synced_if_unchanged(
            record,
            mailbox,
            remote_uid,
            uid_validity,
        )?;
        report.pushed += 1;
        report.deleted_remote += removed;
        if !marked {
            // The uploaded revision remains valid remotely, but a newer local
            // edit must stay dirty for the next synchronization pass.
            report.skipped += 1;
        }
        Ok(())
    }

    fn preserve_remote_fork(
        &self,
        original_id: &str,
        remote: &RemoteDraftCandidate,
    ) -> Result<RemoteForkPreservation> {
        let record = self.remote_fork_record(original_id, remote);
        if self.repository.insert_draft_if_absent(&record)? {
            return Ok(RemoteForkPreservation::Inserted);
        }

        match self.repository.get_draft_record(&record.draft.id) {
            Ok(existing)
                if existing.draft.account_id == record.draft.account_id
                    && existing.revision == record.revision
                    && existing.draft.compose_request() == record.draft.compose_request()
                    && existing.draft.raw_rfc822 == record.draft.raw_rfc822 =>
            {
                Ok(RemoteForkPreservation::AlreadyPreserved)
            }
            Ok(_) | Err(MailError::NotFound { .. }) => {
                Ok(RemoteForkPreservation::IdentityCollision)
            }
            Err(error) => Err(error),
        }
    }

    fn remote_fork_record(&self, original_id: &str, remote: &RemoteDraftCandidate) -> DraftRecord {
        let id = deterministic_remote_fork_id(original_id, remote.uid_validity, remote.uid);
        DraftRecord {
            draft: Draft {
                id,
                local_version: 1,
                has_unsupported_content: remote.has_unsupported_content,
                account_id: self.config.account_id.clone(),
                to: remote.request.to.clone(),
                cc: remote.request.cc.clone(),
                bcc: remote.request.bcc.clone(),
                subject: remote.request.subject.clone(),
                body_text: remote.request.body_text.clone(),
                status: "conflict".to_owned(),
                remote_mailbox: None,
                remote_uid: None,
                created_at: remote.updated_at.clone(),
                updated_at: remote.updated_at.clone(),
                raw_rfc822: remote.raw_rfc822.clone(),
            },
            local_version: 1,
            revision: remote.revision,
            synced_revision: 0,
            remote_uid_validity: None,
            is_deleted: false,
        }
    }

    fn record_from_remote(
        &self,
        remote: &RemoteDraftCandidate,
        existing: Option<&DraftRecord>,
        mailbox: &str,
        uid_validity: Option<u32>,
    ) -> Result<DraftRecord> {
        let created_at = existing
            .map(|record| record.draft.created_at.clone())
            .unwrap_or_else(|| remote.updated_at.clone());
        let local_version = existing.map_or(Ok(1), |record| {
            record.local_version.checked_add(1).ok_or_else(|| {
                MailError::Validation("draft local version limit reached".to_owned())
            })
        })?;
        Ok(DraftRecord {
            draft: Draft {
                id: remote.id.clone(),
                local_version,
                has_unsupported_content: remote.has_unsupported_content,
                account_id: self.config.account_id.clone(),
                to: remote.request.to.clone(),
                cc: remote.request.cc.clone(),
                bcc: remote.request.bcc.clone(),
                subject: remote.request.subject.clone(),
                body_text: remote.request.body_text.clone(),
                status: "synced".to_owned(),
                remote_mailbox: Some(mailbox.to_owned()),
                remote_uid: Some(remote.uid),
                created_at,
                updated_at: remote.updated_at.clone(),
                raw_rfc822: remote.raw_rfc822.clone(),
            },
            local_version,
            revision: remote.revision,
            synced_revision: remote.revision,
            remote_uid_validity: uid_validity,
            is_deleted: false,
        })
    }

    fn conflict_copy_record(&self, local: &DraftRecord) -> Result<DraftRecord> {
        let id = Uuid::now_v7().to_string();
        let timestamp = now();
        let mut request = local.draft.compose_request();
        request.subject = if request.subject.is_empty() {
            "本地冲突副本".to_owned()
        } else {
            format!("{}（本地冲突副本）", request.subject)
        };
        Ok(DraftRecord {
            draft: Draft {
                id: id.clone(),
                local_version: 1,
                has_unsupported_content: false,
                account_id: self.config.account_id.clone(),
                to: request.to.clone(),
                cc: request.cc.clone(),
                bcc: request.bcc.clone(),
                subject: request.subject.clone(),
                body_text: request.body_text.clone(),
                status: "conflict".to_owned(),
                remote_mailbox: None,
                remote_uid: None,
                created_at: timestamp.clone(),
                updated_at: timestamp,
                raw_rfc822: build_draft_message_revision(&self.config.email, &request, &id, 1)?,
            },
            local_version: 1,
            revision: 1,
            synced_revision: 0,
            remote_uid_validity: None,
            is_deleted: false,
        })
    }

    pub async fn send_compose(&self, request: ComposeRequest) -> Result<OutboxItem> {
        self.send_request(request, None).await
    }

    pub async fn send_draft(
        &self,
        draft_id: &str,
        expected_local_version: u64,
        confirmed_recipients: &[String],
    ) -> Result<OutboxItem> {
        let snapshot =
            self.confirmed_draft_snapshot(draft_id, expected_local_version, confirmed_recipients)?;
        self.send_request(
            snapshot.request,
            Some((snapshot.id, snapshot.revision, snapshot.local_version)),
        )
        .await
    }

    /// Reads and confirms one immutable draft version. No later send step
    /// reloads recipients or content, so synchronization cannot change the
    /// message between confirmation and Outbox persistence.
    fn confirmed_draft_snapshot(
        &self,
        draft_id: &str,
        expected_local_version: u64,
        confirmed_recipients: &[String],
    ) -> Result<ConfirmedDraftSnapshot> {
        let record = self.repository.get_draft_record(draft_id)?;
        if record.draft.account_id != self.config.account_id || record.is_deleted {
            return Err(MailError::NotFound {
                entity: "draft",
                id: draft_id.to_owned(),
            });
        }
        if record.draft.status == "sent" {
            return Err(MailError::Validation(
                "this draft has already been sent".to_owned(),
            ));
        }
        if record.draft.has_unsupported_content {
            return Err(MailError::Validation(
                "this draft contains HTML, attachments, or other unsupported MIME content and cannot be sent by the MVP editor"
                    .to_owned(),
            ));
        }
        if record.local_version != expected_local_version {
            return Err(MailError::Validation(
                "the draft changed after it was displayed; refresh and confirm the current version before sending"
                    .to_owned(),
            ));
        }
        let request = record.draft.compose_request();
        require_exact_recipient_confirmation(&request, confirmed_recipients)?;
        Ok(ConfirmedDraftSnapshot {
            id: record.draft.id,
            revision: record.revision,
            local_version: record.local_version,
            request,
        })
    }

    pub fn list_outbox(&self) -> Result<Vec<OutboxItem>> {
        self.repository.list_outbox(&self.config.account_id)
    }

    /// Loads one immutable Outbox message for local body hydration while
    /// preserving the active-account boundary.
    pub fn outbox_message(&self, outbox_id: &str) -> Result<OutboxItem> {
        let item = self.repository.get_outbox(outbox_id)?;
        if item.account_id != self.config.account_id {
            return Err(MailError::NotFound {
                entity: "outbox item",
                id: outbox_id.to_owned(),
            });
        }
        Ok(item)
    }

    /// Manually retries one previously persisted SMTP attempt.
    ///
    /// Only the `retryable` state is accepted. In particular, an ambiguous
    /// `delivery_unknown` result is never retried because doing so could send a
    /// duplicate. The immutable RFC822 bytes and envelope recipients are read
    /// from SQLite; the associated draft is not consulted or rebuilt.
    pub async fn retry_outbox(&self, outbox_id: &str) -> Result<OutboxItem> {
        let _guard = self.smtp_gate.lock().await;
        let snapshot = self.repository.get_outbox(outbox_id)?;
        validate_manual_retry(&snapshot, &self.config.account_id)?;
        let envelope = restore_outbox_envelope(&snapshot.raw_rfc822, &snapshot.recipients)?;
        let client = SmtpClient::new(&self.config)?;

        // The repository repeats the status/account check under an IMMEDIATE
        // SQLite transaction, so a second app process cannot claim the item.
        let claimed = self
            .repository
            .claim_retryable_outbox(outbox_id, &self.config.account_id)?;
        match client.send_raw(&envelope, &claimed.raw_rfc822).await {
            Ok(()) => {
                self.repository.finalize_outbox_sent(outbox_id)?;
            }
            Err(failure) => {
                self.repository.update_outbox_status(
                    outbox_id,
                    failure.status,
                    Some(&failure.safe_reason),
                )?;
            }
        }

        self.repository.get_outbox(outbox_id)
    }

    async fn send_request(
        &self,
        request: ComposeRequest,
        draft_snapshot: Option<(String, u64, u64)>,
    ) -> Result<OutboxItem> {
        // Acquire the lifecycle gate before creating an Outbox row. A second
        // send waits outside SQLite, so it cannot leave a live queued row that
        // a concurrently constructed backend might recover as abandoned.
        let _guard = self.smtp_gate.lock().await;
        if let Some((draft_id, _, draft_local_version)) = draft_snapshot.as_ref()
            && let Some(existing) = self
                .repository
                .get_blocking_outbox_for_draft(draft_id, *draft_local_version)?
        {
            let detail = if existing.status == OutboxStatus::DeliveryUnknown {
                "delivery of an earlier draft version is unknown; resolve it before sending a new version"
            } else {
                "this exact draft version already has an Outbox item"
            };
            return Err(MailError::Validation(format!(
                "{detail} with status '{}'; it will not be sent again",
                existing.status.as_str(),
            )));
        }

        let outgoing = build_outgoing_message(&self.config.email, &request)?;
        let outbox_id = Uuid::now_v7().to_string();
        let queued = OutboxItem {
            id: outbox_id.clone(),
            account_id: self.config.account_id.clone(),
            draft_id: draft_snapshot.as_ref().map(|(id, _, _)| id.clone()),
            draft_revision: draft_snapshot.as_ref().map(|(_, revision, _)| *revision),
            draft_local_version: draft_snapshot
                .as_ref()
                .map(|(_, _, local_version)| *local_version),
            recipients: outgoing.recipients,
            status: OutboxStatus::Queued,
            attempts: 0,
            last_error: None,
            created_at: now(),
            sent_at: None,
            raw_rfc822: outgoing.raw_rfc822.clone(),
        };

        let client = match SmtpClient::new(&self.config) {
            Ok(client) => client,
            Err(error) => {
                let mut retryable = queued;
                retryable.status = OutboxStatus::Retryable;
                retryable.last_error = Some(error.to_string());
                self.repository.enqueue_new_outbox(&retryable)?;
                return self.repository.get_outbox(&outbox_id);
            }
        };

        // INSERT queued + conditional queued->sending happen in one database
        // transaction. No other connection can recover this active item.
        let claimed = self.repository.enqueue_and_claim_outbox(&queued)?;
        match client
            .send_raw(&outgoing.envelope, &claimed.raw_rfc822)
            .await
        {
            Ok(()) => {
                self.repository.finalize_outbox_sent(&outbox_id)?;
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

fn validate_manual_retry(item: &OutboxItem, account_id: &str) -> Result<()> {
    if item.account_id != account_id {
        return Err(MailError::NotFound {
            entity: "outbox item",
            id: item.id.clone(),
        });
    }
    if item.status != OutboxStatus::Retryable {
        return Err(MailError::Validation(format!(
            "outbox item '{}' has status '{}'; only retryable items can be retried",
            item.id,
            item.status.as_str()
        )));
    }
    Ok(())
}

fn require_exact_recipient_confirmation(
    request: &ComposeRequest,
    confirmations: &[String],
) -> Result<()> {
    request.validate()?;
    let expected = normalize_recipient_set(request.all_recipients().map(String::as_str))?;
    let confirmed = normalize_recipient_set(confirmations.iter().map(String::as_str))?;
    if expected != confirmed {
        return Err(MailError::Validation(
            "recipient confirmation does not exactly match the normalized To/Cc/Bcc set; no message was sent"
                .to_owned(),
        ));
    }
    Ok(())
}

fn normalize_recipient_set<'a>(
    addresses: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeSet<String>> {
    let mut normalized = BTreeSet::new();
    for address in addresses {
        let address = address.trim();
        if address.is_empty() {
            return Err(MailError::Validation(
                "recipient confirmations cannot be blank".to_owned(),
            ));
        }
        normalized.insert(address.to_lowercase());
    }
    Ok(normalized)
}

fn classify_inbox_uid_scope(
    local_uid_validity: Option<u32>,
    selected_uid_validity: Option<u32>,
) -> InboxUidScope {
    match (local_uid_validity, selected_uid_validity) {
        (Some(local), Some(remote)) if local == remote => InboxUidScope::Current,
        (Some(_), _) => InboxUidScope::Changed,
        (None, _) => InboxUidScope::NeedsSync,
    }
}

fn mailbox_hint_changed(previous: MailboxHint, current: MailboxHint) -> bool {
    previous.exists != current.exists
        || previous.uid_next != current.uid_next
        || previous.uid_validity != current.uid_validity
}

fn remote_candidates_equivalent(left: &RemoteDraftCandidate, right: &RemoteDraftCandidate) -> bool {
    left.revision == right.revision
        && left.request == right.request
        && left.raw_rfc822 == right.raw_rfc822
}

fn draft_record_matches_remote(local: &DraftRecord, remote: &RemoteDraftCandidate) -> bool {
    local.revision == remote.revision
        && local.draft.compose_request() == remote.request
        && local.draft.raw_rfc822 == remote.raw_rfc822
}

fn deterministic_remote_fork_id(original_id: &str, uid_validity: Option<u32>, uid: u32) -> String {
    // Stable FNV-1a keeps the generated private id short enough for our header
    // validation. A collision is never destructive: persistence verifies the
    // complete raw message before allowing the remote UID to be deleted.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in original_id
        .as_bytes()
        .iter()
        .copied()
        .chain(uid_validity.is_some().then_some(1))
        .chain(uid_validity.unwrap_or_default().to_be_bytes())
        .chain(uid.to_be_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let epoch = uid_validity.map_or_else(|| "none".to_owned(), |value| value.to_string());
    format!("remote-conflict-{epoch}-{uid}-{hash:016x}")
}

fn remote_draft_candidate(
    remote: RemoteMessage,
    uid_validity: Option<u32>,
) -> Result<RemoteDraftCandidate> {
    let fallback_id = || format!("remote-{}-{}", uid_validity.unwrap_or_default(), remote.uid);
    let (id, revision, request, has_unsupported_content) = match parse_draft_message(&remote.raw) {
        Ok(parsed) => (
            parsed.draft_id.unwrap_or_else(fallback_id),
            parsed.revision,
            parsed.request,
            parsed.has_unsupported_content,
        ),
        Err(_) => (
            fallback_id(),
            1,
            ComposeRequest {
                to: Vec::new(),
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: String::new(),
                body_text: String::new(),
            },
            true,
        ),
    };
    Ok(RemoteDraftCandidate {
        id,
        revision,
        uid: remote.uid,
        uid_validity,
        has_unsupported_content,
        request,
        raw_rfc822: remote.raw,
        updated_at: remote.internal_date.unwrap_or_else(now),
    })
}

/// Classifies a local/remote pair against the immutable IMAP object that was
/// last synchronized. `INTERNALDATE` is not a cross-device revision clock: a
/// replacement created on another client can legitimately have an older date.
/// Only the same UID in the same UIDVALIDITY epoch is a reliable old baseline.
fn classify_draft_reconciliation(
    local: &DraftRecord,
    remote: &RemoteDraftCandidate,
) -> DraftReconciliation {
    if draft_record_matches_remote(local, remote) {
        return DraftReconciliation::InSync;
    }

    let local_changed = local.revision > local.synced_revision;
    let is_old_remote_baseline = local.draft.remote_uid == Some(remote.uid)
        && local.remote_uid_validity.is_some()
        && local.remote_uid_validity == remote.uid_validity
        && remote.revision == local.synced_revision;
    let remote_changed = !is_old_remote_baseline;

    match (local_changed, remote_changed) {
        (true, true) => DraftReconciliation::Conflict,
        (true, false) => DraftReconciliation::PushLocal,
        (false, true) => DraftReconciliation::PullRemote,
        (false, false) => DraftReconciliation::InSync,
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
    use std::{
        sync::{Arc, Barrier},
        thread,
    };

    use tempfile::tempdir;

    use super::{
        DraftReconciliation, INBOX, InboxUidScope, MailBackend, RemoteDraftCandidate,
        RemoteForkPreservation, classify_draft_reconciliation, classify_inbox_uid_scope,
        draft_record_matches_remote, mailbox_hint_changed, remote_candidates_equivalent,
        remote_draft_candidate, validate_manual_retry,
    };
    use crate::{
        AccountConfig, ComposeRequest, Draft, DraftDeleteKind, DraftSaveKind, InboxMessage,
        MailError, OutboxItem, OutboxStatus,
        database::{DraftRecord, Repository},
        imap_client::{MailboxHint, RemoteMessage},
        mime::parse_draft_message,
    };

    fn compose(subject: &str, body_text: &str) -> ComposeRequest {
        ComposeRequest {
            to: vec!["receiver@example.com".to_owned()],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: subject.to_owned(),
            body_text: body_text.to_owned(),
        }
    }

    fn local_record(
        subject: &str,
        revision: u64,
        synced_revision: u64,
        updated_at: &str,
    ) -> DraftRecord {
        DraftRecord {
            draft: Draft {
                id: "draft-1".to_owned(),
                local_version: 1,
                has_unsupported_content: false,
                account_id: "primary".to_owned(),
                to: vec!["receiver@example.com".to_owned()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: subject.to_owned(),
                body_text: "body".to_owned(),
                status: "local".to_owned(),
                remote_mailbox: Some("Drafts".to_owned()),
                remote_uid: Some(10),
                created_at: "2026-07-14T00:00:00Z".to_owned(),
                updated_at: updated_at.to_owned(),
                raw_rfc822: Vec::new(),
            },
            local_version: 1,
            revision,
            synced_revision,
            remote_uid_validity: Some(99),
            is_deleted: false,
        }
    }

    fn remote_candidate(subject: &str, revision: u64, updated_at: &str) -> RemoteDraftCandidate {
        RemoteDraftCandidate {
            id: "draft-1".to_owned(),
            revision,
            uid: 10,
            uid_validity: Some(99),
            has_unsupported_content: false,
            request: compose(subject, "body"),
            raw_rfc822: Vec::new(),
            updated_at: updated_at.to_owned(),
        }
    }

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

    #[test]
    fn opening_an_old_cached_body_repairs_unresolved_inline_cid_images_once() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let raw = b"From: sender@example.com\r\nTo: receiver@example.com\r\nSubject: Inline image\r\nContent-Type: multipart/related; boundary=x\r\n\r\n--x\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Hello</p><img src=\"cid:avatar@example.com\">\r\n--x\r\nContent-Type: image/png\r\nContent-Transfer-Encoding: base64\r\nContent-ID: <avatar@example.com>\r\nContent-Disposition: inline; filename=avatar.png\r\n\r\nAQID\r\n--x--\r\n";
        let stale = InboxMessage {
            id: 0,
            account_id: backend.config.account_id.clone(),
            mailbox: INBOX.to_owned(),
            uid: 42,
            message_id: None,
            in_reply_to: Vec::new(),
            references: Vec::new(),
            subject: "Inline image".to_owned(),
            sender: None,
            to: Vec::new(),
            cc: Vec::new(),
            sent_at: None,
            internal_date: None,
            flags: Vec::new(),
            size_bytes: u32::try_from(raw.len()).unwrap(),
            preview: "Hello".to_owned(),
            body_text: Some("Hello".to_owned()),
            body_html: Some("<p>Hello</p><img src=\"cid:avatar@example.com\">".to_owned()),
            attachment_names: vec!["avatar.png".to_owned()],
            body_fetched: true,
            raw_rfc822: raw.to_vec(),
            synced_at: "2026-07-16T00:00:00Z".to_owned(),
        };
        backend
            .repository
            .upsert_message(&stale)
            .expect("stale cache");

        let repaired = backend.cached_inbox_message(42).expect("repaired body");
        let html = repaired.body_html.expect("HTML body");
        assert!(html.contains("data:image/png;base64,AQID"));
        assert!(!html.to_ascii_lowercase().contains("cid:avatar@example.com"));
        assert_eq!(
            backend
                .cached_inbox_message(42)
                .expect("persisted repair")
                .body_html
                .as_deref(),
            Some(html.as_str())
        );
    }

    #[test]
    fn imported_unsupported_drafts_are_persisted_read_only_and_cannot_be_sent() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let fixtures = [
            b"From: sender@example.com\r\nTo: receiver@example.com\r\nSubject: HTML draft\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<strong>Body</strong>".to_vec(),
            b"From: sender@example.com\r\nTo: receiver@example.com\r\nSubject: Attachment draft\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nBody\r\n--x\r\nContent-Type: image/png\r\nContent-Disposition: inline; filename=image.png\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n--x--\r\n".to_vec(),
            b"not an RFC822 message".to_vec(),
        ];

        for (index, raw) in fixtures.into_iter().enumerate() {
            let uid = u32::try_from(index + 40).unwrap();
            let candidate = remote_draft_candidate(
                RemoteMessage {
                    uid,
                    flags: vec!["\\Draft".to_owned()],
                    internal_date: Some("2026-07-14T02:00:00Z".to_owned()),
                    size_bytes: u32::try_from(raw.len()).unwrap(),
                    raw: raw.clone(),
                },
                Some(91),
            )
            .expect("unsupported remote candidate");
            assert!(candidate.has_unsupported_content);
            let record = backend
                .record_from_remote(&candidate, None, "Drafts", Some(91))
                .expect("read-only record");
            assert!(record.draft.has_unsupported_content);
            assert_eq!(record.draft.raw_rfc822, raw);
            assert!(
                backend
                    .repository
                    .insert_draft_if_absent(&record)
                    .expect("persist imported draft")
            );

            assert!(matches!(
                backend.upsert_draft(Some(&record.draft.id), compose("overwrite", "unsafe")),
                Err(MailError::Validation(_))
            ));
            assert!(matches!(
                backend.save_draft_optimistic(
                    Some(&record.draft.id),
                    Some(record.local_version),
                    compose("overwrite", "unsafe"),
                ),
                Err(MailError::Validation(_))
            ));
            assert!(matches!(
                backend.confirmed_draft_snapshot(
                    &record.draft.id,
                    record.local_version,
                    &record.draft.to,
                ),
                Err(MailError::Validation(_))
            ));
        }
    }

    #[test]
    fn draft_send_confirmation_is_bound_to_one_local_snapshot() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let created = backend
            .save_draft_optimistic(None, None, compose("version A", "body A"))
            .expect("create draft");

        let version_a = backend
            .confirmed_draft_snapshot(
                &created.draft.id,
                created.draft.local_version,
                &["receiver@example.com".to_owned()],
            )
            .expect("confirm version A");

        let mut version_b_request = compose("version B", "body B");
        version_b_request.to = vec!["new-recipient@example.com".to_owned()];
        let version_b = backend
            .save_draft_optimistic(
                Some(&created.draft.id),
                Some(created.draft.local_version),
                version_b_request,
            )
            .expect("save version B");

        let stale = backend
            .confirmed_draft_snapshot(
                &created.draft.id,
                created.draft.local_version,
                &["receiver@example.com".to_owned()],
            )
            .expect_err("stale displayed token must fail before recipient confirmation");
        assert!(
            stale
                .to_string()
                .contains("draft changed after it was displayed")
        );

        let wrong_recipient = backend
            .confirmed_draft_snapshot(
                &created.draft.id,
                version_b.draft.local_version,
                &["receiver@example.com".to_owned()],
            )
            .expect_err("current token with stale recipients must fail");
        assert!(
            wrong_recipient
                .to_string()
                .contains("recipient confirmation does not exactly match")
        );

        let current = backend
            .confirmed_draft_snapshot(
                &created.draft.id,
                version_b.draft.local_version,
                &["new-recipient@example.com".to_owned()],
            )
            .expect("confirm version B");
        assert_eq!(version_a.request.subject, "version A");
        assert_eq!(version_a.request.to, ["receiver@example.com"]);
        assert_eq!(current.request.subject, "version B");
        assert_eq!(current.request.to, ["new-recipient@example.com"]);
        assert!(
            backend
                .list_outbox()
                .expect("Outbox remains empty")
                .is_empty()
        );
    }

    #[test]
    fn local_draft_upsert_retains_identity_and_delete_hides_tombstone() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");

        let created = backend
            .save_draft(compose("first", "version one"))
            .expect("create draft");
        let updated = backend
            .upsert_draft(Some(&created.id), compose("second", "version two"))
            .expect("update draft");

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.subject, "second");
        assert_eq!(backend.list_drafts().expect("drafts"), vec![updated]);

        backend.delete_draft(&created.id).expect("delete draft");
        assert!(backend.list_drafts().expect("drafts").is_empty());
    }

    #[test]
    fn optimistic_draft_save_advances_the_exact_expected_revision() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");

        let created = backend
            .save_draft_optimistic(None, None, compose("base", "base body"))
            .expect("create");
        assert_eq!(created.kind, DraftSaveKind::Saved);
        assert_eq!(created.draft.local_version, 1);

        let clean = backend
            .save_draft_optimistic(
                Some(&created.draft.id),
                Some(created.draft.local_version),
                compose("base", "base body"),
            )
            .expect("clean stabilization");
        assert_eq!(clean.draft.local_version, 1);

        let updated = backend
            .save_draft_optimistic(
                Some(&created.draft.id),
                Some(created.draft.local_version),
                compose("updated", "updated body"),
            )
            .expect("update");
        assert_eq!(updated.kind, DraftSaveKind::Saved);
        assert_eq!(updated.draft.id, created.draft.id);
        assert_eq!(updated.draft.local_version, 2);
        assert_eq!(updated.draft.subject, "updated");
    }

    #[test]
    fn stale_optimistic_save_keeps_canonical_and_creates_conflict_copy() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let created = backend
            .save_draft_optimistic(None, None, compose("base", "base body"))
            .expect("create");
        let canonical = backend
            .save_draft_optimistic(
                Some(&created.draft.id),
                Some(1),
                compose("remote canonical", "newer canonical body"),
            )
            .expect("canonical update");

        let stale = backend
            .save_draft_optimistic(
                Some(&created.draft.id),
                Some(1),
                compose("local stale edit", "preserve this body"),
            )
            .expect("stale save");
        assert_eq!(stale.kind, DraftSaveKind::ConflictCopy);
        assert_ne!(stale.draft.id, created.draft.id);
        assert_eq!(stale.draft.local_version, 1);
        assert_eq!(stale.draft.status, "conflict");
        assert_eq!(stale.draft.subject, "local stale edit");
        assert_eq!(
            stale.canonical.as_ref().map(|draft| draft.local_version),
            Some(canonical.draft.local_version)
        );

        let persisted_canonical = backend
            .repository
            .get_draft_record(&created.draft.id)
            .expect("canonical");
        assert_eq!(persisted_canonical.draft.subject, "remote canonical");
        assert_eq!(persisted_canonical.revision, 2);
    }

    #[test]
    fn optimistic_save_after_canonical_deletion_preserves_local_copy() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let created = backend
            .save_draft_optimistic(None, None, compose("base", "base body"))
            .expect("create");
        backend
            .delete_draft(&created.draft.id)
            .expect("delete canonical");

        let preserved = backend
            .save_draft_optimistic(
                Some(&created.draft.id),
                Some(created.draft.local_version),
                compose("offline edit", "must survive deletion"),
            )
            .expect("preserve local edit");
        assert_eq!(preserved.kind, DraftSaveKind::ConflictCopy);
        assert_ne!(preserved.draft.id, created.draft.id);
        assert_eq!(preserved.draft.subject, "offline edit");
        assert!(preserved.canonical.is_none());

        let visible = backend.list_drafts().expect("visible drafts");
        assert_eq!(visible, vec![preserved.draft]);
    }

    #[test]
    fn same_protocol_revision_remote_replacement_invalidates_the_ui_token() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let created = backend
            .save_draft_optimistic(None, None, compose("base", "base body"))
            .expect("create");
        let expected = backend
            .repository
            .get_draft_record(&created.draft.id)
            .expect("base record");
        let remote = RemoteDraftCandidate {
            id: created.draft.id.clone(),
            revision: expected.revision,
            uid: 42,
            uid_validity: Some(91),
            has_unsupported_content: false,
            request: compose("external edit", "external body"),
            raw_rfc822: b"remote replacement".to_vec(),
            updated_at: "2026-07-14T01:00:00Z".to_owned(),
        };
        let replacement = backend
            .record_from_remote(&remote, Some(&expected), "Drafts", Some(91))
            .expect("remote replacement");
        assert_eq!(replacement.revision, expected.revision);
        assert_eq!(replacement.local_version, expected.local_version + 1);
        assert!(
            backend
                .repository
                .replace_draft_if_unchanged(&expected, &replacement, None)
                .expect("replace canonical")
        );

        let stale = backend
            .save_draft_optimistic(
                Some(&created.draft.id),
                Some(created.draft.local_version),
                compose("offline edit", "preserve me"),
            )
            .expect("preserve stale edit");
        assert_eq!(stale.kind, DraftSaveKind::ConflictCopy);
        assert_eq!(
            stale.canonical.as_ref().map(|draft| draft.subject.as_str()),
            Some("external edit")
        );
        assert_eq!(
            backend
                .repository
                .get_draft_record(&created.draft.id)
                .expect("canonical remains")
                .draft
                .subject,
            "external edit"
        );
    }

    #[test]
    fn stale_discard_does_not_delete_a_newer_canonical() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let created = backend
            .save_draft_optimistic(None, None, compose("base", "base body"))
            .expect("create");
        let canonical = backend
            .save_draft_optimistic(
                Some(&created.draft.id),
                Some(created.draft.local_version),
                compose("new canonical", "new canonical body"),
            )
            .expect("update canonical");

        let outcome = backend
            .delete_draft_optimistic(&created.draft.id, created.draft.local_version)
            .expect("stale delete");
        assert_eq!(outcome, DraftDeleteKind::Stale);
        assert_eq!(
            backend
                .repository
                .get_draft_record(&created.draft.id)
                .expect("canonical survives")
                .local_version,
            canonical.draft.local_version
        );
    }

    #[test]
    fn concurrent_local_upserts_allocate_distinct_revisions_and_defeat_stale_sync_cas() {
        let directory = tempdir().expect("tempdir");
        let database_path = directory.path().join("mail.db");
        let creator_config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let creator = MailBackend::open(creator_config, &database_path).expect("creator");
        creator.initialize().expect("initialize");
        let base = creator
            .save_draft(compose("base", "base body"))
            .expect("base draft");
        let stale_sync_snapshot = creator
            .repository
            .get_draft_record(&base.id)
            .expect("sync snapshot");
        drop(creator);

        let first_config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let second_config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let first = MailBackend::open(first_config, &database_path).expect("first backend");
        let second = MailBackend::open(second_config, &database_path).expect("second backend");
        let barrier = Arc::new(Barrier::new(2));
        let first_barrier = Arc::clone(&barrier);
        let second_barrier = Arc::clone(&barrier);
        let first_id = base.id.clone();
        let second_id = base.id.clone();

        let first_save = thread::spawn(move || {
            first_barrier.wait();
            first.upsert_draft(Some(&first_id), compose("first concurrent", "first body"))
        });
        let second_save = thread::spawn(move || {
            second_barrier.wait();
            second.upsert_draft(
                Some(&second_id),
                compose("second concurrent", "second body"),
            )
        });
        let saved = [
            first_save
                .join()
                .expect("first thread")
                .expect("first save"),
            second_save
                .join()
                .expect("second thread")
                .expect("second save"),
        ];
        let mut returned = saved
            .iter()
            .map(|draft| {
                let parsed = parse_draft_message(&draft.raw_rfc822).expect("returned MIME");
                assert_eq!(parsed.draft_id.as_deref(), Some(draft.id.as_str()));
                assert_eq!(parsed.request.subject, draft.subject);
                assert_eq!(parsed.request.body_text, draft.body_text);
                (
                    parsed.revision,
                    draft.subject.clone(),
                    draft.raw_rfc822.clone(),
                )
            })
            .collect::<Vec<_>>();
        returned.sort_by_key(|(revision, _, _)| *revision);
        assert_eq!(
            returned
                .iter()
                .map(|(revision, _, _)| *revision)
                .collect::<Vec<_>>(),
            [2, 3]
        );

        let inspector = Repository::open(&database_path).expect("inspector");
        let before_stale_sync = inspector.get_draft_record(&base.id).unwrap();
        assert_eq!(before_stale_sync.revision, 3);
        let latest_return = returned
            .iter()
            .find(|(revision, _, _)| *revision == 3)
            .expect("latest return");
        assert_eq!(before_stale_sync.draft.subject, latest_return.1);
        assert_eq!(before_stale_sync.draft.raw_rfc822, latest_return.2);

        let mut stale_remote_replacement = stale_sync_snapshot.clone();
        stale_remote_replacement.revision = 2;
        stale_remote_replacement.synced_revision = 2;
        stale_remote_replacement.draft.status = "synced".to_owned();
        stale_remote_replacement.draft.subject = "stale remote".to_owned();
        assert!(
            !inspector
                .replace_draft_if_unchanged(&stale_sync_snapshot, &stale_remote_replacement, None)
                .expect("stale sync CAS")
        );
        assert_eq!(
            inspector.get_draft_record(&base.id).unwrap(),
            before_stale_sync
        );
    }

    #[test]
    fn initialize_recovers_queued_as_retryable_but_sending_as_delivery_unknown() {
        let directory = tempdir().expect("tempdir");
        let database_path = directory.path().join("mail.db");
        let first_config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let first = MailBackend::open(first_config, &database_path).expect("backend");
        first.initialize().expect("initialize");
        let draft = first
            .save_draft(compose("queued draft", "exact draft body"))
            .expect("draft");
        let queued = OutboxItem {
            id: "queued-before-smtp".to_owned(),
            account_id: "primary".to_owned(),
            draft_id: Some(draft.id.clone()),
            draft_revision: Some(1),
            draft_local_version: Some(draft.local_version),
            recipients: vec!["receiver@example.com".to_owned()],
            status: OutboxStatus::Queued,
            attempts: 0,
            last_error: None,
            created_at: "2026-07-14T06:00:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: b"exact queued bytes".to_vec(),
        };
        let sending = OutboxItem {
            id: "interrupted-during-smtp".to_owned(),
            draft_id: None,
            draft_revision: None,
            draft_local_version: None,
            status: OutboxStatus::Sending,
            attempts: 1,
            raw_rfc822: b"exact in-flight bytes".to_vec(),
            ..queued.clone()
        };
        first.repository.enqueue_outbox(&queued).expect("queued");
        first.repository.enqueue_outbox(&sending).expect("sending");
        drop(first);

        let second_config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let restarted = MailBackend::open(second_config, &database_path).expect("restart");
        restarted.initialize().expect("startup recovery");

        let recovered_queued = restarted.repository.get_outbox(&queued.id).unwrap();
        assert_eq!(recovered_queued.status, OutboxStatus::Retryable);
        assert_eq!(recovered_queued.attempts, 0);
        assert_eq!(recovered_queued.raw_rfc822, queued.raw_rfc822);
        assert_eq!(recovered_queued.recipients, queued.recipients);
        assert_eq!(recovered_queued.draft_id, queued.draft_id);
        assert!(
            recovered_queued
                .last_error
                .as_deref()
                .is_some_and(|reason| reason.contains("before SMTP delivery started"))
        );

        let recovered_sending = restarted.repository.get_outbox(&sending.id).unwrap();
        assert_eq!(recovered_sending.status, OutboxStatus::DeliveryUnknown);
        assert_eq!(recovered_sending.attempts, 1);
        assert_eq!(recovered_sending.raw_rfc822, sending.raw_rfc822);
    }

    #[test]
    fn reconciliation_pushes_an_ordinary_local_only_edit() {
        let local = local_record("local edit", 2, 1, "2026-07-14T02:00:00Z");
        let remote = remote_candidate("base", 1, "2026-07-14T01:00:00Z");

        assert_eq!(
            classify_draft_reconciliation(&local, &remote),
            DraftReconciliation::PushLocal
        );
    }

    #[test]
    fn reconciliation_pulls_a_remote_only_edit() {
        let local = local_record("base", 1, 1, "2026-07-14T01:00:00Z");
        let remote = remote_candidate("remote edit", 2, "2026-07-14T02:00:00Z");

        assert_eq!(
            classify_draft_reconciliation(&local, &remote),
            DraftReconciliation::PullRemote
        );
    }

    #[test]
    fn reconciliation_preserves_both_concurrent_edits() {
        let local = local_record("local edit", 2, 1, "2026-07-14T02:00:00Z");
        let remote = remote_candidate("remote edit", 2, "2026-07-14T03:00:00Z");

        assert_eq!(
            classify_draft_reconciliation(&local, &remote),
            DraftReconciliation::Conflict
        );
    }

    #[test]
    fn replacement_uid_conflicts_even_if_its_internal_date_is_older() {
        let local = local_record("local edit", 2, 1, "2026-07-14T03:00:00Z");
        let mut remote = remote_candidate("remote edit", 1, "2026-07-13T03:00:00Z");
        remote.uid = 11;

        assert_eq!(
            classify_draft_reconciliation(&local, &remote),
            DraftReconciliation::Conflict
        );
    }

    #[test]
    fn inbox_body_fetch_requires_the_same_uidvalidity_epoch() {
        assert_eq!(
            classify_inbox_uid_scope(Some(91), Some(91)),
            InboxUidScope::Current
        );
        assert_eq!(
            classify_inbox_uid_scope(None, Some(91)),
            InboxUidScope::NeedsSync
        );
        assert_eq!(
            classify_inbox_uid_scope(Some(91), Some(92)),
            InboxUidScope::Changed
        );
        assert_eq!(
            classify_inbox_uid_scope(Some(91), None),
            InboxUidScope::Changed
        );
    }

    #[test]
    fn inbox_monitor_detects_new_uid_and_mailbox_epoch_changes() {
        let baseline = MailboxHint {
            exists: 10,
            uid_validity: Some(91),
            uid_next: Some(42),
        };
        assert!(!mailbox_hint_changed(baseline, baseline));
        assert!(mailbox_hint_changed(
            baseline,
            MailboxHint {
                exists: 11,
                uid_next: Some(43),
                ..baseline
            }
        ));
        assert!(mailbox_hint_changed(
            baseline,
            MailboxHint {
                uid_validity: Some(92),
                ..baseline
            }
        ));
    }

    #[test]
    fn divergent_same_revision_remote_candidates_are_not_duplicates() {
        let mut first = remote_candidate("branch A", 2, "2026-07-14T01:00:00Z");
        first.uid = 21;
        first.raw_rfc822 = b"same revision branch A".to_vec();
        let mut second = remote_candidate("branch B", 2, "2026-07-14T00:00:00Z");
        second.uid = 22;
        second.raw_rfc822 = b"same revision branch B".to_vec();

        assert!(!remote_candidates_equivalent(&first, &second));
        let mut exact_copy = first.clone();
        exact_copy.uid = 23;
        assert!(remote_candidates_equivalent(&first, &exact_copy));
    }

    #[test]
    fn every_remote_fork_is_persisted_once_under_a_deterministic_identity() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");

        let mut first = remote_candidate("branch A", 2, "2026-07-14T01:00:00Z");
        first.uid = 21;
        first.raw_rfc822 = b"remote branch A".to_vec();
        let mut second = remote_candidate("branch B", 2, "2026-07-14T00:00:00Z");
        second.uid = 22;
        second.raw_rfc822 = b"remote branch B".to_vec();

        assert_eq!(
            backend
                .preserve_remote_fork("shared-draft", &first)
                .unwrap(),
            RemoteForkPreservation::Inserted
        );
        assert_eq!(
            backend
                .preserve_remote_fork("shared-draft", &second)
                .unwrap(),
            RemoteForkPreservation::Inserted
        );
        assert_eq!(
            backend
                .preserve_remote_fork("shared-draft", &first)
                .unwrap(),
            RemoteForkPreservation::AlreadyPreserved
        );

        let drafts = backend.list_drafts().expect("preserved forks");
        assert_eq!(drafts.len(), 2);
        assert!(drafts.iter().all(|draft| draft.status == "conflict"));
        assert!(drafts.iter().any(|draft| draft.subject == "branch A"));
        assert!(drafts.iter().any(|draft| draft.subject == "branch B"));
    }

    #[test]
    fn sent_version_matches_only_its_exact_remote_content_and_preserves_v2() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");

        let mut sent = local_record("sent version", 1, 1, "2026-07-14T01:00:00Z");
        sent.draft.status = "sent".to_owned();
        sent.draft.raw_rfc822 = b"sent version bytes".to_vec();
        backend
            .repository
            .save_draft_record(&sent)
            .expect("sent record");

        let mut matching = remote_candidate("sent version", 1, "2026-07-14T00:00:00Z");
        matching.raw_rfc822 = sent.draft.raw_rfc822.clone();
        assert!(draft_record_matches_remote(&sent, &matching));

        let mut remote_v2 = remote_candidate("remote V2", 2, "2026-07-13T00:00:00Z");
        remote_v2.uid = 11;
        remote_v2.raw_rfc822 = b"remote V2 bytes".to_vec();
        assert!(!draft_record_matches_remote(&sent, &remote_v2));
        assert_eq!(
            backend
                .preserve_remote_fork(&sent.draft.id, &remote_v2)
                .unwrap(),
            RemoteForkPreservation::Inserted
        );

        let visible = backend.list_drafts().expect("visible remote V2");
        assert!(visible.iter().any(|draft| {
            draft.status == "conflict" && draft.subject == "remote V2" && draft.body_text == "body"
        }));
    }

    #[test]
    fn manual_retry_accepts_only_retryable_for_the_active_account() {
        let base = OutboxItem {
            id: "outbox-1".to_owned(),
            account_id: "primary".to_owned(),
            draft_id: None,
            draft_revision: None,
            draft_local_version: None,
            recipients: vec!["receiver@example.com".to_owned()],
            status: OutboxStatus::Retryable,
            attempts: 1,
            last_error: Some("temporary failure".to_owned()),
            created_at: "2026-07-14T06:00:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: b"persisted bytes".to_vec(),
        };
        assert!(validate_manual_retry(&base, "primary").is_ok());

        for status in [
            OutboxStatus::Queued,
            OutboxStatus::Sending,
            OutboxStatus::Sent,
            OutboxStatus::Rejected,
            OutboxStatus::DeliveryUnknown,
        ] {
            let item = OutboxItem {
                status,
                ..base.clone()
            };
            assert!(matches!(
                validate_manual_retry(&item, "primary"),
                Err(MailError::Validation(_))
            ));
        }

        assert!(matches!(
            validate_manual_retry(&base, "another-account"),
            Err(MailError::NotFound { .. })
        ));
    }
}
