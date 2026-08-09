use std::{path::Path, path::PathBuf, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub(super) const DEFAULT_POLL_INTERVAL_MINUTES: u8 = 5;
pub(crate) const MCP_ENDPOINT: &str = "http://127.0.0.1:46321/mcp";
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NotificationDelivery {
    #[default]
    MineMail,
    Windows,
}

impl NotificationDelivery {
    fn as_storage_value(self) -> &'static str {
        match self {
            Self::MineMail => "mine_mail",
            Self::Windows => "windows",
        }
    }

    fn from_storage_value(value: &str) -> Self {
        match value {
            "windows" => Self::Windows,
            _ => Self::MineMail,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NotificationSound {
    Default,
    #[default]
    Mail,
    Im,
    Reminder,
}

impl NotificationSound {
    fn as_storage_value(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Mail => "mail",
            Self::Im => "im",
            Self::Reminder => "reminder",
        }
    }

    fn from_storage_value(value: &str) -> Self {
        match value {
            "default" => Self::Default,
            "mail" => Self::Mail,
            "im" => Self::Im,
            "reminder" => Self::Reminder,
            _ => Self::Mail,
        }
    }

    #[cfg(target_os = "windows")]
    pub(super) fn system_resource_name(self) -> &'static str {
        match self {
            Self::Default => "Notification.Default",
            Self::Mail => "Notification.Mail",
            Self::Im => "Notification.IM",
            Self::Reminder => "Notification.Reminder",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StoredDesktopSettings {
    pub background_enabled: bool,
    pub poll_interval_minutes: u8,
    pub notifications_enabled: bool,
    pub notification_delivery: NotificationDelivery,
    pub notification_sound_enabled: bool,
    pub notification_sound: NotificationSound,
    pub remote_image_mode: RemoteImageMode,
    pub ai_assistant_default_open: bool,
    pub mcp_enabled: bool,
    pub mcp_information_enabled: bool,
    pub mcp_send_enabled: bool,
    pub notification_baseline_initialized: bool,
    pub notification_baseline_uid: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct NotificationBaseline {
    pub initialized: bool,
    pub uid: u32,
}

impl Default for StoredDesktopSettings {
    fn default() -> Self {
        Self {
            background_enabled: true,
            poll_interval_minutes: DEFAULT_POLL_INTERVAL_MINUTES,
            notifications_enabled: true,
            notification_delivery: NotificationDelivery::MineMail,
            notification_sound_enabled: true,
            notification_sound: NotificationSound::Mail,
            remote_image_mode: RemoteImageMode::Automatic,
            ai_assistant_default_open: true,
            mcp_enabled: false,
            mcp_information_enabled: true,
            mcp_send_enabled: false,
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
    pub notification_delivery: Option<NotificationDelivery>,
    pub notification_sound_enabled: Option<bool>,
    pub notification_sound: Option<NotificationSound>,
    pub remote_image_mode: Option<RemoteImageMode>,
    pub ai_assistant_default_open: Option<bool>,
    pub mcp_enabled: Option<bool>,
    pub mcp_information_enabled: Option<bool>,
    pub mcp_send_enabled: Option<bool>,
    pub autostart_enabled: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DesktopSettingsDto {
    pub background_enabled: bool,
    pub poll_interval_minutes: u8,
    pub notifications_enabled: bool,
    pub notification_delivery: NotificationDelivery,
    pub windows_notifications_available: bool,
    pub notification_sound_enabled: bool,
    pub notification_sound: NotificationSound,
    pub remote_image_mode: RemoteImageMode,
    pub ai_assistant_default_open: bool,
    pub mcp_enabled: bool,
    pub mcp_information_enabled: bool,
    pub mcp_send_enabled: bool,
    pub mcp_endpoint: &'static str,
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
                 notification_delivery TEXT NOT NULL DEFAULT 'mine_mail'
                     CHECK (notification_delivery IN ('mine_mail', 'windows')),
                 foreground_notifications_enabled INTEGER NOT NULL DEFAULT 1
                     CHECK (foreground_notifications_enabled IN (0, 1)),
                 notification_sound_enabled INTEGER NOT NULL DEFAULT 1
                     CHECK (notification_sound_enabled IN (0, 1)),
                 notification_sound TEXT NOT NULL DEFAULT 'mail'
                     CHECK (notification_sound IN ('default', 'mail', 'im', 'reminder')),
                 notification_baseline_initialized INTEGER NOT NULL
                     CHECK (notification_baseline_initialized IN (0, 1)),
                 notification_baseline_uid INTEGER NOT NULL DEFAULT 0,
                 remote_image_mode TEXT NOT NULL DEFAULT 'automatic'
                     CHECK (remote_image_mode IN ('automatic', 'ask', 'blocked')),
                 ai_assistant_default_open INTEGER NOT NULL DEFAULT 1
                     CHECK (ai_assistant_default_open IN (0, 1)),
                 mcp_enabled INTEGER NOT NULL DEFAULT 0
                     CHECK (mcp_enabled IN (0, 1)),
                 mcp_information_enabled INTEGER NOT NULL DEFAULT 1
                     CHECK (mcp_information_enabled IN (0, 1)),
                 mcp_send_enabled INTEGER NOT NULL DEFAULT 0
                     CHECK (mcp_send_enabled IN (0, 1)),
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
             CREATE TABLE IF NOT EXISTS account_notification_baselines (
                 account_id TEXT PRIMARY KEY NOT NULL,
                 initialized INTEGER NOT NULL DEFAULT 0
                     CHECK (initialized IN (0, 1)),
                 uid INTEGER NOT NULL DEFAULT 0,
                 updated_at TEXT NOT NULL
                     DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             );
             INSERT INTO desktop_settings (
                 id, background_enabled, poll_interval_minutes,
                 notifications_enabled, notification_baseline_initialized,
                 notification_baseline_uid
             ) VALUES (1, 1, 5, 1, 0, 0)
             ON CONFLICT(id) DO NOTHING;",
        )?;
        let existing_columns = {
            let mut statement = connection.prepare("PRAGMA table_info(desktop_settings)")?;
            let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
            let mut existing = Vec::new();
            for column in columns {
                existing.push(column?);
            }
            existing
        };
        if !existing_columns
            .iter()
            .any(|column| column == "remote_image_mode")
        {
            connection.execute(
                "ALTER TABLE desktop_settings
                 ADD COLUMN remote_image_mode TEXT NOT NULL DEFAULT 'automatic'
                     CHECK (remote_image_mode IN ('automatic', 'ask', 'blocked'))",
                [],
            )?;
        }
        if !existing_columns
            .iter()
            .any(|column| column == "mcp_enabled")
        {
            connection.execute(
                "ALTER TABLE desktop_settings
                 ADD COLUMN mcp_enabled INTEGER NOT NULL DEFAULT 0
                     CHECK (mcp_enabled IN (0, 1))",
                [],
            )?;
        }
        if !existing_columns
            .iter()
            .any(|column| column == "ai_assistant_default_open")
        {
            connection.execute(
                "ALTER TABLE desktop_settings
                 ADD COLUMN ai_assistant_default_open INTEGER NOT NULL DEFAULT 1
                     CHECK (ai_assistant_default_open IN (0, 1))",
                [],
            )?;
        }
        if !existing_columns
            .iter()
            .any(|column| column == "mcp_information_enabled")
        {
            connection.execute(
                "ALTER TABLE desktop_settings
                 ADD COLUMN mcp_information_enabled INTEGER NOT NULL DEFAULT 1
                     CHECK (mcp_information_enabled IN (0, 1))",
                [],
            )?;
        }
        if !existing_columns
            .iter()
            .any(|column| column == "mcp_send_enabled")
        {
            connection.execute(
                "ALTER TABLE desktop_settings
                 ADD COLUMN mcp_send_enabled INTEGER NOT NULL DEFAULT 0
                     CHECK (mcp_send_enabled IN (0, 1))",
                [],
            )?;
        }
        if !existing_columns
            .iter()
            .any(|column| column == "foreground_notifications_enabled")
        {
            connection.execute(
                "ALTER TABLE desktop_settings
                 ADD COLUMN foreground_notifications_enabled INTEGER NOT NULL DEFAULT 1
                     CHECK (foreground_notifications_enabled IN (0, 1))",
                [],
            )?;
        }
        connection.execute(
            "UPDATE desktop_settings
             SET foreground_notifications_enabled = notifications_enabled
             WHERE foreground_notifications_enabled != notifications_enabled",
            [],
        )?;
        if !existing_columns
            .iter()
            .any(|column| column == "notification_delivery")
        {
            connection.execute(
                "ALTER TABLE desktop_settings
                 ADD COLUMN notification_delivery TEXT NOT NULL DEFAULT 'mine_mail'
                     CHECK (notification_delivery IN ('mine_mail', 'windows'))",
                [],
            )?;
        }
        if !existing_columns
            .iter()
            .any(|column| column == "notification_sound_enabled")
        {
            connection.execute(
                "ALTER TABLE desktop_settings
                 ADD COLUMN notification_sound_enabled INTEGER NOT NULL DEFAULT 1
                     CHECK (notification_sound_enabled IN (0, 1))",
                [],
            )?;
        }
        if !existing_columns
            .iter()
            .any(|column| column == "notification_sound")
        {
            connection.execute(
                "ALTER TABLE desktop_settings
                 ADD COLUMN notification_sound TEXT NOT NULL DEFAULT 'mail'
                     CHECK (notification_sound IN ('default', 'mail', 'im', 'reminder'))",
                [],
            )?;
        }
        Ok(store)
    }

    pub(super) fn load(&self) -> rusqlite::Result<StoredDesktopSettings> {
        self.connection()?.query_row(
            "SELECT background_enabled, poll_interval_minutes,
                    notifications_enabled, notification_delivery,
                    notification_baseline_initialized, notification_baseline_uid,
                    remote_image_mode, notification_sound_enabled,
                    notification_sound, ai_assistant_default_open, mcp_enabled,
                    mcp_information_enabled, mcp_send_enabled
             FROM desktop_settings WHERE id = 1",
            [],
            |row| {
                Ok(StoredDesktopSettings {
                    background_enabled: row.get::<_, i64>(0)? != 0,
                    poll_interval_minutes: row.get(1)?,
                    notifications_enabled: row.get::<_, i64>(2)? != 0,
                    notification_delivery: NotificationDelivery::from_storage_value(
                        &row.get::<_, String>(3)?,
                    ),
                    notification_baseline_initialized: row.get::<_, i64>(4)? != 0,
                    notification_baseline_uid: row.get(5)?,
                    remote_image_mode: RemoteImageMode::from_storage_value(
                        &row.get::<_, String>(6)?,
                    ),
                    notification_sound_enabled: row.get::<_, i64>(7)? != 0,
                    notification_sound: NotificationSound::from_storage_value(
                        &row.get::<_, String>(8)?,
                    ),
                    ai_assistant_default_open: row.get::<_, i64>(9)? != 0,
                    mcp_enabled: row.get::<_, i64>(10)? != 0,
                    mcp_information_enabled: row.get::<_, i64>(11)? != 0,
                    mcp_send_enabled: row.get::<_, i64>(12)? != 0,
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
                 notification_delivery = ?4,
                 notification_baseline_initialized = ?5,
                 notification_baseline_uid = ?6,
                 remote_image_mode = ?7,
                 foreground_notifications_enabled = ?3,
                 notification_sound_enabled = ?8,
                 notification_sound = ?9,
                 ai_assistant_default_open = ?10,
                 mcp_enabled = ?11,
                 mcp_information_enabled = ?12,
                 mcp_send_enabled = ?13,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = 1",
            params![
                settings.background_enabled,
                settings.poll_interval_minutes,
                settings.notifications_enabled,
                settings.notification_delivery.as_storage_value(),
                settings.notification_baseline_initialized,
                settings.notification_baseline_uid,
                settings.remote_image_mode.as_storage_value(),
                settings.notification_sound_enabled,
                settings.notification_sound.as_storage_value(),
                settings.ai_assistant_default_open,
                settings.mcp_enabled,
                settings.mcp_information_enabled,
                settings.mcp_send_enabled,
            ],
        )?;
        Ok(())
    }

    pub(super) fn load_notification_baseline(
        &self,
        account_id: &str,
    ) -> rusqlite::Result<NotificationBaseline> {
        self.connection()?
            .query_row(
                "SELECT initialized, uid
                 FROM account_notification_baselines
                 WHERE account_id = ?1",
                [account_id],
                |row| {
                    Ok(NotificationBaseline {
                        initialized: row.get::<_, i64>(0)? != 0,
                        uid: row.get(1)?,
                    })
                },
            )
            .optional()
            .map(|baseline| baseline.unwrap_or_default())
    }

    pub(super) fn save_notification_baseline(
        &self,
        account_id: &str,
        baseline: NotificationBaseline,
    ) -> rusqlite::Result<()> {
        self.connection()?.execute(
            "INSERT INTO account_notification_baselines (
                 account_id, initialized, uid, updated_at
             ) VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(account_id) DO UPDATE SET
                 initialized = excluded.initialized,
                 uid = excluded.uid,
                 updated_at = excluded.updated_at",
            params![account_id, baseline.initialized, baseline.uid],
        )?;
        Ok(())
    }

    pub(super) fn delete_notification_baseline(&self, account_id: &str) -> rusqlite::Result<()> {
        self.connection()?.execute(
            "DELETE FROM account_notification_baselines WHERE account_id = ?1",
            [account_id],
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

    pub(super) fn profile_avatar(
        &self,
        owner_type: ProfileAvatarOwnerType,
        owner_key: &str,
    ) -> rusqlite::Result<Option<String>> {
        let owner_key = owner_key.trim().to_ascii_lowercase();
        self.connection()?
            .query_row(
                "SELECT mime_type, image_bytes
                 FROM profile_avatars
                 WHERE owner_type = ?1 AND owner_key = ?2",
                params![owner_type.as_storage_value(), owner_key],
                |row| {
                    Ok(avatar_data_url(
                        &row.get::<_, String>(0)?,
                        &row.get::<_, Vec<u8>>(1)?,
                    ))
                },
            )
            .optional()
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
        DeleteProfileAvatarRequest, DesktopSettingsStore, NotificationBaseline,
        NotificationDelivery, NotificationSound, ProfileAvatarOwnerType, RemoteImageMode,
        SaveProfileAvatarRequest, StoredDesktopSettings,
    };

    #[test]
    fn notification_baselines_are_isolated_per_account() {
        let directory = tempdir().expect("temporary directory");
        let store = DesktopSettingsStore::open(directory.path().join("desktop.sqlite3"))
            .expect("settings store");
        store
            .save_notification_baseline(
                "account-a",
                NotificationBaseline {
                    initialized: true,
                    uid: 42,
                },
            )
            .expect("save first baseline");
        store
            .save_notification_baseline(
                "account-b",
                NotificationBaseline {
                    initialized: true,
                    uid: 7,
                },
            )
            .expect("save second baseline");

        assert_eq!(
            store.load_notification_baseline("account-a").unwrap().uid,
            42
        );
        assert_eq!(
            store.load_notification_baseline("account-b").unwrap().uid,
            7
        );
        store
            .delete_notification_baseline("account-a")
            .expect("delete first baseline");
        assert_eq!(
            store.load_notification_baseline("account-a").unwrap(),
            NotificationBaseline::default()
        );
        assert_eq!(
            store.load_notification_baseline("account-b").unwrap().uid,
            7
        );
    }

    #[test]
    fn settings_are_persisted_with_safe_defaults() {
        let directory = tempdir().expect("temporary directory");
        let store = DesktopSettingsStore::open(directory.path().join("desktop.sqlite3"))
            .expect("settings store");

        let defaults = store.load().expect("default settings");
        assert!(defaults.background_enabled);
        assert!(defaults.notifications_enabled);
        assert_eq!(
            defaults.notification_delivery,
            NotificationDelivery::MineMail
        );
        assert!(defaults.notification_sound_enabled);
        assert_eq!(defaults.notification_sound, NotificationSound::Mail);
        #[cfg(target_os = "windows")]
        assert_eq!(
            defaults.notification_sound.system_resource_name(),
            "Notification.Mail"
        );
        assert_eq!(defaults.poll_interval_minutes, 5);
        assert_eq!(defaults.remote_image_mode, RemoteImageMode::Automatic);
        assert!(defaults.ai_assistant_default_open);
        assert!(!defaults.mcp_enabled);
        assert!(defaults.mcp_information_enabled);
        assert!(!defaults.mcp_send_enabled);
        assert!(!defaults.notification_baseline_initialized);

        let updated = StoredDesktopSettings {
            background_enabled: false,
            poll_interval_minutes: 3,
            notifications_enabled: false,
            notification_delivery: NotificationDelivery::Windows,
            notification_sound_enabled: false,
            notification_sound: NotificationSound::Reminder,
            remote_image_mode: RemoteImageMode::Blocked,
            ai_assistant_default_open: false,
            mcp_enabled: true,
            mcp_information_enabled: false,
            mcp_send_enabled: true,
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
        let migrated = store.load().expect("migrated notification settings");
        assert_eq!(
            migrated.notification_delivery,
            NotificationDelivery::MineMail
        );
        assert!(migrated.notification_sound_enabled);
        assert_eq!(migrated.notification_sound, NotificationSound::Mail);
        assert!(migrated.ai_assistant_default_open);
        assert!(!migrated.mcp_enabled);
        assert!(migrated.mcp_information_enabled);
        assert!(!migrated.mcp_send_enabled);
    }

    #[test]
    fn legacy_foreground_notification_value_is_folded_into_desktop_switch() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("desktop.sqlite3");
        DesktopSettingsStore::open(&path).expect("initial settings store");
        Connection::open(&path)
            .expect("settings connection")
            .execute(
                "UPDATE desktop_settings
                 SET notifications_enabled = 1,
                     foreground_notifications_enabled = 0
                 WHERE id = 1",
                [],
            )
            .expect("write legacy split notification values");

        let store = DesktopSettingsStore::open(&path).expect("reopened settings store");
        assert!(
            store
                .load()
                .expect("normalized settings")
                .notifications_enabled
        );
        let stored_foreground_value: i64 = Connection::open(&path)
            .expect("settings connection")
            .query_row(
                "SELECT foreground_notifications_enabled
                 FROM desktop_settings
                 WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .expect("legacy notification value");
        assert_eq!(stored_foreground_value, 1);
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
