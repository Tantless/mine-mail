use std::{collections::BTreeSet, time::Duration};

use async_imap::{
    Session,
    extensions::idle::IdleResponse,
    imap_proto::types::{
        BodyContentCommon, BodyContentSinglePart, BodyStructure, ContentEncoding, MessageSection,
        SectionPath,
    },
    types::{Capabilities, Capability, Flag, NameAttribute},
};
use async_native_tls::TlsStream;
use futures::TryStreamExt;
use tokio::{net::TcpStream, time::timeout};

use crate::{AccountConfig, AuthenticationKind, MailError, Result};

type ImapSession = Session<TlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(45);
const DRAFT_FETCH_BATCH_SIZE: usize = 10;
const SUMMARY_PREVIEW_BYTES: usize = 32 * 1024;
const MAX_HISTORY_PAGE_SIZE: usize = 100;
const HISTORY_UID_SEARCH_WINDOW: u32 = 1_000;
const GMAIL_ARCHIVE_SEARCH: &str =
    r#"X-GM-RAW "in:archive -in:sent -in:drafts -in:spam -in:trash""#;

#[derive(Clone, Debug)]
pub(crate) struct MailboxSnapshot {
    pub exists: u32,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    pub all_uids: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MailboxHint {
    pub exists: u32,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
}

/// Chooses the provider-side message set exposed through one selected mailbox.
///
/// Gmail stores archived messages in its `\All` mailbox, but that mailbox also
/// contains Inbox, Sent, and other system-label messages. The dedicated scope
/// keeps Gmail's provider query in Rust instead of leaking label semantics into
/// SQLite or React.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MailboxMessageScope {
    #[default]
    All,
    GmailArchive,
}

