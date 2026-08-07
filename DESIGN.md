# Mine Mail Design System

This is the canonical visual and interaction specification for the Mine Mail
desktop application. This edition records the approved design already present in
the application. Screenshots and old implementation plans may illustrate it, but
they are not independent design authorities.

## Authority

- Read this file before changing visible UI, interaction, theme, copy hierarchy,
  assets, or layout.
- The approved baseline is implemented through `web/src/styles.css` and shared
  components under `web/src/components/`. Future intentional changes must update
  the implementation, relevant tests, and this document together.
- Prefer semantic tokens and shared primitives. A local exception needs a
  functional reason, not a slightly different appearance.
- Check visible changes in all four themes, affected desktop reflows, keyboard
  navigation, and reduced-motion mode.
- Keep temporary screenshots, comparison boards, and implementation plans out of
  the repository. Durable conclusions belong here; behavior belongs in
  `docs/PRODUCT.md`.

## Visual identity

Mine Mail is quiet, atmospheric, compact, and content-first.

- One painterly landscape wallpaper spans the native window. The sidebar remains
  part of that scene; mail, contacts, settings, and compose use layered frosted
  material above it.
- Glass communicates depth without turning every item into a floating card.
  Hierarchy comes from spacing, typography, surface density, and one restrained
  theme accent.
- Avoid decorative gradients, glow, heavy shadows, oversized marketing type,
  mobile-web patterns, and generic dashboard chrome.

### Brand and themes

- The fox holding an envelope at
  `web/src/assets/brand/mine-mail-fox.png` is the only Mine Mail brand mark. Use
  it for the application, tray, sidebar, onboarding, About, and Mine Mail-owned
  installer surfaces. New-mail notifications use the sender avatar.
- The sidebar owns the only shell-level Mine Mail lockup. At compact width it
  keeps the centered fox and hides the wordmark rather than wrapping it.
- The shipped themes are exactly Daylight, Night, Dusk, and Forest, backed by the
  four `web/src/assets/wallpaper-*.png` files. They share layout and component
  anatomy; only wallpaper, semantic color, material opacity, contrast, and
  optical weight vary.
- Product icons use Phosphor Icons. Do not substitute emoji, text glyphs,
  hand-drawn CSS, or one-off SVG icons.
- Avatars resolve locally: user override, built-in known-domain mark, then
  deterministic initials. Preserve brand proportions and never query a remote
  avatar service.

### Type

- UI and mail chrome use Inter Variable for Latin, Noto Sans SC Variable for
  Han, and platform sans-serif fallbacks.
- The Mine Mail wordmark uses Nunito Variable. The empty-reader quotation uses
  Ma Shan Zheng with Chinese calligraphic fallbacks.
- Sender-designed isolated mail may retain sanitized document typography.
- Page headings are about 24–31 px, section headings 19–23 px, primary content
  13–15 px, and secondary metadata 9–12 px. Frequently scanned metadata starts
  at 11 px and subdued previews at 12 px.

## Desktop shell

The primary acceptance viewport is 1440 × 900. The native minimum is
1050 × 680; narrower CSS layouts are defensive desktop reflows, not a separate
mobile or hosted Web product.

| Region | Approved geometry |
| --- | --- |
| Sidebar | `clamp(260px, 20.5vw, 340px)` |
| Mail/contact list | `clamp(390px, 29vw, 486px)` |
| Reader | flexible, minimum 480 px |
| Panel gap | `clamp(8px, 0.75vw, 13px)` |
| Outer edge | `clamp(12px, 1vw, 18px)` |
| Native top area | 38 px titlebar plus 14 px content offset |
| Panel / control / row radius | 12 / 9 / 9 px |

- Keep the undecorated Tauri window and app-owned platform-aware titlebar
  controls. Do not add a filled title strip, divider, duplicate title, or second
  logo.
- Mail and contacts use the established sidebar + list + reader composition.
- Settings keeps the sidebar and replaces the list/reader area with an embedded
  category rail and detail pane. It is not a modal settings window.
- Above 1250 px the full three-column shell is visible. At or below 1250 px the
  sidebar compacts to 78 px. At or below 940 px it becomes a wallpaper-backed
  drawer. At or below 720 px list/detail and settings use a single-pane or
  stacked defensive layout.
- The wide list column may retract. Retraction closes any open detail first,
  hides the current sidebar selection, and lets the quiet empty-reader scene use
  the remaining space. Reopening a destination restores its selection and list.

## Material and shared components

All app-owned color and material decisions flow through custom properties in
`:root` and the four theme blocks.

- Foundation tokens cover text, borders, controls, panels, accent, success,
  warning, danger, focus, geometry, and motion.
- Workspace tokens cover sidebar, list, reader, settings, compose, overlays,
  tooltips, scrollbars, and wallpaper echo.
