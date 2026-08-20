use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

#[cfg(target_os = "windows")]
use std::{os::windows::ffi::OsStrExt, path::Prefix};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use uuid::Uuid;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Storage::FileSystem::{
    GetDriveTypeW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

use crate::diagnostics::{self, ErrorKind as DiagnosticErrorKind, Fields as DiagnosticFields};

const STORAGE_SCHEMA_VERSION: u8 = 1;
const STORAGE_LOCATOR_FILE: &str = "storage-location.json";
const STORAGE_MIGRATION_FILE: &str = "storage-migration.json";
const STORAGE_MIGRATION_RESULT_FILE: &str = "storage-migration-result.json";
const WEBVIEW_CACHE_CLEANUP_FILE: &str = "webview-cache-cleanup.json";
const APP_UPDATE_RELAUNCH_FILE: &str = "app-update-relaunch.json";
const DATA_ROOT_MARKER_FILE: &str = ".mine-mail-data.json";
const INSTALL_DATA_DIRECTORY_NAME: &str = "Data";
const WEBVIEW_DATA_DIRECTORY_NAME: &str = "EBWebView";
const WEBVIEW_CACHE_DIRECTORY_NAMES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GPUPersistentCache",
    "GrShaderCache",
    "ShaderCache",
    "component_crx_cache",
    "extensions_crx_cache",
    "AutofillAiModelCache",
];
const WEBVIEW_CACHE_CLEANUP_RETRY_ATTEMPTS: usize = 20;
const WEBVIEW_CACHE_CLEANUP_RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StorageLocator {
    schema_version: u8,
    data_root: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DataRootMarker {
    schema_version: u8,
    identifier: String,
    state: DataRootState,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DataRootState {
    Migrating,
    Ready,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StorageMigrationRequest {
    schema_version: u8,
    source: PathBuf,
    target: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WebviewCacheCleanupRequest {
    schema_version: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredMigrationResult {
    status: MigrationResultStatus,
    moved_bytes: u64,
    message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AppUpdateRelaunchRequest {
    schema_version: u8,
    expected_version: String,
}

struct MigrationExecution {
    moved_bytes: u64,
    cleanup_warning: bool,
}

struct WebviewCacheCleanupExecution {
    removed_bytes: u64,
    failed_directories: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum MigrationResultStatus {
    Completed,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StorageCategoryDto {
    id: &'static str,
    label: &'static str,
    bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StorageMigrationNoticeDto {
    status: MigrationResultStatus,
    moved_bytes: u64,
    message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebviewCacheCleanupNoticeDto {
    status: MigrationResultStatus,
    removed_bytes: u64,
    message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StorageStatusDto {
    // The user-visible data directory label. The raw path still crosses the
    // UI boundary by explicit product decision so the settings page can show
    // where mail is stored; the directory picker does not use it as a preset.
    data_path: String,
    location_kind: StorageLocationKind,
    available: bool,
    total_bytes: u64,
    reclaimable_webview_bytes: u64,
    categories: Vec<StorageCategoryDto>,
    migration_notice: Option<StorageMigrationNoticeDto>,
    cache_cleanup_notice: Option<WebviewCacheCleanupNoticeDto>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StorageLocationKind {
    InstallDirectory,
    LocalAppData,
    Custom,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparedStorageMigrationDto {
    total_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparedWebviewCacheCleanupDto {
    reclaimable_bytes: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct StorageRuntime {
    bootstrap_dir: PathBuf,
    data_root: PathBuf,
    install_data_root: Option<PathBuf>,
    location_kind: StorageLocationKind,
    migration_notice: Option<StorageMigrationNoticeDto>,
    cache_cleanup_notice: Option<WebviewCacheCleanupNoticeDto>,
    /// Serializes prepare/cancel migration commands so concurrent calls cannot
    /// interleave writes to the migration task file.
    pub(crate) migration_gate: Arc<Mutex<()>>,
}

pub(crate) struct StorageInitialization {
    pub runtime: StorageRuntime,
    pub runtime_data_root: PathBuf,
    pub startup_error: Option<String>,
}

impl StorageRuntime {
    pub(crate) fn initialize<R: Runtime>(app: &AppHandle<R>) -> StorageInitialization {
        let bootstrap_dir = match app.path().app_local_data_dir() {
            Ok(path) => path,
            Err(_) => {
                let degraded = env::temp_dir().join("mine-mail-degraded");
                return StorageInitialization {
                    runtime: Self {
                        bootstrap_dir: degraded.clone(),
                        data_root: degraded.clone(),
                        install_data_root: None,
                        location_kind: StorageLocationKind::LocalAppData,
                        migration_notice: None,
                        cache_cleanup_notice: None,
                        migration_gate: Arc::new(Mutex::new(())),
                    },
                    runtime_data_root: degraded,
                    startup_error: Some(
                        "The application data directory is unavailable; local mail is disabled for this session."
                            .to_owned(),
                    ),
                };
            }
        };

        if fs::create_dir_all(&bootstrap_dir).is_err() {
            let degraded = env::temp_dir().join("mine-mail-degraded");
            return StorageInitialization {
                runtime: Self {
                    bootstrap_dir,
                    data_root: degraded.clone(),
                    install_data_root: None,
                    location_kind: StorageLocationKind::LocalAppData,
                    migration_notice: None,
                    cache_cleanup_notice: None,
                    migration_gate: Arc::new(Mutex::new(())),
                },
                runtime_data_root: degraded,
                startup_error: Some(
                    "The application data directory is unavailable; local mail is disabled for this session."
                        .to_owned(),
                ),
            };
        }

        let executable_dir = match app.path().executable_dir() {
            Ok(directory) => {
                diagnostics::limited_recovery(
                    "storage_executable_directory_failed",
                    "storage_executable_directory_recovered",
                    "storage_runtime_open",
                    None,
                );
                Some(directory)
            }
            Err(_) => {
                diagnostics::limited_failure(
                    "storage_executable_directory_failed",
                    "storage_runtime_open",
                    None,
                    DiagnosticErrorKind::Io,
                );
                None
            }
        };
        let install_data_root = executable_dir
            .as_deref()
            .map(|directory| directory.join(INSTALL_DATA_DIRECTORY_NAME));
        let migration_notice = process_pending_migration(&bootstrap_dir);
        if let Some(notice) = migration_notice.as_ref() {
            let fields = DiagnosticFields::default()
                .operation("storage_migration")
                .outcome(match notice.status {
                    MigrationResultStatus::Completed => "completed",
                    MigrationResultStatus::Failed => "failed",
                })
                .moved_bytes(notice.moved_bytes);
            match notice.status {
                MigrationResultStatus::Completed => {
                    diagnostics::info("storage_migration_completed", fields)
                }
                MigrationResultStatus::Failed => diagnostics::error(
                    "storage_migration_failed",
                    fields.error(DiagnosticErrorKind::Io),
                ),
            }
        }

        let resolved = resolve_data_root(
            &bootstrap_dir,
            executable_dir.as_deref(),
            prefer_install_directory_data(),
        );
        let (data_root, location_kind, startup_error) = match resolved {
            Ok(data_root) => {
                let kind =
                    storage_location_kind(&data_root, &bootstrap_dir, install_data_root.as_deref());
                (data_root, kind, None)
            }
            Err((configured_root, error)) => {
                let kind = storage_location_kind(
                    &configured_root,
                    &bootstrap_dir,
                    install_data_root.as_deref(),
                );
                (configured_root, kind, Some(error))
            }
        };

        let cache_cleanup_notice =
            process_pending_webview_cache_cleanup(&bootstrap_dir, &data_root);
        if let Some(notice) = cache_cleanup_notice.as_ref() {
            let fields = DiagnosticFields::default()
                .operation("webview_cache_cleanup")
                .outcome(match notice.status {
                    MigrationResultStatus::Completed => "completed",
                    MigrationResultStatus::Failed => "partial_or_failed",
                })
                .moved_bytes(notice.removed_bytes);
            match notice.status {
                MigrationResultStatus::Completed => {
                    diagnostics::info("webview_cache_cleanup_completed", fields)
                }
                MigrationResultStatus::Failed => diagnostics::error(
                    "webview_cache_cleanup_failed",
                    fields.error(DiagnosticErrorKind::Io),
                ),
            }
        }

        let runtime_data_root = if startup_error.is_some() {
            env::temp_dir().join("mine-mail-degraded")
        } else {
            data_root.clone()
        };

        StorageInitialization {
            runtime: Self {
                bootstrap_dir,
                data_root,
                install_data_root,
                location_kind,
                migration_notice,
                cache_cleanup_notice,
                migration_gate: Arc::new(Mutex::new(())),
            },
            runtime_data_root,
            startup_error,
        }
    }

    pub(crate) fn status(&self) -> Result<StorageStatusDto, String> {
        let available = self.data_root.is_dir();
        let mut sizes = StorageSizes::default();
        if available {
            sizes = measure_storage_root(&self.data_root)
                .map_err(|_| "无法读取当前数据目录的空间占用。".to_owned())?;
        }
        if self.data_root != self.bootstrap_dir {
            match directory_size(&self.bootstrap_dir.join("logs")) {
                Ok(bytes) => {
                    sizes.logs = bytes;
                    diagnostics::limited_recovery(
                        "storage_log_measurement_failed",
                        "storage_log_measurement_recovered",
                        "measure_storage_logs",
                        None,
                    );
                }
                Err(_) => diagnostics::limited_failure(
                    "storage_log_measurement_failed",
                    "measure_storage_logs",
                    None,
                    DiagnosticErrorKind::Io,
                ),
            }
        }

        let categories = vec![
            StorageCategoryDto {
                id: "mail",
                label: "邮件与本地资料",
                bytes: sizes.mail,
            },
            StorageCategoryDto {
                id: "webview",
                label: "界面与浏览器缓存",
                bytes: sizes.webview,
            },
            StorageCategoryDto {
                id: "user_assets",
                label: "用户资源",
                bytes: sizes.user_assets,
            },
            StorageCategoryDto {
                id: "cache",
                label: "可清理缓存",
                bytes: sizes.cache,
            },
            StorageCategoryDto {
                id: "logs",
                label: "诊断日志",
                bytes: sizes.logs,
            },
            StorageCategoryDto {
                id: "other",
                label: "其他数据",
                bytes: sizes.other,
            },
        ];
        let total_bytes = categories.iter().map(|category| category.bytes).sum();
        let reclaimable_webview_bytes =
            reclaimable_webview_cache_bytes(&self.data_root, &self.bootstrap_dir).unwrap_or_else(
                |_| {
                    diagnostics::limited_failure(
                        "webview_cache_measurement_failed",
                        "measure_webview_cache",
                        None,
                        DiagnosticErrorKind::Io,
                    );
                    0
                },
            );

        Ok(StorageStatusDto {
            data_path: self.data_root.to_string_lossy().into_owned(),
            location_kind: self.location_kind,
            available,
            total_bytes,
            reclaimable_webview_bytes,
            categories,
            migration_notice: self.migration_notice.clone(),
            cache_cleanup_notice: self.cache_cleanup_notice.clone(),
        })
    }

    pub(crate) fn prepare_app_update_relaunch(&self, expected_version: &str) -> Result<(), String> {
        write_json_atomically(
            &self.bootstrap_dir.join(APP_UPDATE_RELAUNCH_FILE),
            &AppUpdateRelaunchRequest {
                schema_version: STORAGE_SCHEMA_VERSION,
                expected_version: expected_version.to_owned(),
            },
        )
        .map_err(|_| {
            diagnostics::error(
                "app_update_relaunch_marker_write_failed",
                DiagnosticFields::default()
                    .operation("app_update_relaunch")
                    .error(DiagnosticErrorKind::Io),
            );
            "Mine Mail could not prepare the update relaunch.".to_owned()
        })
    }

    pub(crate) fn rollback_app_update_relaunch(&self, expected_version: &str) {
        rollback_app_update_relaunch(&self.bootstrap_dir, expected_version);
    }

    pub(crate) fn consume_app_update_relaunch(&self, current_version: &str) -> bool {
        consume_app_update_relaunch(&self.bootstrap_dir, current_version)
    }

    pub(crate) fn prepare_migration(
        &self,
        requested_target: &str,
    ) -> Result<PreparedStorageMigrationDto, String> {
        if !self.data_root.is_dir() {
            return Err("当前数据目录不可用，无法开始迁移。请先重新连接原数据磁盘。".to_owned());
        }

        let target = validate_and_prepare_migration_target(
            requested_target,
            &self.data_root,
            &self.bootstrap_dir,
            self.install_data_root.as_deref(),
        )?;
        let total_bytes = managed_tree_size(&self.data_root)
            .map_err(|_| "无法计算待迁移数据的大小。".to_owned())?;
        let request = StorageMigrationRequest {
            schema_version: STORAGE_SCHEMA_VERSION,
            source: self.data_root.clone(),
            target: target.clone(),
        };
        write_json_atomically(&self.bootstrap_dir.join(STORAGE_MIGRATION_FILE), &request)
            .map_err(|_| "无法保存迁移任务，请确认系统数据目录可写。".to_owned())?;
        let previous_result_path = self.bootstrap_dir.join(STORAGE_MIGRATION_RESULT_FILE);
        match fs::remove_file(previous_result_path) {
            Ok(()) => diagnostics::limited_recovery(
                "storage_migration_result_cleanup_failed",
                "storage_migration_result_cleanup_recovered",
                "prepare_storage_migration",
                None,
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => diagnostics::limited_failure(
                "storage_migration_result_cleanup_failed",
                "prepare_storage_migration",
                None,
                DiagnosticErrorKind::Io,
            ),
        }

        diagnostics::info(
            "storage_migration_prepared",
            DiagnosticFields::default()
                .operation("storage_migration")
                .outcome("restart_required")
                .moved_bytes(total_bytes),
        );

        Ok(PreparedStorageMigrationDto { total_bytes })
    }

    pub(crate) fn cancel_pending_migration(&self) -> Result<(), String> {
        match fs::remove_file(self.bootstrap_dir.join(STORAGE_MIGRATION_FILE)) {
            Ok(()) => {
                diagnostics::info(
                    "storage_migration_cancelled",
                    DiagnosticFields::default().operation("storage_migration"),
                );
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("无法撤销待执行的数据迁移任务。".to_owned()),
        }
    }

    pub(crate) fn prepare_webview_cache_cleanup(
        &self,
    ) -> Result<PreparedWebviewCacheCleanupDto, String> {
        let reclaimable_bytes =
            reclaimable_webview_cache_bytes(&self.data_root, &self.bootstrap_dir)
                .map_err(|_| "无法计算可释放的界面缓存大小。".to_owned())?;
        write_json_atomically(
            &self.bootstrap_dir.join(WEBVIEW_CACHE_CLEANUP_FILE),
            &WebviewCacheCleanupRequest {
                schema_version: STORAGE_SCHEMA_VERSION,
            },
        )
        .map_err(|_| "无法保存缓存清理任务，请确认系统数据目录可写。".to_owned())?;

        diagnostics::info(
            "webview_cache_cleanup_prepared",
            DiagnosticFields::default()
                .operation("webview_cache_cleanup")
                .outcome("restart_required")
                .moved_bytes(reclaimable_bytes),
        );

        Ok(PreparedWebviewCacheCleanupDto { reclaimable_bytes })
    }

    pub(crate) fn cancel_pending_webview_cache_cleanup(&self) -> Result<(), String> {
        match fs::remove_file(self.bootstrap_dir.join(WEBVIEW_CACHE_CLEANUP_FILE)) {
            Ok(()) => {
                diagnostics::info(
                    "webview_cache_cleanup_cancelled",
                    DiagnosticFields::default().operation("webview_cache_cleanup"),
                );
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("无法撤销待执行的界面缓存清理任务。".to_owned()),
        }
    }
}

fn prefer_install_directory_data() -> bool {
    cfg!(target_os = "windows") && !cfg!(debug_assertions)
}

fn resolve_data_root(
    bootstrap_dir: &Path,
    executable_dir: Option<&Path>,
    prefer_install: bool,
) -> Result<PathBuf, (PathBuf, String)> {
    let locator_path = bootstrap_dir.join(STORAGE_LOCATOR_FILE);
    if locator_path.exists() {
        let locator: StorageLocator = read_json(&locator_path).map_err(|_| {
            (
                bootstrap_dir.to_path_buf(),
                "本地数据位置配置损坏；为避免覆盖现有邮件，Mine Mail 已停止加载本地数据。"
                    .to_owned(),
            )
        })?;
        if locator.schema_version != STORAGE_SCHEMA_VERSION || !locator.data_root.is_absolute() {
            return Err((
                locator.data_root,
                "本地数据位置配置不受支持；为避免覆盖现有邮件，Mine Mail 已停止加载本地数据。"
                    .to_owned(),
            ));
        }
        if ensure_existing_data_root(&locator.data_root, locator.data_root == bootstrap_dir)
            .is_err()
        {
            return Err((
                locator.data_root,
                "已配置的数据目录不可用；请重新连接对应磁盘或在设置中选择新的位置。".to_owned(),
            ));
        }
        return Ok(locator.data_root);
    }

    let selected = if has_legacy_data(bootstrap_dir) {
        bootstrap_dir.to_path_buf()
    } else if prefer_install {
        executable_dir
            .map(|directory| directory.join(INSTALL_DATA_DIRECTORY_NAME))
            .filter(|candidate| prepare_new_data_root(candidate).is_ok())
            .unwrap_or_else(|| bootstrap_dir.to_path_buf())
    } else {
        bootstrap_dir.to_path_buf()
    };

    if selected == bootstrap_dir {
        ensure_existing_data_root(&selected, true).map_err(|_| {
            (
                selected.clone(),
                "Windows 本地数据目录不可写；Mine Mail 已停止加载本地数据。".to_owned(),
            )
        })?;
    }
    let locator = StorageLocator {
        schema_version: STORAGE_SCHEMA_VERSION,
        data_root: selected.clone(),
    };
    write_json_atomically(&locator_path, &locator).map_err(|_| {
        (
            selected.clone(),
            "无法保存本地数据位置；Mine Mail 已停止加载本地数据。".to_owned(),
        )
    })?;
    Ok(selected)
}

fn prepare_new_data_root(path: &Path) -> io::Result<()> {
    if is_protected_install_location(path) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "protected install location",
        ));
    }
    if path.exists() {
        if !path.is_dir() || fs::read_dir(path)?.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "data directory is not empty",
            ));
        }
    } else {
        fs::create_dir_all(path)?;
    }
    probe_directory_write(path)?;
    write_data_root_marker(path, DataRootState::Ready)
}

fn ensure_existing_data_root(path: &Path, allow_unmarked_legacy: bool) -> io::Result<()> {
    if !path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "data directory is missing",
        ));
    }
    probe_directory_write(path)?;
    match read_data_root_marker(path) {
        Ok(marker)
            if marker.schema_version == STORAGE_SCHEMA_VERSION
                && marker.identifier == "com.minemail.desktop"
                && marker.state == DataRootState::Ready =>
        {
            Ok(())
        }
        _ if allow_unmarked_legacy => write_data_root_marker(path, DataRootState::Ready),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "data directory marker is missing",
        )),
    }
}

fn has_legacy_data(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        !is_bootstrap_entry(&name)
            && name != "logs"
            && name != DATA_ROOT_MARKER_FILE
            && (name == "account.json"
                || name == WEBVIEW_DATA_DIRECTORY_NAME
                || name.ends_with(".sqlite3")
                || name.ends_with(".sqlite3-wal")
                || name.ends_with(".sqlite3-shm")
                || entry.path().is_dir())
    })
}

fn storage_location_kind(
    data_root: &Path,
    bootstrap_dir: &Path,
    install_data_root: Option<&Path>,
) -> StorageLocationKind {
    if data_root == bootstrap_dir {
        StorageLocationKind::LocalAppData
    } else if install_data_root.is_some_and(|install| data_root == install) {
        StorageLocationKind::InstallDirectory
    } else {
        StorageLocationKind::Custom
    }
}

fn validate_and_prepare_migration_target(
    requested_target: &str,
    source: &Path,
    bootstrap_dir: &Path,
    install_data_root: Option<&Path>,
) -> Result<PathBuf, String> {
    let raw = PathBuf::from(requested_target.trim());
    if requested_target.trim().is_empty() || !raw.is_absolute() || has_parent_component(&raw) {
        return Err("请选择一个有效的本机绝对路径。".to_owned());
    }
    if is_network_path(&raw) {
        return Err("邮件数据库不能迁移到网络共享目录，请选择本机固定磁盘。".to_owned());
    }
    if is_protected_install_location(&raw) {
        return Err("所选目录受 Windows 保护，请选择其他可写目录。".to_owned());
    }

    fs::create_dir_all(&raw).map_err(|_| "无法创建所选数据目录。".to_owned())?;
    let target = fs::canonicalize(&raw).map_err(|_| "无法访问所选数据目录。".to_owned())?;
    let source = fs::canonicalize(source).map_err(|_| "当前数据目录不可用。".to_owned())?;
    let bootstrap =
        fs::canonicalize(bootstrap_dir).map_err(|_| "系统数据目录不可用。".to_owned())?;

    if target == source {
        return Err("所选目录已经是当前数据目录。".to_owned());
    }
    if target.starts_with(&source) || source.starts_with(&target) {
        return Err("新旧数据目录不能互相包含，请选择独立目录。".to_owned());
    }
    if target == bootstrap {
        return Err("请选择与系统配置目录不同的位置。".to_owned());
    }
    if install_data_root.is_some_and(|install| {
        install
            .parent()
            .is_some_and(|install_dir| target == install_dir)
    }) {
        return Err("不能直接使用应用安装目录，请选择其中独立的 Data 文件夹。".to_owned());
    }

    let mut entries = fs::read_dir(&target).map_err(|_| "无法读取所选数据目录。".to_owned())?;
    if entries.next().is_some() {
        return Err("请选择一个空文件夹，避免覆盖其中已有文件。".to_owned());
    }
    probe_directory_write(&target).map_err(|_| "所选数据目录不可写。".to_owned())?;
    Ok(target)
}

fn process_pending_webview_cache_cleanup(
    bootstrap_dir: &Path,
    data_root: &Path,
) -> Option<WebviewCacheCleanupNoticeDto> {
    let request_path = bootstrap_dir.join(WEBVIEW_CACHE_CLEANUP_FILE);
    if !request_path.exists() {
        return None;
    }

    let outcome = read_json::<WebviewCacheCleanupRequest>(&request_path)
        .map_err(|_| "缓存清理任务损坏，未删除任何数据。".to_owned())
        .and_then(|request| {
            if request.schema_version != STORAGE_SCHEMA_VERSION {
                return Err("缓存清理任务版本不受支持，未删除任何数据。".to_owned());
            }
            cleanup_webview_caches(data_root, bootstrap_dir)
                .map_err(|_| "无法读取界面缓存目录，缓存未完成清理。".to_owned())
        });

    match fs::remove_file(&request_path) {
        Ok(()) => diagnostics::limited_recovery(
            "webview_cache_cleanup_request_removal_failed",
            "webview_cache_cleanup_request_removal_recovered",
            "process_webview_cache_cleanup",
            None,
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => diagnostics::limited_failure(
            "webview_cache_cleanup_request_removal_failed",
            "process_webview_cache_cleanup",
            None,
            DiagnosticErrorKind::Io,
        ),
    }

    Some(match outcome {
        Ok(execution) if execution.failed_directories == 0 => WebviewCacheCleanupNoticeDto {
            status: MigrationResultStatus::Completed,
            removed_bytes: execution.removed_bytes,
            message: if execution.removed_bytes == 0 {
                "当前没有需要释放的界面缓存。".to_owned()
            } else {
                "界面缓存已释放。".to_owned()
            },
        },
        Ok(execution) => WebviewCacheCleanupNoticeDto {
            status: MigrationResultStatus::Failed,
            removed_bytes: execution.removed_bytes,
            message: "已释放部分界面缓存；仍有文件被系统占用，可再次重试。".to_owned(),
        },
        Err(message) => WebviewCacheCleanupNoticeDto {
            status: MigrationResultStatus::Failed,
            removed_bytes: 0,
            message,
        },
    })
}

fn reclaimable_webview_cache_bytes(data_root: &Path, bootstrap_dir: &Path) -> io::Result<u64> {
    let targets = collect_reclaimable_webview_cache_directories(data_root, bootstrap_dir)?;
    let mut total = 0u64;
    for target in targets {
        total = total.saturating_add(entry_size(&target)?);
    }
    Ok(total)
}

fn cleanup_webview_caches(
    data_root: &Path,
    bootstrap_dir: &Path,
) -> io::Result<WebviewCacheCleanupExecution> {
    let targets = collect_reclaimable_webview_cache_directories(data_root, bootstrap_dir)?;
    let mut pending = Vec::with_capacity(targets.len());
    let mut failed_directories = 0;
    for target in targets {
        match entry_size(&target) {
            Ok(bytes) => pending.push((target, bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => failed_directories += 1,
        }
    }

    Ok(cleanup_webview_cache_targets(
        pending,
        failed_directories,
        |target| fs::remove_dir_all(target),
        |duration| thread::sleep(duration),
    ))
}

fn cleanup_webview_cache_targets<Remove, Wait>(
    mut pending: Vec<(PathBuf, u64)>,
    initial_failed_directories: usize,
    mut remove: Remove,
    mut wait: Wait,
) -> WebviewCacheCleanupExecution
where
    Remove: FnMut(&Path) -> io::Result<()>,
    Wait: FnMut(Duration),
{
    let mut execution = WebviewCacheCleanupExecution {
        removed_bytes: 0,
        failed_directories: initial_failed_directories,
    };

    for attempt in 0..WEBVIEW_CACHE_CLEANUP_RETRY_ATTEMPTS {
        let mut still_pending = Vec::with_capacity(pending.len());
        for (target, bytes) in pending {
            match remove(&target) {
                Ok(()) => execution.removed_bytes = execution.removed_bytes.saturating_add(bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    execution.removed_bytes = execution.removed_bytes.saturating_add(bytes)
                }
                Err(_) => still_pending.push((target, bytes)),
            }
        }
        pending = still_pending;

        if pending.is_empty() {
            break;
        }
        if attempt + 1 < WEBVIEW_CACHE_CLEANUP_RETRY_ATTEMPTS {
            wait(WEBVIEW_CACHE_CLEANUP_RETRY_DELAY);
        }
    }

    execution.failed_directories = execution.failed_directories.saturating_add(pending.len());
    execution
}

fn collect_reclaimable_webview_cache_directories(
    data_root: &Path,
    bootstrap_dir: &Path,
) -> io::Result<Vec<PathBuf>> {
    let mut targets = BTreeSet::new();
    collect_webview_cache_directories(
        &bootstrap_dir.join(WEBVIEW_DATA_DIRECTORY_NAME),
        &mut targets,
    )?;
    if data_root != bootstrap_dir && is_trusted_cleanup_data_root(data_root) {
        collect_webview_cache_directories(
            &data_root.join(WEBVIEW_DATA_DIRECTORY_NAME),
            &mut targets,
        )?;
    }
    Ok(targets.into_iter().collect())
}

fn is_trusted_cleanup_data_root(path: &Path) -> bool {
    read_data_root_marker(path).is_ok_and(|marker| {
        marker.schema_version == STORAGE_SCHEMA_VERSION
            && marker.identifier == "com.minemail.desktop"
            && marker.state == DataRootState::Ready
    })
}

fn collect_webview_cache_directories(
    path: &Path,
    targets: &mut BTreeSet<PathBuf>,
) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if WEBVIEW_CACHE_DIRECTORY_NAMES
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate))
        {
            targets.insert(entry.path());
        } else {
            collect_webview_cache_directories(&entry.path(), targets)?;
        }
    }
    Ok(())
}

fn process_pending_migration(bootstrap_dir: &Path) -> Option<StorageMigrationNoticeDto> {
    let result_path = bootstrap_dir.join(STORAGE_MIGRATION_RESULT_FILE);
    let previous_result = if result_path.exists() {
        match read_json::<StoredMigrationResult>(&result_path) {
            Ok(result) => {
                diagnostics::limited_recovery(
                    "storage_migration_result_read_failed",
                    "storage_migration_result_read_recovered",
                    "process_storage_migration",
                    None,
                );
                Some(result)
            }
            Err(_) => {
                diagnostics::limited_failure(
                    "storage_migration_result_read_failed",
                    "process_storage_migration",
                    None,
                    DiagnosticErrorKind::Io,
                );
                None
            }
        }
    } else {
        None
    };
    if previous_result.is_some() {
        match fs::remove_file(&result_path) {
            Ok(()) => diagnostics::limited_recovery(
                "storage_migration_result_cleanup_failed",
                "storage_migration_result_cleanup_recovered",
                "process_storage_migration",
                None,
            ),
            Err(_) => diagnostics::limited_failure(
                "storage_migration_result_cleanup_failed",
                "process_storage_migration",
                None,
                DiagnosticErrorKind::Io,
            ),
        }
    }

    let request_path = bootstrap_dir.join(STORAGE_MIGRATION_FILE);
    if !request_path.exists() {
        return previous_result.map(Into::into);
    }

    let outcome = read_json::<StorageMigrationRequest>(&request_path)
        .map_err(|_| "迁移任务损坏，已继续使用原数据目录。".to_owned())
        .and_then(|request| execute_migration(bootstrap_dir, &request));

    let result = match outcome {
        Ok(outcome) => StoredMigrationResult {
            status: MigrationResultStatus::Completed,
            moved_bytes: outcome.moved_bytes,
            message: if outcome.cleanup_warning {
                "本地数据已迁移；部分旧文件仍被系统占用，可在确认邮件正常后手动清理。".to_owned()
            } else {
                "本地数据已迁移到新目录。".to_owned()
            },
        },
        Err(message) => StoredMigrationResult {
            status: MigrationResultStatus::Failed,
            moved_bytes: 0,
            message,
        },
    };
    match write_json_atomically(&result_path, &result) {
        Ok(()) => diagnostics::limited_recovery(
            "storage_migration_result_write_failed",
            "storage_migration_result_write_recovered",
            "process_storage_migration",
            None,
        ),
        Err(_) => diagnostics::limited_failure(
            "storage_migration_result_write_failed",
            "process_storage_migration",
            None,
            DiagnosticErrorKind::Io,
        ),
    }
    match fs::remove_file(request_path) {
        Ok(()) => diagnostics::limited_recovery(
            "storage_migration_request_cleanup_failed",
            "storage_migration_request_cleanup_recovered",
            "process_storage_migration",
            None,
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => diagnostics::limited_failure(
            "storage_migration_request_cleanup_failed",
            "process_storage_migration",
            None,
            DiagnosticErrorKind::Io,
        ),
    }
    Some(result.into())
}

impl From<StoredMigrationResult> for StorageMigrationNoticeDto {
    fn from(value: StoredMigrationResult) -> Self {
        Self {
            status: value.status,
            moved_bytes: value.moved_bytes,
            message: value.message,
        }
    }
}

fn execute_migration(
    bootstrap_dir: &Path,
    request: &StorageMigrationRequest,
) -> Result<MigrationExecution, String> {
    if request.schema_version != STORAGE_SCHEMA_VERSION {
        return Err("迁移任务版本不受支持，已继续使用原数据目录。".to_owned());
    }
    let source = fs::canonicalize(&request.source).map_err(|_| "原数据目录不可用，迁移未开始。")?;
    let target = fs::canonicalize(&request.target)
        .or_else(|_| {
            fs::create_dir_all(&request.target)?;
            fs::canonicalize(&request.target)
        })
        .map_err(|_| "新数据目录不可用，迁移未开始。")?;

    if source == target
        || target.starts_with(&source)
        || source.starts_with(&target)
        || is_network_path(&target)
        || is_protected_install_location(&target)
    {
        return Err("迁移目录不安全，已继续使用原数据目录。".to_owned());
    }

    let locator_path = bootstrap_dir.join(STORAGE_LOCATOR_FILE);
    let current_root = read_json::<StorageLocator>(&locator_path)
        .ok()
        .map(|locator| locator.data_root)
        .unwrap_or_else(|| bootstrap_dir.to_path_buf());
    let current_root = fs::canonicalize(&current_root).unwrap_or(current_root);

    if current_root == target {
        validate_ready_target(&target)?;
        let moved_bytes =
            managed_tree_size(&target).map_err(|_| "迁移后的数据大小无法确认。".to_owned())?;
        return Ok(MigrationExecution {
            moved_bytes,
            cleanup_warning: cleanup_migrated_source_observed(&source, bootstrap_dir),
        });
    }
    if current_root != source {
        return Err("当前数据目录与迁移任务不一致，迁移已取消。".to_owned());
    }

    prepare_target_for_copy(&target)?;
    write_data_root_marker(&target, DataRootState::Migrating)
        .map_err(|_| "无法初始化新数据目录。".to_owned())?;

    let copy_result = (|| {
        copy_managed_tree(&source, &target)?;
        validate_copied_tree(&source, &target)?;
        validate_sqlite_databases(&target)?;
        write_data_root_marker(&target, DataRootState::Ready)
            .map_err(|_| "无法完成新数据目录的状态写入。".to_owned())?;
        let moved_bytes =
            managed_tree_size(&target).map_err(|_| "迁移后的数据大小无法确认。".to_owned())?;
        let locator = StorageLocator {
            schema_version: STORAGE_SCHEMA_VERSION,
            data_root: target.clone(),
        };
        write_json_atomically(&locator_path, &locator)
            .map_err(|_| "数据已复制，但无法切换到新目录。".to_owned())?;
        Ok(moved_bytes)
    })();

    match copy_result {
        Ok(moved_bytes) => Ok(MigrationExecution {
            moved_bytes,
            cleanup_warning: cleanup_migrated_source_observed(&source, bootstrap_dir),
        }),
        Err(error) => {
            match remove_migration_target(&target) {
                Ok(()) => diagnostics::limited_recovery(
                    "storage_migration_rollback_failed",
                    "storage_migration_rollback_recovered",
                    "rollback_storage_migration",
                    None,
                ),
                Err(_) => diagnostics::limited_failure(
                    "storage_migration_rollback_failed",
                    "rollback_storage_migration",
                    None,
                    DiagnosticErrorKind::Io,
                ),
            }
            Err(error)
        }
    }
}

fn cleanup_migrated_source_observed(source: &Path, bootstrap_dir: &Path) -> bool {
    match cleanup_migrated_source(source, bootstrap_dir) {
        Ok(()) => {
            diagnostics::limited_recovery(
                "storage_migration_source_cleanup_failed",
                "storage_migration_source_cleanup_recovered",
                "cleanup_storage_migration_source",
                None,
            );
            false
        }
        Err(_) => {
            diagnostics::limited_failure(
                "storage_migration_source_cleanup_failed",
                "cleanup_storage_migration_source",
                None,
                DiagnosticErrorKind::Io,
            );
            true
        }
    }
}

fn prepare_target_for_copy(target: &Path) -> Result<(), String> {
    if !target.is_dir() {
        return Err("新数据目录不可用。".to_owned());
    }
    let entries = fs::read_dir(target)
        .map_err(|_| "无法读取新数据目录。".to_owned())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "无法读取新数据目录。".to_owned())?;
    if entries.is_empty() {
        return Ok(());
    }
    if entries
        .iter()
        .any(|entry| entry.file_name() == DATA_ROOT_MARKER_FILE)
        && read_data_root_marker(target)
            .is_ok_and(|marker| marker.identifier == "com.minemail.desktop")
    {
        clear_directory_contents(target).map_err(|_| "无法清理未完成的迁移副本。".to_owned())?;
        return Ok(());
    }
    Err("新数据目录已包含其他文件，迁移已取消。".to_owned())
}

fn validate_ready_target(target: &Path) -> Result<(), String> {
    let marker = read_data_root_marker(target)
        .map_err(|_| "迁移副本缺少 Mine Mail 数据标记。".to_owned())?;
    if marker.identifier != "com.minemail.desktop" || marker.state != DataRootState::Ready {
        return Err("迁移副本尚未完成，已继续使用原数据目录。".to_owned());
    }
    validate_sqlite_databases(target)
}

fn copy_managed_tree(source: &Path, target: &Path) -> Result<(), String> {
    for entry in managed_entries(source).map_err(|_| "无法读取原数据目录。".to_owned())? {
        let entry = entry.map_err(|_| "无法读取原数据目录。".to_owned())?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        copy_entry(&source_path, &target_path)?;
    }
    Ok(())
}

fn copy_entry(source: &Path, target: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(|_| "无法读取待迁移文件。".to_owned())?;
    if metadata.file_type().is_symlink() {
        return Err("数据目录包含不受支持的符号链接，迁移已取消。".to_owned());
    }
    if metadata.is_dir() {
        fs::create_dir_all(target).map_err(|_| "无法创建迁移目录。".to_owned())?;
        for entry in fs::read_dir(source).map_err(|_| "无法读取待迁移目录。".to_owned())?
        {
            let entry = entry.map_err(|_| "无法读取待迁移目录。".to_owned())?;
            copy_entry(&entry.path(), &target.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err("数据目录包含不受支持的文件类型，迁移已取消。".to_owned());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|_| "无法创建迁移目录。".to_owned())?;
    }
    let copied = fs::copy(source, target).map_err(|_| "复制本地数据失败。".to_owned())?;
    if copied != metadata.len() {
        return Err("复制后的文件大小不一致，迁移已取消。".to_owned());
    }
    fs::File::options()
        .write(true)
        .open(target)
        .and_then(|file| file.sync_all())
        .map_err(|_| "无法将迁移数据写入磁盘。".to_owned())
}

fn validate_copied_tree(source: &Path, target: &Path) -> Result<(), String> {
    let source_files =
        collect_managed_file_sizes(source).map_err(|_| "无法校验原数据目录。".to_owned())?;
    let target_files =
        collect_managed_file_sizes(target).map_err(|_| "无法校验迁移副本。".to_owned())?;
    if source_files != target_files {
        return Err("迁移副本与原数据不一致，已继续使用原数据目录。".to_owned());
    }
    Ok(())
}

fn validate_sqlite_databases(target: &Path) -> Result<(), String> {
    let entries = fs::read_dir(target).map_err(|_| "无法校验迁移副本。".to_owned())?;
    for entry in entries {
        let entry = entry.map_err(|_| "无法校验迁移副本。".to_owned())?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("sqlite3") {
            continue;
        }
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| "迁移后的数据库无法打开，已继续使用原数据目录。".to_owned())?;
        let result: String = connection
            .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
            .map_err(|_| "迁移后的数据库校验失败，已继续使用原数据目录。".to_owned())?;
        if result != "ok" {
            return Err("迁移后的数据库校验失败，已继续使用原数据目录。".to_owned());
        }
    }
    Ok(())
}

fn cleanup_migrated_source(source: &Path, bootstrap_dir: &Path) -> Result<(), String> {
    for entry in managed_entries(source).map_err(|_| "无法清理原数据目录。".to_owned())? {
        let entry = entry.map_err(|_| "无法清理原数据目录。".to_owned())?;
        remove_entry(&entry.path()).map_err(|_| {
            "新目录已启用，但部分旧数据未能删除；可以在确认邮件正常后手动清理。".to_owned()
        })?;
    }
    if source != bootstrap_dir {
        let marker = source.join(DATA_ROOT_MARKER_FILE);
        match fs::remove_file(marker) {
            Ok(()) => diagnostics::limited_recovery(
                "storage_migration_marker_cleanup_failed",
                "storage_migration_marker_cleanup_recovered",
                "cleanup_storage_migration_source",
                None,
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => diagnostics::limited_failure(
                "storage_migration_marker_cleanup_failed",
                "cleanup_storage_migration_source",
                None,
                DiagnosticErrorKind::Io,
            ),
        }
        match fs::read_dir(source) {
            Ok(mut entries) => {
                if entries.next().is_none() {
                    match fs::remove_dir(source) {
                        Ok(()) => diagnostics::limited_recovery(
                            "storage_migration_directory_cleanup_failed",
                            "storage_migration_directory_cleanup_recovered",
                            "cleanup_storage_migration_source",
                            None,
                        ),
                        Err(_) => diagnostics::limited_failure(
                            "storage_migration_directory_cleanup_failed",
                            "cleanup_storage_migration_source",
                            None,
                            DiagnosticErrorKind::Io,
                        ),
                    }
                }
            }
            Err(_) => diagnostics::limited_failure(
                "storage_migration_directory_inspection_failed",
                "cleanup_storage_migration_source",
                None,
                DiagnosticErrorKind::Io,
            ),
        }
    }
    Ok(())
}

fn remove_migration_target(target: &Path) -> io::Result<()> {
    let marker = read_data_root_marker(target)?;
    if marker.identifier != "com.minemail.desktop" {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "untrusted migration target",
        ));
    }
    clear_directory_contents(target)?;
    fs::remove_dir(target)
}

fn clear_directory_contents(path: &Path) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        remove_entry(&entry?.path())?;
    }
    Ok(())
}

fn remove_entry(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn managed_entries(path: &Path) -> io::Result<impl Iterator<Item = io::Result<fs::DirEntry>>> {
    Ok(fs::read_dir(path)?.filter(|entry| {
        entry.as_ref().is_ok_and(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !is_bootstrap_entry(&name)
                && name != "logs"
                && name != DATA_ROOT_MARKER_FILE
                && !name.starts_with(".mine-mail-write-test-")
        })
    }))
}

fn is_bootstrap_entry(name: &str) -> bool {
    matches!(
        name,
        STORAGE_LOCATOR_FILE
            | STORAGE_MIGRATION_FILE
            | STORAGE_MIGRATION_RESULT_FILE
            | WEBVIEW_CACHE_CLEANUP_FILE
    ) || name.starts_with("storage-location.json.tmp-")
        || name.starts_with("storage-migration.json.tmp-")
        || name.starts_with("storage-migration-result.json.tmp-")
        || name.starts_with("webview-cache-cleanup.json.tmp-")
}

fn managed_tree_size(path: &Path) -> io::Result<u64> {
    let mut total = 0u64;
    for entry in managed_entries(path)? {
        total = total.saturating_add(entry_size(&entry?.path())?);
    }
    Ok(total)
}

fn collect_managed_file_sizes(path: &Path) -> io::Result<BTreeMap<PathBuf, u64>> {
    let mut files = BTreeMap::new();
    for entry in managed_entries(path)? {
        let entry = entry?;
        collect_file_sizes(&entry.path(), Path::new(&entry.file_name()), &mut files)?;
    }
    Ok(files)
}

fn collect_file_sizes(
    path: &Path,
    relative: &Path,
    files: &mut BTreeMap<PathBuf, u64>,
) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "symbolic links are unsupported",
        ));
    }
    if metadata.is_file() {
        files.insert(relative.to_path_buf(), metadata.len());
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            collect_file_sizes(&entry.path(), &relative.join(entry.file_name()), files)?;
        }
    }
    Ok(())
}

