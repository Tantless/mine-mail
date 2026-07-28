use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};

use ammonia::{Builder as HtmlSanitizer, UrlRelative};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD},
};
use chrono::DateTime;
use lettre::{
    Address, Message,
    address::Envelope,
    message::{Attachment, Mailbox, MultiPart, SinglePart, header::ContentType},
};
use mail_parser::{Address as ParsedAddress, HeaderValue, MessageParser, MimeHeaders, PartType};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    ComposeFormat, ComposeRequest, ForwardContext, InboxMessage, MailAddress, MailError,
    ReplyContext, Result, StationeryTheme,
};

const MINE_MAIL_REPLY_FORMAT_HEADER: &str = "X-Mine-Mail-Reply-Format";
const MINE_MAIL_REPLY_FORMAT_VERSION: &str = "1";
const MINE_MAIL_COMPOSE_FORMAT_HEADER: &str = "X-Mine-Mail-Compose-Format";
const MINE_MAIL_COMPOSE_FORMAT_VERSION: &str = "1";
const MINE_MAIL_STATIONERY_HEADER: &str = "X-Mine-Mail-Stationery";
const MINE_MAIL_SEND_STATIONERY_HEADER: &str = "X-Mine-Mail-Send-Stationery";
const MINE_MAIL_FORWARD_FORMAT_HEADER: &str = "X-Mine-Mail-Forward-Format";
const MINE_MAIL_FORWARD_FORMAT_VERSION: &str = "1";
const MINE_MAIL_FORWARD_SEPARATOR: &str = "---------- Forwarded message ----------";
const MINE_MAIL_AUTHORED_START: &str = "<!--mine-mail-authored:start-->";
const MINE_MAIL_AUTHORED_END: &str = "<!--mine-mail-authored:end-->";
const MAX_QUOTED_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_QUOTED_HTML_BYTES: usize = 12 * 1024 * 1024;
pub(crate) const MAX_ATTACHMENT_PARTS: usize = 256;
pub(crate) const MAX_MANAGED_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;
pub(crate) const MAX_MANAGED_ATTACHMENT_TOTAL_BYTES: u64 = 50 * 1024 * 1024;
const MAX_MIME_TREE_PARTS: usize = 4_096;
const MAX_MIME_TREE_DEPTH: usize = 64;
const MAX_ORIGINAL_ATTACHMENT_NAME_BYTES: usize = 512;
const MAX_SAFE_ATTACHMENT_NAME_BYTES: usize = 180;
const MAX_FORWARD_ADDRESSES_PER_FIELD: usize = 256;
const MAX_FORWARD_ADDRESS_BYTES: usize = 320;
const MAX_FORWARD_DISPLAY_NAME_BYTES: usize = 512;
const MAX_FORWARD_SUBJECT_BYTES: usize = 4 * 1024;
const MAX_FORWARD_DATE_BYTES: usize = 128;
const FORWARD_IDENTITY_TRUNCATION_MARKER: &str = "…";
const ATTACHMENT_TOKEN_MAGIC: &[u8; 4] = b"MMA1";
const ATTACHMENT_TOKEN_BYTES: usize = 36;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MimeSourceCompleteness {
    CompleteRfc822,
    BoundedSummaryPrefix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttachmentDisposition {
    Attachment,
    Inline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AttachmentPartMetadata {
    pub id: String,
    pub original_name: Option<String>,
    pub safe_display_name: String,
    pub declared_mime_type: Option<String>,
    pub detected_mime_type: Option<String>,
    pub mime_type: String,
    pub size_bytes: u64,
    pub disposition: AttachmentDisposition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttachmentIndexError {
    NonAuthoritativeSource,
    MessageCouldNotBeParsed,
    InvalidMimeStructure,
    MimeTreeTooLarge,
    PartEncodingProblem,
    InvalidPartToken,
    AttachmentNotFound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ForwardHtmlRenderMode {
    NativeSemanticHtml,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreparedForwardSource {
    pub original_subject: String,
    pub from: Option<MailAddress>,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    pub sent_at: Option<String>,
    pub quoted_text: String,
    pub quoted_html: Option<String>,
    pub quoted_render_mode: Option<ForwardHtmlRenderMode>,
    pub ordinary_attachments: Vec<AttachmentPartMetadata>,
    /// True even when malformed attachment transfer encoding prevents a
    /// trustworthy metadata index. This lets an explicit body-only forward
    /// report that source attachments were intentionally omitted.
    pub has_ordinary_attachments: bool,
    pub has_inline_resources: bool,
    pub html_downgraded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ForwardSourceError {
    NonAuthoritativeSource,
    MessageCouldNotBeParsed,
    BodyTooLarge,
    HeaderMetadataTooLarge,
    AttachmentIndex(AttachmentIndexError),
}

pub(crate) struct OutgoingMessage {
    pub raw_rfc822: Vec<u8>,
    pub envelope: Envelope,
    pub recipients: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedMimeAttachment {
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub bytes: Vec<u8>,
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
    build_outgoing_message_with_attachments(from, request, None, Vec::new())
}

pub(crate) fn build_outgoing_message_with_attachments(
    from: &str,
    request: &ComposeRequest,
    forward_context: Option<&ForwardContext>,
    attachments: Vec<ManagedMimeAttachment>,
) -> Result<OutgoingMessage> {
    request.validate()?;
    validate_forward_compose(request, forward_context)?;
    // Use a stable, app-generated identifier for the exact bytes persisted in
    // Outbox. The same identifier will later arrive through the provider's
    // Sent mailbox and lets the desktop merge both views without guessing.
    // The reserved `.invalid` TLD avoids disclosing the local host name.
    let message_id = format!("<{}@mine-mail.invalid>", Uuid::now_v7());
    let mut headers = vec![("Message-ID".to_owned(), message_id)];
    headers.extend(reply_headers(request)?);
    headers.extend(compose_headers(request));
    let raw_rfc822 = build_rfc822(
        from,
        request,
        &headers,
        false,
        true,
        forward_context,
        attachments,
    )?;
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
    build_draft_message_revision_with_attachments(
        from,
        request,
        draft_id,
        revision,
        None,
        Vec::new(),
    )
}

pub(crate) fn build_draft_message_revision_with_attachments(
    from: &str,
    request: &ComposeRequest,
    draft_id: &str,
    revision: u64,
    forward_context: Option<&ForwardContext>,
    attachments: Vec<ManagedMimeAttachment>,
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
    headers.extend(compose_headers(&request_with_destination));
    if forward_context.is_some() {
        headers.push((
            MINE_MAIL_FORWARD_FORMAT_HEADER.to_owned(),
            MINE_MAIL_FORWARD_FORMAT_VERSION.to_owned(),
        ));
    }
    validate_forward_compose(&request_with_destination, forward_context)?;
    let mut raw = build_rfc822(
        from,
        &request_with_destination,
        &headers,
        true,
        false,
        forward_context,
        attachments,
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
    let encoded_body = message
        .body_text(0)
        .map(|body| body.into_owned())
        .unwrap_or_default();
    let mine_mail_reply = text_header(&message, MINE_MAIL_REPLY_FORMAT_HEADER)
        .is_some_and(|value| value == MINE_MAIL_REPLY_FORMAT_VERSION);
    let mine_mail_compose = text_header(&message, MINE_MAIL_COMPOSE_FORMAT_HEADER)
        .is_some_and(|value| value == MINE_MAIL_COMPOSE_FORMAT_VERSION);
    let mine_mail_forward = text_header(&message, MINE_MAIL_FORWARD_FORMAT_HEADER)
        .is_some_and(|value| value == MINE_MAIL_FORWARD_FORMAT_VERSION);
    let parsed_reply = mine_mail_reply
        .then(|| parse_mine_mail_reply_draft(&message, &encoded_body))
        .transpose()?;
    let (body_text, reply_context) = match parsed_reply {
        Some((body_text, reply_context)) => (body_text, Some(reply_context)),
        None if mine_mail_forward => (
            authored_forward_text(&encoded_body).unwrap_or_else(|| encoded_body.clone()),
            None,
        ),
        None => (encoded_body, None),
    };
    let stationery = mine_mail_compose
        .then(|| {
            text_header(&message, MINE_MAIL_STATIONERY_HEADER)
                .map(|value| StationeryTheme::from_str(&value))
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let body_html = mine_mail_compose
        .then(|| extract_renderable_html(&message))
        .flatten()
        .and_then(|html| extract_mine_mail_authored_html(&html))
        .and_then(|html| sanitize_compose_html(Some(&html)));
    let send_stationery = stationery != StationeryTheme::None
        && text_header(&message, MINE_MAIL_SEND_STATIONERY_HEADER)
            .is_some_and(|value| value == "1");

    Ok(ParsedDraftMessage {
        draft_id,
        revision,
        has_unsupported_content: if mine_mail_forward {
            // A locally persisted forward remains fully editable because its
            // immutable context lives in SQLite. A remote-only copy cannot
            // reconstruct that structured context and is therefore imported
            // read-only rather than flattening quoted content into authored
            // text.
            true
        } else if mine_mail_compose {
            mine_mail_message_has_unsupported_draft_content(&message)
        } else {
            message_has_unsupported_draft_content(&message)
        },
        request: ComposeRequest {
            to: map_compose_addresses(message.to()),
            cc: map_compose_addresses(message.cc()),
            bcc: map_compose_addresses(message.bcc()),
            subject: message.subject().unwrap_or_default().to_owned(),
            body_text,
            format: ComposeFormat {
                body_html,
                stationery,
                send_stationery,
            },
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
            quoted_html: None,
        },
    ))
}

/// Returns true unless the raw draft is one parseable, undecorated text/plain
/// body. Mine Mail-owned restricted rich drafts use the separate checked path;
/// arbitrary HTML, multipart structure, inline resources, attachments, or
/// unknown MIME parts remain unsafe to round-trip.
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

fn mine_mail_message_has_unsupported_draft_content(message: &mail_parser::Message<'_>) -> bool {
    if message.attachment_count() != 0 || message.parts.iter().any(|part| part.is_encoding_problem)
    {
        return true;
    }
    match extract_renderable_html(message) {
        Some(html) => extract_mine_mail_authored_html(&html).is_none(),
        None => {
            message.parts.len() != 1
                || message.parts.first().is_none_or(|part| {
                    part.content_disposition().is_some() || !matches!(part.body, PartType::Text(_))
                })
        }
    }
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
            let body = message.body_text(0).map(|body| body.into_owned());
            let is_mine_mail_reply = text_header(&message, MINE_MAIL_REPLY_FORMAT_HEADER)
                .is_some_and(|value| value == MINE_MAIL_REPLY_FORMAT_VERSION);
            if is_mine_mail_reply
                && let Some(body) = body.as_deref()
                && let Ok((authored, _)) = parse_mine_mail_reply_draft(&message, body)
            {
                return Some(compact_text_preview(&authored, 180));
            }
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

pub(crate) fn outbox_body_html(raw_rfc822: &[u8]) -> Option<String> {
    MessageParser::default()
        .parse(raw_rfc822)
        .and_then(|message| extract_renderable_html(&message))
}

pub(crate) fn outbox_has_reply_headers(raw_rfc822: &[u8]) -> bool {
    MessageParser::default()
        .parse(raw_rfc822)
        .is_some_and(|message| {
            !message_ids(message.in_reply_to()).is_empty()
                || !message_ids(message.references()).is_empty()
                || text_header(&message, MINE_MAIL_REPLY_FORMAT_HEADER)
                    .is_some_and(|value| value == MINE_MAIL_REPLY_FORMAT_VERSION)
        })
}

fn compact_text_preview(value: &str, max_chars: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
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
    include_quoted_html: bool,
    forward_context: Option<&ForwardContext>,
    attachments: Vec<ManagedMimeAttachment>,
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

    validate_managed_mime_attachments(&attachments)?;
    let plain_body = match forward_context {
        Some(context) => forward_plain_body(request, context)?,
        None => reply_plain_body(request)?,
    };
    let has_authored_html = request
        .format
        .body_html
        .as_deref()
        .is_some_and(|html| !html.trim().is_empty());
    let should_build_html = has_authored_html
        || (request.format.send_stationery && request.format.stationery != StationeryTheme::None)
        || (include_quoted_html && request.reply_context.is_some())
        || forward_context
            .and_then(|context| context.quoted_html.as_ref())
            .is_some();
    let rich_body = if should_build_html {
        let (html_body, inline_images) = match forward_context {
            Some(context) => (forward_html_body(request, context)?, Vec::new()),
            None => rich_html_body(request, include_quoted_html)?,
        };
        let plain_part = SinglePart::builder()
            .header(ContentType::TEXT_PLAIN)
            .body(plain_body.clone());
        let html_part = SinglePart::builder()
            .header(ContentType::TEXT_HTML)
            .body(html_body);
        let alternative = if inline_images.is_empty() {
            MultiPart::alternative()
                .singlepart(plain_part)
                .singlepart(html_part)
        } else {
            let mut related = MultiPart::related().singlepart(html_part);
            for image in inline_images {
                let content_type = ContentType::parse(image.media_type).map_err(|error| {
                    MailError::Mime(format!("cannot encode quoted inline image: {error}"))
                })?;
                related = related.singlepart(
                    Attachment::new_inline(image.content_id).body(image.bytes, content_type),
                );
            }
            MultiPart::alternative()
                .singlepart(plain_part)
                .multipart(related)
        };
        Some(alternative)
    } else {
        None
    };

    let message = if attachments.is_empty() {
        if let Some(rich_body) = rich_body {
            builder
                .multipart(rich_body)
                .map_err(|error| MailError::Mime(format!("cannot build message: {error}")))?
        } else {
            builder
                .header(ContentType::TEXT_PLAIN)
                .body(plain_body)
                .map_err(|error| MailError::Mime(format!("cannot build message: {error}")))?
        }
    } else {
        let mut mixed = if let Some(rich_body) = rich_body {
            MultiPart::mixed().multipart(rich_body)
        } else {
            MultiPart::mixed().singlepart(
                SinglePart::builder()
                    .header(ContentType::TEXT_PLAIN)
                    .body(plain_body),
            )
        };
        for attachment in attachments {
            let content_type = ContentType::parse(&attachment.mime_type).map_err(|error| {
                MailError::Mime(format!("cannot encode managed attachment type: {error}"))
            })?;
            mixed = mixed.singlepart(
                Attachment::new(attachment.name.clone()).body(attachment.bytes, content_type),
            );
        }
        builder
            .multipart(mixed)
            .map_err(|error| MailError::Mime(format!("cannot build message: {error}")))?
    };

    let mut raw = message.formatted();
    insert_custom_headers(&mut raw, custom_headers)?;
    Ok(raw)
}

fn compose_headers(request: &ComposeRequest) -> Vec<(String, String)> {
    vec![
        (
            MINE_MAIL_COMPOSE_FORMAT_HEADER.to_owned(),
            MINE_MAIL_COMPOSE_FORMAT_VERSION.to_owned(),
        ),
        (
            MINE_MAIL_STATIONERY_HEADER.to_owned(),
            request.format.stationery.as_str().to_owned(),
        ),
        (
            MINE_MAIL_SEND_STATIONERY_HEADER.to_owned(),
            if request.format.send_stationery && request.format.stationery != StationeryTheme::None
            {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
    ]
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
    if context
        .quoted_html
        .as_ref()
        .is_some_and(|html| html.len() > MAX_QUOTED_HTML_BYTES)
    {
        return Err(MailError::Validation(
            "the quoted HTML message is too large to include in a reply".to_owned(),
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

fn validate_forward_compose(
    request: &ComposeRequest,
    forward_context: Option<&ForwardContext>,
) -> Result<()> {
    let Some(context) = forward_context else {
        return Ok(());
    };
    if request.reply_context.is_some() {
        return Err(MailError::Validation(
            "a compose draft cannot be both a reply and a forward".to_owned(),
        ));
    }
    if context.source_message_id.trim().is_empty() {
        return Err(MailError::Validation(
            "a forward must retain its opaque source identity".to_owned(),
        ));
    }
    if context.quoted_text.len() > MAX_QUOTED_TEXT_BYTES
        || context
            .quoted_html
            .as_ref()
            .is_some_and(|html| html.len() > MAX_QUOTED_HTML_BYTES)
    {
        return Err(MailError::Validation(
            "the forwarded message body is too large".to_owned(),
        ));
    }
    if context.to.len() > MAX_FORWARD_ADDRESSES_PER_FIELD
        || context.cc.len() > MAX_FORWARD_ADDRESSES_PER_FIELD
    {
        return Err(MailError::Validation(
            "the forwarded message has too much address metadata".to_owned(),
        ));
    }
    for address in context.from.iter().chain(&context.to).chain(&context.cc) {
        sanitized_forward_email(&address.email).ok_or_else(|| {
            MailError::Validation("the forwarded message has invalid address metadata".to_owned())
        })?;
    }
    Ok(())
}

fn validate_managed_mime_attachments(attachments: &[ManagedMimeAttachment]) -> Result<()> {
    if attachments.len() > MAX_ATTACHMENT_PARTS {
        return Err(MailError::Validation(
            "too many managed attachments are selected".to_owned(),
        ));
    }
    let mut total_bytes = 0u64;
    for attachment in attachments {
        if attachment.name != safe_attachment_filename(Some(&attachment.name))
            || attachment.mime_type.is_empty()
            || attachment.mime_type.len() > 255
            || attachment.mime_type.chars().any(char::is_control)
        {
            return Err(MailError::Validation(
                "managed attachment metadata is invalid".to_owned(),
            ));
        }
        let actual_size = u64::try_from(attachment.bytes.len()).map_err(|_| {
            MailError::Validation("a managed attachment is too large to encode".to_owned())
        })?;
        if attachment.size_bytes != actual_size {
            return Err(MailError::Validation(
                "a managed attachment does not match its bounded byte metadata".to_owned(),
            ));
        }
        total_bytes = add_managed_attachment_size(total_bytes, actual_size)?;
        ContentType::parse(&attachment.mime_type).map_err(|_| {
            MailError::Validation("managed attachment MIME type is invalid".to_owned())
        })?;
    }
    Ok(())
}

fn add_managed_attachment_size(total_bytes: u64, next_size: u64) -> Result<u64> {
    if next_size > MAX_MANAGED_ATTACHMENT_BYTES {
        return Err(MailError::Validation(
            "a managed attachment exceeds the configured byte limit".to_owned(),
        ));
    }
    let total_bytes = total_bytes.checked_add(next_size).ok_or_else(|| {
        MailError::Validation("managed attachment byte total overflowed".to_owned())
    })?;
    if total_bytes > MAX_MANAGED_ATTACHMENT_TOTAL_BYTES {
        return Err(MailError::Validation(
            "the managed attachment set is too large to encode".to_owned(),
        ));
    }
    Ok(total_bytes)
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

fn forward_plain_body(request: &ComposeRequest, context: &ForwardContext) -> Result<String> {
    validate_forward_compose(request, Some(context))?;
    let mut identity = vec![MINE_MAIL_FORWARD_SEPARATOR.to_owned()];
    identity.extend(
        forward_identity_lines(context)
            .into_iter()
            .map(|line| format!("{}: {}", line.label, line.value)),
    );
    let forwarded = format!("{}\n\n{}", identity.join("\n"), context.quoted_text);
    let authored = request.body_text.trim_end();
    Ok(if authored.is_empty() {
        forwarded
    } else {
        format!("{authored}\n\n{forwarded}")
    })
}

fn authored_forward_text(body: &str) -> Option<String> {
    let separator = body
        .lines()
        .position(|line| line.trim_end_matches('\r') == MINE_MAIL_FORWARD_SEPARATOR)?;
    Some(
        body.lines()
            .take(separator)
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_owned(),
    )
}

struct ForwardIdentityLine {
    label: &'static str,
    value: String,
}

fn forward_identity_lines(context: &ForwardContext) -> Vec<ForwardIdentityLine> {
    let mut identity = vec![ForwardIdentityLine {
        label: "Subject",
        value: sanitize_forward_identity_text(&context.original_subject, MAX_FORWARD_SUBJECT_BYTES),
    }];
    if let Some(from) = context.from.as_ref() {
        identity.push(ForwardIdentityLine {
            label: "From",
            value: forward_mail_address(from),
        });
    }
    if let Some(sent_at) = context.sent_at.as_deref() {
        identity.push(ForwardIdentityLine {
            label: "Date",
            value: sanitize_forward_identity_text(sent_at, MAX_FORWARD_DATE_BYTES),
        });
    }
    if !context.to.is_empty() {
        identity.push(ForwardIdentityLine {
            label: "To",
            value: context
                .to
                .iter()
                .map(forward_mail_address)
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    if !context.cc.is_empty() {
        identity.push(ForwardIdentityLine {
            label: "Cc",
            value: context
                .cc
                .iter()
                .map(forward_mail_address)
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    identity
}

fn forward_mail_address(address: &MailAddress) -> String {
    let email = sanitize_forward_identity_text(&address.email, MAX_FORWARD_ADDRESS_BYTES);
    let name = address
        .name
        .as_deref()
        .map(|name| sanitize_forward_identity_text(name, MAX_FORWARD_DISPLAY_NAME_BYTES))
        .filter(|name| !name.is_empty())
        .map(|name| name.replace('"', "'"));
    name.map_or_else(
        || format!("<{email}>"),
        |name| format!("\"{name}\" <{email}>"),
    )
}

struct InlineReplyImage {
    content_id: String,
    media_type: &'static str,
    bytes: Vec<u8>,
}

fn rich_html_body(
    request: &ComposeRequest,
    include_quoted_html: bool,
) -> Result<(String, Vec<InlineReplyImage>)> {
    let authored = authored_html(request);
    let Some(context) = request.reply_context.as_ref() else {
        return Ok(extract_reply_inline_images(format!(
            "<div class=\"mine-mail-authored\">{authored}</div>"
        )));
    };
    validate_reply_context(context)?;
    let intro = html_escape(&reply_intro(context));
    let quoted = if include_quoted_html {
        context
            .quoted_html
            .as_deref()
            .filter(|html| !html.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| html_text(&context.quoted_text))
    } else {
        html_text(&context.quoted_text)
    };
    let html = format!(
        "<div class=\"mine-mail-authored\">{authored}</div><br><div class=\"mine-mail-quote\"><div>{intro}</div><blockquote id=\"isReplyContent\" type=\"cite\">{quoted}</blockquote></div>"
    );
    Ok(extract_reply_inline_images(html))
}

fn forward_html_body(request: &ComposeRequest, context: &ForwardContext) -> Result<String> {
    validate_forward_compose(request, Some(context))?;
    let identity = forward_identity_lines(context)
        .into_iter()
        .map(|line| {
            format!(
                "<div><strong>{}:</strong> {}</div>",
                line.label,
                html_escape(&line.value)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let quoted = context
        .quoted_html
        .as_deref()
        .filter(|html| !html.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| html_text(&context.quoted_text));
    Ok(format!(
        "<div class=\"mine-mail-authored\">{}</div><br><div class=\"mine-mail-forward\"><div>---------- Forwarded message ----------</div>{identity}<br><blockquote type=\"cite\">{quoted}</blockquote></div>",
        authored_html(request)
    ))
}

fn authored_html(request: &ComposeRequest) -> String {
    let fragment = sanitize_compose_html(request.format.body_html.as_deref())
        .unwrap_or_else(|| html_text(&request.body_text));
    let marked = format!("{MINE_MAIL_AUTHORED_START}{fragment}{MINE_MAIL_AUTHORED_END}");
    if !request.format.send_stationery {
        return marked;
    }
    match request.format.stationery {
        StationeryTheme::None => marked,
        StationeryTheme::Lined => format!(
            "<div data-mine-mail-stationery=\"lined\" style=\"box-sizing:border-box;min-height:168px;padding:24px 28px;border:1px solid #dbe4ec;border-radius:10px;background-color:#fbfcfe;background-image:repeating-linear-gradient(to bottom,transparent 0,transparent 27px,#dbe5ee 28px);color:#243342;font-family:Arial,sans-serif;font-size:14px;line-height:28px;\">{marked}</div>"
        ),
        StationeryTheme::Grid => format!(
            "<div data-mine-mail-stationery=\"grid\" style=\"box-sizing:border-box;min-height:196px;padding:22px 28px;border:1px solid #dbe4ec;border-radius:10px;background-color:#fbfcfe;background-image:linear-gradient(#e4ebf2 1px,transparent 1px),linear-gradient(90deg,#e4ebf2 1px,transparent 1px);background-size:28px 28px;color:#243342;font-family:Arial,sans-serif;font-size:14px;line-height:28px;\">{marked}</div>"
        ),
    }
}

fn extract_mine_mail_authored_html(source: &str) -> Option<String> {
    let start = source.find(MINE_MAIL_AUTHORED_START)? + MINE_MAIL_AUTHORED_START.len();
    let relative_end = source[start..].find(MINE_MAIL_AUTHORED_END)?;
    Some(source[start..start + relative_end].to_owned())
}

pub(crate) fn sanitize_compose_html(source: Option<&str>) -> Option<String> {
    let source = source?.trim();
    if source.is_empty() {
        return None;
    }
    let mut builder = HtmlSanitizer::default();
    builder
        .tags(HashSet::from([
            "a",
            "b",
            "blockquote",
            "br",
            "div",
            "em",
            "font",
            "i",
            "li",
            "ol",
            "p",
            "span",
            "strong",
            "u",
            "ul",
        ]))
        .tag_attributes(HashMap::from([
            ("a", HashSet::from(["href"])),
            ("font", HashSet::from(["face", "size"])),
            ("div", HashSet::from(["align"])),
            ("p", HashSet::from(["align"])),
            ("ol", HashSet::from(["start"])),
            ("li", HashSet::from(["value"])),
        ]))
        .clean_content_tags(HashSet::from(["script", "style"]))
        .url_schemes(HashSet::from(["http", "https", "mailto"]))
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true)
        .attribute_filter(|element, attribute, value| {
            if attribute == "align"
                && !matches!(
                    value.to_ascii_lowercase().as_str(),
                    "left" | "center" | "right" | "justify"
                )
            {
                return None;
            }
            if (element, attribute) == ("font", "face")
                && !matches!(
                    value.to_ascii_lowercase().as_str(),
                    "arial"
                        | "microsoft yahei"
                        | "simsun"
                        | "kaiti"
                        | "consolas"
                        | "sans-serif"
                        | "serif"
                        | "monospace"
                )
            {
                return None;
            }
            if (element, attribute) == ("font", "size")
                && !matches!(value, "1" | "2" | "3" | "4" | "5" | "6" | "7")
            {
                return None;
            }
            Some(Cow::Borrowed(value))
        });
    let cleaned = builder.clean(source).to_string();
    html_fragment_has_visible_text(&cleaned).then_some(cleaned)
}

fn html_fragment_has_visible_text(source: &str) -> bool {
    let mut text = String::new();
    let mut in_tag = false;
    for character in source.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    !text
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .trim()
        .is_empty()
}

fn extract_reply_inline_images(mut html: String) -> (String, Vec<InlineReplyImage>) {
    let mut images = Vec::new();
    let mut search_from = 0usize;
    let mut total_bytes = 0usize;

    while images.len() < 32 && search_from < html.len() {
        let lower = html[search_from..].to_ascii_lowercase();
        let Some(relative_start) = lower.find("data:image/") else {
            break;
        };
        let start = search_from + relative_start;
        let candidate = &html[start..];
        let lower_candidate = candidate.to_ascii_lowercase();
        let Some(header_end) = lower_candidate.find(";base64,") else {
            search_from = start.saturating_add("data:image/".len());
            continue;
        };
        let media_type = match &lower_candidate[..header_end] {
            "data:image/png" => "image/png",
            "data:image/jpeg" => "image/jpeg",
            "data:image/gif" => "image/gif",
            "data:image/webp" => "image/webp",
            _ => {
                search_from = start.saturating_add("data:image/".len());
                continue;
            }
        };
        let payload_start = start + header_end + ";base64,".len();
        let payload_len = html[payload_start..]
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
            .count();
        if payload_len == 0 {
            search_from = payload_start;
            continue;
        }
        let payload_end = payload_start + payload_len;
        let Ok(bytes) = BASE64.decode(&html[payload_start..payload_end]) else {
            search_from = payload_end;
            continue;
        };
        if bytes.is_empty()
            || bytes.len() > MAX_INLINE_IMAGE_BYTES
            || total_bytes.saturating_add(bytes.len()) > MAX_TOTAL_INLINE_IMAGE_BYTES
        {
            search_from = payload_end;
            continue;
        }

        total_bytes += bytes.len();
        let content_id = format!("mine-mail-quote-{}@mine-mail.invalid", images.len() + 1);
        let replacement = format!("cid:{content_id}");
        html.replace_range(start..payload_end, &replacement);
        search_from = start + replacement.len();
        images.push(InlineReplyImage {
            content_id,
            media_type,
            bytes,
        });
    }

    (html, images)
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

struct IndexedMimePart<'message, 'raw> {
    part: &'message mail_parser::MessagePart<'raw>,
    path: Vec<u32>,
    disposition: AttachmentDisposition,
}

pub(crate) fn index_message_attachments(
    raw: &[u8],
    completeness: MimeSourceCompleteness,
) -> std::result::Result<Vec<AttachmentPartMetadata>, AttachmentIndexError> {
    if completeness != MimeSourceCompleteness::CompleteRfc822 {
        return Err(AttachmentIndexError::NonAuthoritativeSource);
    }
    let message = MessageParser::default()
        .parse(raw)
        .ok_or(AttachmentIndexError::MessageCouldNotBeParsed)?;
    index_parsed_message_attachments(raw, &message)
}

fn index_parsed_message_attachments(
    raw: &[u8],
    message: &mail_parser::Message<'_>,
) -> std::result::Result<Vec<AttachmentPartMetadata>, AttachmentIndexError> {
    let parts = collect_indexed_mime_parts(message)?;
    let message_digest = stable_digest(b"mine-mail-message", raw);
    parts
        .into_iter()
        .map(|indexed| {
            let original_name = indexed
                .part
                .attachment_name()
                .and_then(bounded_original_attachment_name);
            let safe_display_name = safe_attachment_filename(original_name.as_deref());
            let declared_mime_type = declared_mime_type(indexed.part);
            let detected_mime_type = detect_mime_type(indexed.part.contents()).map(str::to_owned);
            let mime_type = declared_mime_type
                .as_deref()
                .filter(|value| !value.eq_ignore_ascii_case("application/octet-stream"))
                .or(detected_mime_type.as_deref())
                .unwrap_or("application/octet-stream")
                .to_owned();
            Ok(AttachmentPartMetadata {
                id: encode_attachment_token(
                    message_digest,
                    stable_part_digest(&indexed.path, indexed.part),
                ),
                original_name,
                safe_display_name,
                declared_mime_type,
                detected_mime_type,
                mime_type,
                size_bytes: indexed.part.contents().len() as u64,
                disposition: indexed.disposition,
            })
        })
        .collect()
}

pub(crate) fn extract_attachment(
    raw: &[u8],
    token: &str,
) -> std::result::Result<Vec<u8>, AttachmentIndexError> {
    let (expected_message_digest, expected_part_digest) = decode_attachment_token(token)?;
    if stable_digest(b"mine-mail-message", raw) != expected_message_digest {
        return Err(AttachmentIndexError::AttachmentNotFound);
    }
    let message = MessageParser::default()
        .parse(raw)
        .ok_or(AttachmentIndexError::MessageCouldNotBeParsed)?;
    let parts = collect_indexed_mime_parts(&message)?;
    let mut match_ = None;
    for indexed in parts {
        if indexed.disposition != AttachmentDisposition::Attachment
            || stable_part_digest(&indexed.path, indexed.part) != expected_part_digest
        {
            continue;
        }
        if match_.is_some() {
            return Err(AttachmentIndexError::InvalidPartToken);
        }
        match_ = Some(indexed.part.contents().to_vec());
    }
    match_.ok_or(AttachmentIndexError::AttachmentNotFound)
}

pub(crate) fn validate_attachment_id(token: &str) -> bool {
    decode_attachment_token(token).is_ok()
}

fn collect_indexed_mime_parts<'message, 'raw>(
    message: &'message mail_parser::Message<'raw>,
) -> std::result::Result<Vec<IndexedMimePart<'message, 'raw>>, AttachmentIndexError> {
    if message.parts.is_empty() {
        return Err(AttachmentIndexError::InvalidMimeStructure);
    }
    let mut indexed = Vec::new();
    let mut visited = 0usize;
    collect_message_part(message, 0, &mut vec![0], 0, &mut visited, &mut indexed)?;
    Ok(indexed)
}

fn collect_message_part<'message, 'raw>(
    message: &'message mail_parser::Message<'raw>,
    part_id: u32,
    path: &mut Vec<u32>,
    depth: usize,
    visited: &mut usize,
    indexed: &mut Vec<IndexedMimePart<'message, 'raw>>,
) -> std::result::Result<(), AttachmentIndexError> {
    if depth > MAX_MIME_TREE_DEPTH {
        return Err(AttachmentIndexError::MimeTreeTooLarge);
    }
    *visited = visited.saturating_add(1);
    if *visited > MAX_MIME_TREE_PARTS {
        return Err(AttachmentIndexError::MimeTreeTooLarge);
    }
    let part = message
        .part(part_id)
        .ok_or(AttachmentIndexError::InvalidMimeStructure)?;
    if part.is_encoding_problem {
        return Err(AttachmentIndexError::PartEncodingProblem);
    }

    match &part.body {
        PartType::Multipart(children) => {
            for child in children {
                path.push(*child);
                collect_message_part(message, *child, path, depth + 1, visited, indexed)?;
                path.pop();
            }
        }
        PartType::Message(nested) if classify_attachment_part(message, part_id, part).is_none() => {
            if nested.parts.is_empty() {
                return Err(AttachmentIndexError::InvalidMimeStructure);
            }
            path.push(u32::MAX);
            path.push(0);
            collect_message_part(nested, 0, path, depth + 1, visited, indexed)?;
            path.pop();
            path.pop();
        }
        _ => {
            if let Some(disposition) = classify_attachment_part(message, part_id, part) {
                if indexed.len() >= MAX_ATTACHMENT_PARTS {
                    return Err(AttachmentIndexError::MimeTreeTooLarge);
                }
                indexed.push(IndexedMimePart {
                    part,
                    path: path.clone(),
                    disposition,
                });
            }
        }
    }
    Ok(())
}

fn classify_attachment_part(
    message: &mail_parser::Message<'_>,
    part_id: u32,
    part: &mail_parser::MessagePart<'_>,
) -> Option<AttachmentDisposition> {
    let disposition = part
        .content_disposition()
        .map(|value| value.c_type.as_ref());
    if disposition.is_some_and(|value| value.eq_ignore_ascii_case("attachment")) {
        return Some(AttachmentDisposition::Attachment);
    }
    if disposition.is_some_and(|value| value.eq_ignore_ascii_case("inline")) {
        return Some(AttachmentDisposition::Inline);
    }

    let has_name = part
        .attachment_name()
        .is_some_and(|name| !name.trim().is_empty());
    let has_inline_identity = part.content_id().is_some() || part.content_location().is_some();
    if has_inline_identity || matches!(part.body, PartType::InlineBinary(_)) {
        return Some(AttachmentDisposition::Inline);
    }
    if has_name {
        return Some(AttachmentDisposition::Attachment);
    }

    let is_body = message.text_body.contains(&part_id) || message.html_body.contains(&part_id);
    if !is_body
        && (message.attachments.contains(&part_id)
            || matches!(part.body, PartType::Binary(_) | PartType::Message(_)))
    {
        return Some(AttachmentDisposition::Attachment);
    }
    None
}

fn is_attachment_text_control(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{feff}'
        )
}

fn bounded_original_attachment_name(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let mut bounded = String::new();
    for character in value.chars() {
        let replacement = if is_attachment_text_control(character) {
            '\u{fffd}'
        } else {
            character
        };
        if bounded.len() + replacement.len_utf8() > MAX_ORIGINAL_ATTACHMENT_NAME_BYTES {
            break;
        }
        bounded.push(replacement);
    }
    (!bounded.is_empty()).then_some(bounded)
}

pub(crate) fn safe_attachment_filename(original_name: Option<&str>) -> String {
    let mut safe = String::new();
    for character in original_name.unwrap_or_default().chars() {
        let character = if is_attachment_text_control(character)
            || matches!(
                character,
                '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'
            ) {
            '_'
        } else {
            character
        };
        safe.push(character);
    }
    let trimmed = safe
        .trim()
        .trim_matches(|character| matches!(character, '.' | ' '))
        .to_owned();
    if trimmed.is_empty() {
        return "attachment.bin".to_owned();
    }

    let mut safe = truncate_attachment_name(&trimmed, MAX_SAFE_ATTACHMENT_NAME_BYTES);
    if is_windows_reserved_filename(&safe) {
        safe.insert(0, '_');
        safe = truncate_attachment_name(&safe, MAX_SAFE_ATTACHMENT_NAME_BYTES);
    }
    if safe.is_empty() || matches!(safe.as_str(), "." | "..") {
        "attachment.bin".to_owned()
    } else {
        safe
    }
}

pub(crate) fn attachment_name_candidate(safe_name: &str, collision_index: u32) -> String {
    let safe_name = safe_attachment_filename(Some(safe_name));
    if collision_index == 0 {
        return safe_name;
    }
    let (stem, extension) = split_filename_extension(&safe_name);
    let suffix = format!(" ({collision_index})");
    let available_stem_bytes = MAX_SAFE_ATTACHMENT_NAME_BYTES
        .saturating_sub(suffix.len())
        .saturating_sub(extension.len());
    let stem = truncate_utf8(stem, available_stem_bytes);
    let stem = if stem.is_empty() { "attachment" } else { stem };
    format!("{stem}{suffix}{extension}")
}

fn truncate_attachment_name(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let (stem, extension) = split_filename_extension(value);
    if extension.len() >= max_bytes / 2 {
        return truncate_utf8(value, max_bytes).to_owned();
    }
    let stem = truncate_utf8(stem, max_bytes.saturating_sub(extension.len()));
    format!("{stem}{extension}")
}

fn split_filename_extension(value: &str) -> (&str, &str) {
    value
        .rfind('.')
        .filter(|index| *index > 0 && *index + 1 < value.len())
        .map_or((value, ""), |index| (&value[..index], &value[index..]))
}

fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &value[..boundary]
}

fn is_windows_reserved_filename(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|number| number.len() == 1 && matches!(number.as_bytes()[0], b'1'..=b'9'))
}

fn declared_mime_type(part: &mail_parser::MessagePart<'_>) -> Option<String> {
    let content_type = part.content_type()?;
    let subtype = content_type.c_subtype.as_deref()?;
    let value = format!(
        "{}/{}",
        content_type.c_type.to_ascii_lowercase(),
        subtype.to_ascii_lowercase()
    );
    (value.len() <= 127 && is_safe_mime_type(&value)).then_some(value)
}

fn is_safe_mime_type(value: &str) -> bool {
    let Some((type_, subtype)) = value.split_once('/') else {
        return false;
    };
    !type_.is_empty()
        && !subtype.is_empty()
        && type_.bytes().chain(subtype.bytes()).all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                )
        })
}

fn detect_mime_type(contents: &[u8]) -> Option<&'static str> {
    if contents.starts_with(b"%PDF-") {
        Some("application/pdf")
    } else if contents.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if contents.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if contents.starts_with(b"GIF87a") || contents.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if contents.len() >= 12
        && contents.starts_with(b"RIFF")
        && contents.get(8..12) == Some(b"WEBP")
    {
        Some("image/webp")
    } else if contents.starts_with(b"PK\x03\x04")
        || contents.starts_with(b"PK\x05\x06")
        || contents.starts_with(b"PK\x07\x08")
    {
        Some("application/zip")
    } else if contents.starts_with(b"\x1f\x8b") {
        Some("application/gzip")
    } else {
        None
    }
}

fn encode_attachment_token(message_digest: [u8; 16], part_digest: [u8; 16]) -> String {
    let mut token = [0u8; ATTACHMENT_TOKEN_BYTES];
    token[..4].copy_from_slice(ATTACHMENT_TOKEN_MAGIC);
    token[4..20].copy_from_slice(&message_digest);
    token[20..].copy_from_slice(&part_digest);
    URL_SAFE_NO_PAD.encode(token)
}

fn decode_attachment_token(
    token: &str,
) -> std::result::Result<([u8; 16], [u8; 16]), AttachmentIndexError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| AttachmentIndexError::InvalidPartToken)?;
    if decoded.len() != ATTACHMENT_TOKEN_BYTES || &decoded[..4] != ATTACHMENT_TOKEN_MAGIC {
        return Err(AttachmentIndexError::InvalidPartToken);
    }
    let mut message_digest = [0u8; 16];
    let mut part_digest = [0u8; 16];
    message_digest.copy_from_slice(&decoded[4..20]);
    part_digest.copy_from_slice(&decoded[20..]);
    Ok((message_digest, part_digest))
}

fn stable_part_digest(path: &[u32], part: &mail_parser::MessagePart<'_>) -> [u8; 16] {
    let mut identity = Vec::with_capacity(path.len() * 4 + 44);
    identity.extend_from_slice(&(path.len() as u32).to_be_bytes());
    for segment in path {
        identity.extend_from_slice(&segment.to_be_bytes());
    }
    identity.extend_from_slice(&part.raw_header_offset().to_be_bytes());
    identity.extend_from_slice(&part.raw_body_offset().to_be_bytes());
    identity.extend_from_slice(&part.raw_end_offset().to_be_bytes());
    identity.extend_from_slice(&(part.contents().len() as u64).to_be_bytes());
    identity.extend_from_slice(&stable_digest(b"mine-mail-part-content", part.contents()));
    stable_digest(b"mine-mail-part-identity", &identity)
}

fn stable_digest(domain: &[u8], value: &[u8]) -> [u8; 16] {
    let mut hasher = Sha256::new();
    hasher.update(b"mine-mail-attachment-token-digest-v1\0");
    hasher.update((domain.len() as u64).to_be_bytes());
    hasher.update(domain);
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
    let full_digest = hasher.finalize();
    let mut digest = [0u8; 16];
    digest.copy_from_slice(&full_digest[..16]);
    digest
}

pub(crate) fn prepare_forward_source(
    raw: &[u8],
    completeness: MimeSourceCompleteness,
) -> std::result::Result<PreparedForwardSource, ForwardSourceError> {
    prepare_forward_source_with_attachment_mode(raw, completeness, true)
}

pub(crate) fn prepare_forward_source_without_attachments(
    raw: &[u8],
    completeness: MimeSourceCompleteness,
) -> std::result::Result<PreparedForwardSource, ForwardSourceError> {
    prepare_forward_source_with_attachment_mode(raw, completeness, false)
}

fn prepare_forward_source_with_attachment_mode(
    raw: &[u8],
    completeness: MimeSourceCompleteness,
    require_attachment_index: bool,
) -> std::result::Result<PreparedForwardSource, ForwardSourceError> {
    if completeness != MimeSourceCompleteness::CompleteRfc822 {
        return Err(ForwardSourceError::NonAuthoritativeSource);
    }
    let message = MessageParser::default()
        .parse(raw)
        .ok_or(ForwardSourceError::MessageCouldNotBeParsed)?;
    let attachment_index = index_parsed_message_attachments(raw, &message);
    let (attachments, has_ordinary_attachments, has_inline_resources) = match attachment_index {
        Ok(attachments) => {
            let has_ordinary = attachments
                .iter()
                .any(|attachment| attachment.disposition == AttachmentDisposition::Attachment);
            let has_inline = attachments
                .iter()
                .any(|attachment| attachment.disposition == AttachmentDisposition::Inline);
            (attachments, has_ordinary, has_inline)
        }
        Err(error) if require_attachment_index => {
            return Err(ForwardSourceError::AttachmentIndex(error));
        }
        Err(_) => {
            let (has_ordinary, has_inline) = scan_forward_attachment_presence(&message)
                .map_err(ForwardSourceError::AttachmentIndex)?;
            (Vec::new(), has_ordinary, has_inline)
        }
    };
    let quoted_text = message
        .body_text(0)
        .map(|body| body.into_owned())
        .unwrap_or_default();
    if quoted_text.len() > MAX_QUOTED_TEXT_BYTES {
        return Err(ForwardSourceError::BodyTooLarge);
    }

    let original_subject = bounded_forward_header(
        message.subject().unwrap_or_default(),
        MAX_FORWARD_SUBJECT_BYTES,
    )?;
    let from = message
        .from()
        .and_then(|addresses| addresses.first())
        .and_then(map_address)
        .map(validate_forward_address)
        .transpose()?;
    let to = bounded_forward_addresses(message.to())?;
    let cc = bounded_forward_addresses(message.cc())?;
    let raw_html = message.html_part(0).and_then(|part| match &part.body {
        PartType::Html(html) => Some(html.as_ref()),
        _ => None,
    });
    let quoted_html = raw_html.and_then(sanitize_forward_html);
    let html_downgraded = raw_html.is_some() && quoted_html.is_none();
    let quoted_render_mode = quoted_html
        .as_ref()
        .map(|_| ForwardHtmlRenderMode::NativeSemanticHtml);
    let ordinary_attachments = attachments
        .into_iter()
        .filter(|attachment| attachment.disposition == AttachmentDisposition::Attachment)
        .collect();

    Ok(PreparedForwardSource {
        original_subject,
        from,
        to,
        cc,
        sent_at: message.date().map(|date| date.to_rfc3339()),
        quoted_text,
        quoted_html,
        quoted_render_mode,
        ordinary_attachments,
        has_ordinary_attachments,
        has_inline_resources,
        html_downgraded,
    })
}

fn scan_forward_attachment_presence(
    message: &mail_parser::Message<'_>,
) -> std::result::Result<(bool, bool), AttachmentIndexError> {
    if message.parts.is_empty() {
        return Err(AttachmentIndexError::InvalidMimeStructure);
    }
    let mut visited = 0usize;
    let mut has_ordinary = false;
    let mut has_inline = false;
    scan_message_attachment_presence(message, 0, &mut visited, &mut has_ordinary, &mut has_inline)?;
    Ok((has_ordinary, has_inline))
}

fn scan_message_attachment_presence(
    message: &mail_parser::Message<'_>,
    depth: usize,
    visited: &mut usize,
    has_ordinary: &mut bool,
    has_inline: &mut bool,
) -> std::result::Result<(), AttachmentIndexError> {
    if depth > MAX_MIME_TREE_DEPTH {
        return Err(AttachmentIndexError::MimeTreeTooLarge);
    }
    for (part_id, part) in message.parts.iter().enumerate() {
        *visited = visited.saturating_add(1);
        if *visited > MAX_MIME_TREE_PARTS {
            return Err(AttachmentIndexError::MimeTreeTooLarge);
        }
        let part_id = u32::try_from(part_id).map_err(|_| AttachmentIndexError::MimeTreeTooLarge)?;
        let disposition = classify_attachment_part(message, part_id, part);
        if part.is_encoding_problem && disposition.is_none() {
            // A body-only retry may bypass a broken attachment or inline
            // resource, but never a body part whose decoded contents are not
            // authoritative.
            return Err(AttachmentIndexError::PartEncodingProblem);
        }
        match disposition {
            Some(AttachmentDisposition::Attachment) => *has_ordinary = true,
            Some(AttachmentDisposition::Inline) => *has_inline = true,
            None => {
                if let PartType::Message(nested) = &part.body {
                    scan_message_attachment_presence(
                        nested,
                        depth + 1,
                        visited,
                        has_ordinary,
                        has_inline,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn bounded_forward_addresses(
    addresses: Option<&ParsedAddress<'_>>,
) -> std::result::Result<Vec<MailAddress>, ForwardSourceError> {
    let addresses = map_addresses(addresses);
    if addresses.len() > MAX_FORWARD_ADDRESSES_PER_FIELD {
        return Err(ForwardSourceError::HeaderMetadataTooLarge);
    }
    addresses
        .into_iter()
        .map(validate_forward_address)
        .collect()
}

fn validate_forward_address(
    mut address: MailAddress,
) -> std::result::Result<MailAddress, ForwardSourceError> {
    address.email = sanitized_forward_email(&address.email)
        .ok_or(ForwardSourceError::HeaderMetadataTooLarge)?;
    address.name = address
        .name
        .as_deref()
        .map(|name| sanitize_forward_identity_text(name, MAX_FORWARD_DISPLAY_NAME_BYTES))
        .filter(|name| !name.is_empty());
    Ok(address)
}

fn bounded_forward_header(
    value: &str,
    max_bytes: usize,
) -> std::result::Result<String, ForwardSourceError> {
    Ok(sanitize_forward_identity_text(value, max_bytes))
}

fn sanitized_forward_email(value: &str) -> Option<String> {
    let email = sanitize_forward_identity_text(value, MAX_FORWARD_ADDRESS_BYTES);
    (!email.is_empty()
        && !email.ends_with(FORWARD_IDENTITY_TRUNCATION_MARKER)
        && email.parse::<Address>().is_ok())
    .then_some(email)
}

fn sanitize_forward_identity_text(value: &str, max_bytes: usize) -> String {
    if max_bytes == 0 {
        return String::new();
    }

    let mut normalized = String::with_capacity(value.len().min(max_bytes));
    let mut pending_space = false;
    let mut truncated = false;
    for character in value.chars() {
        if is_unicode_direction_control(character) {
            continue;
        }
        if character.is_control()
            || character.is_whitespace()
            || matches!(character, '\u{2028}' | '\u{2029}')
        {
            pending_space = !normalized.is_empty();
            continue;
        }

        let separator_bytes = usize::from(pending_space);
        if normalized
            .len()
            .saturating_add(separator_bytes)
            .saturating_add(character.len_utf8())
            > max_bytes
        {
            truncated = true;
            break;
        }
        if pending_space {
            normalized.push(' ');
            pending_space = false;
        }
        normalized.push(character);
    }

    if truncated {
        let content_limit = max_bytes.saturating_sub(FORWARD_IDENTITY_TRUNCATION_MARKER.len());
        while normalized.len() > content_limit {
            normalized.pop();
        }
        while normalized.ends_with(' ') {
            normalized.pop();
        }
        if FORWARD_IDENTITY_TRUNCATION_MARKER.len() <= max_bytes {
            normalized.push_str(FORWARD_IDENTITY_TRUNCATION_MARKER);
        }
    }
    normalized
}

fn is_unicode_direction_control(character: char) -> bool {
    matches!(
        character,
        '\u{061C}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{206F}'
    )
}

fn sanitize_forward_html(source: &str) -> Option<String> {
    if source.len() > MAX_QUOTED_HTML_BYTES {
        return None;
    }
    let mut builder = HtmlSanitizer::default();
    builder
        .tags(HashSet::from([
            "a",
            "b",
            "blockquote",
            "br",
            "code",
            "div",
            "em",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "hr",
            "i",
            "li",
            "ol",
            "p",
            "pre",
            "span",
            "strong",
            "table",
            "tbody",
            "td",
            "th",
            "thead",
            "tr",
            "u",
            "ul",
        ]))
        .tag_attributes(HashMap::from([("a", HashSet::from(["href"]))]))
        .clean_content_tags(HashSet::from(["script", "style"]))
        .url_schemes(HashSet::from(["http", "https", "mailto"]))
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true);
    let cleaned = builder.clean(source).to_string();
    (cleaned.len() <= MAX_QUOTED_HTML_BYTES && html_fragment_has_visible_text(&cleaned))
        .then_some(cleaned)
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
    let bcc = map_addresses(message.bcc());
    let attachment_names = message
        .attachments()
        .filter_map(|attachment| attachment.attachment_name())
        .map(|name| safe_attachment_filename(Some(name)))
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
        bcc,
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
        bcc: Vec::new(),
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

    match parse_incoming_message(raw, metadata) {
        Ok(mut summary) => {
            // Summary synchronization may receive a bounded RFC822 prefix, not
            // a complete message. Persist only the derived list preview and
            // header metadata so truncated body data can never be mistaken for
            // a hydrated body or retained as raw RFC822.
            summary.body_text = None;
            summary.body_html = None;
            summary.attachment_names.clear();
            summary.body_fetched = false;
            summary.raw_rfc822.clear();
            summary
        }
        Err(_) => fallback,
    }
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
    use base64::Engine as _;
    use mail_parser::{MessageParser, PartType};

    use super::{
        AttachmentDisposition, AttachmentIndexError, FORWARD_IDENTITY_TRUNCATION_MARKER,
        ForwardHtmlRenderMode, ForwardSourceError, IncomingMetadata, MAX_ATTACHMENT_PARTS,
        MAX_FORWARD_DISPLAY_NAME_BYTES, MAX_FORWARD_SUBJECT_BYTES, MAX_MANAGED_ATTACHMENT_BYTES,
        MAX_SAFE_ATTACHMENT_NAME_BYTES, ManagedMimeAttachment, MimeSourceCompleteness,
        add_managed_attachment_size, attachment_name_candidate, bounded_original_attachment_name,
        build_draft_message_revision, build_draft_message_revision_with_attachments,
        build_outgoing_message, build_outgoing_message_with_attachments,
        draft_has_unsupported_content, extract_attachment, extract_renderable_html,
        index_message_attachments, is_unicode_direction_control, outbox_body_html,
        outbox_body_text, outbox_has_reply_headers, outbox_message_id, outbox_preview,
        outbox_sent_at, outbox_subject, parse_draft_message, parse_incoming_message,
        parse_incoming_summary_or_fallback, prepare_forward_source,
        prepare_forward_source_without_attachments, render_message_html, restore_outbox_envelope,
        safe_attachment_filename, sanitize_compose_html, stable_digest,
    };
    use crate::{
        ComposeFormat, ComposeRequest, ForwardContext, ForwardQuotedRenderMode, MailAddress,
        ReplyContext, StationeryTheme,
    };

    fn compose() -> ComposeRequest {
        ComposeRequest {
            to: vec!["Receiver <receiver@example.com>".to_owned()],
            cc: vec![],
            bcc: vec!["hidden@example.com".to_owned()],
            subject: "中文主题".to_owned(),
            body_text: "Hello, 世界".to_owned(),
            format: Default::default(),
            reply_context: None,
        }
    }

    fn rfc2047_base64_words(value: &str) -> String {
        let mut chunks = Vec::new();
        let mut chunk = String::new();
        for character in value.chars() {
            if !chunk.is_empty() && chunk.len() + character.len_utf8() > 24 {
                chunks.push(std::mem::take(&mut chunk));
            }
            chunk.push(character);
        }
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
        chunks
            .into_iter()
            .map(|chunk| format!("=?UTF-8?B?{}?=", super::BASE64.encode(chunk.as_bytes())))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn attachment_fixture() -> Vec<u8> {
        br#"From: =?UTF-8?B?5Y+R5Lu25Lq6?= <sender@example.com>
To: Receiver <receiver@example.com>
Cc: Copy <copy@example.com>
Bcc: hidden@example.com
Date: Tue, 28 Jul 2026 10:15:00 +0800
Subject: Forward source
MIME-Version: 1.0
Content-Type: multipart/mixed; boundary=outer

--outer
Content-Type: multipart/related; boundary=related

--related
Content-Type: multipart/alternative; boundary=alternative

--alternative
Content-Type: text/plain; charset=utf-8

Complete plain fallback.
--alternative
Content-Type: text/html; charset=utf-8

<html><body><p onclick="bad()">Complete <strong>HTML</strong>.</p><script>steal()</script><img src="https://tracker.invalid/pixel"><a href="https://example.com">safe</a></body></html>
--alternative--
--related
Content-Type: image/png
Content-Transfer-Encoding: base64
Content-ID: <logo@example.com>
Content-Disposition: inline; filename="logo.png"

iVBORw0KGgo=
--related--
--outer
Content-Type: multipart/mixed; boundary=nested

--nested
Content-Type: application/octet-stream
Content-Disposition: attachment; filename="report.bin"
Content-Transfer-Encoding: base64

AQIDBA==
--nested
Content-Type: text/plain; charset=utf-8
Content-Disposition: attachment; filename="report.bin"
Content-Transfer-Encoding: quoted-printable

second=20file=0A
--nested
Content-Type: application/octet-stream
Content-Disposition: attachment

not a pdf
--nested
Content-Type: application/octet-stream
Content-Disposition: attachment; filename*=utf-8''%E6%B5%8B%E8%AF%95.dat
Content-Transfer-Encoding: base64

JVBERi0xLjcK
--nested--
--outer--
"#
        .iter()
        .copied()
        .flat_map(|byte| {
            if byte == b'\n' {
                [b'\r', b'\n'].into_iter().take(2)
            } else {
                [byte, 0].into_iter().take(1)
            }
        })
        .collect()
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
    fn outgoing_message_encodes_the_exact_managed_attachment_bytes() {
        let attachment = ManagedMimeAttachment {
            name: "report.bin".to_owned(),
            mime_type: "application/octet-stream".to_owned(),
            size_bytes: 4,
            bytes: vec![1, 2, 3, 4],
        };
        let outgoing = build_outgoing_message_with_attachments(
            "sender@example.com",
            &compose(),
            None,
            vec![attachment],
        )
        .expect("message with attachment");
        let indexed =
            index_message_attachments(&outgoing.raw_rfc822, MimeSourceCompleteness::CompleteRfc822)
                .expect("attachment index");
        let ordinary = indexed
            .into_iter()
            .find(|part| part.disposition == AttachmentDisposition::Attachment)
            .expect("ordinary attachment");

        assert_eq!(ordinary.safe_display_name, "report.bin");
        assert_eq!(ordinary.size_bytes, 4);
        assert_eq!(
            extract_attachment(&outgoing.raw_rfc822, &ordinary.id).unwrap(),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn managed_attachment_bounds_reject_mismatch_count_and_byte_overflow() {
        let mismatched = ManagedMimeAttachment {
            name: "report.bin".to_owned(),
            mime_type: "application/octet-stream".to_owned(),
            size_bytes: 5,
            bytes: vec![1, 2, 3, 4],
        };
        assert!(
            build_outgoing_message_with_attachments(
                "sender@example.com",
                &compose(),
                None,
                vec![mismatched],
            )
            .is_err()
        );

        let empty = ManagedMimeAttachment {
            name: "empty.bin".to_owned(),
            mime_type: "application/octet-stream".to_owned(),
            size_bytes: 0,
            bytes: Vec::new(),
        };
        assert!(
            build_outgoing_message_with_attachments(
                "sender@example.com",
                &compose(),
                None,
                vec![empty; MAX_ATTACHMENT_PARTS + 1],
            )
            .is_err()
        );
        assert!(add_managed_attachment_size(0, MAX_MANAGED_ATTACHMENT_BYTES + 1).is_err());
        assert!(
            add_managed_attachment_size(MAX_MANAGED_ATTACHMENT_BYTES, 1).is_err(),
            "the aggregate bound is checked with overflow-safe addition"
        );
    }

    #[test]
    fn outgoing_rich_message_keeps_plain_fallback_and_sanitizes_authored_html() {
        let mut request = compose();
        request.body_text = "安全的纯文本版本".to_owned();
        request.format = ComposeFormat {
            body_html: Some(
                r#"<p align="center"><strong>格式正文</strong><script>alert(1)</script><a href="javascript:alert(2)">危险链接</a><font face="KaiTi" size="4">楷体</font></p>"#
                    .to_owned(),
            ),
            stationery: StationeryTheme::None,
            send_stationery: false,
        };

        let outgoing =
            build_outgoing_message("sender@example.com", &request).expect("rich message");
        let raw = String::from_utf8_lossy(&outgoing.raw_rfc822);
        let html = outbox_body_html(&outgoing.raw_rfc822).expect("HTML alternative");

        assert!(raw.contains("Content-Type: multipart/alternative"));
        assert_eq!(
            outbox_body_text(&outgoing.raw_rfc822).as_deref(),
            Some("安全的纯文本版本")
        );
        assert!(html.contains("<strong>格式正文</strong>"));
        assert!(html.contains("align=\"center\""));
        assert!(html.contains("face=\"KaiTi\""));
        assert!(!html.contains("script"));
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn stationery_can_stay_editor_only_or_be_sent_with_the_html_alternative() {
        let mut editor_only = compose();
        editor_only.format.stationery = StationeryTheme::Lined;
        let editor_only_message =
            build_outgoing_message("sender@example.com", &editor_only).expect("plain message");
        let editor_only_raw = String::from_utf8_lossy(&editor_only_message.raw_rfc822);

        assert!(editor_only_raw.contains("X-Mine-Mail-Stationery: lined"));
        assert!(editor_only_raw.contains("X-Mine-Mail-Send-Stationery: 0"));
        assert!(!editor_only_raw.contains("multipart/alternative"));
        assert!(outbox_body_html(&editor_only_message.raw_rfc822).is_none());

        let mut sent_theme = compose();
        sent_theme.format = ComposeFormat {
            body_html: Some("<div><u>Hello</u>, 世界</div>".to_owned()),
            stationery: StationeryTheme::Grid,
            send_stationery: true,
        };
        let themed =
            build_outgoing_message("sender@example.com", &sent_theme).expect("themed message");
        let themed_raw = String::from_utf8_lossy(&themed.raw_rfc822);
        let html = outbox_body_html(&themed.raw_rfc822).expect("themed HTML");

        assert!(themed_raw.contains("X-Mine-Mail-Stationery: grid"));
        assert!(themed_raw.contains("X-Mine-Mail-Send-Stationery: 1"));
        assert!(themed_raw.contains("multipart/alternative"));
        assert!(html.contains("data-mine-mail-stationery=\"grid\""));
        assert!(html.contains("background-size:28px 28px"));
        assert!(html.contains("<u>Hello</u>"));
        assert_eq!(
            outbox_body_text(&themed.raw_rfc822).as_deref(),
            Some("Hello, 世界")
        );
    }

    #[test]
    fn rich_draft_round_trips_owned_format_as_editable_content() {
        let mut request = compose();
        request.format = ComposeFormat {
            body_html: Some(
                r#"<div align="right"><font face="SimSun" size="5">正文</font></div>"#.to_owned(),
            ),
            stationery: StationeryTheme::Lined,
            send_stationery: true,
        };

        let raw =
            build_draft_message_revision("sender@example.com", &request, "formatted-draft", 3)
                .expect("rich draft");
        let parsed = parse_draft_message(&raw).expect("parse rich draft");

        assert!(!parsed.has_unsupported_content);
        assert_eq!(parsed.request.format.stationery, StationeryTheme::Lined);
        assert!(parsed.request.format.send_stationery);
        let html = parsed
            .request
            .format
            .body_html
            .as_deref()
            .expect("authored HTML");
        assert!(html.contains("align=\"right\""));
        assert!(html.contains("face=\"SimSun\""));
        assert!(!html.contains("data-mine-mail-stationery"));
    }

    #[test]
    fn compose_html_sanitizer_rejects_remote_and_executable_content() {
        let cleaned = sanitize_compose_html(Some(
            r#"<style>body{display:none}</style><img src="https://tracker.example/pixel"><p onclick="steal()"><a href="https://example.com">安全链接</a><a href="data:text/html,bad">坏链接</a></p>"#,
        ))
        .expect("visible safe fragment");

        assert!(!cleaned.contains("style"));
        assert!(!cleaned.contains("img"));
        assert!(!cleaned.contains("onclick"));
        assert!(cleaned.contains("href=\"https://example.com\""));
        assert!(!cleaned.contains("data:text/html"));
    }

    #[test]
    fn attachment_index_is_stable_exact_and_distinguishes_inline_resources() {
        let raw = attachment_fixture();
        let first = index_message_attachments(&raw, MimeSourceCompleteness::CompleteRfc822)
            .expect("complete MIME index");
        let second = index_message_attachments(&raw, MimeSourceCompleteness::CompleteRfc822)
            .expect("stable MIME index");

        assert_eq!(first, second);
        assert_eq!(first.len(), 5);
        let inline = first
            .iter()
            .find(|part| part.disposition == AttachmentDisposition::Inline)
            .expect("inline resource");
        assert_eq!(inline.original_name.as_deref(), Some("logo.png"));
        assert_eq!(inline.size_bytes, 8);
        assert_eq!(inline.detected_mime_type.as_deref(), Some("image/png"));
        assert_eq!(
            extract_attachment(&raw, &inline.id),
            Err(AttachmentIndexError::AttachmentNotFound)
        );

        let ordinary = first
            .iter()
            .filter(|part| part.disposition == AttachmentDisposition::Attachment)
            .collect::<Vec<_>>();
        assert_eq!(ordinary.len(), 4);
        assert_eq!(
            ordinary
                .iter()
                .filter(|part| part.original_name.as_deref() == Some("report.bin"))
                .count(),
            2
        );
        assert_ne!(ordinary[0].id, ordinary[1].id);
        assert_eq!(
            extract_attachment(&raw, &ordinary[0].id).expect("base64 attachment"),
            [1, 2, 3, 4]
        );
        assert_eq!(
            extract_attachment(&raw, &ordinary[1].id).expect("quoted-printable attachment"),
            b"second file\n"
        );
        assert_eq!(ordinary[0].size_bytes, 4);
        assert_eq!(ordinary[1].size_bytes, 12);
    }

    #[test]
    fn attachment_index_decodes_rfc2231_names_and_detects_content_without_trusting_extensions() {
        let raw = attachment_fixture();
        let parts = index_message_attachments(&raw, MimeSourceCompleteness::CompleteRfc822)
            .expect("complete MIME index");
        let unnamed = parts
            .iter()
            .find(|part| part.original_name.is_none())
            .expect("unnamed attachment");
        assert_eq!(unnamed.safe_display_name, "attachment.bin");
        assert_eq!(unnamed.mime_type, "application/octet-stream");
        assert_eq!(unnamed.detected_mime_type, None);

        let detected_pdf = parts
            .iter()
            .find(|part| part.original_name.as_deref() == Some("测试.dat"))
            .expect("RFC2231 attachment name");
        assert_eq!(
            detected_pdf.declared_mime_type.as_deref(),
            Some("application/octet-stream")
        );
        assert_eq!(
            detected_pdf.detected_mime_type.as_deref(),
            Some("application/pdf")
        );
        assert_eq!(detected_pdf.mime_type, "application/pdf");
        assert_eq!(detected_pdf.size_bytes, 9);
    }

    #[test]
    fn attachment_index_decodes_rfc2047_filename_words() {
        let raw = b"From: sender@example.com\r\n\
                    MIME-Version: 1.0\r\n\
                    Content-Type: application/octet-stream\r\n\
                    Content-Disposition: attachment; filename=\"=?UTF-8?B?5rWL6K+VLnBkZg==?=\"\r\n\
                    Content-Transfer-Encoding: base64\r\n\
                    \r\n\
                    AQID";
        let parts = index_message_attachments(raw, MimeSourceCompleteness::CompleteRfc822)
            .expect("encoded filename");

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].original_name.as_deref(), Some("测试.pdf"));
        assert_eq!(extract_attachment(raw, &parts[0].id).unwrap(), [1, 2, 3]);
    }

    #[test]
    fn attachment_tokens_reject_tampering_and_a_different_raw_message() {
        let raw = attachment_fixture();
        let token = index_message_attachments(&raw, MimeSourceCompleteness::CompleteRfc822)
            .unwrap()
            .into_iter()
            .find(|part| part.disposition == AttachmentDisposition::Attachment)
            .unwrap()
            .id;
        let mut tampered = token.clone().into_bytes();
        let last = tampered.last_mut().unwrap();
        *last = if *last == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(tampered).unwrap();

        assert!(extract_attachment(&raw, &tampered).is_err());
        let mut changed_raw = raw.clone();
        changed_raw.extend_from_slice(b"\r\n");
        assert_eq!(
            extract_attachment(&changed_raw, &token),
            Err(AttachmentIndexError::AttachmentNotFound)
        );
    }

    #[test]
    fn attachment_token_sha256_digest_is_domain_separated_and_deterministic() {
        let first = stable_digest(b"message", b"same bytes");
        assert_eq!(first, stable_digest(b"message", b"same bytes"));
        assert_ne!(first, stable_digest(b"part", b"same bytes"));
        assert_ne!(first, stable_digest(b"message", b"different bytes"));
        assert_eq!(first.len(), 16);
    }

    #[test]
    fn safe_attachment_names_block_cross_platform_path_and_reserved_name_tricks() {
        let malicious = safe_attachment_filename(Some("../../folder\\evil\u{0}.exe. "));
        assert!(!malicious.contains('/'));
        assert!(!malicious.contains('\\'));
        assert!(!malicious.contains('\0'));
        assert!(!malicious.ends_with(['.', ' ']));
        assert_eq!(safe_attachment_filename(Some("CON.txt")), "_CON.txt");
        assert_eq!(safe_attachment_filename(Some("nul")), "_nul");
        assert_eq!(safe_attachment_filename(Some(" . ")), "attachment.bin");
        assert_eq!(safe_attachment_filename(None), "attachment.bin");

        let long = format!("{}.pdf", "邮件".repeat(100));
        let bounded = safe_attachment_filename(Some(&long));
        assert!(bounded.len() <= 180);
        assert!(bounded.ends_with(".pdf"));
        assert!(std::str::from_utf8(bounded.as_bytes()).is_ok());
    }

    #[test]
    fn attachment_names_replace_bidi_controls_without_changing_rtl_letters() {
        let deceptive = "invoice\u{202e}fdp.exe\u{2066}";
        let original = bounded_original_attachment_name(deceptive).unwrap();
        let safe = safe_attachment_filename(Some(deceptive));
        assert_eq!(original, "invoice\u{fffd}fdp.exe\u{fffd}");
        assert_eq!(safe, "invoice_fdp.exe_");
        assert!(!original.contains(['\u{202e}', '\u{2066}']));
        assert!(!safe.contains(['\u{202e}', '\u{2066}']));

        let ordinary_rtl = "تقرير-שלום.pdf";
        assert_eq!(
            bounded_original_attachment_name(ordinary_rtl).as_deref(),
            Some(ordinary_rtl)
        );
        assert_eq!(safe_attachment_filename(Some(ordinary_rtl)), ordinary_rtl);
    }

    #[test]
    fn incoming_legacy_attachment_names_are_safe_bounded_display_names() {
        let raw = b"From: sender@example.com\r\n\
To: receiver@example.com\r\n\
Subject: unsafe name\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=x\r\n\r\n\
--x\r\nContent-Type: text/plain\r\n\r\nBody\r\n\
--x\r\nContent-Type: application/octet-stream\r\n\
Content-Disposition: attachment; filename*=utf-8''..%2Finvoice%E2%80%AEfdp.exe\r\n\
Content-Transfer-Encoding: base64\r\n\r\nAQID\r\n--x--\r\n";
        let parsed = parse_incoming_message(
            raw,
            IncomingMetadata {
                account_id: "primary",
                mailbox: "INBOX",
                uid: 42,
                flags: Vec::new(),
                internal_date: None,
                size_bytes: raw.len() as u32,
                synced_at: "2026-07-28T00:00:00Z".to_owned(),
                body_fetched: true,
            },
        )
        .expect("parse incoming attachment");

        assert_eq!(
            parsed.attachment_names,
            [safe_attachment_filename(Some("../invoice\u{202e}fdp.exe"))]
        );
        let display_name = &parsed.attachment_names[0];
        assert!(display_name.len() <= MAX_SAFE_ATTACHMENT_NAME_BYTES);
        assert!(!display_name.contains(['/', '\\', '\u{202e}']));
    }

    #[test]
    fn incoming_message_retains_only_an_actual_bcc_header() {
        let with_bcc = b"From: Sender <sender@example.com>\r\n\
To: Receiver <receiver@example.com>\r\n\
Cc: Copy <copy@example.com>\r\n\
Bcc: Blind One <blind-one@example.com>, blind-two@example.com\r\n\
Subject: Explicit Bcc\r\n\r\nBody";
        let parsed = parse_incoming_message(
            with_bcc,
            IncomingMetadata {
                account_id: "primary",
                mailbox: "Sent",
                uid: 43,
                flags: Vec::new(),
                internal_date: None,
                size_bytes: with_bcc.len() as u32,
                synced_at: "2026-07-28T00:00:00Z".to_owned(),
                body_fetched: true,
            },
        )
        .expect("message with an explicit Bcc header");
        assert_eq!(
            parsed
                .bcc
                .iter()
                .map(|address| (address.name.as_deref(), address.email.as_str()))
                .collect::<Vec<_>>(),
            [
                (Some("Blind One"), "blind-one@example.com"),
                (None, "blind-two@example.com"),
            ]
        );

        let without_bcc = b"From: Sender <sender@example.com>\r\n\
To: Receiver <receiver@example.com>\r\n\
Delivered-To: hidden@example.com\r\n\
X-Original-To: hidden@example.com\r\n\
Subject: No Bcc header\r\n\r\nBody";
        let parsed = parse_incoming_message(
            without_bcc,
            IncomingMetadata {
                account_id: "primary",
                mailbox: "INBOX",
                uid: 44,
                flags: Vec::new(),
                internal_date: None,
                size_bytes: without_bcc.len() as u32,
                synced_at: "2026-07-28T00:00:00Z".to_owned(),
                body_fetched: true,
            },
        )
        .expect("message without a Bcc header");
        assert!(parsed.bcc.is_empty());
    }

    #[test]
    fn attachment_collision_candidates_preserve_extension_without_overwrite_names() {
        assert_eq!(attachment_name_candidate("report.pdf", 0), "report.pdf");
        assert_eq!(attachment_name_candidate("report.pdf", 1), "report (1).pdf");
        assert_eq!(
            attachment_name_candidate("archive.tar.gz", 27),
            "archive.tar (27).gz"
        );
        let bounded = attachment_name_candidate(&format!("{}.txt", "a".repeat(200)), u32::MAX);
        assert!(bounded.len() <= 180);
        assert!(bounded.ends_with(" (4294967295).txt"));
    }

    #[test]
    fn bounded_summary_prefix_cannot_authorize_attachment_or_forward_metadata() {
        let raw = attachment_fixture();
        let prefix = &raw[..raw.len() / 2];

        assert_eq!(
            index_message_attachments(prefix, MimeSourceCompleteness::BoundedSummaryPrefix),
            Err(AttachmentIndexError::NonAuthoritativeSource)
        );
        assert_eq!(
            prepare_forward_source(prefix, MimeSourceCompleteness::BoundedSummaryPrefix),
            Err(ForwardSourceError::NonAuthoritativeSource)
        );
    }

    #[test]
    fn forward_source_uses_complete_plain_body_safe_html_and_excludes_bcc() {
        let raw = attachment_fixture();
        let source = prepare_forward_source(&raw, MimeSourceCompleteness::CompleteRfc822)
            .expect("prepared forward source");

        assert_eq!(source.original_subject, "Forward source");
        assert_eq!(
            source.from.as_ref().map(|address| address.email.as_str()),
            Some("sender@example.com")
        );
        assert_eq!(
            source
                .to
                .iter()
                .map(|address| address.email.as_str())
                .collect::<Vec<_>>(),
            ["receiver@example.com"]
        );
        assert_eq!(
            source
                .cc
                .iter()
                .map(|address| address.email.as_str())
                .collect::<Vec<_>>(),
            ["copy@example.com"]
        );
        assert_eq!(source.quoted_text.trim(), "Complete plain fallback.");
        let html = source.quoted_html.as_deref().expect("sanitized HTML");
        assert!(html.contains("<strong>HTML</strong>"));
        assert!(html.contains("href=\"https://example.com\""));
        assert!(!html.contains("onclick"));
        assert!(!html.contains("script"));
        assert!(!html.contains("tracker.invalid"));
        assert_eq!(
            source.quoted_render_mode,
            Some(ForwardHtmlRenderMode::NativeSemanticHtml)
        );
        assert_eq!(source.ordinary_attachments.len(), 4);
        assert!(source.has_inline_resources);
        assert!(!source.html_downgraded);
        assert!(
            source
                .from
                .iter()
                .chain(source.to.iter())
                .chain(source.cc.iter())
                .all(|address| address.email != "hidden@example.com")
        );
    }

    #[test]
    fn forward_identity_normalizes_decoded_controls_and_bidi_in_plain_and_html() {
        let raw = b"From: =?UTF-8?B?QWxpY2XigaYNCkZyb206IGltcG9zdG9yQGV4YW1wbGUuY29tAeKBqQ==?= <sender@example.com>\r\n\
                    To: =?UTF-8?B?UmVjZWl2ZXLigI8KVG86IGltcG9zdG9yQGV4YW1wbGUuY29tBw==?= <receiver@example.com>\r\n\
                    Cc: =?UTF-8?B?Q29wedicDUNjOiBpbXBvc3RvckBleGFtcGxlLmNvbeKArXjigKw=?= <copy@example.com>\r\n\
                    Subject: =?UTF-8?B?UXVhcnRlcmx5IOKArmdwai5leGXigKwNCkNjOiBpbXBvc3RvckBleGFtcGxlLmNvbQcgZW5k?=\r\n\
                    Content-Type: text/plain; charset=utf-8\r\n\
                    \r\n\
                    Complete original body";
        let source = prepare_forward_source(raw, MimeSourceCompleteness::CompleteRfc822)
            .expect("decoded identity metadata remains forwardable");

        assert_eq!(
            source.original_subject,
            "Quarterly gpj.exe Cc: impostor@example.com end"
        );
        assert_eq!(
            source
                .from
                .as_ref()
                .and_then(|address| address.name.as_deref()),
            Some("Alice From: impostor@example.com")
        );
        assert_eq!(
            source
                .to
                .first()
                .and_then(|address| address.name.as_deref()),
            Some("Receiver To: impostor@example.com")
        );
        assert_eq!(
            source
                .cc
                .first()
                .and_then(|address| address.name.as_deref()),
            Some("Copy Cc: impostor@example.comx")
        );
        for value in std::iter::once(source.original_subject.as_str()).chain(
            source
                .from
                .iter()
                .chain(&source.to)
                .chain(&source.cc)
                .flat_map(|address| [address.name.as_deref(), Some(address.email.as_str())])
                .flatten(),
        ) {
            assert!(!value.chars().any(char::is_control));
            assert!(!value.chars().any(is_unicode_direction_control));
        }

        let context = ForwardContext {
            source_message_id: "0198-control-source".to_owned(),
            original_subject: source.original_subject,
            from: source.from,
            to: source.to,
            cc: source.cc,
            sent_at: None,
            quoted_text: source.quoted_text,
            quoted_html: None,
            quoted_render_mode: Some(ForwardQuotedRenderMode::Plain),
            source_attachments: Vec::new(),
        };
        let mut request = compose();
        request.bcc.clear();
        request.body_text = "Authored introduction".to_owned();
        request.format.body_html = Some("<p>Authored introduction</p>".to_owned());
        let raw = build_draft_message_revision_with_attachments(
            "author@example.com",
            &request,
            "control-forward",
            1,
            Some(&context),
            Vec::new(),
        )
        .expect("safe forward MIME");
        let plain = outbox_body_text(&raw).expect("plain forward alternative");
        let html = outbox_body_html(&raw).expect("HTML forward alternative");
        let identity = [
            ("Subject", "Quarterly gpj.exe Cc: impostor@example.com end"),
            (
                "From",
                "\"Alice From: impostor@example.com\" <sender@example.com>",
            ),
            (
                "To",
                "\"Receiver To: impostor@example.com\" <receiver@example.com>",
            ),
            (
                "Cc",
                "\"Copy Cc: impostor@example.comx\" <copy@example.com>",
            ),
        ];
        for (label, value) in identity {
            assert!(plain.contains(&format!("{label}: {value}")));
            assert!(html.contains(&format!(
                "<strong>{label}:</strong> {}",
                super::html_escape(value)
            )));
        }
        assert!(!plain.contains("\r\nFrom: impostor@example.com"));
        assert!(!plain.contains("\nTo: impostor@example.com"));
    }

    #[test]
    fn forward_identity_visibly_bounds_long_decoded_words_without_hiding_real_address() {
        let long_subject = format!("Subject prefix {}", "主题".repeat(1_500));
        let long_name = format!("Named sender {}", "名字".repeat(500));
        let raw = format!(
            "From: {} <sender@example.com>\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nComplete original body",
            rfc2047_base64_words(&long_name),
            rfc2047_base64_words(&long_subject),
        );
        let source = prepare_forward_source(raw.as_bytes(), MimeSourceCompleteness::CompleteRfc822)
            .expect("oversized decoded display text is bounded, not discarded");
        let source_name = source
            .from
            .as_ref()
            .and_then(|address| address.name.as_deref())
            .expect("bounded display name");

        assert!(source.original_subject.starts_with("Subject prefix"));
        assert!(
            source
                .original_subject
                .ends_with(FORWARD_IDENTITY_TRUNCATION_MARKER)
        );
        assert!(source.original_subject.len() <= MAX_FORWARD_SUBJECT_BYTES);
        assert!(source_name.starts_with("Named sender"));
        assert!(source_name.ends_with(FORWARD_IDENTITY_TRUNCATION_MARKER));
        assert!(source_name.len() <= MAX_FORWARD_DISPLAY_NAME_BYTES);
        assert_eq!(
            source.from.as_ref().map(|address| address.email.as_str()),
            Some("sender@example.com")
        );

        let expected_subject = source.original_subject.clone();
        let expected_from = format!("\"{source_name}\" <sender@example.com>");
        let context = ForwardContext {
            source_message_id: "0198-long-identity-source".to_owned(),
            original_subject: source.original_subject,
            from: source.from,
            to: source.to,
            cc: source.cc,
            sent_at: None,
            quoted_text: source.quoted_text,
            quoted_html: None,
            quoted_render_mode: Some(ForwardQuotedRenderMode::Plain),
            source_attachments: Vec::new(),
        };
        let mut request = compose();
        request.bcc.clear();
        request.format.body_html = Some("<p>Authored introduction</p>".to_owned());
        let raw = build_draft_message_revision_with_attachments(
            "author@example.com",
            &request,
            "long-identity-forward",
            1,
            Some(&context),
            Vec::new(),
        )
        .expect("bounded forward MIME");
        let plain = outbox_body_text(&raw).expect("plain forward alternative");
        let html = outbox_body_html(&raw).expect("HTML forward alternative");

        assert!(plain.contains(&format!("Subject: {expected_subject}")));
        assert!(plain.contains(&format!("From: {expected_from}")));
        assert!(html.contains(&format!(
            "<strong>Subject:</strong> {}",
            super::html_escape(&expected_subject)
        )));
        assert!(html.contains(&format!(
            "<strong>From:</strong> {}",
            super::html_escape(&expected_from)
        )));
        assert!(plain.contains("<sender@example.com>"));
        assert!(html.contains("&lt;sender@example.com&gt;"));
    }

    #[test]
    fn forward_source_without_html_keeps_a_complete_plain_fallback() {
        let raw = b"From: Sender <sender@example.com>\r\n\
                    To: receiver@example.com\r\n\
                    Bcc: hidden@example.com\r\n\
                    Subject: Plain only\r\n\
                    Content-Type: text/plain; charset=utf-8\r\n\
                    \r\n\
                    First line\r\nSecond line";
        let source = prepare_forward_source(raw, MimeSourceCompleteness::CompleteRfc822)
            .expect("plain forward source");

        assert_eq!(source.quoted_text, "First line\r\nSecond line");
        assert_eq!(source.quoted_html, None);
        assert_eq!(source.quoted_render_mode, None);
        assert!(source.ordinary_attachments.is_empty());
        assert!(!source.has_inline_resources);
    }

    #[test]
    fn forward_draft_preserves_authored_text_context_and_exact_attachments() {
        let mut request = compose();
        request.bcc.clear();
        request.subject = "Fwd: Original subject".to_owned();
        request.body_text = "Authored introduction".to_owned();
        let context = ForwardContext {
            source_message_id: "0198-message-public-id".to_owned(),
            original_subject: "Original subject".to_owned(),
            from: Some(MailAddress {
                name: Some("Original Sender".to_owned()),
                email: "original@example.com".to_owned(),
            }),
            to: vec![MailAddress {
                name: None,
                email: "original-recipient@example.com".to_owned(),
            }],
            cc: vec![MailAddress {
                name: Some("Copy".to_owned()),
                email: "copy@example.com".to_owned(),
            }],
            sent_at: Some("2026-07-28T10:15:00+08:00".to_owned()),
            quoted_text: "First complete line\nLast complete line".to_owned(),
            quoted_html: None,
            quoted_render_mode: None,
            source_attachments: Vec::new(),
        };
        let attachment = ManagedMimeAttachment {
            name: "forwarded.bin".to_owned(),
            mime_type: "application/octet-stream".to_owned(),
            size_bytes: 3,
            bytes: vec![7, 8, 9],
        };

        let raw = build_draft_message_revision_with_attachments(
            "sender@example.com",
            &request,
            "forward-draft",
            2,
            Some(&context),
            vec![attachment],
        )
        .expect("forward draft");
        let body = outbox_body_text(&raw).expect("plain body");
        assert!(body.starts_with("Authored introduction"));
        assert!(body.contains("---------- Forwarded message ----------"));
        assert!(body.contains("Subject: Original subject"));
        assert!(body.contains("From: \"Original Sender\" <original@example.com>"));
        assert!(body.contains("Last complete line"));
        assert!(!body.contains("Bcc:"));

        let parsed = parse_draft_message(&raw).expect("parse forward draft");
        assert_eq!(parsed.request.body_text, "Authored introduction");
        assert!(
            parsed.has_unsupported_content,
            "a remote-only forward copy must remain read-only without SQLite context"
        );
        let indexed =
            index_message_attachments(&raw, MimeSourceCompleteness::CompleteRfc822).unwrap();
        let forwarded = indexed
            .into_iter()
            .find(|part| part.disposition == AttachmentDisposition::Attachment)
            .expect("forwarded attachment");
        assert_eq!(forwarded.safe_display_name, "forwarded.bin");
        assert_eq!(extract_attachment(&raw, &forwarded.id).unwrap(), [7, 8, 9]);
    }

    #[test]
    fn forward_source_fails_as_a_whole_when_attachment_decoding_is_invalid() {
        let raw = b"From: sender@example.com\r\n\
                    Subject: Broken attachment\r\n\
                    Content-Type: multipart/mixed; boundary=x\r\n\
                    \r\n\
                    --x\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    Body\r\n\
                    --x\r\n\
                    Content-Type: application/octet-stream\r\n\
                    Content-Disposition: attachment; filename=broken.bin\r\n\
                    Content-Transfer-Encoding: base64\r\n\
                    \r\n\
                    this is not base64 !!!\r\n\
                    --x--\r\n";

        assert_eq!(
            prepare_forward_source(raw, MimeSourceCompleteness::CompleteRfc822),
            Err(ForwardSourceError::AttachmentIndex(
                AttachmentIndexError::PartEncodingProblem
            ))
        );
        let body_only =
            prepare_forward_source_without_attachments(raw, MimeSourceCompleteness::CompleteRfc822)
                .expect("body-only forward");
        assert_eq!(body_only.quoted_text.trim(), "Body");
        assert!(body_only.has_ordinary_attachments);
        assert!(body_only.ordinary_attachments.is_empty());
    }

    #[test]
    fn body_only_forward_does_not_bypass_a_broken_body_encoding() {
        let raw = b"From: sender@example.com\r\n\
                    Subject: Broken body\r\n\
                    Content-Type: text/plain; charset=utf-8\r\n\
                    Content-Transfer-Encoding: base64\r\n\
                    \r\n\
                    this is not base64 !!!\r\n";

        assert_eq!(
            prepare_forward_source_without_attachments(raw, MimeSourceCompleteness::CompleteRfc822,),
            Err(ForwardSourceError::AttachmentIndex(
                AttachmentIndexError::PartEncodingProblem
            ))
        );
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
            quoted_html: Some(
                r#"<p>Original <a href="https://paa.moe">linked body</a></p><img alt="avatar" src="data:image/png;base64,AQID">"#
                    .to_owned(),
            ),
        });

        let outgoing = build_outgoing_message("sender@example.com", &request).expect("reply");
        let raw = String::from_utf8_lossy(&outgoing.raw_rfc822);

        assert_eq!(
            outbox_preview(&outgoing.raw_rfc822).as_deref(),
            Some("这是回复内容")
        );
        assert!(outbox_has_reply_headers(&outgoing.raw_rfc822));
        let outbox_html = outbox_body_html(&outgoing.raw_rfc822).expect("Outbox HTML body");
        assert!(outbox_html.contains("href=\"https://paa.moe\""));
        assert!(outbox_html.contains("src=\"data:image/png;base64,AQID\""));

        assert!(raw.contains("In-Reply-To: <parent@example.com>\r\n"));
        assert!(raw.contains("References: <root@example.com> <parent@example.com>\r\n"));
        assert!(raw.contains("Content-Type: multipart/alternative"));
        assert!(raw.contains("Content-Type: text/plain"));
        assert!(raw.contains("Content-Type: text/html"));
        assert!(raw.contains("Content-Type: multipart/related"));
        assert!(raw.contains("Content-ID: <mine-mail-quote-1@mine-mail.invalid>"));

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
        assert!(html.contains("href=\"https://paa.moe\""));
        assert!(html.contains("src=\"cid:mine-mail-quote-1@mine-mail.invalid\""));
        let renderable = extract_renderable_html(&parsed).expect("renderable rich reply");
        assert!(renderable.contains("href=\"https://paa.moe\""));
        assert!(renderable.contains("src=\"data:image/png;base64,AQID\""));
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
            quoted_html: None,
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
    fn bounded_summary_prefix_produces_only_a_preview() {
        let raw = "From: sender@example.com\r\n\
                   To: receiver@example.com\r\n\
                   Subject: Preview\r\n\
                   Content-Type: text/plain; charset=utf-8\r\n\
                   \r\n\
                   这是同步阶段提取的列表摘要，完整正文仍然需要稍后获取。";
        let summary = parse_incoming_summary_or_fallback(
            raw.as_bytes(),
            IncomingMetadata {
                account_id: "primary",
                mailbox: "INBOX",
                uid: 39,
                flags: Vec::new(),
                internal_date: Some("2026-07-14T00:00:00Z".to_owned()),
                size_bytes: 4096,
                synced_at: "2026-07-14T00:01:00Z".to_owned(),
                body_fetched: false,
            },
        );

        assert!(summary.preview.contains("同步阶段提取的列表摘要"));
        assert_eq!(summary.body_text, None);
        assert_eq!(summary.body_html, None);
        assert!(summary.attachment_names.is_empty());
        assert!(!summary.body_fetched);
        assert!(summary.raw_rfc822.is_empty());
    }

    #[test]
    fn bounded_html_summary_is_converted_to_readable_text() {
        let raw = b"From: sender@example.com\r\n\
                    To: receiver@example.com\r\n\
                    Subject: HTML preview\r\n\
                    Content-Type: text/html; charset=utf-8\r\n\
                    \r\n\
                    <html><body><p>Readable <strong>HTML</strong> preview.</p></body></html>";
        let summary = parse_incoming_summary_or_fallback(
            raw,
            IncomingMetadata {
                account_id: "primary",
                mailbox: "Sent",
                uid: 40,
                flags: Vec::new(),
                internal_date: None,
                size_bytes: 8192,
                synced_at: "2026-07-14T00:01:00Z".to_owned(),
                body_fetched: false,
            },
        );

        assert_eq!(summary.preview.trim(), "Readable HTML preview.");
        assert_eq!(summary.body_text, None);
        assert_eq!(summary.body_html, None);
        assert!(summary.raw_rfc822.is_empty());
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