- Do not reference undeclared tokens or add page-local palettes where a semantic
  role fits. Hard-coded color is limited to external brands, platform-standard
  controls, and sender-owned mail content.
- The list is a denser glass surface; the reader is slightly more atmospheric;
  settings uses a denser shell with a distinct rail; compose uses a floating
  writing surface; menus, tooltips, confirmations, and notifications use compact
  high-legibility material.
- Reuse `IconButton`, `TooltipTarget`, `ProfileAvatar`,
  `EditableProfileAvatar`, `ThemedSelect`, and the shared confirmation
  primitives.

### Controls and feedback

- Standard targets are at least 40 × 40 px. A compact 34 px target is reserved
  for spacious composite controls such as compose chrome; titlebar controls use
  their platform-equivalent geometry.
- Hover changes surface or edge; pressed movement is at most 1 px. Keyboard
  focus uses the shared high-contrast ring. Disabled controls remain
  recognizable and distinct from ordinary off states.
- Primary actions use the theme accent, secondary actions use neutral material,
  and destructive actions use semantic danger.
- Use `ThemedSelect`, Mine Mail dialogs, and inline validation. Never expose
  native select styling, browser validation bubbles, `alert`, `confirm`, or
  `prompt` in product UI.
- Product-owned failure copy is concise Simplified Chinese, identifies the
  source, preserves safe state, and gives the next action. Raw backend text,
  internal codes, and stack details do not appear.
- Routine success is expressed by the resulting state or nearby status. Toasts
  are reserved for failures and consequential results that would otherwise be
  invisible.

### Lists, overlays, and scrolling

- Mail, contacts, account switching, and settings navigation share row density,
  truncation, hover, focus, and a moving selection surface. Selected state uses
  both surface and edge; unread state remains semantic rather than becoming a
  second card system.
- Cached content stays mounted during background work. Loading and queued
  mutations do not replace usable rows or move the current selection.
- App chrome and navigation are not text-selectable. Text fields and opened
  message content remain selectable.
- Use the shared portal tooltip for icon-only or unfamiliar actions. Dense
  frosted popups are shared by theme selection, account menus, recipient
  suggestions, and selects.
- Each workspace owns one obvious vertical scroll surface. Avoid nested reader
  scrollbars, horizontal panel drift, and top rubber-banding. Content may have a
  bounded end inset when the last row needs breathing room.
- The desktop WebView suppresses the browser context menu. The explicit Vite
  demo may retain normal browser behavior.

## Workspaces

### Mail list and reader

- Paint cached summaries immediately. Selecting a message keeps list position
  stable while the reader shows a quiet body-loading state; never substitute the
  list preview for the opened body.
- Initial loading and explicit refresh use the compact status band below the
  tabs. Routine background synchronization stays visually quiet.
- Search identifies its local scope as **搜索已同步邮件**. Folder filters and
  result counts must not imply uncached server search.
- Inbox, Starred, Sent, Archive, and Trash append history automatically near the
  list end. While a page is loading, show one bounded end buffer with a spinner
  and **正在加载更多邮件…**. Do not add a manual load-more control or persistent
  end card.
- Starred has no **全部 / 未读** tabs. Unstarring a row keeps it in the current
  visit so the action can be undone; leaving or explicitly refreshing rebuilds
  the list from current star state.
- The reader uses one outer scrollbar. Native text and semantic HTML participate
  in that surface; complex sender HTML remains sanitized and isolated according
  to `docs/MAIL_RENDERING.md`.
- The compact header always keeps the real address available. Recipient details
  open in an overlay without moving the message body and expose only available
  From, To, Cc, and Bcc groups.
- Reader actions follow the current mailbox: Inbox can archive or move to Trash;
  Sent can move to Trash; Archive can move to Inbox or Trash; Trash can move to
  Inbox or permanently delete. Permanent deletion is visually destructive and
  always confirmed.
- Attachments appear below the body as compact shared-surface rows/cards with a
  type icon, safe name, exact or clearly approximate size, and independent save
  state. Unknown types use a generic file icon.
- Reply is the primary bottom action and Forward is secondary. Forward prepares
  the complete safe source before opening compose and never silently omits an
  attachment.

### Empty reader

- With no open message, the reader is transparent and shows the bundled rotating
  quotation scene directly on the wallpaper.
- Quotes render with the brush typeface, visible attribution, and character-level
  entrance. Long text scales rather than crops. Do not add a canvas, glow,
  gradient text, or extra background card.
- Opening mail unmounts the scene, a hidden window pauses it, and reduced-motion
  mode shows a complete static composition.

### Contacts

- Contacts reuse the sidebar + list + detail shell. Search and **全部 / 收藏**
  stay in the list surface; detail keeps the real address visible beneath any
  local remark.
