use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, types::Type};
use serde::{Serialize, de::DeserializeOwned};

use crate::{AccountConfig, Draft, InboxMessage, MailError, OutboxItem, OutboxStatus, Result};

const MESSAGE_COLUMNS: &str = "id, account_id, mailbox, uid, message_id, subject, \
    sender_json, to_json, cc_json, sent_at, internal_date, flags_json, size_bytes, \
    preview, body_text, body_html, attachment_names_json, body_fetched, raw_rfc822, synced_at";
const DRAFT_COLUMNS: &str = "id, account_id, to_json, cc_json, bcc_json, subject, \
    body_text, status, remote_mailbox, remote_uid, created_at, updated_at, raw_rfc822";
const OUTBOX_COLUMNS: &str = "id, account_id, draft_id, recipients_json, status, \
    attempts, last_error, created_at, sent_at, raw_rfc822";

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

             CREATE TABLE IF NOT EXISTS messages (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 account_id TEXT NOT NULL,
                 mailbox TEXT NOT NULL,
                 uid INTEGER NOT NULL,
                 message_id TEXT,
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
                 FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_drafts_account_updated
                 ON drafts(account_id, updated_at DESC);

             CREATE TABLE IF NOT EXISTS outbox (
                 id TEXT PRIMARY KEY NOT NULL,
                 account_id TEXT NOT NULL,
                 draft_id TEXT,
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
             CREATE UNIQUE INDEX IF NOT EXISTS idx_outbox_unique_draft
                 ON outbox(draft_id) WHERE draft_id IS NOT NULL;
             PRAGMA user_version = 1;",
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
                 account_id, mailbox, uid, message_id, subject, sender_json,
                 to_json, cc_json, sent_at, internal_date, flags_json, size_bytes,
                 preview, body_text, body_html, attachment_names_json, body_fetched,
                 raw_rfc822, synced_at
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                 ?13, ?14, ?15, ?16, ?17, ?18, ?19
             )
             ON CONFLICT(account_id, mailbox, uid) DO UPDATE SET
                 message_id = excluded.message_id,
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
            "SELECT {MESSAGE_COLUMNS} FROM messages
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

    pub(crate) fn count_messages(&self, account_id: &str, mailbox: &str) -> Result<usize> {
        let connection = self.connection()?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND mailbox = ?2",
            params![account_id, mailbox],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    pub(crate) fn save_draft(&self, draft: &Draft) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO drafts (
                 id, account_id, to_json, cc_json, bcc_json, subject, body_text,
                 status, remote_mailbox, remote_uid, created_at, updated_at, raw_rfc822
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
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
                 raw_rfc822 = excluded.raw_rfc822",
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
            ],
        )?;
        Ok(())
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

    pub(crate) fn list_drafts(&self, account_id: &str) -> Result<Vec<Draft>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {DRAFT_COLUMNS} FROM drafts
             WHERE account_id = ?1 ORDER BY updated_at DESC, id DESC"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![account_id], row_to_draft)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn mark_draft_synced(
        &self,
        id: &str,
        mailbox: &str,
        remote_uid: Option<u32>,
    ) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE drafts SET status = 'synced', remote_mailbox = ?2, remote_uid = ?3
             WHERE id = ?1",
            params![id, mailbox, remote_uid],
        )?;
        ensure_changed(changed, "draft", id.to_owned())
    }

    pub(crate) fn enqueue_outbox(&self, item: &OutboxItem) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO outbox (
                 id, account_id, draft_id, recipients_json, status, attempts,
                 last_error, created_at, sent_at, raw_rfc822
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO NOTHING",
            params![
                item.id,
                item.account_id,
                item.draft_id,
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

    pub(crate) fn get_outbox_by_draft(&self, draft_id: &str) -> Result<Option<OutboxItem>> {
        let connection = self.connection()?;
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM outbox WHERE draft_id = ?1");
        connection
            .query_row(&sql, params![draft_id], row_to_outbox)
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

    /// Atomically records a successful SMTP delivery and consumes its draft.
    /// This prevents a crash between two independent state transitions from
    /// leaving a sent draft eligible for another send.
    pub(crate) fn mark_outbox_and_draft_sent(&self, outbox_id: &str, draft_id: &str) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let outbox_changed = transaction.execute(
            "UPDATE outbox SET
                 status = 'sent', last_error = NULL,
                 sent_at = COALESCE(sent_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             WHERE id = ?1 AND draft_id = ?2",
            params![outbox_id, draft_id],
        )?;
        ensure_changed(outbox_changed, "outbox item", outbox_id.to_owned())?;
        let draft_changed = transaction.execute(
            "UPDATE drafts SET status = 'sent' WHERE id = ?1",
            params![draft_id],
        )?;
        ensure_changed(draft_changed, "draft", draft_id.to_owned())?;
        transaction.commit()?;
        Ok(())
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

fn row_to_message(row: &Row<'_>) -> rusqlite::Result<InboxMessage> {
    let sender_json: Option<String> = row.get(6)?;
    Ok(InboxMessage {
        id: row.get(0)?,
        account_id: row.get(1)?,
        mailbox: row.get(2)?,
        uid: row.get(3)?,
        message_id: row.get(4)?,
        subject: row.get(5)?,
        sender: sender_json
            .as_deref()
            .map(|json| decode_json(6, json))
            .transpose()?,
        to: decode_json(7, &row.get::<_, String>(7)?)?,
        cc: decode_json(8, &row.get::<_, String>(8)?)?,
        sent_at: row.get(9)?,
        internal_date: row.get(10)?,
        flags: decode_json(11, &row.get::<_, String>(11)?)?,
        size_bytes: row.get(12)?,
        preview: row.get(13)?,
        body_text: row.get(14)?,
        body_html: row.get(15)?,
        attachment_names: decode_json(16, &row.get::<_, String>(16)?)?,
        body_fetched: row.get(17)?,
        raw_rfc822: row.get(18)?,
        synced_at: row.get(19)?,
    })
}

fn row_to_draft(row: &Row<'_>) -> rusqlite::Result<Draft> {
    Ok(Draft {
        id: row.get(0)?,
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

fn row_to_outbox(row: &Row<'_>) -> rusqlite::Result<OutboxItem> {
    let status_text: String = row.get(4)?;
    let status = OutboxStatus::from_str(&status_text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, Type::Text, Box::new(error))
    })?;
    Ok(OutboxItem {
        id: row.get(0)?,
        account_id: row.get(1)?,
        draft_id: row.get(2)?,
        recipients: decode_json(3, &row.get::<_, String>(3)?)?,
        status,
        attempts: row.get(5)?,
        last_error: row.get(6)?,
        created_at: row.get(7)?,
        sent_at: row.get(8)?,
        raw_rfc822: row.get(9)?,
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

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, fs};

    use tempfile::TempDir;

    use super::{MailboxState, Repository};
    use crate::{AccountConfig, Draft, InboxMessage, MailAddress, OutboxItem, OutboxStatus};

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
        repository.save_draft(&draft).expect("save draft");
        repository
            .mark_draft_synced("draft-1", "Drafts", Some(7))
            .unwrap();
        assert_eq!(repository.get_draft("draft-1").unwrap().remote_uid, Some(7));

        let item = OutboxItem {
            id: "outbox-1".to_owned(),
            account_id: account.account_id.clone(),
            draft_id: Some(draft.id.clone()),
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
        repository
            .mark_outbox_and_draft_sent("outbox-1", "draft-1")
            .unwrap();
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
}
