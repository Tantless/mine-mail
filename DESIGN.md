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
- Composite inputs and search fields own focus feedback on their outer shell;
  their editable child never draws a second ring. Unless a surface-specific rule
  explicitly requires the accent, that shell uses restrained neutral inset
  feedback and never a primary-colored outer glow.
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
- Standalone floating success and failure feedback, including the bottom-right
  toast and top notice surfaces, does not repeat the state with check or cross
  icons. Its surface and text remain theme-owned while a semantic green or red
  edge communicates the result. A warning toast uses the theme-owned yellow
  warning surface and edge without adding a state icon. Embedded status feedback
  such as mailbox synchronization remains borderless; informational and
  in-progress indicators may retain their icons.

### Lists, overlays, and scrolling

- Mail, contacts, account switching, and settings navigation share row density,
  truncation, hover, focus, and a moving selection surface. Selected state uses
  both surface and edge; unread state remains semantic rather than becoming a
  second card system.
- Cached content stays mounted during background work. Loading and queued
  mutations do not replace usable rows or move the current selection.
- App chrome and navigation are not text-selectable. Text fields, opened message
  content, and AI assistant conversation content remain selectable; controls
  inside those regions stay inert to selection.
- Use the shared portal tooltip for icon-only or unfamiliar actions. Dense
  frosted popups are shared by theme selection, account menus, recipient
  suggestions, and selects.
- Open selects and model-choice popovers treat the minimized compose bar as a
  lower viewport boundary. They always open downward, show about four choices at
  most, and reduce their height further with internal scrolling when needed to
  remain above that bar.
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
- Incoming Inbox, Starred, Archive, and Trash mail exposes one compact split
  **AI 翻译** capsule in the reader toolbar after the body is ready. Its left
  icon runs translation and its right, content-width language trigger selects
  a language for that message without changing the persisted **AI 翻译语言**.
  The trigger uses a reader-specific bounded popup rather than reusing the
  settings-field presentation. A minimized compose bar is
  always treated as the lower popup boundary. Opening or focusing the control
  never draws a primary-colored outer selection ring around the whole capsule;
  keyboard focus is shown only as restrained inset feedback on the active
  segment. Its language popup uses an opaque theme surface and stays above the
  mail body so sender text cannot show through or intercept its options. While
  translating, the icon alone shows progress. Success replaces the split
  control with a low-profile **原文 / 译文** capsule followed by a compact
  language trigger that displays the current translated language. Selecting a
  different language starts translation immediately, keeps the previous result
  available until the replacement succeeds, and never changes the Agent default.
  The selected half uses the restrained primary tint and switching never reflows
  the toolbar. The toggle switches the header subject and body together while
  each remains in its established region; any missing translated position keeps
  its original text. If the
  Provider returns only some valid text positions, the translated view fills
  those positions, leaves missing positions in the original language, and shows
  one compact warning with the completed and total position counts. Sent, Draft,
  and Outbox content has no translation action. If translation cannot start
  because Agent configuration is incomplete, the reader uses the shared
  bottom-right yellow warning toast with the fixed copy **请先前往设置界面完成AGENT配置**
  instead of placing an error banner above the message.
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
- **桌面通知** remains the master preference. On Windows, one adjacent
  **通知方式** row uses `ThemedSelect` to choose **Mine Mail 通知** or **Windows
  通知**; the row is disabled with the master preference and absent on other
  platforms. Nearby helper copy explains that Windows presentation remains
  subject to system notification and do-not-disturb settings.
- **Agent 配置** is its own category between feature preferences and About. Its
  **模型配置** card is a compact accordion, collapsed by default. Expanding it
  shows configured Provider instances rather than raw connection fields. Each
  row contains a local brand mark, user-defined name, base URL, preferred model,
  instance-local availability and measured latency, plus **设为默认**、连接测试、编辑 and
  删除 actions. The row-level connection action only reruns the bounded connectivity
  test; protocol and capability results stay in the detail surface rather
  than adding diagnostic density to the list. The active default row uses
  a restrained primary edge and **使用中** badge. An unavailable or expired row
  owns its error copy; it never produces a page-wide failure or prevents another
  row from working.
- Provider rows use one drag handle and persist their order. That order resolves
  duplicate model names in the compose model catalog. Dragging changes density
  and opacity without introducing a second card style or decorative motion.
  Deletion uses the shared consequential confirmation and clears, rather than
  silently replacing, a deleted default.
- The accordion's add icon opens an embedded child flow. The first step is a
  searchable grid of preset Provider marks plus **自定义**; choosing one opens
  the shared add/edit detail surface. A back action returns one level without
  closing Settings. The detail surface contains the instance name, **API 协议**,
  `BASE_URL`, masked `API_KEY`, environment-key choice, and editable preferred
  model. **自动（当前使用：…）** previews the protocol for the current
  model; unsupported protocols are absent, model-incompatible choices are disabled,
  and Beta or compatibility limits appear as compact, right-aligned helper copy
  in the **API 协议** field heading. A customized
  `BASE_URL` is never overwritten by later model or protocol changes. A manual key
  is never read back into React. Beside **保存渠道**, **测试连接** first persists
  the current form, retrieves its model list, performs a bounded minimal text
  request, and reports Rust-observed latency; **测试能力** first persists the form
  and independently probes structured output, tool calling, and multi-turn tool
  continuation. Either action stays in the detail surface and reports its own
  result there. **保存渠道** persists without starting a network test. Rust remains
  authoritative when validating and binding the route.
