use std::{
    collections::HashSet,
    f32::consts::TAU,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use image::{
    DynamicImage, GenericImageView, ImageDecoder, ImageFormat, ImageReader,
    codecs::{jpeg::JpegEncoder, png::PngDecoder, webp::WebPDecoder},
    imageops::FilterType,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_SOURCE_BYTES: usize = 20 * 1024 * 1024;
const MAX_SOURCE_DATA_URL_CHARS: usize = 28 * 1024 * 1024;
const MAX_PIXEL_COUNT: u64 = 50_000_000;
const MAX_DECODED_BYTES: u64 = 220 * 1024 * 1024;
const MAX_LONG_EDGE: u32 = 5_120;
const THUMBNAIL_WIDTH: u32 = 480;
const THUMBNAIL_HEIGHT: u32 = 300;
const MAX_PRESET_NAME_CHARS: usize = 40;
const FALLBACK_PALETTE_ID: &str = "daylight";
const BUILTIN_THEME_IDS: [&str; 4] = ["daylight", "night", "dusk", "forest"];
const BUILTIN_PALETTE_IDS: [&str; 4] = ["daylight", "night", "dusk", "forest"];
const PALETTE_FAMILY_IDS: [&str; 12] = [
    "green", "teal", "cyan", "sky", "blue", "indigo", "violet", "purple", "magenta", "rose",
    "orange", "yellow",
];
const PALETTE_HUES: [(&str, f32); 12] = [
    ("green", 142.0),
    ("teal", 170.0),
    ("cyan", 196.0),
    ("sky", 224.0),
    ("blue", 252.0),
    ("indigo", 276.0),
    ("violet", 296.0),
    ("purple", 316.0),
    ("magenta", 336.0),
    ("rose", 6.0),
    ("orange", 52.0),
    ("yellow", 94.0),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AppearanceThemeKind {
    #[default]
    Builtin,
    Custom,
}

impl AppearanceThemeKind {
    fn as_storage_value(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Custom => "custom",
        }
    }

    fn from_storage_value(value: &str) -> Self {
        match value {
            "custom" => Self::Custom,
            _ => Self::Builtin,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AppearanceMode {
    #[default]
    Auto,
    Light,
    Dark,
}

impl AppearanceMode {
    fn as_storage_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    fn from_storage_value(value: &str) -> Self {
        match value {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::Auto,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppearanceSelectionDto {
    pub kind: AppearanceThemeKind,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomThemePresetDto {
    pub id: String,
    pub name: String,
    pub palette_id: String,
    pub focal_x: f32,
    pub focal_y: f32,
    pub thumbnail_data_url: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppearanceSettingsDto {
    pub selection_initialized: bool,
    pub palette_id: String,
    pub minimal_mode_enabled: bool,
    pub active_theme: AppearanceSelectionDto,
    pub custom_presets: Vec<CustomThemePresetDto>,
    pub active_background_data_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectAppearanceThemeRequest {
    pub kind: AppearanceThemeKind,
    pub id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportCustomThemeRequest {
    pub name: Option<String>,
    pub image_data_url: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateCustomThemeRequest {
    pub id: String,
    pub name: Option<String>,
    pub focal_x: Option<f32>,
    pub focal_y: Option<f32>,
    pub image_data_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteCustomThemeRequest {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateAppearancePreferencesRequest {
    pub palette_id: Option<String>,
    pub minimal_mode_enabled: Option<bool>,
}

#[derive(Clone, Debug)]
pub(super) struct AppearanceStore {
    database_path: PathBuf,
    asset_directory: PathBuf,
}

#[derive(Clone, Debug)]
struct StoredSelection {
    initialized: bool,
    active_kind: AppearanceThemeKind,
    active_id: String,
    previous_kind: Option<AppearanceThemeKind>,
    previous_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StoredAppearancePreferences {
    palette_id: String,
    minimal_mode_enabled: bool,
}

struct ProcessedBackground {
    image_bytes: Vec<u8>,
    thumbnail_bytes: Vec<u8>,
    palette_id: String,
}

impl AppearanceStore {
    pub(super) fn open(
        database_path: impl AsRef<Path>,
        existing_install: bool,
    ) -> Result<Self, String> {
        let database_path = database_path.as_ref().to_path_buf();
        let root = database_path
            .parent()
            .ok_or_else(|| "Appearance storage is unavailable.".to_owned())?;
        let asset_directory = root.join("user-assets").join("appearance");
        fs::create_dir_all(&asset_directory)
            .map_err(|_| "Appearance assets could not be initialized.".to_owned())?;
        let store = Self {
            database_path,
            asset_directory,
        };
        store
            .connection()
            .and_then(|connection| {
                connection.execute_batch(
                    "CREATE TABLE IF NOT EXISTS appearance_state (
                         id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
                         active_kind TEXT NOT NULL
                             CHECK (active_kind IN ('builtin', 'custom')),
                         active_theme_id TEXT NOT NULL,
                         previous_kind TEXT
                             CHECK (previous_kind IS NULL OR previous_kind IN ('builtin', 'custom')),
                         previous_theme_id TEXT,
                         updated_at TEXT NOT NULL
                             DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                     );
                     CREATE TABLE IF NOT EXISTS custom_theme_presets (
                         id TEXT PRIMARY KEY NOT NULL,
                         name TEXT NOT NULL,
                         asset_file_name TEXT NOT NULL UNIQUE,
                         thumbnail_bytes BLOB NOT NULL,
                         palette_id TEXT NOT NULL,
                         mode TEXT NOT NULL CHECK (mode IN ('auto', 'light', 'dark')),
                         auto_mode TEXT NOT NULL CHECK (auto_mode IN ('light', 'dark')),
                         focal_x REAL NOT NULL DEFAULT 0.5 CHECK (focal_x >= 0 AND focal_x <= 1),
                         focal_y REAL NOT NULL DEFAULT 0.5 CHECK (focal_y >= 0 AND focal_y <= 1),
                         created_at TEXT NOT NULL
                             DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                         updated_at TEXT NOT NULL
                             DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                     );
                     CREATE TABLE IF NOT EXISTS appearance_preferences (
                         id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
                         palette_id TEXT NOT NULL,
                         minimal_mode_enabled INTEGER NOT NULL
                             CHECK (minimal_mode_enabled IN (0, 1)),
                         updated_at TEXT NOT NULL
                             DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                     );",
                )?;
                migrate_legacy_palettes(&connection)
            })
            .map_err(|_| "Appearance storage could not be initialized.".to_owned())?;
        store.initialize_preferences(existing_install)?;
        Ok(store)
    }

    pub(super) fn load(&self) -> Result<AppearanceSettingsDto, String> {
        let mut selection = self.load_selection()?;
        if !self.selection_exists(selection.active_kind, &selection.active_id)? {
            selection.active_kind = AppearanceThemeKind::Builtin;
            selection.active_id = "daylight".to_owned();
            if selection.initialized {
                self.save_selection(&selection)?;
            }
        }

        let connection = self
            .connection()
            .map_err(|_| "Appearance presets could not be loaded.".to_owned())?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, palette_id, focal_x, focal_y,
                        thumbnail_bytes
                 FROM custom_theme_presets
                 ORDER BY created_at ASC, rowid ASC",
            )
            .map_err(|_| "Appearance presets could not be loaded.".to_owned())?;
        let rows = statement
            .query_map([], |row| {
                Ok(CustomThemePresetDto {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    palette_id: row.get(2)?,
                    focal_x: row.get(3)?,
                    focal_y: row.get(4)?,
                    thumbnail_data_url: image_data_url("image/jpeg", &row.get::<_, Vec<u8>>(5)?),
                })
            })
            .map_err(|_| "Appearance presets could not be loaded.".to_owned())?;
        let custom_presets = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_| "Appearance presets could not be loaded.".to_owned())?;
        let active_background_data_url = if selection.active_kind == AppearanceThemeKind::Custom {
            Some(self.load_background_data_url(&selection.active_id)?)
        } else {
            None
        };
        let preferences = self.load_preferences()?;

        Ok(AppearanceSettingsDto {
            selection_initialized: selection.initialized,
            palette_id: preferences.palette_id,
            minimal_mode_enabled: preferences.minimal_mode_enabled,
            active_theme: AppearanceSelectionDto {
                kind: selection.active_kind,
                id: selection.active_id,
            },
            custom_presets,
            active_background_data_url,
        })
    }

    pub(super) fn update_preferences(
        &self,
        request: UpdateAppearancePreferencesRequest,
    ) -> Result<AppearanceSettingsDto, String> {
        let preferences = self.load_preferences()?;
        let selection = self.load_selection()?;
        let explicit_palette = request.palette_id;
        let minimal_mode_enabled = request
            .minimal_mode_enabled
            .unwrap_or(preferences.minimal_mode_enabled);
        let palette_id = if explicit_palette.is_none()
            && request.minimal_mode_enabled == Some(false)
            && preferences.minimal_mode_enabled
        {
            Some(self.theme_palette_id(selection.active_kind, &selection.active_id)?)
        } else {
            explicit_palette.clone()
        };
        if let Some(palette_id) = palette_id.as_deref() {
            validate_palette_id(palette_id)?;
        }
        let custom_palette = if explicit_palette.is_some()
            && !minimal_mode_enabled
            && selection.active_kind == AppearanceThemeKind::Custom
        {
            palette_id
                .as_ref()
                .map(|palette_id| (selection.active_id.clone(), palette_id.clone()))
        } else {
            None
        };
        let mut connection = self
            .connection()
            .map_err(|_| "Appearance preferences could not be saved.".to_owned())?;
        let transaction = connection
            .transaction()
            .map_err(|_| "Appearance preferences could not be saved.".to_owned())?;
        let changed = transaction
            .execute(
                "UPDATE appearance_preferences SET
                         palette_id = COALESCE(?1, palette_id),
                         minimal_mode_enabled = COALESCE(?2, minimal_mode_enabled),
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = 1",
                params![palette_id, request.minimal_mode_enabled],
            )
            .map_err(|_| "Appearance preferences could not be saved.".to_owned())?;
        if changed == 0 {
            return Err("Appearance preferences could not be saved.".to_owned());
        }
        if let Some((preset_id, palette_id)) = custom_palette {
            let mode = palette_mode(&palette_id)
                .ok_or_else(|| "The selected color palette is invalid.".to_owned())?;
            let changed = transaction
                .execute(
                    "UPDATE custom_theme_presets SET
                         palette_id = ?2,
                         mode = 'auto',
                         auto_mode = ?3,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?1",
                    params![preset_id, palette_id, mode.as_storage_value()],
                )
                .map_err(|_| "Appearance preferences could not be saved.".to_owned())?;
            if changed == 0 {
                return Err("Appearance preferences could not be saved.".to_owned());
            }
        }
        transaction
            .commit()
            .map_err(|_| "Appearance preferences could not be saved.".to_owned())?;
        self.load()
    }

    pub(super) fn select(
        &self,
        request: SelectAppearanceThemeRequest,
    ) -> Result<AppearanceSettingsDto, String> {
        validate_theme_id(request.kind, &request.id)?;
        if !self.selection_exists(request.kind, &request.id)? {
            return Err("The selected appearance preset is unavailable.".to_owned());
        }
        let current = self.load_selection()?;
        let theme_palette_id = if !self.load_preferences()?.minimal_mode_enabled {
            Some(self.theme_palette_id(request.kind, &request.id)?)
        } else {
            None
        };
        if current.active_kind == request.kind && current.active_id == request.id {
            if !current.initialized {
                self.save_selection(&StoredSelection {
                    initialized: true,
                    ..current
                })?;
            }
            if let Some(palette_id) = theme_palette_id {
                self.save_palette_id(&palette_id)?;
            }
            return self.load();
        }
        let selection = StoredSelection {
            initialized: true,
            active_kind: request.kind,
            active_id: request.id,
            previous_kind: Some(current.active_kind),
            previous_id: Some(current.active_id),
        };
        self.save_selection(&selection)?;
        if let Some(palette_id) = theme_palette_id {
            self.save_palette_id(&palette_id)?;
        }
        self.load()
    }

    fn save_palette_id(&self, palette_id: &str) -> Result<(), String> {
        self.connection()
            .and_then(|connection| {
                connection.execute(
                    "UPDATE appearance_preferences SET
                         palette_id = ?1,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = 1",
                    [palette_id],
                )?;
                Ok(())
            })
            .map_err(|_| "Appearance preferences could not be saved.".to_owned())
    }

    fn theme_palette_id(
        &self,
        kind: AppearanceThemeKind,
        theme_id: &str,
    ) -> Result<String, String> {
        if kind == AppearanceThemeKind::Builtin {
            return Ok(builtin_palette_id(theme_id).to_owned());
        }
        self.connection()
            .map_err(|_| "Appearance preferences could not be loaded.".to_owned())?
            .query_row(
                "SELECT palette_id FROM custom_theme_presets WHERE id = ?1",
                [theme_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| "Appearance preferences could not be loaded.".to_owned())?
            .filter(|palette_id| validate_palette_id(palette_id).is_ok())
            .ok_or_else(|| "The selected appearance preset is unavailable.".to_owned())
    }

    pub(super) fn import_custom(
        &self,
        request: ImportCustomThemeRequest,
    ) -> Result<AppearanceSettingsDto, String> {
        let image_bytes = decode_image_data_url(&request.image_data_url)?;
        let processed = process_background(&image_bytes)?;
        let id = Uuid::new_v4().to_string();
        let asset_file_name = format!("{}.jpg", Uuid::new_v4());
        let name = match request.name {
            Some(name) => normalize_preset_name(&name)?,
            None => self.next_default_name()?,
        };
        self.write_asset(&asset_file_name, &processed.image_bytes)?;
        let palette_id = processed.palette_id.clone();
        let palette_mode = palette_mode(&palette_id)
            .ok_or_else(|| "The analyzed color palette is invalid.".to_owned())?;

        let connection = self
            .connection()
            .map_err(|_| "The custom theme could not be saved.".to_owned())?;
        if connection
            .execute(
                "INSERT INTO custom_theme_presets (
                     id, name, asset_file_name, thumbnail_bytes, palette_id,
                     mode, auto_mode, focal_x, focal_y
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'auto', ?6, 0.5, 0.5)",
                params![
                    id,
                    name,
                    asset_file_name,
                    processed.thumbnail_bytes,
                    palette_id,
                    palette_mode.as_storage_value(),
                ],
            )
            .is_err()
        {
            let _ = fs::remove_file(self.asset_directory.join(&asset_file_name));
            return Err("The custom theme could not be saved.".to_owned());
        }
        let current = self.load_selection()?;
        if let Err(error) = self.save_selection(&StoredSelection {
            initialized: true,
            active_kind: AppearanceThemeKind::Custom,
            active_id: id.clone(),
            previous_kind: Some(current.active_kind),
            previous_id: Some(current.active_id),
        }) {
            let _ = connection.execute("DELETE FROM custom_theme_presets WHERE id = ?1", [&id]);
            let _ = fs::remove_file(self.asset_directory.join(&asset_file_name));
            return Err(error);
        }
        if !self.load_preferences()?.minimal_mode_enabled {
            self.save_palette_id(&palette_id)?;
        }
        self.load()
    }

    pub(super) fn update_custom(
        &self,
        request: UpdateCustomThemeRequest,
    ) -> Result<AppearanceSettingsDto, String> {
        validate_custom_id(&request.id)?;
        let old_asset = self.asset_file_name(&request.id)?;
        let mut new_asset: Option<String> = None;
        let mut processed: Option<ProcessedBackground> = None;
        if let Some(image_data_url) = request.image_data_url.as_deref() {
            let image_bytes = decode_image_data_url(image_data_url)?;
            let next = process_background(&image_bytes)?;
            let file_name = format!("{}.jpg", Uuid::new_v4());
            self.write_asset(&file_name, &next.image_bytes)?;
            new_asset = Some(file_name);
            processed = Some(next);
        }
        let name = request
            .name
            .as_deref()
            .map(normalize_preset_name)
            .transpose()?;
        validate_focal_point(request.focal_x)?;
        validate_focal_point(request.focal_y)?;

        let result = self.connection().and_then(|connection| {
            let changed = connection.execute(
                "UPDATE custom_theme_presets SET
                     name = COALESCE(?2, name),
                     focal_x = COALESCE(?3, focal_x),
                     focal_y = COALESCE(?4, focal_y),
                     asset_file_name = COALESCE(?5, asset_file_name),
                     thumbnail_bytes = COALESCE(?6, thumbnail_bytes),
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                params![
                    request.id,
                    name,
                    request.focal_x,
                    request.focal_y,
                    new_asset,
                    processed.as_ref().map(|value| &value.thumbnail_bytes),
                ],
            )?;
            if changed == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            Ok(())
        });
        if result.is_err() {
            if let Some(file_name) = new_asset {
                let _ = fs::remove_file(self.asset_directory.join(file_name));
            }
            return Err("The custom theme could not be updated.".to_owned());
        }
        if processed.is_some() {
            let _ = fs::remove_file(self.asset_directory.join(old_asset));
        }
        self.load()
    }

    pub(super) fn delete_custom(
        &self,
        request: DeleteCustomThemeRequest,
    ) -> Result<AppearanceSettingsDto, String> {
        validate_custom_id(&request.id)?;
        let asset_file_name = self.asset_file_name(&request.id)?;
        let selection = self.load_selection()?;
        let connection = self
            .connection()
            .map_err(|_| "The custom theme could not be removed.".to_owned())?;
        let changed = connection
            .execute(
                "DELETE FROM custom_theme_presets WHERE id = ?1",
                [&request.id],
            )
            .map_err(|_| "The custom theme could not be removed.".to_owned())?;
        if changed == 0 {
            return Err("The custom theme is unavailable.".to_owned());
        }
        if selection.active_kind == AppearanceThemeKind::Custom && selection.active_id == request.id
        {
            let (active_kind, active_id) =
                match (selection.previous_kind, selection.previous_id.as_deref()) {
                    (Some(kind), Some(id)) if self.selection_exists(kind, id)? => {
                        (kind, id.to_owned())
                    }
                    _ => (AppearanceThemeKind::Builtin, "daylight".to_owned()),
                };
            let palette_id = if !self.load_preferences()?.minimal_mode_enabled {
                Some(self.theme_palette_id(active_kind, &active_id)?)
            } else {
                None
            };
            self.save_selection(&StoredSelection {
                initialized: true,
                active_kind,
                active_id,
                previous_kind: Some(AppearanceThemeKind::Builtin),
                previous_id: Some("daylight".to_owned()),
            })?;
            if let Some(palette_id) = palette_id {
                self.save_palette_id(&palette_id)?;
            }
        }
        let _ = fs::remove_file(self.asset_directory.join(asset_file_name));
        self.load()
    }

    fn initialize_preferences(&self, existing_install: bool) -> Result<(), String> {
        let connection = self
            .connection()
            .map_err(|_| "Appearance preferences could not be initialized.".to_owned())?;
        let exists = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM appearance_preferences WHERE id = 1)",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|_| "Appearance preferences could not be initialized.".to_owned())?;
        if exists {
            return Ok(());
        }
        let palette_id = if existing_install {
            self.legacy_effective_palette()?
        } else {
            FALLBACK_PALETTE_ID.to_owned()
        };
        connection
            .execute(
                "INSERT INTO appearance_preferences (
                     id, palette_id, minimal_mode_enabled, updated_at
                 ) VALUES (1, ?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                params![palette_id, !existing_install],
            )
            .map_err(|_| "Appearance preferences could not be initialized.".to_owned())?;
        Ok(())
    }

    fn legacy_effective_palette(&self) -> Result<String, String> {
        let selection = self.load_selection()?;
        if !self.selection_exists(selection.active_kind, &selection.active_id)? {
            return Ok(FALLBACK_PALETTE_ID.to_owned());
        }
        if selection.active_kind == AppearanceThemeKind::Builtin {
            return Ok(builtin_palette_id(&selection.active_id).to_owned());
        }
        self.connection()
            .map_err(|_| "Appearance preferences could not be initialized.".to_owned())?
            .query_row(
                "SELECT palette_id FROM custom_theme_presets WHERE id = ?1",
                [&selection.active_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map(|value| {
                value
                    .filter(|palette_id| validate_palette_id(palette_id).is_ok())
                    .unwrap_or_else(|| FALLBACK_PALETTE_ID.to_owned())
            })
            .map_err(|_| "Appearance preferences could not be initialized.".to_owned())
    }

    fn load_preferences(&self) -> Result<StoredAppearancePreferences, String> {
        self.connection()
            .map_err(|_| "Appearance preferences could not be loaded.".to_owned())?
            .query_row(
                "SELECT palette_id, minimal_mode_enabled
                 FROM appearance_preferences WHERE id = 1",
                [],
                |row| {
                    Ok(StoredAppearancePreferences {
                        palette_id: row.get(0)?,
                        minimal_mode_enabled: row.get::<_, i64>(1)? != 0,
                    })
                },
            )
            .map_err(|_| "Appearance preferences could not be loaded.".to_owned())
    }

    fn connection(&self) -> rusqlite::Result<Connection> {
        let connection = Connection::open(&self.database_path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        Ok(connection)
    }

    fn load_selection(&self) -> Result<StoredSelection, String> {
        self.connection()
            .map_err(|_| "Appearance settings could not be loaded.".to_owned())?
            .query_row(
                "SELECT active_kind, active_theme_id, previous_kind, previous_theme_id
                 FROM appearance_state WHERE id = 1",
                [],
                |row| {
                    Ok(StoredSelection {
                        initialized: true,
                        active_kind: AppearanceThemeKind::from_storage_value(
                            &row.get::<_, String>(0)?,
                        ),
                        active_id: row.get(1)?,
                        previous_kind: row
                            .get::<_, Option<String>>(2)?
                            .as_deref()
                            .map(AppearanceThemeKind::from_storage_value),
                        previous_id: row.get(3)?,
                    })
                },
            )
            .optional()
            .map(|selection| {
                selection.unwrap_or(StoredSelection {
                    initialized: false,
                    active_kind: AppearanceThemeKind::Builtin,
                    active_id: "daylight".to_owned(),
                    previous_kind: None,
                    previous_id: None,
                })
            })
            .map_err(|_| "Appearance settings could not be loaded.".to_owned())
    }

    fn save_selection(&self, selection: &StoredSelection) -> Result<(), String> {
        self.connection()
            .and_then(|connection| {
                connection.execute(
                    "INSERT INTO appearance_state (
                         id, active_kind, active_theme_id, previous_kind,
                         previous_theme_id, updated_at
                     ) VALUES (1, ?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                     ON CONFLICT(id) DO UPDATE SET
                         active_kind = excluded.active_kind,
                         active_theme_id = excluded.active_theme_id,
                         previous_kind = excluded.previous_kind,
                         previous_theme_id = excluded.previous_theme_id,
                         updated_at = excluded.updated_at",
                    params![
                        selection.active_kind.as_storage_value(),
                        selection.active_id,
                        selection
                            .previous_kind
                            .map(AppearanceThemeKind::as_storage_value),
                        selection.previous_id,
                    ],
                )?;
                Ok(())
            })
            .map_err(|_| "Appearance settings could not be saved.".to_owned())
    }

    fn selection_exists(&self, kind: AppearanceThemeKind, id: &str) -> Result<bool, String> {
        validate_theme_id(kind, id)?;
        if kind == AppearanceThemeKind::Builtin {
            return Ok(true);
        }
        let file_name = self
            .connection()
            .and_then(|connection| {
                connection
                    .query_row(
                        "SELECT asset_file_name FROM custom_theme_presets WHERE id = ?1",
                        [id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
            })
            .map_err(|_| "Appearance settings could not be loaded.".to_owned())?;
        let Some(file_name) = file_name else {
            return Ok(false);
        };
        let file_name = validate_asset_file_name(file_name)?;
        Ok(self.asset_directory.join(file_name).is_file())
    }

    fn asset_file_name(&self, id: &str) -> Result<String, String> {
        self.connection()
            .map_err(|_| "The custom theme is unavailable.".to_owned())?
            .query_row(
                "SELECT asset_file_name FROM custom_theme_presets WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| "The custom theme is unavailable.".to_owned())?
            .ok_or_else(|| "The custom theme is unavailable.".to_owned())
            .and_then(validate_asset_file_name)
    }

    fn load_background_data_url(&self, id: &str) -> Result<String, String> {
        let file_name = self.asset_file_name(id)?;
        let bytes = fs::read(self.asset_directory.join(file_name))
            .map_err(|_| "The custom background could not be loaded.".to_owned())?;
        Ok(image_data_url("image/jpeg", &bytes))
    }

    fn next_default_name(&self) -> Result<String, String> {
        let connection = self
            .connection()
            .map_err(|_| "The custom theme could not be saved.".to_owned())?;
        let mut statement = connection
            .prepare("SELECT name FROM custom_theme_presets")
            .map_err(|_| "The custom theme could not be saved.".to_owned())?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))
            .and_then(|rows| rows.collect::<rusqlite::Result<HashSet<_>>>())
            .map_err(|_| "The custom theme could not be saved.".to_owned())?;
        let mut index = 1_u32;
        loop {
            let name = format!("自定义主题 {index}");
            if !names.contains(&name) {
                return Ok(name);
            }
            index = index
                .checked_add(1)
                .ok_or_else(|| "The custom theme could not be named.".to_owned())?;
        }
    }

    fn write_asset(&self, file_name: &str, bytes: &[u8]) -> Result<(), String> {
        let file_name = validate_asset_file_name(file_name.to_owned())?;
        let temporary_name = format!("{}.tmp", Uuid::new_v4());
        let temporary_path = self.asset_directory.join(&temporary_name);
        let final_path = self.asset_directory.join(file_name);
        fs::write(&temporary_path, bytes)
            .map_err(|_| "The custom background could not be saved.".to_owned())?;
        if fs::rename(&temporary_path, &final_path).is_err() {
            let _ = fs::remove_file(&temporary_path);
            return Err("The custom background could not be saved.".to_owned());
        }
        Ok(())
    }
}

fn decode_image_data_url(value: &str) -> Result<Vec<u8>, String> {
    if value.len() > MAX_SOURCE_DATA_URL_CHARS {
        return Err("Background images must be no larger than 20 MB.".to_owned());
    }
    let encoded = [
        "data:image/png;base64,",
        "data:image/jpeg;base64,",
        "data:image/jpg;base64,",
        "data:image/webp;base64,",
    ]
    .iter()
    .find_map(|prefix| value.strip_prefix(prefix))
    .ok_or_else(|| "Only PNG, JPEG, and WebP background images are supported.".to_owned())?;
    let bytes = BASE64_STANDARD
        .decode(encoded)
        .map_err(|_| "The background image could not be read.".to_owned())?;
    if bytes.is_empty() || bytes.len() > MAX_SOURCE_BYTES {
        return Err("Background images must be no larger than 20 MB.".to_owned());
    }
    Ok(bytes)
}

fn process_background(bytes: &[u8]) -> Result<ProcessedBackground, String> {
    if bytes.is_empty() || bytes.len() > MAX_SOURCE_BYTES {
        return Err("Background images must be no larger than 20 MB.".to_owned());
    }
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "The background image could not be read.".to_owned())?;
    let Some(format @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)) = reader.format()
    else {
        return Err("Only PNG, JPEG, and WebP background images are supported.".to_owned());
    };
    reject_animated_background(bytes, format)?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "The background image could not be decoded.".to_owned())?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || u64::from(width).saturating_mul(u64::from(height)) > MAX_PIXEL_COUNT
        || decoder.total_bytes() > MAX_DECODED_BYTES
    {
        return Err("The background image is too large to process safely.".to_owned());
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|_| "The background image could not be decoded.".to_owned())?;
    image.apply_orientation(orientation);
    let palette_id = analyze_palette(&image);
    let image = resize_long_edge(image, MAX_LONG_EDGE);
    let thumbnail = image.thumbnail(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT);
    Ok(ProcessedBackground {
        image_bytes: encode_jpeg(&image, 88)?,
        thumbnail_bytes: encode_jpeg(&thumbnail, 80)?,
        palette_id,
    })
}

fn reject_animated_background(bytes: &[u8], format: ImageFormat) -> Result<(), String> {
    let animated = match format {
        ImageFormat::Png => PngDecoder::new(Cursor::new(bytes))
            .and_then(|decoder| decoder.is_apng())
            .map_err(|_| "The background image could not be decoded.".to_owned())?,
        ImageFormat::WebP => WebPDecoder::new(Cursor::new(bytes))
            .map(|decoder| decoder.has_animation())
            .map_err(|_| "The background image could not be decoded.".to_owned())?,
        ImageFormat::Jpeg => false,
        _ => return Err("Only PNG, JPEG, and WebP background images are supported.".to_owned()),
    };
    if animated {
        Err("Animated background images are not supported.".to_owned())
    } else {
        Ok(())
    }
}

fn resize_long_edge(image: DynamicImage, limit: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    let long_edge = width.max(height);
    if long_edge <= limit {
        return image;
    }
    let scale = limit as f64 / long_edge as f64;
    let next_width = (width as f64 * scale).round().max(1.0) as u32;
    let next_height = (height as f64 * scale).round().max(1.0) as u32;
    image.resize_exact(next_width, next_height, FilterType::Lanczos3)
}

fn encode_jpeg(image: &DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    image
        .write_with_encoder(JpegEncoder::new_with_quality(&mut bytes, quality))
        .map_err(|_| "The background image could not be normalized.".to_owned())?;
    Ok(bytes)
}

fn analyze_palette(image: &DynamicImage) -> String {
    let sample = image.thumbnail(96, 96).to_rgb8();
    let (width, height) = sample.dimensions();
    let mut hue_x = 0.0_f32;
    let mut hue_y = 0.0_f32;
    let mut chroma_weight = 0.0_f32;
    let mut lightness = 0.0_f32;
    let mut lightness_weight = 0.0_f32;
    for (x, _y, pixel) in sample.enumerate_pixels() {
        let edge_weight = if x.saturating_mul(100) < width.saturating_mul(38) {
            1.8
        } else {
            1.0
        };
        let [red, green, blue] = pixel.0;
        let (l, a, b) = srgb_to_oklab(red, green, blue);
        lightness += l * edge_weight;
        lightness_weight += edge_weight;
        let chroma = (a * a + b * b).sqrt();
        if chroma < 0.035 {
            continue;
        }
        let angle = b.atan2(a);
        let weight = chroma * edge_weight;
        hue_x += angle.cos() * weight;
        hue_y += angle.sin() * weight;
        chroma_weight += weight;
    }
    let average_lightness = if lightness_weight > 0.0 {
        lightness / lightness_weight
    } else {
        0.7
    };
    let scheme = if average_lightness < 0.6 {
        "dark"
    } else {
        "light"
    };
    if chroma_weight < (width.saturating_mul(height) as f32 * 0.0025) {
        return format!("sky-{scheme}");
    }
    let hue = hue_y.atan2(hue_x).rem_euclid(TAU).to_degrees();
    let palette = PALETTE_HUES
        .iter()
        .min_by(|(_, left), (_, right)| {
            hue_distance(hue, *left)
                .partial_cmp(&hue_distance(hue, *right))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(id, _)| *id)
        .unwrap_or("sky");
    format!("{palette}-{scheme}")
}

fn srgb_to_oklab(red: u8, green: u8, blue: u8) -> (f32, f32, f32) {
    fn linear(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    let red = linear(red);
    let green = linear(green);
    let blue = linear(blue);
    let l = 0.412_221_46 * red + 0.536_332_55 * green + 0.051_445_995 * blue;
    let m = 0.211_903_5 * red + 0.680_699_5 * green + 0.107_396_96 * blue;
    let s = 0.088_302_46 * red + 0.281_718_85 * green + 0.629_978_7 * blue;
    let l = l.cbrt();
    let m = m.cbrt();
    let s = s.cbrt();
    (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    )
}

fn hue_distance(left: f32, right: f32) -> f32 {
    let distance = (left - right).abs().rem_euclid(360.0);
    distance.min(360.0 - distance)
}

fn palette_mode(id: &str) -> Option<AppearanceMode> {
    if BUILTIN_PALETTE_IDS.contains(&id) {
        return Some(if id == "night" {
            AppearanceMode::Dark
        } else {
            AppearanceMode::Light
        });
    }
    let (family_id, scheme) = id.rsplit_once('-')?;
    if !PALETTE_FAMILY_IDS.contains(&family_id) {
        return None;
    }
    match scheme {
        "light" => Some(AppearanceMode::Light),
        "dark" => Some(AppearanceMode::Dark),
        _ => None,
    }
}

fn normalize_stored_palette_id(
    id: &str,
    mode: AppearanceMode,
    auto_mode: AppearanceMode,
) -> String {
    if palette_mode(id).is_some() {
        return id.to_owned();
    }
    if PALETTE_FAMILY_IDS.contains(&id) {
        let effective_mode = if mode == AppearanceMode::Auto {
            auto_mode
        } else {
            mode
        };
        let scheme = if effective_mode == AppearanceMode::Dark {
            "dark"
        } else {
            "light"
        };
        return format!("{id}-{scheme}");
    }
    FALLBACK_PALETTE_ID.to_owned()
}

fn builtin_palette_id(theme_id: &str) -> &'static str {
    match theme_id {
        "night" => "night",
        "dusk" => "dusk",
        "forest" => "forest",
        _ => FALLBACK_PALETTE_ID,
    }
}

fn migrate_legacy_palettes(connection: &Connection) -> rusqlite::Result<()> {
    let rows = {
        let mut statement = connection
            .prepare("SELECT rowid, palette_id, mode, auto_mode FROM custom_theme_presets")?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    AppearanceMode::from_storage_value(&row.get::<_, String>(2)?),
                    AppearanceMode::from_storage_value(&row.get::<_, String>(3)?),
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };

    for (rowid, current_id, mode, auto_mode) in rows {
        let palette_id = normalize_stored_palette_id(&current_id, mode, auto_mode);
        let palette_mode = palette_mode(&palette_id).unwrap_or(AppearanceMode::Light);
        if palette_id != current_id || mode != AppearanceMode::Auto || auto_mode != palette_mode {
            connection.execute(
                "UPDATE custom_theme_presets
                 SET palette_id = ?1, mode = 'auto', auto_mode = ?2
                 WHERE rowid = ?3",
                params![palette_id, palette_mode.as_storage_value(), rowid],
            )?;
        }
    }
    Ok(())
}

fn validate_theme_id(kind: AppearanceThemeKind, id: &str) -> Result<(), String> {
    match kind {
        AppearanceThemeKind::Builtin if BUILTIN_THEME_IDS.contains(&id) => Ok(()),
        AppearanceThemeKind::Custom => validate_custom_id(id),
        _ => Err("The selected appearance preset is invalid.".to_owned()),
    }
}

fn validate_custom_id(id: &str) -> Result<(), String> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| "The custom theme identifier is invalid.".to_owned())
}

fn validate_palette_id(id: &str) -> Result<(), String> {
    if palette_mode(id).is_some() {
        Ok(())
    } else {
        Err("The selected color palette is invalid.".to_owned())
    }
}

fn validate_focal_point(value: Option<f32>) -> Result<(), String> {
    if value.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
        Err("The background focal point is invalid.".to_owned())
    } else {
        Ok(())
    }
}

fn normalize_preset_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_PRESET_NAME_CHARS {
        return Err("Custom theme names must contain 1 to 40 characters.".to_owned());
    }
    Ok(name.to_owned())
}

fn validate_asset_file_name(file_name: String) -> Result<String, String> {
    let stem = file_name
        .strip_suffix(".jpg")
        .ok_or_else(|| "The custom background asset is invalid.".to_owned())?;
    Uuid::parse_str(stem).map_err(|_| "The custom background asset is invalid.".to_owned())?;
    Ok(file_name)
}

fn image_data_url(mime_type: &str, bytes: &[u8]) -> String {
    format!("data:{mime_type};base64,{}", BASE64_STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use tempfile::tempdir;

    use super::{
        AppearanceStore, AppearanceThemeKind, DeleteCustomThemeRequest, ImportCustomThemeRequest,
        SelectAppearanceThemeRequest, UpdateAppearancePreferencesRequest, UpdateCustomThemeRequest,
        process_background,
    };

    fn jpeg(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(width, height, Rgb(color)));
        super::encode_jpeg(&image, 90).expect("encode test image")
    }

    fn jpeg_data_url(width: u32, height: u32, color: [u8; 3]) -> String {
        super::image_data_url("image/jpeg", &jpeg(width, height, color))
    }

    fn png_with_animation_control() -> Vec<u8> {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = u32::MAX;
            for byte in bytes {
                crc ^= u32::from(*byte);
                for _ in 0..8 {
                    crc = (crc >> 1) ^ (0xedb8_8320 & (0_u32.wrapping_sub(crc & 1)));
                }
            }
            !crc
        }

        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(1, 1, Rgb([1, 2, 3])));
        let mut cursor = Cursor::new(Vec::new());
        image
            .write_to(&mut cursor, ImageFormat::Png)
            .expect("encode PNG");
        let mut bytes = cursor.into_inner();
        let mut animation_chunk = Vec::new();
        animation_chunk.extend_from_slice(&8_u32.to_be_bytes());
        animation_chunk.extend_from_slice(b"acTL");
        animation_chunk.extend_from_slice(&1_u32.to_be_bytes());
        animation_chunk.extend_from_slice(&0_u32.to_be_bytes());
        animation_chunk.extend_from_slice(&crc32(&animation_chunk[4..]).to_be_bytes());
        bytes.splice(33..33, animation_chunk);
        bytes
    }

    #[test]
    fn custom_theme_lifecycle_is_persisted_and_assets_are_private() {
        let directory = tempdir().expect("temporary directory");
        let database = directory.path().join("desktop.sqlite3");
        let store = AppearanceStore::open(&database, false).expect("appearance store");
        let initial = store.load().expect("initial appearance");
        assert!(!initial.selection_initialized);
        assert_eq!(initial.active_theme.id, "daylight");
        assert_eq!(initial.palette_id, "daylight");
        assert!(initial.minimal_mode_enabled);

        let created = store
            .import_custom(ImportCustomThemeRequest {
                name: None,
                image_data_url: jpeg_data_url(1_600, 900, [36, 145, 220]),
            })
            .expect("custom theme");
        assert_eq!(created.active_theme.kind, AppearanceThemeKind::Custom);
        assert_eq!(created.custom_presets.len(), 1);
        assert_eq!(created.custom_presets[0].name, "自定义主题 1");
        assert!(created.active_background_data_url.is_some());

        let id = created.custom_presets[0].id.clone();
        let updated = store
            .update_custom(UpdateCustomThemeRequest {
                id: id.clone(),
                name: Some("海岸".to_owned()),
                focal_x: Some(0.25),
                focal_y: Some(0.75),
                image_data_url: None,
            })
            .expect("update custom theme");
        assert_eq!(updated.custom_presets[0].name, "海岸");
        assert_eq!(updated.custom_presets[0].focal_x, 0.25);
        assert_eq!(updated.custom_presets[0].focal_y, 0.75);

        let selected = store
            .select(SelectAppearanceThemeRequest {
                kind: AppearanceThemeKind::Builtin,
                id: "forest".to_owned(),
            })
            .expect("select built-in");
        assert_eq!(selected.active_theme.id, "forest");
        store
            .select(SelectAppearanceThemeRequest {
                kind: AppearanceThemeKind::Custom,
                id: id.clone(),
            })
            .expect("select custom theme");
        store
            .select(SelectAppearanceThemeRequest {
                kind: AppearanceThemeKind::Custom,
                id: id.clone(),
            })
            .expect("reselect custom theme");
        let deleted = store
            .delete_custom(DeleteCustomThemeRequest { id })
            .expect("delete custom theme");
        assert!(deleted.custom_presets.is_empty());
        assert_eq!(deleted.active_theme.id, "forest");
    }

    #[test]
    fn missing_active_asset_falls_back_without_hiding_the_repairable_preset() {
        let directory = tempdir().expect("temporary directory");
        let database = directory.path().join("desktop.sqlite3");
        let store = AppearanceStore::open(&database, false).expect("appearance store");
        let created = store
            .import_custom(ImportCustomThemeRequest {
                name: None,
                image_data_url: jpeg_data_url(320, 180, [40, 90, 160]),
            })
            .expect("custom theme");
        let id = created.active_theme.id;
        let file_name = store.asset_file_name(&id).expect("managed asset");
        std::fs::remove_file(store.asset_directory.join(file_name)).expect("remove test asset");

        let loaded = store.load().expect("fallback appearance");
        assert_eq!(loaded.active_theme.kind, AppearanceThemeKind::Builtin);
        assert_eq!(loaded.active_theme.id, "daylight");
        assert_eq!(loaded.custom_presets.len(), 1);
    }

    #[test]
    fn default_custom_names_remain_unique_after_deletion() {
        let directory = tempdir().expect("temporary directory");
        let store = AppearanceStore::open(directory.path().join("desktop.sqlite3"), false)
            .expect("appearance store");
        let first = store
            .import_custom(ImportCustomThemeRequest {
                name: None,
                image_data_url: jpeg_data_url(320, 180, [40, 90, 160]),
            })
            .expect("first theme");
        let first_id = first.active_theme.id;
        let second = store
            .import_custom(ImportCustomThemeRequest {
                name: None,
                image_data_url: jpeg_data_url(320, 180, [80, 120, 170]),
            })
            .expect("second theme");
        store
            .delete_custom(DeleteCustomThemeRequest { id: first_id })
            .expect("delete first theme");
        let third = store
            .import_custom(ImportCustomThemeRequest {
                name: None,
                image_data_url: jpeg_data_url(320, 180, [120, 150, 190]),
            })
            .expect("third theme");
        let names = third
            .custom_presets
            .iter()
            .map(|preset| preset.name.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(names.len(), 2);
        assert!(names.contains("自定义主题 1"));
        assert!(names.contains("自定义主题 2"));
        assert_eq!(second.custom_presets.len(), 2);
    }

    #[test]
    fn image_processing_rejects_untrusted_and_oversized_sources() {
        assert!(process_background(b"<svg><script/></svg>").is_err());
        assert!(process_background(&vec![0; super::MAX_SOURCE_BYTES + 1]).is_err());
        assert_eq!(
            process_background(&png_with_animation_control())
                .err()
                .expect("animated image rejection"),
            "Animated background images are not supported."
        );
    }

    #[test]
    fn image_analysis_selects_an_initial_complete_palette() {
        let bright = process_background(&jpeg(320, 180, [35, 190, 150]))
            .expect("bright analyzed background");
        assert_eq!(bright.palette_id, "teal-light");

        let dark =
            process_background(&jpeg(320, 180, [24, 42, 82])).expect("dark analyzed background");
        assert!(dark.palette_id.ends_with("-dark"));
    }

    #[test]
    fn custom_background_restores_the_users_palette_only_in_image_mode() {
        let directory = tempdir().expect("temporary directory");
        let store = AppearanceStore::open(directory.path().join("desktop.sqlite3"), false)
            .expect("appearance store");
        let created = store
            .import_custom(ImportCustomThemeRequest {
                name: None,
                image_data_url: jpeg_data_url(320, 180, [35, 190, 150]),
            })
            .expect("analyzed custom theme");
        let id = created.active_theme.id.clone();
        assert_eq!(created.palette_id, "daylight");
        assert_eq!(created.custom_presets[0].palette_id, "teal-light");

        let image_mode = store
            .update_preferences(UpdateAppearancePreferencesRequest {
                palette_id: None,
                minimal_mode_enabled: Some(false),
            })
            .expect("enable image mode");
        assert_eq!(image_mode.palette_id, "teal-light");

        let chosen = store
            .update_preferences(UpdateAppearancePreferencesRequest {
                palette_id: Some("rose-dark".to_owned()),
                minimal_mode_enabled: None,
            })
            .expect("choose custom palette");
        assert_eq!(chosen.palette_id, "rose-dark");
        assert_eq!(chosen.custom_presets[0].palette_id, "rose-dark");

        let replaced = store
            .update_custom(UpdateCustomThemeRequest {
                id: id.clone(),
                name: None,
                focal_x: None,
                focal_y: None,
                image_data_url: Some(jpeg_data_url(320, 180, [230, 190, 45])),
            })
            .expect("replace custom background");
        assert_eq!(replaced.custom_presets[0].palette_id, "rose-dark");

        store
            .select(SelectAppearanceThemeRequest {
                kind: AppearanceThemeKind::Builtin,
                id: "forest".to_owned(),
            })
            .expect("select built-in theme");
        let restored = store
            .select(SelectAppearanceThemeRequest {
                kind: AppearanceThemeKind::Custom,
                id: id.clone(),
            })
            .expect("restore custom theme");
        assert_eq!(restored.palette_id, "rose-dark");

        store
            .update_preferences(UpdateAppearancePreferencesRequest {
                palette_id: None,
                minimal_mode_enabled: Some(true),
            })
            .expect("enable minimal mode");
        let minimal_palette = store
            .update_preferences(UpdateAppearancePreferencesRequest {
                palette_id: Some("yellow-light".to_owned()),
                minimal_mode_enabled: None,
            })
            .expect("choose minimal palette");
        assert_eq!(minimal_palette.palette_id, "yellow-light");
        assert_eq!(minimal_palette.custom_presets[0].palette_id, "rose-dark");

        store
            .select(SelectAppearanceThemeRequest {
                kind: AppearanceThemeKind::Builtin,
                id: "night".to_owned(),
            })
            .expect("dormant built-in selection");
        let dormant_custom = store
            .select(SelectAppearanceThemeRequest {
                kind: AppearanceThemeKind::Custom,
                id,
            })
            .expect("dormant custom selection");
        assert_eq!(dormant_custom.palette_id, "yellow-light");

        let restored_image_mode = store
            .update_preferences(UpdateAppearancePreferencesRequest {
                palette_id: None,
                minimal_mode_enabled: Some(false),
            })
            .expect("restore image mode");
        assert_eq!(restored_image_mode.palette_id, "rose-dark");
    }

    #[test]
    fn legacy_palette_and_mode_are_migrated_to_one_complete_palette_id() {
        let directory = tempdir().expect("temporary directory");
        let database = directory.path().join("desktop.sqlite3");
        let store = AppearanceStore::open(&database, false).expect("appearance store");
        let created = store
            .import_custom(ImportCustomThemeRequest {
                name: None,
                image_data_url: jpeg_data_url(320, 180, [35, 190, 150]),
            })
            .expect("custom theme");
        let id = created.custom_presets[0].id.clone();
        store
            .connection()
            .expect("appearance connection")
            .execute(
                "UPDATE custom_theme_presets
                 SET palette_id = 'teal', mode = 'dark', auto_mode = 'light'
                 WHERE id = ?1",
                [&id],
            )
            .expect("legacy appearance row");

        let reopened = AppearanceStore::open(&database, true).expect("reopened appearance store");
        let loaded = reopened.load().expect("migrated appearance");
        assert_eq!(loaded.custom_presets[0].palette_id, "teal-dark");
        let stored_modes = reopened
            .connection()
            .expect("appearance connection")
            .query_row(
                "SELECT mode, auto_mode FROM custom_theme_presets WHERE id = ?1",
                [&id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .expect("migrated modes");
        assert_eq!(stored_modes, ("auto".to_owned(), "dark".to_owned()));
    }

    #[test]
    fn existing_install_preserves_the_legacy_surface_and_palette() {
        let directory = tempdir().expect("temporary directory");
        let database = directory.path().join("desktop.sqlite3");
        {
            let connection = rusqlite::Connection::open(&database).expect("legacy database");
            connection
                .execute_batch(
                    "CREATE TABLE appearance_state (
                         id INTEGER PRIMARY KEY NOT NULL,
                         active_kind TEXT NOT NULL,
                         active_theme_id TEXT NOT NULL,
                         previous_kind TEXT,
                         previous_theme_id TEXT,
                         updated_at TEXT NOT NULL
                     );
                     INSERT INTO appearance_state (
                         id, active_kind, active_theme_id, updated_at
                     ) VALUES (1, 'builtin', 'dusk', 'legacy');",
                )
                .expect("legacy appearance state");
        }

        let store = AppearanceStore::open(&database, true).expect("migrated store");
        let appearance = store.load().expect("migrated appearance");
        assert_eq!(appearance.palette_id, "dusk");
        assert!(!appearance.minimal_mode_enabled);
        assert_eq!(appearance.active_theme.id, "dusk");
    }

    #[test]
    fn every_complete_palette_can_drive_minimal_mode_independently_of_the_background() {
        let directory = tempdir().expect("temporary directory");
        let database = directory.path().join("desktop.sqlite3");
        let store = AppearanceStore::open(&database, false).expect("appearance store");
        store
            .select(SelectAppearanceThemeRequest {
                kind: AppearanceThemeKind::Builtin,
                id: "forest".to_owned(),
            })
            .expect("background selection");

        for family in super::PALETTE_FAMILY_IDS {
            for mode in ["light", "dark"] {
                let palette_id = format!("{family}-{mode}");
                let appearance = store
                    .update_preferences(UpdateAppearancePreferencesRequest {
                        palette_id: Some(palette_id.clone()),
                        minimal_mode_enabled: Some(true),
                    })
                    .expect("valid palette preference");
                assert_eq!(appearance.palette_id, palette_id);
                assert!(appearance.minimal_mode_enabled);
                assert_eq!(appearance.active_theme.id, "forest");
            }
        }
        for palette_id in super::BUILTIN_PALETTE_IDS {
            let appearance = store
                .update_preferences(UpdateAppearancePreferencesRequest {
                    palette_id: Some(palette_id.to_owned()),
                    minimal_mode_enabled: Some(true),
                })
                .expect("valid built-in palette preference");
            assert_eq!(appearance.palette_id, palette_id);
            assert!(appearance.minimal_mode_enabled);
            assert_eq!(appearance.active_theme.id, "forest");
        }
    }

    #[test]
    fn built_in_themes_restore_their_original_palette_only_in_image_mode() {
        let directory = tempdir().expect("temporary directory");
        let database = directory.path().join("desktop.sqlite3");
        let store = AppearanceStore::open(&database, false).expect("appearance store");
        store
            .update_preferences(UpdateAppearancePreferencesRequest {
                palette_id: Some("teal-dark".to_owned()),
                minimal_mode_enabled: Some(true),
            })
            .expect("minimal palette");

        let dormant = store
            .select(SelectAppearanceThemeRequest {
                kind: AppearanceThemeKind::Builtin,
                id: "forest".to_owned(),
            })
            .expect("dormant background selection");
        assert_eq!(dormant.palette_id, "teal-dark");

        let image_mode = store
            .update_preferences(UpdateAppearancePreferencesRequest {
                palette_id: None,
                minimal_mode_enabled: Some(false),
            })
            .expect("enable image mode");
        assert_eq!(image_mode.palette_id, "forest");

        let night = store
            .select(SelectAppearanceThemeRequest {
                kind: AppearanceThemeKind::Builtin,
                id: "night".to_owned(),
            })
            .expect("select night theme");
        assert_eq!(night.palette_id, "night");
    }

    #[test]
    fn invalid_palette_does_not_replace_the_saved_preference() {
        let directory = tempdir().expect("temporary directory");
        let database = directory.path().join("desktop.sqlite3");
        let store = AppearanceStore::open(&database, false).expect("appearance store");
        assert!(
            store
                .update_preferences(UpdateAppearancePreferencesRequest {
                    palette_id: Some("rainbow-light".to_owned()),
                    minimal_mode_enabled: Some(false),
                })
                .is_err()
        );
        let appearance = store.load().expect("unchanged preferences");
        assert_eq!(appearance.palette_id, "daylight");
        assert!(appearance.minimal_mode_enabled);
    }
}
