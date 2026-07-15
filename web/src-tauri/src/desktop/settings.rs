use std::{path::Path, path::PathBuf, time::Duration};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

pub(super) const DEFAULT_POLL_INTERVAL_MINUTES: u8 = 5;

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

    fn connection(&self) -> rusqlite::Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        Ok(connection)
    }
}

pub(super) fn valid_poll_interval(value: u8) -> bool {
    matches!(value, 1 | 3 | 5)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use rusqlite::Connection;

    use super::{DesktopSettingsStore, RemoteImageMode, StoredDesktopSettings};

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
}
