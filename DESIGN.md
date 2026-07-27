# Mine Mail Design System

This is the canonical visual and interaction specification for the Mine Mail
desktop application. It was consolidated from the coherent UI already implemented
in the app; historical mockups, QA screenshots, and generated reference images are
not design authority.

## Authority and anti-drift rules

- Read this file before any user-visible UI change.
- After this baseline, `DESIGN.md` is normative and the shared tokens/components
  are its implementation. If they differ, either fix the accidental drift or get
  approval for a design change and update both in the same change.
- Do not promote a screenshot, generated mock, dated QA note, or one-off page to a
  source of truth. Temporary comparisons belong outside the repository.
- Extend semantic tokens and shared primitives before adding page-specific visual
  rules. A local exception must have a functional reason, not merely a slightly
  different desired appearance.
- Visual changes must be checked in all four themes, at every affected desktop
  reflow, with keyboard focus and reduced motion.

The implementation anchors are `web/src/styles.css` and the shared React
components in `web/src/components/`. They are references for implementation, not
permission to preserve dead or duplicated CSS.

## Visual character

Mine Mail is quiet, atmospheric, and content-first:

- One painterly landscape wallpaper spans the complete native window.
- The sidebar sits directly in that scene; mail, settings, and compose surfaces
  use layered, wallpaper-aware frosted material.
- Glass communicates depth without sacrificing mail readability. It is not a
  collection of floating translucent cards.
- Hierarchy comes from spacing, type, surface density, and a restrained theme
  accent—not decorative gradients, glow, excessive shadows, or animation.
- Product UI remains compact and desktop-native. Avoid generic dashboard chrome,
  oversized marketing headings, and mobile-web patterns.

## Brand, assets, type, and icons

### Brand

- The only Mine Mail mascot and brand mark is the fox holding an envelope:
  `web/src/assets/brand/mine-mail-fox.png`.
- Use the fox for the application/package icon, tray, onboarding, About, sidebar,
  and Mine Mail-owned installer surfaces. The sidebar is the only shell/header
  brand; About may show a content-level brand mark.
- New-mail notifications use the sender avatar, not the Mine Mail mark.
- Do not introduce alternate mascots, redraw the fox, mix logo variants, or use
  discussion images as runtime assets.

### Wallpaper and themes

The shipped themes are exactly:

| Theme | Runtime asset | Character |
| --- | --- | --- |
| Daylight | `web/src/assets/wallpaper-daylight.png` | cool, pale, airy |
| Night | `web/src/assets/wallpaper-night.png` | dark blue-gray |
| Dusk | `web/src/assets/wallpaper-dusk.png` | warm wine and coral |
| Forest | `web/src/assets/wallpaper-forest.png` | deep natural green |

Themes share geometry and component anatomy. A theme may override the wallpaper,
semantic colors, contrast, material opacity, wallpaper echo, and optical shadow
weight; it must not create a separate page layout or component family.

### Typography

- UI and mail chrome: `Inter Variable`, then platform sans-serif fallbacks.
- `Mine Mail` brand wordmark only: `Nunito Variable`.
- Empty-reader quotation only: bundled `Ma Shan Zheng`, then Chinese calligraphic
  fallbacks.
- Sender-designed isolated mail may retain its sanitized document typography.
- Use four practical UI ranges rather than one-off sizes:
  - display/page heading: 24–31 px;
  - section/list heading: 19–23 px;
  - body and primary row content: 13–15 px;
  - metadata, eyebrow, helper, and status text: 9–12 px.
- Body copy must remain comfortably readable. Do not use tiny text as decoration.

### Icons and avatars

- Product icons come from Phosphor Icons. Reuse the closest existing weight and
  size; do not use emoji, handmade SVG, CSS drawings, or text glyphs as icons.
- Reuse `IconButton`, `TooltipTarget`, `ProfileAvatar`, and
  `EditableProfileAvatar` instead of recreating their states.
- Avatar resolution is local-first: exact local override, known-domain brand map,
  then deterministic initials. Never query a remote avatar service.
- Known-domain brands use current, unmodified local vector marks when available.
  The reader sender tile is the reference for internal clear space. Compact
  sidebar and mail-list tiles keep the same mark-to-tile ratio; only the tile
  size changes. Preserve source proportions without cropping. Text-based legacy
  marks use a surface-relative size instead of inheriting surrounding UI type.

## Desktop shell and geometry

The window is one composition, not three independent applications.

The primary desktop acceptance viewport is 1440 × 900. The native window minimum
is 1050 × 680; smaller CSS reflows are defensive compact-window behavior, not the
primary product target.

| Token/region | Canonical geometry |
| --- | --- |
| Sidebar | `clamp(260px, 20.5vw, 340px)` |
| Mail/contact list | `clamp(390px, 29vw, 486px)` |
| Reader | flexible, minimum 480 px |
| Panel gap | `clamp(8px, 0.75vw, 13px)` |
| Outer edge | `clamp(12px, 1vw, 18px)` |
| Native top safe area | 38 px titlebar plus 14 px content offset |
| Panel / control / row radius | 12 / 9 / 9 px |

