use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
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

const STORAGE_SCHEMA_VERSION: u8 = 1;
const STORAGE_LOCATOR_FILE: &str = "storage-location.json";
const STORAGE_MIGRATION_FILE: &str = "storage-migration.json";
const STORAGE_MIGRATION_RESULT_FILE: &str = "storage-migration-result.json";
const DATA_ROOT_MARKER_FILE: &str = ".mine-mail-data.json";
const INSTALL_DATA_DIRECTORY_NAME: &str = "Data";
const WEBVIEW_DATA_DIRECTORY_NAME: &str = "EBWebView";

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
struct StoredMigrationResult {
    status: MigrationResultStatus,
    moved_bytes: u64,
    message: String,
}

struct MigrationExecution {
    moved_bytes: u64,
    cleanup_warning: bool,
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
pub(crate) struct StorageStatusDto {
    data_path: String,
    location_kind: StorageLocationKind,
    available: bool,
    total_bytes: u64,
    categories: Vec<StorageCategoryDto>,
    migration_notice: Option<StorageMigrationNoticeDto>,
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
    target_path: String,
    total_bytes: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct StorageRuntime {
    bootstrap_dir: PathBuf,
    data_root: PathBuf,
    install_data_root: Option<PathBuf>,
    location_kind: StorageLocationKind,
    migration_notice: Option<StorageMigrationNoticeDto>,
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
                },
                runtime_data_root: degraded,
                startup_error: Some(
                    "The application data directory is unavailable; local mail is disabled for this session."
                        .to_owned(),
                ),
            };
        }

        let executable_dir = app.path().executable_dir().ok();
        let install_data_root = executable_dir
            .as_deref()
            .map(|directory| directory.join(INSTALL_DATA_DIRECTORY_NAME));
        let migration_notice = process_pending_migration(&bootstrap_dir);

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
            sizes.logs = directory_size(&self.bootstrap_dir.join("logs")).unwrap_or(0);
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

        Ok(StorageStatusDto {
            data_path: self.data_root.to_string_lossy().into_owned(),
            location_kind: self.location_kind,
            available,
            total_bytes,
            categories,
            migration_notice: self.migration_notice.clone(),
        })
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
        let _ = fs::remove_file(self.bootstrap_dir.join(STORAGE_MIGRATION_RESULT_FILE));

        Ok(PreparedStorageMigrationDto {
            target_path: target.to_string_lossy().into_owned(),
            total_bytes,
        })
    }

    pub(crate) fn cancel_pending_migration(&self) -> Result<(), String> {
        match fs::remove_file(self.bootstrap_dir.join(STORAGE_MIGRATION_FILE)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("无法撤销待执行的数据迁移任务。".to_owned()),
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

fn process_pending_migration(bootstrap_dir: &Path) -> Option<StorageMigrationNoticeDto> {
    let result_path = bootstrap_dir.join(STORAGE_MIGRATION_RESULT_FILE);
    let previous_result = read_json::<StoredMigrationResult>(&result_path).ok();
    if previous_result.is_some() {
        let _ = fs::remove_file(&result_path);
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
    let _ = write_json_atomically(&result_path, &result);
    let _ = fs::remove_file(request_path);
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
            cleanup_warning: cleanup_migrated_source(&source, bootstrap_dir).is_err(),
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
            cleanup_warning: cleanup_migrated_source(&source, bootstrap_dir).is_err(),
        }),
        Err(error) => {
            let _ = remove_migration_target(&target);
            Err(error)
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
        let _ = fs::remove_file(marker);
        if fs::read_dir(source).is_ok_and(|mut entries| entries.next().is_none()) {
            let _ = fs::remove_dir(source);
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
        STORAGE_LOCATOR_FILE | STORAGE_MIGRATION_FILE | STORAGE_MIGRATION_RESULT_FILE
    ) || name.starts_with("storage-location.json.tmp-")
        || name.starts_with("storage-migration.json.tmp-")
        || name.starts_with("storage-migration-result.json.tmp-")
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
    let _ = fs::remove_file(probe);
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
        let _ = fs::remove_file(&temporary);
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

fn is_protected_install_location(path: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
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
    }
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
        };

        runtime
            .cancel_pending_migration()
            .expect("first cancellation");
        runtime
            .cancel_pending_migration()
            .expect("second cancellation");

        assert!(!bootstrap.join(STORAGE_MIGRATION_FILE).exists());
    }
}
