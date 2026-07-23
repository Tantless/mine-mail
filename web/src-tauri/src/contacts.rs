use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use mine_mail::{ContactActivity, normalize_contact_email};
use rusqlite::{Connection, params};
use serde::Serialize;

const CONTACTS_DATABASE_NAME: &str = "desktop-contacts.sqlite3";
const CONTACT_REMARK_MAX_CHARACTERS: usize = 80;
const LEGACY_FAVORITE_ACCOUNT_ID: &str = "__legacy_global__";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ContactListItemDto {
    pub account_id: String,
    pub email: String,
    /// Resolved from the local remark, the newest non-empty mail header, then
    /// the normalized email itself. Favorite state never freezes a
    /// sender-owned display name.
    pub display_name: String,
    /// The newest sender-owned mail-header name before applying a local
    /// remark, falling back to the normalized email.
    pub original_name: String,
    pub remark: Option<String>,
    pub is_favorite: bool,
    pub message_count: usize,
    pub last_message_at: Option<String>,
    pub last_subject: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ContactDirectoryDto {
    pub contacts: Vec<ContactListItemDto>,
    pub favorites: Vec<ContactListItemDto>,
}

#[derive(Clone, Debug)]
struct ContactStore {
    path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContactRecord {
    email: String,
    remark: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FavoriteRecord {
    account_id: String,
    email: String,
}

impl ContactStore {
    fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let store = Self {
            path: path.as_ref().to_path_buf(),
        };
        let connection = store.connection()?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS contacts (
                 email TEXT PRIMARY KEY NOT NULL COLLATE NOCASE,
                 display_name TEXT,
                 remark TEXT,
                 is_saved INTEGER NOT NULL DEFAULT 1 CHECK (is_saved IN (0, 1)),
                 is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0, 1)),
                 created_at TEXT NOT NULL
                     DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 updated_at TEXT NOT NULL
                     DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             );
             CREATE TABLE IF NOT EXISTS contact_favorites (
                 account_id TEXT NOT NULL,
                 email TEXT NOT NULL COLLATE NOCASE,
                 created_at TEXT NOT NULL
                     DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 updated_at TEXT NOT NULL
                     DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 PRIMARY KEY (account_id, email)
             );",
        )?;
        let has_remark_column = {
            let mut statement = connection.prepare("PRAGMA table_info(contacts)")?;
            let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
            columns
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|column| column == "remark")
        };
        if !has_remark_column {
            connection.execute("ALTER TABLE contacts ADD COLUMN remark TEXT", [])?;
        }
        connection.execute_batch(
            "DROP INDEX IF EXISTS idx_contacts_saved_favorite;
             CREATE INDEX idx_contacts_saved_favorite
                 ON contacts(remark, email);
             CREATE INDEX IF NOT EXISTS idx_contact_favorites_email
                 ON contact_favorites(email, account_id);",
        )?;
        if connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))? < 3 {
            connection.execute(
                "INSERT OR IGNORE INTO contact_favorites (account_id, email)
                 SELECT ?1, email FROM contacts WHERE is_favorite = 1",
                [LEGACY_FAVORITE_ACCOUNT_ID],
            )?;
            connection.execute("UPDATE contacts SET is_favorite = 0", [])?;
            connection.execute(
                "DELETE FROM contacts
                 WHERE is_favorite = 0
                   AND NULLIF(TRIM(remark), '') IS NULL",
                [],
            )?;
            connection.execute_batch("PRAGMA user_version = 3;")?;
        }
        Ok(store)
    }

    fn list_records(&self) -> rusqlite::Result<Vec<ContactRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT email, remark
             FROM contacts
             WHERE NULLIF(TRIM(remark), '') IS NOT NULL
             ORDER BY email",
        )?;
        statement
            .query_map([], |row| {
                Ok(ContactRecord {
                    email: row.get(0)?,
                    remark: row.get(1)?,
                })
            })?
            .collect()
    }

    fn list_favorites(&self) -> rusqlite::Result<Vec<FavoriteRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT account_id, email
             FROM contact_favorites
             ORDER BY account_id, email",
        )?;
        statement
            .query_map([], |row| {
                Ok(FavoriteRecord {
                    account_id: row.get(0)?,
                    email: row.get(1)?,
                })
            })?
            .collect()
    }

    fn resolve_legacy_favorites(
        &self,
        activity_by_account: &[(String, Vec<ContactActivity>)],
        fallback_account_id: &str,
    ) -> rusqlite::Result<()> {
        let legacy = self
            .list_favorites()?
            .into_iter()
            .filter(|record| record.account_id == LEGACY_FAVORITE_ACCOUNT_ID)
            .collect::<Vec<_>>();
        if legacy.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        for record in legacy {
            // Older Mine Mail builds stored favorites without an account. Keep
            // them under every account where cached correspondence proves the
            // contact existed; if no cache remains, preserve the favorite
            // under the account that triggered the migration.
            let mut matching_accounts = activity_by_account
                .iter()
                .filter(|(_, activity)| activity.iter().any(|item| item.email == record.email))
                .map(|(account_id, _)| account_id.as_str())
                .collect::<Vec<_>>();
            if matching_accounts.is_empty() {
                matching_accounts.push(fallback_account_id);
            }
            for account_id in matching_accounts {
                transaction.execute(
                    "INSERT OR IGNORE INTO contact_favorites (account_id, email)
                     VALUES (?1, ?2)",
                    params![account_id, record.email],
                )?;
            }
            transaction.execute(
                "DELETE FROM contact_favorites WHERE account_id = ?1 AND email = ?2",
                params![LEGACY_FAVORITE_ACCOUNT_ID, record.email],
            )?;
        }
        transaction.commit()
    }

    fn set_favorite(&self, account_id: &str, email: &str, favorite: bool) -> Result<bool, String> {
        if account_id.trim().is_empty() || account_id.chars().any(char::is_control) {
            return Err("A valid account is required for a contact favorite.".to_owned());
        }
        let email = normalize_contact_email(email).map_err(|error| error.to_string())?;
        let connection = self
            .connection()
            .map_err(|_| "Contact storage is unavailable.".to_owned())?;
        let changed = if favorite {
            connection.execute(
                "INSERT INTO contact_favorites (
                     account_id, email, created_at, updated_at
                 ) VALUES (?1, ?2,
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 )
                 ON CONFLICT(account_id, email) DO UPDATE SET
                     updated_at = excluded.updated_at",
                params![account_id, email],
            )
        } else {
            connection.execute(
                "DELETE FROM contact_favorites
                 WHERE account_id = ?1 AND email = ?2",
                params![account_id, email],
            )
        }
        .map_err(|_| "The contact favorite could not be updated.".to_owned())?;
        Ok(changed > 0)
    }

    fn set_remark(&self, email: &str, remark: &str) -> Result<bool, String> {
        let email = normalize_contact_email(email).map_err(|error| error.to_string())?;
        let remark = normalize_contact_remark(remark)?;
        let connection = self
            .connection()
            .map_err(|_| "Contact storage is unavailable.".to_owned())?;
        let changed = if let Some(remark) = remark {
            connection.execute(
                "INSERT INTO contacts (
                     email, display_name, remark, is_saved, is_favorite, created_at, updated_at
                 ) VALUES (
                     ?1, NULL, ?2, 0, 0,
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 )
                 ON CONFLICT(email) DO UPDATE SET
                     remark = excluded.remark,
                     is_saved = 0,
                     updated_at = excluded.updated_at",
                (&email, remark),
            )
        } else {
            connection.execute("DELETE FROM contacts WHERE email = ?1", [&email])
        }
        .map_err(|_| "The contact remark could not be updated.".to_owned())?;
        Ok(changed > 0)
    }

    fn remove_account_favorites(&self, account_id: &str) -> Result<(), String> {
        self.connection()
            .map_err(|_| "Contact storage is unavailable.".to_owned())?
            .execute(
                "DELETE FROM contact_favorites WHERE account_id = ?1",
                [account_id],
            )
            .map(|_| ())
            .map_err(|_| "The removed account's favorites could not be cleared.".to_owned())
    }

    fn connection(&self) -> rusqlite::Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        Ok(connection)
    }
}