- A custom Provider detail also exposes one native themed **上下文窗口** selector
  with 128K, 200K, 500K, 1M, and 2M options. It is a fallback setting rather
  than a model capability claim: a context size returned by that exact tested
  endpoint and model takes precedence. Preset Providers derive documented model
  windows in Rust and do not expose this manual override.
- Provider marks are bundled local assets or the existing deterministic local
  fallback. Mine Mail never fetches a logo at runtime. Marks are optically
  centered on a transparent field; do not force them into avatar tiles or add a
  white backing plate. The preset search uses restrained neutral inset focus
  feedback: neither its wrapper nor its input draws a primary outer ring.
  Multiple instances may use the same preset and keep independent names,
  addresses, credentials, preferred models, discovery results, and connection
  state.
- **AI 翻译语言** and **默认开启 AI 助理** live outside the model accordion as
  two separate, always-visible single-row preference cards. Translation uses
  `ThemedSelect`, defaults to Simplified Chinese, and presents every language in
  its own written form, such as **中文（简体）**、**中文（繁體）**、**English**
  and **日本語**. Its select matches the standard settings-control height, and
  its open menu stays above later preference cards. The assistant preference
  uses the shared compact capsule switch and defaults on.
- The MCP configuration follows the two independent AI preference cards on the
  Agent page. **开启 MCP** is its parent preference with a compact question
  action. **获取信息** and **发送邮件** expand as visibly nested rows while the
  parent is on and collapse while it is off; their saved values are preserved
  while collapsed. The inline note says Mine Mail must remain in the foreground
  or tray. Enabling the parent uses one compact shared confirmation dialog; the
  question action opens a theme-owned tool explanation dialog rather than a
  tooltip. The collapsed model, translation, assistant, and MCP cards share one
  compact outer height; expanded content increases only the active card.
- The Agent configuration page keeps a bounded blank inset after its last card,
  allowing bottom fields and their downward-opening menus to scroll above a
  minimized compose bar.
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
  editor geometry and writing origin. The plain editor and every stationery mode
  share the same theme-owned paper surface; enabling stationery adds only its
  lines or grid, without changing the underlying paper color. Paper rules scroll
  with authored content.
  Secondary icon controls in compose fields and the footer rest on transparent
  fills without a visible border. Selected state relies on icon color; the
  boundary and surface appear only during hover or press feedback.
- Compose keeps AI editing and conversational work separate. The footer's compact
  optimization split control runs from its primary magic-wand action; its second
  segment opens a small upward prompt surface for optional instructions. While
  the request runs only the wand becomes a spinner; compose remains editable.
  A completion with no validated subject, body, or formatting difference keeps
  the wand idle, creates no comparison, and shows the nearby muted status
  **您的邮件已经很通顺了，不需要再改进**. A changed completion adds a small
  danger-colored notice dot to the wand without changing the draft. Reopening it
  shows a large, theme-owned two-pane comparison: the
  submitted snapshot on the left and AI result on the right. Each pane presents
  its subject and body as clearly separated editable regions so the pane reads as
  one complete mail version. Subject changes use a restrained side-colored field
  tint; body-text removals use a restrained danger tint and additions use a
  success tint. Formatting-only changes are not marked, and body text entered
  during review carries no difference tint. Each pane has one check action that
  chooses its subject and body together, followed by a compact confirmation that
  names both the selected side and the complete field group. The comparison can
  be minimized without losing it or permanently closed after confirmation.
  Applying a side backs up the then-live subject, body, and format before atomic
  replacement and enables the adjacent icon-only undo action; undo restores that
  field group atomically and stays visible but disabled without a backup.
  The AI assistant control sits at the far right of the recipient row, beside the
  Cc/Bcc disclosure, and opens or collapses the conversational panel.
- A persisted **默认开启 AI 助理** preference controls the panel's initial state
  whenever a compose surface is opened. It defaults on; the compose control can
  still collapse or reopen the panel for the current surface.
