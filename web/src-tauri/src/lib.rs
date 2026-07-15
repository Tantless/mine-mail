mod account;
mod desktop;

use std::{env, path::PathBuf};

use mine_mail::{
    ComposeRequest, ConnectionReport, Draft, DraftDeleteKind, DraftSaveKind, DraftSaveOutcome,
    InboxMessage, MailAddress, OutboxItem, OutboxStatus, SyncReport,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};

use account::{
    AccountPresetDto, AccountRuntime, AccountStatusDto, BackendState, ConfigureAccountRequest,
};
use desktop::{DesktopRuntime, DesktopSettingsDto, DesktopSettingsUpdate};

const INBOX_SYNC_LIMIT: usize = 100;
const INBOX_LIST_LIMIT: usize = 250;

type CommandResult<T> = Result<T, String>;

#[derive(Clone, Debug, Serialize)]
struct MailAddressDto {
    name: Option<String>,
    email: String,
}

impl From<MailAddress> for MailAddressDto {
    fn from(value: MailAddress) -> Self {
        Self {
            name: value.name,
            email: value.email,
        }
    }
}

/// The desktop boundary intentionally has no HTML or raw RFC822 fields.
/// React receives plain text and attachment names only.
#[derive(Clone, Debug, Serialize)]
struct InboxMessageDto {
    id: i64,
    mailbox: String,
    uid: u32,
    message_id: Option<String>,
    subject: String,
    sender: Option<MailAddressDto>,
    to: Vec<MailAddressDto>,
    cc: Vec<MailAddressDto>,
    sent_at: Option<String>,
    internal_date: Option<String>,
    flags: Vec<String>,
    size_bytes: u32,
    preview: String,
    body_text: Option<String>,
    attachment_names: Vec<String>,
    body_fetched: bool,
    synced_at: String,
}

