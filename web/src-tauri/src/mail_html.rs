use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ammonia::{Builder, UrlRelative};
use html5ever::tree_builder::TreeSink;
use scraper::{ElementRef, Html, HtmlTreeSink, Selector};

const MAX_NATIVE_HTML_BYTES: usize = 32 * 1024;
const MAX_NATIVE_ELEMENTS: usize = 100;
const MAX_NATIVE_DEPTH: usize = 10;
const MAX_NATIVE_IMAGES: usize = 3;
const MAX_DEGRADABLE_STYLE_ELEMENTS: usize = 24;
const MAX_DEGRADABLE_STYLE_DEPTH: usize = 6;
const MAX_NATIVE_TABLE_DEPTH: usize = 24;
const MAX_DEGRADABLE_TABLE_ROWS: usize = 4;
const MAX_DEGRADABLE_TABLE_CELLS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MailHtmlStructure {
    PlainEquivalent,
    Native,
    Isolated,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SanitizedMailHtml {
    pub fragment: String,
    pub native_fragment: Option<String>,
    pub has_remote_images: bool,
    pub structure: MailHtmlStructure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MailBodySegmentKind {
    Authored,
    Quoted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MailBodySegmentConfidence {
    High,
    Medium,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MailBodySegmentMetadata {
    pub subject: Option<String>,
    pub sender: Option<String>,
    pub recipient: Option<String>,
    pub sent_at: Option<String>,
}

impl MailBodySegmentMetadata {
    fn is_empty(&self) -> bool {
        self.subject.is_none()
            && self.sender.is_none()
            && self.recipient.is_none()
            && self.sent_at.is_none()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SanitizedMailBodySegment {
    pub kind: MailBodySegmentKind,
    pub content: String,
    pub is_html: bool,
    pub structure: MailHtmlStructure,
    pub quote_depth: u8,
    pub confidence: MailBodySegmentConfidence,
    pub quote_metadata: Option<MailBodySegmentMetadata>,
}

#[derive(Debug, PartialEq, Eq)]
struct PlainBodySegment {
    kind: MailBodySegmentKind,
    content: String,
    quote_depth: u8,
    confidence: MailBodySegmentConfidence,
    quote_metadata: Option<MailBodySegmentMetadata>,
}

/// Splits replies into authored and quoted regions without discarding either.
/// Plain alternatives are preferred because reply clients tend to preserve
/// quote boundaries there even when their HTML template is layout-heavy. HTML
/// DOM detection is the fallback for HTML-only messages.
pub(crate) fn segment_mail_body(
    body_text: Option<&str>,
    body_html: Option<&str>,
    has_reply_headers: bool,
) -> Vec<SanitizedMailBodySegment> {
    if let Some(text) = body_text
        && let Some(segments) = split_plain_reply(text, has_reply_headers)
    {
        return segments
            .into_iter()
            .map(|segment| SanitizedMailBodySegment {
                kind: segment.kind,
                content: segment.content,
                is_html: false,
                structure: MailHtmlStructure::PlainEquivalent,
                quote_depth: segment.quote_depth,
                confidence: segment.confidence,
                quote_metadata: segment.quote_metadata,
            })
            .collect();
    }

    body_html
        .and_then(|html| split_html_reply(html, has_reply_headers))
        .unwrap_or_default()
}

#[derive(Debug, Default, PartialEq, Eq)]
struct HtmlAnalysis {
    elements: usize,
    max_depth: usize,
    images: usize,
    inline_image_bytes: usize,
    has_meaningful_semantics: bool,
    style_blocks: usize,
    has_styling_hooks: bool,
    tables: usize,
    table_depth: usize,
    max_table_depth: usize,
    table_rows: usize,
    table_cells: usize,
    merged_table_cells: usize,
    has_sizing_layout: bool,
    has_blocking_layout: bool,
}

pub(crate) fn sanitize_mail_html(source: &str) -> SanitizedMailHtml {
    // Ammonia sanitizes an HTML fragment. Full XHTML email documents can
    // cause a head/title node to be re-parented as plain text before the tag
    // blacklist runs, making the document title visible above the body.
    let source_without_titles = strip_title_elements(source);
    let source = source_without_titles.as_ref();
    let has_remote_images = Arc::new(AtomicBool::new(false));
    let remote_image_flag = Arc::clone(&has_remote_images);
    let mut builder = Builder::default();

    // Complex sender HTML keeps its layout attributes and responsive styles,
    // but remains isolated by the reader iframe's no-script sandbox and CSP.
    builder
        .clean_content_tags(HashSet::from(["script"]))
        .add_tags(&["font", "section", "style"])
        .add_generic_attributes(&[
            "align",
            "background",
            "bgcolor",
            "border",
            "cellpadding",
            "cellspacing",
            "class",
            "dir",
            "height",
            "id",
            "style",
            "valign",
            "width",
        ])
        .add_tag_attributes("a", &["name"])
        .add_tag_attributes("font", &["color", "face", "size"])
        .url_schemes(HashSet::from(["data", "http", "https", "mailto"]))
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true)
        .attribute_filter(move |element, attribute, value| {
            if (element, attribute) == ("img", "src")
                && is_data_url(value)
                && !is_safe_image_data_url(value)
            {
                return None;
            }
            if (element, attribute) == ("a", "href") && is_data_url(value) {
                return None;
            }
            if matches!((element, attribute), ("img", "src") | (_, "background"))
                && is_remote_url(value)
            {
                remote_image_flag.store(true, Ordering::Relaxed);
            }
            Some(value.into())
        });

    let fragment = builder.clean(source).to_string();
    let lower_fragment = fragment.to_ascii_lowercase();
    let has_remote_css_image = [
        "url(http://",
        "url(https://",
        "url('http://",
        "url('https://",
        "url(\"http://",
        "url(\"https://",
    ]
    .iter()
    .any(|needle| lower_fragment.contains(needle));
    let analysis = analyze_html(&fragment);
    let structural_bytes = fragment.len().saturating_sub(analysis.inline_image_bytes);
    let source_lower = source.to_ascii_lowercase();
    let has_hard_source_tag = [
        "script", "picture", "form", "input", "button", "select", "textarea", "video", "audio",
        "canvas", "svg", "object", "embed", "iframe",
    ]
    .iter()
    .any(|tag| contains_start_tag(&source_lower, tag));

    let style_dependent_layout = analysis.style_blocks > 0
        && (analysis.has_styling_hooks
            || analysis.elements > MAX_DEGRADABLE_STYLE_ELEMENTS
            || analysis.max_depth > MAX_DEGRADABLE_STYLE_DEPTH
            || analysis.images > 0);
    let degradable_table = analysis.tables == 1
        && analysis.max_table_depth == 1
        && analysis.table_rows <= MAX_DEGRADABLE_TABLE_ROWS
        && analysis.table_cells <= MAX_DEGRADABLE_TABLE_CELLS
        && analysis.merged_table_cells == 0
        && analysis.style_blocks == 0
        && analysis.has_meaningful_semantics
        && !analysis.has_blocking_layout;
    let table_requires_isolation = analysis.tables > 0 && !degradable_table;
    let layout_requires_isolation =
        analysis.has_blocking_layout || (analysis.has_sizing_layout && !degradable_table);
    let depth_limit = if degradable_table {
        MAX_NATIVE_TABLE_DEPTH
    } else {
        MAX_NATIVE_DEPTH
    };

    // Wrapper-only HTML has no presentation worth preserving. Prefer its
    // readable text alternative even when a producer emitted hundreds of
    // nested div/br nodes; wrapper volume alone must never create an iframe.
    let is_plain_equivalent = !analysis.has_meaningful_semantics
        && !has_hard_source_tag
        && analysis.tables == 0
        && !layout_requires_isolation
        && !style_dependent_layout;
    let structure = if is_plain_equivalent {
        MailHtmlStructure::PlainEquivalent
    } else if has_hard_source_tag
        || table_requires_isolation
        || layout_requires_isolation
        || style_dependent_layout
        || structural_bytes > MAX_NATIVE_HTML_BYTES
        || analysis.elements > MAX_NATIVE_ELEMENTS
        || analysis.max_depth > depth_limit
        || analysis.images > MAX_NATIVE_IMAGES
    {
        MailHtmlStructure::Isolated
    } else if analysis.has_meaningful_semantics {
        MailHtmlStructure::Native
    } else {
        MailHtmlStructure::PlainEquivalent
    };
    let native_fragment =
        (structure == MailHtmlStructure::Native).then(|| sanitize_native_mail_html(source));

    SanitizedMailHtml {
        fragment,
        native_fragment,
        has_remote_images: has_remote_images.load(Ordering::Relaxed) || has_remote_css_image,
        structure,
    }
}

fn strip_title_elements(source: &str) -> Cow<'_, str> {
    let lower = source.to_ascii_lowercase();
    let mut ranges = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = lower[cursor..].find("<title") {
        let start = cursor + relative_start;
        let name_end = start + "<title".len();
        if !lower
            .as_bytes()
            .get(name_end)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>')
        {
            cursor = name_end;
            continue;
        }
        let Some(open_end_relative) = lower[name_end..].find('>') else {
            break;
        };
        let content_start = name_end + open_end_relative + 1;
        let Some(close_start_relative) = lower[content_start..].find("</title") else {
            break;
        };
        let close_start = content_start + close_start_relative;
        let Some(close_end_relative) = lower[close_start..].find('>') else {
            break;
        };
        let end = close_start + close_end_relative + 1;
        ranges.push(start..end);
        cursor = end;
    }

    if ranges.is_empty() {
        return Cow::Borrowed(source);
    }
    let removed_bytes = ranges.iter().map(|range| range.len()).sum::<usize>();
    let mut output = String::with_capacity(source.len().saturating_sub(removed_bytes));
    let mut copied_until = 0;
    for range in ranges {
        output.push_str(&source[copied_until..range.start]);
        copied_until = range.end;
    }
    output.push_str(&source[copied_until..]);
    Cow::Owned(output)
}

fn sanitize_native_mail_html(source: &str) -> String {
    let mut builder = Builder::default();
    builder
        .tags(HashSet::from([
            "a",
            "abbr",
            "b",
            "blockquote",
            "br",
            "cite",
            "code",
            "del",
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
            "img",
            "kbd",
            "li",
            "mark",
            "ol",
            "p",
            "pre",
            "q",
            "s",
            "samp",
            "small",
            "span",
            "strong",
            "sub",
            "sup",
            "table",
            "tbody",
            "td",
            "tfoot",
            "th",
            "thead",
            "time",
            "tr",
            "u",
            "ul",
            "var",
            "wbr",
            "col",
            "colgroup",
        ]))
        .tag_attributes(HashMap::from([
            ("a", HashSet::from(["href", "name"])),
            ("img", HashSet::from(["src", "alt"])),
            ("ol", HashSet::from(["start", "reversed"])),
            ("li", HashSet::from(["value"])),
            ("td", HashSet::from(["colspan", "rowspan", "headers"])),
            (
                "th",
                HashSet::from(["colspan", "rowspan", "headers", "scope"]),
            ),
            ("col", HashSet::from(["span"])),
            ("colgroup", HashSet::from(["span"])),
        ]))
        .generic_attributes(HashSet::from(["dir", "lang", "title"]))
        .clean_content_tags(HashSet::from(["script", "style"]))
        .url_schemes(HashSet::from(["data", "http", "https", "mailto"]))
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true)
        .attribute_filter(|element, attribute, value| {
            if (element, attribute) == ("img", "src")
                && is_data_url(value)
                && !is_safe_image_data_url(value)
            {
                return None;
            }
            if (element, attribute) == ("a", "href") && is_data_url(value) {
                return None;
            }
            Some(value.into())
        });
    builder.clean(source).to_string()
}

fn analyze_html(fragment: &str) -> HtmlAnalysis {
    let lower = fragment.to_ascii_lowercase();
    let mut analysis = HtmlAnalysis::default();
    let mut offset = 0;
    let mut depth = 0usize;

    while let Some(relative_start) = lower[offset..].find('<') {
        let start = offset + relative_start;
        let Some(relative_end) = lower[start + 1..].find('>') else {
            break;
        };
        let end = start + 1 + relative_end;
        let token = lower[start + 1..end].trim();
        offset = end + 1;

        if token.is_empty() || token.starts_with('!') || token.starts_with('?') {
            continue;
        }
        if token.starts_with('/') {
            let closing_name = token[1..]
                .trim_start()
                .chars()
                .take_while(|character| character.is_ascii_alphanumeric() || *character == '-')
                .collect::<String>();
            if closing_name == "table" {
                analysis.table_depth = analysis.table_depth.saturating_sub(1);
            }
            depth = depth.saturating_sub(1);
            continue;
        }

        let name = token
            .trim_start()
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '-')
            .collect::<String>();
        if name.is_empty() {
            continue;
        }

        analysis.elements += 1;
        if name == "table" {
            analysis.tables += 1;
            analysis.table_depth += 1;
            analysis.max_table_depth = analysis.max_table_depth.max(analysis.table_depth);
        }
        if name == "tr" {
            analysis.table_rows += 1;
        }
        if matches!(name.as_str(), "td" | "th") {
            analysis.table_cells += 1;
            if ["colspan", "rowspan"].iter().any(|attribute| {
                attribute_value(token, attribute).is_some_and(|value| value.trim() != "1")
            }) {
                analysis.merged_table_cells += 1;
            }
        }
        if name == "img" {
            analysis.images += 1;
            if let Some(source) = attribute_value(token, "src")
                && is_data_url(source)
            {
                analysis.inline_image_bytes += source.len();
            }
        }
        if name == "style" {
            analysis.style_blocks += 1;
        }
        if attribute_value(token, "class").is_some() || attribute_value(token, "id").is_some() {
            analysis.has_styling_hooks = true;
        }
        if is_meaningful_semantic_tag(&name) {
            analysis.has_meaningful_semantics = true;
        }
        if name == "picture" {
            analysis.has_blocking_layout = true;
        }
        if attribute_value(token, "background").is_some()
            || attribute_value(token, "bgcolor").is_some()
        {
            analysis.has_blocking_layout = true;
        }
        if name != "img" {
            if attribute_value(token, "width").is_some()
                || attribute_value(token, "height").is_some()
            {
                analysis.has_sizing_layout = true;
            }
            if let Some(style) = attribute_value(token, "style") {
                let style = analyze_inline_style(style);
                analysis.has_sizing_layout |= style.has_sizing_layout;
                analysis.has_blocking_layout |= style.has_blocking_layout;
            }
        }

        if !is_void_tag(&name) && !token.ends_with('/') {
            depth += 1;
            analysis.max_depth = analysis.max_depth.max(depth);
        }
    }

    analysis
}

fn attribute_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut offset = 0;
    while let Some(relative) = tag[offset..].find(name) {
        let start = offset + relative;
        let before = tag[..start].chars().next_back();
        let after = tag[start + name.len()..].chars().next();
        if before.is_some_and(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_'))
            || after
                .is_some_and(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_'))
        {
            offset = start + name.len();
            continue;
        }
        let rest = tag[start + name.len()..].trim_start();
        let rest = rest.strip_prefix('=')?.trim_start();
        let quote = rest.chars().next()?;
        if matches!(quote, '\'' | '"') {
            let value = &rest[quote.len_utf8()..];
            return value.find(quote).map(|end| &value[..end]);
        }
        let end = rest
            .find(|value: char| value.is_ascii_whitespace() || value == '>')
            .unwrap_or(rest.len());
        return Some(&rest[..end]);
    }
    None
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct InlineStyleAnalysis {
    has_sizing_layout: bool,
    has_blocking_layout: bool,
}

fn analyze_inline_style(style: &str) -> InlineStyleAnalysis {
    let mut analysis = InlineStyleAnalysis::default();
    for declaration in style.split(';') {
        let Some((property, value)) = declaration.split_once(':') else {
            continue;
        };
        let property = property.trim();
        let value = value.trim();
        if matches!(
            property,
            "width" | "min-width" | "max-width" | "height" | "min-height" | "max-height"
        ) {
            analysis.has_sizing_layout = true;
        }
        if matches!(
            property,
            "background"
                | "background-color"
                | "background-image"
                | "position"
                | "float"
                | "transform"
                | "grid"
                | "grid-template"
                | "grid-template-columns"
                | "grid-template-rows"
                | "flex"
                | "flex-flow"
        ) || (property == "display"
            && matches!(
                value,
                "flex" | "inline-flex" | "grid" | "inline-grid" | "table" | "inline-table"
            ))
        {
            analysis.has_blocking_layout = true;
        }
    }
    analysis
}

fn contains_start_tag(source: &str, tag: &str) -> bool {
    let needle = format!("<{tag}");
    source.match_indices(&needle).any(|(start, _)| {
        source[start + needle.len()..]
            .chars()
            .next()
            .is_some_and(|next| next.is_ascii_whitespace() || matches!(next, '>' | '/'))
    })
}

fn is_meaningful_semantic_tag(tag: &str) -> bool {
    matches!(
        tag,
        "a" | "abbr"
            | "b"
            | "blockquote"
            | "cite"
            | "code"
            | "del"
            | "em"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
            | "i"
            | "img"
            | "kbd"
            | "li"
            | "mark"
            | "ol"
            | "pre"
            | "q"
            | "s"
            | "samp"
            | "small"
            | "strong"
            | "sub"
            | "sup"
            | "time"
            | "u"
            | "ul"
            | "var"
    )
}

fn is_void_tag(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn is_remote_url(value: &str) -> bool {
    let value = value.trim_start();
    value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        || value
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
}

fn is_data_url(value: &str) -> bool {
    value
        .trim_start()
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
}

fn is_safe_image_data_url(value: &str) -> bool {
    let value = value.trim_start();
    [
        "data:image/gif;",
        "data:image/jpeg;",
        "data:image/png;",
        "data:image/webp;",
    ]
    .iter()
    .any(|prefix| {
        value
            .get(..prefix.len())
            .is_some_and(|value_prefix| value_prefix.eq_ignore_ascii_case(prefix))
    })
}

fn split_plain_reply(text: &str, has_reply_headers: bool) -> Option<Vec<PlainBodySegment>> {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.len() < 2 {
        return None;
    }

    if let Some(separator) = lines
        .iter()
        .position(|line| is_strong_reply_separator(line))
    {
        let authored = lines[..separator].join("\n").trim().to_owned();
        let (quoted, quote_metadata) = extract_quoted_metadata(&lines[separator + 1..]);
        if !authored.is_empty() && quoted.iter().any(|line| !line.trim().is_empty()) {
            let mut segments = vec![PlainBodySegment {
                kind: MailBodySegmentKind::Authored,
                content: authored,
                quote_depth: 0,
                confidence: MailBodySegmentConfidence::High,
                quote_metadata: None,
            }];
            append_quoted_history(quoted, 1, quote_metadata, &mut segments);
            if segments.len() > 1 {
                return Some(segments);
            }
        }
    }

    if has_reply_headers && let Some(separator) = find_outlook_header_boundary(&lines) {
        let authored = lines[..separator].join("\n").trim().to_owned();
        let (quoted_lines, quote_metadata) = extract_outlook_metadata(&lines[separator..]);
        let quoted = quoted_lines.join("\n").trim().to_owned();
        if !authored.is_empty() && !quoted.is_empty() {
            return Some(vec![
                PlainBodySegment {
                    kind: MailBodySegmentKind::Authored,
                    content: authored,
                    quote_depth: 0,
                    confidence: MailBodySegmentConfidence::High,
                    quote_metadata: None,
                },
                PlainBodySegment {
                    kind: MailBodySegmentKind::Quoted,
                    content: quoted,
                    quote_depth: 1,
                    confidence: MailBodySegmentConfidence::High,
                    quote_metadata,
                },
            ]);
        }
    }

    split_prefixed_plain_quotes(&lines, has_reply_headers)
}

fn append_quoted_history(
    lines: &[&str],
    depth: u8,
    metadata: Option<MailBodySegmentMetadata>,
    segments: &mut Vec<PlainBodySegment>,
) {
    let mut remaining = lines;
    let mut current_depth = depth;
    let mut current_metadata = metadata;

    loop {
        let content = remaining.join("\n").trim().to_owned();
        if content.is_empty() {
            return;
        }

        let Some(separator) = remaining
            .iter()
            .position(|line| is_strong_reply_separator(line))
        else {
            segments.push(PlainBodySegment {
                kind: MailBodySegmentKind::Quoted,
                content,
                quote_depth: current_depth,
                confidence: MailBodySegmentConfidence::High,
                quote_metadata: current_metadata,
            });
            return;
        };

        let current = remaining[..separator].join("\n").trim().to_owned();
        if !current.is_empty() {
            segments.push(PlainBodySegment {
                kind: MailBodySegmentKind::Quoted,
                content: current,
                quote_depth: current_depth,
                confidence: MailBodySegmentConfidence::High,
                quote_metadata: current_metadata.take(),
            });
            current_depth = current_depth.saturating_add(1);
        }
        let (nested, nested_metadata) = extract_quoted_metadata(&remaining[separator + 1..]);
        remaining = nested;
        if nested_metadata.is_some() || current_metadata.is_none() {
            current_metadata = nested_metadata;
        }
    }
}

fn is_strong_reply_separator(line: &str) -> bool {
    let normalized = line
        .trim()
        .trim_matches(|character: char| {
            character.is_ascii_whitespace() || matches!(character, '-' | '_' | '—' | '–')
        })
        .trim()
        .to_lowercase();
    normalized == "回复的原邮件"
        || normalized == "原始邮件"
        || normalized == "original message"
        || normalized == "forwarded message"
        || normalized == "begin forwarded message"
}

fn extract_quoted_metadata<'a>(
    lines: &'a [&'a str],
) -> (&'a [&'a str], Option<MailBodySegmentMetadata>) {
    let mut start = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());
    if start >= lines.len() {
        return (&lines[start..], None);
    }
    let mut metadata = MailBodySegmentMetadata::default();
    if lines[start].trim_start().starts_with('|') {
        while start < lines.len() && lines[start].trim_start().starts_with('|') {
            parse_table_metadata_line(lines[start], &mut metadata);
            start += 1;
        }
        while start < lines.len() && lines[start].trim().is_empty() {
            start += 1;
        }
    }
    (&lines[start..], (!metadata.is_empty()).then_some(metadata))
}

fn parse_table_metadata_line(line: &str, metadata: &mut MailBodySegmentMetadata) {
    let cells = line
        .trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect::<Vec<_>>();
    if cells.len() < 2 {
        return;
    }
    assign_quote_metadata(metadata, cells[0], &cells[1..].join(" | "));
}

fn assign_quote_metadata(metadata: &mut MailBodySegmentMetadata, raw_label: &str, raw_value: &str) {
    let label = raw_label
        .trim()
        .trim_end_matches([':', '：'])
        .trim()
        .to_lowercase();
    let limit = if matches!(label.as_str(), "主题" | "subject") {
        240
    } else {
        360
    };
    let value = clean_metadata_value(raw_value, limit);
    if value.is_none() {
        return;
    }
    match label.as_str() {
        "主题" | "subject" => metadata.subject = value,
        "发件人" | "发信人" | "from" => metadata.sender = value,
        "收件人" | "to" => metadata.recipient = value,
        "发送日期" | "日期" | "时间" | "sent" | "date" => metadata.sent_at = value,
        _ => {}
    }
}

fn clean_metadata_value(value: &str, max_chars: usize) -> Option<String> {
    let value = value
        .chars()
        .filter(|character| !character.is_control() || character.is_whitespace())
        .take(max_chars)
        .collect::<String>();
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn find_outlook_header_boundary(lines: &[&str]) -> Option<usize> {
    lines.iter().enumerate().find_map(|(index, line)| {
        if !has_header_label(line, &["from", "发件人"]) {
            return None;
        }
        let window = &lines[index..lines.len().min(index + 8)];
        let has_destination = window
            .iter()
            .any(|line| has_header_label(line, &["to", "收件人"]));
        let has_subject = window
            .iter()
            .any(|line| has_header_label(line, &["subject", "主题"]));
        (has_destination && has_subject).then_some(index)
    })
}

fn has_header_label(line: &str, labels: &[&str]) -> bool {
    let line = line.trim_start().to_lowercase();
    labels.iter().any(|label| {
        line.strip_prefix(label).is_some_and(|rest| {
            rest.trim_start().starts_with(':') || rest.trim_start().starts_with('：')
        })
    })
}

fn extract_outlook_metadata<'a>(
    lines: &'a [&'a str],
) -> (&'a [&'a str], Option<MailBodySegmentMetadata>) {
    let mut start = 0;
    let mut metadata = MailBodySegmentMetadata::default();
    while start < lines.len() && !lines[start].trim().is_empty() {
        if let Some((label, value)) = lines[start].split_once([':', '：']) {
            assign_quote_metadata(&mut metadata, label, value);
        }
        start += 1;
    }
    while start < lines.len() && lines[start].trim().is_empty() {
        start += 1;
    }
    (&lines[start..], (!metadata.is_empty()).then_some(metadata))
}

