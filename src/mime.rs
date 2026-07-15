use lettre::{
    Address, Message,
    address::Envelope,
    message::{Mailbox, header::ContentType},
};
use mail_parser::{Address as ParsedAddress, HeaderValue, MessageParser, MimeHeaders, PartType};

use crate::{ComposeRequest, InboxMessage, MailAddress, MailError, Result};

pub(crate) struct OutgoingMessage {
    pub raw_rfc822: Vec<u8>,
    pub envelope: Envelope,
    pub recipients: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedDraftMessage {
    pub draft_id: Option<String>,
    pub revision: u64,
    pub request: ComposeRequest,
    pub has_unsupported_content: bool,
}

pub(crate) fn build_outgoing_message(
    from: &str,
    request: &ComposeRequest,
) -> Result<OutgoingMessage> {
    request.validate()?;
    let raw_rfc822 = build_rfc822(from, request, &[], false)?;
    let (envelope, recipients) = build_envelope(from, request)?;
    Ok(OutgoingMessage {
        raw_rfc822,
        envelope,
        recipients,
    })
}

pub(crate) fn build_draft_message_revision(
    from: &str,
    request: &ComposeRequest,
    draft_id: &str,
    revision: u64,
) -> Result<Vec<u8>> {
    if revision == 0 {
        return Err(MailError::Validation(
            "draft revision must be greater than zero".to_owned(),
        ));
    }
    const PLACEHOLDER: &str = "mine-mail-draft-placeholder@invalid.invalid";
    let mut request_with_destination = request.clone();
    let needs_placeholder = request.all_recipients().next().is_none();
    if needs_placeholder {
        request_with_destination.to.push(PLACEHOLDER.to_owned());
    }

    let revision = revision.to_string();
    let mut raw = build_rfc822(
        from,
        &request_with_destination,
        &[
            ("X-Mine-Mail-Draft-Id", draft_id),
            ("X-Mine-Mail-Draft-Revision", revision.as_str()),
        ],
        true,
    )?;
    if needs_placeholder {
        remove_exact_header_line(&mut raw, &format!("To: {PLACEHOLDER}\r\n"))?;
    }
    Ok(raw)
}

pub(crate) fn parse_draft_message(raw: &[u8]) -> Result<ParsedDraftMessage> {
    let message = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| MailError::Mime("draft message could not be parsed".to_owned()))?;
    let draft_id =
        text_header(&message, "X-Mine-Mail-Draft-Id").filter(|value| is_valid_draft_id(value));
    let revision = text_header(&message, "X-Mine-Mail-Draft-Revision")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|revision| *revision > 0)
        .unwrap_or(1);

    Ok(ParsedDraftMessage {
        draft_id,
        revision,
        has_unsupported_content: message_has_unsupported_draft_content(&message),
        request: ComposeRequest {
            to: map_compose_addresses(message.to()),
            cc: map_compose_addresses(message.cc()),
            bcc: map_compose_addresses(message.bcc()),
            subject: message.subject().unwrap_or_default().to_owned(),
            body_text: message
                .body_text(0)
                .map(|body| body.into_owned())
                .unwrap_or_default(),
        },
    })
}

/// Returns true unless the raw draft is one parseable, undecorated text/plain
/// body. Mine Mail's MVP editor cannot round-trip HTML, multipart structure,
/// inline resources, attachments, or unknown MIME parts without data loss.
pub(crate) fn draft_has_unsupported_content(raw: &[u8]) -> bool {
    MessageParser::default()
        .parse(raw)
        .is_none_or(|message| message_has_unsupported_draft_content(&message))
}

fn message_has_unsupported_draft_content(message: &mail_parser::Message<'_>) -> bool {
    // mail-parser intentionally indexes a single plain-text part as both a
    // text and an HTML-renderable body. Inspect the actual leaf instead of its
    // derived body indexes so an ordinary text/plain draft remains editable.
    if message.parts.len() != 1 || message.attachment_count() != 0 {
        return true;
    }
    let Some(part) = message.parts.first() else {
        return true;
    };
    if part.is_encoding_problem
        || part.content_disposition().is_some()
        || !matches!(part.body, PartType::Text(_))
    {
        return true;
    }
    part.content_type().is_some_and(|content_type| {
        !content_type.c_type.eq_ignore_ascii_case("text")
            || !content_type
                .c_subtype
                .as_deref()
                .is_some_and(|subtype| subtype.eq_ignore_ascii_case("plain"))
    })
}

