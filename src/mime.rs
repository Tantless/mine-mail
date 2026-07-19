use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::DateTime;
use lettre::{
    Address, Message,
    address::Envelope,
    message::{Mailbox, MultiPart, SinglePart, header::ContentType},
};
use mail_parser::{Address as ParsedAddress, HeaderValue, MessageParser, MimeHeaders, PartType};
use uuid::Uuid;

use crate::{ComposeRequest, InboxMessage, MailAddress, MailError, ReplyContext, Result};

const MINE_MAIL_REPLY_FORMAT_HEADER: &str = "X-Mine-Mail-Reply-Format";
const MINE_MAIL_REPLY_FORMAT_VERSION: &str = "1";
const MAX_QUOTED_TEXT_BYTES: usize = 2 * 1024 * 1024;

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
    // Use a stable, app-generated identifier for the exact bytes persisted in
    // Outbox. The same identifier will later arrive through the provider's
    // Sent mailbox and lets the desktop merge both views without guessing.
    // The reserved `.invalid` TLD avoids disclosing the local host name.
    let message_id = format!("<{}@mine-mail.invalid>", Uuid::now_v7());
    let mut headers = vec![("Message-ID".to_owned(), message_id)];
    headers.extend(reply_headers(request)?);
    let raw_rfc822 = build_rfc822(from, request, &headers, false, true)?;
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
    let mut headers = vec![
        ("X-Mine-Mail-Draft-Id".to_owned(), draft_id.to_owned()),
        ("X-Mine-Mail-Draft-Revision".to_owned(), revision),
    ];
    headers.extend(reply_headers(&request_with_destination)?);
    let mut raw = build_rfc822(from, &request_with_destination, &headers, true, false)?;
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
    let encoded_body = message
        .body_text(0)
        .map(|body| body.into_owned())
        .unwrap_or_default();
    let mine_mail_reply = text_header(&message, MINE_MAIL_REPLY_FORMAT_HEADER)
        .is_some_and(|value| value == MINE_MAIL_REPLY_FORMAT_VERSION);
    let parsed_reply = mine_mail_reply
        .then(|| parse_mine_mail_reply_draft(&message, &encoded_body))
        .transpose()?;
    let (body_text, reply_context) = match parsed_reply {
        Some((body_text, reply_context)) => (body_text, Some(reply_context)),
        None => (encoded_body, None),
    };

    Ok(ParsedDraftMessage {
        draft_id,
        revision,
        has_unsupported_content: message_has_unsupported_draft_content(&message),
        request: ComposeRequest {
            to: map_compose_addresses(message.to()),
            cc: map_compose_addresses(message.cc()),
            bcc: map_compose_addresses(message.bcc()),
            subject: message.subject().unwrap_or_default().to_owned(),
            body_text,
            reply_context,
        },
    })
}

