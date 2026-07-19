use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{
    Connection, OptionalExtension, Row, TransactionBehavior, named_params, params, types::Type,
};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    AccountConfig, Draft, InboxMessage, MailError, OutboxItem, OutboxStatus, Result,
    mime::{draft_has_unsupported_content, reply_message_ids},
};

const MESSAGE_COLUMNS: &str = "id, account_id, mailbox, uid, message_id, in_reply_to_json, \
    references_json, subject, sender_json, to_json, cc_json, sent_at, internal_date, flags_json, \
    size_bytes, preview, body_text, body_html, attachment_names_json, body_fetched, raw_rfc822, synced_at";
// Inbox rows only need enough local body data to paint an immediate fallback.
// The empty HTML sentinel preserves `body_html.is_some()` without reading the
// potentially large HTML/RFC822 payload for every visible list item.
const MESSAGE_SUMMARY_COLUMNS: &str = "id, account_id, mailbox, uid, message_id, in_reply_to_json, \
    references_json, subject, sender_json, to_json, cc_json, sent_at, internal_date, flags_json, \
    size_bytes, preview, body_text, CASE WHEN body_html IS NULL THEN NULL ELSE '' END, \
    attachment_names_json, body_fetched, X'', synced_at";
const DRAFT_COLUMNS: &str = "id, account_id, to_json, cc_json, bcc_json, subject, \
    body_text, status, remote_mailbox, remote_uid, created_at, updated_at, raw_rfc822, local_version, \
    has_unsupported_content";
const DRAFT_SYNC_COLUMNS: &str = "id, account_id, to_json, cc_json, bcc_json, subject, \
    body_text, status, remote_mailbox, remote_uid, created_at, updated_at, raw_rfc822, \
    local_version, has_unsupported_content, revision, synced_revision, remote_uid_validity, is_deleted";
