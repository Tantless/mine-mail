use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{ArgAction, Args, Parser, Subcommand};
use mine_mail::{
    AccountConfig, ComposeRequest, Draft, InboxMessage, MailAddress, MailBackend, MailError,
    OutboxItem, Result,
};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(
    name = "mine-mail",
    version,
    about = "Safe acceptance CLI for the Mine Mail backend"
)]
struct Cli {
    /// Two-line 163 credentials file: email followed by authorization password.
    #[arg(long, value_name = "PATH")]
    credentials: PathBuf,

    /// Local SQLite database path.
    #[arg(long, default_value = "data/mine-mail.db", value_name = "PATH")]
    database: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create or migrate the local database.
    Init,

    /// Verify authenticated IMAP and SMTP connections.
    Check,

    /// List remote mailbox names.
    Folders,

    /// Incrementally synchronize Inbox metadata into SQLite.
    SyncInbox {
        #[arg(long, default_value_t = 50, value_name = "COUNT")]
        initial_limit: usize,
    },

    /// List locally cached Inbox metadata (message bodies are omitted).
    ListInbox {
        #[arg(long, default_value_t = 20, value_name = "COUNT")]
        limit: usize,
    },

    /// Fetch and cache one message body, while printing metadata only.
    FetchMessage {
        uid: u32,

        /// Download again even when a cached body is available.
        #[arg(long)]
        force: bool,
    },

    /// Save a local-first draft.
    DraftSave {
        #[command(flatten)]
        compose: ComposeArgs,
    },

    /// List local drafts (draft bodies are omitted).
    DraftList,

    /// Synchronize a local draft to the remote Drafts mailbox.
    DraftSync {
        id: String,

        /// Override remote Drafts mailbox discovery.
        #[arg(long)]
        mailbox: Option<String>,
    },

    /// Queue and send a newly composed message.
    Send {
        #[command(flatten)]
        compose: ComposeArgs,

        /// Repeat for every intended recipient. The normalized set must match
        /// To, Cc and Bcc exactly before any SMTP call is made.
        #[arg(
            long = "confirm-recipient",
            value_name = "ADDRESS",
            action = ArgAction::Append,
            required = true
        )]
        confirm_recipient: Vec<String>,
    },

    /// Send an existing draft after exact recipient confirmation.
    SendDraft {
        id: String,

        /// Repeat for every intended recipient. The normalized set must match
        /// the current saved draft exactly before any SMTP call is made.
        #[arg(
            long = "confirm-recipient",
            value_name = "ADDRESS",
            action = ArgAction::Append,
            required = true
        )]
        confirm_recipient: Vec<String>,
    },

    /// List local outbox delivery state (raw messages are omitted).
    Outbox,

    /// Manually retry one item whose current state is exactly `retryable`.
    /// The persisted message and envelope are reused byte-for-byte.
    RetryOutbox { id: String },
}

#[derive(Clone, Debug, Args)]
struct ComposeArgs {
    #[arg(long, value_name = "ADDRESS", action = ArgAction::Append)]
    to: Vec<String>,

    #[arg(long, value_name = "ADDRESS", action = ArgAction::Append)]
    cc: Vec<String>,

    #[arg(long, value_name = "ADDRESS", action = ArgAction::Append)]
    bcc: Vec<String>,

    #[arg(long, default_value = "", value_name = "TEXT")]
    subject: String,

    #[arg(long, default_value = "", value_name = "TEXT")]
    body: String,
}

impl From<ComposeArgs> for ComposeRequest {
    fn from(args: ComposeArgs) -> Self {
        Self {
            to: args.to,
            cc: args.cc,
            bcc: args.bcc,
            subject: args.subject,
            body_text: args.body,
        }
    }
}

#[derive(Serialize)]
struct InboxSummary<'a> {
    id: i64,
    account_id: &'a str,
    mailbox: &'a str,
    uid: u32,
    message_id: &'a Option<String>,
    subject: &'a str,
    sender: &'a Option<MailAddress>,
    to: &'a [MailAddress],
    cc: &'a [MailAddress],
    sent_at: &'a Option<String>,
    internal_date: &'a Option<String>,
    flags: &'a [String],
    size_bytes: u32,
    attachment_names: &'a [String],
    body_fetched: bool,
    text_body_chars: usize,
    html_body_chars: usize,
    synced_at: &'a str,
}