pub(crate) struct ContactRuntime {
    store: Option<ContactStore>,
}

impl ContactRuntime {
    pub(crate) fn open(app_data: &Path) -> Self {
        let store = fs::create_dir_all(app_data)
            .ok()
            .and_then(|()| ContactStore::open(app_data.join(CONTACTS_DATABASE_NAME)).ok());
        Self { store }
    }

    pub(crate) fn list_directory(
        &self,
        active_account_id: &str,
        activity_by_account: Vec<(String, Vec<ContactActivity>)>,
    ) -> Result<ContactDirectoryDto, String> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| "Contact storage is unavailable.".to_owned())?;
        store
            .resolve_legacy_favorites(&activity_by_account, active_account_id)
            .map_err(|_| "Existing contact favorites could not be upgraded.".to_owned())?;
        let records = store
            .list_records()
            .map_err(|_| "Contacts could not be loaded.".to_owned())?;
        let favorites = store
            .list_favorites()
            .map_err(|_| "Contact favorites could not be loaded.".to_owned())?;
        Ok(build_directory(
            records,
            favorites,
            active_account_id,
            activity_by_account,
        ))
    }

    pub(crate) fn set_favorite(
        &self,
        account_id: &str,
        email: &str,
        favorite: bool,
    ) -> Result<bool, String> {
        self.store
            .as_ref()
            .ok_or_else(|| "Contact storage is unavailable.".to_owned())?
            .set_favorite(account_id, email, favorite)
    }

    pub(crate) fn set_remark(&self, email: &str, remark: &str) -> Result<bool, String> {
        self.store
            .as_ref()
            .ok_or_else(|| "Contact storage is unavailable.".to_owned())?
            .set_remark(email, remark)
    }

    pub(crate) fn remove_account(&self, account_id: &str) -> Result<(), String> {
        self.store
            .as_ref()
            .ok_or_else(|| "Contact storage is unavailable.".to_owned())?
            .remove_account_favorites(account_id)
    }
}

