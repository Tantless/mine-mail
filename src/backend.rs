use std::{
    cmp::Ordering,
    collections::{BTreeSet, BinaryHeap, HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
    },
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    sync::{Mutex, MutexGuard, Semaphore},
    time::Instant,
};
use uuid::Uuid;

use crate::{
    AccountConfig, ComposeRequest, ConnectionReport, ContactActivity, ContactMessage,
    ContactMessageDirection, Draft, DraftDeleteKind, DraftSaveKind, DraftSaveOutcome, InboxMessage,
    MailAddress, MailError, MailboxRole, OutboxItem, OutboxStatus, ReplyContext, Result,
    StationeryTheme, SyncBatchProgress, SyncReport,
    database::{
        DraftRecord, MailboxState, ManagedDraftAttachment, NewDraftAttachment,
        PendingMessageAction, PreparedForwardInsert, Repository,
        managed_attachment_integrity_error,
    },
    imap_client::{
        CreatableMailboxRole, DeleteFinalization, ImapConnection, MailboxHint, MailboxMessageScope,
        MessageMoveMethod, RemoteBodyPart, RemoteMailbox, RemoteMessage, RemoteMessageStructure,
        RemoteMimePart, RemoteTransferEncoding,
    },
    mailbox_mutation::{
        PersistedFlagWork, PersistedPhaseWork, persisted_flag_work, persisted_phase_work,
    },
    managed_attachments::{ManagedAttachmentStore, save_extracted_file},
    mime::{
        AttachmentIndexError, AttachmentPartMetadata, ForwardHtmlRenderMode, ForwardSourceError,
        IncomingMetadata, MAX_ATTACHMENT_PARTS, MAX_MANAGED_ATTACHMENT_BYTES,
        MAX_MANAGED_ATTACHMENT_TOTAL_BYTES, ManagedMimeAttachment, MimeSourceCompleteness,
        bounded_original_attachment_name, build_draft_message_revision,
        build_draft_message_revision_with_attachments, build_outgoing_message,
        build_outgoing_message_with_attachments, decode_remote_mime_part, extract_attachment,
        index_message_attachments, outbox_message_id, parse_draft_message, parse_incoming_message,
        parse_incoming_summary_or_fallback, prepare_forward_source,
        prepare_forward_source_without_attachments, render_message_html, restore_outbox_envelope,
        safe_attachment_filename, sanitize_compose_html, validate_attachment_id,
    },
    models::{
        AttachmentDisposition, AttachmentMeta, AttachmentSaveErrorKind, AttachmentSaveResult,
        AttachmentSaveStatus, DraftAttachmentMutationKind, DraftAttachmentMutationOutcome,
        DraftDto, DraftSyncReport, ForwardContext, ForwardPreparationError,
        ForwardPreparationErrorKind, ForwardPreparationOutcome, ForwardQuotedRenderMode,
        ForwardWarning, MailboxCapability, MailboxCapabilityStatus,
        MailboxCapabilityUnavailableReason, MessageActionKind, MessageMutationErrorKind,
        MessageMutationReceipt, MessagePage, MessagePageCursor, MutationStatus, PreparedForward,
        RemoteHistoryState, RemoteMutationPhase, SystemFlagKind, SystemFlagMutationReceipt,
        normalize_contact_email,
    },
    smtp_client::SmtpClient,
};

const INBOX: &str = "INBOX";
const SUMMARY_BATCH_SIZE: usize = 10;
const PREVIEW_BACKFILL_LIMIT: usize = 250;
const MANAGED_ATTACHMENT_CLEANUP_GRACE: Duration = Duration::from_secs(60 * 60);
const FLAG_BATCH_SIZE: usize = 250;
const MAX_CACHED_MESSAGE_BYTES: u32 = 50 * 1024 * 1024;
const MAX_REPLY_QUOTED_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_REPLY_QUOTED_HTML_BYTES: usize = 12 * 1024 * 1024;
const MAX_SELECTED_BODY_SECTION_BYTES: u64 = 12 * 1024 * 1024;
const REMOTE_ATTACHMENT_ID_PREFIX: &str = "MMR1_";
const MAX_LOCAL_DRAFT_CAS_RETRIES: usize = 32;
const BODY_IMAP_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(60);
const DEFAULT_MESSAGE_PAGE_SIZE: usize = 50;
const MAX_MESSAGE_PAGE_SIZE: usize = 100;
const HISTORY_FETCH_PAGE_SIZE: usize = 50;
const PERMANENT_DELETE_PLAN_MINUTES: i64 = 5;
const MAX_PENDING_DELETE_PLANS: usize = 64;
const BODY_PREFETCH_PRIORITY_RECENT: u8 = 0;
const BODY_PREFETCH_PRIORITY_PAGE: u8 = 1;
const BODY_PREFETCH_PRIORITY_NEIGHBOR: u8 = 2;
const BODY_PREFETCH_NEIGHBOR_RADIUS: usize = 2;

fn normalize_owned_compose_html(mut request: ComposeRequest) -> ComposeRequest {
    request.format.body_html = sanitize_compose_html(request.format.body_html.as_deref());
    if request.format.stationery == StationeryTheme::None {
        request.format.send_stationery = false;
    }
    request
}

fn advance_draft_sync_progress<F>(completed: &mut usize, total: usize, on_progress: &mut F)
where
    F: FnMut(SyncBatchProgress),
{
    *completed += 1;
    if *completed % SUMMARY_BATCH_SIZE == 0 || *completed == total {
        on_progress(SyncBatchProgress {
            completed: *completed,
            total,
        });
    }
}

struct BodyImapSession {
    connection: ImapConnection,
    last_used: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyFetchLane {
    Foreground,
    Prefetch,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct BodyDownloadKey {
    mailbox: String,
    uid: u32,
}

struct BodyDownloadOwner<'a> {
    downloads: &'a StdMutex<HashMap<BodyDownloadKey, Arc<Semaphore>>>,
    key: BodyDownloadKey,
    signal: Arc<Semaphore>,
}

impl Drop for BodyDownloadOwner<'_> {
    fn drop(&mut self) {
        let mut downloads = self
            .downloads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if downloads
            .get(&self.key)
            .is_some_and(|current| Arc::ptr_eq(current, &self.signal))
        {
            downloads.remove(&self.key);
        }
        self.signal.close();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BodyPrefetchJob {
    public_id: String,
    priority: u8,
    sequence: u64,
    page_generation: Option<u64>,
}

impl Ord for BodyPrefetchJob {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.sequence.cmp(&self.sequence))
            .then_with(|| self.public_id.cmp(&other.public_id))
    }
}

impl PartialOrd for BodyPrefetchJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QueuedBodyPrefetch {
    priority: u8,
    sequence: u64,
    page_generation: Option<u64>,
}

#[derive(Default)]
struct BodyPrefetchQueue {
    jobs: BinaryHeap<BodyPrefetchJob>,
    queued: HashMap<String, QueuedBodyPrefetch>,
    current_page: Vec<String>,
}

impl BodyPrefetchQueue {
    fn cancel_page_jobs(&mut self) {
        self.queued
            .retain(|_, queued| queued.page_generation.is_none());
    }

    fn enqueue(
        &mut self,
        public_id: String,
        priority: u8,
        sequence: u64,
        page_generation: Option<u64>,
    ) -> bool {
        if self
            .queued
            .get(&public_id)
            .is_some_and(|queued| queued.priority > priority)
        {
            return false;
        }
        self.queued.insert(
            public_id.clone(),
            QueuedBodyPrefetch {
                priority,
                sequence,
                page_generation,
            },
        );
        self.jobs.push(BodyPrefetchJob {
            public_id,
            priority,
            sequence,
            page_generation,
        });
        true
    }

    fn pop_next(&mut self, current_page_generation: u64) -> Option<BodyPrefetchJob> {
        while let Some(job) = self.jobs.pop() {
            let Some(queued) = self.queued.get(&job.public_id).copied() else {
                continue;
            };
            if queued.sequence != job.sequence {
                continue;
            }
            if job
                .page_generation
                .is_some_and(|generation| generation != current_page_generation)
            {
                self.queued.remove(&job.public_id);
                continue;
            }
            self.queued.remove(&job.public_id);
            return Some(job);
        }
        None
    }
}

fn bounded_body_prefetch_ids(
    candidates: impl IntoIterator<Item = (String, u32)>,
    max_total_bytes: u64,
    max_message_bytes: u32,
) -> Vec<String> {
    if max_total_bytes == 0 || max_message_bytes == 0 {
        return Vec::new();
    }
    let mut selected = Vec::new();
    let mut total_bytes = 0u64;
    for (public_id, size_bytes) in candidates {
        if size_bytes == 0 || size_bytes > max_message_bytes {
            continue;
        }
        let next_total = total_bytes.saturating_add(u64::from(size_bytes));
        if next_total > max_total_bytes {
            continue;
        }
        total_bytes = next_total;
        selected.push(public_id);
    }
    selected
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PermanentDeletePlan {
    pub plan_id: String,
    pub expires_at: String,
}

#[derive(Clone, Debug)]
struct PermanentDeletePlanState {
    message_id: i64,
    expires_at: DateTime<Utc>,
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
    attachments: Vec<ManagedMimeAttachment>,
    forward_context: Option<ForwardContext>,
}

struct PendingManagedImports<'a> {
    store: &'a ManagedAttachmentStore,
    additions: Vec<NewDraftAttachment>,
    committed: bool,
}

impl<'a> PendingManagedImports<'a> {
    fn new(store: &'a ManagedAttachmentStore) -> Self {
        Self {
            store,
            additions: Vec::new(),
            committed: false,
        }
    }

    fn push(&mut self, addition: NewDraftAttachment) {
        self.additions.push(addition);
    }

