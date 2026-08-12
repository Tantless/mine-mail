# Mail Rendering, Attachments, Forwarding, and Reply-History Contract

Mail is untrusted input. This document defines the durable safety and experience
boundary. `../AGENTS.md` controls architecture; `../DESIGN.md` controls the
reader's visual language.

## Ownership and data boundary

- Rust owns MIME parsing, body selection, HTML sanitization, structural
  classification, remote-image detection, reply segmentation, attachment
  indexing/extraction, forward preparation, and cached-body persistence.
- Rust may derive a list preview from a bounded, non-marking IMAP body prefix.
  The prefix is discarded after parsing and never counts as a fetched body.
- For a selected uncached message, Rust may request a bounded IMAP MIME structure
  and non-marking MIME/body sections for the chosen plain and HTML leaves. It
  must not request ordinary attachment bodies as a prerequisite for reader
  rendering.
- React receives only the bounded body representation needed by the selected
  render mode. It never receives a complete raw RFC822 message.
- Inbox/list summaries never contain full HTML or raw message source.
- Parsed cached messages retain Bcc only from an actual RFC822 `Bcc` header.
  Missing and legacy values stay empty; transport headers, the account identity,
  and SMTP envelope recipients are never used to infer Bcc.
- React receives attachment metadata and opaque attachment IDs only. Attachment
  bytes, arbitrary local paths, managed storage paths, and raw MIME parts never
  cross the desktop boundary.
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
- Preserve the compose contract's bounded font face/size, text alignment, and
  first-line indentation after validating and narrowing their values.
- Strip every other sender style, class/id hook, sizing/layout attribute, unsafe
  URL, event handler, and unsupported embedded content.
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
- Because an isolated iframe's viewport height follows its content, sender CSS
  orientation media queries use a stable portrait branch. Content height must
  not feed back into sender layout and alternate the document between portrait
  and landscape rules.
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

## AI reader translation

- Translation consumes only the currently hydrated, Rust-sanitized reader
  representation. It never fetches a raw RFC822 message, attachment bytes,
  headers, recipients, or account credentials for the model.
- Plain bodies are sent as bounded text. For native and isolated HTML, Rust
  reparses the sanitized fragment, extracts only non-empty visible text nodes,
  assigns opaque numeric positions, and sends those text values to the configured
  AI Provider. Script, style, title, template, and noscript text is excluded.
- The model should return one strict JSON translation for every supplied
  position. Rust applies every valid numbered translation to its matching text
  node and leaves missing positions unchanged, so a partial result cannot shift
  later text into the wrong location. Empty, duplicate, unknown, oversized, or
  malformed results are rejected.
  Rejection diagnostics record only the failure category and bounded structural
  counts (including expected and actual item counts), never translated or source
  text. A partial completion records its translated and missing counts and the
  reader reports them; a rejected result identifies the failed contract category
  so an intermittent Provider response can be distinguished from mail HTML parsing.
  Returned values are written back as text nodes and serialized, so model output
  cannot introduce tags, attributes, links, styles, images, or active content.
- Element order, attributes, sender styling, table/layout structure, links,
  images, remote-image policy, and the selected native/isolated render mode stay
  unchanged. Segmented replies translate each rendered segment while retaining
  its quote metadata and collapse structure.
- The translated representation is reader-only and in memory. Switching back to
  the original uses the unchanged cached representation; translation never
  rewrites MIME data, body cache rows, reply/forward sources, or search text.
  Runtime translation tasks are keyed by the stable reader message identity and
  an in-memory content-free fingerprint of the sanitized source parts. Switching
  mail therefore keeps active work and the latest successful result available until
  application exit. At most two messages run at once; additional messages wait
  in order. A per-message language override is sent with the translation request
  and does not change the persisted default. Re-translation keeps the previous
  validated representation visible until a replacement succeeds; failure leaves
  that previous result intact.