- Keep the existing app-owned Tauri titlebar: the native window stays
  undecorated, while Mine Mail renders controls that follow the host platform's
  standard position and affordances and call the Tauri window API. Do not enable
  system decorations or add a separate titlebar fill, divider, title, or logo.
- The sidebar owns the only shell-level Mine Mail brand and stays visually
  connected to the wallpaper.
- Mail uses the established three-column grid. Contacts reuse the same shell.
- Settings keeps the primary sidebar and replaces columns two and three with one
  embedded workspace: a 218 px category rail and flexible detail pane. It is not
  a modal settings window.

Desktop reflow is deliberate:

- Above 1250 px: full three-column shell.
- At or below 1250 px: compact 78 px sidebar, 350–420 px list, reader minimum
  420 px.
- At or below 940 px: the sidebar becomes a wallpaper-backed drawer and the
  content uses list + reader columns.
- At or below 720 px: list and reader become a single-pane flow; settings becomes
  a stacked rail/detail layout.

These are compact-window states for a desktop app, not a separate mobile or Web
product.

## Theme and material system

All app-owned color and material decisions flow through declared custom
properties in `:root` and the four `[data-theme]` blocks.

### Token groups

- Foundation: `--color-panel`, `--color-control`, `--color-text*`,
  `--color-border`, `--color-divider`, `--color-primary*`, semantic success and
  danger.
- Shell: wallpaper, sidebar text/scrims, native-titlebar colors, overlay.
- Material: list, reader, settings shell/rail/detail, compose shell/content,
  control surface, borders, highlight, blur, saturation, brightness, and wallpaper
  echo.
- State: hover, selection, focus ring, switches, quote cards, scrollbars.
- Semantic warning and favorite roles must have declared shared tokens before
  reuse; do not scatter raw amber/yellow values through components.
- Geometry and motion: the shared width, gap, radius, and duration tokens.

Do not reference an undeclared token. Do not add a page-local palette when an
existing semantic role fits. Hard-coded colors are limited to external brand
marks, platform-standard controls, and sender-owned mail content.

### Surface hierarchy

1. Wallpaper is continuous and most visible beneath the sidebar.
2. The mail/contact list is the quieter, denser glass surface.
3. The reader is more atmospheric and slightly more transparent, with stronger
   blur; actual mail text still gets a readable document surface.
4. Settings uses a denser shared shell, a distinct glass category rail, and a
   quiet detail layer while preserving the wallpaper.
5. Compose uses a floating glass shell with inset glass fields and editor.
6. Menus, tooltips, confirmations, and notifications use compact, denser
   theme-owned surfaces so text remains immediately legible.

The current baseline uses 24 px list blur, 30 px reader/compose blur, 1 px
theme-aware edges, a restrained inner highlight, and one shared panel shadow
family. Preserve the relative hierarchy when tuning optical values.

## Shared component language

### Controls

- Standard interactive targets are at least 40 × 40 px. Compact controls may be
  34 px only inside an already spacious composite control such as compose chrome
  or a settings row. Platform-shaped controls inside the 38 px app titlebar follow
  that titlebar's native-equivalent geometry instead.
- Hover changes surface and/or border; pressed state may move by at most 1 px.
- Keyboard focus uses the shared high-contrast focus ring and must not depend on
  hover.
- Disabled controls remain recognizable, non-interactive, and visibly distinct
  from an ordinary off state.
- Primary actions use the active theme accent. Secondary actions use a neutral
  glass/control surface. Destructive actions use the semantic danger color.

### Inputs and selection controls

- Text inputs use an inset rounded shell; the visible shell and the actual hit
  area are one focus surface.
- App-owned chrome, navigation, list rows, settings copy, dialogs, and other
  interface labels are not text-selectable. Text entry surfaces and the opened
  message's reader content remain selectable; reader controls and the decorative
  empty-reader scene do not.
- Use `ThemedSelect` for visible choices. Do not expose an operating-system select
  popup or browser validation bubble.
- Menus/listboxes own hover, active, selected, disabled, keyboard, and focus
  states. Opening one must not create a different visual system.
- Use Mine Mail-owned dialogs and inline validation instead of `alert`, `confirm`,
  or `prompt`. Show one relevant error close to the failed action.

### Lists and rows

- Mail and contact lists share topbar, search, heading, tabs, row density,
  selection edge, hover, metadata hierarchy, and truncation behavior.
- A selected row changes both surface and edge; unread state is semantic, not a
  second competing card style.
- Background refreshes preserve the visible rows and selection. Loading must not
  flash usable local content away.

### Menus, tooltips, and scrolling

- Use the shared portal tooltip for icon-only or unfamiliar actions. It appears
  after a short pointer delay, immediately for keyboard focus, stays inside the
  viewport, and never intercepts input.
- Theme pickers, account menus, recipient suggestions, and selects use the same
  dense frosted popup family.
- Each workspace owns one obvious vertical scroll surface. Do not create nested
  reader scrollbars or horizontal panel drift.