- On wide desktop viewports the AI assistant attaches to and extends the compose
  surface on the right. Defensive narrower layouts overlay it within the compose
  bounds. The panel has two full states: an application-level session list, and
  one selected conversation. The bottom prompt and mode selector keep the same
  position in both states. Session rows are compact, single-line controls that
  show only the truncated session title and its last-active time. Rows stack
  tightly, with a low-profile capsule surface appearing on hover or focus. That
  surface carries a stronger primary tint in every non-night theme so the active
  target remains unmistakable against pale panel backgrounds. The default list
  shows at most four recent sessions plus one low-emphasis **查看全部** action.
  **查看全部** replaces the assistant panel's compact list in place with a
  theme-owned, same-level framed surface. It starts directly below the assistant
  header, uses the original session-list column, and never moves into or covers
  the compose editor. It does not create a page-level modal layer or dim the
  workspace. Its lower edge follows its visible results instead of stretching to
  the assistant composer. The frame adds search and a result viewport that
  shrinks to the current result count, up to a maximum of eight compact rows;
  additional rows scroll internally. Filtering must never stretch one or a few
  results to fill that maximum height. A pointer press outside its frame, Escape,
  or its close action dismisses it and restores the unchanged four-row list plus
  focus to **查看全部**. Its search follows the composite-input rule: restrained
  neutral inset feedback on the shell, with no primary outer glow or second ring
  on the input. The assistant composer remains independently pinned to the panel
  bottom in both compact and full-list states; changing the list frame height
  never moves or resizes that composer.
  Hovering or focusing a row replaces its trailing time with a transparent,
  icon-only delete action; permanent deletion uses the shared consequential
  confirmation surface.
- The AI prompt indicates focus only on its rounded outer composer surface; the
  inner textarea never adds a second rectangular focus ring. Its mode trigger
  has no fixed width: the capsule follows the active label, with the caret placed
  immediately after the text at a consistent gap. A compact model selector sits
  immediately to its right, uses the same transparent adaptive-width capsule,
  and identifies both the exact model and resolved Provider. Its upward menu
  anchors to the trigger's left edge and may extend modestly upward and to the
  right within the prompt surface; long model rows scroll horizontally inside
  the menu instead of escaping that surface. Opening compose refreshes every
  configured Provider independently;
  exact model results appear progressively and one failed Provider contributes
  no choices without blocking another. Duplicate names use Provider row order,
  while the configured exact default route is selected initially when available.
- The fixed composer shows one muted, tabular context meter between the model
  capsule and send action, formatted as **CTX 20K/200K (10%)**. It
  updates while the prompt changes, warns only at the 75% compaction threshold,
  and never grows into a progress card or steals the send action's alignment.
  The mode capsule keeps its readable width while the model capsule yields first,
  caps its footprint, and ellipsizes long model names.
- The assistant header centers its text title without a leading AI mark. Its
  left-side actions begin with collapse and then a settings icon; settings is an
  inert entry point until configuration content is implemented, and there is no
  explicit new-session button. An active conversation keeps its back action
  after those global controls; its truncated title begins after the back action
  with reserved spacing so the controls never overlap it. Associated editable
  drafts sit immediately beneath as wrapping, low-profile pill buttons containing
  only the truncated subject and stable short display ID.
  Conversation history owns the remaining scroll surface. **自动**、**生成**
  and **聊天** live in the fixed composer; optimization is not duplicated as an
  agent mode. A user-authorized current-turn generation inside **聊天** does not
  change the visible mode selection and expires when that turn ends.
- The three conversational modes stream safe Markdown into the active assistant
  message. Each running assistant message owns an append-only activity trail:
  a thinking step, every tool call, and later thinking steps remain in execution
  order instead of competing for one transient row. A terminal response that
  used none of the exposed tools is buffered before display and adds a compact
  review step: **正在复核回答…** becomes **回答已复核** when accepted, or
  **发现执行偏差，正在重新处理…** while the host performs its one allowed
  correction. The unreviewed candidate never flashes in the Markdown area.
  Provider-supplied visible
  reasoning deltas may stream only inside the current thinking step; completing
  that step replaces its temporary detail with **分析完成** or **答案整理完毕**.
  Tool arguments, tool results, and unavailable hidden reasoning are never shown.
  Final Markdown streams independently beneath the trail and never clears it.
  While a turn runs, the send action becomes an icon-only stop action, session
  and mode switching are disabled, and the prompt remains editable for the next
  turn. Collapsing or minimizing compose does not cancel generation.
- Agent write tools never replace live compose automatically. A completed
  assistant message may contain two read-only, primary-tinted proposal cards:
  one groups To, Cc, Bcc, and Subject; the other groups body and stationery.
  These cards use theme-tinted frosted material, a restrained highlight edge,
  and layered optical shadows so they read as actionable objects above the
  conversation without becoming glowing decoration. Long body proposals wrap
  within the card and use vertical, non-overscrolling navigation only; they never
  expose a two-dimensional content canvas.
  Each changed group has one icon-only apply action. Applying immediately
  replaces only that group and turns the action into an undo for its stored
  pre-apply backup; neither action opens a confirmation. Old proposal cards stay
  actionable in their conversation until their seven-day rich-state expiry.
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

- The Mine Mail new-mail surface is a compact, always-readable lower-right card.
  It uses the local-first sender avatar, subject, and receiving-account identity,
  never body preview text.
- On Windows, the user may instead select the operating-system notification
  surface. Windows owns its banner and notification-center geometry, material,
  app-name attribution, icon treatment, dismissal, and do-not-disturb behavior;
  Mine Mail supplies bounded sender identity, subject, receiving-account
  identity, and batch count. Do not imitate or overlay the Windows surface with
  a second Mine Mail card.
- Confirmation dialogs are compact, theme-owned, keyboard-operable, and limited
  to consequential actions such as enabling MCP, account removal, storage
  migration, uncertain delivery decisions, and permanent deletion.
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
