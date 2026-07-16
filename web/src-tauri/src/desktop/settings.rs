use std::{path::Path, path::PathBuf, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

pub(super) const DEFAULT_POLL_INTERVAL_MINUTES: u8 = 5;
const MAX_PROFILE_AVATAR_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProfileAvatarOwnerType {
    Account,
    Contact,
}

impl ProfileAvatarOwnerType {
    fn as_storage_value(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Contact => "contact",
        }
    }

    fn from_storage_value(value: &str) -> rusqlite::Result<Self> {
        match value {
            "account" => Ok(Self::Account),
            "contact" => Ok(Self::Contact),
            _ => Err(rusqlite::Error::InvalidColumnType(
                0,
                "owner_type".to_owned(),
                rusqlite::types::Type::Text,
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ProfileAvatarDto {
    pub owner_type: ProfileAvatarOwnerType,
    pub owner_key: String,
    pub image_data_url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SaveProfileAvatarRequest {
    pub owner_type: ProfileAvatarOwnerType,
    pub owner_key: String,
    pub image_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct DeleteProfileAvatarRequest {
    pub owner_type: ProfileAvatarOwnerType,
    pub owner_key: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RemoteImageMode {
    #[default]
    Automatic,
    Ask,
    Blocked,
}

impl RemoteImageMode {
    fn as_storage_value(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Ask => "ask",
            Self::Blocked => "blocked",
        }
    }

    fn from_storage_value(value: &str) -> Self {
        match value {
            "automatic" => Self::Automatic,
            "ask" => Self::Ask,
            "blocked" => Self::Blocked,
            _ => Self::Ask,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StoredDesktopSettings {
    pub background_enabled: bool,
    pub poll_interval_minutes: u8,
    pub notifications_enabled: bool,
    pub remote_image_mode: RemoteImageMode,
    pub notification_baseline_initialized: bool,
    pub notification_baseline_uid: u32,
}

impl Default for StoredDesktopSettings {
    fn default() -> Self {
        Self {
            background_enabled: true,
            poll_interval_minutes: DEFAULT_POLL_INTERVAL_MINUTES,
            notifications_enabled: true,
            remote_image_mode: RemoteImageMode::Automatic,
            notification_baseline_initialized: false,
            notification_baseline_uid: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct DesktopSettingsUpdate {
    pub background_enabled: Option<bool>,
    pub poll_interval_minutes: Option<u8>,
    pub notifications_enabled: Option<bool>,
    pub remote_image_mode: Option<RemoteImageMode>,
    pub autostart_enabled: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DesktopSettingsDto {
    pub background_enabled: bool,
    pub poll_interval_minutes: u8,
    pub notifications_enabled: bool,
    pub remote_image_mode: RemoteImageMode,
    pub autostart_enabled: bool,
    pub startup_error: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct DesktopSettingsStore {
    path: PathBuf,
}

impl DesktopSettingsStore {
    pub(super) fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let store = Self {
            path: path.as_ref().to_path_buf(),
        };
        let connection = store.connection()?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS desktop_settings (
                 id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
                 background_enabled INTEGER NOT NULL CHECK (background_enabled IN (0, 1)),
                 poll_interval_minutes INTEGER NOT NULL
                     CHECK (poll_interval_minutes IN (1, 3, 5)),
                 notifications_enabled INTEGER NOT NULL CHECK (notifications_enabled IN (0, 1)),
                 notification_baseline_initialized INTEGER NOT NULL
                     CHECK (notification_baseline_initialized IN (0, 1)),
                 notification_baseline_uid INTEGER NOT NULL DEFAULT 0,
                 remote_image_mode TEXT NOT NULL DEFAULT 'automatic'
                     CHECK (remote_image_mode IN ('automatic', 'ask', 'blocked')),
                 updated_at TEXT NOT NULL
                     DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             );
             CREATE TABLE IF NOT EXISTS profile_avatars (
                 owner_type TEXT NOT NULL
                     CHECK (owner_type IN ('account', 'contact')),
                 owner_key TEXT NOT NULL,
                 mime_type TEXT NOT NULL
                     CHECK (mime_type IN ('image/png', 'image/jpeg', 'image/webp')),
                 image_bytes BLOB NOT NULL,
                 updated_at TEXT NOT NULL
                     DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 PRIMARY KEY (owner_type, owner_key)
             );
             INSERT INTO desktop_settings (
                 id, background_enabled, poll_interval_minutes,
                 notifications_enabled, notification_baseline_initialized,
                 notification_baseline_uid
             ) VALUES (1, 1, 5, 1, 0, 0)
             ON CONFLICT(id) DO NOTHING;",
        )?;
        let has_remote_image_mode = {
            let mut statement = connection.prepare("PRAGMA table_info(desktop_settings)")?;
            let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
            let mut found = false;
            for column in columns {
                if column? == "remote_image_mode" {
                    found = true;
                    break;
                }
            }
            found
        };
        if !has_remote_image_mode {
            connection.execute(
                "ALTER TABLE desktop_settings
                 ADD COLUMN remote_image_mode TEXT NOT NULL DEFAULT 'automatic'
                     CHECK (remote_image_mode IN ('automatic', 'ask', 'blocked'))",
                [],
            )?;
        }
        Ok(store)
    }

    pub(super) fn load(&self) -> rusqlite::Result<StoredDesktopSettings> {
        self.connection()?.query_row(
            "SELECT background_enabled, poll_interval_minutes,
                    notifications_enabled, notification_baseline_initialized,
                    notification_baseline_uid, remote_image_mode
             FROM desktop_settings WHERE id = 1",
            [],
            |row| {
                Ok(StoredDesktopSettings {
                    background_enabled: row.get::<_, i64>(0)? != 0,
                    poll_interval_minutes: row.get(1)?,
                    notifications_enabled: row.get::<_, i64>(2)? != 0,
                    notification_baseline_initialized: row.get::<_, i64>(3)? != 0,
                    notification_baseline_uid: row.get(4)?,
                    remote_image_mode: RemoteImageMode::from_storage_value(
                        &row.get::<_, String>(5)?,
                    ),
                })
            },
        )
    }

    pub(super) fn save(&self, settings: StoredDesktopSettings) -> rusqlite::Result<()> {
        self.connection()?.execute(
            "UPDATE desktop_settings SET
                 background_enabled = ?1,
                 poll_interval_minutes = ?2,
                 notifications_enabled = ?3,
                 notification_baseline_initialized = ?4,
                 notification_baseline_uid = ?5,
                 remote_image_mode = ?6,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = 1",
            params![
                settings.background_enabled,
                settings.poll_interval_minutes,
                settings.notifications_enabled,
                settings.notification_baseline_initialized,
                settings.notification_baseline_uid,
                settings.remote_image_mode.as_storage_value(),
            ],
        )?;
        Ok(())
    }

    pub(super) fn list_profile_avatars(&self) -> rusqlite::Result<Vec<ProfileAvatarDto>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT owner_type, owner_key, mime_type, image_bytes
             FROM profile_avatars
             ORDER BY owner_type, owner_key",
        )?;
        statement
            .query_map([], |row| {
                Ok(ProfileAvatarDto {
                    owner_type: ProfileAvatarOwnerType::from_storage_value(
                        &row.get::<_, String>(0)?,
                    )?,
                    owner_key: row.get(1)?,
                    image_data_url: avatar_data_url(
                        &row.get::<_, String>(2)?,
                        &row.get::<_, Vec<u8>>(3)?,
                    ),
                })
            })?
            .collect()
    }

    pub(super) fn save_profile_avatar(
        &self,
        request: SaveProfileAvatarRequest,
    ) -> Result<ProfileAvatarDto, String> {
        let owner_key = normalize_avatar_owner_key(&request.owner_key)?;
        let mime_type = sniff_avatar_mime_type(&request.image_bytes)?;
        self.connection()
            .map_err(|_| "The avatar store is unavailable.".to_owned())?
            .execute(
                "INSERT INTO profile_avatars (
                     owner_type, owner_key, mime_type, image_bytes, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ON CONFLICT(owner_type, owner_key) DO UPDATE SET
                     mime_type = excluded.mime_type,
                     image_bytes = excluded.image_bytes,
                     updated_at = excluded.updated_at",
                params![
                    request.owner_type.as_storage_value(),
                    owner_key,
                    mime_type,
                    request.image_bytes,
                ],
            )
            .map_err(|_| "The avatar could not be saved.".to_owned())?;
        Ok(ProfileAvatarDto {
            owner_type: request.owner_type,
            owner_key,
            image_data_url: avatar_data_url(mime_type, &request.image_bytes),
        })
    }

    pub(super) fn delete_profile_avatar(
        &self,
        request: DeleteProfileAvatarRequest,
    ) -> Result<(), String> {
        let owner_key = normalize_avatar_owner_key(&request.owner_key)?;
        self.connection()
            .map_err(|_| "The avatar store is unavailable.".to_owned())?
            .execute(
                "DELETE FROM profile_avatars WHERE owner_type = ?1 AND owner_key = ?2",
                params![request.owner_type.as_storage_value(), owner_key],
            )
            .map_err(|_| "The avatar could not be removed.".to_owned())?;
        Ok(())
    }

    fn connection(&self) -> rusqlite::Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        Ok(connection)
    }
}

fn normalize_avatar_owner_key(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    let valid = normalized.len() <= 320
        && normalized.contains('@')
        && !normalized.chars().any(char::is_whitespace)
        && normalized
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'));
    if !valid {
        return Err("A valid email address is required for an avatar.".to_owned());
    }
    Ok(normalized)
}

fn sniff_avatar_mime_type(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.is_empty() || bytes.len() > MAX_PROFILE_AVATAR_BYTES {
        return Err("Avatar images must be no larger than 2 MB.".to_owned());
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Ok("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok("image/jpeg");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok("image/webp");
    }
    Err("Only PNG, JPEG, and WebP avatar images are supported.".to_owned())
}

fn avatar_data_url(mime_type: &str, bytes: &[u8]) -> String {
    format!("data:{mime_type};base64,{}", BASE64_STANDARD.encode(bytes))
}

pub(super) fn valid_poll_interval(value: u8) -> bool {
    matches!(value, 1 | 3 | 5)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use rusqlite::Connection;

    use super::{
        DeleteProfileAvatarRequest, DesktopSettingsStore, ProfileAvatarOwnerType, RemoteImageMode,
        SaveProfileAvatarRequest, StoredDesktopSettings,
    };

    #[test]
    fn settings_are_persisted_with_safe_defaults() {
        let directory = tempdir().expect("temporary directory");
        let store = DesktopSettingsStore::open(directory.path().join("desktop.sqlite3"))
            .expect("settings store");

        let defaults = store.load().expect("default settings");
        assert!(defaults.background_enabled);
        assert!(defaults.notifications_enabled);
        assert_eq!(defaults.poll_interval_minutes, 5);
        assert_eq!(defaults.remote_image_mode, RemoteImageMode::Automatic);
        assert!(!defaults.notification_baseline_initialized);

        let updated = StoredDesktopSettings {
            background_enabled: false,
            poll_interval_minutes: 3,
            notifications_enabled: false,
            remote_image_mode: RemoteImageMode::Blocked,
            notification_baseline_initialized: true,
            notification_baseline_uid: 42,
        };
        store.save(updated).expect("save settings");
        assert_eq!(store.load().expect("updated settings"), updated);
    }

    #[test]
    fn existing_settings_database_migrates_to_automatic_remote_images() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("desktop.sqlite3");
        let connection = Connection::open(&path).expect("legacy settings database");
        connection
            .execute_batch(
                "CREATE TABLE desktop_settings (
                     id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
                     background_enabled INTEGER NOT NULL CHECK (background_enabled IN (0, 1)),
                     poll_interval_minutes INTEGER NOT NULL,
                     notifications_enabled INTEGER NOT NULL CHECK (notifications_enabled IN (0, 1)),
                     notification_baseline_initialized INTEGER NOT NULL,
                     notification_baseline_uid INTEGER NOT NULL DEFAULT 0,
                     updated_at TEXT NOT NULL DEFAULT ''
                 );
                 INSERT INTO desktop_settings VALUES (1, 1, 5, 1, 0, 0, '');",
            )
            .expect("legacy schema");
        drop(connection);

        let store = DesktopSettingsStore::open(&path).expect("migrated settings store");
        assert_eq!(
            store.load().expect("migrated settings").remote_image_mode,
            RemoteImageMode::Automatic,
        );
    }

    #[test]
    fn profile_avatars_are_bounded_normalized_and_removable() {
        let directory = tempdir().expect("temporary directory");
        let store = DesktopSettingsStore::open(directory.path().join("desktop.sqlite3"))
            .expect("settings store");
        let png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];

        let saved = store
            .save_profile_avatar(SaveProfileAvatarRequest {
                owner_type: ProfileAvatarOwnerType::Contact,
                owner_key: "  Friend@Example.COM ".to_owned(),
                image_bytes: png.clone(),
            })
            .expect("save avatar");
        assert_eq!(saved.owner_key, "friend@example.com");
        assert!(saved.image_data_url.starts_with("data:image/png;base64,"));
        assert_eq!(store.list_profile_avatars().expect("list"), vec![saved]);

        store
            .delete_profile_avatar(DeleteProfileAvatarRequest {
                owner_type: ProfileAvatarOwnerType::Contact,
                owner_key: "FRIEND@example.com".to_owned(),
            })
            .expect("delete avatar");
        assert!(
            store
                .list_profile_avatars()
                .expect("list after delete")
                .is_empty()
        );
    }

    #[test]
    fn profile_avatars_reject_untrusted_image_formats() {
        let directory = tempdir().expect("temporary directory");
        let store = DesktopSettingsStore::open(directory.path().join("desktop.sqlite3"))
            .expect("settings store");
        let error = store
            .save_profile_avatar(SaveProfileAvatarRequest {
                owner_type: ProfileAvatarOwnerType::Account,
                owner_key: "me@example.com".to_owned(),
                image_bytes: b"<svg><script/></svg>".to_vec(),
            })
            .expect_err("SVG must be rejected");
        assert!(error.contains("PNG, JPEG, and WebP"));
    }
}