const OUTBOX_COLUMNS: &str = "id, account_id, draft_id, draft_revision, draft_local_version, \
    recipients_json, status, attempts, last_error, created_at, sent_at, raw_rfc822";

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

             CREATE TABLE IF NOT EXISTS messages (
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
             CREATE INDEX IF NOT EXISTS idx_messages_inbox
                 ON messages(account_id, mailbox, internal_date DESC, uid DESC);
             CREATE INDEX IF NOT EXISTS idx_messages_message_id
                 ON messages(account_id, message_id);

             CREATE TABLE IF NOT EXISTS drafts (
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
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_drafts_remote_identity
                 ON drafts(account_id, remote_mailbox, remote_uid);
             PRAGMA user_version = 6;",
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

    pub(crate) fn assign_mailbox_role(
        &self,
        account_id: &str,
        role: &str,
        mailbox: &str,
    ) -> Result<()> {
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
            params![account_id, role, mailbox],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn mailbox_for_role(&self, account_id: &str, role: &str) -> Result<String> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT mailbox FROM mailbox_roles WHERE account_id = ?1 AND role = ?2",
                params![account_id, role],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| MailError::NotFound {
                entity: "mailbox role",
                id: format!("{account_id}:{role}"),
            })
    }

    /// Clears cached messages and all cursors after an IMAP UIDVALIDITY change.
    pub(crate) fn reset_mailbox(&self, account_id: &str, mailbox: &str) -> Result<usize> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let removed = transaction.execute(
            "DELETE FROM messages WHERE account_id = ?1 AND mailbox = ?2",
            params![account_id, mailbox],
        )?;
        transaction.execute(
            "UPDATE mailboxes SET uid_validity = NULL, uid_next = NULL,
                 highest_uid = NULL, highest_modseq = NULL, last_synced_at = NULL
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
            removed += transaction.execute(
                "DELETE FROM messages WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3",
                params![account_id, mailbox, uid],
            )?;
        }
        transaction.commit()?;
        Ok(removed)
    }

    /// Inserts a summary or refreshes it without discarding an already-fetched
    /// body when the incoming record contains summary-only data.
    pub(crate) fn upsert_message(&self, message: &InboxMessage) -> Result<i64> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO mailboxes (account_id, name) VALUES (?1, ?2)
             ON CONFLICT(account_id, name) DO NOTHING",
            params![message.account_id, message.mailbox],
        )?;
        let sender_json = message.sender.as_ref().map(encode_json).transpose()?;
        connection.execute(
            "INSERT INTO messages (
                 account_id, mailbox, uid, message_id, in_reply_to_json, references_json, subject, sender_json,
                 to_json, cc_json, sent_at, internal_date, flags_json, size_bytes,
                 preview, body_text, body_html, attachment_names_json, body_fetched,
                 raw_rfc822, synced_at
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                 ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21
             )
             ON CONFLICT(account_id, mailbox, uid) DO UPDATE SET
                 message_id = excluded.message_id,
                 in_reply_to_json = excluded.in_reply_to_json,
                 references_json = excluded.references_json,
                 subject = excluded.subject,
                 sender_json = excluded.sender_json,
                 to_json = excluded.to_json,
                 cc_json = excluded.cc_json,
                 sent_at = excluded.sent_at,
                 internal_date = excluded.internal_date,
                 flags_json = excluded.flags_json,
                 size_bytes = excluded.size_bytes,
                 preview = excluded.preview,
                 body_text = CASE WHEN excluded.body_fetched THEN excluded.body_text ELSE messages.body_text END,
                 body_html = CASE WHEN excluded.body_fetched THEN excluded.body_html ELSE messages.body_html END,
                 attachment_names_json = CASE WHEN excluded.body_fetched THEN excluded.attachment_names_json ELSE messages.attachment_names_json END,
                 body_fetched = MAX(messages.body_fetched, excluded.body_fetched),
                 raw_rfc822 = CASE WHEN excluded.body_fetched THEN excluded.raw_rfc822 ELSE messages.raw_rfc822 END,
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
                message.sent_at,
                message.internal_date,
                encode_json(&message.flags)?,
                message.size_bytes,
                message.preview,
                message.body_text,
                message.body_html,
                encode_json(&message.attachment_names)?,
                message.body_fetched,
                message.raw_rfc822,
                message.synced_at,
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

    pub(crate) fn update_message_flags(
        &self,
        account_id: &str,
        mailbox: &str,
        uid: u32,
        flags: &[String],
    ) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE messages SET flags_json = ?4
             WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3",
            params![account_id, mailbox, uid, encode_json(flags)?],
        )?;
        ensure_changed(changed, "message", format!("{account_id}:{mailbox}/{uid}"))
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

    #[cfg(test)]
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
        connection.execute(
            "INSERT INTO drafts (
                 id, account_id, to_json, cc_json, bcc_json, subject, body_text,
                 status, remote_mailbox, remote_uid, created_at, updated_at, raw_rfc822,
                 local_version, has_unsupported_content, revision, synced_revision,
                 remote_uid_validity, is_deleted
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 ?14, ?15, ?16, ?17, ?18, ?19
             )
             ON CONFLICT(id) DO UPDATE SET
                 account_id = excluded.account_id,
                 to_json = excluded.to_json,
                 cc_json = excluded.cc_json,
                 bcc_json = excluded.bcc_json,
                 subject = excluded.subject,
                 body_text = excluded.body_text,
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
        Ok(())
    }

    /// Inserts a draft only if no row with the same stable id already exists.
    pub(crate) fn insert_draft_if_absent(&self, record: &DraftRecord) -> Result<bool> {
        let connection = self.connection()?;
        Ok(insert_draft_record_if_absent(&connection, record)? == 1)
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
        if let Some(copy) = conflict_copy
            && insert_draft_record_if_absent(&transaction, copy)? != 1
        {
            return Err(MailError::Validation(
                "could not reserve a unique draft conflict copy id".to_owned(),
            ));
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

    pub(crate) fn tombstone_draft(&self, id: &str, updated_at: &str) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE drafts SET
                 is_deleted = 1,
                 status = 'local',
                 revision = revision + 1,
                 local_version = local_version + 1,
                 updated_at = ?2
             WHERE id = ?1 AND status != 'sent'",
            params![id, updated_at],
        )?;
        ensure_changed(changed, "draft", id.to_owned())
    }

    pub(crate) fn tombstone_draft_if_local_version(
        &self,
        account_id: &str,
        id: &str,
        expected_local_version: u64,
        updated_at: &str,
    ) -> Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
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
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO outbox (
                 id, account_id, draft_id, draft_revision, draft_local_version,
                 recipients_json, status, attempts,
                 last_error, created_at, sent_at, raw_rfc822
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
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
            ],
        )?;
        Ok(())
    }

    /// Persists a newly confirmed send that could not enter its first SMTP
    /// attempt. For a newer draft version, obsolete retryable attempts are
    /// terminalized in the same transaction so the user can never later send
    /// both the old and new contents.
    pub(crate) fn enqueue_new_outbox(&self, item: &OutboxItem) -> Result<()> {
        validate_outbox_draft_link(item)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        prepare_new_draft_send(&transaction, item)?;
        let inserted = transaction.execute(
            "INSERT INTO outbox (
                 id, account_id, draft_id, draft_revision, draft_local_version,
                 recipients_json, status, attempts,
                 last_error, created_at, sent_at, raw_rfc822
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
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
            ],
        )?;
        if inserted != 1 {
            return Err(MailError::Validation(format!(
                "outbox item '{}' already exists",
                item.id
            )));
        }
        transaction.commit()?;
        Ok(())
    }

    /// Persists a newly composed message and claims its first SMTP attempt in
    /// one transaction. Other connections can observe either no row or the
    /// final `sending` row, never a live `queued` intermediate that startup
    /// recovery could mistake for an abandoned message.
    pub(crate) fn enqueue_and_claim_outbox(&self, item: &OutboxItem) -> Result<OutboxItem> {
        validate_outbox_draft_link(item)?;
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
                 last_error, created_at, sent_at, raw_rfc822
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', 0, ?7, ?8, ?9, ?10)
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
            ],
        )?;
        if inserted != 1 {
            return Err(MailError::Validation(format!(
                "outbox item '{}' already exists",
                item.id
            )));
        }
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
            "WHERE account_id = ?1 ORDER BY created_at ASC, id ASC",
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
                params![outbox_id],
            )?;
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
    connection
        .execute(
            "INSERT INTO drafts (
                 id, account_id, to_json, cc_json, bcc_json, subject, body_text,
             status, remote_mailbox, remote_uid, created_at, updated_at, raw_rfc822,
                 local_version, has_unsupported_content, revision, synced_revision,
                 remote_uid_validity, is_deleted
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 ?14, ?15, ?16, ?17, ?18, ?19
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
        )
        .map_err(Into::into)
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
    Ok(Draft {
        id: row.get(0)?,
        local_version: decode_u64(13, row.get(13)?)?,
        has_unsupported_content: row.get(14)?,
        account_id: row.get(1)?,
        to: decode_json(2, &row.get::<_, String>(2)?)?,
        cc: decode_json(3, &row.get::<_, String>(3)?)?,
        bcc: decode_json(4, &row.get::<_, String>(4)?)?,
        subject: row.get(5)?,
        body_text: row.get(6)?,
        status: row.get(7)?,
        remote_mailbox: row.get(8)?,
        remote_uid: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        raw_rfc822: row.get(12)?,
    })
}

