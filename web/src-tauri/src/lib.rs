mod account;
mod desktop;
mod mail_html;

use std::{env, path::PathBuf};

use mine_mail::{
    ComposeRequest, ConnectionReport, Draft, DraftDeleteKind, DraftSaveKind, DraftSaveOutcome,
    InboxMessage, MailAddress, OutboxItem, OutboxStatus, SyncReport, outbox_body_text,
    outbox_preview, outbox_subject,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};
use url::Url;

use account::{
    AccountPresetDto, AccountRuntime, AccountStatusDto, BackendState, ConfigureAccountRequest,
};
use desktop::{
    DeleteProfileAvatarRequest, DesktopRuntime, DesktopSettingsDto, DesktopSettingsUpdate,
    NewMailNotificationDto, ProfileAvatarDto, SaveProfileAvatarRequest,
};
use mail_html::{
    MailBodySegmentConfidence, MailBodySegmentKind, MailBodySegmentMetadata, MailHtmlStructure,
    SanitizedMailBodySegment, sanitize_mail_html, segment_mail_body,
};

const INBOX_SYNC_LIMIT: usize = 100;
const INBOX_LIST_LIMIT: usize = 250;
const INBOX_PREFETCH_LIMIT: usize = 20;
const INBOX_PREFETCH_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
const INBOX_PREFETCH_MESSAGE_BYTES: u32 = 2 * 1024 * 1024;

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

