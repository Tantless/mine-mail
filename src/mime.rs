use lettre::{
    Address, Message,
    address::Envelope,
    message::{Mailbox, header::ContentType},
};
use mail_parser::{Address as ParsedAddress, MessageParser, MimeHeaders};

use crate::{ComposeRequest, InboxMessage, MailAddress, MailError, Result};

pub(crate) struct OutgoingMessage {
    pub raw_rfc822: Vec<u8>,
    pub envelope: Envelope,
    pub recipients: Vec<String>,
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

pub(crate) fn build_draft_message(
    from: &str,
    request: &ComposeRequest,
    draft_id: &str,
) -> Result<Vec<u8>> {
    const PLACEHOLDER: &str = "mine-mail-draft-placeholder@invalid.invalid";
    let mut request_with_destination = request.clone();
    let needs_placeholder = request.all_recipients().next().is_none();
    if needs_placeholder {
        request_with_destination.to.push(PLACEHOLDER.to_owned());
    }

    let mut raw = build_rfc822(
        from,
        &request_with_destination,
        &[
            ("X-Mine-Mail-Draft-Id", draft_id),
            ("X-Mine-Mail-Draft-Revision", "1"),
        ],
        true,
    )?;
    if needs_placeholder {
        remove_exact_header_line(&mut raw, &format!("To: {PLACEHOLDER}\r\n"))?;
    }
    Ok(raw)
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
        IncomingMetadata, build_draft_message, build_outgoing_message, parse_incoming_message,
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
    fn draft_has_stable_private_id_and_can_be_parsed() {
        let raw = build_draft_message("sender@example.com", &compose(), "draft-123")
            .expect("draft message");
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("X-Mine-Mail-Draft-Id: draft-123"));
        assert!(text.lines().any(|line| line.starts_with("Bcc:")));

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
}
