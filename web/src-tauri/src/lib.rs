mod account;
mod contacts;
mod desktop;
mod diagnostics;
mod mail_html;
mod mailbox_api;
mod storage;

use std::time::Instant;

use mine_mail::{
    AttachmentDisposition, AttachmentMeta, AttachmentSaveErrorKind, AttachmentSaveResult,
    AttachmentSaveStatus, ComposeFormat, ComposeRequest, ContactMessage, ContactMessageDirection,
    DeliveryUnknownDecision, Draft, DraftAttachmentMeta, DraftAttachmentMutationKind,
    DraftAttachmentMutationOutcome, DraftDeleteKind, DraftDto as CoreDraftDto, DraftSaveKind,
    DraftSaveOutcome, ForwardContext, ForwardPreparationError, ForwardPreparationErrorKind,
    ForwardPreparationOutcome, ForwardQuotedRenderMode, ForwardWarning, InboxMessage, MailAddress,
    MailBackend, MailboxRole, OutboxItem, OutboxRecipientGroups, OutboxStatus, PreparedForward,
    ReplyContext, StationeryTheme, outbox_body_html, outbox_body_text, outbox_has_reply_headers,
    outbox_message_id, outbox_preview, outbox_sent_at, outbox_subject, sanitize_compose_html,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};
use tauri_plugin_dialog::DialogExt;
use url::Url;

use account::{
    AccountPresetDto, AccountRuntime, AccountStatusDto, BackendState, ConfigureAccountRequest,
    RemoveAccountRequest, RemoveAccountResultDto,
};
use contacts::{ContactDirectoryDto, ContactRuntime};
use desktop::{
    DeleteProfileAvatarRequest, DesktopRuntime, DesktopSettingsDto, DesktopSettingsUpdate,
    NewMailNotificationDto, ProfileAvatarDto, SaveProfileAvatarRequest,
};
use diagnostics::{ErrorKind as DiagnosticErrorKind, Fields as DiagnosticFields};
use mail_html::{
    MailBodySegmentConfidence, MailBodySegmentKind, MailBodySegmentMetadata, MailHtmlStructure,
    SanitizedMailBodySegment, quote_metadata_matches_cached, sanitize_mail_html,
    segment_mail_body_with_metadata, segment_mail_body_with_metadata_chain,
};
use mailbox_api::{
    archive_message, assign_archive_folder, confirm_permanent_delete, create_mailbox_role,
    fetch_mailbox_message, get_mailbox_capabilities, list_archive_folder_candidates,
    list_mailbox_page, list_starred_mailbox_page, load_older_mailbox_page,
    load_older_starred_mailbox_page, move_message_to_inbox, move_message_to_trash,
    prepare_forward, prepare_permanent_delete, save_message_attachment, set_message_seen,
    set_message_starred_by_id, sync_mailbox,
};
use storage::{PreparedStorageMigrationDto, StorageRuntime, StorageStatusDto};

const INBOX_SYNC_LIMIT: usize = 100;
const INBOX_LIST_LIMIT: usize = 250;
const INBOX_PREFETCH_LIMIT: usize = 20;
const INBOX_PREFETCH_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
const INBOX_PREFETCH_MESSAGE_BYTES: u32 = 2 * 1024 * 1024;
const PAGE_BODY_PREFETCH_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const BODY_CACHE_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const SENT_SYNC_LIMIT: usize = 250;
const CONTACT_MESSAGE_LIST_LIMIT: usize = 250;
const MAX_OUTBOX_ID_BYTES: usize = 128;
const EXTERNAL_LINK_OPEN_FAILED_EVENT: &str = "mail:external-link-open-failed";

type CommandResult<T> = Result<T, String>;

#[cfg(test)]
fn assert_no_private_mail_coordinates(value: &serde_json::Value) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                assert_no_private_mail_coordinates(value);
            }
        }
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                assert!(
                    !matches!(
                        key.as_str(),
                        "account_id"
                            | "accountId"
                            | "mailbox"
                            | "uid"
                            | "remote_mailbox"
                            | "remoteMailbox"
                            | "remote_uid"
                            | "remoteUid"
                            | "raw_rfc822"
                            | "rawRfc822"
                            | "internal_name"
                            | "internalName"
                            | "rowid"
                            | "row_id"
                            | "rowId"
                            | "message_row_id"
                            | "messageRowId"
                            | "path"
                            | "bytes"
                    ),
                    "private mail coordinate crossed the desktop boundary: {key}"
                );
                assert!(
                    !matches!(key.as_str(), "id" | "message_id" | "messageId")
                        || !value.is_number(),
                    "numeric internal row identity crossed the desktop boundary: {key}"
                );
                assert_no_private_mail_coordinates(value);
            }
        }
        _ => {}
    }
}

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
    body_segments: Vec<BodySegmentDto>,
    body_html_available: bool,
    body_html_loaded: bool,
    has_remote_images: bool,
    attachment_names: Vec<String>,
    body_fetched: bool,
    synced_at: String,
}

#[derive(Clone, Debug, Serialize)]
struct ContactMessageDto {
    id: String,
    direction: ContactMessageDirection,
    mailbox_role: Option<MailboxRole>,
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

impl From<ContactMessage> for ContactMessageDto {
    fn from(value: ContactMessage) -> Self {
        let ContactMessage {
            public_id,
            direction,
            mailbox_role,
            message,
        } = value;
        let body_html_available = message.body_html.is_some();
        Self {
            id: public_id,
            direction,
            mailbox_role,
            subject: message.subject,
            sender: message.sender.map(Into::into),
            to: message.to.into_iter().map(Into::into).collect(),
            cc: message.cc.into_iter().map(Into::into).collect(),
            bcc: message.bcc.into_iter().map(Into::into).collect(),
            sent_at: message.sent_at,
            internal_date: message.internal_date,
            flags: message.flags,
            size_bytes: message.size_bytes,
            preview: message.preview,
            body_html_available,
            attachment_names: message.attachment_names,
            body_fetched: message.body_fetched,
            synced_at: message.synced_at,
        }
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    navigation_target: Option<MessageNavigationTargetDto>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct BodySegmentMetadataDto {
    subject: Option<String>,
    sender: Option<String>,
    recipient: Option<String>,
    sent_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct MessageNavigationTargetDto {
    id: String,
}

#[derive(Clone, Debug, Serialize)]
struct ReplyContextDto {
    parent_message_id: Option<String>,
    references: Vec<String>,
    subject: String,
    sender: Option<MailAddressDto>,
    recipients: Vec<MailAddressDto>,
    sent_at: Option<String>,
    quoted_text: String,
    quoted_html: Option<String>,
    quoted_render_mode: Option<BodyRenderMode>,
    has_remote_images: bool,
}

#[derive(Clone, Debug, Serialize)]
struct ComposeRequestDto {
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    body_text: String,
    format: ComposeFormat,
    reply_context: Option<ReplyContextDto>,
}

fn sanitize_reply_html(source: Option<&str>) -> (Option<String>, Option<BodyRenderMode>, bool) {
    let Some(source) = source.filter(|html| !html.trim().is_empty()) else {
        return (None, None, false);
    };
    let sanitized = sanitize_mail_html(source);
    let has_remote_images = sanitized.has_remote_images;
    match sanitized.structure {
        MailHtmlStructure::PlainEquivalent => (None, None, has_remote_images),
        MailHtmlStructure::Native => (
            sanitized.native_fragment.or(Some(sanitized.fragment)),
            Some(BodyRenderMode::NativeHtml),
            has_remote_images,
        ),
        MailHtmlStructure::Isolated => (
            Some(sanitized.fragment),
            Some(BodyRenderMode::IsolatedHtml),
            has_remote_images,
        ),
    }
}

impl From<ReplyContext> for ReplyContextDto {
    fn from(value: ReplyContext) -> Self {
        let (quoted_html, quoted_render_mode, has_remote_images) =
            sanitize_reply_html(value.quoted_html.as_deref());
        Self {
            parent_message_id: value.parent_message_id,
            references: value.references,
            subject: value.subject,
            sender: value.sender.map(Into::into),
            recipients: value.recipients.into_iter().map(Into::into).collect(),
            sent_at: value.sent_at,
            quoted_text: value.quoted_text,
            quoted_html,
            quoted_render_mode,
            has_remote_images,
        }
    }
}

impl From<ComposeRequest> for ComposeRequestDto {
    fn from(value: ComposeRequest) -> Self {
        Self {
            to: value.to,
            cc: value.cc,
            bcc: value.bcc,
            subject: value.subject,
            body_text: value.body_text,
            format: value.format,
            reply_context: value.reply_context.map(Into::into),
        }
    }
}

fn sanitize_compose_request(mut request: ComposeRequest) -> ComposeRequest {
    request.format.body_html = sanitize_compose_html(request.format.body_html.as_deref());
    if request.format.stationery == StationeryTheme::None {
        request.format.send_stationery = false;
    }
    if let Some(context) = request.reply_context.as_mut() {
        context.quoted_html = sanitize_reply_html(context.quoted_html.as_deref()).0;
    }
    request
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
            navigation_target: None,
        }
    }
}

fn quote_navigation_target(
    segment: &SanitizedMailBodySegment,
    metadata_chain: &[Option<MailBodySegmentMetadata>],
    navigation_targets: &[Option<MessageNavigationTargetDto>],
) -> Option<MessageNavigationTargetDto> {
    if segment.kind != MailBodySegmentKind::Quoted || segment.quote_depth == 0 {
        return None;
    }
    let index = usize::from(segment.quote_depth - 1);
    let cached_metadata = metadata_chain.get(index)?.as_ref()?;
    let detected_metadata = segment.quote_metadata.as_ref()?;
    if !quote_metadata_matches_cached(detected_metadata, cached_metadata) {
        return None;
    }
    navigation_targets.get(index)?.clone()
}

impl InboxMessageDto {
    #[cfg(test)]
    fn summary(mut value: InboxMessage) -> Self {
        let body_html_available = value.body_html.is_some();
        // List commands expose only the bounded preview. A locally cached full
        // text body remains available through the selected-message command but
        // must not cross the desktop boundary with every list row.
        value.body_text = None;
        value.body_html = None;
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
        Self::full_with_parent(value, None)
    }

    fn full_with_parent(value: InboxMessage, parent: Option<&InboxMessage>) -> Self {
        let has_reply_headers = !value.in_reply_to.is_empty() || !value.references.is_empty();
        let metadata_chain = reply_quote_metadata(&value, parent, has_reply_headers)
            .map(Some)
            .into_iter()
            .collect::<Vec<_>>();
        // A provider mailbox/UID tuple is never an acceptable navigation
        // capability. Callers without a repository-resolved public ID leave
        // the quote informational rather than guessing a destination.
        let navigation_targets = parent.map(|_| None).into_iter().collect::<Vec<_>>();
        Self::full_with_metadata_chain(value, &metadata_chain, &navigation_targets)
    }

    #[cfg(test)]
    fn full_with_ancestors(value: InboxMessage, ancestors: &[Option<InboxMessage>]) -> Self {
        let navigation_targets = vec![None; ancestors.len()];
        Self::full_with_resolved_ancestors(value, ancestors, &navigation_targets)
    }

