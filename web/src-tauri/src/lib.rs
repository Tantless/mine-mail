use std::{env, fs, io, path::PathBuf};

use mine_mail::{
    AccountConfig, ComposeRequest, ConnectionReport, Draft, InboxMessage, MailAddress, MailBackend,
    OutboxItem, OutboxStatus, SyncReport,
};
use serde::Serialize;
use tauri::{Manager, State};

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
async fn check_connections(backend: State<'_, MailBackend>) -> CommandResult<ConnectionReport> {
    backend.check_connections().await.map_err(safe_mail_error)
}

#[tauri::command]
async fn sync_inbox(backend: State<'_, MailBackend>) -> CommandResult<SyncReport> {
    backend
        .sync_inbox(INBOX_SYNC_LIMIT)
        .await
        .map_err(safe_mail_error)
}

#[tauri::command]
fn list_inbox(backend: State<'_, MailBackend>) -> CommandResult<Vec<InboxMessageDto>> {
    backend
        .list_inbox(INBOX_LIST_LIMIT)
        .map(|messages| messages.into_iter().map(Into::into).collect())
        .map_err(safe_mail_error)
}

#[tauri::command]
async fn fetch_message(
    backend: State<'_, MailBackend>,
    uid: u32,
) -> CommandResult<InboxMessageDto> {
    backend
        .fetch_message(uid, false)
        .await
        .map(Into::into)
        .map_err(safe_mail_error)
}

#[tauri::command]
fn save_draft(backend: State<'_, MailBackend>, request: ComposeRequest) -> CommandResult<DraftDto> {
    backend
        .save_draft(request)
        .map(Into::into)
        .map_err(safe_mail_error)
}

#[tauri::command]
fn list_drafts(backend: State<'_, MailBackend>) -> CommandResult<Vec<DraftDto>> {
    backend
        .list_drafts()
        .map(|drafts| drafts.into_iter().map(Into::into).collect())
        .map_err(safe_mail_error)
}

/// SMTP is reachable only through an already-persisted draft and a second,
/// exact recipient confirmation supplied by the UI at send time.
#[tauri::command]
async fn send_draft(
    backend: State<'_, MailBackend>,
    draft_id: String,
    confirmed_recipients: Vec<String>,
) -> CommandResult<OutboxItemDto> {
    let draft = backend
        .list_drafts()
        .map_err(safe_mail_error)?
        .into_iter()
        .find(|draft| draft.id == draft_id)
        .ok_or_else(|| "The selected draft no longer exists.".to_owned())?;

    ensure_recipient_confirmation(&draft, &confirmed_recipients)?;

    backend
        .send_draft(&draft_id)
        .await
        .map(Into::into)
        .map_err(safe_mail_error)
}

#[tauri::command]
fn list_outbox(backend: State<'_, MailBackend>) -> CommandResult<Vec<OutboxItemDto>> {
    backend
        .list_outbox()
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(safe_mail_error)
}

fn ensure_recipient_confirmation(draft: &Draft, confirmed: &[String]) -> CommandResult<()> {
    let expected = normalize_recipients(
        draft
            .to
            .iter()
            .chain(&draft.cc)
            .chain(&draft.bcc)
            .map(String::as_str),
    )?;
    let confirmed = normalize_recipients(confirmed.iter().map(String::as_str))?;

    if expected.is_empty() || expected != confirmed {
        return Err(
            "Recipient confirmation does not exactly match this draft; nothing was sent."
                .to_owned(),
        );
    }
    Ok(())
}

fn normalize_recipients<'a>(
    recipients: impl Iterator<Item = &'a str>,
) -> CommandResult<Vec<String>> {
    let mut normalized = Vec::new();
    for address in recipients {
        let address = address.trim();
        if address.is_empty() {
            return Err("Recipient confirmation contains a blank address.".to_owned());
        }
        normalized.push(address.to_ascii_lowercase());
    }
    normalized.sort_unstable();
    Ok(normalized)
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

fn credentials_file() -> io::Result<PathBuf> {
    if let Some(value) = env::var_os("MINE_MAIL_CREDENTIALS_FILE")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value));
    }

    #[cfg(debug_assertions)]
    {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../password.txt"))
    }

    #[cfg(not(debug_assertions))]
    {
        Err(io::Error::other(
            "MINE_MAIL_CREDENTIALS_FILE must be set for packaged builds",
        ))
    }
}

fn initialize_backend(app: &tauri::App) -> io::Result<MailBackend> {
    let app_data = app
        .path()
        .app_local_data_dir()
        .map_err(|_| io::Error::other("application data directory is unavailable"))?;
    fs::create_dir_all(&app_data)
        .map_err(|_| io::Error::other("application data directory could not be created"))?;

    let config = AccountConfig::from_163_password_file(credentials_file()?)
        .map_err(|_| io::Error::other("mail account credentials could not be loaded"))?;
    let backend = MailBackend::open(config, app_data.join("mine-mail.sqlite3"))
        .map_err(|_| io::Error::other("the local mail database could not be opened"))?;
    backend
        .initialize()
        .map_err(|_| io::Error::other("the local mail database could not be initialized"))?;
    Ok(backend)
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let backend = initialize_backend(app)?;
            app.manage(backend);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_connections,
            sync_inbox,
            list_inbox,
            fetch_message,
            save_draft,
            list_drafts,
            send_draft,
            list_outbox,
        ])
        .run(tauri::generate_context!())
        .expect("Mine Mail desktop runtime failed");
}

#[cfg(test)]
mod tests {
    use mine_mail::Draft;

    use super::ensure_recipient_confirmation;

    fn draft() -> Draft {
        Draft {
            id: "draft-1".to_owned(),
            account_id: "primary".to_owned(),
            to: vec!["Alice@Example.com".to_owned()],
            cc: vec!["bob@example.com".to_owned()],
            bcc: vec![],
            subject: "Test".to_owned(),
            body_text: "Body".to_owned(),
            status: "local".to_owned(),
            remote_mailbox: None,
            remote_uid: None,
            created_at: "2026-07-14T00:00:00Z".to_owned(),
            updated_at: "2026-07-14T00:00:00Z".to_owned(),
            raw_rfc822: Vec::new(),
        }
    }

    #[test]
    fn recipient_confirmation_is_order_and_case_insensitive_but_exact() {
        let draft = draft();
        assert!(
            ensure_recipient_confirmation(
                &draft,
                &[
                    " bob@example.com ".to_owned(),
                    "alice@example.com".to_owned()
                ]
            )
            .is_ok()
        );
        assert!(ensure_recipient_confirmation(&draft, &["alice@example.com".to_owned()]).is_err());
        assert!(
            ensure_recipient_confirmation(
                &draft,
                &[
                    "alice@example.com".to_owned(),
                    "bob@example.com".to_owned(),
                    "mallory@example.com".to_owned(),
                ]
            )
            .is_err()
        );
    }
}
