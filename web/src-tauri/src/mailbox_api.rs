use std::{future::Future, time::Instant};

use mine_mail::{
    AttachmentMeta, AttachmentSaveErrorKind, AttachmentSaveStatus, ForwardPreparationErrorKind,
    ForwardPreparationOutcome, InboxMessage, MailBackend, MailError, MailboxCapability,
    MailboxCapabilityStatus, MailboxCapabilityUnavailableReason, MailboxRole,
    MessageMutationReceipt, MessagePage, MessagePageCursor, MessagePageItem,
    PendingMessageProjection, PermanentDeletePlan, RemoteHistoryState, SystemFlagMutationReceipt,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    AttachmentMetaDto, AttachmentSaveResultDto, BodyRenderMode, BodySegmentConfidenceDto,
    BodySegmentDto, BodySegmentKindDto, BodySegmentMetadataDto, CommandResult,
    ForwardPreparationOutcomeDto, InboxMessageDto, MailAddressDto, MessageNavigationTargetDto,
    account::{AccountRuntime, BackendState},
    desktop,
    diagnostics::{self, Fields},
    full_message_dto, safe_mail_error,
};

const MAX_ACCOUNT_ID_CHARS: usize = 128;
const MAX_ATTACHMENT_ID_BYTES: usize = 256;
const MAX_CURSOR_BYTES: usize = 64;
const MAX_LOCAL_SEARCH_CHARS: usize = 256;
const MAX_MESSAGE_PAGE_SIZE: usize = 100;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MailboxSyncReportDto {
    synced: usize,
    removed: usize,
    uid_validity_reset: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MailboxCapabilityDto {
    role: MailboxRole,
    status: MailboxCapabilityStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable_reason: Option<MailboxCapabilityUnavailableReason>,
    retryable: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArchiveFolderCandidateDto {
    selection_id: String,
    display_name: String,
}

impl From<MailboxCapability> for MailboxCapabilityDto {
    fn from(value: MailboxCapability) -> Self {
        // `display_name` is the concrete provider mailbox name. It is used by
        // Rust/SQLite only and must never cross into React.
        Self {
            role: value.role,
            status: value.status,
            unavailable_reason: value.unavailable_reason,
            retryable: value.retryable,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct MessageSummaryDto {
    id: String,
    subject: String,
    sender: Option<MailAddressDto>,
    to: Vec<MailAddressDto>,
    cc: Vec<MailAddressDto>,
    bcc: Vec<MailAddressDto>,
    sent_at: Option<String>,
    internal_date: Option<String>,
    flags: Vec<String>,
    size_bytes: u32,
    preview: String,
    body_html_available: bool,
    attachment_names: Vec<String>,
    body_fetched: bool,
    synced_at: String,
}

#[derive(Clone, Debug, Serialize)]
struct MailboxBodySegmentDto {
    kind: BodySegmentKindDto,
    content: String,
    render_mode: BodyRenderMode,
    quote_depth: u8,
    confidence: BodySegmentConfidenceDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    quote_metadata: Option<BodySegmentMetadataDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    navigation_target: Option<MessageNavigationTargetDto>,
}

impl From<BodySegmentDto> for MailboxBodySegmentDto {
    fn from(value: BodySegmentDto) -> Self {
        Self {
            kind: value.kind,
            content: value.content,
            render_mode: value.render_mode,
            quote_depth: value.quote_depth,
            confidence: value.confidence,
            quote_metadata: value.quote_metadata,
            navigation_target: value.navigation_target,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MailboxMessageDto {
    id: String,
    subject: String,
    sender: Option<MailAddressDto>,
    to: Vec<MailAddressDto>,
    cc: Vec<MailAddressDto>,
    bcc: Vec<MailAddressDto>,
    sent_at: Option<String>,
    internal_date: Option<String>,
    flags: Vec<String>,
    size_bytes: u32,
    preview: String,
    body_text: Option<String>,
    body_html: Option<String>,
    body_render_mode: Option<BodyRenderMode>,
    body_segments: Vec<MailboxBodySegmentDto>,
    body_html_available: bool,
    body_html_loaded: bool,
    has_remote_images: bool,
    attachment_names: Vec<String>,
    /// `Some`, including an empty vector, is authoritative for the current
    /// server MIME structure or the completely cached MIME. `None` keeps a
    /// readable cached body available when attachment metadata is temporarily
    /// unavailable.
    attachments: Option<Vec<AttachmentMetaDto>>,
    body_fetched: bool,
    synced_at: String,
}

impl MailboxMessageDto {
    fn from_full_message(
        public_id: String,
        value: InboxMessageDto,
        attachments: Option<Vec<AttachmentMetaDto>>,
    ) -> Self {
        Self {
            id: public_id,
            subject: value.subject,
            sender: value.sender,
            to: value.to,
            cc: value.cc,
            bcc: value.bcc,
            sent_at: value.sent_at,
            internal_date: value.internal_date,
            flags: value.flags,
            size_bytes: value.size_bytes,
            preview: value.preview,
            body_text: value.body_text,
            body_html: value.body_html,
            body_render_mode: value.body_render_mode,
            body_segments: value.body_segments.into_iter().map(Into::into).collect(),
            body_html_available: value.body_html_available,
            body_html_loaded: value.body_html_loaded,
            has_remote_images: value.has_remote_images,
            attachment_names: value.attachment_names,
            attachments,
            body_fetched: value.body_fetched,
            synced_at: value.synced_at,
        }
    }
}

impl MessageSummaryDto {
    fn from_page_item(public_id: String, value: InboxMessage) -> Self {
        Self {
            id: public_id,
            subject: value.subject,
            sender: value.sender.map(Into::into),
            to: value.to.into_iter().map(Into::into).collect(),
            cc: value.cc.into_iter().map(Into::into).collect(),
            bcc: value.bcc.into_iter().map(Into::into).collect(),
            sent_at: value.sent_at,
            internal_date: value.internal_date,
            flags: value.flags,
            size_bytes: value.size_bytes,
            preview: value.preview,
            body_html_available: value.body_html.is_some(),
            attachment_names: value.attachment_names,
            body_fetched: value.body_fetched,
            synced_at: value.synced_at,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MessagePageItemDto {
    #[serde(flatten)]
    message: MessageSummaryDto,
    displayed_role: MailboxRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_mutation: Option<PendingMessageProjection>,
}

impl From<MessagePageItem> for MessagePageItemDto {
    fn from(value: MessagePageItem) -> Self {
        Self {
            message: MessageSummaryDto::from_page_item(value.public_id, value.message),
            displayed_role: value.displayed_role,
            pending_mutation: value.pending_mutation,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MessagePageDto {
    items: Vec<MessagePageItemDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<MessagePageCursor>,
    has_more_local: bool,
    remote_history_state: RemoteHistoryState,
    end_reached: bool,
}

impl From<MessagePage> for MessagePageDto {
    fn from(value: MessagePage) -> Self {
        Self {
            items: value.items.into_iter().map(Into::into).collect(),
            next_cursor: value.next_cursor,
            has_more_local: value.has_more_local,
            remote_history_state: value.remote_history_state,
            end_reached: value.end_reached,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct MailboxUpdatedEvent {
    account_id: String,
    role: MailboxRole,
}

#[derive(Clone, Debug, Serialize)]
struct MailboxCapabilitiesUpdatedEvent {
    account_id: String,
}

pub(crate) fn validate_account_id(account_id: &str) -> CommandResult<()> {
    if account_id.is_empty()
        || account_id.chars().count() > MAX_ACCOUNT_ID_CHARS
        || account_id.chars().any(char::is_control)
    {
        return Err("The account identifier is invalid.".to_owned());
    }
    Ok(())
}

pub(crate) fn validate_message_id(message_id: &str) -> CommandResult<()> {
    if message_id.len() != 36 || message_id.chars().any(char::is_control) {
        return Err("The message identifier is invalid.".to_owned());
    }
    let parsed = uuid::Uuid::parse_str(message_id)
        .map_err(|_| "The message identifier is invalid.".to_owned())?;
    if parsed.to_string() != message_id || parsed.get_version() != Some(uuid::Version::Random) {
        return Err("The message identifier is invalid.".to_owned());
    }
    Ok(())
}

fn validate_attachment_id(attachment_id: &str) -> CommandResult<()> {
    if attachment_id.is_empty()
        || attachment_id.len() > MAX_ATTACHMENT_ID_BYTES
        || attachment_id.chars().any(char::is_control)
    {
        return Err("The attachment identifier is invalid.".to_owned());
    }
    Ok(())
}

fn is_offline_history_error(error: &MailError) -> bool {
    matches!(
        error,
        MailError::Imap(_) | MailError::Timeout { .. } | MailError::Connection(_)
    )
}

fn validate_page_role(role: MailboxRole) -> CommandResult<()> {
    if matches!(
        role,
        MailboxRole::Inbox | MailboxRole::Sent | MailboxRole::Archive | MailboxRole::Trash
    ) {
        Ok(())
    } else {
        Err("This mailbox does not use message pagination.".to_owned())
    }
}

fn validate_starred_page_role(role: MailboxRole) -> CommandResult<()> {
    if matches!(
        role,
        MailboxRole::Inbox | MailboxRole::Sent | MailboxRole::Archive
    ) {
        Ok(())
    } else {
        Err("This mailbox does not participate in the starred aggregate.".to_owned())
    }
}

fn validate_page_size(page_size: usize) -> CommandResult<()> {
    if (1..=MAX_MESSAGE_PAGE_SIZE).contains(&page_size) {
        Ok(())
    } else {
        Err(format!(
            "Message page size must be between 1 and {MAX_MESSAGE_PAGE_SIZE}."
        ))
    }
}

fn normalize_query(query: Option<String>) -> CommandResult<Option<String>> {
    let Some(query) = query.map(|value| value.trim().to_owned()) else {
        return Ok(None);
    };
    if query.is_empty() {
        return Ok(None);
    }
    if query.chars().count() > MAX_LOCAL_SEARCH_CHARS || query.chars().any(char::is_control) {
        return Err("The local mail search query is invalid.".to_owned());
    }
    Ok(Some(query))
}

fn parse_cursor(value: Option<String>, required: bool) -> CommandResult<Option<MessagePageCursor>> {
    let Some(value) = value else {
        return if required {
            Err("A continuation cursor is required.".to_owned())
        } else {
            Ok(None)
        };
    };
    if value.is_empty() || value.len() > MAX_CURSOR_BYTES || value.chars().any(char::is_control) {
        return Err("The continuation cursor is invalid or expired.".to_owned());
    }
    serde_json::from_value(serde_json::Value::String(value))
        .map(Some)
        .map_err(|_| "The continuation cursor is invalid or expired.".to_owned())
}

fn active_local_backend(
    backend: &BackendState,
) -> CommandResult<(String, std::sync::Arc<MailBackend>)> {
    let account_id = backend
        .active_account_id()
        .ok_or_else(|| "No mail account is selected.".to_owned())?;
    let local = backend.local_for(&account_id)?;
    Ok((account_id, local))
}

fn complete_mailbox_message_dto(
    backend: &MailBackend,
    public_id: String,
    message: InboxMessage,
    supplied_attachments: Option<Vec<AttachmentMeta>>,
) -> CommandResult<MailboxMessageDto> {
    let attachments = supplied_attachments
        .or_else(|| backend.cached_message_attachments(&public_id).ok())
        .map(|attachments| attachments.into_iter().map(Into::into).collect());
    Ok(MailboxMessageDto::from_full_message(
        public_id,
        full_message_dto(backend, message),
        attachments,
    ))
}

fn emit_mailbox_updated(app: &AppHandle, account_id: &str, role: MailboxRole) {
    diagnostics::emit_event(
        app,
        "mail:mailbox-updated",
        MailboxUpdatedEvent {
            account_id: account_id.to_owned(),
            role,
        },
    );
}

fn schedule_message_mutation_flush(app: &AppHandle, account_id: &str) {
    desktop::request_message_mutation_flush(app, account_id);
}

fn offline_page(mut page: MessagePage) -> MessagePage {
    if page.remote_history_state == RemoteHistoryState::MayHaveMore {
        page.remote_history_state = RemoteHistoryState::Offline;
        page.end_reached = false;
    }
    page
}

fn schedule_page_body_prefetch(backend: &BackendState, account_id: &str, page: &MessagePage) {
    let Ok(network) = backend.network_for(account_id) else {
        return;
    };
    let candidates = page
        .items
        .iter()
        .map(|item| {
            (
                item.public_id.clone(),
                item.message.size_bytes,
                item.message.body_fetched,
            )
        })
        .collect();
    network.schedule_page_body_prefetch(
        candidates,
        crate::PAGE_BODY_PREFETCH_TOTAL_BYTES,
        crate::INBOX_PREFETCH_MESSAGE_BYTES,
    );
}

fn page_with_prefetch(
    backend: &BackendState,
    account_id: &str,
    page: MessagePage,
) -> MessagePageDto {
    schedule_page_body_prefetch(backend, account_id, &page);
    page.into()
}

#[tauri::command]
pub(crate) fn get_mailbox_capabilities(
    backend: State<'_, BackendState>,
    account_id: String,
) -> CommandResult<Vec<MailboxCapabilityDto>> {
    diagnostics::command("get_mailbox_capabilities", Fields::default(), || {
        validate_account_id(&account_id)?;
        backend
            .local_for(&account_id)?
            .get_mailbox_capabilities(&account_id)
            .map(|capabilities| capabilities.into_iter().map(Into::into).collect())
            .map_err(safe_mail_error)
    })
}

#[tauri::command]
pub(crate) async fn create_mailbox_role(
    app: AppHandle,
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    account_id: String,
    role: MailboxRole,
) -> CommandResult<MailboxCapabilityDto> {
    diagnostics::command_lifecycle_async("create_mailbox_role", Fields::default(), async {
        validate_account_id(&account_id)?;
        if role != MailboxRole::Trash {
            return Err("Only Trash can be created.".to_owned());
        }
        let local = backend.local_for(&account_id)?;
        // Refreshing all OAuth-backed runtimes is account-safe: the exact backend
        // below is still selected by the caller's validated stable account ID.
        let _ = account.refresh_oauth_backends(&backend).await;
        let capability = match backend.network_for(&account_id) {
            Ok(network) => network
                .create_mailbox_role(&account_id, role)
                .await
                .map_err(safe_mail_error)?,
            Err(_) => local
                .record_mailbox_role_creation_unavailable(&account_id, role)
                .map_err(safe_mail_error)?,
        };
        let capability = MailboxCapabilityDto::from(capability);
        diagnostics::emit_event(
            &app,
            "mail:mailbox-capabilities-updated",
            MailboxCapabilitiesUpdatedEvent {
                account_id: account_id.clone(),
            },
        );
        Ok(capability)
    })
    .await
}

#[tauri::command]
pub(crate) async fn list_archive_folder_candidates(
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    account_id: String,
) -> CommandResult<Vec<ArchiveFolderCandidateDto>> {
    diagnostics::command_async("list_archive_folder_candidates", Fields::default(), async {
        validate_account_id(&account_id)?;
        let _ = account.refresh_oauth_backends(&backend).await;
        let candidates = backend
            .network_for(&account_id)?
            .list_archive_folder_candidates(&account_id)
            .await
            .map_err(safe_mail_error)?;
        backend.clear_archive_folder_selections(&account_id)?;
        candidates
            .into_iter()
            .map(|candidate| {
                backend
                    .register_archive_folder_selection(&account_id, candidate.mailbox_name)
                    .map(|selection_id| ArchiveFolderCandidateDto {
                        selection_id,
                        display_name: candidate.display_name,
                    })
            })
            .collect()
    })
    .await
}

#[tauri::command]
pub(crate) async fn assign_archive_folder(
    app: AppHandle,
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    account_id: String,
    selection_id: String,
) -> CommandResult<MailboxCapabilityDto> {
    diagnostics::command_lifecycle_async("assign_archive_folder", Fields::default(), async {
        validate_account_id(&account_id)?;
        validate_message_id(&selection_id)
            .map_err(|_| "The Archive folder choice is invalid or expired.".to_owned())?;
        let mailbox_name = backend.resolve_archive_folder_selection(&account_id, &selection_id)?;
        let _ = account.refresh_oauth_backends(&backend).await;
        let capability = backend
            .network_for(&account_id)?
            .assign_archive_folder(&account_id, &mailbox_name)
            .await
            .map_err(safe_mail_error)?;
        backend.clear_archive_folder_selections(&account_id)?;
        let capability = MailboxCapabilityDto::from(capability);
        diagnostics::emit_event(
            &app,
            "mail:mailbox-capabilities-updated",
            MailboxCapabilitiesUpdatedEvent {
                account_id: account_id.clone(),
            },
        );
        Ok(capability)
    })
    .await
}

#[tauri::command]
pub(crate) async fn list_mailbox_page(
    backend: State<'_, BackendState>,
    account_id: String,
    role: MailboxRole,
    cursor: Option<String>,
    page_size: usize,
    query: Option<String>,
) -> CommandResult<MessagePageDto> {
    diagnostics::command_async("list_mailbox_page", Fields::default(), async {
        validate_account_id(&account_id)?;
        validate_page_role(role)?;
        validate_page_size(page_size)?;
        let cursor = parse_cursor(cursor, false)?;
        let query = normalize_query(query)?;
        let page = backend
            .local_for(&account_id)?
            .list_mailbox_page(
                &account_id,
                role,
                cursor.as_ref(),
                page_size,
                query.as_deref(),
            )
            .map_err(safe_mail_error)?;
        Ok(page_with_prefetch(&backend, &account_id, page))
    })
    .await
}

#[tauri::command]
pub(crate) async fn load_older_mailbox_page(
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    account_id: String,
    role: MailboxRole,
    cursor: String,
    page_size: usize,
    query: Option<String>,
) -> CommandResult<MessagePageDto> {
    diagnostics::command_async("load_older_mailbox_page", Fields::default(), async {
        validate_account_id(&account_id)?;
        validate_page_role(role)?;
        validate_page_size(page_size)?;
        let cursor = parse_cursor(Some(cursor), true)?.expect("required cursor was validated");
        let query = normalize_query(query)?;
        let local = backend
            .local_for(&account_id)?
            .list_mailbox_page(
                &account_id,
                role,
                Some(&cursor),
                page_size,
                query.as_deref(),
            )
            .map_err(safe_mail_error)?;
        if local.has_more_local
            || local.remote_history_state != RemoteHistoryState::MayHaveMore
            || query.is_some()
        {
            return Ok(page_with_prefetch(&backend, &account_id, local));
        }

        let _ = account.refresh_oauth_backends(&backend).await;
        let Ok(network) = backend.network_for(&account_id) else {
            return Ok(page_with_prefetch(
                &backend,
                &account_id,
                offline_page(local),
            ));
        };
        match network
            .load_older_mailbox_page(&account_id, role, &cursor, page_size, query.as_deref())
            .await
        {
            Ok(page) => Ok(page_with_prefetch(&backend, &account_id, page)),
            Err(error) if is_offline_history_error(&error) => Ok(page_with_prefetch(
                &backend,
                &account_id,
                offline_page(local),
            )),
            Err(error) => Err(safe_mail_error(error)),
        }
    })
    .await
}

#[tauri::command]
pub(crate) async fn list_starred_mailbox_page(
    backend: State<'_, BackendState>,
    account_id: String,
    role: MailboxRole,
    cursor: Option<String>,
    page_size: usize,
    query: Option<String>,
) -> CommandResult<MessagePageDto> {
    diagnostics::command_async("list_starred_mailbox_page", Fields::default(), async {
        validate_account_id(&account_id)?;
        validate_starred_page_role(role)?;
        validate_page_size(page_size)?;
        let cursor = parse_cursor(cursor, false)?;
        let query = normalize_query(query)?;
        let page = backend
            .local_for(&account_id)?
            .list_starred_mailbox_page(
                &account_id,
                role,
                cursor.as_ref(),
                page_size,
                query.as_deref(),
            )
            .map_err(safe_mail_error)?;
        Ok(page_with_prefetch(&backend, &account_id, page))
    })
    .await
}

#[tauri::command]
pub(crate) async fn load_older_starred_mailbox_page(
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    account_id: String,
    role: MailboxRole,
    cursor: String,
    page_size: usize,
    query: Option<String>,
) -> CommandResult<MessagePageDto> {
    diagnostics::command_async(
        "load_older_starred_mailbox_page",
        Fields::default(),
        async {
            validate_account_id(&account_id)?;
            validate_starred_page_role(role)?;
            validate_page_size(page_size)?;
            let cursor = parse_cursor(Some(cursor), true)?.expect("required cursor was validated");
            let query = normalize_query(query)?;
            let local = backend
                .local_for(&account_id)?
                .list_starred_mailbox_page(
                    &account_id,
                    role,
                    Some(&cursor),
                    page_size,
                    query.as_deref(),
                )
                .map_err(safe_mail_error)?;
            if local.has_more_local
                || local.remote_history_state != RemoteHistoryState::MayHaveMore
                || query.is_some()
            {
                return Ok(page_with_prefetch(&backend, &account_id, local));
            }

            let _ = account.refresh_oauth_backends(&backend).await;
            let Ok(network) = backend.network_for(&account_id) else {
                return Ok(page_with_prefetch(
                    &backend,
                    &account_id,
                    offline_page(local),
                ));
            };
            match network
                .load_older_starred_mailbox_page(
                    &account_id,
                    role,
                    &cursor,
                    page_size,
                    query.as_deref(),
                )
                .await
            {
                Ok(page) => Ok(page_with_prefetch(&backend, &account_id, page)),
                Err(error) if is_offline_history_error(&error) => Ok(page_with_prefetch(
                    &backend,
                    &account_id,
                    offline_page(local),
                )),
                Err(error) => Err(safe_mail_error(error)),
            }
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn sync_mailbox(
    app: AppHandle,
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    account_id: String,
    role: MailboxRole,
) -> CommandResult<MailboxSyncReportDto> {
    diagnostics::command_async("sync_mailbox", Fields::default(), async {
        let started = Instant::now();
        validate_account_id(&account_id)?;
        backend.local_for(&account_id)?;
        let role_name = mailbox_role_name(role);
        let operation_id = diagnostics::operation_id();
        diagnostics::info(
            "mailbox_sync_started",
            Fields::default()
                .operation_id(operation_id.clone())
                .account(&account_id)
                .operation("mailbox_sync")
                .mode(role_name),
        );

        let runtime = app.state::<desktop::DesktopRuntime>();
        let access_started = Instant::now();
        let _access_guard = runtime.acquire_sync_access().await;
        diagnostics::info(
            "mailbox_sync_stage_completed",
            Fields::default()
                .operation_id(operation_id.clone())
                .account(&account_id)
                .operation("lifecycle_wait")
                .mode(role_name)
                .duration(access_started.elapsed()),
        );

        let oauth_started = Instant::now();
        let oauth_result = account
            .refresh_oauth_backend_for(&backend, &account_id)
            .await;
        diagnostics::info(
            "mailbox_sync_stage_completed",
            Fields::default()
                .operation_id(operation_id.clone())
                .account(&account_id)
                .operation("oauth_refresh_check")
                .mode(role_name)
                .outcome(if oauth_result.is_ok() {
                    "completed"
                } else {
                    "degraded"
                })
                .duration(oauth_started.elapsed()),
        );
        let network = backend.network_for(&account_id)?;
        let backend_started = Instant::now();
        let sync_result = match role {
            MailboxRole::Inbox => desktop::perform_inbox_mailbox_sync(&app, &account_id)
                .await
                .map(|report| (report.fetched, report.removed, report.uid_validity_reset))
                .map_err(|message| (message, diagnostics::ErrorKind::Runtime)),
            _ => network
                .sync_mailbox(&account_id, role)
                .await
                .map(|synced| (synced, 0, false))
                .map_err(|error| {
                    let kind = diagnostics::mail_error_kind(&error);
                    (safe_mail_error(error), kind)
                }),
        };
        let (synced, removed, uid_validity_reset) = match sync_result {
            Ok(report) => report,
            Err((message, error_kind)) => {
                diagnostics::error(
                    "mailbox_sync_completed",
                    Fields::default()
                        .operation_id(operation_id)
                        .account(&account_id)
                        .operation("mailbox_sync")
                        .mode(role_name)
                        .outcome("failed")
                        .error(error_kind)
                        .duration(started.elapsed()),
                );
                return Err(message);
            }
        };
        diagnostics::info(
            "mailbox_sync_stage_completed",
            Fields::default()
                .operation_id(operation_id.clone())
                .account(&account_id)
                .operation("backend_sync")
                .mode(role_name)
                .outcome("completed")
                .duration(backend_started.elapsed()),
        );
        // Inbox synchronization already emits its start, persisted-batch, and
        // authoritative terminal event. A second generic event would schedule
        // another identical SQLite projection read in React.
        if role != MailboxRole::Inbox {
            emit_mailbox_updated(&app, &account_id, role);
        }
        diagnostics::info(
            "mailbox_sync_completed",
            Fields::default()
                .operation_id(operation_id)
                .account(&account_id)
                .operation("mailbox_sync")
                .mode(role_name)
                .outcome("completed")
                .duration(started.elapsed()),
        );
        Ok(MailboxSyncReportDto {
            synced,
            removed,
            uid_validity_reset,
        })
    })
    .await
}

fn mailbox_role_name(role: MailboxRole) -> &'static str {
    match role {
        MailboxRole::Inbox => "inbox",
        MailboxRole::Sent => "sent",
        MailboxRole::Drafts => "drafts",
        MailboxRole::Archive => "archive",
        MailboxRole::Trash => "trash",
    }
}

async fn cached_message_or_refresh_owner<T, E, Cache, Refresh, RefreshFuture>(
    account_id: &str,
    cached_message: Cache,
    refresh_owner: Refresh,
) -> Option<T>
where
    Cache: FnOnce() -> Result<T, E>,
    Refresh: FnOnce(String) -> RefreshFuture,
    RefreshFuture: Future<Output = ()>,
{
    match cached_message() {
        Ok(message) => Some(message),
        Err(_) => {
            refresh_owner(account_id.to_owned()).await;
            None
        }
    }
}

#[tauri::command]
pub(crate) async fn fetch_mailbox_message(
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    message_id: String,
) -> CommandResult<MailboxMessageDto> {
    diagnostics::command_async("fetch_mailbox_message", Fields::default(), async {
        validate_message_id(&message_id)?;
        let (account_id, local) = active_local_backend(&backend)?;
        let account_runtime: &AccountRuntime = &account;
        let backend_state: &BackendState = &backend;
        if let Some(message) = cached_message_or_refresh_owner(
            &account_id,
            || local.cached_message_by_id(&message_id),
            |owner_account_id| async move {
                let _ = account_runtime
                    .refresh_oauth_backend_for(backend_state, &owner_account_id)
                    .await;
            },
        )
        .await
        {
            return complete_mailbox_message_dto(&local, message_id, message, None);
        }
        if let Ok(network) = backend.network_for(&account_id) {
            network.promote_body_prefetch_for_selection(&message_id);
            match network.fetch_message_view_by_id(&message_id, false).await {
                Ok((message, attachments)) => {
                    diagnostics::limited_recovery(
                        "message_fetch_network_failed",
                        "message_fetch_network_recovered",
                        "fetch_mailbox_message",
                        Some(&account_id),
                    );
                    return complete_mailbox_message_dto(
                        &network,
                        message_id,
                        message,
                        Some(attachments),
                    );
                }
                Err(network_error) => {
                    diagnostics::limited_failure(
                        "message_fetch_network_failed",
                        "fetch_mailbox_message",
                        Some(&account_id),
                        diagnostics::mail_error_kind(&network_error),
                    );
                    if let Ok(message) = local.cached_message_by_id(&message_id) {
                        return complete_mailbox_message_dto(&local, message_id, message, None);
                    }
                    return Err(safe_mail_error(network_error));
                }
            }
        } else {
            diagnostics::limited_failure(
                "message_fetch_network_failed",
                "fetch_mailbox_message",
                Some(&account_id),
                diagnostics::ErrorKind::Runtime,
            );
        }
        local
            .cached_message_by_id(&message_id)
            .map_err(safe_mail_error)
            .and_then(|message| complete_mailbox_message_dto(&local, message_id, message, None))
    })
    .await
}

#[tauri::command]
pub(crate) async fn save_message_attachment(
    app: AppHandle,
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    message_id: String,
    attachment_id: String,
) -> CommandResult<AttachmentSaveResultDto> {
    diagnostics::command_lifecycle_async("save_message_attachment", Fields::default(), async {
        validate_message_id(&message_id)?;
        validate_attachment_id(&attachment_id)?;
        let (account_id, local) = active_local_backend(&backend)?;
        let _ = account.refresh_oauth_backends(&backend).await;

        let mut source = local.clone();
        let attachments = if let Ok(network) = backend.network_for(&account_id) {
            match network.message_attachments(&message_id).await {
                Ok(attachments) => {
                    diagnostics::limited_recovery(
                        "attachment_fetch_network_failed",
                        "attachment_fetch_network_recovered",
                        "save_message_attachment",
                        Some(&account_id),
                    );
                    source = network;
                    attachments
                }
                Err(error) => {
                    diagnostics::limited_failure(
                        "attachment_fetch_network_failed",
                        "save_message_attachment",
                        Some(&account_id),
                        diagnostics::mail_error_kind(&error),
                    );
                    match local.cached_message_attachments(&message_id) {
                        Ok(attachments) => attachments,
                        Err(_) => {
                            return finish_attachment_save(
                                &account_id,
                                &attachment_id,
                                AttachmentSaveResultDto::error(
                                    AttachmentSaveErrorKind::MessageUnavailable,
                                    true,
                                ),
                            );
                        }
                    }
                }
            }
        } else {
            diagnostics::limited_failure(
                "attachment_fetch_network_failed",
                "save_message_attachment",
                Some(&account_id),
                diagnostics::ErrorKind::Runtime,
            );
            match local.cached_message_attachments(&message_id) {
                Ok(attachments) => attachments,
                Err(_) => {
                    return finish_attachment_save(
                        &account_id,
                        &attachment_id,
                        AttachmentSaveResultDto::error(
                            AttachmentSaveErrorKind::MessageUnavailable,
                            true,
                        ),
                    );
                }
            }
        };
        let Some(metadata) = attachments
            .iter()
            .find(|attachment| attachment.id == attachment_id)
        else {
            return finish_attachment_save(
                &account_id,
                &attachment_id,
                AttachmentSaveResultDto::error(AttachmentSaveErrorKind::AttachmentNotFound, false),
            );
        };

        let selected = app
            .dialog()
            .file()
            .set_file_name(&metadata.safe_display_name)
            .blocking_save_file();
        let selected_path = match selected {
            Some(path) => match path.into_path() {
                Ok(path) => Some(path),
                Err(_) => {
                    return finish_attachment_save(
                        &account_id,
                        &attachment_id,
                        AttachmentSaveResultDto::error(AttachmentSaveErrorKind::WriteFailed, true),
                    );
                }
            },
            None => None,
        };
        match source
            .save_message_attachment_to(&message_id, &attachment_id, selected_path.as_deref())
            .await
        {
            Ok(result) => finish_attachment_save(&account_id, &attachment_id, result.into()),
            Err(_) => finish_attachment_save(
                &account_id,
                &attachment_id,
                AttachmentSaveResultDto::error(AttachmentSaveErrorKind::MessageUnavailable, true),
            ),
        }
    })
    .await
}

fn should_retry_forward_from_cache(outcome: &ForwardPreparationOutcome) -> bool {
    matches!(
        outcome,
        ForwardPreparationOutcome::Error { error }
            if matches!(
                error.kind,
                ForwardPreparationErrorKind::MessageUnavailable
                    | ForwardPreparationErrorKind::BodyUnavailable
            )
    )
}

fn attachment_save_error_name(kind: AttachmentSaveErrorKind) -> &'static str {
    match kind {
        AttachmentSaveErrorKind::MessageUnavailable => "message_unavailable",
        AttachmentSaveErrorKind::AttachmentNotFound => "attachment_not_found",
        AttachmentSaveErrorKind::PermissionDenied => "permission_denied",
        AttachmentSaveErrorKind::DiskFull => "disk_full",
        AttachmentSaveErrorKind::WriteFailed => "write_failed",
    }
}

fn attachment_save_error_category(kind: AttachmentSaveErrorKind) -> diagnostics::ErrorKind {
    match kind {
        AttachmentSaveErrorKind::MessageUnavailable
        | AttachmentSaveErrorKind::AttachmentNotFound => diagnostics::ErrorKind::NotFound,
        AttachmentSaveErrorKind::PermissionDenied
        | AttachmentSaveErrorKind::DiskFull
        | AttachmentSaveErrorKind::WriteFailed => diagnostics::ErrorKind::Io,
    }
}

fn finish_attachment_save(
    account_id: &str,
    attachment_id: &str,
    result: AttachmentSaveResultDto,
) -> CommandResult<AttachmentSaveResultDto> {
    let fields = Fields::default()
        .account(account_id)
        .item("attachment", attachment_id)
        .operation("save_message_attachment");
    match result.status {
        AttachmentSaveStatus::Saved => {
            diagnostics::info("attachment_save_completed", fields.outcome("saved"))
        }
        AttachmentSaveStatus::Canceled => {
            diagnostics::info("attachment_save_completed", fields.outcome("cancelled"))
        }
        AttachmentSaveStatus::Error => {
            let (outcome, error_kind) =
                result
                    .error_kind
                    .map_or(("unknown", diagnostics::ErrorKind::Runtime), |kind| {
                        (
                            attachment_save_error_name(kind),
                            attachment_save_error_category(kind),
                        )
                    });
            diagnostics::error(
                "attachment_save_failed",
                fields.outcome(outcome).error(error_kind),
            );
        }
    }
    Ok(result)
}

fn forward_error_name(kind: ForwardPreparationErrorKind) -> &'static str {
    match kind {
        ForwardPreparationErrorKind::MessageUnavailable => "message_unavailable",
        ForwardPreparationErrorKind::BodyUnavailable => "body_unavailable",
        ForwardPreparationErrorKind::AttachmentUnavailable => "attachment_unavailable",
        ForwardPreparationErrorKind::AttachmentStageFailed => "attachment_stage_failed",
        ForwardPreparationErrorKind::SourceChanged => "source_changed",
    }
}

fn forward_error_category(kind: ForwardPreparationErrorKind) -> diagnostics::ErrorKind {
    match kind {
        ForwardPreparationErrorKind::MessageUnavailable
        | ForwardPreparationErrorKind::BodyUnavailable
        | ForwardPreparationErrorKind::AttachmentUnavailable => diagnostics::ErrorKind::NotFound,
        ForwardPreparationErrorKind::AttachmentStageFailed => diagnostics::ErrorKind::Io,
        ForwardPreparationErrorKind::SourceChanged => diagnostics::ErrorKind::Validation,
    }
}

fn finish_forward_preparation(
    account_id: &str,
    message_id: &str,
    outcome: ForwardPreparationOutcomeDto,
) -> CommandResult<ForwardPreparationOutcomeDto> {
    let fields = Fields::default()
        .account(account_id)
        .item("message", message_id)
        .operation("prepare_forward");
    match &outcome {
        ForwardPreparationOutcomeDto::Prepared { .. } => {
            diagnostics::info("forward_preparation_completed", fields.outcome("prepared"))
        }
        ForwardPreparationOutcomeDto::Error { error } => diagnostics::error(
            "forward_preparation_failed",
            fields
                .outcome(forward_error_name(error.kind))
                .error(forward_error_category(error.kind)),
        ),
    }
    Ok(outcome)
}

#[tauri::command]
pub(crate) async fn prepare_forward(
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    message_id: String,
    include_attachments: bool,
) -> CommandResult<ForwardPreparationOutcomeDto> {
    diagnostics::command_async("prepare_forward", Fields::default(), async {
        validate_message_id(&message_id)?;
        let (account_id, local) = active_local_backend(&backend)?;
        let _ = account.refresh_oauth_backends(&backend).await;
        if let Ok(network) = backend.network_for(&account_id) {
            match network
                .prepare_forward(&message_id, include_attachments)
                .await
            {
                Ok(outcome) if !should_retry_forward_from_cache(&outcome) => {
                    diagnostics::limited_recovery(
                        "forward_network_fallback",
                        "forward_network_recovered",
                        "prepare_forward",
                        Some(&account_id),
                    );
                    return finish_forward_preparation(&account_id, &message_id, outcome.into());
                }
                Ok(outcome) => {
                    let error_kind = match outcome {
                        ForwardPreparationOutcome::Error { error } => {
                            forward_error_category(error.kind)
                        }
                        ForwardPreparationOutcome::Prepared { .. } => {
                            diagnostics::ErrorKind::Runtime
                        }
                    };
                    diagnostics::limited_failure(
                        "forward_network_fallback",
                        "prepare_forward",
                        Some(&account_id),
                        error_kind,
                    );
                }
                Err(error) => diagnostics::limited_failure(
                    "forward_network_fallback",
                    "prepare_forward",
                    Some(&account_id),
                    diagnostics::mail_error_kind(&error),
                ),
            }
        } else {
            diagnostics::limited_failure(
                "forward_network_fallback",
                "prepare_forward",
                Some(&account_id),
                diagnostics::ErrorKind::Runtime,
            );
        }
        match local
            .prepare_forward(&message_id, include_attachments)
            .await
        {
            Ok(outcome) => finish_forward_preparation(&account_id, &message_id, outcome.into()),
            Err(_) => finish_forward_preparation(
                &account_id,
                &message_id,
                ForwardPreparationOutcomeDto::error(
                    ForwardPreparationErrorKind::MessageUnavailable,
                    Vec::new(),
                    false,
                ),
            ),
        }
    })
    .await
}

#[tauri::command]
pub(crate) fn set_message_seen(
    app: AppHandle,
    backend: State<'_, BackendState>,
    message_id: String,
    seen: bool,
) -> CommandResult<SystemFlagMutationReceipt> {
    diagnostics::command("set_message_seen", Fields::default(), || {
        validate_message_id(&message_id)?;
        let (account_id, local) = active_local_backend(&backend)?;
        let receipt = local
            .set_message_seen(&message_id, seen)
            .map_err(safe_mail_error)?;
        diagnostics::info(
            "message_mutation_queued",
            Fields::default()
                .account(&account_id)
                .item("message", &message_id)
                .operation("seen_mutation")
                .outcome(if seen { "seen" } else { "unseen" }),
        );
        schedule_message_mutation_flush(&app, &account_id);
        Ok(receipt)
    })
}

#[tauri::command]
pub(crate) fn set_message_starred_by_id(
    app: AppHandle,
    backend: State<'_, BackendState>,
    message_id: String,
    starred: bool,
) -> CommandResult<SystemFlagMutationReceipt> {
    diagnostics::command("set_message_starred_by_id", Fields::default(), || {
        validate_message_id(&message_id)?;
        let (account_id, local) = active_local_backend(&backend)?;
        let receipt = local
            .set_message_starred_by_id(&message_id, starred)
            .map_err(safe_mail_error)?;
        diagnostics::info(
            "message_mutation_queued",
            Fields::default()
                .account(&account_id)
                .item("message", &message_id)
                .operation("flagged_mutation")
                .outcome(if starred { "starred" } else { "unstarred" }),
        );
        schedule_message_mutation_flush(&app, &account_id);
        Ok(receipt)
    })
}

#[tauri::command]
pub(crate) fn archive_message(
    app: AppHandle,
    backend: State<'_, BackendState>,
    message_id: String,
) -> CommandResult<MessageMutationReceipt> {
    diagnostics::command("archive_message", Fields::default(), || {
        validate_message_id(&message_id)?;
        let (account_id, local) = active_local_backend(&backend)?;
        let receipt = local
            .archive_message(&message_id)
            .map_err(safe_mail_error)?;
        diagnostics::info(
            "message_mutation_queued",
            Fields::default()
                .account(&account_id)
                .item("message", &message_id)
                .operation("archive_message")
                .outcome("queued"),
        );
        schedule_message_mutation_flush(&app, &account_id);
        Ok(receipt)
    })
}

#[tauri::command]
pub(crate) fn move_message_to_trash(
    app: AppHandle,
    backend: State<'_, BackendState>,
    message_id: String,
) -> CommandResult<MessageMutationReceipt> {
    diagnostics::command("move_message_to_trash", Fields::default(), || {
        validate_message_id(&message_id)?;
        let (account_id, local) = active_local_backend(&backend)?;
        let receipt = local
            .move_message_to_trash(&message_id)
            .map_err(safe_mail_error)?;
        diagnostics::info(
            "message_mutation_queued",
            Fields::default()
                .account(&account_id)
                .item("message", &message_id)
                .operation("move_message_to_trash")
                .outcome("queued"),
        );
        schedule_message_mutation_flush(&app, &account_id);
        Ok(receipt)
    })
}

#[tauri::command]
pub(crate) fn move_message_to_inbox(
    app: AppHandle,
    backend: State<'_, BackendState>,
    message_id: String,
) -> CommandResult<MessageMutationReceipt> {
    diagnostics::command("move_message_to_inbox", Fields::default(), || {
        validate_message_id(&message_id)?;
        let (account_id, local) = active_local_backend(&backend)?;
        let receipt = local
            .move_message_to_inbox(&message_id)
            .map_err(safe_mail_error)?;
        diagnostics::info(
            "message_mutation_queued",
            Fields::default()
                .account(&account_id)
                .item("message", &message_id)
                .operation("move_message_to_inbox")
                .outcome("queued"),
        );
        schedule_message_mutation_flush(&app, &account_id);
        Ok(receipt)
    })
}

#[tauri::command]
pub(crate) async fn prepare_permanent_delete(
    backend: State<'_, BackendState>,
    message_id: String,
) -> CommandResult<PermanentDeletePlan> {
    diagnostics::command_async("prepare_permanent_delete", Fields::default(), async {
        validate_message_id(&message_id)?;
        let (_, local) = active_local_backend(&backend)?;
        local
            .prepare_permanent_delete(&message_id)
            .await
            .map_err(safe_mail_error)
    })
    .await
}

#[tauri::command]
pub(crate) async fn confirm_permanent_delete(
    app: AppHandle,
    backend: State<'_, BackendState>,
    plan_id: String,
) -> CommandResult<MessageMutationReceipt> {
    diagnostics::command_lifecycle_async("confirm_permanent_delete", Fields::default(), async {
        if plan_id.is_empty() || plan_id.len() > 128 || plan_id.chars().any(char::is_control) {
            return Err("The permanent-delete plan is invalid or expired.".to_owned());
        }
        let (account_id, local) = active_local_backend(&backend)?;
        let receipt = local
            .confirm_permanent_delete(&plan_id)
            .await
            .map_err(safe_mail_error)?;
        diagnostics::info(
            "message_mutation_queued",
            Fields::default()
                .account(&account_id)
                .item("delete_plan", &plan_id)
                .operation("permanent_delete")
                .outcome("queued"),
        );
        schedule_message_mutation_flush(&app, &account_id);
        Ok(receipt)
    })
    .await
}

#[cfg(test)]
mod tests {
    use mine_mail::{
        AttachmentDisposition, AttachmentMeta, InboxMessage, MailAddress, MailboxCapability,
        MailboxCapabilityStatus, MailboxRole, MessageActionKind, MessageMutationErrorKind,
        MessageMutationReceipt, MessagePage, MessagePageItem, MutationStatus,
        PendingMessageProjection, PermanentDeletePlan, RemoteHistoryState, SystemFlagKind,
        SystemFlagMutationReceipt,
    };

    use super::{
        ArchiveFolderCandidateDto, MailboxBodySegmentDto, MailboxCapabilityDto, MailboxMessageDto,
        MailboxSyncReportDto, MessagePageDto, cached_message_or_refresh_owner,
        is_offline_history_error, normalize_query, offline_page, parse_cursor, validate_account_id,
        validate_attachment_id, validate_message_id, validate_page_role, validate_page_size,
        validate_starred_page_role,
    };
    use crate::{
        AttachmentMetaDto, BodyRenderMode, BodySegmentConfidenceDto, BodySegmentDto,
        BodySegmentKindDto, InboxMessageDto, MessageNavigationTargetDto,
        assert_no_private_mail_coordinates,
    };

    fn summary() -> InboxMessage {
        InboxMessage {
            id: 7,
            account_id: "account-private".to_owned(),
            mailbox: "INBOX".to_owned(),
            uid: 42,
            message_id: Some("<safe@example.com>".to_owned()),
            in_reply_to: Vec::new(),
            references: Vec::new(),
            subject: "Bounded summary".to_owned(),
            sender: Some(MailAddress {
                name: Some("Sender".to_owned()),
                email: "sender@example.com".to_owned(),
            }),
            to: Vec::new(),
            cc: Vec::new(),
            bcc: vec![MailAddress {
                name: Some("Hidden recipient".to_owned()),
                email: "hidden@example.com".to_owned(),
            }],
            sent_at: None,
            internal_date: None,
            flags: Vec::new(),
            size_bytes: 123,
            preview: "bounded preview".to_owned(),
            body_text: Some("must not cross a list boundary".to_owned()),
            body_html: Some("<p>must not cross</p>".to_owned()),
            attachment_names: Vec::new(),
            body_fetched: true,
            raw_rfc822: b"must not cross".to_vec(),
            synced_at: "2026-07-28T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn command_validation_is_strict_and_bounded() {
        assert!(validate_account_id("account-a").is_ok());
        assert!(validate_account_id("").is_err());
        assert!(validate_account_id("bad\naccount").is_err());
        assert!(validate_message_id("9f1a7b32-4b55-4d6d-8db7-0e7bf1a32c41").is_ok());
        assert!(validate_message_id("").is_err());
        assert!(validate_message_id("9F1A7B32-4B55-4D6D-8DB7-0E7BF1A32C41").is_err());
        assert!(validate_attachment_id("opaque-attachment").is_ok());
        assert!(validate_attachment_id("").is_err());
        assert!(validate_attachment_id(&"a".repeat(257)).is_err());
        assert!(validate_attachment_id("bad\nattachment").is_err());
        assert!(validate_page_size(1).is_ok());
        assert!(validate_page_size(100).is_ok());
        assert!(validate_page_size(0).is_err());
        assert!(validate_page_size(101).is_err());
        assert!(validate_page_role(MailboxRole::Inbox).is_ok());
        assert!(validate_page_role(MailboxRole::Trash).is_ok());
        assert!(validate_page_role(MailboxRole::Drafts).is_err());
        assert!(validate_starred_page_role(MailboxRole::Inbox).is_ok());
        assert!(validate_starred_page_role(MailboxRole::Sent).is_ok());
        assert!(validate_starred_page_role(MailboxRole::Archive).is_ok());
        assert!(validate_starred_page_role(MailboxRole::Trash).is_err());
        assert!(
            normalize_query(Some("  needle  ".to_owned()))
                .is_ok_and(|query| query.as_deref() == Some("needle"))
        );
        assert!(normalize_query(Some("x".repeat(257))).is_err());
        assert!(normalize_query(Some("bad\nquery".to_owned())).is_err());
        assert!(parse_cursor(None, false).is_ok_and(|cursor| cursor.is_none()));
        assert!(parse_cursor(None, true).is_err());
        assert!(parse_cursor(Some("x".repeat(65)), true).is_err());
    }

    #[tokio::test]
    async fn cached_reader_messages_do_not_enter_authentication_path() {
        for authentication in ["password", "google-oauth-with-unavailable-authorization"] {
            let message = cached_message_or_refresh_owner(
                "account-owner",
                || Ok::<_, &'static str>(authentication),
                |_| async {
                    panic!("a complete cached body must not refresh credentials");
                },
            )
            .await;

            assert_eq!(message, Some(authentication));
        }
    }

    #[tokio::test]
    async fn cache_miss_refreshes_only_the_owning_account() {
        let refreshed_accounts = std::cell::RefCell::new(Vec::new());
        let refreshed_accounts_ref = &refreshed_accounts;
        let message = cached_message_or_refresh_owner(
            "account-owner",
            || Err::<(), _>("cached body is incomplete"),
            |account_id| async move {
                refreshed_accounts_ref.borrow_mut().push(account_id);
            },
        )
        .await;

        assert!(message.is_none());
        assert_eq!(
            refreshed_accounts.into_inner(),
            vec!["account-owner".to_owned()]
        );
    }

    #[test]
    fn mutation_receipts_and_delete_plans_expose_only_semantic_state() {
        let flag_receipt = serde_json::to_value(SystemFlagMutationReceipt {
            operation_id: "operation-1".to_owned(),
            local_revision: 4,
            status: MutationStatus::Pending,
            source_role: MailboxRole::Inbox,
            flag: SystemFlagKind::Seen,
            desired: true,
        })
        .expect("serialize flag receipt");
        let move_receipt = serde_json::to_value(MessageMutationReceipt {
            operation_id: "operation-2".to_owned(),
            local_revision: 5,
            status: MutationStatus::Pending,
            source_role: MailboxRole::Inbox,
            destination_role: Some(MailboxRole::Archive),
        })
        .expect("serialize move receipt");
        let delete_plan = serde_json::to_value(PermanentDeletePlan {
            plan_id: "opaque-delete-plan".to_owned(),
            expires_at: "2026-07-28T00:01:00Z".to_owned(),
        })
        .expect("serialize delete plan");

        assert_eq!(flag_receipt["source_role"], "inbox");
        assert_eq!(move_receipt["destination_role"], "archive");
        assert_eq!(delete_plan["plan_id"], "opaque-delete-plan");
        assert_no_private_mail_coordinates(&flag_receipt);
        assert_no_private_mail_coordinates(&move_receipt);
        assert_no_private_mail_coordinates(&delete_plan);
    }

    #[test]
    fn mailbox_page_dto_is_body_free_and_keeps_typed_mutation_state() {
        let dto = MessagePageDto::from(MessagePage {
            items: vec![MessagePageItem {
                public_id: "9f1a7b32-4b55-4d6d-8db7-0e7bf1a32c41".to_owned(),
                message: summary(),
                displayed_role: MailboxRole::Archive,
                pending_mutation: Some(PendingMessageProjection {
                    operation_id: "operation-1".to_owned(),
                    local_revision: 3,
                    status: MutationStatus::OutcomeUnknown,
                    kind: MessageActionKind::Archive,
                    source_role: MailboxRole::Inbox,
                    destination_role: MailboxRole::Archive,
                    error_kind: Some(MessageMutationErrorKind::AmbiguousRemoteState),
                }),
            }],
            next_cursor: None,
            has_more_local: false,
            remote_history_state: RemoteHistoryState::Offline,
            end_reached: false,
        });
        let json = serde_json::to_value(dto).expect("serialize message page");

        assert_eq!(
            json["items"][0]["id"],
            "9f1a7b32-4b55-4d6d-8db7-0e7bf1a32c41"
        );
        assert_ne!(json["items"][0]["id"], 7);
        assert_eq!(json["items"][0]["displayed_role"], "archive");
        assert_eq!(json["items"][0]["bcc"][0]["email"], "hidden@example.com");
        assert_eq!(
            json["items"][0]["pending_mutation"]["status"],
            "outcome_unknown"
        );
        assert_eq!(json["remote_history_state"], "offline");
        assert_eq!(json["end_reached"], false);
        assert!(json["items"][0].get("body_text").is_none());
        assert!(json["items"][0].get("body_html").is_none());
        assert!(json["items"][0].get("attachments").is_none());
        assert!(json["items"][0].get("account_id").is_none());
        assert!(json["items"][0].get("mailbox").is_none());
        assert!(json["items"][0].get("uid").is_none());
        assert!(json["items"][0].get("raw_rfc822").is_none());
        assert_no_private_mail_coordinates(&json);
    }

    #[test]
    fn offline_history_never_claims_a_confirmed_end() {
        let page = offline_page(MessagePage {
            items: Vec::new(),
            next_cursor: None,
            has_more_local: false,
            remote_history_state: RemoteHistoryState::MayHaveMore,
            end_reached: false,
        });

        assert_eq!(page.remote_history_state, RemoteHistoryState::Offline);
        assert!(!page.end_reached);
        assert!(is_offline_history_error(&mine_mail::MailError::Timeout {
            operation: "mailbox history"
        }));
        assert!(is_offline_history_error(&mine_mail::MailError::Imap(
            "privacy-safe".to_owned()
        )));
        assert!(!is_offline_history_error(
            &mine_mail::MailError::Validation("UIDVALIDITY changed".to_owned())
        ));
    }

    #[test]
    fn capability_dto_never_exposes_the_provider_mailbox_name() {
        let dto = MailboxCapabilityDto::from(MailboxCapability {
            role: MailboxRole::Archive,
            status: MailboxCapabilityStatus::Available,
            display_name: Some("[Gmail]/所有邮件".to_owned()),
            unavailable_reason: None,
            retryable: false,
        });
        let json = serde_json::to_value(dto).expect("serialize capability");

        assert_eq!(json["role"], "archive");
        assert_eq!(json["status"], "available");
        assert!(json.get("display_name").is_none());
        assert!(!json.to_string().contains("所有邮件"));
        assert_no_private_mail_coordinates(&json);
    }

    #[test]
    fn archive_candidate_dto_exposes_only_an_opaque_choice_and_safe_label() {
        let json = serde_json::to_value(ArchiveFolderCandidateDto {
            selection_id: "9f1a7b32-4b55-4d6d-8db7-0e7bf1a32c41".to_owned(),
            display_name: "其他文件夹/往年邮件".to_owned(),
        })
        .expect("serialize Archive folder candidate");

        assert_eq!(json["selectionId"], "9f1a7b32-4b55-4d6d-8db7-0e7bf1a32c41");
        assert_eq!(json["displayName"], "其他文件夹/往年邮件");
        assert!(json.get("mailbox_name").is_none());
        assert!(!json.to_string().contains("&UXZO1mWHTvZZOQ-"));
        assert_no_private_mail_coordinates(&json);
    }

    #[test]
    fn mailbox_sync_report_exposes_only_bounded_state() {
        let json = serde_json::to_value(MailboxSyncReportDto {
            synced: 4,
            removed: 2,
            uid_validity_reset: true,
        })
        .expect("serialize mailbox sync report");

        assert_eq!(json["synced"], 4);
        assert_eq!(json["removed"], 2);
        assert_eq!(json["uid_validity_reset"], true);
        assert_eq!(json.as_object().map(|fields| fields.len()), Some(3));
        assert_no_private_mail_coordinates(&json);
    }

    #[test]
    fn selected_message_dto_keeps_body_but_not_provider_identity() {
        let dto = MailboxMessageDto::from_full_message(
            "9f1a7b32-4b55-4d6d-8db7-0e7bf1a32c41".to_owned(),
            InboxMessageDto::full(summary()),
            Some(vec![AttachmentMetaDto::from(AttachmentMeta {
                id: "opaque-attachment".to_owned(),
                original_name: Some("invoice.pdf".to_owned()),
                safe_display_name: "invoice.pdf".to_owned(),
                mime_type: "application/pdf".to_owned(),
                size_bytes: 42,
                size_is_estimate: true,
                disposition: AttachmentDisposition::Attachment,
            })]),
        );
        let json = serde_json::to_value(dto).expect("serialize selected message");

        assert_eq!(json["id"], "9f1a7b32-4b55-4d6d-8db7-0e7bf1a32c41");
        assert_eq!(json["body_text"], "must not cross a list boundary");
        assert_eq!(json["bcc"][0]["email"], "hidden@example.com");
        assert!(json.get("account_id").is_none());
        assert!(json.get("mailbox").is_none());
        assert!(json.get("uid").is_none());
        assert!(json.get("raw_rfc822").is_none());
        assert_eq!(json["attachments"][0]["id"], "opaque-attachment");
        assert_eq!(json["attachments"][0]["size_bytes"], 42);
        assert_eq!(json["attachments"][0]["size_is_estimate"], true);
        assert!(json["attachments"][0].get("bytes").is_none());
        assert!(json["attachments"][0].get("path").is_none());
        assert_no_private_mail_coordinates(&json);
    }

    #[test]
    fn unavailable_attachment_index_does_not_hide_a_valid_cached_body() {
        let dto = MailboxMessageDto::from_full_message(
            "9f1a7b32-4b55-4d6d-8db7-0e7bf1a32c41".to_owned(),
            InboxMessageDto::full(summary()),
            None,
        );
        let json = serde_json::to_value(dto).expect("serialize selected message");

        assert_eq!(json["body_text"], "must not cross a list boundary");
        assert!(json["attachments"].is_null());
        assert!(json.get("raw_rfc822").is_none());
        assert_no_private_mail_coordinates(&json);
    }

    #[test]
    fn selected_message_segments_keep_only_opaque_navigation_identity() {
        let dto = MailboxBodySegmentDto::from(BodySegmentDto {
            kind: BodySegmentKindDto::Quoted,
            content: "Quoted".to_owned(),
            render_mode: BodyRenderMode::Plain,
            quote_depth: 1,
            confidence: BodySegmentConfidenceDto::High,
            quote_metadata: None,
            navigation_target: Some(MessageNavigationTargetDto {
                id: "opaque-ancestor".to_owned(),
            }),
        });
        let json = serde_json::to_value(dto).expect("serialize body segment");

        assert_eq!(json["navigation_target"]["id"], "opaque-ancestor");
        assert!(!json.to_string().contains("mailbox"));
        assert!(!json.to_string().contains("\"uid\""));
        assert_no_private_mail_coordinates(&json);
    }
}
