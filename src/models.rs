use serde::{Deserialize, Serialize};

use crate::{MailError, Result};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct MailAddress {
    pub name: Option<String>,
    pub email: String,
}

/// Normalizes an address for Mine Mail's local contact identity.
///
/// Mailbox local-parts are technically allowed to be case-sensitive, but the
/// providers supported by Mine Mail and the existing local avatar override
/// behavior treat complete addresses case-insensitively. Keeping one shared
/// normalized key also prevents duplicate contacts that differ only by case or
/// incidental surrounding whitespace.
pub fn normalize_contact_email(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > 320
        || trimmed.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '<' | '>' | ',' | ';')
        })
        || trimmed.matches('@').count() != 1
    {
        return Err(MailError::Validation(
            "a valid contact email address is required".to_owned(),
        ));
    }

    let (local, domain) = trimmed.split_once('@').expect("one @ was checked above");
    let domain_is_valid = !domain.is_empty()
        && domain.len() <= 255
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..")
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|character| character.is_alphanumeric() || character == '-')
        });
    let local_is_valid = !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..");
    if !local_is_valid || !domain_is_valid {
        return Err(MailError::Validation(
            "a valid contact email address is required".to_owned(),
        ));
    }

    Ok(trimmed.to_ascii_lowercase())
}

/// Bounded contact activity derived only from cached message headers. It is
/// combined with the desktop-wide local contact record at the Tauri boundary;
/// no body, HTML, or RFC822 content is carried here.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ContactActivity {
    pub email: String,
    pub display_name: Option<String>,
    pub message_count: usize,
    pub last_message_at: Option<String>,
    pub last_subject: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactMessageDirection {
    Incoming,
    Outgoing,
}

/// Semantic role of a cached mailbox. The provider's exact mailbox name stays
/// on `InboxMessage` as an opaque IMAP identifier; this role is safe to render
/// consistently even when that identifier uses IMAP modified UTF-7.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxRole {
    Inbox,
    Sent,
    Drafts,
    Archive,
    Trash,
}

impl MailboxRole {
    pub(crate) const ALL: [Self; 5] = [
        Self::Inbox,
        Self::Sent,
        Self::Drafts,
        Self::Archive,
        Self::Trash,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Sent => "sent",
            Self::Drafts => "drafts",
            Self::Archive => "archive",
            Self::Trash => "trash",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "inbox" => Some(Self::Inbox),
            "sent" => Some(Self::Sent),
            "drafts" => Some(Self::Drafts),
            "archive" => Some(Self::Archive),
            "trash" => Some(Self::Trash),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxCapabilityStatus {
    DiscoveryPending,
    Available,
    NeedsCreationConfirmation,
    Unavailable,
}

impl MailboxCapabilityStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::DiscoveryPending => "discovery_pending",
            Self::Available => "available",
            Self::NeedsCreationConfirmation => "needs_creation_confirmation",
            Self::Unavailable => "unavailable",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "discovery_pending" => Some(Self::DiscoveryPending),
            "available" => Some(Self::Available),
            "needs_creation_confirmation" => Some(Self::NeedsCreationConfirmation),
            "unavailable" => Some(Self::Unavailable),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxCapabilityUnavailableReason {
    CreateNotSupported,
    CreateFailed,
    CreatedMailboxNotSelectable,
    ProviderUnsupported,
}

impl MailboxCapabilityUnavailableReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::CreateNotSupported => "create_not_supported",
            Self::CreateFailed => "create_failed",
            Self::CreatedMailboxNotSelectable => "created_mailbox_not_selectable",
            Self::ProviderUnsupported => "provider_unsupported",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "create_not_supported" => Some(Self::CreateNotSupported),
            "create_failed" => Some(Self::CreateFailed),
            "created_mailbox_not_selectable" => Some(Self::CreatedMailboxNotSelectable),
            "provider_unsupported" => Some(Self::ProviderUnsupported),
            _ => None,
        }
    }
}

