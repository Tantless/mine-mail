# Mine Mail Visual System

This is the canonical visual specification for the Mine Mail desktop
application. It defines the reusable visual language that gives the product a
coherent identity. Product behavior, workflows, feature availability, state
transitions, defaults, and exact product copy belong in `docs/PRODUCT.md`.

## Authority and change threshold

- Read this file before changing visual identity, theme or semantic tokens,
  typography, spatial hierarchy, shell geometry, component appearance, motion,
  visual assets, or accessibility presentation.
- Update this file only when a change introduces or revises a durable visual
  rule: a new component anatomy, motion pattern, layout system, theme role,
  product-wide visual style, or other change with new visual impact.
- Adding a feature, setting, action, or sentence with existing components and
  visual patterns does not by itself change this document. Renaming ordinary UI
  copy without changing its typographic role also does not change this document.
- `docs/PRODUCT.md` owns what the product does and what durable product copy
  means. Code and tests own implementation detail. This file may name a product
  surface to locate a visual rule, but must not become a feature inventory or
  interaction specification.
- The approved baseline is implemented through `web/src/styles.css` and shared
  components under `web/src/components/`. Prefer semantic tokens and shared
  primitives. A local exception needs a functional reason, not a slightly
  different appearance.
- Check visual-system changes in all four built-in themes, representative custom
  light/dark palettes, affected desktop reflows, keyboard focus, and
  reduced-motion mode.
- Screenshots and old implementation plans may illustrate the system, but they
  are not independent design authorities. Keep temporary visual evidence out of
  the repository.

## Visual identity

Mine Mail is quiet, atmospheric, compact, and content-first.

- The shell supports two material expressions without changing its geometry:
  the default minimal expression uses calm near-solid palette surfaces, while
  the optional image expression lets one painterly landscape wallpaper span the
  native window beneath layered frosted material.
- Glass communicates depth without turning every item into a floating card.
  Hierarchy comes from spacing, typography, surface density, and one restrained
  theme accent.
- Avoid decorative gradients, glow, heavy shadows, oversized marketing type,
  mobile-web patterns, and generic dashboard chrome.

### Brand, themes, and imagery

- The fox holding an envelope at
  `web/src/assets/brand/mine-mail-fox.png` is the only Mine Mail brand mark. Use
  it for application, tray, sidebar, onboarding, About, and Mine Mail-owned
  installer surfaces; new-mail notifications use sender identity instead. Do not
  introduce alternate mascots or logo variants.
- The sidebar owns the only shell-level Mine Mail lockup. At compact width it
  keeps the centered fox and hides the wordmark rather than wrapping it.
- Daylight, Night, Dusk, and Forest are the four built-in background themes,
  backed by `web/src/assets/wallpaper-*.png`. Built-in and custom backgrounds
  share the same geometry and component anatomy. Each built-in theme retains
  its original complete palette as its image-mode default; custom backgrounds
  keep the currently selected palette. The palette remains independently
  selectable, and background selection is dormant while minimal mode is active.
- Every appearance palette is a complete visual system. App-owned canvas, panels,
  controls, text hierarchy, edges, selection, focus, correspondence, data
  visualization, and status feedback all resolve through it. Semantic states
  retain recognizable hue families while contrast and intensity follow the
  palette. The four original built-in palettes and every general-purpose palette
  must support both the near-solid minimal material and wallpaper-backed material.
- External brands, avatars, sender HTML, wallpaper pixels, and platform-owned
  surfaces remain outside theme recoloring.
- Product icons use Phosphor Icons. Do not substitute emoji, text glyphs,
  hand-drawn CSS, or one-off SVG icons.
- Avatars use local artwork or deterministic initials. Preserve source
  proportions, optical centering, and consistent mark-to-tile ratios; do not
  force transparent brand marks onto white plates.

### Typography

- UI and mail chrome use Inter Variable for Latin, Noto Sans SC Variable for
  Han, and platform sans-serif fallbacks.