fn parse_mine_mail_reply_draft(
    message: &mail_parser::Message<'_>,
    body: &str,
) -> Result<(String, ReplyContext)> {
    let lines = body.lines().collect::<Vec<_>>();
    let separator = lines
        .iter()
        .position(|line| parse_reply_intro(line).is_some())
        .ok_or_else(|| {
            MailError::Mime(
                "Mine Mail reply draft lost its quoted-message boundary; it cannot be edited safely"
                    .to_owned(),
            )
        })?;
    let intro = parse_reply_intro(lines[separator]).expect("reply boundary was just validated");
    let authored = lines[..separator].join("\n").trim_end().to_owned();
    let quoted = strip_one_quote_level(&lines[separator + 1..]);
    if quoted.trim().is_empty() {
        return Err(MailError::Mime(
            "Mine Mail reply draft has no quoted message body".to_owned(),
        ));
    }
    let mut references = message_ids(message.references());
    let parent_message_id = message_ids(message.in_reply_to()).pop();
    if let Some(parent) = parent_message_id.as_deref() {
        references.retain(|value| !value.eq_ignore_ascii_case(parent));
    }
    Ok((
        authored,
        ReplyContext {
            parent_message_id,
            references,
            subject: reply_parent_subject(message.subject().unwrap_or_default()),
            sender: intro.sender,
            recipients: map_addresses(message.from()),
            sent_at: intro.sent_at,
            quoted_text: quoted,
        },
    ))
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

#[derive(Debug)]
struct ParsedReplyIntro {
    sender: Option<MailAddress>,
    sent_at: Option<String>,
}

fn parse_reply_intro(line: &str) -> Option<ParsedReplyIntro> {
    let remainder = line.trim().strip_prefix("At ")?.strip_suffix(" wrote:")?;
    let (sent_at, sender) = remainder.split_once(", ")?;
    let parsed_time = if sent_at == "unknown time" {
        Some(None)
    } else {
        DateTime::parse_from_str(sent_at, "%Y-%m-%d %H:%M:%S %:z")
            .or_else(|_| DateTime::parse_from_str(sent_at, "%Y-%m-%d %H:%M:%S %z"))
            .ok()
            .map(|value| Some(value.to_rfc3339()))
    }?;
    let sender = if sender == "unknown sender" {
        None
    } else {
        Some(parse_reply_sender(sender)?)
    };
    Some(ParsedReplyIntro {
        sender,
        sent_at: parsed_time,
    })
}

fn parse_reply_sender(value: &str) -> Option<MailAddress> {
    let value = value.trim();
    let (name, email) = if let Some(open) = value.rfind('<') {
        let email = value.get(open + 1..)?.strip_suffix('>')?.trim();
        let name = value[..open].trim().trim_matches('"').trim().to_owned();
        ((!name.is_empty()).then_some(name), email)
    } else {
        (None, value)
    };
    email.parse::<Address>().ok()?;
    Some(MailAddress {
        name,
        email: email.to_owned(),
    })
}

fn strip_one_quote_level(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|line| {
            let line = line.strip_prefix('>').unwrap_or(line);
            line.strip_prefix(' ').unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_owned()
}

fn reply_parent_subject(subject: &str) -> String {
    let subject = subject.trim();
    let subject = subject
        .get(..3)
        .filter(|prefix| prefix.eq_ignore_ascii_case("re:"))
        .map_or(subject, |_| subject[3..].trim_start());
    subject.to_owned()
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

pub(crate) fn outbox_subject(raw_rfc822: &[u8]) -> Option<String> {
    MessageParser::default()
        .parse(raw_rfc822)
        .and_then(|message| message.subject().map(str::to_owned))
}

pub(crate) fn outbox_preview(raw_rfc822: &[u8]) -> Option<String> {
    MessageParser::default()
        .parse(raw_rfc822)
        .and_then(|message| {
            message
                .body_preview(180)
                .map(|preview| preview.into_owned())
        })
}

pub(crate) fn outbox_body_text(raw_rfc822: &[u8]) -> Option<String> {
    MessageParser::default()
        .parse(raw_rfc822)
        .and_then(|message| message.body_text(0).map(|body| body.into_owned()))
}

pub(crate) fn outbox_message_id(raw_rfc822: &[u8]) -> Option<String> {
    MessageParser::default()
        .parse(raw_rfc822)
        .and_then(|message| message.message_id().map(str::to_owned))
}

pub(crate) fn outbox_sent_at(raw_rfc822: &[u8]) -> Option<String> {
    MessageParser::default()
        .parse(raw_rfc822)
        .and_then(|message| message.date().map(|date| date.to_rfc3339()))
}

fn build_rfc822(
    from: &str,
    request: &ComposeRequest,
    custom_headers: &[(String, String)],
    include_bcc_header: bool,
    allow_html_reply: bool,
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

    let plain_body = reply_plain_body(request)?;
    let message = if allow_html_reply && request.reply_context.is_some() {
        builder
            .multipart(
                MultiPart::alternative()
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_PLAIN)
                            .body(plain_body),
                    )
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_HTML)
                            .body(reply_html_body(request)?),
                    ),
            )
            .map_err(|error| MailError::Mime(format!("cannot build message: {error}")))?
    } else {
        builder
            .header(ContentType::TEXT_PLAIN)
            .body(plain_body)
            .map_err(|error| MailError::Mime(format!("cannot build message: {error}")))?
    };

    let mut raw = message.formatted();
    insert_custom_headers(&mut raw, custom_headers)?;
    Ok(raw)
}