/// Account-scoped availability of one semantic mailbox role. Provider mailbox
/// names remain in Rust and SQLite; React receives only this bounded status.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct MailboxCapability {
    pub role: MailboxRole,
    pub status: MailboxCapabilityStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<MailboxCapabilityUnavailableReason>,
    pub retryable: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteHistoryState {
    #[default]
    NotChecked,
    MayHaveMore,
    Offline,
    Complete,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationStatus {
    Pending,
    InFlight,
    Confirmed,
    NeedsAttention,
    OutcomeUnknown,
}

impl MutationStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InFlight => "in_flight",
            Self::Confirmed => "confirmed",
            Self::NeedsAttention => "needs_attention",
            Self::OutcomeUnknown => "outcome_unknown",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "in_flight" => Some(Self::InFlight),
            "confirmed" => Some(Self::Confirmed),
            "needs_attention" => Some(Self::NeedsAttention),
            "outcome_unknown" => Some(Self::OutcomeUnknown),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemFlagKind {
    Seen,
    Flagged,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteMutationPhase {
    #[default]
    Queued,
    TransferStarted,
    TransferAcknowledged,
    SourceDeleteStarted,
    SourceDeleteAcknowledged,
}

impl RemoteMutationPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::TransferStarted => "transfer_started",
            Self::TransferAcknowledged => "transfer_acknowledged",
            Self::SourceDeleteStarted => "source_delete_started",
            Self::SourceDeleteAcknowledged => "source_delete_acknowledged",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "transfer_started" => Some(Self::TransferStarted),
            "transfer_acknowledged" => Some(Self::TransferAcknowledged),
            "source_delete_started" => Some(Self::SourceDeleteStarted),
            "source_delete_acknowledged" => Some(Self::SourceDeleteAcknowledged),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageActionKind {
    Archive,
    MoveToTrash,
    PermanentDelete,
}

impl MessageActionKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Archive => "archive",
            Self::MoveToTrash => "move_to_trash",
            Self::PermanentDelete => "permanent_delete",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "archive" => Some(Self::Archive),
            "move_to_trash" => Some(Self::MoveToTrash),
            "permanent_delete" => Some(Self::PermanentDelete),
            _ => None,
        }
    }
}

/// Privacy-safe reason attached to a mutation requiring recovery. It contains
/// no mailbox address, subject, body, path, or server response text.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageMutationErrorKind {
    UidValidityChanged,
    SourceMissing,
    AmbiguousRemoteState,
    NetworkUnavailable,
    MailboxUnavailable,
    PermissionDenied,
    ServerRejected,
    Unsupported,
    Unknown,
}

impl MessageMutationErrorKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::UidValidityChanged => "uid_validity_changed",
            Self::SourceMissing => "source_missing",
            Self::AmbiguousRemoteState => "ambiguous_remote_state",
            Self::NetworkUnavailable => "network_unavailable",
            Self::MailboxUnavailable => "mailbox_unavailable",
            Self::PermissionDenied => "permission_denied",
            Self::ServerRejected => "server_rejected",
            Self::Unsupported => "unsupported",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "uid_validity_changed" => Some(Self::UidValidityChanged),
            "source_missing" => Some(Self::SourceMissing),
            "ambiguous_remote_state" => Some(Self::AmbiguousRemoteState),
            "network_unavailable" => Some(Self::NetworkUnavailable),
            "mailbox_unavailable" => Some(Self::MailboxUnavailable),
            "permission_denied" => Some(Self::PermissionDenied),
            "server_rejected" => Some(Self::ServerRejected),
            "unsupported" => Some(Self::Unsupported),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// Opaque keyset cursor. Desktop and React callers must return this string
/// unchanged and must not derive mailbox names, UIDs, or search behavior from it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct MessagePageCursor(String);

impl MessagePageCursor {
    pub(crate) fn new(encoded: String) -> Self {
        Self(encoded)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Rust-only contact-history aggregate. IPC layers must map this value into an
/// explicit body-free DTO keyed by `public_id`; deriving serde here would make
/// it too easy to leak the nested row ID, account ID, mailbox, or UID.
///
/// Direction is derived from the configured account identity rather than
/// provider-specific mailbox names, which are not portable across IMAP
/// servers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContactMessage {
    /// Opaque, account-scoped identity for opening this cached message without
    /// exposing an IMAP mailbox/UID tuple as an application capability.
    pub public_id: String,
    pub direction: ContactMessageDirection,
    pub mailbox_role: Option<MailboxRole>,
    pub message: InboxMessage,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ComposeRequest {
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    #[serde(default)]
    pub format: ComposeFormat,
    #[serde(default)]
    pub reply_context: Option<ReplyContext>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StationeryTheme {
    #[default]
    None,
    Lined,
    Grid,
}

impl StationeryTheme {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lined => "lined",
            Self::Grid => "grid",
        }
    }

    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "lined" => Self::Lined,
            "grid" => Self::Grid,
            _ => Self::None,
        }
    }
}

