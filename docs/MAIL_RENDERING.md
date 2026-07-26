# Mail Rendering and Reply-History Contract

Mail is untrusted input. This document defines the durable safety and experience
boundary. `../AGENTS.md` controls architecture; `../DESIGN.md` controls the
reader's visual language.

## Ownership and data boundary

- Rust owns MIME parsing, body selection, HTML sanitization, structural
  classification, remote-image detection, reply segmentation, and cached-body
  persistence.
- React receives only the bounded body representation needed by the selected
  render mode. It never receives a complete raw RFC822 message.
- Inbox/list summaries never contain full HTML or raw message source.
- Low-confidence parsing must preserve content. A classifier may choose a safer
  presentation, but it may not silently delete ambiguous authored or quoted text.

## Body render modes

Rust selects exactly one of three structural modes in
`../web/src-tauri/src/mail_html.rs`:

### Plain-equivalent

Use Mine Mail's native themed text reader when HTML is only a readable text
wrapper with no meaningful presentation to preserve.

- Redundant wrapper count or depth alone must not force an iframe.
- Use the readable text alternative and preserve line/paragraph meaning.
- Do not carry sender CSS into the native reader.

### Native semantic HTML

Use Mine Mail's themed semantic reader for bounded markup whose meaning survives
removing sender layout and styling.

- Preserve safe semantics such as paragraphs, emphasis, links, bounded images,
  and an eligible small flat table.
- Compact, text-dominant notification/profile templates may use this mode when
  their ornamental styling is unnecessary for understanding.
- Strip sender styles, class/id hooks, sizing/layout attributes, unsafe URLs,
  event handlers, and unsupported embedded content.
- Do not describe the eligible table as “signature-only”; eligibility is based on
  bounded structure and readable meaning.

### Isolated sender HTML

Use the sanitized no-script iframe when sender-controlled structure or layout is
meaningful or exceeds the native-reader bounds.

Typical isolation triggers include style-dependent DOM, complex/nested/merged
tables, fixed or positioning layout, backgrounds, forms/media/embedded content,
large documents, image-heavy content, and structures outside the tested bounds.

- Sanitize before rendering.
- Scripts and active form behavior remain disabled.
- Keep the sender's sanitized document structure intact rather than partially
  restyling it into a broken native layout.

## Classification bounds

The exact byte, element, depth, image, table, and text-dominance thresholds are
implementation constants next to the classifier in
`../web/src-tauri/src/mail_html.rs`. Their executable contract is the adjacent
Rust test suite.

Do not copy those numeric thresholds into `AGENTS.md`, `DESIGN.md`, QA notes, or
frontend code. An intentional threshold change must:

1. explain which safe document class changes mode;
2. update classifier tests for both sides of the boundary;
3. verify plain, native, and isolated frontend rendering;
4. preserve the safety and low-confidence rules above.

## Reader scrolling and sizing

- The reader owns one outer vertical scrollbar.
- Native text/HTML participates directly in that scroll surface.
- An isolated iframe reports content height to the outer reader and must not
  create a competing vertical scrollbar.
- When isolated sender content is wider than the reader, scale the complete
  sanitized document proportionally to fit. Do not change the application's
  column layout or selectively rewrite the sender's internal structure.
- Do not allow touchpad horizontal deltas to make the app panels drift.

## Remote images and links

- Sanitization records whether a body contains remote images or remote CSS-image
  requests.
- Apply the user's automatic/ask/blocked policy before a remote request is made.
- A blocked or not-yet-approved image cannot make the surrounding body unreadable.
- Inline data images remain subject to the sanitizer's accepted formats and size
  boundaries.
- Links must use safe schemes and open through the desktop-owned path; mail HTML
  cannot execute script or navigate the application surface directly.

## Reply segmentation

- Parse replies in Rust into ordered authored and quoted segments.
- Prefer standards and explicit reply headers, followed by maintained adapters for
  common NetEase, Gmail, and Outlook structures.
- Treat uncertain boundaries as content to preserve, not content to discard.
- High-confidence history renders as sibling collapsible cards from newest to
  oldest. Never recursively nest quote cards.
- Show parsed subject, sender → recipient, and time when available; use a numbered
  fallback when metadata cannot be recovered.
- Keep an original-format fallback for content that cannot be represented safely
  by the native quote surface.

## Local history navigation

A quote card shows a separate “open local source” affordance only when all of the
following are true:

- its exact cached ancestor is known;
- that ancestor belongs to the active account;
- it is present in the currently loaded Inbox or Sent list.

The affordance opens and focuses that list row without toggling the quote card.
Unavailable, cross-account, or third-party history has no navigation affordance.

## Required verification

For a rendering-boundary change, run at minimum:

- `cd web/src-tauri && cargo test`
- `cd web && npm test -- --run`
- `cd web && npm run build`

Add focused regression cases using synthetic mail fixtures. Do not commit real
messages, raw personal RFC822, remote credentials, or screenshot-only evidence.