fn normalize_contact_remark(value: &str) -> Result<Option<String>, String> {
    let remark = value.trim();
    if remark.is_empty() {
        return Ok(None);
    }
    if remark.chars().count() > CONTACT_REMARK_MAX_CHARACTERS {
        return Err(format!(
            "Contact remarks can contain at most {CONTACT_REMARK_MAX_CHARACTERS} characters."
        ));
    }
    if remark.chars().any(char::is_control) {
        return Err("Contact remarks cannot contain control characters.".to_owned());
    }
    Ok(Some(remark.to_owned()))
}

fn build_directory(
    records: Vec<ContactRecord>,
    favorites: Vec<FavoriteRecord>,
    active_account_id: &str,
    activity_by_account: Vec<(String, Vec<ContactActivity>)>,
) -> ContactDirectoryDto {
    let remarks = records
        .into_iter()
        .map(|record| (record.email, record.remark))
        .collect::<HashMap<_, _>>();
    let activity_by_account = activity_by_account
        .into_iter()
        .map(|(account_id, activity)| {
            (
                account_id,
                activity
                    .into_iter()
                    .map(|item| (item.email.clone(), item))
                    .collect::<HashMap<_, _>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let favorite_keys = favorites
        .iter()
        .map(|record| (record.account_id.clone(), record.email.clone()))
        .collect::<HashSet<_>>();

    let mut contacts = activity_by_account
        .get(active_account_id)
        .into_iter()
        .flat_map(|activity| activity.values())
        .map(|activity| {
            contact_item(
                active_account_id,
                activity.email.as_str(),
                Some(activity),
                remarks.get(&activity.email).and_then(Option::as_ref),
                favorite_keys.contains(&(active_account_id.to_owned(), activity.email.clone())),
            )
        })
        .collect::<Vec<_>>();
    contacts.sort_by(compare_contact_items);

    let mut favorite_contacts = favorites
        .into_iter()
        .filter(|record| record.account_id != LEGACY_FAVORITE_ACCOUNT_ID)
        .map(|record| {
            let activity = activity_by_account
                .get(&record.account_id)
                .and_then(|items| items.get(&record.email));
            contact_item(
                &record.account_id,
                &record.email,
                activity,
                remarks.get(&record.email).and_then(Option::as_ref),
                true,
            )
        })
        .collect::<Vec<_>>();
    favorite_contacts.sort_by(compare_contact_items);

    ContactDirectoryDto {
        contacts,
        favorites: favorite_contacts,
    }
}

fn contact_item(
    account_id: &str,
    email: &str,
    activity: Option<&ContactActivity>,
    remark: Option<&String>,
    is_favorite: bool,
) -> ContactListItemDto {
    let original_name = activity
        .and_then(|item| item.display_name.clone())
        .unwrap_or_else(|| email.to_owned());
    ContactListItemDto {
        account_id: account_id.to_owned(),
        email: email.to_owned(),
        display_name: remark.cloned().unwrap_or_else(|| original_name.clone()),
        original_name,
        remark: remark.cloned(),
        is_favorite,
        message_count: activity.map_or(0, |item| item.message_count),
        last_message_at: activity.and_then(|item| item.last_message_at.clone()),
        last_subject: activity.map_or_else(String::new, |item| item.last_subject.clone()),
    }
}

fn compare_contact_items(left: &ContactListItemDto, right: &ContactListItemDto) -> Ordering {
    right
        .is_favorite
        .cmp(&left.is_favorite)
        .then_with(|| right.last_message_at.cmp(&left.last_message_at))
        .then_with(|| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase())
        })
        .then_with(|| left.email.cmp(&right.email))
        .then_with(|| left.account_id.cmp(&right.account_id))
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::{ContactActivity, ContactRecord, ContactStore, FavoriteRecord, build_directory};

    #[test]
    fn contact_store_migrates_the_existing_favorites_database() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("contacts.sqlite3");
        let connection = Connection::open(&path).expect("legacy database");
        connection
            .execute_batch(
                "CREATE TABLE contacts (
                     email TEXT PRIMARY KEY NOT NULL COLLATE NOCASE,
                     display_name TEXT,
                     is_saved INTEGER NOT NULL DEFAULT 1,
                     is_favorite INTEGER NOT NULL DEFAULT 0,
                     created_at TEXT NOT NULL,
                     updated_at TEXT NOT NULL
                 );
                 INSERT INTO contacts VALUES (
                     'friend@example.com', NULL, 0, 1,
                     '2026-07-01T00:00:00Z', '2026-07-01T00:00:00Z'
                 );
                 PRAGMA user_version = 1;",
            )
            .expect("legacy schema");
        drop(connection);

        let store = ContactStore::open(&path).expect("migrated store");
        assert!(store.list_records().expect("metadata only").is_empty());
        store
            .resolve_legacy_favorites(
                &[
                    (
                        "account-163".to_owned(),
                        vec![activity("friend@example.com", "163 Friend", 2)],
                    ),
                    (
                        "account-gmail".to_owned(),
                        vec![activity("friend@example.com", "Gmail Friend", 1)],
                    ),
                ],
                "account-163",
            )
            .expect("resolve legacy favorite");
        assert_eq!(
            store.list_favorites().expect("preserved favorites"),
            vec![
                FavoriteRecord {
                    account_id: "account-163".to_owned(),
                    email: "friend@example.com".to_owned(),
                },
                FavoriteRecord {
                    account_id: "account-gmail".to_owned(),
                    email: "friend@example.com".to_owned(),
                },
            ]
        );
        assert!(
            store
                .set_remark("friend@example.com", "旧友")
                .expect("remark")
        );

        let connection = Connection::open(path).expect("migrated database");
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .expect("schema version"),
            3
        );
    }

    #[test]
    fn contact_store_normalizes_and_persists_favorites_and_remarks_independently() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("contacts.sqlite3");
        let store = ContactStore::open(&path).expect("store");
        assert!(
            store
                .set_favorite("account-163", "  Friend@Example.COM ", true)
                .expect("favorite")
        );
        assert_eq!(
            store.list_favorites().expect("list"),
            vec![FavoriteRecord {
                account_id: "account-163".to_owned(),
                email: "friend@example.com".to_owned(),
            }]
        );
        assert!(store.list_records().expect("no remark yet").is_empty());
        assert!(
            store
                .set_remark("friend@example.com", "  林老师  ")
                .expect("remark")
        );

        let reopened = ContactStore::open(path).expect("reopen");
        assert_eq!(
            reopened.list_records().expect("reopened list"),
            vec![ContactRecord {
                email: "friend@example.com".to_owned(),
                remark: Some("林老师".to_owned()),
            }]
        );
        assert!(
            reopened
                .set_favorite("account-163", "FRIEND@example.com", false)
                .expect("unfavorite")
        );
        assert_eq!(
            reopened.list_records().expect("remark remains"),
            vec![ContactRecord {
                email: "friend@example.com".to_owned(),
                remark: Some("林老师".to_owned()),
            }]
        );
        assert!(
            reopened
                .set_remark("friend@example.com", " ")
                .expect("clear")
        );
        assert!(reopened.list_records().expect("empty").is_empty());
        assert!(reopened.list_favorites().expect("no favorites").is_empty());
    }

    #[test]
    fn directory_keeps_all_current_and_favorites_app_wide_with_account_scope() {
        let result = build_directory(
            vec![ContactRecord {
                email: "saved@example.com".to_owned(),
                remark: Some("Local Remark".to_owned()),
            }],
            vec![
                FavoriteRecord {
                    account_id: "account-163".to_owned(),
                    email: "saved@example.com".to_owned(),
                },
                FavoriteRecord {
                    account_id: "account-gmail".to_owned(),
                    email: "gmail-only@example.com".to_owned(),
                },
            ],
            "account-163",
            vec![
                (
                    "account-163".to_owned(),
                    vec![
                        activity("recent@example.com", "Recent Header", 3),
                        activity("saved@example.com", "Remote Header", 1),
                    ],
                ),
                (
                    "account-gmail".to_owned(),
                    vec![activity("gmail-only@example.com", "Gmail Friend", 4)],
                ),
            ],
        );

        assert_eq!(result.contacts.len(), 2);
        assert_eq!(result.contacts[0].email, "saved@example.com");
        assert_eq!(result.contacts[0].account_id, "account-163");
        assert!(result.contacts[0].is_favorite);
        assert_eq!(result.contacts[0].display_name, "Local Remark");
        assert_eq!(result.contacts[1].email, "recent@example.com");
        assert!(!result.contacts[1].is_favorite);

        assert_eq!(result.favorites.len(), 2);
        assert!(result.favorites.iter().any(|item| {
            item.account_id == "account-163"
                && item.email == "saved@example.com"
                && item.message_count == 1
        }));
        assert!(result.favorites.iter().any(|item| {
            item.account_id == "account-gmail"
                && item.email == "gmail-only@example.com"
                && item.message_count == 4
        }));
        assert!(
            !result
                .contacts
                .iter()
                .any(|item| { item.email == "gmail-only@example.com" })
        );
    }

    fn activity(email: &str, display_name: &str, message_count: usize) -> ContactActivity {
        ContactActivity {
            email: email.to_owned(),
            display_name: Some(display_name.to_owned()),
            message_count,
            last_message_at: Some("2026-07-21T10:00:00Z".to_owned()),
            last_subject: "Hello".to_owned(),
        }
    }
}