fn text_header(message: &mail_parser::Message<'_>, name: &str) -> Option<String> {
    match message.header(name)? {
        HeaderValue::Text(value) => Some(value.trim().to_owned()),
        HeaderValue::TextList(values) => values.last().map(|value| value.trim().to_owned()),
        _ => None,
    }
}

fn is_valid_draft_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn map_compose_addresses(addresses: Option<&ParsedAddress<'_>>) -> Vec<String> {
    addresses
        .into_iter()
        .flat_map(ParsedAddress::iter)
        .filter_map(|address| address.address().map(str::to_owned))
        .collect()
}

pub(crate) fn build_envelope(
    from: &str,
    request: &ComposeRequest,
) -> Result<(Envelope, Vec<String>)> {
    let from_address = from
        .parse::<Address>()
        .map_err(|error| MailError::Validation(format!("invalid sender address: {error}")))?;

    let envelope_recipients = request
        .all_recipients()
        .map(|address| parse_mailbox(address, "recipient").map(|mailbox| mailbox.email))
        .collect::<Result<Vec<_>>>()?;
    let recipients = envelope_recipients
        .iter()
        .map(ToString::to_string)
        .collect();

    let envelope = Envelope::new(Some(from_address), envelope_recipients)
        .map_err(|error| MailError::Validation(format!("invalid SMTP envelope: {error}")))?;
    Ok((envelope, recipients))
}

