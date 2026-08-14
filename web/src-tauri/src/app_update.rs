use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use serde::Serialize;
use tauri::{
    AppHandle, State,
    async_runtime::{JoinHandle, Mutex},
    ipc::Channel,
};
use tauri_plugin_updater::{Error as UpdaterError, UpdaterExt};

use crate::{
    diagnostics::{self, Fields as DiagnosticFields},
    storage::StorageRuntime,
};

const MAX_UPDATE_IDENTIFIER_BYTES: usize = 64;

fn safe_update_metadata_error(_error: UpdaterError) -> String {
    "获取更新信息失败，请检查网络后重试。".to_owned()
}

fn safe_update_download_error(error: UpdaterError) -> String {
    if matches!(
        error,
        UpdaterError::Minisign(_) | UpdaterError::Base64(_) | UpdaterError::SignatureUtf8(_)
    ) {
        "更新签名验证失败，当前版本和本地邮件未受影响，请稍后重试。".to_owned()
    } else {
        "更新包下载失败，请检查网络后重试。".to_owned()
    }
}

fn safe_update_install_error(_error: UpdaterError) -> String {
    "更新安装失败，当前版本和本地邮件未受影响，请稍后重试。".to_owned()
}

fn install_with_prepared_relaunch(
    prepare_relaunch: impl FnOnce() -> Result<(), String>,
    install: impl FnOnce() -> Result<(), String>,
    rollback_relaunch: impl FnOnce(),
) -> Result<(), String> {
    prepare_relaunch()?;
    match install() {
        Ok(()) => Ok(()),
        Err(error) => {
            rollback_relaunch();
            Err(error)
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct AppUpdateRuntime {
    active: Arc<Mutex<Option<ActiveUpdate>>>,
    completed_version: Arc<Mutex<Option<String>>>,
}

struct ActiveUpdate {
    session_id: String,
    installing: Arc<AtomicBool>,
    on_progress: Channel<AppUpdateEvent>,
    task: JoinHandle<()>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", content = "data")]
pub(crate) enum AppUpdateEvent {
    Started {
        #[serde(rename = "contentLength")]
        content_length: Option<u64>,
    },
    Progress {
        #[serde(rename = "chunkLength")]
        chunk_length: usize,
    },
    Finished,
    Installing,
    Completed,
    Cancelled,
    Failed {
        message: String,
    },
}

fn validate_session_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_UPDATE_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("更新下载会话标识无效。".to_owned());
    }
    Ok(())
}

fn validate_version(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_UPDATE_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'_'))
    {
        return Err("待安装的更新版本无效。".to_owned());
    }
    Ok(())
}

async fn download_and_install(
    app: AppHandle,
    storage: StorageRuntime,
    expected_version: String,
    installing: Arc<AtomicBool>,
    on_progress: Channel<AppUpdateEvent>,
) -> Result<(), String> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(safe_update_metadata_error)?;
    let update = updater
        .check()
        .await
        .map_err(safe_update_metadata_error)?
        .ok_or_else(|| "已找不到用户确认安装的 Mine Mail 更新。".to_owned())?;

    if update.version != expected_version {
        return Err("可用更新版本已变化，请重新检查并确认更新。".to_owned());
    }

    let progress_channel = on_progress.clone();
    let finish_channel = on_progress.clone();
    let installing_after_download = installing.clone();
    let mut first_chunk = true;
    let bytes = update
        .download(
            move |chunk_length, content_length| {
                if first_chunk {
                    first_chunk = false;
                    let _ = progress_channel.send(AppUpdateEvent::Started { content_length });
                }
                let _ = progress_channel.send(AppUpdateEvent::Progress { chunk_length });
            },
            move || {
                installing_after_download.store(true, Ordering::Release);
                let _ = finish_channel.send(AppUpdateEvent::Finished);
            },
        )
        .await
        .map_err(safe_update_download_error)?;

    installing.store(true, Ordering::Release);
    let _ = on_progress.send(AppUpdateEvent::Installing);
    // The Windows updater launches NSIS and terminates this process from inside
    // `install`, so the foreground marker must be durable before this call.
    install_with_prepared_relaunch(
        || {
            storage.prepare_app_update_relaunch(&expected_version)?;
            diagnostics::info(
                "app_update_relaunch_marker_prepared",
                DiagnosticFields::default()
                    .operation("app_update_relaunch")
                    .outcome("foreground"),
            );
            Ok(())
        },
        || update.install(bytes).map_err(safe_update_install_error),
        || storage.rollback_app_update_relaunch(&expected_version),
    )
}

#[tauri::command]
pub(crate) async fn start_app_update(
    app: AppHandle,
    runtime: State<'_, AppUpdateRuntime>,
    storage: State<'_, StorageRuntime>,
    expected_version: String,
    session_id: String,
    on_progress: Channel<AppUpdateEvent>,
) -> Result<(), String> {
    validate_version(&expected_version)?;
    validate_session_id(&session_id)?;

    let mut active = runtime.active.lock().await;
    if active.is_some() {
        return Err("已有 Mine Mail 更新正在下载。".to_owned());
    }
    *runtime.completed_version.lock().await = None;

    let shared_runtime = runtime.inner().clone();
    let task_session_id = session_id.clone();
    let task_expected_version = expected_version.clone();
    let task_channel = on_progress.clone();
    let installing = Arc::new(AtomicBool::new(false));
    let task_installing = installing.clone();
    let task_storage = storage.inner().clone();
    let task = tauri::async_runtime::spawn(async move {
        let result = download_and_install(
            app,
            task_storage,
            expected_version,
            task_installing,
            task_channel.clone(),
        )
        .await;
        let mut active = shared_runtime.active.lock().await;
        if active.as_ref().map(|task| task.session_id.as_str()) == Some(task_session_id.as_str()) {
            *active = None;
        }
        drop(active);

        match result {
            Ok(()) => {
                *shared_runtime.completed_version.lock().await = Some(task_expected_version);
                let _ = task_channel.send(AppUpdateEvent::Completed);
            }
            Err(message) => {
                let _ = task_channel.send(AppUpdateEvent::Failed { message });
            }
        }
    });

    *active = Some(ActiveUpdate {
        session_id,
        installing,
        on_progress,
        task,
    });
    Ok(())
}

