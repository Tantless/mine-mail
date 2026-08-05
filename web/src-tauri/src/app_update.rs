use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::Serialize;
use tauri::{
    AppHandle, State,
    async_runtime::{JoinHandle, Mutex},
    ipc::Channel,
};
use tauri_plugin_updater::UpdaterExt;

const UPDATE_OPERATION_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_UPDATE_IDENTIFIER_BYTES: usize = 64;

#[derive(Clone, Default)]
pub(crate) struct AppUpdateRuntime {
    active: Arc<Mutex<Option<ActiveUpdate>>>,
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
    expected_version: String,
    installing: Arc<AtomicBool>,
    on_progress: Channel<AppUpdateEvent>,
) -> Result<(), String> {
    let updater = app
        .updater_builder()
        .timeout(UPDATE_OPERATION_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|error| error.to_string())?
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
        .map_err(|error| error.to_string())?;

    installing.store(true, Ordering::Release);
    let _ = on_progress.send(AppUpdateEvent::Installing);
    update.install(bytes).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn start_app_update(
    app: AppHandle,
    runtime: State<'_, AppUpdateRuntime>,
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

    let shared_runtime = runtime.inner().clone();
    let task_session_id = session_id.clone();
    let task_channel = on_progress.clone();
    let installing = Arc::new(AtomicBool::new(false));
    let task_installing = installing.clone();
    let task = tauri::async_runtime::spawn(async move {
        let result =
            download_and_install(app, expected_version, task_installing, task_channel.clone())
                .await;
        let mut active = shared_runtime.active.lock().await;
        if active.as_ref().map(|task| task.session_id.as_str()) == Some(task_session_id.as_str()) {
            *active = None;
        }
        drop(active);

        match result {
            Ok(()) => {
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
}