/// Reconstructs the exact SMTP envelope needed to retry a persisted Outbox
/// item without rebuilding the message from a mutable draft.
///
/// The reverse path is recovered from the immutable RFC822 `From` header. The
/// forward paths deliberately come from the separately persisted recipient
/// list because Bcc recipients are absent from a sent message's headers.
pub(crate) fn restore_outbox_envelope(
    raw_rfc822: &[u8],
    persisted_recipients: &[String],
) -> Result<Envelope> {
    let message = MessageParser::default().parse(raw_rfc822).ok_or_else(|| {
        MailError::Mime("persisted Outbox message could not be parsed".to_owned())
    })?;
    let from = message
        .from()
        .ok_or_else(|| MailError::Mime("persisted Outbox message has no From header".to_owned()))?;
    let from_addresses = from.iter().collect::<Vec<_>>();
    if from_addresses.len() != 1 {
        return Err(MailError::Mime(
            "persisted Outbox message must have exactly one sender".to_owned(),
        ));
    }
    let from_address = from_addresses[0]
        .address()
        .ok_or_else(|| MailError::Mime("persisted Outbox sender has no address".to_owned()))?
        .parse::<Address>()
        .map_err(|error| MailError::Mime(format!("persisted Outbox sender is invalid: {error}")))?;

    if persisted_recipients.is_empty() {
        return Err(MailError::Mime(
            "persisted Outbox envelope has no recipients".to_owned(),
        ));
    }
    let recipients = persisted_recipients
        .iter()
        .map(|recipient| {
            recipient.parse::<Address>().map_err(|error| {
                MailError::Mime(format!("persisted Outbox recipient is invalid: {error}"))
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Envelope::new(Some(from_address), recipients)
        .map_err(|error| MailError::Mime(format!("persisted SMTP envelope is invalid: {error}")))
}

fn build_rfc822(
    from: &str,
    request: &ComposeRequest,
    custom_headers: &[(&str, &str)],
    include_bcc_header: bool,
) -> Result<Vec<u8>> {
    let mut builder = Message::builder()
        .from(parse_mailbox(from, "sender")?)
        .subject(request.subject.clone());

    for address in &request.to {
        builder = builder.to(parse_mailbox(address, "To recipient")?);
    }
    for address in &request.cc {
        builder = builder.cc(parse_mailbox(address, "Cc recipient")?);
    }
    if include_bcc_header {
        for address in &request.bcc {
            builder = builder.bcc(parse_mailbox(address, "Bcc recipient")?);
        }
        builder = builder.keep_bcc();
    }

    let message = builder
        .header(ContentType::TEXT_PLAIN)
        .body(request.body_text.clone())
        .map_err(|error| MailError::Mime(format!("cannot build message: {error}")))?;

    let mut raw = message.formatted();
    insert_custom_headers(&mut raw, custom_headers)?;
    Ok(raw)
}

fn parse_mailbox(value: &str, label: &str) -> Result<Mailbox> {
    value
        .parse::<Mailbox>()
        .map_err(|error| MailError::Validation(format!("invalid {label}: {error}")))
}

fn insert_custom_headers(raw: &mut Vec<u8>, headers: &[(&str, &str)]) -> Result<()> {
    if headers.is_empty() {
        return Ok(());
    }
    if headers.iter().any(|(name, value)| {
        !name.is_ascii()
            || name
                .bytes()
                .any(|byte| byte == b':' || byte.is_ascii_whitespace())
            || value.contains('\r')
            || value.contains('\n')
    }) {
        return Err(MailError::Mime(
            "custom message header contains invalid characters".to_owned(),
        ));
    }

    let separator = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| MailError::Mime("message has no header separator".to_owned()))?;
    let insert_at = separator + 2;
    let mut encoded = Vec::new();
    for (name, value) in headers {
        encoded.extend_from_slice(name.as_bytes());
        encoded.extend_from_slice(b": ");
        encoded.extend_from_slice(value.as_bytes());
        encoded.extend_from_slice(b"\r\n");
    }
    raw.splice(insert_at..insert_at, encoded);
    Ok(())
}

fn remove_exact_header_line(raw: &mut Vec<u8>, line: &str) -> Result<()> {
    let header_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| MailError::Mime("message has no header separator".to_owned()))?;
    let needle = line.as_bytes();
    let Some(start) = raw[..header_end]
        .windows(needle.len())
        .position(|window| window == needle)
    else {
        return Err(MailError::Mime(
            "could not remove internal draft placeholder".to_owned(),
        ));
    };
    raw.drain(start..start + needle.len());
    Ok(())
}

pub(crate) struct IncomingMetadata<'a> {
    pub account_id: &'a str,
    pub mailbox: &'a str,
    pub uid: u32,
    pub flags: Vec<String>,
    pub internal_date: Option<String>,
    pub size_bytes: u32,
    pub synced_at: String,
    pub body_fetched: bool,
}

pub(crate) fn parse_incoming_message(
    raw: &[u8],
    metadata: IncomingMetadata<'_>,
) -> Result<InboxMessage> {
    let message = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| MailError::Mime("message could not be parsed".to_owned()))?;

    let sender = message
        .from()
        .and_then(|address| address.first())
        .and_then(map_address);
    let to = map_addresses(message.to());
    let cc = map_addresses(message.cc());
    let attachment_names = message
        .attachments()
        .filter_map(|attachment| attachment.attachment_name().map(str::to_owned))
        .collect();

    Ok(InboxMessage {
        id: 0,
        account_id: metadata.account_id.to_owned(),
        mailbox: metadata.mailbox.to_owned(),
        uid: metadata.uid,
        message_id: message.message_id().map(str::to_owned),
        subject: message.subject().unwrap_or_default().to_owned(),
        sender,
        to,
        cc,
        sent_at: message.date().map(|date| date.to_rfc3339()),
        internal_date: metadata.internal_date,
        flags: metadata.flags,
        size_bytes: metadata.size_bytes,
        preview: message
            .body_preview(180)
            .map(|preview| preview.into_owned())
            .unwrap_or_default(),
        body_text: message.body_text(0).map(|body| body.into_owned()),
        body_html: message.body_html(0).map(|body| body.into_owned()),
        attachment_names,
        body_fetched: metadata.body_fetched,
        raw_rfc822: raw.to_vec(),
        synced_at: metadata.synced_at,
    })
}