fn row_to_draft_record(row: &Row<'_>) -> rusqlite::Result<DraftRecord> {
    let draft = row_to_draft(row)?;
    let local_version = draft.local_version;
    Ok(DraftRecord {
        draft,
        local_version,
        revision: decode_u64(15, row.get(15)?)?,
        synced_revision: decode_u64(16, row.get(16)?)?,
        remote_uid_validity: row.get(17)?,
        is_deleted: row.get(18)?,
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
        sync::{Arc, Barrier},
        thread,
    };

    use rusqlite::Connection;
    use tempfile::TempDir;

    use super::{DraftRecord, MailboxState, Repository};
    use crate::{
        AccountConfig, Draft, InboxMessage, MailAddress, MailError, OutboxItem, OutboxStatus,
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
            status,
            attempts,
            last_error: None,
            created_at: format!("2026-07-14T06:00:0{attempts}Z"),
            sent_at: None,
            raw_rfc822: format!("exact bytes for {id}").into_bytes(),
        }
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
            1
        );
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
    fn stale_push_delete_and_remote_import_results_preserve_concurrent_local_state() {
        let (_directory, first, account) = setup();
        let second = Repository::open(&first.path).expect("second repository connection");
        let base = draft_record(&account.account_id, "push-draft", "first edit", 1, 0);
        first.save_draft_record(&base).expect("base draft");
        let sync_snapshot = first
            .get_draft_record(&base.draft.id)
            .expect("sync snapshot");

        let mut concurrent_edit = sync_snapshot.clone();
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
        assert_eq!(version, 6);
        for column in [
            "local_version",
            "has_unsupported_content",
            "revision",
            "synced_revision",
            "remote_uid_validity",
            "is_deleted",
        ] {
            assert!(super::table_has_column(&connection, "drafts", column).unwrap());
        }
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
        assert_eq!(upgraded.raw_rfc822, [1, 2, 3]);

        let connection = repository.connection().expect("connection");
        assert!(super::table_has_column(&connection, "outbox", "draft_revision").unwrap());
        assert!(super::table_has_column(&connection, "outbox", "draft_local_version").unwrap());
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