fn split_prefixed_plain_quotes(
    lines: &[&str],
    has_reply_headers: bool,
) -> Option<Vec<PlainBodySegment>> {
    let first_quoted = lines.iter().position(|line| quote_depth(line) > 0)?;
    let intro = previous_nonempty(lines, first_quoted).filter(|(_, line)| is_reply_intro(line));
    if !has_reply_headers && intro.is_none() {
        return None;
    }

    let mut quoted = lines
        .iter()
        .map(|line| (!line.trim().is_empty()).then(|| quote_depth(line) > 0))
        .collect::<Vec<_>>();
    if let Some((index, _)) = intro {
        quoted[index] = Some(true);
    }
    for index in 0..quoted.len() {
        if quoted[index].is_some() {
            continue;
        }
        let before = quoted[..index].iter().rev().flatten().next().copied();
        let after = quoted[index + 1..].iter().flatten().next().copied();
        quoted[index] = Some(matches!((before, after), (Some(true), Some(true))));
    }

    let mut segments = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let is_quoted = quoted[start].unwrap_or(false);
        let mut end = start + 1;
        while end < lines.len() && quoted[end].unwrap_or(false) == is_quoted {
            end += 1;
        }
        let source = &lines[start..end];
        let content = if is_quoted {
            strip_quote_prefix(source)
        } else {
            source.join("\n").trim().to_owned()
        };
        if !content.is_empty() {
            segments.push(PlainBodySegment {
                kind: if is_quoted {
                    MailBodySegmentKind::Quoted
                } else {
                    MailBodySegmentKind::Authored
                },
                content,
                quote_depth: if is_quoted {
                    minimum_quote_depth(source).max(1)
                } else {
                    0
                },
                confidence: MailBodySegmentConfidence::Medium,
                quote_metadata: None,
            });
        }
        start = end;
    }
    let has_authored = segments
        .iter()
        .any(|segment| segment.kind == MailBodySegmentKind::Authored);
    let has_quote = segments
        .iter()
        .any(|segment| segment.kind == MailBodySegmentKind::Quoted);
    (has_authored && has_quote).then_some(segments)
}