#[derive(Default)]
struct StorageSizes {
    mail: u64,
    webview: u64,
    user_assets: u64,
    cache: u64,
    logs: u64,
    other: u64,
}

fn measure_storage_root(root: &Path) -> io::Result<StorageSizes> {
    let mut sizes = StorageSizes::default();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_bootstrap_entry(&name) || name == DATA_ROOT_MARKER_FILE {
            continue;
        }
        let bytes = entry_size(&entry.path())?;
        if name == WEBVIEW_DATA_DIRECTORY_NAME {
            sizes.webview = sizes.webview.saturating_add(bytes);
        } else if name == "user-assets" {
            sizes.user_assets = sizes.user_assets.saturating_add(bytes);
        } else if name == "cache" {
            sizes.cache = sizes.cache.saturating_add(bytes);
        } else if name == "logs" {
            sizes.logs = sizes.logs.saturating_add(bytes);
        } else if name == "account.json"
            || name.ends_with(".sqlite3")
            || name.ends_with(".sqlite3-wal")
            || name.ends_with(".sqlite3-shm")
        {
            sizes.mail = sizes.mail.saturating_add(bytes);
        } else {
            sizes.other = sizes.other.saturating_add(bytes);
        }
    }
    Ok(sizes)
}

fn directory_size(path: &Path) -> io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    entry_size(path)
}