impl MailboxMessageScope {
    fn search_query(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::GmailArchive => GMAIL_ARCHIVE_SEARCH,
        }
    }

    fn bounded_search_query(self, window: UidSearchWindow, flagged_only: bool) -> String {
        let flag = if flagged_only { " FLAGGED" } else { "" };
        match self {
            Self::All => format!("{}{flag}", window.query()),
            Self::GmailArchive => {
                format!("{} {GMAIL_ARCHIVE_SEARCH}{flag}", window.query())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RemoteMessage {
    pub uid: u32,
    pub flags: Vec<String>,
    pub internal_date: Option<String>,
    pub size_bytes: u32,
    pub raw: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RemoteTransferEncoding {
    SevenBit,
    EightBit,
    Binary,
    Base64,
    QuotedPrintable,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteMimePart {
    pub path: Vec<u32>,
    pub mime_type: String,
    pub original_name: Option<String>,
    pub disposition: Option<String>,
    pub content_id: Option<String>,
    pub transfer_encoding: RemoteTransferEncoding,
    pub encoded_size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteMessageStructure {
    pub uid: u32,
    pub parts: Vec<RemoteMimePart>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteBodyPart {
    pub path: Vec<u32>,
    pub mime_header: Vec<u8>,
    pub encoded_body: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct RemoteMailbox {
    pub name: String,
    pub is_all: bool,
    pub is_drafts: bool,
    pub is_sent: bool,
    pub is_archive: bool,
    pub is_trash: bool,
    pub is_selectable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CreatableMailboxRole {
    Archive,
    Trash,
}

impl CreatableMailboxRole {
    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::Archive => "Archive",
            Self::Trash => "Trash",
        }
    }
}

/// `async-imap` 0.11.2 consumes COPYUID response codes and exposes only
/// `Result<()>` for UID COPY and UID MOVE. Callers must therefore reconcile the
/// destination mailbox instead of inventing a source-to-destination UID map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UidTransferOutcome {
    CompletedWithoutUidMapping,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageMoveMethod {
    UidMove,
    UidCopyThenDelete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeleteFinalization {
    UidExpunge,
    DeferredServerCleanup,
}

/// One bounded numeric UID search window below an opaque history cursor.
///
/// UIDs are sorted ascending for efficient summary fetching. `next_before_uid`
/// is an exclusive upper bound for a later request and may advance even when a
/// sparse numeric window contains no messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OlderUidSearchPage {
    pub uids: Vec<u32>,
    pub next_before_uid: Option<u32>,
    pub reached_uid_floor: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UidSearchWindow {
    lower: u32,
    upper: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FixedFlagMutation {
    Seen(bool),
    Flagged(bool),
    Deleted,
}

impl FixedFlagMutation {
    fn query(self) -> &'static str {
        match self {
            Self::Seen(true) => "+FLAGS.SILENT (\\Seen)",
            Self::Seen(false) => "-FLAGS.SILENT (\\Seen)",
            Self::Flagged(true) => "+FLAGS.SILENT (\\Flagged)",
            Self::Flagged(false) => "-FLAGS.SILENT (\\Flagged)",
            Self::Deleted => "+FLAGS.SILENT (\\Deleted)",
        }
    }

    fn operation(self) -> &'static str {
        match self {
            Self::Seen(_) => "IMAP update message read state",
            Self::Flagged(_) => "IMAP update message star",
            Self::Deleted => "IMAP mark message deleted",
        }
    }

    fn response_operation(self) -> &'static str {
        match self {
            Self::Seen(_) => "IMAP update message read state response",
            Self::Flagged(_) => "IMAP update message star response",
            Self::Deleted => "IMAP mark message deleted response",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RemoteDraftSnapshot {
    pub mailbox: String,
    pub uid_validity: Option<u32>,
    pub messages: Vec<RemoteMessage>,
}

pub(crate) struct ImapConnection {
    session: ImapSession,
    supports_move: bool,
    supports_uidplus: bool,
    supports_special_use: bool,
    supports_idle: bool,
    supports_condstore: bool,
}

impl ImapConnection {
    pub async fn connect(config: &AccountConfig) -> Result<Self> {
        let stream = timeout(
            CONNECT_TIMEOUT,
            TcpStream::connect((config.imap.host.as_str(), config.imap.port)),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: "IMAP connection",
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;

        let connector = async_native_tls::TlsConnector::new();
        let tls_stream = timeout(
            CONNECT_TIMEOUT,
            connector.connect(config.imap.host.as_str(), stream),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: "IMAP TLS handshake",
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;

        let mut client = async_imap::Client::new(tls_stream);
        timeout(CONNECT_TIMEOUT, client.read_response())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP greeting",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?
            .ok_or_else(|| MailError::Imap("server closed before IMAP greeting".to_owned()))?;

        let mut session = match config.authentication_kind() {
            AuthenticationKind::Password => timeout(
                CONNECT_TIMEOUT,
                client.login(&config.email, config.authorization_secret()),
            )
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP authentication",
            })?
            .map_err(|(error, _client)| MailError::Imap(error.to_string()))?,
            AuthenticationKind::OAuth2 => {
                let authenticator = OAuth2Authenticator {
                    email: &config.email,
                    access_token: config.authorization_secret(),
                };
                timeout(
                    CONNECT_TIMEOUT,
                    client.authenticate("XOAUTH2", authenticator),
                )
                .await
                .map_err(|_| MailError::Timeout {
                    operation: "IMAP OAuth authentication",
                })?
                .map_err(|(error, _client)| MailError::Imap(error.to_string()))?
            }
        };

        let capabilities = timeout(COMMAND_TIMEOUT, session.capabilities())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP CAPABILITY",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;

        // NetEase documents/uses RFC 2971 client identification. Sending it
        // after LOGIN and before SELECT avoids the provider's “Unsafe Login”
        // path while containing no user data.
        let supports_id = has_capability(&capabilities, "ID");
        if supports_id {
            timeout(
                COMMAND_TIMEOUT,
                session.id([
                    ("name", Some("mine-mail")),
                    ("version", Some(env!("CARGO_PKG_VERSION"))),
                    ("vendor", Some("mine-mail")),
                ]),
            )
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP ID",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        }

        // Some providers adjust their advertised extensions after RFC 2971
        // identification, so capability-driven behavior must use a fresh
        // post-ID snapshot rather than a provider-name allowlist.
        let capabilities = if supports_id {
            timeout(COMMAND_TIMEOUT, session.capabilities())
                .await
                .map_err(|_| MailError::Timeout {
                    operation: "IMAP CAPABILITY",
                })?
                .map_err(|error| MailError::Imap(error.to_string()))?
        } else {
            capabilities
        };
        let supports_move = has_capability(&capabilities, "MOVE");
        let supports_uidplus = has_capability(&capabilities, "UIDPLUS");
        let supports_special_use = has_capability(&capabilities, "SPECIAL-USE");
        let supports_idle = has_capability(&capabilities, "IDLE");
        let supports_condstore = has_capability(&capabilities, "CONDSTORE");
        Ok(Self {
            session,
            supports_move,
            supports_uidplus,
            supports_special_use,
            supports_idle,
            supports_condstore,
        })
    }

    pub fn supports_idle(&self) -> bool {
        self.supports_idle
    }

    pub fn supports_condstore(&self) -> bool {
        self.supports_condstore
    }

    pub fn message_move_method(&self) -> MessageMoveMethod {
        choose_message_move_method(self.supports_move)
    }

    pub fn delete_finalization(&self) -> DeleteFinalization {
        choose_delete_finalization(self.supports_uidplus)
    }

    pub async fn probe(mut self) -> Result<()> {
        self.noop().await?;
        self.logout().await
    }

    pub async fn noop(&mut self) -> Result<()> {
        timeout(COMMAND_TIMEOUT, self.session.noop())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP NOOP",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))
    }

    pub async fn list_mailboxes(&mut self) -> Result<Vec<RemoteMailbox>> {
        let stream = timeout(COMMAND_TIMEOUT, self.session.list(None, Some("*")))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP LIST",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let names = timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP LIST response",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;

        Ok(names
            .into_iter()
            .map(|name| classify_remote_mailbox(name.name(), name.attributes()))
            .collect())
    }

    /// Creates only the two fixed product-managed fallback mailboxes. The
    /// caller must issue LIST afterward and confirm the requested SPECIAL-USE
    /// role is advertised and selectable before treating creation as success.
    pub async fn create_mailbox_role(&mut self, role: CreatableMailboxRole) -> Result<()> {
        timeout(COMMAND_TIMEOUT, self.session.create(role.canonical_name()))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP CREATE product mailbox",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))
    }

    pub async fn select_mailbox_with_scope(
        &mut self,
        mailbox: &str,
        scope: MailboxMessageScope,
    ) -> Result<MailboxSnapshot> {
        let selected = if self.supports_condstore {
            timeout(COMMAND_TIMEOUT, self.session.select_condstore(mailbox)).await
        } else {
            timeout(COMMAND_TIMEOUT, self.session.select(mailbox)).await
        }
        .map_err(|_| MailError::Timeout {
            operation: "IMAP SELECT mailbox",
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;
        let all_uids = self.search_uids(scope.search_query()).await?;
        let exists = match scope {
            MailboxMessageScope::All => selected.exists,
            MailboxMessageScope::GmailArchive => u32::try_from(all_uids.len()).unwrap_or(u32::MAX),
        };

        Ok(MailboxSnapshot {
            exists,
            uid_validity: selected.uid_validity,
            uid_next: selected.uid_next,
            highest_modseq: selected.highest_modseq,
            all_uids,
        })
    }

    /// Select INBOX without enumerating UIDs. This is intentionally cheap and
    /// is used by the long-lived change monitor and the incremental sync path.
    pub async fn select_inbox_hint(&mut self) -> Result<MailboxHint> {
        let selected = timeout(COMMAND_TIMEOUT, self.session.select("INBOX"))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP SELECT INBOX",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        Ok(MailboxHint {
            exists: selected.exists,
            uid_validity: selected.uid_validity,
            uid_next: selected.uid_next,
        })
    }

    /// Selects a mailbox for bounded keyset history loading without performing
    /// the unbounded ALL search used by full reconciliation.
    pub async fn select_mailbox_for_history(&mut self, mailbox: &str) -> Result<MailboxHint> {
        let mailbox = validated_mailbox_name(mailbox)?;
        let selected = timeout(COMMAND_TIMEOUT, self.session.select(mailbox))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP SELECT history mailbox",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        Ok(MailboxHint {
            exists: selected.exists,
            uid_validity: selected.uid_validity,
            uid_next: selected.uid_next,
        })
    }

    pub async fn search_uids_after(&mut self, highest_uid: u32) -> Result<Vec<u32>> {
        let first = highest_uid.saturating_add(1);
        if first == 0 {
            return Ok(Vec::new());
        }
        let query = format!("UID {first}:*");
        let uids = timeout(COMMAND_TIMEOUT, self.session.uid_search(query))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP incremental UID SEARCH",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let mut uids: Vec<u32> = uids
            .into_iter()
            // An empty range expressed as n:* is interpreted differently by
            // some IMAP implementations. Filtering makes the cursor strict.
            .filter(|uid| *uid > highest_uid)
            .collect();
        uids.sort_unstable();
        Ok(uids)
    }

    /// Searches one bounded numeric UID window below an exclusive cursor.
    ///
    /// Sparse mailboxes may return an empty page with another cursor. This is
    /// intentional: the bounded query prevents a single history request from
    /// turning into an unbounded server scan.
    pub async fn search_uids_before_with_scope(
        &mut self,
        before_uid: u32,
        page_size: usize,
        scope: MailboxMessageScope,
    ) -> Result<OlderUidSearchPage> {
        self.search_uids_before_with_filter(before_uid, page_size, scope, false)
            .await
    }

    pub async fn search_flagged_uids_before_with_scope(
        &mut self,
        before_uid: u32,
        page_size: usize,
        scope: MailboxMessageScope,
    ) -> Result<OlderUidSearchPage> {
        self.search_uids_before_with_filter(before_uid, page_size, scope, true)
            .await
    }

    async fn search_uids_before_with_filter(
        &mut self,
        before_uid: u32,
        page_size: usize,
        scope: MailboxMessageScope,
        flagged_only: bool,
    ) -> Result<OlderUidSearchPage> {
        validate_history_page_size(page_size)?;
        let Some(window) = older_uid_search_window(before_uid) else {
            return Ok(OlderUidSearchPage {
                uids: Vec::new(),
                next_before_uid: None,
                reached_uid_floor: true,
            });
        };
        let uids = timeout(
            COMMAND_TIMEOUT,
            self.session
                .uid_search(scope.bounded_search_query(window, flagged_only)),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: "IMAP older history UID SEARCH",
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;
        Ok(finish_older_uid_search(uids, window, page_size))
    }

    /// Moves the selected mailbox's requested UIDs when the server advertises
    /// RFC 6851 MOVE. No COPYUID mapping is returned by async-imap, so callers
    /// must reconcile the destination after an acknowledged command.
    pub async fn move_uids(
        &mut self,
        uids: &[u32],
        destination: &str,
    ) -> Result<UidTransferOutcome> {
        if !self.supports_move {
            return Err(MailError::Validation(
                "the IMAP server does not advertise MOVE".to_owned(),
            ));
        }
        let sequence_set = required_uid_set(uids)?;
        let destination = validated_mailbox_name(destination)?;
        timeout(
            COMMAND_TIMEOUT,
            self.session.uid_mv(&sequence_set, destination),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: "IMAP UID MOVE",
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;
        Ok(UidTransferOutcome::CompletedWithoutUidMapping)
    }

    /// Copies the selected mailbox's requested UIDs. This deliberately does
    /// not claim a source-to-destination UID mapping because async-imap does
    /// not expose COPYUID response data.
    pub async fn copy_uids(
        &mut self,
        uids: &[u32],
        destination: &str,
    ) -> Result<UidTransferOutcome> {
        let sequence_set = required_uid_set(uids)?;
        let destination = validated_mailbox_name(destination)?;
        timeout(
            COMMAND_TIMEOUT,
            self.session.uid_copy(&sequence_set, destination),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: "IMAP UID COPY",
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;
        Ok(UidTransferOutcome::CompletedWithoutUidMapping)
    }

    /// Enter one bounded IDLE cycle and restore the session with DONE before
    /// returning. The caller reconnects on any error or maintenance timeout.
    pub async fn wait_for_idle_change(self, duration: Duration) -> Result<(Self, bool)> {
        let supports_move = self.supports_move;
        let supports_uidplus = self.supports_uidplus;
        let supports_special_use = self.supports_special_use;
        let supports_idle = self.supports_idle;
        let supports_condstore = self.supports_condstore;
        if !supports_idle {
            return Err(MailError::Validation(
                "the IMAP server does not advertise IDLE".to_owned(),
            ));
        }

        let mut handle = self.session.idle();
        timeout(COMMAND_TIMEOUT, handle.init())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP IDLE initialization",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let response = {
            let (wait, _interrupt) = handle.wait_with_timeout(duration);
            wait.await
                .map_err(|error| MailError::Imap(error.to_string()))?
        };
        let session = timeout(COMMAND_TIMEOUT, handle.done())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP IDLE completion",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        Ok((
            Self {
                session,
                supports_move,
                supports_uidplus,
                supports_special_use,
                supports_idle,
                supports_condstore,
            },
            matches!(response, IdleResponse::NewData(_)),
        ))
    }

    /// Selects one mailbox for a known-UID body fetch without the full UID
    /// SEARCH required by metadata reconciliation.
    pub async fn select_mailbox_for_fetch(&mut self, mailbox: &str) -> Result<Option<u32>> {
        timeout(COMMAND_TIMEOUT, self.session.select(mailbox))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP SELECT mailbox",
            })?
            .map(|selected| selected.uid_validity)
            .map_err(|error| MailError::Imap(error.to_string()))
    }

    /// Selects a mailbox read-write and verifies the server's advertised
    /// permanent flags when that metadata is present. `\Seen` is a standard
    /// IMAP system flag rather than an optional CAPABILITY token, so the final
    /// authority is a successful STORE followed by a FLAGS fetch.
    pub async fn select_mailbox_for_seen_update(&mut self, mailbox: &str) -> Result<Option<u32>> {
        let selected = timeout(COMMAND_TIMEOUT, self.session.select(mailbox))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP SELECT mailbox",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        if !mailbox_allows_seen_updates(&selected.permanent_flags) {
            return Err(MailError::Validation(
                "the IMAP mailbox does not allow persistent \\Seen updates".to_owned(),
            ));
        }
        Ok(selected.uid_validity)
    }

    /// Selects a mailbox read-write and rejects a server that explicitly
    /// omits the standard `\Flagged` flag from PERMANENTFLAGS.
    pub async fn select_mailbox_for_flagged_update(
        &mut self,
        mailbox: &str,
    ) -> Result<Option<u32>> {
        let selected = timeout(COMMAND_TIMEOUT, self.session.select(mailbox))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP SELECT mailbox",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        if !mailbox_allows_flagged_updates(&selected.permanent_flags) {
            return Err(MailError::Validation(
                "the IMAP mailbox does not allow persistent \\Flagged updates".to_owned(),
            ));
        }
        Ok(selected.uid_validity)
    }

    /// Selects a mailbox read-write and rejects a server that explicitly
    /// omits `\Deleted` from PERMANENTFLAGS.
    pub async fn select_mailbox_for_deleted_update(
        &mut self,
        mailbox: &str,
    ) -> Result<Option<u32>> {
        let selected = timeout(COMMAND_TIMEOUT, self.session.select(mailbox))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP SELECT mailbox",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        if !mailbox_allows_deleted_updates(&selected.permanent_flags) {
            return Err(MailError::Validation(
                "the IMAP mailbox does not allow persistent \\Deleted updates".to_owned(),
            ));
        }
        Ok(selected.uid_validity)
    }

    async fn search_uids(&mut self, query: &str) -> Result<Vec<u32>> {
        let uids = timeout(COMMAND_TIMEOUT, self.session.uid_search(query))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP UID SEARCH",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let mut uids: Vec<u32> = uids.into_iter().collect();
        uids.sort_unstable();
        Ok(uids)
    }

    pub async fn fetch_summaries(&mut self, uids: &[u32]) -> Result<Vec<RemoteMessage>> {
        let query = summary_fetch_query();
        self.fetch_messages(uids, &query, true).await
    }

    pub async fn fetch_full_message(&mut self, uid: u32) -> Result<RemoteMessage> {
        self.fetch_messages(
            &[uid],
            "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[])",
            true,
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| MailError::NotFound {
            entity: "remote message UID",
            id: uid.to_string(),
        })
    }

    pub async fn fetch_message_structure(&mut self, uid: u32) -> Result<RemoteMessageStructure> {
        if uid == 0 {
            return Err(MailError::Validation(
                "message UID must be greater than zero".to_owned(),
            ));
        }
        let stream = timeout(
            COMMAND_TIMEOUT,
            self.session
                .uid_fetch(uid.to_string(), message_structure_fetch_query()),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: "IMAP message structure fetch",
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;
        let fetched = timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP message structure response",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let fetched = fetched
            .into_iter()
            .find(|message| message.uid == Some(uid))
            .ok_or_else(|| MailError::NotFound {
                entity: "remote message UID",
                id: uid.to_string(),
            })?;
        let structure = fetched
            .bodystructure()
            .ok_or_else(|| MailError::Imap("server omitted BODYSTRUCTURE".to_owned()))?;
        let mut parts = Vec::new();
        collect_remote_mime_parts(structure, &[], &mut parts)?;
        Ok(RemoteMessageStructure { uid, parts })
    }

    pub async fn fetch_message_parts(
        &mut self,
        uid: u32,
        paths: &[Vec<u32>],
    ) -> Result<Vec<RemoteBodyPart>> {
        self.fetch_message_parts_with_limit(uid, paths, None).await
    }

    pub async fn fetch_message_parts_bounded(
        &mut self,
        uid: u32,
        paths: &[Vec<u32>],
        max_body_bytes: u64,
    ) -> Result<Vec<RemoteBodyPart>> {
        if max_body_bytes == 0 {
            return Err(MailError::Validation(
                "message MIME part byte limit must be greater than zero".to_owned(),
            ));
        }
        self.fetch_message_parts_with_limit(uid, paths, Some(max_body_bytes))
            .await
    }

    async fn fetch_message_parts_with_limit(
        &mut self,
        uid: u32,
        paths: &[Vec<u32>],
        max_body_bytes: Option<u64>,
    ) -> Result<Vec<RemoteBodyPart>> {
        if uid == 0 {
            return Err(MailError::Validation(
                "message UID must be greater than zero".to_owned(),
            ));
        }
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let query = message_parts_fetch_query_with_limit(paths, max_body_bytes)?;
        let stream = timeout(
            COMMAND_TIMEOUT,
            self.session.uid_fetch(uid.to_string(), query),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: "IMAP message part fetch",
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;
        let fetched = timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP message part response",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let fetched = fetched
            .into_iter()
            .find(|message| message.uid == Some(uid))
            .ok_or_else(|| MailError::NotFound {
                entity: "remote message UID",
                id: uid.to_string(),
            })?;

        paths
            .iter()
            .map(|path| {
                let mime_path = SectionPath::Part(path.clone(), Some(MessageSection::Mime));
                let body_path = SectionPath::Part(path.clone(), None);
                let mime_header = fetched
                    .section(&mime_path)
                    .ok_or_else(|| {
                        MailError::Imap("server omitted requested MIME part header".to_owned())
                    })?
                    .to_vec();
                let encoded_body = fetched
                    .section(&body_path)
                    .ok_or_else(|| {
                        MailError::Imap("server omitted requested MIME part body".to_owned())
                    })?
                    .to_vec();
                if max_body_bytes.is_some_and(|limit| encoded_body.len() as u64 > limit) {
                    return Err(MailError::Validation(
                        "selected message body part exceeds the reader byte limit".to_owned(),
                    ));
                }
                Ok(RemoteBodyPart {
                    path: path.clone(),
                    mime_header,
                    encoded_body,
                })
            })
            .collect()
    }

    pub async fn fetch_flags(&mut self, uids: &[u32]) -> Result<Vec<(u32, Vec<String>)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let sequence_set = required_uid_set(uids)?;
        let stream = timeout(
            COMMAND_TIMEOUT,
            self.session.uid_fetch(sequence_set, "(UID FLAGS)"),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: "IMAP flag fetch",
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;
        let fetched = timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP flag response",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;

        Ok(fetched
            .into_iter()
            .filter_map(|message| {
                let uid = message.uid?;
                let flags = message.flags().map(flag_name).collect();
                Some((uid, flags))
            })
            .collect())
    }

    /// Fetch only flags whose per-message modification sequence advanced
    /// after the last committed mailbox snapshot (RFC 7162 CONDSTORE).
    pub async fn fetch_flags_changed_since(
        &mut self,
        uids: &[u32],
        highest_modseq: u64,
    ) -> Result<Vec<(u32, Vec<String>)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        if !self.supports_condstore || highest_modseq == 0 {
            return self.fetch_flags(uids).await;
        }
        let sequence_set = required_uid_set(uids)?;
        let query = format!("(UID FLAGS) (CHANGEDSINCE {highest_modseq})");
        let stream = timeout(COMMAND_TIMEOUT, self.session.uid_fetch(sequence_set, query))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP changed flag fetch",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let fetched = timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP changed flag response",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;

        Ok(fetched
            .into_iter()
            .filter_map(|message| {
                let uid = message.uid?;
                let flags = message.flags().map(flag_name).collect();
                Some((uid, flags))
            })
            .collect())
    }

    async fn store_fixed_flag_mutation(
        &mut self,
        uids: &[u32],
        mutation: FixedFlagMutation,
    ) -> Result<()> {
        let sequence_set = required_uid_set(uids)?;
        let stream = timeout(
            COMMAND_TIMEOUT,
            self.session.uid_store(&sequence_set, mutation.query()),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: mutation.operation(),
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;
        timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
            .await
            .map_err(|_| MailError::Timeout {
                operation: mutation.response_operation(),
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        Ok(())
    }

    /// Adds or removes `\Seen` without replacing unrelated flags, then returns
    /// the server-confirmed final flag set for every requested UID.
    pub async fn set_seen(
        &mut self,
        uids: &[u32],
        desired: bool,
    ) -> Result<Vec<(u32, Vec<String>)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        self.store_fixed_flag_mutation(uids, FixedFlagMutation::Seen(desired))
            .await?;
        let confirmed = self.fetch_flags(uids).await?;
        ensure_flag_state(&confirmed, uids, "\\Seen", desired, "read")?;
        Ok(confirmed)
    }

    pub async fn set_seen_flags(
        &mut self,
        uids: &[u32],
        desired: bool,
    ) -> Result<Vec<(u32, Vec<String>)>> {
        self.set_seen(uids, desired).await
    }

    /// Adds or removes the standard `\Flagged` system flag without replacing
    /// unrelated message state, then verifies the server's persisted result.
    pub async fn set_flagged_flags(
        &mut self,
        uids: &[u32],
        desired: bool,
    ) -> Result<Vec<(u32, Vec<String>)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        self.store_fixed_flag_mutation(uids, FixedFlagMutation::Flagged(desired))
            .await?;
        let confirmed = self.fetch_flags(uids).await?;
        ensure_flag_state(&confirmed, uids, "\\Flagged", desired, "star")?;
        Ok(confirmed)
    }

    /// Marks only the selected mailbox's requested UIDs as `\Deleted` and
    /// verifies the flags. This never performs EXPUNGE.
    pub async fn mark_deleted_flags(&mut self, uids: &[u32]) -> Result<Vec<(u32, Vec<String>)>> {
        self.store_fixed_flag_mutation(uids, FixedFlagMutation::Deleted)
            .await?;
        let confirmed = self.fetch_flags(uids).await?;
        ensure_flag_state(&confirmed, uids, "\\Deleted", true, "deleted")?;
        Ok(confirmed)
    }

    /// Permanently removes only the requested deleted UIDs. Global EXPUNGE is
    /// intentionally unavailable; servers without UIDPLUS must defer cleanup.
    pub async fn expunge_deleted_uids(&mut self, uids: &[u32]) -> Result<usize> {
        if !self.supports_uidplus {
            return Err(MailError::Validation(
                "the IMAP server does not advertise UIDPLUS".to_owned(),
            ));
        }
        let sequence_set = required_uid_set(uids)?;
        let stream = timeout(COMMAND_TIMEOUT, self.session.uid_expunge(&sequence_set))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP UID EXPUNGE",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let expunged = timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP UID EXPUNGE response",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        Ok(expunged.len())
    }

    async fn fetch_messages(
        &mut self,
        uids: &[u32],
        query: &str,
        full: bool,
    ) -> Result<Vec<RemoteMessage>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let sequence_set = required_uid_set(uids)?;
        let stream = timeout(COMMAND_TIMEOUT, self.session.uid_fetch(sequence_set, query))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP message fetch",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let fetched = timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP message response",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;

        fetched
            .into_iter()
            .map(|message| {
                let uid = message.uid.ok_or_else(|| {
                    MailError::Imap("server returned a message without UID".to_owned())
                })?;
                let raw = if full {
                    message.body()
                } else {
                    message.header()
                }
                .ok_or_else(|| {
                    MailError::Imap(format!("server returned UID {uid} without requested data"))
                })?
                .to_vec();

                Ok(RemoteMessage {
                    uid,
                    flags: message.flags().map(flag_name).collect(),
                    internal_date: message.internal_date().map(|date| date.to_rfc3339()),
                    size_bytes: message.size.unwrap_or(raw.len() as u32),
                    raw,
                })
            })
            .collect()
    }

    /// Fetch all drafts from the selected Drafts mailbox. Draft synchronization
    /// needs full RFC822 data because another client may have created the draft
    /// without Mine Mail's private identity headers.
    pub async fn fetch_draft_snapshot(
        &mut self,
        mailbox_override: Option<&str>,
    ) -> Result<RemoteDraftSnapshot> {
        let mailbox = match mailbox_override {
            Some(mailbox) => mailbox.to_owned(),
            None => self.discover_drafts_mailbox().await?,
        };
        let selected = timeout(COMMAND_TIMEOUT, self.session.select(&mailbox))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP SELECT drafts",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let mut uids: Vec<u32> = timeout(COMMAND_TIMEOUT, self.session.uid_search("UNDELETED"))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP draft UID SEARCH",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?
            .into_iter()
            .collect();
        uids.sort_unstable();

        let mut messages = Vec::with_capacity(uids.len());
        for batch in uids.chunks(DRAFT_FETCH_BATCH_SIZE) {
            messages.extend(
                self.fetch_messages(
                    batch,
                    "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[])",
                    true,
                )
                .await?,
            );
        }
        Ok(RemoteDraftSnapshot {
            mailbox,
            uid_validity: selected.uid_validity,
            messages,
        })
    }

    /// Append the new canonical revision before retiring old copies. If the
    /// server does not expose APPENDUID and its header index is delayed, the old
    /// copy is intentionally retained; a later reconciliation will recognize
    /// the higher revision and remove duplicates safely.
    pub async fn append_and_replace_draft(
        &mut self,
        mailbox: &str,
        draft_id: &str,
        raw_rfc822: &[u8],
        old_uids: &[u32],
    ) -> Result<(Option<u32>, usize)> {
        timeout(
            COMMAND_TIMEOUT,
            self.session
                .append(mailbox, Some("(\\Draft)"), None, raw_rfc822),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: "IMAP APPEND draft revision",
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;

        let new_uid = self.find_draft_uids(draft_id).await?.into_iter().max();
        let Some(new_uid) = new_uid else {
            return Ok((None, 0));
        };
        let obsolete: Vec<u32> = old_uids
            .iter()
            .copied()
            .filter(|uid| *uid != new_uid)
            .collect();
        let removed = self.delete_draft_uids(&obsolete).await?;
        Ok((Some(new_uid), removed))
    }

    /// Mark only the requested UIDs deleted. UIDPLUS servers are expunged with
    /// UID EXPUNGE; other servers retain the hidden `\\Deleted` records until
    /// their normal expunge cycle rather than risking deletion of unrelated
    /// messages with a global EXPUNGE.
    pub async fn delete_draft_uids(&mut self, uids: &[u32]) -> Result<usize> {
        if uids.is_empty() {
            return Ok(0);
        }
        self.mark_deleted_flags(uids).await?;
        if self.supports_uidplus {
            self.expunge_deleted_uids(uids).await?;
        }
        Ok(uids.iter().copied().collect::<BTreeSet<_>>().len())
    }

    async fn discover_drafts_mailbox(&mut self) -> Result<String> {
        let mailboxes = self.list_mailboxes().await?;
        if let Some(mailbox) = mailboxes.iter().find(|mailbox| mailbox.is_drafts) {
            return Ok(mailbox.name.clone());
        }
        if let Some(mailbox) = mailboxes
            .iter()
            .find(|mailbox| mailbox.name.eq_ignore_ascii_case("Drafts"))
        {
            return Ok(mailbox.name.clone());
        }
        Err(MailError::Config(
            "server did not advertise a Drafts mailbox; provide an explicit mailbox name"
                .to_owned(),
        ))
    }

    pub(crate) async fn discover_sent_mailbox(&mut self) -> Result<String> {
        let mailboxes = self.list_mailboxes().await?;
        if let Some(mailbox) = mailboxes
            .iter()
            .find(|mailbox| mailbox.is_sent && mailbox.is_selectable)
        {
            return Ok(mailbox.name.clone());
        }

        const FALLBACK_NAMES: &[&str] = &[
            "Sent",
            "Sent Messages",
            "Sent Items",
            "已发送",
            "已发送邮件",
        ];
        for fallback in FALLBACK_NAMES {
            if let Some(mailbox) = mailboxes.iter().find(|mailbox| {
                mailbox.is_selectable
                    && (mailbox.name.eq_ignore_ascii_case(fallback)
                        || mailbox
                            .name
                            .rsplit(['/', '.'])
                            .next()
                            .is_some_and(|leaf| leaf.eq_ignore_ascii_case(fallback)))
            }) {
                return Ok(mailbox.name.clone());
            }
        }

        Err(MailError::Config(
            "server did not advertise a Sent mailbox and no common Sent folder name was found"
                .to_owned(),
        ))
    }

    async fn find_draft_uids(&mut self, draft_id: &str) -> Result<Vec<u32>> {
        if !draft_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(MailError::Validation("invalid draft id".to_owned()));
        }
        let query = format!("UNDELETED HEADER X-Mine-Mail-Draft-Id \"{draft_id}\"");
        let uids = timeout(COMMAND_TIMEOUT, self.session.uid_search(query))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP draft search",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let mut uids: Vec<u32> = uids.into_iter().collect();
        uids.sort_unstable();
        Ok(uids)
    }

    pub async fn logout(&mut self) -> Result<()> {
        timeout(COMMAND_TIMEOUT, self.session.logout())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP logout",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))
    }
}

fn summary_fetch_query() -> String {
    format!("(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[]<0.{SUMMARY_PREVIEW_BYTES}>)")
}

fn message_structure_fetch_query() -> &'static str {
    "(UID BODYSTRUCTURE)"
}

fn message_parts_fetch_query_with_limit(
    paths: &[Vec<u32>],
    max_body_bytes: Option<u64>,
) -> Result<String> {
    if paths.is_empty() {
        return Err(MailError::Validation(
            "at least one message MIME part is required".to_owned(),
        ));
    }
    let requested_body_bytes = max_body_bytes
        .map(|limit| {
            limit.checked_add(1).ok_or_else(|| {
                MailError::Validation("message MIME part byte limit is invalid".to_owned())
            })
        })
        .transpose()?;
    let mut query = String::from("(UID");
    for path in paths {
        let section = validated_part_section(path)?;
        query.push_str(&format!(" BODY.PEEK[{section}.MIME]"));
        if let Some(requested_body_bytes) = requested_body_bytes {
            query.push_str(&format!(" BODY.PEEK[{section}]<0.{requested_body_bytes}>"));
        } else {
            query.push_str(&format!(" BODY.PEEK[{section}]"));
        }
    }
    query.push(')');
    Ok(query)
}

fn collect_remote_mime_parts(
    structure: &BodyStructure<'_>,
    parent_path: &[u32],
    parts: &mut Vec<RemoteMimePart>,
) -> Result<()> {
    const MAX_REMOTE_MIME_PARTS: usize = 4_096;
    const MAX_REMOTE_MIME_DEPTH: usize = 64;

    if parent_path.len() > MAX_REMOTE_MIME_DEPTH {
        return Err(MailError::Mime(
            "remote MIME structure is too deeply nested".to_owned(),
        ));
    }
    match structure {
        BodyStructure::Multipart { bodies, .. } => {
            for (index, body) in bodies.iter().enumerate() {
                if parts.len() >= MAX_REMOTE_MIME_PARTS {
                    return Err(MailError::Mime(
                        "remote MIME structure contains too many parts".to_owned(),
                    ));
                }
                let mut path = parent_path.to_vec();
                path.push(u32::try_from(index + 1).map_err(|_| {
                    MailError::Mime("remote MIME part index is invalid".to_owned())
                })?);
                collect_remote_mime_parts(body, &path, parts)?;
            }
        }
        BodyStructure::Basic { common, other, .. }
        | BodyStructure::Text { common, other, .. }
        | BodyStructure::Message { common, other, .. } => {
            if parts.len() >= MAX_REMOTE_MIME_PARTS {
                return Err(MailError::Mime(
                    "remote MIME structure contains too many parts".to_owned(),
                ));
            }
            let path = if parent_path.is_empty() {
                vec![1]
            } else {
                parent_path.to_vec()
            };
            parts.push(remote_mime_part(path, common, other));
        }
    }
    Ok(())
}

fn remote_mime_part(
    path: Vec<u32>,
    common: &BodyContentCommon<'_>,
    other: &BodyContentSinglePart<'_>,
) -> RemoteMimePart {
    let original_name = body_parameter(
        common
            .disposition
            .as_ref()
            .and_then(|disposition| disposition.params.as_ref()),
        "filename",
    )
    .or_else(|| body_parameter(common.ty.params.as_ref(), "name"));
    let transfer_encoding = match &other.transfer_encoding {
        ContentEncoding::SevenBit => RemoteTransferEncoding::SevenBit,
        ContentEncoding::EightBit => RemoteTransferEncoding::EightBit,
        ContentEncoding::Binary => RemoteTransferEncoding::Binary,
        ContentEncoding::Base64 => RemoteTransferEncoding::Base64,
        ContentEncoding::QuotedPrintable => RemoteTransferEncoding::QuotedPrintable,
        ContentEncoding::Other(value) => RemoteTransferEncoding::Other(value.to_string()),
    };
    RemoteMimePart {
        path,
        mime_type: format!(
            "{}/{}",
            common.ty.ty.to_ascii_lowercase(),
            common.ty.subtype.to_ascii_lowercase()
        ),
        original_name,
        disposition: common
            .disposition
            .as_ref()
            .map(|disposition| disposition.ty.to_ascii_lowercase()),
        content_id: other.id.as_ref().map(ToString::to_string),
        transfer_encoding,
        encoded_size_bytes: u64::from(other.octets),
    }
}

fn body_parameter(
    parameters: Option<&Vec<(std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)>>,
    expected: &str,
) -> Option<String> {
    parameters?
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(expected))
        .map(|(_, value)| value.to_string())
        .filter(|value| !value.trim().is_empty())
}

fn validated_part_section(path: &[u32]) -> Result<String> {
    if path.is_empty() || path.len() > 64 || path.iter().any(|segment| *segment == 0) {
        return Err(MailError::Validation(
            "message MIME part path is invalid".to_owned(),
        ));
    }
    Ok(path
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join("."))
}

fn has_capability(capabilities: &Capabilities, expected: &str) -> bool {
    capabilities
        .iter()
        .any(|capability| capability_atom_matches(capability, expected))
}

fn capability_atom_matches(capability: &Capability, expected: &str) -> bool {
    matches!(
        capability,
        Capability::Atom(value) if value.eq_ignore_ascii_case(expected)
    )
}

fn classify_remote_mailbox(name: &str, attributes: &[NameAttribute<'_>]) -> RemoteMailbox {
    RemoteMailbox {
        name: name.to_owned(),
        is_all: attributes
            .iter()
            .any(|attribute| matches!(attribute, NameAttribute::All)),
        is_drafts: attributes
            .iter()
            .any(|attribute| matches!(attribute, NameAttribute::Drafts)),
        is_sent: attributes
            .iter()
            .any(|attribute| matches!(attribute, NameAttribute::Sent)),
        is_archive: attributes
            .iter()
            .any(|attribute| matches!(attribute, NameAttribute::Archive)),
        is_trash: attributes
            .iter()
            .any(|attribute| matches!(attribute, NameAttribute::Trash)),
        is_selectable: !attributes
            .iter()
            .any(|attribute| matches!(attribute, NameAttribute::NoSelect)),
    }
}

fn choose_message_move_method(supports_move: bool) -> MessageMoveMethod {
    if supports_move {
        MessageMoveMethod::UidMove
    } else {
        MessageMoveMethod::UidCopyThenDelete
    }
}

fn choose_delete_finalization(supports_uidplus: bool) -> DeleteFinalization {
    if supports_uidplus {
        DeleteFinalization::UidExpunge
    } else {
        DeleteFinalization::DeferredServerCleanup
    }
}

fn required_uid_set(uids: &[u32]) -> Result<String> {
    if uids.is_empty() {
        return Err(MailError::Validation(
            "at least one message UID is required".to_owned(),
        ));
    }
    if uids.contains(&0) {
        return Err(MailError::Validation(
            "message UID zero is invalid".to_owned(),
        ));
    }
    Ok(compress_uid_set(uids))
}

fn validated_mailbox_name(mailbox: &str) -> Result<&str> {
    if mailbox.trim().is_empty() {
        return Err(MailError::Validation(
            "mailbox name cannot be empty".to_owned(),
        ));
    }
    if mailbox.len() > 1_024 || mailbox.chars().any(char::is_control) {
        return Err(MailError::Validation("invalid mailbox name".to_owned()));
    }
    Ok(mailbox)
}

fn validate_history_page_size(page_size: usize) -> Result<()> {
    if page_size == 0 || page_size > MAX_HISTORY_PAGE_SIZE {
        return Err(MailError::Validation(format!(
            "history page size must be between 1 and {MAX_HISTORY_PAGE_SIZE}"
        )));
    }
    Ok(())
}

fn older_uid_search_window(before_uid: u32) -> Option<UidSearchWindow> {
    let upper = before_uid.checked_sub(1)?;
    if upper == 0 {
        return None;
    }
    Some(UidSearchWindow {
        lower: upper
            .saturating_sub(HISTORY_UID_SEARCH_WINDOW.saturating_sub(1))
            .max(1),
        upper,
    })
}

impl UidSearchWindow {
    fn query(self) -> String {
        format!("UID {}:{}", self.lower, self.upper)
    }
}

fn finish_older_uid_search(
    uids: impl IntoIterator<Item = u32>,
    window: UidSearchWindow,
    page_size: usize,
) -> OlderUidSearchPage {
    let mut uids: Vec<u32> = uids
        .into_iter()
        .filter(|uid| *uid >= window.lower && *uid <= window.upper)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let had_more_matches = uids.len() > page_size;
    if had_more_matches {
        uids.drain(..uids.len() - page_size);
    }

    let next_before_uid = if had_more_matches {
        uids.first().copied()
    } else if window.lower > 1 {
        Some(window.lower)
    } else {
        None
    };
    OlderUidSearchPage {
        uids,
        next_before_uid,
        reached_uid_floor: next_before_uid.is_none(),
    }
}

fn ensure_flag_state(
    confirmed: &[(u32, Vec<String>)],
    uids: &[u32],
    flag_name: &str,
    desired: bool,
    state_name: &str,
) -> Result<()> {
    for uid in uids.iter().copied().collect::<BTreeSet<_>>() {
        let flags = confirmed
            .iter()
            .find_map(|(candidate, flags)| (*candidate == uid).then_some(flags))
            .ok_or_else(|| MailError::NotFound {
                entity: "remote message UID",
                id: uid.to_string(),
            })?;
        let persisted = flags
            .iter()
            .any(|flag| flag.eq_ignore_ascii_case(flag_name));
        if persisted != desired {
            return Err(MailError::Validation(format!(
                "the IMAP server did not persist the requested {state_name} state for UID {uid}"
            )));
        }
    }
    Ok(())
}

struct OAuth2Authenticator<'a> {
    email: &'a str,
    access_token: &'a str,
}

impl async_imap::Authenticator for OAuth2Authenticator<'_> {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.email, self.access_token
        )
    }
}