impl From<InboxMessage> for InboxMessageDto {
    fn from(value: InboxMessage) -> Self {
        Self {
            id: value.id,
            mailbox: value.mailbox,
            uid: value.uid,
            message_id: value.message_id,
            subject: value.subject,
            sender: value.sender.map(Into::into),
            to: value.to.into_iter().map(Into::into).collect(),
            cc: value.cc.into_iter().map(Into::into).collect(),
            sent_at: value.sent_at,
            internal_date: value.internal_date,
            flags: value.flags,
            size_bytes: value.size_bytes,
            preview: value.preview,
            body_text: value.body_text,
            attachment_names: value.attachment_names,
            body_fetched: value.body_fetched,
            synced_at: value.synced_at,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct DraftDto {
    id: String,
    local_version: u64,
    has_unsupported_content: bool,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    body_text: String,
    status: String,
    remote_mailbox: Option<String>,
    remote_uid: Option<u32>,
    created_at: String,
    updated_at: String,
}

impl From<Draft> for DraftDto {
    fn from(value: Draft) -> Self {
        Self {
            id: value.id,
            local_version: value.local_version,
            has_unsupported_content: value.has_unsupported_content,
            to: value.to,
            cc: value.cc,
            bcc: value.bcc,
            subject: value.subject,
            body_text: value.body_text,
            status: value.status,
            remote_mailbox: value.remote_mailbox,
            remote_uid: value.remote_uid,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct DraftSaveOutcomeDto {
    kind: DraftSaveKind,
    draft: DraftDto,
    canonical: Option<DraftDto>,
}

#[derive(Clone, Debug, Serialize)]
struct DraftDeleteOutcomeDto {
    kind: DraftDeleteKind,
}

impl From<DraftSaveOutcome> for DraftSaveOutcomeDto {
    fn from(value: DraftSaveOutcome) -> Self {
        Self {
            kind: value.kind,
            draft: value.draft.into(),
            canonical: value.canonical.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct OutboxItemDto {
    id: String,
    draft_id: Option<String>,
    recipients: Vec<String>,
    status: OutboxStatus,
    attempts: u32,
    last_error: Option<String>,
    created_at: String,
    sent_at: Option<String>,
}

impl From<OutboxItem> for OutboxItemDto {
    fn from(value: OutboxItem) -> Self {
        Self {
            id: value.id,
            draft_id: value.draft_id,
            recipients: value.recipients,
            status: value.status,
            attempts: value.attempts,
            last_error: value.last_error,
            created_at: value.created_at,
            sent_at: value.sent_at,
        }
    }
}

#[tauri::command]
async fn check_connections(backend: State<'_, BackendState>) -> CommandResult<ConnectionReport> {
    let backend = backend.network()?;
    backend.check_connections().await.map_err(safe_mail_error)
}

#[tauri::command]
async fn sync_inbox(app: AppHandle) -> CommandResult<SyncReport> {
    desktop::perform_inbox_sync(&app).await
}

#[tauri::command]
fn list_inbox(
    backend: State<'_, BackendState>,
    limit: Option<usize>,
) -> CommandResult<Vec<InboxMessageDto>> {
    let backend = backend.local()?;
    let limit = limit.unwrap_or(INBOX_LIST_LIMIT).clamp(1, INBOX_LIST_LIMIT);
    backend
        .list_inbox(limit)
        .map(|messages| messages.into_iter().map(Into::into).collect())
        .map_err(safe_mail_error)
}

#[tauri::command]
async fn fetch_message(
    backend: State<'_, BackendState>,
    uid: u32,
) -> CommandResult<InboxMessageDto> {
    match backend.network() {
        Ok(network) => network
            .fetch_message(uid, false)
            .await
            .map(Into::into)
            .map_err(safe_mail_error),
        Err(network_error) => {
            let cached = backend
                .local()?
                .list_inbox(usize::MAX)
                .map_err(safe_mail_error)?
                .into_iter()
                .find(|message| message.uid == uid && message.body_fetched);
            cached.map(Into::into).ok_or(network_error)
        }
    }
}

#[tauri::command]
fn save_draft(
    app: AppHandle,
    backend: State<'_, BackendState>,
    request: ComposeRequest,
    draft_id: Option<String>,
    expected_local_version: Option<u64>,
) -> CommandResult<DraftSaveOutcomeDto> {
    let backend = backend.local()?;
    let outcome = backend
        .save_draft_optimistic(draft_id.as_deref(), expected_local_version, request)
        .map(Into::into)
        .map_err(safe_mail_error)?;
    let _ = app.emit("mail:drafts-updated", desktop::DraftsUpdatedEvent::saved());
    Ok(outcome)
}

#[tauri::command]
fn list_drafts(backend: State<'_, BackendState>) -> CommandResult<Vec<DraftDto>> {
    let backend = backend.local()?;
    backend
        .list_drafts()
        .map(|drafts| drafts.into_iter().map(Into::into).collect())
        .map_err(safe_mail_error)
}

#[tauri::command]
fn delete_draft(
    app: AppHandle,
    backend: State<'_, BackendState>,
    draft_id: String,
    expected_local_version: u64,
) -> CommandResult<DraftDeleteOutcomeDto> {
    let backend = backend.local()?;
    let kind = backend
        .delete_draft_optimistic(&draft_id, expected_local_version)
        .map_err(safe_mail_error)?;
    if kind == DraftDeleteKind::Deleted {
        let _ = app.emit(
            "mail:drafts-updated",
            desktop::DraftsUpdatedEvent::deleted(),
        );
    }
    Ok(DraftDeleteOutcomeDto { kind })
}

/// SMTP is reachable only through an already-persisted draft and a second,
/// exact recipient confirmation supplied by the UI at send time.
#[tauri::command]
async fn send_draft(
    backend: State<'_, BackendState>,
    draft_id: String,
    expected_local_version: u64,
    confirmed_recipients: Vec<String>,
) -> CommandResult<OutboxItemDto> {
    let backend = backend.network()?;
    backend
        .send_draft(&draft_id, expected_local_version, &confirmed_recipients)
        .await
        .map(Into::into)
        .map_err(safe_mail_error)
}

/// A manual retry reuses the immutable RFC822 message and SMTP envelope that
/// were already confirmed and persisted in Outbox. Only the Rust core's
/// `retryable` state gate can authorize the transition back to `sending`.
#[tauri::command]
async fn retry_outbox(
    backend: State<'_, BackendState>,
    outbox_id: String,
) -> CommandResult<OutboxItemDto> {
    let backend = backend.network()?;
    backend
        .retry_outbox(&outbox_id)
        .await
        .map(Into::into)
        .map_err(safe_mail_error)
}

#[tauri::command]
fn list_outbox(backend: State<'_, BackendState>) -> CommandResult<Vec<OutboxItemDto>> {
    let backend = backend.local()?;
    backend
        .list_outbox()
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(safe_mail_error)
}

#[tauri::command]
fn get_desktop_settings(
    app: AppHandle,
    runtime: State<'_, DesktopRuntime>,
) -> CommandResult<DesktopSettingsDto> {
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or_else(|_| {
        runtime.record_startup_error(
            "The system startup setting could not be read; autostart is shown as disabled.",
        );
        false
    });
    runtime.settings_dto(autostart_enabled)
}

#[tauri::command]
fn update_desktop_settings(
    app: AppHandle,
    runtime: State<'_, DesktopRuntime>,
    settings: DesktopSettingsUpdate,
) -> CommandResult<DesktopSettingsDto> {
    let previous_settings = runtime.user_settings_snapshot()?;
    let previous_autostart = app.autolaunch().is_enabled().map_err(|_| {
        "The system startup setting could not be read; no settings were changed.".to_owned()
    })?;

    runtime.update_settings(settings)?;

    let autostart_enabled = if let Some(enabled) = settings.autostart_enabled {
        if set_autostart_enabled(&app, enabled).is_err() {
            let local_rollback_failed = runtime.update_settings(previous_settings).is_err();
            let system_rollback_failed = set_autostart_enabled(&app, previous_autostart).is_err();
            let mut error = if enabled {
                "Mine Mail could not be enabled at system startup; the settings update was rolled back."
                    .to_owned()
            } else {
                "Mine Mail could not be disabled at system startup; the settings update was rolled back."
                    .to_owned()
            };
            if local_rollback_failed || system_rollback_failed {
                error.push_str(" Part of the rollback could not be verified.");
            }
            return Err(error);
        }
        enabled
    } else {
        previous_autostart
    };
    runtime.settings_dto(autostart_enabled)
}

fn set_autostart_enabled(app: &AppHandle, enabled: bool) -> CommandResult<()> {
    let autostart = app.autolaunch();
    if enabled {
        autostart.enable()
    } else {
        autostart.disable()
    }
    .map_err(|_| "The system startup setting could not be updated.".to_owned())
}

#[tauri::command]
fn complete_exit(app: AppHandle, request_id: u64) -> CommandResult<bool> {
    desktop::complete_exit(&app, request_id)
}

#[tauri::command]
fn cancel_exit(app: AppHandle, request_id: u64) -> CommandResult<bool> {
    desktop::cancel_exit(&app, request_id)
}

#[tauri::command]
async fn sync_all(app: AppHandle) -> CommandResult<desktop::SyncAllReport> {
    desktop::perform_sync_all(&app, true)
        .await?
        .ok_or_else(|| "The requested synchronization was skipped.".to_owned())
}

#[tauri::command]
async fn sync_drafts(app: AppHandle) -> CommandResult<mine_mail::DraftSyncReport> {
    desktop::perform_draft_sync(&app).await
}

#[tauri::command]
fn list_account_presets() -> Vec<AccountPresetDto> {
    account::account_presets()
}

#[tauri::command]
fn get_account_status(
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
) -> AccountStatusDto {
    account.status(&backend)
}

#[tauri::command]
async fn configure_account(
    app: AppHandle,
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    desktop_runtime: State<'_, DesktopRuntime>,
    request: ConfigureAccountRequest,
) -> CommandResult<AccountStatusDto> {
    let _sync_guard = desktop_runtime.acquire_sync_gate().await;
    let (status, account_changed) = account.configure(&backend, request).await?;
    if account_changed && let Err(error) = desktop_runtime.reset_notification_baseline() {
        // Account credentials, metadata and backend have already switched
        // successfully. Keep that result authoritative, retain the safe
        // in-memory reset, and expose the persistence problem as a desktop
        // diagnostic instead of falsely reporting account failure.
        desktop_runtime.record_startup_error(error);
    }
    let _ = app.emit("mail:account-updated", status.clone());
    desktop::request_sync(&app, true);
    Ok(status)
}

fn safe_mail_error(error: mine_mail::MailError) -> String {
    use mine_mail::MailError;

    match error {
        MailError::Validation(message) => format!("Validation failed: {message}"),
        MailError::NotFound { entity, id } => format!("{entity} was not found: {id}"),
        MailError::Timeout { operation } => format!("{operation} timed out. Please try again."),
        MailError::Imap(_) => "The mail server could not complete the Inbox request.".to_owned(),
        MailError::Smtp(_) => "The mail server could not complete the send request.".to_owned(),
        MailError::Config(_)
        | MailError::Database(_)
        | MailError::Io(_)
        | MailError::Serialization(_)
        | MailError::Mime(_) => "Mine Mail could not complete the local operation.".to_owned(),
    }
}

fn legacy_credentials_file() -> Option<PathBuf> {
    if let Some(value) = env::var_os("MINE_MAIL_CREDENTIALS_FILE")
        && !value.is_empty()
    {
        return Some(PathBuf::from(value));
    }

    #[cfg(debug_assertions)]
    {
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../password.txt"))
    }

    #[cfg(not(debug_assertions))]
    {
        None
    }
}

fn initialize_state(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let (app_data, path_error) = match app.path().app_local_data_dir() {
        Ok(path) => (path, None),
        Err(_) => (
            env::temp_dir().join("mine-mail-degraded"),
            Some(
                "The application data directory is unavailable; local mail is disabled for this session."
                    .to_owned(),
            ),
        ),
    };
    let (account, backend) = if let Some(error) = path_error.as_ref() {
        AccountRuntime::fallback(&app_data, error.clone())
    } else {
        let legacy_credentials = legacy_credentials_file();
        AccountRuntime::open(&app_data, legacy_credentials.as_deref())
            .unwrap_or_else(|error| AccountRuntime::fallback(&app_data, error))
    };
    let local_backend_ready = backend.is_local_ready();
    let (desktop, sync_rx, shutdown_rx) = DesktopRuntime::open(&app_data);
    if let Some(error) = path_error {
        desktop.record_startup_error(error);
    }
    let startup_degraded = desktop.has_startup_error();

    app.manage(account);
    app.manage(backend);
    app.manage(desktop);
    let tray_available = match desktop::build_tray(app) {
        Ok(()) => true,
        Err(_) => {
            app.state::<DesktopRuntime>().record_startup_error(
                "The system tray could not be initialized; Mine Mail will remain visible.",
            );
            false
        }
    };
    desktop::start_background_loop(app.handle().clone(), sync_rx, shutdown_rx);

    if is_background_launch(std::env::args())
        && tray_available
        && local_backend_ready
        && !startup_degraded
    {
        desktop::request_sync(app.handle(), true);
    } else {
        desktop::show_main_window(app.handle(), true);
    }
    Ok(())
}

fn is_background_launch(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().any(|argument| argument == "--background")
}

pub fn run() {
    let app = tauri::Builder::default()
        // The single-instance plugin must remain the first plugin registered.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if is_background_launch(args) {
                desktop::request_sync(app, true);
            } else {
                desktop::show_main_window(app, true);
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .setup(initialize_state)
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                if let Some(runtime) = window.app_handle().try_state::<DesktopRuntime>() {
                    if runtime.is_exit_committed() {
                        return;
                    }
                    api.prevent_close();
                    if runtime.is_quitting() {
                        return;
                    }
                    if runtime.background_enabled() {
                        let _ = window.hide();
                    } else {
                        desktop::quit_app(window.app_handle());
                    }
                }
            }
            WindowEvent::Focused(true) => desktop::request_sync(window.app_handle(), false),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_account_status,
            list_account_presets,
            configure_account,
            check_connections,
            sync_inbox,
            sync_all,
            list_inbox,
            fetch_message,
            save_draft,
            list_drafts,
            delete_draft,
            sync_drafts,
            send_draft,
            retry_outbox,
            list_outbox,
            get_desktop_settings,
            update_desktop_settings,
            complete_exit,
            cancel_exit,
        ])
        .build(tauri::generate_context!())
        .expect("Mine Mail desktop runtime failed");

    app.run(|app, event| match event {
        RunEvent::Resumed => desktop::request_sync(app, false),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => desktop::show_main_window(app, false),
        RunEvent::ExitRequested { api, .. } => {
            if let Some(runtime) = app.try_state::<DesktopRuntime>() {
                if runtime.is_exit_committed() {
                    return;
                }
                api.prevent_exit();
                if !runtime.is_quitting() {
                    desktop::quit_app(app);
                }
            }
        }
        RunEvent::Exit => {
            if let Some(runtime) = app.try_state::<DesktopRuntime>() {
                runtime.finish_quit();
            }
        }
        _ => {}
    });
}