- Correspondence is one continuous list with subtle dividers, not a stack of
  cards. Incoming and outgoing use distinct accessible theme tokens and the
  labels **收件箱 / 已发送**.
- Large contact sets retain the same fixed-density experience while windowing
  offscreen rows.
- Contact detail has a persistent back action. Opening correspondence reuses the
  mail reader and provides a clear return to contact history.

### Settings

- Settings is embedded beside the persistent sidebar. The category rail and
  detail pane share one glass shell; preferences save immediately and there is
  no global Save/Cancel footer.
- Account rows keep a stable height. Avatar editing starts from the avatar;
  remarks and secondary actions live in the row menu. Adding an account drills
  from provider choice into the form.
- The provider list contains 163, QQ, Gmail, and custom IMAP/SMTP. Outlook is
  absent until Modern Auth is supported. Legacy Outlook accounts remain visible
  as cache-only records.
- 163 and QQ use one composite address field with a fixed provider suffix and an
  adjacent icon-only offline tutorial action for obtaining an authorization
  code. Gmail shows the current preview-access note beside its OAuth action.
- Persistent backend health is not decoration. Show progress for the action the
  user started and failures that require attention.
- About shows the version first, then the exact active product-data directory,
  total use, one segmented composition bar, and the icon-only **更改位置**
  action. Storage migration uses the shared consequential confirmation surface
  and the platform folder picker.

### Compose

- Compose is a floating, draggable, edge-resizable writing page over the app
  scrim. The expanded form is one continuous opaque surface with integrated
  address/format rows and one inset rounded editor. Its top edge is the invisible
  drag strip; clicking the scrim minimizes it.
- Restore the last valid normal geometry and keep the page inside the visible app
  bounds. The default is a broad centered correspondence page.
- To, Cc, Bcc, subject, recipient tokens, suggestions, editor, attachment state,
  and footer share the same divider and focus language. Collapsing Cc/Bcc does
  not clear values.
- The compact format row supports the current fonts, size, emphasis, lists,
  alignment, links, and clear formatting. Controls reflect the active range or
  caret; an empty editor has no instructional body placeholder.
- The footer's icon-only stationery control exposes no paper, lined paper, and
  grid paper plus edit-only or send-with-message behavior. Every mode preserves
  editor geometry and writing origin. Paper rules scroll with authored content.
- Managed attachments show safe metadata, progress, conflict state, and
  keyboard-operable removal. Reply/forward source remains immutable and separate
  from the authored editor.
- Minimized compose is the established compact bottom-center summary bar. Its
  summary derives from subject and first recipient; the only visible close
  control sits at the far right. **写信** restores an existing minimized session
  instead of creating another.
- **保存并最小化** stabilizes authored content locally before minimizing.
  **发送** binds the visible recipients and exact draft version, closes compose
  once background sending begins, and relies on Outbox/Sent state rather than a
  second confirmation dialog.
- Each account owns at most one live compose surface. Switching accounts saves
  and hides the source session and restores it as minimized when returning.

### Notifications, confirmations, and updates

- The native new-mail surface is a compact, always-readable lower-right card. It
  uses the local-first sender avatar, subject, and receiving-account identity,
  never body preview text.
- Confirmation dialogs are compact, theme-owned, keyboard-operable, and limited
  to consequential actions such as account removal, storage migration, uncertain
  delivery decisions, and permanent deletion.
- A user-started update continues when its dialog is dismissed or Settings is
  left. The minimized bottom-right strip shows version, progress, and one
  icon-only stop action. Cancellation is unavailable once installation begins.
- Native file pickers select attachment sources/destinations and storage
  directories. Ordinary mail UI shows bounded metadata, not complete local
  paths.

## Motion, accessibility, and copy

- Normal state transitions use `--motion-fast` (120 ms) or `--motion-normal`
  (180 ms). Only meaningful window/workspace entrances use a restrained
  220–280 ms transition. Avoid spring, overshoot, and decorative motion.
- `prefers-reduced-motion` collapses transitions while preserving complete final
  state.
- Preserve semantic order, accessible names, visible focus, keyboard operation,
  readable contrast, and truncation that never hides required identity.
- Chinese product copy is concise, literal, and action-led. Helper text explains
  a consequence or next step rather than narrating the interface.

## Acceptance checklist

Before handing off a visible change:

1. Reuse or extend shared tokens/components and remove superseded rules.
2. Check Daylight, Night, Dusk, and Forest.
3. Check the affected wide and defensive desktop reflows.
4. Check relevant hover, pressed, focus, disabled, loading, empty, error, and
   long-content states.
5. Check keyboard navigation, reduced motion, scrolling, truncation, and
   contrast.
6. Run relevant React tests and the production build.
7. Keep temporary visual evidence outside the repository.
