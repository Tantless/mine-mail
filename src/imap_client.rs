use std::{collections::BTreeSet, time::Duration};

use async_imap::{
    Session,
    types::{Flag, NameAttribute},
};
use async_native_tls::TlsStream;
use futures::TryStreamExt;
use tokio::{net::TcpStream, time::timeout};

use crate::{AccountConfig, MailError, Result};

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

        let mut session = timeout(
            CONNECT_TIMEOUT,
            client.login(&config.email, config.authorization_password()),
        )
        .await
        .map_err(|_| MailError::Timeout {
            operation: "IMAP authentication",
        })?
        .map_err(|(error, _client)| MailError::Imap(error.to_string()))?;

        let capabilities = timeout(COMMAND_TIMEOUT, session.capabilities())
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP CAPABILITY",
            })?
            .map_err(|error| MailError::Imap(error.to_string()))?;

        // NetEase documents/uses RFC 2971 client identification. Sending it
        // after LOGIN and before SELECT avoids the provider's “Unsafe Login”
        // path while containing no user data.
        if capabilities.has_str("ID") {
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

        let supports_uidplus = capabilities.has_str("UIDPLUS");
        Ok(Self {
            session,
            supports_uidplus,
        })
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
            })
            .collect())
    }

    pub async fn select_inbox(&mut self) -> Result<MailboxSnapshot> {
        let selected = timeout(COMMAND_TIMEOUT, self.session.select("INBOX"))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP SELECT INBOX",
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

    /// Selects INBOX for a known-UID body fetch without the full UID SEARCH
    /// required by metadata reconciliation.
    pub async fn select_inbox_for_fetch(&mut self) -> Result<Option<u32>> {
        timeout(COMMAND_TIMEOUT, self.session.select("INBOX"))
            .await
            .map_err(|_| MailError::Timeout {
                operation: "IMAP SELECT INBOX",
            })?
            .map(|selected| selected.uid_validity)
            .map_err(|error| MailError::Imap(error.to_string()))
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

#[cfg(test)]
mod tests {
    use super::compress_uid_set;

    #[test]
    fn compresses_sorted_or_unsorted_uid_sets() {
        assert_eq!(compress_uid_set(&[]), "");
        assert_eq!(compress_uid_set(&[9]), "9");
        assert_eq!(compress_uid_set(&[8, 1, 2, 3, 3, 7, 10]), "1:3,7:8,10");
    }
}