impl<'a> From<&'a InboxMessage> for InboxSummary<'a> {
    fn from(message: &'a InboxMessage) -> Self {
        Self {
            id: message.id,
            account_id: &message.account_id,
            mailbox: &message.mailbox,
            uid: message.uid,
            message_id: &message.message_id,
            subject: &message.subject,
            sender: &message.sender,
            to: &message.to,
            cc: &message.cc,
            sent_at: &message.sent_at,
            internal_date: &message.internal_date,
            flags: &message.flags,
            size_bytes: message.size_bytes,
            attachment_names: &message.attachment_names,
            body_fetched: message.body_fetched,
            text_body_chars: message
                .body_text
                .as_deref()
                .map(str::chars)
                .map(Iterator::count)
                .unwrap_or_default(),
            html_body_chars: message
                .body_html
                .as_deref()
                .map(str::chars)
                .map(Iterator::count)
                .unwrap_or_default(),
            synced_at: &message.synced_at,
        }
    }
}

#[derive(Serialize)]
struct DraftSummary<'a> {
    id: &'a str,
    account_id: &'a str,
    to: &'a [String],
    cc: &'a [String],
    bcc: &'a [String],
    subject: &'a str,
    body_chars: usize,
    status: &'a str,
    remote_mailbox: &'a Option<String>,
    remote_uid: Option<u32>,
    created_at: &'a str,
    updated_at: &'a str,
}