fn entry_size(path: &Path) -> io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(0);
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    let mut total = 0u64;
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            total = total.saturating_add(entry_size(&entry?.path())?);
        }
    }
    Ok(total)
}

fn probe_directory_write(path: &Path) -> io::Result<()> {
    let probe = path.join(format!(".mine-mail-write-test-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&probe)?;
        file.write_all(b"mine-mail")?;
        file.sync_all()
    })();
    match fs::remove_file(probe) {
        Ok(()) => diagnostics::limited_recovery(
            "storage_write_probe_cleanup_failed",
            "storage_write_probe_cleanup_recovered",
            "probe_storage_write",
            None,
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => diagnostics::limited_failure(
            "storage_write_probe_cleanup_failed",
            "probe_storage_write",
            None,
            DiagnosticErrorKind::Io,
        ),
    }
    result
}

fn write_data_root_marker(path: &Path, state: DataRootState) -> io::Result<()> {
    let marker = DataRootMarker {
        schema_version: STORAGE_SCHEMA_VERSION,
        identifier: "com.minemail.desktop".to_owned(),
        state,
    };
    write_json_atomically(&path.join(DATA_ROOT_MARKER_FILE), &marker)
}

fn read_data_root_marker(path: &Path) -> io::Result<DataRootMarker> {
    read_json(&path.join(DATA_ROOT_MARKER_FILE))
}

fn consume_app_update_relaunch(bootstrap_dir: &Path, current_version: &str) -> bool {
    let path = bootstrap_dir.join(APP_UPDATE_RELAUNCH_FILE);
    let request = match read_json::<AppUpdateRelaunchRequest>(&path) {
        Ok(request) => request,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return false,
        Err(_) => {
            diagnostics::warn(
                "app_update_relaunch_marker_read_failed",
                DiagnosticFields::default()
                    .operation("app_update_relaunch")
                    .error(DiagnosticErrorKind::Io),
            );
            remove_app_update_relaunch_marker(&path);
            return false;
        }
    };

    remove_app_update_relaunch_marker(&path);
    let requested = request.schema_version == STORAGE_SCHEMA_VERSION
        && request.expected_version == current_version;
    diagnostics::info(
        "app_update_relaunch_marker_consumed",
        DiagnosticFields::default()
            .operation("app_update_relaunch")
            .outcome(if requested { "foreground" } else { "ignored" }),
    );
    requested
}

fn rollback_app_update_relaunch(bootstrap_dir: &Path, expected_version: &str) {
    let path = bootstrap_dir.join(APP_UPDATE_RELAUNCH_FILE);
    let request = match read_json::<AppUpdateRelaunchRequest>(&path) {
        Ok(request) => request,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(_) => {
            diagnostics::warn(
                "app_update_relaunch_marker_rollback_read_failed",
                DiagnosticFields::default()
                    .operation("app_update_relaunch")
                    .error(DiagnosticErrorKind::Io),
            );
            return;
        }
    };

    if request.schema_version != STORAGE_SCHEMA_VERSION
        || request.expected_version != expected_version
    {
        return;
    }

    if remove_app_update_relaunch_marker(&path) {
        diagnostics::info(
            "app_update_relaunch_marker_rolled_back",
            DiagnosticFields::default()
                .operation("app_update_relaunch")
                .outcome("install_failed"),
        );
    }
}

fn remove_app_update_relaunch_marker(path: &Path) -> bool {
    match fs::remove_file(path) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(_) => {
            diagnostics::warn(
                "app_update_relaunch_marker_cleanup_failed",
                DiagnosticFields::default()
                    .operation("app_update_relaunch")
                    .error(DiagnosticErrorKind::Io),
            );
            false
        }
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<T> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn write_json_atomically(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        "{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("storage"),
        Uuid::new_v4()
    ));
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    let result = replace_file(&temporary, path);
    if result.is_err() {
        match fs::remove_file(&temporary) {
            Ok(()) => diagnostics::limited_recovery(
                "storage_atomic_write_cleanup_failed",
                "storage_atomic_write_cleanup_recovered",
                "write_storage_json",
                None,
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => diagnostics::limited_failure(
                "storage_atomic_write_cleanup_failed",
                "write_storage_json",
                None,
                DiagnosticErrorKind::Io,
            ),
        }
    }
    result
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: both paths are owned, NUL-terminated UTF-16 buffers that remain
    // alive for the duration of the Win32 call.
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    fs::rename(source, target)
}

fn has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| component == Component::ParentDir)
}