fn reply_headers(request: &ComposeRequest) -> Result<Vec<(String, String)>> {
    let Some(context) = request.reply_context.as_ref() else {
        return Ok(Vec::new());
    };
    validate_reply_context(context)?;
    let mut headers = vec![(
        MINE_MAIL_REPLY_FORMAT_HEADER.to_owned(),
        MINE_MAIL_REPLY_FORMAT_VERSION.to_owned(),
    )];
    let parent = context
        .parent_message_id
        .as_deref()
        .and_then(normalize_message_id);
    if let Some(parent) = parent.as_ref() {
        headers.push(("In-Reply-To".to_owned(), format!("<{parent}>")));
    }

    let mut references = context
        .references
        .iter()
        .filter_map(|value| normalize_message_id(value))
        .collect::<Vec<_>>();
    if let Some(parent) = parent {
        references.retain(|value| !value.eq_ignore_ascii_case(&parent));
        references.push(parent);
    }
    references = bounded_reference_chain(references);
    if !references.is_empty() {
        headers.push((
            "References".to_owned(),
            references
                .into_iter()
                .map(|value| format!("<{value}>"))
                .collect::<Vec<_>>()
                .join(" "),
        ));
    }
    Ok(headers)
}

fn validate_reply_context(context: &ReplyContext) -> Result<()> {
    if context.quoted_text.trim().is_empty() {
        return Err(MailError::Validation(
            "a reply must retain the quoted message body".to_owned(),
        ));
    }
    if context.quoted_text.len() > MAX_QUOTED_TEXT_BYTES {
        return Err(MailError::Validation(
            "the quoted message is too large to include in a reply".to_owned(),
        ));
    }
    if let Some(sender) = context.sender.as_ref() {
        sender.email.parse::<Address>().map_err(|error| {
            MailError::Validation(format!("invalid quoted-message sender: {error}"))
        })?;
    }
    if context
        .parent_message_id
        .as_deref()
        .is_some_and(|value| normalize_message_id(value).is_none())
        || context
            .references
            .iter()
            .any(|value| normalize_message_id(value).is_none())
    {
        return Err(MailError::Validation(
            "reply message identifiers contain invalid characters".to_owned(),
        ));
    }
    Ok(())
}

fn normalize_message_id(value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches('<').trim_end_matches('>');
    (!value.is_empty()
        && value.len() <= 512
        && value.is_ascii()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'<' | b'>')))
    .then(|| value.to_owned())
}

fn bounded_reference_chain(mut references: Vec<String>) -> Vec<String> {
    references.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    let mut total = 0usize;
    let mut kept = Vec::new();
    for reference in references.into_iter().rev() {
        let encoded_len = reference.len().saturating_add(3);
        if !kept.is_empty() && total.saturating_add(encoded_len) > 850 {
            break;
        }
        total = total.saturating_add(encoded_len);
        kept.push(reference);
    }
    kept.reverse();
    kept
}

