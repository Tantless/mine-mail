use std::{
    cmp::Ordering,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use mine_mail::{ContactActivity, normalize_contact_email};
use rusqlite::Connection;
use serde::Serialize;

const CONTACTS_DATABASE_NAME: &str = "desktop-contacts.sqlite3";
const CONTACT_REMARK_MAX_CHARACTERS: usize = 80;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ContactListItemDto {
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

#[derive(Clone, Debug)]
struct ContactStore {
    path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContactRecord {
    email: String,
    remark: Option<String>,
    is_favorite: bool,
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
                 ON contacts(is_favorite DESC, remark, email);
             PRAGMA user_version = 2;",
        )?;
        Ok(store)
    }

    fn list_records(&self) -> rusqlite::Result<Vec<ContactRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT email, remark, is_favorite
             FROM contacts
             WHERE is_favorite = 1 OR NULLIF(TRIM(remark), '') IS NOT NULL
             ORDER BY email",
        )?;
        statement
            .query_map([], |row| {
                Ok(ContactRecord {
                    email: row.get(0)?,
                    remark: row.get(1)?,
                    is_favorite: row.get(2)?,
                })
            })?
            .collect()
    }

    fn set_favorite(&self, email: &str, favorite: bool) -> Result<bool, String> {
        let email = normalize_contact_email(email).map_err(|error| error.to_string())?;
        let connection = self
            .connection()
            .map_err(|_| "Contact storage is unavailable.".to_owned())?;
        let changed = if favorite {
            connection.execute(
                "INSERT INTO contacts (
                     email, display_name, remark, is_saved, is_favorite, created_at, updated_at
                 ) VALUES (
                     ?1, NULL, NULL, 0, 1,
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 )
                 ON CONFLICT(email) DO UPDATE SET
                     is_saved = 0,
                     is_favorite = 1,
                     updated_at = excluded.updated_at",
                [email],
            )
        } else {
            (|| -> rusqlite::Result<usize> {
                let updated = connection.execute(
                    "UPDATE contacts
                     SET is_favorite = 0,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE email = ?1 AND is_favorite = 1",
                    [&email],
                )?;
                connection.execute(
                    "DELETE FROM contacts
                     WHERE email = ?1
                       AND is_favorite = 0
                       AND NULLIF(TRIM(remark), '') IS NULL",
                    [&email],
                )?;
                Ok(updated)
            })()
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
            (|| -> rusqlite::Result<usize> {
                let updated = connection.execute(
                    "UPDATE contacts
                     SET remark = NULL,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE email = ?1 AND remark IS NOT NULL",
                    [&email],
                )?;
                connection.execute(
                    "DELETE FROM contacts WHERE email = ?1 AND is_favorite = 0",
                    [&email],
                )?;
                Ok(updated)
            })()
        }
        .map_err(|_| "The contact remark could not be updated.".to_owned())?;
        Ok(changed > 0)
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

    pub(crate) fn list_contacts(
        &self,
        activity: Vec<ContactActivity>,
    ) -> Result<Vec<ContactListItemDto>, String> {
        let records = self
            .store
            .as_ref()
            .ok_or_else(|| "Contact storage is unavailable.".to_owned())?
            .list_records()
            .map_err(|_| "Contacts could not be loaded.".to_owned())?;
        Ok(merge_contacts(records, activity))
    }

    pub(crate) fn set_favorite(&self, email: &str, favorite: bool) -> Result<bool, String> {
        self.store
            .as_ref()
            .ok_or_else(|| "Contact storage is unavailable.".to_owned())?
            .set_favorite(email, favorite)
    }

    pub(crate) fn set_remark(&self, email: &str, remark: &str) -> Result<bool, String> {
        self.store
            .as_ref()
            .ok_or_else(|| "Contact storage is unavailable.".to_owned())?
            .set_remark(email, remark)
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

fn merge_contacts(
    records: Vec<ContactRecord>,
    activity: Vec<ContactActivity>,
) -> Vec<ContactListItemDto> {
    let mut contacts = HashMap::<String, ContactListItemDto>::new();
    for record in records {
        let display_name = record
            .remark
            .clone()
            .unwrap_or_else(|| record.email.clone());
        contacts.insert(
            record.email.clone(),
            ContactListItemDto {
                email: record.email.clone(),
                display_name,
                original_name: record.email,
                remark: record.remark,
                is_favorite: record.is_favorite,
                message_count: 0,
                last_message_at: None,
                last_subject: String::new(),
            },
        );
    }

    for activity in activity {
        let item = contacts
            .entry(activity.email.clone())
            .or_insert_with(|| ContactListItemDto {
                email: activity.email.clone(),
                display_name: activity.email.clone(),
                original_name: activity.email.clone(),
                remark: None,
                is_favorite: false,
                message_count: 0,
                last_message_at: None,
                last_subject: String::new(),
            });
        item.original_name = activity
            .display_name
            .unwrap_or_else(|| activity.email.clone());
        item.display_name = item
            .remark
            .clone()
            .unwrap_or_else(|| item.original_name.clone());
        item.message_count = activity.message_count;
        item.last_message_at = activity.last_message_at;
        item.last_subject = activity.last_subject;
    }

    let mut contacts: Vec<_> = contacts.into_values().collect();
    contacts.sort_by(compare_contact_items);
    contacts
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
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{ContactActivity, ContactRecord, ContactStore, merge_contacts};

    #[test]
    fn contact_store_normalizes_and_persists_favorites_and_remarks_independently() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("contacts.sqlite3");
        let store = ContactStore::open(&path).expect("store");
        assert!(
            store
                .set_favorite("  Friend@Example.COM ", true)
                .expect("favorite")
        );
        assert_eq!(
            store.list_records().expect("list"),
            vec![ContactRecord {
                email: "friend@example.com".to_owned(),
                remark: None,
                is_favorite: true,
            }]
        );
        assert!(store.set_remark("friend@example.com", "  林老师  ").expect("remark"));

        let reopened = ContactStore::open(path).expect("reopen");
        assert_eq!(
            reopened.list_records().expect("reopened list"),
            vec![ContactRecord {
                email: "friend@example.com".to_owned(),
                remark: Some("林老师".to_owned()),
                is_favorite: true,
            }]
        );
        assert!(
            reopened
                .set_favorite("FRIEND@example.com", false)
                .expect("unfavorite")
        );
        assert_eq!(
            reopened.list_records().expect("remark remains"),
            vec![ContactRecord {
                email: "friend@example.com".to_owned(),
                remark: Some("林老师".to_owned()),
                is_favorite: false,
            }]
        );
        assert!(reopened.set_remark("friend@example.com", " ").expect("clear"));
        assert!(reopened.list_records().expect("empty").is_empty());
    }

    #[test]
    fn merge_pins_favorites_and_uses_current_header_names() {
        let result = merge_contacts(
            vec![ContactRecord {
                email: "saved@example.com".to_owned(),
                remark: Some("Local Remark".to_owned()),
                is_favorite: true,
            }],
            vec![
                ContactActivity {
                    email: "recent@example.com".to_owned(),
                    display_name: Some("Recent Header".to_owned()),
                    message_count: 3,
                    last_message_at: Some("2026-07-21T10:00:00Z".to_owned()),
                    last_subject: "Recent mail".to_owned(),
                },
                ContactActivity {
                    email: "saved@example.com".to_owned(),
                    display_name: Some("Remote Header".to_owned()),
                    message_count: 1,
                    last_message_at: Some("2026-07-20T10:00:00Z".to_owned()),
                    last_subject: "Older mail".to_owned(),
                },
            ],
        );

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].email, "saved@example.com");
        assert!(result[0].is_favorite);
        assert_eq!(result[0].display_name, "Local Remark");
        assert_eq!(result[0].original_name, "Remote Header");
        assert_eq!(result[0].remark.as_deref(), Some("Local Remark"));
        assert_eq!(result[0].message_count, 1);
        assert_eq!(result[1].display_name, "Recent Header");
        assert_eq!(result[1].original_name, "Recent Header");
        assert_eq!(result[1].remark, None);
        assert!(!result[1].is_favorite);
    }
}