/// The desktop boundary never exposes raw RFC822 or untrusted HTML. Full-body
/// responses may include a Rust-sanitized HTML fragment for the sandboxed
/// reader; list responses only advertise that such a body is available.
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
    body_html: Option<String>,
    body_render_mode: Option<BodyRenderMode>,
    body_segments: Vec<BodySegmentDto>,
    body_html_available: bool,
    body_html_loaded: bool,
    has_remote_images: bool,
    attachment_names: Vec<String>,
    body_fetched: bool,
    synced_at: String,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BodyRenderMode {
    Plain,
    NativeHtml,
    IsolatedHtml,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct BodySegmentDto {
    kind: BodySegmentKindDto,
    content: String,
    render_mode: BodyRenderMode,
    quote_depth: u8,
    confidence: BodySegmentConfidenceDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    quote_metadata: Option<BodySegmentMetadataDto>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct BodySegmentMetadataDto {
    subject: Option<String>,
    sender: Option<String>,
    recipient: Option<String>,
    sent_at: Option<String>,
}

impl From<MailBodySegmentMetadata> for BodySegmentMetadataDto {
    fn from(value: MailBodySegmentMetadata) -> Self {
        Self {
            subject: value.subject,
            sender: value.sender,
            recipient: value.recipient,
            sent_at: value.sent_at,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BodySegmentKindDto {
    Authored,
    Quoted,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BodySegmentConfidenceDto {
    High,
    Medium,
}

impl From<SanitizedMailBodySegment> for BodySegmentDto {
    fn from(value: SanitizedMailBodySegment) -> Self {
        let render_mode = if !value.is_html {
            BodyRenderMode::Plain
        } else {
            match value.structure {
                MailHtmlStructure::Isolated => BodyRenderMode::IsolatedHtml,
                MailHtmlStructure::Native | MailHtmlStructure::PlainEquivalent => {
                    BodyRenderMode::NativeHtml
                }
            }
        };
        Self {
            kind: match value.kind {
                MailBodySegmentKind::Authored => BodySegmentKindDto::Authored,
                MailBodySegmentKind::Quoted => BodySegmentKindDto::Quoted,
            },
            content: value.content,
            render_mode,
            quote_depth: value.quote_depth,
            confidence: match value.confidence {
                MailBodySegmentConfidence::High => BodySegmentConfidenceDto::High,
                MailBodySegmentConfidence::Medium => BodySegmentConfidenceDto::Medium,
            },
            quote_metadata: value.quote_metadata.map(Into::into),
        }
    }
}

impl InboxMessageDto {
    fn summary(value: InboxMessage) -> Self {
        let body_html_available = value.body_html.is_some();
        Self::from_parts(
            value,
            None,
            None,
            Vec::new(),
            body_html_available,
            false,
            false,
        )
    }

    fn full(value: InboxMessage) -> Self {
        // MIME extraction (including safe CID image resolution) already ran
        // when the body entered SQLite. Re-parsing raw RFC822 on every click
        // made cached HTML feel like a network operation.
        let body_html_available = value.body_html.is_some();
        let has_readable_text = value
            .body_text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty());
        let has_reply_headers = !value.in_reply_to.is_empty() || !value.references.is_empty();
        let body_segments = segment_mail_body(
            value.body_text.as_deref(),
            value.body_html.as_deref(),
            has_reply_headers,
        )
        .into_iter()
        .map(Into::into)
        .collect();
        let sanitized = value.body_html.as_deref().map(sanitize_mail_html);
        let has_remote_images = sanitized
            .as_ref()
            .is_some_and(|html| html.has_remote_images);
        // Text-equivalent wrappers use the existing plain reader. Bounded,
        // semantic HTML is stripped of sender styling and rendered natively
        // against the Mine Mail material. Layout-dependent sender HTML (and
        // HTML without a readable text alternative) stays isolated.
        let (body_html, body_render_mode) = match sanitized {
            None => (None, BodyRenderMode::Plain),
            Some(html) if !has_readable_text => (Some(html.fragment), BodyRenderMode::IsolatedHtml),
            Some(html) => match html.structure {
                MailHtmlStructure::PlainEquivalent => (None, BodyRenderMode::Plain),
                MailHtmlStructure::Native => match html.native_fragment {
                    Some(fragment) => (Some(fragment), BodyRenderMode::NativeHtml),
                    None => (Some(html.fragment), BodyRenderMode::IsolatedHtml),
                },
                MailHtmlStructure::Isolated => (Some(html.fragment), BodyRenderMode::IsolatedHtml),
            },
        };
        Self::from_parts(
            value,
            body_html,
            Some(body_render_mode),
            body_segments,
            body_html_available,
            true,
            has_remote_images,
        )
    }

    fn from_parts(
        value: InboxMessage,
        body_html: Option<String>,
        body_render_mode: Option<BodyRenderMode>,
        body_segments: Vec<BodySegmentDto>,
        body_html_available: bool,
        body_html_loaded: bool,
        has_remote_images: bool,
    ) -> Self {
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
            body_html,
            body_render_mode,
            body_segments,
            body_html_available,
            body_html_loaded,
            has_remote_images,
            attachment_names: value.attachment_names,
            body_fetched: value.body_fetched,
            synced_at: value.synced_at,
        }
    }
}

impl From<InboxMessage> for InboxMessageDto {
    fn from(value: InboxMessage) -> Self {
        Self::full(value)
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
    subject: String,
    preview: String,
    status: OutboxStatus,
    attempts: u32,
    last_error: Option<String>,
    created_at: String,
    sent_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct OutboxMessageDto {
    id: String,
    subject: String,
    body_text: String,
    body_fetched: bool,
}

#[derive(Clone, Debug, Serialize)]
struct AccountMailboxSnapshotDto {
    account_id: String,
    inbox: Vec<InboxMessageDto>,
    drafts: Vec<DraftDto>,
    outbox: Vec<OutboxItemDto>,
}

impl From<OutboxItem> for OutboxItemDto {
    fn from(value: OutboxItem) -> Self {
        let subject = outbox_subject(&value).unwrap_or_default();
        let preview = outbox_preview(&value).unwrap_or_default();
        Self {
            id: value.id,
            draft_id: value.draft_id,
            recipients: value.recipients,
            subject,
            preview,
            status: value.status,
            attempts: value.attempts,
            last_error: value.last_error,
            created_at: value.created_at,
            sent_at: value.sent_at,
        }
    }
}

impl From<OutboxItem> for OutboxMessageDto {
    fn from(value: OutboxItem) -> Self {
        Self {
            id: value.id.clone(),
            subject: outbox_subject(&value).unwrap_or_default(),
            body_text: outbox_body_text(&value).unwrap_or_default(),
            body_fetched: true,
        }
    }
}

#[tauri::command]
async fn check_connections(
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
) -> CommandResult<ConnectionReport> {
    account.refresh_active_oauth_backend(&backend).await?;
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
        .map(|messages| messages.into_iter().map(InboxMessageDto::summary).collect())
        .map_err(safe_mail_error)
}

#[tauri::command]
fn open_external_url(url: String) -> CommandResult<()> {
    let url = validate_external_url(&url)?;
    open::that(url.as_str())
        .map_err(|_| "The link could not be opened in the system browser.".to_owned())
}

fn validate_external_url(value: &str) -> CommandResult<Url> {
    let url = Url::parse(value.trim()).map_err(|_| "The link is invalid.".to_owned())?;
    match url.scheme() {
        "http" | "https" => {
            if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
                return Err("The link is not safe to open.".to_owned());
            }
        }
        "mailto" => {
            if url.path().trim().is_empty() {
                return Err("The email link has no recipient.".to_owned());
            }
        }
        _ => return Err("This link type is not supported.".to_owned()),
    }
    Ok(url)
}

#[tauri::command]
async fn fetch_message(
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    uid: u32,
) -> CommandResult<InboxMessageDto> {
    let _ = account.refresh_active_oauth_backend(&backend).await;
    match backend.network() {
        Ok(network) => network
            .fetch_message(uid, false)
            .await
            .map(Into::into)
            .map_err(safe_mail_error),
        Err(network_error) => backend
            .local()?
            .cached_inbox_message(uid)
            .map(Into::into)
            .map_err(|_| network_error),
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
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    draft_id: String,
    expected_local_version: u64,
    confirmed_recipients: Vec<String>,
) -> CommandResult<OutboxItemDto> {
    account.refresh_active_oauth_backend(&backend).await?;
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
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    outbox_id: String,
) -> CommandResult<OutboxItemDto> {
    account.refresh_active_oauth_backend(&backend).await?;
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

/// Hydrates only the selected local Outbox body. Raw RFC822 bytes never cross
/// the desktop boundary, and list responses remain bounded summaries.
#[tauri::command]
fn fetch_outbox_message(
    backend: State<'_, BackendState>,
    outbox_id: String,
) -> CommandResult<OutboxMessageDto> {
    let backend = backend.local()?;
    backend
        .outbox_message(&outbox_id)
        .map(Into::into)
        .map_err(safe_mail_error)
}

/// Read one account's complete local navigation snapshot without changing the
/// active account. React prewarms these bounded SQLite views so switching an
/// already connected mailbox never waits for IMAP or exposes another account's
/// messages while the target view is loading.
#[tauri::command]
fn get_account_mailbox_snapshot(
    backend: State<'_, BackendState>,
    account_id: String,
    limit: Option<usize>,
) -> CommandResult<AccountMailboxSnapshotDto> {
    let local = backend.local_for(&account_id)?;
    let limit = limit.unwrap_or(INBOX_LIST_LIMIT).clamp(1, INBOX_LIST_LIMIT);
    let inbox = local
        .list_inbox(limit)
        .map(|messages| messages.into_iter().map(InboxMessageDto::summary).collect())
        .map_err(safe_mail_error)?;
    let drafts = local
        .list_drafts()
        .map(|drafts| drafts.into_iter().map(Into::into).collect())
        .map_err(safe_mail_error)?;
    let outbox = local
        .list_outbox()
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(safe_mail_error)?;
    Ok(AccountMailboxSnapshotDto {
        account_id,
        inbox,
        drafts,
        outbox,
    })
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
fn get_new_mail_notification(
    runtime: State<'_, DesktopRuntime>,
) -> CommandResult<Option<NewMailNotificationDto>> {
    runtime.latest_new_mail_notification()
}

#[tauri::command]
fn dismiss_new_mail_notification(app: AppHandle, notification_id: u64) -> CommandResult<bool> {
    desktop::dismiss_new_mail_notification(&app, notification_id)
}

#[tauri::command]
fn open_new_mail_notification(
    app: AppHandle,
    notification_id: u64,
    uid: u32,
    account_id: String,
) -> CommandResult<bool> {
    desktop::open_new_mail_notification(&app, notification_id, uid, account_id)
}

#[tauri::command]
fn list_profile_avatars(
    runtime: State<'_, DesktopRuntime>,
) -> CommandResult<Vec<ProfileAvatarDto>> {
    runtime.list_profile_avatars()
}

#[tauri::command]
fn save_profile_avatar(
    runtime: State<'_, DesktopRuntime>,
    request: SaveProfileAvatarRequest,
) -> CommandResult<ProfileAvatarDto> {
    runtime.save_profile_avatar(request)
}

#[tauri::command]
fn delete_profile_avatar(
    runtime: State<'_, DesktopRuntime>,
    request: DeleteProfileAvatarRequest,
) -> CommandResult<()> {
    runtime.delete_profile_avatar(request)
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

    let autostart_enabled = if let Some(enabled) =
        requested_autostart_change(previous_autostart, settings.autostart_enabled)
    {
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
    let current = autostart
        .is_enabled()
        .map_err(|_| "The system startup setting could not be read.".to_owned())?;
    if current == enabled {
        return Ok(());
    }
    if enabled {
        autostart.enable()
    } else {
        autostart.disable()
    }
    .map_err(|_| "The system startup setting could not be updated.".to_owned())
}

fn requested_autostart_change(current: bool, requested: Option<bool>) -> Option<bool> {
    requested.filter(|requested| *requested != current)
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
    let (status, _account_changed) = account.configure(&backend, request).await?;
    let _ = app.emit("mail:account-updated", status.clone());
    desktop::request_sync(&app, true);
    Ok(status)
}

#[tauri::command]
async fn connect_google_account(
    app: AppHandle,
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    desktop_runtime: State<'_, DesktopRuntime>,
) -> CommandResult<AccountStatusDto> {
    let _sync_guard = desktop_runtime.acquire_sync_gate().await;
    let (status, _account_changed) = account.connect_google(&backend).await?;
    let _ = app.emit("mail:account-updated", status.clone());
    desktop::request_sync(&app, true);
    Ok(status)
}

#[tauri::command]
fn switch_account(
    app: AppHandle,
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    account_id: String,
) -> CommandResult<AccountStatusDto> {
    let status = account.switch_account(&backend, &account_id)?;
    let _ = app.emit("mail:account-updated", status.clone());
    Ok(status)
}

#[tauri::command]
async fn remove_account(
    app: AppHandle,
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    desktop_runtime: State<'_, DesktopRuntime>,
    account_id: String,
) -> CommandResult<AccountStatusDto> {
    let _sync_guard = desktop_runtime.acquire_sync_gate().await;
    let status = account.remove_account(&backend, &account_id)?;
    if let Err(error) = desktop_runtime.remove_notification_baseline(&account_id) {
        desktop_runtime.record_startup_error(error);
    }
    let _ = app.emit("mail:account-updated", status.clone());
    if status.configured {
        desktop::request_sync(&app, true);
    }
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
    desktop::start_inbox_monitor_supervisor(app.handle().clone(), shutdown_rx.clone());
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
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .setup(initialize_state)
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                if window.label() == "new-mail-notification" {
                    api.prevent_close();
                    let _ = window.hide();
                    return;
                }
                if window.label() != "main" {
                    return;
                }
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
            WindowEvent::Focused(true) if window.label() == "main" => {
                desktop::request_sync(window.app_handle(), false)
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_account_status,
            list_account_presets,
            configure_account,
            connect_google_account,
            switch_account,
            remove_account,
            check_connections,
            sync_inbox,
            sync_all,
            list_inbox,
            fetch_message,
            open_external_url,
            save_draft,
            list_drafts,
            delete_draft,
            sync_drafts,
            send_draft,
            retry_outbox,
            list_outbox,
            fetch_outbox_message,
            get_account_mailbox_snapshot,
            get_desktop_settings,
            update_desktop_settings,
            get_new_mail_notification,
            dismiss_new_mail_notification,
            open_new_mail_notification,
            list_profile_avatars,
            save_profile_avatar,
            delete_profile_avatar,
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

#[cfg(test)]
mod tests {
    use mine_mail::{InboxMessage, OutboxItem, OutboxStatus};

    use super::{
        InboxMessageDto, OutboxItemDto, OutboxMessageDto, requested_autostart_change,
        validate_external_url,
    };

    fn rich_message() -> InboxMessage {
        InboxMessage {
            id: 1,
            account_id: "primary".to_owned(),
            mailbox: "INBOX".to_owned(),
            uid: 7,
            message_id: None,
            in_reply_to: Vec::new(),
            references: Vec::new(),
            subject: "Rich".to_owned(),
            sender: None,
            to: Vec::new(),
            cc: Vec::new(),
            sent_at: None,
            internal_date: None,
            flags: Vec::new(),
            size_bytes: 100,
            preview: "Preview".to_owned(),
            body_text: Some("Fallback".to_owned()),
            body_html: Some(
                r#"<style>.desktop{display:block}</style><div onclick="alert(1)">Rich</div><script>alert(2)</script>"#
                    .to_owned(),
            ),
            attachment_names: Vec::new(),
            body_fetched: true,
            raw_rfc822: Vec::new(),
            synced_at: "2026-07-15T00:00:00Z".to_owned(),
        }
    }

    fn outbox_item() -> OutboxItem {
        OutboxItem {
            id: "outbox-1".to_owned(),
            account_id: "primary".to_owned(),
            draft_id: None,
            draft_revision: None,
            draft_local_version: None,
            recipients: vec!["receiver@example.com".to_owned()],
            status: OutboxStatus::Sent,
            attempts: 1,
            last_error: None,
            created_at: "2026-07-18T00:00:00Z".to_owned(),
            sent_at: Some("2026-07-18T00:00:01Z".to_owned()),
            raw_rfc822: b"From: sender@example.com\r\nTo: receiver@example.com\r\nSubject: Re: Actual subject\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nActual sent body".to_vec(),
        }
    }

    #[test]
    fn outbox_summaries_and_selected_bodies_cross_separate_safe_boundaries() {
        let summary = serde_json::to_value(OutboxItemDto::from(outbox_item()))
            .expect("serialize Outbox summary");
        assert_eq!(summary["subject"], "Re: Actual subject");
        assert_eq!(summary["preview"], "Actual sent body");
        assert!(summary.get("body_text").is_none());
        assert!(summary.get("raw_rfc822").is_none());

        let selected = serde_json::to_value(OutboxMessageDto::from(outbox_item()))
            .expect("serialize selected Outbox body");
        assert_eq!(selected["subject"], "Re: Actual subject");
        assert_eq!(selected["body_text"], "Actual sent body");
        assert_eq!(selected["body_fetched"], true);
        assert!(selected.get("raw_rfc822").is_none());
    }

    #[test]
    fn unchanged_autostart_requests_are_no_ops() {
        assert_eq!(requested_autostart_change(false, Some(false)), None);
        assert_eq!(requested_autostart_change(true, Some(true)), None);
        assert_eq!(requested_autostart_change(false, None), None);
        assert_eq!(requested_autostart_change(false, Some(true)), Some(true));
        assert_eq!(requested_autostart_change(true, Some(false)), Some(false));
    }

    #[test]
    fn summaries_advertise_html_without_crossing_the_body_boundary() {
        let dto = InboxMessageDto::summary(rich_message());
        let json = serde_json::to_value(dto).expect("serialize summary");

        assert_eq!(json["body_html_available"], true);
        assert_eq!(json["body_html_loaded"], false);
        assert!(json["body_html"].is_null());
        assert!(json["body_render_mode"].is_null());
        assert!(json.get("raw_rfc822").is_none());
    }

    #[test]
    fn full_bodies_cross_the_boundary_only_after_sanitization() {
        let dto = InboxMessageDto::full(rich_message());
        let json = serde_json::to_value(dto).expect("serialize full body");
        let html = json["body_html"].as_str().expect("safe HTML");

        assert!(html.contains("<style>"));
        assert!(html.contains("Rich"));
        assert!(!html.contains("onclick"));
        assert!(!html.contains("<script"));
        assert_eq!(json["body_render_mode"], "isolated_html");
        assert_eq!(json["body_html_loaded"], true);
        assert!(json.get("raw_rfc822").is_none());
    }

    #[test]
    fn reply_bodies_cross_as_safe_authored_and_quoted_segments() {
        let mut message = rich_message();
        message.in_reply_to = vec!["parent@example.com".to_owned()];
        message.body_text = Some(
            "My reply.\n\n---- 回复的原邮件 ----\n| 发件人 | sender@example.com |\n| 收件人 | receiver@example.com |\n| 主题 | Earlier note |\n| 日期 | 2026-07-01 |\nOriginal body.\n\n---- 回复的原邮件 ----\n| 发件人 | older@example.com |\nOlder body."
                .to_owned(),
        );
        message.body_html = Some(
            r#"<div>My reply.</div><div class="ntes-mailmaster-quote"><table><tr><td>Original body.</td></tr></table></div>"#
                .to_owned(),
        );

        let dto = InboxMessageDto::full(message);
        let json = serde_json::to_value(dto).expect("serialize segmented body");
        let segments = json["body_segments"].as_array().expect("body segments");

        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0]["kind"], "authored");
        assert_eq!(segments[0]["render_mode"], "plain");
        assert_eq!(segments[0]["content"], "My reply.");
        assert_eq!(segments[1]["kind"], "quoted");
        assert_eq!(segments[1]["confidence"], "high");
        assert_eq!(segments[1]["content"], "Original body.");
        assert_eq!(segments[1]["quote_depth"], 1);
        assert_eq!(segments[1]["quote_metadata"]["subject"], "Earlier note");
        assert_eq!(
            segments[1]["quote_metadata"]["sender"],
            "sender@example.com"
        );
        assert_eq!(
            segments[1]["quote_metadata"]["recipient"],
            "receiver@example.com"
        );
        assert_eq!(segments[1]["quote_metadata"]["sent_at"], "2026-07-01");
        assert_eq!(segments[2]["kind"], "quoted");
        assert_eq!(segments[2]["content"], "Older body.");
        assert_eq!(segments[2]["quote_depth"], 2);
        assert!(json.get("raw_rfc822").is_none());
    }

    #[test]
    fn plain_html_wrappers_use_the_plain_text_reader() {
        let mut message = rich_message();
        message.body_html = Some("<div>Hello there</div><p>A short reply.</p>".to_owned());
        message.body_text = Some("Hello there".to_owned());

        let dto = InboxMessageDto::full(message);
        let json = serde_json::to_value(dto).expect("serialize native body");

        assert!(json["body_html"].is_null());
        assert_eq!(json["body_text"], "Hello there");
        assert_eq!(json["body_render_mode"], "plain");
        assert_eq!(json["body_html_available"], true);
        assert_eq!(json["body_html_loaded"], true);
    }

    #[test]
    fn bounded_semantic_html_uses_the_native_themed_html_reader() {
        let mut message = rich_message();
        message.body_html = Some(
            r#"<div class="signature"><strong style="color:red">Myo</strong>
               <a href="https://paa.moe">myo@paa.moe</a></div>"#
                .to_owned(),
        );
        message.body_text = Some("Myo myo@paa.moe".to_owned());

        let dto = InboxMessageDto::full(message);
        let json = serde_json::to_value(dto).expect("serialize native HTML body");
        let html = json["body_html"].as_str().expect("native HTML");

        assert_eq!(json["body_render_mode"], "native_html");
        assert!(html.contains("<strong>Myo</strong>"));
        assert!(html.contains("href=\"https://paa.moe\""));
        assert!(!html.contains("class="));
        assert!(!html.contains("style="));
    }

    #[test]
    fn small_signature_table_uses_the_native_themed_html_reader() {
        let mut message = rich_message();
        message.body_html = Some(
            r#"<div style="width:640px"><table width="640" border="0"><tbody><tr>
               <td style="width:72px"><img alt="avatar" width="64" src="data:image/png;base64,AQID"></td>
               <td><strong>Myo</strong><br><a href="https://paa.moe">myo@paa.moe</a></td>
               </tr></tbody></table><i>A short signature.</i></div>"#
                .to_owned(),
        );
        message.body_text = Some("Myo myo@paa.moe A short signature.".to_owned());

        let dto = InboxMessageDto::full(message);
        let json = serde_json::to_value(dto).expect("serialize native table body");
        let html = json["body_html"].as_str().expect("native table HTML");

        assert_eq!(json["body_render_mode"], "native_html");
        assert!(html.contains("<table>"));
        assert!(html.contains("data:image/png;base64,AQID"));
        assert!(!html.contains("style="));
        assert!(!html.contains("width="));
    }

    #[test]
    fn external_links_accept_only_explicit_safe_schemes() {
        assert!(validate_external_url("https://example.com/mail").is_ok());
        assert!(validate_external_url("mailto:friend@example.com").is_ok());
        assert!(validate_external_url("javascript:alert(1)").is_err());
        assert!(validate_external_url("file:///C:/Windows/system.ini").is_err());
        assert!(validate_external_url("https://user:pass@example.com/").is_err());
    }
}