fn previous_nonempty<'a>(lines: &'a [&'a str], before: usize) -> Option<(usize, &'a str)> {
    lines[..before]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| (index, *line))
}

fn is_reply_intro(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    (lower.starts_with("on ") && lower.ends_with("wrote:"))
        || lower.ends_with("写道：")
        || lower.ends_with("写道:")
}

fn quote_depth(line: &str) -> u8 {
    line.trim_start()
        .chars()
        .take_while(|character| *character == '>')
        .count()
        .min(usize::from(u8::MAX)) as u8
}

fn minimum_quote_depth(lines: &[&str]) -> u8 {
    lines
        .iter()
        .map(|line| quote_depth(line))
        .filter(|depth| *depth > 0)
        .min()
        .unwrap_or(1)
}

fn strip_quote_prefix(lines: &[&str]) -> String {
    let depth = minimum_quote_depth(lines);
    lines
        .iter()
        .map(|line| {
            let mut value = line.trim_start();
            for _ in 0..depth {
                value = value.strip_prefix('>').unwrap_or(value);
            }
            value.strip_prefix(' ').unwrap_or(value)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn split_html_reply(
    source: &str,
    has_reply_headers: bool,
) -> Option<Vec<SanitizedMailBodySegment>> {
    if let Some((authored, quoted)) = split_outlook_html_suffix(source) {
        return html_segments(authored, quoted, MailBodySegmentConfidence::High);
    }

    let document = Html::parse_fragment(source);
    let strong_selector =
        Selector::parse(".ntes-mailmaster-quote, .gmail_quote, [id^=\"divRplyFwdMsg\"]")
            .expect("static provider quote selector");
    let generic_selector = Selector::parse("blockquote[type=\"cite\"], blockquote")
        .expect("static generic quote selector");
    let mut candidates = document.select(&strong_selector).collect::<Vec<_>>();
    let confidence = if candidates.is_empty() {
        if !has_reply_headers {
            return None;
        }
        candidates = document.select(&generic_selector).collect();
        MailBodySegmentConfidence::Medium
    } else {
        MailBodySegmentConfidence::High
    };
    if candidates.is_empty() {
        return None;
    }

    let candidate_ids = candidates
        .iter()
        .map(|candidate| candidate.id())
        .collect::<HashSet<_>>();
    let outer = candidates
        .into_iter()
        .filter(|candidate| {
            !candidate
                .ancestors()
                .skip(1)
                .any(|ancestor| candidate_ids.contains(&ancestor.id()))
        })
        .collect::<Vec<_>>();
    if outer.is_empty()
        || outer.iter().any(|candidate| {
            candidate.next_siblings().any(|sibling| {
                !candidate_ids.contains(&sibling.id()) && node_has_visible_content(sibling)
            })
        })
    {
        // Inline HTML replies need ordered DOM segmentation. Do not move or
        // hide content when a quote is followed by authored siblings.
        return None;
    }

    let quoted = outer
        .iter()
        .map(ElementRef::html)
        .collect::<Vec<_>>()
        .join("\n");
    let mut authored_document = document.clone();
    let sink = HtmlTreeSink::new(authored_document);
    for candidate in outer {
        sink.remove_from_parent(&candidate.id());
    }
    authored_document = sink.finish();
    let body_selector = Selector::parse("body").expect("static body selector");
    let authored = authored_document
        .select(&body_selector)
        .next()
        .map(|body| body.inner_html())
        .unwrap_or_else(|| authored_document.root_element().inner_html());
    html_segments(&authored, &quoted, confidence)
}

fn split_outlook_html_suffix(source: &str) -> Option<(&str, &str)> {
    let lower = source.to_ascii_lowercase();
    let marker = ["id=\"divrplyfwdmsg", "id='divrplyfwdmsg"]
        .iter()
        .filter_map(|needle| lower.find(needle))
        .min()?;
    let boundary = lower[..marker].rfind('<')?;
    let authored = source[..boundary].trim();
    let quoted = source[boundary..].trim();
    (!authored.is_empty() && !quoted.is_empty()).then_some((authored, quoted))
}

fn node_has_visible_content(node: ego_tree::NodeRef<'_, scraper::Node>) -> bool {
    if let Some(text) = node.value().as_text() {
        return !text.trim().is_empty();
    }
    ElementRef::wrap(node).is_some_and(|element| {
        matches!(element.value().name(), "img" | "table" | "hr")
            || element.text().any(|text| !text.trim().is_empty())
    })
}

fn html_segments(
    authored: &str,
    quoted: &str,
    confidence: MailBodySegmentConfidence,
) -> Option<Vec<SanitizedMailBodySegment>> {
    if !html_has_visible_content(authored) || !html_has_visible_content(quoted) {
        return None;
    }
    Some(vec![
        sanitize_html_segment(MailBodySegmentKind::Authored, authored, 0, confidence),
        sanitize_html_segment(MailBodySegmentKind::Quoted, quoted, 1, confidence),
    ])
}

fn html_has_visible_content(source: &str) -> bool {
    let document = Html::parse_fragment(source);
    document
        .root_element()
        .text()
        .any(|text| !text.trim().is_empty())
        || document
            .select(&Selector::parse("img, table, hr").expect("static visible selector"))
            .next()
            .is_some()
}

fn sanitize_html_segment(
    kind: MailBodySegmentKind,
    source: &str,
    quote_depth: u8,
    confidence: MailBodySegmentConfidence,
) -> SanitizedMailBodySegment {
    let sanitized = sanitize_mail_html(source);
    let (content, structure) = match sanitized.structure {
        MailHtmlStructure::PlainEquivalent => {
            (sanitize_native_mail_html(source), MailHtmlStructure::Native)
        }
        MailHtmlStructure::Native => (
            sanitized
                .native_fragment
                .unwrap_or_else(|| sanitize_native_mail_html(source)),
            MailHtmlStructure::Native,
        ),
        MailHtmlStructure::Isolated => (sanitized.fragment, MailHtmlStructure::Isolated),
    };
    SanitizedMailBodySegment {
        kind,
        content,
        is_html: true,
        structure,
        quote_depth,
        confidence,
        quote_metadata: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MailBodySegmentConfidence, MailBodySegmentKind, MailHtmlStructure, sanitize_mail_html,
        segment_mail_body,
    };

    #[test]
    fn keeps_complex_email_layout_but_removes_active_content_and_dangerous_urls() {
        let result = sanitize_mail_html(
            r#"<style>.mobile { display:none } @media(max-width:600px){.mobile{display:block}}</style>
               <script>window.top.location='https://evil.example'</script>
               <table width="640" style="color:#123"><tr><td class="mobile">Hello</td></tr></table>
               <img src="https://images.example/logo.png" onerror="alert(1)">
               <a href="javascript:alert(2)" onclick="alert(3)">unsafe</a>"#,
        );

        assert!(result.fragment.contains("<style>"));
        assert!(result.fragment.contains("@media"));
        assert!(result.fragment.contains("<table"));
        assert!(result.fragment.contains("style=\"color:#123\""));
        assert!(!result.fragment.contains("<script"));
        assert!(!result.fragment.contains("onerror"));
        assert!(!result.fragment.contains("onclick"));
        assert!(!result.fragment.contains("javascript:"));
        assert!(result.has_remote_images);
        assert_eq!(result.structure, MailHtmlStructure::Isolated);
        assert!(result.native_fragment.is_none());
    }

    #[test]
    fn native_html_keeps_semantics_but_drops_sender_styles_and_unsafe_data() {
        let result = sanitize_mail_html(
            r#"<div class="signature"><strong style="color:red">Myo</strong>
               <a href="https://paa.moe" onclick="alert(1)">myo@paa.moe</a>
               <img alt="avatar" width="240" style="width:240px" src="data:image/png;base64,AQID">
               <img alt="unsafe" src="data:image/svg+xml;base64,PHN2Zz48L3N2Zz4="></div>"#,
        );

        assert_eq!(result.structure, MailHtmlStructure::Native);
        assert!(!result.has_remote_images);
        let native = result.native_fragment.expect("native fragment");
        assert!(native.contains("<strong>Myo</strong>"));
        assert!(native.contains("href=\"https://paa.moe\""));
        assert!(native.contains("data:image/png;base64,AQID"));
        assert!(!native.contains("class="));
        assert!(!native.contains("style="));
        assert!(!native.contains("width="));
        assert!(!native.contains("onclick"));
        assert!(!native.contains("image/svg+xml"));
    }

    #[test]
    fn plain_wrappers_use_the_native_text_reader() {
        let result = sanitize_mail_html(r#"<div>Hello there</div><p>A short reply.</p>"#);

        assert_eq!(result.structure, MailHtmlStructure::PlainEquivalent);
        assert!(result.native_fragment.is_none());
    }

    #[test]
    fn wrapper_volume_does_not_turn_plain_equivalent_html_into_an_iframe() {
        let mut source = String::new();
        for _ in 0..48 {
            source.push_str("<div>");
        }
        source.push_str("A long plain message<br>with line breaks");
        for _ in 0..48 {
            source.push_str("</div>");
        }

        let result = sanitize_mail_html(&source);

        assert_eq!(result.structure, MailHtmlStructure::PlainEquivalent);
        assert!(result.native_fragment.is_none());
    }

    #[test]
    fn unused_generic_style_wrapper_can_degrade_to_native_semantics() {
        let result = sanitize_mail_html(
            r#"<style>.unused { background:#fff; display:grid; width:900px }</style>
               <p>Automatic notice</p><a href="https://example.com">Contact</a>"#,
        );

        assert_eq!(result.structure, MailHtmlStructure::Native);
        let native = result.native_fragment.expect("native fragment");
        assert!(!native.contains("<style"));
        assert!(native.contains("href=\"https://example.com\""));
    }

    #[test]
    fn style_dependent_dom_stays_isolated() {
        let result = sanitize_mail_html(
            r#"<style>.card { background:#fff; display:grid; width:900px }</style>
               <div class="card"><a href="https://example.com">Designed card</a></div>"#,
        );

        assert_eq!(result.structure, MailHtmlStructure::Isolated);
        assert!(result.native_fragment.is_none());
    }

    #[test]
    fn layout_attributes_and_backgrounds_remain_isolated() {
        for source in [
            r#"<div style="width:640px">fixed</div>"#,
            r#"<div style="display:grid">grid</div>"#,
            r##"<section bgcolor="#fff">background</section>"##,
            r#"<table><tr><td>layout</td></tr></table>"#,
        ] {
            assert_eq!(
                sanitize_mail_html(source).structure,
                MailHtmlStructure::Isolated,
                "source should remain isolated: {source}",
            );
        }
    }

    #[test]
    fn bounded_images_are_native_but_large_image_galleries_are_isolated() {
        let bounded = sanitize_mail_html(
            r#"<p>Hello <a href="https://example.com">there</a></p>
               <img src="https://images.example/one.png" alt="one">"#,
        );
        assert_eq!(bounded.structure, MailHtmlStructure::Native);
        assert!(bounded.has_remote_images);

        let gallery = sanitize_mail_html(
            r#"<p>Gallery</p><img src="https://images.example/1.png"><img src="https://images.example/2.png"><img src="https://images.example/3.png"><img src="https://images.example/4.png">"#,
        );
        assert_eq!(gallery.structure, MailHtmlStructure::Isolated);
    }

    #[test]
    fn small_signature_table_degrades_to_the_native_reader() {
        let result = sanitize_mail_html(
            r#"<div id="mail" style="width:640px;color:#333">
               <table class="signature" width="640" border="0" style="border-collapse:collapse">
               <colgroup><col width="72"><col></colgroup><tbody><tr>
               <td style="width:72px;min-width:72px"><img alt="avatar" width="64" height="64" src="data:image/png;base64,AQID"></td>
               <td style="min-width:160px"><b>Myo</b><br><a href="https://paa.moe">myo@paa.moe</a></td>
               </tr></tbody></table><i>A short signature.</i></div>"#,
        );

        assert_eq!(result.structure, MailHtmlStructure::Native);
        let native = result.native_fragment.expect("native signature");
        assert!(native.contains("<table>"));
        assert!(native.contains("<td>"));
        assert!(native.contains("data:image/png;base64,AQID"));
        assert!(!native.contains("class="));
        assert!(!native.contains("style="));
        assert!(!native.contains("width="));
        assert!(!native.contains("border="));
    }

    #[test]
    fn marketing_and_layout_dependent_tables_stay_isolated() {
        for source in [
            r#"<table><tr><td><strong>One</strong><table><tr><td>Nested</td></tr></table></td></tr></table>"#,
            r#"<table><tr><td><strong>1</strong></td></tr><tr><td>2</td></tr><tr><td>3</td></tr><tr><td>4</td></tr><tr><td>5</td></tr></table>"#,
            r##"<table bgcolor="#fff"><tr><td><strong>Card</strong></td></tr></table>"##,
            r#"<style>.signature{display:grid}</style><table class="signature"><tr><td><strong>Styled</strong></td></tr></table>"#,
        ] {
            assert_eq!(
                sanitize_mail_html(source).structure,
                MailHtmlStructure::Isolated,
                "table should remain isolated: {source}",
            );
        }
    }

    #[test]
    fn xhtml_document_titles_never_become_visible_body_copy() {
        let result = sanitize_mail_html(
            r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN"
               "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd">
               <html xmlns="http://www.w3.org/1999/xhtml"><head>
               <meta charset="utf-8"><title>Repeated subject</title>
               <style>.mail { color: #123; }</style>
               </head><body><table><tr><td>Actual body</td></tr></table></body></html>"#,
        );

        assert!(!result.fragment.contains("Repeated subject"));
        assert!(result.fragment.contains("Actual body"));
        assert_eq!(result.structure, MailHtmlStructure::Isolated);
    }

    #[test]
    fn netease_plain_reply_becomes_a_nested_collapsible_history() {
        let text = "Thanks for the thoughtful reply.\n\nAll the best.\n\n---- 回复的原邮件 ----\n| 发件人 | sender@example.com |\n| 日期 | 2026-07-01 |\n| 收件人 | receiver@example.com |\n| 主题 | Earlier note |\nOriginal message body.\n\n---- 回复的原邮件 ----\n| 发件人 | receiver@example.com |\nOlder nested quote.";

        let segments = segment_mail_body(Some(text), None, false);

        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].kind, MailBodySegmentKind::Authored);
        assert_eq!(segments[0].quote_depth, 0);
        assert!(segments[0].content.starts_with("Thanks for"));
        assert_eq!(segments[1].kind, MailBodySegmentKind::Quoted);
        assert_eq!(segments[1].quote_depth, 1);
        assert_eq!(segments[1].confidence, MailBodySegmentConfidence::High);
        assert_eq!(segments[1].content, "Original message body.");
        let first_metadata = segments[1]
            .quote_metadata
            .as_ref()
            .expect("first quoted message metadata");
        assert_eq!(first_metadata.subject.as_deref(), Some("Earlier note"));
        assert_eq!(first_metadata.sender.as_deref(), Some("sender@example.com"));
        assert_eq!(
            first_metadata.recipient.as_deref(),
            Some("receiver@example.com")
        );
        assert_eq!(first_metadata.sent_at.as_deref(), Some("2026-07-01"));
        assert!(!segments[1].content.starts_with('|'));
        assert!(!segments[1].content.contains("回复的原邮件"));
        assert_eq!(segments[2].kind, MailBodySegmentKind::Quoted);
        assert_eq!(segments[2].quote_depth, 2);
        assert_eq!(segments[2].content, "Older nested quote.");
        assert_eq!(
            segments[2]
                .quote_metadata
                .as_ref()
                .and_then(|metadata| metadata.sender.as_deref()),
            Some("receiver@example.com")
        );
        assert!(!segments[2].content.starts_with('|'));
    }

    #[test]
    fn long_netease_history_is_split_without_an_arbitrary_depth_limit() {
        let mut text = String::from("Newest authored reply.");
        for index in 1..=16 {
            text.push_str(&format!(
                "\n\n---- 回复的原邮件 ----\n| 发件人 | sender{index}@example.com |\nQuoted message {index}."
            ));
        }

        let segments = segment_mail_body(Some(&text), None, false);

        assert_eq!(segments.len(), 17);
        assert_eq!(segments[16].kind, MailBodySegmentKind::Quoted);
        assert_eq!(segments[16].quote_depth, 16);
        assert_eq!(segments[16].content, "Quoted message 16.");
    }

    #[test]
    fn prefixed_plain_quotes_keep_inline_authored_replies_in_order() {
        let text = "My opening reply.\n\nOn Monday, Alex wrote:\n> First question?\n\nMy inline answer.\n\n> Second question?";

        let segments = segment_mail_body(Some(text), None, true);

        assert_eq!(segments.len(), 4);
        assert_eq!(segments[0].kind, MailBodySegmentKind::Authored);
        assert_eq!(segments[1].kind, MailBodySegmentKind::Quoted);
        assert_eq!(segments[2].kind, MailBodySegmentKind::Authored);
        assert_eq!(segments[3].kind, MailBodySegmentKind::Quoted);
        assert!(segments[1].content.contains("First question?"));
        assert!(!segments[1].content.contains("> First"));
    }

    #[test]
    fn gmail_html_only_reply_uses_dom_quote_boundary() {
        let html = r#"<div>My short reply.</div>
            <div class="gmail_quote"><div>On Monday, Alex wrote:</div>
            <blockquote><p>Original <strong>message</strong>.</p></blockquote></div>"#;

        let segments = segment_mail_body(None, Some(html), true);

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].kind, MailBodySegmentKind::Authored);
        assert!(segments[0].is_html);
        assert!(segments[0].content.contains("My short reply"));
        assert_eq!(segments[1].kind, MailBodySegmentKind::Quoted);
        assert!(segments[1].content.contains("Original"));
        assert_eq!(segments[1].confidence, MailBodySegmentConfidence::High);
    }

    #[test]
    fn outlook_html_only_reply_treats_the_reply_header_as_a_suffix_boundary() {
        let html = r#"<html><body><div>My Outlook reply.</div><hr>
            <div id="divRplyFwdMsg"><b>From:</b> sender@example.com<br>
            <b>To:</b> receiver@example.com<br><b>Subject:</b> Earlier note</div>
            <div>Original Outlook body.</div></body></html>"#;

        let segments = segment_mail_body(None, Some(html), true);

        assert_eq!(segments.len(), 2);
        assert!(segments[0].content.contains("My Outlook reply"));
        assert_eq!(segments[1].kind, MailBodySegmentKind::Quoted);
        assert!(segments[1].content.contains("Original Outlook body"));
        assert_eq!(segments[1].confidence, MailBodySegmentConfidence::High);
    }

    #[test]
    fn generic_blockquote_uses_medium_confidence_only_for_a_known_reply() {
        let html = r#"<p>My reply.</p><blockquote type="cite"><p>Earlier body.</p></blockquote>"#;

        let reply_segments = segment_mail_body(None, Some(html), true);
        let standalone_segments = segment_mail_body(None, Some(html), false);

        assert_eq!(reply_segments.len(), 2);
        assert_eq!(
            reply_segments[1].confidence,
            MailBodySegmentConfidence::Medium
        );
        assert!(standalone_segments.is_empty());
    }

    #[test]
    fn ordinary_html_is_not_split_without_quote_evidence() {
        let segments = segment_mail_body(
            Some("A greater-than sign is ordinary prose: 2 > 1."),
            Some("<p>A greater-than sign is ordinary prose: 2 &gt; 1.</p>"),
            false,
        );

        assert!(segments.is_empty());
    }
}