- All translation protocols use their SSE adapter internally with a 180-second
  total request limit and a 45-second between-chunk idle limit; MiMo additionally
  disables model thinking. A long plain body or visible HTML text node is split
  at a sentence, newline, or whitespace boundary into numbered units of at most
  800 UTF-8 bytes. Batches contain at most six units and normally 800 bytes. The
  scheduler starts four Provider requests, can rise to six after consecutive
  successes, reduces concurrency after a failed or partial batch, and fills an
  available slot as soon as any request completes. Each batch retains the original
  global unit IDs. Retryable missing units receive one smaller retry using at most
  two units and normally 400 bytes; an individual larger unit remains intact.
  Units still missing keep their exact original fragment
  while successful fragments are recombined into their original text node. Output
  limits are bounded from batch request size instead of always requesting the
  maximum. The reader still receives only the final validated representation;
  raw deltas are never rendered or persisted.

## Attachment indexing and extraction

- A bounded summary prefix advertises neither attachment bytes nor authoritative
  attachment metadata. An IMAP MIME structure may supply the reader's bounded
  attachment inventory before any attachment body is downloaded. A completely
  cached RFC822 message remains the authority for byte-backed local indexing and
  exact decoded sizes.
- Each ordinary attachment receives an opaque stable part ID tied to the message
  and MIME tree. A remote ID binds the current public message epoch, MIME part
  path, and bounded structure metadata without exposing the path to React. File
  name alone and display order are not identities.
- The bounded metadata is original name when present, safe display name, declared
  or detected MIME type, byte size, whether that size is approximate, and
  disposition. A transfer-encoded structure size is shown as approximate until
  the part is decoded; complete cached MIME metadata uses the exact decoded size.
  Unknown types remain generic files; they are never presented as PDF merely
  because no icon is known.
- Inline resources remain distinct from ordinary named attachments. Rendering an
  inline resource does not automatically make it downloadable or forward it as a
  separate ordinary attachment.
- Saving resolves exactly one message and part ID in Rust. A cached ID reparses
  the complete MIME. A remote ID is matched against a freshly fetched structure
  before Rust requests and decodes only that MIME part. Both paths stream only
  the selected decoded part to a platform-selected destination. React never
  supplies a source path or receives the bytes.
- Safe output names remove separators and control characters, reject platform
  reserved names, trim unsafe trailing dots/spaces, enforce a bounded length, and
  fall back to `attachment.bin`. The final path must remain inside the directory
  selected by the platform Save As flow.
- Existing files are never overwritten. Resolve collisions with a numeric suffix
  before the extension. Write a newly created temporary sibling and finalize it
  only after complete extraction; cancellation or failure removes partial output.
- If the full message is absent, saving one remote attachment does not hydrate the
  other parts or replace the already readable body. Failure leaves the reader and
  every other attachment unchanged and may be retried.

## Authored rich text and stationery

- The compose editor maintains one plain authored body and an optional
  Mine Mail-owned restricted HTML fragment. The plain body remains authoritative
  for previews, notifications, accessibility fallback, and clients that do not
  render HTML.
- React constrains editor output, but Rust sanitizes it again before persistence
  or MIME construction. The authored allowlist covers basic text semantics,
  bounded font face/size, alignment, lists, and safe HTTP(S)/mailto links. It
  accepts no scripts, event handlers, forms, images, remote resources, arbitrary
  CSS, or sender-controlled layout hooks.
- A formatted message is `multipart/alternative` with complete plain and HTML
  bodies. A plain message stays `text/plain` unless a reply context or explicitly
  sent stationery requires an HTML alternative.
- **仅编辑** persists the selected lined/grid paper for the local editor while
  leaving the outgoing body undecorated. **随信发送** wraps the sanitized authored
  fragment in Mine Mail-generated inline stationery styles so common mail clients
  can render it without external assets. Before wrapping, Rust adds a
  transport-only inline rhythm to authored paragraphs, list items, and lists:
  client default margins are removed and their line boxes stay synchronized with
  the paper rules. These compatibility declarations are discarded when an owned
  draft is restored and never become editor formatting. The paper background is
  progressive enhancement because a receiving client may remove CSS backgrounds;
  the normalized text rhythm and complete plain alternative remain readable when
  it does. The plain alternative never contains paper decoration.