    fn full_with_resolved_ancestors(
        value: InboxMessage,
        ancestors: &[Option<InboxMessage>],
        navigation_targets: &[Option<MessageNavigationTargetDto>],
    ) -> Self {
        let has_reply_headers = !value.in_reply_to.is_empty() || !value.references.is_empty();
        let metadata_chain = reply_quote_metadata_chain(&value, ancestors, has_reply_headers);
        Self::full_with_metadata_chain(value, &metadata_chain, navigation_targets)
    }

    fn full_with_metadata_chain(
        value: InboxMessage,
        metadata_chain: &[Option<MailBodySegmentMetadata>],
        navigation_targets: &[Option<MessageNavigationTargetDto>],
    ) -> Self {
        // MIME extraction (including safe CID image resolution) already ran
        // when the body entered SQLite. Re-parsing raw RFC822 on every click
        // made cached HTML feel like a network operation.
        let body_html_available = value.body_html.is_some();
        let has_readable_text = value
            .body_text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty());
        let has_reply_headers = !value.in_reply_to.is_empty() || !value.references.is_empty();
        let body_segments = segment_mail_body_with_metadata_chain(
            value.body_text.as_deref(),
            value.body_html.as_deref(),
            has_reply_headers,
            metadata_chain,
        )
        .into_iter()
        .map(|segment| {
            let navigation_target =
                quote_navigation_target(&segment, metadata_chain, navigation_targets);
            let mut dto = BodySegmentDto::from(segment);
            dto.navigation_target = navigation_target;
            dto
        })
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

fn reply_quote_metadata(
    message: &InboxMessage,
    parent: Option<&InboxMessage>,
    has_reply_headers: bool,
) -> Option<MailBodySegmentMetadata> {
    if let Some(parent) = parent {
        return Some(reply_message_metadata(parent));
    }
    has_reply_headers.then(|| MailBodySegmentMetadata {
        subject: reply_parent_subject(&message.subject),
        // In both received and sent replies the current author was a
        // recipient of the immediately quoted message.
        sender: None,
        recipient: message.sender.as_ref().map(format_mail_address),
        sent_at: None,
    })
}

fn reply_quote_metadata_chain(
    message: &InboxMessage,
    ancestors: &[Option<InboxMessage>],
    has_reply_headers: bool,
) -> Vec<Option<MailBodySegmentMetadata>> {
    let mut metadata_chain = ancestors
        .iter()
        .map(|ancestor| ancestor.as_ref().map(reply_message_metadata))
        .collect::<Vec<_>>();
    let fallback = reply_quote_metadata(message, None, has_reply_headers);
    if metadata_chain.is_empty() {
        if fallback.is_some() {
            metadata_chain.push(fallback);
        }
    } else if metadata_chain[0].is_none() {
        metadata_chain[0] = fallback;
    }
    metadata_chain
}

fn reply_message_metadata(message: &InboxMessage) -> MailBodySegmentMetadata {
    MailBodySegmentMetadata {
        subject: nonempty(message.subject.as_str()),
        sender: message.sender.as_ref().map(format_mail_address),
        recipient: joined_mail_addresses(&message.to),
        sent_at: message
            .sent_at
            .clone()
            .or_else(|| message.internal_date.clone()),
    }
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn reply_parent_subject(subject: &str) -> Option<String> {
    let subject = subject.trim();
    let without_prefix = if subject
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("re:"))
    {
        &subject[3..]
    } else if let Some(subject) = subject.strip_prefix("回复：") {
        subject
    } else if let Some(subject) = subject.strip_prefix("回复:") {
        subject
    } else {
        subject
    };
    nonempty(without_prefix)
}

fn format_mail_address(address: &MailAddress) -> String {
    match address
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) => format!("{name} <{}>", address.email),
        None => address.email.clone(),
    }
}

fn joined_mail_addresses(addresses: &[MailAddress]) -> Option<String> {
    let joined = addresses
        .iter()
        .map(format_mail_address)
        .collect::<Vec<_>>()
        .join(", ");
    nonempty(&joined)
}

fn full_message_dto(backend: &MailBackend, message: InboxMessage) -> InboxMessageDto {
    let ancestors = backend.cached_reply_ancestors(&message).unwrap_or_default();
    let navigation_targets = ancestors
        .iter()
        .map(|ancestor| {
            ancestor
                .as_ref()
                .and_then(|ancestor| backend.public_id_for_cached_message(ancestor).ok())
                .map(|id| MessageNavigationTargetDto { id })
        })
        .collect::<Vec<_>>();
    InboxMessageDto::full_with_resolved_ancestors(message, &ancestors, &navigation_targets)
}

impl From<InboxMessage> for InboxMessageDto {
    fn from(value: InboxMessage) -> Self {
        Self::full(value)
    }
}

#[derive(Clone, Debug, Serialize)]
struct AttachmentMetaDto {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_name: Option<String>,
    safe_display_name: String,
    mime_type: String,
    size_bytes: u64,
    size_is_estimate: bool,
    disposition: AttachmentDisposition,
}

impl From<AttachmentMeta> for AttachmentMetaDto {
    fn from(value: AttachmentMeta) -> Self {
        Self {
            id: value.id,
            original_name: value.original_name,
            safe_display_name: value.safe_display_name,
            mime_type: value.mime_type,
            size_bytes: value.size_bytes,
            size_is_estimate: value.size_is_estimate,
            disposition: value.disposition,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct DraftAttachmentMetaDto {
    id: String,
    name: String,
    mime_type: String,
    size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_attachment_id: Option<String>,
}

impl From<DraftAttachmentMeta> for DraftAttachmentMetaDto {
    fn from(value: DraftAttachmentMeta) -> Self {
        Self {
            id: value.id,
            name: value.name,
            mime_type: value.mime_type,
            size_bytes: value.size_bytes,
            source_attachment_id: value.source_attachment_id,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct ForwardContextDto {
    source_message_id: String,
    original_subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<MailAddressDto>,
    to: Vec<MailAddressDto>,
    cc: Vec<MailAddressDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sent_at: Option<String>,
    quoted_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    quoted_html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quoted_render_mode: Option<ForwardQuotedRenderMode>,
    source_attachments: Vec<AttachmentMetaDto>,
}

impl From<ForwardContext> for ForwardContextDto {
    fn from(value: ForwardContext) -> Self {
        Self {
            source_message_id: value.source_message_id,
            original_subject: value.original_subject,
            from: value.from.map(Into::into),
            to: value.to.into_iter().map(Into::into).collect(),
            cc: value.cc.into_iter().map(Into::into).collect(),
            sent_at: value.sent_at,
            quoted_text: value.quoted_text,
            quoted_html: value.quoted_html,
            quoted_render_mode: value.quoted_render_mode,
            source_attachments: value
                .source_attachments
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

/// Explicit compose boundary. Do not flatten or serialize the core `DraftDto`:
/// it contains Rust-only account and provider positioning fields.
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
    format: ComposeFormat,
    reply_context: Option<ReplyContextDto>,
    status: String,
    created_at: String,
    updated_at: String,
    attachments: Vec<DraftAttachmentMetaDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    forward_context: Option<ForwardContextDto>,
}

impl From<CoreDraftDto> for DraftDto {
    fn from(value: CoreDraftDto) -> Self {
        let CoreDraftDto {
            draft,
            attachments,
            forward_context,
        } = value;
        Self {
            id: draft.id,
            local_version: draft.local_version,
            has_unsupported_content: draft.has_unsupported_content,
            to: draft.to,
            cc: draft.cc,
            bcc: draft.bcc,
            subject: draft.subject,
            body_text: draft.body_text,
            format: draft.format,
            reply_context: draft.reply_context.map(Into::into),
            status: draft.status,
            created_at: draft.created_at,
            updated_at: draft.updated_at,
            attachments: attachments.into_iter().map(Into::into).collect(),
            forward_context: forward_context.map(Into::into),
        }
    }
}

impl From<Draft> for DraftDto {
    fn from(value: Draft) -> Self {
        CoreDraftDto::from(value).into()
    }
}

#[derive(Clone, Debug, Serialize)]
struct DraftSaveOutcomeDto {
    kind: DraftSaveKind,
    draft: DraftDto,
    canonical: Option<DraftDto>,
}

#[derive(Clone, Debug, Serialize)]
struct DraftAttachmentMutationOutcomeDto {
    kind: DraftAttachmentMutationKind,
    draft: DraftDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical: Option<DraftDto>,
}

impl From<DraftAttachmentMutationOutcome> for DraftAttachmentMutationOutcomeDto {
    fn from(value: DraftAttachmentMutationOutcome) -> Self {
        Self {
            kind: value.kind,
            draft: value.draft.into(),
            canonical: value.canonical.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct AttachmentSaveResultDto {
    status: AttachmentSaveStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<AttachmentSaveErrorKind>,
    retryable: bool,
}

impl From<AttachmentSaveResult> for AttachmentSaveResultDto {
    fn from(value: AttachmentSaveResult) -> Self {
        Self {
            status: value.status,
            file_name: value.file_name.and_then(|name| {
                let base_name = name.rsplit(['/', '\\']).next().unwrap_or_default().trim();
                (!base_name.is_empty() && !base_name.chars().any(char::is_control))
                    .then(|| base_name.to_owned())
            }),
            error_kind: value.error_kind,
            retryable: value.retryable,
        }
    }
}

impl AttachmentSaveResultDto {
    fn error(error_kind: AttachmentSaveErrorKind, retryable: bool) -> Self {
        Self {
            status: AttachmentSaveStatus::Error,
            file_name: None,
            error_kind: Some(error_kind),
            retryable,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct PreparedForwardDto {
    draft: DraftDto,
    warnings: Vec<ForwardWarning>,
}

impl From<PreparedForward> for PreparedForwardDto {
    fn from(value: PreparedForward) -> Self {
        Self {
            draft: value.draft.into(),
            warnings: value.warnings,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct ForwardPreparationErrorDto {
    kind: ForwardPreparationErrorKind,
    failed_attachment_ids: Vec<String>,
    retry_without_attachments_allowed: bool,
}

impl From<ForwardPreparationError> for ForwardPreparationErrorDto {
    fn from(value: ForwardPreparationError) -> Self {
        Self {
            kind: value.kind,
            failed_attachment_ids: value.failed_attachment_ids,
            retry_without_attachments_allowed: value.retry_without_attachments_allowed,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ForwardPreparationOutcomeDto {
    Prepared { prepared: PreparedForwardDto },
    Error { error: ForwardPreparationErrorDto },
}

impl From<ForwardPreparationOutcome> for ForwardPreparationOutcomeDto {
    fn from(value: ForwardPreparationOutcome) -> Self {
        match value {
            ForwardPreparationOutcome::Prepared { prepared } => Self::Prepared {
                prepared: prepared.into(),
            },
            ForwardPreparationOutcome::Error { error } => Self::Error {
                error: error.into(),
            },
        }
    }
}

impl ForwardPreparationOutcomeDto {
    fn error(
        kind: ForwardPreparationErrorKind,
        failed_attachment_ids: Vec<String>,
        retry_without_attachments_allowed: bool,
    ) -> Self {
        Self::Error {
            error: ForwardPreparationErrorDto {
                kind,
                failed_attachment_ids,
                retry_without_attachments_allowed,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct DraftDeleteOutcomeDto {
    kind: DraftDeleteKind,
}

fn complete_draft_dto(backend: &MailBackend, draft_id: &str) -> CommandResult<DraftDto> {
    backend
        .draft_dto(draft_id)
        .map(Into::into)
        .map_err(safe_mail_error)
}

fn complete_draft_save_outcome(
    backend: &MailBackend,
    value: DraftSaveOutcome,
) -> CommandResult<DraftSaveOutcomeDto> {
    let draft = complete_draft_dto(backend, &value.draft.id)?;
    let canonical = value
        .canonical
        .as_ref()
        .map(|draft| complete_draft_dto(backend, &draft.id))
        .transpose()?;
    Ok(DraftSaveOutcomeDto {
        kind: value.kind,
        draft,
        canonical,
    })
}

#[derive(Clone, Debug, Serialize)]
struct OutboxItemDto {
    id: String,
    draft_id: Option<String>,
    recipients: Vec<String>,
    recipient_groups: Option<OutboxRecipientGroupsDto>,
    subject: String,
    preview: String,
    status: OutboxStatus,
    attempts: u32,
    last_error: Option<String>,
    created_at: String,
    sent_at: Option<String>,
    message_id: Option<String>,
    message_date: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct OutboxMessageDto {
    id: String,
    subject: String,
    recipient_groups: Option<OutboxRecipientGroupsDto>,
    body_text: String,
    body_html: Option<String>,
    body_render_mode: BodyRenderMode,
    body_segments: Vec<BodySegmentDto>,
    body_html_available: bool,
    body_html_loaded: bool,
    has_remote_images: bool,
    body_fetched: bool,
}

#[derive(Clone, Debug, Serialize)]
struct OutboxRecipientGroupsDto {
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
}

impl From<OutboxRecipientGroups> for OutboxRecipientGroupsDto {
    fn from(value: OutboxRecipientGroups) -> Self {
        Self {
            to: value.to,
            cc: value.cc,
            bcc: value.bcc,
        }
    }
}

fn safe_outbox_last_error(status: OutboxStatus, error: Option<&str>) -> Option<String> {
    error.map(|_| {
        match status {
            OutboxStatus::Retryable => "邮箱服务未确认本次投递，可以安全重试。",
            OutboxStatus::Rejected => "邮箱服务器拒绝了这封邮件，请检查收件人和账户设置后重试。",
            OutboxStatus::DeliveryUnknown => {
                "投递结果仍待确认，请先到邮箱服务商的“已发送”文件夹核对，再决定是否重试。"
            }
            OutboxStatus::Queued | OutboxStatus::Sending | OutboxStatus::Sent => {
                "Mine Mail 内部处理失败：上一次发送尝试未能完成。"
            }
        }
        .to_owned()
    })
}

impl From<OutboxItem> for OutboxItemDto {
    fn from(value: OutboxItem) -> Self {
        let subject = outbox_subject(&value).unwrap_or_default();
        let preview = outbox_preview(&value).unwrap_or_default();
        let message_id = outbox_message_id(&value);
        let message_date = outbox_sent_at(&value);
        let last_error = safe_outbox_last_error(value.status, value.last_error.as_deref());
        let recipient_groups = value.recipient_groups.map(Into::into);
        Self {
            id: value.id,
            draft_id: value.draft_id,
            recipients: value.recipients,
            recipient_groups,
            subject,
            preview,
            status: value.status,
            attempts: value.attempts,
            last_error,
            created_at: value.created_at,
            sent_at: value.sent_at,
            message_id,
            message_date,
        }
    }
}

impl From<OutboxItem> for OutboxMessageDto {
    fn from(value: OutboxItem) -> Self {
        let subject = outbox_subject(&value).unwrap_or_default();
        let body_text = outbox_body_text(&value).unwrap_or_default();
        let raw_body_html = outbox_body_html(&value);
        let body_html_available = raw_body_html.is_some();
        let has_reply_headers = outbox_has_reply_headers(&value);
        let quote_metadata = has_reply_headers.then(|| MailBodySegmentMetadata {
            subject: reply_parent_subject(&subject),
            sender: value.recipients.first().cloned(),
            recipient: None,
            sent_at: None,
        });
        let body_segments = segment_mail_body_with_metadata(
            Some(&body_text),
            raw_body_html.as_deref(),
            has_reply_headers,
            quote_metadata.as_ref(),
        )
        .into_iter()
        .map(Into::into)
        .collect();
        let sanitized = raw_body_html.as_deref().map(sanitize_mail_html);
        let has_remote_images = sanitized
            .as_ref()
            .is_some_and(|html| html.has_remote_images);
        let (body_html, body_render_mode) = match sanitized {
            None => (None, BodyRenderMode::Plain),
            Some(html) if body_text.trim().is_empty() => {
                (Some(html.fragment), BodyRenderMode::IsolatedHtml)
            }
            Some(html) => match html.structure {
                MailHtmlStructure::PlainEquivalent => (None, BodyRenderMode::Plain),
                MailHtmlStructure::Native => match html.native_fragment {
                    Some(fragment) => (Some(fragment), BodyRenderMode::NativeHtml),
                    None => (Some(html.fragment), BodyRenderMode::IsolatedHtml),
                },
                MailHtmlStructure::Isolated => (Some(html.fragment), BodyRenderMode::IsolatedHtml),
            },
        };
        let recipient_groups = value.recipient_groups.map(Into::into);
        Self {
            id: value.id.clone(),
            subject,
            recipient_groups,
            body_text,
            body_html,
            body_render_mode,
            body_segments,
            body_html_available,
            body_html_loaded: true,
            has_remote_images,
            body_fetched: true,
        }
    }
}

#[tauri::command]
async fn sync_sent(app: AppHandle) -> CommandResult<desktop::SyncReportDto> {
    desktop::perform_sent_sync(&app).await.map(Into::into)
}

/// Returns current-account correspondents separately from app-wide favorites.
/// Each favorite retains the account that owns it so the UI can make the
/// otherwise mixed scope explicit.
#[tauri::command]
fn list_contacts(
    backend: State<'_, BackendState>,
    account: State<'_, AccountRuntime>,
    contacts: State<'_, ContactRuntime>,
    account_id: String,
) -> CommandResult<ContactDirectoryDto> {
    backend.local_for(&account_id)?;
    let activity_by_account = account
        .account_ids()
        .into_iter()
        .map(|configured_account_id| {
            backend
                .local_for(&configured_account_id)?
                .list_contact_activity()
                .map(|activity| (configured_account_id, activity))
                .map_err(safe_mail_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    contacts.list_directory(&account_id, activity_by_account)
}

/// Returns only existing message-summary fields plus a portable direction.
/// Complete body text, sender HTML, and RFC822 bytes are never loaded by this
/// query or serialized across the Tauri boundary.
#[tauri::command]
fn list_contact_messages(
    backend: State<'_, BackendState>,
    account_id: String,
    email: String,
    limit: Option<usize>,
) -> CommandResult<Vec<ContactMessageDto>> {
    let limit = limit
        .unwrap_or(CONTACT_MESSAGE_LIST_LIMIT)
        .clamp(1, CONTACT_MESSAGE_LIST_LIMIT);
    backend
        .local_for(&account_id)?
        .list_contact_messages(&email, limit)
        .map(|messages| messages.into_iter().map(Into::into).collect())
        .map_err(safe_mail_error)
}

#[tauri::command]
fn set_contact_favorite(
    backend: State<'_, BackendState>,
    contacts: State<'_, ContactRuntime>,
    account_id: String,
    email: String,
    favorite: bool,
) -> CommandResult<bool> {
    backend.local_for(&account_id)?;
    contacts.set_favorite(&account_id, &email, favorite)
}

#[tauri::command]
fn set_contact_remark(
    contacts: State<'_, ContactRuntime>,
    email: String,
    remark: String,
) -> CommandResult<bool> {
    contacts.set_remark(&email, &remark)
}

#[tauri::command]
fn open_external_url(url: String) -> CommandResult<()> {
    let url = validate_external_url(&url)?;
    open_validated_external_url(&url)
}

fn open_validated_external_url(url: &Url) -> CommandResult<()> {
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

#[derive(Debug, PartialEq, Eq)]
enum WebviewNavigationDecision {
    AllowInternal,
    OpenExternal(Url),
    Deny,
}

fn same_webview_origin(url: &Url, app_origin: &Url) -> bool {
    url.scheme() == app_origin.scheme()
        && url.host_str() == app_origin.host_str()
        && url.port_or_known_default() == app_origin.port_or_known_default()
        && url.username().is_empty()
        && url.password().is_none()
}

fn is_isolated_document_url(url: &Url) -> bool {
    url.scheme() == "about" && matches!(url.path(), "blank" | "srcdoc") && url.query().is_none()
}

fn classify_webview_navigation(url: &Url, app_origin: &Url) -> WebviewNavigationDecision {
    if same_webview_origin(url, app_origin) || is_isolated_document_url(url) {
        return WebviewNavigationDecision::AllowInternal;
    }

    match validate_external_url(url.as_str()) {
        Ok(url) => WebviewNavigationDecision::OpenExternal(url),
        Err(_) => WebviewNavigationDecision::Deny,
    }
}

fn configured_webview_app_origin(use_https_scheme: bool, dev_url: Option<&Url>) -> Url {
    if cfg!(debug_assertions)
        && let Some(dev_url) = dev_url
    {
        return dev_url.clone();
    }

    #[cfg(any(windows, target_os = "android"))]
    {
        let scheme = if use_https_scheme { "https" } else { "http" };
        Url::parse(&format!("{scheme}://tauri.localhost"))
            .expect("the Tauri application origin must be valid")
    }

    #[cfg(not(any(windows, target_os = "android")))]
    {
        let _ = use_https_scheme;
        Url::parse("tauri://localhost").expect("the Tauri application origin must be valid")
    }
}

#[tauri::command]
fn prepare_reply(
    backend: State<'_, BackendState>,
    message_id: String,
) -> CommandResult<ComposeRequestDto> {
    mailbox_api::validate_message_id(&message_id)?;
    let backend = backend.local()?;
    backend
        .prepare_reply(&message_id)
        .map(Into::into)
        .map_err(safe_mail_error)
}

#[tauri::command]
fn save_draft(
    app: AppHandle,
    backend: State<'_, BackendState>,
    request: ComposeRequest,
    draft_id: Option<String>,
    expected_local_version: Option<u64>,
) -> CommandResult<DraftSaveOutcomeDto> {
    let request = sanitize_compose_request(request);
    let account_id = backend.active_account_id();
    let backend = match backend.local() {
        Ok(backend) => backend,
        Err(error) => {
            diagnostics::limited_failure(
                "draft_save_failed",
                "draft_save",
                account_id.as_deref(),
                DiagnosticErrorKind::Runtime,
            );
            return Err(error);
        }
    };
    let outcome =
        match backend.save_draft_optimistic(draft_id.as_deref(), expected_local_version, request) {
            Ok(outcome) => outcome,
            Err(error) => {
                let error_kind = diagnostics::mail_error_kind(&error);
                diagnostics::limited_failure(
                    "draft_save_failed",
                    "draft_save",
                    account_id.as_deref(),
                    error_kind,
                );
                return Err(safe_mail_error(error));
            }
        };
    diagnostics::limited_recovery(
        "draft_save_failed",
        "draft_save_recovered",
        "draft_save",
        account_id.as_deref(),
    );
    if outcome.kind == DraftSaveKind::ConflictCopy {
        let mut fields = DiagnosticFields::default()
            .operation("draft_save")
            .item("draft", &outcome.draft.id)
            .outcome("conflict_copy")
            .draft_version(outcome.draft.local_version);
        if let Some(account_id) = account_id.as_deref() {
            fields = fields.account(account_id);
        }
        diagnostics::warn("draft_conflict_created", fields);
    }
    let outcome = complete_draft_save_outcome(&backend, outcome)?;
    let _ = app.emit("mail:drafts-updated", desktop::DraftsUpdatedEvent::saved());
    Ok(outcome)
}

#[tauri::command]
fn list_drafts(backend: State<'_, BackendState>) -> CommandResult<Vec<DraftDto>> {
    let backend = backend.local()?;
    let drafts = backend.list_drafts().map_err(safe_mail_error)?;
    drafts
        .into_iter()
        .map(|draft| complete_draft_dto(&backend, &draft.id))
        .collect()
}

#[tauri::command]
fn create_compose_draft(
    app: AppHandle,
    backend: State<'_, BackendState>,
) -> CommandResult<DraftDto> {
    let draft = backend
        .local()?
        .create_compose_draft()
        .map(Into::into)
        .map_err(safe_mail_error)?;
    let _ = app.emit("mail:drafts-updated", desktop::DraftsUpdatedEvent::saved());
    Ok(draft)
}

#[tauri::command]
async fn add_draft_attachments(
    app: AppHandle,
    backend: State<'_, BackendState>,
    draft_id: String,
    expected_local_version: u64,
) -> CommandResult<DraftAttachmentMutationOutcomeDto> {
    let backend = backend.local()?;
    // Validate the opaque draft identity before opening a platform picker.
    backend.draft_dto(&draft_id).map_err(safe_mail_error)?;
    let selected = app.dialog().file().blocking_pick_files();
    let selected_paths = match selected {
        Some(files) => files
            .into_iter()
            .map(|file| {
                file.into_path()
                    .map_err(|_| "The selected attachment could not be accessed.".to_owned())
            })
            .collect::<CommandResult<Vec<_>>>()?,
        None => Vec::new(),
    };
    let outcome = backend
        .add_draft_attachments(&draft_id, expected_local_version, &selected_paths)
        .map_err(safe_mail_error)?;
    if matches!(
        outcome.kind,
        DraftAttachmentMutationKind::Saved | DraftAttachmentMutationKind::ConflictCopy
    ) {
        let _ = app.emit("mail:drafts-updated", desktop::DraftsUpdatedEvent::saved());
    }
    Ok(outcome.into())
}

#[tauri::command]
fn remove_draft_attachment(
    app: AppHandle,
    backend: State<'_, BackendState>,
    draft_id: String,
    attachment_id: String,
    expected_local_version: u64,
) -> CommandResult<DraftAttachmentMutationOutcomeDto> {
    let backend = backend.local()?;
    let outcome = backend
        .remove_draft_attachment(&draft_id, &attachment_id, expected_local_version)
        .map_err(safe_mail_error)?;
    if outcome.kind == DraftAttachmentMutationKind::Saved {
        let _ = app.emit("mail:drafts-updated", desktop::DraftsUpdatedEvent::saved());
    }
    Ok(outcome.into())
}

#[tauri::command]
fn delete_draft(
    app: AppHandle,
    backend: State<'_, BackendState>,
    draft_id: String,
    expected_local_version: u64,
) -> CommandResult<DraftDeleteOutcomeDto> {
    let operation_id = diagnostics::operation_id();
    let account_id = backend.active_account_id();
    let mut fields = DiagnosticFields::default()
        .operation_id(operation_id)
        .operation("draft_delete")
        .item("draft", &draft_id)
        .draft_version(expected_local_version);
    if let Some(account_id) = account_id.as_deref() {
        fields = fields.account(account_id);
    }
    diagnostics::info("draft_delete_started", fields.clone());
    let backend = match backend.local() {
        Ok(backend) => backend,
        Err(error) => {
            diagnostics::error(
                "draft_delete_failed",
                fields.error(DiagnosticErrorKind::Runtime),
            );
            return Err(error);
        }
    };
    let kind = match backend.delete_draft_optimistic(&draft_id, expected_local_version) {
        Ok(kind) => kind,
        Err(error) => {
            let error_kind = diagnostics::mail_error_kind(&error);
            diagnostics::error("draft_delete_failed", fields.error(error_kind));
            return Err(safe_mail_error(error));
        }
    };
    diagnostics::info(
        "draft_delete_completed",
        fields.outcome(match kind {
            DraftDeleteKind::Deleted => "deleted",
            DraftDeleteKind::Stale => "stale_version",
        }),
    );
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
    desktop_runtime: State<'_, DesktopRuntime>,
    draft_id: String,
    expected_local_version: u64,
    confirmed_recipients: Vec<String>,
) -> CommandResult<OutboxItemDto> {
    let started = Instant::now();
    let operation_id = diagnostics::operation_id();
    let account_id = backend.active_account_id();
    let mut fields = DiagnosticFields::default()
        .operation_id(operation_id)
        .operation("send")
        .item("draft", &draft_id)
        .draft_version(expected_local_version);
    if let Some(account_id) = account_id.as_deref() {
        fields = fields.account(account_id);
    }
    diagnostics::info("send_started", fields.clone());
    let _smtp_operation = desktop_runtime.begin_smtp_operation()?;
    if let Err(error) = account.refresh_active_oauth_backend(&backend).await {
        diagnostics::error(
            "send_failed",
            fields
                .clone()
                .error(DiagnosticErrorKind::Runtime)
                .outcome("oauth_refresh_failed")
                .duration(started.elapsed()),
        );
        return Err(error);
    }
    let backend = match backend.network() {
        Ok(backend) => backend,
        Err(error) => {
            diagnostics::error(
                "send_failed",
                fields
                    .error(DiagnosticErrorKind::Runtime)
                    .outcome("backend_unavailable")
                    .duration(started.elapsed()),
            );
            return Err(error);
        }
    };
    match backend
        .send_draft(&draft_id, expected_local_version, &confirmed_recipients)
        .await
    {
        Ok(item) => {
            diagnostics::info(
                "send_completed",
                fields
                    .item("outbox", &item.id)
                    .outcome(outbox_status_name(item.status))
                    .duration(started.elapsed()),
            );
            Ok(item.into())
        }
        Err(error) => {
            let error_kind = diagnostics::mail_error_kind(&error);
            diagnostics::error(
                "send_failed",
                fields.error(error_kind).duration(started.elapsed()),
            );
            Err(safe_mail_error(error))
        }
    }
}

/// A manual retry reuses the immutable RFC822 message and SMTP envelope that
/// were already confirmed and persisted in Outbox. Only the Rust core's
/// `retryable` state gate can authorize the transition back to `sending`.
#[tauri::command]
async fn retry_outbox(
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    desktop_runtime: State<'_, DesktopRuntime>,
    outbox_id: String,
) -> CommandResult<OutboxItemDto> {
    let started = Instant::now();
    let operation_id = diagnostics::operation_id();
    let account_id = backend.active_account_id();
    let mut fields = DiagnosticFields::default()
        .operation_id(operation_id)
        .operation("outbox_retry")
        .item("outbox", &outbox_id);
    if let Some(account_id) = account_id.as_deref() {
        fields = fields.account(account_id);
    }
    diagnostics::info("outbox_retry_started", fields.clone());
    let _smtp_operation = desktop_runtime.begin_smtp_operation()?;
    if let Err(error) = account.refresh_active_oauth_backend(&backend).await {
        diagnostics::error(
            "outbox_retry_failed",
            fields
                .clone()
                .error(DiagnosticErrorKind::Runtime)
                .outcome("oauth_refresh_failed")
                .duration(started.elapsed()),
        );
        return Err(error);
    }
    let backend = match backend.network() {
        Ok(backend) => backend,
        Err(error) => {
            diagnostics::error(
                "outbox_retry_failed",
                fields
                    .error(DiagnosticErrorKind::Runtime)
                    .outcome("backend_unavailable")
                    .duration(started.elapsed()),
            );
            return Err(error);
        }
    };
    match backend.retry_outbox(&outbox_id).await {
        Ok(item) => {
            diagnostics::info(
                "outbox_retry_completed",
                fields
                    .outcome(outbox_status_name(item.status))
                    .duration(started.elapsed()),
            );
            Ok(item.into())
        }
        Err(error) => {
            let error_kind = diagnostics::mail_error_kind(&error);
            diagnostics::error(
                "outbox_retry_failed",
                fields.error(error_kind).duration(started.elapsed()),
            );
            Err(safe_mail_error(error))
        }
    }
}

fn validate_outbox_id(outbox_id: &str) -> CommandResult<()> {
    if outbox_id.is_empty()
        || outbox_id.len() > MAX_OUTBOX_ID_BYTES
        || outbox_id.chars().any(char::is_control)
    {
        return Err("The Outbox identifier is invalid.".to_owned());
    }
    Ok(())
}

fn validate_delivery_unknown_request(
    decision: DeliveryUnknownDecision,
    acknowledge_duplicate_risk: bool,
) -> CommandResult<()> {
    match (decision, acknowledge_duplicate_risk) {
        (DeliveryUnknownDecision::ConfirmDelivered, false)
        | (DeliveryUnknownDecision::RetryOnce, true) => Ok(()),
        (DeliveryUnknownDecision::RetryOnce, false) => Err(
            "Retrying an unknown delivery requires explicit acknowledgement of duplicate risk."
                .to_owned(),
        ),
        (DeliveryUnknownDecision::ConfirmDelivered, true) => {
            Err("Duplicate-risk acknowledgement is valid only for an explicit retry.".to_owned())
        }
    }
}

fn delivery_unknown_decision_name(decision: DeliveryUnknownDecision) -> &'static str {
    match decision {
        DeliveryUnknownDecision::ConfirmDelivered => "confirm_delivered",
        DeliveryUnknownDecision::RetryOnce => "retry_once",
    }
}

/// Resolves exactly one user-reviewed `delivery_unknown` attempt generation.
///
/// `confirm_delivered` performs only an atomic local transition after the user
/// checked the provider. `retry_once` reuses the persisted immutable RFC822 and
/// envelope, and requires a duplicate-risk acknowledgement at both this command
/// boundary and the core backend.
#[tauri::command]
async fn resolve_delivery_unknown(
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    desktop_runtime: State<'_, DesktopRuntime>,
    outbox_id: String,
    expected_attempts: u32,
    decision: DeliveryUnknownDecision,
    acknowledge_duplicate_risk: bool,
) -> CommandResult<OutboxItemDto> {
    validate_outbox_id(&outbox_id)?;
    validate_delivery_unknown_request(decision, acknowledge_duplicate_risk)?;

    let started = Instant::now();
    let operation_id = diagnostics::operation_id();
    let mut fields = DiagnosticFields::default()
        .operation_id(operation_id)
        .operation("delivery_unknown_resolution")
        .item("outbox", &outbox_id)
        .outcome(delivery_unknown_decision_name(decision));
    if let Some(account_id) = backend.active_account_id().as_deref() {
        fields = fields.account(account_id);
    }
    diagnostics::info("delivery_unknown_resolution_started", fields.clone());

    let result = match decision {
        DeliveryUnknownDecision::ConfirmDelivered => backend
            .local()
            .and_then(|local| {
                local
                    .confirm_delivery_unknown(&outbox_id, expected_attempts)
                    .map_err(safe_mail_error)
            })
            .map(OutboxItemDto::from),
        DeliveryUnknownDecision::RetryOnce => {
            let _smtp_operation = desktop_runtime.begin_smtp_operation()?;
            if let Err(error) = account.refresh_active_oauth_backend(&backend).await {
                diagnostics::error(
                    "delivery_unknown_resolution_failed",
                    fields
                        .clone()
                        .error(DiagnosticErrorKind::Runtime)
                        .outcome("oauth_refresh_failed")
                        .duration(started.elapsed()),
                );
                return Err(error);
            }
            let network = match backend.network() {
                Ok(network) => network,
                Err(error) => {
                    diagnostics::error(
                        "delivery_unknown_resolution_failed",
                        fields
                            .error(DiagnosticErrorKind::Runtime)
                            .outcome("backend_unavailable")
                            .duration(started.elapsed()),
                    );
                    return Err(error);
                }
            };
            network
                .retry_delivery_unknown_once(
                    &outbox_id,
                    expected_attempts,
                    acknowledge_duplicate_risk,
                )
                .await
                .map(OutboxItemDto::from)
                .map_err(safe_mail_error)
        }
    };

    match result {
        Ok(item) => {
            diagnostics::info(
                "delivery_unknown_resolution_completed",
                fields
                    .outcome(outbox_status_name(item.status))
                    .duration(started.elapsed()),
            );
            Ok(item)
        }
        Err(error) => {
            diagnostics::error(
                "delivery_unknown_resolution_failed",
                fields
                    .error(DiagnosticErrorKind::Runtime)
                    .duration(started.elapsed()),
            );
            Err(error)
        }
    }
}

fn outbox_status_name(status: OutboxStatus) -> &'static str {
    match status {
        OutboxStatus::Queued => "queued",
        OutboxStatus::Sending => "sending",
        OutboxStatus::Sent => "sent",
        OutboxStatus::Retryable => "retryable",
        OutboxStatus::Rejected => "rejected",
        OutboxStatus::DeliveryUnknown => "delivery_unknown",
    }
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
fn list_sent_outbox_fallbacks(
    backend: State<'_, BackendState>,
) -> CommandResult<Vec<OutboxItemDto>> {
    let backend = backend.local()?;
    backend
        .list_sent_outbox_fallbacks()
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

#[tauri::command]
async fn get_storage_status(storage: State<'_, StorageRuntime>) -> CommandResult<StorageStatusDto> {
    let storage = storage.inner().clone();
    tauri::async_runtime::spawn_blocking(move || storage.status())
        .await
        .map_err(|_| "无法完成本地存储统计。".to_owned())?
}

#[tauri::command]
fn prepare_storage_migration(
    storage: State<'_, StorageRuntime>,
    target_path: String,
) -> CommandResult<PreparedStorageMigrationDto> {
    storage.prepare_migration(&target_path)
}

#[tauri::command]
fn cancel_storage_migration(storage: State<'_, StorageRuntime>) -> CommandResult<()> {
    storage.cancel_pending_migration()
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
fn open_new_mail_notification(app: AppHandle, notification_id: u64) -> CommandResult<bool> {
    desktop::open_new_mail_notification(&app, notification_id)
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
    if !enabled && !current {
        return Ok(());
    }
    if enabled {
        // `is_enabled` only tells us that an entry exists; it does not verify
        // its executable arguments. Re-enabling rewrites entries left by older
        // releases so login startup always includes `--background`.
        autostart.enable()
    } else {
        autostart.disable()
    }
    .map_err(|_| "The system startup setting could not be updated.".to_owned())
}

fn refresh_enabled_autostart_registration(app: &AppHandle) -> CommandResult<bool> {
    let autostart = app.autolaunch();
    let enabled = autostart
        .is_enabled()
        .map_err(|_| "The system startup setting could not be read.".to_owned())?;
    if enabled {
        // The autostart plugin owns the platform-specific registration and is
        // configured with `--background`. Rewriting an enabled entry migrates
        // historical registrations without changing the user's preference.
        autostart
            .enable()
            .map_err(|_| "The system startup setting could not be refreshed.".to_owned())?;
    }
    Ok(enabled)
}

fn requested_autostart_change(current: bool, requested: Option<bool>) -> Option<bool> {
    requested.filter(|requested| *requested != current)
}

#[tauri::command]
async fn complete_exit(app: AppHandle, request_id: u64) -> CommandResult<bool> {
    desktop::complete_exit(&app, request_id).await
}

#[tauri::command]
fn cancel_exit(app: AppHandle, request_id: u64) -> CommandResult<bool> {
    desktop::cancel_exit(&app, request_id)
}

#[tauri::command]
async fn sync_all(app: AppHandle) -> CommandResult<desktop::SyncAllReport> {
    desktop::perform_sync_all(&app, true, "manual")
        .await?
        .ok_or_else(|| "The requested synchronization was skipped.".to_owned())
}

#[tauri::command]
async fn sync_drafts(app: AppHandle) -> CommandResult<desktop::DraftSyncReportDto> {
    desktop::perform_draft_sync(&app).await.map(Into::into)
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
    let (status, account_added) = account.configure(&backend, request).await?;
    if account_added
        && let Some(account_id) = backend.active_account_id()
        && let Err(error) = desktop_runtime.begin_notification_baseline(&account_id)
    {
        desktop_runtime.record_startup_error(error);
    }
    let _ = app.emit("mail:account-updated", status.clone());
    desktop::request_sync(&app, true, "account_change");
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
    let (status, account_added) = account.connect_google(&backend).await?;
    if account_added
        && let Some(account_id) = backend.active_account_id()
        && let Err(error) = desktop_runtime.begin_notification_baseline(&account_id)
    {
        desktop_runtime.record_startup_error(error);
    }
    let _ = app.emit("mail:account-updated", status.clone());
    desktop::request_sync(&app, true, "account_change");
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
fn set_account_remark(
    app: AppHandle,
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    account_id: String,
    remark: String,
) -> CommandResult<AccountStatusDto> {
    let status = account.set_remark(&backend, &account_id, &remark)?;
    let _ = app.emit("mail:account-updated", status.clone());
    Ok(status)
}

#[tauri::command]
async fn remove_account(
    app: AppHandle,
    account: State<'_, AccountRuntime>,
    backend: State<'_, BackendState>,
    contacts: State<'_, ContactRuntime>,
    desktop_runtime: State<'_, DesktopRuntime>,
    request: RemoveAccountRequest,
) -> CommandResult<RemoveAccountResultDto> {
    let _sync_guard = desktop_runtime.acquire_sync_gate().await;
    let account_id = request.account_id.clone();
    let mut result = account.remove_account(&backend, &request).await?;
    if request.delete_local_data {
        let mut cleanup_warnings = result.warning.take().into_iter().collect::<Vec<_>>();
        if let Err(error) = desktop_runtime.remove_notification_baseline(&account_id) {
            desktop_runtime.record_startup_error(error);
            cleanup_warnings.push("The notification baseline could not be deleted.".to_owned());
        }
        if let Err(error) = contacts.remove_account(&account_id) {
            desktop_runtime.record_startup_error(error);
            cleanup_warnings
                .push("Account-scoped contact favorites could not be deleted.".to_owned());
        }
        if let Err(error) = desktop_runtime.remove_account_avatar(&result.removed_email) {
            desktop_runtime.record_startup_error(error);
            cleanup_warnings.push("The account avatar could not be deleted.".to_owned());
        }
        result.local_data_deleted = cleanup_warnings.is_empty();
        result.warning = (!cleanup_warnings.is_empty()).then(|| cleanup_warnings.join(" "));
    }
    let _ = app.emit("mail:account-updated", result.status.clone());
    if result.status.configured {
        desktop::request_sync(&app, true, "account_change");
    }
    Ok(result)
}

fn safe_mail_error(error: mine_mail::MailError) -> String {
    use mine_mail::MailError;

    match error {
        MailError::Validation(message) => format!("Validation failed: {message}"),
        MailError::NotFound { entity, id } => format!("{entity} was not found: {id}"),
        MailError::Timeout { operation } => format!("{operation} timed out. Please try again."),
        MailError::Imap(_) => "The mail server could not complete the Inbox request.".to_owned(),
        MailError::Smtp(_) => "The mail server could not complete the send request.".to_owned(),
        MailError::Connection(_) => {
            "The mail server connection could not be established. Please try again.".to_owned()
        }
        MailError::Config(_)
        | MailError::Database(_)
        | MailError::Io(_)
        | MailError::Serialization(_)
        | MailError::Mime(_) => "Mine Mail could not complete the local operation.".to_owned(),
    }
}

fn initialize_state(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    diagnostics::cleanup_on_startup(app.handle());
    let background_launch = is_background_launch(std::env::args());
    diagnostics::info(
        "app_starting",
        DiagnosticFields::default().mode(if background_launch {
            "background"
        } else {
            "foreground"
        }),
    );
    if refresh_enabled_autostart_registration(app.handle()).is_err() {
        diagnostics::warn(
            "autostart_registration_refresh_failed",
            DiagnosticFields::default()
                .operation("autostart_registration")
                .error(DiagnosticErrorKind::Runtime),
        );
    }
    let storage = StorageRuntime::initialize(app.handle());
    let app_data = storage.runtime_data_root.clone();
    let path_error = storage.startup_error;
    let path_degraded = path_error.is_some();
    let (account, backend, account_degraded) = if let Some(error) = path_error.as_ref() {
        let (account, backend) = AccountRuntime::fallback(&app_data, error.clone());
        (account, backend, true)
    } else {
        match AccountRuntime::open(&app_data) {
            Ok((account, backend)) => (account, backend, false),
            Err(error) => {
                diagnostics::error(
                    "account_store_open_failed",
                    DiagnosticFields::default().error(DiagnosticErrorKind::Runtime),
                );
                let (account, backend) = AccountRuntime::fallback(&app_data, error);
                (account, backend, true)
            }
        }
    };
    if path_degraded {
        diagnostics::error(
            "app_data_directory_unavailable",
            DiagnosticFields::default().error(DiagnosticErrorKind::Io),
        );
    }
    let local_backend_ready = backend.is_local_ready();
    let (desktop, sync_rx, shutdown_rx) = DesktopRuntime::open(&app_data);
    let contacts = ContactRuntime::open(&app_data);
    if let Some(error) = path_error {
        desktop.record_startup_error(error);
    }
    let startup_degraded = desktop.has_startup_error();

    app.manage(account);
    app.manage(backend);
    app.manage(desktop);
    app.manage(contacts);
    app.manage(storage.runtime);
    build_configured_windows(app, &app_data)?;
    let tray_available = match desktop::build_tray(app) {
        Ok(()) => {
            diagnostics::info("tray_initialized", DiagnosticFields::default());
            true
        }
        Err(_) => {
            diagnostics::error(
                "tray_initialization_failed",
                DiagnosticFields::default().error(DiagnosticErrorKind::Runtime),
            );
            app.state::<DesktopRuntime>().record_startup_error(
                "The system tray could not be initialized; Mine Mail will remain visible.",
            );
            false
        }
    };
    desktop::start_inbox_monitor_supervisor(app.handle().clone(), shutdown_rx.clone());
    desktop::start_background_loop(app.handle().clone(), sync_rx, shutdown_rx);

    diagnostics::info(
        "app_ready",
        DiagnosticFields::default()
            .accounts(app.state::<AccountRuntime>().account_ids().len())
            .degraded(path_degraded || account_degraded || startup_degraded || !tray_available),
    );

    if background_launch && tray_available && local_backend_ready && !startup_degraded {
        desktop::request_sync(app.handle(), true, "startup");
    } else {
        desktop::show_main_window(app.handle(), true);
        if local_backend_ready && !startup_degraded {
            desktop::request_sync(app.handle(), true, "startup");
        }
    }
    Ok(())
}

fn build_configured_windows(
    app: &tauri::App,
    _runtime_data_root: &std::path::Path,
) -> tauri::Result<()> {
    for window_config in &app.config().app.windows {
        let app_origin = configured_webview_app_origin(
            window_config.use_https_scheme,
            app.config().build.dev_url.as_ref(),
        );
        let app_handle = app.handle().clone();
        let builder = WebviewWindowBuilder::from_config(app.handle(), window_config)?
            .on_navigation(
                move |url| match classify_webview_navigation(url, &app_origin) {
                    WebviewNavigationDecision::AllowInternal => true,
                    WebviewNavigationDecision::OpenExternal(url) => {
                        if open_validated_external_url(&url).is_err() {
                            diagnostics::warn(
                                "external_link_open_failed",
                                DiagnosticFields::default().error(DiagnosticErrorKind::Runtime),
                            );
                            let _ = app_handle.emit_to("main", EXTERNAL_LINK_OPEN_FAILED_EVENT, ());
                        }
                        false
                    }
                    WebviewNavigationDecision::Deny => false,
                },
            );
        #[cfg(target_os = "windows")]
        let builder = builder.data_directory(_runtime_data_root.join("EBWebView"));
        builder.build()?;
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
                desktop::request_sync(app, true, "single_instance");
            } else {
                desktop::show_main_window(app, true);
            }
        }))
        .plugin(diagnostics::plugin())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
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
                desktop::request_incremental_inbox_refresh(window.app_handle(), "window_focus")
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_account_status,
            list_account_presets,
            configure_account,
            connect_google_account,
            switch_account,
            set_account_remark,
            remove_account,
            sync_sent,
            sync_all,
            list_contacts,
            list_contact_messages,
            set_contact_favorite,
            set_contact_remark,
            set_message_seen,
            set_message_starred_by_id,
            get_mailbox_capabilities,
            create_mailbox_role,
            list_archive_folder_candidates,
            assign_archive_folder,
            list_mailbox_page,
            load_older_mailbox_page,
            list_starred_mailbox_page,
            load_older_starred_mailbox_page,
            sync_mailbox,
            archive_message,
            move_message_to_inbox,
            move_message_to_trash,
            prepare_permanent_delete,
            confirm_permanent_delete,
            fetch_mailbox_message,
            save_message_attachment,
            prepare_forward,
            prepare_reply,
            open_external_url,
            save_draft,
            list_drafts,
            create_compose_draft,
            add_draft_attachments,
            remove_draft_attachment,
            delete_draft,
            sync_drafts,
            send_draft,
            retry_outbox,
            resolve_delivery_unknown,
            list_outbox,
            list_sent_outbox_fallbacks,
            fetch_outbox_message,
            get_storage_status,
            prepare_storage_migration,
            cancel_storage_migration,
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
        RunEvent::Resumed => desktop::request_sync(app, false, "resume"),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => desktop::show_main_window(app, false),
        RunEvent::ExitRequested { api, .. } => {
            diagnostics::info(
                "shutdown_requested",
                DiagnosticFields::default().operation("app_exit"),
            );
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
            diagnostics::info("app_exit", DiagnosticFields::default().outcome("completed"));
            if let Some(runtime) = app.try_state::<DesktopRuntime>() {
                runtime.finish_quit();
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use mine_mail::{
        AttachmentDisposition, AttachmentMeta, AttachmentSaveResult, AttachmentSaveStatus,
        ComposeFormat, ComposeRequest, ContactMessage, ContactMessageDirection,
        DeliveryUnknownDecision, Draft, DraftAttachmentMeta, DraftAttachmentMutationKind,
        DraftAttachmentMutationOutcome, DraftDto as CoreDraftDto, ForwardContext,
        ForwardPreparationOutcome, ForwardQuotedRenderMode, ForwardWarning, InboxMessage,
        MailAddress, MailboxRole, OutboxItem, OutboxRecipientGroups, OutboxStatus, PreparedForward,
        ReplyContext, StationeryTheme,
    };

    use super::{
        AttachmentSaveResultDto, ContactMessageDto, DraftAttachmentMutationOutcomeDto, DraftDto,
        ForwardPreparationOutcomeDto, InboxMessageDto, MessageNavigationTargetDto, OutboxItemDto,
        OutboxMessageDto, ReplyContextDto, Url, WebviewNavigationDecision,
        assert_no_private_mail_coordinates, classify_webview_navigation,
        delivery_unknown_decision_name, is_background_launch, requested_autostart_change,
        sanitize_compose_request, validate_delivery_unknown_request, validate_external_url,
        validate_outbox_id,
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
            bcc: vec![MailAddress {
                name: Some("Hidden recipient".to_owned()),
                email: "hidden@example.com".to_owned(),
            }],
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
            recipient_groups: Some(OutboxRecipientGroups {
                to: vec!["receiver@example.com".to_owned()],
                cc: vec!["copy@example.com".to_owned()],
                bcc: vec!["hidden@example.com".to_owned()],
            }),
            status: OutboxStatus::Sent,
            attempts: 1,
            last_error: None,
            created_at: "2026-07-18T00:00:00Z".to_owned(),
            sent_at: Some("2026-07-18T00:00:01Z".to_owned()),
            raw_rfc822: b"From: sender@example.com\r\nTo: receiver@example.com\r\nSubject: Re: Actual subject\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nActual sent body".to_vec(),
        }
    }

    #[test]
    fn compose_boundary_sanitizes_owned_html_and_normalizes_empty_stationery() {
        let request = sanitize_compose_request(ComposeRequest {
            to: vec!["receiver@example.com".to_owned()],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Formatted".to_owned(),
            body_text: "Safe fallback".to_owned(),
            format: ComposeFormat {
                body_html: Some(
                    r#"<div onclick="bad()"><strong>Safe</strong><script>bad()</script></div>"#
                        .to_owned(),
                ),
                stationery: StationeryTheme::None,
                send_stationery: true,
            },
            reply_context: None,
        });

        let html = request.format.body_html.expect("sanitized fragment");
        assert!(html.contains("<strong>Safe</strong>"));
        assert!(!html.contains("onclick"));
        assert!(!html.contains("script"));
        assert!(!request.format.send_stationery);
    }

    fn reply_outbox_item() -> OutboxItem {
        let mut item = outbox_item();
        item.raw_rfc822 = b"From: sender@example.com\r\nTo: receiver@example.com\r\nSubject: Re: Actual subject\r\nIn-Reply-To: <parent@example.com>\r\nX-Mine-Mail-Reply-Format: 1\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nFresh reply\r\n\r\nAt 2026-07-17 09:54:29 +08:00, \"Receiver\" <receiver@example.com> wrote:\r\n> Original body".to_vec();
        item
    }

    fn stationery_outbox_item() -> OutboxItem {
        let mut item = outbox_item();
        item.raw_rfc822 = concat!(
            "From: sender@example.com\r\n",
            "To: receiver@example.com\r\n",
            "Subject: Stationery\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/alternative; boundary=\"stationery\"\r\n",
            "\r\n",
            "--stationery\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "Short body\r\n",
            "--stationery\r\n",
            "Content-Type: text/html; charset=utf-8\r\n",
            "\r\n",
            "<div data-mine-mail-stationery=\"lined\"><strong>Short body</strong></div>\r\n",
            "--stationery--\r\n",
        )
        .as_bytes()
        .to_vec();
        item
    }

    #[test]
    fn outbox_summaries_and_selected_bodies_cross_separate_safe_boundaries() {
        let summary = serde_json::to_value(OutboxItemDto::from(outbox_item()))
            .expect("serialize Outbox summary");
        assert_eq!(summary["subject"], "Re: Actual subject");
        assert_eq!(summary["preview"], "Actual sent body");
        assert_eq!(summary["recipient_groups"]["bcc"][0], "hidden@example.com");
        assert!(summary.get("body_text").is_none());
        assert!(summary.get("raw_rfc822").is_none());
        assert_no_private_mail_coordinates(&summary);

        let selected = serde_json::to_value(OutboxMessageDto::from(outbox_item()))
            .expect("serialize selected Outbox body");
        assert_eq!(selected["subject"], "Re: Actual subject");
        assert_eq!(selected["body_text"], "Actual sent body");
        assert_eq!(selected["recipient_groups"]["bcc"][0], "hidden@example.com");
        assert_eq!(selected["body_render_mode"], "plain");
        assert_eq!(selected["body_segments"].as_array().unwrap().len(), 0);
        assert_eq!(selected["body_fetched"], true);
        assert!(selected.get("raw_rfc822").is_none());
        assert_no_private_mail_coordinates(&selected);

        let mut failed = outbox_item();
        failed.status = OutboxStatus::Retryable;
        failed.last_error = Some(
            r#"provider mailbox "[Gmail]/Sent" UID 44 at C:\Users\private\mail.eml"#.to_owned(),
        );
        let failed_summary =
            serde_json::to_value(OutboxItemDto::from(failed)).expect("serialize failed Outbox");
        assert_eq!(
            failed_summary["last_error"],
            "邮箱服务未确认本次投递，可以安全重试。"
        );
        assert!(!failed_summary.to_string().contains("[Gmail]"));
        assert!(!failed_summary.to_string().contains(r"C:\Users"));
        assert_no_private_mail_coordinates(&failed_summary);

        let mut legacy = outbox_item();
        legacy.recipient_groups = None;
        let legacy_summary = serde_json::to_value(OutboxItemDto::from(legacy.clone()))
            .expect("serialize legacy Outbox summary");
        let legacy_selected = serde_json::to_value(OutboxMessageDto::from(legacy))
            .expect("serialize legacy selected Outbox");
        assert!(legacy_summary["recipient_groups"].is_null());
        assert!(legacy_selected["recipient_groups"].is_null());
        assert_no_private_mail_coordinates(&legacy_summary);
        assert_no_private_mail_coordinates(&legacy_selected);
    }

    #[test]
    fn stationery_uses_the_isolated_reader_for_incoming_and_outbox_bodies() {
        let outbox = serde_json::to_value(OutboxMessageDto::from(stationery_outbox_item()))
            .expect("serialize stationery Outbox body");
        assert_eq!(outbox["body_render_mode"], "isolated_html");
        assert!(
            outbox["body_html"]
                .as_str()
                .is_some_and(|html| html.contains(r#"data-mine-mail-stationery="lined""#))
        );

        let mut incoming = rich_message();
        incoming.body_text = Some("Short body".to_owned());
        incoming.body_html = Some(
            r#"<div data-mine-mail-stationery="grid"><strong>Short body</strong></div>"#.to_owned(),
        );
        let incoming = serde_json::to_value(InboxMessageDto::full(incoming))
            .expect("serialize incoming stationery body");
        assert_eq!(incoming["body_render_mode"], "isolated_html");
        assert!(
            incoming["body_html"]
                .as_str()
                .is_some_and(|html| html.contains(r#"data-mine-mail-stationery="grid""#))
        );
    }

    #[test]
    fn delivery_unknown_command_boundary_uses_strict_decisions_and_explicit_risk_ack() {
        assert_eq!(
            serde_json::from_str::<DeliveryUnknownDecision>(r#""confirm_delivered""#)
                .expect("confirm decision"),
            DeliveryUnknownDecision::ConfirmDelivered
        );
        assert_eq!(
            serde_json::from_str::<DeliveryUnknownDecision>(r#""retry_once""#)
                .expect("retry decision"),
            DeliveryUnknownDecision::RetryOnce
        );
        assert!(
            serde_json::from_str::<DeliveryUnknownDecision>(r#""retry""#).is_err(),
            "unknown or abbreviated decisions must be rejected during command deserialization"
        );
        assert_eq!(
            delivery_unknown_decision_name(DeliveryUnknownDecision::ConfirmDelivered),
            "confirm_delivered"
        );
        assert_eq!(
            delivery_unknown_decision_name(DeliveryUnknownDecision::RetryOnce),
            "retry_once"
        );

        assert!(validate_outbox_id("0190f4ca-e13f-7a11-8be2-6c8c435f4da2").is_ok());
        assert!(validate_outbox_id("legacy-opaque-outbox-id").is_ok());
        assert!(validate_outbox_id("").is_err());
        assert!(validate_outbox_id("bad\noutbox").is_err());
        assert!(validate_outbox_id(&"x".repeat(129)).is_err());

        assert!(
            validate_delivery_unknown_request(DeliveryUnknownDecision::ConfirmDelivered, false)
                .is_ok()
        );
        assert!(
            validate_delivery_unknown_request(DeliveryUnknownDecision::RetryOnce, true).is_ok()
        );
        assert!(
            validate_delivery_unknown_request(DeliveryUnknownDecision::RetryOnce, false).is_err()
        );
        assert!(
            validate_delivery_unknown_request(DeliveryUnknownDecision::ConfirmDelivered, true)
                .is_err()
        );
    }

    #[test]
    fn selected_outbox_reply_uses_the_same_segmented_reader_as_remote_sent_mail() {
        let summary = serde_json::to_value(OutboxItemDto::from(reply_outbox_item()))
            .expect("serialize reply Outbox summary");
        assert_eq!(summary["preview"], "Fresh reply");

        let selected = serde_json::to_value(OutboxMessageDto::from(reply_outbox_item()))
            .expect("serialize selected reply Outbox body");
        let segments = selected["body_segments"].as_array().expect("body segments");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0]["kind"], "authored");
        assert_eq!(segments[0]["content"], "Fresh reply");
        assert_eq!(segments[1]["kind"], "quoted");
        assert_eq!(segments[1]["content"], "Original body");
        assert_eq!(segments[1]["quote_metadata"]["subject"], "Actual subject");
        assert!(!segments[0]["content"].as_str().unwrap().contains("At 2026"));
        assert_no_private_mail_coordinates(&summary);
        assert_no_private_mail_coordinates(&selected);
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
    fn background_launch_requires_the_explicit_autostart_argument() {
        assert!(is_background_launch([
            "Mine Mail.exe".to_owned(),
            "--background".to_owned(),
        ]));
        assert!(!is_background_launch(["Mine Mail.exe".to_owned()]));
        assert!(!is_background_launch([
            "Mine Mail.exe".to_owned(),
            "--background=false".to_owned(),
        ]));
    }

    #[test]
    fn summaries_advertise_html_without_crossing_the_body_boundary() {
        let dto = InboxMessageDto::summary(rich_message());
        let json = serde_json::to_value(dto).expect("serialize summary");

        assert_eq!(json["body_html_available"], true);
        assert_eq!(json["body_html_loaded"], false);
        assert!(json["body_text"].is_null());
        assert!(json["body_html"].is_null());
        assert!(json["body_render_mode"].is_null());
        assert_eq!(json["bcc"][0]["email"], "hidden@example.com");
        assert!(json.get("raw_rfc822").is_none());
        assert_no_private_mail_coordinates(&json);
    }

    #[test]
    fn contact_summaries_include_direction_without_body_or_rfc822_content() {
        let mut message = rich_message();
        message.mailbox = "&XfJT0ZAB-".to_owned();
        message.body_text = None;
        message.body_html = None;
        message.raw_rfc822 = vec![1; 32 * 1024];
        let json = serde_json::to_value(ContactMessageDto::from(ContactMessage {
            public_id: "opaque-contact-message".to_owned(),
            direction: ContactMessageDirection::Outgoing,
            mailbox_role: Some(MailboxRole::Sent),
            message,
        }))
        .expect("serialize contact summary");

        assert_eq!(json["id"], "opaque-contact-message");
        assert_eq!(json["direction"], "outgoing");
        assert_eq!(json["mailbox_role"], "sent");
        assert_eq!(json["subject"], "Rich");
        assert_eq!(json["bcc"][0]["email"], "hidden@example.com");
        assert!(json.get("body_text").is_none());
        assert!(json.get("body_html").is_none());
        assert_no_private_mail_coordinates(&json);
    }

    fn core_draft_boundary_fixture() -> CoreDraftDto {
        CoreDraftDto {
            draft: Draft {
                id: "draft-1".to_owned(),
                local_version: 3,
                has_unsupported_content: false,
                account_id: "primary".to_owned(),
                to: vec!["receiver@example.com".to_owned()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "Draft".to_owned(),
                body_text: "Body".to_owned(),
                format: ComposeFormat {
                    body_html: None,
                    stationery: StationeryTheme::None,
                    send_stationery: false,
                },
                reply_context: None,
                status: "synced".to_owned(),
                remote_mailbox: Some("Drafts".to_owned()),
                remote_uid: Some(41),
                created_at: "2026-07-28T00:00:00Z".to_owned(),
                updated_at: "2026-07-28T00:01:00Z".to_owned(),
                raw_rfc822: b"private draft source".to_vec(),
            },
            attachments: vec![DraftAttachmentMeta {
                id: "opaque-managed-attachment".to_owned(),
                name: "invoice.pdf".to_owned(),
                mime_type: "application/pdf".to_owned(),
                size_bytes: 42,
                source_attachment_id: Some("opaque-source-attachment".to_owned()),
            }],
            forward_context: Some(ForwardContext {
                source_message_id: "9f1a7b32-4b55-4d6d-8db7-0e7bf1a32c41".to_owned(),
                original_subject: "Original".to_owned(),
                from: Some(MailAddress {
                    name: Some("Sender".to_owned()),
                    email: "sender@example.com".to_owned(),
                }),
                to: Vec::new(),
                cc: Vec::new(),
                sent_at: Some("2026-07-27T23:59:00Z".to_owned()),
                quoted_text: "Original body".to_owned(),
                quoted_html: Some("<p>Original body</p>".to_owned()),
                quoted_render_mode: Some(ForwardQuotedRenderMode::NativeHtml),
                source_attachments: vec![AttachmentMeta {
                    id: "opaque-source-attachment".to_owned(),
                    original_name: Some("invoice.pdf".to_owned()),
                    safe_display_name: "invoice.pdf".to_owned(),
                    mime_type: "application/pdf".to_owned(),
                    size_bytes: 42,
                    size_is_estimate: false,
                    disposition: AttachmentDisposition::Attachment,
                }],
            }),
        }
    }

    #[test]
    fn compose_dtos_recursively_omit_account_provider_raw_path_and_byte_fields() {
        let fixture = core_draft_boundary_fixture();
        let draft_json = serde_json::to_value(DraftDto::from(fixture.clone()))
            .expect("serialize draft boundary");

        assert_eq!(draft_json["id"], "draft-1");
        assert_eq!(draft_json["local_version"], 3);
        assert_eq!(draft_json["status"], "synced");
        assert_eq!(
            draft_json["attachments"][0]["id"],
            "opaque-managed-attachment"
        );
        assert_eq!(
            draft_json["forward_context"]["source_attachments"][0]["id"],
            "opaque-source-attachment"
        );
        assert_no_private_mail_coordinates(&draft_json);

        let mutation_json = serde_json::to_value(DraftAttachmentMutationOutcomeDto::from(
            DraftAttachmentMutationOutcome {
                kind: DraftAttachmentMutationKind::ConflictCopy,
                draft: fixture.clone(),
                canonical: Some(fixture.clone()),
            },
        ))
        .expect("serialize attachment mutation");
        assert_no_private_mail_coordinates(&mutation_json);

        let forward_json = serde_json::to_value(ForwardPreparationOutcomeDto::from(
            ForwardPreparationOutcome::Prepared {
                prepared: PreparedForward {
                    draft: fixture,
                    warnings: vec![ForwardWarning::AttachmentsOmittedByUser],
                },
            },
        ))
        .expect("serialize prepared forward");
        assert_eq!(forward_json["kind"], "prepared");
        assert_no_private_mail_coordinates(&forward_json);
    }

    #[test]
    fn attachment_save_result_defensively_exposes_only_a_final_base_name() {
        let json = serde_json::to_value(AttachmentSaveResultDto::from(AttachmentSaveResult {
            status: AttachmentSaveStatus::Saved,
            file_name: Some(r"C:\Users\private\invoice.pdf".to_owned()),
            error_kind: None,
            retryable: false,
        }))
        .expect("serialize attachment save result");

        assert_eq!(json["status"], "saved");
        assert_eq!(json["file_name"], "invoice.pdf");
        assert!(!json.to_string().contains("Users"));
        assert_no_private_mail_coordinates(&json);
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
    fn rich_reply_context_crosses_only_as_sanitized_renderable_html() {
        let dto = ReplyContextDto::from(ReplyContext {
            parent_message_id: Some("parent@example.com".to_owned()),
            references: Vec::new(),
            subject: "Hey tantless".to_owned(),
            sender: Some(MailAddress {
                name: Some("myouo".to_owned()),
                email: "dev@myouo.me".to_owned(),
            }),
            recipients: Vec::new(),
            sent_at: Some("2026-07-13T13:06:24Z".to_owned()),
            quoted_text: "Hey tantless A mail from paa.moe!".to_owned(),
            quoted_html: Some(
                r#"<p onclick="alert(1)">Hey tantless</p><a href="https://paa.moe">paa.moe</a><img alt="avatar" src="data:image/png;base64,AQID">"#
                    .to_owned(),
            ),
        });
        let json = serde_json::to_value(dto).expect("serialize reply context");
        let html = json["quoted_html"].as_str().expect("safe quoted HTML");

        assert!(html.contains("href=\"https://paa.moe\""));
        assert!(html.contains("data:image/png;base64,AQID"));
        assert!(!html.contains("onclick"));
        assert_eq!(json["quoted_render_mode"], "native_html");
        assert_eq!(json["has_remote_images"], false);
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
    fn netease_at_wrote_reply_uses_cached_parent_metadata_at_the_ui_boundary() {
        let mut message = rich_message();
        message.subject = "Re:1".to_owned();
        message.in_reply_to = vec!["parent@example.com".to_owned()];
        message.sender = Some(MailAddress {
            name: Some("Mine Mail".to_owned()),
            email: "receiver@example.com".to_owned(),
        });
        message.body_text = Some(
            "\n\n123\n\nAt 2026-07-17 09:54:29, \"tantless\" <sender@example.com> wrote:\n\nOriginal body"
                .to_owned(),
        );
        message.body_html = Some(
            r#"<div style="background:#aaa">123</div>
               <p>At 2026-07-17 09:54:29, &quot;tantless&quot; &lt;sender@example.com&gt; wrote:</p>
               <blockquote id="isReplyContent" style="margin:0">Original body</blockquote>"#
                .to_owned(),
        );
        let mut parent = rich_message();
        parent.subject = "1".to_owned();
        parent.sender = Some(MailAddress {
            name: Some("tantless".to_owned()),
            email: "sender@example.com".to_owned(),
        });
        parent.to = vec![MailAddress {
            name: Some("Mine Mail".to_owned()),
            email: "receiver@example.com".to_owned(),
        }];
        parent.sent_at = Some("2026-07-17T09:54:29+08:00".to_owned());

        let dto = InboxMessageDto::full_with_parent(message, Some(&parent));
        let json = serde_json::to_value(dto).expect("serialize NetEase reply");
        let segments = json["body_segments"].as_array().expect("body segments");

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0]["content"], "123");
        assert_eq!(segments[0]["render_mode"], "plain");
        assert!(!segments[0]["content"].as_str().unwrap().contains("At 2026"));
        assert!(
            segments[1]["content"]
                .as_str()
                .unwrap()
                .contains("Original body")
        );
        assert_eq!(segments[1]["quote_metadata"]["subject"], "1");
        assert_eq!(
            segments[1]["quote_metadata"]["sender"],
            "tantless <sender@example.com>"
        );
        assert_eq!(
            segments[1]["quote_metadata"]["recipient"],
            "Mine Mail <receiver@example.com>"
        );
        assert_eq!(
            segments[1]["quote_metadata"]["sent_at"],
            "2026-07-17T09:54:29+08:00"
        );
    }

    #[test]
    fn gmail_and_netease_reply_chain_restores_every_cached_quote_header() {
        let mut message = rich_message();
        message.mailbox = "[Gmail]/Sent Mail".to_owned();
        message.message_id = Some("current@mine-mail.invalid".to_owned());
        message.in_reply_to = vec!["parent@mine-mail.invalid".to_owned()];
        message.references = vec![
            "root@mine-mail.invalid".to_owned(),
            "parent@mine-mail.invalid".to_owned(),
        ];
        message.subject = "Re: test1".to_owned();
        message.sender = Some(MailAddress {
            name: None,
            email: "tantless8@gmail.com".to_owned(),
        });
        message.to = vec![MailAddress {
            name: None,
            email: "tantless@163.com".to_owned(),
        }];
        message.sent_at = Some("2026-07-20T01:47:38Z".to_owned());
        message.body_text = Some(
            "ok1\n\nAt 2026-07-20 01:46:26 +00:00, <tantless@163.com> wrote:\n> yes,receive1\n>\n> At 2026-07-20 01:45:53 +00:00, <tantless8@gmail.com> wrote:\n> > testtt"
                .to_owned(),
        );
        message.body_html = Some(
            r#"<div class="mine-mail-authored">ok1</div><br><div class="mine-mail-quote"><div>At 2026-07-20 01:46:26 +00:00, &lt;tantless@163.com&gt; wrote:</div><blockquote id="isReplyContent" type="cite"><div>yes,receive1</div><br><div><div>At 2026-07-20 01:45:53 +00:00, &lt;tantless8@gmail.com&gt; wrote:</div><blockquote>testtt<br></blockquote></div></blockquote></div>"#
                .to_owned(),
        );

        let mut parent = rich_message();
        parent.id = 2;
        parent.mailbox = "INBOX".to_owned();
        parent.uid = 12;
        parent.message_id = Some("parent@mine-mail.invalid".to_owned());
        parent.subject = "Re: test1".to_owned();
        parent.sender = Some(MailAddress {
            name: None,
            email: "tantless@163.com".to_owned(),
        });
        parent.to = vec![MailAddress {
            name: None,
            email: "tantless8@gmail.com".to_owned(),
        }];
        parent.sent_at = Some("2026-07-20T01:46:26Z".to_owned());

        let mut root = rich_message();
        root.id = 3;
        root.mailbox = "[Gmail]/Sent Mail".to_owned();
        root.uid = 34;
        root.message_id = Some("root@mine-mail.invalid".to_owned());
        root.subject = "test1".to_owned();
        root.sender = Some(MailAddress {
            name: None,
            email: "tantless8@gmail.com".to_owned(),
        });
        root.to = vec![MailAddress {
            name: None,
            email: "tantless@163.com".to_owned(),
        }];
        root.sent_at = Some("2026-07-20T01:45:53Z".to_owned());

        let dto = InboxMessageDto::full_with_resolved_ancestors(
            message,
            &[Some(parent), Some(root)],
            &[
                Some(MessageNavigationTargetDto {
                    id: "opaque-parent".to_owned(),
                }),
                Some(MessageNavigationTargetDto {
                    id: "opaque-root".to_owned(),
                }),
            ],
        );
        let json = serde_json::to_value(dto).expect("serialize reply chain");
        let segments = json["body_segments"].as_array().expect("body segments");

        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0]["content"], "ok1");
        assert_eq!(segments[1]["quote_metadata"]["subject"], "Re: test1");
        assert_eq!(segments[1]["quote_metadata"]["sender"], "tantless@163.com");
        assert_eq!(segments[1]["navigation_target"]["id"], "opaque-parent");
        assert_eq!(
            segments[1]["quote_metadata"]["recipient"],
            "tantless8@gmail.com"
        );
        assert_eq!(segments[2]["quote_metadata"]["subject"], "test1");
        assert_eq!(
            segments[2]["quote_metadata"]["sender"],
            "<tantless8@gmail.com>"
        );
        assert_eq!(
            segments[2]["quote_metadata"]["recipient"],
            "tantless@163.com"
        );
        assert_eq!(
            segments[2]["quote_metadata"]["sent_at"],
            "2026-07-20 01:45:53 +00:00"
        );
        assert_eq!(segments[2]["navigation_target"]["id"], "opaque-root");
        assert_no_private_mail_coordinates(&json);
        assert!(json.get("raw_rfc822").is_none());
    }

    #[test]
    fn unrelated_detected_quote_never_inherits_a_cached_navigation_target() {
        let mut message = rich_message();
        message.subject = "Re: mixed thread".to_owned();
        message.in_reply_to = vec!["parent@mine-mail.invalid".to_owned()];
        message.body_text = Some(
            "Current reply\n\nAt 2026-07-20 09:00:00 +00:00, <third@example.com> wrote:\n> Third-party history"
                .to_owned(),
        );
        message.body_html = None;

        let mut parent = rich_message();
        parent.mailbox = "INBOX".to_owned();
        parent.uid = 99;
        parent.sender = Some(MailAddress {
            name: None,
            email: "expected@example.com".to_owned(),
        });

        let dto = InboxMessageDto::full_with_ancestors(message, &[Some(parent)]);
        let json = serde_json::to_value(dto).expect("serialize mixed quote");
        let quoted = json["body_segments"]
            .as_array()
            .expect("body segments")
            .iter()
            .find(|segment| segment["kind"] == "quoted")
            .expect("quoted segment");

        assert_eq!(quoted["quote_metadata"]["sender"], "<third@example.com>");
        assert!(quoted.get("navigation_target").is_none());
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

    #[test]
    fn webview_navigation_keeps_only_app_and_isolated_documents_internal() {
        let app_origin = Url::parse("http://tauri.localhost").expect("app origin");

        for internal in [
            "http://tauri.localhost/index.html",
            "http://tauri.localhost/index.html?surface=new-mail-notification",
            "about:blank",
            "about:srcdoc",
        ] {
            let url = Url::parse(internal).expect("internal URL");
            assert_eq!(
                classify_webview_navigation(&url, &app_origin),
                WebviewNavigationDecision::AllowInternal,
                "{internal} should remain inside the application webview"
            );
        }

        for denied in [
            "javascript:alert(1)",
            "data:text/html,unsafe",
            "file:///tmp/private",
            "about:config",
        ] {
            let url = Url::parse(denied).expect("denied URL");
            assert_eq!(
                classify_webview_navigation(&url, &app_origin),
                WebviewNavigationDecision::Deny,
                "{denied} must never navigate or launch externally"
            );
        }
    }

    #[test]
    fn webview_navigation_routes_safe_external_urls_outside_the_app() {
        let app_origin = Url::parse("tauri://localhost").expect("app origin");

        for external in [
            "https://help.steampowered.com/",
            "http://example.com/message",
            "mailto:friend@example.com",
        ] {
            let url = Url::parse(external).expect("external URL");
            assert_eq!(
                classify_webview_navigation(&url, &app_origin),
                WebviewNavigationDecision::OpenExternal(url),
                "{external} should use the system-owned handler"
            );
        }

        let credentialed =
            Url::parse("https://user:password@example.com/").expect("credentialed URL");
        assert_eq!(
            classify_webview_navigation(&credentialed, &app_origin),
            WebviewNavigationDecision::Deny
        );
    }
}