#[tauri::command]
pub(crate) async fn relaunch_after_app_update(
    app: AppHandle,
    runtime: State<'_, AppUpdateRuntime>,
    storage: State<'_, StorageRuntime>,
    expected_version: String,
) -> Result<(), String> {
    validate_version(&expected_version)?;
    let mut completed_version = runtime.completed_version.lock().await;
    if !completed_update_matches(completed_version.as_deref(), &expected_version) {
        return Err("Mine Mail update relaunch is not ready.".to_owned());
    }

    storage.prepare_app_update_relaunch(&expected_version)?;
    *completed_version = None;
    drop(completed_version);

    diagnostics::info(
        "app_update_relaunch_requested",
        DiagnosticFields::default()
            .operation("app_update_relaunch")
            .outcome("foreground"),
    );
    app.request_restart();
    Ok(())
}

fn completed_update_matches(completed_version: Option<&str>, expected_version: &str) -> bool {
    completed_version == Some(expected_version)
}

#[tauri::command]
pub(crate) async fn cancel_app_update(
    runtime: State<'_, AppUpdateRuntime>,
    session_id: String,
) -> Result<bool, String> {
    validate_session_id(&session_id)?;
    let mut active = runtime.active.lock().await;
    let Some(task) = active.as_ref() else {
        return Ok(false);
    };
    if task.session_id != session_id || task.installing.load(Ordering::Acquire) {
        return Ok(false);
    }

    let task = active.take().expect("active update checked above");
    let _ = task.on_progress.send(AppUpdateEvent::Cancelled);
    task.task.abort();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_identifiers_are_bounded_and_path_free() {
        assert!(validate_session_id("update-m42-1").is_ok());
        assert!(validate_session_id("../update").is_err());
        assert!(validate_session_id(&"a".repeat(65)).is_err());

        assert!(validate_version("1.2.3-beta.1+desktop").is_ok());
        assert!(validate_version("https://example.com/update").is_err());
        assert!(validate_version(&"1".repeat(65)).is_err());
    }

    #[test]
    fn foreground_relaunch_requires_the_exact_completed_update() {
        assert!(completed_update_matches(Some("1.4.0"), "1.4.0"));
        assert!(!completed_update_matches(Some("1.3.2"), "1.4.0"));
        assert!(!completed_update_matches(None, "1.4.0"));
    }

    #[test]
    fn terminal_installer_is_preceded_by_the_foreground_relaunch_marker() {
        let steps = std::cell::RefCell::new(Vec::new());

        install_with_prepared_relaunch(
            || {
                steps.borrow_mut().push("prepare");
                Ok(())
            },
            || {
                assert_eq!(steps.borrow().as_slice(), ["prepare"]);
                steps.borrow_mut().push("install");
                Ok(())
            },
            || steps.borrow_mut().push("rollback"),
        )
        .expect("install succeeds");

        assert_eq!(steps.borrow().as_slice(), ["prepare", "install"]);
    }

    #[test]
    fn returned_install_failure_rolls_back_the_relaunch_marker() {
        let steps = std::cell::RefCell::new(Vec::new());

        let error = install_with_prepared_relaunch(
            || {
                steps.borrow_mut().push("prepare");
                Ok(())
            },
            || {
                steps.borrow_mut().push("install");
                Err("install failed".to_owned())
            },
            || steps.borrow_mut().push("rollback"),
        )
        .expect_err("install failure");

        assert_eq!(error, "install failed");
        assert_eq!(
            steps.borrow().as_slice(),
            ["prepare", "install", "rollback"]
        );
    }

    #[test]
    fn progress_events_expose_only_bounded_update_progress() {
        let started = serde_json::to_value(AppUpdateEvent::Started {
            content_length: Some(4096),
        })
        .expect("serialize started event");
        let progress = serde_json::to_value(AppUpdateEvent::Progress { chunk_length: 512 })
            .expect("serialize progress event");

        assert_eq!(started["event"], "Started");
        assert_eq!(started["data"]["contentLength"], 4096);
        assert_eq!(progress["event"], "Progress");
        assert_eq!(progress["data"]["chunkLength"], 512);
    }

    #[test]
    fn update_failure_messages_are_bounded_chinese_stage_descriptions() {
        assert_eq!(
            safe_update_metadata_error(UpdaterError::ReleaseNotFound),
            "获取更新信息失败，请检查网络后重试。"
        );
        assert_eq!(
            safe_update_download_error(UpdaterError::Network(
                "http://cdn.example.com/private/update.exe".to_owned(),
            )),
            "更新包下载失败，请检查网络后重试。"
        );
        assert_eq!(
            safe_update_download_error(UpdaterError::SignatureUtf8(
                "C:\\Users\\tester\\secret.sig".to_owned(),
            )),
            "更新签名验证失败，当前版本和本地邮件未受影响，请稍后重试。"
        );
        assert_eq!(
            safe_update_install_error(UpdaterError::PackageInstallFailed),
            "更新安装失败，当前版本和本地邮件未受影响，请稍后重试。"
        );
    }
}