- The isolated reader retains only the bounded `lined` or `grid` Mine Mail
  stationery marker and reapplies the same block rhythm when displaying older
  owned messages that predate transport normalization. This is a presentation
  repair only: cached HTML and the original RFC822 bytes are never rewritten.
  A valid marker always selects isolated rendering, including for short or
  plain-text-derived HTML; it must never enter native/plain degradation because
  those paths intentionally remove sender styles and custom attributes.
- Mine Mail draft headers and authored-boundary markers identify restricted rich
  content that the editor can round-trip. Parsing restores only the marked,
  re-sanitized authored fragment; stationery wrappers and immutable reply history
  do not become editable text. Missing or malformed ownership markers make an
  HTML draft unsupported and read-only.

## Managed outgoing attachments

- Selecting an outgoing file is a Rust-owned import, not a durable reference to
  the user's original path. Rust copies it into the product-data directory under
  account- and draft-scoped management before returning bounded metadata.
- Managed blobs are immutable and addressed through opaque IDs. Draft attachment
  associations carry the same optimistic `local_version` semantics as body and
  recipient edits; conflict copies preserve their exact associations.
- Older local rows that predate persisted SHA-256 metadata have one narrow lazy
  upgrade path: Rust reads at most the stored size plus one from the validated
  direct regular blob in the active account scope, computes SHA-256, and performs
  a one-time SQLite compare-and-set against the same account, blob ID, internal
  name, size, and still-`NULL` digest. The bytes are usable only after that bind
  succeeds. Missing, linked/reparse, changed, or concurrently disagreeing blobs
  fail integrity validation; ordinary reads always require and verify a persisted
  digest.
- Add, remove, discard, account-cache removal, and orphan cleanup operate on
  references. A blob still referenced by a draft, conflict copy, or immutable
  Outbox MIME cannot be removed.
- Confirmed SMTP success retains the immutable Outbox attachment set only while
  that row supplies the local Sent fallback. Once a synchronized provider Sent
  message has the same normalized Message-ID, Rust retires the Outbox row,
  cascades its attachment references, and lets ordinary orphan cleanup remove
  blobs that have no remaining draft or Outbox owner.
- MIME construction reads only the attachment set bound to the confirmed draft
  version and preserves the safe file name, MIME type, disposition, transfer
  encoding, and complete bytes. A newer draft version cannot change a persisted
  Outbox message.
- Mine Mail-authored plain drafts with managed ordinary attachments are editable.
  External HTML, multipart, inline-resource, or attachment-bearing drafts remain
  read-only until every MIME part can be round-tripped without loss.

## Forward preparation

- Forward preparation is a Rust operation over one fully hydrated cached message.
  It never substitutes a list preview for a complete body and never asks React to
  reconstruct sender content.
- Rust captures original subject, From, To, Cc, and time as immutable structured
  metadata. It does not infer Bcc. The user's new authored text stays separate.
- After RFC encoded-word decoding, Rust applies one bounded identity-display
  normalization to Subject, From, To, and Cc for both the plain and safe HTML
  alternatives. Header line breaks and other control characters collapse to
  ordinary spacing, Unicode bidirectional/direction controls are removed, and
  display names are quoted while the actual address stays explicit. Oversized
  display text is visibly truncated instead of discarding the complete forward;
  missing fields remain missing and no replacement identity is invented.
- The plain alternative always contains a trustworthy complete textual fallback.
  An HTML alternative may be retained only after the same sanitization and
  structural classification used by the reader; raw sender HTML never enters the
  compose state.
- Ordinary named attachments are included by default through managed opaque
  references. If any requested attachment cannot be extracted or staged, forward
  preparation fails as a whole. Omitting attachments requires a second explicit
  preparation request; no attachment is silently dropped.
- Rust assembles final identity headers, quoted body alternatives, and attachments
  when saving or sending the exact draft version. React renders the immutable
  context but does not concatenate it into the editable body.

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
Attachment cases must cover malicious and colliding names, unknown types, exact
and approximate sizes, selective body requests that exclude large attachment
sections, multi-attachment identity, cancellation, partial-write cleanup, stale
draft versions, and forward preparation with and without requested attachments.