pub(crate) fn compress_uid_set(uids: &[u32]) -> String {
    let sorted: BTreeSet<u32> = uids.iter().copied().collect();
    let mut ranges = Vec::new();
    let mut start: Option<u32> = None;
    let mut previous = 0;

    for uid in sorted {
        match start {
            None => {
                start = Some(uid);
                previous = uid;
            }
            Some(_) if uid == previous.saturating_add(1) => previous = uid,
            Some(range_start) => {
                push_uid_range(&mut ranges, range_start, previous);
                start = Some(uid);
                previous = uid;
            }
        }
    }
    if let Some(range_start) = start {
        push_uid_range(&mut ranges, range_start, previous);
    }
    ranges.join(",")
}

fn push_uid_range(ranges: &mut Vec<String>, start: u32, end: u32) {
    if start == end {
        ranges.push(start.to_string());
    } else {
        ranges.push(format!("{start}:{end}"));
    }
}

fn flag_name(flag: Flag<'_>) -> String {
    match flag {
        Flag::Seen => "\\Seen".to_owned(),
        Flag::Answered => "\\Answered".to_owned(),
        Flag::Flagged => "\\Flagged".to_owned(),
        Flag::Deleted => "\\Deleted".to_owned(),
        Flag::Draft => "\\Draft".to_owned(),
        Flag::Recent => "\\Recent".to_owned(),
        Flag::MayCreate => "\\*".to_owned(),
        Flag::Custom(value) => value.into_owned(),
    }
}