fn reply_plain_body(request: &ComposeRequest) -> Result<String> {
    let Some(context) = request.reply_context.as_ref() else {
        return Ok(request.body_text.clone());
    };
    validate_reply_context(context)?;
    let intro = reply_intro(context);
    let quoted = context
        .quoted_text
        .lines()
        .map(|line| {
            if line.is_empty() {
                ">".to_owned()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let authored = request.body_text.trim_end();
    Ok(if authored.is_empty() {
        format!("{intro}\n{quoted}")
    } else {
        format!("{authored}\n\n{intro}\n{quoted}")
    })
}

fn reply_html_body(request: &ComposeRequest) -> Result<String> {
    let context = request
        .reply_context
        .as_ref()
        .ok_or_else(|| MailError::Mime("HTML reply requested without reply context".to_owned()))?;
    validate_reply_context(context)?;
    let authored = html_text(&request.body_text);
    let intro = html_escape(&reply_intro(context));
    let quoted = html_text(&context.quoted_text);
    Ok(format!(
        "<div class=\"mine-mail-authored\">{authored}</div><br><div class=\"mine-mail-quote\"><div>{intro}</div><blockquote id=\"isReplyContent\" type=\"cite\">{quoted}</blockquote></div>"
    ))
}

fn reply_intro(context: &ReplyContext) -> String {
    let sent_at = context
        .sent_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.format("%Y-%m-%d %H:%M:%S %:z").to_string())
        .unwrap_or_else(|| "unknown time".to_owned());
    let sender = context
        .sender
        .as_ref()
        .map(format_reply_address)
        .unwrap_or_else(|| "unknown sender".to_owned());
    format!("At {sent_at}, {sender} wrote:")
}

fn format_reply_address(address: &MailAddress) -> String {
    let name = address
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.replace(['\r', '\n', '"'], "'"));
    match name {
        Some(name) => format!("\"{name}\" <{}>", address.email),
        None => format!("<{}>", address.email),
    }
}

fn html_text(value: &str) -> String {
    html_escape(value)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "<br>")
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn parse_mailbox(value: &str, label: &str) -> Result<Mailbox> {
    value
        .parse::<Mailbox>()
        .map_err(|error| MailError::Validation(format!("invalid {label}: {error}")))
}

fn insert_custom_headers(raw: &mut Vec<u8>, headers: &[(String, String)]) -> Result<()> {
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

const MAX_INLINE_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOTAL_INLINE_IMAGE_BYTES: usize = 12 * 1024 * 1024;

/// Returns only a real text/html MIME leaf. `mail-parser::body_html` also
/// synthesizes HTML for text/plain-only messages, which is useful for generic
/// callers but would make the desktop reader treat every message as rich mail.
fn extract_renderable_html(message: &mail_parser::Message<'_>) -> Option<String> {
    let mut html = match &message.html_part(0)?.body {
        PartType::Html(html) => html.as_ref().to_owned(),
        _ => return None,
    };
    let mut total_inline_bytes = 0usize;

    for part in &message.parts {
        let Some(content_id) = part.content_id().map(normalize_content_id) else {
            continue;
        };
        let Some(media_type) = safe_inline_image_media_type(part) else {
            continue;
        };
        let contents = part.contents();
        if contents.is_empty()
            || contents.len() > MAX_INLINE_IMAGE_BYTES
            || total_inline_bytes.saturating_add(contents.len()) > MAX_TOTAL_INLINE_IMAGE_BYTES
        {
            continue;
        }

        total_inline_bytes += contents.len();
        let data_url = format!("data:{media_type};base64,{}", BASE64.encode(contents));
        html = replace_ascii_case_insensitive(&html, &format!("cid:{content_id}"), &data_url);
        html = replace_ascii_case_insensitive(&html, &format!("cid:<{content_id}>"), &data_url);
    }

    Some(html)
}

fn normalize_content_id(value: &str) -> &str {
    value.trim().trim_start_matches('<').trim_end_matches('>')
}

fn safe_inline_image_media_type(part: &mail_parser::MessagePart<'_>) -> Option<&'static str> {
    let content_type = part.content_type()?;
    if !content_type.c_type.eq_ignore_ascii_case("image") {
        return None;
    }
    match content_type.c_subtype.as_deref()? {
        subtype if subtype.eq_ignore_ascii_case("png") => Some("image/png"),
        subtype if subtype.eq_ignore_ascii_case("jpeg") || subtype.eq_ignore_ascii_case("jpg") => {
            Some("image/jpeg")
        }
        subtype if subtype.eq_ignore_ascii_case("gif") => Some("image/gif"),
        subtype if subtype.eq_ignore_ascii_case("webp") => Some("image/webp"),
        _ => None,
    }
}

fn replace_ascii_case_insensitive(input: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return input.to_owned();
    }
    let lower_input = input.to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut offset = 0;

    while let Some(relative) = lower_input[offset..].find(&lower_needle) {
        let start = offset + relative;
        output.push_str(&input[offset..start]);
        output.push_str(replacement);
        offset = start + needle.len();
    }
    output.push_str(&input[offset..]);
    output
}

pub(crate) fn render_message_html(message: &InboxMessage) -> Option<String> {
    if message.raw_rfc822.is_empty() {
        return message.body_html.clone();
    }
    match MessageParser::default().parse(&message.raw_rfc822) {
        Some(parsed) => extract_renderable_html(&parsed),
        None => message.body_html.clone(),
    }
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

    let body_html = extract_renderable_html(&message);

    Ok(InboxMessage {
        id: 0,
        account_id: metadata.account_id.to_owned(),
        mailbox: metadata.mailbox.to_owned(),
        uid: metadata.uid,
        message_id: message.message_id().map(str::to_owned),
        in_reply_to: message_ids(message.in_reply_to()),
        references: message_ids(message.references()),
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
        body_html,
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
        in_reply_to: Vec::new(),
        references: Vec::new(),
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

fn message_ids(value: &HeaderValue<'_>) -> Vec<String> {
    value
        .as_text_list()
        .into_iter()
        .flatten()
        .flat_map(|value| value.split_ascii_whitespace())
        .map(|value| {
            value
                .trim_matches(|character| matches!(character, '<' | '>'))
                .to_owned()
        })
        .filter(|value| !value.is_empty())
        .collect()
}

pub(crate) fn reply_message_ids(raw: &[u8]) -> (Vec<String>, Vec<String>) {
    MessageParser::default().parse(raw).map_or_else(
        || (Vec::new(), Vec::new()),
        |message| {
            (
                message_ids(message.in_reply_to()),
                message_ids(message.references()),
            )
        },
    )
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
    use mail_parser::{MessageParser, PartType};

    use super::{
        IncomingMetadata, build_draft_message_revision, build_outgoing_message,
        draft_has_unsupported_content, outbox_body_text, outbox_message_id, outbox_preview,
        outbox_sent_at, outbox_subject, parse_draft_message, parse_incoming_message,
        parse_incoming_summary_or_fallback, render_message_html, restore_outbox_envelope,
    };
    use crate::{ComposeRequest, MailAddress, ReplyContext};

    fn compose() -> ComposeRequest {
        ComposeRequest {
            to: vec!["Receiver <receiver@example.com>".to_owned()],
            cc: vec![],
            bcc: vec!["hidden@example.com".to_owned()],
            subject: "中文主题".to_owned(),
            body_text: "Hello, 世界".to_owned(),
            reply_context: None,
        }
    }

    #[test]
    fn outgoing_message_keeps_bcc_in_envelope_but_not_headers() {
        let outgoing = build_outgoing_message("sender@example.com", &compose()).expect("message");
        let text = String::from_utf8_lossy(&outgoing.raw_rfc822);

        assert_eq!(outgoing.recipients.len(), 2);
        assert_eq!(
            outbox_subject(&outgoing.raw_rfc822).as_deref(),
            Some("中文主题")
        );
        assert_eq!(
            outbox_preview(&outgoing.raw_rfc822).as_deref(),
            Some("Hello, 世界")
        );
        assert_eq!(
            outbox_body_text(&outgoing.raw_rfc822).as_deref(),
            Some("Hello, 世界")
        );
        let message_id = outbox_message_id(&outgoing.raw_rfc822).expect("Message-ID");
        assert!(message_id.ends_with("@mine-mail.invalid"));
        assert!(outbox_sent_at(&outgoing.raw_rfc822).is_some());
        assert!(!text.lines().any(|line| line.starts_with("Bcc:")));
        assert!(!text.contains("hidden@example.com"));
    }

    #[test]
    fn reply_message_uses_standard_headers_and_plain_html_alternatives() {
        let mut request = compose();
        request.subject = "Re: Earlier note".to_owned();
        request.body_text = "这是回复内容".to_owned();
        request.reply_context = Some(ReplyContext {
            parent_message_id: Some("parent@example.com".to_owned()),
            references: vec!["root@example.com".to_owned()],
            subject: "Earlier note".to_owned(),
            sender: Some(MailAddress {
                name: Some("tantless".to_owned()),
                email: "sender@example.com".to_owned(),
            }),
            recipients: vec![MailAddress {
                name: None,
                email: "sender@example.com".to_owned(),
            }],
            sent_at: Some("2026-07-17T09:54:29+08:00".to_owned()),
            quoted_text: "Original body\nSecond line".to_owned(),
        });

        let outgoing = build_outgoing_message("sender@example.com", &request).expect("reply");
        let raw = String::from_utf8_lossy(&outgoing.raw_rfc822);

        assert!(raw.contains("In-Reply-To: <parent@example.com>\r\n"));
        assert!(raw.contains("References: <root@example.com> <parent@example.com>\r\n"));
        assert!(raw.contains("Content-Type: multipart/alternative"));
        assert!(raw.contains("Content-Type: text/plain"));
        assert!(raw.contains("Content-Type: text/html"));

        let parsed = MessageParser::default()
            .parse(&outgoing.raw_rfc822)
            .expect("parse reply");
        let plain = parsed.body_text(0).expect("plain body");
        assert!(plain.starts_with("这是回复内容"));
        assert!(
            plain.contains(
                "At 2026-07-17 09:54:29 +08:00, \"tantless\" <sender@example.com> wrote:"
            )
        );
        assert!(
            plain
                .replace("\r\n", "\n")
                .contains("> Original body\n> Second line")
        );
        let html = match &parsed.html_part(0).expect("HTML body").body {
            PartType::Html(html) => html.as_ref(),
            other => panic!("expected HTML leaf, got {other:?}"),
        };
        assert!(html.contains("blockquote id=\"isReplyContent\" type=\"cite\""));
        assert!(html.contains("这是回复内容"));
        assert!(html.contains("Original body<br>Second line"));
    }

    #[test]
    fn reply_draft_round_trips_as_editable_plain_text_with_structured_context() {
        let mut request = compose();
        request.subject = "Re: Earlier note".to_owned();
        request.body_text = "Drafted reply".to_owned();
        request.reply_context = Some(ReplyContext {
            parent_message_id: Some("parent@example.com".to_owned()),
            references: vec!["root@example.com".to_owned()],
            subject: "Earlier note".to_owned(),
            sender: Some(MailAddress {
                name: Some("Sender".to_owned()),
                email: "receiver@example.com".to_owned(),
            }),
            recipients: vec![MailAddress {
                name: None,
                email: "sender@example.com".to_owned(),
            }],
            sent_at: Some("2026-07-17T09:54:29+08:00".to_owned()),
            quoted_text: "Original body".to_owned(),
        });

        let raw = build_draft_message_revision("sender@example.com", &request, "draft-123", 4)
            .expect("draft MIME");
        let raw_text = String::from_utf8_lossy(&raw);
        assert!(raw_text.contains("Content-Type: text/plain"));
        assert!(!raw_text.contains("multipart/alternative"));

        let parsed = parse_draft_message(&raw).expect("parse own reply draft");
        assert!(!parsed.has_unsupported_content);
        assert_eq!(parsed.request.body_text, "Drafted reply");
        let context = parsed.request.reply_context.expect("reply context");
        assert_eq!(
            context.parent_message_id.as_deref(),
            Some("parent@example.com")
        );
        assert_eq!(context.references, ["root@example.com"]);
        assert_eq!(context.subject, "Earlier note");
        assert_eq!(context.quoted_text, "Original body");
        assert_eq!(
            context.sender.and_then(|sender| sender.name).as_deref(),
            Some("Sender")
        );
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
    fn incoming_multipart_prefers_real_html_and_resolves_safe_cid_images() {
        let raw = b"From: sender@example.com\r\nTo: receiver@example.com\r\nSubject: Rich message\r\nContent-Type: multipart/related; boundary=outer\r\n\r\n--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n\r\n--inner\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nPlain fallback\r\n--inner\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<html><body><strong>Rich body</strong><img src=\"CID:logo@example.com\"></body></html>\r\n--inner--\r\n--outer\r\nContent-Type: image/png\r\nContent-Transfer-Encoding: base64\r\nContent-ID: <logo@example.com>\r\nContent-Disposition: inline\r\n\r\nAQID\r\n--outer--\r\n";
        let parsed = parse_incoming_message(
            raw,
            IncomingMetadata {
                account_id: "primary",
                mailbox: "INBOX",
                uid: 43,
                flags: Vec::new(),
                internal_date: None,
                size_bytes: raw.len() as u32,
                synced_at: "2026-07-15T00:00:00Z".to_owned(),
                body_fetched: true,
            },
        )
        .expect("parse rich message");

        assert_eq!(parsed.body_text.as_deref(), Some("Plain fallback"));
        let html = parsed.body_html.as_deref().expect("real HTML body");
        assert!(html.contains("<strong>Rich body</strong>"));
        assert!(html.contains("data:image/png;base64,AQID"));
        assert!(!html.to_ascii_lowercase().contains("cid:logo@example.com"));
        assert_eq!(render_message_html(&parsed).as_deref(), Some(html));
    }

    #[test]
    fn incoming_plain_text_does_not_claim_to_have_a_real_html_part() {
        let raw = b"From: sender@example.com\r\nSubject: Plain\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nOnly text";
        let parsed = parse_incoming_message(
            raw,
            IncomingMetadata {
                account_id: "primary",
                mailbox: "INBOX",
                uid: 44,
                flags: Vec::new(),
                internal_date: None,
                size_bytes: raw.len() as u32,
                synced_at: "2026-07-15T00:00:00Z".to_owned(),
                body_fetched: true,
            },
        )
        .expect("parse plain message");

        assert_eq!(parsed.body_text.as_deref(), Some("Only text"));
        assert_eq!(parsed.body_html, None);
        assert_eq!(render_message_html(&parsed), None);
    }

    #[test]
    fn incoming_reply_retains_parent_and_thread_message_ids() {
        let raw = b"From: sender@example.com\r\nSubject: Reply\r\nMessage-ID: <reply@example.com>\r\nIn-Reply-To: <parent@example.com>\r\nReferences: <root@example.com> <parent@example.com>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nReply body";
        let parsed = parse_incoming_message(
            raw,
            IncomingMetadata {
                account_id: "primary",
                mailbox: "INBOX",
                uid: 45,
                flags: Vec::new(),
                internal_date: None,
                size_bytes: raw.len() as u32,
                synced_at: "2026-07-16T00:00:00Z".to_owned(),
                body_fetched: true,
            },
        )
        .expect("parse reply");

        assert_eq!(parsed.in_reply_to, ["parent@example.com"]);
        assert_eq!(
            parsed.references,
            ["root@example.com", "parent@example.com"]
        );
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