/// Parses one Inbox header without allowing a malformed message to stop the
/// mailbox cursor. The fallback deliberately contains no body or raw bytes,
/// while retaining the IMAP identity and metadata needed for later repair.
pub(crate) fn parse_incoming_summary_or_fallback(
    raw: &[u8],
    metadata: IncomingMetadata<'_>,
) -> InboxMessage {
    let fallback = InboxMessage {
        id: 0,
        account_id: metadata.account_id.to_owned(),
        mailbox: metadata.mailbox.to_owned(),
        uid: metadata.uid,
        message_id: None,
        subject: "无法解析的邮件".to_owned(),
        sender: None,
        to: Vec::new(),
        cc: Vec::new(),
        sent_at: None,
        internal_date: metadata.internal_date.clone(),
        flags: metadata.flags.clone(),
        size_bytes: metadata.size_bytes,
        preview: String::new(),
        body_text: None,
        body_html: None,
        attachment_names: Vec::new(),
        body_fetched: false,
        raw_rfc822: Vec::new(),
        synced_at: metadata.synced_at.clone(),
    };

    parse_incoming_message(raw, metadata).unwrap_or(fallback)
}

fn map_addresses(addresses: Option<&ParsedAddress<'_>>) -> Vec<MailAddress> {
    addresses
        .into_iter()
        .flat_map(ParsedAddress::iter)
        .filter_map(map_address)
        .collect()
}