impl<'a> From<&'a Draft> for DraftSummary<'a> {
    fn from(draft: &'a Draft) -> Self {
        Self {
            id: &draft.id,
            account_id: &draft.account_id,
            to: &draft.to,
            cc: &draft.cc,
            bcc: &draft.bcc,
            subject: &draft.subject,
            body_chars: draft.body_text.chars().count(),
            status: &draft.status,
            remote_mailbox: &draft.remote_mailbox,
            remote_uid: draft.remote_uid,
            created_at: &draft.created_at,
            updated_at: &draft.updated_at,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(data) => {
            println!("{}", success_json(data));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error_json(&error));
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<Value> {
    let config = AccountConfig::from_163_password_file(&cli.credentials)?;
    let backend = MailBackend::open(config, &cli.database)?;
    backend.initialize()?;

    match cli.command {
        Command::Init => Ok(json!({
            "initialized": true,
            "database": display_path(&cli.database),
        })),
        Command::Check => Ok(json!(backend.check_connections().await?)),
        Command::Folders => Ok(json!(backend.list_remote_mailboxes().await?)),
        Command::SyncInbox { initial_limit } => {
            require_positive(initial_limit, "initial-limit")?;
            Ok(json!(backend.sync_inbox(initial_limit).await?))
        }
        Command::ListInbox { limit } => {
            require_positive(limit, "limit")?;
            let messages = backend.list_inbox(limit)?;
            let summaries = messages.iter().map(InboxSummary::from).collect::<Vec<_>>();
            Ok(json!(summaries))
        }
        Command::FetchMessage { uid, force } => {
            if uid == 0 {
                return Err(MailError::Validation(
                    "message UID must be greater than zero".to_owned(),
                ));
            }
            let message = backend.fetch_message(uid, force).await?;
            Ok(json!(InboxSummary::from(&message)))
        }
        Command::DraftSave { compose } => {
            let request = ComposeRequest::from(compose);
            let draft = backend.save_draft(request)?;
            Ok(json!(DraftSummary::from(&draft)))
        }
        Command::DraftList => {
            let drafts = backend.list_drafts()?;
            let summaries = drafts.iter().map(DraftSummary::from).collect::<Vec<_>>();
            Ok(json!(summaries))
        }
        Command::DraftSync { id, mailbox } => {
            let draft = backend.sync_draft(&id, mailbox.as_deref()).await?;
            Ok(json!(DraftSummary::from(&draft)))
        }
        Command::Send {
            compose,
            confirm_recipient,
        } => {
            let request = ComposeRequest::from(compose);
            require_exact_recipient_confirmation(&request, &confirm_recipient)?;
            Ok(json!(safe_outbox(backend.send_compose(request).await?)))
        }
        Command::SendDraft {
            id,
            confirm_recipient,
        } => {
            let draft = backend
                .list_drafts()?
                .into_iter()
                .find(|draft| draft.id == id)
                .ok_or_else(|| MailError::NotFound {
                    entity: "draft",
                    id: id.clone(),
                })?;
            Ok(json!(safe_outbox(
                backend
                    .send_draft(&id, draft.local_version, &confirm_recipient)
                    .await?
            )))
        }
        Command::Outbox => {
            let items = backend
                .list_outbox()?
                .into_iter()
                .map(safe_outbox)
                .collect::<Vec<_>>();
            Ok(json!(items))
        }
        Command::RetryOutbox { id } => Ok(json!(safe_outbox(backend.retry_outbox(&id).await?))),
    }
}

fn require_positive(value: usize, argument: &str) -> Result<()> {
    if value == 0 {
        return Err(MailError::Validation(format!(
            "--{argument} must be greater than zero"
        )));
    }
    Ok(())
}

fn require_exact_recipient_confirmation(
    request: &ComposeRequest,
    confirmations: &[String],
) -> Result<()> {
    request.validate()?;

    let actual = normalized_addresses(request.all_recipients().map(String::as_str))?;
    let confirmed = normalized_addresses(confirmations.iter().map(String::as_str))?;

    if actual != confirmed {
        return Err(MailError::Validation(
            "recipient confirmation does not exactly match the normalized To/Cc/Bcc set; no message was sent"
                .to_owned(),
        ));
    }

    Ok(())
}

fn normalized_addresses<'a>(
    addresses: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeSet<String>> {
    let mut normalized = BTreeSet::new();
    for address in addresses {
        let address = address.trim();
        if address.is_empty() {
            return Err(MailError::Validation(
                "recipient confirmations cannot be blank".to_owned(),
            ));
        }
        normalized.insert(address.to_lowercase());
    }
    Ok(normalized)
}

fn safe_outbox(mut item: OutboxItem) -> OutboxItem {
    item.raw_rfc822.clear();
    item
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

fn success_json(data: Value) -> String {
    serde_json::to_string_pretty(&json!({ "ok": true, "data": data }))
        .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"could not encode result\"}".to_owned())
}

fn error_json(error: &MailError) -> String {
    serde_json::to_string_pretty(&json!({ "ok": false, "error": error.to_string() }))
        .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"could not encode error\"}".to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, ComposeRequest, require_exact_recipient_confirmation};

    fn request() -> ComposeRequest {
        ComposeRequest {
            to: vec!["Alice@Example.com".to_owned()],
            cc: vec!["bob@example.com".to_owned()],
            bcc: Vec::new(),
            subject: "test".to_owned(),
            body_text: "body".to_owned(),
        }
    }

    #[test]
    fn accepts_exact_normalized_recipient_set_in_any_order() {
        let confirmations = vec![
            " BOB@example.com ".to_owned(),
            "alice@example.COM".to_owned(),
        ];
        assert!(require_exact_recipient_confirmation(&request(), &confirmations).is_ok());
    }

    #[test]
    fn rejects_missing_or_extra_confirmed_recipient() {
        assert!(
            require_exact_recipient_confirmation(&request(), &["alice@example.com".to_owned()])
                .is_err()
        );
        assert!(
            require_exact_recipient_confirmation(
                &request(),
                &[
                    "alice@example.com".to_owned(),
                    "bob@example.com".to_owned(),
                    "mallory@example.com".to_owned(),
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn cli_requires_an_explicit_credentials_path() {
        assert!(Cli::try_parse_from(["mine-mail", "check"]).is_err());
        let parsed = Cli::try_parse_from([
            "mine-mail",
            "--credentials",
            "C:/private/mine-mail-credentials.txt",
            "check",
        ])
        .expect("explicit credentials path");
        assert_eq!(
            parsed.credentials,
            PathBuf::from("C:/private/mine-mail-credentials.txt")
        );
    }
}