fn mailbox_allows_seen_updates(permanent_flags: &[Flag<'_>]) -> bool {
    permanent_flags.is_empty()
        || permanent_flags
            .iter()
            .any(|flag| matches!(flag, Flag::Seen))
}

fn mailbox_allows_flagged_updates(permanent_flags: &[Flag<'_>]) -> bool {
    permanent_flags.is_empty()
        || permanent_flags
            .iter()
            .any(|flag| matches!(flag, Flag::Flagged))
}

fn mailbox_allows_deleted_updates(permanent_flags: &[Flag<'_>]) -> bool {
    permanent_flags.is_empty()
        || permanent_flags
            .iter()
            .any(|flag| matches!(flag, Flag::Deleted))
}

#[cfg(test)]
mod tests {
    use async_imap::types::{Capability, Flag, NameAttribute};

    use super::{
        CreatableMailboxRole, DeleteFinalization, FixedFlagMutation, HISTORY_UID_SEARCH_WINDOW,
        MAX_HISTORY_PAGE_SIZE, MailboxMessageScope, MessageMoveMethod, SUMMARY_PREVIEW_BYTES,
        UidSearchWindow, capability_atom_matches, choose_delete_finalization,
        choose_message_move_method, classify_remote_mailbox, compress_uid_set, ensure_flag_state,
        finish_older_uid_search, mailbox_allows_deleted_updates, mailbox_allows_flagged_updates,
        mailbox_allows_seen_updates, message_parts_fetch_query_with_limit,
        message_structure_fetch_query, older_uid_search_window, required_uid_set,
        summary_fetch_query, validate_history_page_size, validated_mailbox_name,
    };

    #[test]
    fn summary_fetch_is_bounded_and_does_not_mark_messages_seen() {
        let query = summary_fetch_query();

        assert!(query.contains("BODY.PEEK[]"));
        assert!(query.contains(&format!("<0.{SUMMARY_PREVIEW_BYTES}>")));
        assert!(!query.contains("BODY[]"));
    }

    #[test]
    fn selected_reader_fetches_structure_and_only_requested_mime_parts() {
        assert_eq!(message_structure_fetch_query(), "(UID BODYSTRUCTURE)");
        let query =
            message_parts_fetch_query_with_limit(&[vec![1], vec![2, 1]], None).expect("part query");

        assert!(query.contains("BODY.PEEK[1.MIME]"));
        assert!(query.contains("BODY.PEEK[1]"));
        assert!(query.contains("BODY.PEEK[2.1.MIME]"));
        assert!(query.contains("BODY.PEEK[2.1]"));
        assert!(!query.contains("BODY.PEEK[]"));
        assert!(message_parts_fetch_query_with_limit(&[vec![0]], None).is_err());

        let bounded =
            message_parts_fetch_query_with_limit(&[vec![1]], Some(12)).expect("bounded query");
        assert!(bounded.contains("BODY.PEEK[1]<0.13>"));
        assert!(!bounded.contains("BODY.PEEK[1] BODY.PEEK"));
    }

    #[test]
    fn compresses_sorted_or_unsorted_uid_sets() {
        assert_eq!(compress_uid_set(&[]), "");
        assert_eq!(compress_uid_set(&[9]), "9");
        assert_eq!(compress_uid_set(&[8, 1, 2, 3, 3, 7, 10]), "1:3,7:8,10");
    }

    #[test]
    fn dangerous_uid_sets_are_non_empty_and_never_contain_zero() {
        assert!(required_uid_set(&[]).is_err());
        assert!(required_uid_set(&[0]).is_err());
        assert!(required_uid_set(&[1, 2, 2, 4]).is_ok_and(|set| set == "1:2,4"));
    }

    #[test]
    fn mailbox_names_reject_empty_oversized_and_control_char_inputs() {
        assert!(validated_mailbox_name("Archive").is_ok());
        assert!(validated_mailbox_name(" ").is_err());
        assert!(validated_mailbox_name("Bad\r\nMailbox").is_err());
        assert!(validated_mailbox_name(&"x".repeat(1_025)).is_err());
    }

    #[test]
    fn capability_atoms_are_matched_case_insensitively_without_auth_confusion() {
        assert!(capability_atom_matches(
            &Capability::Atom("move".to_owned()),
            "MOVE"
        ));
        assert!(capability_atom_matches(
            &Capability::Atom("Special-Use".to_owned()),
            "SPECIAL-USE"
        ));
        assert!(!capability_atom_matches(
            &Capability::Auth("MOVE".to_owned()),
            "MOVE"
        ));
        assert!(!capability_atom_matches(&Capability::Imap4rev1, "MOVE"));
    }

    #[test]
    fn classifies_selectable_special_use_mailboxes() {
        let archive = classify_remote_mailbox(
            "All Mail",
            &[
                NameAttribute::All,
                NameAttribute::Archive,
                NameAttribute::Drafts,
            ],
        );
        assert_eq!(archive.name, "All Mail");
        assert!(archive.is_all);
        assert!(archive.is_archive);
        assert!(archive.is_drafts);
        assert!(!archive.is_sent);
        assert!(!archive.is_trash);
        assert!(archive.is_selectable);

        let trash =
            classify_remote_mailbox("Bin", &[NameAttribute::Trash, NameAttribute::NoSelect]);
        assert!(trash.is_trash);
        assert!(!trash.is_selectable);
    }

    #[test]
    fn product_managed_create_roles_have_fixed_names() {
        assert_eq!(CreatableMailboxRole::Archive.canonical_name(), "Archive");
        assert_eq!(CreatableMailboxRole::Trash.canonical_name(), "Trash");
    }

    #[test]
    fn capability_driven_command_selection_is_explicit() {
        assert_eq!(choose_message_move_method(true), MessageMoveMethod::UidMove);
        assert_eq!(
            choose_message_move_method(false),
            MessageMoveMethod::UidCopyThenDelete
        );
        assert_eq!(
            choose_delete_finalization(true),
            DeleteFinalization::UidExpunge
        );
        assert_eq!(
            choose_delete_finalization(false),
            DeleteFinalization::DeferredServerCleanup
        );
    }

    #[test]
    fn store_queries_are_fixed_and_never_replace_unrelated_flags() {
        assert_eq!(
            FixedFlagMutation::Seen(true).query(),
            "+FLAGS.SILENT (\\Seen)"
        );
        assert_eq!(
            FixedFlagMutation::Seen(false).query(),
            "-FLAGS.SILENT (\\Seen)"
        );
        assert_eq!(
            FixedFlagMutation::Flagged(true).query(),
            "+FLAGS.SILENT (\\Flagged)"
        );
        assert_eq!(
            FixedFlagMutation::Flagged(false).query(),
            "-FLAGS.SILENT (\\Flagged)"
        );
        assert_eq!(
            FixedFlagMutation::Deleted.query(),
            "+FLAGS.SILENT (\\Deleted)"
        );
    }

    #[test]
    fn verifies_both_positive_and_negative_flag_state() {
        let confirmed = vec![
            (7, vec!["\\Seen".to_owned(), "\\Flagged".to_owned()]),
            (8, vec!["\\Flagged".to_owned()]),
        ];
        assert!(ensure_flag_state(&confirmed, &[7], "\\Seen", true, "read").is_ok());
        assert!(ensure_flag_state(&confirmed, &[8], "\\Seen", false, "read").is_ok());
        assert!(ensure_flag_state(&confirmed, &[8], "\\Seen", true, "read").is_err());
        assert!(ensure_flag_state(&confirmed, &[9], "\\Seen", false, "read").is_err());
    }

    #[test]
    fn older_history_uses_a_bounded_exclusive_uid_window() {
        let window = older_uid_search_window(5_001).expect("window");
        assert_eq!(
            window,
            UidSearchWindow {
                lower: 4_001,
                upper: 5_000
            }
        );
        assert_eq!(window.upper - window.lower + 1, HISTORY_UID_SEARCH_WINDOW);
        assert_eq!(window.query(), "UID 4001:5000");
        assert_eq!(older_uid_search_window(1), None);
        assert_eq!(older_uid_search_window(0), None);
    }

    #[test]
    fn gmail_archive_scope_uses_the_same_provider_query_for_sync_and_history() {
        let scope = MailboxMessageScope::GmailArchive;
        assert_eq!(
            scope.search_query(),
            r#"X-GM-RAW "in:archive -in:sent -in:drafts -in:spam -in:trash""#
        );
        assert_eq!(
            scope.bounded_search_query(
                UidSearchWindow {
                    lower: 4_001,
                    upper: 5_000,
                },
                false,
            ),
            r#"UID 4001:5000 X-GM-RAW "in:archive -in:sent -in:drafts -in:spam -in:trash""#
        );
        assert_eq!(
            scope.bounded_search_query(
                UidSearchWindow {
                    lower: 4_001,
                    upper: 5_000,
                },
                true,
            ),
            r#"UID 4001:5000 X-GM-RAW "in:archive -in:sent -in:drafts -in:spam -in:trash" FLAGGED"#
        );
    }

    #[test]
    fn older_history_returns_newest_page_and_an_exclusive_cursor() {
        let page = finish_older_uid_search(
            [5_001, 4_999, 4_997, 4_998, 4_998, 4_000],
            UidSearchWindow {
                lower: 4_001,
                upper: 5_000,
            },
            2,
        );
        assert_eq!(page.uids, vec![4_998, 4_999]);
        assert_eq!(page.next_before_uid, Some(4_998));
        assert!(!page.reached_uid_floor);
    }

    #[test]
    fn sparse_history_advances_without_claiming_the_uid_floor_was_reached() {
        let sparse_page = finish_older_uid_search(
            [],
            UidSearchWindow {
                lower: 4_001,
                upper: 5_000,
            },
            50,
        );
        assert!(sparse_page.uids.is_empty());
        assert_eq!(sparse_page.next_before_uid, Some(4_001));
        assert!(!sparse_page.reached_uid_floor);

        let final_page = finish_older_uid_search([], UidSearchWindow { lower: 1, upper: 2 }, 50);
        assert_eq!(final_page.next_before_uid, None);
        assert!(final_page.reached_uid_floor);
    }

    #[test]
    fn history_page_size_is_bounded() {
        assert!(validate_history_page_size(1).is_ok());
        assert!(validate_history_page_size(MAX_HISTORY_PAGE_SIZE).is_ok());
        assert!(validate_history_page_size(0).is_err());
        assert!(validate_history_page_size(MAX_HISTORY_PAGE_SIZE + 1).is_err());
    }

    #[test]
    fn detects_advertised_seen_flag_support() {
        assert!(mailbox_allows_seen_updates(&[]));
        assert!(mailbox_allows_seen_updates(&[Flag::Seen, Flag::Flagged]));
        assert!(!mailbox_allows_seen_updates(&[Flag::Flagged]));
    }

    #[test]
    fn detects_advertised_flagged_support() {
        assert!(mailbox_allows_flagged_updates(&[]));
        assert!(mailbox_allows_flagged_updates(&[Flag::Seen, Flag::Flagged]));
        assert!(!mailbox_allows_flagged_updates(&[Flag::Seen]));
    }

    #[test]
    fn detects_advertised_deleted_flag_support() {
        assert!(mailbox_allows_deleted_updates(&[]));
        assert!(mailbox_allows_deleted_updates(&[
            Flag::Deleted,
            Flag::Flagged
        ]));
        assert!(!mailbox_allows_deleted_updates(&[Flag::Seen]));
    }
}