fn map_address(address: &mail_parser::Addr<'_>) -> Option<MailAddress> {
    Some(MailAddress {
        name: address.name().map(str::to_owned),
        email: address.address()?.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        IncomingMetadata, build_draft_message_revision, build_outgoing_message,
        draft_has_unsupported_content, parse_draft_message, parse_incoming_message,
        parse_incoming_summary_or_fallback, restore_outbox_envelope,
    };
    use crate::ComposeRequest;

    fn compose() -> ComposeRequest {
        ComposeRequest {
            to: vec!["Receiver <receiver@example.com>".to_owned()],
            cc: vec![],
            bcc: vec!["hidden@example.com".to_owned()],
            subject: "中文主题".to_owned(),
            body_text: "Hello, 世界".to_owned(),
        }
    }

    #[test]
    fn outgoing_message_keeps_bcc_in_envelope_but_not_headers() {
        let outgoing = build_outgoing_message("sender@example.com", &compose()).expect("message");
        let text = String::from_utf8_lossy(&outgoing.raw_rfc822);

        assert_eq!(outgoing.recipients.len(), 2);
        assert!(!text.lines().any(|line| line.starts_with("Bcc:")));
        assert!(!text.contains("hidden@example.com"));
    }

    #[test]
    fn persisted_outbox_envelope_restores_sender_and_hidden_recipient() {
        let outgoing = build_outgoing_message("sender@example.com", &compose()).expect("message");

        let restored =
            restore_outbox_envelope(&outgoing.raw_rfc822, &outgoing.recipients).expect("envelope");

        assert_eq!(
            restored.from().map(ToString::to_string).as_deref(),
            Some("sender@example.com")
        );
        assert_eq!(
            restored
                .to()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["receiver@example.com", "hidden@example.com"]
        );
    }

    #[test]
    fn persisted_outbox_envelope_rejects_unsafe_or_incomplete_state() {
        let raw_without_from = b"To: receiver@example.com\r\n\r\nBody";
        assert!(
            restore_outbox_envelope(raw_without_from, &["receiver@example.com".to_owned()])
                .is_err()
        );

        let multiple_from =
            b"From: first@example.com, second@example.com\r\nTo: receiver@example.com\r\n\r\nBody";
        assert!(
            restore_outbox_envelope(multiple_from, &["receiver@example.com".to_owned()]).is_err()
        );

        let no_recipients = b"From: sender@example.com\r\n\r\nBody";
        assert!(restore_outbox_envelope(no_recipients, &[]).is_err());
    }

    #[test]
    fn draft_has_stable_private_id_and_can_be_parsed() {
        let raw = build_draft_message_revision("sender@example.com", &compose(), "draft-123", 7)
            .expect("draft message");
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("X-Mine-Mail-Draft-Id: draft-123"));
        assert!(text.contains("X-Mine-Mail-Draft-Revision: 7"));
        assert!(text.lines().any(|line| line.starts_with("Bcc:")));

        let parsed_draft = parse_draft_message(&raw).expect("parse draft metadata");
        assert_eq!(parsed_draft.draft_id.as_deref(), Some("draft-123"));
        assert_eq!(parsed_draft.revision, 7);
        assert_eq!(parsed_draft.request.to, ["receiver@example.com"]);
        assert_eq!(parsed_draft.request.bcc, ["hidden@example.com"]);
        assert_eq!(parsed_draft.request.subject, "中文主题");
        assert_eq!(parsed_draft.request.body_text, "Hello, 世界");
        assert!(!parsed_draft.has_unsupported_content);

        let parsed = parse_incoming_message(
            &raw,
            IncomingMetadata {
                account_id: "primary",
                mailbox: "Drafts",
                uid: 42,
                flags: vec!["Draft".to_owned()],
                internal_date: None,
                size_bytes: raw.len() as u32,
                synced_at: "2026-07-14T00:00:00Z".to_owned(),
                body_fetched: true,
            },
        )
        .expect("parse draft");
        assert_eq!(parsed.subject, "中文主题");
        assert_eq!(parsed.body_text.as_deref(), Some("Hello, 世界"));
    }

    #[test]
    fn foreign_draft_without_private_headers_gets_default_revision() {
        let raw = b"From: sender@example.com\r\nTo: receiver@example.com\r\nSubject: Foreign\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nBody";

        let parsed = parse_draft_message(raw).expect("foreign draft");

        assert_eq!(parsed.draft_id, None);
        assert_eq!(parsed.revision, 1);
        assert_eq!(parsed.request.to, ["receiver@example.com"]);
        assert_eq!(parsed.request.subject, "Foreign");
        assert_eq!(parsed.request.body_text, "Body");
        assert!(!parsed.has_unsupported_content);
    }

    #[test]
    fn classifies_html_attachments_and_parse_failures_as_unsupported_drafts() {
        let html = b"From: sender@example.com\r\nTo: receiver@example.com\r\nSubject: HTML\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<strong>Body</strong>";
        assert!(draft_has_unsupported_content(html));
        assert!(parse_draft_message(html).unwrap().has_unsupported_content);

        let attachment = b"From: sender@example.com\r\nTo: receiver@example.com\r\nSubject: Attachment\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nBody\r\n--x\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=file.bin\r\nContent-Transfer-Encoding: base64\r\n\r\nAQID\r\n--x--\r\n";
        assert!(draft_has_unsupported_content(attachment));
        assert!(
            parse_draft_message(attachment)
                .unwrap()
                .has_unsupported_content
        );

        assert!(draft_has_unsupported_content(b"not an RFC822 message"));
    }

    #[test]
    fn malformed_summary_falls_back_and_does_not_block_the_next_valid_header() {
        let malformed = parse_incoming_summary_or_fallback(
            b"",
            IncomingMetadata {
                account_id: "primary",
                mailbox: "INBOX",
                uid: 40,
                flags: vec!["Seen".to_owned()],
                internal_date: Some("2026-07-14T00:00:00Z".to_owned()),
                size_bytes: 27,
                synced_at: "2026-07-14T00:01:00Z".to_owned(),
                body_fetched: false,
            },
        );
        let valid = parse_incoming_summary_or_fallback(
            b"From: sender@example.com\r\nSubject: Later message\r\n\r\n",
            IncomingMetadata {
                account_id: "primary",
                mailbox: "INBOX",
                uid: 41,
                flags: Vec::new(),
                internal_date: Some("2026-07-14T00:02:00Z".to_owned()),
                size_bytes: 54,
                synced_at: "2026-07-14T00:03:00Z".to_owned(),
                body_fetched: false,
            },
        );

        assert_eq!(malformed.uid, 40);
        assert_eq!(malformed.subject, "无法解析的邮件");
        assert_eq!(malformed.flags, ["Seen"]);
        assert_eq!(
            malformed.internal_date.as_deref(),
            Some("2026-07-14T00:00:00Z")
        );
        assert_eq!(malformed.size_bytes, 27);
        assert_eq!(malformed.body_text, None);
        assert_eq!(malformed.body_html, None);
        assert!(!malformed.body_fetched);
        assert!(malformed.raw_rfc822.is_empty());

        assert_eq!(valid.uid, 41);
        assert_eq!(valid.subject, "Later message");
    }
}