- The Mine Mail wordmark uses Nunito Variable. The empty-reader quotation uses
  Ma Shan Zheng with Chinese calligraphic fallbacks.
- Sender-designed isolated mail may retain sanitized document typography.
- Page headings are about 24–31 px, section headings 19–23 px, primary content
  13–15 px, and secondary metadata 9–12 px. Frequently scanned metadata starts
  at 11 px and subdued previews at 12 px.
- Text hierarchy is expressed through size, weight, color, and spacing. Do not
  create hierarchy by adding ornamental labels or arbitrary one-off type styles.

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
- Retracting the wide list column leaves the remaining reader area visually
  balanced rather than exposing a broken two-column composition.

## Material and shared components

All app-owned color decisions flow through the complete semantic palette applied
at `:root`. Background themes select their original palette by default only in
image mode; otherwise they supply imagery and focal position. Material-mode
tokens decide whether surfaces are near-solid or wallpaper-backed.

- Foundation tokens cover text, borders, controls, panels, accent, success,
  warning, danger, focus, geometry, and motion.
- Workspace tokens cover sidebar, list, reader, settings, compose, overlays,
  tooltips, scrollbars, and wallpaper echo.
- Do not reference undeclared tokens or add page-local palettes where a semantic
  role fits. Hard-coded color is limited to external brands, platform-standard
  controls, and sender-owned mail content.
- In image mode, the list is denser glass and the reader is slightly more
  atmospheric. In minimal mode, shell regions use opaque or near-opaque palette
  surfaces with subtle borders, highlights, and restrained shadow preserving
  depth without wallpaper or decorative imagery. Settings keeps a distinct
  rail, compose keeps a floating writing surface, and compact overlays remain
  high-legibility in both modes.
- Reuse `IconButton`, `TooltipTarget`, `ProfileAvatar`,
  `EditableProfileAvatar`, `ThemedSelect`, and the shared confirmation
  primitives.

### Controls and focus

- Standard targets are at least 40 × 40 px. A compact 34 px target is reserved
  for spacious composite controls such as compose chrome; titlebar controls use
  their platform-equivalent geometry.
- Hover changes surface or edge; pressed movement is at most 1 px. Disabled
  controls remain recognizable and distinct from ordinary off states.
- Keyboard focus uses the shared high-contrast ring. Composite inputs and search
  fields own focus feedback on their outer shell; their editable child never
  draws a second ring.
- Unless a surface-specific rule requires the accent, focused input shells use
  restrained neutral inset feedback and never a primary-colored outer glow.
- Primary actions use the theme accent, secondary actions use neutral material,
  and destructive actions use semantic danger.
- Use `ThemedSelect`, Mine Mail dialogs, and inline validation. Do not expose
  native select styling, browser validation bubbles, or browser-native dialog
  styling in product UI.
- Secondary icon controls may rest on transparent fills. Their boundary and
  surface appear during hover, focus, selection, or press only when needed to
  make the target legible.

### Status and feedback surfaces

- Routine state should remain visually close to the affected control or content.
  Reserve floating surfaces for results that would otherwise be invisible.
- Standalone success and failure feedback does not repeat state with check or
  cross icons. Its material and text remain theme-owned while a semantic green
  or red edge communicates the result.
- Warning feedback uses the theme-owned yellow surface and edge without adding a
  redundant state icon.
- Embedded progress and status feedback remains borderless unless separation is
  necessary for legibility; informational and in-progress indicators may retain
  their icons. A centered empty-mailbox result may pair loading, authoritative
  success, or failure copy with a Phosphor icon, using only the palette's
  primary, success, or danger role and no card surface, border, or shadow.

### Lists, overlays, and scrolling

- Mail, contacts, account switching, and settings navigation share row density,
  truncation, hover, focus, and a moving selection surface. Selected state uses
  both surface and edge; unread state remains semantic rather than becoming a
  second card system.
- Use the shared portal tooltip for icon-only or unfamiliar actions. Dense
  frosted popups are shared by theme selection, account menus, recipient
  suggestions, and selects.