    fn additions(&self) -> &[NewDraftAttachment] {
        &self.additions
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for PendingManagedImports<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for addition in &self.additions {
            let _ = self
                .store
                .remove_internal_file(&addition.imported.internal_name);
        }
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExactSourceDeleteOutcome {
    Removed,
    DeferredServerCleanup,
}

fn available_mailbox_capability(role: MailboxRole, mailbox: &str) -> MailboxCapability {
    MailboxCapability {
        role,
        status: MailboxCapabilityStatus::Available,
        display_name: Some(mailbox.to_owned()),
        unavailable_reason: None,
        retryable: false,
    }
}

fn missing_mailbox_capability(role: MailboxRole) -> MailboxCapability {
    match role {
        MailboxRole::Archive | MailboxRole::Trash => MailboxCapability {
            role,
            status: MailboxCapabilityStatus::NeedsCreationConfirmation,
            display_name: None,
            unavailable_reason: None,
            retryable: true,
        },
        MailboxRole::Sent | MailboxRole::Drafts => MailboxCapability {
            role,
            status: MailboxCapabilityStatus::Unavailable,
            display_name: None,
            unavailable_reason: Some(MailboxCapabilityUnavailableReason::ProviderUnsupported),
            retryable: true,
        },
        MailboxRole::Inbox => available_mailbox_capability(MailboxRole::Inbox, INBOX),
    }
}

fn failed_mailbox_creation(
    role: MailboxRole,
    reason: MailboxCapabilityUnavailableReason,
    retryable: bool,
) -> MailboxCapability {
    MailboxCapability {
        role,
        status: MailboxCapabilityStatus::Unavailable,
        display_name: None,
        unavailable_reason: Some(reason),
        retryable,
    }
}

fn selectable_role_mailbox<'a>(
    role: MailboxRole,
    mailboxes: &'a [RemoteMailbox],
    gmail_adapter: bool,
) -> Option<&'a RemoteMailbox> {
    let mut candidates = mailboxes
        .iter()
        .filter(|mailbox| {
            if !mailbox.is_selectable {
                return false;
            }
            match role {
                MailboxRole::Inbox => mailbox.name.eq_ignore_ascii_case(INBOX),
                MailboxRole::Sent => mailbox.is_sent || sent_fallback_name_matches(&mailbox.name),
                MailboxRole::Drafts => {
                    mailbox.is_drafts || mailbox.name.eq_ignore_ascii_case("Drafts")
                }
                MailboxRole::Archive => mailbox.is_archive || (gmail_adapter && mailbox.is_all),
                MailboxRole::Trash => mailbox.is_trash,
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|mailbox| mailbox.name.to_lowercase());
    candidates.into_iter().next()
}

fn sent_fallback_name_matches(name: &str) -> bool {
    const FALLBACK_NAMES: &[&str] = &[
        "Sent",
        "Sent Messages",
        "Sent Items",
        "已发送",
        "已发送邮件",
    ];
    let leaf = name.rsplit(['/', '.']).next().unwrap_or(name);
    FALLBACK_NAMES
        .iter()
        .any(|fallback| name.eq_ignore_ascii_case(fallback) || leaf.eq_ignore_ascii_case(fallback))
}

fn discovered_mailbox_capability(
    role: MailboxRole,
    mailboxes: &[RemoteMailbox],
    gmail_adapter: bool,
) -> MailboxCapability {
    selectable_role_mailbox(role, mailboxes, gmail_adapter)
        .map(|mailbox| available_mailbox_capability(role, &mailbox.name))
        .unwrap_or_else(|| missing_mailbox_capability(role))
}

fn confirmed_created_mailbox_capability(
    role: MailboxRole,
    mailboxes: &[RemoteMailbox],
    gmail_adapter: bool,
) -> MailboxCapability {
    let discovered = discovered_mailbox_capability(role, mailboxes, gmail_adapter);
    if discovered.status == MailboxCapabilityStatus::Available {
        return discovered;
    }
    let canonical_name = match role {
        MailboxRole::Archive => Some(CreatableMailboxRole::Archive.canonical_name()),
        MailboxRole::Trash => Some(CreatableMailboxRole::Trash.canonical_name()),
        _ => None,
    };
    canonical_name
        .and_then(|canonical_name| {
            mailboxes.iter().find(|mailbox| {
                mailbox.is_selectable && mailbox.name.eq_ignore_ascii_case(canonical_name)
            })
        })
        .map(|mailbox| available_mailbox_capability(role, &mailbox.name))
        .unwrap_or(discovered)
}

/// Reusable application service for the future Tauri command layer.
///
/// The React UI must call this service through narrowly scoped Tauri commands;
/// it should never receive the authorization password or open IMAP/SMTP itself.
pub struct MailBackend {
    config: AccountConfig,
    repository: Repository,
    managed_attachments: ManagedAttachmentStore,
    general_imap_gate: Mutex<()>,
    inbox_imap_gate: Mutex<()>,
    inbox_sync_imap: Mutex<Option<ImapConnection>>,
    inbox_sync_state: StdMutex<(u64, Option<SyncReport>)>,
    sent_imap_gate: Mutex<()>,
    draft_imap_gate: Mutex<()>,
    body_imap: Mutex<Option<BodyImapSession>>,
    body_prefetch_imap: Mutex<Option<BodyImapSession>>,
    body_downloads: StdMutex<HashMap<BodyDownloadKey, Arc<Semaphore>>>,
    body_prefetch_queue: StdMutex<BodyPrefetchQueue>,
    body_prefetch_worker_started: AtomicBool,
    body_prefetch_page_generation: AtomicU64,
    body_prefetch_sequence: AtomicU64,
    body_cache_budget_bytes: AtomicU64,
    permanent_delete_plans: Mutex<HashMap<String, PermanentDeletePlanState>>,
    smtp_gate: Mutex<()>,
}

impl MailBackend {
    pub fn open(config: AccountConfig, database_path: impl AsRef<Path>) -> Result<Self> {
        let database_path = database_path.as_ref();
        let database_path = if database_path.is_absolute() {
            database_path.to_path_buf()
        } else {
            std::env::current_dir()?.join(database_path)
        };
        let product_data_root = database_path.parent().ok_or_else(|| {
            MailError::Validation(
                "the local mail database has no product-data directory".to_owned(),
            )
        })?;
        let repository = Repository::open(&database_path)?;
        let managed_attachments =
            ManagedAttachmentStore::new(product_data_root, &config.account_id)?;
        Ok(Self {
            config,
            repository,
            managed_attachments,
            general_imap_gate: Mutex::new(()),
            inbox_imap_gate: Mutex::new(()),
            inbox_sync_imap: Mutex::new(None),
            inbox_sync_state: StdMutex::new((0, None)),
            sent_imap_gate: Mutex::new(()),
            draft_imap_gate: Mutex::new(()),
            body_imap: Mutex::new(None),
            body_prefetch_imap: Mutex::new(None),
            body_downloads: StdMutex::new(HashMap::new()),
            body_prefetch_queue: StdMutex::new(BodyPrefetchQueue::default()),
            body_prefetch_worker_started: AtomicBool::new(false),
            body_prefetch_page_generation: AtomicU64::new(0),
            body_prefetch_sequence: AtomicU64::new(0),
            body_cache_budget_bytes: AtomicU64::new(u64::MAX),
            permanent_delete_plans: Mutex::new(HashMap::new()),
            smtp_gate: Mutex::new(()),
        })
    }

    pub fn initialize(&self) -> Result<()> {
        self.initialize_internal(true)
    }

    /// Initializes another runtime handle over an already-open account
    /// database without interpreting a live SMTP attempt as abandoned.
    pub fn initialize_without_outbox_recovery(&self) -> Result<()> {
        self.initialize_internal(false)
    }

    fn initialize_internal(&self, recover_outbox: bool) -> Result<()> {
        self.repository.initialize_account(&self.config)?;
        if recover_outbox {
            // Current senders create queued and claim sending inside one SQLite
            // transaction, so a visible queued row can only be a legacy/crashed
            // item from an older lifecycle and is safe to expose for manual retry.
            self.repository.recover_queued_as_retryable()?;
            self.repository.recover_sending_as_delivery_unknown()?;
        }
        self.cleanup_managed_attachments()?;
        Ok(())
    }

    /// Cleans only Rust-owned temporary or unreferenced managed files. A blob
    /// retained by any draft, conflict copy, or immutable Outbox row remains
    /// protected by SQLite references.
    pub fn cleanup_managed_attachments(&self) -> Result<usize> {
        for (draft_id, local_version) in self
            .repository
            .terminal_draft_attachment_versions(&self.config.account_id)?
        {
            self.repository.release_terminal_draft_attachments(
                &self.config.account_id,
                &draft_id,
                local_version,
            )?;
        }
        let mut removed = self
            .managed_attachments
            .cleanup_temporary_files(MANAGED_ATTACHMENT_CLEANUP_GRACE)?;
        for orphan in self
            .repository
            .list_orphaned_managed_attachments(&self.config.account_id)?
        {
            let Some(orphan) = self
                .repository
                .take_orphaned_managed_attachment(&orphan.account_id, &orphan.id)?
            else {
                continue;
            };
            if self
                .managed_attachments
                .remove_internal_file(&orphan.internal_name)?
            {
                removed += 1;
            }
        }
        let registered = self.repository.all_managed_attachment_internal_names()?;
        removed += self
            .managed_attachments
            .cleanup_unregistered_files(&registered, MANAGED_ATTACHMENT_CLEANUP_GRACE)?;
        Ok(removed)
    }

    /// Removes only this backend account's Rust-owned attachment directory.
    /// Desktop account removal may call this after explicit local-data
    /// confirmation; keeping local data must not call it.
    pub fn delete_managed_attachment_data(&self) -> Result<bool> {
        self.managed_attachments.delete_account_storage()
    }

    fn ensure_account_scope(&self, account_id: &str) -> Result<()> {
        if account_id != self.config.account_id {
            return Err(MailError::Validation(
                "the requested account does not match this backend".to_owned(),
            ));
        }
        Ok(())
    }

    fn uses_gmail_archive_adapter(&self) -> bool {
        self.config.imap.host.eq_ignore_ascii_case("imap.gmail.com")
    }

    fn mailbox_message_scope(&self, role: MailboxRole) -> MailboxMessageScope {
        if role == MailboxRole::Archive && self.uses_gmail_archive_adapter() {
            MailboxMessageScope::GmailArchive
        } else {
            MailboxMessageScope::All
        }
    }

    fn semantic_role_for_mailbox(&self, mailbox: &str) -> Result<Option<MailboxRole>> {
        for role in [
            MailboxRole::Inbox,
            MailboxRole::Sent,
            MailboxRole::Drafts,
            MailboxRole::Archive,
            MailboxRole::Trash,
        ] {
            let mapped = match self
                .repository
                .mailbox_for_semantic_role(&self.config.account_id, role)
            {
                Ok(mapped) => mapped,
                Err(MailError::NotFound { .. }) => continue,
                Err(error) => return Err(error),
            };
            let matches = if role == MailboxRole::Inbox {
                mapped.eq_ignore_ascii_case(mailbox)
            } else {
                mapped == mailbox
            };
            if matches {
                return Ok(Some(role));
            }
        }
        Ok(None)
    }

    fn ensure_role_available(&self, role: MailboxRole) -> Result<()> {
        let capability = self
            .repository
            .mailbox_capability(&self.config.account_id, role)?;
        if capability
            .as_ref()
            .is_some_and(|capability| capability.status == MailboxCapabilityStatus::Available)
        {
            Ok(())
        } else {
            Err(MailError::Validation(
                "the requested mailbox role is unavailable".to_owned(),
            ))
        }
    }

    /// Returns the last persisted account-scoped role discovery immediately;
    /// this method never opens the network.
    pub fn get_mailbox_capabilities(&self, account_id: &str) -> Result<Vec<MailboxCapability>> {
        self.ensure_account_scope(account_id)?;
        self.repository.mailbox_capabilities(account_id)
    }

    pub fn mailbox_capabilities(&self, account_id: &str) -> Result<Vec<MailboxCapability>> {
        self.get_mailbox_capabilities(account_id)
    }

    /// Reports whether a semantic mailbox has completed at least one local
    /// summary synchronization. Role discovery alone creates the mailbox row,
    /// so row existence is not sufficient to opt Archive/Trash into periodic
    /// reconciliation.
    pub fn mailbox_role_initialized(&self, account_id: &str, role: MailboxRole) -> Result<bool> {
        self.ensure_account_scope(account_id)?;
        let mailbox = self
            .repository
            .mailbox_for_semantic_role(account_id, role)?;
        Ok(self
            .repository
            .mailbox_state(account_id, &mailbox)?
            .and_then(|state| state.last_synced_at)
            .is_some())
    }

    /// Returns whether this account has durable message-mutation work,
    /// including confirmed source-cleanup tombstones. The scheduler uses this
    /// to keep optional destination mailboxes participating until convergence.
    pub fn has_message_mutation_activity(&self, account_id: &str) -> Result<bool> {
        self.ensure_account_scope(account_id)?;
        Ok(!self
            .repository
            .pending_message_actions(account_id)?
            .is_empty()
            || !self
                .repository
                .message_actions_requiring_reconciliation(account_id)?
                .is_empty()
            || !self
                .repository
                .confirmed_source_cleanup_tombstones(account_id)?
                .is_empty())
    }

    /// Performs one authoritative LIST and persists only semantic role
    /// mappings. Archive and Trash never use ordinary-name guesses.
    pub async fn discover_mailbox_roles(&self, account_id: &str) -> Result<Vec<MailboxCapability>> {
        self.ensure_account_scope(account_id)?;
        let _guard = self.general_imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config)
            .await
            .map_err(|_| privacy_safe_imap_error("mailbox discovery"))?;
        let mailboxes = connection
            .list_mailboxes()
            .await
            .map_err(|_| privacy_safe_imap_error("mailbox discovery"))?;
        let gmail_adapter = self.uses_gmail_archive_adapter();
        let mut capabilities = Vec::with_capacity(5);
        for role in [
            MailboxRole::Inbox,
            MailboxRole::Sent,
            MailboxRole::Drafts,
            MailboxRole::Archive,
            MailboxRole::Trash,
        ] {
            let capability = discovered_mailbox_capability(role, &mailboxes, gmail_adapter);
            self.repository
                .set_mailbox_capability(account_id, &capability)?;
            capabilities.push(capability);
        }
        let _ = connection.logout().await;
        Ok(capabilities)
    }

    /// Creates only the fixed Archive or Trash fallback after the caller's
    /// confirmation. LIST both before and after CREATE makes this idempotent
    /// and prevents an ordinary same-named folder from becoming a role.
    pub async fn create_mailbox_role(
        &self,
        account_id: &str,
        role: MailboxRole,
    ) -> Result<MailboxCapability> {
        self.ensure_account_scope(account_id)?;
        let creatable = match role {
            MailboxRole::Archive => CreatableMailboxRole::Archive,
            MailboxRole::Trash => CreatableMailboxRole::Trash,
            _ => {
                return Err(MailError::Validation(
                    "only Archive or Trash can be created by Mine Mail".to_owned(),
                ));
            }
        };
        let _guard = self.general_imap_gate.lock().await;
        let mut connection = match ImapConnection::connect(&self.config).await {
            Ok(connection) => connection,
            Err(_) => {
                let capability = failed_mailbox_creation(
                    role,
                    MailboxCapabilityUnavailableReason::CreateFailed,
                    true,
                );
                self.repository
                    .set_mailbox_capability(account_id, &capability)?;
                return Ok(capability);
            }
        };
        let gmail_adapter = self.uses_gmail_archive_adapter();
        let before = match connection.list_mailboxes().await {
            Ok(mailboxes) => mailboxes,
            Err(_) => {
                let capability = failed_mailbox_creation(
                    role,
                    MailboxCapabilityUnavailableReason::CreateFailed,
                    true,
                );
                self.repository
                    .set_mailbox_capability(account_id, &capability)?;
                let _ = connection.logout().await;
                return Ok(capability);
            }
        };
        let existing = discovered_mailbox_capability(role, &before, gmail_adapter);
        if existing.status == MailboxCapabilityStatus::Available {
            self.repository
                .set_mailbox_capability(account_id, &existing)?;
            let _ = connection.logout().await;
            return Ok(existing);
        }

        let create_succeeded = connection.create_mailbox_role(creatable).await.is_ok();
        let after = connection.list_mailboxes().await;
        let capability = match after {
            Ok(mailboxes) => {
                let discovered =
                    confirmed_created_mailbox_capability(role, &mailboxes, gmail_adapter);
                if discovered.status == MailboxCapabilityStatus::Available {
                    discovered
                } else if create_succeeded {
                    failed_mailbox_creation(
                        role,
                        MailboxCapabilityUnavailableReason::CreatedMailboxNotSelectable,
                        false,
                    )
                } else {
                    failed_mailbox_creation(
                        role,
                        MailboxCapabilityUnavailableReason::CreateFailed,
                        true,
                    )
                }
            }
            Err(_) => failed_mailbox_creation(
                role,
                MailboxCapabilityUnavailableReason::CreateFailed,
                true,
            ),
        };
        self.repository
            .set_mailbox_capability(account_id, &capability)?;
        let _ = connection.logout().await;
        Ok(capability)
    }

    /// Persists the same typed role-creation failure when the desktop cannot
    /// obtain a network-ready backend for the selected account.
    pub fn record_mailbox_role_creation_unavailable(
        &self,
        account_id: &str,
        role: MailboxRole,
    ) -> Result<MailboxCapability> {
        self.ensure_account_scope(account_id)?;
        if !matches!(role, MailboxRole::Archive | MailboxRole::Trash) {
            return Err(MailError::Validation(
                "only Archive or Trash can record a role-creation failure".to_owned(),
            ));
        }
        let capability =
            failed_mailbox_creation(role, MailboxCapabilityUnavailableReason::CreateFailed, true);
        self.repository
            .set_mailbox_capability(account_id, &capability)?;
        Ok(capability)
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
            let _guard = self.general_imap_gate.lock().await;
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
        let _guard = self.general_imap_gate.lock().await;
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

    fn inbox_sync_generation(&self) -> Result<u64> {
        self.inbox_sync_state
            .lock()
            .map(|state| state.0)
            .map_err(|_| MailError::Io(std::io::Error::other("Inbox sync state is unavailable")))
    }

    fn inbox_sync_completed_after(&self, observed_generation: u64) -> Result<Option<SyncReport>> {
        let state = self
            .inbox_sync_state
            .lock()
            .map_err(|_| MailError::Io(std::io::Error::other("Inbox sync state is unavailable")))?;
        Ok((state.0 != observed_generation)
            .then(|| state.1.clone())
            .flatten())
    }

    fn record_inbox_sync_completion(&self, report: &SyncReport) -> Result<()> {
        let mut state = self
            .inbox_sync_state
            .lock()
            .map_err(|_| MailError::Io(std::io::Error::other("Inbox sync state is unavailable")))?;
        state.0 = state.0.wrapping_add(1);
        state.1 = Some(report.clone());
        Ok(())
    }

    async fn take_inbox_sync_connection(&self) -> Result<ImapConnection> {
        let cached = self.inbox_sync_imap.lock().await.take();
        if let Some(mut connection) = cached
            && connection.noop().await.is_ok()
        {
            return Ok(connection);
        }
        ImapConnection::connect(&self.config).await
    }

    async fn store_inbox_sync_connection(&self, connection: ImapConnection) {
        *self.inbox_sync_imap.lock().await = Some(connection);
    }

    /// Synchronize Inbox metadata without downloading message bodies.
    ///
    /// On the first run only the newest `initial_limit` messages are cached.
    /// Later runs fetch new UIDs, reconcile flags and remove locally cached UIDs
    /// that no longer exist on the server.
    pub async fn sync_inbox(&self, initial_limit: usize) -> Result<SyncReport> {
        self.sync_inbox_with_progress(initial_limit, |_| {}).await
    }

    pub async fn sync_inbox_with_progress<F>(
        &self,
        initial_limit: usize,
        mut on_progress: F,
    ) -> Result<SyncReport>
    where
        F: FnMut(SyncBatchProgress) + Send,
    {
        self.validate_sync_limit(initial_limit)?;
        let observed_generation = self.inbox_sync_generation()?;

        let _ = self
            .flush_pending_message_mutations(&self.config.account_id)
            .await;
        let _guard = self.inbox_imap_gate.lock().await;
        if let Some(report) = self.inbox_sync_completed_after(observed_generation)? {
            return Ok(report);
        }
        let mut connection = self.take_inbox_sync_connection().await?;
        let report = self
            .sync_selected_mailbox(&mut connection, INBOX, initial_limit, &mut on_progress)
            .await;
        if let Ok(completed) = report.as_ref() {
            self.store_inbox_sync_connection(connection).await;
            self.record_inbox_sync_completion(completed)?;
        }
        report
    }

    /// Synchronize the server-designated Sent mailbox. The discovered mailbox
    /// name is persisted as a role so all later local reads stay offline-first
    /// and do not have to guess provider-specific or localized folder names.
    pub async fn sync_sent(&self, initial_limit: usize) -> Result<SyncReport> {
        self.sync_sent_with_progress(initial_limit, |_| {}).await
    }

    pub async fn sync_sent_with_progress<F>(
        &self,
        initial_limit: usize,
        mut on_progress: F,
    ) -> Result<SyncReport>
    where
        F: FnMut(SyncBatchProgress) + Send,
    {
        self.validate_sync_limit(initial_limit)?;

        let _ = self
            .flush_pending_message_mutations(&self.config.account_id)
            .await;
        let _guard = self.sent_imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config).await?;
        let mailbox = connection.discover_sent_mailbox().await?;
        let report = self
            .sync_selected_mailbox(&mut connection, &mailbox, initial_limit, &mut on_progress)
            .await;
        let _ = connection.logout().await;
        let report = report?;
        self.repository
            .assign_mailbox_role(&self.config.account_id, "sent", &mailbox)?;
        self.reconcile_sent_outbox(&mailbox)?;
        Ok(report)
    }

    fn reconcile_sent_outbox(&self, sent_mailbox: &str) -> Result<usize> {
        let mut retired = 0;
        for candidate in self
            .repository
            .list_sent_reconciliation_candidates(&self.config.account_id)?
        {
            let Some(message_id) = outbox_message_id(&candidate.raw_rfc822) else {
                continue;
            };
            if self.repository.reconcile_outbox_with_cached_sent(
                &candidate.id,
                &self.config.account_id,
                sent_mailbox,
                &message_id,
            )? {
                retired += 1;
            }
        }
        if retired > 0 {
            let _ = self.cleanup_managed_attachments();
        }
        Ok(retired)
    }

    /// Synchronizes one semantic role while preserving the per-account backend
    /// boundary and returns the bounded number of newly synchronized messages.
    /// Archive and Trash require a persisted available capability; no action is
    /// silently redirected to another folder.
    pub async fn sync_mailbox(&self, account_id: &str, role: MailboxRole) -> Result<usize> {
        self.ensure_account_scope(account_id)?;
        let synced = match role {
            MailboxRole::Inbox => self.sync_inbox(DEFAULT_MESSAGE_PAGE_SIZE).await?.fetched,
            MailboxRole::Sent => self.sync_sent(DEFAULT_MESSAGE_PAGE_SIZE).await?.fetched,
            MailboxRole::Drafts => {
                let report = self.sync_drafts(None).await?;
                report.pulled.saturating_add(report.pushed)
            }
            MailboxRole::Archive | MailboxRole::Trash => {
                let _ = self.flush_pending_message_mutations(account_id).await;
                let capability = self.repository.mailbox_capability(account_id, role)?;
                if capability.as_ref().is_none_or(|capability| {
                    capability.status != MailboxCapabilityStatus::Available
                }) {
                    return Err(MailError::Validation(
                        "the requested mailbox role is unavailable".to_owned(),
                    ));
                }
                let mailbox = self
                    .repository
                    .mailbox_for_semantic_role(account_id, role)?;
                let _guard = self.general_imap_gate.lock().await;
                let mut connection = ImapConnection::connect(&self.config)
                    .await
                    .map_err(|_| privacy_safe_imap_error("mailbox synchronization"))?;
                let result = self
                    .sync_selected_mailbox_with_scope(
                        &mut connection,
                        &mailbox,
                        DEFAULT_MESSAGE_PAGE_SIZE,
                        self.mailbox_message_scope(role),
                        &mut |_| {},
                    )
                    .await
                    .map(|report| report.fetched)
                    .map_err(|error| privacy_safe_network_error(error, "mailbox synchronization"));
                let _ = connection.logout().await;
                result?
            }
        };
        Ok(synced)
    }

    fn validate_sync_limit(&self, initial_limit: usize) -> Result<()> {
        if initial_limit == 0 {
            return Err(MailError::Validation(
                "initial sync limit must be greater than zero".to_owned(),
            ));
        }
        Ok(())
    }

    async fn fetch_and_cache_summaries(
        &self,
        connection: &mut ImapConnection,
        mailbox: &str,
        uids: &[u32],
    ) -> Result<usize> {
        let remotes = connection.fetch_summaries(uids).await?;
        let fetched = remotes.len();
        for remote in remotes {
            let message = self.parse_remote_summary(mailbox, remote);
            self.repository.upsert_message_summary(&message)?;
        }
        Ok(fetched)
    }

    fn parse_remote_summary(&self, mailbox: &str, remote: RemoteMessage) -> InboxMessage {
        parse_incoming_summary_or_fallback(
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
        )
    }

    async fn sync_selected_mailbox<F>(
        &self,
        connection: &mut ImapConnection,
        mailbox: &str,
        initial_limit: usize,
        on_progress: &mut F,
    ) -> Result<SyncReport>
    where
        F: FnMut(SyncBatchProgress) + Send,
    {
        self.sync_selected_mailbox_with_scope(
            connection,
            mailbox,
            initial_limit,
            MailboxMessageScope::All,
            on_progress,
        )
        .await
    }

    async fn sync_selected_mailbox_with_scope<F>(
        &self,
        connection: &mut ImapConnection,
        mailbox: &str,
        initial_limit: usize,
        scope: MailboxMessageScope,
        on_progress: &mut F,
    ) -> Result<SyncReport>
    where
        F: FnMut(SyncBatchProgress) + Send,
    {
        let snapshot = connection.select_mailbox_with_scope(mailbox, scope).await?;

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

        // A read action is committed locally before the network round trip.
        // Push those durable intents before accepting a remote flag snapshot,
        // otherwise a stale server `FLAGS` response could make the message
        // appear unread again while the write is still pending.
        let _ = self
            .flush_pending_seen_updates(connection, mailbox, snapshot.uid_validity)
            .await;
        let _ = self
            .flush_pending_flagged_updates(connection, mailbox, snapshot.uid_validity)
            .await;

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
        let preview_backfill = self.repository.mailbox_preview_backfill_candidates(
            &self.config.account_id,
            mailbox,
            PREVIEW_BACKFILL_LIMIT,
        )?;
        let total = requested.len() + preview_backfill.len();
        on_progress(SyncBatchProgress {
            completed: 0,
            total,
        });
        let mut fetched = 0;
        let mut completed = 0;
        for batch in requested.chunks(SUMMARY_BATCH_SIZE) {
            fetched += self
                .fetch_and_cache_summaries(connection, mailbox, batch)
                .await?;
            completed += batch.len();
            on_progress(SyncBatchProgress { completed, total });
        }
        for batch in preview_backfill.chunks(SUMMARY_BATCH_SIZE) {
            self.fetch_and_cache_summaries(connection, mailbox, batch)
                .await?;
            completed += batch.len();
            on_progress(SyncBatchProgress { completed, total });
        }

        let existing_remote_uids: Vec<u32> =
            cached_uids.intersection(&remote_uids).copied().collect();
        let changed_since = changed_flags_cursor(
            connection.supports_condstore(),
            uid_validity_reset,
            previous_state
                .as_ref()
                .and_then(|state| state.highest_modseq),
            snapshot.highest_modseq,
        );
        let mut updated_flags = 0;
        for batch in existing_remote_uids.chunks(FLAG_BATCH_SIZE) {
            let updates = match changed_since {
                Some(highest_modseq) => {
                    connection
                        .fetch_flags_changed_since(batch, highest_modseq)
                        .await?
                }
                None => connection.fetch_flags(batch).await?,
            };
            updated_flags += self.repository.update_message_flags_batch(
                &self.config.account_id,
                mailbox,
                &updates,
            )?;
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
        let oldest_cached_uid = self
            .repository
            .cached_uids(&self.config.account_id, mailbox)?
            .into_iter()
            .min();
        if let Some(uid_validity) = snapshot.uid_validity.filter(|value| *value > 0) {
            let history = self
                .repository
                .mailbox_history(&self.config.account_id, mailbox)?
                .unwrap_or_default();
            let complete = cached_total >= remote_uids.len();
            let next_before_uid = if complete {
                None
            } else {
                earlier_history_bound(history.before_uid, oldest_cached_uid)
            };
            let cursor_advances = match (history.before_uid, next_before_uid) {
                (None, Some(_)) | (Some(_), None) => true,
                (Some(current), Some(next)) => next < current,
                (None, None) => complete,
            };
            if cursor_advances {
                self.repository.advance_mailbox_history(
                    &self.config.account_id,
                    mailbox,
                    uid_validity,
                    history.before_uid,
                    next_before_uid,
                    complete,
                    Some(snapshot.exists),
                )?;
            }
        }

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
        self.sync_new_inbox_with_progress(initial_limit, |_| {})
            .await
    }

    pub async fn sync_new_inbox_with_progress<F>(
        &self,
        initial_limit: usize,
        mut on_progress: F,
    ) -> Result<SyncReport>
    where
        F: FnMut(SyncBatchProgress) + Send,
    {
        if initial_limit == 0 {
            return Err(MailError::Validation(
                "initial sync limit must be greater than zero".to_owned(),
            ));
        }

        let observed_generation = self.inbox_sync_generation()?;
        let guard = self.inbox_imap_gate.lock().await;
        if let Some(report) = self.inbox_sync_completed_after(observed_generation)? {
            return Ok(report);
        }
        let mut connection = self.take_inbox_sync_connection().await?;
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
            self.store_inbox_sync_connection(connection).await;
            drop(guard);
            return self
                .sync_inbox_with_progress(initial_limit, on_progress)
                .await;
        }

        let previous_state = previous_state.expect("full sync fallback handles a missing cursor");
        let previous_highest_uid = previous_state
            .highest_uid
            .expect("full sync fallback handles a missing highest UID");
        let _ = self
            .flush_pending_seen_updates(&mut connection, INBOX, hint.uid_validity)
            .await;
        let _ = self
            .flush_pending_flagged_updates(&mut connection, INBOX, hint.uid_validity)
            .await;
        let requested = connection.search_uids_after(previous_highest_uid).await?;
        let preview_backfill = self.repository.mailbox_preview_backfill_candidates(
            &self.config.account_id,
            INBOX,
            PREVIEW_BACKFILL_LIMIT,
        )?;
        let total = requested.len() + preview_backfill.len();
        on_progress(SyncBatchProgress {
            completed: 0,
            total,
        });
        let mut fetched = 0;
        let mut completed = 0;
        for batch in requested.chunks(SUMMARY_BATCH_SIZE) {
            fetched += self
                .fetch_and_cache_summaries(&mut connection, INBOX, batch)
                .await?;
            completed += batch.len();
            on_progress(SyncBatchProgress { completed, total });
        }
        for batch in preview_backfill.chunks(SUMMARY_BATCH_SIZE) {
            self.fetch_and_cache_summaries(&mut connection, INBOX, batch)
                .await?;
            completed += batch.len();
            on_progress(SyncBatchProgress { completed, total });
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
        let report = SyncReport {
            mailbox: INBOX.to_owned(),
            remote_total: hint.exists,
            fetched,
            updated_flags: 0,
            removed: 0,
            cached_total,
            uid_validity_reset: false,
        };
        self.store_inbox_sync_connection(connection).await;
        self.record_inbox_sync_completion(&report)?;
        Ok(report)
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

    /// Returns one SQLite-backed keyset page without opening IMAP.
    pub fn list_mailbox_page(
        &self,
        account_id: &str,
        role: MailboxRole,
        cursor: Option<&MessagePageCursor>,
        page_size: usize,
        query: Option<&str>,
    ) -> Result<MessagePage> {
        self.ensure_account_scope(account_id)?;
        let mut page = self
            .repository
            .list_mailbox_page(account_id, role, cursor, page_size, query)?;
        if query.is_some_and(|query| !query.trim().is_empty()) && !page.has_more_local {
            page.remote_history_state = RemoteHistoryState::Complete;
            page.end_reached = true;
            page.next_cursor = None;
        }
        Ok(page)
    }

    /// Returns one SQLite-backed `\Flagged` page without scanning ordinary
    /// unstarred summaries in React.
    pub fn list_starred_mailbox_page(
        &self,
        account_id: &str,
        role: MailboxRole,
        cursor: Option<&MessagePageCursor>,
        page_size: usize,
        query: Option<&str>,
    ) -> Result<MessagePage> {
        self.ensure_account_scope(account_id)?;
        let mut page = self
            .repository
            .list_starred_mailbox_page(account_id, role, cursor, page_size, query)?;
        if query.is_some_and(|query| !query.trim().is_empty()) && !page.has_more_local {
            page.remote_history_state = RemoteHistoryState::Complete;
            page.end_reached = true;
            page.next_cursor = None;
        }
        Ok(page)
    }

    /// Loads an older local page first. Only when SQLite is exhausted does it
    /// perform one bounded UID-window fetch and then re-read the same keyset.
    pub async fn load_older_mailbox_page(
        &self,
        account_id: &str,
        role: MailboxRole,
        cursor: &MessagePageCursor,
        page_size: usize,
        query: Option<&str>,
    ) -> Result<MessagePage> {
        self.load_older_mailbox_page_filtered(account_id, role, cursor, page_size, query, false)
            .await
    }

    pub async fn load_older_starred_mailbox_page(
        &self,
        account_id: &str,
        role: MailboxRole,
        cursor: &MessagePageCursor,
        page_size: usize,
        query: Option<&str>,
    ) -> Result<MessagePage> {
        self.load_older_mailbox_page_filtered(account_id, role, cursor, page_size, query, true)
            .await
    }

    async fn load_older_mailbox_page_filtered(
        &self,
        account_id: &str,
        role: MailboxRole,
        cursor: &MessagePageCursor,
        page_size: usize,
        query: Option<&str>,
        flagged_only: bool,
    ) -> Result<MessagePage> {
        self.ensure_account_scope(account_id)?;
        let local = if flagged_only {
            self.repository
                .load_older_starred_mailbox_page(account_id, role, cursor, page_size, query)?
        } else {
            self.repository
                .load_older_mailbox_page(account_id, role, cursor, page_size, query)?
        };
        if local.has_more_local
            || local.remote_history_state != RemoteHistoryState::MayHaveMore
            || query.is_some_and(|query| !query.trim().is_empty())
        {
            return if flagged_only {
                self.list_starred_mailbox_page(account_id, role, Some(cursor), page_size, query)
            } else {
                self.list_mailbox_page(account_id, role, Some(cursor), page_size, query)
            };
        }

        let context = self.repository.message_page_cursor_context(cursor)?;
        if context.account_id != account_id
            || context.role != role
            || context.flagged_only != flagged_only
        {
            return Err(MailError::Validation(
                "the message cursor does not match this account and mailbox role".to_owned(),
            ));
        }
        let capability = self.repository.mailbox_capability(account_id, role)?;
        if capability
            .as_ref()
            .is_none_or(|capability| capability.status != MailboxCapabilityStatus::Available)
        {
            return Ok(page_with_remote_state(
                local,
                RemoteHistoryState::Unavailable,
            ));
        }
        let ordinary_history = (!flagged_only)
            .then(|| {
                self.repository
                    .mailbox_history(account_id, &context.mailbox)
            })
            .transpose()?
            .flatten()
            .unwrap_or_default();
        let starred_history = flagged_only
            .then(|| {
                self.repository
                    .starred_mailbox_history(account_id, &context.mailbox)
            })
            .transpose()?
            .flatten()
            .unwrap_or_default();
        let history_before_uid = if flagged_only {
            starred_history.before_uid
        } else {
            ordinary_history.before_uid
        };
        let history_complete = if flagged_only {
            starred_history.complete
        } else {
            ordinary_history.complete
        };
        if history_complete {
            return Ok(page_with_remote_state(local, RemoteHistoryState::Complete));
        }

        let _guard = self.general_imap_gate.lock().await;
        let mut connection = match ImapConnection::connect(&self.config).await {
            Ok(connection) => connection,
            Err(_) => {
                return Ok(page_with_remote_state(local, RemoteHistoryState::Offline));
            }
        };
        let selected = match connection
            .select_mailbox_for_history(&context.mailbox)
            .await
        {
            Ok(selected) => selected,
            Err(_) => {
                let _ = connection.logout().await;
                return Ok(page_with_remote_state(local, RemoteHistoryState::Offline));
            }
        };
        let scope = self.mailbox_message_scope(role);
        let Some(selected_uid_validity) = selected.uid_validity else {
            let _ = connection.logout().await;
            return Ok(page_with_remote_state(
                local,
                RemoteHistoryState::Unavailable,
            ));
        };
        if context.uid_validity != Some(selected_uid_validity) {
            let _ = connection.logout().await;
            return Err(MailError::Validation(
                "the mailbox epoch changed; synchronize before loading more messages".to_owned(),
            ));
        }
        // The unsigned client cursor is never trusted to choose a server UID
        // bound. Only SQLite's account/mailbox history state may advance it.
        let before_uid = earlier_history_bound(history_before_uid, selected.uid_next);
        let Some(before_uid) = before_uid else {
            if selected.exists > 0 {
                let _ = connection.logout().await;
                return Ok(page_with_remote_state(
                    local,
                    RemoteHistoryState::Unavailable,
                ));
            }
            if flagged_only {
                self.repository.advance_starred_mailbox_history(
                    account_id,
                    &context.mailbox,
                    selected_uid_validity,
                    starred_history.before_uid,
                    None,
                    true,
                )?;
            } else {
                self.repository.advance_mailbox_history(
                    account_id,
                    &context.mailbox,
                    selected_uid_validity,
                    ordinary_history.before_uid,
                    None,
                    true,
                    Some(selected.exists),
                )?;
            }
            let _ = connection.logout().await;
            return if flagged_only {
                self.repository
                    .load_older_starred_mailbox_page(account_id, role, cursor, page_size, query)
            } else {
                self.repository
                    .load_older_mailbox_page(account_id, role, cursor, page_size, query)
            };
        };
        let fetch_limit = normalized_message_page_size(page_size).min(HISTORY_FETCH_PAGE_SIZE);
        let searched_result = if flagged_only {
            connection
                .search_flagged_uids_before_with_scope(before_uid, fetch_limit, scope)
                .await
        } else {
            connection
                .search_uids_before_with_scope(before_uid, fetch_limit, scope)
                .await
        };
        let searched = match searched_result {
            Ok(searched) => searched,
            Err(_) => {
                let _ = connection.logout().await;
                return Ok(page_with_remote_state(local, RemoteHistoryState::Offline));
            }
        };
        if !searched.uids.is_empty() {
            let fetched = self
                .fetch_and_cache_summaries(&mut connection, &context.mailbox, &searched.uids)
                .await;
            if fetched.ok() != Some(searched.uids.len()) {
                let _ = connection.logout().await;
                return Ok(page_with_remote_state(local, RemoteHistoryState::Offline));
            }
        }
        if flagged_only {
            self.repository.advance_starred_mailbox_history(
                account_id,
                &context.mailbox,
                selected_uid_validity,
                starred_history.before_uid,
                searched.next_before_uid,
                searched.reached_uid_floor,
            )?;
        } else {
            self.repository.advance_mailbox_history(
                account_id,
                &context.mailbox,
                selected_uid_validity,
                ordinary_history.before_uid,
                searched.next_before_uid,
                searched.reached_uid_floor,
                match scope {
                    MailboxMessageScope::All => Some(selected.exists),
                    MailboxMessageScope::GmailArchive => ordinary_history.remote_total,
                },
            )?;
        }
        let _ = connection.logout().await;
        if flagged_only {
            self.repository
                .load_older_starred_mailbox_page(account_id, role, cursor, page_size, query)
        } else {
            self.repository
                .load_older_mailbox_page(account_id, role, cursor, page_size, query)
        }
    }

    /// Derives one contact row per normalized address from all cached message
    /// headers for this account. The account's own address is excluded and a
    /// participant appearing more than once in one message is counted once.
    pub fn list_contact_activity(&self) -> Result<Vec<ContactActivity>> {
        let own_email = normalize_contact_email(&self.config.email)?;
        let messages = self
            .repository
            .list_contact_source_messages(&self.config.account_id)?;
        let mut order = Vec::new();
        let mut activity_by_email: HashMap<String, ContactActivity> = HashMap::new();

        for source in messages {
            let message = source.message;
            let participants = contact_participants(&message, &own_email);
            for (email, display_name) in participants {
                let activity = activity_by_email.entry(email.clone()).or_insert_with(|| {
                    order.push(email.clone());
                    ContactActivity {
                        email,
                        display_name: None,
                        message_count: 0,
                        last_message_at: message_activity_at(&message),
                        last_subject: message.subject.clone(),
                    }
                });
                activity.message_count += 1;
                if activity.display_name.is_none() {
                    activity.display_name = display_name;
                }
            }
        }

        Ok(order
            .into_iter()
            .filter_map(|email| activity_by_email.remove(&email))
            .collect())
    }

    /// Lists bounded, body-free summaries involving one normalized contact
    /// across every locally cached mailbox. Direction is identity-derived and
    /// therefore does not depend on localized provider folder names.
    pub fn list_contact_messages(&self, email: &str, limit: usize) -> Result<Vec<ContactMessage>> {
        if limit == 0 {
            return Err(MailError::Validation(
                "contact message list limit must be greater than zero".to_owned(),
            ));
        }
        let target_email = normalize_contact_email(email)?;
        let own_email = normalize_contact_email(&self.config.email)?;
        let messages = self.repository.list_contact_source_messages_for_email(
            &self.config.account_id,
            &target_email,
            limit,
        )?;
        let sent_mailbox = match self
            .repository
            .mailbox_for_role(&self.config.account_id, "sent")
        {
            Ok(mailbox) => Some(mailbox),
            Err(MailError::NotFound { .. }) => None,
            Err(error) => return Err(error),
        };

        Ok(messages
            .into_iter()
            .map(|source| {
                let message = source.message;
                let direction = if message
                    .sender
                    .as_ref()
                    .and_then(|sender| normalize_contact_email(&sender.email).ok())
                    .as_deref()
                    == Some(own_email.as_str())
                {
                    ContactMessageDirection::Outgoing
                } else {
                    ContactMessageDirection::Incoming
                };
                let mailbox_role = if message.mailbox.eq_ignore_ascii_case(INBOX) {
                    Some(MailboxRole::Inbox)
                } else if sent_mailbox.as_deref() == Some(message.mailbox.as_str()) {
                    Some(MailboxRole::Sent)
                } else {
                    None
                };
                ContactMessage {
                    public_id: source.public_id,
                    direction,
                    mailbox_role,
                    message,
                }
            })
            .collect())
    }

    /// Returns the opaque identity of a row that Rust has already loaded for
    /// this exact account. This is intentionally not a mailbox/UID lookup
    /// surface: callers may only convert the cached object they already hold.
    pub fn public_id_for_cached_message(&self, message: &InboxMessage) -> Result<String> {
        if message.account_id != self.config.account_id || message.id <= 0 {
            return Err(MailError::NotFound {
                entity: "cached message",
                id: "local-row".to_owned(),
            });
        }
        self.repository
            .message_public_id_by_local_id(&self.config.account_id, message.id)
    }

    /// Notification-specific restriction over the general cached-object
    /// converter. Desktop notification flows may only open an Inbox row.
    pub fn public_id_for_cached_inbox_message(&self, message: &InboxMessage) -> Result<String> {
        if !message.mailbox.eq_ignore_ascii_case(INBOX) {
            return Err(MailError::NotFound {
                entity: "cached Inbox message",
                id: "local-row".to_owned(),
            });
        }
        self.public_id_for_cached_message(message)
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

    /// Resolves a contact-history message by its exact IMAP identity. UIDs are
    /// mailbox-scoped, so callers must never infer the mailbox from direction.
    pub fn cached_contact_message(&self, mailbox: &str, uid: u32) -> Result<InboxMessage> {
        if mailbox.trim().is_empty() {
            return Err(MailError::Validation(
                "message mailbox must not be blank".to_owned(),
            ));
        }
        if uid == 0 {
            return Err(MailError::Validation(
                "message UID must be greater than zero".to_owned(),
            ));
        }
        self.cached_mailbox_message(mailbox, uid)
    }

    /// Resolves a selected semantic-mailbox row by its opaque SQLite identity.
    ///
    /// The concrete provider mailbox and UID stay inside Rust. This is the
    /// reader entry point for keyset-page items, including Archive and Trash,
    /// whose localized mailbox names must not cross into React.
    pub fn cached_message_by_id(&self, public_id: &str) -> Result<InboxMessage> {
        let message = self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)?;
        if !message.body_fetched {
            return Err(MailError::NotFound {
                entity: "cached message body",
                id: public_id.to_owned(),
            });
        }
        self.repair_cached_inline_images(message)
    }

    /// Indexes attachment metadata only from one completely cached RFC822
    /// message. The returned IDs are opaque digests bound to that exact MIME
    /// tree; neither MIME part numbers nor bytes cross this boundary.
    pub fn cached_message_attachments(&self, public_id: &str) -> Result<Vec<AttachmentMeta>> {
        let message = self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)?;
        if !message.body_fetched || message.raw_rfc822.is_empty() {
            return Err(MailError::NotFound {
                entity: "cached message MIME",
                id: public_id.to_owned(),
            });
        }
        index_message_attachments(&message.raw_rfc822, MimeSourceCompleteness::CompleteRfc822)
            .map(|attachments| {
                attachments
                    .into_iter()
                    .map(public_attachment_meta)
                    .collect()
            })
            .map_err(|_| MailError::Mime("cached attachment indexing failed".to_owned()))
    }

    /// Returns attachment metadata for one known message without downloading
    /// ordinary attachment bodies. A completely cached MIME uses the local
    /// authoritative index; otherwise the server BODYSTRUCTURE is normalized
    /// behind opaque IDs.
    pub async fn message_attachments(&self, public_id: &str) -> Result<Vec<AttachmentMeta>> {
        let cached = self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)?;
        if cached.body_fetched && !cached.raw_rfc822.is_empty() {
            return self.cached_message_attachments(public_id);
        }
        self.fetch_message_view_by_id(public_id, false)
            .await
            .map(|(_, attachments)| attachments)
    }

    /// Completes the Rust side of a platform Save As flow. `selected_destination`
    /// is supplied only by the desktop picker and is never serialized to
    /// React. The typed result exposes at most the final base file name.
    pub async fn save_message_attachment_to(
        &self,
        public_id: &str,
        attachment_id: &str,
        selected_destination: Option<&Path>,
    ) -> Result<AttachmentSaveResult> {
        let cached = self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)?;
        if !validate_attachment_id(attachment_id) && !is_remote_attachment_id(attachment_id) {
            return Err(MailError::Validation(
                "the attachment identifier is invalid".to_owned(),
            ));
        }
        let Some(selected_destination) = selected_destination else {
            return Ok(AttachmentSaveResult {
                status: AttachmentSaveStatus::Canceled,
                file_name: None,
                error_kind: None,
                retryable: false,
            });
        };

        let (safe_display_name, bytes) = if validate_attachment_id(attachment_id)
            && !cached.raw_rfc822.is_empty()
        {
            let metadata = match index_message_attachments(
                &cached.raw_rfc822,
                MimeSourceCompleteness::CompleteRfc822,
            ) {
                Ok(metadata) => metadata,
                Err(_) => {
                    return Ok(attachment_save_error(
                        AttachmentSaveErrorKind::MessageUnavailable,
                        true,
                    ));
                }
            };
            let Some(metadata) = metadata
                .into_iter()
                .find(|metadata| metadata.id == attachment_id)
            else {
                return Ok(attachment_save_error(
                    AttachmentSaveErrorKind::AttachmentNotFound,
                    false,
                ));
            };
            let bytes = match extract_attachment(&cached.raw_rfc822, attachment_id) {
                Ok(bytes) => bytes,
                Err(AttachmentIndexError::AttachmentNotFound)
                | Err(AttachmentIndexError::InvalidPartToken) => {
                    return Ok(attachment_save_error(
                        AttachmentSaveErrorKind::AttachmentNotFound,
                        false,
                    ));
                }
                Err(_) => {
                    return Ok(attachment_save_error(
                        AttachmentSaveErrorKind::MessageUnavailable,
                        true,
                    ));
                }
            };
            if bytes.len() as u64 != metadata.size_bytes {
                return Ok(attachment_save_error(
                    AttachmentSaveErrorKind::MessageUnavailable,
                    true,
                ));
            }
            (metadata.safe_display_name, bytes)
        } else if is_remote_attachment_id(attachment_id) {
            let mut body_imap = match self.selected_foreground_body_session(&cached.mailbox).await {
                Ok(session) => session,
                Err(_) => {
                    return Ok(attachment_save_error(
                        AttachmentSaveErrorKind::MessageUnavailable,
                        true,
                    ));
                }
            };
            let result = async {
                let session = body_imap
                    .as_mut()
                    .expect("foreground body IMAP session is connected before attachment fetch");
                let structure = session
                    .connection
                    .fetch_message_structure(cached.uid)
                    .await?;
                let listing = remote_attachment_listing(public_id, &structure);
                let Some((metadata, path)) = listing
                    .into_iter()
                    .find(|(metadata, _)| metadata.id == attachment_id)
                else {
                    return Err(MailError::NotFound {
                        entity: "remote attachment",
                        id: "opaque".to_owned(),
                    });
                };
                if metadata.disposition != AttachmentDisposition::Attachment {
                    return Err(MailError::Validation(
                        "inline MIME resources cannot be saved as ordinary attachments".to_owned(),
                    ));
                }
                let mut fetched = session
                    .connection
                    .fetch_message_parts(cached.uid, std::slice::from_ref(&path))
                    .await?;
                let fetched = fetched.pop().ok_or_else(|| MailError::NotFound {
                    entity: "remote attachment part",
                    id: "opaque".to_owned(),
                })?;
                let decoded = decode_remote_mime_part(&fetched.mime_header, &fetched.encoded_body)?;
                if !metadata.size_is_estimate
                    && decoded.contents.len() as u64 != metadata.size_bytes
                {
                    return Err(MailError::Mime(
                        "remote attachment size changed during download".to_owned(),
                    ));
                }
                Ok((metadata.safe_display_name, decoded.contents))
            }
            .await;
            match result {
                Ok(value) => {
                    if let Some(session) = body_imap.as_mut() {
                        session.last_used = Instant::now();
                    }
                    value
                }
                Err(MailError::NotFound { .. }) | Err(MailError::Validation(_)) => {
                    return Ok(attachment_save_error(
                        AttachmentSaveErrorKind::AttachmentNotFound,
                        false,
                    ));
                }
                Err(_) => {
                    *body_imap = None;
                    return Ok(attachment_save_error(
                        AttachmentSaveErrorKind::MessageUnavailable,
                        true,
                    ));
                }
            }
        } else {
            return Ok(attachment_save_error(
                AttachmentSaveErrorKind::AttachmentNotFound,
                false,
            ));
        };
        match save_extracted_file(selected_destination, &safe_display_name, &bytes) {
            Ok(file_name) => Ok(AttachmentSaveResult {
                status: AttachmentSaveStatus::Saved,
                file_name: Some(file_name),
                error_kind: None,
                retryable: false,
            }),
            Err(error) => Ok(attachment_save_error(
                attachment_save_io_error(&error),
                true,
            )),
        }
    }

