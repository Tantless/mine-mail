use std::{collections::BTreeSet, time::Duration};

use async_imap::{
    Session,
    extensions::idle::IdleResponse,
    types::{Flag, NameAttribute},
};
use async_native_tls::TlsStream;
use futures::TryStreamExt;
use tokio::{net::TcpStream, time::timeout};

use crate::{AccountConfig, AuthenticationKind, MailError, Result};

type ImapSession = Session<TlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(45);

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

#[derive(Clone, Debug)]
pub(crate) struct RemoteMessage {
    pub uid: u32,
    pub flags: Vec<String>,
    pub internal_date: Option<String>,
    pub size_bytes: u32,
    pub raw: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct RemoteMailbox {
    pub name: String,
    pub is_drafts: bool,
    pub is_sent: bool,
    pub is_selectable: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct RemoteDraftSnapshot {
    pub mailbox: String,
    pub uid_validity: Option<u32>,
    pub messages: Vec<RemoteMessage>,
}

pub(crate) struct ImapConnection {
    session: ImapSession,
    supports_uidplus: bool,
    supports_idle: bool,
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
        let supports_id = capabilities.has_str("ID");
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
        let supports_uidplus = capabilities.has_str("UIDPLUS");
        let supports_idle = capabilities.has_str("IDLE");
        Ok(Self {
            session,
            supports_uidplus,
            supports_idle,
        })
    }

    pub fn supports_idle(&self) -> bool {
        self.supports_idle
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
            .map(|name| RemoteMailbox {
                name: name.name().to_owned(),
                is_drafts: name
                    .attributes()
                    .iter()
                    .any(|attribute| matches!(attribute, NameAttribute::Drafts)),
                is_sent: name
                    .attributes()
                    .iter()
                    .any(|attribute| matches!(attribute, NameAttribute::Sent)),
                is_selectable: !name
                    .attributes()
                    .iter()
                    .any(|attribute| matches!(attribute, NameAttribute::NoSelect)),
            })
            .collect())
    }

    pub async fn select_mailbox(&mut self, mailbox: &str) -> Result<MailboxSnapshot> {
        let selected = timeout(COMMAND_TIMEOUT, self.session.select(mailbox))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP SELECT mailbox",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
        let all_uids = self.search_all_uids().await?;

        Ok(MailboxSnapshot {
            exists: selected.exists,
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

    /// Enter one bounded IDLE cycle and restore the session with DONE before
    /// returning. The caller reconnects on any error or maintenance timeout.
    pub async fn wait_for_idle_change(self, duration: Duration) -> Result<(Self, bool)> {
        let supports_uidplus = self.supports_uidplus;
        let supports_idle = self.supports_idle;
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
                supports_uidplus,
                supports_idle,
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

    async fn search_all_uids(&mut self) -> Result<Vec<u32>> {
        let uids = timeout(COMMAND_TIMEOUT, self.session.uid_search("ALL"))
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
        self.fetch_messages(
            uids,
            "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER])",
            false,
        )
        .await
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

    pub async fn fetch_flags(&mut self, uids: &[u32]) -> Result<Vec<(u32, Vec<String>)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let sequence_set = compress_uid_set(uids);
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

    /// Adds `\Seen` without replacing unrelated flags and returns the
    /// server-confirmed flag set for every requested UID.
    pub async fn add_seen_flags(&mut self, uids: &[u32]) -> Result<Vec<(u32, Vec<String>)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let sequence_set = compress_uid_set(uids);
        {
            let stream = timeout(
                COMMAND_TIMEOUT,
                self.session
                    .uid_store(&sequence_set, "+FLAGS.SILENT (\\Seen)"),
            )
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP mark message read",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
            timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
                .await
                .map_err(|_| MailError::Timeout {
                    operation: "IMAP mark message read response",
                })?
                .map_err(|error| MailError::Imap(error.to_string()))?;
        }

        let confirmed = self.fetch_flags(uids).await?;
        for uid in uids.iter().copied().collect::<BTreeSet<_>>() {
            let flags = confirmed
                .iter()
                .find_map(|(candidate, flags)| (*candidate == uid).then_some(flags))
                .ok_or_else(|| MailError::NotFound {
                    entity: "remote message UID",
                    id: uid.to_string(),
                })?;
            if !flags.iter().any(|flag| flag.eq_ignore_ascii_case("\\Seen")) {
                return Err(MailError::Validation(format!(
                    "the IMAP server did not persist the read flag for UID {uid}"
                )));
            }
        }
        Ok(confirmed)
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
        let sequence_set = compress_uid_set(uids);
        let query = if desired {
            "+FLAGS.SILENT (\\Flagged)"
        } else {
            "-FLAGS.SILENT (\\Flagged)"
        };
        {
            let stream = timeout(
                COMMAND_TIMEOUT,
                self.session.uid_store(&sequence_set, query),
            )
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP update message star",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;
            timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
                .await
                .map_err(|_| MailError::Timeout {
                    operation: "IMAP update message star response",
                })?
                .map_err(|error| MailError::Imap(error.to_string()))?;
        }

        let confirmed = self.fetch_flags(uids).await?;
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
                .any(|flag| flag.eq_ignore_ascii_case("\\Flagged"));
            if persisted != desired {
                return Err(MailError::Validation(format!(
                    "the IMAP server did not persist the requested star state for UID {uid}"
                )));
            }
        }
        Ok(confirmed)
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
        let sequence_set = compress_uid_set(uids);
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
        for batch in uids.chunks(100) {
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
        let sequence_set = compress_uid_set(uids);
        let stream = timeout(
            COMMAND_TIMEOUT,
            self.session
                .uid_store(&sequence_set, "+FLAGS.SILENT (\\Deleted)"),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: "IMAP mark draft deleted",
        })?
        .map_err(|error| MailError::Imap(error.to_string()))?;
        timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP draft delete response",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;

        if self.supports_uidplus {
            let stream = timeout(COMMAND_TIMEOUT, self.session.uid_expunge(&sequence_set))
                .await
                .map_err(|_| MailError::Timeout {
                    operation: "IMAP UID EXPUNGE draft",
                })?
                .map_err(|error| MailError::Imap(error.to_string()))?;
            timeout(COMMAND_TIMEOUT, stream.try_collect::<Vec<_>>())
                .await
                .map_err(|_| MailError::Timeout {
                    operation: "IMAP UID EXPUNGE response",
                })?
                .map_err(|error| MailError::Imap(error.to_string()))?;
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

#[cfg(test)]
mod tests {
    use async_imap::types::Flag;

    use super::{compress_uid_set, mailbox_allows_flagged_updates, mailbox_allows_seen_updates};

    #[test]
    fn compresses_sorted_or_unsorted_uid_sets() {
        assert_eq!(compress_uid_set(&[]), "");
        assert_eq!(compress_uid_set(&[9]), "9");
        assert_eq!(compress_uid_set(&[8, 1, 2, 3, 3, 7, 10]), "1:3,7:8,10");
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
}