- Preserve the layer order: wallpaper, app shell, contextual drawer/banner,
  compose, consequential confirmation, native titlebar controls, toast, then
  tooltip/listbox. Do not solve a local overlap with an arbitrary new z-index.

## Workspace contracts

### Mail and reader

- The message list paints cached summaries immediately. Selecting a message keeps
  list position stable and hydrates the body silently.
- The reader has one outer scrollbar. Native text/semantic HTML uses Mine Mail
  typography; complex sender HTML remains sanitized and isolated. See
  `docs/MAIL_RENDERING.md`.
- Reply history is a sequence of sibling collapsible cards, never recursively
  nested panels.
- The bottom action row spans the reading width: primary text-and-icon reply on
  the left, secondary icon-only forward on the right.

### Empty reader

- With no selected mail, the reader becomes transparent and shows the bundled
  “future letter” quotation experience directly on the wallpaper.
- The 42-entry library plays in random order without immediate repetition.
  Quotations form one glyph at a time in the bundled brush typeface; attribution
  uses the same ink and a single continuous rule.
- Long lines reduce the shared type size rather than crop. The scene adds no
  canvas, glow, gradient text, or extra background layer.
- Opening mail unmounts the scene, a hidden window pauses it, and reduced-motion
  users receive a static completed composition.

### Contacts

- Contacts preserve the three-column shell: global navigation, contact list, and
  detail/relationship history.
- The unselected detail pane uses the current quiet, theme-owned “选择一个联系人”
  placeholder rather than inventing a new illustration or card system.
- Contact detail has a persistent back action. Opening a correspondence message
  reuses the mail reader and provides a separate return to contact history.

### Settings

- Settings is embedded across columns two and three. Keep the primary sidebar
  visible; never reintroduce the legacy centered modal, full-screen scrim, global
  Save/Cancel footer, or always-expanded account form.
- Preferences save immediately and expose only local saving/error feedback.
- Account avatar editing starts from the avatar. Secondary account actions use a
  compact icon/menu. Adding an account is a provider-first drill-in inside the
  detail pane.
- Persistent backend health is not decorative chrome. Show only explicit action
  progress and failures that require user attention.
- About keeps the compact version card first, then presents local storage as one
  quiet subsection rather than a dashboard. Show the active path, total size,
  compact category rows, and a secondary **更改位置** action using the existing
  settings material and semantic progress treatment.
- Selecting a directory uses the platform folder picker. The consequential
  restart migration uses the standard Mine Mail confirmation surface, exposes
  the chosen path with truncation and a full hover title, and disables dismissal
  only while the migration task is being scheduled.

### Compose

- Compose is a floating, draggable, edge-resizable glass work surface with only
  minimize and close in its top-right chrome.
- It restores the last valid normal geometry and remains within the visible app
  bounds.
- Address, Cc/Bcc, subject, recipient tokens/suggestions, editor, and footer share
  the inset material and focus language. Collapsing Cc/Bcc never clears values.
- Minimized compose is a 340 × 44 px subject-only bar at bottom center. The
  full-window scrim and blur disappear; the bar retains its own compact glass.
  An empty subject reads **新邮件**.

### Notifications and confirmations

- The lower-right desktop new-mail card is always readable over the desktop,
  theme-aware, compact, and more opaque than decorative glass.
- Its native surface is 388 × 148 px, always on top, non-resizable, transparent
  outside the card, and absent from the taskbar. The sender avatar is 54 px.
- It uses the sender's local-first avatar and clearly identifies the receiving
  account. Body preview text is never shown.
- Confirmation dialogs are compact, theme-owned, keyboard-operable, and reserved
  for consequential actions. Do not turn routine settings into confirmations.

## Motion, accessibility, and copy

- Normal hover/focus/state transitions use `--motion-fast` (120 ms) or
  `--motion-normal` (180 ms). Only a meaningful surface/scene entrance may use a
  restrained 220–280 ms transition.
- Respect `prefers-reduced-motion`; content and state must remain complete when
  animations collapse to effectively zero duration.
- Preserve semantic reading order, accessible names, visible focus, keyboard
  operation, and theme contrast. A screenshot can identify risks but cannot prove
  accessibility.
- Chinese product copy is concise, literal, and action-led. Helper text explains a
  consequence or next step; it does not narrate the interface.
- Keep exact user-facing labels defined by product behavior in
  `docs/PRODUCT.md`; do not fork wording in CSS, screenshots, or QA notes.

## UI change acceptance checklist

Before handing off a visible change:

1. Reuse or extend a shared component/token; remove superseded CSS instead of
   leaving a second visual implementation.
2. Check Daylight, Night, Dusk, and Forest.
3. Check the affected states above 1250 px, at 1250/940 px reflows, and at the
   720 px single-pane boundary when relevant.
4. Check default, hover, pressed, focus-visible, disabled, loading, empty, error,
   and long-content states that the component supports.
5. Check keyboard navigation, one-scrollbar behavior, reduced motion, text
   truncation, and readable contrast.
6. Run the relevant React tests and production build.
7. Keep any screenshots or comparison images in the OS temporary directory; put
   only the durable conclusion in this file or a test.