fn is_network_path(path: &Path) -> bool {
    let text = path.as_os_str().to_string_lossy();
    #[cfg(target_os = "windows")]
    {
        if text.starts_with(r"\\?\UNC\") {
            return true;
        }
        if text.starts_with(r"\\?\") || text.starts_with(r"\\.\") {
            return is_remote_windows_drive(path);
        }
        return text.starts_with(r"\\") || text.starts_with("//") || is_remote_windows_drive(path);
    }
    #[cfg(not(target_os = "windows"))]
    {
        text.starts_with(r"\\") || text.starts_with("//")
    }
}

#[cfg(target_os = "windows")]
fn is_remote_windows_drive(path: &Path) -> bool {
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return false;
    };
    let letter = match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
        Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _) => return true,
        _ => return false,
    };
    let root = format!("{}:\\", char::from(letter))
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: root is a valid NUL-terminated UTF-16 drive-root string.
    unsafe { GetDriveTypeW(root.as_ptr()) == 4 }
}

#[cfg(target_os = "windows")]
fn is_protected_install_location(path: &Path) -> bool {
    for variable in [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "SystemRoot",
    ] {
        if let Some(protected) = env::var_os(variable) {
            if windows_path_starts_with(path, Path::new(&protected)) {
                return true;
            }
        }
    }
    false
}