- Open selects and model-choice popovers treat the minimized compose bar as a
  lower visual boundary. They open downward, show about four choices at most,
  and reduce their height with internal scrolling rather than escaping the
  visible workspace.
- Each workspace owns one obvious vertical scroll surface. Avoid nested reader
  scrollbars, horizontal panel drift, and top rubber-banding. Content may have a
  bounded end inset when the last row needs breathing room.

## Workspace composition

These rules describe visual composition only. `docs/PRODUCT.md` determines when
a state exists, which actions are available, what they do, and what they say.

### Mail list and reader

- With cached mail present, explicit refresh feedback uses a transparent compact
  line below the list heading without replacing or moving rows. With no cached
  mail, loading, authoritative success, and failure use one compact icon-and-text
  state centered in the list. Search and filter zero-results use centered text
  without a success icon. Additional history uses one bounded end buffer rather
  than a persistent card.
- A folder with no filter tabs places its count beside the heading actions and
  collapses the absent tab row to the list divider. Folders with tabs keep the
  count and established geometry in the tab row.
- The reader uses one outer scrollbar. Native text and semantic HTML participate
  in that surface; isolated sender HTML must not introduce a competing app-level
  scrollbar.
- Reader toolbar controls are compact and content-width. Split controls use
  clearly separated segments, restrained inset focus, and stable geometry while
  their state changes.
- Reader-owned popups use an opaque theme surface above sender-controlled
  content so mail text cannot show through or visually compete with options.
- The compact header preserves identity hierarchy without moving the body.
  Secondary address detail belongs in an overlay rather than expanding the
  header in place.
- Attachments use compact shared-surface rows or cards with a file-type icon,
  safe-name region, metadata, and independent progress affordance. Unknown types
  use the generic file icon.
- Bottom reader actions form one restrained group with an obvious primary action
  and quieter secondary actions.

### Empty reader

- The empty reader is transparent and allows the wallpaper to remain the
  dominant surface. The optional quotation scene sits directly on the wallpaper
  without a card.
- Quotations use the brush typeface, visible attribution, and character-level
  entrance. Long text scales rather than crops.
- Do not add canvas effects, glow, gradient text, or a background panel to the
  quotation scene.
- Reduced motion shows the complete composition statically.

### Contacts

- Contacts reuse the sidebar + list + detail shell. Search and filtering remain
  in the list surface; identity detail keeps the real address visually available
  beneath any local presentation name.
- Correspondence is one continuous list with subtle dividers, not a stack of
  cards. Incoming and outgoing items use distinct accessible theme tokens.
- Contact lists retain the same fixed visual density regardless of list size.

### Settings

- Settings is embedded beside the persistent sidebar. The category rail and
  detail pane share one glass shell.
- Appearance presets are presented as a compact card sequence. Built-in and
  custom cards share anatomy; the selected card uses the shared primary edge and
  check treatment.
- Theme configuration uses compact disclosure rows. Palette choices use square
  tiles with circular four-part previews covering glass, accent, selection, and
  edge tones. Tray and tile surfaces stay transparent.
- Preference cards use a consistent collapsed height. Expanded content grows
  only the active card and retains a bounded bottom inset for menus and the
  minimized compose bar.
- Account rows keep a stable height. Avatars anchor identity; remarks and
  secondary controls must not increase baseline row density.
- Provider instances use compact rows with locally bundled, optically centered
  marks. The active row uses a restrained primary edge and low-profile state
  badge. Dragging changes opacity and density without introducing a second card
  style or decorative motion.
- Add/edit configuration uses an embedded child surface rather than a new modal
  visual language. Search fields follow the shared composite-input focus rule.
- About leads with version and storage summary. Composition and maintenance
  controls remain compact and subordinate to the total rather than becoming a
  dashboard.

### Compose

- Compose is a floating writing page over the app scrim. The expanded form is
  one continuous opaque surface with integrated address and format rows plus one
  inset rounded editor.
- The default geometry is a broad centered correspondence page. Resizing and
  reflow must preserve a usable writing surface and keep controls inside visible
  bounds.
