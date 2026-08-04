use std::{
    cmp::Ordering,
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{
    Connection, OptionalExtension, Row, TransactionBehavior, named_params, params, types::Type,
};
use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;

use crate::{
    AccountConfig, ComposeFormat, ComposeRequest, Draft, InboxMessage, MailError, OutboxItem,
    OutboxRecipientGroups, OutboxStatus, Result,
    managed_attachments::ImportedManagedAttachment,
    mime::{build_envelope, draft_has_unsupported_content, reply_message_ids},
    models::{
        AttachmentDisposition, AttachmentMeta, DraftAttachmentMeta, ForwardContext,
        ForwardQuotedRenderMode, MailboxCapability, MailboxCapabilityStatus,
        MailboxCapabilityUnavailableReason, MailboxRole, MessageActionKind,
        MessageMutationErrorKind, MessageMutationReceipt, MessagePage, MessagePageCursor,
        MessagePageItem, MutationStatus, PendingMessageProjection, RemoteHistoryState,
        RemoteMutationPhase, SystemFlagKind,
    },
};

#[cfg(test)]
use crate::models::SystemFlagMutationReceipt;

const MESSAGE_COLUMNS: &str = "id, account_id, mailbox, uid, message_id, in_reply_to_json, \
    references_json, subject, sender_json, to_json, cc_json, sent_at, internal_date, flags_json, \
    size_bytes, preview, body_text, body_html, attachment_names_json, body_fetched, raw_rfc822, \
    synced_at, bcc_json";
// Inbox rows only need enough local body data to paint an immediate fallback.
// The empty HTML sentinel preserves `body_html.is_some()` without reading the
// potentially large HTML/RFC822 payload for every visible list item.
const MESSAGE_SUMMARY_COLUMNS: &str = "id, account_id, mailbox, uid, message_id, in_reply_to_json, \
    references_json, subject, sender_json, to_json, cc_json, sent_at, internal_date, flags_json, \
    size_bytes, preview, body_text, CASE WHEN body_html IS NULL THEN NULL ELSE '' END, \
    attachment_names_json, body_fetched, X'', synced_at, bcc_json";
// Contact history is a header-derived view. Keep the familiar InboxMessage
// shape for the desktop DTO while ensuring a contact-list query cannot carry a
// complete text body, HTML fragment, or RFC822 payload into React.
const CONTACT_MESSAGE_SUMMARY_COLUMNS: &str = "id, account_id, mailbox, uid, message_id, \
    in_reply_to_json, references_json, subject, sender_json, to_json, cc_json, sent_at, \
    internal_date, flags_json, size_bytes, preview, NULL, NULL, attachment_names_json, \
    body_fetched, X'', synced_at, bcc_json";
const DRAFT_COLUMNS: &str = "id, account_id, to_json, cc_json, bcc_json, subject, \
    body_text, compose_format_json, reply_context_json, status, remote_mailbox, remote_uid, \
    created_at, updated_at, raw_rfc822, local_version, has_unsupported_content";
const DRAFT_SYNC_COLUMNS: &str = "id, account_id, to_json, cc_json, bcc_json, subject, \
    body_text, compose_format_json, reply_context_json, status, remote_mailbox, remote_uid, \
    created_at, updated_at, raw_rfc822, local_version, has_unsupported_content, revision, synced_revision, \
    remote_uid_validity, is_deleted";
const DRAFT_VERSION_SNAPSHOT_COLUMNS: &str = "account_id, draft_id, draft_local_version, \
    protocol_revision, to_json, cc_json, bcc_json, subject, body_text, compose_format_json, \
    reply_context_json, has_unsupported_content";
const OUTBOX_COLUMNS: &str = "id, account_id, draft_id, draft_revision, draft_local_version, \
    recipients_json, status, attempts, last_error, created_at, sent_at, raw_rfc822, \
    recipient_groups_json";
const ALIASED_MESSAGE_SUMMARY_COLUMNS: &str = "m.id, m.account_id, m.mailbox, m.uid, \
    m.message_id, m.in_reply_to_json, m.references_json, m.subject, m.sender_json, m.to_json, \
    m.cc_json, m.sent_at, m.internal_date, m.flags_json, m.size_bytes, m.preview, m.body_text, \
    CASE WHEN m.body_html IS NULL THEN NULL ELSE '' END, m.attachment_names_json, \
    m.body_fetched, X'', m.synced_at, m.bcc_json";
const DEFAULT_MESSAGE_PAGE_SIZE: usize = 50;
const MAX_MESSAGE_PAGE_SIZE: usize = 100;
const MAX_CURSOR_TOKEN_BYTES: usize = 64;
const MESSAGE_CURSOR_TTL_SECONDS: i64 = 24 * 60 * 60;
const MAX_SEARCH_QUERY_CHARS: usize = 256;
const MAX_MAILBOX_DISPLAY_CHARS: usize = 1_024;
const MAX_IDENTITY_MESSAGE_ID_CHARS: usize = 1_024;
const MAX_IDENTITY_DATE_CHARS: usize = 128;

/// Persisted IMAP synchronization cursor for one account/mailbox pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MailboxState {
    pub account_id: String,
    pub mailbox: String,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub highest_uid: Option<u32>,
    pub highest_modseq: Option<u64>,
    pub last_synced_at: Option<String>,
}

/// Server-history cursor persisted independently from the newest-message sync
/// cursor. The server is authoritative only when `complete` is true.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MailboxHistory {
    pub before_uid: Option<u32>,
    pub complete: bool,
    pub remote_total: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StarredMailboxHistory {
    pub before_uid: Option<u32>,
    pub complete: bool,
}

/// One durable move/delete intent. It intentionally has no foreign key to
/// `messages`, so UIDVALIDITY reset and cache reconciliation cannot erase an
/// operation that still needs remote reconciliation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingMessageAction {
    pub operation_id: String,
    pub account_id: String,
    pub source_mailbox: String,
    pub source_uid_validity: u32,
    pub source_uid: u32,
    pub source_role: MailboxRole,
    pub destination_role: Option<MailboxRole>,
    pub kind: MessageActionKind,
    pub revision: u64,
    pub status: MutationStatus,
    pub remote_phase: RemoteMutationPhase,
    pub source_message_id: Option<String>,
    pub source_internal_date: Option<String>,
    pub source_size_bytes: u32,
    pub error_kind: Option<MessageMutationErrorKind>,
    /// The provider acknowledged `\Deleted`, but no UIDPLUS-capable command
    /// proved that the source UID vanished from this mailbox epoch.
    pub source_cleanup_pending: bool,
    /// A unique strong-identity destination row has replaced the optimistic
    /// destination projection. This remains separate from source cleanup.
    pub destination_reconciled: bool,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingSystemFlagMutation {
    pub operation_id: String,
    pub account_id: String,
    pub source_mailbox: String,
    pub source_uid_validity: u32,
    pub source_uid: u32,
    pub source_role: MailboxRole,
    pub flag: SystemFlagKind,
    pub desired: bool,
    pub revision: u64,
    pub status: MutationStatus,
    pub error_kind: Option<MessageMutationErrorKind>,
    pub updated_at: String,
}

#[derive(Clone, Debug)]
struct MessageCursorPayload {
    account_id: String,
    mailbox: String,
    role: MailboxRole,
    uid_validity: Option<u32>,
    query_normalized: String,
    sort_at: Option<String>,
    uid: Option<u32>,
    id: Option<i64>,
    remote_before_uid: u32,
    flagged_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MessagePageCursorContext {
    pub account_id: String,
    pub mailbox: String,
    pub role: MailboxRole,
    pub uid_validity: Option<u32>,
    pub remote_before_uid: Option<u32>,
    pub flagged_only: bool,
}

#[derive(Clone, Debug)]
struct PageCandidate {
    item: MessagePageItem,
    sort_at: String,
    uid: u32,
    id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContactMessageSource {
    pub public_id: String,
    pub message: InboxMessage,
}

/// Internal draft row including synchronization metadata. The public `Draft`
/// model stays backwards compatible while the repository retains the base
/// revision needed for deterministic two-way reconciliation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DraftRecord {
    pub draft: Draft,
    pub local_version: u64,
    pub revision: u64,
    pub synced_revision: u64,
    pub remote_uid_validity: Option<u32>,
    pub is_deleted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NewDraftAttachment {
    pub imported: ImportedManagedAttachment,
    pub source_attachment_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedDraftAttachment {
    pub meta: DraftAttachmentMeta,
    pub internal_name: String,
    pub sha256_hex: Option<String>,
    pub disposition: AttachmentDisposition,
    pub transfer_encoding: String,
}

/// Immutable editor-visible state for one exact local draft version. Attachment
/// references and the optional forward-context reference are stored in sibling
/// version-keyed tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DraftVersionSnapshot {
    pub account_id: String,
    pub draft_id: String,
    pub local_version: u64,
    pub protocol_revision: u64,
    pub request: ComposeRequest,
    pub has_unsupported_content: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DraftAttachmentVersionSnapshot {
    pub local_version: u64,
    pub attachments: Vec<ManagedDraftAttachment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OrphanedManagedAttachment {
    pub id: String,
    pub account_id: String,
    pub internal_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreparedForwardInsert {
    Inserted,
    SourceChanged,
    IdCollision,
}

/// A thread-safe repository handle. It contains only a path; short-lived
/// SQLite connections are opened per operation so this value is `Send + Sync`
/// and can safely be managed by Tauri's cross-thread application state.
#[derive(Clone, Debug)]
pub(crate) struct Repository {
    path: PathBuf,
}

impl Repository {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        let repository = Self { path };
        let connection = Connection::open(&repository.path)?;
        configure_connection(&connection)?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS accounts (
                 id TEXT PRIMARY KEY NOT NULL,
                 email TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             );
             CREATE UNIQUE INDEX IF NOT EXISTS idx_accounts_email ON accounts(email);

             CREATE TABLE IF NOT EXISTS mailboxes (
                 account_id TEXT NOT NULL,
                 name TEXT NOT NULL,
                 uid_validity INTEGER,
                 uid_next INTEGER,
                 highest_uid INTEGER,
                 highest_modseq TEXT,
                 last_synced_at TEXT,
                 history_before_uid INTEGER,
                 history_complete INTEGER NOT NULL DEFAULT 0
                     CHECK (history_complete IN (0, 1)),
                 starred_history_before_uid INTEGER,
                 starred_history_complete INTEGER NOT NULL DEFAULT 0
                     CHECK (starred_history_complete IN (0, 1)),
                 remote_total INTEGER,
                 PRIMARY KEY (account_id, name),
                 FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
             );

             CREATE TABLE IF NOT EXISTS mailbox_roles (
                 account_id TEXT NOT NULL,
                 role TEXT NOT NULL,
                 mailbox TEXT NOT NULL,
                 PRIMARY KEY (account_id, role),
                 FOREIGN KEY (account_id, mailbox)
                     REFERENCES mailboxes(account_id, name) ON DELETE CASCADE
             );

             CREATE TABLE IF NOT EXISTS mailbox_capabilities (
                 account_id TEXT NOT NULL,
                 role TEXT NOT NULL CHECK (
                     role IN ('inbox', 'sent', 'drafts', 'archive', 'trash')
                 ),
                 status TEXT NOT NULL CHECK (
                     status IN (
                         'discovery_pending', 'available',
                         'needs_creation_confirmation', 'unavailable'
                     )
                 ),
                 display_name TEXT,
                 unavailable_reason TEXT CHECK (
                     unavailable_reason IS NULL OR unavailable_reason IN (
                         'create_not_supported', 'create_failed',
                         'created_mailbox_not_selectable', 'provider_unsupported'
                     )
                 ),
                 retryable INTEGER NOT NULL DEFAULT 0 CHECK (retryable IN (0, 1)),
                 updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 PRIMARY KEY (account_id, role),
                 FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
             );

             CREATE TABLE IF NOT EXISTS messages (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 public_id TEXT NOT NULL CHECK (length(public_id) = 36),
                 account_id TEXT NOT NULL,
                 mailbox TEXT NOT NULL,
                 uid INTEGER NOT NULL,
                 message_id TEXT,
                 in_reply_to_json TEXT NOT NULL DEFAULT '[]',
                 references_json TEXT NOT NULL DEFAULT '[]',
                 subject TEXT NOT NULL DEFAULT '',
                 sender_json TEXT,
                 to_json TEXT NOT NULL DEFAULT '[]',
                 cc_json TEXT NOT NULL DEFAULT '[]',
                 bcc_json TEXT NOT NULL DEFAULT '[]',
                 sent_at TEXT,
                 internal_date TEXT,
                 flags_json TEXT NOT NULL DEFAULT '[]',
                 size_bytes INTEGER NOT NULL DEFAULT 0,
                 preview TEXT NOT NULL DEFAULT '',
                 preview_fetched INTEGER NOT NULL DEFAULT 0
                     CHECK (preview_fetched IN (0, 1)),
                 body_text TEXT,
                 body_html TEXT,
                 attachment_names_json TEXT NOT NULL DEFAULT '[]',
                 body_fetched INTEGER NOT NULL DEFAULT 0,
                 raw_rfc822 BLOB NOT NULL DEFAULT X'',
                 body_cached_bytes INTEGER NOT NULL DEFAULT 0
                     CHECK (body_cached_bytes >= 0),
                 body_last_accessed_at TEXT,
                 synced_at TEXT NOT NULL,
                 UNIQUE (account_id, mailbox, uid),
                 FOREIGN KEY (account_id, mailbox)
                     REFERENCES mailboxes(account_id, name) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_messages_inbox
                 ON messages(account_id, mailbox, internal_date DESC, uid DESC);
             CREATE INDEX IF NOT EXISTS idx_messages_message_id
                 ON messages(account_id, message_id);

             CREATE TABLE IF NOT EXISTS pending_seen_updates (
                 operation_id TEXT NOT NULL UNIQUE,
                 account_id TEXT NOT NULL,
                 mailbox TEXT NOT NULL,
                 source_uid_validity INTEGER NOT NULL CHECK (source_uid_validity >= 0),
                 uid INTEGER NOT NULL,
                 desired INTEGER NOT NULL DEFAULT 1 CHECK (desired IN (0, 1)),
                 revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
                 status TEXT NOT NULL DEFAULT 'pending' CHECK (
                     status IN (
                         'pending', 'in_flight', 'confirmed',
                         'needs_attention', 'outcome_unknown'
                     )
                 ),
                 error_kind TEXT CHECK (
                     error_kind IS NULL OR error_kind IN (
                         'uid_validity_changed', 'source_missing',
                         'ambiguous_remote_state', 'network_unavailable',
                         'mailbox_unavailable', 'permission_denied',
                         'server_rejected', 'unsupported', 'unknown'
                     )
                 ),
                 updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 PRIMARY KEY (account_id, mailbox, source_uid_validity, uid),
                 FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS pending_flagged_updates (
                 operation_id TEXT NOT NULL UNIQUE,
                 account_id TEXT NOT NULL,
                 mailbox TEXT NOT NULL,
                 source_uid_validity INTEGER NOT NULL CHECK (source_uid_validity >= 0),
                 uid INTEGER NOT NULL,
                 desired INTEGER NOT NULL CHECK (desired IN (0, 1)),
                 revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
                 status TEXT NOT NULL DEFAULT 'pending' CHECK (
                     status IN (
                         'pending', 'in_flight', 'confirmed',
                         'needs_attention', 'outcome_unknown'
                     )
                 ),
                 error_kind TEXT CHECK (
                     error_kind IS NULL OR error_kind IN (
                         'uid_validity_changed', 'source_missing',
                         'ambiguous_remote_state', 'network_unavailable',
                         'mailbox_unavailable', 'permission_denied',
                         'server_rejected', 'unsupported', 'unknown'
                     )
                 ),
                 updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 PRIMARY KEY (account_id, mailbox, source_uid_validity, uid),
                 FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS pending_message_actions (
                 operation_id TEXT PRIMARY KEY NOT NULL,
                 account_id TEXT NOT NULL,
                 source_mailbox TEXT NOT NULL,
                 source_uid_validity INTEGER NOT NULL CHECK (source_uid_validity > 0),
                 source_uid INTEGER NOT NULL CHECK (source_uid > 0),
                 source_role TEXT NOT NULL CHECK (
                     source_role IN ('inbox', 'sent', 'drafts', 'archive', 'trash')
                 ),
                 destination_role TEXT CHECK (
                     destination_role IS NULL OR destination_role IN (
                         'inbox', 'sent', 'drafts', 'archive', 'trash'
                     )
                 ),
                 kind TEXT NOT NULL CHECK (
                     kind IN ('archive', 'move_to_trash', 'permanent_delete')
                 ),
                 revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
                 status TEXT NOT NULL CHECK (
                     status IN (
                         'pending', 'in_flight', 'confirmed',
                         'needs_attention', 'outcome_unknown'
                     )
                 ),
                 remote_phase TEXT NOT NULL DEFAULT 'queued' CHECK (
                     remote_phase IN (
                         'queued', 'transfer_started', 'transfer_acknowledged',
                         'source_delete_started', 'source_delete_acknowledged'
                     )
                 ),
                 source_message_id TEXT,
                 source_internal_date TEXT,
                 source_size_bytes INTEGER NOT NULL DEFAULT 0,
                 error_kind TEXT CHECK (
                     error_kind IS NULL OR error_kind IN (
                         'uid_validity_changed', 'source_missing',
                         'ambiguous_remote_state', 'network_unavailable',
                         'mailbox_unavailable', 'permission_denied',
                         'server_rejected', 'unsupported', 'unknown'
                     )
                 ),
                 source_cleanup_pending INTEGER NOT NULL DEFAULT 0
                     CHECK (source_cleanup_pending IN (0, 1)),
                 destination_reconciled INTEGER NOT NULL DEFAULT 0
                     CHECK (destination_reconciled IN (0, 1)),
                 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 CHECK (
                     (
                         kind = 'archive'
                         AND source_role IN ('inbox', 'sent')
                         AND destination_role = 'archive'
                     ) OR (
                         kind = 'move_to_trash'
                         AND source_role IN ('inbox', 'sent', 'archive')
                         AND destination_role = 'trash'
                     ) OR (
                         kind = 'permanent_delete'
                         AND source_role = 'trash'
                         AND destination_role IS NULL
                     )
                 ),
                 UNIQUE (account_id, source_mailbox, source_uid_validity, source_uid),
                 FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_pending_message_actions_account_status
                 ON pending_message_actions(account_id, status, updated_at);
             CREATE INDEX IF NOT EXISTS idx_pending_message_actions_destination
                 ON pending_message_actions(account_id, destination_role, status);

             CREATE TABLE IF NOT EXISTS message_page_cursors (
                 token TEXT PRIMARY KEY NOT NULL,
                 account_id TEXT NOT NULL,
                 role TEXT NOT NULL CHECK (
                     role IN ('inbox', 'sent', 'drafts', 'archive', 'trash')
                 ),
                 mailbox TEXT NOT NULL,
                 uid_validity INTEGER,
                 sort_at TEXT,
                 uid INTEGER,
                 message_row_id INTEGER,
                 remote_before_uid INTEGER NOT NULL CHECK (remote_before_uid > 0),
                 query_normalized TEXT NOT NULL,
                 flagged_only INTEGER NOT NULL DEFAULT 0
                     CHECK (flagged_only IN (0, 1)),
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 CHECK (
                     (sort_at IS NULL AND uid IS NULL AND message_row_id IS NULL)
                     OR
                     (sort_at IS NOT NULL AND uid > 0 AND message_row_id > 0)
                 ),
                 FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_message_page_cursors_expiry
                 ON message_page_cursors(created_at);

             CREATE TABLE IF NOT EXISTS drafts (
                 id TEXT PRIMARY KEY NOT NULL,
                 account_id TEXT NOT NULL,
                 to_json TEXT NOT NULL DEFAULT '[]',
                 cc_json TEXT NOT NULL DEFAULT '[]',
                 bcc_json TEXT NOT NULL DEFAULT '[]',
                 subject TEXT NOT NULL DEFAULT '',
                 body_text TEXT NOT NULL DEFAULT '',
                 compose_format_json TEXT NOT NULL DEFAULT '{}',
                 reply_context_json TEXT,
                 status TEXT NOT NULL,
                 remote_mailbox TEXT,
                 remote_uid INTEGER,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL,
                 raw_rfc822 BLOB NOT NULL DEFAULT X'',
                 local_version INTEGER NOT NULL DEFAULT 1,
                 has_unsupported_content INTEGER NOT NULL DEFAULT 0,
                 revision INTEGER NOT NULL DEFAULT 1,
                 synced_revision INTEGER NOT NULL DEFAULT 0,
                 remote_uid_validity INTEGER,
                 is_deleted INTEGER NOT NULL DEFAULT 0,
                 FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_drafts_account_updated
                 ON drafts(account_id, updated_at DESC);

             CREATE TABLE IF NOT EXISTS outbox (
                 id TEXT PRIMARY KEY NOT NULL,
                 account_id TEXT NOT NULL,
                 draft_id TEXT,
                 draft_revision INTEGER CHECK (draft_revision IS NULL OR draft_revision > 0),
                 draft_local_version INTEGER CHECK (
                     draft_local_version IS NULL OR draft_local_version > 0
                 ),
                 recipients_json TEXT NOT NULL DEFAULT '[]',
                 recipient_groups_json TEXT,
                 status TEXT NOT NULL CHECK (status IN (
                     'queued', 'sending', 'sent', 'retryable', 'rejected', 'delivery_unknown'
                 )),
                 attempts INTEGER NOT NULL DEFAULT 0,
                 last_error TEXT,
                 created_at TEXT NOT NULL,
                 sent_at TEXT,
                 raw_rfc822 BLOB NOT NULL,
                 FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
                 FOREIGN KEY (draft_id) REFERENCES drafts(id) ON DELETE SET NULL
             );
             CREATE INDEX IF NOT EXISTS idx_outbox_account_status_created
                 ON outbox(account_id, status, created_at);
             ",
        )?;
        migrate_drafts_v2(&connection)?;
        migrate_outbox_v3(&connection)?;
        migrate_drafts_v4(&connection)?;
        migrate_messages_v5(&connection)?;
        migrate_drafts_v7(&connection)?;
        migrate_pending_seen_v8(&connection)?;
        migrate_pending_flagged_v9(&connection)?;
        migrate_message_previews_v10(&connection)?;
        migrate_compose_format_v11(&connection)?;
        migrate_mailboxes_and_mutations_v12(&connection)?;
        migrate_managed_attachments_v13(&connection)?;
        migrate_message_public_ids_v14(&connection)?;
        migrate_immutable_draft_versions_v15(&connection)?;
        migrate_bcc_and_outbox_recipient_groups_v16(&connection)?;
        migrate_managed_attachment_digests_v17(&connection)?;
        migrate_message_body_cache_v18(&connection)?;
        migrate_message_contact_emails_v19(&connection)?;
        migrate_starred_history_v20(&connection)?;
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_drafts_remote_identity
                 ON drafts(account_id, remote_mailbox, remote_uid);
             CREATE INDEX IF NOT EXISTS idx_messages_preview_backfill
                 ON messages(account_id, mailbox, internal_date DESC, uid DESC)
                 WHERE preview_fetched = 0 AND body_fetched = 0;
             CREATE INDEX IF NOT EXISTS idx_messages_body_cache_lru
                 ON messages(account_id, body_last_accessed_at, id)
                 WHERE body_fetched = 1;
             PRAGMA user_version = 20;",
        )?;
        Ok(repository)
    }

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)?;
        configure_connection(&connection)?;
        Ok(connection)
    }

    /// Stores only the stable account id and public email address. The
    /// authorization password is intentionally inaccessible to the SQL layer.
    pub(crate) fn initialize_account(&self, account: &AccountConfig) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing_email: Option<String> = transaction
            .query_row(
                "SELECT email FROM accounts WHERE id = ?1",
                params![account.account_id],
                |row| row.get(0),
            )
            .optional()?;
        if existing_email
            .as_deref()
            .is_some_and(|email| !email.eq_ignore_ascii_case(&account.email))
        {
            return Err(MailError::Config(
                "this database belongs to a different email account; use a separate database file"
                    .to_owned(),
            ));
        }
        transaction.execute(
            "INSERT INTO accounts (id, email) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![account.account_id, account.email],
        )?;
        let stored_email: String = transaction.query_row(
            "SELECT email FROM accounts WHERE id = ?1",
            params![account.account_id],
            |row| row.get(0),
        )?;
        if !stored_email.eq_ignore_ascii_case(&account.email) {
            return Err(MailError::Config(
                "this database belongs to a different email account; use a separate database file"
                    .to_owned(),
            ));
        }
        transaction.execute(
            "INSERT INTO mailboxes (account_id, name) VALUES (?1, 'INBOX')
             ON CONFLICT(account_id, name) DO NOTHING",
            params![account.account_id],
        )?;
        transaction.execute(
            "INSERT INTO mailbox_roles (account_id, role, mailbox)
             VALUES (?1, 'inbox', 'INBOX')
             ON CONFLICT(account_id, role) DO UPDATE SET mailbox = excluded.mailbox",
            params![account.account_id],
        )?;
        for role in MailboxRole::ALL {
            let (status, display_name, retryable) = if role == MailboxRole::Inbox {
                (MailboxCapabilityStatus::Available, Some("INBOX"), false)
            } else {
                (MailboxCapabilityStatus::DiscoveryPending, None, true)
            };
            transaction.execute(
                "INSERT INTO mailbox_capabilities (
                     account_id, role, status, display_name, unavailable_reason, retryable
                 ) VALUES (?1, ?2, ?3, ?4, NULL, ?5)
                 ON CONFLICT(account_id, role) DO NOTHING",
                params![
                    account.account_id,
                    role.as_str(),
                    status.as_str(),
                    display_name,
                    retryable,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn mailbox_state(
        &self,
        account_id: &str,
        mailbox: &str,
    ) -> Result<Option<MailboxState>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT account_id, name, uid_validity, uid_next, highest_uid,
                        highest_modseq, last_synced_at
                 FROM mailboxes WHERE account_id = ?1 AND name = ?2",
                params![account_id, mailbox],
                |row| {
                    Ok(MailboxState {
                        account_id: row.get(0)?,
                        mailbox: row.get(1)?,
                        uid_validity: row.get(2)?,
                        uid_next: row.get(3)?,
                        highest_uid: row.get(4)?,
                        highest_modseq: decode_optional_u64(5, row.get(5)?)?,
                        last_synced_at: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn upsert_mailbox_state(&self, state: &MailboxState) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO mailboxes (
                 account_id, name, uid_validity, uid_next, highest_uid,
                 highest_modseq, last_synced_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(account_id, name) DO UPDATE SET
                 uid_validity = excluded.uid_validity,
                 uid_next = excluded.uid_next,
                 highest_uid = excluded.highest_uid,
                 highest_modseq = excluded.highest_modseq,
                 last_synced_at = excluded.last_synced_at",
            params![
                state.account_id,
                state.mailbox,
                state.uid_validity,
                state.uid_next,
                state.highest_uid,
                state.highest_modseq.map(|value| value.to_string()),
                state.last_synced_at,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn mailbox_history(
        &self,
        account_id: &str,
        mailbox: &str,
    ) -> Result<Option<MailboxHistory>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT history_before_uid, history_complete, remote_total
                 FROM mailboxes
                 WHERE account_id = ?1 AND name = ?2",
                params![account_id, mailbox],
                |row| {
                    Ok(MailboxHistory {
                        before_uid: row.get(0)?,
                        complete: row.get(1)?,
                        remote_total: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn starred_mailbox_history(
        &self,
        account_id: &str,
        mailbox: &str,
    ) -> Result<Option<StarredMailboxHistory>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT starred_history_before_uid, starred_history_complete
                 FROM mailboxes
                 WHERE account_id = ?1 AND name = ?2",
                params![account_id, mailbox],
                |row| {
                    Ok(StarredMailboxHistory {
                        before_uid: row.get(0)?,
                        complete: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    #[cfg(test)]
    pub(crate) fn update_mailbox_history(
        &self,
        account_id: &str,
        mailbox: &str,
        history: &MailboxHistory,
    ) -> Result<()> {
        let state = self
            .mailbox_state(account_id, mailbox)?
            .ok_or_else(|| privacy_safe_not_found("mailbox"))?;
        let uid_validity = state
            .uid_validity
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                MailError::Validation(
                    "the mailbox epoch is not ready for history advancement".to_owned(),
                )
            })?;
        let current = self
            .mailbox_history(account_id, mailbox)?
            .ok_or_else(|| privacy_safe_not_found("mailbox"))?;
        self.advance_mailbox_history(
            account_id,
            mailbox,
            uid_validity,
            current.before_uid,
            history.before_uid,
            history.complete,
            history.remote_total,
        )?;
        Ok(())
    }

    /// Advances the remote-history boundary only if both the mailbox epoch and
    /// the caller's previously observed exclusive UID bound still match.
    pub(crate) fn advance_mailbox_history(
        &self,
        expected_account_id: &str,
        mailbox: &str,
        expected_uid_validity: u32,
        expected_before_uid: Option<u32>,
        next_before_uid: Option<u32>,
        complete: bool,
        remote_total: Option<u32>,
    ) -> Result<bool> {
        validate_history_advance(
            expected_uid_validity,
            expected_before_uid,
            next_before_uid,
            complete,
            "mailbox history",
        )?;
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mailboxes
             SET history_before_uid = CASE
                     WHEN history_complete = 1 THEN history_before_uid
                     ELSE ?5
                 END,
                 history_complete = MAX(history_complete, ?6),
                 remote_total = COALESCE(?7, remote_total)
             WHERE account_id = ?1 AND name = ?2
               AND uid_validity = ?3
               AND history_before_uid IS ?4
               AND (history_complete = 0 OR ?6 = 1)",
            params![
                expected_account_id,
                mailbox,
                expected_uid_validity,
                expected_before_uid,
                next_before_uid,
                complete,
                remote_total,
            ],
        )?;
        Ok(changed == 1)
    }

    /// Reconciles the exclusive history bound from one complete, confirmed UID
    /// snapshot. Unlike page advancement, this may move toward a higher UID
    /// when date-ordered initial caching proves that newer UID positions were
    /// intentionally skipped. Epoch and prior-bound CAS still reject stale
    /// writers.
    pub(crate) fn reconcile_mailbox_history(
        &self,
        expected_account_id: &str,
        mailbox: &str,
        expected_uid_validity: u32,
        expected_before_uid: Option<u32>,
        next_before_uid: Option<u32>,
        complete: bool,
        remote_total: u32,
    ) -> Result<bool> {
        if expected_uid_validity == 0
            || next_before_uid == Some(0)
            || (complete && next_before_uid.is_some())
            || (!complete && next_before_uid.is_none())
        {
            return Err(MailError::Validation(
                "the confirmed mailbox history snapshot is invalid".to_owned(),
            ));
        }
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mailboxes
             SET history_before_uid = CASE
                     WHEN history_complete = 1 THEN history_before_uid
                     ELSE ?5
                 END,
                 history_complete = MAX(history_complete, ?6),
                 remote_total = ?7
             WHERE account_id = ?1 AND name = ?2
               AND uid_validity = ?3
               AND history_before_uid IS ?4
               AND (history_complete = 0 OR ?6 = 1)",
            params![
                expected_account_id,
                mailbox,
                expected_uid_validity,
                expected_before_uid,
                next_before_uid,
                complete,
                remote_total,
            ],
        )?;
        Ok(changed == 1)
    }

    /// Advances the independent remote `\Flagged` discovery boundary. A
    /// starred scan must never mark ordinary message history as complete.
    pub(crate) fn advance_starred_mailbox_history(
        &self,
        expected_account_id: &str,
        mailbox: &str,
        expected_uid_validity: u32,
        expected_before_uid: Option<u32>,
        next_before_uid: Option<u32>,
        complete: bool,
    ) -> Result<bool> {
        validate_history_advance(
            expected_uid_validity,
            expected_before_uid,
            next_before_uid,
            complete,
            "starred mailbox history",
        )?;
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mailboxes
             SET starred_history_before_uid = CASE
                     WHEN starred_history_complete = 1 THEN starred_history_before_uid
                     ELSE ?5
                 END,
                 starred_history_complete = MAX(starred_history_complete, ?6)
             WHERE account_id = ?1 AND name = ?2
               AND uid_validity = ?3
               AND starred_history_before_uid IS ?4
               AND (starred_history_complete = 0 OR ?6 = 1)",
            params![
                expected_account_id,
                mailbox,
                expected_uid_validity,
                expected_before_uid,
                next_before_uid,
                complete,
            ],
        )?;
        Ok(changed == 1)
    }

    pub(crate) fn assign_mailbox_role(
        &self,
        account_id: &str,
        role: &str,
        mailbox: &str,
    ) -> Result<()> {
        let role = parse_mailbox_role(role)?;
        self.assign_semantic_mailbox_role(account_id, role, mailbox)
    }

    pub(crate) fn assign_semantic_mailbox_role(
        &self,
        account_id: &str,
        role: MailboxRole,
        mailbox: &str,
    ) -> Result<()> {
        validate_mailbox_display_name(mailbox)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO mailboxes (account_id, name) VALUES (?1, ?2)
             ON CONFLICT(account_id, name) DO NOTHING",
            params![account_id, mailbox],
        )?;
        transaction.execute(
            "INSERT INTO mailbox_roles (account_id, role, mailbox) VALUES (?1, ?2, ?3)
             ON CONFLICT(account_id, role) DO UPDATE SET mailbox = excluded.mailbox",
            params![account_id, role.as_str(), mailbox],
        )?;
        transaction.execute(
            "INSERT INTO mailbox_capabilities (
                 account_id, role, status, display_name, unavailable_reason, retryable
             ) VALUES (?1, ?2, 'available', ?3, NULL, 0)
             ON CONFLICT(account_id, role) DO UPDATE SET
                 status = 'available',
                 display_name = excluded.display_name,
                 unavailable_reason = NULL,
                 retryable = 0,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![account_id, role.as_str(), mailbox],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn mailbox_for_role(&self, account_id: &str, role: &str) -> Result<String> {
        self.mailbox_for_semantic_role(account_id, parse_mailbox_role(role)?)
    }

    pub(crate) fn mailbox_for_semantic_role(
        &self,
        account_id: &str,
        role: MailboxRole,
    ) -> Result<String> {
        self.assigned_mailbox_for_semantic_role(account_id, role)?
            .ok_or_else(|| MailError::NotFound {
                entity: "mailbox role",
                id: format!("{account_id}:{}", role.as_str()),
            })
    }

    pub(crate) fn assigned_mailbox_for_semantic_role(
        &self,
        account_id: &str,
        role: MailboxRole,
    ) -> Result<Option<String>> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT mailbox FROM mailbox_roles WHERE account_id = ?1 AND role = ?2",
                params![account_id, role.as_str()],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub(crate) fn set_mailbox_capability(
        &self,
        account_id: &str,
        capability: &MailboxCapability,
    ) -> Result<()> {
        validate_mailbox_capability(capability)?;
        if capability.status == MailboxCapabilityStatus::Available {
            return self.assign_semantic_mailbox_role(
                account_id,
                capability.role,
                capability
                    .display_name
                    .as_deref()
                    .expect("validated available capability has a display name"),
            );
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO mailbox_capabilities (
                 account_id, role, status, display_name, unavailable_reason, retryable
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(account_id, role) DO UPDATE SET
                 status = excluded.status,
                 display_name = excluded.display_name,
                 unavailable_reason = excluded.unavailable_reason,
                 retryable = excluded.retryable,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![
                account_id,
                capability.role.as_str(),
                capability.status.as_str(),
                capability.display_name,
                capability
                    .unavailable_reason
                    .map(MailboxCapabilityUnavailableReason::as_str),
                capability.retryable,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn mailbox_capabilities(&self, account_id: &str) -> Result<Vec<MailboxCapability>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT role, status, display_name, unavailable_reason, retryable
             FROM mailbox_capabilities
             WHERE account_id = ?1
             ORDER BY CASE role
                 WHEN 'inbox' THEN 0
                 WHEN 'sent' THEN 1
                 WHEN 'drafts' THEN 2
                 WHEN 'archive' THEN 3
                 WHEN 'trash' THEN 4
                 ELSE 5
             END",
        )?;
        statement
            .query_map(params![account_id], row_to_mailbox_capability)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn mailbox_capability(
        &self,
        account_id: &str,
        role: MailboxRole,
    ) -> Result<Option<MailboxCapability>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT role, status, display_name, unavailable_reason, retryable
                 FROM mailbox_capabilities
                 WHERE account_id = ?1 AND role = ?2",
                params![account_id, role.as_str()],
                row_to_mailbox_capability,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Clears cached messages and all cursors after an IMAP UIDVALIDITY change.
    pub(crate) fn reset_mailbox(&self, account_id: &str, mailbox: &str) -> Result<usize> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        for table in ["pending_seen_updates", "pending_flagged_updates"] {
            transaction.execute(
                &format!(
                    "UPDATE {table}
                     SET status = 'needs_attention',
                         error_kind = 'uid_validity_changed',
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE account_id = ?1 AND mailbox = ?2 AND status <> 'confirmed'"
                ),
                params![account_id, mailbox],
            )?;
        }
        transaction.execute(
            "UPDATE pending_message_actions
             SET status = 'needs_attention',
                 error_kind = 'uid_validity_changed',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE account_id = ?1 AND source_mailbox = ?2 AND status <> 'confirmed'",
            params![account_id, mailbox],
        )?;
        let removed = transaction.execute(
            "DELETE FROM messages WHERE account_id = ?1 AND mailbox = ?2",
            params![account_id, mailbox],
        )?;
        transaction.execute(
            "UPDATE mailboxes SET uid_validity = NULL, uid_next = NULL,
                 highest_uid = NULL, highest_modseq = NULL, last_synced_at = NULL,
                 history_before_uid = NULL, history_complete = 0,
                 starred_history_before_uid = NULL, starred_history_complete = 0,
                 remote_total = NULL
             WHERE account_id = ?1 AND name = ?2",
            params![account_id, mailbox],
        )?;
        transaction.commit()?;
        Ok(removed)
    }

    pub(crate) fn cached_uids(&self, account_id: &str, mailbox: &str) -> Result<HashSet<u32>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT uid FROM messages WHERE account_id = ?1 AND mailbox = ?2")?;
        let rows = statement.query_map(params![account_id, mailbox], |row| row.get(0))?;
        rows.collect::<std::result::Result<HashSet<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn delete_missing_uids(
        &self,
        account_id: &str,
        mailbox: &str,
        remote_uids: &HashSet<u32>,
    ) -> Result<usize> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let cached = {
            let mut statement = transaction
                .prepare("SELECT uid FROM messages WHERE account_id = ?1 AND mailbox = ?2")?;
            statement
                .query_map(params![account_id, mailbox], |row| row.get::<_, u32>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut removed = 0;
        for uid in cached.into_iter().filter(|uid| !remote_uids.contains(uid)) {
            for table in ["pending_seen_updates", "pending_flagged_updates"] {
                transaction.execute(
                    &format!(
                        "UPDATE {table}
                         SET status = 'needs_attention',
                             error_kind = 'source_missing',
                             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                         WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3
                           AND status <> 'confirmed'"
                    ),
                    params![account_id, mailbox, uid],
                )?;
            }
            transaction.execute(
                "UPDATE pending_message_actions
                 SET status = 'needs_attention',
                     error_kind = 'source_missing',
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE account_id = ?1 AND source_mailbox = ?2 AND source_uid = ?3
                   AND status <> 'confirmed'",
                params![account_id, mailbox, uid],
            )?;
            transaction.execute(
                "DELETE FROM pending_message_actions AS p
                 WHERE p.account_id = ?1 AND p.source_mailbox = ?2 AND p.source_uid = ?3
                   AND p.status = 'confirmed'
                   AND p.source_cleanup_pending = 0
                   AND p.kind = 'permanent_delete'
                   AND p.destination_role IS NULL
                   AND p.source_uid_validity = (
                       SELECT b.uid_validity
                       FROM mailboxes b
                       WHERE b.account_id = p.account_id AND b.name = p.source_mailbox
                   )",
                params![account_id, mailbox, uid],
            )?;
            removed += transaction.execute(
                "DELETE FROM messages AS m
                 WHERE m.account_id = ?1 AND m.mailbox = ?2 AND m.uid = ?3
                   AND NOT EXISTS (
                       SELECT 1
                       FROM pending_message_actions p
                       JOIN mailboxes b
                         ON b.account_id = p.account_id
                        AND b.name = p.source_mailbox
                        AND b.uid_validity = p.source_uid_validity
                       WHERE p.account_id = m.account_id
                         AND p.source_mailbox = m.mailbox
                         AND p.source_uid = m.uid
                         AND p.status = 'confirmed'
                   )",
                params![account_id, mailbox, uid],
            )?;
        }
        transaction.commit()?;
        Ok(removed)
    }

    /// Inserts a summary or refreshes it without discarding an already-fetched
    /// body when the incoming record contains summary-only data.
    pub(crate) fn upsert_message(&self, message: &InboxMessage) -> Result<i64> {
        let preview_fetched = message.body_fetched || !message.preview.trim().is_empty();
        self.upsert_message_with_preview_state(message, preview_fetched)
    }

    /// Persists a bounded preview attempt. An empty preview is still resolved:
    /// it may represent a genuinely body-free or attachment-only message and
    /// must not be downloaded again on every synchronization.
    pub(crate) fn upsert_message_summary(&self, message: &InboxMessage) -> Result<i64> {
        self.upsert_message_with_preview_state(message, true)
    }

    fn upsert_message_with_preview_state(
        &self,
        message: &InboxMessage,
        preview_fetched: bool,
    ) -> Result<i64> {
        let connection = self.connection()?;
        let public_id = Uuid::new_v4().to_string();
        connection.execute(
            "INSERT INTO mailboxes (account_id, name) VALUES (?1, ?2)
             ON CONFLICT(account_id, name) DO NOTHING",
            params![message.account_id, message.mailbox],
        )?;
        let sender_json = message.sender.as_ref().map(encode_json).transpose()?;
        let flags = flags_with_pending_updates(
            &connection,
            &message.account_id,
            &message.mailbox,
            message.uid,
            &message.flags,
        )?;
        connection.execute(
            "INSERT INTO messages (
                 account_id, mailbox, uid, message_id, in_reply_to_json, references_json, subject, sender_json,
                 to_json, cc_json, bcc_json, sent_at, internal_date, flags_json, size_bytes,
                 preview, preview_fetched, body_text, body_html, attachment_names_json, body_fetched,
                 raw_rfc822, body_cached_bytes, body_last_accessed_at, synced_at, public_id
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                 ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22,
                 CASE
                     WHEN ?21 THEN
                         length(?22)
                         + length(CAST(COALESCE(?18, '') AS BLOB))
                         + length(CAST(COALESCE(?19, '') AS BLOB))
                         + length(CAST(?20 AS BLOB))
                     ELSE 0
                 END,
                 CASE
                     WHEN ?21 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     ELSE NULL
                 END,
                 ?23, ?24
             )
             ON CONFLICT(account_id, mailbox, uid) DO UPDATE SET
                 message_id = excluded.message_id,
                 in_reply_to_json = excluded.in_reply_to_json,
                 references_json = excluded.references_json,
                 subject = excluded.subject,
                 sender_json = excluded.sender_json,
                 to_json = excluded.to_json,
                 cc_json = excluded.cc_json,
                 bcc_json = excluded.bcc_json,
                 sent_at = excluded.sent_at,
                 internal_date = excluded.internal_date,
                 flags_json = excluded.flags_json,
                 size_bytes = excluded.size_bytes,
                 preview = CASE
                     WHEN excluded.preview_fetched THEN excluded.preview
                     ELSE messages.preview
                 END,
                 preview_fetched = MAX(messages.preview_fetched, excluded.preview_fetched),
                 body_text = CASE WHEN excluded.body_fetched THEN excluded.body_text ELSE messages.body_text END,
                 body_html = CASE WHEN excluded.body_fetched THEN excluded.body_html ELSE messages.body_html END,
                 attachment_names_json = CASE WHEN excluded.body_fetched THEN excluded.attachment_names_json ELSE messages.attachment_names_json END,
                 body_fetched = MAX(messages.body_fetched, excluded.body_fetched),
                 raw_rfc822 = CASE WHEN excluded.body_fetched THEN excluded.raw_rfc822 ELSE messages.raw_rfc822 END,
                 body_cached_bytes = CASE
                     WHEN excluded.body_fetched THEN excluded.body_cached_bytes
                     ELSE messages.body_cached_bytes
                 END,
                 body_last_accessed_at = CASE
                     WHEN excluded.body_fetched THEN excluded.body_last_accessed_at
                     ELSE messages.body_last_accessed_at
                 END,
                 synced_at = excluded.synced_at",
            params![
                message.account_id,
                message.mailbox,
                message.uid,
                message.message_id,
                encode_json(&message.in_reply_to)?,
                encode_json(&message.references)?,
                message.subject,
                sender_json,
                encode_json(&message.to)?,
                encode_json(&message.cc)?,
                encode_json(&message.bcc)?,
                message.sent_at,
                message.internal_date,
                encode_json(&flags)?,
                message.size_bytes,
                message.preview,
                preview_fetched,
                message.body_text,
                message.body_html,
                encode_json(&message.attachment_names)?,
                message.body_fetched,
                message.raw_rfc822,
                message.synced_at,
                public_id,
            ],
        )?;
        connection
            .query_row(
                "SELECT id FROM messages WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3",
                params![message.account_id, message.mailbox, message.uid],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    #[cfg(test)]
    pub(crate) fn update_message_flags(
        &self,
        account_id: &str,
        mailbox: &str,
        uid: u32,
        flags: &[String],
    ) -> Result<()> {
        let connection = self.connection()?;
        let flags = flags_with_pending_updates(&connection, account_id, mailbox, uid, flags)?;
        let changed = connection.execute(
            "UPDATE messages SET flags_json = ?4
             WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3",
            params![account_id, mailbox, uid, encode_json(&flags)?],
        )?;
        ensure_changed(changed, "message", format!("{account_id}:{mailbox}/{uid}"))
    }

    /// Apply one server FLAGS batch with a single SQLite connection and
    /// transaction. Pending local Seen/Flagged intent is merged per row before
    /// writing so batching cannot regress optimistic UI state.
    pub(crate) fn update_message_flags_batch(
        &self,
        account_id: &str,
        mailbox: &str,
        updates: &[(u32, Vec<String>)],
    ) -> Result<usize> {
        if updates.is_empty() {
            return Ok(0);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (uid, remote_flags) in updates {
            let flags =
                flags_with_pending_updates(&transaction, account_id, mailbox, *uid, remote_flags)?;
            let changed = transaction.execute(
                "UPDATE messages SET flags_json = ?4
                 WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3",
                params![account_id, mailbox, uid, encode_json(&flags)?],
            )?;
            ensure_changed(changed, "message", format!("{account_id}:{mailbox}/{uid}"))?;
        }
        transaction.commit()?;
        Ok(updates.len())
    }

    pub(crate) fn queue_system_flag_mutation(
        &self,
        expected_account_id: &str,
        mailbox: &str,
        uid: u32,
        flag: SystemFlagKind,
        desired: bool,
    ) -> Result<(bool, PendingSystemFlagMutation)> {
        let table = system_flag_table(flag);
        let target = system_flag_name(flag);
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source: Option<(String, Option<u32>, String, String)> = transaction
            .query_row(
                "SELECT m.flags_json, b.uid_validity, m.account_id,
                        COALESCE((
                            SELECT r.role FROM mailbox_roles r
                            WHERE r.account_id = m.account_id AND r.mailbox = m.mailbox
                            ORDER BY CASE r.role WHEN 'inbox' THEN 0 ELSE 1 END
                            LIMIT 1
                        ), '')
                 FROM messages m
                 JOIN mailboxes b
                   ON b.account_id = m.account_id AND b.name = m.mailbox
                 WHERE m.account_id = ?1 AND m.mailbox = ?2 AND m.uid = ?3",
                params![expected_account_id, mailbox, uid],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let (encoded, source_uid_validity, actual_account_id, role) =
            source.ok_or_else(|| privacy_safe_not_found("message"))?;
        if actual_account_id != expected_account_id {
            return Err(account_scope_mismatch());
        }
        let source_uid_validity =
            source_uid_validity
                .filter(|epoch| *epoch > 0)
                .ok_or_else(|| {
                    MailError::Validation(
                        "the message mailbox epoch is not ready for a remote mutation".to_owned(),
                    )
                })?;
        let source_role = parse_mailbox_role(&role)?;
        if source_role == MailboxRole::Drafts {
            return Err(MailError::Validation(
                "system flags are unavailable for this mailbox role".to_owned(),
            ));
        }
        let mut flags: Vec<String> = serde_json::from_str(&encoded)?;
        let changed = set_system_flag(&mut flags, target, desired);
        if changed {
            transaction.execute(
                "UPDATE messages SET flags_json = ?4
                 WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3",
                params![expected_account_id, mailbox, uid, encode_json(&flags)?],
            )?;
        }

        let existing = query_system_flag_mutation_by_identity(
            &transaction,
            flag,
            expected_account_id,
            mailbox,
            source_uid_validity,
            uid,
        )?;
        let mutation = if let Some(existing) = existing {
            if existing.desired != desired {
                // Seen and Flagged mutations write an idempotent final state. A new
                // local target can therefore supersede any earlier lifecycle state:
                // claim/finalize calls are revision-guarded, so an older in-flight
                // result cannot update either this queue row or the message flags.
                transaction.execute(
                    &format!(
                        "UPDATE {table}
                         SET desired = ?3,
                             revision = revision + 1,
                             status = 'pending',
                             error_kind = NULL,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                         WHERE account_id = ?1 AND operation_id = ?2"
                    ),
                    params![expected_account_id, existing.operation_id, desired],
                )?;
            }
            query_system_flag_mutation_by_operation(
                &transaction,
                flag,
                expected_account_id,
                &existing.operation_id,
            )?
            .expect("existing flag operation remains present")
        } else {
            let operation_id = Uuid::now_v7().to_string();
            transaction.execute(
                &format!(
                    "INSERT INTO {table} (
                         operation_id, account_id, mailbox, source_uid_validity, uid,
                         desired, revision, status, error_kind
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 'pending', NULL)"
                ),
                params![
                    operation_id,
                    expected_account_id,
                    mailbox,
                    source_uid_validity,
                    uid,
                    desired,
                ],
            )?;
            query_system_flag_mutation_by_operation(
                &transaction,
                flag,
                expected_account_id,
                &operation_id,
            )?
            .expect("inserted flag operation")
        };
        transaction.commit()?;
        Ok((changed, mutation))
    }

    pub(crate) fn pending_system_flag_mutations(
        &self,
        expected_account_id: &str,
        mailbox: &str,
        flag: SystemFlagKind,
    ) -> Result<Vec<PendingSystemFlagMutation>> {
        let connection = self.connection()?;
        query_pending_system_flag_mutations(&connection, flag, expected_account_id, mailbox)
    }

    pub(crate) fn system_flag_mutations_requiring_reconciliation(
        &self,
        expected_account_id: &str,
        mailbox: &str,
        flag: SystemFlagKind,
    ) -> Result<Vec<PendingSystemFlagMutation>> {
        let connection = self.connection()?;
        query_reconcilable_system_flag_mutations(&connection, flag, expected_account_id, mailbox)
    }

    pub(crate) fn claim_system_flag_mutation(
        &self,
        expected_account_id: &str,
        operation_id: &str,
        flag: SystemFlagKind,
        expected_revision: u64,
    ) -> Result<Option<PendingSystemFlagMutation>> {
        let table = system_flag_table(flag);
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            &format!(
                "UPDATE {table} AS q
                 SET status = 'in_flight',
                     error_kind = NULL,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE q.account_id = ?1 AND q.operation_id = ?2
                   AND q.revision = ?3 AND q.status = 'pending'
                   AND EXISTS (
                       SELECT 1 FROM mailboxes b
                       WHERE b.account_id = q.account_id AND b.name = q.mailbox
                         AND b.uid_validity = q.source_uid_validity
                   )"
            ),
            params![
                expected_account_id,
                operation_id,
                u64_to_i64(expected_revision)
            ],
        )?;
        if changed == 0 {
            transaction.execute(
                &format!(
                    "UPDATE {table} AS q
                     SET status = 'needs_attention',
                         error_kind = 'uid_validity_changed',
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE q.account_id = ?1 AND q.operation_id = ?2
                       AND q.revision = ?3 AND q.status = 'pending'
                       AND NOT EXISTS (
                           SELECT 1 FROM mailboxes b
                           WHERE b.account_id = q.account_id AND b.name = q.mailbox
                             AND b.uid_validity = q.source_uid_validity
                       )"
                ),
                params![
                    expected_account_id,
                    operation_id,
                    u64_to_i64(expected_revision)
                ],
            )?;
            transaction.commit()?;
            return Ok(None);
        }
        let claimed = query_system_flag_mutation_by_operation(
            &transaction,
            flag,
            expected_account_id,
            operation_id,
        )?;
        transaction.commit()?;
        Ok(claimed)
    }

    pub(crate) fn finalize_system_flag_mutation_confirmed(
        &self,
        expected_account_id: &str,
        operation_id: &str,
        flag: SystemFlagKind,
        expected_revision: u64,
        server_flags: &[String],
    ) -> Result<bool> {
        finalize_system_flag_mutation_confirmed(
            &mut self.connection()?,
            expected_account_id,
            operation_id,
            flag,
            expected_revision,
            server_flags,
            false,
        )
    }

    pub(crate) fn finalize_system_flag_mutation_failure(
        &self,
        expected_account_id: &str,
        operation_id: &str,
        flag: SystemFlagKind,
        expected_revision: u64,
        status: MutationStatus,
        error_kind: MessageMutationErrorKind,
    ) -> Result<bool> {
        if !matches!(
            status,
            MutationStatus::OutcomeUnknown | MutationStatus::NeedsAttention
        ) {
            return Err(MailError::Validation(
                "an in-flight flag mutation may only stop in a recoverable state".to_owned(),
            ));
        }
        let table = system_flag_table(flag);
        let connection = self.connection()?;
        let changed = connection.execute(
            &format!(
                "UPDATE {table}
                 SET status = ?4,
                     error_kind = ?5,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE account_id = ?1 AND operation_id = ?2
                   AND revision = ?3 AND status = 'in_flight'"
            ),
            params![
                expected_account_id,
                operation_id,
                u64_to_i64(expected_revision),
                status.as_str(),
                error_kind.as_str(),
            ],
        )?;
        Ok(changed == 1)
    }

    pub(crate) fn reconcile_system_flag_mutation_confirmed(
        &self,
        expected_account_id: &str,
        operation_id: &str,
        flag: SystemFlagKind,
        expected_revision: u64,
        server_flags: &[String],
    ) -> Result<bool> {
        finalize_system_flag_mutation_confirmed(
            &mut self.connection()?,
            expected_account_id,
            operation_id,
            flag,
            expected_revision,
            server_flags,
            true,
        )
    }

    pub(crate) fn requeue_system_flag_mutation_after_reconcile(
        &self,
        expected_account_id: &str,
        operation_id: &str,
        flag: SystemFlagKind,
        expected_revision: u64,
    ) -> Result<bool> {
        let table = system_flag_table(flag);
        let connection = self.connection()?;
        let changed = connection.execute(
            &format!(
                "UPDATE {table} AS q
                 SET status = 'pending',
                     revision = revision + 1,
                     error_kind = NULL,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE q.account_id = ?1 AND q.operation_id = ?2 AND q.revision = ?3
                   AND q.status IN ('outcome_unknown', 'needs_attention')
                   AND EXISTS (
                       SELECT 1 FROM mailboxes b
                       WHERE b.account_id = q.account_id AND b.name = q.mailbox
                         AND b.uid_validity = q.source_uid_validity
                   )"
            ),
            params![
                expected_account_id,
                operation_id,
                u64_to_i64(expected_revision)
            ],
        )?;
        Ok(changed == 1)
    }

    #[cfg(test)]
    pub(crate) fn system_flag_mutation_receipt(
        &self,
        expected_account_id: &str,
        operation_id: &str,
        flag: SystemFlagKind,
    ) -> Result<Option<SystemFlagMutationReceipt>> {
        let connection = self.connection()?;
        Ok(query_system_flag_mutation_by_operation(
            &connection,
            flag,
            expected_account_id,
            operation_id,
        )?
        .map(system_flag_mutation_receipt))
    }

    // Compatibility wrappers retained while backend integration moves to the
    // explicit queue/claim/finalize API above.
    pub(crate) fn set_message_seen_pending(
        &self,
        account_id: &str,
        mailbox: &str,
        uid: u32,
        desired: bool,
    ) -> Result<(bool, u64)> {
        self.queue_system_flag_mutation(account_id, mailbox, uid, SystemFlagKind::Seen, desired)
            .map(|(changed, mutation)| (changed, mutation.revision))
    }

    pub(crate) fn mark_message_seen_pending(
        &self,
        account_id: &str,
        mailbox: &str,
        uid: u32,
    ) -> Result<bool> {
        self.set_message_seen_pending(account_id, mailbox, uid, true)
            .map(|(changed, _)| changed)
    }

    #[cfg(test)]
    pub(crate) fn pending_seen_updates(
        &self,
        account_id: &str,
        mailbox: &str,
    ) -> Result<Vec<(u32, bool, u64)>> {
        self.pending_system_flag_mutations(account_id, mailbox, SystemFlagKind::Seen)
            .map(|mutations| {
                mutations
                    .into_iter()
                    .map(|mutation| (mutation.source_uid, mutation.desired, mutation.revision))
                    .collect()
            })
    }

    #[cfg(test)]
    pub(crate) fn pending_seen_uids(&self, account_id: &str, mailbox: &str) -> Result<Vec<u32>> {
        self.pending_seen_updates(account_id, mailbox)
            .map(|updates| {
                updates
                    .into_iter()
                    .filter_map(|(uid, desired, _)| desired.then_some(uid))
                    .collect()
            })
    }

    #[cfg(test)]
    pub(crate) fn complete_pending_seen_if_unchanged(
        &self,
        account_id: &str,
        mailbox: &str,
        uid: u32,
        desired: bool,
        revision: u64,
        flags: &[String],
    ) -> Result<bool> {
        self.compatibility_complete_system_flag(
            account_id,
            mailbox,
            uid,
            desired,
            revision,
            SystemFlagKind::Seen,
            flags,
        )
    }

    #[cfg(test)]
    pub(crate) fn complete_pending_seen(
        &self,
        account_id: &str,
        mailbox: &str,
        uid: u32,
        flags: &[String],
    ) -> Result<()> {
        if let Some((_, desired, revision)) = self
            .pending_seen_updates(account_id, mailbox)?
            .into_iter()
            .find(|(pending_uid, desired, _)| *pending_uid == uid && *desired)
        {
            self.complete_pending_seen_if_unchanged(
                account_id, mailbox, uid, desired, revision, flags,
            )?;
        }
        Ok(())
    }

    pub(crate) fn set_message_flagged_pending(
        &self,
        account_id: &str,
        mailbox: &str,
        uid: u32,
        desired: bool,
    ) -> Result<bool> {
        self.queue_system_flag_mutation(account_id, mailbox, uid, SystemFlagKind::Flagged, desired)
            .map(|(changed, _)| changed)
    }

    #[cfg(test)]
    pub(crate) fn pending_flagged_updates(
        &self,
        account_id: &str,
        mailbox: &str,
    ) -> Result<Vec<(u32, bool, u64)>> {
        self.pending_system_flag_mutations(account_id, mailbox, SystemFlagKind::Flagged)
            .map(|mutations| {
                mutations
                    .into_iter()
                    .map(|mutation| (mutation.source_uid, mutation.desired, mutation.revision))
                    .collect()
            })
    }

    #[cfg(test)]
    pub(crate) fn complete_pending_flagged(
        &self,
        account_id: &str,
        mailbox: &str,
        uid: u32,
        desired: bool,
        revision: u64,
        flags: &[String],
    ) -> Result<bool> {
        self.compatibility_complete_system_flag(
            account_id,
            mailbox,
            uid,
            desired,
            revision,
            SystemFlagKind::Flagged,
            flags,
        )
    }

    #[cfg(test)]
    fn compatibility_complete_system_flag(
        &self,
        account_id: &str,
        mailbox: &str,
        uid: u32,
        desired: bool,
        revision: u64,
        flag: SystemFlagKind,
        flags: &[String],
    ) -> Result<bool> {
        let pending = self
            .pending_system_flag_mutations(account_id, mailbox, flag)?
            .into_iter()
            .find(|mutation| {
                mutation.source_uid == uid
                    && mutation.desired == desired
                    && mutation.revision == revision
            });
        let Some(pending) = pending else {
            return Ok(false);
        };
        if self
            .claim_system_flag_mutation(account_id, &pending.operation_id, flag, revision)?
            .is_none()
        {
            return Ok(false);
        }
        self.finalize_system_flag_mutation_confirmed(
            account_id,
            &pending.operation_id,
            flag,
            revision,
            flags,
        )
    }

    /// Compatibility entry point for callers that have not yet threaded their
    /// already-validated account ID through to the repository boundary.
    #[cfg(test)]
    pub(crate) fn queue_message_action(
        &self,
        message_id: i64,
        source_role: MailboxRole,
        kind: MessageActionKind,
        destination_role: Option<MailboxRole>,
    ) -> Result<MessageMutationReceipt> {
        let connection = self.connection()?;
        let account_id = connection
            .query_row(
                "SELECT account_id FROM messages WHERE id = ?1",
                params![message_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| privacy_safe_not_found("message"))?;
        self.queue_message_action_for_account(
            &account_id,
            message_id,
            source_role,
            kind,
            destination_role,
        )
    }

    /// Persists one account-scoped optimistic Archive/Trash/permanent-delete
    /// intent before any network operation. Only an ordinary pending intent may
    /// be folded; an in-flight or recoverable outcome must first be reconciled.
    pub(crate) fn queue_message_action_for_account(
        &self,
        expected_account_id: &str,
        message_id: i64,
        source_role: MailboxRole,
        kind: MessageActionKind,
        destination_role: Option<MailboxRole>,
    ) -> Result<MessageMutationReceipt> {
        validate_message_action(source_role, kind, destination_role)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source: Option<(
            String,
            String,
            u32,
            Option<String>,
            Option<String>,
            u32,
            Option<u32>,
        )> = transaction
            .query_row(
                "SELECT m.account_id, m.mailbox, m.uid, m.message_id,
                        m.internal_date, m.size_bytes, b.uid_validity
                 FROM messages m
                 JOIN mailboxes b
                   ON b.account_id = m.account_id AND b.name = m.mailbox
                 WHERE m.id = ?1 AND m.account_id = ?2",
                params![message_id, expected_account_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional()?;
        let (
            account_id,
            source_mailbox,
            source_uid,
            source_message_id,
            source_internal_date,
            source_size_bytes,
            source_uid_validity,
        ) = source.ok_or_else(|| privacy_safe_not_found("message"))?;
        if account_id != expected_account_id {
            return Err(account_scope_mismatch());
        }
        let source_uid_validity = source_uid_validity.ok_or_else(|| {
            MailError::Validation(
                "the message mailbox has no confirmed UIDVALIDITY epoch".to_owned(),
            )
        })?;
        if source_uid_validity == 0 {
            return Err(MailError::Validation(
                "the message mailbox has an invalid UIDVALIDITY epoch".to_owned(),
            ));
        }
        let mapped_source: Option<String> = transaction
            .query_row(
                "SELECT mailbox FROM mailbox_roles
                 WHERE account_id = ?1 AND role = ?2",
                params![account_id, source_role.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        let source_matches = mapped_source.as_deref().is_some_and(|mapped| {
            if source_role == MailboxRole::Inbox {
                mapped.eq_ignore_ascii_case(&source_mailbox)
            } else {
                mapped == source_mailbox
            }
        });
        if !source_matches {
            return Err(MailError::Validation(
                "the message does not belong to the requested semantic mailbox".to_owned(),
            ));
        }
        if let Some(destination_role) = destination_role {
            let destination_available = transaction.query_row(
                "SELECT EXISTS(
                     SELECT 1
                     FROM mailbox_roles r
                     JOIN mailbox_capabilities c
                       ON c.account_id = r.account_id AND c.role = r.role
                     WHERE r.account_id = ?1 AND r.role = ?2 AND c.status = 'available'
                 )",
                params![account_id, destination_role.as_str()],
                |row| row.get::<_, bool>(0),
            )?;
            if !destination_available {
                return Err(MailError::Validation(
                    "the destination mailbox capability is unavailable".to_owned(),
                ));
            }
        }

        let existing = query_message_action_by_identity(
            &transaction,
            expected_account_id,
            &source_mailbox,
            source_uid_validity,
            source_uid,
        )?;
        let action = if let Some(existing) = existing {
            if existing.status != MutationStatus::Pending {
                return Err(MailError::Validation(
                    "the earlier message action must be reconciled before another action"
                        .to_owned(),
                ));
            }
            let same_intent = existing.source_role == source_role
                && existing.kind == kind
                && existing.destination_role == destination_role;
            if !same_intent {
                transaction.execute(
                    "UPDATE pending_message_actions
                     SET source_role = ?2,
                         destination_role = ?3,
                         kind = ?4,
                         revision = revision + 1,
                         remote_phase = 'queued',
                         source_message_id = ?5,
                         source_internal_date = ?6,
                         source_size_bytes = ?7,
                         error_kind = NULL,
                         source_cleanup_pending = 0,
                         destination_reconciled = 0,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE operation_id = ?1 AND status = 'pending'",
                    params![
                        existing.operation_id,
                        source_role.as_str(),
                        destination_role.map(MailboxRole::as_str),
                        kind.as_str(),
                        bounded_identity_field(
                            source_message_id.as_deref(),
                            MAX_IDENTITY_MESSAGE_ID_CHARS
                        ),
                        bounded_identity_field(
                            source_internal_date.as_deref(),
                            MAX_IDENTITY_DATE_CHARS
                        ),
                        source_size_bytes,
                    ],
                )?;
            }
            query_message_action_by_operation(
                &transaction,
                expected_account_id,
                &existing.operation_id,
            )?
            .expect("existing message operation remains present")
        } else {
            let operation_id = Uuid::now_v7().to_string();
            transaction.execute(
                "INSERT INTO pending_message_actions (
                     operation_id, account_id, source_mailbox, source_uid_validity, source_uid,
                     source_role, destination_role, kind, revision, status, remote_phase,
                     source_message_id, source_internal_date, source_size_bytes, error_kind
                 ) VALUES (
                     ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 'pending', 'queued',
                     ?9, ?10, ?11, NULL
                 )",
                params![
                    operation_id,
                    expected_account_id,
                    source_mailbox,
                    source_uid_validity,
                    source_uid,
                    source_role.as_str(),
                    destination_role.map(MailboxRole::as_str),
                    kind.as_str(),
                    bounded_identity_field(
                        source_message_id.as_deref(),
                        MAX_IDENTITY_MESSAGE_ID_CHARS
                    ),
                    bounded_identity_field(
                        source_internal_date.as_deref(),
                        MAX_IDENTITY_DATE_CHARS
                    ),
                    source_size_bytes,
                ],
            )?;
            query_message_action_by_operation(&transaction, expected_account_id, &operation_id)?
                .expect("inserted message operation")
        };
        let receipt = message_mutation_receipt(&action);
        transaction.commit()?;
        Ok(receipt)
    }

    pub(crate) fn pending_message_actions(
        &self,
        account_id: &str,
    ) -> Result<Vec<PendingMessageAction>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT operation_id, account_id, source_mailbox, source_uid_validity,
                    source_uid, source_role, destination_role, kind, revision, status,
                    remote_phase, source_message_id, source_internal_date, source_size_bytes,
                    error_kind, source_cleanup_pending, destination_reconciled, updated_at
             FROM pending_message_actions p
             WHERE p.account_id = ?1 AND p.status = 'pending'
               AND EXISTS (
                   SELECT 1 FROM mailboxes b
                   WHERE b.account_id = p.account_id AND b.name = p.source_mailbox
                     AND b.uid_validity = p.source_uid_validity
               )
               AND EXISTS (
                   SELECT 1 FROM messages m
                   WHERE m.account_id = p.account_id AND m.mailbox = p.source_mailbox
                     AND m.uid = p.source_uid
               )
             ORDER BY p.updated_at, p.operation_id",
        )?;
        statement
            .query_map(params![account_id], row_to_pending_message_action)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn message_actions_requiring_reconciliation(
        &self,
        account_id: &str,
    ) -> Result<Vec<PendingMessageAction>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT operation_id, account_id, source_mailbox, source_uid_validity,
                    source_uid, source_role, destination_role, kind, revision, status,
                    remote_phase, source_message_id, source_internal_date, source_size_bytes,
                    error_kind, source_cleanup_pending, destination_reconciled, updated_at
             FROM pending_message_actions
             WHERE account_id = ?1
               AND status IN ('in_flight', 'outcome_unknown', 'needs_attention')
             ORDER BY updated_at, operation_id",
        )?;
        statement
            .query_map(params![account_id], row_to_pending_message_action)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    #[cfg(test)]
    pub(crate) fn message_action(
        &self,
        account_id: &str,
        operation_id: &str,
    ) -> Result<Option<PendingMessageAction>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT operation_id, account_id, source_mailbox, source_uid_validity,
                        source_uid, source_role, destination_role, kind, revision, status,
                        remote_phase, source_message_id, source_internal_date, source_size_bytes,
                        error_kind, source_cleanup_pending, destination_reconciled, updated_at
                 FROM pending_message_actions
                 WHERE account_id = ?1 AND operation_id = ?2",
                params![account_id, operation_id],
                row_to_pending_message_action,
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn claim_message_action(
        &self,
        account_id: &str,
        operation_id: &str,
        expected_revision: u64,
    ) -> Result<Option<PendingMessageAction>> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(action) =
            query_message_action_by_operation(&transaction, account_id, operation_id)?
        else {
            transaction.commit()?;
            return Ok(None);
        };
        if action.revision != expected_revision || action.status != MutationStatus::Pending {
            transaction.commit()?;
            return Ok(None);
        }
        let current_epoch: Option<Option<u32>> = transaction
            .query_row(
                "SELECT uid_validity FROM mailboxes
                 WHERE account_id = ?1 AND name = ?2",
                params![account_id, action.source_mailbox],
                |row| row.get(0),
            )
            .optional()?;
        let source_exists = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM messages
                 WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3
             )",
            params![account_id, action.source_mailbox, action.source_uid],
            |row| row.get::<_, bool>(0),
        )?;
        if current_epoch.flatten() != Some(action.source_uid_validity) || !source_exists {
            let error = if current_epoch.flatten() != Some(action.source_uid_validity) {
                MessageMutationErrorKind::UidValidityChanged
            } else {
                MessageMutationErrorKind::SourceMissing
            };
            transaction.execute(
                "UPDATE pending_message_actions
                 SET status = 'needs_attention',
                     error_kind = ?4,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
                   AND status = 'pending'",
                params![
                    account_id,
                    operation_id,
                    u64_to_i64(expected_revision),
                    error.as_str(),
                ],
            )?;
            transaction.commit()?;
            return Ok(None);
        }
        let changed = transaction.execute(
            "UPDATE pending_message_actions
             SET status = 'in_flight',
                 error_kind = NULL,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
               AND status = 'pending'",
            params![account_id, operation_id, u64_to_i64(expected_revision)],
        )?;
        let claimed = if changed == 1 {
            query_message_action_by_operation(&transaction, account_id, operation_id)?
        } else {
            None
        };
        transaction.commit()?;
        Ok(claimed)
    }

    pub(crate) fn advance_message_action_remote_phase(
        &self,
        account_id: &str,
        operation_id: &str,
        expected_revision: u64,
        expected_phase: RemoteMutationPhase,
        next_phase: RemoteMutationPhase,
    ) -> Result<bool> {
        let connection = self.connection()?;
        let Some(action) =
            query_message_action_by_operation(&connection, account_id, operation_id)?
        else {
            return Ok(false);
        };
        if action.revision != expected_revision
            || action.status != MutationStatus::InFlight
            || action.remote_phase != expected_phase
        {
            return Ok(false);
        }
        if !valid_remote_phase_transition(action.kind, expected_phase, next_phase) {
            return Err(MailError::Validation(
                "the remote mutation phase transition is invalid".to_owned(),
            ));
        }
        let changed = connection.execute(
            "UPDATE pending_message_actions
             SET remote_phase = ?5,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
               AND status = 'in_flight' AND remote_phase = ?4",
            params![
                account_id,
                operation_id,
                u64_to_i64(expected_revision),
                expected_phase.as_str(),
                next_phase.as_str(),
            ],
        )?;
        Ok(changed == 1)
    }

    pub(crate) fn finalize_message_action(
        &self,
        account_id: &str,
        operation_id: &str,
        expected_revision: u64,
        status: MutationStatus,
        error_kind: Option<MessageMutationErrorKind>,
    ) -> Result<bool> {
        if status == MutationStatus::Confirmed {
            if error_kind.is_some() {
                return Err(MailError::Validation(
                    "a confirmed message action cannot retain an error category".to_owned(),
                ));
            }
            return self.finalize_message_action_confirmed(
                account_id,
                operation_id,
                expected_revision,
                false,
            );
        }
        if !matches!(
            status,
            MutationStatus::OutcomeUnknown | MutationStatus::NeedsAttention
        ) {
            return Err(MailError::Validation(
                "an in-flight message action has an invalid final status".to_owned(),
            ));
        }
        let connection = self.connection()?;
        let Some(action) =
            query_message_action_by_operation(&connection, account_id, operation_id)?
        else {
            return Ok(false);
        };
        if action.revision != expected_revision || action.status != MutationStatus::InFlight {
            return Ok(false);
        }
        if error_kind.is_none() {
            return Err(MailError::Validation(
                "a recoverable message action status requires an error category".to_owned(),
            ));
        }
        let changed = connection.execute(
            "UPDATE pending_message_actions
             SET status = ?4,
                 error_kind = ?5,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
               AND status = 'in_flight'",
            params![
                account_id,
                operation_id,
                u64_to_i64(expected_revision),
                status.as_str(),
                error_kind.map(MessageMutationErrorKind::as_str),
            ],
        )?;
        Ok(changed == 1)
    }

    /// Atomically records a successful source delete and whether the provider
    /// proved final UID removal. Without that proof the confirmed action is a
    /// durable source tombstone until a same-epoch synchronization confirms
    /// that the UID is absent.
    pub(crate) fn finalize_message_action_confirmed(
        &self,
        account_id: &str,
        operation_id: &str,
        expected_revision: u64,
        source_cleanup_pending: bool,
    ) -> Result<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(action) =
            query_message_action_by_operation(&transaction, account_id, operation_id)?
        else {
            transaction.commit()?;
            return Ok(false);
        };
        if action.revision != expected_revision || action.status != MutationStatus::InFlight {
            transaction.commit()?;
            return Ok(false);
        }
        if action.remote_phase != RemoteMutationPhase::SourceDeleteAcknowledged {
            return Err(MailError::Validation(
                "a confirmed message action requires an acknowledged remote delete".to_owned(),
            ));
        }
        let changed = transaction.execute(
            "UPDATE pending_message_actions
             SET status = 'confirmed',
                 remote_phase = 'source_delete_acknowledged',
                 error_kind = NULL,
                 source_cleanup_pending = ?4,
                 destination_reconciled = 0,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
               AND status = 'in_flight'
               AND remote_phase = 'source_delete_acknowledged'",
            params![
                account_id,
                operation_id,
                u64_to_i64(expected_revision),
                source_cleanup_pending,
            ],
        )?;
        transaction.commit()?;
        Ok(changed == 1)
    }

    pub(crate) fn reconcile_message_action(
        &self,
        account_id: &str,
        operation_id: &str,
        expected_revision: u64,
        resolution: MutationStatus,
        error_kind: Option<MessageMutationErrorKind>,
    ) -> Result<bool> {
        if resolution == MutationStatus::Confirmed {
            if error_kind.is_some() {
                return Err(MailError::Validation(
                    "a reconciled message action cannot retain an error category".to_owned(),
                ));
            }
            return self.reconcile_message_action_confirmed(
                account_id,
                operation_id,
                expected_revision,
                false,
            );
        }
        if !matches!(
            resolution,
            MutationStatus::Pending
                | MutationStatus::NeedsAttention
                | MutationStatus::OutcomeUnknown
        ) {
            return Err(MailError::Validation(
                "the message action reconciliation result is invalid".to_owned(),
            ));
        }
        if matches!(
            resolution,
            MutationStatus::NeedsAttention | MutationStatus::OutcomeUnknown
        ) && error_kind.is_none()
        {
            return Err(MailError::Validation(
                "a recoverable message action status requires an error category".to_owned(),
            ));
        }
        if matches!(resolution, MutationStatus::Pending) && error_kind.is_some() {
            return Err(MailError::Validation(
                "a reconciled message action cannot retain an error category".to_owned(),
            ));
        }
        let connection = self.connection()?;
        let (next_revision, next_phase) = match resolution {
            MutationStatus::Pending => (
                expected_revision.saturating_add(1),
                RemoteMutationPhase::Queued,
            ),
            _ => {
                let Some(action) =
                    query_message_action_by_operation(&connection, account_id, operation_id)?
                else {
                    return Ok(false);
                };
                (expected_revision, action.remote_phase)
            }
        };
        let changed = connection.execute(
            "UPDATE pending_message_actions
             SET status = ?4,
                 revision = ?5,
                 remote_phase = ?6,
                 error_kind = ?7,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
               AND status IN ('in_flight', 'outcome_unknown', 'needs_attention')",
            params![
                account_id,
                operation_id,
                u64_to_i64(expected_revision),
                resolution.as_str(),
                u64_to_i64(next_revision),
                next_phase.as_str(),
                error_kind.map(MessageMutationErrorKind::as_str),
            ],
        )?;
        Ok(changed == 1)
    }

    /// Reconciliation counterpart to `finalize_message_action_confirmed`.
    /// The reconciliation worker calls this only after it has proved the exact
    /// source deletion outcome. A persisted transfer-acknowledged or
    /// source-delete-started phase is advanced to the acknowledged phase in
    /// the same statement; transfer-started is intentionally excluded.
    pub(crate) fn reconcile_message_action_confirmed(
        &self,
        account_id: &str,
        operation_id: &str,
        expected_revision: u64,
        source_cleanup_pending: bool,
    ) -> Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE pending_message_actions
             SET status = 'confirmed',
                 remote_phase = 'source_delete_acknowledged',
                 error_kind = NULL,
                 source_cleanup_pending = ?4,
                 destination_reconciled = 0,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
               AND status IN ('in_flight', 'outcome_unknown', 'needs_attention')
               AND (
                   remote_phase IN (
                       'transfer_acknowledged', 'source_delete_started',
                       'source_delete_acknowledged'
                   )
                   OR (
                       ?4 = 0
                       AND remote_phase = 'queued'
                       AND kind = 'permanent_delete'
                       AND destination_role IS NULL
                   )
               )",
            params![
                account_id,
                operation_id,
                u64_to_i64(expected_revision),
                source_cleanup_pending,
            ],
        )?;
        Ok(changed == 1)
    }

    pub(crate) fn purge_confirmed_message_action_after_convergence(
        &self,
        account_id: &str,
        operation_id: &str,
        expected_revision: u64,
    ) -> Result<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source: Option<(String, u32, u32)> = transaction
            .query_row(
                "SELECT source_mailbox, source_uid_validity, source_uid
                 FROM pending_message_actions
                 WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
                   AND status = 'confirmed'
                   AND source_cleanup_pending = 0",
                params![account_id, operation_id, u64_to_i64(expected_revision)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((source_mailbox, source_uid_validity, source_uid)) = source else {
            transaction.commit()?;
            return Ok(false);
        };
        let changed = transaction.execute(
            "DELETE FROM pending_message_actions
             WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
               AND status = 'confirmed'
               AND source_cleanup_pending = 0",
            params![account_id, operation_id, u64_to_i64(expected_revision)],
        )?;
        if changed == 1 {
            transaction.execute(
                "DELETE FROM messages AS m
                 WHERE m.account_id = ?1 AND m.mailbox = ?2 AND m.uid = ?3
                   AND EXISTS (
                       SELECT 1 FROM mailboxes b
                       WHERE b.account_id = m.account_id AND b.name = m.mailbox
                         AND b.uid_validity = ?4
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM pending_message_actions p
                       WHERE p.account_id = m.account_id
                         AND p.source_mailbox = m.mailbox
                         AND p.source_uid_validity = ?4
                         AND p.source_uid = m.uid
                   )",
                params![account_id, source_mailbox, source_uid, source_uid_validity,],
            )?;
        }
        transaction.commit()?;
        Ok(changed == 1)
    }

    pub(crate) fn purge_confirmed_message_action_if_destination_unique(
        &self,
        account_id: &str,
        operation_id: &str,
    ) -> Result<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let action: Option<(String, u32, u32, String, String, u32, String, bool)> = transaction
            .query_row(
                "SELECT source_mailbox, source_uid_validity, source_uid,
                        source_message_id, source_internal_date, source_size_bytes,
                        destination_role, source_cleanup_pending
                 FROM pending_message_actions
                 WHERE account_id = ?1 AND operation_id = ?2
                   AND status = 'confirmed'
                   AND source_message_id IS NOT NULL
                   AND source_internal_date IS NOT NULL
                   AND source_size_bytes > 0
                   AND destination_role IS NOT NULL",
                params![account_id, operation_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            source_mailbox,
            source_uid_validity,
            source_uid,
            source_message_id,
            source_internal_date,
            source_size_bytes,
            destination_role,
            source_cleanup_pending,
        )) = action
        else {
            transaction.commit()?;
            return Ok(false);
        };
        let destination_mailbox: Option<String> = transaction
            .query_row(
                "SELECT mailbox FROM mailbox_roles
                 WHERE account_id = ?1 AND role = ?2",
                params![account_id, destination_role],
                |row| row.get(0),
            )
            .optional()?;
        let Some(destination_mailbox) =
            destination_mailbox.filter(|mailbox| mailbox != &source_mailbox)
        else {
            transaction.commit()?;
            return Ok(false);
        };
        let destination_matches: u32 = transaction.query_row(
            "SELECT COUNT(*)
             FROM messages
             WHERE account_id = ?1 AND mailbox = ?2
               AND message_id = ?3 AND internal_date = ?4 AND size_bytes = ?5",
            params![
                account_id,
                destination_mailbox,
                source_message_id,
                source_internal_date,
                source_size_bytes,
            ],
            |row| row.get(0),
        )?;
        if destination_matches != 1 {
            transaction.commit()?;
            return Ok(false);
        }
        let changed = if source_cleanup_pending {
            transaction.execute(
                "UPDATE pending_message_actions
                 SET destination_reconciled = 1,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE account_id = ?1 AND operation_id = ?2
                   AND status = 'confirmed' AND source_cleanup_pending = 1",
                params![account_id, operation_id],
            )?
        } else {
            transaction.execute(
                "DELETE FROM pending_message_actions
                 WHERE account_id = ?1 AND operation_id = ?2
                   AND status = 'confirmed' AND source_cleanup_pending = 0",
                params![account_id, operation_id],
            )?
        };
        if changed == 1 {
            transaction.execute(
                "DELETE FROM messages AS m
                 WHERE m.account_id = ?1 AND m.mailbox = ?2 AND m.uid = ?3
                   AND EXISTS (
                       SELECT 1 FROM mailboxes b
                       WHERE b.account_id = m.account_id AND b.name = m.mailbox
                         AND b.uid_validity = ?4
                   )",
                params![account_id, source_mailbox, source_uid, source_uid_validity,],
            )?;
        }
        transaction.commit()?;
        Ok(changed == 1)
    }

    /// Lists confirmed actions whose source UID still requires same-epoch
    /// disappearance proof. These rows are tombstones, not retryable network
    /// actions, and must never cause COPY or STORE to run again.
    pub(crate) fn confirmed_source_cleanup_tombstones(
        &self,
        account_id: &str,
    ) -> Result<Vec<PendingMessageAction>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT operation_id, account_id, source_mailbox, source_uid_validity,
                    source_uid, source_role, destination_role, kind, revision, status,
                    remote_phase, source_message_id, source_internal_date, source_size_bytes,
                    error_kind, source_cleanup_pending, destination_reconciled, updated_at
             FROM pending_message_actions
             WHERE account_id = ?1
               AND status = 'confirmed'
               AND source_cleanup_pending = 1
             ORDER BY updated_at, operation_id",
        )?;
        statement
            .query_map(params![account_id], row_to_pending_message_action)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Purges a source-cleanup tombstone only after the caller has completed a
    /// same-epoch remote UID listing and proved this exact UID absent.
    pub(crate) fn purge_confirmed_source_cleanup_if_remote_absent(
        &self,
        account_id: &str,
        operation_id: &str,
        expected_revision: u64,
        confirmed_uid_validity: u32,
        confirmed_source_uid: u32,
    ) -> Result<bool> {
        if confirmed_uid_validity == 0 || confirmed_source_uid == 0 {
            return Err(MailError::Validation(
                "source cleanup confirmation requires a valid mailbox epoch and UID".to_owned(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source: Option<(String, u32, u32)> = transaction
            .query_row(
                "SELECT source_mailbox, source_uid_validity, source_uid
                 FROM pending_message_actions
                 WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
                   AND status = 'confirmed' AND source_cleanup_pending = 1
                   AND source_uid_validity = ?4 AND source_uid = ?5",
                params![
                    account_id,
                    operation_id,
                    u64_to_i64(expected_revision),
                    confirmed_uid_validity,
                    confirmed_source_uid,
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((source_mailbox, source_uid_validity, source_uid)) = source else {
            transaction.commit()?;
            return Ok(false);
        };
        let epoch_matches = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM mailboxes
                 WHERE account_id = ?1 AND name = ?2 AND uid_validity = ?3
             )",
            params![account_id, source_mailbox, source_uid_validity],
            |row| row.get::<_, bool>(0),
        )?;
        if !epoch_matches {
            transaction.commit()?;
            return Ok(false);
        }
        let cleanup_is_unblocked = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM pending_message_actions
                 WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
                   AND status = 'confirmed' AND source_cleanup_pending = 1
                   AND (
                       (kind = 'permanent_delete' AND destination_role IS NULL)
                       OR destination_reconciled = 1
                   )
             )",
            params![account_id, operation_id, u64_to_i64(expected_revision)],
            |row| row.get::<_, bool>(0),
        )?;
        if !cleanup_is_unblocked {
            transaction.commit()?;
            return Ok(false);
        }
        let changed = transaction.execute(
            "DELETE FROM pending_message_actions
             WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
               AND status = 'confirmed' AND source_cleanup_pending = 1
               AND source_uid_validity = ?4 AND source_uid = ?5
               AND (
                   (kind = 'permanent_delete' AND destination_role IS NULL)
                   OR destination_reconciled = 1
               )",
            params![
                account_id,
                operation_id,
                u64_to_i64(expected_revision),
                source_uid_validity,
                source_uid,
            ],
        )?;
        if changed == 1 {
            transaction.execute(
                "DELETE FROM messages
                 WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3",
                params![account_id, source_mailbox, source_uid],
            )?;
        }
        transaction.commit()?;
        Ok(changed == 1)
    }

    /// Compatibility adapter retained while backend integration migrates to
    /// the explicit queue/claim/phase/finalize/reconcile API.
    #[cfg(test)]
    pub(crate) fn update_message_action_status_if_unchanged(
        &self,
        account_id: &str,
        operation_id: &str,
        revision: u64,
        status: MutationStatus,
        error_kind: Option<MessageMutationErrorKind>,
    ) -> Result<bool> {
        match status {
            MutationStatus::InFlight => self
                .claim_message_action(account_id, operation_id, revision)
                .map(|claimed| claimed.is_some()),
            MutationStatus::Confirmed
            | MutationStatus::OutcomeUnknown
            | MutationStatus::NeedsAttention => {
                self.finalize_message_action(account_id, operation_id, revision, status, error_kind)
            }
            MutationStatus::Pending => self.reconcile_message_action(
                account_id,
                operation_id,
                revision,
                status,
                error_kind,
            ),
        }
    }

    /// Lists one account-scoped semantic mailbox using a stable keyset. Search
    /// is restricted to synchronized summary fields and never inspects a cached
    /// body or raw RFC822 payload.
    pub(crate) fn list_mailbox_page(
        &self,
        account_id: &str,
        role: MailboxRole,
        cursor: Option<&MessagePageCursor>,
        page_size: usize,
        query: Option<&str>,
    ) -> Result<MessagePage> {
        self.list_mailbox_page_filtered(account_id, role, cursor, page_size, query, false)
    }

    pub(crate) fn list_starred_mailbox_page(
        &self,
        account_id: &str,
        role: MailboxRole,
        cursor: Option<&MessagePageCursor>,
        page_size: usize,
        query: Option<&str>,
    ) -> Result<MessagePage> {
        self.list_mailbox_page_filtered(account_id, role, cursor, page_size, query, true)
    }

    fn list_mailbox_page_filtered(
        &self,
        account_id: &str,
        role: MailboxRole,
        cursor: Option<&MessagePageCursor>,
        page_size: usize,
        query: Option<&str>,
        flagged_only: bool,
    ) -> Result<MessagePage> {
        let page_size = validate_page_size(page_size)?;
        let query = normalize_search_query(query)?;
        let capability = self.mailbox_capability(account_id, role)?;
        let Some(capability) = capability else {
            return Ok(unavailable_message_page(RemoteHistoryState::NotChecked));
        };
        if capability.status != MailboxCapabilityStatus::Available {
            let state = match capability.status {
                MailboxCapabilityStatus::Unavailable
                | MailboxCapabilityStatus::NeedsCreationConfirmation => {
                    RemoteHistoryState::Unavailable
                }
                MailboxCapabilityStatus::DiscoveryPending => RemoteHistoryState::NotChecked,
                MailboxCapabilityStatus::Available => unreachable!(),
            };
            return Ok(unavailable_message_page(state));
        }
        let mailbox = self.mailbox_for_semantic_role(account_id, role)?;
        let state =
            self.mailbox_state(account_id, &mailbox)?
                .ok_or_else(|| MailError::NotFound {
                    entity: "mailbox",
                    id: format!("{account_id}:{}", bounded_diagnostic_id(&mailbox)),
                })?;
        let (history_before_uid, history_complete) = if flagged_only {
            let history = self
                .starred_mailbox_history(account_id, &mailbox)?
                .unwrap_or_default();
            (history.before_uid, history.complete)
        } else {
            let history = self
                .mailbox_history(account_id, &mailbox)?
                .unwrap_or_default();
            (history.before_uid, history.complete)
        };
        let connection = self.connection()?;
        let decoded_cursor = cursor
            .map(|cursor| {
                load_and_validate_message_cursor(
                    &connection,
                    cursor,
                    account_id,
                    &mailbox,
                    role,
                    state.uid_validity,
                    query.as_deref().unwrap_or_default(),
                    flagged_only,
                )
            })
            .transpose()?;
        let search_pattern = query.as_deref().map(search_like_pattern);
        let fetch_limit = page_size.saturating_add(1);
        let mut candidates = query_regular_page_candidates(
            &connection,
            account_id,
            &mailbox,
            role,
            state.uid_validity,
            decoded_cursor.as_ref(),
            search_pattern.as_deref(),
            fetch_limit,
            flagged_only,
        )?;
        candidates.extend(query_pending_page_candidates(
            &connection,
            account_id,
            role,
            decoded_cursor.as_ref(),
            search_pattern.as_deref(),
            fetch_limit,
            flagged_only,
        )?);
        candidates.sort_by(compare_page_candidates);
        let has_more_local = candidates.len() > page_size;
        candidates.truncate(page_size);
        let items = candidates
            .iter()
            .map(|candidate| candidate.item.clone())
            .collect::<Vec<_>>();
        let remote_history_state = if has_more_local {
            RemoteHistoryState::NotChecked
        } else if history_complete {
            RemoteHistoryState::Complete
        } else {
            RemoteHistoryState::MayHaveMore
        };
        let end_reached = !has_more_local && remote_history_state == RemoteHistoryState::Complete;
        let next_cursor = if end_reached {
            None
        } else {
            let boundary = candidates.last().map(|candidate| {
                (
                    Some(candidate.sort_at.clone()),
                    Some(candidate.uid),
                    Some(candidate.id),
                )
            });
            let (sort_at, uid, id) = boundary.unwrap_or_else(|| {
                decoded_cursor
                    .as_ref()
                    .map(|cursor| (cursor.sort_at.clone(), cursor.uid, cursor.id))
                    .unwrap_or((None, None, None))
            });
            let remote_before_uid = history_before_uid
                .or(state.uid_next)
                .or_else(|| state.highest_uid.and_then(|uid| uid.checked_add(1)))
                .unwrap_or(1);
            Some(issue_message_cursor(
                &connection,
                MessageCursorPayload {
                    account_id: account_id.to_owned(),
                    mailbox,
                    role,
                    uid_validity: state.uid_validity,
                    query_normalized: query.unwrap_or_default(),
                    sort_at,
                    uid,
                    id,
                    remote_before_uid,
                    flagged_only,
                },
            )?)
        };
        Ok(MessagePage {
            items,
            next_cursor,
            has_more_local,
            remote_history_state,
            end_reached,
        })
    }

    /// Returns the local rows now available behind an existing continuation
    /// cursor. The backend performs any bounded IMAP history fetch first.
    pub(crate) fn load_older_mailbox_page(
        &self,
        account_id: &str,
        role: MailboxRole,
        cursor: &MessagePageCursor,
        page_size: usize,
        query: Option<&str>,
    ) -> Result<MessagePage> {
        self.list_mailbox_page(account_id, role, Some(cursor), page_size, query)
    }

    pub(crate) fn load_older_starred_mailbox_page(
        &self,
        account_id: &str,
        role: MailboxRole,
        cursor: &MessagePageCursor,
        page_size: usize,
        query: Option<&str>,
    ) -> Result<MessagePage> {
        self.list_starred_mailbox_page(account_id, role, Some(cursor), page_size, query)
    }

    /// Extracts only the bounded server-history context needed by the backend.
    /// Callers still validate the cursor again through `list_mailbox_page` after
    /// synchronizing older summaries.
    pub(crate) fn message_page_cursor_context(
        &self,
        cursor: &MessagePageCursor,
    ) -> Result<MessagePageCursorContext> {
        let connection = self.connection()?;
        let payload = load_message_cursor(&connection, cursor)?;
        Ok(MessagePageCursorContext {
            account_id: payload.account_id,
            mailbox: payload.mailbox,
            role: payload.role,
            uid_validity: payload.uid_validity,
            remote_before_uid: Some(payload.remote_before_uid),
            flagged_only: payload.flagged_only,
        })
    }

    pub(crate) fn list_inbox(
        &self,
        account_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<InboxMessage>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {MESSAGE_SUMMARY_COLUMNS} FROM messages
             WHERE account_id = ?1 AND mailbox = 'INBOX' COLLATE NOCASE
             ORDER BY COALESCE(internal_date, sent_at, synced_at) DESC, uid DESC
             LIMIT ?2 OFFSET ?3"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(
            params![account_id, usize_to_i64(limit), usize_to_i64(offset)],
            row_to_message,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn list_mailbox(
        &self,
        account_id: &str,
        mailbox: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<InboxMessage>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {MESSAGE_SUMMARY_COLUMNS} FROM messages
             WHERE account_id = ?1 AND mailbox = ?2
             ORDER BY COALESCE(internal_date, sent_at, synced_at) DESC, uid DESC
             LIMIT ?3 OFFSET ?4"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(
            params![
                account_id,
                mailbox,
                usize_to_i64(limit),
                usize_to_i64(offset)
            ],
            row_to_message,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Returns body-free summaries for all locally cached mailboxes belonging
    /// to one account. Contact aggregation deliberately happens over parsed
    /// MailAddress values in Rust instead of substring matching JSON in SQL.
    pub(crate) fn list_contact_source_messages(
        &self,
        account_id: &str,
    ) -> Result<Vec<ContactMessageSource>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {CONTACT_MESSAGE_SUMMARY_COLUMNS}, public_id FROM messages
             WHERE account_id = ?1
             ORDER BY COALESCE(internal_date, sent_at, synced_at) DESC, uid DESC, id DESC"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![account_id], |row| {
            Ok(ContactMessageSource {
                public_id: row.get(23)?,
                message: row_to_message(row)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Returns only the newest body-free summaries involving one normalized
    /// address. Filtering and limiting in SQLite avoids decoding every cached
    /// message into Rust whenever the selected contact changes.
    pub(crate) fn list_contact_source_messages_for_email(
        &self,
        account_id: &str,
        email: &str,
        limit: usize,
    ) -> Result<Vec<ContactMessageSource>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {CONTACT_MESSAGE_SUMMARY_COLUMNS}, public_id FROM messages
             WHERE id IN (
                 SELECT message_id FROM message_contact_emails
                 WHERE account_id = ?1 AND email = ?2
             )
             ORDER BY COALESCE(internal_date, sent_at, synced_at) DESC, uid DESC, id DESC
             LIMIT ?3"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![account_id, email, usize_to_i64(limit)], |row| {
            Ok(ContactMessageSource {
                public_id: row.get(23)?,
                message: row_to_message(row)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn get_message(&self, id: i64) -> Result<InboxMessage> {
        let connection = self.connection()?;
        let sql = format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1");
        connection
            .query_row(&sql, params![id], row_to_message)
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "message",
                id: id.to_string(),
            })
    }

    /// Resolves an opaque desktop message identity only inside its owning
    /// account. The returned model retains the internal row ID for Rust-only
    /// queue and cursor work.
    pub(crate) fn get_message_by_public_id(
        &self,
        expected_account_id: &str,
        public_id: &str,
    ) -> Result<InboxMessage> {
        validate_message_public_id(public_id)?;
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages
             WHERE account_id = ?1 AND public_id = ?2"
        );
        connection
            .query_row(
                &sql,
                params![expected_account_id, public_id],
                row_to_message,
            )
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "message",
                id: "opaque-id".to_owned(),
            })
    }

    /// Converts a Rust-only local row identity to its opaque desktop identity.
    /// Both predicates are required because SQLite row IDs can collide across
    /// the separate databases used by different accounts.
    pub(crate) fn message_public_id_by_local_id(
        &self,
        expected_account_id: &str,
        local_id: i64,
    ) -> Result<String> {
        if local_id <= 0 {
            return Err(MailError::NotFound {
                entity: "message",
                id: "local-id".to_owned(),
            });
        }
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT public_id FROM messages WHERE account_id = ?1 AND id = ?2",
                params![expected_account_id, local_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "message",
                id: "local-id".to_owned(),
            })
    }

    pub(crate) fn get_message_by_uid(
        &self,
        account_id: &str,
        mailbox: &str,
        uid: u32,
    ) -> Result<InboxMessage> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages
             WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3"
        );
        connection
            .query_row(&sql, params![account_id, mailbox, uid], row_to_message)
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "message UID",
                id: format!("{account_id}:{mailbox}/{uid}"),
            })
    }

    pub(crate) fn find_message_by_message_id(
        &self,
        account_id: &str,
        message_id: &str,
    ) -> Result<Option<InboxMessage>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {MESSAGE_SUMMARY_COLUMNS} FROM messages
             WHERE account_id = ?1
               AND lower(trim(message_id, '<> ')) = lower(trim(?2, '<> '))
             ORDER BY body_fetched DESC,
                      COALESCE(internal_date, sent_at, synced_at) DESC
             LIMIT 1"
        );
        connection
            .query_row(&sql, params![account_id, message_id], row_to_message)
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn legacy_reply_parent_candidates(
        &self,
        account_id: &str,
        excluded_id: i64,
        limit: usize,
    ) -> Result<Vec<InboxMessage>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {MESSAGE_SUMMARY_COLUMNS} FROM messages
             WHERE account_id = ?1 AND id <> ?2 AND body_text IS NOT NULL
             ORDER BY COALESCE(internal_date, sent_at, synced_at) DESC, uid DESC
             LIMIT ?3"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(
            params![account_id, excluded_id, usize_to_i64(limit)],
            row_to_message,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn mailbox_preview_backfill_candidates(
        &self,
        account_id: &str,
        mailbox: &str,
        limit: usize,
    ) -> Result<Vec<u32>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT uid FROM messages
             WHERE account_id = ?1
               AND mailbox = ?2
               AND preview_fetched = 0
               AND body_fetched = 0
             ORDER BY COALESCE(internal_date, sent_at, synced_at) DESC, uid DESC
             LIMIT ?3",
        )?;
        let rows = statement
            .query_map(params![account_id, mailbox, usize_to_i64(limit)], |row| {
                row.get(0)
            })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn mailbox_body_prefetch_candidates(
        &self,
        account_id: &str,
        mailbox: &str,
        limit: usize,
        max_message_bytes: u32,
    ) -> Result<Vec<(u32, u32)>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT uid, size_bytes FROM messages
             WHERE account_id = ?1
               AND mailbox = ?2
               AND body_fetched = 0
               AND size_bytes > 0
               AND size_bytes <= ?3
             ORDER BY COALESCE(internal_date, sent_at, synced_at) DESC, uid DESC
             LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![account_id, mailbox, max_message_bytes, usize_to_i64(limit)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn mailbox_body_prefetch_page_candidates(
        &self,
        account_id: &str,
        mailbox: &str,
        limit: usize,
    ) -> Result<Vec<(String, u32)>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT public_id, size_bytes FROM messages
             WHERE account_id = ?1
               AND mailbox = ?2
               AND body_fetched = 0
               AND size_bytes > 0
             ORDER BY COALESCE(internal_date, sent_at, synced_at) DESC, uid DESC
             LIMIT ?3",
        )?;
        let rows = statement
            .query_map(params![account_id, mailbox, usize_to_i64(limit)], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn touch_message_body_access(&self, message_id: i64) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE messages
             SET body_last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND body_fetched = 1",
            params![message_id],
        )?;
        Ok(())
    }

    pub(crate) fn message_body_cache_usage_bytes(&self, account_id: &str) -> Result<u64> {
        let connection = self.connection()?;
        let total: i64 = connection.query_row(
            "SELECT COALESCE(SUM(body_cached_bytes), 0)
             FROM messages
             WHERE account_id = ?1 AND body_fetched = 1",
            params![account_id],
            |row| row.get(0),
        )?;
        Ok(u64::try_from(total).unwrap_or(u64::MAX))
    }

    pub(crate) fn evict_message_body_cache_to_limit(
        &self,
        account_id: &str,
        max_total_bytes: u64,
        protected_message_id: Option<i64>,
    ) -> Result<usize> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut total: i64 = transaction.query_row(
            "SELECT COALESCE(SUM(body_cached_bytes), 0)
             FROM messages
             WHERE account_id = ?1 AND body_fetched = 1",
            params![account_id],
            |row| row.get(0),
        )?;
        let target = i64::try_from(max_total_bytes).unwrap_or(i64::MAX);
        let mut evicted = 0usize;
        while total > target {
            let candidate = transaction
                .query_row(
                    "SELECT id, body_cached_bytes
                     FROM messages
                     WHERE account_id = ?1
                       AND body_fetched = 1
                       AND (?2 IS NULL OR id <> ?2)
                     ORDER BY COALESCE(body_last_accessed_at, '') ASC, id ASC
                     LIMIT 1",
                    params![account_id, protected_message_id],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?;
            let Some((message_id, cached_bytes)) = candidate else {
                break;
            };
            transaction.execute(
                "UPDATE messages
                 SET body_text = NULL,
                     body_html = NULL,
                     attachment_names_json = '[]',
                     body_fetched = 0,
                     raw_rfc822 = X'',
                     body_cached_bytes = 0,
                     body_last_accessed_at = NULL
                 WHERE id = ?1",
                params![message_id],
            )?;
            total = total.saturating_sub(cached_bytes.max(0));
            evicted += 1;
        }
        transaction.commit()?;
        Ok(evicted)
    }

    pub(crate) fn count_messages(&self, account_id: &str, mailbox: &str) -> Result<usize> {
        let connection = self.connection()?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND mailbox = ?2",
            params![account_id, mailbox],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    #[cfg(test)]
    pub(crate) fn save_draft_record(&self, record: &DraftRecord) -> Result<()> {
        let draft = &record.draft;
        let connection = self.connection()?;
        let expected_snapshot = DraftVersionSnapshot {
            account_id: draft.account_id.clone(),
            draft_id: draft.id.clone(),
            local_version: record.local_version,
            protocol_revision: record.revision,
            request: draft.compose_request(),
            has_unsupported_content: draft.has_unsupported_content,
        };
        let existing_snapshot = query_draft_version_snapshot(
            &connection,
            &draft.account_id,
            &draft.id,
            record.local_version,
        )?;
        if existing_snapshot
            .as_ref()
            .is_some_and(|existing| existing != &expected_snapshot)
        {
            return Err(MailError::Validation(
                "an immutable draft version snapshot cannot be rewritten".to_owned(),
            ));
        }
        connection.execute(
            "INSERT INTO drafts (
                 id, account_id, to_json, cc_json, bcc_json, subject, body_text,
                 compose_format_json, reply_context_json, status, remote_mailbox, remote_uid,
                 created_at, updated_at, raw_rfc822,
                 local_version, has_unsupported_content, revision, synced_revision,
                 remote_uid_validity, is_deleted
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21
             )
             ON CONFLICT(id) DO UPDATE SET
                 account_id = excluded.account_id,
                 to_json = excluded.to_json,
                 cc_json = excluded.cc_json,
                 bcc_json = excluded.bcc_json,
                 subject = excluded.subject,
                 body_text = excluded.body_text,
                 compose_format_json = excluded.compose_format_json,
                 reply_context_json = excluded.reply_context_json,
                 status = excluded.status,
                 remote_mailbox = excluded.remote_mailbox,
                 remote_uid = excluded.remote_uid,
                 updated_at = excluded.updated_at,
                 raw_rfc822 = excluded.raw_rfc822,
                 local_version = excluded.local_version,
                 has_unsupported_content = excluded.has_unsupported_content,
                 revision = excluded.revision,
                 synced_revision = excluded.synced_revision,
                 remote_uid_validity = excluded.remote_uid_validity,
                 is_deleted = excluded.is_deleted",
            params![
                draft.id,
                draft.account_id,
                encode_json(&draft.to)?,
                encode_json(&draft.cc)?,
                encode_json(&draft.bcc)?,
                draft.subject,
                draft.body_text,
                encode_json(&draft.format)?,
                draft.reply_context.as_ref().map(encode_json).transpose()?,
                draft.status,
                draft.remote_mailbox,
                draft.remote_uid,
                draft.created_at,
                draft.updated_at,
                draft.raw_rfc822,
                u64_to_i64(record.local_version),
                draft.has_unsupported_content,
                u64_to_i64(record.revision),
                u64_to_i64(record.synced_revision),
                record.remote_uid_validity,
                record.is_deleted,
            ],
        )?;
        if existing_snapshot.is_none() {
            insert_draft_version_snapshot(&connection, record)?;
        }
        Ok(())
    }

    /// Inserts a draft only if no row with the same stable id already exists.
    pub(crate) fn insert_draft_if_absent(&self, record: &DraftRecord) -> Result<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let inserted = insert_draft_record_if_absent(&transaction, record)? == 1;
        transaction.commit()?;
        Ok(inserted)
    }

    /// Replaces a draft only while every local sync token still matches. This
    /// is shared by local editing and remote reconciliation. An optional
    /// conflict copy is inserted in the same transaction and therefore is
    /// never created after a CAS miss.
    pub(crate) fn replace_draft_if_unchanged(
        &self,
        expected: &DraftRecord,
        replacement: &DraftRecord,
        conflict_copy: Option<&DraftRecord>,
    ) -> Result<bool> {
        validate_same_draft_identity(expected, replacement)?;
        if conflict_copy.is_some_and(|copy| {
            copy.draft.account_id != expected.draft.account_id || copy.draft.id == expected.draft.id
        }) {
            return Err(MailError::Validation(
                "a draft conflict copy must use the same account and a new id".to_owned(),
            ));
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let to_json = encode_json(&replacement.draft.to)?;
        let cc_json = encode_json(&replacement.draft.cc)?;
        let bcc_json = encode_json(&replacement.draft.bcc)?;
        let changed = transaction.execute(
            "UPDATE drafts SET
                 to_json = :to_json,
                 cc_json = :cc_json,
                 bcc_json = :bcc_json,
                 subject = :subject,
                 body_text = :body_text,
                 compose_format_json = :compose_format_json,
                 reply_context_json = :reply_context_json,
                 status = :replacement_status,
                 remote_mailbox = :replacement_mailbox,
                 remote_uid = :replacement_uid,
                 updated_at = :updated_at,
                 raw_rfc822 = :raw_rfc822,
                 local_version = :replacement_local_version,
                 has_unsupported_content = :replacement_has_unsupported_content,
                 revision = :replacement_revision,
                 synced_revision = :replacement_synced_revision,
                 remote_uid_validity = :replacement_uid_validity,
                 is_deleted = :replacement_is_deleted
             WHERE id = :id
               AND account_id = :account_id
               AND local_version = :expected_local_version
               AND revision = :expected_revision
               AND synced_revision = :expected_synced_revision
               AND status = :expected_status
               AND is_deleted = :expected_is_deleted
               AND remote_mailbox IS :expected_mailbox
               AND remote_uid IS :expected_uid
               AND remote_uid_validity IS :expected_uid_validity",
            named_params! {
                ":id": expected.draft.id,
                ":account_id": expected.draft.account_id,
                ":to_json": to_json,
                ":cc_json": cc_json,
                ":bcc_json": bcc_json,
                ":subject": replacement.draft.subject,
                ":body_text": replacement.draft.body_text,
                ":compose_format_json": encode_json(&replacement.draft.format)?,
                ":reply_context_json": replacement
                    .draft
                    .reply_context
                    .as_ref()
                    .map(encode_json)
                    .transpose()?,
                ":replacement_status": replacement.draft.status,
                ":replacement_mailbox": replacement.draft.remote_mailbox,
                ":replacement_uid": replacement.draft.remote_uid,
                ":updated_at": replacement.draft.updated_at,
                ":raw_rfc822": replacement.draft.raw_rfc822,
                ":replacement_local_version": u64_to_i64(replacement.local_version),
                ":replacement_has_unsupported_content": replacement.draft.has_unsupported_content,
                ":replacement_revision": u64_to_i64(replacement.revision),
                ":replacement_synced_revision": u64_to_i64(replacement.synced_revision),
                ":replacement_uid_validity": replacement.remote_uid_validity,
                ":replacement_is_deleted": replacement.is_deleted,
                ":expected_local_version": u64_to_i64(expected.local_version),
                ":expected_revision": u64_to_i64(expected.revision),
                ":expected_synced_revision": u64_to_i64(expected.synced_revision),
                ":expected_status": expected.draft.status,
                ":expected_is_deleted": expected.is_deleted,
                ":expected_mailbox": expected.draft.remote_mailbox,
                ":expected_uid": expected.draft.remote_uid,
                ":expected_uid_validity": expected.remote_uid_validity,
            },
        )?;
        if changed == 0 {
            return Ok(false);
        }
        if changed != 1 {
            return Err(MailError::Database(rusqlite::Error::ExecuteReturnedResults));
        }
        if replacement.local_version == expected.local_version {
            let persisted = query_draft_version_snapshot(
                &transaction,
                &replacement.draft.account_id,
                &replacement.draft.id,
                replacement.local_version,
            )?
            .ok_or_else(|| {
                MailError::Validation(
                    "the immutable draft version snapshot is unavailable".to_owned(),
                )
            })?;
            let replacement_snapshot = DraftVersionSnapshot {
                account_id: replacement.draft.account_id.clone(),
                draft_id: replacement.draft.id.clone(),
                local_version: replacement.local_version,
                protocol_revision: replacement.revision,
                request: replacement.draft.compose_request(),
                has_unsupported_content: replacement.draft.has_unsupported_content,
            };
            if persisted != replacement_snapshot {
                return Err(MailError::Validation(
                    "an immutable draft version snapshot cannot be rewritten".to_owned(),
                ));
            }
        } else {
            insert_draft_version_snapshot(&transaction, replacement)?;
            clone_draft_attachment_rows(
                &transaction,
                &expected.draft.account_id,
                &expected.draft.id,
                expected.local_version,
                &replacement.draft.id,
                replacement.local_version,
            )?;
            clone_draft_version_forward_context_ref(
                &transaction,
                &expected.draft.account_id,
                &expected.draft.id,
                expected.local_version,
                &replacement.draft.id,
                replacement.local_version,
            )?;
        }
        if let Some(copy) = conflict_copy
            && insert_draft_record_if_absent(&transaction, copy)? != 1
        {
            return Err(MailError::Validation(
                "could not reserve a unique draft conflict copy id".to_owned(),
            ));
        }
        if let Some(copy) = conflict_copy {
            clone_draft_attachment_rows(
                &transaction,
                &expected.draft.account_id,
                &expected.draft.id,
                expected.local_version,
                &copy.draft.id,
                copy.local_version,
            )?;
            clone_forward_context_rows(
                &transaction,
                &expected.draft.account_id,
                &expected.draft.id,
                &copy.draft.id,
            )?;
            clone_draft_version_forward_context_ref(
                &transaction,
                &expected.draft.account_id,
                &expected.draft.id,
                expected.local_version,
                &copy.draft.id,
                copy.local_version,
            )?;
        }
        transaction.commit()?;
        Ok(true)
    }

    pub(crate) fn get_draft(&self, id: &str) -> Result<Draft> {
        let connection = self.connection()?;
        let sql = format!("SELECT {DRAFT_COLUMNS} FROM drafts WHERE id = ?1");
        connection
            .query_row(&sql, params![id], row_to_draft)
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "draft",
                id: id.to_owned(),
            })
    }

    pub(crate) fn get_draft_record(&self, id: &str) -> Result<DraftRecord> {
        let connection = self.connection()?;
        let sql = format!("SELECT {DRAFT_SYNC_COLUMNS} FROM drafts WHERE id = ?1");
        connection
            .query_row(&sql, params![id], row_to_draft_record)
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "draft",
                id: id.to_owned(),
            })
    }

    pub(crate) fn draft_version_snapshot(
        &self,
        account_id: &str,
        draft_id: &str,
        local_version: u64,
    ) -> Result<Option<DraftVersionSnapshot>> {
        if local_version == 0 {
            return Err(MailError::Validation(
                "a draft version snapshot must be positive".to_owned(),
            ));
        }
        let connection = self.connection()?;
        query_draft_version_snapshot(&connection, account_id, draft_id, local_version)
    }

    pub(crate) fn list_drafts(&self, account_id: &str) -> Result<Vec<Draft>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {DRAFT_COLUMNS} FROM drafts
             WHERE account_id = ?1 AND is_deleted = 0
             ORDER BY updated_at DESC, id DESC"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![account_id], row_to_draft)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn list_draft_records(&self, account_id: &str) -> Result<Vec<DraftRecord>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {DRAFT_SYNC_COLUMNS} FROM drafts
             WHERE account_id = ?1 ORDER BY updated_at DESC, id DESC"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![account_id], row_to_draft_record)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Returns the attachment set only when the caller's draft version is
    /// still current. `Some([])` is a valid exact empty set; `None` is a CAS
    /// miss and must never be treated as an empty newer draft.
    pub(crate) fn list_draft_attachments_at_version(
        &self,
        account_id: &str,
        draft_id: &str,
        expected_local_version: u64,
    ) -> Result<Option<DraftAttachmentVersionSnapshot>> {
        if expected_local_version == 0 {
            return Err(MailError::Validation(
                "a draft attachment version must be positive".to_owned(),
            ));
        }
        let connection = self.connection()?;
        if !draft_version_exists(
            &connection,
            account_id,
            draft_id,
            expected_local_version,
            false,
        )? {
            return Ok(None);
        }
        Ok(Some(DraftAttachmentVersionSnapshot {
            local_version: expected_local_version,
            attachments: query_draft_attachments(
                &connection,
                account_id,
                draft_id,
                expected_local_version,
            )?,
        }))
    }

    /// Imports new immutable blob records and appends their associations while
    /// advancing the exact editable draft version in one transaction. A stale
    /// version returns `None` without registering any blob or changing any
    /// association.
    #[cfg(test)]
    pub(crate) fn add_draft_attachments_if_local_version(
        &self,
        account_id: &str,
        draft_id: &str,
        expected_local_version: u64,
        additions: &[NewDraftAttachment],
        updated_at: &str,
    ) -> Result<Option<DraftAttachmentVersionSnapshot>> {
        self.add_draft_attachments_with_raw_if_local_version(
            account_id,
            draft_id,
            expected_local_version,
            additions,
            updated_at,
            None,
        )
    }

    /// Production attachment edits persist the MIME bytes built from the same
    /// exact attachment snapshot in the version-advancing transaction.
    pub(crate) fn add_draft_attachments_and_raw_if_local_version(
        &self,
        account_id: &str,
        draft_id: &str,
        expected_local_version: u64,
        additions: &[NewDraftAttachment],
        updated_at: &str,
        raw_rfc822: &[u8],
    ) -> Result<Option<DraftAttachmentVersionSnapshot>> {
        self.add_draft_attachments_with_raw_if_local_version(
            account_id,
            draft_id,
            expected_local_version,
            additions,
            updated_at,
            Some(raw_rfc822),
        )
    }

    fn add_draft_attachments_with_raw_if_local_version(
        &self,
        account_id: &str,
        draft_id: &str,
        expected_local_version: u64,
        additions: &[NewDraftAttachment],
        updated_at: &str,
        raw_rfc822: Option<&[u8]>,
    ) -> Result<Option<DraftAttachmentVersionSnapshot>> {
        if additions.is_empty() {
            return Err(MailError::Validation(
                "at least one managed attachment is required".to_owned(),
            ));
        }
        let next_local_version = expected_local_version
            .checked_add(1)
            .ok_or_else(|| MailError::Validation("draft local version limit reached".to_owned()))?;
        validate_new_draft_attachments(additions)?;

        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_combined_draft_attachment_metadata(
            &transaction,
            account_id,
            draft_id,
            expected_local_version,
            additions,
        )?;
        let changed = transaction.execute(
            "UPDATE drafts
             SET local_version = ?4,
                 revision = revision + 1,
                 updated_at = ?5,
                 raw_rfc822 = COALESCE(?6, raw_rfc822),
                 status = 'local',
                 has_unsupported_content = 0
             WHERE account_id = ?1 AND id = ?2 AND local_version = ?3
               AND is_deleted = 0 AND status != 'sent'",
            params![
                account_id,
                draft_id,
                u64_to_i64(expected_local_version),
                u64_to_i64(next_local_version),
                updated_at,
                raw_rfc822,
            ],
        )?;
        if changed == 0 {
            transaction.commit()?;
            return Ok(None);
        }
        if changed != 1 {
            return Err(MailError::Database(rusqlite::Error::ExecuteReturnedResults));
        }
        insert_current_draft_version_snapshot(&transaction, account_id, draft_id)?;
        clone_draft_attachment_rows(
            &transaction,
            account_id,
            draft_id,
            expected_local_version,
            draft_id,
            next_local_version,
        )?;
        clone_draft_version_forward_context_ref(
            &transaction,
            account_id,
            draft_id,
            expected_local_version,
            draft_id,
            next_local_version,
        )?;

        let mut next_position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0)
             FROM draft_attachment_refs
             WHERE account_id = ?1 AND draft_id = ?2 AND draft_local_version = ?3",
            params![account_id, draft_id, u64_to_i64(next_local_version)],
            |row| row.get(0),
        )?;
        for addition in additions {
            let imported = &addition.imported;
            let inserted = transaction.execute(
                "INSERT INTO managed_attachment_blobs (
                     id, account_id, origin_draft_id, internal_name, name, mime_type,
                     size_bytes, sha256_hex, disposition, transfer_encoding
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'attachment', 'base64')
                 ON CONFLICT(id) DO NOTHING",
                params![
                    imported.id,
                    account_id,
                    draft_id,
                    imported.internal_name,
                    imported.name,
                    imported.mime_type,
                    u64_to_i64(imported.size_bytes),
                    imported.sha256_hex,
                ],
            )?;
            if inserted != 1 {
                return Err(MailError::Validation(
                    "a managed attachment identifier collision was detected".to_owned(),
                ));
            }
            transaction.execute(
                "INSERT INTO draft_attachment_refs (
                     account_id, draft_id, draft_local_version, position,
                     blob_id, source_attachment_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    account_id,
                    draft_id,
                    u64_to_i64(next_local_version),
                    next_position,
                    imported.id,
                    addition.source_attachment_id,
                ],
            )?;
            next_position = next_position.checked_add(1).ok_or_else(|| {
                MailError::Validation("draft attachment position limit reached".to_owned())
            })?;
        }
        let attachments =
            query_draft_attachments(&transaction, account_id, draft_id, next_local_version)?;
        transaction.commit()?;
        Ok(Some(DraftAttachmentVersionSnapshot {
            local_version: next_local_version,
            attachments,
        }))
    }

    /// Removes one exact association and advances the draft version. A stale
    /// caller returns `None` before the attachment lookup, so it can never
    /// remove an attachment from a newer draft.
    #[cfg(test)]
    pub(crate) fn remove_draft_attachment_if_local_version(
        &self,
        account_id: &str,
        draft_id: &str,
        attachment_id: &str,
        expected_local_version: u64,
        updated_at: &str,
    ) -> Result<Option<DraftAttachmentVersionSnapshot>> {
        self.remove_draft_attachment_with_raw_if_local_version(
            account_id,
            draft_id,
            attachment_id,
            expected_local_version,
            updated_at,
            None,
        )
    }

    pub(crate) fn remove_draft_attachment_and_raw_if_local_version(
        &self,
        account_id: &str,
        draft_id: &str,
        attachment_id: &str,
        expected_local_version: u64,
        updated_at: &str,
        raw_rfc822: &[u8],
    ) -> Result<Option<DraftAttachmentVersionSnapshot>> {
        self.remove_draft_attachment_with_raw_if_local_version(
            account_id,
            draft_id,
            attachment_id,
            expected_local_version,
            updated_at,
            Some(raw_rfc822),
        )
    }

    fn remove_draft_attachment_with_raw_if_local_version(
        &self,
        account_id: &str,
        draft_id: &str,
        attachment_id: &str,
        expected_local_version: u64,
        updated_at: &str,
        raw_rfc822: Option<&[u8]>,
    ) -> Result<Option<DraftAttachmentVersionSnapshot>> {
        let next_local_version = expected_local_version
            .checked_add(1)
            .ok_or_else(|| MailError::Validation("draft local version limit reached".to_owned()))?;
        validate_opaque_attachment_id(attachment_id)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !draft_version_exists(
            &transaction,
            account_id,
            draft_id,
            expected_local_version,
            true,
        )? {
            transaction.commit()?;
            return Ok(None);
        }
        let changed = transaction.execute(
            "UPDATE drafts
             SET local_version = ?4,
                 revision = revision + 1,
                 updated_at = ?5,
                 raw_rfc822 = COALESCE(?6, raw_rfc822),
                 status = 'local',
                 has_unsupported_content = 0
             WHERE account_id = ?1 AND id = ?2 AND local_version = ?3
               AND is_deleted = 0 AND status != 'sent'",
            params![
                account_id,
                draft_id,
                u64_to_i64(expected_local_version),
                u64_to_i64(next_local_version),
                updated_at,
                raw_rfc822,
            ],
        )?;
        if changed != 1 {
            return Err(MailError::Database(rusqlite::Error::ExecuteReturnedResults));
        }
        insert_current_draft_version_snapshot(&transaction, account_id, draft_id)?;
        clone_draft_attachment_rows(
            &transaction,
            account_id,
            draft_id,
            expected_local_version,
            draft_id,
            next_local_version,
        )?;
        clone_draft_version_forward_context_ref(
            &transaction,
            account_id,
            draft_id,
            expected_local_version,
            draft_id,
            next_local_version,
        )?;
        let removed = transaction.execute(
            "DELETE FROM draft_attachment_refs
             WHERE account_id = ?1 AND draft_id = ?2
               AND draft_local_version = ?3 AND blob_id = ?4",
            params![
                account_id,
                draft_id,
                u64_to_i64(next_local_version),
                attachment_id,
            ],
        )?;
        if removed != 1 {
            return Err(privacy_safe_not_found("draft attachment"));
        }
        let attachments =
            query_draft_attachments(&transaction, account_id, draft_id, next_local_version)?;
        transaction.commit()?;
        Ok(Some(DraftAttachmentVersionSnapshot {
            local_version: next_local_version,
            attachments,
        }))
    }

    #[cfg(test)]
    pub(crate) fn clone_draft_attachments_to_conflict(
        &self,
        account_id: &str,
        source_draft_id: &str,
        source_local_version: u64,
        conflict_draft_id: &str,
        conflict_local_version: u64,
    ) -> Result<bool> {
        if source_draft_id == conflict_draft_id {
            return Err(MailError::Validation(
                "a draft conflict copy requires a distinct id".to_owned(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !draft_version_exists(
            &transaction,
            account_id,
            source_draft_id,
            source_local_version,
            false,
        )? || !draft_version_exists(
            &transaction,
            account_id,
            conflict_draft_id,
            conflict_local_version,
            false,
        )? {
            transaction.commit()?;
            return Ok(false);
        }
        clone_draft_attachment_rows(
            &transaction,
            account_id,
            source_draft_id,
            source_local_version,
            conflict_draft_id,
            conflict_local_version,
        )?;
        clone_forward_context_rows(&transaction, account_id, source_draft_id, conflict_draft_id)?;
        clone_draft_version_forward_context_ref(
            &transaction,
            account_id,
            source_draft_id,
            source_local_version,
            conflict_draft_id,
            conflict_local_version,
        )?;
        transaction.commit()?;
        Ok(true)
    }

    /// Creates one fully prepared forward draft in a single SQLite
    /// transaction. The complete source RFC822 is rechecked under the same
    /// write lock so a force-refresh cannot silently change the message after
    /// extraction but before the immutable context is persisted.
    pub(crate) fn insert_prepared_forward_if_source_unchanged(
        &self,
        source_message_row_id: i64,
        expected_source_raw: &[u8],
        record: &DraftRecord,
        context: &ForwardContext,
        additions: &[NewDraftAttachment],
    ) -> Result<PreparedForwardInsert> {
        validate_forward_context(context)?;
        validate_new_draft_attachments(additions)?;
        if record.draft.account_id.is_empty()
            || record.local_version == 0
            || record.local_version != record.draft.local_version
            || record.is_deleted
        {
            return Err(MailError::Validation(
                "the prepared forward draft record is invalid".to_owned(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source_unchanged: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM messages
                 WHERE id = ?1 AND account_id = ?2 AND body_fetched = 1
                   AND raw_rfc822 = ?3
             )",
            params![
                source_message_row_id,
                record.draft.account_id,
                expected_source_raw
            ],
            |row| row.get(0),
        )?;
        if !source_unchanged {
            transaction.commit()?;
            return Ok(PreparedForwardInsert::SourceChanged);
        }
        if insert_draft_record_if_absent(&transaction, record)? != 1 {
            transaction.commit()?;
            return Ok(PreparedForwardInsert::IdCollision);
        }
        insert_forward_context_rows(
            &transaction,
            &record.draft.account_id,
            &record.draft.id,
            context,
        )?;
        attach_forward_context_to_all_versions(
            &transaction,
            &record.draft.account_id,
            &record.draft.id,
        )?;
        insert_new_draft_attachment_rows(
            &transaction,
            &record.draft.account_id,
            &record.draft.id,
            record.local_version,
            0,
            additions,
        )?;
        transaction.commit()?;
        Ok(PreparedForwardInsert::Inserted)
    }

    /// Inserts a stale editor branch without changing the canonical draft.
    /// Existing immutable refs and forward context are cloned from one exact
    /// current snapshot, and newly selected bytes are registered atomically.
    pub(crate) fn insert_attachment_conflict_if_source_unchanged(
        &self,
        source: &DraftRecord,
        source_attachment_local_version: u64,
        conflict: &DraftRecord,
        additions: &[NewDraftAttachment],
    ) -> Result<bool> {
        validate_new_draft_attachments(additions)?;
        if source.draft.account_id != conflict.draft.account_id
            || source.draft.id == conflict.draft.id
            || conflict.local_version == 0
            || conflict.local_version != conflict.draft.local_version
        {
            return Err(MailError::Validation(
                "the draft attachment conflict identity is invalid".to_owned(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !draft_version_exists(
            &transaction,
            &source.draft.account_id,
            &source.draft.id,
            source_attachment_local_version,
            false,
        )? {
            return Err(MailError::Validation(
                "the exact stale draft attachment snapshot is unavailable".to_owned(),
            ));
        }
        validate_combined_draft_attachment_metadata(
            &transaction,
            &source.draft.account_id,
            &source.draft.id,
            source_attachment_local_version,
            additions,
        )?;
        let source_is_current: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM drafts
                 WHERE account_id = ?1 AND id = ?2 AND local_version = ?3
                   AND revision = ?4 AND status = ?5 AND is_deleted = ?6
                   AND raw_rfc822 = ?7
             )",
            params![
                source.draft.account_id,
                source.draft.id,
                u64_to_i64(source.local_version),
                u64_to_i64(source.revision),
                source.draft.status,
                source.is_deleted,
                source.draft.raw_rfc822,
            ],
            |row| row.get(0),
        )?;
        if !source_is_current {
            transaction.commit()?;
            return Ok(false);
        }
        if insert_draft_record_if_absent(&transaction, conflict)? != 1 {
            transaction.commit()?;
            return Ok(false);
        }
        clone_draft_attachment_rows(
            &transaction,
            &source.draft.account_id,
            &source.draft.id,
            source_attachment_local_version,
            &conflict.draft.id,
            conflict.local_version,
        )?;
        clone_forward_context_rows(
            &transaction,
            &source.draft.account_id,
            &source.draft.id,
            &conflict.draft.id,
        )?;
        clone_draft_version_forward_context_ref(
            &transaction,
            &source.draft.account_id,
            &source.draft.id,
            source_attachment_local_version,
            &conflict.draft.id,
            conflict.local_version,
        )?;
        let next_position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0)
             FROM draft_attachment_refs
             WHERE account_id = ?1 AND draft_id = ?2 AND draft_local_version = ?3",
            params![
                conflict.draft.account_id,
                conflict.draft.id,
                u64_to_i64(conflict.local_version)
            ],
            |row| row.get(0),
        )?;
        insert_new_draft_attachment_rows(
            &transaction,
            &conflict.draft.account_id,
            &conflict.draft.id,
            conflict.local_version,
            next_position,
            additions,
        )?;
        transaction.commit()?;
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn list_outbox_attachments(
        &self,
        account_id: &str,
        outbox_id: &str,
    ) -> Result<Vec<ManagedDraftAttachment>> {
        let connection = self.connection()?;
        query_outbox_attachments(&connection, account_id, outbox_id)
    }

    /// Initializes the digest of one legacy managed blob after Rust has read
    /// and hashed the exact account-scoped file. The compare-and-set includes
    /// every filesystem identity field used by that read. A concurrent writer
    /// may win only with the same digest; cross-account and changed metadata
    /// are indistinguishable from an unavailable blob.
    pub(crate) fn initialize_managed_attachment_digest(
        &self,
        account_id: &str,
        blob_id: &str,
        internal_name: &str,
        size_bytes: u64,
        sha256_hex: &str,
    ) -> Result<String> {
        validate_opaque_attachment_id(blob_id)?;
        if internal_name != format!("{blob_id}.blob") || internal_name.len() > 80 {
            return Err(managed_attachment_integrity_error());
        }
        if size_bytes > crate::mime::MAX_MANAGED_ATTACHMENT_BYTES {
            return Err(managed_attachment_integrity_error());
        }
        validate_managed_attachment_digest(sha256_hex)?;

        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE managed_attachment_blobs
             SET sha256_hex = ?5
             WHERE account_id = ?1
               AND id = ?2
               AND internal_name = ?3
               AND size_bytes = ?4
               AND sha256_hex IS NULL",
            params![
                account_id,
                blob_id,
                internal_name,
                u64_to_i64(size_bytes),
                sha256_hex,
            ],
        )?;
        let stored = transaction
            .query_row(
                "SELECT sha256_hex
                 FROM managed_attachment_blobs
                 WHERE account_id = ?1
                   AND id = ?2
                   AND internal_name = ?3
                   AND size_bytes = ?4",
                params![account_id, blob_id, internal_name, u64_to_i64(size_bytes),],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten()
            .ok_or_else(managed_attachment_integrity_error)?;
        if stored != sha256_hex {
            return Err(managed_attachment_integrity_error());
        }
        transaction.commit()?;
        Ok(stored)
    }

    /// Releases only a terminal draft's associations. Outbox references remain
    /// intact and therefore continue protecting every immutable send blob.
    pub(crate) fn release_terminal_draft_attachments(
        &self,
        account_id: &str,
        draft_id: &str,
        expected_local_version: u64,
    ) -> Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "DELETE FROM draft_attachment_refs
             WHERE account_id = ?1 AND draft_id = ?2
               AND EXISTS (
                   SELECT 1 FROM drafts d
                   WHERE d.account_id = ?1 AND d.id = ?2 AND d.local_version = ?3
                     AND (d.is_deleted = 1 OR d.status = 'sent')
               )",
            params![account_id, draft_id, u64_to_i64(expected_local_version),],
        )?;
        Ok(changed > 0)
    }

    pub(crate) fn terminal_draft_attachment_versions(
        &self,
        account_id: &str,
    ) -> Result<Vec<(String, u64)>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT d.id, d.local_version
             FROM drafts d
             WHERE d.account_id = ?1
               AND (d.is_deleted = 1 OR d.status = 'sent')
               AND EXISTS (
                   SELECT 1 FROM draft_attachment_refs r
                   WHERE r.account_id = d.account_id AND r.draft_id = d.id
               )
             ORDER BY d.id, d.local_version",
        )?;
        statement
            .query_map(params![account_id], |row| {
                Ok((row.get(0)?, decode_u64(1, row.get(1)?)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn list_orphaned_managed_attachments(
        &self,
        account_id: &str,
    ) -> Result<Vec<OrphanedManagedAttachment>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT b.id, b.account_id, b.internal_name
             FROM managed_attachment_blobs b
             WHERE b.account_id = ?1
               AND NOT EXISTS (
                   SELECT 1 FROM draft_attachment_refs d
                   WHERE d.account_id = b.account_id AND d.blob_id = b.id
               )
               AND NOT EXISTS (
                   SELECT 1 FROM outbox_attachment_refs o
                   WHERE o.account_id = b.account_id AND o.blob_id = b.id
               )
             ORDER BY b.created_at, b.id",
        )?;
        statement
            .query_map(params![account_id], |row| {
                Ok(OrphanedManagedAttachment {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    internal_name: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn all_managed_attachment_internal_names(&self) -> Result<HashSet<String>> {
        let connection = self.connection()?;
        let mut statement =
            connection.prepare("SELECT internal_name FROM managed_attachment_blobs")?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<HashSet<_>, _>>()?;
        Ok(names)
    }

    /// Removes the database ownership row only if it remains unreferenced at
    /// commit time. The returned internal name can then be deleted from the
    /// controlled store; a later sweep safely catches an interrupted file
    /// deletion.
    pub(crate) fn take_orphaned_managed_attachment(
        &self,
        account_id: &str,
        attachment_id: &str,
    ) -> Result<Option<OrphanedManagedAttachment>> {
        validate_opaque_attachment_id(attachment_id)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let orphan = transaction
            .query_row(
                "SELECT b.id, b.account_id, b.internal_name
                 FROM managed_attachment_blobs b
                 WHERE b.account_id = ?1 AND b.id = ?2
                   AND NOT EXISTS (
                       SELECT 1 FROM draft_attachment_refs d
                       WHERE d.account_id = b.account_id AND d.blob_id = b.id
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM outbox_attachment_refs o
                       WHERE o.account_id = b.account_id AND o.blob_id = b.id
                   )",
                params![account_id, attachment_id],
                |row| {
                    Ok(OrphanedManagedAttachment {
                        id: row.get(0)?,
                        account_id: row.get(1)?,
                        internal_name: row.get(2)?,
                    })
                },
            )
            .optional()?;
        let Some(orphan) = orphan else {
            transaction.commit()?;
            return Ok(None);
        };
        let removed = transaction.execute(
            "DELETE FROM managed_attachment_blobs
             WHERE account_id = ?1 AND id = ?2
               AND NOT EXISTS (
                   SELECT 1 FROM draft_attachment_refs d
                   WHERE d.account_id = ?1 AND d.blob_id = ?2
               )
               AND NOT EXISTS (
                   SELECT 1 FROM outbox_attachment_refs o
                   WHERE o.account_id = ?1 AND o.blob_id = ?2
               )",
            params![account_id, attachment_id],
        )?;
        transaction.commit()?;
        Ok((removed == 1).then_some(orphan))
    }

    #[cfg(test)]
    pub(crate) fn save_forward_context_if_absent(
        &self,
        account_id: &str,
        draft_id: &str,
        context: &ForwardContext,
    ) -> Result<bool> {
        validate_forward_context(context)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current_version: Option<u64> = transaction
            .query_row(
                "SELECT local_version FROM drafts
                 WHERE account_id = ?1 AND id = ?2 AND is_deleted = 0",
                params![account_id, draft_id],
                |row| decode_u64(0, row.get(0)?),
            )
            .optional()?;
        if current_version.is_none() {
            return Err(privacy_safe_not_found("draft"));
        }
        if let Some(existing) = query_forward_context(&transaction, account_id, draft_id)? {
            if existing == *context {
                attach_forward_context_to_current_version(&transaction, account_id, draft_id)?;
                transaction.commit()?;
                return Ok(false);
            }
            return Err(MailError::Validation(
                "the immutable forward context cannot be replaced".to_owned(),
            ));
        }
        insert_forward_context_rows(&transaction, account_id, draft_id, context)?;
        attach_forward_context_to_current_version(&transaction, account_id, draft_id)?;
        transaction.commit()?;
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn forward_context(
        &self,
        account_id: &str,
        draft_id: &str,
    ) -> Result<Option<ForwardContext>> {
        let connection = self.connection()?;
        query_forward_context(&connection, account_id, draft_id)
    }

    pub(crate) fn forward_context_at_version(
        &self,
        account_id: &str,
        draft_id: &str,
        local_version: u64,
    ) -> Result<Option<ForwardContext>> {
        if local_version == 0 {
            return Err(MailError::Validation(
                "a draft forward-context version must be positive".to_owned(),
            ));
        }
        let connection = self.connection()?;
        let linked: bool = connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM draft_version_forward_context_refs
                 WHERE account_id = ?1 AND draft_id = ?2
                   AND draft_local_version = ?3
             )",
            params![account_id, draft_id, u64_to_i64(local_version)],
            |row| row.get(0),
        )?;
        if !linked {
            return Ok(None);
        }
        query_forward_context(&connection, account_id, draft_id)
    }

    pub(crate) fn tombstone_draft(&self, id: &str, updated_at: &str) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE drafts SET
                 is_deleted = 1,
                 status = 'local',
                 revision = revision + 1,
                 local_version = local_version + 1,
                 updated_at = ?2
             WHERE id = ?1 AND status != 'sent'",
            params![id, updated_at],
        )?;
        if changed == 1 {
            let account_id: String = transaction.query_row(
                "SELECT account_id FROM drafts WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )?;
            insert_current_draft_version_snapshot(&transaction, &account_id, id)?;
            attach_forward_context_to_current_version(&transaction, &account_id, id)?;
            transaction.execute(
                "DELETE FROM draft_attachment_refs
                 WHERE account_id = ?2 AND draft_id = ?1
                   AND EXISTS (
                       SELECT 1 FROM drafts d
                       WHERE d.id = ?1 AND d.account_id = ?2 AND d.is_deleted = 1
                   )",
                params![id, account_id],
            )?;
        }
        transaction.commit()?;
        ensure_changed(changed, "draft", id.to_owned())
    }

    pub(crate) fn tombstone_draft_if_local_version(
        &self,
        account_id: &str,
        id: &str,
        expected_local_version: u64,
        updated_at: &str,
    ) -> Result<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE drafts SET
                 is_deleted = 1,
                 status = 'local',
                 revision = revision + 1,
                 local_version = local_version + 1,
                 updated_at = ?4
             WHERE id = ?1
               AND account_id = ?2
               AND local_version = ?3
               AND is_deleted = 0
               AND status != 'sent'",
            params![
                id,
                account_id,
                u64_to_i64(expected_local_version),
                updated_at
            ],
        )?;
        if changed == 1 {
            insert_current_draft_version_snapshot(&transaction, account_id, id)?;
            attach_forward_context_to_current_version(&transaction, account_id, id)?;
            transaction.execute(
                "DELETE FROM draft_attachment_refs
                 WHERE account_id = ?1 AND draft_id = ?2",
                params![account_id, id],
            )?;
        }
        transaction.commit()?;
        Ok(changed == 1)
    }

    /// Permanently deletes only the exact draft snapshot that the sync loop
    /// reconciled. A concurrent local edit increments the revision and wins.
    pub(crate) fn delete_draft_if_unchanged(&self, expected: &DraftRecord) -> Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "DELETE FROM drafts
             WHERE id = :id
               AND account_id = :account_id
               AND local_version = :expected_local_version
               AND revision = :expected_revision
               AND synced_revision = :expected_synced_revision
               AND status = :expected_status
               AND is_deleted = :expected_is_deleted
               AND remote_mailbox IS :expected_mailbox
               AND remote_uid IS :expected_uid
               AND remote_uid_validity IS :expected_uid_validity",
            named_params! {
                ":id": expected.draft.id,
                ":account_id": expected.draft.account_id,
                ":expected_local_version": u64_to_i64(expected.local_version),
                ":expected_revision": u64_to_i64(expected.revision),
                ":expected_synced_revision": u64_to_i64(expected.synced_revision),
                ":expected_status": expected.draft.status,
                ":expected_is_deleted": expected.is_deleted,
                ":expected_mailbox": expected.draft.remote_mailbox,
                ":expected_uid": expected.draft.remote_uid,
                ":expected_uid_validity": expected.remote_uid_validity,
            },
        )?;
        Ok(changed == 1)
    }

    /// Applies the remote UID produced by a push (or confirmed by an in-sync
    /// snapshot) without ever marking a newer local revision as synchronized.
    pub(crate) fn mark_draft_record_synced_if_unchanged(
        &self,
        expected: &DraftRecord,
        mailbox: &str,
        remote_uid: Option<u32>,
        remote_uid_validity: Option<u32>,
    ) -> Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE drafts SET
                 status = 'synced',
                 remote_mailbox = :mailbox,
                 remote_uid = :remote_uid,
                 remote_uid_validity = :remote_uid_validity,
                 synced_revision = :expected_revision,
                 is_deleted = 0
             WHERE id = :id
               AND account_id = :account_id
               AND local_version = :expected_local_version
               AND revision = :expected_revision
               AND synced_revision = :expected_synced_revision
               AND status = :expected_status
               AND is_deleted = :expected_is_deleted
               AND remote_mailbox IS :expected_mailbox
               AND remote_uid IS :expected_uid
               AND remote_uid_validity IS :expected_uid_validity",
            named_params! {
                ":id": expected.draft.id,
                ":account_id": expected.draft.account_id,
                ":mailbox": mailbox,
                ":remote_uid": remote_uid,
                ":remote_uid_validity": remote_uid_validity,
                ":expected_local_version": u64_to_i64(expected.local_version),
                ":expected_revision": u64_to_i64(expected.revision),
                ":expected_synced_revision": u64_to_i64(expected.synced_revision),
                ":expected_status": expected.draft.status,
                ":expected_is_deleted": expected.is_deleted,
                ":expected_mailbox": expected.draft.remote_mailbox,
                ":expected_uid": expected.draft.remote_uid,
                ":expected_uid_validity": expected.remote_uid_validity,
            },
        )?;
        Ok(changed == 1)
    }

    #[cfg(test)]
    pub(crate) fn enqueue_outbox(&self, item: &OutboxItem) -> Result<()> {
        validate_outbox_draft_link(item)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO outbox (
                 id, account_id, draft_id, draft_revision, draft_local_version,
                 recipients_json, status, attempts,
                 last_error, created_at, sent_at, raw_rfc822, recipient_groups_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO NOTHING",
            params![
                item.id,
                item.account_id,
                item.draft_id,
                item.draft_revision.map(u64_to_i64),
                item.draft_local_version.map(u64_to_i64),
                encode_json(&item.recipients)?,
                item.status.as_str(),
                item.attempts,
                item.last_error,
                item.created_at,
                item.sent_at,
                item.raw_rfc822,
                item.recipient_groups
                    .as_ref()
                    .map(encode_json)
                    .transpose()?,
            ],
        )?;
        bind_outbox_item_attachment_rows(&transaction, item)?;
        transaction.commit()?;
        Ok(())
    }

    /// Persists a newly confirmed send that could not enter its first SMTP
    /// attempt. For a newer draft version, obsolete retryable attempts are
    /// terminalized in the same transaction so the user can never later send
    /// both the old and new contents.
    pub(crate) fn enqueue_new_outbox(&self, item: &OutboxItem) -> Result<()> {
        validate_outbox_draft_link(item)?;
        validate_new_outbox_recipient_groups(item)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        prepare_new_draft_send(&transaction, item)?;
        let inserted = transaction.execute(
            "INSERT INTO outbox (
                 id, account_id, draft_id, draft_revision, draft_local_version,
                 recipients_json, status, attempts,
                 last_error, created_at, sent_at, raw_rfc822, recipient_groups_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO NOTHING",
            params![
                item.id,
                item.account_id,
                item.draft_id,
                item.draft_revision.map(u64_to_i64),
                item.draft_local_version.map(u64_to_i64),
                encode_json(&item.recipients)?,
                item.status.as_str(),
                item.attempts,
                item.last_error,
                item.created_at,
                item.sent_at,
                item.raw_rfc822,
                item.recipient_groups
                    .as_ref()
                    .map(encode_json)
                    .transpose()?,
            ],
        )?;
        if inserted != 1 {
            return Err(MailError::Validation(format!(
                "outbox item '{}' already exists",
                item.id
            )));
        }
        bind_outbox_item_attachment_rows(&transaction, item)?;
        transaction.commit()?;
        Ok(())
    }

    /// Persists a newly composed message and claims its first SMTP attempt in
    /// one transaction. Other connections can observe either no row or the
    /// final `sending` row, never a live `queued` intermediate that startup
    /// recovery could mistake for an abandoned message.
    pub(crate) fn enqueue_and_claim_outbox(&self, item: &OutboxItem) -> Result<OutboxItem> {
        validate_outbox_draft_link(item)?;
        validate_new_outbox_recipient_groups(item)?;
        if item.status != OutboxStatus::Queued || item.attempts != 0 {
            return Err(MailError::Validation(
                "a new Outbox claim must start queued with zero attempts".to_owned(),
            ));
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        prepare_new_draft_send(&transaction, item)?;
        let inserted = transaction.execute(
            "INSERT INTO outbox (
                 id, account_id, draft_id, draft_revision, draft_local_version,
                 recipients_json, status, attempts,
                 last_error, created_at, sent_at, raw_rfc822, recipient_groups_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', 0, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO NOTHING",
            params![
                item.id,
                item.account_id,
                item.draft_id,
                item.draft_revision.map(u64_to_i64),
                item.draft_local_version.map(u64_to_i64),
                encode_json(&item.recipients)?,
                item.last_error,
                item.created_at,
                item.sent_at,
                item.raw_rfc822,
                item.recipient_groups
                    .as_ref()
                    .map(encode_json)
                    .transpose()?,
            ],
        )?;
        if inserted != 1 {
            return Err(MailError::Validation(format!(
                "outbox item '{}' already exists",
                item.id
            )));
        }
        bind_outbox_item_attachment_rows(&transaction, item)?;
        let claimed = transaction.execute(
            "UPDATE outbox SET status = 'sending', attempts = attempts + 1, last_error = NULL
             WHERE id = ?1 AND account_id = ?2 AND status = 'queued' AND attempts = 0",
            params![item.id, item.account_id],
        )?;
        if claimed != 1 {
            return Err(MailError::Validation(format!(
                "outbox item '{}' could not be claimed for its first attempt",
                item.id
            )));
        }
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM outbox WHERE id = ?1");
        let sending = transaction.query_row(&sql, params![item.id], row_to_outbox)?;
        transaction.commit()?;
        Ok(sending)
    }

    pub(crate) fn get_outbox(&self, id: &str) -> Result<OutboxItem> {
        let connection = self.connection()?;
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM outbox WHERE id = ?1");
        connection
            .query_row(&sql, params![id], row_to_outbox)
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "outbox item",
                id: id.to_owned(),
            })
    }

    /// Returns an Outbox item that must block a new send of this draft
    /// snapshot. The same version is always single-shot; an unresolved queued,
    /// sending or delivery-unknown older version also blocks until its outcome
    /// is explicit. Definite retryable/rejected older versions do not block a
    /// genuinely newer edit.
    pub(crate) fn get_blocking_outbox_for_draft(
        &self,
        draft_id: &str,
        draft_local_version: u64,
    ) -> Result<Option<OutboxItem>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {OUTBOX_COLUMNS} FROM outbox
             WHERE draft_id = ?1
               AND (
                   draft_local_version = ?2
                   OR status IN ('queued', 'sending', 'delivery_unknown')
               )
             ORDER BY
                 CASE WHEN status = 'delivery_unknown' THEN 0 ELSE 1 END,
                 created_at ASC, id ASC
             LIMIT 1"
        );
        connection
            .query_row(
                &sql,
                params![draft_id, u64_to_i64(draft_local_version)],
                row_to_outbox,
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn list_outbox(&self, account_id: &str) -> Result<Vec<OutboxItem>> {
        self.query_outbox(
            "WHERE account_id = ?1
               AND status <> 'sent'
             ORDER BY created_at ASC, id ASC",
            account_id,
        )
    }

    pub(crate) fn list_sent_outbox_fallbacks(&self, account_id: &str) -> Result<Vec<OutboxItem>> {
        self.query_outbox(
            "WHERE account_id = ?1
               AND status = 'sent'
             ORDER BY COALESCE(sent_at, created_at) DESC, id DESC",
            account_id,
        )
    }

    pub(crate) fn list_sent_reconciliation_candidates(
        &self,
        account_id: &str,
    ) -> Result<Vec<OutboxItem>> {
        self.query_outbox(
            "WHERE account_id = ?1
               AND status IN ('sent', 'delivery_unknown')
             ORDER BY created_at ASC, id ASC",
            account_id,
        )
    }

    fn query_outbox(&self, suffix: &str, account_id: &str) -> Result<Vec<OutboxItem>> {
        let connection = self.connection()?;
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM outbox {suffix}");
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![account_id], row_to_outbox)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn update_outbox_status(
        &self,
        id: &str,
        status: OutboxStatus,
        last_error: Option<&str>,
    ) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE outbox SET
                 status = ?2,
                 attempts = attempts + CASE WHEN ?2 = 'sending' THEN 1 ELSE 0 END,
                 last_error = ?3,
                 sent_at = CASE
                     WHEN ?2 = 'sent' THEN COALESCE(sent_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                     ELSE sent_at
                 END
             WHERE id = ?1",
            params![id, status.as_str(), last_error],
        )?;
        ensure_changed(changed, "outbox item", id.to_owned())
    }

    /// Atomically claims one explicitly retryable Outbox item for a manual
    /// SMTP attempt. The guarded update prevents two app processes from
    /// retrying the same immutable message, and `attempts` is incremented only
    /// when the item actually enters `sending`.
    pub(crate) fn claim_retryable_outbox(&self, id: &str, account_id: &str) -> Result<OutboxItem> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM outbox WHERE id = ?1");
        let current = transaction
            .query_row(&sql, params![id], row_to_outbox)
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "outbox item",
                id: id.to_owned(),
            })?;

        if current.account_id != account_id {
            return Err(MailError::NotFound {
                entity: "outbox item",
                id: id.to_owned(),
            });
        }
        if current.status != OutboxStatus::Retryable {
            return Err(MailError::Validation(format!(
                "outbox item '{id}' has status '{}'; only retryable items can be retried",
                current.status.as_str()
            )));
        }

        let changed = transaction.execute(
            "UPDATE outbox SET
                 status = 'sending', attempts = attempts + 1, last_error = NULL
             WHERE id = ?1 AND account_id = ?2 AND status = 'retryable'",
            params![id, account_id],
        )?;
        if changed != 1 {
            return Err(MailError::Validation(format!(
                "outbox item '{id}' is no longer retryable"
            )));
        }
        let claimed = transaction.query_row(&sql, params![id], row_to_outbox)?;
        transaction.commit()?;
        Ok(claimed)
    }

    /// Atomically claims one exact ambiguous SMTP attempt for a user-approved
    /// duplicate-risk retry. `expected_attempts` binds the decision to the
    /// generation the user reviewed: if this retry itself becomes ambiguous,
    /// a repeated invocation carrying the old generation cannot send again.
    pub(crate) fn claim_delivery_unknown_retry(
        &self,
        id: &str,
        account_id: &str,
        expected_attempts: u32,
    ) -> Result<OutboxItem> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM outbox WHERE id = ?1");
        let current = transaction
            .query_row(&sql, params![id], row_to_outbox)
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "outbox item",
                id: id.to_owned(),
            })?;

        if current.account_id != account_id {
            return Err(MailError::NotFound {
                entity: "outbox item",
                id: id.to_owned(),
            });
        }
        if current.status != OutboxStatus::DeliveryUnknown || current.attempts != expected_attempts
        {
            return Err(MailError::Validation(format!(
                "outbox item '{id}' is no longer the reviewed delivery-unknown attempt; refresh before deciding again"
            )));
        }

        let changed = transaction.execute(
            "UPDATE outbox SET
                 status = 'sending', attempts = attempts + 1, last_error = NULL
             WHERE id = ?1
               AND account_id = ?2
               AND status = 'delivery_unknown'
               AND attempts = ?3",
            params![id, account_id, i64::from(expected_attempts)],
        )?;
        if changed != 1 {
            return Err(MailError::Validation(format!(
                "outbox item '{id}' is no longer the reviewed delivery-unknown attempt; refresh before deciding again"
            )));
        }
        let claimed = transaction.query_row(&sql, params![id], row_to_outbox)?;
        transaction.commit()?;
        Ok(claimed)
    }

    /// Atomically records successful SMTP delivery. The editable draft is
    /// consumed only when it is still the exact revision used to build this
    /// immutable Outbox message. A newer/deleted draft is preserved and the
    /// stale relation is released so that version remains independently
    /// sendable.
    pub(crate) fn finalize_outbox_sent(&self, outbox_id: &str) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM outbox WHERE id = ?1");
        let outbox = transaction
            .query_row(&sql, params![outbox_id], row_to_outbox)
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "outbox item",
                id: outbox_id.to_owned(),
            })?;
        let outbox_changed = transaction.execute(
            "UPDATE outbox SET
                 status = 'sent', last_error = NULL,
                 sent_at = COALESCE(sent_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             WHERE id = ?1",
            params![outbox_id],
        )?;
        ensure_changed(outbox_changed, "outbox item", outbox_id.to_owned())?;
        finalize_outbox_draft_state(&transaction, &outbox)?;
        transaction.commit()?;
        Ok(())
    }

    /// Applies the user's "confirmed delivered" decision to exactly the
    /// ambiguous attempt generation they reviewed. A repeated/concurrent call
    /// after the transition is rejected without changing the row.
    pub(crate) fn confirm_delivery_unknown_as_sent(
        &self,
        outbox_id: &str,
        account_id: &str,
        expected_attempts: u32,
    ) -> Result<OutboxItem> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM outbox WHERE id = ?1");
        let outbox = transaction
            .query_row(&sql, params![outbox_id], row_to_outbox)
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "outbox item",
                id: outbox_id.to_owned(),
            })?;
        if outbox.account_id != account_id {
            return Err(MailError::NotFound {
                entity: "outbox item",
                id: outbox_id.to_owned(),
            });
        }
        if outbox.status != OutboxStatus::DeliveryUnknown || outbox.attempts != expected_attempts {
            return Err(MailError::Validation(format!(
                "outbox item '{outbox_id}' is no longer the reviewed delivery-unknown attempt; refresh before deciding again"
            )));
        }

        let changed = transaction.execute(
            "UPDATE outbox SET
                 status = 'sent', last_error = NULL
             WHERE id = ?1
               AND account_id = ?2
               AND status = 'delivery_unknown'
               AND attempts = ?3",
            params![outbox_id, account_id, i64::from(expected_attempts)],
        )?;
        if changed != 1 {
            return Err(MailError::Validation(format!(
                "outbox item '{outbox_id}' is no longer the reviewed delivery-unknown attempt; refresh before deciding again"
            )));
        }
        finalize_outbox_draft_state(&transaction, &outbox)?;
        let confirmed = transaction.query_row(&sql, params![outbox_id], row_to_outbox)?;
        transaction.commit()?;
        Ok(confirmed)
    }

    /// Records confirmed SMTP success only if this is still the exact claimed
    /// `sending` generation. A late result from an older process cannot finish
    /// a newer manual attempt.
    pub(crate) fn finalize_claimed_outbox_sent(
        &self,
        outbox_id: &str,
        account_id: &str,
        claimed_attempts: u32,
    ) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM outbox WHERE id = ?1");
        let outbox = transaction
            .query_row(&sql, params![outbox_id], row_to_outbox)
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "outbox item",
                id: outbox_id.to_owned(),
            })?;
        if outbox.account_id != account_id {
            return Err(MailError::NotFound {
                entity: "outbox item",
                id: outbox_id.to_owned(),
            });
        }
        let changed = transaction.execute(
            "UPDATE outbox SET
                 status = 'sent', last_error = NULL,
                 sent_at = COALESCE(sent_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             WHERE id = ?1
               AND account_id = ?2
               AND status = 'sending'
               AND attempts = ?3",
            params![outbox_id, account_id, i64::from(claimed_attempts)],
        )?;
        if changed != 1 {
            return Err(MailError::Validation(format!(
                "outbox item '{outbox_id}' is no longer the claimed SMTP attempt"
            )));
        }
        finalize_outbox_draft_state(&transaction, &outbox)?;
        transaction.commit()?;
        Ok(())
    }

    /// Records the classified failure of one exact claimed SMTP generation.
    /// Only terminal/review states produced by the SMTP classifier are
    /// accepted, and a late result cannot overwrite a newer attempt.
    pub(crate) fn complete_claimed_outbox_failure(
        &self,
        outbox_id: &str,
        account_id: &str,
        claimed_attempts: u32,
        status: OutboxStatus,
        last_error: &str,
    ) -> Result<()> {
        if !matches!(
            status,
            OutboxStatus::Retryable | OutboxStatus::Rejected | OutboxStatus::DeliveryUnknown
        ) {
            return Err(MailError::Validation(
                "a claimed SMTP failure must be retryable, rejected, or delivery-unknown"
                    .to_owned(),
            ));
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM outbox WHERE id = ?1");
        let current = transaction
            .query_row(&sql, params![outbox_id], row_to_outbox)
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "outbox item",
                id: outbox_id.to_owned(),
            })?;
        if current.account_id != account_id {
            return Err(MailError::NotFound {
                entity: "outbox item",
                id: outbox_id.to_owned(),
            });
        }
        let changed = transaction.execute(
            "UPDATE outbox SET status = ?4, last_error = ?5
             WHERE id = ?1
               AND account_id = ?2
               AND status = 'sending'
               AND attempts = ?3",
            params![
                outbox_id,
                account_id,
                i64::from(claimed_attempts),
                status.as_str(),
                last_error
            ],
        )?;
        if changed != 1 {
            return Err(MailError::Validation(format!(
                "outbox item '{outbox_id}' is no longer the claimed SMTP attempt"
            )));
        }
        transaction.commit()?;
        Ok(())
    }

    /// A queued item has been committed to SQLite but has not entered the SMTP
    /// delivery attempt yet. Recovering it as retryable is duplicate-safe and
    /// keeps the immutable MIME, envelope recipients and draft relation.
    pub(crate) fn recover_queued_as_retryable(&self) -> Result<usize> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE outbox SET
                     status = 'retryable',
                     last_error = 'application stopped before SMTP delivery started; manual retry is safe'
                 WHERE status = 'queued'",
                [],
            )
            .map_err(Into::into)
    }

    /// A process can crash after the SMTP server accepted a message but before
    /// the local `sent` transition. Such messages must not be blindly retried.
    pub(crate) fn recover_sending_as_delivery_unknown(&self) -> Result<usize> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE outbox SET
                     status = 'delivery_unknown',
                     last_error = COALESCE(
                         last_error,
                         'application stopped while SMTP delivery was in progress'
                     )
                 WHERE status = 'sending'",
                [],
            )
            .map_err(Into::into)
    }

    /// Retires one local Outbox lifecycle only after the provider's cached Sent
    /// mailbox contains the same normalized RFC822 Message-ID. This is also an
    /// authoritative resolution for `delivery_unknown`: no subject, recipient,
    /// timestamp, or other heuristic can trigger the transition.
    pub(crate) fn reconcile_outbox_with_cached_sent(
        &self,
        outbox_id: &str,
        account_id: &str,
        sent_mailbox: &str,
        message_id: &str,
    ) -> Result<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM outbox WHERE id = ?1");
        let Some(outbox) = transaction
            .query_row(&sql, params![outbox_id], row_to_outbox)
            .optional()?
        else {
            return Ok(false);
        };
        if outbox.account_id != account_id
            || !matches!(
                outbox.status,
                OutboxStatus::Sent | OutboxStatus::DeliveryUnknown
            )
        {
            return Ok(false);
        }

        let cached_match = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1
                 FROM messages
                 WHERE account_id = ?1
                   AND mailbox = ?2
                   AND message_id IS NOT NULL
                   AND lower(trim(message_id, '<> ')) =
                       lower(trim(?3, '<> '))
             )",
            params![account_id, sent_mailbox, message_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !cached_match {
            return Ok(false);
        }

        if outbox.status == OutboxStatus::DeliveryUnknown {
            let changed = transaction.execute(
                "UPDATE outbox SET
                     status = 'sent',
                     last_error = NULL,
                     sent_at = COALESCE(
                         sent_at,
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     )
                 WHERE id = ?1
                   AND account_id = ?2
                   AND status = 'delivery_unknown'",
                params![outbox_id, account_id],
            )?;
            if changed != 1 {
                return Ok(false);
            }
            finalize_outbox_draft_state(&transaction, &outbox)?;
        }

        let deleted = transaction.execute(
            "DELETE FROM outbox
             WHERE id = ?1
               AND account_id = ?2
               AND status = 'sent'",
            params![outbox_id, account_id],
        )?;
        if deleted != 1 {
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }
}

fn finalize_outbox_draft_state(
    transaction: &rusqlite::Transaction<'_>,
    outbox: &OutboxItem,
) -> Result<()> {
    let consumed = match (outbox.draft_id.as_deref(), outbox.draft_local_version) {
        (Some(draft_id), Some(draft_local_version)) => {
            transaction.execute(
                "UPDATE drafts SET status = 'sent'
                 WHERE id = ?1 AND account_id = ?2 AND local_version = ?3 AND is_deleted = 0",
                params![draft_id, outbox.account_id, u64_to_i64(draft_local_version)],
            )? == 1
        }
        _ => false,
    };
    if !consumed {
        transaction.execute(
            "UPDATE outbox SET
                 draft_id = NULL, draft_revision = NULL, draft_local_version = NULL
             WHERE id = ?1",
            params![outbox.id],
        )?;
    } else if let Some(draft_id) = outbox.draft_id.as_deref() {
        // The immutable Outbox refs were inserted in the same transaction
        // that created this item, so a consumed draft no longer needs a
        // second reference set to protect the exact blobs.
        transaction.execute(
            "DELETE FROM draft_attachment_refs
             WHERE account_id = ?1 AND draft_id = ?2",
            params![outbox.account_id, draft_id],
        )?;
    }
    Ok(())
}

fn configure_connection(connection: &Connection) -> Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

fn migrate_drafts_v2(connection: &Connection) -> Result<()> {
    let columns = [
        ("revision", "INTEGER NOT NULL DEFAULT 1"),
        ("synced_revision", "INTEGER NOT NULL DEFAULT 0"),
        ("remote_uid_validity", "INTEGER"),
        ("is_deleted", "INTEGER NOT NULL DEFAULT 0"),
    ];
    for (column, declaration) in columns {
        if !table_has_column(connection, "drafts", column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE drafts ADD COLUMN {column} {declaration};"
            ))?;
        }
    }
    connection.execute(
        "UPDATE drafts SET synced_revision = revision
         WHERE status IN ('synced', 'sent') AND synced_revision = 0",
        [],
    )?;
    Ok(())
}

fn migrate_outbox_v3(connection: &Connection) -> Result<()> {
    if !table_has_column(connection, "outbox", "draft_revision")? {
        // Legacy rows intentionally remain NULL: their exact source revision
        // cannot be reconstructed safely from the currently editable draft.
        connection.execute_batch("ALTER TABLE outbox ADD COLUMN draft_revision INTEGER;")?;
    }
    if !table_has_column(connection, "outbox", "draft_local_version")? {
        connection.execute_batch("ALTER TABLE outbox ADD COLUMN draft_local_version INTEGER;")?;
    }
    connection.execute_batch(
        "DROP INDEX IF EXISTS idx_outbox_unique_draft;
         DROP INDEX IF EXISTS idx_outbox_unique_draft_revision;
         CREATE UNIQUE INDEX IF NOT EXISTS idx_outbox_unique_draft_local_version
             ON outbox(draft_id, draft_local_version)
             WHERE draft_id IS NOT NULL AND draft_local_version IS NOT NULL;",
    )?;
    Ok(())
}

fn migrate_drafts_v4(connection: &Connection) -> Result<()> {
    if !table_has_column(connection, "drafts", "local_version")? {
        // Legacy rows begin at one. From this point onward every local edit,
        // content replacement, or tombstone increments this SQLite-only token.
        connection.execute_batch(
            "ALTER TABLE drafts ADD COLUMN local_version INTEGER NOT NULL DEFAULT 1;",
        )?;
    }
    if !table_has_column(connection, "drafts", "has_unsupported_content")? {
        // Start conservatively, then clear the flag only for rows whose exact
        // persisted RFC822 bytes can be proven safe for the plain-text editor.
        connection.execute_batch(
            "ALTER TABLE drafts ADD COLUMN has_unsupported_content INTEGER NOT NULL DEFAULT 1;",
        )?;
        let rows = {
            let mut statement = connection.prepare("SELECT id, raw_rfc822 FROM drafts")?;
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (id, raw_rfc822) in rows {
            connection.execute(
                "UPDATE drafts SET has_unsupported_content = ?2 WHERE id = ?1",
                params![id, draft_has_unsupported_content(&raw_rfc822)],
            )?;
        }
    }
    Ok(())
}

fn migrate_messages_v5(connection: &Connection) -> Result<()> {
    let mut needs_backfill = false;
    for column in ["in_reply_to_json", "references_json"] {
        if !table_has_column(connection, "messages", column)? {
            needs_backfill = true;
            connection.execute_batch(&format!(
                "ALTER TABLE messages ADD COLUMN {column} TEXT NOT NULL DEFAULT '[]';"
            ))?;
        }
    }
    if needs_backfill {
        let rows = {
            let mut statement = connection
                .prepare("SELECT id, raw_rfc822 FROM messages WHERE length(raw_rfc822) > 0")?;
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (id, raw_rfc822) in rows {
            let (in_reply_to, references) = reply_message_ids(&raw_rfc822);
            connection.execute(
                "UPDATE messages
                 SET in_reply_to_json = ?2, references_json = ?3
                 WHERE id = ?1",
                params![id, encode_json(&in_reply_to)?, encode_json(&references)?],
            )?;
        }
    }
    Ok(())
}

fn migrate_drafts_v7(connection: &Connection) -> Result<()> {
    if !table_has_column(connection, "drafts", "reply_context_json")? {
        connection.execute_batch("ALTER TABLE drafts ADD COLUMN reply_context_json TEXT;")?;
    }
    Ok(())
}

fn migrate_pending_seen_v8(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS pending_seen_updates (
             account_id TEXT NOT NULL,
             mailbox TEXT NOT NULL,
             uid INTEGER NOT NULL,
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             PRIMARY KEY (account_id, mailbox, uid),
             FOREIGN KEY (account_id, mailbox, uid)
                 REFERENCES messages(account_id, mailbox, uid) ON DELETE CASCADE
         );",
    )?;
    Ok(())
}

fn migrate_pending_flagged_v9(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS pending_flagged_updates (
             account_id TEXT NOT NULL,
             mailbox TEXT NOT NULL,
             uid INTEGER NOT NULL,
             desired INTEGER NOT NULL CHECK (desired IN (0, 1)),
             revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
             updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             PRIMARY KEY (account_id, mailbox, uid),
             FOREIGN KEY (account_id, mailbox, uid)
                 REFERENCES messages(account_id, mailbox, uid) ON DELETE CASCADE
         );",
    )?;
    Ok(())
}

fn migrate_message_previews_v10(connection: &Connection) -> Result<()> {
    if !table_has_column(connection, "messages", "preview_fetched")? {
        connection.execute_batch(
            "ALTER TABLE messages
                 ADD COLUMN preview_fetched INTEGER NOT NULL DEFAULT 0
                 CHECK (preview_fetched IN (0, 1));",
        )?;
    }
    connection.execute(
        "UPDATE messages
         SET preview_fetched = 1
         WHERE preview_fetched = 0
           AND (body_fetched = 1 OR trim(preview) <> '')",
        [],
    )?;
    Ok(())
}

fn migrate_compose_format_v11(connection: &Connection) -> Result<()> {
    if !table_has_column(connection, "drafts", "compose_format_json")? {
        connection.execute_batch(
            "ALTER TABLE drafts
                 ADD COLUMN compose_format_json TEXT NOT NULL DEFAULT '{}';",
        )?;
    }
    Ok(())
}

fn migrate_system_flag_queue_v12(
    connection: &Connection,
    table: &str,
    legacy_desired: bool,
) -> Result<()> {
    debug_assert!(matches!(
        table,
        "pending_seen_updates" | "pending_flagged_updates"
    ));
    let required_columns = [
        "operation_id",
        "source_uid_validity",
        "desired",
        "revision",
        "status",
        "error_kind",
        "updated_at",
    ];
    let has_required_columns = required_columns
        .into_iter()
        .map(|column| table_has_column(connection, table, column))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .all(|present| present);
    let is_current =
        has_required_columns && !table_references_table(connection, table, "messages")?;
    if is_current {
        return Ok(());
    }

    let backup = format!("{table}_v12_backup");
    let operation_expression = if table_has_column(connection, table, "operation_id")? {
        "q.operation_id"
    } else {
        "NULL"
    };
    let epoch_expression = if table_has_column(connection, table, "source_uid_validity")? {
        "q.source_uid_validity"
    } else {
        "b.uid_validity"
    };
    let desired_expression = if table_has_column(connection, table, "desired")? {
        "q.desired"
    } else if legacy_desired {
        "1"
    } else {
        "0"
    };
    let revision_expression = if table_has_column(connection, table, "revision")? {
        "q.revision"
    } else {
        "1"
    };
    let status_expression = if table_has_column(connection, table, "status")? {
        "q.status"
    } else {
        "'pending'"
    };
    let error_expression = if table_has_column(connection, table, "error_kind")? {
        "q.error_kind"
    } else {
        "NULL"
    };
    let updated_expression = if table_has_column(connection, table, "updated_at")? {
        "q.updated_at"
    } else if table_has_column(connection, table, "created_at")? {
        "q.created_at"
    } else {
        "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
    };

    let migrate = (|| -> Result<()> {
        connection.execute_batch(&format!(
            "BEGIN IMMEDIATE;
             ALTER TABLE {table} RENAME TO {backup};
             CREATE TABLE {table} (
                 operation_id TEXT NOT NULL UNIQUE,
                 account_id TEXT NOT NULL,
                 mailbox TEXT NOT NULL,
                 source_uid_validity INTEGER NOT NULL CHECK (source_uid_validity >= 0),
                 uid INTEGER NOT NULL,
                 desired INTEGER NOT NULL CHECK (desired IN (0, 1)),
                 revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
                 status TEXT NOT NULL DEFAULT 'pending' CHECK (
                     status IN (
                         'pending', 'in_flight', 'confirmed',
                         'needs_attention', 'outcome_unknown'
                     )
                 ),
                 error_kind TEXT CHECK (
                     error_kind IS NULL OR error_kind IN (
                         'uid_validity_changed', 'source_missing',
                         'ambiguous_remote_state', 'network_unavailable',
                         'mailbox_unavailable', 'permission_denied',
                         'server_rejected', 'unsupported', 'unknown'
                     )
                 ),
                 updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 PRIMARY KEY (account_id, mailbox, source_uid_validity, uid),
                 FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
             );"
        ))?;
        let rows = {
            let sql = format!(
                "SELECT {operation_expression}, q.account_id, q.mailbox,
                        COALESCE({epoch_expression}, 0), q.uid, {desired_expression},
                        {revision_expression}, {status_expression}, {error_expression},
                        {updated_expression}
                 FROM {backup} q
                 LEFT JOIN mailboxes b
                   ON b.account_id = q.account_id AND b.name = q.mailbox"
            );
            let mut statement = connection.prepare(&sql)?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, u32>(3)?,
                        row.get::<_, u32>(4)?,
                        row.get::<_, bool>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, String>(9)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (
            operation_id,
            account_id,
            mailbox,
            source_uid_validity,
            uid,
            desired,
            revision,
            old_status,
            old_error,
            updated_at,
        ) in rows
        {
            let status = if source_uid_validity == 0 {
                MutationStatus::NeedsAttention
            } else {
                MutationStatus::from_str(&old_status).unwrap_or(MutationStatus::NeedsAttention)
            };
            let error_kind = if source_uid_validity == 0 {
                Some(MessageMutationErrorKind::UidValidityChanged)
            } else {
                old_error
                    .as_deref()
                    .and_then(MessageMutationErrorKind::from_str)
            };
            let operation_id = operation_id
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| Uuid::now_v7().to_string());
            connection.execute(
                &format!(
                    "INSERT INTO {table} (
                         operation_id, account_id, mailbox, source_uid_validity, uid,
                         desired, revision, status, error_kind, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
                ),
                params![
                    operation_id,
                    account_id,
                    mailbox,
                    source_uid_validity,
                    uid,
                    desired,
                    revision.max(1),
                    status.as_str(),
                    error_kind.map(MessageMutationErrorKind::as_str),
                    updated_at,
                ],
            )?;
        }
        connection.execute_batch(&format!(
            "DROP TABLE {backup};
             CREATE INDEX idx_{table}_worker
                 ON {table}(account_id, mailbox, status, updated_at);
             COMMIT;"
        ))?;
        Ok(())
    })();
    if migrate.is_err() {
        let _ = connection.execute_batch("ROLLBACK;");
    }
    migrate
}

fn migrate_mailboxes_and_mutations_v12(connection: &Connection) -> Result<()> {
    for (column, declaration) in [
        ("history_before_uid", "INTEGER"),
        (
            "history_complete",
            "INTEGER NOT NULL DEFAULT 0 CHECK (history_complete IN (0, 1))",
        ),
        ("remote_total", "INTEGER"),
    ] {
        if !table_has_column(connection, "mailboxes", column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE mailboxes ADD COLUMN {column} {declaration};"
            ))?;
        }
    }

    migrate_system_flag_queue_v12(connection, "pending_seen_updates", true)?;
    migrate_system_flag_queue_v12(connection, "pending_flagged_updates", false)?;
    connection.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_pending_seen_worker
             ON pending_seen_updates(account_id, mailbox, status, updated_at);
         CREATE INDEX IF NOT EXISTS idx_pending_flagged_worker
             ON pending_flagged_updates(account_id, mailbox, status, updated_at);",
    )?;
    if !table_has_column(connection, "pending_message_actions", "remote_phase")? {
        connection.execute_batch(
            "ALTER TABLE pending_message_actions
                 ADD COLUMN remote_phase TEXT NOT NULL DEFAULT 'queued'
                 CHECK (
                     remote_phase IN (
                         'queued', 'transfer_started', 'transfer_acknowledged',
                         'source_delete_started', 'source_delete_acknowledged'
                     )
                 );",
        )?;
    }
    connection.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS trg_pending_message_actions_validate_insert
         BEFORE INSERT ON pending_message_actions
         WHEN NOT (
             (
                 NEW.kind = 'archive'
                 AND NEW.source_role IN ('inbox', 'sent')
                 AND NEW.destination_role = 'archive'
             ) OR (
                 NEW.kind = 'move_to_trash'
                 AND NEW.source_role IN ('inbox', 'sent', 'archive')
                 AND NEW.destination_role = 'trash'
             ) OR (
                 NEW.kind = 'permanent_delete'
                 AND NEW.source_role = 'trash'
                 AND NEW.destination_role IS NULL
             )
         )
         BEGIN
             SELECT RAISE(ABORT, 'invalid message action role combination');
         END;
         CREATE TRIGGER IF NOT EXISTS trg_pending_message_actions_validate_update
         BEFORE UPDATE OF source_role, destination_role, kind ON pending_message_actions
         WHEN NOT (
             (
                 NEW.kind = 'archive'
                 AND NEW.source_role IN ('inbox', 'sent')
                 AND NEW.destination_role = 'archive'
             ) OR (
                 NEW.kind = 'move_to_trash'
                 AND NEW.source_role IN ('inbox', 'sent', 'archive')
                 AND NEW.destination_role = 'trash'
             ) OR (
                 NEW.kind = 'permanent_delete'
                 AND NEW.source_role = 'trash'
                 AND NEW.destination_role IS NULL
             )
         )
         BEGIN
             SELECT RAISE(ABORT, 'invalid message action role combination');
         END;",
    )?;

    connection.execute(
        "INSERT INTO mailbox_roles (account_id, role, mailbox)
         SELECT a.id, 'inbox', m.name
         FROM accounts a
         JOIN mailboxes m
           ON m.account_id = a.id AND m.name = 'INBOX' COLLATE NOCASE
         WHERE true
         ON CONFLICT(account_id, role) DO NOTHING",
        [],
    )?;
    for role in MailboxRole::ALL {
        connection.execute(
            "INSERT INTO mailbox_capabilities (
                 account_id, role, status, display_name, unavailable_reason, retryable
             )
             SELECT id, ?1, 'discovery_pending', NULL, NULL, 1
             FROM accounts
             WHERE true
             ON CONFLICT(account_id, role) DO NOTHING",
            params![role.as_str()],
        )?;
    }
    connection.execute(
        "UPDATE mailbox_capabilities
         SET status = 'available',
             display_name = (
                 SELECT r.mailbox
                 FROM mailbox_roles r
                 WHERE r.account_id = mailbox_capabilities.account_id
                   AND r.role = mailbox_capabilities.role
             ),
             unavailable_reason = NULL,
             retryable = 0,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE EXISTS (
             SELECT 1
             FROM mailbox_roles r
             WHERE r.account_id = mailbox_capabilities.account_id
               AND r.role = mailbox_capabilities.role
         )",
        [],
    )?;
    connection.execute_batch(
        "UPDATE pending_seen_updates
         SET status = 'outcome_unknown',
             error_kind = COALESCE(error_kind, 'unknown'),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE status = 'in_flight';
         UPDATE pending_flagged_updates
         SET status = 'outcome_unknown',
             error_kind = COALESCE(error_kind, 'unknown'),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE status = 'in_flight';
         UPDATE pending_message_actions
         SET status = 'outcome_unknown',
             error_kind = COALESCE(error_kind, 'unknown'),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE status = 'in_flight';",
    )?;
    connection.execute(
        "DELETE FROM message_page_cursors
         WHERE created_at < unixepoch() - ?1",
        params![MESSAGE_CURSOR_TTL_SECONDS],
    )?;
    Ok(())
}

fn migrate_managed_attachments_v13(connection: &Connection) -> Result<()> {
    for (column, declaration) in [
        (
            "source_cleanup_pending",
            "INTEGER NOT NULL DEFAULT 0 CHECK (source_cleanup_pending IN (0, 1))",
        ),
        (
            "destination_reconciled",
            "INTEGER NOT NULL DEFAULT 0 CHECK (destination_reconciled IN (0, 1))",
        ),
    ] {
        if !table_has_column(connection, "pending_message_actions", column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE pending_message_actions ADD COLUMN {column} {declaration};"
            ))?;
        }
    }

    connection.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_drafts_account_id
             ON drafts(account_id, id);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_drafts_account_version
             ON drafts(account_id, id, local_version);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_outbox_account_id
             ON outbox(account_id, id);

         CREATE TABLE IF NOT EXISTS managed_attachment_blobs (
             id TEXT PRIMARY KEY NOT NULL
                 CHECK (length(id) BETWEEN 1 AND 128),
             account_id TEXT NOT NULL,
             origin_draft_id TEXT,
             internal_name TEXT NOT NULL UNIQUE
                 CHECK (
                     length(internal_name) BETWEEN 1 AND 80
                     AND instr(internal_name, '/') = 0
                     AND instr(internal_name, char(92)) = 0
                     AND instr(internal_name, ':') = 0
                     AND internal_name NOT IN ('.', '..')
                     AND substr(internal_name, -5) = '.blob'
                 ),
             name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 720),
             mime_type TEXT NOT NULL CHECK (length(mime_type) BETWEEN 1 AND 255),
             size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
             sha256_hex TEXT CHECK (
                 sha256_hex IS NULL
                 OR (
                     length(sha256_hex) = 64
                     AND lower(sha256_hex) = sha256_hex
                     AND sha256_hex NOT GLOB '*[^0-9a-f]*'
                 )
             ),
             disposition TEXT NOT NULL DEFAULT 'attachment'
                 CHECK (disposition IN ('attachment', 'inline')),
             transfer_encoding TEXT NOT NULL DEFAULT 'base64'
                 CHECK (transfer_encoding IN ('base64')),
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             UNIQUE (account_id, id),
             FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS idx_managed_attachment_blobs_account
             ON managed_attachment_blobs(account_id, created_at, id);

         CREATE TABLE IF NOT EXISTS draft_attachment_refs (
             account_id TEXT NOT NULL,
             draft_id TEXT NOT NULL,
             draft_local_version INTEGER NOT NULL CHECK (draft_local_version > 0),
             position INTEGER NOT NULL CHECK (position >= 0),
             blob_id TEXT NOT NULL,
             source_attachment_id TEXT,
             PRIMARY KEY (account_id, draft_id, draft_local_version, position),
             UNIQUE (account_id, draft_id, draft_local_version, blob_id),
             FOREIGN KEY (account_id, draft_id, draft_local_version)
                 REFERENCES drafts(account_id, id, local_version)
                 ON UPDATE CASCADE ON DELETE CASCADE,
             FOREIGN KEY (account_id, blob_id)
                 REFERENCES managed_attachment_blobs(account_id, id)
                 ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
         );
         CREATE INDEX IF NOT EXISTS idx_draft_attachment_refs_blob
             ON draft_attachment_refs(account_id, blob_id);

         CREATE TABLE IF NOT EXISTS outbox_attachment_sets (
             account_id TEXT NOT NULL,
             outbox_id TEXT NOT NULL,
             draft_id TEXT NOT NULL,
             draft_local_version INTEGER NOT NULL CHECK (draft_local_version > 0),
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             PRIMARY KEY (account_id, outbox_id),
             FOREIGN KEY (account_id, outbox_id)
                 REFERENCES outbox(account_id, id) ON DELETE CASCADE
         );

         CREATE TABLE IF NOT EXISTS outbox_attachment_refs (
             account_id TEXT NOT NULL,
             outbox_id TEXT NOT NULL,
             position INTEGER NOT NULL CHECK (position >= 0),
             blob_id TEXT NOT NULL,
             PRIMARY KEY (account_id, outbox_id, position),
             UNIQUE (account_id, outbox_id, blob_id),
             FOREIGN KEY (account_id, outbox_id)
                 REFERENCES outbox_attachment_sets(account_id, outbox_id)
                 ON DELETE CASCADE,
             FOREIGN KEY (account_id, blob_id)
                 REFERENCES managed_attachment_blobs(account_id, id)
                 ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
         );
         CREATE INDEX IF NOT EXISTS idx_outbox_attachment_refs_blob
             ON outbox_attachment_refs(account_id, blob_id);

         CREATE TABLE IF NOT EXISTS draft_forward_contexts (
             account_id TEXT NOT NULL,
             draft_id TEXT NOT NULL,
             source_message_id TEXT NOT NULL,
             original_subject TEXT NOT NULL,
             from_json TEXT,
             to_json TEXT NOT NULL DEFAULT '[]',
             cc_json TEXT NOT NULL DEFAULT '[]',
             sent_at TEXT,
             quoted_text TEXT NOT NULL,
             quoted_html TEXT,
             quoted_render_mode TEXT CHECK (
                 quoted_render_mode IS NULL OR quoted_render_mode IN (
                     'plain', 'native_html', 'isolated_html'
                 )
             ),
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             PRIMARY KEY (account_id, draft_id),
             FOREIGN KEY (account_id, draft_id)
                 REFERENCES drafts(account_id, id) ON DELETE CASCADE
         );

         CREATE TABLE IF NOT EXISTS draft_forward_source_attachments (
             account_id TEXT NOT NULL,
             draft_id TEXT NOT NULL,
             position INTEGER NOT NULL CHECK (position >= 0),
             attachment_id TEXT NOT NULL,
             original_name TEXT,
             safe_display_name TEXT NOT NULL,
             mime_type TEXT NOT NULL,
             size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
             disposition TEXT NOT NULL CHECK (disposition IN ('attachment', 'inline')),
             PRIMARY KEY (account_id, draft_id, position),
             UNIQUE (account_id, draft_id, attachment_id),
             FOREIGN KEY (account_id, draft_id)
                 REFERENCES draft_forward_contexts(account_id, draft_id)
                 ON DELETE CASCADE
         );

         CREATE TRIGGER IF NOT EXISTS trg_managed_attachment_blobs_immutable
         BEFORE UPDATE ON managed_attachment_blobs
         BEGIN
             SELECT RAISE(ABORT, 'managed attachment blobs are immutable');
         END;
         CREATE TRIGGER IF NOT EXISTS trg_draft_forward_contexts_immutable
         BEFORE UPDATE ON draft_forward_contexts
         BEGIN
             SELECT RAISE(ABORT, 'forward context is immutable');
         END;
         CREATE TRIGGER IF NOT EXISTS trg_draft_forward_source_attachments_immutable
         BEFORE UPDATE ON draft_forward_source_attachments
         BEGIN
             SELECT RAISE(ABORT, 'forward source attachment inventory is immutable');
         END;",
    )?;
    Ok(())
}

fn migrate_message_public_ids_v14(connection: &Connection) -> Result<()> {
    let schema_version: u32 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let added_column = !table_has_column(connection, "messages", "public_id")?;
    if added_column {
        connection.execute_batch("ALTER TABLE messages ADD COLUMN public_id TEXT;")?;
    }

    // A normal v14 startup must not scan a potentially million-row mailbox.
    // Only an initial or interrupted migration performs the bounded repair
    // query; schema objects below are cheap and idempotent on every open.
    let needs_repair = added_column
        || (schema_version < 14
            && connection.query_row(
                "SELECT EXISTS (
                     SELECT 1 FROM messages
                     WHERE public_id IS NULL
                        OR length(public_id) <> 36
                        OR substr(public_id, 9, 1) <> '-'
                        OR substr(public_id, 14, 1) <> '-'
                        OR substr(public_id, 19, 1) <> '-'
                        OR substr(public_id, 24, 1) <> '-'
                        OR substr(public_id, 15, 1) <> '4'
                        OR substr(public_id, 20, 1) NOT IN ('8', '9', 'a', 'b')
                        OR lower(public_id) <> public_id
                        OR public_id GLOB '*[^0-9a-f-]*'
                     LIMIT 1
                 ) OR EXISTS (
                     SELECT 1 FROM messages
                     WHERE public_id IS NOT NULL
                     GROUP BY public_id HAVING count(*) > 1
                     LIMIT 1
                 )",
                [],
                |row| row.get::<_, bool>(0),
            )?);
    if needs_repair {
        connection.execute_batch(
            "DROP TRIGGER IF EXISTS trg_messages_public_id_required_insert;
             DROP TRIGGER IF EXISTS trg_messages_public_id_immutable;",
        )?;
        let transaction = connection.unchecked_transaction()?;
        let row_ids = {
            let mut statement = transaction.prepare(
                "SELECT m.id
                 FROM messages m
                 WHERE m.public_id IS NULL
                    OR length(m.public_id) <> 36
                    OR substr(m.public_id, 9, 1) <> '-'
                    OR substr(m.public_id, 14, 1) <> '-'
                    OR substr(m.public_id, 19, 1) <> '-'
                    OR substr(m.public_id, 24, 1) <> '-'
                    OR substr(m.public_id, 15, 1) <> '4'
                    OR substr(m.public_id, 20, 1) NOT IN ('8', '9', 'a', 'b')
                    OR lower(m.public_id) <> m.public_id
                    OR m.public_id GLOB '*[^0-9a-f-]*'
                    OR EXISTS (
                        SELECT 1 FROM messages duplicate
                        WHERE duplicate.public_id = m.public_id
                          AND duplicate.id < m.id
                    )
                 ORDER BY m.id",
            )?;
            statement
                .query_map([], |row| row.get::<_, i64>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for row_id in row_ids {
            transaction.execute(
                "UPDATE messages SET public_id = ?2 WHERE id = ?1",
                params![row_id, Uuid::new_v4().to_string()],
            )?;
        }
        transaction.commit()?;
    }
    connection.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_public_id
             ON messages(public_id);
         CREATE TRIGGER IF NOT EXISTS trg_messages_public_id_required_insert
         BEFORE INSERT ON messages
         WHEN NEW.public_id IS NULL
           OR length(NEW.public_id) <> 36
           OR substr(NEW.public_id, 9, 1) <> '-'
           OR substr(NEW.public_id, 14, 1) <> '-'
           OR substr(NEW.public_id, 19, 1) <> '-'
           OR substr(NEW.public_id, 24, 1) <> '-'
           OR substr(NEW.public_id, 15, 1) <> '4'
           OR substr(NEW.public_id, 20, 1) NOT IN ('8', '9', 'a', 'b')
           OR lower(NEW.public_id) <> NEW.public_id
           OR NEW.public_id GLOB '*[^0-9a-f-]*'
         BEGIN
             SELECT RAISE(ABORT, 'message public id is required');
         END;
         CREATE TRIGGER IF NOT EXISTS trg_messages_public_id_immutable
         BEFORE UPDATE OF public_id ON messages
         WHEN NEW.public_id IS NOT OLD.public_id
         BEGIN
             SELECT RAISE(ABORT, 'message public id is immutable');
         END;",
    )?;
    Ok(())
}

fn migrate_immutable_draft_versions_v15(connection: &Connection) -> Result<()> {
    let schema_version: u32 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if !table_has_column(connection, "managed_attachment_blobs", "sha256_hex")? {
        connection.execute_batch(
            "ALTER TABLE managed_attachment_blobs
             ADD COLUMN sha256_hex TEXT CHECK (
                 sha256_hex IS NULL
                 OR (
                     length(sha256_hex) = 64
                     AND lower(sha256_hex) = sha256_hex
                     AND sha256_hex NOT GLOB '*[^0-9a-f]*'
                 )
             );",
        )?;
    }

    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS draft_version_snapshots (
             account_id TEXT NOT NULL,
             draft_id TEXT NOT NULL,
             draft_local_version INTEGER NOT NULL CHECK (draft_local_version > 0),
             protocol_revision INTEGER NOT NULL CHECK (protocol_revision > 0),
             to_json TEXT NOT NULL DEFAULT '[]',
             cc_json TEXT NOT NULL DEFAULT '[]',
             bcc_json TEXT NOT NULL DEFAULT '[]',
             subject TEXT NOT NULL DEFAULT '',
             body_text TEXT NOT NULL DEFAULT '',
             compose_format_json TEXT NOT NULL DEFAULT '{}',
             reply_context_json TEXT,
             has_unsupported_content INTEGER NOT NULL DEFAULT 0
                 CHECK (has_unsupported_content IN (0, 1)),
             created_at TEXT NOT NULL
                 DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
             PRIMARY KEY (account_id, draft_id, draft_local_version),
             FOREIGN KEY (account_id, draft_id)
                 REFERENCES drafts(account_id, id) ON DELETE CASCADE
         );

         INSERT OR IGNORE INTO draft_version_snapshots (
             account_id, draft_id, draft_local_version, protocol_revision,
             to_json, cc_json, bcc_json, subject, body_text,
             compose_format_json, reply_context_json, has_unsupported_content
         )
         SELECT account_id, id, local_version, revision,
                to_json, cc_json, bcc_json, subject, body_text,
                compose_format_json, reply_context_json, has_unsupported_content
         FROM drafts;

         CREATE TABLE IF NOT EXISTS draft_version_forward_context_refs (
             account_id TEXT NOT NULL,
             draft_id TEXT NOT NULL,
             draft_local_version INTEGER NOT NULL CHECK (draft_local_version > 0),
             PRIMARY KEY (account_id, draft_id, draft_local_version),
             FOREIGN KEY (account_id, draft_id, draft_local_version)
                 REFERENCES draft_version_snapshots(
                     account_id, draft_id, draft_local_version
                 ) ON DELETE CASCADE,
             FOREIGN KEY (account_id, draft_id)
                 REFERENCES draft_forward_contexts(account_id, draft_id)
                 ON DELETE CASCADE
         );

         INSERT OR IGNORE INTO draft_version_forward_context_refs (
             account_id, draft_id, draft_local_version
         )
         SELECT s.account_id, s.draft_id, s.draft_local_version
         FROM draft_version_snapshots s
         JOIN draft_forward_contexts c
           ON c.account_id = s.account_id AND c.draft_id = s.draft_id;",
    )?;

    if schema_version < 15 {
        transaction.execute_batch(
            "DROP TABLE IF EXISTS draft_attachment_refs_v15;
             CREATE TABLE draft_attachment_refs_v15 (
                 account_id TEXT NOT NULL,
                 draft_id TEXT NOT NULL,
                 draft_local_version INTEGER NOT NULL
                     CHECK (draft_local_version > 0),
                 position INTEGER NOT NULL CHECK (position >= 0),
                 blob_id TEXT NOT NULL,
                 source_attachment_id TEXT,
                 PRIMARY KEY (
                     account_id, draft_id, draft_local_version, position
                 ),
                 UNIQUE (
                     account_id, draft_id, draft_local_version, blob_id
                 ),
                 FOREIGN KEY (
                     account_id, draft_id, draft_local_version
                 ) REFERENCES draft_version_snapshots(
                     account_id, draft_id, draft_local_version
                 ) ON DELETE CASCADE,
                 FOREIGN KEY (account_id, blob_id)
                     REFERENCES managed_attachment_blobs(account_id, id)
                     ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
             );
             INSERT INTO draft_attachment_refs_v15 (
                 account_id, draft_id, draft_local_version, position,
                 blob_id, source_attachment_id
             )
             SELECT account_id, draft_id, draft_local_version, position,
                    blob_id, source_attachment_id
             FROM draft_attachment_refs;
             DROP TABLE draft_attachment_refs;
             ALTER TABLE draft_attachment_refs_v15
                 RENAME TO draft_attachment_refs;",
        )?;
    }
    transaction.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_draft_attachment_refs_blob
             ON draft_attachment_refs(account_id, blob_id);
         CREATE INDEX IF NOT EXISTS idx_draft_version_snapshots_draft
             ON draft_version_snapshots(account_id, draft_id, draft_local_version);
         CREATE TRIGGER IF NOT EXISTS trg_draft_version_snapshots_immutable
         BEFORE UPDATE ON draft_version_snapshots
         BEGIN
             SELECT RAISE(ABORT, 'draft version snapshot is immutable');
         END;",
    )?;
    transaction.commit()?;
    Ok(())
}

fn migrate_bcc_and_outbox_recipient_groups_v16(connection: &Connection) -> Result<()> {
    if !table_has_column(connection, "messages", "bcc_json")? {
        // A legacy cached message has no trustworthy Bcc source. In
        // particular, neither the account address nor any transport envelope
        // proves that a Bcc header existed, so old rows intentionally start
        // empty.
        connection.execute_batch(
            "ALTER TABLE messages
                 ADD COLUMN bcc_json TEXT NOT NULL DEFAULT '[]';",
        )?;
    }
    if !table_has_column(connection, "outbox", "recipient_groups_json")? {
        // Legacy rows retain only the flat SMTP envelope. Keep grouping NULL:
        // deriving To/Cc/Bcc from either the envelope or RFC822 bytes could
        // silently reveal or misclassify a blind recipient.
        connection.execute_batch(
            "ALTER TABLE outbox
                 ADD COLUMN recipient_groups_json TEXT;",
        )?;
    }
    connection.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS trg_outbox_delivery_payload_immutable
         BEFORE UPDATE OF recipients_json, recipient_groups_json, raw_rfc822 ON outbox
         WHEN NEW.recipients_json IS NOT OLD.recipients_json
           OR NEW.recipient_groups_json IS NOT OLD.recipient_groups_json
           OR NEW.raw_rfc822 IS NOT OLD.raw_rfc822
         BEGIN
             SELECT RAISE(ABORT, 'Outbox delivery payload is immutable');
         END;",
    )?;
    Ok(())
}

fn migrate_managed_attachment_digests_v17(connection: &Connection) -> Result<()> {
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(
        "DROP TRIGGER IF EXISTS trg_managed_attachment_blobs_immutable;
         DROP TRIGGER IF EXISTS trg_managed_attachment_digest_once;

         CREATE TRIGGER trg_managed_attachment_blobs_immutable
         BEFORE UPDATE OF
             id, account_id, origin_draft_id, internal_name, name, mime_type,
             size_bytes, disposition, transfer_encoding, created_at
         ON managed_attachment_blobs
         BEGIN
             SELECT RAISE(ABORT, 'managed attachment blobs are immutable');
         END;

         CREATE TRIGGER trg_managed_attachment_digest_once
         BEFORE UPDATE OF sha256_hex ON managed_attachment_blobs
         WHEN OLD.sha256_hex IS NOT NULL
           OR NEW.sha256_hex IS NULL
           OR length(NEW.sha256_hex) <> 64
           OR lower(NEW.sha256_hex) <> NEW.sha256_hex
           OR NEW.sha256_hex GLOB '*[^0-9a-f]*'
         BEGIN
             SELECT RAISE(
                 ABORT,
                 'managed attachment digest may only be initialized once'
             );
         END;",
    )?;
    transaction.commit()?;
    Ok(())
}

fn migrate_message_body_cache_v18(connection: &Connection) -> Result<()> {
    if !table_has_column(connection, "messages", "body_cached_bytes")? {
        connection.execute_batch(
            "ALTER TABLE messages
                 ADD COLUMN body_cached_bytes INTEGER NOT NULL DEFAULT 0
                 CHECK (body_cached_bytes >= 0);",
        )?;
    }
    if !table_has_column(connection, "messages", "body_last_accessed_at")? {
        connection.execute_batch(
            "ALTER TABLE messages
                 ADD COLUMN body_last_accessed_at TEXT;",
        )?;
    }
    connection.execute_batch(
        "UPDATE messages
         SET body_cached_bytes =
                 length(raw_rfc822)
                 + length(CAST(COALESCE(body_text, '') AS BLOB))
                 + length(CAST(COALESCE(body_html, '') AS BLOB))
                 + length(CAST(attachment_names_json AS BLOB)),
             body_last_accessed_at = COALESCE(
                 body_last_accessed_at,
                 synced_at,
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             )
         WHERE body_fetched = 1
           AND (
               body_cached_bytes = 0
               OR body_last_accessed_at IS NULL
           );
         UPDATE messages
         SET body_cached_bytes = 0,
             body_last_accessed_at = NULL
         WHERE body_fetched = 0;",
    )?;
    Ok(())
}

fn migrate_message_contact_emails_v19(connection: &Connection) -> Result<()> {
    let schema_version: u32 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS message_contact_emails (
             message_id INTEGER NOT NULL,
             account_id TEXT NOT NULL,
             email TEXT NOT NULL CHECK (
                 length(email) > 0 AND email = lower(trim(email))
             ),
             PRIMARY KEY (message_id, email),
             FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS idx_message_contact_emails_lookup
             ON message_contact_emails(account_id, email, message_id);

         CREATE TRIGGER IF NOT EXISTS trg_messages_contact_emails_insert
         AFTER INSERT ON messages
         BEGIN
             INSERT OR IGNORE INTO message_contact_emails (message_id, account_id, email)
             SELECT NEW.id, NEW.account_id,
                    lower(trim(json_extract(NEW.sender_json, '$.email')))
             WHERE json_valid(NEW.sender_json)
               AND length(trim(COALESCE(json_extract(NEW.sender_json, '$.email'), ''))) > 0;
             INSERT OR IGNORE INTO message_contact_emails (message_id, account_id, email)
             SELECT NEW.id, NEW.account_id,
                    lower(trim(json_extract(recipient.value, '$.email')))
             FROM json_each(CASE
                 WHEN json_valid(NEW.to_json) THEN NEW.to_json ELSE '[]'
             END) AS recipient
             WHERE length(trim(COALESCE(json_extract(recipient.value, '$.email'), ''))) > 0;
             INSERT OR IGNORE INTO message_contact_emails (message_id, account_id, email)
             SELECT NEW.id, NEW.account_id,
                    lower(trim(json_extract(recipient.value, '$.email')))
             FROM json_each(CASE
                 WHEN json_valid(NEW.cc_json) THEN NEW.cc_json ELSE '[]'
             END) AS recipient
             WHERE length(trim(COALESCE(json_extract(recipient.value, '$.email'), ''))) > 0;
         END;

         CREATE TRIGGER IF NOT EXISTS trg_messages_contact_emails_update
         AFTER UPDATE OF account_id, sender_json, to_json, cc_json ON messages
         BEGIN
             DELETE FROM message_contact_emails WHERE message_id = OLD.id;
             INSERT OR IGNORE INTO message_contact_emails (message_id, account_id, email)
             SELECT NEW.id, NEW.account_id,
                    lower(trim(json_extract(NEW.sender_json, '$.email')))
             WHERE json_valid(NEW.sender_json)
               AND length(trim(COALESCE(json_extract(NEW.sender_json, '$.email'), ''))) > 0;
             INSERT OR IGNORE INTO message_contact_emails (message_id, account_id, email)
             SELECT NEW.id, NEW.account_id,
                    lower(trim(json_extract(recipient.value, '$.email')))
             FROM json_each(CASE
                 WHEN json_valid(NEW.to_json) THEN NEW.to_json ELSE '[]'
             END) AS recipient
             WHERE length(trim(COALESCE(json_extract(recipient.value, '$.email'), ''))) > 0;
             INSERT OR IGNORE INTO message_contact_emails (message_id, account_id, email)
             SELECT NEW.id, NEW.account_id,
                    lower(trim(json_extract(recipient.value, '$.email')))
             FROM json_each(CASE
                 WHEN json_valid(NEW.cc_json) THEN NEW.cc_json ELSE '[]'
             END) AS recipient
             WHERE length(trim(COALESCE(json_extract(recipient.value, '$.email'), ''))) > 0;
         END;",
    )?;

    if schema_version >= 19 {
        return Ok(());
    }

    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(
        "INSERT OR IGNORE INTO message_contact_emails (message_id, account_id, email)
         SELECT id, account_id, lower(trim(json_extract(sender_json, '$.email')))
         FROM messages
         WHERE json_valid(sender_json)
           AND length(trim(COALESCE(json_extract(sender_json, '$.email'), ''))) > 0;
         INSERT OR IGNORE INTO message_contact_emails (message_id, account_id, email)
         SELECT messages.id, messages.account_id,
                lower(trim(json_extract(recipient.value, '$.email')))
         FROM messages
         JOIN json_each(CASE
             WHEN json_valid(messages.to_json) THEN messages.to_json ELSE '[]'
         END) AS recipient
         WHERE length(trim(COALESCE(json_extract(recipient.value, '$.email'), ''))) > 0;
         INSERT OR IGNORE INTO message_contact_emails (message_id, account_id, email)
         SELECT messages.id, messages.account_id,
                lower(trim(json_extract(recipient.value, '$.email')))
         FROM messages
         JOIN json_each(CASE
             WHEN json_valid(messages.cc_json) THEN messages.cc_json ELSE '[]'
         END) AS recipient
         WHERE length(trim(COALESCE(json_extract(recipient.value, '$.email'), ''))) > 0;",
    )?;
    transaction.commit()?;
    Ok(())
}

fn migrate_starred_history_v20(connection: &Connection) -> Result<()> {
    let mailbox_columns = [
        ("starred_history_before_uid", "INTEGER"),
        (
            "starred_history_complete",
            "INTEGER NOT NULL DEFAULT 0 CHECK (starred_history_complete IN (0, 1))",
        ),
    ];
    for (column, declaration) in mailbox_columns {
        if !table_has_column(connection, "mailboxes", column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE mailboxes ADD COLUMN {column} {declaration};"
            ))?;
        }
    }
    if !table_has_column(connection, "message_page_cursors", "flagged_only")? {
        connection.execute_batch(
            "ALTER TABLE message_page_cursors
             ADD COLUMN flagged_only INTEGER NOT NULL DEFAULT 0
             CHECK (flagged_only IN (0, 1));",
        )?;
    }
    Ok(())
}

fn flags_with_pending_updates(
    connection: &Connection,
    account_id: &str,
    mailbox: &str,
    uid: u32,
    flags: &[String],
) -> Result<Vec<String>> {
    let pending_seen: Option<bool> = connection
        .query_row(
            "SELECT q.desired
             FROM pending_seen_updates q
             JOIN mailboxes b
               ON b.account_id = q.account_id
              AND b.name = q.mailbox
              AND b.uid_validity = q.source_uid_validity
             WHERE q.account_id = ?1 AND q.mailbox = ?2 AND q.uid = ?3
               AND q.status <> 'confirmed'",
            params![account_id, mailbox, uid],
            |row| row.get(0),
        )
        .optional()?;
    let mut flags = flags.to_vec();
    if let Some(desired) = pending_seen {
        set_system_flag(&mut flags, "\\Seen", desired);
    }
    let pending_flagged: Option<bool> = connection
        .query_row(
            "SELECT q.desired
             FROM pending_flagged_updates q
             JOIN mailboxes b
               ON b.account_id = q.account_id
              AND b.name = q.mailbox
              AND b.uid_validity = q.source_uid_validity
             WHERE q.account_id = ?1 AND q.mailbox = ?2 AND q.uid = ?3
               AND q.status <> 'confirmed'",
            params![account_id, mailbox, uid],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(desired) = pending_flagged {
        set_system_flag(&mut flags, "\\Flagged", desired);
    }
    Ok(flags)
}

fn set_system_flag(flags: &mut Vec<String>, target: &str, desired: bool) -> bool {
    let present = flags.iter().any(|flag| flag.eq_ignore_ascii_case(target));
    if present == desired {
        return false;
    }
    if desired {
        flags.push(target.to_owned());
    } else {
        flags.retain(|flag| !flag.eq_ignore_ascii_case(target));
    }
    true
}

fn system_flag_table(flag: SystemFlagKind) -> &'static str {
    match flag {
        SystemFlagKind::Seen => "pending_seen_updates",
        SystemFlagKind::Flagged => "pending_flagged_updates",
    }
}

fn system_flag_name(flag: SystemFlagKind) -> &'static str {
    match flag {
        SystemFlagKind::Seen => "\\Seen",
        SystemFlagKind::Flagged => "\\Flagged",
    }
}

fn privacy_safe_not_found(entity: &'static str) -> MailError {
    MailError::NotFound {
        entity,
        id: "unavailable".to_owned(),
    }
}

fn account_scope_mismatch() -> MailError {
    MailError::Validation("the requested item is outside the active account".to_owned())
}

fn query_system_flag_mutation_by_identity(
    connection: &Connection,
    flag: SystemFlagKind,
    account_id: &str,
    mailbox: &str,
    source_uid_validity: u32,
    uid: u32,
) -> Result<Option<PendingSystemFlagMutation>> {
    let table = system_flag_table(flag);
    connection
        .query_row(
            &format!(
                "SELECT q.operation_id, q.account_id, q.mailbox, q.source_uid_validity,
                        q.uid,
                        COALESCE((
                            SELECT r.role
                            FROM mailbox_roles r
                            WHERE r.account_id = q.account_id AND r.mailbox = q.mailbox
                            ORDER BY CASE r.role WHEN 'inbox' THEN 0 ELSE 1 END
                            LIMIT 1
                        ), 'inbox'),
                        q.desired, q.revision, q.status, q.error_kind, q.updated_at
                 FROM {table} q
                 WHERE q.account_id = ?1 AND q.mailbox = ?2
                   AND q.source_uid_validity = ?3 AND q.uid = ?4"
            ),
            params![account_id, mailbox, source_uid_validity, uid],
            |row| row_to_pending_system_flag_mutation(row, flag),
        )
        .optional()
        .map_err(Into::into)
}

fn query_system_flag_mutation_by_operation(
    connection: &Connection,
    flag: SystemFlagKind,
    account_id: &str,
    operation_id: &str,
) -> Result<Option<PendingSystemFlagMutation>> {
    let table = system_flag_table(flag);
    connection
        .query_row(
            &format!(
                "SELECT q.operation_id, q.account_id, q.mailbox, q.source_uid_validity,
                        q.uid,
                        COALESCE((
                            SELECT r.role
                            FROM mailbox_roles r
                            WHERE r.account_id = q.account_id AND r.mailbox = q.mailbox
                            ORDER BY CASE r.role WHEN 'inbox' THEN 0 ELSE 1 END
                            LIMIT 1
                        ), 'inbox'),
                        q.desired, q.revision, q.status, q.error_kind, q.updated_at
                 FROM {table} q
                 WHERE q.account_id = ?1 AND q.operation_id = ?2"
            ),
            params![account_id, operation_id],
            |row| row_to_pending_system_flag_mutation(row, flag),
        )
        .optional()
        .map_err(Into::into)
}

fn query_pending_system_flag_mutations(
    connection: &Connection,
    flag: SystemFlagKind,
    account_id: &str,
    mailbox: &str,
) -> Result<Vec<PendingSystemFlagMutation>> {
    let table = system_flag_table(flag);
    let mut statement = connection.prepare(&format!(
        "SELECT q.operation_id, q.account_id, q.mailbox, q.source_uid_validity,
                q.uid,
                COALESCE((
                    SELECT r.role
                    FROM mailbox_roles r
                    WHERE r.account_id = q.account_id AND r.mailbox = q.mailbox
                    ORDER BY CASE r.role WHEN 'inbox' THEN 0 ELSE 1 END
                    LIMIT 1
                ), 'inbox'),
                q.desired, q.revision, q.status, q.error_kind, q.updated_at
         FROM {table} q
         JOIN mailboxes b
           ON b.account_id = q.account_id AND b.name = q.mailbox
          AND b.uid_validity = q.source_uid_validity
         WHERE q.account_id = ?1 AND q.mailbox = ?2 AND q.status = 'pending'
         ORDER BY q.updated_at, q.operation_id"
    ))?;
    statement
        .query_map(params![account_id, mailbox], |row| {
            row_to_pending_system_flag_mutation(row, flag)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn query_reconcilable_system_flag_mutations(
    connection: &Connection,
    flag: SystemFlagKind,
    account_id: &str,
    mailbox: &str,
) -> Result<Vec<PendingSystemFlagMutation>> {
    let table = system_flag_table(flag);
    let mut statement = connection.prepare(&format!(
        "SELECT q.operation_id, q.account_id, q.mailbox, q.source_uid_validity,
                q.uid,
                COALESCE((
                    SELECT r.role
                    FROM mailbox_roles r
                    WHERE r.account_id = q.account_id AND r.mailbox = q.mailbox
                    ORDER BY CASE r.role WHEN 'inbox' THEN 0 ELSE 1 END
                    LIMIT 1
                ), 'inbox'),
                q.desired, q.revision, q.status, q.error_kind, q.updated_at
         FROM {table} q
         JOIN mailboxes b
           ON b.account_id = q.account_id AND b.name = q.mailbox
          AND b.uid_validity = q.source_uid_validity
         WHERE q.account_id = ?1 AND q.mailbox = ?2
           AND q.status IN ('outcome_unknown', 'needs_attention')
         ORDER BY q.updated_at, q.operation_id"
    ))?;
    statement
        .query_map(params![account_id, mailbox], |row| {
            row_to_pending_system_flag_mutation(row, flag)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn row_to_pending_system_flag_mutation(
    row: &Row<'_>,
    flag: SystemFlagKind,
) -> rusqlite::Result<PendingSystemFlagMutation> {
    let error_kind = row.get::<_, Option<String>>(9)?;
    Ok(PendingSystemFlagMutation {
        operation_id: row.get(0)?,
        account_id: row.get(1)?,
        source_mailbox: row.get(2)?,
        source_uid_validity: row.get(3)?,
        source_uid: row.get(4)?,
        source_role: mailbox_role_from_column(5, row.get(5)?)?,
        flag,
        desired: row.get(6)?,
        revision: decode_u64(7, row.get(7)?)?,
        status: mutation_status_from_column(8, row.get(8)?)?,
        error_kind: error_kind
            .map(|value| message_mutation_error_from_column(9, value))
            .transpose()?,
        updated_at: row.get(10)?,
    })
}

#[cfg(test)]
fn system_flag_mutation_receipt(mutation: PendingSystemFlagMutation) -> SystemFlagMutationReceipt {
    SystemFlagMutationReceipt {
        operation_id: mutation.operation_id,
        local_revision: mutation.revision,
        status: mutation.status,
        source_role: mutation.source_role,
        flag: mutation.flag,
        desired: mutation.desired,
    }
}

fn finalize_system_flag_mutation_confirmed(
    connection: &mut Connection,
    account_id: &str,
    operation_id: &str,
    flag: SystemFlagKind,
    expected_revision: u64,
    server_flags: &[String],
    allow_reconcile: bool,
) -> Result<bool> {
    let table = system_flag_table(flag);
    let target = system_flag_name(flag);
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let Some(mutation) =
        query_system_flag_mutation_by_operation(&transaction, flag, account_id, operation_id)?
    else {
        transaction.commit()?;
        return Ok(false);
    };
    let status_is_valid = if allow_reconcile {
        matches!(
            mutation.status,
            MutationStatus::OutcomeUnknown | MutationStatus::NeedsAttention
        )
    } else {
        mutation.status == MutationStatus::InFlight
    };
    if mutation.revision != expected_revision || !status_is_valid {
        transaction.commit()?;
        return Ok(false);
    }
    let server_matches = server_flags
        .iter()
        .any(|value| value.eq_ignore_ascii_case(target))
        == mutation.desired;
    if !server_matches {
        transaction.execute(
            &format!(
                "UPDATE {table}
                 SET status = 'needs_attention',
                     error_kind = 'ambiguous_remote_state',
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
                   AND status = ?4"
            ),
            params![
                account_id,
                operation_id,
                u64_to_i64(expected_revision),
                mutation.status.as_str(),
            ],
        )?;
        transaction.commit()?;
        return Ok(false);
    }
    transaction.execute(
        "UPDATE messages
         SET flags_json = ?5
         WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3
           AND EXISTS (
               SELECT 1 FROM mailboxes b
               WHERE b.account_id = messages.account_id AND b.name = messages.mailbox
                 AND b.uid_validity = ?4
           )",
        params![
            account_id,
            mutation.source_mailbox,
            mutation.source_uid,
            mutation.source_uid_validity,
            encode_json(server_flags)?,
        ],
    )?;
    let changed = transaction.execute(
        &format!(
            "UPDATE {table}
             SET status = 'confirmed',
                 error_kind = NULL,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE account_id = ?1 AND operation_id = ?2 AND revision = ?3
               AND status = ?4"
        ),
        params![
            account_id,
            operation_id,
            u64_to_i64(expected_revision),
            mutation.status.as_str(),
        ],
    )?;
    transaction.commit()?;
    Ok(changed == 1)
}

fn parse_mailbox_role(value: &str) -> Result<MailboxRole> {
    MailboxRole::from_str(value).ok_or_else(|| {
        MailError::Validation("the mailbox role is not supported by this version".to_owned())
    })
}

fn validate_mailbox_display_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.chars().count() > MAX_MAILBOX_DISPLAY_CHARS
        || value.chars().any(char::is_control)
    {
        return Err(MailError::Validation(
            "the mailbox display name is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn validate_mailbox_capability(capability: &MailboxCapability) -> Result<()> {
    if let Some(display_name) = capability.display_name.as_deref() {
        validate_mailbox_display_name(display_name)?;
    }
    let reason_is_valid = match capability.status {
        MailboxCapabilityStatus::Unavailable => capability.unavailable_reason.is_some(),
        _ => capability.unavailable_reason.is_none(),
    };
    if !reason_is_valid {
        return Err(MailError::Validation(
            "the mailbox capability reason does not match its status".to_owned(),
        ));
    }
    if capability.status == MailboxCapabilityStatus::Available && capability.display_name.is_none()
    {
        return Err(MailError::Validation(
            "an available mailbox capability requires a display name".to_owned(),
        ));
    }
    Ok(())
}

fn validate_message_action(
    source_role: MailboxRole,
    kind: MessageActionKind,
    destination_role: Option<MailboxRole>,
) -> Result<()> {
    let valid = match kind {
        MessageActionKind::Archive => {
            matches!(source_role, MailboxRole::Inbox | MailboxRole::Sent)
                && destination_role == Some(MailboxRole::Archive)
        }
        MessageActionKind::MoveToTrash => {
            matches!(
                source_role,
                MailboxRole::Inbox | MailboxRole::Sent | MailboxRole::Archive
            ) && destination_role == Some(MailboxRole::Trash)
        }
        MessageActionKind::PermanentDelete => {
            source_role == MailboxRole::Trash && destination_role.is_none()
        }
    };
    if !valid {
        return Err(MailError::Validation(
            "the message action has an invalid semantic destination".to_owned(),
        ));
    }
    Ok(())
}

fn query_message_action_by_identity(
    connection: &Connection,
    account_id: &str,
    source_mailbox: &str,
    source_uid_validity: u32,
    source_uid: u32,
) -> Result<Option<PendingMessageAction>> {
    connection
        .query_row(
            "SELECT operation_id, account_id, source_mailbox, source_uid_validity,
                    source_uid, source_role, destination_role, kind, revision, status,
                    remote_phase, source_message_id, source_internal_date, source_size_bytes,
                    error_kind, source_cleanup_pending, destination_reconciled, updated_at
             FROM pending_message_actions
             WHERE account_id = ?1 AND source_mailbox = ?2
               AND source_uid_validity = ?3 AND source_uid = ?4",
            params![account_id, source_mailbox, source_uid_validity, source_uid],
            row_to_pending_message_action,
        )
        .optional()
        .map_err(Into::into)
}

fn query_message_action_by_operation(
    connection: &Connection,
    account_id: &str,
    operation_id: &str,
) -> Result<Option<PendingMessageAction>> {
    connection
        .query_row(
            "SELECT operation_id, account_id, source_mailbox, source_uid_validity,
                    source_uid, source_role, destination_role, kind, revision, status,
                    remote_phase, source_message_id, source_internal_date, source_size_bytes,
                    error_kind, source_cleanup_pending, destination_reconciled, updated_at
             FROM pending_message_actions
             WHERE account_id = ?1 AND operation_id = ?2",
            params![account_id, operation_id],
            row_to_pending_message_action,
        )
        .optional()
        .map_err(Into::into)
}

fn message_mutation_receipt(action: &PendingMessageAction) -> MessageMutationReceipt {
    MessageMutationReceipt {
        operation_id: action.operation_id.clone(),
        local_revision: action.revision,
        status: action.status,
        source_role: action.source_role,
        destination_role: action.destination_role,
    }
}

fn valid_remote_phase_transition(
    kind: MessageActionKind,
    current: RemoteMutationPhase,
    next: RemoteMutationPhase,
) -> bool {
    match kind {
        MessageActionKind::Archive | MessageActionKind::MoveToTrash => matches!(
            (current, next),
            (
                RemoteMutationPhase::Queued,
                RemoteMutationPhase::TransferStarted
            ) | (
                RemoteMutationPhase::TransferStarted,
                RemoteMutationPhase::TransferAcknowledged
            ) | (
                RemoteMutationPhase::TransferStarted,
                RemoteMutationPhase::SourceDeleteAcknowledged
            ) | (
                RemoteMutationPhase::TransferAcknowledged,
                RemoteMutationPhase::SourceDeleteStarted
            ) | (
                RemoteMutationPhase::SourceDeleteStarted,
                RemoteMutationPhase::SourceDeleteAcknowledged
            )
        ),
        MessageActionKind::PermanentDelete => matches!(
            (current, next),
            (
                RemoteMutationPhase::Queued,
                RemoteMutationPhase::SourceDeleteStarted
            ) | (
                RemoteMutationPhase::SourceDeleteStarted,
                RemoteMutationPhase::SourceDeleteAcknowledged
            )
        ),
    }
}

fn bounded_identity_field(value: Option<&str>, max_chars: usize) -> Option<String> {
    let bounded = value?
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect::<String>();
    (!bounded.is_empty()).then_some(bounded)
}

fn bounded_diagnostic_id(value: &str) -> String {
    format!("{:016x}", stable_fingerprint(value.as_bytes()))
}

fn validate_page_size(page_size: usize) -> Result<usize> {
    let page_size = if page_size == 0 {
        DEFAULT_MESSAGE_PAGE_SIZE
    } else {
        page_size
    };
    if page_size > MAX_MESSAGE_PAGE_SIZE {
        return Err(MailError::Validation(format!(
            "message page size cannot exceed {MAX_MESSAGE_PAGE_SIZE}"
        )));
    }
    Ok(page_size)
}

fn normalize_search_query(query: Option<&str>) -> Result<Option<String>> {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return Ok(None);
    };
    if query.chars().count() > MAX_SEARCH_QUERY_CHARS || query.chars().any(char::is_control) {
        return Err(MailError::Validation(
            "the local mail search query is invalid".to_owned(),
        ));
    }
    Ok(Some(query.to_lowercase()))
}

fn search_like_pattern(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len().saturating_add(2));
    escaped.push('%');
    for character in query.chars().flat_map(char::to_lowercase) {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped.push('%');
    escaped
}

fn stable_fingerprint(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn unavailable_message_page(remote_history_state: RemoteHistoryState) -> MessagePage {
    MessagePage {
        items: Vec::new(),
        next_cursor: None,
        has_more_local: false,
        remote_history_state,
        end_reached: remote_history_state == RemoteHistoryState::Complete,
    }
}

fn validate_history_advance(
    expected_uid_validity: u32,
    expected_before_uid: Option<u32>,
    next_before_uid: Option<u32>,
    complete: bool,
    label: &str,
) -> Result<()> {
    if expected_uid_validity == 0 || expected_before_uid == Some(0) || next_before_uid == Some(0) {
        return Err(MailError::Validation(format!(
            "the {label} cursor is invalid"
        )));
    }
    if let (Some(expected), Some(next)) = (expected_before_uid, next_before_uid)
        && next >= expected
    {
        return Err(MailError::Validation(format!(
            "the {label} cursor must move to an older UID bound"
        )));
    }
    if expected_before_uid.is_some() && next_before_uid.is_none() && !complete {
        return Err(MailError::Validation(format!(
            "an unfinished {label} scan cannot discard its UID bound"
        )));
    }
    Ok(())
}

fn issue_message_cursor(
    connection: &Connection,
    payload: MessageCursorPayload,
) -> Result<MessagePageCursor> {
    validate_message_cursor_payload(&payload)?;
    connection.execute(
        "DELETE FROM message_page_cursors
         WHERE created_at < unixepoch() - ?1",
        params![MESSAGE_CURSOR_TTL_SECONDS],
    )?;
    let token = Uuid::now_v7().to_string();
    connection.execute(
        "INSERT INTO message_page_cursors (
             token, account_id, role, mailbox, uid_validity,
             sort_at, uid, message_row_id, remote_before_uid, query_normalized,
             flagged_only
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            token,
            payload.account_id,
            payload.role.as_str(),
            payload.mailbox,
            payload.uid_validity,
            payload.sort_at,
            payload.uid,
            payload.id,
            payload.remote_before_uid,
            payload.query_normalized,
            payload.flagged_only,
        ],
    )?;
    Ok(MessagePageCursor::new(token))
}

fn load_message_cursor(
    connection: &Connection,
    cursor: &MessagePageCursor,
) -> Result<MessageCursorPayload> {
    validate_message_cursor_token(cursor)?;
    let payload = connection
        .query_row(
            "SELECT account_id, mailbox, role, uid_validity, query_normalized,
                    sort_at, uid, message_row_id, remote_before_uid, flagged_only
             FROM message_page_cursors
             WHERE token = ?1
               AND created_at >= unixepoch() - ?2",
            params![cursor.as_str(), MESSAGE_CURSOR_TTL_SECONDS],
            |row| {
                Ok(MessageCursorPayload {
                    account_id: row.get(0)?,
                    mailbox: row.get(1)?,
                    role: mailbox_role_from_column(2, row.get(2)?)?,
                    uid_validity: row.get(3)?,
                    query_normalized: row.get(4)?,
                    sort_at: row.get(5)?,
                    uid: row.get(6)?,
                    id: row.get(7)?,
                    remote_before_uid: row.get(8)?,
                    flagged_only: row.get(9)?,
                })
            },
        )
        .optional()?
        .ok_or_else(invalid_message_cursor)?;
    validate_message_cursor_payload(&payload)?;
    Ok(payload)
}

fn load_and_validate_message_cursor(
    connection: &Connection,
    cursor: &MessagePageCursor,
    account_id: &str,
    mailbox: &str,
    role: MailboxRole,
    uid_validity: Option<u32>,
    query_normalized: &str,
    flagged_only: bool,
) -> Result<MessageCursorPayload> {
    let payload = load_message_cursor(connection, cursor)?;
    if payload.account_id != account_id
        || payload.mailbox != mailbox
        || payload.role != role
        || payload.uid_validity != uid_validity
        || payload.query_normalized != query_normalized
        || payload.flagged_only != flagged_only
    {
        return Err(invalid_message_cursor());
    }
    Ok(payload)
}

fn validate_message_cursor_token(cursor: &MessagePageCursor) -> Result<()> {
    let token = cursor.as_str();
    if token.is_empty() || token.len() > MAX_CURSOR_TOKEN_BYTES || Uuid::parse_str(token).is_err() {
        return Err(invalid_message_cursor());
    }
    Ok(())
}

fn validate_message_cursor_payload(payload: &MessageCursorPayload) -> Result<()> {
    let keyset_is_consistent = matches!(
        (&payload.sort_at, payload.uid, payload.id),
        (None, None, None) | (Some(_), Some(_), Some(_))
    );
    if payload.account_id.is_empty()
        || payload.account_id.len() > 256
        || payload.mailbox.is_empty()
        || payload.mailbox.chars().count() > MAX_MAILBOX_DISPLAY_CHARS
        || payload.mailbox.chars().any(char::is_control)
        || payload.query_normalized.chars().count() > MAX_SEARCH_QUERY_CHARS
        || payload.query_normalized.chars().any(char::is_control)
        || !keyset_is_consistent
        || payload.uid == Some(0)
        || payload.id.is_some_and(|id| id <= 0)
        || payload.remote_before_uid == 0
        || payload.sort_at.as_deref().is_some_and(|value| {
            value.len() > MAX_IDENTITY_DATE_CHARS || value.chars().any(char::is_control)
        })
    {
        return Err(invalid_message_cursor());
    }
    Ok(())
}

fn invalid_message_cursor() -> MailError {
    MailError::Validation(
        "the message cursor does not belong to this account, mailbox epoch, or search".to_owned(),
    )
}

fn validate_message_public_id(public_id: &str) -> Result<()> {
    let parsed = Uuid::parse_str(public_id).map_err(|_| {
        MailError::Validation("the opaque message identifier is invalid".to_owned())
    })?;
    if parsed.to_string() != public_id || parsed.get_version() != Some(uuid::Version::Random) {
        return Err(MailError::Validation(
            "the opaque message identifier is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn query_regular_page_candidates(
    connection: &Connection,
    account_id: &str,
    mailbox: &str,
    role: MailboxRole,
    uid_validity: Option<u32>,
    cursor: Option<&MessageCursorPayload>,
    search_pattern: Option<&str>,
    limit: usize,
    flagged_only: bool,
) -> Result<Vec<PageCandidate>> {
    let sql = format!(
        "SELECT {ALIASED_MESSAGE_SUMMARY_COLUMNS}, m.public_id,
                COALESCE(m.internal_date, m.sent_at, m.synced_at) AS sort_at
         FROM messages m
         WHERE m.account_id = :account_id
           AND m.mailbox = :mailbox
           AND NOT EXISTS (
               SELECT 1
               FROM pending_message_actions p
               WHERE p.account_id = m.account_id
                 AND p.source_mailbox = m.mailbox
                 AND p.source_uid = m.uid
                 AND p.source_uid_validity = :uid_validity
           )
           AND (
               :flagged_only = 0
               OR EXISTS (
                   SELECT 1
                   FROM json_each(CASE
                       WHEN json_valid(m.flags_json) THEN m.flags_json ELSE '[]'
                   END) AS message_flag
                   WHERE lower(CAST(message_flag.value AS TEXT)) = lower(:flagged_name)
               )
           )
           AND (
               :search_pattern IS NULL
               OR lower(m.subject) LIKE :search_pattern ESCAPE '\\'
               OR lower(COALESCE(m.sender_json, '')) LIKE :search_pattern ESCAPE '\\'
               OR lower(m.to_json) LIKE :search_pattern ESCAPE '\\'
               OR lower(m.cc_json) LIKE :search_pattern ESCAPE '\\'
               OR lower(m.preview) LIKE :search_pattern ESCAPE '\\'
           )
           AND (
               :cursor_sort IS NULL
               OR COALESCE(m.internal_date, m.sent_at, m.synced_at) < :cursor_sort
               OR (
                   COALESCE(m.internal_date, m.sent_at, m.synced_at) = :cursor_sort
                   AND m.uid < :cursor_uid
               )
               OR (
                   COALESCE(m.internal_date, m.sent_at, m.synced_at) = :cursor_sort
                   AND m.uid = :cursor_uid
                   AND m.id < :cursor_id
               )
           )
         ORDER BY sort_at DESC, m.uid DESC, m.id DESC
         LIMIT :limit"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query(named_params! {
        ":account_id": account_id,
        ":mailbox": mailbox,
        ":uid_validity": uid_validity,
        ":search_pattern": search_pattern,
        ":flagged_only": flagged_only,
        ":flagged_name": "\\Flagged",
        ":cursor_sort": cursor.and_then(|cursor| cursor.sort_at.as_deref()),
        ":cursor_uid": cursor.and_then(|cursor| cursor.uid),
        ":cursor_id": cursor.and_then(|cursor| cursor.id),
        ":limit": usize_to_i64(limit),
    })?;
    let mut candidates = Vec::new();
    while let Some(row) = rows.next()? {
        let message = row_to_message(row)?;
        let public_id = row.get(23)?;
        let sort_at = row.get(24)?;
        candidates.push(PageCandidate {
            uid: message.uid,
            id: message.id,
            item: MessagePageItem {
                public_id,
                message,
                displayed_role: role,
                pending_mutation: None,
            },
            sort_at,
        });
    }
    Ok(candidates)
}

fn query_pending_page_candidates(
    connection: &Connection,
    account_id: &str,
    role: MailboxRole,
    cursor: Option<&MessageCursorPayload>,
    search_pattern: Option<&str>,
    limit: usize,
    flagged_only: bool,
) -> Result<Vec<PageCandidate>> {
    let sql = format!(
        "SELECT {ALIASED_MESSAGE_SUMMARY_COLUMNS}, m.public_id,
                COALESCE(m.internal_date, m.sent_at, m.synced_at) AS sort_at,
                p.operation_id, p.revision, p.status, p.kind, p.source_role,
                p.destination_role, p.error_kind
         FROM pending_message_actions p
         JOIN messages m
           ON m.account_id = p.account_id
          AND m.mailbox = p.source_mailbox
          AND m.uid = p.source_uid
         JOIN mailboxes b
           ON b.account_id = p.account_id
          AND b.name = p.source_mailbox
          AND b.uid_validity = p.source_uid_validity
         WHERE p.account_id = :account_id
           AND p.destination_role = :destination_role
           AND p.destination_reconciled = 0
           AND p.status IN (
               'pending', 'in_flight', 'confirmed', 'needs_attention', 'outcome_unknown'
           )
           AND (
               :flagged_only = 0
               OR EXISTS (
                   SELECT 1
                   FROM json_each(CASE
                       WHEN json_valid(m.flags_json) THEN m.flags_json ELSE '[]'
                   END) AS message_flag
                   WHERE lower(CAST(message_flag.value AS TEXT)) = lower(:flagged_name)
               )
           )
           AND (
               :search_pattern IS NULL
               OR lower(m.subject) LIKE :search_pattern ESCAPE '\\'
               OR lower(COALESCE(m.sender_json, '')) LIKE :search_pattern ESCAPE '\\'
               OR lower(m.to_json) LIKE :search_pattern ESCAPE '\\'
               OR lower(m.cc_json) LIKE :search_pattern ESCAPE '\\'
               OR lower(m.preview) LIKE :search_pattern ESCAPE '\\'
           )
           AND (
               :cursor_sort IS NULL
               OR COALESCE(m.internal_date, m.sent_at, m.synced_at) < :cursor_sort
               OR (
                   COALESCE(m.internal_date, m.sent_at, m.synced_at) = :cursor_sort
                   AND m.uid < :cursor_uid
               )
               OR (
                   COALESCE(m.internal_date, m.sent_at, m.synced_at) = :cursor_sort
                   AND m.uid = :cursor_uid
                   AND m.id < :cursor_id
               )
           )
         ORDER BY sort_at DESC, m.uid DESC, m.id DESC
         LIMIT :limit"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query(named_params! {
        ":account_id": account_id,
        ":destination_role": role.as_str(),
        ":search_pattern": search_pattern,
        ":flagged_only": flagged_only,
        ":flagged_name": "\\Flagged",
        ":cursor_sort": cursor.and_then(|cursor| cursor.sort_at.as_deref()),
        ":cursor_uid": cursor.and_then(|cursor| cursor.uid),
        ":cursor_id": cursor.and_then(|cursor| cursor.id),
        ":limit": usize_to_i64(limit),
    })?;
    let mut candidates = Vec::new();
    while let Some(row) = rows.next()? {
        let message = row_to_message(row)?;
        let public_id = row.get(23)?;
        let sort_at = row.get(24)?;
        let destination_role = mailbox_role_from_column(30, row.get(30)?)?;
        let projection = PendingMessageProjection {
            operation_id: row.get(25)?,
            local_revision: decode_u64(26, row.get(26)?)?,
            status: mutation_status_from_column(27, row.get(27)?)?,
            kind: message_action_kind_from_column(28, row.get(28)?)?,
            source_role: mailbox_role_from_column(29, row.get(29)?)?,
            destination_role,
            error_kind: row
                .get::<_, Option<String>>(31)?
                .map(|value| message_mutation_error_from_column(31, value))
                .transpose()?,
        };
        candidates.push(PageCandidate {
            uid: message.uid,
            id: message.id,
            item: MessagePageItem {
                public_id,
                message,
                displayed_role: role,
                pending_mutation: Some(projection),
            },
            sort_at,
        });
    }
    Ok(candidates)
}

fn compare_page_candidates(left: &PageCandidate, right: &PageCandidate) -> Ordering {
    right
        .sort_at
        .cmp(&left.sort_at)
        .then_with(|| right.uid.cmp(&left.uid))
        .then_with(|| right.id.cmp(&left.id))
}

fn row_to_mailbox_capability(row: &Row<'_>) -> rusqlite::Result<MailboxCapability> {
    let reason = row.get::<_, Option<String>>(3)?;
    Ok(MailboxCapability {
        role: mailbox_role_from_column(0, row.get(0)?)?,
        status: mailbox_capability_status_from_column(1, row.get(1)?)?,
        display_name: row.get(2)?,
        unavailable_reason: reason
            .map(|value| mailbox_unavailable_reason_from_column(3, value))
            .transpose()?,
        retryable: row.get(4)?,
    })
}

fn row_to_pending_message_action(row: &Row<'_>) -> rusqlite::Result<PendingMessageAction> {
    let destination = row.get::<_, Option<String>>(6)?;
    let error_kind = row.get::<_, Option<String>>(14)?;
    Ok(PendingMessageAction {
        operation_id: row.get(0)?,
        account_id: row.get(1)?,
        source_mailbox: row.get(2)?,
        source_uid_validity: row.get(3)?,
        source_uid: row.get(4)?,
        source_role: mailbox_role_from_column(5, row.get(5)?)?,
        destination_role: destination
            .map(|value| mailbox_role_from_column(6, value))
            .transpose()?,
        kind: message_action_kind_from_column(7, row.get(7)?)?,
        revision: decode_u64(8, row.get(8)?)?,
        status: mutation_status_from_column(9, row.get(9)?)?,
        remote_phase: remote_mutation_phase_from_column(10, row.get(10)?)?,
        source_message_id: row.get(11)?,
        source_internal_date: row.get(12)?,
        source_size_bytes: row.get(13)?,
        error_kind: error_kind
            .map(|value| message_mutation_error_from_column(14, value))
            .transpose()?,
        source_cleanup_pending: row.get(15)?,
        destination_reconciled: row.get(16)?,
        updated_at: row.get(17)?,
    })
}

fn mailbox_role_from_column(index: usize, value: String) -> rusqlite::Result<MailboxRole> {
    MailboxRole::from_str(&value).ok_or_else(|| invalid_enum_column(index, &value))
}

fn mailbox_capability_status_from_column(
    index: usize,
    value: String,
) -> rusqlite::Result<MailboxCapabilityStatus> {
    MailboxCapabilityStatus::from_str(&value).ok_or_else(|| invalid_enum_column(index, &value))
}

fn mailbox_unavailable_reason_from_column(
    index: usize,
    value: String,
) -> rusqlite::Result<MailboxCapabilityUnavailableReason> {
    MailboxCapabilityUnavailableReason::from_str(&value)
        .ok_or_else(|| invalid_enum_column(index, &value))
}

fn mutation_status_from_column(index: usize, value: String) -> rusqlite::Result<MutationStatus> {
    MutationStatus::from_str(&value).ok_or_else(|| invalid_enum_column(index, &value))
}

fn remote_mutation_phase_from_column(
    index: usize,
    value: String,
) -> rusqlite::Result<RemoteMutationPhase> {
    RemoteMutationPhase::from_str(&value).ok_or_else(|| invalid_enum_column(index, &value))
}

fn message_action_kind_from_column(
    index: usize,
    value: String,
) -> rusqlite::Result<MessageActionKind> {
    MessageActionKind::from_str(&value).ok_or_else(|| invalid_enum_column(index, &value))
}

fn message_mutation_error_from_column(
    index: usize,
    value: String,
) -> rusqlite::Result<MessageMutationErrorKind> {
    MessageMutationErrorKind::from_str(&value).ok_or_else(|| invalid_enum_column(index, &value))
}

fn invalid_enum_column(index: usize, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unsupported bounded enum value {value}"),
        )),
    )
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    debug_assert!(
        table
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(names.iter().any(|name| name == column))
}

fn table_references_table(
    connection: &Connection,
    table: &str,
    referenced_table: &str,
) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA foreign_key_list({table})"))?;
    let targets = statement
        .query_map([], |row| row.get::<_, String>(2))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(targets
        .iter()
        .any(|target| target.eq_ignore_ascii_case(referenced_table)))
}

fn validate_same_draft_identity(expected: &DraftRecord, replacement: &DraftRecord) -> Result<()> {
    if expected.draft.id != replacement.draft.id
        || expected.draft.account_id != replacement.draft.account_id
    {
        return Err(MailError::Validation(
            "a draft replacement must retain its local id and account".to_owned(),
        ));
    }
    Ok(())
}

fn validate_opaque_attachment_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value.chars().any(char::is_control)
        || Uuid::parse_str(value).is_err()
    {
        return Err(MailError::Validation(
            "the managed attachment identifier is invalid".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn managed_attachment_integrity_error() -> MailError {
    MailError::Validation(
        "a managed attachment is unavailable or failed its immutable content check; remove it and attach the original file again"
            .to_owned(),
    )
}

fn validate_managed_attachment_digest(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(managed_attachment_integrity_error());
    }
    Ok(())
}

fn validate_managed_internal_name(attachment: &ImportedManagedAttachment) -> Result<()> {
    validate_opaque_attachment_id(&attachment.id)?;
    let expected_internal_name = format!("{}.blob", attachment.id);
    let path = Path::new(&attachment.internal_name);
    let mut components = path.components();
    let single_component = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none();
    if !single_component
        || attachment.internal_name != expected_internal_name
        || attachment.internal_name.len() > 80
    {
        return Err(MailError::Validation(
            "the managed attachment storage name is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn validate_new_draft_attachments(additions: &[NewDraftAttachment]) -> Result<()> {
    if additions.len() > crate::mime::MAX_ATTACHMENT_PARTS {
        return Err(MailError::Validation(
            "too many managed attachments are selected".to_owned(),
        ));
    }
    let mut ids = HashSet::new();
    let mut internal_names = HashSet::new();
    let mut total_bytes = 0u64;
    for addition in additions {
        let imported = &addition.imported;
        validate_managed_internal_name(imported)?;
        if !ids.insert(imported.id.as_str())
            || !internal_names.insert(imported.internal_name.as_str())
        {
            return Err(MailError::Validation(
                "the selected attachment set contains duplicate identifiers".to_owned(),
            ));
        }
        if imported.name != crate::mime::safe_attachment_filename(Some(&imported.name))
            || imported.mime_type.is_empty()
            || imported.mime_type.len() > 255
            || !imported.mime_type.contains('/')
            || imported.mime_type.chars().any(char::is_control)
            || imported.size_bytes > i64::MAX as u64
            || imported.size_bytes > crate::mime::MAX_MANAGED_ATTACHMENT_BYTES
            || imported.sha256_hex.len() != 64
            || !imported
                .sha256_hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(MailError::Validation(
                "the managed attachment metadata is invalid".to_owned(),
            ));
        }
        total_bytes = total_bytes
            .checked_add(imported.size_bytes)
            .ok_or_else(|| {
                MailError::Validation("managed attachment byte total overflowed".to_owned())
            })?;
        if total_bytes > crate::mime::MAX_MANAGED_ATTACHMENT_TOTAL_BYTES {
            return Err(MailError::Validation(
                "the selected attachment set is too large".to_owned(),
            ));
        }
        if addition
            .source_attachment_id
            .as_deref()
            .is_some_and(|value| {
                value.is_empty() || value.len() > 256 || value.chars().any(char::is_control)
            })
        {
            return Err(MailError::Validation(
                "the forwarded source attachment identifier is invalid".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_combined_draft_attachment_metadata(
    connection: &Connection,
    account_id: &str,
    draft_id: &str,
    local_version: u64,
    additions: &[NewDraftAttachment],
) -> Result<()> {
    let (existing_count, existing_bytes): (u64, u64) = connection.query_row(
        "SELECT COUNT(*), COALESCE(SUM(b.size_bytes), 0)
         FROM draft_attachment_refs r
         JOIN managed_attachment_blobs b
           ON b.account_id = r.account_id AND b.id = r.blob_id
         WHERE r.account_id = ?1 AND r.draft_id = ?2
           AND r.draft_local_version = ?3",
        params![account_id, draft_id, u64_to_i64(local_version)],
        |row| Ok((decode_u64(0, row.get(0)?)?, decode_u64(1, row.get(1)?)?)),
    )?;
    let combined_count = existing_count
        .checked_add(additions.len() as u64)
        .ok_or_else(|| MailError::Validation("managed attachment count overflowed".to_owned()))?;
    if combined_count > crate::mime::MAX_ATTACHMENT_PARTS as u64 {
        return Err(MailError::Validation(
            "too many managed attachments are selected".to_owned(),
        ));
    }
    let mut combined_bytes = existing_bytes;
    for addition in additions {
        combined_bytes = combined_bytes
            .checked_add(addition.imported.size_bytes)
            .ok_or_else(|| {
                MailError::Validation("managed attachment byte total overflowed".to_owned())
            })?;
    }
    if combined_bytes > crate::mime::MAX_MANAGED_ATTACHMENT_TOTAL_BYTES {
        return Err(MailError::Validation(
            "the combined managed attachment set is too large".to_owned(),
        ));
    }
    Ok(())
}

fn insert_new_draft_attachment_rows(
    connection: &Connection,
    account_id: &str,
    draft_id: &str,
    draft_local_version: u64,
    mut next_position: i64,
    additions: &[NewDraftAttachment],
) -> Result<()> {
    for addition in additions {
        let imported = &addition.imported;
        let inserted = connection.execute(
            "INSERT INTO managed_attachment_blobs (
                 id, account_id, origin_draft_id, internal_name, name, mime_type,
                 size_bytes, sha256_hex, disposition, transfer_encoding
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'attachment', 'base64')
             ON CONFLICT(id) DO NOTHING",
            params![
                imported.id,
                account_id,
                draft_id,
                imported.internal_name,
                imported.name,
                imported.mime_type,
                u64_to_i64(imported.size_bytes),
                imported.sha256_hex,
            ],
        )?;
        if inserted != 1 {
            return Err(MailError::Validation(
                "a managed attachment identifier collision was detected".to_owned(),
            ));
        }
        connection.execute(
            "INSERT INTO draft_attachment_refs (
                 account_id, draft_id, draft_local_version, position,
                 blob_id, source_attachment_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                account_id,
                draft_id,
                u64_to_i64(draft_local_version),
                next_position,
                imported.id,
                addition.source_attachment_id,
            ],
        )?;
        next_position = next_position.checked_add(1).ok_or_else(|| {
            MailError::Validation("draft attachment position limit reached".to_owned())
        })?;
    }
    Ok(())
}

fn draft_version_exists(
    connection: &Connection,
    account_id: &str,
    draft_id: &str,
    local_version: u64,
    require_editable: bool,
) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1
                 FROM draft_version_snapshots v
                 JOIN drafts d
                   ON d.account_id = v.account_id AND d.id = v.draft_id
                 WHERE v.account_id = ?1
                   AND v.draft_id = ?2
                   AND v.draft_local_version = ?3
                   AND (
                       ?4 = 0
                       OR (
                           d.local_version = v.draft_local_version
                           AND d.is_deleted = 0
                           AND d.status != 'sent'
                       )
                   )
             )",
            params![
                account_id,
                draft_id,
                u64_to_i64(local_version),
                require_editable,
            ],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn query_draft_attachments(
    connection: &Connection,
    account_id: &str,
    draft_id: &str,
    local_version: u64,
) -> Result<Vec<ManagedDraftAttachment>> {
    let mut statement = connection.prepare(
        "SELECT b.id, b.name, b.mime_type, b.size_bytes, r.source_attachment_id,
                b.internal_name, b.sha256_hex, b.disposition, b.transfer_encoding
         FROM draft_attachment_refs r
         JOIN managed_attachment_blobs b
           ON b.account_id = r.account_id AND b.id = r.blob_id
         WHERE r.account_id = ?1 AND r.draft_id = ?2 AND r.draft_local_version = ?3
         ORDER BY r.position",
    )?;
    statement
        .query_map(
            params![account_id, draft_id, u64_to_i64(local_version)],
            |row| {
                let disposition = row.get::<_, String>(7)?;
                Ok(ManagedDraftAttachment {
                    meta: DraftAttachmentMeta {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        mime_type: row.get(2)?,
                        size_bytes: decode_u64(3, row.get(3)?)?,
                        source_attachment_id: row.get(4)?,
                    },
                    internal_name: row.get(5)?,
                    sha256_hex: row.get(6)?,
                    disposition: attachment_disposition_from_column(7, disposition)?,
                    transfer_encoding: row.get(8)?,
                })
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[cfg(test)]
fn query_outbox_attachments(
    connection: &Connection,
    account_id: &str,
    outbox_id: &str,
) -> Result<Vec<ManagedDraftAttachment>> {
    let mut statement = connection.prepare(
        "SELECT b.id, b.name, b.mime_type, b.size_bytes, b.internal_name,
                b.sha256_hex, b.disposition, b.transfer_encoding
         FROM outbox_attachment_refs r
         JOIN managed_attachment_blobs b
           ON b.account_id = r.account_id AND b.id = r.blob_id
         WHERE r.account_id = ?1 AND r.outbox_id = ?2
         ORDER BY r.position",
    )?;
    statement
        .query_map(params![account_id, outbox_id], |row| {
            let disposition = row.get::<_, String>(6)?;
            Ok(ManagedDraftAttachment {
                meta: DraftAttachmentMeta {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    mime_type: row.get(2)?,
                    size_bytes: decode_u64(3, row.get(3)?)?,
                    source_attachment_id: None,
                },
                internal_name: row.get(4)?,
                sha256_hex: row.get(5)?,
                disposition: attachment_disposition_from_column(6, disposition)?,
                transfer_encoding: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn clone_draft_attachment_rows(
    connection: &Connection,
    account_id: &str,
    source_draft_id: &str,
    source_local_version: u64,
    target_draft_id: &str,
    target_local_version: u64,
) -> Result<()> {
    let target_count: u32 = connection.query_row(
        "SELECT COUNT(*) FROM draft_attachment_refs
         WHERE account_id = ?1 AND draft_id = ?2 AND draft_local_version = ?3",
        params![
            account_id,
            target_draft_id,
            u64_to_i64(target_local_version)
        ],
        |row| row.get(0),
    )?;
    if target_count > 0 {
        let source = query_draft_attachments(
            connection,
            account_id,
            source_draft_id,
            source_local_version,
        )?;
        let target = query_draft_attachments(
            connection,
            account_id,
            target_draft_id,
            target_local_version,
        )?;
        if source == target {
            return Ok(());
        }
        return Err(MailError::Validation(
            "the draft conflict attachment set is already different".to_owned(),
        ));
    }
    connection.execute(
        "INSERT INTO draft_attachment_refs (
             account_id, draft_id, draft_local_version, position,
             blob_id, source_attachment_id
         )
         SELECT account_id, ?4, ?5, position, blob_id, source_attachment_id
         FROM draft_attachment_refs
         WHERE account_id = ?1 AND draft_id = ?2 AND draft_local_version = ?3
         ORDER BY position",
        params![
            account_id,
            source_draft_id,
            u64_to_i64(source_local_version),
            target_draft_id,
            u64_to_i64(target_local_version),
        ],
    )?;
    Ok(())
}

fn bind_outbox_item_attachment_rows(connection: &Connection, item: &OutboxItem) -> Result<bool> {
    match (
        item.draft_id.as_deref(),
        item.draft_revision,
        item.draft_local_version,
    ) {
        (Some(draft_id), Some(_), Some(local_version)) => bind_outbox_attachment_rows(
            connection,
            &item.account_id,
            &item.id,
            draft_id,
            local_version,
        ),
        (None, None, None) => Ok(false),
        _ => Err(MailError::Validation(
            "an Outbox draft link must include both exact draft versions".to_owned(),
        )),
    }
}

fn bind_outbox_attachment_rows(
    connection: &Connection,
    account_id: &str,
    outbox_id: &str,
    draft_id: &str,
    draft_local_version: u64,
) -> Result<bool> {
    if draft_local_version == 0 {
        return Err(MailError::Validation(
            "an Outbox attachment set requires a positive draft version".to_owned(),
        ));
    }
    let link: Option<(Option<String>, Option<u64>, Option<u64>)> = connection
        .query_row(
            "SELECT draft_id, draft_revision, draft_local_version
             FROM outbox WHERE account_id = ?1 AND id = ?2",
            params![account_id, outbox_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, Option<i64>>(1)?
                        .map(|value| decode_u64(1, value))
                        .transpose()?,
                    row.get::<_, Option<i64>>(2)?
                        .map(|value| decode_u64(2, value))
                        .transpose()?,
                ))
            },
        )
        .optional()?;
    let Some((Some(linked_draft_id), Some(draft_revision), Some(linked_local_version))) = link
    else {
        return Err(MailError::Validation(
            "the Outbox attachment binding is incomplete".to_owned(),
        ));
    };
    if linked_draft_id != draft_id || linked_local_version != draft_local_version {
        return Err(MailError::Validation(
            "the Outbox attachment binding does not match its confirmed draft version".to_owned(),
        ));
    }
    let existing: Option<(String, u64)> = connection
        .query_row(
            "SELECT draft_id, draft_local_version
             FROM outbox_attachment_sets
             WHERE account_id = ?1 AND outbox_id = ?2",
            params![account_id, outbox_id],
            |row| Ok((row.get(0)?, decode_u64(1, row.get(1)?)?)),
        )
        .optional()?;
    if let Some(existing) = existing {
        if existing == (draft_id.to_owned(), draft_local_version) {
            return Ok(false);
        }
        return Err(MailError::Validation(
            "the immutable Outbox attachment set is already bound".to_owned(),
        ));
    }
    let exact_draft_exists: bool = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM drafts
             WHERE account_id = ?1 AND id = ?2
               AND revision = ?3 AND local_version = ?4
               AND is_deleted = 0 AND status != 'sent'
         )",
        params![
            account_id,
            draft_id,
            u64_to_i64(draft_revision),
            u64_to_i64(draft_local_version),
        ],
        |row| row.get(0),
    )?;
    if !exact_draft_exists {
        return Err(MailError::Validation(
            "the confirmed draft attachment version is no longer current".to_owned(),
        ));
    }
    connection.execute(
        "INSERT INTO outbox_attachment_sets (
             account_id, outbox_id, draft_id, draft_local_version
         ) VALUES (?1, ?2, ?3, ?4)",
        params![
            account_id,
            outbox_id,
            draft_id,
            u64_to_i64(draft_local_version),
        ],
    )?;
    connection.execute(
        "INSERT INTO outbox_attachment_refs (account_id, outbox_id, position, blob_id)
         SELECT account_id, ?4, position, blob_id
         FROM draft_attachment_refs
         WHERE account_id = ?1 AND draft_id = ?2 AND draft_local_version = ?3
         ORDER BY position",
        params![
            account_id,
            draft_id,
            u64_to_i64(draft_local_version),
            outbox_id,
        ],
    )?;
    Ok(true)
}

fn validate_forward_context(context: &ForwardContext) -> Result<()> {
    if context.source_message_id.is_empty()
        || context.source_message_id.len() > 256
        || context.source_message_id.chars().any(char::is_control)
    {
        return Err(MailError::Validation(
            "the forward source message identifier is invalid".to_owned(),
        ));
    }
    let mut ids = HashSet::new();
    for attachment in &context.source_attachments {
        if attachment.id.is_empty()
            || attachment.id.len() > 256
            || attachment.id.chars().any(char::is_control)
            || !ids.insert(attachment.id.as_str())
            || attachment.safe_display_name.is_empty()
            || attachment.safe_display_name.len() > 720
            || attachment.mime_type.is_empty()
            || attachment.mime_type.len() > 255
            || attachment.size_bytes > i64::MAX as u64
        {
            return Err(MailError::Validation(
                "the immutable forward attachment inventory is invalid".to_owned(),
            ));
        }
    }
    Ok(())
}

fn insert_forward_context_rows(
    connection: &Connection,
    account_id: &str,
    draft_id: &str,
    context: &ForwardContext,
) -> Result<()> {
    connection.execute(
        "INSERT INTO draft_forward_contexts (
             account_id, draft_id, source_message_id, original_subject, from_json,
             to_json, cc_json, sent_at, quoted_text, quoted_html, quoted_render_mode
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            account_id,
            draft_id,
            context.source_message_id,
            context.original_subject,
            context.from.as_ref().map(encode_json).transpose()?,
            encode_json(&context.to)?,
            encode_json(&context.cc)?,
            context.sent_at,
            context.quoted_text,
            context.quoted_html,
            context
                .quoted_render_mode
                .map(forward_quoted_render_mode_as_str),
        ],
    )?;
    for (position, attachment) in context.source_attachments.iter().enumerate() {
        connection.execute(
            "INSERT INTO draft_forward_source_attachments (
                 account_id, draft_id, position, attachment_id, original_name,
                 safe_display_name, mime_type, size_bytes, disposition
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                account_id,
                draft_id,
                usize_to_i64(position),
                attachment.id,
                attachment.original_name,
                attachment.safe_display_name,
                attachment.mime_type,
                u64_to_i64(attachment.size_bytes),
                attachment_disposition_as_str(attachment.disposition),
            ],
        )?;
    }
    Ok(())
}

fn query_forward_context(
    connection: &Connection,
    account_id: &str,
    draft_id: &str,
) -> Result<Option<ForwardContext>> {
    let context: Option<(
        String,
        String,
        Option<String>,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
    )> = connection
        .query_row(
            "SELECT source_message_id, original_subject, from_json, to_json, cc_json,
                    sent_at, quoted_text, quoted_html, quoted_render_mode
             FROM draft_forward_contexts
             WHERE account_id = ?1 AND draft_id = ?2",
            params![account_id, draft_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .optional()?;
    let Some((
        source_message_id,
        original_subject,
        from_json,
        to_json,
        cc_json,
        sent_at,
        quoted_text,
        quoted_html,
        quoted_render_mode,
    )) = context
    else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT attachment_id, original_name, safe_display_name, mime_type,
                size_bytes, disposition
         FROM draft_forward_source_attachments
         WHERE account_id = ?1 AND draft_id = ?2
         ORDER BY position",
    )?;
    let source_attachments = statement
        .query_map(params![account_id, draft_id], |row| {
            let disposition = row.get::<_, String>(5)?;
            Ok(AttachmentMeta {
                id: row.get(0)?,
                original_name: row.get(1)?,
                safe_display_name: row.get(2)?,
                mime_type: row.get(3)?,
                size_bytes: decode_u64(4, row.get(4)?)?,
                size_is_estimate: false,
                disposition: attachment_disposition_from_column(5, disposition)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(Some(ForwardContext {
        source_message_id,
        original_subject,
        from: from_json
            .as_deref()
            .map(|value| decode_json(2, value))
            .transpose()?,
        to: decode_json(3, &to_json)?,
        cc: decode_json(4, &cc_json)?,
        sent_at,
        quoted_text,
        quoted_html,
        quoted_render_mode: quoted_render_mode
            .map(|value| forward_quoted_render_mode_from_column(8, value))
            .transpose()?,
        source_attachments,
    }))
}

fn clone_forward_context_rows(
    connection: &Connection,
    account_id: &str,
    source_draft_id: &str,
    target_draft_id: &str,
) -> Result<()> {
    if let Some(context) = query_forward_context(connection, account_id, source_draft_id)? {
        if let Some(existing) = query_forward_context(connection, account_id, target_draft_id)? {
            if existing == context {
                return Ok(());
            }
            return Err(MailError::Validation(
                "the draft conflict forward context is already different".to_owned(),
            ));
        }
        insert_forward_context_rows(connection, account_id, target_draft_id, &context)?;
    }
    Ok(())
}

fn attachment_disposition_as_str(disposition: AttachmentDisposition) -> &'static str {
    match disposition {
        AttachmentDisposition::Attachment => "attachment",
        AttachmentDisposition::Inline => "inline",
    }
}

fn attachment_disposition_from_column(
    index: usize,
    value: String,
) -> rusqlite::Result<AttachmentDisposition> {
    match value.as_str() {
        "attachment" => Ok(AttachmentDisposition::Attachment),
        "inline" => Ok(AttachmentDisposition::Inline),
        _ => Err(invalid_enum_column(index, &value)),
    }
}

fn forward_quoted_render_mode_as_str(mode: ForwardQuotedRenderMode) -> &'static str {
    match mode {
        ForwardQuotedRenderMode::Plain => "plain",
        ForwardQuotedRenderMode::NativeHtml => "native_html",
        ForwardQuotedRenderMode::IsolatedHtml => "isolated_html",
    }
}

fn forward_quoted_render_mode_from_column(
    index: usize,
    value: String,
) -> rusqlite::Result<ForwardQuotedRenderMode> {
    match value.as_str() {
        "plain" => Ok(ForwardQuotedRenderMode::Plain),
        "native_html" => Ok(ForwardQuotedRenderMode::NativeHtml),
        "isolated_html" => Ok(ForwardQuotedRenderMode::IsolatedHtml),
        _ => Err(invalid_enum_column(index, &value)),
    }
}

fn validate_outbox_draft_link(item: &OutboxItem) -> Result<()> {
    let linked = item.draft_id.is_some();
    if linked != item.draft_revision.is_some() || linked != item.draft_local_version.is_some() {
        return Err(MailError::Validation(
            "an Outbox draft link requires id, protocol revision and local version".to_owned(),
        ));
    }
    if item.draft_revision == Some(0) || item.draft_local_version == Some(0) {
        return Err(MailError::Validation(
            "an Outbox draft revision must be greater than zero".to_owned(),
        ));
    }
    Ok(())
}

fn validate_new_outbox_recipient_groups(item: &OutboxItem) -> Result<()> {
    let groups = item.recipient_groups.as_ref().ok_or_else(|| {
        MailError::Validation(
            "a new Outbox item requires exact To, Cc and Bcc recipient grouping".to_owned(),
        )
    })?;
    let request = ComposeRequest {
        to: groups.to.clone(),
        cc: groups.cc.clone(),
        bcc: groups.bcc.clone(),
        subject: String::new(),
        body_text: String::new(),
        format: ComposeFormat::default(),
        reply_context: None,
    };
    request.validate()?;
    let (_, normalized_recipients) =
        build_envelope("outbox-validation@mine-mail.invalid", &request)?;
    if normalized_recipients != item.recipients {
        return Err(MailError::Validation(
            "the grouped Outbox recipients do not match its immutable SMTP envelope".to_owned(),
        ));
    }
    Ok(())
}

/// Revalidates draft-version send safety while holding an IMMEDIATE write
/// transaction. This closes the race between the earlier UI/core check and a
/// concurrent manual retry in another process.
fn prepare_new_draft_send(
    transaction: &rusqlite::Transaction<'_>,
    item: &OutboxItem,
) -> Result<()> {
    let (Some(draft_id), Some(draft_local_version)) =
        (item.draft_id.as_deref(), item.draft_local_version)
    else {
        return Ok(());
    };
    let snapshot =
        query_draft_version_snapshot(transaction, &item.account_id, draft_id, draft_local_version)?
            .ok_or_else(|| {
                MailError::Validation(
                    "the confirmed draft version has no immutable recipient snapshot".to_owned(),
                )
            })?;
    if Some(snapshot.protocol_revision) != item.draft_revision {
        return Err(MailError::Validation(
            "the Outbox protocol revision does not match its draft snapshot".to_owned(),
        ));
    }
    let expected_groups = OutboxRecipientGroups::from(&snapshot.request);
    if item.recipient_groups.as_ref() != Some(&expected_groups) {
        return Err(MailError::Validation(
            "the grouped Outbox recipients do not match its confirmed draft version".to_owned(),
        ));
    }

    let sql = format!(
        "SELECT {OUTBOX_COLUMNS} FROM outbox
         WHERE draft_id = ?1
           AND (
               draft_local_version = ?2
               OR status IN ('queued', 'sending', 'delivery_unknown')
           )
         ORDER BY
             CASE WHEN status = 'delivery_unknown' THEN 0 ELSE 1 END,
             created_at ASC, id ASC
         LIMIT 1"
    );
    if let Some(existing) = transaction
        .query_row(
            &sql,
            params![draft_id, u64_to_i64(draft_local_version)],
            row_to_outbox,
        )
        .optional()?
    {
        let detail = if existing.status == OutboxStatus::DeliveryUnknown {
            "delivery of an earlier draft version is unknown; resolve it before sending a new version"
        } else {
            "this exact draft version or another active attempt already has an Outbox item"
        };
        return Err(MailError::Validation(format!(
            "{detail} with status '{}'; it will not be sent again",
            existing.status.as_str(),
        )));
    }

    transaction.execute(
        "UPDATE outbox SET
             status = 'rejected',
             last_error = 'superseded by a newer confirmed draft version before delivery',
             draft_id = NULL,
             draft_revision = NULL,
             draft_local_version = NULL
         WHERE draft_id = ?1
           AND status = 'retryable'
           AND (draft_local_version IS NULL OR draft_local_version <> ?2)",
        params![draft_id, u64_to_i64(draft_local_version)],
    )?;
    Ok(())
}

fn insert_draft_record_if_absent(connection: &Connection, record: &DraftRecord) -> Result<usize> {
    let draft = &record.draft;
    let inserted = connection.execute(
        "INSERT INTO drafts (
                 id, account_id, to_json, cc_json, bcc_json, subject, body_text,
                 compose_format_json, reply_context_json, status, remote_mailbox, remote_uid,
                 created_at, updated_at, raw_rfc822,
                 local_version, has_unsupported_content, revision, synced_revision,
                 remote_uid_validity, is_deleted
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21
             )
             ON CONFLICT(id) DO NOTHING",
        params![
            draft.id,
            draft.account_id,
            encode_json(&draft.to)?,
            encode_json(&draft.cc)?,
            encode_json(&draft.bcc)?,
            draft.subject,
            draft.body_text,
            encode_json(&draft.format)?,
            draft.reply_context.as_ref().map(encode_json).transpose()?,
            draft.status,
            draft.remote_mailbox,
            draft.remote_uid,
            draft.created_at,
            draft.updated_at,
            draft.raw_rfc822,
            u64_to_i64(record.local_version),
            draft.has_unsupported_content,
            u64_to_i64(record.revision),
            u64_to_i64(record.synced_revision),
            record.remote_uid_validity,
            record.is_deleted,
        ],
    )?;
    if inserted == 1 {
        insert_draft_version_snapshot(connection, record)?;
    }
    Ok(inserted)
}

fn insert_draft_version_snapshot(connection: &Connection, record: &DraftRecord) -> Result<()> {
    if record.local_version == 0
        || record.revision == 0
        || record.local_version != record.draft.local_version
    {
        return Err(MailError::Validation(
            "the immutable draft version snapshot is invalid".to_owned(),
        ));
    }
    let changed = connection.execute(
        "INSERT INTO draft_version_snapshots (
             account_id, draft_id, draft_local_version, protocol_revision,
             to_json, cc_json, bcc_json, subject, body_text,
             compose_format_json, reply_context_json, has_unsupported_content
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            record.draft.account_id,
            record.draft.id,
            u64_to_i64(record.local_version),
            u64_to_i64(record.revision),
            encode_json(&record.draft.to)?,
            encode_json(&record.draft.cc)?,
            encode_json(&record.draft.bcc)?,
            record.draft.subject,
            record.draft.body_text,
            encode_json(&record.draft.format)?,
            record
                .draft
                .reply_context
                .as_ref()
                .map(encode_json)
                .transpose()?,
            record.draft.has_unsupported_content,
        ],
    )?;
    if changed != 1 {
        return Err(MailError::Database(rusqlite::Error::ExecuteReturnedResults));
    }
    Ok(())
}

fn insert_current_draft_version_snapshot(
    connection: &Connection,
    account_id: &str,
    draft_id: &str,
) -> Result<()> {
    let changed = connection.execute(
        "INSERT INTO draft_version_snapshots (
             account_id, draft_id, draft_local_version, protocol_revision,
             to_json, cc_json, bcc_json, subject, body_text,
             compose_format_json, reply_context_json, has_unsupported_content
         )
         SELECT account_id, id, local_version, revision,
                to_json, cc_json, bcc_json, subject, body_text,
                compose_format_json, reply_context_json, has_unsupported_content
         FROM drafts
         WHERE account_id = ?1 AND id = ?2",
        params![account_id, draft_id],
    )?;
    if changed != 1 {
        return Err(MailError::Validation(
            "the current draft version could not be snapshotted".to_owned(),
        ));
    }
    Ok(())
}

fn query_draft_version_snapshot(
    connection: &Connection,
    account_id: &str,
    draft_id: &str,
    local_version: u64,
) -> Result<Option<DraftVersionSnapshot>> {
    let sql = format!(
        "SELECT {DRAFT_VERSION_SNAPSHOT_COLUMNS}
         FROM draft_version_snapshots
         WHERE account_id = ?1 AND draft_id = ?2 AND draft_local_version = ?3"
    );
    connection
        .query_row(
            &sql,
            params![account_id, draft_id, u64_to_i64(local_version)],
            row_to_draft_version_snapshot,
        )
        .optional()
        .map_err(Into::into)
}

fn clone_draft_version_forward_context_ref(
    connection: &Connection,
    account_id: &str,
    source_draft_id: &str,
    source_local_version: u64,
    target_draft_id: &str,
    target_local_version: u64,
) -> Result<()> {
    connection.execute(
        "INSERT OR IGNORE INTO draft_version_forward_context_refs (
             account_id, draft_id, draft_local_version
         )
         SELECT account_id, ?4, ?5
         FROM draft_version_forward_context_refs
         WHERE account_id = ?1 AND draft_id = ?2 AND draft_local_version = ?3",
        params![
            account_id,
            source_draft_id,
            u64_to_i64(source_local_version),
            target_draft_id,
            u64_to_i64(target_local_version),
        ],
    )?;
    Ok(())
}

fn attach_forward_context_to_all_versions(
    connection: &Connection,
    account_id: &str,
    draft_id: &str,
) -> Result<()> {
    connection.execute(
        "INSERT OR IGNORE INTO draft_version_forward_context_refs (
             account_id, draft_id, draft_local_version
         )
         SELECT s.account_id, s.draft_id, s.draft_local_version
         FROM draft_version_snapshots s
         JOIN draft_forward_contexts c
           ON c.account_id = s.account_id AND c.draft_id = s.draft_id
         WHERE s.account_id = ?1 AND s.draft_id = ?2",
        params![account_id, draft_id],
    )?;
    Ok(())
}

fn attach_forward_context_to_current_version(
    connection: &Connection,
    account_id: &str,
    draft_id: &str,
) -> Result<()> {
    connection.execute(
        "INSERT OR IGNORE INTO draft_version_forward_context_refs (
             account_id, draft_id, draft_local_version
         )
         SELECT s.account_id, s.draft_id, s.draft_local_version
         FROM drafts d
         JOIN draft_version_snapshots s
           ON s.account_id = d.account_id
          AND s.draft_id = d.id
          AND s.draft_local_version = d.local_version
         JOIN draft_forward_contexts c
           ON c.account_id = d.account_id AND c.draft_id = d.id
         WHERE d.account_id = ?1 AND d.id = ?2",
        params![account_id, draft_id],
    )?;
    Ok(())
}

fn encode_json<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(Into::into)
}

fn decode_json<T: DeserializeOwned>(column: usize, value: &str) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
    })
}

fn decode_optional_u64(column: usize, value: Option<String>) -> rusqlite::Result<Option<u64>> {
    value
        .map(|value| {
            value.parse::<u64>().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
            })
        })
        .transpose()
}

fn decode_u64(column: usize, value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(column, Type::Integer, Box::new(error))
    })
}

fn row_to_message(row: &Row<'_>) -> rusqlite::Result<InboxMessage> {
    let sender_json: Option<String> = row.get(8)?;
    Ok(InboxMessage {
        id: row.get(0)?,
        account_id: row.get(1)?,
        mailbox: row.get(2)?,
        uid: row.get(3)?,
        message_id: row.get(4)?,
        in_reply_to: decode_json(5, &row.get::<_, String>(5)?)?,
        references: decode_json(6, &row.get::<_, String>(6)?)?,
        subject: row.get(7)?,
        sender: sender_json
            .as_deref()
            .map(|json| decode_json(8, json))
            .transpose()?,
        to: decode_json(9, &row.get::<_, String>(9)?)?,
        cc: decode_json(10, &row.get::<_, String>(10)?)?,
        bcc: decode_json(22, &row.get::<_, String>(22)?)?,
        sent_at: row.get(11)?,
        internal_date: row.get(12)?,
        flags: decode_json(13, &row.get::<_, String>(13)?)?,
        size_bytes: row.get(14)?,
        preview: row.get(15)?,
        body_text: row.get(16)?,
        body_html: row.get(17)?,
        attachment_names: decode_json(18, &row.get::<_, String>(18)?)?,
        body_fetched: row.get(19)?,
        raw_rfc822: row.get(20)?,
        synced_at: row.get(21)?,
    })
}

fn row_to_draft(row: &Row<'_>) -> rusqlite::Result<Draft> {
    let compose_format_json: String = row.get(7)?;
    let reply_context_json: Option<String> = row.get(8)?;
    Ok(Draft {
        id: row.get(0)?,
        local_version: decode_u64(15, row.get(15)?)?,
        has_unsupported_content: row.get(16)?,
        account_id: row.get(1)?,
        to: decode_json(2, &row.get::<_, String>(2)?)?,
        cc: decode_json(3, &row.get::<_, String>(3)?)?,
        bcc: decode_json(4, &row.get::<_, String>(4)?)?,
        subject: row.get(5)?,
        body_text: row.get(6)?,
        format: if compose_format_json.trim().is_empty() {
            ComposeFormat::default()
        } else {
            decode_json(7, &compose_format_json)?
        },
        reply_context: reply_context_json
            .as_deref()
            .map(|json| decode_json(8, json))
            .transpose()?,
        status: row.get(9)?,
        remote_mailbox: row.get(10)?,
        remote_uid: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        raw_rfc822: row.get(14)?,
    })
}

fn row_to_draft_version_snapshot(row: &Row<'_>) -> rusqlite::Result<DraftVersionSnapshot> {
    let compose_format_json: String = row.get(9)?;
    let reply_context_json: Option<String> = row.get(10)?;
    Ok(DraftVersionSnapshot {
        account_id: row.get(0)?,
        draft_id: row.get(1)?,
        local_version: decode_u64(2, row.get(2)?)?,
        protocol_revision: decode_u64(3, row.get(3)?)?,
        request: ComposeRequest {
            to: decode_json(4, &row.get::<_, String>(4)?)?,
            cc: decode_json(5, &row.get::<_, String>(5)?)?,
            bcc: decode_json(6, &row.get::<_, String>(6)?)?,
            subject: row.get(7)?,
            body_text: row.get(8)?,
            format: if compose_format_json.trim().is_empty() {
                ComposeFormat::default()
            } else {
                decode_json(9, &compose_format_json)?
            },
            reply_context: reply_context_json
                .as_deref()
                .map(|json| decode_json(10, json))
                .transpose()?,
        },
        has_unsupported_content: row.get(11)?,
    })
}

fn row_to_draft_record(row: &Row<'_>) -> rusqlite::Result<DraftRecord> {
    let draft = row_to_draft(row)?;
    let local_version = draft.local_version;
    Ok(DraftRecord {
        draft,
        local_version,
        revision: decode_u64(17, row.get(17)?)?,
        synced_revision: decode_u64(18, row.get(18)?)?,
        remote_uid_validity: row.get(19)?,
        is_deleted: row.get(20)?,
    })
}

fn row_to_outbox(row: &Row<'_>) -> rusqlite::Result<OutboxItem> {
    let status_text: String = row.get(6)?;
    let status = OutboxStatus::from_str(&status_text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(6, Type::Text, Box::new(error))
    })?;
    let draft_revision = row
        .get::<_, Option<i64>>(3)?
        .map(|value| decode_u64(3, value))
        .transpose()?;
    let draft_local_version = row
        .get::<_, Option<i64>>(4)?
        .map(|value| decode_u64(4, value))
        .transpose()?;
    Ok(OutboxItem {
        id: row.get(0)?,
        account_id: row.get(1)?,
        draft_id: row.get(2)?,
        draft_revision,
        draft_local_version,
        recipients: decode_json(5, &row.get::<_, String>(5)?)?,
        recipient_groups: row
            .get::<_, Option<String>>(12)?
            .as_deref()
            .map(|json| decode_json(12, json))
            .transpose()?,
        status,
        attempts: row.get(7)?,
        last_error: row.get(8)?,
        created_at: row.get(9)?,
        sent_at: row.get(10)?,
        raw_rfc822: row.get(11)?,
    })
}

fn ensure_changed(changed: usize, entity: &'static str, id: String) -> Result<()> {
    if changed == 0 {
        Err(MailError::NotFound { entity, id })
    } else {
        Ok(())
    }
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        fs,
        path::Path,
        sync::{Arc, Barrier},
        thread,
    };

    use rusqlite::{Connection, params};
    use tempfile::TempDir;

    use super::{
        DraftRecord, MailboxHistory, MailboxState, NewDraftAttachment, Repository,
        StarredMailboxHistory, migrate_message_previews_v10,
    };
    use crate::{
        AccountConfig, AttachmentDisposition, AttachmentMeta, ComposeFormat, Draft, ForwardContext,
        ForwardQuotedRenderMode, InboxMessage, MailAddress, MailError, OutboxItem,
        OutboxRecipientGroups, OutboxStatus, StationeryTheme,
        managed_attachments::ImportedManagedAttachment,
        models::{
            MailboxCapability, MailboxCapabilityStatus, MailboxCapabilityUnavailableReason,
            MailboxRole, MessageActionKind, MessageMutationErrorKind, MessagePageCursor,
            MutationStatus, RemoteHistoryState, RemoteMutationPhase, SystemFlagKind,
        },
    };

    fn setup() -> (TempDir, Repository, AccountConfig) {
        let directory = TempDir::new().expect("temporary directory");
        let repository =
            Repository::open(directory.path().join("mail.sqlite3")).expect("repository");
        let account = AccountConfig::from_163_lines([
            "database-test@163.com",
            "super-secret-authorization-value",
        ])
        .expect("account");
        repository
            .initialize_account(&account)
            .expect("account row");
        (directory, repository, account)
    }

    fn create_legacy_core_fixture(path: &Path) -> Connection {
        let connection = Connection::open(path).expect("legacy fixture database");
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE accounts (
                     id TEXT PRIMARY KEY NOT NULL,
                     email TEXT NOT NULL,
                     created_at TEXT NOT NULL,
                     updated_at TEXT NOT NULL
                 );
                 CREATE TABLE mailboxes (
                     account_id TEXT NOT NULL,
                     name TEXT NOT NULL,
                     uid_validity INTEGER,
                     uid_next INTEGER,
                     highest_uid INTEGER,
                     highest_modseq TEXT,
                     last_synced_at TEXT,
                     PRIMARY KEY (account_id, name),
                     FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
                 );
                 CREATE TABLE messages (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     account_id TEXT NOT NULL,
                     mailbox TEXT NOT NULL,
                     uid INTEGER NOT NULL,
                     message_id TEXT,
                     in_reply_to_json TEXT NOT NULL DEFAULT '[]',
                     references_json TEXT NOT NULL DEFAULT '[]',
                     subject TEXT NOT NULL DEFAULT '',
                     sender_json TEXT,
                     to_json TEXT NOT NULL DEFAULT '[]',
                     cc_json TEXT NOT NULL DEFAULT '[]',
                     sent_at TEXT,
                     internal_date TEXT,
                     flags_json TEXT NOT NULL DEFAULT '[]',
                     size_bytes INTEGER NOT NULL DEFAULT 0,
                     preview TEXT NOT NULL DEFAULT '',
                     preview_fetched INTEGER NOT NULL DEFAULT 0,
                     body_text TEXT,
                     body_html TEXT,
                     attachment_names_json TEXT NOT NULL DEFAULT '[]',
                     body_fetched INTEGER NOT NULL DEFAULT 0,
                     raw_rfc822 BLOB NOT NULL DEFAULT X'',
                     synced_at TEXT NOT NULL,
                     UNIQUE (account_id, mailbox, uid),
                     FOREIGN KEY (account_id, mailbox)
                         REFERENCES mailboxes(account_id, name) ON DELETE CASCADE
                 );
                 CREATE TABLE drafts (
                     id TEXT PRIMARY KEY NOT NULL,
                     account_id TEXT NOT NULL,
                     to_json TEXT NOT NULL DEFAULT '[]',
                     cc_json TEXT NOT NULL DEFAULT '[]',
                     bcc_json TEXT NOT NULL DEFAULT '[]',
                     subject TEXT NOT NULL DEFAULT '',
                     body_text TEXT NOT NULL DEFAULT '',
                     reply_context_json TEXT,
                     status TEXT NOT NULL,
                     remote_mailbox TEXT,
                     remote_uid INTEGER,
                     created_at TEXT NOT NULL,
                     updated_at TEXT NOT NULL,
                     raw_rfc822 BLOB NOT NULL DEFAULT X'',
                     local_version INTEGER NOT NULL DEFAULT 1,
                     has_unsupported_content INTEGER NOT NULL DEFAULT 0,
                     revision INTEGER NOT NULL DEFAULT 1,
                     synced_revision INTEGER NOT NULL DEFAULT 0,
                     remote_uid_validity INTEGER,
                     is_deleted INTEGER NOT NULL DEFAULT 0,
                     FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
                 );
                 INSERT INTO accounts (
                     id, email, created_at, updated_at
                 ) VALUES (
                     'fixture', 'fixture@example.invalid',
                     '2026-07-01T00:00:00Z', '2026-07-01T00:00:00Z'
                 );
                 INSERT INTO mailboxes (
                     account_id, name, uid_validity, uid_next, highest_uid,
                     highest_modseq, last_synced_at
                 ) VALUES (
                     'fixture', 'INBOX', 88, 43, 42, NULL, '2026-07-01T00:00:00Z'
                 );
                 INSERT INTO messages (
                     account_id, mailbox, uid, message_id, in_reply_to_json,
                     references_json, subject, sender_json, to_json, cc_json,
                     sent_at, internal_date, flags_json, size_bytes, preview,
                     preview_fetched, body_text, body_html, attachment_names_json,
                     body_fetched, raw_rfc822, synced_at
                 ) VALUES (
                     'fixture', 'INBOX', 42, 'fixture-42@example.invalid', '[]',
                     '[]', 'Fixture', NULL, '[]', '[]', NULL,
                     '2026-07-01T00:00:00Z', '[\"\\\\Seen\"]', 10, 'fixture',
                     1, NULL, NULL, '[]', 0, X'', '2026-07-01T00:00:00Z'
                 );",
            )
            .expect("legacy core schema");
        connection
    }

    fn create_real_pre_v12_fixture(path: &Path, version: u32, compose_json: Option<&str>) {
        let connection = create_legacy_core_fixture(path);
        if compose_json.is_some() {
            connection
                .execute_batch(
                    "ALTER TABLE drafts
                         ADD COLUMN compose_format_json TEXT NOT NULL DEFAULT '{}';",
                )
                .expect("v11 compose column");
        }
        connection
            .execute_batch(
                "CREATE TABLE pending_seen_updates (
                     account_id TEXT NOT NULL,
                     mailbox TEXT NOT NULL,
                     uid INTEGER NOT NULL,
                     created_at TEXT NOT NULL,
                     PRIMARY KEY (account_id, mailbox, uid),
                     FOREIGN KEY (account_id, mailbox, uid)
                         REFERENCES messages(account_id, mailbox, uid) ON DELETE CASCADE
                 );
                 CREATE TABLE pending_flagged_updates (
                     account_id TEXT NOT NULL,
                     mailbox TEXT NOT NULL,
                     uid INTEGER NOT NULL,
                     desired INTEGER NOT NULL,
                     revision INTEGER NOT NULL,
                     updated_at TEXT NOT NULL,
                     PRIMARY KEY (account_id, mailbox, uid),
                     FOREIGN KEY (account_id, mailbox, uid)
                         REFERENCES messages(account_id, mailbox, uid) ON DELETE CASCADE
                 );
                 INSERT INTO pending_seen_updates (
                     account_id, mailbox, uid, created_at
                 ) VALUES (
                     'fixture', 'INBOX', 42, '2026-07-01T00:01:00Z'
                 );
                 INSERT INTO pending_flagged_updates (
                     account_id, mailbox, uid, desired, revision, updated_at
                 ) VALUES (
                     'fixture', 'INBOX', 42, 0, 4, '2026-07-01T00:02:00Z'
                 );",
            )
            .expect("pre-v12 flag queues");
        if let Some(compose_json) = compose_json {
            connection
                .execute(
                    "INSERT INTO drafts (
                         id, account_id, subject, body_text, compose_format_json,
                         status, created_at, updated_at, local_version,
                         has_unsupported_content, revision, synced_revision, is_deleted
                     ) VALUES (
                         'fixture-draft', 'fixture', 'Fixture draft', 'legacy body', ?1,
                         'local', '2026-07-01T00:00:00Z', '2026-07-01T00:00:00Z',
                         3, 0, 3, 0, 0
                     )",
                    params![compose_json],
                )
                .expect("v11 draft");
        } else {
            connection
                .execute_batch(
                    "INSERT INTO drafts (
                         id, account_id, subject, body_text, status, created_at,
                         updated_at, local_version, has_unsupported_content,
                         revision, synced_revision, is_deleted
                     ) VALUES (
                         'fixture-draft', 'fixture', 'Fixture draft', 'legacy body',
                         'local', '2026-07-01T00:00:00Z', '2026-07-01T00:00:00Z',
                         3, 0, 3, 0, 0
                     );",
                )
                .expect("v10 draft");
        }
        connection
            .pragma_update(None, "user_version", version)
            .expect("legacy schema version");
    }

    fn create_real_intermediate_v12_fixture(path: &Path, compose_json: &str) {
        let connection = create_legacy_core_fixture(path);
        connection
            .execute_batch(
                "ALTER TABLE mailboxes ADD COLUMN history_before_uid INTEGER;
                 ALTER TABLE mailboxes
                     ADD COLUMN history_complete INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE mailboxes ADD COLUMN remote_total INTEGER;
                 ALTER TABLE drafts
                     ADD COLUMN compose_format_json TEXT NOT NULL DEFAULT '{}';
                 CREATE TABLE pending_seen_updates (
                     operation_id TEXT NOT NULL UNIQUE,
                     account_id TEXT NOT NULL,
                     mailbox TEXT NOT NULL,
                     source_uid_validity INTEGER NOT NULL,
                     uid INTEGER NOT NULL,
                     desired INTEGER NOT NULL,
                     revision INTEGER NOT NULL,
                     status TEXT NOT NULL,
                     error_kind TEXT,
                     updated_at TEXT NOT NULL,
                     PRIMARY KEY (account_id, mailbox, uid),
                     FOREIGN KEY (account_id, mailbox, uid)
                         REFERENCES messages(account_id, mailbox, uid) ON DELETE CASCADE
                 );
                 CREATE TABLE pending_flagged_updates (
                     operation_id TEXT NOT NULL UNIQUE,
                     account_id TEXT NOT NULL,
                     mailbox TEXT NOT NULL,
                     source_uid_validity INTEGER NOT NULL,
                     uid INTEGER NOT NULL,
                     desired INTEGER NOT NULL,
                     revision INTEGER NOT NULL,
                     status TEXT NOT NULL,
                     error_kind TEXT,
                     updated_at TEXT NOT NULL,
                     PRIMARY KEY (account_id, mailbox, uid),
                     FOREIGN KEY (account_id, mailbox, uid)
                         REFERENCES messages(account_id, mailbox, uid) ON DELETE CASCADE
                 );
                 CREATE TABLE pending_message_actions (
                     operation_id TEXT PRIMARY KEY NOT NULL,
                     account_id TEXT NOT NULL,
                     source_mailbox TEXT NOT NULL,
                     source_uid_validity INTEGER NOT NULL,
                     source_uid INTEGER NOT NULL,
                     source_role TEXT NOT NULL,
                     destination_role TEXT,
                     kind TEXT NOT NULL,
                     revision INTEGER NOT NULL,
                     status TEXT NOT NULL,
                     source_message_id TEXT,
                     source_internal_date TEXT,
                     source_size_bytes INTEGER NOT NULL,
                     error_kind TEXT,
                     created_at TEXT NOT NULL,
                     updated_at TEXT NOT NULL,
                     UNIQUE (account_id, source_mailbox, source_uid_validity, source_uid),
                     FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
                 );
                 INSERT INTO pending_seen_updates (
                     operation_id, account_id, mailbox, source_uid_validity, uid,
                     desired, revision, status, error_kind, updated_at
                 ) VALUES (
                     'seen-intermediate', 'fixture', 'INBOX', 88, 42,
                     1, 5, 'in_flight', NULL, '2026-07-01T00:03:00Z'
                 );
                 INSERT INTO pending_flagged_updates (
                     operation_id, account_id, mailbox, source_uid_validity, uid,
                     desired, revision, status, error_kind, updated_at
                 ) VALUES (
                     'flagged-intermediate', 'fixture', 'INBOX', 88, 42,
                     0, 6, 'pending', NULL, '2026-07-01T00:04:00Z'
                 );
                 INSERT INTO pending_message_actions (
                     operation_id, account_id, source_mailbox, source_uid_validity,
                     source_uid, source_role, destination_role, kind, revision, status,
                     source_message_id, source_internal_date, source_size_bytes,
                     error_kind, created_at, updated_at
                 ) VALUES (
                     'action-intermediate', 'fixture', 'INBOX', 88, 42,
                     'inbox', 'archive', 'archive', 3, 'in_flight',
                     'fixture-42@example.invalid', '2026-07-01T00:00:00Z', 10,
                     NULL, '2026-07-01T00:05:00Z', '2026-07-01T00:05:00Z'
                 );",
            )
            .expect("intermediate v12 schema");
        connection
            .execute(
                "INSERT INTO drafts (
                     id, account_id, subject, body_text, compose_format_json,
                     status, created_at, updated_at, local_version,
                     has_unsupported_content, revision, synced_revision, is_deleted
                 ) VALUES (
                     'fixture-draft', 'fixture', 'Fixture draft', 'legacy body', ?1,
                     'local', '2026-07-01T00:00:00Z', '2026-07-01T00:00:00Z',
                     3, 0, 3, 0, 0
                 )",
                params![compose_json],
            )
            .expect("intermediate v12 draft");
        connection
            .pragma_update(None, "user_version", 12)
            .expect("intermediate schema version");
    }

    fn message(account_id: &str, body_fetched: bool) -> InboxMessage {
        InboxMessage {
            id: 0,
            account_id: account_id.to_owned(),
            mailbox: "INBOX".to_owned(),
            uid: 42,
            message_id: Some("message-42@example.com".to_owned()),
            in_reply_to: Vec::new(),
            references: Vec::new(),
            subject: "First subject".to_owned(),
            sender: Some(MailAddress {
                name: Some("Alice".to_owned()),
                email: "alice@example.com".to_owned(),
            }),
            to: vec![],
            cc: vec![],
            bcc: vec![],
            sent_at: Some("2026-07-14T01:00:00Z".to_owned()),
            internal_date: Some("2026-07-14T01:00:01Z".to_owned()),
            flags: vec!["\\Seen".to_owned()],
            size_bytes: 321,
            preview: "Preview".to_owned(),
            body_text: body_fetched.then(|| "Full body".to_owned()),
            body_html: None,
            attachment_names: vec![],
            body_fetched,
            raw_rfc822: if body_fetched {
                b"full raw message".to_vec()
            } else {
                Vec::new()
            },
            synced_at: "2026-07-14T01:00:02Z".to_owned(),
        }
    }

    fn initialize_mailbox(
        repository: &Repository,
        account_id: &str,
        role: MailboxRole,
        mailbox: &str,
        uid_validity: u32,
    ) {
        repository
            .assign_semantic_mailbox_role(account_id, role, mailbox)
            .expect("mailbox role");
        repository
            .upsert_mailbox_state(&MailboxState {
                account_id: account_id.to_owned(),
                mailbox: mailbox.to_owned(),
                uid_validity: Some(uid_validity),
                uid_next: None,
                highest_uid: None,
                highest_modseq: None,
                last_synced_at: None,
            })
            .expect("mailbox state");
    }

    fn message_with_identity(
        account_id: &str,
        mailbox: &str,
        uid: u32,
        internal_date: &str,
        subject: &str,
    ) -> InboxMessage {
        let mut mail = message(account_id, false);
        mail.mailbox = mailbox.to_owned();
        mail.uid = uid;
        mail.message_id = Some(format!("message-{uid}@example.com"));
        mail.internal_date = Some(internal_date.to_owned());
        mail.subject = subject.to_owned();
        mail
    }

    fn secondary_account(primary: &AccountConfig) -> AccountConfig {
        AccountConfig::new(
            "secondary",
            "secondary@163.com",
            "secondary-not-real-secret",
            primary.imap.clone(),
            primary.smtp.clone(),
            primary.smtp_security,
        )
        .expect("secondary account")
    }

    fn draft_record(
        account_id: &str,
        id: &str,
        subject: &str,
        revision: u64,
        synced_revision: u64,
    ) -> DraftRecord {
        DraftRecord {
            draft: Draft {
                id: id.to_owned(),
                local_version: revision,
                has_unsupported_content: false,
                account_id: account_id.to_owned(),
                to: vec!["receiver@example.com".to_owned()],
                cc: vec![],
                bcc: vec![],
                subject: subject.to_owned(),
                body_text: format!("body for {subject}"),
                format: Default::default(),
                reply_context: None,
                status: if revision == synced_revision {
                    "synced".to_owned()
                } else {
                    "local".to_owned()
                },
                remote_mailbox: (synced_revision > 0).then(|| "Drafts".to_owned()),
                remote_uid: (synced_revision > 0).then_some(17),
                created_at: "2026-07-14T00:00:00Z".to_owned(),
                updated_at: format!("2026-07-14T00:00:0{revision}Z"),
                raw_rfc822: format!("raw revision {revision}").into_bytes(),
            },
            local_version: revision,
            revision,
            synced_revision,
            remote_uid_validity: (synced_revision > 0).then_some(91),
            is_deleted: false,
        }
    }

    fn linked_outbox(
        draft: &DraftRecord,
        id: &str,
        status: OutboxStatus,
        attempts: u32,
    ) -> OutboxItem {
        OutboxItem {
            id: id.to_owned(),
            account_id: draft.draft.account_id.clone(),
            draft_id: Some(draft.draft.id.clone()),
            draft_revision: Some(draft.revision),
            draft_local_version: Some(draft.local_version),
            recipients: draft.draft.to.clone(),
            recipient_groups: Some(OutboxRecipientGroups::from(&draft.draft.compose_request())),
            status,
            attempts,
            last_error: None,
            created_at: format!("2026-07-14T06:00:0{attempts}Z"),
            sent_at: None,
            raw_rfc822: format!("exact bytes for {id}").into_bytes(),
        }
    }

    fn new_attachment(name: &str, source_attachment_id: Option<&str>) -> NewDraftAttachment {
        let id = uuid::Uuid::now_v7().to_string();
        NewDraftAttachment {
            imported: ImportedManagedAttachment {
                internal_name: format!("{id}.blob"),
                id,
                name: name.to_owned(),
                mime_type: if name.ends_with(".txt") {
                    "text/plain".to_owned()
                } else {
                    "application/octet-stream".to_owned()
                },
                size_bytes: name.len() as u64,
                sha256_hex: "00".repeat(32),
            },
            source_attachment_id: source_attachment_id.map(str::to_owned),
        }
    }

    fn clear_attachment_digest_as_legacy_fixture(
        repository: &Repository,
        account_id: &str,
        blob_id: &str,
    ) {
        let connection = repository.connection().expect("legacy fixture connection");
        connection
            .execute_batch(
                "DROP TRIGGER IF EXISTS trg_managed_attachment_blobs_immutable;
                 DROP TRIGGER IF EXISTS trg_managed_attachment_digest_once;",
            )
            .expect("disable current digest guards for legacy fixture");
        assert_eq!(
            connection
                .execute(
                    "UPDATE managed_attachment_blobs
                     SET sha256_hex = NULL
                     WHERE account_id = ?1 AND id = ?2",
                    params![account_id, blob_id],
                )
                .expect("clear legacy digest"),
            1
        );
        super::migrate_managed_attachment_digests_v17(&connection)
            .expect("restore current digest guards");
    }

    #[test]
    fn message_upsert_is_idempotent_and_keeps_fetched_body() {
        let (_directory, repository, account) = setup();
        let full = message(&account.account_id, true);
        let first_id = repository.upsert_message(&full).expect("first upsert");

        let mut summary = message(&account.account_id, false);
        summary.subject = "Updated subject".to_owned();
        let second_id = repository.upsert_message(&summary).expect("second upsert");

        assert_eq!(first_id, second_id);
        assert_eq!(
            repository
                .count_messages(&account.account_id, "INBOX")
                .unwrap(),
            1
        );
        let stored = repository.get_message(first_id).expect("stored message");
        assert_eq!(stored.subject, "Updated subject");
        assert_eq!(stored.body_text.as_deref(), Some("Full body"));
        assert!(stored.body_fetched);

        repository
            .update_message_flags(&account.account_id, "INBOX", 42, &["\\Flagged".to_owned()])
            .expect("flags");
        assert_eq!(
            repository.get_message(first_id).unwrap().flags,
            ["\\Flagged"]
        );
    }

    #[test]
    fn selective_body_cache_can_upgrade_to_a_complete_mime_snapshot() {
        let (_directory, repository, account) = setup();
        let mut selective = message(&account.account_id, true);
        selective.body_text = Some("Selected text part".to_owned());
        selective.raw_rfc822.clear();
        let message_id = repository
            .upsert_message(&selective)
            .expect("selective body");

        let stored = repository
            .get_message(message_id)
            .expect("stored selective body");
        assert!(stored.body_fetched);
        assert_eq!(stored.body_text.as_deref(), Some("Selected text part"));
        assert!(stored.raw_rfc822.is_empty());

        let complete = message(&account.account_id, true);
        repository.upsert_message(&complete).expect("complete MIME");
        let upgraded = repository
            .get_message(message_id)
            .expect("upgraded complete MIME");
        assert_eq!(upgraded.body_text.as_deref(), Some("Full body"));
        assert_eq!(upgraded.raw_rfc822, b"full raw message");
    }

    #[test]
    fn message_public_id_survives_upsert_and_repository_reopen() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("stable-public-id.sqlite3");
        let account = AccountConfig::from_163_lines([
            "stable-public-id@163.com",
            "not-a-real-authorization-value",
        ])
        .expect("account");
        let repository = Repository::open(&path).expect("repository");
        repository
            .initialize_account(&account)
            .expect("account row");
        repository
            .upsert_message(&message(&account.account_id, false))
            .expect("message");
        let first_public_id = repository
            .list_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 50, None)
            .expect("first page")
            .items[0]
            .public_id
            .clone();
        let parsed_public_id =
            uuid::Uuid::parse_str(&first_public_id).expect("random UUID public id");
        assert_eq!(parsed_public_id.get_version(), Some(uuid::Version::Random));
        let first_contact = repository
            .list_contact_source_messages(&account.account_id)
            .expect("contact summary")
            .remove(0);
        assert_eq!(first_contact.public_id, first_public_id);
        assert!(first_contact.message.body_text.is_none());
        assert!(first_contact.message.body_html.is_none());
        assert!(first_contact.message.raw_rfc822.is_empty());

        let mut refreshed = message(&account.account_id, false);
        refreshed.subject = "Refreshed without replacing identity".to_owned();
        repository
            .upsert_message(&refreshed)
            .expect("ordinary conflict upsert");
        let after_upsert = repository
            .list_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 50, None)
            .expect("page after upsert")
            .items[0]
            .public_id
            .clone();
        assert_eq!(after_upsert, first_public_id);
        drop(repository);

        let reopened = Repository::open(&path).expect("reopened repository");
        let after_reopen = reopened
            .list_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 50, None)
            .expect("page after reopen")
            .items[0]
            .public_id
            .clone();
        assert_eq!(after_reopen, first_public_id);
        assert_eq!(
            reopened
                .list_contact_source_messages(&account.account_id)
                .expect("contact summary after reopen")[0]
                .public_id,
            first_public_id
        );
        assert_eq!(
            reopened
                .get_message_by_public_id(&account.account_id, &first_public_id)
                .expect("account-bound public lookup")
                .subject,
            "Refreshed without replacing identity"
        );
    }

    #[test]
    fn contact_email_index_tracks_message_participants_updates_and_limits() {
        let (_directory, repository, account) = setup();
        let mut first = message(&account.account_id, false);
        first.uid = 1;
        first.message_id = Some("contact-index-first@example.com".to_owned());
        first.bcc = vec![MailAddress {
            name: None,
            email: "blind@example.com".to_owned(),
        }];
        repository.upsert_message(&first).expect("first message");

        let mut second = message(&account.account_id, false);
        second.uid = 2;
        second.message_id = Some("contact-index-second@example.com".to_owned());
        second.sender = Some(MailAddress {
            name: Some("Bob".to_owned()),
            email: "bob@example.com".to_owned(),
        });
        second.to = vec![MailAddress {
            name: Some("Alice".to_owned()),
            email: "ALICE@example.com".to_owned(),
        }];
        second.internal_date = Some("2026-07-15T01:00:01Z".to_owned());
        repository.upsert_message(&second).expect("second message");

        let newest = repository
            .list_contact_source_messages_for_email(&account.account_id, "alice@example.com", 1)
            .expect("indexed contact lookup");
        assert_eq!(newest.len(), 1);
        assert_eq!(newest[0].message.uid, 2);
        assert!(
            repository
                .list_contact_source_messages_for_email(
                    &account.account_id,
                    "blind@example.com",
                    10,
                )
                .expect("Bcc-excluded lookup")
                .is_empty()
        );

        second.to = vec![MailAddress {
            name: Some("Carol".to_owned()),
            email: "carol@example.com".to_owned(),
        }];
        repository
            .upsert_message(&second)
            .expect("updated participants");
        let alice = repository
            .list_contact_source_messages_for_email(&account.account_id, "alice@example.com", 10)
            .expect("updated Alice lookup");
        assert_eq!(alice.len(), 1);
        assert_eq!(alice[0].message.uid, 1);
        assert_eq!(
            repository
                .list_contact_source_messages_for_email(
                    &account.account_id,
                    "carol@example.com",
                    10,
                )
                .expect("updated Carol lookup")[0]
                .message
                .uid,
            2
        );
    }

    #[test]
    fn message_bcc_round_trips_through_every_read_shape_without_expanding_search() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("message-bcc.sqlite3");
        let repository = Repository::open(&path).expect("repository");
        let account = AccountConfig::from_163_lines([
            "bcc-persistence@163.com",
            "not-a-real-authorization-value",
        ])
        .expect("account");
        repository
            .initialize_account(&account)
            .expect("account row");
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            411,
        );

        let mut full = message(&account.account_id, true);
        full.bcc = vec![MailAddress {
            name: Some("Old Blind Header".to_owned()),
            email: "old-blind@example.com".to_owned(),
        }];
        let row_id = repository.upsert_message(&full).expect("full message");

        let mut refreshed_summary = message(&account.account_id, false);
        refreshed_summary.subject = "Header refresh".to_owned();
        refreshed_summary.bcc = vec![MailAddress {
            name: Some("BlindPersistenceOnly".to_owned()),
            email: "blind@example.com".to_owned(),
        }];
        repository
            .upsert_message_summary(&refreshed_summary)
            .expect("summary refresh");

        let assert_bcc = |message: &InboxMessage| {
            assert_eq!(
                message
                    .bcc
                    .iter()
                    .map(|address| (address.name.as_deref(), address.email.as_str()))
                    .collect::<Vec<_>>(),
                [(Some("BlindPersistenceOnly"), "blind@example.com")]
            );
        };
        let stored = repository.get_message(row_id).expect("full read");
        assert_bcc(&stored);
        assert_eq!(stored.body_text.as_deref(), Some("Full body"));
        assert_bcc(&repository.list_inbox(&account.account_id, 10, 0).unwrap()[0]);
        assert_bcc(
            &repository
                .list_mailbox(&account.account_id, "INBOX", 10, 0)
                .unwrap()[0],
        );
        assert_bcc(
            &repository
                .list_contact_source_messages(&account.account_id)
                .unwrap()[0]
                .message,
        );
        assert_bcc(
            &repository
                .list_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 10, None)
                .unwrap()
                .items[0]
                .message,
        );
        assert!(
            repository
                .list_mailbox_page(
                    &account.account_id,
                    MailboxRole::Inbox,
                    None,
                    10,
                    Some("BlindPersistenceOnly"),
                )
                .expect("Bcc-excluded search")
                .items
                .is_empty()
        );
        drop(repository);

        let reopened = Repository::open(&path).expect("reopened repository");
        assert_bcc(&reopened.get_message(row_id).expect("read after restart"));
    }

    #[test]
    fn uidvalidity_reset_reimport_gets_a_new_message_public_id() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            91,
        );
        let source = message(&account.account_id, false);
        repository.upsert_message(&source).expect("first epoch");
        let old_public_id = repository
            .list_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 50, None)
            .expect("old epoch page")
            .items[0]
            .public_id
            .clone();

        assert_eq!(
            repository
                .reset_mailbox(&account.account_id, "INBOX")
                .expect("UIDVALIDITY reset"),
            1
        );
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            92,
        );
        repository.upsert_message(&source).expect("new epoch");
        let new_public_id = repository
            .list_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 50, None)
            .expect("new epoch page")
            .items[0]
            .public_id
            .clone();

        assert_ne!(new_public_id, old_public_id);
        assert_eq!(
            repository
                .list_contact_source_messages(&account.account_id)
                .expect("new epoch contact summary")[0]
                .public_id,
            new_public_id
        );
        assert!(
            repository
                .get_message_by_public_id(&account.account_id, &old_public_id)
                .is_err()
        );
    }

    #[test]
    fn separate_account_databases_never_accept_each_others_public_id() {
        let first_directory = TempDir::new().expect("first directory");
        let second_directory = TempDir::new().expect("second directory");
        let first = AccountConfig::from_163_lines([
            "opaque-first@163.com",
            "not-a-real-first-authorization-value",
        ])
        .expect("first account");
        let second = AccountConfig::from_163_lines([
            "opaque-second@163.com",
            "not-a-real-second-authorization-value",
        ])
        .expect("second account");
        let first_repository =
            Repository::open(first_directory.path().join("mail.sqlite3")).expect("first repo");
        let second_repository =
            Repository::open(second_directory.path().join("mail.sqlite3")).expect("second repo");
        first_repository
            .initialize_account(&first)
            .expect("first account row");
        second_repository
            .initialize_account(&second)
            .expect("second account row");

        assert_eq!(
            first_repository
                .upsert_message(&message(&first.account_id, false))
                .expect("first row"),
            1
        );
        assert_eq!(
            second_repository
                .upsert_message(&message(&second.account_id, false))
                .expect("second row"),
            1
        );
        let first_token = first_repository
            .list_mailbox_page(&first.account_id, MailboxRole::Inbox, None, 50, None)
            .expect("first page")
            .items[0]
            .public_id
            .clone();
        let second_token = second_repository
            .list_mailbox_page(&second.account_id, MailboxRole::Inbox, None, 50, None)
            .expect("second page")
            .items[0]
            .public_id
            .clone();

        assert_ne!(first_token, second_token);
        assert!(
            second_repository
                .get_message_by_public_id(&second.account_id, &first_token)
                .is_err()
        );
        assert!(
            first_repository
                .get_message_by_public_id(&first.account_id, &second_token)
                .is_err()
        );
    }

    #[test]
    fn flag_batches_are_atomic_and_preserve_pending_local_intent() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            70,
        );
        let first = message_with_identity(
            &account.account_id,
            "INBOX",
            42,
            "2026-07-14T01:00:01Z",
            "First",
        );
        let second = message_with_identity(
            &account.account_id,
            "INBOX",
            43,
            "2026-07-14T01:00:02Z",
            "Second",
        );
        let first_id = repository.upsert_message(&first).expect("first message");
        let second_id = repository.upsert_message(&second).expect("second message");
        repository
            .set_message_flagged_pending(&account.account_id, "INBOX", first.uid, true)
            .expect("pending star");

        assert_eq!(
            repository
                .update_message_flags_batch(
                    &account.account_id,
                    "INBOX",
                    &[
                        (first.uid, vec!["\\Seen".to_owned()]),
                        (second.uid, vec!["\\Flagged".to_owned()]),
                    ],
                )
                .expect("flag batch"),
            2
        );
        assert_eq!(
            repository.get_message(first_id).expect("first flags").flags,
            ["\\Seen", "\\Flagged"]
        );
        assert_eq!(
            repository
                .get_message(second_id)
                .expect("second flags")
                .flags,
            ["\\Flagged"]
        );

        assert!(
            repository
                .update_message_flags_batch(
                    &account.account_id,
                    "INBOX",
                    &[(first.uid, Vec::new()), (999, Vec::new())],
                )
                .is_err()
        );
        assert_eq!(
            repository
                .get_message(first_id)
                .expect("rolled back flags")
                .flags,
            ["\\Seen", "\\Flagged"]
        );
    }

    #[test]
    fn pending_seen_write_is_immediate_durable_and_reconciliation_safe() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            71,
        );
        let mut unread = message(&account.account_id, false);
        unread.flags = vec!["\\Flagged".to_owned()];
        let row_id = repository.upsert_message(&unread).expect("unread message");

        assert!(
            repository
                .mark_message_seen_pending(&account.account_id, "INBOX", unread.uid)
                .expect("mark seen")
        );
        assert_eq!(
            repository
                .pending_seen_uids(&account.account_id, "INBOX")
                .expect("pending seen"),
            [unread.uid]
        );
        assert_eq!(
            repository.get_message(row_id).expect("local message").flags,
            ["\\Flagged", "\\Seen"]
        );

        repository
            .update_message_flags(&account.account_id, "INBOX", unread.uid, &[])
            .expect("stale remote flags");
        let mut refreshed = unread.clone();
        refreshed.flags.clear();
        repository
            .upsert_message(&refreshed)
            .expect("stale remote summary");
        assert_eq!(
            repository
                .get_message(row_id)
                .expect("preserved seen")
                .flags,
            ["\\Seen"]
        );

        repository
            .complete_pending_seen(
                &account.account_id,
                "INBOX",
                unread.uid,
                &["\\Seen".to_owned()],
            )
            .expect("server confirmation");
        assert!(
            repository
                .pending_seen_uids(&account.account_id, "INBOX")
                .expect("cleared pending seen")
                .is_empty()
        );
        repository
            .update_message_flags(&account.account_id, "INBOX", unread.uid, &[])
            .expect("later remote unread state");
        assert!(
            repository
                .get_message(row_id)
                .expect("reconciled message")
                .flags
                .is_empty()
        );
    }

    #[test]
    fn pending_seen_revision_prevents_stale_read_result_from_overwriting_unread_intent() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            72,
        );
        let mut unread = message(&account.account_id, false);
        unread.flags.clear();
        let row_id = repository.upsert_message(&unread).expect("unread message");

        let (_, read_revision) = repository
            .set_message_seen_pending(&account.account_id, "INBOX", unread.uid, true)
            .expect("mark read");
        let (_, unread_revision) = repository
            .set_message_seen_pending(&account.account_id, "INBOX", unread.uid, false)
            .expect("mark unread");
        assert_eq!((read_revision, unread_revision), (1, 2));
        assert_eq!(
            repository
                .pending_seen_updates(&account.account_id, "INBOX")
                .expect("pending desired state"),
            [(unread.uid, false, unread_revision)]
        );

        assert!(
            !repository
                .complete_pending_seen_if_unchanged(
                    &account.account_id,
                    "INBOX",
                    unread.uid,
                    true,
                    read_revision,
                    &["\\Seen".to_owned()],
                )
                .expect("ignore stale read result")
        );
        repository
            .update_message_flags(
                &account.account_id,
                "INBOX",
                unread.uid,
                &["\\Seen".to_owned()],
            )
            .expect("stale server seen state");
        assert!(
            repository
                .get_message(row_id)
                .expect("unread overlay")
                .flags
                .is_empty()
        );

        assert!(
            repository
                .complete_pending_seen_if_unchanged(
                    &account.account_id,
                    "INBOX",
                    unread.uid,
                    false,
                    unread_revision,
                    &[],
                )
                .expect("confirm unread")
        );
        assert!(
            repository
                .pending_seen_updates(&account.account_id, "INBOX")
                .expect("seen queue")
                .is_empty()
        );
    }

    #[test]
    fn confirmed_seen_mutation_can_queue_a_later_unread_intent() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            74,
        );
        let mut unread = message(&account.account_id, false);
        unread.flags.clear();
        repository.upsert_message(&unread).expect("unread message");
        let (_, read) = repository
            .queue_system_flag_mutation(
                &account.account_id,
                "INBOX",
                unread.uid,
                SystemFlagKind::Seen,
                true,
            )
            .expect("read intent");
        repository
            .claim_system_flag_mutation(
                &account.account_id,
                &read.operation_id,
                SystemFlagKind::Seen,
                read.revision,
            )
            .expect("claim read")
            .expect("claimed read");
        assert!(
            repository
                .finalize_system_flag_mutation_confirmed(
                    &account.account_id,
                    &read.operation_id,
                    SystemFlagKind::Seen,
                    read.revision,
                    &["\\Seen".to_owned()],
                )
                .expect("confirm read")
        );

        let (_, unread_intent) = repository
            .queue_system_flag_mutation(
                &account.account_id,
                "INBOX",
                unread.uid,
                SystemFlagKind::Seen,
                false,
            )
            .expect("later unread intent");
        assert_eq!(unread_intent.operation_id, read.operation_id);
        assert_eq!(unread_intent.revision, read.revision + 1);
        assert_eq!(unread_intent.status, MutationStatus::Pending);
        assert!(!unread_intent.desired);
    }

    #[test]
    fn pending_flagged_toggle_is_durable_and_newer_intent_wins() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            73,
        );
        let mail = message(&account.account_id, false);
        let row_id = repository.upsert_message(&mail).expect("message");

        assert!(
            repository
                .set_message_flagged_pending(&account.account_id, "INBOX", mail.uid, true)
                .expect("star message")
        );
        assert_eq!(
            repository
                .pending_flagged_updates(&account.account_id, "INBOX")
                .expect("pending star"),
            [(mail.uid, true, 1)]
        );
        assert_eq!(
            repository.get_message(row_id).expect("starred local").flags,
            ["\\Seen", "\\Flagged"]
        );

        repository
            .update_message_flags(
                &account.account_id,
                "INBOX",
                mail.uid,
                &["\\Seen".to_owned()],
            )
            .expect("stale unstarred server state");
        assert_eq!(
            repository.get_message(row_id).expect("star overlay").flags,
            ["\\Seen", "\\Flagged"]
        );

        assert!(
            repository
                .set_message_flagged_pending(&account.account_id, "INBOX", mail.uid, false)
                .expect("unstar message")
        );
        assert_eq!(
            repository
                .pending_flagged_updates(&account.account_id, "INBOX")
                .expect("pending unstar"),
            [(mail.uid, false, 2)]
        );
        assert!(
            !repository
                .complete_pending_flagged(
                    &account.account_id,
                    "INBOX",
                    mail.uid,
                    true,
                    1,
                    &["\\Seen".to_owned(), "\\Flagged".to_owned()],
                )
                .expect("ignore stale confirmation")
        );
        assert_eq!(
            repository
                .get_message(row_id)
                .expect("newer unstar wins")
                .flags,
            ["\\Seen"]
        );

        assert!(
            repository
                .complete_pending_flagged(
                    &account.account_id,
                    "INBOX",
                    mail.uid,
                    false,
                    2,
                    &["\\Seen".to_owned()],
                )
                .expect("confirm unstar")
        );
        assert!(
            repository
                .pending_flagged_updates(&account.account_id, "INBOX")
                .expect("cleared star update")
                .is_empty()
        );
        repository
            .update_message_flags(
                &account.account_id,
                "INBOX",
                mail.uid,
                &["\\Seen".to_owned(), "\\Flagged".to_owned()],
            )
            .expect("later remote star");
        assert!(
            repository
                .get_message(row_id)
                .expect("remote star accepted")
                .flags
                .iter()
                .any(|flag| flag.eq_ignore_ascii_case("\\Flagged"))
        );
    }

    #[test]
    fn in_flight_flagged_toggle_is_retargeted_and_ignores_the_older_result() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            75,
        );
        let mail = message(&account.account_id, false);
        let row_id = repository.upsert_message(&mail).expect("message");
        let (_, star) = repository
            .queue_system_flag_mutation(
                &account.account_id,
                "INBOX",
                mail.uid,
                SystemFlagKind::Flagged,
                true,
            )
            .expect("star intent");
        repository
            .claim_system_flag_mutation(
                &account.account_id,
                &star.operation_id,
                SystemFlagKind::Flagged,
                star.revision,
            )
            .expect("claim star")
            .expect("in-flight star");

        let (_, unstar) = repository
            .queue_system_flag_mutation(
                &account.account_id,
                "INBOX",
                mail.uid,
                SystemFlagKind::Flagged,
                false,
            )
            .expect("unstar supersedes in-flight star");
        assert_eq!(unstar.operation_id, star.operation_id);
        assert_eq!(unstar.revision, star.revision + 1);
        assert_eq!(unstar.status, MutationStatus::Pending);
        assert!(!unstar.desired);

        assert!(
            !repository
                .finalize_system_flag_mutation_confirmed(
                    &account.account_id,
                    &star.operation_id,
                    SystemFlagKind::Flagged,
                    star.revision,
                    &["\\Seen".to_owned(), "\\Flagged".to_owned()],
                )
                .expect("ignore the older star confirmation")
        );
        assert_eq!(
            repository
                .get_message(row_id)
                .expect("newer unstar remains local")
                .flags,
            ["\\Seen"]
        );

        let claimed_unstar = repository
            .claim_system_flag_mutation(
                &account.account_id,
                &unstar.operation_id,
                SystemFlagKind::Flagged,
                unstar.revision,
            )
            .expect("claim unstar")
            .expect("in-flight unstar");
        assert!(
            repository
                .finalize_system_flag_mutation_confirmed(
                    &account.account_id,
                    &claimed_unstar.operation_id,
                    SystemFlagKind::Flagged,
                    claimed_unstar.revision,
                    &["\\Seen".to_owned()],
                )
                .expect("confirm latest unstar")
        );
        let receipt = repository
            .system_flag_mutation_receipt(
                &account.account_id,
                &unstar.operation_id,
                SystemFlagKind::Flagged,
            )
            .expect("unstar receipt")
            .expect("durable unstar operation");
        assert_eq!(receipt.status, MutationStatus::Confirmed);
        assert_eq!(receipt.local_revision, unstar.revision);
        assert!(!receipt.desired);
    }

    #[test]
    fn uidvalidity_reset_preserves_flag_intent_without_overlaying_reused_uid() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            401,
        );
        let mut original = message(&account.account_id, false);
        original.flags = vec!["\\Seen".to_owned()];
        repository
            .upsert_message(&original)
            .expect("original message");
        let (_, mutation) = repository
            .queue_system_flag_mutation(
                &account.account_id,
                "INBOX",
                original.uid,
                SystemFlagKind::Flagged,
                true,
            )
            .expect("queue flag");

        repository
            .reset_mailbox(&account.account_id, "INBOX")
            .expect("reset old epoch");
        let receipt = repository
            .system_flag_mutation_receipt(
                &account.account_id,
                &mutation.operation_id,
                SystemFlagKind::Flagged,
            )
            .expect("flag receipt")
            .expect("durable flag intent");
        assert_eq!(receipt.status, MutationStatus::NeedsAttention);

        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            402,
        );
        let mut reused = original;
        reused.flags = vec!["\\Seen".to_owned()];
        let reused_id = repository.upsert_message(&reused).expect("reused UID");
        repository
            .update_message_flags(
                &account.account_id,
                "INBOX",
                reused.uid,
                &["\\Seen".to_owned()],
            )
            .expect("new epoch flags");
        assert!(
            !repository
                .get_message(reused_id)
                .expect("new epoch message")
                .flags
                .iter()
                .any(|flag| flag.eq_ignore_ascii_case("\\Flagged"))
        );
        assert!(
            repository
                .system_flag_mutations_requiring_reconciliation(
                    &account.account_id,
                    "INBOX",
                    SystemFlagKind::Flagged,
                )
                .expect("current epoch reconciliation queue")
                .is_empty()
        );
    }

    #[test]
    fn sent_role_resolves_provider_mailbox_and_lists_only_that_mailbox() {
        let (_directory, repository, account) = setup();
        let mut sent = message(&account.account_id, false);
        sent.mailbox = "已发送".to_owned();
        sent.uid = 7;
        repository.upsert_message(&sent).expect("sent summary");
        repository
            .assign_mailbox_role(&account.account_id, "sent", &sent.mailbox)
            .expect("sent role");

        assert_eq!(
            repository
                .mailbox_for_role(&account.account_id, "sent")
                .expect("resolved role"),
            "已发送"
        );
        let listed = repository
            .list_mailbox(&account.account_id, "已发送", 10, 0)
            .expect("sent list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].uid, 7);
        assert!(
            repository
                .list_inbox(&account.account_id, 10, 0)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn mailbox_capabilities_and_history_are_account_scoped_and_round_trip() {
        let (_directory, repository, account) = setup();
        let capabilities = repository
            .mailbox_capabilities(&account.account_id)
            .expect("default capabilities");
        assert_eq!(capabilities.len(), MailboxRole::ALL.len());
        assert_eq!(
            capabilities
                .iter()
                .find(|capability| capability.role == MailboxRole::Inbox)
                .expect("inbox capability")
                .status,
            MailboxCapabilityStatus::Available
        );
        assert!(
            capabilities
                .iter()
                .filter(|capability| capability.role != MailboxRole::Inbox)
                .all(|capability| {
                    capability.status == MailboxCapabilityStatus::DiscoveryPending
                        && capability.retryable
                })
        );

        repository
            .set_mailbox_capability(
                &account.account_id,
                &MailboxCapability {
                    role: MailboxRole::Archive,
                    status: MailboxCapabilityStatus::Unavailable,
                    display_name: None,
                    unavailable_reason: Some(
                        MailboxCapabilityUnavailableReason::CreateNotSupported,
                    ),
                    retryable: false,
                },
            )
            .expect("unavailable archive");
        let archive = repository
            .mailbox_capability(&account.account_id, MailboxRole::Archive)
            .expect("archive capability")
            .expect("archive row");
        assert_eq!(
            archive.unavailable_reason,
            Some(MailboxCapabilityUnavailableReason::CreateNotSupported)
        );

        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Archive,
            "Archive",
            51,
        );
        let history = MailboxHistory {
            before_uid: Some(700),
            complete: false,
            remote_total: Some(1_234),
        };
        repository
            .update_mailbox_history(&account.account_id, "Archive", &history)
            .expect("history state");
        assert_eq!(
            repository
                .mailbox_history(&account.account_id, "Archive")
                .expect("history")
                .expect("history row"),
            history
        );
        repository
            .reset_mailbox(&account.account_id, "Archive")
            .expect("reset archive");
        assert_eq!(
            repository
                .mailbox_history(&account.account_id, "Archive")
                .expect("reset history")
                .expect("archive row"),
            MailboxHistory::default()
        );
    }

    #[test]
    fn confirmed_uid_snapshot_can_rebase_an_incomplete_history_boundary() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            601,
        );
        assert!(
            repository
                .reconcile_mailbox_history(
                    &account.account_id,
                    "INBOX",
                    601,
                    None,
                    Some(500),
                    false,
                    2_000,
                )
                .expect("initial confirmed boundary")
        );
        assert!(
            repository
                .reconcile_mailbox_history(
                    &account.account_id,
                    "INBOX",
                    601,
                    Some(500),
                    Some(900),
                    false,
                    2_000,
                )
                .expect("rebase skipped higher UIDs")
        );
        assert!(
            !repository
                .reconcile_mailbox_history(
                    &account.account_id,
                    "INBOX",
                    601,
                    Some(500),
                    Some(700),
                    false,
                    2_000,
                )
                .expect("stale confirmed writer")
        );
        assert_eq!(
            repository
                .mailbox_history(&account.account_id, "INBOX")
                .expect("history state")
                .expect("mailbox row"),
            MailboxHistory {
                before_uid: Some(900),
                complete: false,
                remote_total: Some(2_000),
            }
        );
    }

    #[test]
    fn mailbox_history_cas_rejects_late_epoch_and_boundary_writers() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Archive,
            "Archive",
            501,
        );
        assert!(
            repository
                .advance_mailbox_history(
                    &account.account_id,
                    "Archive",
                    501,
                    None,
                    Some(900),
                    false,
                    Some(2_000),
                )
                .expect("establish history boundary")
        );
        assert!(
            repository
                .advance_mailbox_history(
                    &account.account_id,
                    "Archive",
                    501,
                    Some(900),
                    Some(700),
                    false,
                    Some(2_000),
                )
                .expect("newer history page")
        );
        assert!(
            !repository
                .advance_mailbox_history(
                    &account.account_id,
                    "Archive",
                    501,
                    Some(900),
                    Some(800),
                    false,
                    Some(2_000),
                )
                .expect("late page completion")
        );
        assert!(
            !repository
                .advance_mailbox_history(
                    &account.account_id,
                    "Archive",
                    500,
                    Some(700),
                    Some(600),
                    false,
                    Some(2_000),
                )
                .expect("stale epoch")
        );
        assert!(
            repository
                .advance_mailbox_history(
                    &account.account_id,
                    "Archive",
                    501,
                    Some(700),
                    None,
                    true,
                    Some(2_000),
                )
                .expect("history floor")
        );
        assert!(
            !repository
                .advance_mailbox_history(
                    &account.account_id,
                    "Archive",
                    501,
                    None,
                    Some(100),
                    false,
                    Some(2_000),
                )
                .expect("complete history cannot regress")
        );
        assert_eq!(
            repository
                .mailbox_history(&account.account_id, "Archive")
                .expect("history")
                .expect("history row"),
            MailboxHistory {
                before_uid: None,
                complete: true,
                remote_total: Some(2_000),
            }
        );
    }

    #[test]
    fn move_queue_collapses_to_latest_intent_and_projects_without_forging_uid() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            77,
        );
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Archive,
            "Archive",
            81,
        );
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Trash,
            "Trash",
            82,
        );
        let source = message_with_identity(
            &account.account_id,
            "INBOX",
            42,
            "2026-07-20T12:00:00Z",
            "Queued move",
        );
        let message_id = repository.upsert_message(&source).expect("source message");

        let archive = repository
            .queue_message_action(
                message_id,
                MailboxRole::Inbox,
                MessageActionKind::Archive,
                Some(MailboxRole::Archive),
            )
            .expect("archive intent");
        assert_eq!(archive.local_revision, 1);
        assert_eq!(archive.status, MutationStatus::Pending);
        assert!(
            repository
                .list_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 50, None)
                .expect("inbox overlay")
                .items
                .is_empty()
        );
        let archive_page = repository
            .list_mailbox_page(&account.account_id, MailboxRole::Archive, None, 50, None)
            .expect("archive projection");
        assert_eq!(archive_page.items.len(), 1);
        let projected = &archive_page.items[0];
        assert_eq!(projected.message.mailbox, "INBOX");
        assert_eq!(projected.message.uid, 42);
        assert_eq!(projected.displayed_role, MailboxRole::Archive);
        assert_eq!(
            projected
                .pending_mutation
                .as_ref()
                .expect("pending projection")
                .operation_id,
            archive.operation_id
        );

        let trash = repository
            .queue_message_action(
                message_id,
                MailboxRole::Inbox,
                MessageActionKind::MoveToTrash,
                Some(MailboxRole::Trash),
            )
            .expect("newer trash intent");
        assert_eq!(trash.operation_id, archive.operation_id);
        assert_eq!(trash.local_revision, 2);
        assert!(
            repository
                .list_mailbox_page(&account.account_id, MailboxRole::Archive, None, 50, None)
                .expect("archive no longer projected")
                .items
                .is_empty()
        );
        assert_eq!(
            repository
                .list_mailbox_page(&account.account_id, MailboxRole::Trash, None, 50, None)
                .expect("trash projection")
                .items
                .len(),
            1
        );
        assert!(
            !repository
                .update_message_action_status_if_unchanged(
                    &account.account_id,
                    &trash.operation_id,
                    1,
                    MutationStatus::Confirmed,
                    None,
                )
                .expect("stale completion")
        );
        assert!(
            repository
                .claim_message_action(
                    &account.account_id,
                    &trash.operation_id,
                    trash.local_revision,
                )
                .expect("claim message action")
                .is_some()
        );
        assert!(
            repository
                .update_message_action_status_if_unchanged(
                    &account.account_id,
                    &trash.operation_id,
                    trash.local_revision,
                    MutationStatus::OutcomeUnknown,
                    Some(MessageMutationErrorKind::NetworkUnavailable),
                )
                .expect("uncertain outcome")
        );

        let other = secondary_account(&account);
        repository
            .initialize_account(&other)
            .expect("secondary account row");
        assert!(
            repository
                .pending_message_actions(&other.account_id)
                .expect("secondary queue")
                .is_empty()
        );
        repository
            .reset_mailbox(&account.account_id, "INBOX")
            .expect("UIDVALIDITY reset");
        assert!(
            repository
                .pending_message_actions(&account.account_id)
                .expect("worker queue")
                .is_empty()
        );
        let durable = repository
            .message_action(&account.account_id, &trash.operation_id)
            .expect("durable action query")
            .expect("durable action");
        assert_eq!(durable.source_uid_validity, 77);
        assert_eq!(durable.status, MutationStatus::NeedsAttention);
        assert_eq!(
            durable.error_kind,
            Some(MessageMutationErrorKind::UidValidityChanged)
        );
    }

    #[test]
    fn unknown_message_action_cannot_be_reordered_or_cross_account_scoped() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            601,
        );
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Archive,
            "Archive",
            602,
        );
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Trash,
            "Trash",
            603,
        );
        let message_id = repository
            .upsert_message(&message(&account.account_id, false))
            .expect("source message");
        let other = secondary_account(&account);
        repository
            .initialize_account(&other)
            .expect("secondary account");
        assert!(
            repository
                .queue_message_action_for_account(
                    &other.account_id,
                    message_id,
                    MailboxRole::Inbox,
                    MessageActionKind::Archive,
                    Some(MailboxRole::Archive),
                )
                .is_err()
        );

        let queued = repository
            .queue_message_action_for_account(
                &account.account_id,
                message_id,
                MailboxRole::Inbox,
                MessageActionKind::Archive,
                Some(MailboxRole::Archive),
            )
            .expect("archive intent");
        assert!(
            repository
                .claim_message_action(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                )
                .expect("claim action")
                .is_some()
        );
        assert!(
            repository
                .advance_message_action_remote_phase(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    RemoteMutationPhase::Queued,
                    RemoteMutationPhase::TransferStarted,
                )
                .expect("transfer started")
        );
        assert!(
            repository
                .finalize_message_action(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    MutationStatus::OutcomeUnknown,
                    Some(MessageMutationErrorKind::NetworkUnavailable),
                )
                .expect("unknown outcome")
        );
        assert!(
            repository
                .queue_message_action_for_account(
                    &account.account_id,
                    message_id,
                    MailboxRole::Inbox,
                    MessageActionKind::MoveToTrash,
                    Some(MailboxRole::Trash),
                )
                .is_err()
        );
        assert!(
            repository
                .pending_message_actions(&account.account_id)
                .expect("ordinary worker queue")
                .is_empty()
        );
        assert_eq!(
            repository
                .message_actions_requiring_reconciliation(&account.account_id)
                .expect("reconciliation queue")
                .len(),
            1
        );
        assert!(
            repository
                .reconcile_message_action(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    MutationStatus::Pending,
                    None,
                )
                .expect("explicit requeue")
        );
        let requeued = repository
            .message_action(&account.account_id, &queued.operation_id)
            .expect("message action")
            .expect("requeued action");
        assert_eq!(requeued.revision, queued.local_revision + 1);
        assert_eq!(requeued.remote_phase, RemoteMutationPhase::Queued);

        let connection = repository.connection().expect("connection");
        let invalid_insert = connection.execute(
            "INSERT INTO pending_message_actions (
                 operation_id, account_id, source_mailbox, source_uid_validity, source_uid,
                 source_role, destination_role, kind, revision, status, remote_phase,
                 source_size_bytes
             ) VALUES (?1, ?2, 'INBOX', 601, 999, 'trash', 'archive', 'archive',
                       1, 'pending', 'queued', 0)",
            params![uuid::Uuid::now_v7().to_string(), account.account_id],
        );
        assert!(invalid_insert.is_err());
    }

    #[test]
    fn copy_delete_phase_survives_crash_as_reconcilable_unknown() {
        let (directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            701,
        );
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Archive,
            "Archive",
            702,
        );
        let message_id = repository
            .upsert_message(&message(&account.account_id, false))
            .expect("source message");
        let queued = repository
            .queue_message_action_for_account(
                &account.account_id,
                message_id,
                MailboxRole::Inbox,
                MessageActionKind::Archive,
                Some(MailboxRole::Archive),
            )
            .expect("archive intent");
        repository
            .claim_message_action(
                &account.account_id,
                &queued.operation_id,
                queued.local_revision,
            )
            .expect("claim")
            .expect("claimed action");
        for (expected, next) in [
            (
                RemoteMutationPhase::Queued,
                RemoteMutationPhase::TransferStarted,
            ),
            (
                RemoteMutationPhase::TransferStarted,
                RemoteMutationPhase::TransferAcknowledged,
            ),
            (
                RemoteMutationPhase::TransferAcknowledged,
                RemoteMutationPhase::SourceDeleteStarted,
            ),
        ] {
            assert!(
                repository
                    .advance_message_action_remote_phase(
                        &account.account_id,
                        &queued.operation_id,
                        queued.local_revision,
                        expected,
                        next,
                    )
                    .expect("advance persisted phase")
            );
        }
        drop(repository);

        let reopened =
            Repository::open(directory.path().join("mail.sqlite3")).expect("startup recovery");
        let recovered = reopened
            .message_action(&account.account_id, &queued.operation_id)
            .expect("recovered action")
            .expect("durable recovered action");
        assert_eq!(recovered.status, MutationStatus::OutcomeUnknown);
        assert_eq!(
            recovered.remote_phase,
            RemoteMutationPhase::SourceDeleteStarted
        );
        assert_eq!(
            recovered.error_kind,
            Some(MessageMutationErrorKind::Unknown)
        );
        assert!(
            reopened
                .pending_message_actions(&account.account_id)
                .expect("worker queue")
                .is_empty()
        );
        assert_eq!(
            reopened
                .message_actions_requiring_reconciliation(&account.account_id)
                .expect("recovery queue"),
            [recovered]
        );
    }

    #[test]
    fn confirmed_projection_survives_source_cleanup_until_unique_destination_converges() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            801,
        );
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Archive,
            "Archive",
            802,
        );
        let source = message(&account.account_id, false);
        let source_id = repository.upsert_message(&source).expect("source message");
        let queued = repository
            .queue_message_action_for_account(
                &account.account_id,
                source_id,
                MailboxRole::Inbox,
                MessageActionKind::Archive,
                Some(MailboxRole::Archive),
            )
            .expect("archive action");
        repository
            .claim_message_action(
                &account.account_id,
                &queued.operation_id,
                queued.local_revision,
            )
            .expect("claim action")
            .expect("claimed action");
        assert!(
            repository
                .advance_message_action_remote_phase(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    RemoteMutationPhase::Queued,
                    RemoteMutationPhase::TransferStarted,
                )
                .expect("move started")
        );
        assert!(
            repository
                .advance_message_action_remote_phase(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    RemoteMutationPhase::TransferStarted,
                    RemoteMutationPhase::SourceDeleteAcknowledged,
                )
                .expect("move acknowledged")
        );
        assert!(
            repository
                .finalize_message_action(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    MutationStatus::Confirmed,
                    None,
                )
                .expect("confirm action")
        );
        assert_eq!(
            repository
                .delete_missing_uids(&account.account_id, "INBOX", &HashSet::new())
                .expect("source reconciliation"),
            0
        );
        assert!(repository.get_message(source_id).is_ok());
        let projected = repository
            .list_mailbox_page(&account.account_id, MailboxRole::Archive, None, 50, None)
            .expect("confirmed target projection");
        assert_eq!(projected.items.len(), 1);
        assert_eq!(
            projected.items[0]
                .pending_mutation
                .as_ref()
                .expect("confirmed projection")
                .status,
            MutationStatus::Confirmed
        );

        let mut destination = source.clone();
        destination.mailbox = "Archive".to_owned();
        destination.uid = 900;
        repository
            .upsert_message(&destination)
            .expect("real destination summary");
        assert!(
            repository
                .purge_confirmed_message_action_if_destination_unique(
                    &account.account_id,
                    &queued.operation_id,
                )
                .expect("converge projection")
        );
        assert!(repository.get_message(source_id).is_err());
        let converged = repository
            .list_mailbox_page(&account.account_id, MailboxRole::Archive, None, 50, None)
            .expect("converged target");
        assert_eq!(converged.items.len(), 1);
        assert_eq!(converged.items[0].message.uid, 900);
        assert!(converged.items[0].pending_mutation.is_none());
    }

    #[test]
    fn deferred_source_cleanup_keeps_tombstone_until_destination_and_source_converge() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            901,
        );
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Archive,
            "Archive",
            902,
        );
        let source = message(&account.account_id, false);
        let source_id = repository.upsert_message(&source).expect("source");
        let queued = repository
            .queue_message_action_for_account(
                &account.account_id,
                source_id,
                MailboxRole::Inbox,
                MessageActionKind::Archive,
                Some(MailboxRole::Archive),
            )
            .expect("archive");
        repository
            .claim_message_action(
                &account.account_id,
                &queued.operation_id,
                queued.local_revision,
            )
            .expect("claim")
            .expect("claimed");
        for (expected, next) in [
            (
                RemoteMutationPhase::Queued,
                RemoteMutationPhase::TransferStarted,
            ),
            (
                RemoteMutationPhase::TransferStarted,
                RemoteMutationPhase::SourceDeleteAcknowledged,
            ),
        ] {
            assert!(
                repository
                    .advance_message_action_remote_phase(
                        &account.account_id,
                        &queued.operation_id,
                        queued.local_revision,
                        expected,
                        next,
                    )
                    .expect("phase")
            );
        }
        assert!(
            repository
                .finalize_message_action_confirmed(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    true,
                )
                .expect("atomic deferred confirmation")
        );
        let tombstone = repository
            .confirmed_source_cleanup_tombstones(&account.account_id)
            .expect("tombstones")
            .pop()
            .expect("cleanup tombstone");
        assert!(tombstone.source_cleanup_pending);
        assert!(!tombstone.destination_reconciled);
        assert!(
            !repository
                .purge_confirmed_source_cleanup_if_remote_absent(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    901,
                    source.uid,
                )
                .expect("destination still projected")
        );

        let projected = repository
            .list_mailbox_page(&account.account_id, MailboxRole::Archive, None, 50, None)
            .expect("optimistic projection");
        assert_eq!(projected.items.len(), 1);
        assert!(projected.items[0].pending_mutation.is_some());

        let mut destination = source.clone();
        destination.mailbox = "Archive".to_owned();
        destination.uid = 942;
        repository
            .upsert_message(&destination)
            .expect("destination");
        assert!(
            repository
                .purge_confirmed_message_action_if_destination_unique(
                    &account.account_id,
                    &queued.operation_id,
                )
                .expect("destination reconciliation")
        );
        let retained = repository
            .message_action(&account.account_id, &queued.operation_id)
            .expect("action query")
            .expect("retained tombstone");
        assert!(retained.source_cleanup_pending);
        assert!(retained.destination_reconciled);
        assert!(repository.get_message(source_id).is_err());
        let converged = repository
            .list_mailbox_page(&account.account_id, MailboxRole::Archive, None, 50, None)
            .expect("real destination only");
        assert_eq!(converged.items.len(), 1);
        assert!(converged.items[0].pending_mutation.is_none());

        repository
            .upsert_message(&source)
            .expect("source rediscovered before expunge");
        assert!(
            repository
                .list_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 50, None)
                .expect("hidden rediscovered source")
                .items
                .is_empty()
        );
        assert!(
            !repository
                .purge_confirmed_source_cleanup_if_remote_absent(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    999,
                    source.uid,
                )
                .expect("wrong epoch")
        );
        assert!(
            repository
                .purge_confirmed_source_cleanup_if_remote_absent(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    901,
                    source.uid,
                )
                .expect("same epoch source absent")
        );
        assert!(
            repository
                .message_action(&account.account_id, &queued.operation_id)
                .expect("purged action")
                .is_none()
        );
    }

    #[test]
    fn recoverable_permanent_delete_can_become_a_deferred_cleanup_tombstone() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Trash,
            "Trash",
            903,
        );
        let mut source = message(&account.account_id, false);
        source.mailbox = "Trash".to_owned();
        source.uid = 52;
        let source_id = repository.upsert_message(&source).expect("trash source");
        let queued = repository
            .queue_message_action_for_account(
                &account.account_id,
                source_id,
                MailboxRole::Trash,
                MessageActionKind::PermanentDelete,
                None,
            )
            .expect("permanent delete");
        repository
            .claim_message_action(
                &account.account_id,
                &queued.operation_id,
                queued.local_revision,
            )
            .expect("claim")
            .expect("claimed");
        for (expected, next) in [(
            RemoteMutationPhase::Queued,
            RemoteMutationPhase::SourceDeleteStarted,
        )] {
            assert!(
                repository
                    .advance_message_action_remote_phase(
                        &account.account_id,
                        &queued.operation_id,
                        queued.local_revision,
                        expected,
                        next,
                    )
                    .expect("delete phase")
            );
        }
        assert!(
            repository
                .finalize_message_action(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    MutationStatus::OutcomeUnknown,
                    Some(MessageMutationErrorKind::NetworkUnavailable),
                )
                .expect("recoverable finalize")
        );
        assert!(
            repository
                .reconcile_message_action_confirmed(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    true,
                )
                .expect("atomic reconciled confirmation")
        );
        let action = repository
            .message_action(&account.account_id, &queued.operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(action.status, MutationStatus::Confirmed);
        assert_eq!(
            action.remote_phase,
            RemoteMutationPhase::SourceDeleteAcknowledged
        );
        assert_eq!(action.error_kind, None);
        assert!(action.source_cleanup_pending);
        assert!(!action.destination_reconciled);
        assert!(
            repository
                .purge_confirmed_source_cleanup_if_remote_absent(
                    &account.account_id,
                    &queued.operation_id,
                    queued.local_revision,
                    903,
                    source.uid,
                )
                .expect("permanent-delete source absent")
        );
        assert!(repository.get_message(source_id).is_err());
    }

    #[test]
    fn reconciled_confirmation_allows_only_proven_safe_durable_phases() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            904,
        );
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Archive,
            "Archive",
            905,
        );
        let archive_source = repository
            .upsert_message(&message(&account.account_id, false))
            .expect("archive source");
        let archive = repository
            .queue_message_action_for_account(
                &account.account_id,
                archive_source,
                MailboxRole::Inbox,
                MessageActionKind::Archive,
                Some(MailboxRole::Archive),
            )
            .expect("archive action");
        repository
            .claim_message_action(
                &account.account_id,
                &archive.operation_id,
                archive.local_revision,
            )
            .unwrap()
            .expect("archive claimed");
        assert!(
            repository
                .advance_message_action_remote_phase(
                    &account.account_id,
                    &archive.operation_id,
                    archive.local_revision,
                    RemoteMutationPhase::Queued,
                    RemoteMutationPhase::TransferStarted,
                )
                .unwrap()
        );
        assert!(
            repository
                .finalize_message_action(
                    &account.account_id,
                    &archive.operation_id,
                    archive.local_revision,
                    MutationStatus::OutcomeUnknown,
                    Some(MessageMutationErrorKind::Unknown),
                )
                .unwrap()
        );
        assert!(
            !repository
                .reconcile_message_action_confirmed(
                    &account.account_id,
                    &archive.operation_id,
                    archive.local_revision,
                    true,
                )
                .expect("transfer-started must not confirm")
        );
        assert!(
            repository
                .reconcile_message_action(
                    &account.account_id,
                    &archive.operation_id,
                    archive.local_revision,
                    MutationStatus::Pending,
                    None,
                )
                .expect("requeue archive")
        );
        let archive_revision = archive.local_revision + 1;
        repository
            .claim_message_action(&account.account_id, &archive.operation_id, archive_revision)
            .unwrap()
            .expect("archive reclaimed");
        for (expected, next) in [
            (
                RemoteMutationPhase::Queued,
                RemoteMutationPhase::TransferStarted,
            ),
            (
                RemoteMutationPhase::TransferStarted,
                RemoteMutationPhase::TransferAcknowledged,
            ),
        ] {
            assert!(
                repository
                    .advance_message_action_remote_phase(
                        &account.account_id,
                        &archive.operation_id,
                        archive_revision,
                        expected,
                        next,
                    )
                    .unwrap()
            );
        }
        assert!(
            repository
                .finalize_message_action(
                    &account.account_id,
                    &archive.operation_id,
                    archive_revision,
                    MutationStatus::OutcomeUnknown,
                    Some(MessageMutationErrorKind::Unknown),
                )
                .unwrap()
        );
        assert!(
            repository
                .reconcile_message_action_confirmed(
                    &account.account_id,
                    &archive.operation_id,
                    archive_revision,
                    true,
                )
                .expect("transfer acknowledged confirmation")
        );
        assert_eq!(
            repository
                .message_action(&account.account_id, &archive.operation_id)
                .unwrap()
                .unwrap()
                .remote_phase,
            RemoteMutationPhase::SourceDeleteAcknowledged
        );

        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Trash,
            "Trash",
            906,
        );
        let mut trash = message(&account.account_id, false);
        trash.mailbox = "Trash".to_owned();
        trash.uid = 54;
        let trash_id = repository.upsert_message(&trash).expect("trash source");
        let permanent = repository
            .queue_message_action_for_account(
                &account.account_id,
                trash_id,
                MailboxRole::Trash,
                MessageActionKind::PermanentDelete,
                None,
            )
            .expect("permanent action");
        repository
            .claim_message_action(
                &account.account_id,
                &permanent.operation_id,
                permanent.local_revision,
            )
            .unwrap()
            .expect("permanent claimed");
        assert!(
            repository
                .finalize_message_action(
                    &account.account_id,
                    &permanent.operation_id,
                    permanent.local_revision,
                    MutationStatus::NeedsAttention,
                    Some(MessageMutationErrorKind::SourceMissing),
                )
                .unwrap()
        );
        assert!(
            !repository
                .reconcile_message_action_confirmed(
                    &account.account_id,
                    &permanent.operation_id,
                    permanent.local_revision,
                    true,
                )
                .expect("queued cleanup-pending confirmation is forbidden")
        );
        assert!(
            repository
                .reconcile_message_action_confirmed(
                    &account.account_id,
                    &permanent.operation_id,
                    permanent.local_revision,
                    false,
                )
                .expect("queued permanent source already absent")
        );
        let permanent = repository
            .message_action(&account.account_id, &permanent.operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(permanent.status, MutationStatus::Confirmed);
        assert!(!permanent.source_cleanup_pending);
    }

    #[test]
    fn every_inflight_phase_enters_reconciliation_without_unsafe_confirmation() {
        let (directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            907,
        );
        let phases = [
            RemoteMutationPhase::Queued,
            RemoteMutationPhase::TransferStarted,
            RemoteMutationPhase::TransferAcknowledged,
            RemoteMutationPhase::SourceDeleteStarted,
            RemoteMutationPhase::SourceDeleteAcknowledged,
        ];
        let connection = repository.connection().expect("connection");
        let mut operation_ids = Vec::new();
        for (offset, phase) in phases.into_iter().enumerate() {
            let operation_id = uuid::Uuid::now_v7().to_string();
            connection
                .execute(
                    "INSERT INTO pending_message_actions (
                         operation_id, account_id, source_mailbox, source_uid_validity,
                         source_uid, source_role, destination_role, kind, revision,
                         status, remote_phase, source_size_bytes
                     ) VALUES (
                         ?1, ?2, 'INBOX', 907, ?3, 'inbox', 'archive', 'archive',
                         1, 'in_flight', ?4, 10
                     )",
                    params![
                        operation_id,
                        account.account_id,
                        100 + offset as u32,
                        phase.as_str(),
                    ],
                )
                .expect("in-flight fixture");
            operation_ids.push(operation_id);
        }
        drop(connection);

        let requiring = repository
            .message_actions_requiring_reconciliation(&account.account_id)
            .expect("all in-flight actions");
        assert_eq!(requiring.len(), phases.len());
        assert!(
            requiring
                .iter()
                .all(|action| action.status == MutationStatus::InFlight)
        );
        for phase in phases {
            assert!(requiring.iter().any(|action| action.remote_phase == phase));
        }
        drop(repository);
        let repository =
            Repository::open(directory.path().join("mail.sqlite3")).expect("restart repository");
        let requiring_after_restart = repository
            .message_actions_requiring_reconciliation(&account.account_id)
            .expect("restart reconciliation queue");
        assert_eq!(requiring_after_restart.len(), phases.len());
        for phase in phases {
            assert!(
                requiring_after_restart
                    .iter()
                    .any(|action| action.remote_phase == phase)
            );
        }
        assert!(
            repository
                .reconcile_message_action(
                    &account.account_id,
                    &operation_ids[0],
                    1,
                    MutationStatus::OutcomeUnknown,
                    Some(MessageMutationErrorKind::Unknown),
                )
                .expect("queued in-flight reconciliation")
        );
        assert!(
            !repository
                .reconcile_message_action_confirmed(
                    &account.account_id,
                    &operation_ids[1],
                    1,
                    true,
                )
                .expect("transfer-started confirmation is forbidden")
        );
        for operation_id in &operation_ids[2..] {
            assert!(
                repository
                    .reconcile_message_action_confirmed(&account.account_id, operation_id, 1, true,)
                    .expect("post-transfer source cleanup confirmation")
            );
        }
        let transfer_started = repository
            .message_action(&account.account_id, &operation_ids[1])
            .unwrap()
            .unwrap();
        assert_eq!(transfer_started.status, MutationStatus::OutcomeUnknown);
        assert_eq!(
            transfer_started.remote_phase,
            RemoteMutationPhase::TransferStarted
        );
    }

    #[test]
    fn keyset_page_is_stable_for_equal_timestamps_and_newer_insertions() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            90,
        );
        repository
            .update_mailbox_history(
                &account.account_id,
                "INBOX",
                &MailboxHistory {
                    before_uid: Some(1),
                    complete: true,
                    remote_total: Some(6),
                },
            )
            .expect("complete history");
        for uid in 1..=6 {
            repository
                .upsert_message(&message_with_identity(
                    &account.account_id,
                    "INBOX",
                    uid,
                    "2026-07-21T09:00:00Z",
                    &format!("Message {uid}"),
                ))
                .expect("message");
        }

        let first = repository
            .list_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 2, None)
            .expect("first page");
        assert_eq!(
            first
                .items
                .iter()
                .map(|item| item.message.uid)
                .collect::<Vec<_>>(),
            [6, 5]
        );
        assert!(first.has_more_local);
        assert_eq!(first.remote_history_state, RemoteHistoryState::NotChecked);
        let cursor = first.next_cursor.as_ref().expect("continuation");

        repository
            .upsert_message(&message_with_identity(
                &account.account_id,
                "INBOX",
                7,
                "2026-07-21T09:00:00Z",
                "New arrival",
            ))
            .expect("newer insertion");
        let second = repository
            .load_older_mailbox_page(&account.account_id, MailboxRole::Inbox, cursor, 2, None)
            .expect("second page");
        assert_eq!(
            second
                .items
                .iter()
                .map(|item| item.message.uid)
                .collect::<Vec<_>>(),
            [4, 3]
        );
        assert!(
            repository
                .list_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 101, None)
                .is_err()
        );
    }

    #[test]
    fn starred_pages_filter_flags_and_keep_their_cursor_and_history_isolated() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            91,
        );
        repository
            .update_mailbox_history(
                &account.account_id,
                "INBOX",
                &MailboxHistory {
                    before_uid: Some(1),
                    complete: true,
                    remote_total: Some(6),
                },
            )
            .expect("ordinary history");
        for uid in 1..=6 {
            let mut message = message_with_identity(
                &account.account_id,
                "INBOX",
                uid,
                "2026-07-21T09:00:00Z",
                &format!("Message {uid}"),
            );
            if uid % 2 == 0 {
                message.flags = vec!["\\Flagged".to_owned()];
            }
            repository.upsert_message(&message).expect("message");
        }

        let first = repository
            .list_starred_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 1, None)
            .expect("first starred page");
        assert_eq!(first.items[0].message.uid, 6);
        let starred_cursor = first.next_cursor.as_ref().expect("starred cursor");
        assert!(
            repository
                .message_page_cursor_context(starred_cursor)
                .expect("starred cursor context")
                .flagged_only
        );
        let second = repository
            .load_older_starred_mailbox_page(
                &account.account_id,
                MailboxRole::Inbox,
                starred_cursor,
                1,
                None,
            )
            .expect("second starred page");
        assert_eq!(second.items[0].message.uid, 4);
        assert!(
            repository
                .load_older_mailbox_page(
                    &account.account_id,
                    MailboxRole::Inbox,
                    starred_cursor,
                    1,
                    None,
                )
                .is_err()
        );

        let ordinary = repository
            .list_mailbox_page(&account.account_id, MailboxRole::Inbox, None, 1, None)
            .expect("ordinary page");
        let ordinary_cursor = ordinary.next_cursor.as_ref().expect("ordinary cursor");
        assert!(
            repository
                .load_older_starred_mailbox_page(
                    &account.account_id,
                    MailboxRole::Inbox,
                    ordinary_cursor,
                    1,
                    None,
                )
                .is_err()
        );

        assert!(
            repository
                .advance_starred_mailbox_history(
                    &account.account_id,
                    "INBOX",
                    91,
                    None,
                    Some(50),
                    false,
                )
                .expect("advance starred history")
        );
        assert_eq!(
            repository
                .mailbox_history(&account.account_id, "INBOX")
                .expect("ordinary history query")
                .expect("ordinary history row"),
            MailboxHistory {
                before_uid: Some(1),
                complete: true,
                remote_total: Some(6),
            }
        );
        assert_eq!(
            repository
                .starred_mailbox_history(&account.account_id, "INBOX")
                .expect("starred history query")
                .expect("starred history row"),
            StarredMailboxHistory {
                before_uid: Some(50),
                complete: false,
            }
        );
    }

    #[test]
    fn page_cursor_rejects_cross_account_folder_epoch_and_search_reuse() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            101,
        );
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Archive,
            "Archive",
            201,
        );
        repository
            .upsert_message(&message_with_identity(
                &account.account_id,
                "INBOX",
                9,
                "2026-07-21T10:00:00Z",
                "Needle",
            ))
            .expect("message");
        let first = repository
            .list_mailbox_page(
                &account.account_id,
                MailboxRole::Inbox,
                None,
                1,
                Some("Needle"),
            )
            .expect("first page");
        let cursor = first.next_cursor.as_ref().expect("remote continuation");
        assert!(uuid::Uuid::parse_str(cursor.as_str()).is_ok());
        assert!(!cursor.as_str().contains("Needle"));
        let mut tampered_token = cursor.as_str().to_owned();
        let replacement = if tampered_token.ends_with('0') {
            '1'
        } else {
            '0'
        };
        tampered_token.pop();
        tampered_token.push(replacement);
        assert!(
            repository
                .message_page_cursor_context(&MessagePageCursor::new(tampered_token))
                .is_err()
        );
        assert!(
            repository
                .message_page_cursor_context(&MessagePageCursor::new(
                    uuid::Uuid::now_v7().to_string(),
                ))
                .is_err()
        );
        let stored_query: String = repository
            .connection()
            .expect("connection")
            .query_row(
                "SELECT query_normalized FROM message_page_cursors WHERE token = ?1",
                params![cursor.as_str()],
                |row| row.get(0),
            )
            .expect("stored cursor binding");
        assert_eq!(stored_query, "needle");

        let other = secondary_account(&account);
        repository
            .initialize_account(&other)
            .expect("other account");
        initialize_mailbox(
            &repository,
            &other.account_id,
            MailboxRole::Inbox,
            "INBOX",
            101,
        );
        assert!(
            repository
                .list_mailbox_page(
                    &other.account_id,
                    MailboxRole::Inbox,
                    Some(cursor),
                    1,
                    Some("Needle"),
                )
                .is_err()
        );
        assert!(
            repository
                .list_mailbox_page(
                    &account.account_id,
                    MailboxRole::Archive,
                    Some(cursor),
                    1,
                    Some("Needle"),
                )
                .is_err()
        );
        assert!(
            repository
                .list_mailbox_page(
                    &account.account_id,
                    MailboxRole::Inbox,
                    Some(cursor),
                    1,
                    Some("Different"),
                )
                .is_err()
        );
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            102,
        );
        assert!(
            repository
                .list_mailbox_page(
                    &account.account_id,
                    MailboxRole::Inbox,
                    Some(cursor),
                    1,
                    Some("Needle"),
                )
                .is_err()
        );
    }

    #[test]
    fn local_search_uses_all_summary_identity_fields_but_never_body_text() {
        let (_directory, repository, account) = setup();
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            301,
        );
        let mut subject = message_with_identity(
            &account.account_id,
            "INBOX",
            1,
            "2026-07-21T11:00:01Z",
            "SubjectToken",
        );
        subject.sender = Some(MailAddress {
            name: Some("SenderToken".to_owned()),
            email: "sender@example.com".to_owned(),
        });
        subject.to = vec![MailAddress {
            name: Some("ToToken".to_owned()),
            email: "to@example.com".to_owned(),
        }];
        subject.cc = vec![MailAddress {
            name: Some("CcToken".to_owned()),
            email: "cc@example.com".to_owned(),
        }];
        subject.preview = "PreviewToken and 100% literal".to_owned();
        subject.body_text = Some("BodyOnlySecret".to_owned());
        subject.body_fetched = true;
        repository
            .upsert_message(&subject)
            .expect("searchable summary");
        repository
            .upsert_message(&message_with_identity(
                &account.account_id,
                "INBOX",
                2,
                "2026-07-21T11:00:02Z",
                "Unrelated",
            ))
            .expect("unrelated summary");

        for query in [
            "SubjectToken",
            "SenderToken",
            "ToToken",
            "CcToken",
            "PreviewToken",
            "100%",
        ] {
            let page = repository
                .list_mailbox_page(
                    &account.account_id,
                    MailboxRole::Inbox,
                    None,
                    1,
                    Some(query),
                )
                .expect("local search");
            assert_eq!(page.items.len(), 1, "query {query}");
            assert_eq!(page.items[0].message.uid, 1, "query {query}");
        }
        assert!(
            repository
                .list_mailbox_page(
                    &account.account_id,
                    MailboxRole::Inbox,
                    None,
                    50,
                    Some("BodyOnlySecret"),
                )
                .expect("body excluded")
                .items
                .is_empty()
        );
    }

    #[test]
    fn reply_parent_lookup_normalizes_message_id_brackets_and_case() {
        let (_directory, repository, account) = setup();
        let parent = message(&account.account_id, true);
        repository.upsert_message(&parent).expect("parent message");

        let found = repository
            .find_message_by_message_id(&account.account_id, "<MESSAGE-42@EXAMPLE.COM>")
            .expect("parent lookup")
            .expect("cached parent");

        assert_eq!(found.uid, parent.uid);
        assert_eq!(found.subject, parent.subject);
        assert!(found.raw_rfc822.is_empty());
    }

    #[test]
    fn inbox_summary_avoids_large_payloads_but_preserves_body_availability() {
        let (_directory, repository, account) = setup();
        let mut full = message(&account.account_id, true);
        full.body_html = Some("<table><tr><td>large HTML</td></tr></table>".repeat(500));
        full.raw_rfc822 = vec![b'x'; 256 * 1024];
        repository.upsert_message(&full).expect("full body");

        let summary = repository
            .list_inbox(&account.account_id, 10, 0)
            .expect("inbox summary")
            .pop()
            .expect("message");

        assert_eq!(summary.body_text.as_deref(), Some("Full body"));
        assert_eq!(summary.body_html.as_deref(), Some(""));
        assert!(summary.raw_rfc822.is_empty());
        assert!(summary.body_fetched);
    }

    #[test]
    fn preview_backfill_tracks_empty_and_resolved_summaries_without_fetching_bodies() {
        let (_directory, repository, account) = setup();
        let mut pending = message(&account.account_id, false);
        pending.preview.clear();
        repository
            .upsert_message(&pending)
            .expect("pending preview");

        assert_eq!(
            repository
                .mailbox_preview_backfill_candidates(&account.account_id, "INBOX", 10)
                .expect("preview candidates"),
            vec![pending.uid]
        );

        pending.preview = "Bounded synchronized preview".to_owned();
        repository
            .upsert_message_summary(&pending)
            .expect("resolved preview");
        assert!(
            repository
                .mailbox_preview_backfill_candidates(&account.account_id, "INBOX", 10)
                .expect("resolved candidates")
                .is_empty()
        );
        let stored = repository
            .get_message_by_uid(&account.account_id, "INBOX", pending.uid)
            .expect("stored preview");
        assert_eq!(stored.preview, "Bounded synchronized preview");
        assert_eq!(stored.body_text, None);
        assert!(!stored.body_fetched);

        pending.preview.clear();
        repository
            .upsert_message_summary(&pending)
            .expect("resolved empty preview");
        assert!(
            repository
                .mailbox_preview_backfill_candidates(&account.account_id, "INBOX", 10)
                .expect("empty preview candidates")
                .is_empty()
        );
    }

    #[test]
    fn unresolved_header_refresh_does_not_erase_a_cached_preview() {
        let (_directory, repository, account) = setup();
        let mut full = message(&account.account_id, true);
        full.preview = "Canonical body preview".to_owned();
        repository.upsert_message(&full).expect("full message");

        let mut header_only = message(&account.account_id, false);
        header_only.preview.clear();
        repository
            .upsert_message(&header_only)
            .expect("header refresh");

        assert_eq!(
            repository
                .get_message_by_uid(&account.account_id, "INBOX", full.uid)
                .expect("preserved message")
                .preview,
            "Canonical body preview"
        );
    }

    #[test]
    fn body_prefetch_candidates_are_recent_unfetched_messages_within_size_limit() {
        let (_directory, repository, account) = setup();
        let pending = message(&account.account_id, false);
        repository.upsert_message(&pending).expect("pending body");

        assert_eq!(
            repository
                .mailbox_body_prefetch_candidates(&account.account_id, "INBOX", 10, 1024)
                .expect("candidates"),
            vec![(42, 321)]
        );
        assert!(
            repository
                .mailbox_body_prefetch_candidates(&account.account_id, "INBOX", 10, 100)
                .expect("size-filtered candidates")
                .is_empty()
        );
    }

    #[test]
    fn body_cache_lru_evicts_oldest_body_but_preserves_its_summary() {
        let (_directory, repository, account) = setup();
        let mut oldest = message(&account.account_id, true);
        oldest.raw_rfc822 = vec![b'a'; 128];
        oldest.preview = "Old preview remains".to_owned();
        repository.upsert_message(&oldest).expect("oldest body");

        let mut newest = message(&account.account_id, true);
        newest.uid = 43;
        newest.message_id = Some("message-43@example.com".to_owned());
        newest.raw_rfc822 = vec![b'b'; 256];
        repository.upsert_message(&newest).expect("newest body");

        let connection = repository.connection().expect("cache timestamps");
        connection
            .execute(
                "UPDATE messages
                 SET body_last_accessed_at = CASE uid
                     WHEN 42 THEN '2026-07-01T00:00:00.000Z'
                     ELSE '2026-07-02T00:00:00.000Z'
                 END
                 WHERE account_id = ?1",
                params![account.account_id],
            )
            .expect("ordered cache timestamps");
        let newest_cached_bytes = u64::try_from(
            connection
                .query_row(
                    "SELECT body_cached_bytes FROM messages
                 WHERE account_id = ?1 AND uid = 43",
                    params![account.account_id],
                    |row| row.get::<_, i64>(0),
                )
                .expect("newest cached bytes"),
        )
        .expect("non-negative newest cached bytes");
        drop(connection);

        assert_eq!(
            repository
                .evict_message_body_cache_to_limit(
                    &account.account_id,
                    newest_cached_bytes,
                    Some(
                        repository
                            .get_message_by_uid(&account.account_id, "INBOX", 43)
                            .expect("protected newest")
                            .id,
                    ),
                )
                .expect("eviction"),
            1
        );
        let evicted = repository
            .get_message_by_uid(&account.account_id, "INBOX", 42)
            .expect("evicted summary");
        assert!(!evicted.body_fetched);
        assert!(evicted.raw_rfc822.is_empty());
        assert_eq!(evicted.body_text, None);
        assert_eq!(evicted.preview, "Old preview remains");
        assert!(
            repository
                .get_message_by_uid(&account.account_id, "INBOX", 43)
                .expect("retained body")
                .body_fetched
        );
        assert!(
            repository
                .message_body_cache_usage_bytes(&account.account_id)
                .expect("cache usage")
                <= newest_cached_bytes
        );
    }

    #[test]
    fn touching_a_cached_body_updates_its_lru_timestamp() {
        let (_directory, repository, account) = setup();
        let cached = message(&account.account_id, true);
        repository.upsert_message(&cached).expect("cached body");
        let stored = repository
            .get_message_by_uid(&account.account_id, "INBOX", cached.uid)
            .expect("stored body");
        let connection = repository.connection().expect("old timestamp");
        connection
            .execute(
                "UPDATE messages
                 SET body_last_accessed_at = '2000-01-01T00:00:00.000Z'
                 WHERE id = ?1",
                params![stored.id],
            )
            .expect("set old timestamp");
        drop(connection);

        repository
            .touch_message_body_access(stored.id)
            .expect("touch body");
        let connection = repository.connection().expect("new timestamp");
        let touched: String = connection
            .query_row(
                "SELECT body_last_accessed_at FROM messages WHERE id = ?1",
                params![stored.id],
                |row| row.get(0),
            )
            .expect("touched timestamp");
        assert!(touched.as_str() > "2000-01-01T00:00:00.000Z");
    }

    #[test]
    fn mailbox_cursor_and_missing_uid_cleanup_round_trip() {
        let (_directory, repository, account) = setup();
        repository
            .upsert_message(&message(&account.account_id, false))
            .unwrap();
        let state = MailboxState {
            account_id: account.account_id.clone(),
            mailbox: "INBOX".to_owned(),
            uid_validity: Some(9),
            uid_next: Some(100),
            highest_uid: Some(99),
            highest_modseq: Some(1234),
            last_synced_at: Some("2026-07-14T02:00:00Z".to_owned()),
        };
        repository.upsert_mailbox_state(&state).unwrap();
        assert_eq!(
            repository
                .mailbox_state(&account.account_id, "INBOX")
                .unwrap(),
            Some(state)
        );
        assert_eq!(
            repository
                .delete_missing_uids(&account.account_id, "INBOX", &HashSet::new())
                .unwrap(),
            1
        );
    }

    #[test]
    fn drafts_and_outbox_survive_state_transitions() {
        let (_directory, repository, account) = setup();
        let draft = Draft {
            id: "draft-1".to_owned(),
            local_version: 3,
            has_unsupported_content: false,
            account_id: account.account_id.clone(),
            to: vec!["receiver@example.com".to_owned()],
            cc: vec![],
            bcc: vec![],
            subject: "Draft".to_owned(),
            body_text: "Body".to_owned(),
            format: ComposeFormat {
                body_html: Some("<p><strong>Body</strong></p>".to_owned()),
                stationery: StationeryTheme::Grid,
                send_stationery: true,
            },
            reply_context: None,
            status: "local".to_owned(),
            remote_mailbox: None,
            remote_uid: None,
            created_at: "2026-07-14T03:00:00Z".to_owned(),
            updated_at: "2026-07-14T03:00:00Z".to_owned(),
            raw_rfc822: b"draft raw".to_vec(),
        };
        repository
            .save_draft_record(&DraftRecord {
                draft: draft.clone(),
                local_version: 3,
                revision: 3,
                synced_revision: 0,
                remote_uid_validity: None,
                is_deleted: false,
            })
            .expect("save draft");
        let unsynced = repository.get_draft_record("draft-1").unwrap();
        assert_eq!(unsynced.draft.format, draft.format);
        assert!(
            repository
                .mark_draft_record_synced_if_unchanged(&unsynced, "Drafts", Some(7), Some(91))
                .unwrap()
        );
        let synced = repository.get_draft_record("draft-1").unwrap();
        assert_eq!(synced.draft.remote_uid, Some(7));
        assert_eq!(synced.revision, 3);
        assert_eq!(synced.synced_revision, 3);
        assert_eq!(synced.remote_uid_validity, Some(91));

        let item = OutboxItem {
            id: "outbox-1".to_owned(),
            account_id: account.account_id.clone(),
            draft_id: Some(draft.id.clone()),
            draft_revision: Some(3),
            draft_local_version: Some(draft.local_version),
            recipients: draft.to.clone(),
            recipient_groups: None,
            status: OutboxStatus::Queued,
            attempts: 0,
            last_error: None,
            created_at: "2026-07-14T03:01:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: b"outgoing raw".to_vec(),
        };
        repository.enqueue_outbox(&item).expect("enqueue");
        repository
            .update_outbox_status("outbox-1", OutboxStatus::Sending, None)
            .unwrap();
        assert_eq!(repository.recover_sending_as_delivery_unknown().unwrap(), 1);
        let recovered = repository.get_outbox("outbox-1").unwrap();
        assert_eq!(recovered.status, OutboxStatus::DeliveryUnknown);
        assert_eq!(recovered.attempts, 1);
        let duplicate_for_same_draft = OutboxItem {
            id: "outbox-2".to_owned(),
            ..item.clone()
        };
        assert!(
            repository
                .enqueue_outbox(&duplicate_for_same_draft)
                .is_err()
        );
        repository.finalize_outbox_sent("outbox-1").unwrap();
        assert_eq!(
            repository.get_outbox("outbox-1").unwrap().status,
            OutboxStatus::Sent
        );
        assert_eq!(repository.get_draft("draft-1").unwrap().status, "sent");
        assert_eq!(
            repository.list_outbox(&account.account_id).unwrap().len(),
            0
        );
        assert_eq!(
            repository
                .list_sent_outbox_fallbacks(&account.account_id)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn exact_cached_sent_message_retires_sent_and_unknown_outbox_rows() {
        let (_directory, repository, account) = setup();
        let base = OutboxItem {
            id: "sent-fallback".to_owned(),
            account_id: account.account_id.clone(),
            draft_id: None,
            draft_revision: None,
            draft_local_version: None,
            recipients: vec!["receiver@example.com".to_owned()],
            recipient_groups: None,
            status: OutboxStatus::Queued,
            attempts: 0,
            last_error: None,
            created_at: "2026-07-14T03:01:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: b"Message-ID: <same-message@example.com>\r\nSubject: Exact\r\n\r\nBody"
                .to_vec(),
        };
        repository.enqueue_outbox(&base).expect("enqueue fallback");
        repository
            .finalize_outbox_sent(&base.id)
            .expect("mark sent fallback");

        assert!(
            !repository
                .reconcile_outbox_with_cached_sent(
                    &base.id,
                    &account.account_id,
                    "Sent",
                    "<same-message@example.com>",
                )
                .expect("no cached sent match")
        );

        let mut cached = message(&account.account_id, false);
        cached.mailbox = "Sent".to_owned();
        cached.uid = 77;
        cached.message_id = Some("SAME-MESSAGE@EXAMPLE.COM".to_owned());
        repository
            .upsert_message(&cached)
            .expect("cached Sent copy");
        assert!(
            repository
                .reconcile_outbox_with_cached_sent(
                    &base.id,
                    &account.account_id,
                    "Sent",
                    "<same-message@example.com>",
                )
                .expect("retire sent fallback")
        );
        assert!(repository.get_outbox(&base.id).is_err());

        let unknown = OutboxItem {
            id: "unknown-fallback".to_owned(),
            raw_rfc822: b"Message-ID: <same-message@example.com>\r\nSubject: Exact\r\n\r\nBody"
                .to_vec(),
            ..base
        };
        repository
            .enqueue_outbox(&unknown)
            .expect("enqueue unknown");
        repository
            .update_outbox_status(&unknown.id, OutboxStatus::Sending, None)
            .expect("claim unknown");
        repository
            .recover_sending_as_delivery_unknown()
            .expect("recover unknown");
        assert!(
            repository
                .reconcile_outbox_with_cached_sent(
                    &unknown.id,
                    &account.account_id,
                    "Sent",
                    "same-message@example.com",
                )
                .expect("resolve unknown from exact Sent copy")
        );
        assert!(repository.get_outbox(&unknown.id).is_err());
    }

    #[test]
    fn stale_remote_pull_cannot_overwrite_a_concurrent_local_edit_or_create_conflict_copy() {
        let (_directory, first, account) = setup();
        let second = Repository::open(&first.path).expect("second repository connection");
        let base = draft_record(&account.account_id, "shared-draft", "base", 1, 1);
        first.save_draft_record(&base).expect("base draft");
        let sync_snapshot = first
            .get_draft_record(&base.draft.id)
            .expect("sync snapshot");

        let mut concurrent_edit = sync_snapshot.clone();
        concurrent_edit.local_version = 2;
        concurrent_edit.draft.local_version = 2;
        concurrent_edit.revision = 2;
        concurrent_edit.draft.status = "local".to_owned();
        concurrent_edit.draft.subject = "new local edit".to_owned();
        concurrent_edit.draft.raw_rfc822 = b"new local bytes".to_vec();
        second
            .save_draft_record(&concurrent_edit)
            .expect("concurrent edit");

        let mut stale_remote = sync_snapshot.clone();
        stale_remote.revision = 2;
        stale_remote.synced_revision = 2;
        stale_remote.draft.subject = "remote replacement".to_owned();
        stale_remote.draft.raw_rfc822 = b"remote bytes".to_vec();
        let conflict_copy = draft_record(
            &account.account_id,
            "conflict-copy",
            "stale conflict copy",
            1,
            0,
        );

        assert!(
            !first
                .replace_draft_if_unchanged(&sync_snapshot, &stale_remote, Some(&conflict_copy))
                .expect("CAS replacement")
        );
        let preserved = first.get_draft_record(&base.draft.id).unwrap();
        assert_eq!(preserved.revision, 2);
        assert_eq!(preserved.synced_revision, 1);
        assert_eq!(preserved.draft.subject, "new local edit");
        assert_eq!(preserved.draft.raw_rfc822, b"new local bytes");
        assert!(matches!(
            first.get_draft_record(&conflict_copy.draft.id),
            Err(MailError::NotFound { .. })
        ));
    }

    #[test]
    fn test_helper_rejects_rewriting_an_immutable_draft_version_snapshot() {
        let (_directory, repository, account) = setup();
        let original = draft_record(
            &account.account_id,
            "immutable-test-snapshot",
            "Original",
            1,
            0,
        );
        repository
            .save_draft_record(&original)
            .expect("initial immutable snapshot");

        let mut idempotent = original.clone();
        idempotent.draft.updated_at = "2026-07-28T01:00:00Z".to_owned();
        idempotent.draft.raw_rfc822 = b"same compose state, refreshed bytes".to_vec();
        repository
            .save_draft_record(&idempotent)
            .expect("identical compose snapshot is idempotent");

        let mut rewritten = idempotent.clone();
        rewritten.draft.subject = "Different compose content".to_owned();
        assert!(matches!(
            repository.save_draft_record(&rewritten),
            Err(MailError::Validation(_))
        ));
        assert_eq!(
            repository
                .draft_version_snapshot(&account.account_id, &original.draft.id, 1)
                .unwrap()
                .unwrap()
                .request
                .subject,
            "Original"
        );
    }

    #[test]
    fn stale_push_delete_and_remote_import_results_preserve_concurrent_local_state() {
        let (_directory, first, account) = setup();
        let second = Repository::open(&first.path).expect("second repository connection");
        let base = draft_record(&account.account_id, "push-draft", "first edit", 1, 0);
        first.save_draft_record(&base).expect("base draft");
        let sync_snapshot = first
            .get_draft_record(&base.draft.id)
            .expect("sync snapshot");

        let mut concurrent_edit = sync_snapshot.clone();
        concurrent_edit.local_version = 2;
        concurrent_edit.draft.local_version = 2;
        concurrent_edit.revision = 2;
        concurrent_edit.draft.subject = "second edit".to_owned();
        concurrent_edit.draft.raw_rfc822 = b"second edit bytes".to_vec();
        second
            .save_draft_record(&concurrent_edit)
            .expect("concurrent edit");

        assert!(
            !first
                .mark_draft_record_synced_if_unchanged(&sync_snapshot, "Drafts", Some(22), Some(91))
                .expect("stale push CAS")
        );
        assert!(
            !first
                .delete_draft_if_unchanged(&sync_snapshot)
                .expect("stale delete CAS")
        );
        let preserved = first.get_draft_record(&base.draft.id).unwrap();
        assert_eq!(preserved.revision, 2);
        assert_eq!(preserved.synced_revision, 0);
        assert_eq!(preserved.draft.status, "local");
        assert_eq!(preserved.draft.remote_uid, None);

        let local_collision = draft_record(
            &account.account_id,
            "remote-import-id",
            "locally created",
            1,
            0,
        );
        second
            .save_draft_record(&local_collision)
            .expect("concurrent local create");
        let mut remote_collision = local_collision.clone();
        remote_collision.draft.subject = "remote import".to_owned();
        remote_collision.draft.status = "synced".to_owned();
        remote_collision.synced_revision = 1;
        assert!(
            !first
                .insert_draft_if_absent(&remote_collision)
                .expect("remote import CAS")
        );
        assert_eq!(
            first
                .get_draft_record(&local_collision.draft.id)
                .unwrap()
                .draft
                .subject,
            "locally created"
        );
    }

    #[test]
    fn manual_retry_claim_is_status_gated_and_increments_one_attempt() {
        let (_directory, repository, account) = setup();
        let retryable = OutboxItem {
            id: "retryable-outbox".to_owned(),
            account_id: account.account_id.clone(),
            draft_id: None,
            draft_revision: None,
            draft_local_version: None,
            recipients: vec!["receiver@example.com".to_owned()],
            recipient_groups: None,
            status: OutboxStatus::Retryable,
            attempts: 1,
            last_error: Some("temporary SMTP response".to_owned()),
            created_at: "2026-07-14T04:00:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: b"From: sender@example.com\r\nTo: receiver@example.com\r\n\r\nBody"
                .to_vec(),
        };
        repository.enqueue_outbox(&retryable).expect("enqueue");

        let claimed = repository
            .claim_retryable_outbox(&retryable.id, &account.account_id)
            .expect("claim retryable");
        assert_eq!(claimed.status, OutboxStatus::Sending);
        assert_eq!(claimed.attempts, 2);
        assert_eq!(claimed.last_error, None);

        let second_claim = repository.claim_retryable_outbox(&retryable.id, &account.account_id);
        assert!(matches!(second_claim, Err(crate::MailError::Validation(_))));
        assert_eq!(repository.get_outbox(&retryable.id).unwrap().attempts, 2);

        for (index, status) in [
            OutboxStatus::Queued,
            OutboxStatus::Sending,
            OutboxStatus::Sent,
            OutboxStatus::Rejected,
            OutboxStatus::DeliveryUnknown,
        ]
        .into_iter()
        .enumerate()
        {
            let item = OutboxItem {
                id: format!("not-retryable-{index}"),
                status,
                attempts: 7,
                ..retryable.clone()
            };
            repository.enqueue_outbox(&item).expect("enqueue status");
            let result = repository.claim_retryable_outbox(&item.id, &account.account_id);
            assert!(matches!(result, Err(crate::MailError::Validation(_))));
            assert_eq!(repository.get_outbox(&item.id).unwrap().attempts, 7);
        }
    }

    #[test]
    fn delivery_unknown_retry_is_bound_to_one_reviewed_attempt_generation() {
        let (_directory, repository, account) = setup();
        let secondary = secondary_account(&account);
        repository
            .initialize_account(&secondary)
            .expect("secondary account");
        let unknown = OutboxItem {
            id: "ambiguous-outbox".to_owned(),
            account_id: account.account_id.clone(),
            draft_id: None,
            draft_revision: None,
            draft_local_version: None,
            recipients: vec![
                "receiver@example.com".to_owned(),
                "hidden@example.com".to_owned(),
            ],
            recipient_groups: Some(OutboxRecipientGroups {
                to: vec!["receiver@example.com".to_owned()],
                cc: Vec::new(),
                bcc: vec!["hidden@example.com".to_owned()],
            }),
            status: OutboxStatus::DeliveryUnknown,
            attempts: 1,
            last_error: Some("SMTP delivery state is unknown".to_owned()),
            created_at: "2026-07-14T04:00:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: b"From: sender@example.com\r\nTo: receiver@example.com\r\n\r\nExact body"
                .to_vec(),
        };
        repository.enqueue_outbox(&unknown).expect("unknown item");

        assert!(matches!(
            repository.claim_delivery_unknown_retry(&unknown.id, &secondary.account_id, 1),
            Err(MailError::NotFound { .. })
        ));
        assert!(matches!(
            repository.claim_delivery_unknown_retry(&unknown.id, &account.account_id, 0),
            Err(MailError::Validation(_))
        ));

        let claimed = repository
            .claim_delivery_unknown_retry(&unknown.id, &account.account_id, 1)
            .expect("claim reviewed generation");
        assert_eq!(claimed.status, OutboxStatus::Sending);
        assert_eq!(claimed.attempts, 2);
        assert_eq!(claimed.raw_rfc822, unknown.raw_rfc822);
        assert_eq!(claimed.recipients, unknown.recipients);
        assert_eq!(claimed.recipient_groups, unknown.recipient_groups);

        repository
            .complete_claimed_outbox_failure(
                &unknown.id,
                &account.account_id,
                claimed.attempts,
                OutboxStatus::DeliveryUnknown,
                "second ambiguous SMTP outcome",
            )
            .expect("new ambiguous outcome");
        assert!(matches!(
            repository.claim_delivery_unknown_retry(&unknown.id, &account.account_id, 1),
            Err(MailError::Validation(_))
        ));
        let still_unknown = repository.get_outbox(&unknown.id).unwrap();
        assert_eq!(still_unknown.status, OutboxStatus::DeliveryUnknown);
        assert_eq!(still_unknown.attempts, 2);
        assert_eq!(still_unknown.raw_rfc822, unknown.raw_rfc822);

        let next_claim = repository
            .claim_delivery_unknown_retry(&unknown.id, &account.account_id, 2)
            .expect("new explicit decision");
        assert_eq!(next_claim.status, OutboxStatus::Sending);
        assert_eq!(next_claim.attempts, 3);
        assert_eq!(next_claim.raw_rfc822, unknown.raw_rfc822);
        assert_eq!(next_claim.recipients, unknown.recipients);
        assert!(matches!(
            repository.complete_claimed_outbox_failure(
                &unknown.id,
                &account.account_id,
                claimed.attempts,
                OutboxStatus::DeliveryUnknown,
                "late result from the older attempt"
            ),
            Err(MailError::Validation(_))
        ));
        assert_eq!(
            repository.get_outbox(&unknown.id).unwrap().status,
            OutboxStatus::Sending
        );
        assert_eq!(repository.get_outbox(&unknown.id).unwrap().attempts, 3);

        repository
            .finalize_claimed_outbox_sent(&unknown.id, &account.account_id, next_claim.attempts)
            .expect("finish current exact attempt");
        let sent = repository.get_outbox(&unknown.id).unwrap();
        assert_eq!(sent.status, OutboxStatus::Sent);
        assert_eq!(sent.attempts, 3);
        assert_eq!(sent.raw_rfc822, unknown.raw_rfc822);
        assert_eq!(sent.recipients, unknown.recipients);
    }

    #[test]
    fn confirming_delivery_unknown_is_atomic_account_scoped_and_single_transition() {
        let (_directory, first, account) = setup();
        let database_path = first.path.clone();
        let second = Repository::open(&first.path).expect("second connection");
        let secondary = secondary_account(&account);
        first
            .initialize_account(&secondary)
            .expect("secondary account");
        let draft = draft_record(
            &account.account_id,
            "ambiguous-draft",
            "Exact ambiguous draft",
            1,
            0,
        );
        first.save_draft_record(&draft).expect("draft");
        let mut unknown = linked_outbox(
            &draft,
            "confirmed-ambiguous-outbox",
            OutboxStatus::DeliveryUnknown,
            1,
        );
        unknown.last_error = Some("SMTP delivery state is unknown".to_owned());
        let immutable_bytes = unknown.raw_rfc822.clone();
        let immutable_recipients = unknown.recipients.clone();
        first.enqueue_outbox(&unknown).expect("unknown Outbox");

        assert!(matches!(
            first.confirm_delivery_unknown_as_sent(&unknown.id, &secondary.account_id, 1),
            Err(MailError::NotFound { .. })
        ));
        assert_eq!(
            first.get_outbox(&unknown.id).unwrap().status,
            OutboxStatus::DeliveryUnknown
        );

        let barrier = Arc::new(Barrier::new(2));
        let first_barrier = Arc::clone(&barrier);
        let second_barrier = Arc::clone(&barrier);
        let first_id = unknown.id.clone();
        let second_id = unknown.id.clone();
        let first_account_id = account.account_id.clone();
        let second_account_id = account.account_id.clone();
        let first_confirmation = thread::spawn(move || {
            first_barrier.wait();
            first.confirm_delivery_unknown_as_sent(&first_id, &first_account_id, 1)
        });
        let second_confirmation = thread::spawn(move || {
            second_barrier.wait();
            second.confirm_delivery_unknown_as_sent(&second_id, &second_account_id, 1)
        });
        let results = [
            first_confirmation.join().expect("first thread"),
            second_confirmation.join().expect("second thread"),
        ];
        let successful = results
            .iter()
            .filter(|result| {
                result.as_ref().is_ok_and(|item| {
                    item.status == OutboxStatus::Sent
                        && item.attempts == 1
                        && item.raw_rfc822 == immutable_bytes
                        && item.recipients == immutable_recipients
                })
            })
            .count();
        let rejected = results
            .iter()
            .filter(|result| matches!(result, Err(MailError::Validation(_))))
            .count();
        assert_eq!(successful, 1);
        assert_eq!(rejected, 1);

        let inspector = Repository::open(database_path).expect("inspector");
        let sent = inspector.get_outbox(&unknown.id).expect("sent Outbox");
        assert_eq!(sent.status, OutboxStatus::Sent);
        assert_eq!(sent.attempts, 1);
        assert_eq!(
            sent.sent_at, None,
            "the user's decision time is not the unknown SMTP delivery time"
        );
        assert_eq!(sent.raw_rfc822, immutable_bytes);
        assert_eq!(sent.recipients, immutable_recipients);
        assert_eq!(inspector.get_draft(&draft.draft.id).unwrap().status, "sent");
        assert!(matches!(
            inspector.claim_delivery_unknown_retry(&unknown.id, &account.account_id, 1),
            Err(MailError::Validation(_))
        ));
        assert!(matches!(
            inspector.confirm_delivery_unknown_as_sent(&unknown.id, &account.account_id, 2),
            Err(MailError::Validation(_))
        ));
        assert_eq!(
            inspector.get_outbox(&unknown.id).unwrap().attempts,
            1,
            "stale decisions must not mutate the confirmed item"
        );
    }

    #[test]
    fn first_outbox_attempt_is_atomically_persisted_and_claimed_once() {
        let (_directory, first, account) = setup();
        let database_path = first.path.clone();
        let second = Repository::open(&first.path).expect("second connection");
        let recovery = Repository::open(&first.path).expect("recovery connection");
        let item = OutboxItem {
            id: "atomic-first-attempt".to_owned(),
            account_id: account.account_id.clone(),
            draft_id: None,
            draft_revision: None,
            draft_local_version: None,
            recipients: vec!["receiver@example.com".to_owned()],
            recipient_groups: Some(OutboxRecipientGroups {
                to: vec!["receiver@example.com".to_owned()],
                cc: Vec::new(),
                bcc: Vec::new(),
            }),
            status: OutboxStatus::Queued,
            attempts: 0,
            last_error: None,
            created_at: "2026-07-14T07:00:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: b"exact first-attempt bytes".to_vec(),
        };
        let barrier = Arc::new(Barrier::new(3));
        let first_barrier = Arc::clone(&barrier);
        let second_barrier = Arc::clone(&barrier);
        let recovery_barrier = Arc::clone(&barrier);
        let first_item = item.clone();
        let second_item = item.clone();
        let first_claim = thread::spawn(move || {
            first_barrier.wait();
            first.enqueue_and_claim_outbox(&first_item)
        });
        let second_claim = thread::spawn(move || {
            second_barrier.wait();
            second.enqueue_and_claim_outbox(&second_item)
        });
        let startup_recovery = thread::spawn(move || {
            recovery_barrier.wait();
            recovery.recover_queued_as_retryable()
        });
        let outcomes = [
            first_claim.join().expect("first claimant"),
            second_claim.join().expect("second claimant"),
        ];
        assert_eq!(
            startup_recovery
                .join()
                .expect("startup recovery thread")
                .expect("startup recovery"),
            0
        );
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.is_err()).count(),
            1
        );

        let inspector = Repository::open(&database_path).expect("inspector");
        let claimed = inspector.get_outbox(&item.id).expect("claimed row");
        assert_eq!(claimed.status, OutboxStatus::Sending);
        assert_eq!(claimed.attempts, 1);
        assert_eq!(claimed.raw_rfc822, item.raw_rfc822);
        assert_eq!(inspector.recover_queued_as_retryable().unwrap(), 0);
        assert_eq!(
            inspector.get_outbox(&item.id).unwrap().status,
            OutboxStatus::Sending
        );
    }

    #[test]
    fn outbox_recipient_groups_survive_restart_recovery_and_retry_without_legacy_inference() {
        let (directory, repository, account) = setup();
        let database_path = repository.path.clone();
        let groups = OutboxRecipientGroups {
            to: vec!["To Person <to@example.com>".to_owned()],
            cc: vec!["Copy Person <copy@example.com>".to_owned()],
            bcc: vec!["Blind Person <blind@example.com>".to_owned()],
        };
        let queued = OutboxItem {
            id: "grouped-first-attempt".to_owned(),
            account_id: account.account_id.clone(),
            draft_id: None,
            draft_revision: None,
            draft_local_version: None,
            recipients: vec![
                "to@example.com".to_owned(),
                "copy@example.com".to_owned(),
                "blind@example.com".to_owned(),
            ],
            recipient_groups: Some(groups.clone()),
            status: OutboxStatus::Queued,
            attempts: 0,
            last_error: None,
            created_at: "2026-07-28T03:00:00Z".to_owned(),
            sent_at: None,
            raw_rfc822:
                b"From: sender@example.com\r\nTo: to@example.com\r\nCc: copy@example.com\r\n\r\nBody"
                    .to_vec(),
        };
        let claimed = repository
            .enqueue_and_claim_outbox(&queued)
            .expect("first grouped claim");
        assert_eq!(claimed.recipient_groups.as_ref(), Some(&groups));

        let retryable = OutboxItem {
            id: "grouped-retry".to_owned(),
            status: OutboxStatus::Retryable,
            attempts: 1,
            last_error: Some("temporary SMTP failure".to_owned()),
            created_at: "2026-07-28T03:01:00Z".to_owned(),
            ..queued.clone()
        };
        repository
            .enqueue_new_outbox(&retryable)
            .expect("persist grouped retry");

        let legacy = OutboxItem {
            id: "legacy-ungrouped".to_owned(),
            recipient_groups: None,
            status: OutboxStatus::Retryable,
            attempts: 1,
            raw_rfc822: b"From: sender@example.com\r\nTo: to@example.com\r\nBcc: blind@example.com\r\n\r\nLegacy"
                .to_vec(),
            created_at: "2026-07-28T03:02:00Z".to_owned(),
            ..queued.clone()
        };
        assert!(
            repository
                .enqueue_new_outbox(&legacy)
                .expect_err("new rows must never omit recipient grouping")
                .to_string()
                .contains("exact To, Cc and Bcc")
        );
        repository
            .enqueue_outbox(&legacy)
            .expect("legacy ungrouped row");
        drop(repository);

        let reopened = Repository::open(&database_path).expect("restart repository");
        assert_eq!(reopened.recover_sending_as_delivery_unknown().unwrap(), 1);
        let recovered = reopened
            .get_outbox(&queued.id)
            .expect("recovered first send");
        assert_eq!(recovered.status, OutboxStatus::DeliveryUnknown);
        assert_eq!(recovered.recipient_groups.as_ref(), Some(&groups));
        let retried = reopened
            .claim_retryable_outbox(&retryable.id, &account.account_id)
            .expect("claim persisted retry");
        assert_eq!(retried.recipient_groups.as_ref(), Some(&groups));
        assert_eq!(
            reopened
                .get_outbox(&legacy.id)
                .expect("legacy row after restart")
                .recipient_groups,
            None
        );

        let connection = reopened.connection().expect("immutable payload check");
        assert!(
            connection
                .execute(
                    "UPDATE outbox SET recipient_groups_json = '{}' WHERE id = ?1",
                    params![queued.id],
                )
                .is_err()
        );
        drop(connection);
        drop(reopened);
        drop(directory);
    }

    #[test]
    fn linked_outbox_groups_must_match_the_exact_immutable_draft_snapshot() {
        let (_directory, repository, account) = setup();
        let mut version_one =
            draft_record(&account.account_id, "grouped-draft", "Grouped V1", 1, 0);
        version_one.draft.to = vec!["to@example.com".to_owned()];
        version_one.draft.cc = vec!["copy@example.com".to_owned()];
        version_one.draft.bcc = vec!["blind@example.com".to_owned()];
        repository
            .save_draft_record(&version_one)
            .expect("version one");

        let exact = OutboxItem {
            id: "grouped-draft-exact".to_owned(),
            account_id: account.account_id.clone(),
            draft_id: Some(version_one.draft.id.clone()),
            draft_revision: Some(version_one.revision),
            draft_local_version: Some(version_one.local_version),
            recipients: vec![
                "to@example.com".to_owned(),
                "copy@example.com".to_owned(),
                "blind@example.com".to_owned(),
            ],
            recipient_groups: Some(OutboxRecipientGroups::from(
                &version_one.draft.compose_request(),
            )),
            status: OutboxStatus::Queued,
            attempts: 0,
            last_error: None,
            created_at: "2026-07-28T04:00:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: b"immutable grouped bytes".to_vec(),
        };
        let mismatched = OutboxItem {
            id: "grouped-draft-mismatch".to_owned(),
            recipient_groups: Some(OutboxRecipientGroups {
                to: vec!["to@example.com".to_owned()],
                cc: Vec::new(),
                bcc: vec![
                    "copy@example.com".to_owned(),
                    "blind@example.com".to_owned(),
                ],
            }),
            ..exact.clone()
        };
        assert!(
            repository
                .enqueue_and_claim_outbox(&mismatched)
                .expect_err("same envelope with changed grouping must be rejected")
                .to_string()
                .contains("confirmed draft version")
        );
        assert!(matches!(
            repository.get_outbox(&mismatched.id),
            Err(MailError::NotFound { .. })
        ));

        let claimed = repository
            .enqueue_and_claim_outbox(&exact)
            .expect("exact grouped draft claim");
        assert_eq!(claimed.recipient_groups, exact.recipient_groups);

        let mut version_two = version_one.clone();
        version_two.local_version = 2;
        version_two.revision = 2;
        version_two.draft.local_version = 2;
        version_two.draft.to = vec!["new-to@example.com".to_owned()];
        version_two.draft.cc.clear();
        version_two.draft.bcc.clear();
        version_two.draft.subject = "Grouped V2".to_owned();
        repository
            .save_draft_record(&version_two)
            .expect("newer draft edit");
        assert_eq!(
            repository
                .get_outbox(&exact.id)
                .expect("immutable V1 Outbox")
                .recipient_groups,
            exact.recipient_groups
        );
    }

    #[test]
    fn successful_retry_atomically_marks_outbox_and_linked_draft_sent() {
        let (_directory, repository, account) = setup();
        let draft = Draft {
            id: "retry-draft".to_owned(),
            local_version: 1,
            has_unsupported_content: false,
            account_id: account.account_id.clone(),
            to: vec!["receiver@example.com".to_owned()],
            cc: vec![],
            bcc: vec![],
            subject: "Retry draft".to_owned(),
            body_text: "Exact persisted body".to_owned(),
            format: Default::default(),
            reply_context: None,
            status: "local".to_owned(),
            remote_mailbox: None,
            remote_uid: None,
            created_at: "2026-07-14T05:00:00Z".to_owned(),
            updated_at: "2026-07-14T05:00:00Z".to_owned(),
            raw_rfc822: b"draft bytes that must not be sent".to_vec(),
        };
        repository
            .save_draft_record(&DraftRecord {
                draft: draft.clone(),
                local_version: 1,
                revision: 1,
                synced_revision: 0,
                remote_uid_validity: None,
                is_deleted: false,
            })
            .expect("draft");
        let outbox = OutboxItem {
            id: "retry-outbox-with-draft".to_owned(),
            account_id: account.account_id.clone(),
            draft_id: Some(draft.id.clone()),
            draft_revision: Some(1),
            draft_local_version: Some(draft.local_version),
            recipients: draft.to.clone(),
            recipient_groups: None,
            status: OutboxStatus::Retryable,
            attempts: 1,
            last_error: Some("temporary SMTP response".to_owned()),
            created_at: "2026-07-14T05:01:00Z".to_owned(),
            sent_at: None,
            raw_rfc822: b"exact persisted outgoing bytes".to_vec(),
        };
        repository.enqueue_outbox(&outbox).expect("outbox");
        let claimed = repository
            .claim_retryable_outbox(&outbox.id, &account.account_id)
            .expect("claim");

        repository
            .finalize_outbox_sent(&claimed.id)
            .expect("successful delivery transition");

        let sent = repository.get_outbox(&outbox.id).unwrap();
        assert_eq!(sent.status, OutboxStatus::Sent);
        assert_eq!(sent.attempts, 2);
        assert!(sent.sent_at.is_some());
        assert_eq!(sent.raw_rfc822, outbox.raw_rfc822);
        assert_eq!(sent.draft_id.as_deref(), Some(draft.id.as_str()));
        assert_eq!(sent.draft_local_version, Some(draft.local_version));
        assert_eq!(repository.get_draft(&draft.id).unwrap().status, "sent");
    }

    #[test]
    fn first_attempt_success_preserves_a_newer_draft_and_allows_its_send() {
        let (_directory, repository, account) = setup();
        let version_one = draft_record(&account.account_id, "edited-during-send", "V1", 1, 0);
        repository
            .save_draft_record(&version_one)
            .expect("version one");
        let old_attempt = linked_outbox(&version_one, "first-attempt-v1", OutboxStatus::Sending, 1);
        repository
            .enqueue_outbox(&old_attempt)
            .expect("in-flight version one");

        let mut version_two = version_one.clone();
        version_two.local_version = 2;
        version_two.revision = 2;
        version_two.draft.local_version = 2;
        version_two.draft.subject = "V2 preserved".to_owned();
        version_two.draft.raw_rfc822 = b"version two draft bytes".to_vec();
        repository
            .save_draft_record(&version_two)
            .expect("concurrent version two edit");

        repository
            .finalize_outbox_sent(&old_attempt.id)
            .expect("version one accepted");
        let sent_v1 = repository.get_outbox(&old_attempt.id).unwrap();
        assert_eq!(sent_v1.status, OutboxStatus::Sent);
        assert_eq!(sent_v1.draft_id, None);
        assert_eq!(sent_v1.draft_revision, None);
        assert_eq!(sent_v1.draft_local_version, None);
        let preserved = repository.get_draft_record(&version_two.draft.id).unwrap();
        assert_eq!(preserved.local_version, 2);
        assert_eq!(preserved.draft.subject, "V2 preserved");
        assert_eq!(preserved.draft.status, "local");

        let queued_v2 = linked_outbox(&version_two, "first-attempt-v2", OutboxStatus::Queued, 0);
        let claimed_v2 = repository
            .enqueue_and_claim_outbox(&queued_v2)
            .expect("newer draft version remains sendable");
        assert_eq!(claimed_v2.status, OutboxStatus::Sending);
        assert_eq!(claimed_v2.draft_local_version, Some(2));
    }

    #[test]
    fn retry_success_preserves_a_draft_edited_after_the_retry_claim() {
        let (_directory, repository, account) = setup();
        let version_one = draft_record(&account.account_id, "retry-edit", "retry V1", 1, 0);
        repository
            .save_draft_record(&version_one)
            .expect("version one");
        let retryable = linked_outbox(&version_one, "retry-edit-v1", OutboxStatus::Retryable, 1);
        repository.enqueue_outbox(&retryable).expect("retryable V1");
        let claimed = repository
            .claim_retryable_outbox(&retryable.id, &account.account_id)
            .expect("manual retry claim");

        let mut version_two = version_one.clone();
        version_two.local_version = 2;
        version_two.revision = 2;
        version_two.draft.local_version = 2;
        version_two.draft.subject = "retry V2 preserved".to_owned();
        repository
            .save_draft_record(&version_two)
            .expect("edit during retry");
        repository
            .finalize_outbox_sent(&claimed.id)
            .expect("retry accepted");

        let sent_v1 = repository.get_outbox(&claimed.id).unwrap();
        assert_eq!(sent_v1.status, OutboxStatus::Sent);
        assert_eq!(sent_v1.attempts, 2);
        assert_eq!(sent_v1.draft_id, None);
        let preserved = repository.get_draft_record(&version_two.draft.id).unwrap();
        assert_eq!(preserved.local_version, 2);
        assert_eq!(preserved.draft.subject, "retry V2 preserved");
        assert_eq!(preserved.draft.status, "local");
    }

    #[test]
    fn rejected_and_retryable_v1_do_not_leave_a_second_send_path_after_v2() {
        let (_directory, repository, account) = setup();

        let rejected_v1 = draft_record(&account.account_id, "rejected-draft", "rejected V1", 1, 0);
        repository
            .save_draft_record(&rejected_v1)
            .expect("rejected draft V1");
        let mut rejected_attempt =
            linked_outbox(&rejected_v1, "rejected-v1", OutboxStatus::Rejected, 1);
        rejected_attempt.last_error = Some("permanent SMTP rejection".to_owned());
        repository
            .enqueue_outbox(&rejected_attempt)
            .expect("rejected audit item");
        let mut rejected_v2 = rejected_v1.clone();
        rejected_v2.local_version = 2;
        rejected_v2.revision = 2;
        rejected_v2.draft.local_version = 2;
        rejected_v2.draft.subject = "rejected V2".to_owned();
        repository
            .save_draft_record(&rejected_v2)
            .expect("rejected draft V2");
        let rejected_v2_send = linked_outbox(&rejected_v2, "rejected-v2", OutboxStatus::Queued, 0);
        repository
            .enqueue_and_claim_outbox(&rejected_v2_send)
            .expect("definitively rejected V1 must not block V2");
        let old_rejected = repository.get_outbox(&rejected_attempt.id).unwrap();
        assert_eq!(old_rejected.status, OutboxStatus::Rejected);
        assert_eq!(old_rejected.raw_rfc822, rejected_attempt.raw_rfc822);

        let retryable_v1 =
            draft_record(&account.account_id, "retryable-draft", "retryable V1", 1, 0);
        repository
            .save_draft_record(&retryable_v1)
            .expect("retryable draft V1");
        let retryable_attempt =
            linked_outbox(&retryable_v1, "retryable-v1", OutboxStatus::Retryable, 1);
        repository
            .enqueue_outbox(&retryable_attempt)
            .expect("retryable V1");
        let mut retryable_v2 = retryable_v1.clone();
        retryable_v2.local_version = 2;
        retryable_v2.revision = 2;
        retryable_v2.draft.local_version = 2;
        retryable_v2.draft.subject = "retryable V2".to_owned();
        repository
            .save_draft_record(&retryable_v2)
            .expect("retryable draft V2");
        let retryable_v2_send =
            linked_outbox(&retryable_v2, "retryable-v2", OutboxStatus::Queued, 0);
        repository
            .enqueue_and_claim_outbox(&retryable_v2_send)
            .expect("V2 atomically supersedes retryable V1");

        let superseded = repository.get_outbox(&retryable_attempt.id).unwrap();
        assert_eq!(superseded.status, OutboxStatus::Rejected);
        assert_eq!(superseded.draft_id, None);
        assert!(
            superseded
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("superseded by a newer confirmed draft"))
        );
        assert!(matches!(
            repository.claim_retryable_outbox(&retryable_attempt.id, &account.account_id),
            Err(MailError::Validation(_))
        ));
    }

    #[test]
    fn delivery_unknown_v1_blocks_every_newer_draft_version() {
        let (_directory, repository, account) = setup();
        let version_one = draft_record(&account.account_id, "unknown-draft", "unknown V1", 1, 0);
        repository
            .save_draft_record(&version_one)
            .expect("version one");
        let unknown = linked_outbox(&version_one, "unknown-v1", OutboxStatus::DeliveryUnknown, 1);
        repository.enqueue_outbox(&unknown).expect("unknown V1");

        let mut version_two = version_one.clone();
        version_two.local_version = 2;
        version_two.revision = 2;
        version_two.draft.local_version = 2;
        version_two.draft.subject = "unknown V2".to_owned();
        repository
            .save_draft_record(&version_two)
            .expect("version two");
        let v2_send = linked_outbox(&version_two, "unknown-v2", OutboxStatus::Queued, 0);
        let blocked = repository
            .enqueue_and_claim_outbox(&v2_send)
            .expect_err("unknown delivery must block all versions");
        assert!(
            blocked
                .to_string()
                .contains("delivery of an earlier draft version is unknown")
        );
        assert!(matches!(
            repository.get_outbox(&v2_send.id),
            Err(MailError::NotFound { .. })
        ));
        let preserved_unknown = repository.get_outbox(&unknown.id).unwrap();
        assert_eq!(preserved_unknown.status, OutboxStatus::DeliveryUnknown);
        assert_eq!(
            preserved_unknown.draft_id.as_deref(),
            Some(version_one.draft.id.as_str())
        );
    }

    #[test]
    fn authorization_secret_is_never_part_of_schema_or_database_bytes() {
        let (directory, repository, _account) = setup();
        let connection = repository.connection().expect("connection");
        let schema: String = connection
            .query_row(
                "SELECT group_concat(sql, ' ') FROM sqlite_master WHERE sql IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("schema");
        let normalized = schema.to_ascii_lowercase();
        assert!(!normalized.contains("password"));
        assert!(!normalized.contains("authorization_password"));
        drop(connection);

        for entry in fs::read_dir(directory.path()).expect("database files") {
            let path = entry.expect("entry").path();
            if path.is_file() {
                let bytes = fs::read(path).expect("read database artifact");
                assert!(
                    !String::from_utf8_lossy(&bytes).contains("super-secret-authorization-value")
                );
            }
        }
    }

    #[test]
    fn refuses_to_reuse_a_database_for_a_different_account() {
        let (_directory, repository, _account) = setup();
        let other = AccountConfig::from_163_lines([
            "different-account@163.com",
            "another-not-real-authorization-value",
        ])
        .expect("other account");

        assert!(repository.initialize_account(&other).is_err());
    }

    #[test]
    fn preview_migration_marks_existing_preview_and_body_rows_as_resolved() {
        let connection = Connection::open_in_memory().expect("legacy database");
        connection
            .execute_batch(
                "CREATE TABLE messages (
                     id INTEGER PRIMARY KEY,
                     preview TEXT NOT NULL DEFAULT '',
                     body_fetched INTEGER NOT NULL DEFAULT 0
                 );
                 INSERT INTO messages (id, preview, body_fetched) VALUES
                     (1, '', 0),
                     (2, 'Existing preview', 0),
                     (3, '', 1);",
            )
            .expect("legacy messages");

        migrate_message_previews_v10(&connection).expect("preview migration");

        let states = {
            let mut statement = connection
                .prepare("SELECT preview_fetched FROM messages ORDER BY id")
                .expect("preview state query");
            statement
                .query_map([], |row| row.get::<_, bool>(0))
                .expect("preview state rows")
                .collect::<std::result::Result<Vec<_>, _>>()
                .expect("preview states")
        };
        assert_eq!(states, [false, true, true]);
    }

    #[test]
    fn body_cache_migration_backfills_utf8_bytes_and_access_time() {
        let connection = Connection::open_in_memory().expect("legacy database");
        let body_text = "你好";
        let body_html = "<p>邮件</p>";
        let attachment_names = "[\"附件.txt\"]";
        let raw = b"raw-message";
        connection
            .execute_batch(
                "CREATE TABLE messages (
                     id INTEGER PRIMARY KEY,
                     body_text TEXT,
                     body_html TEXT,
                     attachment_names_json TEXT NOT NULL DEFAULT '[]',
                     body_fetched INTEGER NOT NULL DEFAULT 0,
                     raw_rfc822 BLOB NOT NULL DEFAULT X'',
                     synced_at TEXT NOT NULL
                 );",
            )
            .expect("legacy messages");
        connection
            .execute(
                "INSERT INTO messages (
                     id, body_text, body_html, attachment_names_json,
                     body_fetched, raw_rfc822, synced_at
                 ) VALUES (
                     1, ?1, ?2, ?3, 1, ?4, '2026-07-01T00:00:00.000Z'
                 ), (
                     2, NULL, NULL, '[]', 0, X'', '2026-07-02T00:00:00.000Z'
                 )",
                params![body_text, body_html, attachment_names, raw],
            )
            .expect("legacy body rows");

        super::migrate_message_body_cache_v18(&connection).expect("body cache migration");

        let (cached_bytes, accessed_at): (i64, String) = connection
            .query_row(
                "SELECT body_cached_bytes, body_last_accessed_at
                 FROM messages WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("migrated cached body");
        let expected = raw.len() + body_text.len() + body_html.len() + attachment_names.len();
        assert_eq!(cached_bytes, i64::try_from(expected).unwrap());
        assert_eq!(accessed_at, "2026-07-01T00:00:00.000Z");
        assert_eq!(
            connection
                .query_row(
                    "SELECT body_cached_bytes FROM messages WHERE id = 2",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("uncached row"),
            0
        );
        assert!(
            connection
                .query_row(
                    "SELECT body_last_accessed_at IS NULL FROM messages WHERE id = 2",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .expect("uncached access time")
        );
    }

    #[test]
    fn v14_migrates_existing_message_public_id_once_and_enforces_integrity() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("real-v13-public-id.sqlite3");
        let legacy = create_legacy_core_fixture(&path);
        legacy
            .execute(
                "UPDATE messages SET sender_json = ?1 WHERE id = 1",
                params![r#"{"name":"Alice","email":"ALICE@example.com"}"#],
            )
            .expect("legacy contact header");
        legacy
            .pragma_update(None, "user_version", 13)
            .expect("v13 marker");
        drop(legacy);

        let upgraded = Repository::open(&path).expect("v14 upgrade");
        let public_id: String = upgraded
            .connection()
            .expect("connection")
            .query_row("SELECT public_id FROM messages WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("migrated public id");
        let parsed_public_id = uuid::Uuid::parse_str(&public_id).expect("canonical UUID");
        assert_eq!(parsed_public_id.get_version(), Some(uuid::Version::Random));
        assert_eq!(
            upgraded
                .get_message_by_public_id("fixture", &public_id)
                .expect("migrated account lookup")
                .id,
            1
        );
        assert_eq!(
            upgraded
                .list_contact_source_messages_for_email("fixture", "alice@example.com", 10,)
                .expect("backfilled contact index")
                .len(),
            1
        );
        drop(upgraded);

        let reopened = Repository::open(&path).expect("normal v14 reopen");
        let connection = reopened.connection().expect("reopened connection");
        let stable: String = connection
            .query_row("SELECT public_id FROM messages WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("stable public id");
        assert_eq!(stable, public_id);
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        assert_eq!(version, 20);
        assert!(
            connection
                .execute(
                    "UPDATE messages SET public_id = ?1 WHERE id = 1",
                    params![uuid::Uuid::new_v4().to_string()],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "UPDATE messages SET public_id = 'not-an-opaque-id' WHERE id = 1",
                    []
                )
                .is_err()
        );
    }

    #[test]
    fn real_v10_fixture_upgrades_flags_and_plain_draft_without_cascade() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("real-v10.sqlite3");
        create_real_pre_v12_fixture(&path, 10, None);

        let upgraded = Repository::open(&path).expect("upgrade real v10 fixture");
        assert!(
            upgraded
                .get_message(1)
                .expect("legacy message")
                .bcc
                .is_empty()
        );
        assert_eq!(
            upgraded
                .pending_seen_updates("fixture", "INBOX")
                .expect("seen queue"),
            [(42, true, 1)]
        );
        assert_eq!(
            upgraded
                .pending_flagged_updates("fixture", "INBOX")
                .expect("flagged queue"),
            [(42, false, 4)]
        );
        assert_eq!(
            upgraded
                .get_draft_record("fixture-draft")
                .expect("migrated draft")
                .draft
                .format,
            ComposeFormat::default()
        );
        let connection = upgraded.connection().expect("connection");
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        assert_eq!(version, 20);
        assert!(
            super::table_has_column(&connection, "mailboxes", "starred_history_before_uid")
                .unwrap()
        );
        assert!(
            super::table_has_column(&connection, "mailboxes", "starred_history_complete").unwrap()
        );
        assert!(
            super::table_has_column(&connection, "message_page_cursors", "flagged_only").unwrap()
        );
        assert!(super::table_has_column(&connection, "messages", "bcc_json").unwrap());
        let seen_targets = {
            let mut statement = connection
                .prepare("PRAGMA foreign_key_list(pending_seen_updates)")
                .expect("seen foreign keys");
            statement
                .query_map([], |row| row.get::<_, String>(2))
                .expect("seen foreign key rows")
                .collect::<std::result::Result<Vec<_>, _>>()
                .expect("seen foreign key targets")
        };
        assert_eq!(seen_targets, ["accounts"]);
        connection
            .execute(
                "DELETE FROM messages
                 WHERE account_id = 'fixture' AND mailbox = 'INBOX' AND uid = 42",
                [],
            )
            .expect("delete migrated source");
        assert_eq!(
            upgraded
                .pending_seen_updates("fixture", "INBOX")
                .expect("durable seen queue"),
            [(42, true, 1)]
        );
    }

    #[test]
    fn real_v11_fixture_preserves_compose_format_bytes_and_flag_intents() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("real-v11.sqlite3");
        let expected = ComposeFormat {
            body_html: Some("<p><strong>Real v11</strong></p>".to_owned()),
            stationery: StationeryTheme::Lined,
            send_stationery: true,
        };
        let encoded = serde_json::to_string(&expected).expect("compose format json");
        create_real_pre_v12_fixture(&path, 11, Some(&encoded));

        let upgraded = Repository::open(&path).expect("upgrade real v11 fixture");
        assert_eq!(
            upgraded
                .get_draft_record("fixture-draft")
                .expect("migrated rich draft")
                .draft
                .format,
            expected
        );
        let stored: String = upgraded
            .connection()
            .expect("connection")
            .query_row(
                "SELECT compose_format_json FROM drafts WHERE id = 'fixture-draft'",
                [],
                |row| row.get(0),
            )
            .expect("stored compose json");
        assert_eq!(stored, encoded);
        assert_eq!(
            upgraded
                .pending_flagged_updates("fixture", "INBOX")
                .expect("flagged queue"),
            [(42, false, 4)]
        );
    }

    #[test]
    fn real_v13_and_v14_null_digest_rows_upgrade_and_persist_one_time_backfill() {
        const CONTENT_SHA256: &str =
            "ed7002b439e9ac845f22357d822bac1444730fbdb6016d3ec9432297b9ec9f73";

        for legacy_version in [13_u32, 14_u32] {
            let directory = TempDir::new().expect("temporary directory");
            let path = directory
                .path()
                .join(format!("real-v{legacy_version}-null-digest.sqlite3"));
            create_real_intermediate_v12_fixture(&path, "{}");
            let connection = Connection::open(&path).expect("legacy attachment fixture");
            super::configure_connection(&connection).expect("legacy connection settings");
            connection
                .execute_batch(
                    "CREATE TABLE outbox (
                         id TEXT PRIMARY KEY NOT NULL,
                         account_id TEXT NOT NULL,
                         draft_id TEXT,
                         recipients_json TEXT NOT NULL DEFAULT '[]',
                         status TEXT NOT NULL,
                         attempts INTEGER NOT NULL DEFAULT 0,
                         last_error TEXT,
                         created_at TEXT NOT NULL,
                         sent_at TEXT,
                         raw_rfc822 BLOB NOT NULL,
                         FOREIGN KEY (account_id)
                             REFERENCES accounts(id) ON DELETE CASCADE,
                         FOREIGN KEY (draft_id)
                             REFERENCES drafts(id) ON DELETE SET NULL
                     );",
                )
                .expect("real pre-v13 Outbox schema");
            super::migrate_managed_attachments_v13(&connection).expect("real v13 schema");
            if legacy_version == 14 {
                super::migrate_message_public_ids_v14(&connection).expect("real v14 schema");
            }
            let blob_id = uuid::Uuid::now_v7().to_string();
            connection
                .execute(
                    "INSERT INTO managed_attachment_blobs (
                         id, account_id, origin_draft_id, internal_name, name,
                         mime_type, size_bytes, sha256_hex
                     ) VALUES (
                         ?1, 'fixture', 'fixture-draft', ?2, 'legacy.txt',
                         'text/plain', 7, NULL
                     )",
                    params![blob_id, format!("{blob_id}.blob")],
                )
                .expect("legacy null-digest blob");
            connection
                .execute(
                    "INSERT INTO draft_attachment_refs (
                         account_id, draft_id, draft_local_version, position, blob_id
                     ) VALUES ('fixture', 'fixture-draft', 3, 0, ?1)",
                    params![blob_id],
                )
                .expect("legacy attachment reference");
            connection
                .pragma_update(None, "user_version", legacy_version)
                .expect("legacy schema marker");
            drop(connection);

            let upgraded = Repository::open(&path).expect("upgrade legacy attachment schema");
            let attachment = upgraded
                .list_draft_attachments_at_version("fixture", "fixture-draft", 3)
                .expect("attachment query")
                .expect("migrated version")
                .attachments
                .into_iter()
                .next()
                .expect("legacy attachment");
            assert_eq!(attachment.meta.id, blob_id);
            assert_eq!(attachment.sha256_hex, None);
            assert_eq!(
                upgraded
                    .connection()
                    .expect("schema version connection")
                    .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                    .expect("schema version"),
                20
            );

            assert_eq!(
                upgraded
                    .initialize_managed_attachment_digest(
                        "fixture",
                        &blob_id,
                        &attachment.internal_name,
                        attachment.meta.size_bytes,
                        CONTENT_SHA256,
                    )
                    .expect("one-time digest initialization"),
                CONTENT_SHA256
            );
            drop(upgraded);

            let restarted = Repository::open(&path).expect("restart upgraded repository");
            assert_eq!(
                restarted
                    .list_draft_attachments_at_version("fixture", "fixture-draft", 3)
                    .expect("restarted attachment query")
                    .expect("restarted version")
                    .attachments[0]
                    .sha256_hex
                    .as_deref(),
                Some(CONTENT_SHA256)
            );
            assert!(
                restarted
                    .initialize_managed_attachment_digest(
                        "fixture",
                        &blob_id,
                        &attachment.internal_name,
                        attachment.meta.size_bytes,
                        &"11".repeat(32),
                    )
                    .is_err()
            );
            assert!(
                restarted
                    .connection()
                    .expect("immutability connection")
                    .execute(
                        "UPDATE managed_attachment_blobs
                         SET name = 'rewritten.txt'
                         WHERE account_id = 'fixture' AND id = ?1",
                        params![blob_id],
                    )
                    .is_err()
            );
        }
    }

    #[test]
    fn digest_backfill_cas_is_account_scoped_and_rejects_concurrent_disagreement() {
        let (_directory, repository, account) = setup();
        let other = secondary_account(&account);
        repository
            .initialize_account(&other)
            .expect("secondary account");
        let draft = draft_record(&account.account_id, "digest-cas-draft", "Digest CAS", 1, 0);
        repository.save_draft_record(&draft).expect("primary draft");
        let attachment = new_attachment("digest-cas.txt", None);
        repository
            .add_draft_attachments_if_local_version(
                &account.account_id,
                &draft.draft.id,
                draft.local_version,
                std::slice::from_ref(&attachment),
                "2026-07-28T08:00:00Z",
            )
            .expect("add attachment")
            .expect("current draft");
        clear_attachment_digest_as_legacy_fixture(
            &repository,
            &account.account_id,
            &attachment.imported.id,
        );

        assert!(
            repository
                .initialize_managed_attachment_digest(
                    &other.account_id,
                    &attachment.imported.id,
                    &attachment.imported.internal_name,
                    attachment.imported.size_bytes,
                    &"11".repeat(32),
                )
                .is_err()
        );
        let missing_blob_id = uuid::Uuid::now_v7().to_string();
        assert!(
            repository
                .initialize_managed_attachment_digest(
                    &account.account_id,
                    &missing_blob_id,
                    &format!("{missing_blob_id}.blob"),
                    attachment.imported.size_bytes,
                    &"11".repeat(32),
                )
                .is_err()
        );

        let barrier = Arc::new(Barrier::new(3));
        let attempts = ["11".repeat(32), "22".repeat(32)]
            .into_iter()
            .map(|digest| {
                let repository = repository.clone();
                let barrier = Arc::clone(&barrier);
                let account_id = account.account_id.clone();
                let blob_id = attachment.imported.id.clone();
                let internal_name = attachment.imported.internal_name.clone();
                let size_bytes = attachment.imported.size_bytes;
                thread::spawn(move || {
                    barrier.wait();
                    repository.initialize_managed_attachment_digest(
                        &account_id,
                        &blob_id,
                        &internal_name,
                        size_bytes,
                        &digest,
                    )
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = attempts
            .into_iter()
            .map(|attempt| attempt.join().expect("digest CAS thread"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);

        let stored: String = repository
            .connection()
            .expect("digest query connection")
            .query_row(
                "SELECT sha256_hex
                 FROM managed_attachment_blobs
                 WHERE account_id = ?1 AND id = ?2",
                params![account.account_id, attachment.imported.id],
                |row| row.get(0),
            )
            .expect("stored digest");
        assert!(
            results
                .iter()
                .filter_map(|result| result.as_ref().ok())
                .any(|winner| winner == &stored)
        );
    }

    #[test]
    fn managed_attachments_are_account_version_and_outbox_scoped() {
        let (_directory, repository, account) = setup();
        let other = secondary_account(&account);
        repository
            .initialize_account(&other)
            .expect("secondary account");
        let base = draft_record(&account.account_id, "attachment-draft", "Attachments", 1, 0);
        repository
            .save_draft_record(&base)
            .expect("attachment draft");
        let other_draft = draft_record(&other.account_id, "other-attachment-draft", "Other", 1, 0);
        repository
            .save_draft_record(&other_draft)
            .expect("other draft");

        let first = new_attachment("first.txt", None);
        let second = new_attachment("second.bin", Some("source-part-2"));
        let added = repository
            .add_draft_attachments_if_local_version(
                &account.account_id,
                &base.draft.id,
                1,
                &[first.clone(), second.clone()],
                "2026-07-28T01:00:00Z",
            )
            .expect("attachment add")
            .expect("exact version");
        assert_eq!(added.local_version, 2);
        assert_eq!(added.attachments.len(), 2);
        assert_eq!(
            added.attachments[1].meta.source_attachment_id.as_deref(),
            Some("source-part-2")
        );
        assert!(
            repository
                .list_draft_attachments_at_version(
                    &other.account_id,
                    &base.draft.id,
                    added.local_version,
                )
                .expect("account isolation")
                .is_none()
        );

        assert!(
            repository
                .remove_draft_attachment_if_local_version(
                    &account.account_id,
                    &base.draft.id,
                    &first.imported.id,
                    1,
                    "2026-07-28T01:01:00Z",
                )
                .expect("stale remove")
                .is_none()
        );
        assert_eq!(
            repository
                .list_draft_attachments_at_version(&account.account_id, &base.draft.id, 2,)
                .unwrap()
                .unwrap()
                .attachments
                .len(),
            2
        );
        let removed = repository
            .remove_draft_attachment_if_local_version(
                &account.account_id,
                &base.draft.id,
                &first.imported.id,
                2,
                "2026-07-28T01:02:00Z",
            )
            .expect("remove")
            .expect("current version");
        assert_eq!(removed.local_version, 3);
        assert_eq!(removed.attachments.len(), 1);
        assert_eq!(removed.attachments[0].meta.id, second.imported.id);

        let expected = repository
            .get_draft_record(&base.draft.id)
            .expect("current draft");
        let mut replacement = expected.clone();
        replacement.local_version += 1;
        replacement.draft.local_version = replacement.local_version;
        replacement.revision += 1;
        replacement.draft.subject = "Body edit keeps attachments".to_owned();
        replacement.draft.updated_at = "2026-07-28T01:03:00Z".to_owned();
        assert!(
            repository
                .replace_draft_if_unchanged(&expected, &replacement, None)
                .expect("body CAS")
        );
        assert_eq!(
            repository
                .list_draft_attachments_at_version(
                    &account.account_id,
                    &base.draft.id,
                    replacement.local_version,
                )
                .unwrap()
                .unwrap()
                .attachments
                .len(),
            1
        );
        assert_eq!(
            repository
                .list_draft_attachments_at_version(&account.account_id, &base.draft.id, 2,)
                .unwrap()
                .unwrap()
                .attachments
                .len(),
            2,
            "body and remove edits must not rewrite the version-two set"
        );
        assert_eq!(
            repository
                .draft_version_snapshot(
                    &account.account_id,
                    &base.draft.id,
                    replacement.local_version,
                )
                .unwrap()
                .unwrap()
                .request
                .subject,
            "Body edit keeps attachments"
        );
        let reopened = Repository::open(&repository.path).expect("restart repository");
        assert_eq!(
            reopened
                .list_draft_attachments_at_version(&account.account_id, &base.draft.id, 2,)
                .unwrap()
                .unwrap()
                .attachments
                .len(),
            2
        );
        assert_eq!(
            reopened
                .draft_version_snapshot(
                    &account.account_id,
                    &base.draft.id,
                    replacement.local_version,
                )
                .unwrap()
                .unwrap()
                .request,
            replacement.draft.compose_request()
        );
        drop(reopened);

        let forward_context = ForwardContext {
            source_message_id: "message-opaque".to_owned(),
            original_subject: "Original".to_owned(),
            from: Some(MailAddress {
                name: Some("Alice".to_owned()),
                email: "alice@example.com".to_owned(),
            }),
            to: vec![MailAddress {
                name: None,
                email: "receiver@example.com".to_owned(),
            }],
            cc: Vec::new(),
            sent_at: Some("2026-07-27T01:00:00Z".to_owned()),
            quoted_text: "quoted".to_owned(),
            quoted_html: Some("<p>quoted</p>".to_owned()),
            quoted_render_mode: Some(ForwardQuotedRenderMode::NativeHtml),
            source_attachments: vec![AttachmentMeta {
                id: "source-part-2".to_owned(),
                original_name: Some("second.bin".to_owned()),
                safe_display_name: "second.bin".to_owned(),
                mime_type: "application/octet-stream".to_owned(),
                size_bytes: second.imported.size_bytes,
                size_is_estimate: false,
                disposition: AttachmentDisposition::Attachment,
            }],
        };
        assert!(
            repository
                .save_forward_context_if_absent(
                    &account.account_id,
                    &base.draft.id,
                    &forward_context,
                )
                .expect("forward context")
        );
        assert!(
            !repository
                .save_forward_context_if_absent(
                    &account.account_id,
                    &base.draft.id,
                    &forward_context,
                )
                .expect("idempotent forward context")
        );

        let conflict = draft_record(&account.account_id, "attachment-conflict", "Conflict", 1, 0);
        assert!(
            repository
                .insert_draft_if_absent(&conflict)
                .expect("insert conflict")
        );
        assert!(
            repository
                .clone_draft_attachments_to_conflict(
                    &account.account_id,
                    &base.draft.id,
                    replacement.local_version,
                    &conflict.draft.id,
                    conflict.local_version,
                )
                .expect("clone exact set")
        );
        assert_eq!(
            repository
                .forward_context(&account.account_id, &conflict.draft.id)
                .expect("conflict forward context"),
            Some(forward_context)
        );

        let outbox = linked_outbox(
            &repository
                .get_draft_record(&base.draft.id)
                .expect("outbox draft"),
            "attachment-outbox",
            OutboxStatus::Queued,
            0,
        );
        repository
            .enqueue_outbox(&outbox)
            .expect("bind Outbox attachments");
        assert_eq!(
            repository
                .list_outbox_attachments(&account.account_id, &outbox.id)
                .expect("Outbox attachment set")
                .len(),
            1
        );

        let current = repository
            .get_draft_record(&base.draft.id)
            .expect("draft before final remove");
        repository
            .remove_draft_attachment_if_local_version(
                &account.account_id,
                &base.draft.id,
                &second.imported.id,
                current.local_version,
                "2026-07-28T01:04:00Z",
            )
            .expect("remove current attachment")
            .expect("current remove");
        assert_eq!(
            repository
                .list_outbox_attachments(&account.account_id, &outbox.id)
                .expect("immutable Outbox set")
                .len(),
            1
        );
        assert!(
            !repository
                .list_orphaned_managed_attachments(&account.account_id)
                .expect("historical snapshot refs")
                .iter()
                .any(|orphan| orphan.id == first.imported.id)
        );
        repository
            .tombstone_draft(&base.draft.id, "2026-07-28T01:05:00Z")
            .expect("terminal draft releases every historical snapshot ref");
        assert!(
            repository
                .list_orphaned_managed_attachments(&account.account_id)
                .expect("orphan list after terminal release")
                .iter()
                .any(|orphan| orphan.id == first.imported.id)
        );
        assert!(
            !repository
                .list_orphaned_managed_attachments(&account.account_id)
                .expect("Outbox protection")
                .iter()
                .any(|orphan| orphan.id == second.imported.id)
        );
        assert!(
            repository
                .take_orphaned_managed_attachment(&other.account_id, &first.imported.id)
                .expect("other account cleanup")
                .is_none()
        );
        assert_eq!(
            repository
                .take_orphaned_managed_attachment(&account.account_id, &first.imported.id)
                .expect("take orphan")
                .expect("unreferenced blob")
                .internal_name,
            first.imported.internal_name
        );

        let connection = repository.connection().expect("constraint inspection");
        let attachment_foreign_keys = {
            let mut statement = connection
                .prepare("PRAGMA foreign_key_list(draft_attachment_refs)")
                .unwrap();
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                })
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
        };
        assert!(attachment_foreign_keys.iter().any(|(table, on_update, _)| {
            table == "draft_version_snapshots" && on_update == "NO ACTION"
        }));
        assert_eq!(
            connection
                .query_row(
                    "SELECT sha256_hex FROM managed_attachment_blobs
                     WHERE account_id = ?1 AND id = ?2",
                    params![account.account_id, second.imported.id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .unwrap()
                .as_deref(),
            Some(second.imported.sha256_hex.as_str())
        );
        assert!(
            connection
                .execute(
                    "UPDATE draft_version_snapshots SET subject = 'rewritten'
                     WHERE account_id = ?1 AND draft_id = ?2",
                    params![account.account_id, base.draft.id],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "INSERT INTO draft_attachment_refs (
                         account_id, draft_id, draft_local_version, position, blob_id
                     ) VALUES (?1, ?2, ?3, 9, ?4)",
                    params![
                        other.account_id,
                        other_draft.draft.id,
                        super::u64_to_i64(other_draft.local_version),
                        second.imported.id,
                    ],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "UPDATE managed_attachment_blobs SET name = 'changed.bin'
                     WHERE account_id = ?1 AND id = ?2",
                    params![account.account_id, second.imported.id],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "UPDATE draft_forward_contexts SET original_subject = 'changed'
                     WHERE account_id = ?1 AND draft_id = ?2",
                    params![account.account_id, base.draft.id],
                )
                .is_err()
        );
        connection
            .execute(
                "DELETE FROM accounts WHERE id = ?1",
                params![account.account_id],
            )
            .expect("account cache removal cascades attachment references");
        let remaining_blobs: u32 = connection
            .query_row(
                "SELECT COUNT(*) FROM managed_attachment_blobs WHERE account_id = ?1",
                params![account.account_id],
                |row| row.get(0),
            )
            .expect("remaining account blobs");
        assert_eq!(remaining_blobs, 0);
    }

    #[test]
    fn v15_migrates_cascading_current_refs_into_immutable_version_snapshots() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("v14-draft-versions.sqlite3");
        let account = AccountConfig::from_163_lines([
            "v15-migration@163.com",
            "not-a-real-authorization-value",
        ])
        .expect("account");
        let repository = Repository::open(&path).expect("repository");
        repository
            .initialize_account(&account)
            .expect("account row");
        let base = draft_record(
            &account.account_id,
            "v15-migrated-draft",
            "Version one",
            1,
            0,
        );
        repository.save_draft_record(&base).expect("base draft");
        let attachment = new_attachment("migration.txt", None);
        repository
            .add_draft_attachments_if_local_version(
                &account.account_id,
                &base.draft.id,
                1,
                std::slice::from_ref(&attachment),
                "2026-07-28T02:00:00Z",
            )
            .expect("add attachment")
            .expect("current version");
        drop(repository);

        let legacy = Connection::open(&path).expect("legacy v14 connection");
        legacy
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 DROP TRIGGER IF EXISTS trg_draft_version_snapshots_immutable;
                 DROP TABLE IF EXISTS draft_attachment_refs_v14;
                 CREATE TABLE draft_attachment_refs_v14 (
                     account_id TEXT NOT NULL,
                     draft_id TEXT NOT NULL,
                     draft_local_version INTEGER NOT NULL CHECK (draft_local_version > 0),
                     position INTEGER NOT NULL CHECK (position >= 0),
                     blob_id TEXT NOT NULL,
                     source_attachment_id TEXT,
                     PRIMARY KEY (account_id, draft_id, draft_local_version, position),
                     UNIQUE (account_id, draft_id, draft_local_version, blob_id),
                     FOREIGN KEY (account_id, draft_id, draft_local_version)
                         REFERENCES drafts(account_id, id, local_version)
                         ON UPDATE CASCADE ON DELETE CASCADE,
                     FOREIGN KEY (account_id, blob_id)
                         REFERENCES managed_attachment_blobs(account_id, id)
                         ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
                 );
                 INSERT INTO draft_attachment_refs_v14
                 SELECT account_id, draft_id, draft_local_version, position,
                        blob_id, source_attachment_id
                 FROM draft_attachment_refs;
                 DROP TABLE draft_attachment_refs;
                 ALTER TABLE draft_attachment_refs_v14 RENAME TO draft_attachment_refs;
                 DROP TABLE draft_version_forward_context_refs;
                 DROP TABLE draft_version_snapshots;
                 PRAGMA user_version = 14;",
            )
            .expect("downgrade fixture to the real v14 relationship");
        drop(legacy);

        let upgraded = Repository::open(&path).expect("v15 upgrade");
        let migrated = upgraded
            .draft_version_snapshot(&account.account_id, &base.draft.id, 2)
            .expect("snapshot query")
            .expect("migrated current snapshot");
        assert_eq!(migrated.request.subject, "Version one");
        assert_eq!(
            upgraded
                .list_draft_attachments_at_version(&account.account_id, &base.draft.id, 2)
                .unwrap()
                .unwrap()
                .attachments[0]
                .meta
                .id,
            attachment.imported.id
        );
        let expected = upgraded.get_draft_record(&base.draft.id).unwrap();
        let mut replacement = expected.clone();
        replacement.local_version += 1;
        replacement.draft.local_version = replacement.local_version;
        replacement.revision += 1;
        replacement.draft.subject = "Version two body".to_owned();
        replacement.draft.body_text = "immutable second body".to_owned();
        replacement.draft.updated_at = "2026-07-28T02:01:00Z".to_owned();
        upgraded
            .replace_draft_if_unchanged(&expected, &replacement, None)
            .expect("body replacement");
        assert_eq!(
            upgraded
                .list_draft_attachments_at_version(&account.account_id, &base.draft.id, 2)
                .unwrap()
                .unwrap()
                .attachments
                .len(),
            1
        );
        assert_eq!(
            upgraded
                .list_draft_attachments_at_version(&account.account_id, &base.draft.id, 3)
                .unwrap()
                .unwrap()
                .attachments
                .len(),
            1
        );
        drop(upgraded);

        let reopened = Repository::open(&path).expect("restart upgraded repository");
        assert_eq!(
            reopened
                .draft_version_snapshot(&account.account_id, &base.draft.id, 2)
                .unwrap()
                .unwrap()
                .request
                .body_text,
            base.draft.body_text
        );
        assert_eq!(
            reopened
                .draft_version_snapshot(&account.account_id, &base.draft.id, 3)
                .unwrap()
                .unwrap()
                .request
                .body_text,
            "immutable second body"
        );
        let foreign_targets = {
            let connection = reopened.connection().unwrap();
            let mut statement = connection
                .prepare("PRAGMA foreign_key_list(draft_attachment_refs)")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(2))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
        };
        assert!(foreign_targets.contains(&"draft_version_snapshots".to_owned()));
    }

    #[test]
    fn real_intermediate_v12_fixture_rebuilds_cascades_and_recovers_inflight() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("real-intermediate-v12.sqlite3");
        let format = ComposeFormat {
            body_html: Some("<p>Intermediate v12</p>".to_owned()),
            stationery: StationeryTheme::Grid,
            send_stationery: false,
        };
        let encoded = serde_json::to_string(&format).expect("compose format json");
        create_real_intermediate_v12_fixture(&path, &encoded);

        let upgraded = Repository::open(&path).expect("upgrade intermediate v12 fixture");
        let seen = upgraded
            .system_flag_mutation_receipt("fixture", "seen-intermediate", SystemFlagKind::Seen)
            .expect("seen receipt")
            .expect("seen operation");
        assert_eq!(seen.local_revision, 5);
        assert_eq!(seen.status, MutationStatus::OutcomeUnknown);
        let flagged = upgraded
            .pending_system_flag_mutations("fixture", "INBOX", SystemFlagKind::Flagged)
            .expect("pending flag queue");
        assert_eq!(flagged.len(), 1);
        assert_eq!(flagged[0].operation_id, "flagged-intermediate");
        assert_eq!(flagged[0].revision, 6);
        let action = upgraded
            .message_action("fixture", "action-intermediate")
            .expect("action query")
            .expect("action");
        assert_eq!(action.status, MutationStatus::OutcomeUnknown);
        assert_eq!(action.remote_phase, RemoteMutationPhase::Queued);
        assert_eq!(action.error_kind, Some(MessageMutationErrorKind::Unknown));
        assert!(!action.source_cleanup_pending);
        assert!(!action.destination_reconciled);
        assert_eq!(
            upgraded
                .get_draft_record("fixture-draft")
                .expect("draft")
                .draft
                .format,
            format
        );
        let connection = upgraded.connection().expect("connection");
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        assert_eq!(version, 20);
        for column in ["source_cleanup_pending", "destination_reconciled"] {
            assert!(
                super::table_has_column(&connection, "pending_message_actions", column).unwrap()
            );
        }
        for table in [
            "managed_attachment_blobs",
            "draft_attachment_refs",
            "outbox_attachment_sets",
            "outbox_attachment_refs",
            "draft_forward_contexts",
            "draft_forward_source_attachments",
        ] {
            let present: bool = connection
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
                     )",
                    params![table],
                    |row| row.get(0),
                )
                .expect("v13 table");
            assert!(present, "{table}");
        }
        let targets = {
            let mut statement = connection
                .prepare("PRAGMA foreign_key_list(pending_seen_updates)")
                .expect("seen foreign keys");
            statement
                .query_map([], |row| row.get::<_, String>(2))
                .expect("foreign key rows")
                .collect::<std::result::Result<Vec<_>, _>>()
                .expect("foreign key targets")
        };
        assert_eq!(targets, ["accounts"]);
        connection
            .execute(
                "DELETE FROM messages
                 WHERE account_id = 'fixture' AND mailbox = 'INBOX' AND uid = 42",
                [],
            )
            .expect("delete source after rebuild");
        assert!(
            upgraded
                .system_flag_mutation_receipt("fixture", "seen-intermediate", SystemFlagKind::Seen,)
                .expect("durable seen receipt")
                .is_some()
        );
        assert!(
            upgraded
                .message_action("fixture", "action-intermediate")
                .expect("durable action query")
                .is_some()
        );
    }

    #[test]
    fn v12_migrates_legacy_seen_rows_to_desired_revision_without_losing_intent() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("legacy-seen.sqlite3");
        let repository = Repository::open(&path).expect("initial repository");
        let account = AccountConfig::from_163_lines([
            "legacy-seen@163.com",
            "legacy-not-real-authorization-value",
        ])
        .expect("account");
        repository
            .initialize_account(&account)
            .expect("account row");
        initialize_mailbox(
            &repository,
            &account.account_id,
            MailboxRole::Inbox,
            "INBOX",
            91,
        );
        repository
            .upsert_message(&message(&account.account_id, false))
            .expect("message");
        drop(repository);

        let legacy = Connection::open(&path).expect("legacy database");
        legacy
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 DROP TABLE pending_seen_updates;
                 CREATE TABLE pending_seen_updates (
                     account_id TEXT NOT NULL,
                     mailbox TEXT NOT NULL,
                     uid INTEGER NOT NULL,
                     created_at TEXT NOT NULL,
                     PRIMARY KEY (account_id, mailbox, uid),
                     FOREIGN KEY (account_id, mailbox, uid)
                         REFERENCES messages(account_id, mailbox, uid) ON DELETE CASCADE
                 );
                 INSERT INTO pending_seen_updates (
                     account_id, mailbox, uid, created_at
                 ) VALUES (
                     'primary', 'INBOX', 42, '2026-07-21T12:00:00Z'
                 );
                 PRAGMA user_version = 11;",
            )
            .expect("legacy seen queue");
        drop(legacy);

        let upgraded = Repository::open(&path).expect("v12 upgrade");
        assert_eq!(
            upgraded
                .pending_seen_updates(&account.account_id, "INBOX")
                .expect("migrated seen intent"),
            [(42, true, 1)]
        );
        let connection = upgraded.connection().expect("connection");
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        assert_eq!(version, 20);
        for column in [
            "operation_id",
            "source_uid_validity",
            "desired",
            "revision",
            "status",
            "error_kind",
            "updated_at",
        ] {
            assert!(super::table_has_column(&connection, "pending_seen_updates", column).unwrap());
        }
        let seen_foreign_tables = {
            let mut statement = connection
                .prepare("PRAGMA foreign_key_list(pending_seen_updates)")
                .expect("seen foreign keys");
            statement
                .query_map([], |row| row.get::<_, String>(2))
                .expect("foreign key rows")
                .collect::<std::result::Result<Vec<_>, _>>()
                .expect("foreign key tables")
        };
        assert_eq!(seen_foreign_tables, ["accounts"]);
    }

    #[test]
    fn v12_preserves_v11_compose_format_bytes_and_draft_column_order() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("v11-compose.sqlite3");
        let repository = Repository::open(&path).expect("repository");
        let account = AccountConfig::from_163_lines([
            "v11-compose@163.com",
            "compose-not-real-authorization-value",
        ])
        .expect("account");
        repository
            .initialize_account(&account)
            .expect("account row");
        let mut record = draft_record(&account.account_id, "rich-v11", "Rich", 3, 0);
        record.draft.format = ComposeFormat {
            body_html: Some("<p><strong>Preserve me</strong></p>".to_owned()),
            stationery: StationeryTheme::Lined,
            send_stationery: true,
        };
        repository
            .save_draft_record(&record)
            .expect("v11 rich draft");
        let before_columns = {
            let connection = repository.connection().expect("connection");
            connection
                .pragma_update(None, "user_version", 11)
                .expect("v11 marker");
            let mut statement = connection
                .prepare("PRAGMA table_info(drafts)")
                .expect("draft columns");
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .expect("column rows")
                .collect::<std::result::Result<Vec<_>, _>>()
                .expect("columns")
        };
        drop(repository);

        let upgraded = Repository::open(&path).expect("v12 upgrade");
        let stored = upgraded
            .get_draft_record("rich-v11")
            .expect("preserved rich draft");
        assert_eq!(stored.draft.format, record.draft.format);
        let after_columns = {
            let connection = upgraded.connection().expect("connection");
            let mut statement = connection
                .prepare("PRAGMA table_info(drafts)")
                .expect("draft columns");
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .expect("column rows")
                .collect::<std::result::Result<Vec<_>, _>>()
                .expect("columns")
        };
        assert_eq!(after_columns, before_columns);
    }

    #[test]
    fn upgrades_legacy_drafts_with_synced_revision_metadata() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("legacy.sqlite3");
        let legacy = Connection::open(&path).expect("legacy database");
        legacy
            .execute_batch(
                "CREATE TABLE drafts (
                     id TEXT PRIMARY KEY NOT NULL,
                     account_id TEXT NOT NULL,
                     to_json TEXT NOT NULL DEFAULT '[]',
                     cc_json TEXT NOT NULL DEFAULT '[]',
                     bcc_json TEXT NOT NULL DEFAULT '[]',
                     subject TEXT NOT NULL DEFAULT '',
                     body_text TEXT NOT NULL DEFAULT '',
                     status TEXT NOT NULL,
                     remote_mailbox TEXT,
                     remote_uid INTEGER,
                     created_at TEXT NOT NULL,
                     updated_at TEXT NOT NULL,
                     raw_rfc822 BLOB NOT NULL DEFAULT X''
                 );
                 INSERT INTO drafts (
                     id, account_id, subject, body_text, status, remote_mailbox,
                     remote_uid, created_at, updated_at, raw_rfc822
                 ) VALUES (
                     'legacy-draft', 'primary', 'Legacy', 'Body', 'synced',
                     'Drafts', 17, '2026-07-14T00:00:00Z', '2026-07-14T00:00:00Z',
                     CAST('From: sender@example.com
To: receiver@example.com
Content-Type: text/plain; charset=utf-8

Body' AS BLOB)
                 );
                 INSERT INTO drafts (
                     id, account_id, subject, body_text, status, remote_mailbox,
                     remote_uid, created_at, updated_at
                 ) VALUES (
                     'legacy-broken', 'primary', 'Broken', '', 'synced',
                     'Drafts', 18, '2026-07-14T00:00:00Z', '2026-07-14T00:00:00Z'
                 );",
            )
            .expect("legacy schema");
        drop(legacy);

        let repository = Repository::open(&path).expect("upgrade database");
        let record = repository
            .get_draft_record("legacy-draft")
            .expect("upgraded draft");
        assert_eq!(record.revision, 1);
        assert_eq!(record.local_version, 1);
        assert_eq!(record.synced_revision, 1);
        assert_eq!(record.remote_uid_validity, None);
        assert!(!record.is_deleted);
        assert!(!record.draft.has_unsupported_content);
        assert!(
            repository
                .get_draft_record("legacy-broken")
                .expect("conservative legacy draft")
                .draft
                .has_unsupported_content
        );

        let connection = repository.connection().expect("connection");
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        assert_eq!(version, 20);
        for column in [
            "local_version",
            "has_unsupported_content",
            "revision",
            "synced_revision",
            "remote_uid_validity",
            "is_deleted",
            "reply_context_json",
            "compose_format_json",
        ] {
            assert!(super::table_has_column(&connection, "drafts", column).unwrap());
        }
        assert_eq!(record.draft.format, Default::default());
    }

    #[test]
    fn upgrades_legacy_outbox_before_creating_the_versioned_unique_index() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("legacy-outbox.sqlite3");
        let legacy = Connection::open(&path).expect("legacy database");
        legacy
            .execute_batch(
                "CREATE TABLE outbox (
                     id TEXT PRIMARY KEY NOT NULL,
                     account_id TEXT NOT NULL,
                     draft_id TEXT,
                     recipients_json TEXT NOT NULL DEFAULT '[]',
                     status TEXT NOT NULL,
                     attempts INTEGER NOT NULL DEFAULT 0,
                     last_error TEXT,
                     created_at TEXT NOT NULL,
                     sent_at TEXT,
                     raw_rfc822 BLOB NOT NULL
                 );
                 CREATE UNIQUE INDEX idx_outbox_unique_draft
                     ON outbox(draft_id) WHERE draft_id IS NOT NULL;
                 INSERT INTO outbox (
                     id, account_id, draft_id, recipients_json, status,
                     created_at, raw_rfc822
                 ) VALUES (
                     'legacy-outbox', 'primary', 'legacy-draft',
                     '[\"receiver@example.com\"]', 'retryable',
                     '2026-07-14T00:00:00Z', X'010203'
                 );",
            )
            .expect("legacy Outbox schema");
        drop(legacy);

        let repository = Repository::open(&path).expect("upgrade legacy Outbox");
        let upgraded = repository
            .get_outbox("legacy-outbox")
            .expect("legacy item remains readable");
        assert_eq!(upgraded.draft_revision, None);
        assert_eq!(upgraded.draft_local_version, None);
        assert_eq!(upgraded.recipient_groups, None);
        assert_eq!(upgraded.raw_rfc822, [1, 2, 3]);

        let connection = repository.connection().expect("connection");
        assert!(super::table_has_column(&connection, "outbox", "draft_revision").unwrap());
        assert!(super::table_has_column(&connection, "outbox", "draft_local_version").unwrap());
        assert!(super::table_has_column(&connection, "outbox", "recipient_groups_json").unwrap());
        let old_index: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_outbox_unique_draft'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let new_index: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_outbox_unique_draft_local_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_index, 0);
        assert_eq!(new_index, 1);
    }
}