#[cfg(not(target_os = "windows"))]
fn is_protected_install_location(_path: &Path) -> bool {
    false
}

#[cfg(target_os = "windows")]
fn windows_path_starts_with(path: &Path, prefix: &Path) -> bool {
    let path = path
        .as_os_str()
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    let prefix = prefix
        .as_os_str()
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    path == prefix || path.starts_with(&format!("{prefix}\\"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn app_update_relaunch_marker_is_version_bound_and_one_shot() {
        let directory = tempdir().expect("temporary directory");
        let bootstrap = directory.path();
        let marker = bootstrap.join(APP_UPDATE_RELAUNCH_FILE);

        write_json_atomically(
            &marker,
            &AppUpdateRelaunchRequest {
                schema_version: STORAGE_SCHEMA_VERSION,
                expected_version: "1.4.0".to_owned(),
            },
        )
        .expect("foreground relaunch marker");

        assert!(consume_app_update_relaunch(bootstrap, "1.4.0"));
        assert!(!marker.exists());
        assert!(!consume_app_update_relaunch(bootstrap, "1.4.0"));

        write_json_atomically(
            &marker,
            &AppUpdateRelaunchRequest {
                schema_version: STORAGE_SCHEMA_VERSION,
                expected_version: "1.5.0".to_owned(),
            },
        )
        .expect("mismatched relaunch marker");

        assert!(!consume_app_update_relaunch(bootstrap, "1.4.0"));
        assert!(!marker.exists());
    }

    #[test]
    fn corrupt_app_update_relaunch_marker_is_removed_without_showing() {
        let directory = tempdir().expect("temporary directory");
        let marker = directory.path().join(APP_UPDATE_RELAUNCH_FILE);
        fs::write(&marker, b"not-json").expect("corrupt marker");

        assert!(!consume_app_update_relaunch(directory.path(), "1.4.0"));
        assert!(!marker.exists());
    }

    #[test]
    fn failed_update_rolls_back_only_its_matching_relaunch_marker() {
        let directory = tempdir().expect("temporary directory");
        let marker = directory.path().join(APP_UPDATE_RELAUNCH_FILE);
        write_json_atomically(
            &marker,
            &AppUpdateRelaunchRequest {
                schema_version: STORAGE_SCHEMA_VERSION,
                expected_version: "1.4.0".to_owned(),
            },
        )
        .expect("foreground relaunch marker");

        rollback_app_update_relaunch(directory.path(), "1.5.0");
        assert!(marker.exists());

        rollback_app_update_relaunch(directory.path(), "1.4.0");
        assert!(!marker.exists());
    }

    #[test]
    fn new_windows_install_prefers_a_writable_sibling_data_directory() {
        let directory = tempdir().expect("temporary directory");
        let bootstrap = directory.path().join("local");
        let executable = directory.path().join("installed");
        fs::create_dir_all(&bootstrap).expect("bootstrap");
        fs::create_dir_all(&executable).expect("executable");

        let selected =
            resolve_data_root(&bootstrap, Some(&executable), true).expect("resolved data root");

        assert_eq!(selected, executable.join("Data"));
        assert!(selected.join(DATA_ROOT_MARKER_FILE).is_file());
    }

    #[test]
    fn existing_local_data_is_never_silently_replaced_by_install_data() {
        let directory = tempdir().expect("temporary directory");
        let bootstrap = directory.path().join("local");
        let executable = directory.path().join("installed");
        fs::create_dir_all(&bootstrap).expect("bootstrap");
        fs::create_dir_all(&executable).expect("executable");
        fs::write(bootstrap.join("account.json"), b"{}").expect("legacy account");

        let selected =
            resolve_data_root(&bootstrap, Some(&executable), true).expect("resolved data root");

        assert_eq!(selected, bootstrap);
        assert!(!executable.join("Data").exists());
    }

    #[test]
    fn occupied_install_data_path_falls_back_to_local_app_data() {
        let directory = tempdir().expect("temporary directory");
        let bootstrap = directory.path().join("local");
        let executable = directory.path().join("installed");
        fs::create_dir_all(&bootstrap).expect("bootstrap");
        fs::create_dir_all(&executable).expect("executable");
        fs::write(executable.join("Data"), b"occupied").expect("occupied path");

        let selected =
            resolve_data_root(&bootstrap, Some(&executable), true).expect("resolved data root");

        assert_eq!(selected, bootstrap);
    }

    #[test]
    fn migration_copies_validates_switches_and_removes_only_managed_source_data() {
        let directory = tempdir().expect("temporary directory");
        let bootstrap = directory.path().join("local");
        let target = directory.path().join("new-data");
        fs::create_dir_all(bootstrap.join("EBWebView/Default/Cache")).expect("legacy data");
        fs::create_dir_all(bootstrap.join("logs")).expect("logs");
        fs::write(bootstrap.join("account.json"), b"{}").expect("account");
        fs::write(
            bootstrap.join("EBWebView/Default/Cache/item"),
            b"browser cache",
        )
        .expect("cache");
        fs::write(bootstrap.join("logs/mine-mail.log"), b"log").expect("log");
        let database =
            Connection::open(bootstrap.join("desktop-runtime.sqlite3")).expect("database");
        database
            .execute_batch(
                "CREATE TABLE settings (id INTEGER PRIMARY KEY); INSERT INTO settings VALUES (1);",
            )
            .expect("schema");
        drop(database);
        write_data_root_marker(&bootstrap, DataRootState::Ready).expect("source marker");
        write_json_atomically(
            &bootstrap.join(STORAGE_LOCATOR_FILE),
            &StorageLocator {
                schema_version: STORAGE_SCHEMA_VERSION,
                data_root: bootstrap.clone(),
            },
        )
        .expect("locator");
        fs::create_dir_all(&target).expect("target");

        let outcome = execute_migration(
            &bootstrap,
            &StorageMigrationRequest {
                schema_version: STORAGE_SCHEMA_VERSION,
                source: bootstrap.clone(),
                target: target.clone(),
            },
        )
        .expect("migration");

        assert!(outcome.moved_bytes > 0);
        assert!(!outcome.cleanup_warning);
        assert!(target.join("account.json").is_file());
        assert!(target.join("desktop-runtime.sqlite3").is_file());
        assert!(target.join("EBWebView/Default/Cache/item").is_file());
        assert!(!bootstrap.join("account.json").exists());
        assert!(bootstrap.join("logs/mine-mail.log").is_file());
        let locator: StorageLocator =
            read_json(&bootstrap.join(STORAGE_LOCATOR_FILE)).expect("updated locator");
        assert_eq!(
            locator.data_root,
            fs::canonicalize(target).expect("target path")
        );
    }

    #[test]
    fn migration_target_must_be_empty_and_independent() {
        let directory = tempdir().expect("temporary directory");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        let bootstrap = directory.path().join("bootstrap");
        fs::create_dir_all(&source).expect("source");
        fs::create_dir_all(&target).expect("target");
        fs::create_dir_all(&bootstrap).expect("bootstrap");
        fs::write(target.join("keep.txt"), b"keep").expect("existing file");

        let error = validate_and_prepare_migration_target(
            &target.to_string_lossy(),
            &source,
            &bootstrap,
            None,
        )
        .expect_err("non-empty target");

        assert!(error.contains("空文件夹"));
        assert!(target.join("keep.txt").is_file());
    }

    #[test]
    fn cancelling_a_pending_migration_is_idempotent() {
        let directory = tempdir().expect("temporary directory");
        let bootstrap = directory.path().join("bootstrap");
        fs::create_dir_all(&bootstrap).expect("bootstrap");
        fs::write(bootstrap.join(STORAGE_MIGRATION_FILE), b"pending").expect("request");
        let runtime = StorageRuntime {
            bootstrap_dir: bootstrap.clone(),
            data_root: bootstrap.clone(),
            install_data_root: None,
            location_kind: StorageLocationKind::LocalAppData,
            migration_notice: None,
            cache_cleanup_notice: None,
            migration_gate: Arc::new(Mutex::new(())),
        };

        runtime
            .cancel_pending_migration()
            .expect("first cancellation");
        runtime
            .cancel_pending_migration()
            .expect("second cancellation");

        assert!(!bootstrap.join(STORAGE_MIGRATION_FILE).exists());
    }

    #[test]
    fn webview_cache_cleanup_removes_only_regenerable_directories() {
        let directory = tempdir().expect("temporary directory");
        let bootstrap = directory.path().join("bootstrap");
        let data_root = directory.path().join("data");
        let legacy_profile = bootstrap.join("EBWebView/Default");
        let active_profile = data_root.join("EBWebView/EBWebView/Default");
        fs::create_dir_all(&data_root).expect("data root");
        write_data_root_marker(&data_root, DataRootState::Ready).expect("data root marker");

        for path in [
            legacy_profile.join("Cache"),
            legacy_profile.join("Code Cache/js"),
            active_profile.join("GPUCache"),
            data_root.join("EBWebView/GrShaderCache"),
            active_profile.join("Local Storage/leveldb"),
            active_profile.join("IndexedDB"),
        ] {
            fs::create_dir_all(path).expect("cache fixture directory");
        }
        fs::write(legacy_profile.join("Cache/http.bin"), b"http-cache").expect("http cache");
        fs::write(legacy_profile.join("Code Cache/js/module.bin"), b"js-cache")
            .expect("code cache");
        fs::write(active_profile.join("GPUCache/gpu.bin"), b"gpu-cache").expect("gpu cache");
        fs::write(
            data_root.join("EBWebView/GrShaderCache/shader.bin"),
            b"shader-cache",
        )
        .expect("shader cache");
        fs::write(
            active_profile.join("Local Storage/leveldb/state.bin"),
            b"persistent-state",
        )
        .expect("local storage");
        fs::write(active_profile.join("IndexedDB/mail.bin"), b"indexed-db").expect("indexed db");

        let reclaimable =
            reclaimable_webview_cache_bytes(&data_root, &bootstrap).expect("cache size");
        let execution = cleanup_webview_caches(&data_root, &bootstrap).expect("cache cleanup");

        assert_eq!(execution.failed_directories, 0);
        assert_eq!(execution.removed_bytes, reclaimable);
        assert!(reclaimable > 0);
        assert!(!legacy_profile.join("Cache").exists());
        assert!(!legacy_profile.join("Code Cache").exists());
        assert!(!active_profile.join("GPUCache").exists());
        assert!(!data_root.join("EBWebView/GrShaderCache").exists());
        assert!(
            active_profile
                .join("Local Storage/leveldb/state.bin")
                .is_file()
        );
        assert!(active_profile.join("IndexedDB/mail.bin").is_file());
    }

    #[test]
    fn webview_cache_cleanup_retries_transient_lock_failures() {
        let directory = tempdir().expect("temporary directory");
        let cache = directory.path().join("Cache");
        fs::create_dir_all(&cache).expect("cache directory");
        fs::write(cache.join("item.bin"), b"cache").expect("cache item");
        let mut attempts = 0;
        let mut waits = 0;

        let execution = cleanup_webview_cache_targets(
            vec![(cache.clone(), 5)],
            0,
            |target| {
                attempts += 1;
                if attempts == 1 {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "cache is still locked",
                    ))
                } else {
                    fs::remove_dir_all(target)
                }
            },
            |_| waits += 1,
        );

        assert_eq!(attempts, 2);
        assert_eq!(waits, 1);
        assert_eq!(execution.failed_directories, 0);
        assert_eq!(execution.removed_bytes, 5);
        assert!(!cache.exists());
    }

    #[test]
    fn webview_cache_cleanup_reports_persistently_locked_directories() {
        let mut attempts = 0;
        let mut waits = 0;
        let execution = cleanup_webview_cache_targets(
            vec![(PathBuf::from("locked-cache"), 9)],
            0,
            |_| {
                attempts += 1;
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "cache remains locked",
                ))
            },
            |_| waits += 1,
        );

        assert_eq!(attempts, WEBVIEW_CACHE_CLEANUP_RETRY_ATTEMPTS);
        assert_eq!(waits, WEBVIEW_CACHE_CLEANUP_RETRY_ATTEMPTS - 1);
        assert_eq!(execution.failed_directories, 1);
        assert_eq!(execution.removed_bytes, 0);
    }

    #[test]
    fn pending_webview_cache_cleanup_is_versioned_and_one_shot() {
        let directory = tempdir().expect("temporary directory");
        let bootstrap = directory.path().join("bootstrap");
        let cache = bootstrap.join("EBWebView/Default/Cache");
        fs::create_dir_all(&cache).expect("cache directory");
        fs::write(cache.join("item.bin"), b"cache").expect("cache item");
        write_json_atomically(
            &bootstrap.join(WEBVIEW_CACHE_CLEANUP_FILE),
            &WebviewCacheCleanupRequest {
                schema_version: STORAGE_SCHEMA_VERSION,
            },
        )
        .expect("cleanup marker");

        let notice =
            process_pending_webview_cache_cleanup(&bootstrap, &bootstrap).expect("cleanup notice");

        assert!(matches!(notice.status, MigrationResultStatus::Completed));
        assert_eq!(notice.removed_bytes, 5);
        assert!(!cache.exists());
        assert!(!bootstrap.join(WEBVIEW_CACHE_CLEANUP_FILE).exists());
        assert!(process_pending_webview_cache_cleanup(&bootstrap, &bootstrap).is_none());
    }

    #[test]
    fn webview_cache_cleanup_ignores_an_untrusted_configured_root() {
        let directory = tempdir().expect("temporary directory");
        let bootstrap = directory.path().join("bootstrap");
        let untrusted = directory.path().join("untrusted");
        let cache = untrusted.join("EBWebView/Default/Cache");
        fs::create_dir_all(&bootstrap).expect("bootstrap");
        fs::create_dir_all(&cache).expect("untrusted cache");
        fs::write(cache.join("keep.bin"), b"keep").expect("untrusted cache item");

        let execution = cleanup_webview_caches(&untrusted, &bootstrap).expect("safe cleanup");

        assert_eq!(execution.removed_bytes, 0);
        assert!(cache.join("keep.bin").is_file());
    }
}