- Recipient, subject, formatting, editor, attachment, and footer regions share
  the same divider, spacing, and focus language.
- The format row is compact. Controls reflect the active range without turning
  the toolbar into a dense ribbon.
- Plain and stationery editors share the same theme-owned paper surface and
  writing origin. Paper rules or grids add texture without changing the base
  paper color or editor geometry.
- AI optimization uses a compact split control. Progress changes the initiating
  icon without locking or visually replacing the writing surface.
- A completed comparison is a large theme-owned two-pane surface. Each pane
  presents subject and body as one coherent mail version; restrained success and
  danger tints distinguish additions and removals without overwhelming authored
  text.
- The AI assistant attaches to the right edge of compose on wide viewports and
  overlays within compose bounds at defensive widths. Its session list and
  selected-conversation states preserve the same header and bottom composer
  alignment.
- Session rows are compact, single-line controls. Hover and focus reveal one
  low-profile capsule surface; selected state uses a stronger primary tint in
  light themes so it remains visible against pale panels.
- Full-session browsing stays inside the assistant's original list region. Its
  framed surface follows the visible result count up to a bounded height and
  scrolls internally; it does not dim the workspace or cover the compose editor.
- The assistant composer owns one rounded focus surface. Mode and model triggers
  are transparent, content-width capsules; long model names yield and ellipsize
  before the mode control does.
- The assistant header centers its text title without a leading AI mark. Global
  and conversation controls reserve their own space so long titles never overlap
  them.
- Context usage is one muted tabular line aligned with the composer actions. It
  never grows into a progress card.
- Assistant activity is an append-only vertical trail above safe Markdown.
  Thinking and tool steps use compact rows and never compete with the answer for
  visual emphasis.
- Proposed mail changes use one or two read-only, primary-tinted cards with a
  restrained highlight edge and layered optical shadow. Long content wraps and
  scrolls vertically without creating a two-dimensional canvas.
- Managed attachments use the same compact metadata and progress language as
  reader attachments.
- Minimized compose is a compact bottom-center summary bar with one far-right
  close control. It remains a lower boundary for nearby popovers.

### Notifications, confirmations, and updates

- The Mine Mail notification surface is a compact, always-readable lower-right
  card. It uses dense, high-legibility material and visually aligns to the
  monitor work-area edges.
- Platform-owned notification surfaces retain their native geometry, material,
  attribution, and dismissal treatment. Do not imitate them with a second Mine
  Mail layer.
- Confirmation dialogs are compact, theme-owned, keyboard-operable, and visually
  proportional to the consequential action they guard.
- Long-running update progress may collapse into a compact bottom-right strip
  with version, progress, and one icon-only control.

## Motion and accessibility presentation

- Normal state transitions use `--motion-fast` (120 ms) or `--motion-normal`
  (180 ms). Only meaningful window or workspace entrances use a restrained
  220–280 ms transition.
- Avoid spring, overshoot, decorative motion, and animation that competes with
  mail content.
- `prefers-reduced-motion` collapses transitions while preserving the complete
  final visual state.
- Preserve semantic order, visible focus, readable contrast, and truncation that
  never hides required identity.
- Visual state must never rely on color alone when shape, edge, label, or
  placement can provide the second cue.

## Acceptance checklist

Before handing off a visual-system change:

1. Confirm that the change truly introduces or revises a durable visual rule. If
   it only adds product behavior or copy using existing patterns, update
   `docs/PRODUCT.md` instead of this file.
2. Reuse or extend shared tokens/components and remove superseded visual rules.
3. Check Daylight, Night, Dusk, Forest, and representative custom light/dark
   palettes.
4. Check affected wide and defensive desktop reflows.
5. Check hover, pressed, focus, disabled, loading, empty, error, and long-content
   presentation where relevant.
6. Check keyboard focus, reduced motion, scrolling, truncation, and contrast.
7. Run relevant React tests and the production build.
8. Keep temporary visual evidence outside the repository.