/// Mine Mail-authored rich composition data.
///
/// The plain-text body remains authoritative for interoperability, previews,
/// notifications, and clients that do not render HTML. `body_html` is a
/// bounded, sanitized fragment containing only editor-owned formatting.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct ComposeFormat {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_html: Option<String>,
    #[serde(default)]
    pub stationery: StationeryTheme,
    #[serde(default)]
    pub send_stationery: bool,
}

/// Immutable context captured when a reply composer is created. The editable
/// body remains separate so quoted history cannot accidentally become ordinary
/// authored text. Rust uses this snapshot to build standards-compliant reply
/// headers and MIME at send time.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ReplyContext {
    pub parent_message_id: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
    pub subject: String,
    pub sender: Option<MailAddress>,
    #[serde(default)]
    pub recipients: Vec<MailAddress>,
    pub sent_at: Option<String>,
    pub quoted_text: String,
    /// Optional rich alternative for the quoted body. Desktop callers must
    /// sanitize this fragment before it crosses into React; the plain text is
    /// always retained as the interoperability and accessibility fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quoted_html: Option<String>,
}

impl ComposeRequest {
    pub fn validate(&self) -> Result<()> {
        if self.to.is_empty() && self.cc.is_empty() && self.bcc.is_empty() {
            return Err(MailError::Validation(
                "at least one recipient is required".to_owned(),
            ));
        }
        if self
            .to
            .iter()
            .chain(&self.cc)
            .chain(&self.bcc)
            .any(|address| address.trim().is_empty())
        {
            return Err(MailError::Validation(
                "recipient addresses cannot be blank".to_owned(),
            ));
        }
        if self
            .format
            .body_html
            .as_ref()
            .is_some_and(|html| html.len() > 512 * 1024)
        {
            return Err(MailError::Validation(
                "formatted message body is too large".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn all_recipients(&self) -> impl Iterator<Item = &String> {
        self.to.iter().chain(&self.cc).chain(&self.bcc)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct InboxMessage {
    pub id: i64,
    pub account_id: String,
    pub mailbox: String,
    pub uid: u32,
    pub message_id: Option<String>,
    #[serde(default)]
    pub in_reply_to: Vec<String>,
    #[serde(default)]
    pub references: Vec<String>,
    pub subject: String,
    pub sender: Option<MailAddress>,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    /// Addresses from an actual RFC822 `Bcc` header, when one was present in
    /// the cached message. This is never inferred from the account identity or
    /// transport envelope.
    #[serde(default)]
    pub bcc: Vec<MailAddress>,
    pub sent_at: Option<String>,
    pub internal_date: Option<String>,
    pub flags: Vec<String>,
    pub size_bytes: u32,
    pub preview: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub attachment_names: Vec<String>,
    pub body_fetched: bool,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub raw_rfc822: Vec<u8>,
    pub synced_at: String,
}

/// A message shown in one semantic mailbox. Pending moves retain the real
/// source mailbox and UID in `message`; `displayed_role` and
/// `pending_mutation` make the unconfirmed destination projection explicit.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct MessagePageItem {
    /// Account-bound, restart-stable identity exposed to the desktop UI.
    /// SQLite row IDs, provider mailbox names, and IMAP UIDs remain Rust-only.
    pub public_id: String,
    #[serde(flatten)]
    pub message: InboxMessage,
    pub displayed_role: MailboxRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_mutation: Option<PendingMessageProjection>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PendingMessageProjection {
    pub operation_id: String,
    pub local_revision: u64,
    pub status: MutationStatus,
    pub kind: MessageActionKind,
    pub source_role: MailboxRole,
    pub destination_role: MailboxRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<MessageMutationErrorKind>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct MessagePage {
    pub items: Vec<MessagePageItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<MessagePageCursor>,
    pub has_more_local: bool,
    pub remote_history_state: RemoteHistoryState,
    pub end_reached: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct MessageMutationReceipt {
    pub operation_id: String,
    pub local_revision: u64,
    pub status: MutationStatus,
    pub source_role: MailboxRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_role: Option<MailboxRole>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct SystemFlagMutationReceipt {
    pub operation_id: String,
    pub local_revision: u64,
    pub status: MutationStatus,
    pub source_role: MailboxRole,
    pub flag: SystemFlagKind,
    pub desired: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentDisposition {
    Attachment,
    Inline,
}

/// Bounded metadata for one attachment. `id` is opaque to React and is never a
/// MIME part number or path. A remote BODYSTRUCTURE listing can report only the
/// transfer-encoded size, so `size_is_estimate` remains explicit until the part
/// has been decoded.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct AttachmentMeta {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_name: Option<String>,
    pub safe_display_name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub size_is_estimate: bool,
    pub disposition: AttachmentDisposition,
}

/// Bounded metadata for one immutable blob associated with an exact draft
/// version. A forwarded ordinary attachment retains only its opaque source ID.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DraftAttachmentMeta {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_attachment_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForwardQuotedRenderMode {
    Plain,
    NativeHtml,
    IsolatedHtml,
}

/// Immutable source snapshot captured when a forward draft is prepared.
/// Authored body text and the mutable staged attachment set live outside this
/// value so later edits cannot replace the original message inventory.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ForwardContext {
    pub source_message_id: String,
    pub original_subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<MailAddress>,
    #[serde(default)]
    pub to: Vec<MailAddress>,
    #[serde(default)]
    pub cc: Vec<MailAddress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<String>,
    pub quoted_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quoted_html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quoted_render_mode: Option<ForwardQuotedRenderMode>,
    #[serde(default)]
    pub source_attachments: Vec<AttachmentMeta>,
}

/// Rust-only compose aggregate. IPC layers must explicitly map these fields
/// into a safe boundary DTO rather than serializing or flattening the nested
/// `Draft`, which also contains account and provider positioning data.
///
/// ```compile_fail
/// fn require_serialize<T: serde::Serialize>() {}
/// require_serialize::<mine_mail::DraftDto>();
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftDto {
    pub draft: Draft,
    pub attachments: Vec<DraftAttachmentMeta>,
    pub forward_context: Option<ForwardContext>,
}

impl From<Draft> for DraftDto {
    fn from(draft: Draft) -> Self {
        Self {
            draft,
            attachments: Vec::new(),
            forward_context: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftAttachmentMutationKind {
    Saved,
    ConflictCopy,
    Stale,
    Canceled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftAttachmentMutationOutcome {
    pub kind: DraftAttachmentMutationKind,
    pub draft: DraftDto,
    pub canonical: Option<DraftDto>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentSaveStatus {
    Saved,
    Canceled,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentSaveErrorKind {
    MessageUnavailable,
    AttachmentNotFound,
    PermissionDenied,
    DiskFull,
    WriteFailed,
}

/// Typed Save As result. `file_name` is always a final base name and must
/// never contain a directory or complete local path.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct AttachmentSaveResult {
    pub status: AttachmentSaveStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<AttachmentSaveErrorKind>,
    pub retryable: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForwardWarning {
    HtmlDowngraded,
    InlineResourcesNotForwarded,
    AttachmentsOmittedByUser,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedForward {
    pub draft: DraftDto,
    pub warnings: Vec<ForwardWarning>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForwardPreparationErrorKind {
    MessageUnavailable,
    BodyUnavailable,
    AttachmentUnavailable,
    AttachmentStageFailed,
    SourceChanged,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ForwardPreparationError {
    pub kind: ForwardPreparationErrorKind,
    #[serde(default)]
    pub failed_attachment_ids: Vec<String>,
    pub retry_without_attachments_allowed: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForwardPreparationOutcomeKind {
    Prepared,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForwardPreparationOutcome {
    Prepared { prepared: PreparedForward },
    Error { error: ForwardPreparationError },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Draft {
    pub id: String,
    /// Monotonic SQLite row token used for optimistic editor saves. It is
    /// intentionally independent from the IMAP `X-Mine-Mail-Draft-Revision`.
    pub local_version: u64,
    /// True when the original MIME is not a Mine Mail-owned restricted rich
    /// draft and contains content the editor cannot round-trip safely (HTML,
    /// multipart, inline data, attachments, or an unparseable body). Such drafts
    /// are exposed read-only.
    pub has_unsupported_content: bool,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub account_id: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub format: ComposeFormat,
    pub reply_context: Option<ReplyContext>,
    pub status: String,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub remote_mailbox: Option<String>,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub remote_uid: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub raw_rfc822: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftSaveKind {
    Saved,
    ConflictCopy,
}

/// Typed result of an optimistic local draft save. A conflict never mutates
/// the canonical row: `draft` is a newly inserted local conflict copy and
/// `canonical` is the newest visible canonical draft, when it still exists.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DraftSaveOutcome {
    pub kind: DraftSaveKind,
    pub draft: Draft,
    pub canonical: Option<Draft>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftDeleteKind {
    Deleted,
    Stale,
}

impl Draft {
    pub fn compose_request(&self) -> ComposeRequest {
        ComposeRequest {
            to: self.to.clone(),
            cc: self.cc.clone(),
            bcc: self.bcc.clone(),
            subject: self.subject.clone(),
            body_text: self.body_text.clone(),
            format: self.format.clone(),
            reply_context: self.reply_context.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxStatus {
    Queued,
    Sending,
    Sent,
    Retryable,
    Rejected,
    DeliveryUnknown,
}

/// The two explicit user decisions available for an ambiguous SMTP outcome.
///
/// A retry is deliberately named as a single attempt. If that attempt also
/// ends in `delivery_unknown`, the caller must load the new attempt generation
/// and make another explicit decision.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryUnknownDecision {
    ConfirmDelivered,
    RetryOnce,
}

impl OutboxStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Retryable => "retryable",
            Self::Rejected => "rejected",
            Self::DeliveryUnknown => "delivery_unknown",
        }
    }

    pub(crate) fn from_str(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "sending" => Ok(Self::Sending),
            "sent" => Ok(Self::Sent),
            "retryable" => Ok(Self::Retryable),
            "rejected" => Ok(Self::Rejected),
            "delivery_unknown" => Ok(Self::DeliveryUnknown),
            other => Err(MailError::Database(rusqlite::Error::InvalidParameterName(
                format!("unknown outbox status {other}"),
            ))),
        }
    }
}

/// Exact authored recipient grouping captured for a newly created immutable
/// Outbox item. Legacy Outbox rows use `None`; their flat SMTP envelope cannot
/// safely reconstruct whether an address originally appeared in To, Cc, or Bcc.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct OutboxRecipientGroups {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
}

impl From<&ComposeRequest> for OutboxRecipientGroups {
    fn from(request: &ComposeRequest) -> Self {
        Self {
            to: request.to.clone(),
            cc: request.cc.clone(),
            bcc: request.bcc.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct OutboxItem {
    pub id: String,
    pub account_id: String,
    pub draft_id: Option<String>,
    /// Mail protocol revision embedded in the draft MIME at send time.
    pub draft_revision: Option<u64>,
    /// Monotonic local row token bound to the UI confirmation and send. Unlike
    /// the protocol revision, external draft content cannot reuse this token.
    pub draft_local_version: Option<u64>,
    pub recipients: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_groups: Option<OutboxRecipientGroups>,
    pub status: OutboxStatus,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub created_at: String,
    pub sent_at: Option<String>,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub raw_rfc822: Vec<u8>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub mailbox: String,
    pub remote_total: u32,
    pub fetched: usize,
    pub updated_flags: usize,
    pub removed: usize,
    pub cached_total: usize,
    pub uid_validity_reset: bool,
}

/// Progress emitted after one bounded synchronization batch has been persisted.
///
/// The desktop layer uses this body-free counter to refresh SQLite-backed
/// summaries without exposing protocol responses or message contents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncBatchProgress {
    pub completed: usize,
    pub total: usize,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct ConnectionReport {
    pub imap_ok: bool,
    pub smtp_ok: bool,
}

/// Result of reconciling the local draft store with the remote IMAP Drafts
/// mailbox.
///
/// Conflict policy is deliberately deterministic and data preserving:
///
/// - a remote-only edit replaces an unchanged local draft;
/// - a local-only edit replaces the remote copy;
/// - concurrent edits keep the remote version as the canonical draft and save
///   the local edit as a new local-only conflict copy;
/// - a remote deletion removes an unchanged local draft, but a locally edited
///   draft is recreated remotely;
/// - a local deletion removes an unchanged remote draft, while a concurrently
///   edited remote draft wins and is restored locally.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct DraftSyncReport {
    pub mailbox: String,
    pub pulled: usize,
    pub pushed: usize,
    pub deleted_local: usize,
    pub deleted_remote: usize,
    pub conflicts: usize,
    pub skipped: usize,
    pub local_total: usize,
}

#[cfg(test)]
mod tests {
    use super::{
        AttachmentSaveErrorKind, AttachmentSaveResult, AttachmentSaveStatus, Draft,
        DraftAttachmentMutationKind, DraftDto, ForwardPreparationError,
        ForwardPreparationErrorKind, ForwardWarning, MailboxCapabilityStatus,
        MailboxCapabilityUnavailableReason, MailboxRole, MessageActionKind,
        MessageMutationErrorKind, MutationStatus, RemoteHistoryState, normalize_contact_email,
    };

    #[test]
    fn contact_email_normalization_is_case_insensitive_and_rejects_invalid_keys() {
        assert_eq!(
            normalize_contact_email("  Person@Example.COM ").expect("valid address"),
            "person@example.com"
        );
        for invalid in [
            "",
            "missing-at.example.com",
            "two@@example.com",
            ".person@example.com",
            "person@example..com",
            "person@-example.com",
            "Person <person@example.com>",
        ] {
            assert!(
                normalize_contact_email(invalid).is_err(),
                "{invalid} should be rejected"
            );
        }
    }

    #[test]
    fn mailbox_history_and_mutation_enums_use_the_product_contract_values() {
        for (value, expected) in [
            (
                serde_json::to_string(&MailboxRole::Drafts).unwrap(),
                "\"drafts\"",
            ),
            (
                serde_json::to_string(&MailboxCapabilityStatus::NeedsCreationConfirmation).unwrap(),
                "\"needs_creation_confirmation\"",
            ),
            (
                serde_json::to_string(
                    &MailboxCapabilityUnavailableReason::CreatedMailboxNotSelectable,
                )
                .unwrap(),
                "\"created_mailbox_not_selectable\"",
            ),
            (
                serde_json::to_string(&RemoteHistoryState::MayHaveMore).unwrap(),
                "\"may_have_more\"",
            ),
            (
                serde_json::to_string(&MutationStatus::OutcomeUnknown).unwrap(),
                "\"outcome_unknown\"",
            ),
            (
                serde_json::to_string(&MessageActionKind::MoveToTrash).unwrap(),
                "\"move_to_trash\"",
            ),
            (
                serde_json::to_string(&MessageMutationErrorKind::UidValidityChanged).unwrap(),
                "\"uid_validity_changed\"",
            ),
        ] {
            assert_eq!(value, expected);
        }
    }

    #[test]
    fn attachment_and_forward_dtos_use_the_product_contract_values() {
        assert_eq!(
            serde_json::to_string(&DraftAttachmentMutationKind::ConflictCopy).unwrap(),
            "\"conflict_copy\""
        );
        assert_eq!(
            serde_json::to_string(&ForwardWarning::InlineResourcesNotForwarded).unwrap(),
            "\"inline_resources_not_forwarded\""
        );
        let canceled = serde_json::to_value(AttachmentSaveResult {
            status: AttachmentSaveStatus::Canceled,
            file_name: None,
            error_kind: None,
            retryable: false,
        })
        .unwrap();
        assert_eq!(canceled["status"], "canceled");
        assert!(canceled.get("file_name").is_none());
        assert!(canceled.get("error_kind").is_none());

        let error = serde_json::to_value(ForwardPreparationError {
            kind: ForwardPreparationErrorKind::AttachmentStageFailed,
            failed_attachment_ids: vec!["opaque-part".to_owned()],
            retry_without_attachments_allowed: true,
        })
        .unwrap();
        assert_eq!(error["kind"], "attachment_stage_failed");
        assert_eq!(error["failed_attachment_ids"][0], "opaque-part");
        assert_eq!(
            serde_json::to_string(&AttachmentSaveErrorKind::PermissionDenied).unwrap(),
            "\"permission_denied\""
        );
    }

    #[test]
    fn draft_dto_retains_internal_state_only_for_explicit_boundary_mapping() {
        let dto = DraftDto::from(Draft {
            id: "draft-safe-boundary".to_owned(),
            local_version: 3,
            has_unsupported_content: false,
            account_id: "private-account".to_owned(),
            to: vec!["receiver@example.com".to_owned()],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Subject".to_owned(),
            body_text: "Body".to_owned(),
            format: Default::default(),
            reply_context: None,
            status: "local".to_owned(),
            remote_mailbox: Some("Provider/Drafts".to_owned()),
            remote_uid: Some(42),
            created_at: "2026-07-28T00:00:00Z".to_owned(),
            updated_at: "2026-07-28T00:00:01Z".to_owned(),
            raw_rfc822: b"private RFC822".to_vec(),
        });

        assert_eq!(dto.draft.account_id, "private-account");
        assert_eq!(dto.draft.remote_mailbox.as_deref(), Some("Provider/Drafts"));
        assert_eq!(dto.draft.remote_uid, Some(42));
        assert_eq!(dto.draft.raw_rfc822, b"private RFC822");
    }
}