    /// Records the user's read action in SQLite without waiting for IMAP. The
    /// pending row is retried by foreground marking and normal Inbox sync.
    pub fn mark_inbox_message_read(&self, uid: u32) -> Result<bool> {
        if uid == 0 {
            return Err(MailError::Validation(
                "message UID must be greater than zero".to_owned(),
            ));
        }
        self.repository
            .mark_message_seen_pending(&self.config.account_id, INBOX, uid)
    }

    /// Queues the desired read state entirely in SQLite and returns the durable
    /// operation identity used by the independent network worker.
    pub fn set_message_seen(
        &self,
        public_id: &str,
        desired: bool,
    ) -> Result<SystemFlagMutationReceipt> {
        self.queue_message_system_flag(public_id, SystemFlagKind::Seen, desired)
    }

    /// Queues a star/unstar intent by the same opaque local message identity
    /// used by semantic mailbox pages. React never supplies a provider mailbox
    /// name or UID.
    pub fn set_message_starred_by_id(
        &self,
        public_id: &str,
        desired: bool,
    ) -> Result<SystemFlagMutationReceipt> {
        self.queue_message_system_flag(public_id, SystemFlagKind::Flagged, desired)
    }

    fn queue_message_system_flag(
        &self,
        public_id: &str,
        flag: SystemFlagKind,
        desired: bool,
    ) -> Result<SystemFlagMutationReceipt> {
        let message = self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)?;
        let role = self
            .semantic_role_for_mailbox(&message.mailbox)?
            .ok_or_else(|| {
                MailError::Validation(
                    "the message mailbox has no available semantic role".to_owned(),
                )
            })?;
        if role == MailboxRole::Drafts {
            return Err(MailError::Validation(
                "read state is unavailable for Drafts".to_owned(),
            ));
        }
        self.ensure_role_available(role)?;
        self.repository
            .queue_system_flag_mutation(
                &self.config.account_id,
                &message.mailbox,
                message.uid,
                flag,
                desired,
            )
            .map(|(_, mutation)| SystemFlagMutationReceipt {
                operation_id: mutation.operation_id,
                local_revision: mutation.revision,
                status: mutation.status,
                source_role: mutation.source_role,
                flag: mutation.flag,
                desired: mutation.desired,
            })
    }

    /// Flushes pending read/unread intents for one available semantic mailbox.
    /// UIDVALIDITY is checked before any UID STORE and a mismatch retains the
    /// queue for the data-layer recovery migration.
    pub async fn flush_pending_seen_mutations(
        &self,
        account_id: &str,
        role: MailboxRole,
    ) -> Result<usize> {
        self.flush_pending_system_flag_mutations(account_id, role, SystemFlagKind::Seen)
            .await
    }

    pub async fn flush_pending_flagged_mutations(
        &self,
        account_id: &str,
        role: MailboxRole,
    ) -> Result<usize> {
        self.flush_pending_system_flag_mutations(account_id, role, SystemFlagKind::Flagged)
            .await
    }

    async fn flush_pending_system_flag_mutations(
        &self,
        account_id: &str,
        role: MailboxRole,
        flag: SystemFlagKind,
    ) -> Result<usize> {
        self.ensure_account_scope(account_id)?;
        if role == MailboxRole::Drafts {
            return Err(MailError::Validation(
                "system flags are unavailable for Drafts".to_owned(),
            ));
        }
        let mailbox = self
            .repository
            .mailbox_for_semantic_role(account_id, role)?;
        let _guard = self.general_imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config)
            .await
            .map_err(|_| privacy_safe_imap_error("system-flag synchronization"))?;
        let selected_uid_validity = match flag {
            SystemFlagKind::Seen => connection.select_mailbox_for_seen_update(&mailbox).await,
            SystemFlagKind::Flagged => connection.select_mailbox_for_flagged_update(&mailbox).await,
        }
        .map_err(|_| privacy_safe_imap_error("system-flag synchronization"))?;
        let local_uid_validity = self
            .repository
            .mailbox_state(account_id, &mailbox)?
            .and_then(|state| state.uid_validity);
        if classify_inbox_uid_scope(local_uid_validity, selected_uid_validity)
            != InboxUidScope::Current
        {
            let _ = connection.logout().await;
            return Err(MailError::Validation(
                "the mailbox epoch changed; synchronize before updating message state".to_owned(),
            ));
        }
        let result = match flag {
            SystemFlagKind::Seen => {
                self.flush_pending_seen_updates(&mut connection, &mailbox, selected_uid_validity)
                    .await
            }
            SystemFlagKind::Flagged => {
                self.flush_pending_flagged_updates(&mut connection, &mailbox, selected_uid_validity)
                    .await
            }
        };
        let _ = connection.logout().await;
        result
    }

    fn queue_remote_message_action(
        &self,
        message_id: i64,
        kind: MessageActionKind,
        destination_role: Option<MailboxRole>,
    ) -> Result<MessageMutationReceipt> {
        let message = self.repository.get_message(message_id)?;
        if message.account_id != self.config.account_id {
            return Err(MailError::NotFound {
                entity: "message",
                id: message_id.to_string(),
            });
        }
        let source_role = self
            .semantic_role_for_mailbox(&message.mailbox)?
            .ok_or_else(|| {
                MailError::Validation(
                    "the message mailbox has no available semantic role".to_owned(),
                )
            })?;
        let source_is_valid = match kind {
            MessageActionKind::Archive => {
                matches!(source_role, MailboxRole::Inbox | MailboxRole::Sent)
            }
            MessageActionKind::MoveToTrash => matches!(
                source_role,
                MailboxRole::Inbox | MailboxRole::Sent | MailboxRole::Archive
            ),
            MessageActionKind::PermanentDelete => source_role == MailboxRole::Trash,
        };
        if !source_is_valid {
            return Err(MailError::Validation(
                "the message action is unavailable in this mailbox".to_owned(),
            ));
        }
        self.ensure_role_available(source_role)?;
        if let Some(destination_role) = destination_role {
            let capability = self
                .repository
                .mailbox_capability(&self.config.account_id, destination_role)?;
            if capability
                .as_ref()
                .is_none_or(|capability| capability.status != MailboxCapabilityStatus::Available)
            {
                return Err(MailError::Validation(
                    "the destination mailbox capability is unavailable".to_owned(),
                ));
            }
        }
        self.repository.queue_message_action_for_account(
            &self.config.account_id,
            message_id,
            source_role,
            kind,
            destination_role,
        )
    }

    /// Persists the Archive overlay and returns without waiting for IMAP.
    pub fn archive_message(&self, public_id: &str) -> Result<MessageMutationReceipt> {
        let message = self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)?;
        self.queue_remote_message_action(
            message.id,
            MessageActionKind::Archive,
            Some(MailboxRole::Archive),
        )
    }

    /// Persists the Trash overlay and returns without waiting for IMAP.
    pub fn move_message_to_trash(&self, public_id: &str) -> Result<MessageMutationReceipt> {
        let message = self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)?;
        self.queue_remote_message_action(
            message.id,
            MessageActionKind::MoveToTrash,
            Some(MailboxRole::Trash),
        )
    }

    /// Creates a short-lived, single-use confirmation plan bound to one cached
    /// Trash message. No remote or local deletion is queued yet.
    pub async fn prepare_permanent_delete(&self, public_id: &str) -> Result<PermanentDeletePlan> {
        let message = self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)?;
        if self.semantic_role_for_mailbox(&message.mailbox)? != Some(MailboxRole::Trash) {
            return Err(MailError::NotFound {
                entity: "Trash message",
                id: public_id.to_owned(),
            });
        }
        self.ensure_role_available(MailboxRole::Trash)?;
        let expires_at = Utc::now() + TimeDelta::minutes(PERMANENT_DELETE_PLAN_MINUTES);
        let plan_id = Uuid::now_v7().to_string();
        let mut plans = self.permanent_delete_plans.lock().await;
        let now = Utc::now();
        plans.retain(|_, plan| plan.expires_at > now);
        if plans.len() >= MAX_PENDING_DELETE_PLANS {
            if let Some(oldest_id) = plans
                .iter()
                .min_by_key(|(_, plan)| plan.expires_at)
                .map(|(plan_id, _)| plan_id.clone())
            {
                plans.remove(&oldest_id);
            }
        }
        plans.insert(
            plan_id.clone(),
            PermanentDeletePlanState {
                message_id: message.id,
                expires_at,
            },
        );
        Ok(PermanentDeletePlan {
            plan_id,
            expires_at: expires_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        })
    }

    /// Consumes one confirmation plan and queues the exact permanent-delete
    /// intent. Network execution remains a separate scheduler operation.
    pub async fn confirm_permanent_delete(&self, plan_id: &str) -> Result<MessageMutationReceipt> {
        if plan_id.trim().is_empty() || plan_id.len() > 128 {
            return Err(MailError::Validation(
                "the permanent-delete plan is invalid".to_owned(),
            ));
        }
        let plan = self
            .permanent_delete_plans
            .lock()
            .await
            .remove(plan_id)
            .filter(|plan| plan.expires_at > Utc::now())
            .ok_or_else(|| MailError::NotFound {
                entity: "permanent-delete plan",
                id: "expired-or-consumed".to_owned(),
            })?;
        self.queue_remote_message_action(plan.message_id, MessageActionKind::PermanentDelete, None)
    }

    /// Reconciles interrupted actions and then executes only freshly claimed
    /// account-scoped intents. Every mutating IMAP command is preceded by a
    /// persisted phase transition; an uncertain transfer is never sent again.
    pub async fn flush_pending_message_mutations(&self, account_id: &str) -> Result<usize> {
        self.ensure_account_scope(account_id)?;
        let recoverable = self
            .repository
            .message_actions_requiring_reconciliation(account_id)?;
        let pending = self.repository.pending_message_actions(account_id)?;
        let cleanup_tombstones = self
            .repository
            .confirmed_source_cleanup_tombstones(account_id)?;
        if recoverable.is_empty() && pending.is_empty() && cleanup_tombstones.is_empty() {
            return Ok(0);
        }

        let _guard = self.general_imap_gate.lock().await;
        let mut connection = ImapConnection::connect(&self.config)
            .await
            .map_err(|_| privacy_safe_imap_error("message mutation synchronization"))?;
        let result = async {
            let mut confirmed = 0;
            for action in recoverable {
                if self
                    .reconcile_remote_message_action(&mut connection, &action)
                    .await?
                {
                    confirmed += 1;
                }
            }

            // Reconciliation may safely requeue an action whose durable phase
            // proves that no mutating command had started.
            for action in self.repository.pending_message_actions(account_id)? {
                if self
                    .execute_pending_message_action(&mut connection, &action)
                    .await?
                {
                    confirmed += 1;
                }
            }

            // A confirmed COPY fallback on a server without UIDPLUS can leave
            // only the exact source UID marked `\Deleted`. These rows are
            // tombstones, never retryable transfer work. Retire them only after
            // the destination has converged and a same-epoch FLAGS lookup
            // proves the source UID no longer exists.
            for action in self
                .repository
                .confirmed_source_cleanup_tombstones(account_id)?
            {
                self.reconcile_confirmed_source_cleanup(&mut connection, &action)
                    .await?;
            }
            Ok(confirmed)
        }
        .await;
        let _ = connection.logout().await;
        result
            .map_err(|error| privacy_safe_network_error(error, "message mutation synchronization"))
    }

    async fn execute_pending_message_action(
        &self,
        connection: &mut ImapConnection,
        action: &PendingMessageAction,
    ) -> Result<bool> {
        let destination = match action
            .destination_role
            .map(|role| {
                self.repository
                    .mailbox_for_semantic_role(&action.account_id, role)
            })
            .transpose()
        {
            Ok(destination) => destination,
            Err(error) => {
                let Some(claimed) = self.repository.claim_message_action(
                    &action.account_id,
                    &action.operation_id,
                    action.revision,
                )?
                else {
                    return Ok(false);
                };
                self.repository.finalize_message_action(
                    &claimed.account_id,
                    &claimed.operation_id,
                    claimed.revision,
                    MutationStatus::NeedsAttention,
                    Some(MessageMutationErrorKind::MailboxUnavailable),
                )?;
                return match error {
                    MailError::NotFound { .. } | MailError::Validation(_) => Ok(false),
                    other => Err(other),
                };
            }
        };

        let selected_uid_validity = self
            .select_source_for_message_action(connection, action)
            .await?;
        if selected_uid_validity != Some(action.source_uid_validity) {
            if let Some(claimed) = self.repository.claim_message_action(
                &action.account_id,
                &action.operation_id,
                action.revision,
            )? {
                self.repository.finalize_message_action(
                    &claimed.account_id,
                    &claimed.operation_id,
                    claimed.revision,
                    MutationStatus::NeedsAttention,
                    Some(MessageMutationErrorKind::UidValidityChanged),
                )?;
            }
            return Ok(false);
        }
        let source_flags = connection.fetch_flags(&[action.source_uid]).await?;
        if source_flags
            .iter()
            .all(|(remote_uid, _)| *remote_uid != action.source_uid)
        {
            if let Some(claimed) = self.repository.claim_message_action(
                &action.account_id,
                &action.operation_id,
                action.revision,
            )? {
                self.repository.finalize_message_action(
                    &claimed.account_id,
                    &claimed.operation_id,
                    claimed.revision,
                    MutationStatus::NeedsAttention,
                    Some(MessageMutationErrorKind::SourceMissing),
                )?;
            }
            return Ok(false);
        }

        let Some(claimed) = self.repository.claim_message_action(
            &action.account_id,
            &action.operation_id,
            action.revision,
        )?
        else {
            return Ok(false);
        };
        match persisted_phase_work(claimed.kind, claimed.status, claimed.remote_phase) {
            PersistedPhaseWork::Transfer => {
                let destination = destination.as_deref().ok_or_else(|| {
                    MailError::Validation(
                        "a transfer action has no available destination mailbox".to_owned(),
                    )
                })?;
                self.execute_claimed_transfer(connection, &claimed, destination)
                    .await
            }
            PersistedPhaseWork::SourceDelete => {
                self.execute_claimed_source_delete(
                    connection,
                    &claimed,
                    RemoteMutationPhase::Queued,
                    None,
                )
                .await
            }
            // This state should normally be recovered through the
            // reconciliation query. Conservatively retain a cleanup tombstone
            // if an older database exposes it as fresh pending work.
            PersistedPhaseWork::Finalize => self.confirm_claimed_message_action(&claimed, true),
            PersistedPhaseWork::Reconcile | PersistedPhaseWork::Done => Ok(false),
            PersistedPhaseWork::Stop { status, error_kind } => {
                self.repository.finalize_message_action(
                    &claimed.account_id,
                    &claimed.operation_id,
                    claimed.revision,
                    status,
                    Some(error_kind),
                )?;
                Ok(false)
            }
        }
    }

    async fn select_source_for_message_action(
        &self,
        connection: &mut ImapConnection,
        action: &PendingMessageAction,
    ) -> Result<Option<u32>> {
        let requires_deleted_flag = action.kind == MessageActionKind::PermanentDelete
            || connection.message_move_method() == MessageMoveMethod::UidCopyThenDelete
            || matches!(
                action.remote_phase,
                RemoteMutationPhase::TransferAcknowledged
                    | RemoteMutationPhase::SourceDeleteStarted
            );
        if requires_deleted_flag {
            connection
                .select_mailbox_for_deleted_update(&action.source_mailbox)
                .await
        } else {
            connection
                .select_mailbox_for_history(&action.source_mailbox)
                .await
                .map(|selected| selected.uid_validity)
        }
    }

    async fn execute_claimed_transfer(
        &self,
        connection: &mut ImapConnection,
        action: &PendingMessageAction,
        destination: &str,
    ) -> Result<bool> {
        if !self.repository.advance_message_action_remote_phase(
            &action.account_id,
            &action.operation_id,
            action.revision,
            RemoteMutationPhase::Queued,
            RemoteMutationPhase::TransferStarted,
        )? {
            return Ok(false);
        }

        let transfer = match connection.message_move_method() {
            MessageMoveMethod::UidMove => {
                connection
                    .move_uids(&[action.source_uid], destination)
                    .await
            }
            MessageMoveMethod::UidCopyThenDelete => {
                connection
                    .copy_uids(&[action.source_uid], destination)
                    .await
            }
        };
        if let Err(error) = transfer {
            return self.stop_claimed_after_remote_error(action, &error);
        }

        match connection.message_move_method() {
            MessageMoveMethod::UidMove => {
                if !self.repository.advance_message_action_remote_phase(
                    &action.account_id,
                    &action.operation_id,
                    action.revision,
                    RemoteMutationPhase::TransferStarted,
                    RemoteMutationPhase::SourceDeleteAcknowledged,
                )? {
                    return Ok(false);
                }
                let confirmed = self.confirm_claimed_message_action(action, false)?;
                if confirmed {
                    self.converge_confirmed_move(connection, action, destination)
                        .await?;
                }
                Ok(confirmed)
            }
            MessageMoveMethod::UidCopyThenDelete => {
                if !self.repository.advance_message_action_remote_phase(
                    &action.account_id,
                    &action.operation_id,
                    action.revision,
                    RemoteMutationPhase::TransferStarted,
                    RemoteMutationPhase::TransferAcknowledged,
                )? {
                    return Ok(false);
                }
                self.execute_claimed_source_delete(
                    connection,
                    action,
                    RemoteMutationPhase::TransferAcknowledged,
                    Some(destination),
                )
                .await
            }
        }
    }

    async fn execute_claimed_source_delete(
        &self,
        connection: &mut ImapConnection,
        action: &PendingMessageAction,
        expected_phase: RemoteMutationPhase,
        convergence_destination: Option<&str>,
    ) -> Result<bool> {
        if !self.repository.advance_message_action_remote_phase(
            &action.account_id,
            &action.operation_id,
            action.revision,
            expected_phase,
            RemoteMutationPhase::SourceDeleteStarted,
        )? {
            return Ok(false);
        }
        let deletion = match self
            .delete_exact_source_uid(connection, action.source_uid)
            .await
        {
            Ok(deletion) => deletion,
            Err(error) => return self.stop_claimed_after_remote_error(action, &error),
        };
        if !self.repository.advance_message_action_remote_phase(
            &action.account_id,
            &action.operation_id,
            action.revision,
            RemoteMutationPhase::SourceDeleteStarted,
            RemoteMutationPhase::SourceDeleteAcknowledged,
        )? {
            return Ok(false);
        }
        let source_cleanup_pending = deletion == ExactSourceDeleteOutcome::DeferredServerCleanup;
        let confirmed = self.confirm_claimed_message_action(action, source_cleanup_pending)?;
        if confirmed {
            if let Some(destination) = convergence_destination {
                // A COPY fallback may have to leave the exact source UID marked
                // `\Deleted` on servers without UIDPLUS. Destination
                // reconciliation is still safe and necessary: the strong
                // identity check can retire the local move projection without
                // issuing a global EXPUNGE.
                self.converge_confirmed_move(connection, action, destination)
                    .await?;
            } else if action.kind == MessageActionKind::PermanentDelete
                && deletion == ExactSourceDeleteOutcome::Removed
            {
                self.repository
                    .purge_confirmed_message_action_after_convergence(
                        &action.account_id,
                        &action.operation_id,
                        action.revision,
                    )?;
            }
        }
        Ok(confirmed)
    }

    async fn delete_exact_source_uid(
        &self,
        connection: &mut ImapConnection,
        source_uid: u32,
    ) -> Result<ExactSourceDeleteOutcome> {
        connection.mark_deleted_flags(&[source_uid]).await?;
        if connection.delete_finalization() == DeleteFinalization::UidExpunge {
            connection.expunge_deleted_uids(&[source_uid]).await?;
            if connection
                .fetch_flags(&[source_uid])
                .await?
                .iter()
                .any(|(remote_uid, _)| *remote_uid == source_uid)
            {
                return Err(MailError::Imap(
                    "UID-scoped deletion was not confirmed".to_owned(),
                ));
            }
            Ok(ExactSourceDeleteOutcome::Removed)
        } else {
            Ok(ExactSourceDeleteOutcome::DeferredServerCleanup)
        }
    }

    async fn converge_confirmed_move(
        &self,
        connection: &mut ImapConnection,
        action: &PendingMessageAction,
        destination: &str,
    ) -> Result<()> {
        match self
            .sync_selected_mailbox_with_scope(
                connection,
                destination,
                DEFAULT_MESSAGE_PAGE_SIZE,
                action
                    .destination_role
                    .map(|role| self.mailbox_message_scope(role))
                    .unwrap_or_default(),
                &mut |_| {},
            )
            .await
        {
            Ok(_) => {
                self.repository
                    .purge_confirmed_message_action_if_destination_unique(
                        &action.account_id,
                        &action.operation_id,
                    )?;
                Ok(())
            }
            Err(MailError::Imap(_) | MailError::Timeout { .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn confirm_claimed_message_action(
        &self,
        action: &PendingMessageAction,
        source_cleanup_pending: bool,
    ) -> Result<bool> {
        self.repository.finalize_message_action_confirmed(
            &action.account_id,
            &action.operation_id,
            action.revision,
            source_cleanup_pending,
        )
    }

    async fn reconcile_confirmed_source_cleanup(
        &self,
        connection: &mut ImapConnection,
        action: &PendingMessageAction,
    ) -> Result<bool> {
        if action.status != MutationStatus::Confirmed || !action.source_cleanup_pending {
            return Ok(false);
        }

        if let Some(destination_role) = action.destination_role {
            let Ok(destination) = self
                .repository
                .mailbox_for_semantic_role(&action.account_id, destination_role)
            else {
                return Ok(false);
            };
            self.converge_confirmed_move(connection, action, &destination)
                .await?;
        }

        let selected = connection
            .select_mailbox_for_history(&action.source_mailbox)
            .await?;
        let Some(selected_uid_validity) = selected.uid_validity else {
            return Ok(false);
        };
        if selected_uid_validity != action.source_uid_validity {
            return Ok(false);
        }
        let source_exists = connection
            .fetch_flags(&[action.source_uid])
            .await?
            .iter()
            .any(|(remote_uid, _)| *remote_uid == action.source_uid);
        if source_exists {
            return Ok(false);
        }

        self.repository
            .purge_confirmed_source_cleanup_if_remote_absent(
                &action.account_id,
                &action.operation_id,
                action.revision,
                selected_uid_validity,
                action.source_uid,
            )
    }

    fn stop_claimed_after_remote_error(
        &self,
        action: &PendingMessageAction,
        error: &MailError,
    ) -> Result<bool> {
        self.repository.finalize_message_action(
            &action.account_id,
            &action.operation_id,
            action.revision,
            MutationStatus::OutcomeUnknown,
            Some(message_mutation_error_kind(error)),
        )?;
        Ok(false)
    }

    async fn reconcile_remote_message_action(
        &self,
        connection: &mut ImapConnection,
        action: &PendingMessageAction,
    ) -> Result<bool> {
        if action.remote_phase == RemoteMutationPhase::SourceDeleteAcknowledged {
            let selected = connection
                .select_mailbox_for_history(&action.source_mailbox)
                .await?;
            if selected.uid_validity != Some(action.source_uid_validity) {
                self.repository.reconcile_message_action(
                    &action.account_id,
                    &action.operation_id,
                    action.revision,
                    MutationStatus::NeedsAttention,
                    Some(MessageMutationErrorKind::UidValidityChanged),
                )?;
                return Ok(false);
            }
            let source_flags = connection.fetch_flags(&[action.source_uid]).await?;
            let source = source_flags
                .iter()
                .find(|(remote_uid, _)| *remote_uid == action.source_uid);
            if source.is_some_and(|(_, flags)| {
                !flags
                    .iter()
                    .any(|flag| flag.eq_ignore_ascii_case("\\Deleted"))
            }) {
                self.repository.reconcile_message_action(
                    &action.account_id,
                    &action.operation_id,
                    action.revision,
                    MutationStatus::NeedsAttention,
                    Some(MessageMutationErrorKind::AmbiguousRemoteState),
                )?;
                return Ok(false);
            }
            let source_cleanup_pending = source.is_some();
            let confirmed = self.repository.reconcile_message_action_confirmed(
                &action.account_id,
                &action.operation_id,
                action.revision,
                source_cleanup_pending,
            )?;
            if confirmed {
                if let Some(destination_role) = action.destination_role {
                    if let Ok(destination) = self
                        .repository
                        .mailbox_for_semantic_role(&action.account_id, destination_role)
                    {
                        self.converge_confirmed_move(connection, action, &destination)
                            .await?;
                    }
                } else if !source_cleanup_pending {
                    self.repository
                        .purge_confirmed_message_action_after_convergence(
                            &action.account_id,
                            &action.operation_id,
                            action.revision,
                        )?;
                }
            }
            return Ok(confirmed);
        }

        let selected_uid_validity = self
            .select_source_for_message_action(connection, action)
            .await?;
        if selected_uid_validity != Some(action.source_uid_validity) {
            self.repository.reconcile_message_action(
                &action.account_id,
                &action.operation_id,
                action.revision,
                MutationStatus::NeedsAttention,
                Some(MessageMutationErrorKind::UidValidityChanged),
            )?;
            return Ok(false);
        }
        let source_exists = connection
            .fetch_flags(&[action.source_uid])
            .await?
            .iter()
            .any(|(remote_uid, _)| *remote_uid == action.source_uid);

        match action.remote_phase {
            RemoteMutationPhase::Queued if source_exists => {
                self.repository.reconcile_message_action(
                    &action.account_id,
                    &action.operation_id,
                    action.revision,
                    MutationStatus::Pending,
                    None,
                )?;
                Ok(false)
            }
            RemoteMutationPhase::Queued if action.kind == MessageActionKind::PermanentDelete => {
                let confirmed = self.repository.reconcile_message_action_confirmed(
                    &action.account_id,
                    &action.operation_id,
                    action.revision,
                    false,
                )?;
                if confirmed {
                    self.repository
                        .purge_confirmed_message_action_after_convergence(
                            &action.account_id,
                            &action.operation_id,
                            action.revision,
                        )?;
                }
                Ok(confirmed)
            }
            RemoteMutationPhase::TransferAcknowledged
            | RemoteMutationPhase::SourceDeleteStarted => {
                let deletion = if source_exists {
                    match self
                        .delete_exact_source_uid(connection, action.source_uid)
                        .await
                    {
                        Ok(outcome) => outcome,
                        Err(_) => return Ok(false),
                    }
                } else {
                    ExactSourceDeleteOutcome::Removed
                };
                let source_cleanup_pending =
                    deletion == ExactSourceDeleteOutcome::DeferredServerCleanup;
                let confirmed = self.repository.reconcile_message_action_confirmed(
                    &action.account_id,
                    &action.operation_id,
                    action.revision,
                    source_cleanup_pending,
                )?;
                if confirmed {
                    if let Some(destination_role) = action.destination_role {
                        if let Ok(destination) = self
                            .repository
                            .mailbox_for_semantic_role(&action.account_id, destination_role)
                        {
                            self.converge_confirmed_move(connection, action, &destination)
                                .await?;
                        }
                    } else if action.kind == MessageActionKind::PermanentDelete
                        && !source_cleanup_pending
                    {
                        self.repository
                            .purge_confirmed_message_action_after_convergence(
                                &action.account_id,
                                &action.operation_id,
                                action.revision,
                            )?;
                    }
                }
                Ok(confirmed)
            }
            RemoteMutationPhase::TransferStarted => {
                // The existing IMAP adapter cannot authoritatively search the
                // destination by the complete strong identity triple. Neither
                // a present nor missing source is permission to repeat MOVE or
                // COPY, so this action remains explicitly uncertain.
                self.repository.reconcile_message_action(
                    &action.account_id,
                    &action.operation_id,
                    action.revision,
                    MutationStatus::OutcomeUnknown,
                    Some(MessageMutationErrorKind::AmbiguousRemoteState),
                )?;
                Ok(false)
            }
            RemoteMutationPhase::Queued => {
                self.repository.reconcile_message_action(
                    &action.account_id,
                    &action.operation_id,
                    action.revision,
                    MutationStatus::NeedsAttention,
                    Some(MessageMutationErrorKind::SourceMissing),
                )?;
                Ok(false)
            }
            RemoteMutationPhase::SourceDeleteAcknowledged => unreachable!(),
        }
    }

    /// Optimistically stars or unstars one cached remote message. The exact
    /// mailbox and UID are retained so Inbox and provider Sent folders can be
    /// synchronized without relying on localized mailbox names.
    pub fn set_message_starred(&self, mailbox: &str, uid: u32, starred: bool) -> Result<bool> {
        if mailbox.trim().is_empty() {
            return Err(MailError::Validation(
                "message mailbox must not be blank".to_owned(),
            ));
        }
        if uid == 0 {
            return Err(MailError::Validation(
                "message UID must be greater than zero".to_owned(),
            ));
        }
        self.repository
            .set_message_flagged_pending(&self.config.account_id, mailbox, uid, starred)
    }

    /// Pushes every pending Inbox read action through UID STORE and clears a
    /// write-behind row only after a FLAGS fetch confirms `\Seen` persisted.
    pub async fn sync_pending_inbox_read_flags(&self) -> Result<usize> {
        self.flush_pending_seen_mutations(&self.config.account_id, MailboxRole::Inbox)
            .await
    }

    /// Pushes pending star/unstar actions for one cached mailbox. Server
    /// PERMANENTFLAGS and UIDVALIDITY are checked before UID STORE, and each
    /// result is removed only after the requested state is fetched back.
    pub async fn sync_pending_message_star_flags(&self, mailbox: &str) -> Result<usize> {
        if mailbox.trim().is_empty() {
            return Err(MailError::Validation(
                "message mailbox must not be blank".to_owned(),
            ));
        }
        if mailbox.eq_ignore_ascii_case(INBOX) {
            let _guard = self.inbox_imap_gate.lock().await;
            self.sync_pending_message_star_flags_locked(mailbox).await
        } else {
            let _guard = self.sent_imap_gate.lock().await;
            self.sync_pending_message_star_flags_locked(mailbox).await
        }
    }

    async fn sync_pending_message_star_flags_locked(&self, mailbox: &str) -> Result<usize> {
        let mut connection = ImapConnection::connect(&self.config).await?;
        let selected_uid_validity = connection
            .select_mailbox_for_flagged_update(mailbox)
            .await?;
        let local_uid_validity = self
            .repository
            .mailbox_state(&self.config.account_id, mailbox)?
            .and_then(|state| state.uid_validity);
        match classify_inbox_uid_scope(local_uid_validity, selected_uid_validity) {
            InboxUidScope::Current => {}
            InboxUidScope::NeedsSync => {
                return Err(MailError::Validation(
                    "Mailbox must be synchronized before updating message flags".to_owned(),
                ));
            }
            InboxUidScope::Changed => {
                self.repository
                    .reset_mailbox(&self.config.account_id, mailbox)?;
                return Err(MailError::Validation(
                    "Mailbox UIDVALIDITY changed; synchronize before updating message flags"
                        .to_owned(),
                ));
            }
        }
        let result = self
            .flush_pending_flagged_updates(&mut connection, mailbox, selected_uid_validity)
            .await;
        let _ = connection.logout().await;
        result
    }

    async fn flush_pending_seen_updates(
        &self,
        connection: &mut ImapConnection,
        mailbox: &str,
        selected_uid_validity: Option<u32>,
    ) -> Result<usize> {
        self.flush_pending_system_flag_updates(
            connection,
            mailbox,
            selected_uid_validity,
            SystemFlagKind::Seen,
        )
        .await
    }

    async fn flush_pending_flagged_updates(
        &self,
        connection: &mut ImapConnection,
        mailbox: &str,
        selected_uid_validity: Option<u32>,
    ) -> Result<usize> {
        self.flush_pending_system_flag_updates(
            connection,
            mailbox,
            selected_uid_validity,
            SystemFlagKind::Flagged,
        )
        .await
    }

    async fn flush_pending_system_flag_updates(
        &self,
        connection: &mut ImapConnection,
        mailbox: &str,
        selected_uid_validity: Option<u32>,
        flag: SystemFlagKind,
    ) -> Result<usize> {
        let Some(selected_uid_validity) = selected_uid_validity else {
            return Ok(0);
        };
        let mut completed = 0;
        for mutation in self
            .repository
            .system_flag_mutations_requiring_reconciliation(
                &self.config.account_id,
                mailbox,
                flag,
            )?
        {
            if persisted_flag_work(mutation.status) != PersistedFlagWork::Reconcile
                || mutation.source_uid_validity != selected_uid_validity
            {
                continue;
            }
            let Some((_, server_flags)) = connection
                .fetch_flags(&[mutation.source_uid])
                .await?
                .into_iter()
                .find(|(uid, _)| *uid == mutation.source_uid)
            else {
                continue;
            };
            let server_matches = system_flag_is_set(&server_flags, flag) == mutation.desired;
            if server_matches {
                if self.repository.reconcile_system_flag_mutation_confirmed(
                    &mutation.account_id,
                    &mutation.operation_id,
                    flag,
                    mutation.revision,
                    &server_flags,
                )? {
                    completed += 1;
                }
            } else {
                self.repository
                    .requeue_system_flag_mutation_after_reconcile(
                        &mutation.account_id,
                        &mutation.operation_id,
                        flag,
                        mutation.revision,
                    )?;
            }
        }

        for mutation in
            self.repository
                .pending_system_flag_mutations(&self.config.account_id, mailbox, flag)?
        {
            if persisted_flag_work(mutation.status) != PersistedFlagWork::Execute {
                continue;
            }
            let current_flags = connection
                .fetch_flags(&[mutation.source_uid])
                .await?
                .into_iter()
                .find(|(uid, _)| *uid == mutation.source_uid);
            let Some(claimed) = self.repository.claim_system_flag_mutation(
                &mutation.account_id,
                &mutation.operation_id,
                flag,
                mutation.revision,
            )?
            else {
                continue;
            };
            if claimed.source_uid_validity != selected_uid_validity {
                self.repository.finalize_system_flag_mutation_failure(
                    &claimed.account_id,
                    &claimed.operation_id,
                    flag,
                    claimed.revision,
                    MutationStatus::NeedsAttention,
                    MessageMutationErrorKind::UidValidityChanged,
                )?;
                continue;
            }
            let Some((_, current_flags)) = current_flags else {
                self.repository.finalize_system_flag_mutation_failure(
                    &claimed.account_id,
                    &claimed.operation_id,
                    flag,
                    claimed.revision,
                    MutationStatus::NeedsAttention,
                    MessageMutationErrorKind::SourceMissing,
                )?;
                continue;
            };
            if system_flag_is_set(&current_flags, flag) == claimed.desired {
                if self.repository.finalize_system_flag_mutation_confirmed(
                    &claimed.account_id,
                    &claimed.operation_id,
                    flag,
                    claimed.revision,
                    &current_flags,
                )? {
                    completed += 1;
                }
                continue;
            }

            let remote_result = match flag {
                SystemFlagKind::Seen => {
                    connection
                        .set_seen_flags(&[claimed.source_uid], claimed.desired)
                        .await
                }
                SystemFlagKind::Flagged => {
                    connection
                        .set_flagged_flags(&[claimed.source_uid], claimed.desired)
                        .await
                }
            };
            let confirmed = match remote_result {
                Ok(confirmed) => confirmed,
                Err(error) => {
                    self.repository.finalize_system_flag_mutation_failure(
                        &claimed.account_id,
                        &claimed.operation_id,
                        flag,
                        claimed.revision,
                        MutationStatus::OutcomeUnknown,
                        message_mutation_error_kind(&error),
                    )?;
                    continue;
                }
            };
            let Some((_, server_flags)) = confirmed
                .into_iter()
                .find(|(uid, _)| *uid == claimed.source_uid)
            else {
                self.repository.finalize_system_flag_mutation_failure(
                    &claimed.account_id,
                    &claimed.operation_id,
                    flag,
                    claimed.revision,
                    MutationStatus::OutcomeUnknown,
                    MessageMutationErrorKind::Unknown,
                )?;
                continue;
            };
            if self.repository.finalize_system_flag_mutation_confirmed(
                &claimed.account_id,
                &claimed.operation_id,
                flag,
                claimed.revision,
                &server_flags,
            )? {
                completed += 1;
            }
        }
        Ok(completed)
    }

    /// Resolve reply ancestors from SQLite without opening IMAP. Slots remain
    /// ordered from the direct parent to the oldest referenced message, and a
    /// missing cache entry remains `None` so deeper quote metadata cannot be
    /// applied to the wrong quote level.
    pub fn cached_reply_ancestors(
        &self,
        message: &InboxMessage,
    ) -> Result<Vec<Option<InboxMessage>>> {
        let current_message_id = message.message_id.as_deref().map(normalized_message_id_key);
        let mut seen = HashSet::new();
        let mut ancestors = Vec::new();
        for message_id in message
            .in_reply_to
            .iter()
            .chain(message.references.iter().rev())
        {
            let key = normalized_message_id_key(message_id);
            if key.is_empty()
                || current_message_id
                    .as_ref()
                    .is_some_and(|current| current == &key)
                || !seen.insert(key)
            {
                continue;
            }
            let ancestor = self
                .repository
                .find_message_by_message_id(&self.config.account_id, message_id)?
                .filter(|ancestor| ancestor.id != message.id);
            ancestors.push(ancestor);
        }

        if ancestors.is_empty() {
            if let Some(parent) = self.cached_legacy_reply_parent(message)? {
                ancestors.push(Some(parent));
            }
        } else if ancestors[0].is_none() {
            ancestors[0] = self.cached_legacy_reply_parent(message)?;
        }
        Ok(ancestors)
    }

    /// Resolve the nearest cached reply ancestor for legacy callers.
    pub fn cached_reply_parent(&self, message: &InboxMessage) -> Result<Option<InboxMessage>> {
        Ok(self
            .cached_reply_ancestors(message)?
            .into_iter()
            .flatten()
            .next())
    }

    fn cached_legacy_reply_parent(&self, message: &InboxMessage) -> Result<Option<InboxMessage>> {
        let Some(quoted) = legacy_mine_mail_quoted_text(message.body_text.as_deref()) else {
            return Ok(None);
        };
        let Some(current_sender) = message.sender.as_ref() else {
            return Ok(None);
        };
        let current_recipients = message
            .to
            .iter()
            .chain(&message.cc)
            .map(|address| address.email.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let quoted = normalize_legacy_reply_text(quoted);
        let subject = normalized_reply_subject(&message.subject);
        let candidates = self.repository.legacy_reply_parent_candidates(
            &self.config.account_id,
            message.id,
            250,
        )?;
        let mut matches = candidates
            .into_iter()
            .filter(|candidate| normalized_reply_subject(&candidate.subject) == subject)
            .filter(|candidate| {
                let Some(sender) = candidate.sender.as_ref() else {
                    return false;
                };
                current_recipients.contains(&sender.email.to_ascii_lowercase())
                    && candidate.to.iter().chain(&candidate.cc).any(|recipient| {
                        recipient.email.eq_ignore_ascii_case(&current_sender.email)
                    })
            })
            .filter(|candidate| {
                candidate
                    .body_text
                    .as_deref()
                    .is_some_and(|body| normalize_legacy_reply_text(body) == quoted)
            })
            .collect::<Vec<_>>();
        matches.dedup_by(|left, right| {
            left.message_id
                .as_deref()
                .zip(right.message_id.as_deref())
                .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
        });
        Ok((matches.len() == 1).then(|| matches.remove(0)))
    }

    /// Creates a reply request from one fully cached local message. React gets
    /// only the narrow editable request and immutable quote metadata; raw
    /// RFC822 never crosses the desktop boundary.
    pub fn prepare_reply(&self, public_id: &str) -> Result<ComposeRequest> {
        let message = self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)?;
        if !message.body_fetched {
            return Err(MailError::Validation(
                "wait for the complete message body before replying".to_owned(),
            ));
        }
        let quoted_text = message
            .body_text
            .clone()
            .filter(|body| !body.trim().is_empty())
            .ok_or_else(|| {
                MailError::Validation("this message has no readable text to quote".to_owned())
            })?;
        if quoted_text.len() > MAX_REPLY_QUOTED_TEXT_BYTES {
            return Err(MailError::Validation(
                "this message is too large to include as quoted reply text".to_owned(),
            ));
        }
        let quoted_html = message
            .body_html
            .clone()
            .filter(|html| !html.trim().is_empty() && html.len() <= MAX_REPLY_QUOTED_HTML_BYTES);

        let authored_by_account = message
            .sender
            .as_ref()
            .is_some_and(|sender| sender.email.eq_ignore_ascii_case(&self.config.email));
        let reply_target = if authored_by_account {
            message
                .to
                .iter()
                .find(|address| !address.email.eq_ignore_ascii_case(&self.config.email))
        } else {
            message.sender.as_ref()
        }
        .ok_or_else(|| MailError::Validation("this message has no reply recipient".to_owned()))?;

        let references = if message.references.is_empty() {
            message.in_reply_to.clone()
        } else {
            message.references.clone()
        };
        let subject = reply_subject(&message.subject);
        Ok(ComposeRequest {
            to: vec![reply_target.email.clone()],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject,
            body_text: String::new(),
            format: Default::default(),
            reply_context: Some(ReplyContext {
                parent_message_id: message.message_id.clone(),
                references,
                subject: message.subject,
                sender: message.sender,
                recipients: message.to,
                sent_at: message.sent_at.or(message.internal_date),
                quoted_text,
                quoted_html,
            }),
        })
    }

    /// Prepares a forward from one fully hydrated, immutable source snapshot.
    /// Expected hydration/extraction/staging failures stay typed; no list
    /// preview or partially staged attachment set is ever returned.
    pub async fn prepare_forward(
        &self,
        public_id: &str,
        include_attachments: bool,
    ) -> Result<ForwardPreparationOutcome> {
        let cached = match self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)
        {
            Ok(message) => message,
            Err(MailError::NotFound { .. }) => {
                return Ok(forward_error(
                    ForwardPreparationErrorKind::MessageUnavailable,
                    Vec::new(),
                    false,
                ));
            }
            Err(error) => return Err(error),
        };
        let message = if cached.body_fetched && !cached.raw_rfc822.is_empty() {
            cached
        } else {
            match self.fetch_message_by_id(public_id, false).await {
                Ok(message) if !message.raw_rfc822.is_empty() => message,
                Ok(_) | Err(_) => {
                    return Ok(forward_error(
                        ForwardPreparationErrorKind::BodyUnavailable,
                        Vec::new(),
                        false,
                    ));
                }
            }
        };
        let source_result = if include_attachments {
            prepare_forward_source(&message.raw_rfc822, MimeSourceCompleteness::CompleteRfc822)
        } else {
            prepare_forward_source_without_attachments(
                &message.raw_rfc822,
                MimeSourceCompleteness::CompleteRfc822,
            )
        };
        let source = match source_result {
            Ok(source) => source,
            Err(ForwardSourceError::AttachmentIndex(_)) => {
                let body_only_retry_succeeds = include_attachments
                    && prepare_forward_source_without_attachments(
                        &message.raw_rfc822,
                        MimeSourceCompleteness::CompleteRfc822,
                    )
                    .is_ok();
                return Ok(forward_error(
                    ForwardPreparationErrorKind::AttachmentUnavailable,
                    Vec::new(),
                    body_only_retry_succeeds,
                ));
            }
            Err(ForwardSourceError::NonAuthoritativeSource)
            | Err(ForwardSourceError::MessageCouldNotBeParsed)
            | Err(ForwardSourceError::BodyTooLarge)
            | Err(ForwardSourceError::HeaderMetadataTooLarge) => {
                return Ok(forward_error(
                    ForwardPreparationErrorKind::BodyUnavailable,
                    Vec::new(),
                    false,
                ));
            }
        };

        let source_attachments = source
            .ordinary_attachments
            .iter()
            .cloned()
            .map(public_attachment_meta)
            .collect::<Vec<_>>();
        let context = ForwardContext {
            source_message_id: public_id.to_owned(),
            original_subject: source.original_subject.clone(),
            from: source.from,
            to: source.to,
            cc: source.cc,
            sent_at: source.sent_at,
            quoted_text: source.quoted_text,
            quoted_html: source.quoted_html,
            quoted_render_mode: source.quoted_render_mode.map(|mode| match mode {
                ForwardHtmlRenderMode::NativeSemanticHtml => ForwardQuotedRenderMode::NativeHtml,
            }),
            source_attachments,
        };
        let mut warnings = Vec::new();
        if source.html_downgraded {
            warnings.push(ForwardWarning::HtmlDowngraded);
        }
        if source.has_inline_resources {
            warnings.push(ForwardWarning::InlineResourcesNotForwarded);
        }

        let mut pending = if include_attachments {
            match self.stage_forward_attachments(
                &message.raw_rfc822,
                &source.ordinary_attachments,
                MAX_MANAGED_ATTACHMENT_TOTAL_BYTES,
            ) {
                Ok(pending) => pending,
                Err(error) => return Ok(ForwardPreparationOutcome::Error { error }),
            }
        } else {
            warnings.push(ForwardWarning::AttachmentsOmittedByUser);
            PendingManagedImports::new(&self.managed_attachments)
        };

        let request = ComposeRequest {
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: forward_subject(&context.original_subject),
            body_text: String::new(),
            format: Default::default(),
            reply_context: None,
        };
        for _ in 0..MAX_LOCAL_DRAFT_CAS_RETRIES {
            let timestamp = now();
            let draft_id = Uuid::now_v7().to_string();
            let mime_attachments = self.read_pending_mime_attachments(pending.additions())?;
            let raw_rfc822 = build_draft_message_revision_with_attachments(
                &self.config.email,
                &request,
                &draft_id,
                1,
                Some(&context),
                mime_attachments,
            )?;
            let record = DraftRecord {
                draft: Draft {
                    id: draft_id.clone(),
                    local_version: 1,
                    has_unsupported_content: false,
                    account_id: self.config.account_id.clone(),
                    to: Vec::new(),
                    cc: Vec::new(),
                    bcc: Vec::new(),
                    subject: request.subject.clone(),
                    body_text: String::new(),
                    format: Default::default(),
                    reply_context: None,
                    status: "local".to_owned(),
                    remote_mailbox: None,
                    remote_uid: None,
                    created_at: timestamp.clone(),
                    updated_at: timestamp,
                    raw_rfc822,
                },
                local_version: 1,
                revision: 1,
                synced_revision: 0,
                remote_uid_validity: None,
                is_deleted: false,
            };
            match self
                .repository
                .insert_prepared_forward_if_source_unchanged(
                    message.id,
                    &message.raw_rfc822,
                    &record,
                    &context,
                    pending.additions(),
                )? {
                PreparedForwardInsert::Inserted => {
                    pending.commit();
                    return Ok(ForwardPreparationOutcome::Prepared {
                        prepared: PreparedForward {
                            draft: self.draft_dto(&draft_id)?,
                            warnings,
                        },
                    });
                }
                PreparedForwardInsert::SourceChanged => {
                    return Ok(forward_error(
                        ForwardPreparationErrorKind::SourceChanged,
                        Vec::new(),
                        false,
                    ));
                }
                PreparedForwardInsert::IdCollision => continue,
            }
        }
        Err(MailError::Validation(
            "could not allocate a unique forward draft id".to_owned(),
        ))
    }

    fn stage_forward_attachments<'a>(
        &'a self,
        raw_rfc822: &[u8],
        attachments: &[AttachmentPartMetadata],
        maximum_total_bytes: u64,
    ) -> std::result::Result<PendingManagedImports<'a>, ForwardPreparationError> {
        let maximum_total_bytes = maximum_total_bytes.min(MAX_MANAGED_ATTACHMENT_TOTAL_BYTES);
        let mut pending = PendingManagedImports::new(&self.managed_attachments);
        let mut total_bytes = 0u64;
        for metadata in attachments {
            if metadata.size_bytes > MAX_MANAGED_ATTACHMENT_BYTES {
                return Err(forward_preparation_error(
                    ForwardPreparationErrorKind::AttachmentStageFailed,
                    vec![metadata.id.clone()],
                    true,
                ));
            }
            total_bytes = match total_bytes.checked_add(metadata.size_bytes) {
                Some(total) if total <= maximum_total_bytes => total,
                _ => {
                    return Err(forward_preparation_error(
                        ForwardPreparationErrorKind::AttachmentStageFailed,
                        vec![metadata.id.clone()],
                        true,
                    ));
                }
            };
            let bytes = match extract_attachment(raw_rfc822, &metadata.id) {
                Ok(bytes) if bytes.len() as u64 == metadata.size_bytes => bytes,
                Ok(_) | Err(_) => {
                    return Err(forward_preparation_error(
                        ForwardPreparationErrorKind::AttachmentUnavailable,
                        vec![metadata.id.clone()],
                        true,
                    ));
                }
            };
            let imported = match self.managed_attachments.import_bytes(
                &bytes,
                &metadata.safe_display_name,
                &metadata.mime_type,
            ) {
                Ok(imported) => imported,
                Err(_) => {
                    return Err(forward_preparation_error(
                        ForwardPreparationErrorKind::AttachmentStageFailed,
                        vec![metadata.id.clone()],
                        true,
                    ));
                }
            };
            pending.push(NewDraftAttachment {
                imported,
                source_attachment_id: Some(metadata.id.clone()),
            });
        }
        Ok(pending)
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

    pub fn set_body_cache_budget_bytes(&self, max_total_bytes: u64) {
        self.body_cache_budget_bytes
            .store(max_total_bytes, AtomicOrdering::Release);
    }

    pub fn body_cache_budget_bytes(&self) -> u64 {
        self.body_cache_budget_bytes.load(AtomicOrdering::Acquire)
    }

    pub fn body_cache_usage_bytes(&self) -> Result<u64> {
        self.repository
            .message_body_cache_usage_bytes(&self.config.account_id)
    }

    pub fn enforce_body_cache_budget(&self, protected_message_id: Option<i64>) -> Result<usize> {
        let max_total_bytes = self.body_cache_budget_bytes();
        if max_total_bytes == u64::MAX {
            return Ok(0);
        }
        self.repository.evict_message_body_cache_to_limit(
            &self.config.account_id,
            max_total_bytes,
            protected_message_id,
        )
    }

    pub fn schedule_page_body_prefetch(
        self: &Arc<Self>,
        candidates: Vec<(String, u32, bool)>,
        max_total_bytes: u64,
        max_message_bytes: u32,
    ) -> usize {
        let generation = self
            .body_prefetch_page_generation
            .fetch_add(1, AtomicOrdering::AcqRel)
            .wrapping_add(1);
        let current_page = candidates
            .iter()
            .map(|(public_id, _, _)| public_id.clone())
            .collect();
        let selected = bounded_body_prefetch_ids(
            candidates
                .iter()
                .filter(|(_, _, body_fetched)| !body_fetched)
                .map(|(public_id, size_bytes, _)| (public_id.clone(), *size_bytes)),
            max_total_bytes,
            max_message_bytes,
        );
        let mut queue = self
            .body_prefetch_queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queue.cancel_page_jobs();
        queue.current_page = current_page;
        let mut queued = 0usize;
        for public_id in selected {
            let sequence = self
                .body_prefetch_sequence
                .fetch_add(1, AtomicOrdering::Relaxed);
            if queue.enqueue(
                public_id,
                BODY_PREFETCH_PRIORITY_PAGE,
                sequence,
                Some(generation),
            ) {
                queued += 1;
            }
        }
        drop(queue);
        if queued > 0 {
            self.start_body_prefetch_worker();
        }
        queued
    }

    pub fn promote_body_prefetch_for_selection(self: &Arc<Self>, public_id: &str) {
        let mut queue = self
            .body_prefetch_queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queue.queued.remove(public_id);
        let Some(selected_index) = queue
            .current_page
            .iter()
            .position(|candidate| candidate == public_id)
        else {
            return;
        };
        let start = selected_index.saturating_sub(BODY_PREFETCH_NEIGHBOR_RADIUS);
        let end =
            (selected_index + BODY_PREFETCH_NEIGHBOR_RADIUS + 1).min(queue.current_page.len());
        let neighbors = queue.current_page[start..end]
            .iter()
            .filter(|candidate| candidate.as_str() != public_id)
            .filter(|candidate| queue.queued.contains_key(candidate.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let generation = self
            .body_prefetch_page_generation
            .load(AtomicOrdering::Acquire);
        let mut promoted = 0usize;
        for neighbor in neighbors {
            let sequence = self
                .body_prefetch_sequence
                .fetch_add(1, AtomicOrdering::Relaxed);
            if queue.enqueue(
                neighbor,
                BODY_PREFETCH_PRIORITY_NEIGHBOR,
                sequence,
                Some(generation),
            ) {
                promoted += 1;
            }
        }
        drop(queue);
        if promoted > 0 {
            self.start_body_prefetch_worker();
        }
    }

    pub fn schedule_inbox_body_prefetch(
        self: &Arc<Self>,
        limit: usize,
        max_total_bytes: u64,
        max_message_bytes: u32,
    ) -> Result<usize> {
        self.schedule_recent_mailbox_body_prefetch(INBOX, limit, max_total_bytes, max_message_bytes)
    }

    pub fn schedule_sent_body_prefetch(
        self: &Arc<Self>,
        limit: usize,
        max_total_bytes: u64,
        max_message_bytes: u32,
    ) -> Result<usize> {
        let mailbox = self
            .repository
            .mailbox_for_role(&self.config.account_id, "sent")?;
        self.schedule_recent_mailbox_body_prefetch(
            &mailbox,
            limit,
            max_total_bytes,
            max_message_bytes,
        )
    }

    fn schedule_recent_mailbox_body_prefetch(
        self: &Arc<Self>,
        mailbox: &str,
        limit: usize,
        max_total_bytes: u64,
        max_message_bytes: u32,
    ) -> Result<usize> {
        if limit == 0 || max_total_bytes == 0 || max_message_bytes == 0 {
            return Ok(0);
        }
        let candidates = self.repository.mailbox_body_prefetch_page_candidates(
            &self.config.account_id,
            mailbox,
            limit,
        )?;
        let selected = bounded_body_prefetch_ids(candidates, max_total_bytes, max_message_bytes);
        let mut queue = self
            .body_prefetch_queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut queued = 0usize;
        for public_id in selected {
            let sequence = self
                .body_prefetch_sequence
                .fetch_add(1, AtomicOrdering::Relaxed);
            if queue.enqueue(public_id, BODY_PREFETCH_PRIORITY_RECENT, sequence, None) {
                queued += 1;
            }
        }
        drop(queue);
        if queued > 0 {
            self.start_body_prefetch_worker();
        }
        Ok(queued)
    }

    fn start_body_prefetch_worker(self: &Arc<Self>) {
        if self
            .body_prefetch_worker_started
            .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
            .is_ok()
        {
            let backend = Arc::clone(self);
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    backend.run_body_prefetch_worker().await;
                });
            } else {
                self.body_prefetch_worker_started
                    .store(false, AtomicOrdering::Release);
            }
        }
    }

    async fn run_body_prefetch_worker(self: Arc<Self>) {
        loop {
            let generation = self
                .body_prefetch_page_generation
                .load(AtomicOrdering::Acquire);
            let next = self
                .body_prefetch_queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pop_next(generation);
            if let Some(job) = next {
                let _ = self
                    .fetch_message_by_id_in_lane(&job.public_id, false, BodyFetchLane::Prefetch)
                    .await;
                continue;
            }

            self.body_prefetch_worker_started
                .store(false, AtomicOrdering::Release);
            let has_more = !self
                .body_prefetch_queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .queued
                .is_empty();
            if has_more
                && self
                    .body_prefetch_worker_started
                    .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
                    .is_ok()
            {
                continue;
            }
            return;
        }
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
                .fetch_mailbox_message_in_lane(mailbox, uid, false, BodyFetchLane::Prefetch)
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

    /// Fetches only the renderable text/HTML parts and attachment metadata for
    /// the selected reader. Ordinary attachment bodies are excluded from this
    /// path and remain on-demand.
    pub async fn fetch_message_view_by_id(
        &self,
        public_id: &str,
        force: bool,
    ) -> Result<(InboxMessage, Vec<AttachmentMeta>)> {
        let cached = self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)?;
        if cached.body_fetched && !cached.raw_rfc822.is_empty() && !force {
            self.repository.touch_message_body_access(cached.id)?;
            let attachments = self.cached_message_attachments(public_id)?;
            return self
                .repair_cached_inline_images(cached)
                .map(|message| (message, attachments));
        }

        let mut body_imap = self
            .selected_foreground_body_session(&cached.mailbox)
            .await?;
        let result = async {
            let session = body_imap
                .as_mut()
                .expect("foreground body IMAP session is connected before use");
            let structure = session
                .connection
                .fetch_message_structure(cached.uid)
                .await?;
            let attachment_pairs = remote_attachment_listing(public_id, &structure);
            let attachments = attachment_pairs
                .iter()
                .map(|(metadata, _)| metadata.clone())
                .collect::<Vec<_>>();
            let message = if cached.body_fetched && !force {
                self.repository.touch_message_body_access(cached.id)?;
                cached
            } else {
                let paths = selected_remote_body_paths(&structure.parts)?;
                let fetched_parts = session
                    .connection
                    .fetch_message_parts_bounded(
                        cached.uid,
                        &paths,
                        MAX_SELECTED_BODY_SECTION_BYTES,
                    )
                    .await?;
                let hydrated =
                    hydrate_selected_message_body(cached, &structure, fetched_parts, &attachments)?;
                self.repository.upsert_message(&hydrated)?;
                let stored = self
                    .repository
                    .get_message_by_public_id(&self.config.account_id, public_id)?;
                self.enforce_body_cache_budget(Some(stored.id))?;
                stored
            };
            Ok((message, attachments))
        }
        .await;
        match result {
            Ok(value) => {
                if let Some(session) = body_imap.as_mut() {
                    session.last_used = Instant::now();
                }
                Ok(value)
            }
            Err(error) => {
                *body_imap = None;
                Err(error)
            }
        }
    }

    async fn selected_foreground_body_session(
        &self,
        mailbox: &str,
    ) -> Result<MutexGuard<'_, Option<BodyImapSession>>> {
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
        let session = body_imap
            .as_mut()
            .expect("foreground body IMAP session is connected before mailbox selection");
        let selected_uid_validity = match session.connection.select_mailbox_for_fetch(mailbox).await
        {
            Ok(value) => value,
            Err(error) => {
                *body_imap = None;
                return Err(error);
            }
        };
        let local_uid_validity = self
            .repository
            .mailbox_state(&self.config.account_id, mailbox)?
            .and_then(|state| state.uid_validity);
        match classify_inbox_uid_scope(local_uid_validity, selected_uid_validity) {
            InboxUidScope::Current => Ok(body_imap),
            InboxUidScope::NeedsSync => {
                *body_imap = None;
                Err(MailError::Validation(
                    "Mailbox must be synchronized before downloading message bodies".to_owned(),
                ))
            }
            InboxUidScope::Changed => {
                self.repository
                    .reset_mailbox(&self.config.account_id, mailbox)?;
                *body_imap = None;
                Err(MailError::Validation(
                    "Mailbox UIDVALIDITY changed; synchronize the mailbox before downloading this message"
                        .to_owned(),
                ))
            }
        }
    }

    /// Hydrates a keyset-page item without accepting an arbitrary mailbox name
    /// or UID from the desktop UI.
    pub async fn fetch_message_by_id(&self, public_id: &str, force: bool) -> Result<InboxMessage> {
        self.fetch_message_by_id_in_lane(public_id, force, BodyFetchLane::Foreground)
            .await
    }

    async fn fetch_message_by_id_in_lane(
        &self,
        public_id: &str,
        force: bool,
        lane: BodyFetchLane,
    ) -> Result<InboxMessage> {
        let message = self
            .repository
            .get_message_by_public_id(&self.config.account_id, public_id)?;
        self.fetch_mailbox_message_in_lane(&message.mailbox, message.uid, force, lane)
            .await
    }

    /// Hydrates only a message already discovered in the local contact index,
    /// preserving the exact mailbox + UID identity across the UI boundary.
    pub async fn fetch_contact_message(&self, mailbox: &str, uid: u32) -> Result<InboxMessage> {
        if mailbox.trim().is_empty() {
            return Err(MailError::Validation(
                "message mailbox must not be blank".to_owned(),
            ));
        }
        if uid == 0 {
            return Err(MailError::Validation(
                "message UID must be greater than zero".to_owned(),
            ));
        }
        // Do not turn the contact reader into an arbitrary mailbox fetch API:
        // the exact identity must already exist in this account's SQLite cache.
        self.repository
            .get_message_by_uid(&self.config.account_id, mailbox, uid)?;
        self.fetch_mailbox_message(mailbox, uid, false).await
    }

    async fn fetch_mailbox_message(
        &self,
        mailbox: &str,
        uid: u32,
        force: bool,
    ) -> Result<InboxMessage> {
        self.fetch_mailbox_message_in_lane(mailbox, uid, force, BodyFetchLane::Foreground)
            .await
    }

    async fn fetch_mailbox_message_in_lane(
        &self,
        mailbox: &str,
        uid: u32,
        force: bool,
        lane: BodyFetchLane,
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
            Ok(message) if message.body_fetched && !message.raw_rfc822.is_empty() && !force => {
                if lane == BodyFetchLane::Foreground {
                    self.repository.touch_message_body_access(message.id)?;
                }
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

        let key = BodyDownloadKey {
            mailbox: mailbox.to_owned(),
            uid,
        };
        loop {
            let (owner_signal, waiter) = {
                let mut downloads = self
                    .body_downloads
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(signal) = downloads.get(&key) {
                    (None, Some(signal.clone().acquire_owned()))
                } else {
                    let signal = Arc::new(Semaphore::new(0));
                    downloads.insert(key.clone(), signal.clone());
                    (Some(signal), None)
                }
            };
            if let Some(waiter) = waiter {
                let _ = waiter.await;
                if !force
                    && let Ok(message) =
                        self.repository
                            .get_message_by_uid(&self.config.account_id, mailbox, uid)
                    && message.body_fetched
                    && !message.raw_rfc822.is_empty()
                {
                    if lane == BodyFetchLane::Foreground {
                        self.repository.touch_message_body_access(message.id)?;
                    }
                    return self.repair_cached_inline_images(message);
                }
                continue;
            }

            let _owner = BodyDownloadOwner {
                downloads: &self.body_downloads,
                key: key.clone(),
                signal: owner_signal.expect("a new body download owns its signal"),
            };
            return self
                .fetch_mailbox_message_owner(mailbox, uid, force, lane)
                .await;
        }
    }

    async fn fetch_mailbox_message_owner(
        &self,
        mailbox: &str,
        uid: u32,
        force: bool,
        lane: BodyFetchLane,
    ) -> Result<InboxMessage> {
        let body_imap = match lane {
            BodyFetchLane::Foreground => &self.body_imap,
            BodyFetchLane::Prefetch => &self.body_prefetch_imap,
        };
        let mut body_imap = body_imap.lock().await;
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

        if !force
            && let Ok(message) =
                self.repository
                    .get_message_by_uid(&self.config.account_id, mailbox, uid)
            && message.body_fetched
            && !message.raw_rfc822.is_empty()
        {
            if lane == BodyFetchLane::Foreground {
                self.repository.touch_message_body_access(message.id)?;
            }
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
            let stored =
                self.repository
                    .get_message_by_uid(&self.config.account_id, mailbox, uid)?;
            self.enforce_body_cache_budget(Some(stored.id))?;
            Ok(stored)
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

    /// Starts the approved empty stable compose draft (v1, no attachments).
    /// If the editor already has input, the desktop must persist that input
    /// through `save_draft_optimistic` before opening the first attachment
    /// picker; merging this DTO must never replace newer editor state.
    pub fn create_compose_draft(&self) -> Result<DraftDto> {
        let draft = self.insert_local_draft(
            &ComposeRequest {
                to: Vec::new(),
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: String::new(),
                body_text: String::new(),
                format: Default::default(),
                reply_context: None,
            },
            "local",
        )?;
        self.draft_dto(&draft.id)
    }

    pub fn draft_dto(&self, draft_id: &str) -> Result<DraftDto> {
        let draft = self.repository.get_draft(draft_id)?;
        if draft.account_id != self.config.account_id {
            return Err(MailError::NotFound {
                entity: "draft",
                id: draft_id.to_owned(),
            });
        }
        let attachments = self
            .repository
            .list_draft_attachments_at_version(
                &self.config.account_id,
                draft_id,
                draft.local_version,
            )?
            .ok_or_else(|| MailError::NotFound {
                entity: "draft version",
                id: draft_id.to_owned(),
            })?
            .attachments
            .into_iter()
            .map(|attachment| attachment.meta)
            .collect();
        let forward_context = self.repository.forward_context_at_version(
            &self.config.account_id,
            draft_id,
            draft.local_version,
        )?;
        Ok(DraftDto {
            draft,
            attachments,
            forward_context,
        })
    }

    /// Imports platform-selected paths into immutable storage and advances one
    /// exact draft version. The paths are Rust-only picker results.
    pub fn add_draft_attachments(
        &self,
        draft_id: &str,
        expected_local_version: u64,
        selected_files: &[PathBuf],
    ) -> Result<DraftAttachmentMutationOutcome> {
        if selected_files.is_empty() {
            return Ok(DraftAttachmentMutationOutcome {
                kind: DraftAttachmentMutationKind::Canceled,
                draft: self.draft_dto(draft_id)?,
                canonical: None,
            });
        }
        if selected_files.len() > MAX_ATTACHMENT_PARTS {
            return Err(MailError::Validation(
                "too many managed attachments are selected".to_owned(),
            ));
        }
        let mut selected_sizes = Vec::with_capacity(selected_files.len());
        for path in selected_files {
            let metadata = std::fs::metadata(path)?;
            selected_sizes.push(metadata.len());
        }

        let initial = self.repository.get_draft_record(draft_id)?;
        if initial.draft.account_id != self.config.account_id {
            return Err(MailError::NotFound {
                entity: "draft",
                id: draft_id.to_owned(),
            });
        }
        let expected_snapshot = self
            .repository
            .draft_version_snapshot(&self.config.account_id, draft_id, expected_local_version)?
            .ok_or_else(|| {
                MailError::Validation(
                    "the exact stale draft content snapshot is unavailable".to_owned(),
                )
            })?;
        if expected_snapshot.has_unsupported_content {
            return Err(MailError::Validation(
                "this draft contains unsupported MIME content and is read-only".to_owned(),
            ));
        }
        let expected_attachments = self
            .repository
            .list_draft_attachments_at_version(
                &self.config.account_id,
                draft_id,
                expected_local_version,
            )?
            .ok_or_else(|| {
                MailError::Validation(
                    "the exact stale draft attachment snapshot is unavailable".to_owned(),
                )
            })?;
        validate_managed_attachment_inventory(
            expected_attachments
                .attachments
                .len()
                .checked_add(selected_sizes.len())
                .ok_or_else(|| {
                    MailError::Validation("managed attachment count overflowed".to_owned())
                })?,
            expected_attachments
                .attachments
                .iter()
                .map(|attachment| attachment.meta.size_bytes)
                .chain(selected_sizes.iter().copied()),
        )?;

        let mut pending = PendingManagedImports::new(&self.managed_attachments);
        for path in selected_files {
            let imported = self.managed_attachments.import_file(path)?;
            pending.push(NewDraftAttachment {
                imported,
                source_attachment_id: None,
            });
        }

        if !initial.is_deleted
            && initial.draft.status != "sent"
            && initial.local_version == expected_local_version
        {
            let mut mime_attachments =
                self.read_managed_mime_attachments(&expected_attachments.attachments)?;
            mime_attachments.extend(self.read_pending_mime_attachments(pending.additions())?);
            let forward_context = self.repository.forward_context_at_version(
                &self.config.account_id,
                &initial.draft.id,
                expected_local_version,
            )?;
            let next_revision = initial
                .revision
                .checked_add(1)
                .ok_or_else(|| MailError::Validation("draft revision limit reached".to_owned()))?;
            let raw_rfc822 = build_draft_message_revision_with_attachments(
                &self.config.email,
                &expected_snapshot.request,
                &initial.draft.id,
                next_revision,
                forward_context.as_ref(),
                mime_attachments,
            )?;
            if self
                .repository
                .add_draft_attachments_and_raw_if_local_version(
                    &self.config.account_id,
                    &initial.draft.id,
                    expected_local_version,
                    pending.additions(),
                    &now(),
                    &raw_rfc822,
                )?
                .is_some()
            {
                pending.commit();
                return Ok(DraftAttachmentMutationOutcome {
                    kind: DraftAttachmentMutationKind::Saved,
                    draft: self.draft_dto(draft_id)?,
                    canonical: None,
                });
            }
        }

        // A stale add is data-creating, so preserve the newly copied bytes in
        // a conflict branch while leaving the latest canonical row unchanged.
        for _ in 0..MAX_LOCAL_DRAFT_CAS_RETRIES {
            let source = self.repository.get_draft_record(draft_id)?;
            if source.draft.account_id != self.config.account_id {
                return Err(MailError::NotFound {
                    entity: "draft",
                    id: draft_id.to_owned(),
                });
            }
            let mut mime_attachments =
                self.read_managed_mime_attachments(&expected_attachments.attachments)?;
            mime_attachments.extend(self.read_pending_mime_attachments(pending.additions())?);
            let context = self.repository.forward_context_at_version(
                &self.config.account_id,
                &source.draft.id,
                expected_local_version,
            )?;
            let timestamp = now();
            let conflict_id = Uuid::now_v7().to_string();
            let request = expected_snapshot.request.clone();
            let conflict = DraftRecord {
                draft: Draft {
                    id: conflict_id.clone(),
                    local_version: 1,
                    has_unsupported_content: false,
                    account_id: self.config.account_id.clone(),
                    to: request.to.clone(),
                    cc: request.cc.clone(),
                    bcc: request.bcc.clone(),
                    subject: conflict_subject(&request.subject),
                    body_text: request.body_text.clone(),
                    format: request.format.clone(),
                    reply_context: request.reply_context.clone(),
                    status: "conflict".to_owned(),
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
            let mut conflict = conflict;
            conflict.draft.raw_rfc822 = build_draft_message_revision_with_attachments(
                &self.config.email,
                &conflict.draft.compose_request(),
                &conflict_id,
                1,
                context.as_ref(),
                mime_attachments,
            )?;
            if self
                .repository
                .insert_attachment_conflict_if_source_unchanged(
                    &source,
                    expected_local_version,
                    &conflict,
                    pending.additions(),
                )?
            {
                pending.commit();
                let canonical = (!source.is_deleted)
                    .then(|| self.draft_dto(&source.draft.id))
                    .transpose()?;
                return Ok(DraftAttachmentMutationOutcome {
                    kind: DraftAttachmentMutationKind::ConflictCopy,
                    draft: self.draft_dto(&conflict_id)?,
                    canonical,
                });
            }
        }
        Err(MailError::Validation(
            "draft changed too frequently; add the attachments again".to_owned(),
        ))
    }

    pub fn remove_draft_attachment(
        &self,
        draft_id: &str,
        attachment_id: &str,
        expected_local_version: u64,
    ) -> Result<DraftAttachmentMutationOutcome> {
        let record = self.repository.get_draft_record(draft_id)?;
        if record.draft.account_id != self.config.account_id {
            return Err(MailError::NotFound {
                entity: "draft",
                id: draft_id.to_owned(),
            });
        }
        if record.local_version != expected_local_version
            || record.is_deleted
            || record.draft.status == "sent"
        {
            return Ok(DraftAttachmentMutationOutcome {
                kind: DraftAttachmentMutationKind::Stale,
                draft: self.draft_dto(draft_id)?,
                canonical: None,
            });
        }
        if record.draft.has_unsupported_content {
            return Err(MailError::Validation(
                "this draft contains unsupported MIME content and is read-only".to_owned(),
            ));
        }
        let mut existing = self.managed_draft_attachments(&record)?;
        let Some(remove_at) = existing
            .iter()
            .position(|attachment| attachment.meta.id == attachment_id)
        else {
            return Err(MailError::NotFound {
                entity: "draft attachment",
                id: attachment_id.to_owned(),
            });
        };
        existing.remove(remove_at);
        let mime_attachments = self.read_managed_mime_attachments(&existing)?;
        let context = self.repository.forward_context_at_version(
            &self.config.account_id,
            draft_id,
            expected_local_version,
        )?;
        let next_revision = record
            .revision
            .checked_add(1)
            .ok_or_else(|| MailError::Validation("draft revision limit reached".to_owned()))?;
        let raw_rfc822 = build_draft_message_revision_with_attachments(
            &self.config.email,
            &record.draft.compose_request(),
            draft_id,
            next_revision,
            context.as_ref(),
            mime_attachments,
        )?;
        let Some(_) = self
            .repository
            .remove_draft_attachment_and_raw_if_local_version(
                &self.config.account_id,
                draft_id,
                attachment_id,
                expected_local_version,
                &now(),
                &raw_rfc822,
            )?
        else {
            return Ok(DraftAttachmentMutationOutcome {
                kind: DraftAttachmentMutationKind::Stale,
                draft: self.draft_dto(draft_id)?,
                canonical: None,
            });
        };
        let draft = self.draft_dto(draft_id)?;
        self.cleanup_managed_attachments()?;
        Ok(DraftAttachmentMutationOutcome {
            kind: DraftAttachmentMutationKind::Saved,
            draft,
            canonical: None,
        })
    }

    fn managed_draft_attachments(
        &self,
        record: &DraftRecord,
    ) -> Result<Vec<ManagedDraftAttachment>> {
        self.repository
            .list_draft_attachments_at_version(
                &self.config.account_id,
                &record.draft.id,
                record.local_version,
            )?
            .map(|snapshot| snapshot.attachments)
            .ok_or_else(|| MailError::NotFound {
                entity: "draft attachment version",
                id: record.draft.id.clone(),
            })
    }

    fn read_managed_mime_attachments(
        &self,
        attachments: &[ManagedDraftAttachment],
    ) -> Result<Vec<ManagedMimeAttachment>> {
        validate_managed_attachment_inventory(
            attachments.len(),
            attachments
                .iter()
                .map(|attachment| attachment.meta.size_bytes),
        )?;
        attachments
            .iter()
            .map(|attachment| {
                if attachment.disposition != AttachmentDisposition::Attachment
                    || attachment.transfer_encoding != "base64"
                {
                    return Err(MailError::Validation(
                        "the managed attachment encoding is unsupported".to_owned(),
                    ));
                }
                let bytes = match attachment.sha256_hex.as_deref() {
                    Some(expected_sha256_hex) => self
                        .managed_attachments
                        .read_internal_file(
                            &attachment.internal_name,
                            attachment.meta.size_bytes,
                            expected_sha256_hex,
                        )
                        .map_err(|_| managed_attachment_integrity_error())?,
                    None => {
                        let (bytes, computed_sha256_hex) = self
                            .managed_attachments
                            .read_internal_file_for_digest_backfill(
                                &attachment.internal_name,
                                attachment.meta.size_bytes,
                            )
                            .map_err(|_| managed_attachment_integrity_error())?;
                        self.repository.initialize_managed_attachment_digest(
                            &self.config.account_id,
                            &attachment.meta.id,
                            &attachment.internal_name,
                            attachment.meta.size_bytes,
                            &computed_sha256_hex,
                        )?;
                        bytes
                    }
                };
                Ok(ManagedMimeAttachment {
                    name: attachment.meta.name.clone(),
                    mime_type: attachment.meta.mime_type.clone(),
                    size_bytes: attachment.meta.size_bytes,
                    bytes,
                })
            })
            .collect()
    }

    fn read_pending_mime_attachments(
        &self,
        additions: &[NewDraftAttachment],
    ) -> Result<Vec<ManagedMimeAttachment>> {
        validate_managed_attachment_inventory(
            additions.len(),
            additions
                .iter()
                .map(|addition| addition.imported.size_bytes),
        )?;
        additions
            .iter()
            .map(|addition| {
                let imported = &addition.imported;
                let bytes = self.managed_attachments.read_internal_file(
                    &imported.internal_name,
                    imported.size_bytes,
                    &imported.sha256_hex,
                )?;
                Ok(ManagedMimeAttachment {
                    name: imported.name.clone(),
                    mime_type: imported.mime_type.clone(),
                    size_bytes: imported.size_bytes,
                    bytes,
                })
            })
            .collect()
    }

    fn build_draft_raw_for_snapshot(
        &self,
        record: &DraftRecord,
        request: &ComposeRequest,
        draft_id: &str,
        revision: u64,
    ) -> Result<Vec<u8>> {
        let attachments = self.managed_draft_attachments(record)?;
        let mime_attachments = self.read_managed_mime_attachments(&attachments)?;
        let context = self.repository.forward_context_at_version(
            &self.config.account_id,
            &record.draft.id,
            record.local_version,
        )?;
        build_draft_message_revision_with_attachments(
            &self.config.email,
            request,
            draft_id,
            revision,
            context.as_ref(),
            mime_attachments,
        )
    }

    fn insert_body_conflict_copy(
        &self,
        initial_source: &DraftRecord,
        attachment_source_local_version: u64,
        request: &ComposeRequest,
    ) -> Result<Draft> {
        let attachment_snapshot = self
            .repository
            .list_draft_attachments_at_version(
                &self.config.account_id,
                &initial_source.draft.id,
                attachment_source_local_version,
            )?
            .ok_or_else(|| {
                MailError::Validation(
                    "the exact stale draft attachment snapshot is unavailable".to_owned(),
                )
            })?;
        let context = self.repository.forward_context_at_version(
            &self.config.account_id,
            &initial_source.draft.id,
            attachment_source_local_version,
        )?;
        let mut source = initial_source.clone();
        for _ in 0..MAX_LOCAL_DRAFT_CAS_RETRIES {
            let mime_attachments =
                self.read_managed_mime_attachments(&attachment_snapshot.attachments)?;
            let timestamp = now();
            let id = Uuid::now_v7().to_string();
            let mut conflict_request = request.clone();
            conflict_request.subject = conflict_subject(&request.subject);
            let raw_rfc822 = build_draft_message_revision_with_attachments(
                &self.config.email,
                &conflict_request,
                &id,
                1,
                context.as_ref(),
                mime_attachments,
            )?;
            let conflict = DraftRecord {
                draft: Draft {
                    id: id.clone(),
                    local_version: 1,
                    has_unsupported_content: false,
                    account_id: self.config.account_id.clone(),
                    to: conflict_request.to.clone(),
                    cc: conflict_request.cc.clone(),
                    bcc: conflict_request.bcc.clone(),
                    subject: conflict_request.subject.clone(),
                    body_text: conflict_request.body_text.clone(),
                    format: conflict_request.format.clone(),
                    reply_context: conflict_request.reply_context.clone(),
                    status: "conflict".to_owned(),
                    remote_mailbox: None,
                    remote_uid: None,
                    created_at: timestamp.clone(),
                    updated_at: timestamp,
                    raw_rfc822,
                },
                local_version: 1,
                revision: 1,
                synced_revision: 0,
                remote_uid_validity: None,
                is_deleted: false,
            };
            if self
                .repository
                .insert_attachment_conflict_if_source_unchanged(
                    &source,
                    attachment_source_local_version,
                    &conflict,
                    &[],
                )?
            {
                return Ok(conflict.draft);
            }
            source = self.repository.get_draft_record(&source.draft.id)?;
            if source.draft.account_id != self.config.account_id || source.is_deleted {
                break;
            }
        }
        Err(MailError::Validation(
            "draft changed too frequently; save the conflict copy again".to_owned(),
        ))
    }

    pub fn save_draft(&self, request: ComposeRequest) -> Result<Draft> {
        self.upsert_draft(None, request)
    }

    /// Create a draft or update an existing draft while retaining its stable
    /// identity. Updates increment the private draft revision used by the IMAP
    /// reconciliation algorithm.
    pub fn upsert_draft(&self, draft_id: Option<&str>, request: ComposeRequest) -> Result<Draft> {
        let request = normalize_owned_compose_html(request);
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
                    replacement.draft.format = request.format.clone();
                    replacement.draft.reply_context = request.reply_context.clone();
                    replacement.draft.status = "local".to_owned();
                    replacement.draft.updated_at = now();
                    replacement.is_deleted = false;
                    replacement.draft.raw_rfc822 = self.build_draft_raw_for_snapshot(
                        &expected,
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
        let request = normalize_owned_compose_html(request);
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
                    replacement.draft.format = request.format.clone();
                    replacement.draft.reply_context = request.reply_context.clone();
                    replacement.draft.status = "local".to_owned();
                    replacement.draft.updated_at = now();
                    replacement.is_deleted = false;
                    replacement.draft.raw_rfc822 = self.build_draft_raw_for_snapshot(
                        expected,
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

                let canonical_record = match self.repository.get_draft_record(id) {
                    Ok(record)
                        if record.draft.account_id == self.config.account_id
                            && !record.is_deleted =>
                    {
                        Some(record)
                    }
                    Ok(_) | Err(MailError::NotFound { .. }) => None,
                    Err(error) => return Err(error),
                };
                let canonical = canonical_record.as_ref().map(|record| record.draft.clone());

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

                let draft = match canonical_record.as_ref() {
                    Some(source) => {
                        self.insert_body_conflict_copy(source, expected_local_version, &request)?
                    }
                    None => self.insert_local_draft(&request, "conflict")?,
                };
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
                    format: request.format.clone(),
                    reply_context: request.reply_context.clone(),
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
        if draft.account_id != self.config.account_id {
            return Err(MailError::NotFound {
                entity: "draft",
                id: draft_id.to_owned(),
            });
        }
        if draft.status == "sent" {
            return Err(MailError::Validation(
                "a sent draft cannot be deleted as an active draft".to_owned(),
            ));
        }
        self.repository.tombstone_draft(draft_id, &now())?;
        // The tombstone and reference release are committed before file
        // cleanup. A cleanup failure cannot make a discarded draft visible
        // again, and startup retries orphan cleanup conservatively.
        let _ = self.cleanup_managed_attachments();
        Ok(())
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
        if deleted {
            let _ = self.cleanup_managed_attachments();
        }
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
        self.sync_drafts_with_progress(mailbox_override, |_| {})
            .await
    }

    pub async fn sync_drafts_with_progress<F>(
        &self,
        mailbox_override: Option<&str>,
        mut on_progress: F,
    ) -> Result<DraftSyncReport>
    where
        F: FnMut(SyncBatchProgress) + Send,
    {
        let _guard = self.draft_imap_gate.lock().await;
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
        let total = remote_groups
            .keys()
            .chain(local_records.keys())
            .collect::<HashSet<_>>()
            .len();
        let mut completed = 0;
        on_progress(SyncBatchProgress { completed, total });

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
                advance_draft_sync_progress(&mut completed, total, &mut on_progress);
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
                advance_draft_sync_progress(&mut completed, total, &mut on_progress);
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
                advance_draft_sync_progress(&mut completed, total, &mut on_progress);
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
                advance_draft_sync_progress(&mut completed, total, &mut on_progress);
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
            advance_draft_sync_progress(&mut completed, total, &mut on_progress);
        }

        for record in local_records.into_values() {
            if record.draft.status == "sent" || record.draft.status == "conflict" {
                advance_draft_sync_progress(&mut completed, total, &mut on_progress);
                continue;
            }
            if record.is_deleted {
                if !self.repository.delete_draft_if_unchanged(&record)? {
                    report.skipped += 1;
                }
                advance_draft_sync_progress(&mut completed, total, &mut on_progress);
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
            advance_draft_sync_progress(&mut completed, total, &mut on_progress);
        }

        let _ = connection.logout().await;
        let _ = self.cleanup_managed_attachments();
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
                format: remote.request.format.clone(),
                reply_context: remote.request.reply_context.clone(),
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
                format: remote.request.format.clone(),
                reply_context: remote.request.reply_context.clone(),
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
        request.subject = conflict_subject(&request.subject);
        let raw_rfc822 = self.build_draft_raw_for_snapshot(local, &request, &id, 1)?;
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
                format: request.format.clone(),
                reply_context: request.reply_context.clone(),
                status: "conflict".to_owned(),
                remote_mailbox: None,
                remote_uid: None,
                created_at: timestamp.clone(),
                updated_at: timestamp,
                raw_rfc822,
            },
            local_version: 1,
            revision: 1,
            synced_revision: 0,
            remote_uid_validity: None,
            is_deleted: false,
        })
    }

    pub async fn send_compose(&self, request: ComposeRequest) -> Result<OutboxItem> {
        self.send_request(request, None, Vec::new(), None).await
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
            snapshot.attachments,
            snapshot.forward_context,
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
        let managed_attachments = self.managed_draft_attachments(&record)?;
        let attachments = self.read_managed_mime_attachments(&managed_attachments)?;
        let forward_context = self.repository.forward_context_at_version(
            &self.config.account_id,
            &record.draft.id,
            record.local_version,
        )?;
        Ok(ConfirmedDraftSnapshot {
            id: record.draft.id,
            revision: record.revision,
            local_version: record.local_version,
            request,
            attachments,
            forward_context,
        })
    }

    pub fn list_outbox(&self) -> Result<Vec<OutboxItem>> {
        self.repository.list_outbox(&self.config.account_id)
    }

    pub fn list_sent_outbox_fallbacks(&self) -> Result<Vec<OutboxItem>> {
        self.repository
            .list_sent_outbox_fallbacks(&self.config.account_id)
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
                let _ = self.cleanup_managed_attachments();
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

    /// Records the user's externally verified "already delivered" decision for
    /// one exact ambiguous attempt generation. This performs no network work.
    /// Replaying the decision after the transition is safely rejected.
    pub fn confirm_delivery_unknown(
        &self,
        outbox_id: &str,
        expected_attempts: u32,
    ) -> Result<OutboxItem> {
        let item = self.repository.confirm_delivery_unknown_as_sent(
            outbox_id,
            &self.config.account_id,
            expected_attempts,
        )?;
        let _ = self.cleanup_managed_attachments();
        Ok(item)
    }

    /// Performs one explicitly user-approved duplicate-risk retry of the exact
    /// immutable RFC822 bytes and SMTP envelope persisted in Outbox.
    ///
    /// The attempt generation is checked both before and inside the claiming
    /// transaction. A stale/double-submitted decision cannot become a second
    /// retry, including when the first retry also ends in `delivery_unknown`.
    pub async fn retry_delivery_unknown_once(
        &self,
        outbox_id: &str,
        expected_attempts: u32,
        acknowledge_duplicate_risk: bool,
    ) -> Result<OutboxItem> {
        if !acknowledge_duplicate_risk {
            return Err(MailError::Validation(
                "retrying an unknown delivery requires explicit acknowledgement of duplicate risk"
                    .to_owned(),
            ));
        }

        let _guard = self.smtp_gate.lock().await;
        let snapshot = self.repository.get_outbox(outbox_id)?;
        validate_delivery_unknown_attempt(&snapshot, &self.config.account_id, expected_attempts)?;
        let envelope = restore_outbox_envelope(&snapshot.raw_rfc822, &snapshot.recipients)?;
        let client = SmtpClient::new(&self.config)?;

        // The IMMEDIATE transaction repeats account, state, and generation
        // checks before changing the item to `sending`.
        let claimed = self.repository.claim_delivery_unknown_retry(
            outbox_id,
            &self.config.account_id,
            expected_attempts,
        )?;
        match client.send_raw(&envelope, &claimed.raw_rfc822).await {
            Ok(()) => {
                self.repository.finalize_claimed_outbox_sent(
                    outbox_id,
                    &self.config.account_id,
                    claimed.attempts,
                )?;
                let _ = self.cleanup_managed_attachments();
            }
            Err(failure) => {
                // Reuse the ordinary SMTP classifier. An unprovable result
                // returns to `delivery_unknown` at the incremented generation
                // and is never scheduled automatically.
                self.repository.complete_claimed_outbox_failure(
                    outbox_id,
                    &self.config.account_id,
                    claimed.attempts,
                    failure.status,
                    &failure.safe_reason,
                )?;
            }
        }

        self.repository.get_outbox(outbox_id)
    }

    async fn send_request(
        &self,
        request: ComposeRequest,
        draft_snapshot: Option<(String, u64, u64)>,
        attachments: Vec<ManagedMimeAttachment>,
        forward_context: Option<ForwardContext>,
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

        let outgoing = if attachments.is_empty() && forward_context.is_none() {
            build_outgoing_message(&self.config.email, &request)?
        } else {
            build_outgoing_message_with_attachments(
                &self.config.email,
                &request,
                forward_context.as_ref(),
                attachments,
            )?
        };
        let envelope = outgoing.envelope;
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
            recipient_groups: Some(crate::OutboxRecipientGroups::from(&request)),
            status: OutboxStatus::Queued,
            attempts: 0,
            last_error: None,
            created_at: now(),
            sent_at: None,
            raw_rfc822: outgoing.raw_rfc822,
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
        drop(queued);
        match client.send_raw(&envelope, &claimed.raw_rfc822).await {
            Ok(()) => {
                self.repository.finalize_outbox_sent(&outbox_id)?;
                let _ = self.cleanup_managed_attachments();
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

fn public_attachment_meta(metadata: AttachmentPartMetadata) -> AttachmentMeta {
    AttachmentMeta {
        id: metadata.id,
        original_name: metadata.original_name,
        safe_display_name: metadata.safe_display_name,
        mime_type: metadata.mime_type,
        size_bytes: metadata.size_bytes,
        size_is_estimate: false,
        disposition: match metadata.disposition {
            crate::mime::AttachmentDisposition::Attachment => AttachmentDisposition::Attachment,
            crate::mime::AttachmentDisposition::Inline => AttachmentDisposition::Inline,
        },
    }
}

fn remote_attachment_listing(
    public_id: &str,
    structure: &RemoteMessageStructure,
) -> Vec<(AttachmentMeta, Vec<u32>)> {
    structure
        .parts
        .iter()
        .filter_map(|part| {
            let disposition = remote_attachment_disposition(part)?;
            let original_name = part
                .original_name
                .as_deref()
                .and_then(bounded_original_attachment_name);
            let safe_display_name = safe_attachment_filename(original_name.as_deref());
            let (size_bytes, size_is_estimate) = remote_attachment_display_size(part);
            Some((
                AttachmentMeta {
                    id: remote_attachment_id(
                        public_id,
                        &part.path,
                        &part.mime_type,
                        original_name.as_deref(),
                        disposition,
                        part.encoded_size_bytes,
                    ),
                    original_name,
                    safe_display_name,
                    mime_type: part.mime_type.clone(),
                    size_bytes,
                    size_is_estimate,
                    disposition,
                },
                part.path.clone(),
            ))
        })
        .take(MAX_ATTACHMENT_PARTS)
        .collect()
}

fn remote_attachment_disposition(part: &RemoteMimePart) -> Option<AttachmentDisposition> {
    match part.disposition.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("attachment") => {
            Some(AttachmentDisposition::Attachment)
        }
        _ if matches!(part.mime_type.as_str(), "text/plain" | "text/html")
            && part.original_name.is_none() =>
        {
            None
        }
        Some(value) if value.eq_ignore_ascii_case("inline") => Some(AttachmentDisposition::Inline),
        _ if part.content_id.is_some() => Some(AttachmentDisposition::Inline),
        _ if part.original_name.is_some() => Some(AttachmentDisposition::Attachment),
        _ if !matches!(
            part.mime_type.as_str(),
            "text/plain" | "text/html" | "message/rfc822"
        ) =>
        {
            Some(AttachmentDisposition::Attachment)
        }
        _ => None,
    }
}

fn remote_attachment_display_size(part: &RemoteMimePart) -> (u64, bool) {
    match part.transfer_encoding {
        RemoteTransferEncoding::SevenBit
        | RemoteTransferEncoding::EightBit
        | RemoteTransferEncoding::Binary => (part.encoded_size_bytes, false),
        RemoteTransferEncoding::Base64 => (part.encoded_size_bytes.saturating_mul(3) / 4, true),
        RemoteTransferEncoding::QuotedPrintable | RemoteTransferEncoding::Other(_) => {
            (part.encoded_size_bytes, true)
        }
    }
}

fn remote_attachment_id(
    public_id: &str,
    path: &[u32],
    mime_type: &str,
    original_name: Option<&str>,
    disposition: AttachmentDisposition,
    encoded_size_bytes: u64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"mine-mail-remote-attachment-v1\0");
    hasher.update(public_id.as_bytes());
    for segment in path {
        hasher.update(segment.to_be_bytes());
    }
    hasher.update(mime_type.as_bytes());
    if let Some(original_name) = original_name {
        hasher.update(original_name.as_bytes());
    }
    hasher.update([match disposition {
        AttachmentDisposition::Attachment => 1,
        AttachmentDisposition::Inline => 2,
    }]);
    hasher.update(encoded_size_bytes.to_be_bytes());
    let digest = hasher.finalize();
    format!(
        "{REMOTE_ATTACHMENT_ID_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(digest)
    )
}

fn is_remote_attachment_id(value: &str) -> bool {
    value
        .strip_prefix(REMOTE_ATTACHMENT_ID_PREFIX)
        .is_some_and(|digest| {
            digest.len() == 43
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

fn selected_remote_body_paths(parts: &[RemoteMimePart]) -> Result<Vec<Vec<u32>>> {
    let mut plain = None;
    let mut html = None;
    let mut saw_oversized_body = false;
    for part in parts {
        if remote_attachment_disposition(part).is_some() {
            continue;
        }
        if !matches!(part.mime_type.as_str(), "text/plain" | "text/html") {
            continue;
        }
        if part.encoded_size_bytes > MAX_SELECTED_BODY_SECTION_BYTES {
            saw_oversized_body = true;
            continue;
        }
        if part.mime_type == "text/plain" && plain.is_none() {
            plain = Some(part.path.clone());
        } else if part.mime_type == "text/html" && html.is_none() {
            html = Some(part.path.clone());
        }
    }
    let selected = plain.into_iter().chain(html).collect::<Vec<_>>();
    if selected.is_empty() && saw_oversized_body {
        return Err(MailError::Validation(
            "message body exceeds the selective reader limit".to_owned(),
        ));
    }
    Ok(selected)
}

fn hydrate_selected_message_body(
    mut message: InboxMessage,
    structure: &RemoteMessageStructure,
    fetched_parts: Vec<RemoteBodyPart>,
    attachments: &[AttachmentMeta],
) -> Result<InboxMessage> {
    let mut body_text = None;
    let mut html_fallback_text = None;
    let mut body_html = None;
    for fetched in fetched_parts {
        let Some(part) = structure
            .parts
            .iter()
            .find(|part| part.path == fetched.path)
        else {
            return Err(MailError::Mime(
                "selected MIME part no longer matches BODYSTRUCTURE".to_owned(),
            ));
        };
        let decoded = decode_remote_mime_part(&fetched.mime_header, &fetched.encoded_body)?;
        match part.mime_type.as_str() {
            "text/plain" if body_text.is_none() => body_text = decoded.body_text,
            "text/html" if body_html.is_none() => {
                html_fallback_text = decoded.body_text;
                body_html = decoded.body_html;
            }
            _ => {}
        }
    }
    message.body_text = body_text.or(html_fallback_text);
    message.body_html = body_html;
    message.attachment_names = attachments
        .iter()
        .filter(|attachment| attachment.disposition == AttachmentDisposition::Attachment)
        .map(|attachment| attachment.safe_display_name.clone())
        .collect();
    message.body_fetched = true;
    message.synced_at = now();
    if message.preview.trim().is_empty()
        && let Some(body) = message.body_text.as_deref()
    {
        message.preview = body.chars().take(180).collect();
    }
    Ok(message)
}

fn attachment_save_error(
    error_kind: AttachmentSaveErrorKind,
    retryable: bool,
) -> AttachmentSaveResult {
    AttachmentSaveResult {
        status: AttachmentSaveStatus::Error,
        file_name: None,
        error_kind: Some(error_kind),
        retryable,
    }
}

fn attachment_save_io_error(error: &io::Error) -> AttachmentSaveErrorKind {
    match error.kind() {
        io::ErrorKind::PermissionDenied => AttachmentSaveErrorKind::PermissionDenied,
        io::ErrorKind::StorageFull => AttachmentSaveErrorKind::DiskFull,
        _ => AttachmentSaveErrorKind::WriteFailed,
    }
}

fn conflict_subject(subject: &str) -> String {
    if subject.trim().is_empty() {
        "本地冲突副本".to_owned()
    } else {
        format!("{subject}（本地冲突副本）")
    }
}

fn forward_subject(subject: &str) -> String {
    let subject = subject.trim();
    if subject
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("fwd:"))
    {
        subject.to_owned()
    } else if subject.is_empty() {
        "Fwd:".to_owned()
    } else {
        format!("Fwd: {subject}")
    }
}

fn forward_error(
    kind: ForwardPreparationErrorKind,
    failed_attachment_ids: Vec<String>,
    retry_without_attachments_allowed: bool,
) -> ForwardPreparationOutcome {
    ForwardPreparationOutcome::Error {
        error: forward_preparation_error(
            kind,
            failed_attachment_ids,
            retry_without_attachments_allowed,
        ),
    }
}

fn forward_preparation_error(
    kind: ForwardPreparationErrorKind,
    failed_attachment_ids: Vec<String>,
    retry_without_attachments_allowed: bool,
) -> ForwardPreparationError {
    ForwardPreparationError {
        kind,
        failed_attachment_ids,
        retry_without_attachments_allowed,
    }
}

fn privacy_safe_imap_error(operation: &'static str) -> MailError {
    MailError::Imap(format!("{operation} failed"))
}

fn privacy_safe_network_error(error: MailError, operation: &'static str) -> MailError {
    match error {
        MailError::Imap(_) | MailError::Timeout { .. } => privacy_safe_imap_error(operation),
        other => other,
    }
}

fn message_mutation_error_kind(error: &MailError) -> MessageMutationErrorKind {
    match error {
        MailError::Timeout { .. } | MailError::Io(_) => {
            MessageMutationErrorKind::NetworkUnavailable
        }
        MailError::Validation(_) => MessageMutationErrorKind::Unsupported,
        MailError::Imap(_) => MessageMutationErrorKind::Unknown,
        _ => MessageMutationErrorKind::Unknown,
    }
}

fn system_flag_is_set(flags: &[String], flag: SystemFlagKind) -> bool {
    let target = match flag {
        SystemFlagKind::Seen => "\\Seen",
        SystemFlagKind::Flagged => "\\Flagged",
    };
    flags.iter().any(|value| value.eq_ignore_ascii_case(target))
}

fn normalized_message_page_size(page_size: usize) -> usize {
    if page_size == 0 {
        DEFAULT_MESSAGE_PAGE_SIZE
    } else {
        page_size.min(MAX_MESSAGE_PAGE_SIZE)
    }
}

fn earlier_history_bound(
    persisted_before_uid: Option<u32>,
    selected_uid_next: Option<u32>,
) -> Option<u32> {
    [persisted_before_uid, selected_uid_next]
        .into_iter()
        .flatten()
        .min()
}

fn page_with_remote_state(mut page: MessagePage, state: RemoteHistoryState) -> MessagePage {
    page.remote_history_state = state;
    page.end_reached = !page.has_more_local && state == RemoteHistoryState::Complete;
    if page.end_reached || state == RemoteHistoryState::Unavailable {
        page.next_cursor = None;
    }
    page
}

fn validate_managed_attachment_inventory(
    expected_count: usize,
    sizes: impl IntoIterator<Item = u64>,
) -> Result<()> {
    if expected_count > MAX_ATTACHMENT_PARTS {
        return Err(MailError::Validation(
            "too many managed attachments are selected".to_owned(),
        ));
    }
    let mut actual_count = 0usize;
    let mut total_bytes = 0u64;
    for size_bytes in sizes {
        actual_count = actual_count.checked_add(1).ok_or_else(|| {
            MailError::Validation("managed attachment count overflowed".to_owned())
        })?;
        if size_bytes > MAX_MANAGED_ATTACHMENT_BYTES {
            return Err(MailError::Validation(
                "a managed attachment exceeds the configured byte limit".to_owned(),
            ));
        }
        total_bytes = total_bytes.checked_add(size_bytes).ok_or_else(|| {
            MailError::Validation("managed attachment byte total overflowed".to_owned())
        })?;
        if total_bytes > MAX_MANAGED_ATTACHMENT_TOTAL_BYTES {
            return Err(MailError::Validation(
                "the combined managed attachment set is too large".to_owned(),
            ));
        }
    }
    if actual_count != expected_count {
        return Err(MailError::Validation(
            "the managed attachment inventory changed during validation".to_owned(),
        ));
    }
    Ok(())
}

fn message_activity_at(message: &InboxMessage) -> Option<String> {
    message
        .internal_date
        .clone()
        .or_else(|| message.sent_at.clone())
        .or_else(|| Some(message.synced_at.clone()))
}

fn normalized_header_name(address: &MailAddress) -> Option<String> {
    let value = address.name.as_deref()?.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    Some(value.chars().take(200).collect())
}

fn contact_participants(message: &InboxMessage, own_email: &str) -> Vec<(String, Option<String>)> {
    let mut participants = Vec::<(String, Option<String>)>::new();
    let mut indexes = HashMap::<String, usize>::new();
    for address in message
        .sender
        .iter()
        .chain(message.to.iter())
        .chain(message.cc.iter())
    {
        let Ok(email) = normalize_contact_email(&address.email) else {
            continue;
        };
        if email == own_email {
            continue;
        }
        let name = normalized_header_name(address);
        if let Some(index) = indexes.get(&email).copied() {
            if participants[index].1.is_none() && name.is_some() {
                participants[index].1 = name;
            }
        } else {
            indexes.insert(email.clone(), participants.len());
            participants.push((email, name));
        }
    }
    participants
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

fn validate_delivery_unknown_attempt(
    item: &OutboxItem,
    account_id: &str,
    expected_attempts: u32,
) -> Result<()> {
    if item.account_id != account_id {
        return Err(MailError::NotFound {
            entity: "outbox item",
            id: item.id.clone(),
        });
    }
    if item.status != OutboxStatus::DeliveryUnknown || item.attempts != expected_attempts {
        return Err(MailError::Validation(format!(
            "outbox item '{}' is no longer the reviewed delivery-unknown attempt; refresh before deciding again",
            item.id
        )));
    }
    Ok(())
}

fn reply_subject(subject: &str) -> String {
    let subject = subject.trim();
    if subject
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("re:"))
        || subject.starts_with("回复：")
        || subject.starts_with("回复:")
    {
        subject.to_owned()
    } else {
        format!("Re: {subject}")
    }
}

fn legacy_mine_mail_quoted_text(body: Option<&str>) -> Option<&str> {
    let body = body?;
    let marker = "—— 原邮件 ——";
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        if content.trim() == marker {
            let quoted = body.get(offset + line.len()..)?.trim();
            return (!quoted.is_empty()).then_some(quoted);
        }
        offset += line.len();
    }
    None
}

fn normalize_legacy_reply_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalized_message_id_key(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_ascii_lowercase()
}

fn normalized_reply_subject(subject: &str) -> String {
    let mut subject = subject.trim();
    loop {
        if subject
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("re:"))
        {
            subject = subject[3..].trim_start();
        } else if let Some(value) = subject.strip_prefix("回复：") {
            subject = value.trim_start();
        } else if let Some(value) = subject.strip_prefix("回复:") {
            subject = value.trim_start();
        } else {
            break;
        }
    }
    subject.to_lowercase()
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

fn changed_flags_cursor(
    supports_condstore: bool,
    uid_validity_reset: bool,
    previous_highest_modseq: Option<u64>,
    current_highest_modseq: Option<u64>,
) -> Option<u64> {
    if !supports_condstore || uid_validity_reset {
        return None;
    }
    previous_highest_modseq
        .filter(|previous| *previous > 0)
        .filter(|previous| current_highest_modseq.is_some_and(|current| current >= *previous))
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
                format: Default::default(),
                reply_context: None,
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
        fs, io,
        path::{Path, PathBuf},
        sync::{Arc, Barrier},
        thread,
    };

    use tempfile::tempdir;

    use super::{
        BODY_PREFETCH_PRIORITY_NEIGHBOR, BODY_PREFETCH_PRIORITY_PAGE,
        BODY_PREFETCH_PRIORITY_RECENT, BodyDownloadKey, BodyDownloadOwner, BodyPrefetchQueue,
        DraftReconciliation, INBOX, InboxUidScope, MailBackend, RemoteDraftCandidate,
        RemoteForkPreservation, advance_draft_sync_progress, bounded_body_prefetch_ids,
        changed_flags_cursor, classify_draft_reconciliation, classify_inbox_uid_scope,
        confirmed_created_mailbox_capability, discovered_mailbox_capability,
        draft_record_matches_remote, earlier_history_bound, mailbox_hint_changed,
        normalized_message_page_size, remote_attachment_listing, remote_candidates_equivalent,
        remote_draft_candidate, selected_remote_body_paths, validate_delivery_unknown_attempt,
        validate_manual_retry,
    };
    use crate::{
        AccountConfig, ComposeFormat, ComposeRequest, ContactMessageDirection, Draft,
        DraftDeleteKind, DraftSaveKind, InboxMessage, MailAddress, MailError, MailboxRole,
        OutboxItem, OutboxStatus, ServerConfig, SmtpSecurity, StationeryTheme, SyncReport,
        database::{DraftRecord, MailboxState, Repository},
        imap_client::{
            MailboxHint, MailboxMessageScope, RemoteMailbox, RemoteMessage, RemoteMessageStructure,
            RemoteMimePart, RemoteTransferEncoding,
        },
        mime::{
            MimeSourceCompleteness, build_outgoing_message_with_attachments,
            index_message_attachments, outbox_body_text, outbox_message_id, parse_draft_message,
            prepare_forward_source,
        },
        models::{
            AttachmentDisposition, AttachmentSaveStatus, DraftAttachmentMutationKind,
            ForwardPreparationErrorKind, ForwardPreparationOutcome, ForwardWarning,
            MailboxCapability, MailboxCapabilityStatus, MailboxCapabilityUnavailableReason,
            MutationStatus, SystemFlagKind,
        },
    };

    fn compose(subject: &str, body_text: &str) -> ComposeRequest {
        ComposeRequest {
            to: vec!["receiver@example.com".to_owned()],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: subject.to_owned(),
            body_text: body_text.to_owned(),
            format: Default::default(),
            reply_context: None,
        }
    }

    #[test]
    fn body_prefetch_budget_keeps_order_and_skips_oversized_candidates() {
        let selected = bounded_body_prefetch_ids(
            [
                ("first".to_owned(), 512 * 1024),
                ("oversized".to_owned(), 3 * 1024 * 1024),
                ("second".to_owned(), 1536 * 1024),
                ("over-budget".to_owned(), 1),
            ],
            2 * 1024 * 1024,
            2 * 1024 * 1024,
        );

        assert_eq!(selected, vec!["first", "second"]);
    }

    #[test]
    fn selected_reader_keeps_large_attachment_bytes_out_of_body_paths() {
        let plain = RemoteMimePart {
            path: vec![1],
            mime_type: "text/plain".to_owned(),
            original_name: None,
            disposition: None,
            content_id: None,
            transfer_encoding: RemoteTransferEncoding::QuotedPrintable,
            encoded_size_bytes: 512,
        };
        let attachment = RemoteMimePart {
            path: vec![2],
            mime_type: "application/zip".to_owned(),
            original_name: Some("large.zip".to_owned()),
            disposition: Some("attachment".to_owned()),
            content_id: None,
            transfer_encoding: RemoteTransferEncoding::Base64,
            encoded_size_bytes: 40 * 1024 * 1024,
        };
        let structure = RemoteMessageStructure {
            uid: 7,
            parts: vec![plain, attachment],
        };

        assert_eq!(
            selected_remote_body_paths(&structure.parts).expect("body paths"),
            [vec![1]]
        );
        let listing = remote_attachment_listing("opaque-message", &structure);
        assert_eq!(listing.len(), 1);
        assert_eq!(listing[0].1, [2]);
        assert_eq!(listing[0].0.safe_display_name, "large.zip");
        assert_eq!(listing[0].0.disposition, AttachmentDisposition::Attachment);
        assert!(listing[0].0.size_is_estimate);
        assert_eq!(listing[0].0.size_bytes, 30 * 1024 * 1024);
        assert!(!listing[0].0.id.contains("large.zip"));
    }

    #[test]
    fn selected_reader_treats_an_inline_html_root_as_body_content() {
        let html = RemoteMimePart {
            path: vec![1],
            mime_type: "text/html".to_owned(),
            original_name: None,
            disposition: Some("inline".to_owned()),
            content_id: Some("<root@example.com>".to_owned()),
            transfer_encoding: RemoteTransferEncoding::QuotedPrintable,
            encoded_size_bytes: 1_024,
        };
        let inline_image = RemoteMimePart {
            path: vec![2],
            mime_type: "image/png".to_owned(),
            original_name: None,
            disposition: Some("inline".to_owned()),
            content_id: Some("<hero@example.com>".to_owned()),
            transfer_encoding: RemoteTransferEncoding::Base64,
            encoded_size_bytes: 4_096,
        };
        let structure = RemoteMessageStructure {
            uid: 8,
            parts: vec![html, inline_image],
        };

        assert_eq!(
            selected_remote_body_paths(&structure.parts).expect("body paths"),
            [vec![1]]
        );
        let listing = remote_attachment_listing("opaque-message", &structure);
        assert_eq!(listing.len(), 1);
        assert_eq!(listing[0].0.disposition, AttachmentDisposition::Inline);
        assert_eq!(listing[0].1, [2]);
    }

    #[test]
    fn body_prefetch_queue_prioritizes_neighbors_and_drops_stale_pages() {
        let mut queue = BodyPrefetchQueue::default();
        assert!(queue.enqueue("recent".to_owned(), BODY_PREFETCH_PRIORITY_RECENT, 1, None,));
        assert!(queue.enqueue("page".to_owned(), BODY_PREFETCH_PRIORITY_PAGE, 2, Some(7),));
        assert!(queue.enqueue(
            "neighbor".to_owned(),
            BODY_PREFETCH_PRIORITY_NEIGHBOR,
            3,
            Some(7),
        ));
        assert!(queue.enqueue(
            "stale".to_owned(),
            BODY_PREFETCH_PRIORITY_NEIGHBOR,
            4,
            Some(6),
        ));

        assert_eq!(
            queue.pop_next(7).map(|job| job.public_id),
            Some("neighbor".to_owned())
        );
        assert_eq!(
            queue.pop_next(7).map(|job| job.public_id),
            Some("page".to_owned())
        );
        assert_eq!(
            queue.pop_next(7).map(|job| job.public_id),
            Some("recent".to_owned())
        );
        assert!(queue.pop_next(7).is_none());
    }

    #[test]
    fn body_prefetch_queue_promotes_an_existing_page_job() {
        let mut queue = BodyPrefetchQueue::default();
        assert!(queue.enqueue(
            "selected-neighbor".to_owned(),
            BODY_PREFETCH_PRIORITY_PAGE,
            1,
            Some(8),
        ));
        assert!(queue.enqueue(
            "selected-neighbor".to_owned(),
            BODY_PREFETCH_PRIORITY_NEIGHBOR,
            2,
            Some(8),
        ));

        let promoted = queue.pop_next(8).expect("promoted job");
        assert_eq!(promoted.public_id, "selected-neighbor");
        assert_eq!(promoted.priority, BODY_PREFETCH_PRIORITY_NEIGHBOR);
        assert!(queue.pop_next(8).is_none());
    }

    #[tokio::test]
    async fn body_download_waiter_observes_completion_even_before_first_poll() {
        let key = BodyDownloadKey {
            mailbox: INBOX.to_owned(),
            uid: 42,
        };
        let signal = Arc::new(tokio::sync::Semaphore::new(0));
        let downloads = std::sync::Mutex::new(std::collections::HashMap::from([(
            key.clone(),
            signal.clone(),
        )]));
        let waiter = signal.clone().acquire_owned();

        drop(BodyDownloadOwner {
            downloads: &downloads,
            key,
            signal,
        });

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), waiter)
                .await
                .expect("closed download signal must resolve")
                .is_err()
        );
        assert!(downloads.lock().expect("download registry").is_empty());
    }

    fn scoped_account_config(account_id: &str, email: &str) -> AccountConfig {
        AccountConfig::new(
            account_id,
            email,
            "not-a-real-secret",
            ServerConfig {
                host: "imap.example.com".to_owned(),
                port: 993,
            },
            ServerConfig {
                host: "smtp.example.com".to_owned(),
                port: 465,
            },
            SmtpSecurity::ImplicitTls,
        )
        .expect("scoped account config")
    }

    fn managed_blob_path(product_data_root: &Path, internal_name: &str) -> PathBuf {
        let account_directories = fs::read_dir(product_data_root.join("managed-attachments"))
            .expect("managed attachment root")
            .map(|entry| entry.expect("managed account entry").path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        assert_eq!(account_directories.len(), 1);
        account_directories[0].join(internal_name)
    }

    fn downgrade_managed_digest_fixture(
        database_path: &Path,
        account_id: &str,
        blob_id: &str,
        legacy_version: u32,
    ) {
        let connection = rusqlite::Connection::open(database_path).expect("legacy database");
        connection
            .execute_batch(
                "DROP TRIGGER IF EXISTS trg_managed_attachment_blobs_immutable;
                 DROP TRIGGER IF EXISTS trg_managed_attachment_digest_once;",
            )
            .expect("disable current attachment guards");
        assert_eq!(
            connection
                .execute(
                    "UPDATE managed_attachment_blobs
                     SET sha256_hex = NULL
                     WHERE account_id = ?1 AND id = ?2",
                    rusqlite::params![account_id, blob_id],
                )
                .expect("legacy null digest"),
            1
        );
        connection
            .execute_batch(
                "CREATE TRIGGER trg_managed_attachment_blobs_immutable
                 BEFORE UPDATE ON managed_attachment_blobs
                 BEGIN
                     SELECT RAISE(ABORT, 'managed attachment blobs are immutable');
                 END;",
            )
            .expect("legacy immutable trigger");
        connection
            .pragma_update(None, "user_version", legacy_version)
            .expect("legacy schema marker");
    }

    fn stored_managed_digest(
        database_path: &Path,
        account_id: &str,
        blob_id: &str,
    ) -> Option<String> {
        rusqlite::Connection::open(database_path)
            .expect("digest query database")
            .query_row(
                "SELECT sha256_hex
                 FROM managed_attachment_blobs
                 WHERE account_id = ?1 AND id = ?2",
                rusqlite::params![account_id, blob_id],
                |row| row.get(0),
            )
            .expect("stored managed digest")
    }

    #[cfg(unix)]
    fn create_file_link(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_file_link(target: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    fn create_file_link_or_skip(target: &Path, link: &Path) -> bool {
        match create_file_link(target, link) {
            Ok(()) => true,
            #[cfg(windows)]
            Err(error)
                if error.raw_os_error() == Some(1314)
                    || matches!(
                        error.kind(),
                        io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                    ) =>
            {
                false
            }
            Err(error) => panic!("create test reparse link: {error}"),
        }
    }

    #[test]
    fn draft_sync_progress_is_emitted_after_each_persisted_ten_records() {
        let mut completed = 0;
        let mut updates = Vec::new();
        for _ in 0..25 {
            advance_draft_sync_progress(&mut completed, 25, &mut |progress| {
                updates.push((progress.completed, progress.total));
            });
        }

        assert_eq!(updates, vec![(10, 25), (20, 25), (25, 25)]);
    }

    fn remote_mailbox(
        name: &str,
        is_archive: bool,
        is_trash: bool,
        is_selectable: bool,
    ) -> RemoteMailbox {
        RemoteMailbox {
            name: name.to_owned(),
            is_all: false,
            is_drafts: false,
            is_sent: false,
            is_archive,
            is_trash,
            is_selectable,
        }
    }

    #[test]
    fn discovery_never_guesses_an_ordinary_archive_name() {
        let mailboxes = [remote_mailbox("Archive", false, false, true)];
        let capability = discovered_mailbox_capability(MailboxRole::Archive, &mailboxes, false);

        assert_eq!(
            capability.status,
            MailboxCapabilityStatus::NeedsCreationConfirmation
        );
        assert_eq!(capability.display_name, None);
    }

    #[test]
    fn confirmed_create_accepts_only_the_exact_selectable_canonical_name() {
        let ordinary = [remote_mailbox("Archive", false, false, true)];
        let confirmed =
            confirmed_created_mailbox_capability(MailboxRole::Archive, &ordinary, false);
        assert_eq!(confirmed.status, MailboxCapabilityStatus::Available);
        assert_eq!(confirmed.display_name.as_deref(), Some("Archive"));

        let non_selectable = [remote_mailbox("Trash", false, false, false)];
        assert_ne!(
            confirmed_created_mailbox_capability(MailboxRole::Trash, &non_selectable, false).status,
            MailboxCapabilityStatus::Available
        );
    }

    #[test]
    fn role_creation_network_failure_is_typed_persisted_and_role_bounded() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");

        let returned = backend
            .record_mailbox_role_creation_unavailable(
                &backend.config.account_id,
                MailboxRole::Archive,
            )
            .expect("typed failure");
        assert_eq!(returned.status, MailboxCapabilityStatus::Unavailable);
        assert_eq!(
            returned.unavailable_reason,
            Some(MailboxCapabilityUnavailableReason::CreateFailed)
        );
        assert!(returned.retryable);
        assert_eq!(
            backend
                .repository
                .mailbox_capability(&backend.config.account_id, MailboxRole::Archive)
                .expect("persisted capability"),
            Some(returned)
        );

        assert!(matches!(
            backend.record_mailbox_role_creation_unavailable(
                &backend.config.account_id,
                MailboxRole::Inbox,
            ),
            Err(MailError::Validation(_))
        ));
    }

    #[test]
    fn gmail_all_mail_adapter_is_provider_scoped() {
        let mut localized_all_mail = remote_mailbox("[Gmail]/所有邮件", false, false, true);
        localized_all_mail.is_all = true;
        let all_mail = [localized_all_mail];
        assert_eq!(
            discovered_mailbox_capability(MailboxRole::Archive, &all_mail, true).status,
            MailboxCapabilityStatus::Available
        );
        assert_eq!(
            discovered_mailbox_capability(MailboxRole::Archive, &all_mail, false).status,
            MailboxCapabilityStatus::NeedsCreationConfirmation
        );
    }

    #[test]
    fn completed_inbox_sync_is_reused_only_by_an_existing_waiter() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        let waiting_generation = backend.inbox_sync_generation().expect("generation");
        let report = SyncReport {
            mailbox: INBOX.to_owned(),
            remote_total: 12,
            fetched: 2,
            updated_flags: 3,
            removed: 1,
            cached_total: 11,
            uid_validity_reset: false,
        };

        assert!(
            backend
                .inbox_sync_completed_after(waiting_generation)
                .expect("no completion")
                .is_none()
        );
        backend
            .record_inbox_sync_completion(&report)
            .expect("record completion");
        assert_eq!(
            backend
                .inbox_sync_completed_after(waiting_generation)
                .expect("joined completion"),
            Some(report)
        );
        let next_generation = backend.inbox_sync_generation().expect("next generation");
        assert!(
            backend
                .inbox_sync_completed_after(next_generation)
                .expect("new request")
                .is_none()
        );
    }

    #[test]
    fn condstore_cursor_requires_a_current_monotonic_mailbox_epoch() {
        assert_eq!(
            changed_flags_cursor(true, false, Some(100), Some(120)),
            Some(100)
        );
        assert_eq!(
            changed_flags_cursor(false, false, Some(100), Some(120)),
            None
        );
        assert_eq!(changed_flags_cursor(true, true, Some(100), Some(120)), None);
        assert_eq!(changed_flags_cursor(true, false, None, Some(120)), None);
        assert_eq!(changed_flags_cursor(true, false, Some(0), Some(120)), None);
        assert_eq!(
            changed_flags_cursor(true, false, Some(120), Some(100)),
            None
        );
    }

    #[test]
    fn gmail_archive_role_uses_filtered_message_scope_only_for_gmail() {
        let directory = tempdir().expect("tempdir");
        let gmail = AccountConfig::new_oauth2(
            "gmail-account",
            "gmail@example.com",
            "oauth-token",
            ServerConfig {
                host: "imap.gmail.com".to_owned(),
                port: 993,
            },
            ServerConfig {
                host: "smtp.gmail.com".to_owned(),
                port: 465,
            },
            SmtpSecurity::ImplicitTls,
        )
        .expect("gmail config");
        let gmail_backend =
            MailBackend::open(gmail, directory.path().join("gmail.db")).expect("gmail backend");
        assert_eq!(
            gmail_backend.mailbox_message_scope(MailboxRole::Archive),
            MailboxMessageScope::GmailArchive
        );
        assert_eq!(
            gmail_backend.mailbox_message_scope(MailboxRole::Inbox),
            MailboxMessageScope::All
        );

        let custom =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let custom_backend =
            MailBackend::open(custom, directory.path().join("custom.db")).expect("custom backend");
        assert_eq!(
            custom_backend.mailbox_message_scope(MailboxRole::Archive),
            MailboxMessageScope::All
        );
    }

    #[test]
    fn history_bounds_only_move_toward_older_uids_and_page_sizes_are_bounded() {
        assert_eq!(earlier_history_bound(Some(700), Some(1_100)), Some(700));
        assert_eq!(earlier_history_bound(None, Some(51)), Some(51));
        assert_eq!(normalized_message_page_size(0), 50);
        assert_eq!(normalized_message_page_size(100), 100);
        assert_eq!(normalized_message_page_size(500), 100);
    }

    #[test]
    fn discovered_optional_role_is_not_initialized_until_summary_sync_completes() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        backend
            .repository
            .set_mailbox_capability(
                &backend.config.account_id,
                &MailboxCapability {
                    role: MailboxRole::Archive,
                    status: MailboxCapabilityStatus::Available,
                    display_name: Some("Archive".to_owned()),
                    unavailable_reason: None,
                    retryable: false,
                },
            )
            .expect("persist discovered role");

        assert!(
            !backend
                .mailbox_role_initialized(&backend.config.account_id, MailboxRole::Archive)
                .expect("role state")
        );

        backend
            .repository
            .upsert_mailbox_state(&MailboxState {
                account_id: backend.config.account_id.clone(),
                mailbox: "Archive".to_owned(),
                uid_validity: Some(77),
                uid_next: Some(1),
                highest_uid: None,
                highest_modseq: None,
                last_synced_at: Some("2026-07-28T00:00:00Z".to_owned()),
            })
            .expect("complete bounded summary sync");
        assert!(
            backend
                .mailbox_role_initialized(&backend.config.account_id, MailboxRole::Archive)
                .expect("role state")
        );
    }

    #[test]
    fn read_state_returns_a_durable_receipt_and_updates_sqlite_immediately() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        backend
            .repository
            .upsert_mailbox_state(&MailboxState {
                account_id: backend.config.account_id.clone(),
                mailbox: INBOX.to_owned(),
                uid_validity: Some(77),
                uid_next: Some(2),
                highest_uid: Some(1),
                highest_modseq: None,
                last_synced_at: None,
            })
            .unwrap();
        let mut message = cached_message(
            &backend.config.account_id,
            1,
            "<read-state@example.com>",
            "read state",
            "sender@example.com",
            "demo@163.com",
        );
        message.flags.push("\\Seen".to_owned());
        let message_id = backend.repository.upsert_message(&message).unwrap();
        let public_id = public_message_id(&backend, MailboxRole::Inbox, message_id);

        let receipt = backend
            .set_message_seen(&public_id, false)
            .expect("queue unread");
        assert_eq!(receipt.status, MutationStatus::Pending);
        assert_eq!(receipt.flag, SystemFlagKind::Seen);
        assert!(!receipt.desired);
        assert!(
            !backend
                .repository
                .get_message(message_id)
                .unwrap()
                .flags
                .iter()
                .any(|flag| flag.eq_ignore_ascii_case("\\Seen"))
        );

        let repeated = backend
            .set_message_seen(&public_id, false)
            .expect("repeat same desired state");
        assert_eq!(repeated.operation_id, receipt.operation_id);
        assert_eq!(repeated.local_revision, receipt.local_revision);
    }

    #[test]
    fn archive_is_an_immediate_source_hide_and_destination_projection() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        backend
            .repository
            .upsert_mailbox_state(&MailboxState {
                account_id: backend.config.account_id.clone(),
                mailbox: INBOX.to_owned(),
                uid_validity: Some(77),
                uid_next: Some(2),
                highest_uid: Some(1),
                highest_modseq: None,
                last_synced_at: None,
            })
            .unwrap();
        backend
            .repository
            .set_mailbox_capability(
                &backend.config.account_id,
                &MailboxCapability {
                    role: MailboxRole::Archive,
                    status: MailboxCapabilityStatus::Available,
                    display_name: Some("Archive".to_owned()),
                    unavailable_reason: None,
                    retryable: false,
                },
            )
            .unwrap();
        let message = cached_message(
            &backend.config.account_id,
            1,
            "<archive@example.com>",
            "archive",
            "sender@example.com",
            "demo@163.com",
        );
        let message_id = backend.repository.upsert_message(&message).unwrap();
        let public_id = public_message_id(&backend, MailboxRole::Inbox, message_id);

        let receipt = backend.archive_message(&public_id).expect("queue archive");
        assert_eq!(receipt.status, MutationStatus::Pending);
        assert!(
            backend
                .list_mailbox_page(
                    &backend.config.account_id,
                    MailboxRole::Inbox,
                    None,
                    50,
                    None,
                )
                .unwrap()
                .items
                .is_empty()
        );
        let archive = backend
            .list_mailbox_page(
                &backend.config.account_id,
                MailboxRole::Archive,
                None,
                50,
                None,
            )
            .unwrap();
        assert_eq!(archive.items.len(), 1);
        assert_eq!(
            archive.items[0]
                .pending_mutation
                .as_ref()
                .map(|pending| pending.operation_id.as_str()),
            Some(receipt.operation_id.as_str())
        );
    }

    #[tokio::test]
    async fn permanent_delete_requires_a_live_single_use_trash_plan() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        backend
            .repository
            .set_mailbox_capability(
                &backend.config.account_id,
                &MailboxCapability {
                    role: MailboxRole::Trash,
                    status: MailboxCapabilityStatus::Available,
                    display_name: Some("Trash".to_owned()),
                    unavailable_reason: None,
                    retryable: false,
                },
            )
            .unwrap();
        backend
            .repository
            .upsert_mailbox_state(&MailboxState {
                account_id: backend.config.account_id.clone(),
                mailbox: "Trash".to_owned(),
                uid_validity: Some(88),
                uid_next: Some(3),
                highest_uid: Some(2),
                highest_modseq: None,
                last_synced_at: None,
            })
            .unwrap();
        let mut message = cached_message(
            &backend.config.account_id,
            2,
            "<delete@example.com>",
            "delete",
            "sender@example.com",
            "demo@163.com",
        );
        message.mailbox = "Trash".to_owned();
        let message_id = backend.repository.upsert_message(&message).unwrap();
        let public_id = public_message_id(&backend, MailboxRole::Trash, message_id);

        let plan = backend
            .prepare_permanent_delete(&public_id)
            .await
            .expect("prepare plan");
        let receipt = backend
            .confirm_permanent_delete(&plan.plan_id)
            .await
            .expect("consume plan");
        assert_eq!(receipt.status, MutationStatus::Pending);
        assert!(
            backend
                .confirm_permanent_delete(&plan.plan_id)
                .await
                .is_err()
        );
    }

    fn cached_message(
        account_id: &str,
        uid: u32,
        message_id: &str,
        subject: &str,
        sender: &str,
        recipient: &str,
    ) -> InboxMessage {
        InboxMessage {
            id: 0,
            account_id: account_id.to_owned(),
            mailbox: INBOX.to_owned(),
            uid,
            message_id: Some(message_id.to_owned()),
            in_reply_to: Vec::new(),
            references: Vec::new(),
            subject: subject.to_owned(),
            sender: Some(MailAddress {
                name: None,
                email: sender.to_owned(),
            }),
            to: vec![MailAddress {
                name: None,
                email: recipient.to_owned(),
            }],
            cc: Vec::new(),
            bcc: Vec::new(),
            sent_at: Some(format!("2026-07-20T01:45:5{uid}Z")),
            internal_date: None,
            flags: Vec::new(),
            size_bytes: 100,
            preview: subject.to_owned(),
            body_text: Some(format!("body {uid}")),
            body_html: None,
            attachment_names: Vec::new(),
            body_fetched: true,
            raw_rfc822: Vec::new(),
            synced_at: "2026-07-20T02:00:00Z".to_owned(),
        }
    }

    fn public_message_id(backend: &MailBackend, role: MailboxRole, row_id: i64) -> String {
        backend
            .list_mailbox_page(&backend.config.account_id, role, None, 100, None)
            .expect("message page")
            .items
            .into_iter()
            .find(|item| item.message.id == row_id)
            .expect("public message identity")
            .public_id
    }

    fn cache_complete_raw_message(backend: &MailBackend, uid: u32, raw_rfc822: Vec<u8>) -> String {
        backend
            .repository
            .upsert_mailbox_state(&MailboxState {
                account_id: backend.config.account_id.clone(),
                mailbox: INBOX.to_owned(),
                uid_validity: Some(77),
                uid_next: Some(uid.saturating_add(1)),
                highest_uid: Some(uid),
                highest_modseq: None,
                last_synced_at: Some("2026-07-28T00:00:00Z".to_owned()),
            })
            .expect("mailbox state");
        let mut message = cached_message(
            &backend.config.account_id,
            uid,
            &format!("<attachment-{uid}@example.com>"),
            "attachment source",
            "sender@example.com",
            &backend.config.email,
        );
        message.size_bytes = u32::try_from(raw_rfc822.len()).expect("bounded test fixture");
        message.raw_rfc822 = raw_rfc822;
        let row_id = backend
            .repository
            .upsert_message(&message)
            .expect("cached message");
        public_message_id(backend, MailboxRole::Inbox, row_id)
    }

    fn one_attachment_message() -> Vec<u8> {
        b"From: Sender <sender@example.com>\r\n\
          To: demo@163.com\r\n\
          Subject: Attachment source\r\n\
          MIME-Version: 1.0\r\n\
          Content-Type: multipart/mixed; boundary=outer\r\n\
          \r\n\
          --outer\r\n\
          Content-Type: text/plain; charset=utf-8\r\n\
          \r\n\
          Complete body first line.\r\nComplete body last line.\r\n\
          --outer\r\n\
          Content-Type: application/octet-stream\r\n\
          Content-Disposition: attachment; filename=report.bin\r\n\
          Content-Transfer-Encoding: base64\r\n\
          \r\n\
          AQIDBA==\r\n\
          --outer--\r\n"
            .to_vec()
    }

    fn two_attachment_message() -> Vec<u8> {
        b"From: sender@example.com\r\n\
          To: demo@163.com\r\n\
          Subject: Two attachments\r\n\
          MIME-Version: 1.0\r\n\
          Content-Type: multipart/mixed; boundary=outer\r\n\
          \r\n\
          --outer\r\n\
          Content-Type: text/plain\r\n\
          \r\n\
          Complete body.\r\n\
          --outer\r\n\
          Content-Type: application/octet-stream\r\n\
          Content-Disposition: attachment; filename=first.bin\r\n\
          Content-Transfer-Encoding: base64\r\n\
          \r\n\
          AQID\r\n\
          --outer\r\n\
          Content-Type: application/octet-stream\r\n\
          Content-Disposition: attachment; filename=second.bin\r\n\
          Content-Transfer-Encoding: base64\r\n\
          \r\n\
          BAUG\r\n\
          --outer--\r\n"
            .to_vec()
    }

    fn broken_attachment_message() -> Vec<u8> {
        b"From: Original <sender@example.com>\r\n\
          To: demo@163.com\r\n\
          Bcc: hidden@example.com\r\n\
          Subject: Broken attachment\r\n\
          MIME-Version: 1.0\r\n\
          Content-Type: multipart/mixed; boundary=outer\r\n\
          \r\n\
          --outer\r\n\
          Content-Type: text/plain; charset=utf-8\r\n\
          \r\n\
          Complete body survives.\r\n\
          --outer\r\n\
          Content-Type: application/octet-stream\r\n\
          Content-Disposition: attachment; filename=broken.bin\r\n\
          Content-Transfer-Encoding: base64\r\n\
          \r\n\
          this is not base64 !!!\r\n\
          --outer--\r\n"
            .to_vec()
    }

    #[test]
    fn opaque_message_identity_is_rejected_by_a_different_account_backend() {
        let directory = tempdir().expect("tempdir");
        let first_config = scoped_account_config("stable-first", "first@example.com");
        let second_config = scoped_account_config("stable-second", "second@example.com");
        let first = MailBackend::open(first_config, directory.path().join("first.db"))
            .expect("first backend");
        let second = MailBackend::open(second_config, directory.path().join("second.db"))
            .expect("second backend");
        first.initialize().expect("first initialize");
        second.initialize().expect("second initialize");
        let first_public_id = cache_complete_raw_message(&first, 1, one_attachment_message());
        cache_complete_raw_message(&second, 1, one_attachment_message());

        assert!(matches!(
            second.cached_message_by_id(&first_public_id),
            Err(MailError::NotFound { .. })
        ));
    }

    #[test]
    fn cached_object_public_id_conversion_is_account_scoped_despite_rowid_collision() {
        let directory = tempdir().expect("tempdir");
        let first = MailBackend::open(
            scoped_account_config("stable-first", "first@example.com"),
            directory.path().join("first.db"),
        )
        .expect("first backend");
        let second = MailBackend::open(
            scoped_account_config("stable-second", "second@example.com"),
            directory.path().join("second.db"),
        )
        .expect("second backend");
        first.initialize().expect("first initialize");
        second.initialize().expect("second initialize");
        let first_public_id = cache_complete_raw_message(&first, 1, one_attachment_message());
        let second_public_id = cache_complete_raw_message(&second, 1, one_attachment_message());
        let first_message = first
            .cached_message_by_id(&first_public_id)
            .expect("first cached message");
        let second_message = second
            .cached_message_by_id(&second_public_id)
            .expect("second cached message");
        assert_eq!(first_message.id, second_message.id);

        assert_eq!(
            first
                .public_id_for_cached_message(&first_message)
                .expect("general cached-object conversion"),
            first_public_id
        );
        assert_eq!(
            first
                .public_id_for_cached_inbox_message(&first_message)
                .expect("Inbox notification conversion"),
            first_public_id
        );
        assert!(matches!(
            first.public_id_for_cached_message(&second_message),
            Err(MailError::NotFound { .. })
        ));
        assert!(matches!(
            first.public_id_for_cached_inbox_message(&second_message),
            Err(MailError::NotFound { .. })
        ));

        let mut archived = first_message;
        archived.id = 0;
        archived.mailbox = "Archive".to_owned();
        archived.uid = 2;
        archived.message_id = Some("archived@example.com".to_owned());
        let archived_row_id = first.repository.upsert_message(&archived).unwrap();
        let archived = first.repository.get_message(archived_row_id).unwrap();
        let archived_public_id = first
            .repository
            .message_public_id_by_local_id(&first.config.account_id, archived_row_id)
            .unwrap();
        assert_eq!(
            first.public_id_for_cached_message(&archived).unwrap(),
            archived_public_id
        );
        assert!(first.public_id_for_cached_inbox_message(&archived).is_err());
    }

    #[test]
    fn account_attachment_cleanup_is_physically_isolated_in_a_shared_parent() {
        let directory = tempdir().expect("tempdir");
        let selected_directory = tempdir().expect("selected files");
        let first_selected = selected_directory.path().join("first.txt");
        let second_selected = selected_directory.path().join("second.txt");
        fs::write(&first_selected, b"first account bytes").unwrap();
        fs::write(&second_selected, b"second account bytes").unwrap();
        let first_database = directory.path().join("first.db");
        let second_database = directory.path().join("second.db");
        let first = MailBackend::open(
            scoped_account_config("stable-first", "first@example.com"),
            &first_database,
        )
        .expect("first backend");
        let second = MailBackend::open(
            scoped_account_config("stable-second", "second@example.com"),
            &second_database,
        )
        .expect("second backend");
        first.initialize().unwrap();
        second.initialize().unwrap();
        let first_draft = first.save_draft(compose("first", "body")).unwrap();
        let second_draft = second.save_draft(compose("second", "body")).unwrap();
        let first_attached = first
            .add_draft_attachments(
                &first_draft.id,
                first_draft.local_version,
                &[first_selected],
            )
            .unwrap();
        let second_attached = second
            .add_draft_attachments(
                &second_draft.id,
                second_draft.local_version,
                &[second_selected],
            )
            .unwrap();
        assert_eq!(
            first
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            second
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .len(),
            1
        );

        first.cleanup_managed_attachments().unwrap();
        assert_eq!(
            second
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .len(),
            1
        );
        let restarted_second = MailBackend::open(
            scoped_account_config("stable-second", "second@example.com"),
            &second_database,
        )
        .expect("second restart");
        restarted_second.initialize().unwrap();
        assert_eq!(
            first
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            restarted_second
                .confirmed_draft_snapshot(
                    &second_draft.id,
                    second_attached.draft.draft.local_version,
                    &["receiver@example.com".to_owned()],
                )
                .unwrap()
                .attachments[0]
                .bytes,
            b"second account bytes"
        );
        assert_eq!(
            first
                .confirmed_draft_snapshot(
                    &first_draft.id,
                    first_attached.draft.draft.local_version,
                    &["receiver@example.com".to_owned()],
                )
                .unwrap()
                .attachments[0]
                .bytes,
            b"first account bytes"
        );
    }

    #[test]
    fn legacy_null_digest_is_backfilled_before_send_and_reused_after_restart() {
        let directory = tempdir().expect("tempdir");
        let selected_directory = tempdir().expect("selected file");
        let selected = selected_directory.path().join("legacy.txt");
        let original_bytes = b"legacy attachment bytes";
        fs::write(&selected, original_bytes).expect("selected attachment");
        let database_path = directory.path().join("mail.db");
        let backend = MailBackend::open(
            scoped_account_config("legacy-digest", "legacy@example.com"),
            &database_path,
        )
        .expect("backend");
        backend.initialize().expect("initialize");
        let draft = backend
            .save_draft(compose("legacy digest", "body"))
            .expect("draft");
        let attached = backend
            .add_draft_attachments(&draft.id, draft.local_version, &[selected])
            .expect("attachment")
            .draft;
        let attachment = backend
            .repository
            .list_draft_attachments_at_version(
                &backend.config.account_id,
                &draft.id,
                attached.draft.local_version,
            )
            .expect("attachment query")
            .expect("exact version")
            .attachments
            .into_iter()
            .next()
            .expect("managed attachment");
        let blob_path = managed_blob_path(directory.path(), &attachment.internal_name);
        downgrade_managed_digest_fixture(
            &database_path,
            &backend.config.account_id,
            &attachment.meta.id,
            14,
        );
        drop(backend);

        let upgraded = MailBackend::open(
            scoped_account_config("legacy-digest", "legacy@example.com"),
            &database_path,
        )
        .expect("upgrade backend");
        upgraded.initialize().expect("upgrade initialize");
        assert_eq!(
            upgraded
                .confirmed_draft_snapshot(
                    &draft.id,
                    attached.draft.local_version,
                    &["receiver@example.com".to_owned()],
                )
                .expect("safe legacy attachment backfill")
                .attachments[0]
                .bytes,
            original_bytes
        );
        let persisted_digest = stored_managed_digest(
            &database_path,
            &upgraded.config.account_id,
            &attachment.meta.id,
        )
        .expect("backfilled digest");
        assert_eq!(persisted_digest.len(), 64);
        drop(upgraded);

        let restarted = MailBackend::open(
            scoped_account_config("legacy-digest", "legacy@example.com"),
            &database_path,
        )
        .expect("restart backend");
        restarted.initialize().expect("restart initialize");
        assert_eq!(
            restarted
                .confirmed_draft_snapshot(
                    &draft.id,
                    attached.draft.local_version,
                    &["receiver@example.com".to_owned()],
                )
                .expect("persisted digest read")
                .attachments[0]
                .bytes,
            original_bytes
        );
        drop(restarted);

        fs::write(&blob_path, vec![b'x'; original_bytes.len()])
            .expect("equal-length local replacement");
        let tampered = MailBackend::open(
            scoped_account_config("legacy-digest", "legacy@example.com"),
            &database_path,
        )
        .expect("tampered restart");
        tampered.initialize().expect("tampered initialize");
        assert!(matches!(
            tampered.confirmed_draft_snapshot(
                &draft.id,
                attached.draft.local_version,
                &["receiver@example.com".to_owned()],
            ),
            Err(MailError::Validation(message))
                if message.contains("immutable content check")
        ));
        assert_eq!(
            stored_managed_digest(
                &database_path,
                &tampered.config.account_id,
                &attachment.meta.id,
            )
            .as_deref(),
            Some(persisted_digest.as_str())
        );
    }

    #[test]
    fn null_digest_missing_blob_is_rejected_without_database_backfill() {
        let directory = tempdir().expect("tempdir");
        let selected_directory = tempdir().expect("selected file");
        let selected = selected_directory.path().join("missing.txt");
        fs::write(&selected, b"missing after migration").expect("selected attachment");
        let database_path = directory.path().join("mail.db");
        let backend = MailBackend::open(
            scoped_account_config("missing-digest", "missing@example.com"),
            &database_path,
        )
        .expect("backend");
        backend.initialize().expect("initialize");
        let draft = backend
            .save_draft(compose("missing", "body"))
            .expect("draft");
        let attached = backend
            .add_draft_attachments(&draft.id, draft.local_version, &[selected])
            .expect("attachment")
            .draft;
        let attachment = backend
            .repository
            .list_draft_attachments_at_version(
                &backend.config.account_id,
                &draft.id,
                attached.draft.local_version,
            )
            .unwrap()
            .unwrap()
            .attachments[0]
            .clone();
        let blob_path = managed_blob_path(directory.path(), &attachment.internal_name);
        downgrade_managed_digest_fixture(
            &database_path,
            &backend.config.account_id,
            &attachment.meta.id,
            14,
        );
        fs::remove_file(blob_path).expect("remove legacy blob");
        drop(backend);

        let restarted = MailBackend::open(
            scoped_account_config("missing-digest", "missing@example.com"),
            &database_path,
        )
        .expect("restart backend");
        assert!(matches!(
            restarted.confirmed_draft_snapshot(
                &draft.id,
                attached.draft.local_version,
                &["receiver@example.com".to_owned()],
            ),
            Err(MailError::Validation(message))
                if message.contains("immutable content check")
        ));
        assert_eq!(
            stored_managed_digest(
                &database_path,
                &restarted.config.account_id,
                &attachment.meta.id,
            ),
            None
        );
    }

    #[test]
    fn null_digest_reparse_blob_is_rejected_without_reading_external_target() {
        let directory = tempdir().expect("tempdir");
        let selected_directory = tempdir().expect("selected file");
        let selected = selected_directory.path().join("linked.txt");
        let bytes = b"linked legacy bytes";
        fs::write(&selected, bytes).expect("selected attachment");
        let database_path = directory.path().join("mail.db");
        let backend = MailBackend::open(
            scoped_account_config("linked-digest", "linked@example.com"),
            &database_path,
        )
        .expect("backend");
        backend.initialize().expect("initialize");
        let draft = backend
            .save_draft(compose("linked", "body"))
            .expect("draft");
        let attached = backend
            .add_draft_attachments(&draft.id, draft.local_version, &[selected])
            .expect("attachment")
            .draft;
        let attachment = backend
            .repository
            .list_draft_attachments_at_version(
                &backend.config.account_id,
                &draft.id,
                attached.draft.local_version,
            )
            .unwrap()
            .unwrap()
            .attachments[0]
            .clone();
        let blob_path = managed_blob_path(directory.path(), &attachment.internal_name);
        let external_directory = tempdir().expect("external directory");
        let external = external_directory.path().join("external.bin");
        fs::write(&external, bytes).expect("external target");
        downgrade_managed_digest_fixture(
            &database_path,
            &backend.config.account_id,
            &attachment.meta.id,
            14,
        );
        fs::remove_file(&blob_path).expect("remove managed blob");
        if !create_file_link_or_skip(&external, &blob_path) {
            return;
        }
        drop(backend);

        let restarted = MailBackend::open(
            scoped_account_config("linked-digest", "linked@example.com"),
            &database_path,
        )
        .expect("restart backend");
        assert!(matches!(
            restarted.confirmed_draft_snapshot(
                &draft.id,
                attached.draft.local_version,
                &["receiver@example.com".to_owned()],
            ),
            Err(MailError::Validation(message))
                if message.contains("immutable content check")
        ));
        assert_eq!(fs::read(&external).expect("external target remains"), bytes);
        assert_eq!(
            stored_managed_digest(
                &database_path,
                &restarted.config.account_id,
                &attachment.meta.id,
            ),
            None
        );
    }

    #[test]
    fn deleting_managed_data_removes_only_the_selected_account_directory() {
        let directory = tempdir().expect("tempdir");
        let first = MailBackend::open(
            scoped_account_config("stable-first", "first@example.com"),
            directory.path().join("first.db"),
        )
        .unwrap();
        let second = MailBackend::open(
            scoped_account_config("stable-second", "second@example.com"),
            directory.path().join("second.db"),
        )
        .unwrap();
        first.initialize().unwrap();
        second.initialize().unwrap();
        first
            .managed_attachments
            .import_bytes(b"first", "first.bin", "application/octet-stream")
            .unwrap();
        second
            .managed_attachments
            .import_bytes(b"second", "second.bin", "application/octet-stream")
            .unwrap();

        assert!(first.delete_managed_attachment_data().unwrap());
        assert_eq!(
            second
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn attachment_save_as_cancels_and_never_overwrites_an_existing_file() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let public_id = cache_complete_raw_message(&backend, 1, one_attachment_message());
        let metadata = backend
            .cached_message_attachments(&public_id)
            .expect("attachment metadata");
        assert_eq!(metadata.len(), 1);

        let canceled = backend
            .save_message_attachment_to(&public_id, &metadata[0].id, None)
            .await
            .expect("cancel");
        assert_eq!(canceled.status, AttachmentSaveStatus::Canceled);
        assert_eq!(canceled.file_name, None);

        let destination_directory = tempdir().expect("destination");
        let requested = destination_directory.path().join("report.bin");
        fs::write(&requested, b"existing").expect("existing destination");
        let saved = backend
            .save_message_attachment_to(&public_id, &metadata[0].id, Some(&requested))
            .await
            .expect("save result");
        assert_eq!(saved.status, AttachmentSaveStatus::Saved);
        assert_eq!(saved.file_name.as_deref(), Some("report (1).bin"));
        assert_eq!(fs::read(&requested).unwrap(), b"existing");
        assert_eq!(
            fs::read(destination_directory.path().join(saved.file_name.unwrap())).unwrap(),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn forward_staging_limit_rolls_back_prior_blobs_before_any_outbox_item() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let raw = two_attachment_message();
        let source = prepare_forward_source(&raw, MimeSourceCompleteness::CompleteRfc822)
            .expect("forward source");
        assert_eq!(source.ordinary_attachments.len(), 2);

        let failure = match backend.stage_forward_attachments(&raw, &source.ordinary_attachments, 3)
        {
            Ok(_) => panic!("second attachment should exceed the injected total"),
            Err(error) => error,
        };
        assert_eq!(
            failure.kind,
            ForwardPreparationErrorKind::AttachmentStageFailed
        );
        assert_eq!(
            failure.failed_attachment_ids,
            [source.ordinary_attachments[1].id.clone()]
        );
        assert!(failure.retry_without_attachments_allowed);
        assert!(
            backend
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .is_empty()
        );
        assert!(backend.list_outbox().unwrap().is_empty());
    }

    #[tokio::test]
    async fn malformed_attachment_can_retry_as_a_complete_body_only_forward() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let public_id = cache_complete_raw_message(&backend, 1, broken_attachment_message());

        let default = backend
            .prepare_forward(&public_id, true)
            .await
            .expect("typed default outcome");
        match default {
            ForwardPreparationOutcome::Error { error } => {
                assert_eq!(
                    error.kind,
                    ForwardPreparationErrorKind::AttachmentUnavailable
                );
                assert!(error.retry_without_attachments_allowed);
            }
            other => panic!("expected attachment error, got {other:?}"),
        }
        assert!(backend.list_drafts().unwrap().is_empty());
        assert!(
            backend
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .is_empty()
        );

        let body_only = backend
            .prepare_forward(&public_id, false)
            .await
            .expect("body-only retry");
        let prepared = match body_only {
            ForwardPreparationOutcome::Prepared { prepared } => prepared,
            other => panic!("expected prepared forward, got {other:?}"),
        };
        assert!(
            prepared
                .warnings
                .contains(&ForwardWarning::AttachmentsOmittedByUser)
        );
        assert!(prepared.draft.attachments.is_empty());
        let context = prepared
            .draft
            .forward_context
            .as_ref()
            .expect("immutable forward context");
        assert_eq!(context.quoted_text.trim(), "Complete body survives.");
        assert!(context.source_attachments.is_empty());
        assert!(
            context
                .from
                .iter()
                .chain(context.to.iter())
                .chain(context.cc.iter())
                .all(|address| address.email != "hidden@example.com")
        );
        let raw = &backend
            .repository
            .get_draft_record(&prepared.draft.draft.id)
            .expect("persisted forward")
            .draft
            .raw_rfc822;
        assert!(
            outbox_body_text(raw)
                .as_deref()
                .is_some_and(|body| body.contains("Complete body survives."))
        );
        assert!(!String::from_utf8_lossy(raw).contains("hidden@example.com"));
    }

    #[tokio::test]
    async fn explicit_body_only_forward_always_reports_the_omission_choice() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let raw = b"From: sender@example.com\r\n\
                    To: demo@163.com\r\n\
                    Subject: Plain source\r\n\
                    Content-Type: text/plain; charset=utf-8\r\n\
                    \r\n\
                    Complete plain body."
            .to_vec();
        let public_id = cache_complete_raw_message(&backend, 1, raw);

        let prepared = match backend.prepare_forward(&public_id, false).await.unwrap() {
            ForwardPreparationOutcome::Prepared { prepared } => prepared,
            other => panic!("expected prepared forward, got {other:?}"),
        };
        assert!(
            prepared
                .warnings
                .contains(&ForwardWarning::AttachmentsOmittedByUser)
        );
    }

    #[test]
    fn first_attachment_preserves_input_saved_before_the_picker_opens() {
        let directory = tempdir().expect("tempdir");
        let selected_directory = tempdir().expect("selected file");
        let selected = selected_directory.path().join("note.txt");
        fs::write(&selected, b"attachment").unwrap();
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let mut request = compose("unsaved subject", "unsaved complete body");
        request.cc = vec!["copy@example.com".to_owned()];
        request.bcc = vec!["hidden@example.com".to_owned()];

        let stable = backend.create_compose_draft().expect("empty stable draft");
        assert_eq!(stable.draft.local_version, 1);
        let saved_input = backend
            .save_draft_optimistic(
                Some(&stable.draft.id),
                Some(stable.draft.local_version),
                request.clone(),
            )
            .expect("persist current editor before picker");
        assert_eq!(saved_input.kind, DraftSaveKind::Saved);
        assert_eq!(saved_input.draft.compose_request(), request);
        let attached = backend
            .add_draft_attachments(
                &stable.draft.id,
                saved_input.draft.local_version,
                &[selected],
            )
            .expect("first attachment");

        assert_eq!(attached.kind, DraftAttachmentMutationKind::Saved);
        assert_eq!(attached.draft.draft.compose_request(), request);
        assert_eq!(attached.draft.attachments.len(), 1);
        let persisted = backend
            .repository
            .get_draft_record(&stable.draft.id)
            .expect("persisted exact draft");
        assert_eq!(persisted.draft.compose_request(), request);
        assert!(
            outbox_body_text(&persisted.draft.raw_rfc822)
                .as_deref()
                .is_some_and(|body| body == "unsaved complete body")
        );
    }

    #[test]
    fn stale_attachment_add_creates_an_exact_conflict_set_without_mutating_canonical() {
        let directory = tempdir().expect("tempdir");
        let selected_directory = tempdir().expect("selected files");
        let first = selected_directory.path().join("first.txt");
        let second = selected_directory.path().join("second.txt");
        fs::write(&first, b"first attachment").unwrap();
        fs::write(&second, b"second attachment").unwrap();
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().unwrap();
        let draft = backend.save_draft(compose("base", "base body")).unwrap();
        let first_version = backend
            .add_draft_attachments(&draft.id, draft.local_version, &[first])
            .unwrap()
            .draft;
        let canonical = backend
            .save_draft_optimistic(
                Some(&draft.id),
                Some(first_version.draft.local_version),
                compose("canonical", "newer body"),
            )
            .unwrap()
            .draft;

        let conflict = backend
            .add_draft_attachments(&draft.id, first_version.draft.local_version, &[second])
            .expect("stale add preserves bytes");

        assert_eq!(conflict.kind, DraftAttachmentMutationKind::ConflictCopy);
        assert_eq!(conflict.draft.attachments.len(), 2);
        assert_eq!(
            conflict
                .canonical
                .as_ref()
                .map(|draft| draft.draft.local_version),
            Some(canonical.local_version)
        );
        assert_eq!(
            backend.draft_dto(&draft.id).unwrap().attachments.len(),
            1,
            "canonical keeps only its exact attachment set"
        );
        let conflict_raw = backend
            .repository
            .get_draft_record(&conflict.draft.draft.id)
            .unwrap()
            .draft
            .raw_rfc822;
        assert_eq!(
            index_message_attachments(&conflict_raw, MimeSourceCompleteness::CompleteRfc822,)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            backend
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn stale_body_and_attachment_branches_use_the_exact_historical_snapshot() {
        let directory = tempdir().expect("tempdir");
        let selected_directory = tempdir().expect("selected files");
        let attachment_a = selected_directory.path().join("A.txt");
        let attachment_b = selected_directory.path().join("B.txt");
        let attachment_c = selected_directory.path().join("C.txt");
        fs::write(&attachment_a, b"attachment A").unwrap();
        fs::write(&attachment_b, b"attachment B").unwrap();
        fs::write(&attachment_c, b"attachment C").unwrap();
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");

        let created = backend
            .save_draft(compose("version one", "body one"))
            .expect("create version one");
        let version_with_a = backend
            .add_draft_attachments(
                &created.id,
                created.local_version,
                std::slice::from_ref(&attachment_a),
            )
            .expect("version one attachment A")
            .draft;
        assert_eq!(
            version_with_a
                .attachments
                .iter()
                .map(|attachment| attachment.name.as_str())
                .collect::<Vec<_>>(),
            ["A.txt"]
        );
        let historical_version = version_with_a.draft.local_version;

        let without_a = backend
            .remove_draft_attachment(
                &created.id,
                &version_with_a.attachments[0].id,
                historical_version,
            )
            .expect("remove A from canonical")
            .draft;
        let with_b = backend
            .add_draft_attachments(
                &created.id,
                without_a.draft.local_version,
                std::slice::from_ref(&attachment_b),
            )
            .expect("canonical attachment B")
            .draft;
        let canonical = backend
            .save_draft_optimistic(
                Some(&created.id),
                Some(with_b.draft.local_version),
                compose("version two", "body two"),
            )
            .expect("canonical body two")
            .draft;

        let stale_body = backend
            .save_draft_optimistic(
                Some(&created.id),
                Some(historical_version),
                compose("offline body edit", "caller body"),
            )
            .expect("stale body conflict");
        assert_eq!(stale_body.kind, DraftSaveKind::ConflictCopy);
        assert_eq!(stale_body.draft.body_text, "caller body");
        let stale_body_dto = backend
            .draft_dto(&stale_body.draft.id)
            .expect("stale body DTO");
        assert_eq!(
            stale_body_dto
                .attachments
                .iter()
                .map(|attachment| attachment.name.as_str())
                .collect::<Vec<_>>(),
            ["A.txt"]
        );

        let stale_add = backend
            .add_draft_attachments(
                &created.id,
                historical_version,
                std::slice::from_ref(&attachment_c),
            )
            .expect("stale attachment conflict");
        assert_eq!(stale_add.kind, DraftAttachmentMutationKind::ConflictCopy);
        assert_eq!(stale_add.draft.draft.body_text, "body one");
        assert_eq!(
            stale_add
                .draft
                .attachments
                .iter()
                .map(|attachment| attachment.name.as_str())
                .collect::<Vec<_>>(),
            ["A.txt", "C.txt"]
        );

        let persisted_canonical = backend.draft_dto(&created.id).expect("canonical DTO");
        assert_eq!(
            persisted_canonical.draft.local_version,
            canonical.local_version
        );
        assert_eq!(persisted_canonical.draft.body_text, "body two");
        assert_eq!(
            persisted_canonical
                .attachments
                .iter()
                .map(|attachment| attachment.name.as_str())
                .collect::<Vec<_>>(),
            ["B.txt"]
        );
        let canonical_raw = backend
            .repository
            .get_draft_record(&created.id)
            .expect("canonical record")
            .draft
            .raw_rfc822;
        assert_eq!(
            outbox_body_text(&canonical_raw).as_deref(),
            Some("body two")
        );
        assert_eq!(
            index_message_attachments(&canonical_raw, MimeSourceCompleteness::CompleteRfc822)
                .unwrap()
                .iter()
                .map(|attachment| attachment.safe_display_name.as_str())
                .collect::<Vec<_>>(),
            ["B.txt"]
        );
    }

    #[test]
    fn exact_attachment_remove_rebuilds_mime_and_retains_the_historical_snapshot() {
        let directory = tempdir().expect("tempdir");
        let selected_directory = tempdir().expect("selected files");
        let first = selected_directory.path().join("first.txt");
        let second = selected_directory.path().join("second.txt");
        fs::write(&first, b"first attachment").unwrap();
        fs::write(&second, b"second attachment").unwrap();
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().unwrap();
        let draft = backend.save_draft(compose("draft", "body")).unwrap();
        let attached = backend
            .add_draft_attachments(&draft.id, draft.local_version, &[first, second])
            .unwrap()
            .draft;
        let removed = backend
            .remove_draft_attachment(
                &draft.id,
                &attached.attachments[0].id,
                attached.draft.local_version,
            )
            .unwrap();

        assert_eq!(removed.kind, DraftAttachmentMutationKind::Saved);
        assert_eq!(removed.draft.attachments.len(), 1);
        let persisted = backend.repository.get_draft_record(&draft.id).unwrap();
        assert_eq!(
            index_message_attachments(
                &persisted.draft.raw_rfc822,
                MimeSourceCompleteness::CompleteRfc822,
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            backend
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            backend
                .repository
                .list_draft_attachments_at_version(
                    &backend.config.account_id,
                    &draft.id,
                    attached.draft.local_version,
                )
                .unwrap()
                .unwrap()
                .attachments
                .len(),
            2
        );
    }

    #[test]
    fn unsupported_draft_rejects_attachment_remove_before_any_state_change() {
        let directory = tempdir().expect("tempdir");
        let database_path = directory.path().join("mail.db");
        let selected_directory = tempdir().expect("selected file");
        let selected = selected_directory.path().join("read-only.txt");
        fs::write(&selected, b"immutable attachment").unwrap();
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, &database_path).expect("backend");
        backend.initialize().expect("initialize");
        let draft = backend.save_draft(compose("read only", "body")).unwrap();
        let attached = backend
            .add_draft_attachments(
                &draft.id,
                draft.local_version,
                std::slice::from_ref(&selected),
            )
            .unwrap()
            .draft;
        rusqlite::Connection::open(&database_path)
            .unwrap()
            .execute(
                "UPDATE drafts SET has_unsupported_content = 1 WHERE id = ?1",
                rusqlite::params![draft.id],
            )
            .unwrap();
        let before = backend.repository.get_draft_record(&draft.id).unwrap();
        let refs_before = backend
            .repository
            .list_draft_attachments_at_version(
                &backend.config.account_id,
                &draft.id,
                before.local_version,
            )
            .unwrap()
            .unwrap();

        assert!(matches!(
            backend.remove_draft_attachment(
                &draft.id,
                &attached.attachments[0].id,
                attached.draft.local_version,
            ),
            Err(MailError::Validation(_))
        ));

        let after = backend.repository.get_draft_record(&draft.id).unwrap();
        let refs_after = backend
            .repository
            .list_draft_attachments_at_version(
                &backend.config.account_id,
                &draft.id,
                after.local_version,
            )
            .unwrap()
            .unwrap();
        assert_eq!(after.local_version, before.local_version);
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.draft.raw_rfc822, before.draft.raw_rfc822);
        assert_eq!(refs_after, refs_before);
    }

    #[test]
    fn discarding_a_draft_releases_its_managed_blob() {
        let directory = tempdir().expect("tempdir");
        let selected_directory = tempdir().expect("selected file");
        let selected = selected_directory.path().join("note.txt");
        fs::write(&selected, b"discard me").unwrap();
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let draft = backend.save_draft(compose("draft", "body")).unwrap();
        let attached = backend
            .add_draft_attachments(&draft.id, draft.local_version, &[selected])
            .unwrap();
        assert_eq!(attached.kind, DraftAttachmentMutationKind::Saved);
        assert_eq!(
            backend
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .len(),
            1
        );

        backend.delete_draft(&draft.id).expect("discard draft");

        assert!(backend.list_drafts().unwrap().is_empty());
        assert!(
            backend
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .is_empty()
        );
        assert!(
            backend
                .repository
                .list_orphaned_managed_attachments(&backend.config.account_id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn stale_discard_preserves_the_newer_draft_attachment_version() {
        let directory = tempdir().expect("tempdir");
        let selected_directory = tempdir().expect("selected file");
        let selected = selected_directory.path().join("note.txt");
        fs::write(&selected, b"keep me").unwrap();
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let draft = backend.save_draft(compose("draft", "body")).unwrap();
        let attached = backend
            .add_draft_attachments(&draft.id, draft.local_version, &[selected])
            .unwrap();
        let attached_version = attached.draft.draft.local_version;
        let newer = backend
            .save_draft_optimistic(
                Some(&draft.id),
                Some(attached_version),
                compose("newer", "newer body"),
            )
            .unwrap();
        assert_eq!(newer.kind, DraftSaveKind::Saved);

        assert_eq!(
            backend
                .delete_draft_optimistic(&draft.id, attached_version)
                .unwrap(),
            DraftDeleteKind::Stale
        );
        assert_eq!(backend.draft_dto(&draft.id).unwrap().attachments.len(), 1);
        assert_eq!(
            backend
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn sent_fallback_keeps_exact_blob_until_cached_sent_reconciliation() {
        let directory = tempdir().expect("tempdir");
        let selected_directory = tempdir().expect("selected file");
        let selected = selected_directory.path().join("note.txt");
        fs::write(&selected, b"exact send bytes").unwrap();
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let draft = backend.save_draft(compose("draft", "body")).unwrap();
        let attached = backend
            .add_draft_attachments(&draft.id, draft.local_version, &[selected])
            .unwrap()
            .draft;
        let snapshot = backend
            .confirmed_draft_snapshot(
                &draft.id,
                attached.draft.local_version,
                &["receiver@example.com".to_owned()],
            )
            .expect("confirmed exact draft");
        let outgoing = build_outgoing_message_with_attachments(
            &backend.config.email,
            &snapshot.request,
            snapshot.forward_context.as_ref(),
            snapshot.attachments,
        )
        .expect("exact outgoing MIME");
        let outbox = OutboxItem {
            id: "attachment-outbox".to_owned(),
            account_id: backend.config.account_id.clone(),
            draft_id: Some(snapshot.id.clone()),
            draft_revision: Some(snapshot.revision),
            draft_local_version: Some(snapshot.local_version),
            recipients: outgoing.recipients,
            recipient_groups: Some(crate::OutboxRecipientGroups::from(&snapshot.request)),
            status: OutboxStatus::Retryable,
            attempts: 0,
            last_error: Some("synthetic offline state".to_owned()),
            created_at: "2026-07-28T00:00:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: outgoing.raw_rfc822,
        };
        backend
            .repository
            .enqueue_new_outbox(&outbox)
            .expect("persist exact Outbox");
        assert_eq!(
            backend
                .repository
                .list_outbox_attachments(&backend.config.account_id, &outbox.id)
                .unwrap()
                .len(),
            1
        );

        backend
            .repository
            .finalize_outbox_sent(&outbox.id)
            .expect("mark sent");
        backend.cleanup_managed_attachments().expect("cleanup");

        assert_eq!(
            backend.repository.get_draft(&draft.id).unwrap().status,
            "sent"
        );
        assert!(backend.draft_dto(&draft.id).unwrap().attachments.is_empty());
        assert_eq!(
            backend
                .repository
                .list_outbox_attachments(&backend.config.account_id, &outbox.id)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            backend
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .len(),
            1
        );

        let message_id = outbox_message_id(&outbox.raw_rfc822).expect("outgoing Message-ID");
        let mut cached = cached_message(
            &backend.config.account_id,
            77,
            &message_id,
            "draft",
            &backend.config.email,
            "receiver@example.com",
        );
        cached.mailbox = "Sent".to_owned();
        backend
            .repository
            .upsert_message(&cached)
            .expect("cache provider Sent copy");
        assert_eq!(backend.reconcile_sent_outbox("Sent").unwrap(), 1);
        assert!(backend.repository.get_outbox(&outbox.id).is_err());
        assert!(
            backend
                .managed_attachments
                .list_internal_names()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn runtime_backend_clone_does_not_recover_a_live_sending_attempt() {
        let directory = tempdir().expect("tempdir");
        let database_path = directory.path().join("mail.db");
        let primary_config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(primary_config, &database_path).expect("primary backend");
        backend.initialize().expect("initialize primary");
        let queued = OutboxItem {
            id: "live-send".to_owned(),
            account_id: backend.config.account_id.clone(),
            draft_id: None,
            draft_revision: None,
            draft_local_version: None,
            recipients: vec!["receiver@example.com".to_owned()],
            recipient_groups: Some(crate::OutboxRecipientGroups {
                to: vec!["receiver@example.com".to_owned()],
                cc: Vec::new(),
                bcc: Vec::new(),
            }),
            status: OutboxStatus::Queued,
            attempts: 0,
            last_error: None,
            created_at: "2026-07-28T00:00:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: b"Message-ID: <live-send@example.com>\r\n\r\nBody".to_vec(),
        };
        backend
            .repository
            .enqueue_and_claim_outbox(&queued)
            .expect("claim live SMTP attempt");

        let clone_config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let runtime_clone =
            MailBackend::open(clone_config, &database_path).expect("runtime backend clone");
        runtime_clone
            .initialize_without_outbox_recovery()
            .expect("initialize clone");
        assert_eq!(
            backend.repository.get_outbox(&queued.id).unwrap().status,
            OutboxStatus::Sending
        );
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
                format: Default::default(),
                reply_context: None,
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
                format: ComposeFormat {
                    body_html: Some(
                        "<div onclick=\"bad()\"><strong>local text</strong><script>bad()</script></div>"
                            .to_owned(),
                    ),
                    stationery: StationeryTheme::None,
                    send_stationery: true,
                },
                reply_context: None,
            })
            .expect("save draft");

        assert_eq!(saved.status, "local");
        let html = saved
            .format
            .body_html
            .as_deref()
            .expect("safe authored HTML");
        assert!(html.contains("<strong>local text</strong>"));
        assert!(!html.contains("onclick"));
        assert!(!html.contains("script"));
        assert!(!saved.format.send_stationery);
        assert_eq!(backend.list_drafts().expect("drafts").len(), 1);
    }

    #[test]
    fn contact_activity_is_normalized_deduplicated_and_body_free_across_mailboxes() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");

        let mut incoming = cached_message(
            &backend.config.account_id,
            9,
            "incoming@example.com",
            "Newest subject",
            "Friend@Example.COM",
            "demo@163.com",
        );
        incoming.sender.as_mut().expect("sender").name = Some("Latest Friend".to_owned());
        incoming.cc.push(MailAddress {
            name: Some("Duplicate copy".to_owned()),
            email: "friend@example.com".to_owned(),
        });
        incoming.bcc.push(MailAddress {
            name: Some("Must Stay Hidden".to_owned()),
            email: "blind-contact@example.com".to_owned(),
        });

        let mut outgoing = cached_message(
            &backend.config.account_id,
            8,
            "outgoing@example.com",
            "Older subject",
            "DEMO@163.COM",
            "friend@example.com",
        );
        outgoing.mailbox = "&XfJT0ZAB-".to_owned();
        outgoing.to[0].name = Some("Older Friend".to_owned());
        outgoing.cc.push(MailAddress {
            name: None,
            email: "FRIEND@example.com".to_owned(),
        });
        backend
            .repository
            .assign_mailbox_role(&backend.config.account_id, "sent", &outgoing.mailbox)
            .expect("assign encoded Sent mailbox role");

        backend
            .repository
            .upsert_message(&outgoing)
            .expect("outgoing cache");
        backend
            .repository
            .upsert_message(&incoming)
            .expect("incoming cache");

        let activity = backend.list_contact_activity().expect("contact activity");
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].email, "friend@example.com");
        assert_eq!(activity[0].display_name.as_deref(), Some("Latest Friend"));
        assert_eq!(activity[0].message_count, 2);
        assert_eq!(activity[0].last_subject, "Newest subject");
        assert!(
            backend
                .list_contact_messages("blind-contact@example.com", 10)
                .expect("Bcc-excluded contact lookup")
                .is_empty()
        );

        let messages = backend
            .list_contact_messages(" FRIEND@example.com ", 10)
            .expect("contact messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].direction, ContactMessageDirection::Incoming);
        assert_eq!(messages[0].mailbox_role, Some(MailboxRole::Inbox));
        assert_eq!(messages[1].direction, ContactMessageDirection::Outgoing);
        assert_eq!(messages[1].mailbox_role, Some(MailboxRole::Sent));
        assert_eq!(messages[1].message.mailbox, "&XfJT0ZAB-");
        assert_ne!(messages[0].public_id, messages[1].public_id);
        assert!(messages.iter().all(|item| {
            uuid::Uuid::parse_str(&item.public_id)
                .is_ok_and(|id| id.get_version() == Some(uuid::Version::Random))
        }));
        assert!(messages.iter().all(|item| item.message.body_text.is_none()));
        assert!(messages.iter().all(|item| item.message.body_html.is_none()));
        assert!(
            messages
                .iter()
                .all(|item| item.message.raw_rfc822.is_empty())
        );
        assert_eq!(
            backend
                .list_contact_messages("friend@example.com", 1)
                .expect("bounded contact messages")
                .len(),
            1
        );
    }

    #[test]
    fn cached_contact_message_uses_the_exact_mailbox_uid_pair() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");

        let mut inbox = cached_message(
            &backend.config.account_id,
            42,
            "inbox@example.com",
            "Inbox copy",
            "friend@example.com",
            "demo@163.com",
        );
        inbox.body_text = Some("Inbox body".to_owned());
        let mut archived = inbox.clone();
        archived.id = 0;
        archived.mailbox = "Archive/2026".to_owned();
        archived.message_id = Some("archive@example.com".to_owned());
        archived.subject = "Archived copy".to_owned();
        archived.body_text = Some("Archived body".to_owned());

        backend
            .repository
            .upsert_message(&inbox)
            .expect("inbox cache");
        backend
            .repository
            .upsert_message(&archived)
            .expect("archive cache");

        let selected = backend
            .cached_contact_message("Archive/2026", 42)
            .expect("exact cached contact message");
        assert_eq!(selected.mailbox, "Archive/2026");
        assert_eq!(selected.subject, "Archived copy");
        assert_eq!(selected.body_text.as_deref(), Some("Archived body"));
    }

    #[test]
    fn cached_reply_ancestors_follow_reference_depth_and_keep_missing_slots() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");

        let root = cached_message(
            &backend.config.account_id,
            1,
            "root@example.com",
            "test1",
            "gmail@example.com",
            "demo@163.com",
        );
        let mut parent = cached_message(
            &backend.config.account_id,
            2,
            "parent@example.com",
            "Re: test1",
            "demo@163.com",
            "gmail@example.com",
        );
        parent.in_reply_to = vec!["root@example.com".to_owned()];
        parent.references = vec!["root@example.com".to_owned()];
        backend
            .repository
            .upsert_message(&root)
            .expect("root cache");
        backend
            .repository
            .upsert_message(&parent)
            .expect("parent cache");

        let mut current = cached_message(
            &backend.config.account_id,
            3,
            "current@example.com",
            "Re: test1",
            "gmail@example.com",
            "demo@163.com",
        );
        current.in_reply_to = vec!["parent@example.com".to_owned()];
        current.references = vec![
            "root@example.com".to_owned(),
            "parent@example.com".to_owned(),
        ];

        let ancestors = backend
            .cached_reply_ancestors(&current)
            .expect("ancestor chain");
        assert_eq!(ancestors.len(), 2);
        assert_eq!(
            ancestors[0]
                .as_ref()
                .and_then(|message| message.message_id.as_deref()),
            Some("parent@example.com")
        );
        assert_eq!(
            ancestors[1]
                .as_ref()
                .and_then(|message| message.message_id.as_deref()),
            Some("root@example.com")
        );

        current.in_reply_to = vec!["missing-parent@example.com".to_owned()];
        current.references = vec![
            "root@example.com".to_owned(),
            "missing-parent@example.com".to_owned(),
        ];
        let incomplete = backend
            .cached_reply_ancestors(&current)
            .expect("incomplete ancestor chain");
        assert_eq!(incomplete.len(), 2);
        assert!(incomplete[0].is_none());
        assert_eq!(
            incomplete[1]
                .as_ref()
                .and_then(|message| message.message_id.as_deref()),
            Some("root@example.com")
        );
    }

    #[test]
    fn prepares_a_structured_reply_from_the_cached_message() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let message = InboxMessage {
            id: 0,
            account_id: backend.config.account_id.clone(),
            mailbox: INBOX.to_owned(),
            uid: 41,
            message_id: Some("parent@example.com".to_owned()),
            in_reply_to: vec!["root@example.com".to_owned()],
            references: vec!["root@example.com".to_owned()],
            subject: "Earlier note".to_owned(),
            sender: Some(MailAddress {
                name: Some("Sender".to_owned()),
                email: "sender@example.com".to_owned(),
            }),
            to: vec![MailAddress {
                name: None,
                email: "demo@163.com".to_owned(),
            }],
            cc: Vec::new(),
            bcc: vec![MailAddress {
                name: Some("Hidden Recipient".to_owned()),
                email: "hidden@example.com".to_owned(),
            }],
            sent_at: Some("2026-07-17T09:54:29+08:00".to_owned()),
            internal_date: None,
            flags: Vec::new(),
            size_bytes: 100,
            preview: "Original".to_owned(),
            body_text: Some("Complete original body".to_owned()),
            body_html: Some(
                r#"<p>Complete <a href="https://paa.moe">original body</a></p><img alt="avatar" src="data:image/png;base64,AQID">"#
                    .to_owned(),
            ),
            attachment_names: Vec::new(),
            body_fetched: true,
            raw_rfc822: Vec::new(),
            synced_at: "2026-07-17T10:00:00+08:00".to_owned(),
        };
        let row_id = backend
            .repository
            .upsert_message(&message)
            .expect("cache message");
        let public_id = public_message_id(&backend, MailboxRole::Inbox, row_id);

        let reply = backend.prepare_reply(&public_id).expect("prepare reply");

        assert_eq!(reply.to, ["sender@example.com"]);
        assert!(reply.cc.is_empty());
        assert!(reply.bcc.is_empty());
        assert_eq!(reply.subject, "Re: Earlier note");
        assert!(reply.body_text.is_empty());
        let saved = backend
            .save_draft(reply.clone())
            .expect("persist reply draft");
        assert_eq!(
            backend
                .list_drafts()
                .expect("reload reply draft")
                .into_iter()
                .find(|draft| draft.id == saved.id)
                .and_then(|draft| draft.reply_context)
                .map(|context| context.quoted_text),
            Some("Complete original body".to_owned())
        );
        let context = reply.reply_context.expect("reply context");
        assert_eq!(
            context.parent_message_id.as_deref(),
            Some("parent@example.com")
        );
        assert_eq!(context.references, ["root@example.com"]);
        assert_eq!(context.subject, "Earlier note");
        assert_eq!(context.quoted_text, "Complete original body");
        assert!(
            context
                .recipients
                .iter()
                .all(|recipient| recipient.email != "hidden@example.com")
        );
        assert!(
            context
                .quoted_html
                .as_deref()
                .is_some_and(|html| html.contains("https://paa.moe"))
        );

        let mut legacy_reply = message;
        legacy_reply.id = 0;
        legacy_reply.uid = 42;
        legacy_reply.mailbox = "Sent".to_owned();
        legacy_reply.message_id = Some("legacy-reply@example.com".to_owned());
        legacy_reply.in_reply_to.clear();
        legacy_reply.references.clear();
        legacy_reply.subject = "Re: Earlier note".to_owned();
        legacy_reply.sender = Some(MailAddress {
            name: Some("Me".to_owned()),
            email: "demo@163.com".to_owned(),
        });
        legacy_reply.to = vec![MailAddress {
            name: Some("Sender".to_owned()),
            email: "sender@example.com".to_owned(),
        }];
        legacy_reply.body_text =
            Some("Legacy reply\n\n—— 原邮件 ——\nComplete original body".to_owned());
        let legacy_id = backend
            .repository
            .upsert_message(&legacy_reply)
            .expect("cache legacy reply");
        let legacy_reply = backend
            .repository
            .get_message(legacy_id)
            .expect("load legacy reply");
        assert_eq!(
            backend
                .cached_reply_parent(&legacy_reply)
                .expect("resolve legacy parent")
                .and_then(|parent| parent.message_id)
                .as_deref(),
            Some("parent@example.com")
        );
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
            bcc: Vec::new(),
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
    fn existing_plain_draft_keeps_selected_stationery_in_confirmed_send_snapshot() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let created = backend
            .save_draft_optimistic(
                None,
                None,
                compose("plain draft", "first line\nsecond line"),
            )
            .expect("create plain draft")
            .draft;
        assert!(created.format.body_html.is_none());

        let mut themed_request = created.compose_request();
        themed_request.format.stationery = StationeryTheme::Lined;
        themed_request.format.send_stationery = true;
        let themed = backend
            .save_draft_optimistic(
                Some(&created.id),
                Some(created.local_version),
                themed_request,
            )
            .expect("save stationery selection")
            .draft;
        let confirmed = backend
            .confirmed_draft_snapshot(
                &themed.id,
                themed.local_version,
                &["receiver@example.com".to_owned()],
            )
            .expect("confirmed draft snapshot");

        assert_eq!(confirmed.request.format.stationery, StationeryTheme::Lined);
        assert!(confirmed.request.format.send_stationery);
        let outgoing = crate::mime::build_outgoing_message("demo@163.com", &confirmed.request)
            .expect("outgoing stationery message");
        let html = crate::mime::outbox_body_html(&outgoing.raw_rfc822).expect("HTML alternative");
        assert!(html.contains(r#"data-mine-mail-stationery="lined""#));
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
        assert_eq!(stale.draft.subject, "local stale edit（本地冲突副本）");
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
            recipient_groups: None,
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
            recipient_groups: None,
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

    #[test]
    fn delivery_unknown_decision_is_bound_to_status_account_and_attempt_generation() {
        let base = OutboxItem {
            id: "ambiguous-outbox".to_owned(),
            account_id: "primary".to_owned(),
            draft_id: None,
            draft_revision: None,
            draft_local_version: None,
            recipients: vec!["receiver@example.com".to_owned()],
            recipient_groups: None,
            status: OutboxStatus::DeliveryUnknown,
            attempts: 3,
            last_error: Some("delivery state is unknown".to_owned()),
            created_at: "2026-07-14T06:00:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: b"persisted bytes".to_vec(),
        };
        assert!(validate_delivery_unknown_attempt(&base, "primary", 3).is_ok());
        assert!(matches!(
            validate_delivery_unknown_attempt(&base, "another-account", 3),
            Err(MailError::NotFound { .. })
        ));
        assert!(matches!(
            validate_delivery_unknown_attempt(&base, "primary", 2),
            Err(MailError::Validation(_))
        ));
        for status in [
            OutboxStatus::Queued,
            OutboxStatus::Sending,
            OutboxStatus::Sent,
            OutboxStatus::Retryable,
            OutboxStatus::Rejected,
        ] {
            assert!(matches!(
                validate_delivery_unknown_attempt(
                    &OutboxItem {
                        status,
                        ..base.clone()
                    },
                    "primary",
                    3
                ),
                Err(MailError::Validation(_))
            ));
        }
    }

    #[tokio::test]
    async fn delivery_unknown_retry_requires_duplicate_risk_ack_before_any_state_change() {
        let directory = tempdir().expect("tempdir");
        let config =
            AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"]).expect("config");
        let backend = MailBackend::open(config, directory.path().join("mail.db")).expect("backend");
        backend.initialize().expect("initialize");
        let unknown = OutboxItem {
            id: "ambiguous-no-ack".to_owned(),
            account_id: backend.config.account_id.clone(),
            draft_id: None,
            draft_revision: None,
            draft_local_version: None,
            recipients: vec!["receiver@example.com".to_owned()],
            recipient_groups: None,
            status: OutboxStatus::DeliveryUnknown,
            attempts: 1,
            last_error: Some("delivery state is unknown".to_owned()),
            created_at: "2026-07-14T06:00:00Z".to_owned(),
            sent_at: None,
            raw_rfc822:
                b"From: demo@163.com\r\nTo: receiver@example.com\r\n\r\nExact persisted body"
                    .to_vec(),
        };
        backend
            .repository
            .enqueue_outbox(&unknown)
            .expect("unknown Outbox");

        assert!(matches!(
            backend
                .retry_delivery_unknown_once(&unknown.id, 1, false)
                .await,
            Err(MailError::Validation(_))
        ));
        assert_eq!(
            backend.repository.get_outbox(&unknown.id).unwrap(),
            unknown,
            "missing acknowledgement must not claim or rebuild the immutable attempt"
        );
    }
}
