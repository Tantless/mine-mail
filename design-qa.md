# Design QA — Settings Account Flow and Glass Material — 2026-07-22

- Source visual truth: `C:\Users\tantl\AppData\Local\Temp\QQ_1784726488415.png`
- Combined source/implementation comparison: `Z:\mine-mail\design-qa\settings-glass-comparison.png`
- Account overview implementation: `Z:\mine-mail\design-qa\settings-glass-overview.png`
- Provider picker implementation: `Z:\mine-mail\design-qa\settings-add-account-glass-provider.png`
- 163 connection form implementation: `Z:\mine-mail\design-qa\settings-add-account-glass-form.png`
- Viewport: 2000 × 1200 CSS pixels at WebView2 density 2; implementation captures are 4000 × 2400 physical pixels.
- Pixel dimensions: source board 2272 × 1456; implementation 4000 × 2400; comparison board 4000 × 1500. The source and implementation were proportionally downsampled into equal-width columns on the comparison board; no density-derived visual issue was filed.
- State: running Windows Tauri desktop app, Forest theme, two connected accounts; account overview, provider selection, and 163 credential-entry states.

## Full-view Comparison Evidence

The combined board places the supplied embedded-settings direction and the running account overview in one comparison input. Both keep the main Mine Mail sidebar visible, use a quiet category rail, and reserve the right pane for the active setting. The revised implementation now lets the same mountain wallpaper read through the complete settings workspace; the detail pane remains deliberately denser than the mail reader so controls and small copy retain contrast.

## Focused Region Comparison Evidence

No additional crop was necessary because the material issue affects the complete workspace and remains clearly visible in the full 4000 × 1500 comparison. The two separate full-density interaction captures verify the smaller provider-card and bordered-form surfaces: the provider picker stays open after the Add Account action, and the subsequent 163 form keeps the same glass hierarchy without clipping or reverting.

## Required Fidelity Surfaces

- Fonts and typography: Mine Mail's existing Inter/system stack, optical weights, line heights, and compact supporting copy are unchanged; headings and form labels remain legible over the wallpaper echo.
- Spacing and layout rhythm: the primary sidebar, settings rail, centered 830 px content track, account-row dividers, provider cards, input card, radii, and vertical rhythm remain aligned with the approved embedded reference.
- Colors and visual tokens: new settings-specific shell, rail, detail, and wallpaper-echo tokens keep every theme wallpaper-aware. The shell uses a slightly denser alpha than the reader, while the former opaque overlay has been removed and replaced by a transparent tonal wash.
- Image quality and asset fidelity: the approved painterly wallpaper and transparent fox mark are reused directly at native quality. Existing account/provider assets and Phosphor icons remain unchanged; no substitute asset was introduced.
- Copy and content: account capacity, provider descriptions, credential guidance, settings labels, and actions remain unchanged. The fix affects flow state and material only.

## Findings

No actionable P0, P1, or P2 issue remains.

## Comparison History

### Iteration 1

- Earlier finding [P1]: clicking **添加账户** briefly opened the provider picker and immediately returned to the account overview whenever the parent still held a stale `saved` submission status.
- Fix: account-flow completion now reacts only to the current `saving → saved` transition; an old terminal status can no longer collapse a newly opened flow. Two regression tests cover stale status and a genuine completed connection.
- Post-fix evidence: the actual Tauri accessibility tree retained **选择邮箱服务商** after the button action, then retained **连接 163 邮箱** after selecting the 163 provider. `settings-add-account-glass-provider.png` and `settings-add-account-glass-form.png` capture both stable states.

### Iteration 2

- Earlier finding [P1]: the embedded settings shell composited an opaque panel and an additional opaque tint over the wallpaper, reading as a flat pale fill rather than Mine Mail glass.
- Fix: introduced theme-specific settings material tokens, lowered shell/rail/detail opacity, exposed a controlled wallpaper echo, and removed the redundant opaque pseudo-element layer.
- Post-fix evidence: `settings-glass-comparison.png` and `settings-glass-overview.png` show the mountain texture continuing through the settings workspace while account text, borders, and controls remain clear.

## Interaction, Accessibility, and Runtime Checks

- Tested Add Account and 163 provider selection through the running Tauri accessibility tree, including a pause after each action to verify that neither state flashes back.
- Added a component regression suite covering stale `saved` state and the valid `saving → saved` completion path.
- Full React suite: 15 files and 137 tests passed. Production Vite build passed. `git diff --check` passed.
- The actual Tauri window remains open on the 163 connection form for inspection.

## Follow-up Polish

No P3 follow-up is required for this fix.

final result: passed

---

# Design QA — Themed Select Controls — 2026-07-22

- Source visual truth: `Z:\mine-mail\design-qa\select-user-reference.png`
- Full implementation screenshot: `Z:\mine-mail\design-qa\select-audit-after.png`
- Focused implementation crop: `Z:\mine-mail\design-qa\select-audit-focused-after.png`
- Focused before/after comparison: `Z:\mine-mail\design-qa\select-audit-comparison.png`
- Additional audited states: `Z:\mine-mail\design-qa\select-audit-settings-options.png`, `Z:\mine-mail\design-qa\select-audit-smtp.png`
- Viewport: 1549 × 925 CSS pixels at WebView2 density 2; full implementation capture 3098 × 1850 physical pixels.
- Pixel dimensions: source crop 568 × 336; focused implementation crop 520 × 520; comparison board 1240 × 650.
- State: Windows Tauri runtime, light frosted theme, sync interval expanded; additional captures cover remote-image mode and custom SMTP security.

## Full-view Comparison Evidence

The source is a focused crop rather than a full application view, so full-view pixel matching is not applicable. The implementation full view confirms that the themed menu remains inside the settings reading track, does not clip persistent controls at the desktop viewport, and preserves the account-page rhythm while open.

## Focused Region Comparison Evidence

The comparison board places the exact user-provided native dropdown crop beside the revised control. The grey Windows selection slab, square popup corners, unthemed type treatment, and system arrow are replaced by a rounded frosted menu, theme-token border and shadow, existing Phosphor caret/check icons, green selected state, and consistent row spacing.

## Required Fidelity Surfaces

- Fonts and typography: trigger and options use the app font stack, 11 px UI sizing, controlled 600/690 weights, and Mine Mail text colors instead of the system menu font.
- Spacing and layout rhythm: the trigger keeps its existing 190 × 40 CSS footprint; the menu uses 6 px inset padding, 3 px row gaps, 36 px options, 8/12 px radii, and aligns to the trigger edge.
- Colors and visual tokens: popup, hover, focus, selected, disabled, border, shadow, blur, and scrollbar all derive from the existing semantic theme tokens. No fixed grey selection color remains.
- Image quality and asset fidelity: no raster asset was needed. Caret and selected check use the existing Phosphor icon package; no custom SVG, glyph approximation, or CSS-drawn icon was introduced.
- Copy and content: all existing values remain unchanged: 1/3/5 minutes, notification presets, remote-image modes, and TLS/STARTTLS.

## Findings

No actionable P0, P1, or P2 issue remains.

## Comparison History

### Iteration 1

- Earlier finding [P1]: opening a native HTML `select` delegated the popup to Windows/WebView2, producing square white/grey menu chrome that visibly broke the frosted Mine Mail theme.
- Fix: replaced every native select with one reusable theme-owned combobox/listbox component and added themed open, hover, selected, disabled, focus, and reduced-motion-compatible states.
- Post-fix evidence: `select-audit-comparison.png` shows the requested sync dropdown before and after; `select-audit-settings-options.png` and `select-audit-smtp.png` confirm the same treatment in the other settings flows.

## Project-wide Native-Control Audit

- Native `<select>` / `<option>` elements remaining: 0.
- Replaced visible dropdowns: complete reconciliation interval, notification sound, remote-image mode, and SMTP security.
- Native date/time/month/week/color/range pickers remaining: 0.
- Two numeric port inputs remain semantically `type="number"`, but their WebView spin buttons are now suppressed so their visual surface stays themed.
- The only file input is the 1 × 1 px visually hidden avatar picker activated through the themed avatar surface; it exposes no native file control in the interface.
- Settings switches are already custom themed controls, and the reply-history native disclosure marker is already suppressed in favor of the app icon system.

## Interaction and Accessibility Checks

- Mouse selection, click-outside close, Arrow/Home/End navigation, Escape close, focus return, disabled state, selected check, and account-setting persistence are implemented in the shared control.
- The trigger exposes `combobox`, expanded state, active descendant and listbox relationship; each item exposes `option` and selected state in the real Tauri accessibility tree.
- Full React suite: 14 test files and 135 tests passed. Production Vite build passed. `git diff --check` passed.

## Follow-up Polish

No P3 follow-up is required for this control pass.

final result: passed

---

# Design QA — Embedded Settings Workspace — 2026-07-22

- Source visual truth: `C:\Users\tantl\AppData\Local\Temp\QQ_1784726488415.png`
- Revised account overview: `Z:\mine-mail\design-qa\settings-embedded-account-overview-revised-normalized.png`
- Provider picker: `Z:\mine-mail\design-qa\settings-embedded-provider-picker-normalized.png`
- 163 connection form: `Z:\mine-mail\design-qa\settings-embedded-163-form-normalized.png`
- Function settings: `Z:\mine-mail\design-qa\settings-embedded-features.png`
- About page: `Z:\mine-mail\design-qa\settings-embedded-about.png`
- Final side-by-side comparison: `Z:\mine-mail\design-qa\settings-embedded-comparison-final.png`
- Viewport: 1574 × 900 logical pixels at WebView2 density 2; the raw `PrintWindow` capture is 4000 × 1800 physical pixels and includes an off-screen black strip, so implementation evidence was normalized to the 3148 × 1800 rendered WebView region before comparison.
- Pixel dimensions: source 2272 × 1456; normalized implementation 3148 × 1800; comparison board 3240 × 2080.
- State: Windows Tauri desktop runtime, light frosted theme, two connected accounts, account overview/provider picker/163 credential form.

## Comparison Scope

The supplied image is a design-direction board rather than a single pixel-identical viewport: it combines the embedded account overview, two add-account flow states, and explanatory footer notes. The implementation comparison therefore checks the same three product states independently while preserving Mine Mail's existing desktop shell, tokens, wallpaper, real account data, and native window controls.

## Full-view Comparison Evidence

- The main Mine Mail sidebar remains visible and selected while settings replaces the message list and reader region as one embedded workspace.
- The workspace uses the existing wallpaper echo, theme glass surfaces, subtle edge, radius, and blur instead of a detached modal/backdrop treatment.
- The settings category rail and detail pane follow the reference hierarchy without duplicating the Mine Mail brand or introducing a title bar/footer.
- Account overview density now follows the reference: connected accounts, current sending account, and synchronization settings form one vertically complete page.
- Provider selection and credential entry are separate drill-in states, keeping the add-account form hidden until the user explicitly chooses a provider.

## Focused Region Comparison Evidence

The final 2 × 2 comparison board places the full source board beside the revised account overview, provider picker, and 163 form at readable scale. The account rows, compact current state, icon-only switch/menu actions, direct avatar-edit affordance, provider cards, and bordered credential fields are visible without needing an additional crop. Function settings and About were also captured separately because they do not appear in the reference board.

## Required Fidelity Surfaces

- Fonts and typography: the implementation keeps Mine Mail's Inter/system stack, dark optical weights, compact uppercase eyebrow, and restrained supporting copy. Titles, account identities, row labels, and captions remain legible with no clipping or unintended wrapping.
- Spacing and layout rhythm: the primary sidebar, 218 px settings rail, centered detail track, 6–14 px radii, divided account rows, and 30 px section rhythm reproduce the reference's desktop density. Persistent controls remain visible and the detail pane owns the single themed scrollbar.
- Colors and visual tokens: every surface uses existing semantic wallpaper, panel, edge, accent, muted-text, danger, shadow, and blur tokens. The former generic green `2/3` success indicator is removed; account capacity is neutral supporting copy.
- Image quality and asset fidelity: the existing painterly wallpaper and approved transparent fox asset are reused at native quality. Account/provider marks use the repository avatar system and Phosphor icons; no placeholder image, custom SVG, emoji, or CSS-drawn asset was introduced.
- Copy and content: account capacity, current sending identity, reconciliation cadence, provider authorization explanations, local credential storage, notification behavior, remote-image privacy, autostart, and version copy match existing product behavior.

## Findings

No actionable P0, P1, or P2 issue remains.

## Comparison History

### Iteration 1

- Earlier finding [P2]: the first embedded account overview ended immediately after the two account rows, leaving a large unstructured lower field and falling short of the reference's “账户与同步” hierarchy.
- Fix: added a quiet current-sending-account surface and moved the complete-reconciliation interval into a dedicated Synchronization section on the account page; removed the duplicate interval row from Function settings.
- Post-fix evidence: `settings-embedded-account-overview-revised-normalized.png` and `settings-embedded-comparison-final.png` show the completed vertical hierarchy with no clipped content or persistent-control overflow.

## Interaction, Accessibility, and Runtime Checks

- Opened Settings through the real Tauri accessibility tree, switched Account / Function settings / About, entered and backed out of the provider picker, and opened the 163 connection form.
- The 163 email and authorization-secret inputs expose visible resting borders and accessible labels before focus.
- Account avatars are direct file-picker controls, account switching is icon-only with an accessible account-specific label, and destructive removal is isolated in each account's local menu.
- Preferences persist immediately; the saving indicator is transient and the global Cancel / Save footer has been removed.
- Full React suite: 13 files and 132 tests passed. Production Vite build passed. `git diff --check` passed.
- The in-app browser runtime was unavailable, so visual evidence was captured from the actual running Tauri WebView; the inspected settings states remained responsive and exposed their expected accessibility nodes.

## Follow-up Polish

No P3 follow-up is required for this redesign.

final result: passed

---

# Design QA — Menu-based Settings Window

- Source visual truth: `C:\Users\tantl\Documents\xwechat_files\wxid_0qqrc78n3hvm12_adc0\temp\InputTemp\c99bc93d-f6a0-499e-af99-e1245699f8ce.png`
- Account implementation screenshot: `Z:\mine-mail\design-qa\settings-account-final.png`
- Function settings screenshot: `Z:\mine-mail\design-qa\settings-features-final.png`
- Version screenshot: `Z:\mine-mail\design-qa\settings-version-final.png`
- Full-view comparison: `Z:\mine-mail\design-qa\settings-full-comparison.png`
- Focused comparison: `Z:\mine-mail\design-qa\settings-focus-comparison.png`
- Viewport: 1440 × 900 logical pixels / 2906 × 1815 physical capture pixels
- State: Windows desktop runtime, Dusk theme, connected 163 account

## Comparison Scope

The supplied screenshot establishes the existing settings state and the blue target bounds. The written brief supplies the new information architecture: no title bar, a left navigation, a right content area, wallpaper-aware glass, and separate Account, Preferences, and Version pages.

## Full-view Comparison Evidence

- The old narrow, vertically dense sheet has been replaced by a landscape modal whose width closely follows the marked target bounds.
- A persistent left settings menu and a right content area make the three requested categories independently scannable.
- The close action floats over the content instead of creating a dedicated title bar.
- The modal and its cards reuse the current theme wallpaper echo, blur, border, highlight, shadow, and accent tokens.
- The footer remains fixed while only the active settings page scrolls.

## Focused Region Comparison Evidence

- Account identity, provider selection, credential fields, and avatar management remain present, but are grouped into “Connected account” and “Connect new account” cards.
- The function page shows synchronization interval and remote-image mode as compact dropdowns; autostart remains an explicit checkbox.
- The version page displays `v0.0.1` and reserves a disabled “Check for updates” button without implying that updates are already implemented.

## Required Fidelity Surfaces

- Fonts and typography: Existing Inter/Segoe UI typography is preserved. Page titles, section labels, supporting text, and navigation labels have distinct weights and line heights without clipping.
- Spacing and layout rhythm: The modal matches the requested wide/short proportion, uses a stable 220 px navigation rail, keeps the footer fixed, and preserves a single content scrollbar. Cards, rows, and controls align to a consistent 12–20 px rhythm.
- Colors and visual tokens: Dusk accent, glass opacity, wallpaper echo, surface borders, focus states, success state, and disabled state all derive from existing semantic tokens.
- Image quality and asset fidelity: The approved Mine Mail wallpaper remains the only raster backdrop. All controls use the existing Phosphor icon library; no placeholder or approximate image assets were introduced.
- Copy and content: The three menu labels, account purpose, synchronization choices, remote-image risk helper, autostart copy, fixed version, and update placeholder match the requested product behavior.

## Findings

No actionable P0, P1, or P2 differences remain for the requested settings redesign.

## Comparison History

- Pass 1: The first implementation used almost the full desktop width and height. This exceeded the blue target bounds and made the menu feel like a replacement screen rather than a settings window.
- Fix: Reduced the desktop modal from 1420 × 780 CSS pixels to 1080 × 620, narrowed the navigation rail, tightened content padding, and retained internal account-page scrolling.
- Pass 2: Full and focused comparisons show the final modal close to the marked landscape proportions. Account, function, and version states fit without clipped persistent controls. No P0/P1/P2 issue remains.
- P3 polish applied after Pass 2: Reduced the native autostart checkbox footprint so it aligns with the dropdown controls.

## Interaction Checks

- Account, Function settings, and Version menu items switch content without closing the modal.
- Synchronization and remote-image dropdown changes remain in local component state across menu switches and are submitted together by Save settings.
- Cancel and Close leave settings without saving; Save settings uses the existing desktop persistence command.
- Account provider selection, credential submission, account avatar selection/removal, and the remote-image risk helper remain available.
- The actual Tauri window was used for visual capture because the in-app browser surface was unavailable; web interaction coverage passed in Vitest.

## Follow-up Polish

- P3: The future updater can replace the disabled placeholder in place without changing the page layout.

final result: passed

---

# Sidebar Brand Size and Position QA — 2026-07-19

- Source visual truth: user-provided current-state screenshot and `Z:\mine-mail\web\design\qa\mine-mail-brand-lockup-focus-final.png`
- Running implementation screenshot: `Z:\mine-mail\web\design\qa\mine-mail-brand-lockup-larger-current.png`
- Focused implementation crop: `Z:\mine-mail\web\design\qa\mine-mail-brand-lockup-larger-focus.png`
- Focused before/after comparison: `Z:\mine-mail\web\design\qa\mine-mail-brand-lockup-larger-comparison.png`
- Viewport: 1453 × 908 physical pixels, DPI-aware capture of the running Tauri window.
- State: daylight theme, existing local mailbox, sidebar fully expanded.

## Full-view Comparison Evidence

The running desktop capture shows the enlarged brand lockup fitting within the existing sidebar width and transparent top safe area. The compose button remains at its previous layout position, and the larger fox and wordmark do not collide with the mail list, window controls, or navigation.

## Focused Comparison Evidence

The focused before/after image places the previous approved lockup beside the revised implementation. The fox grows from 36 to 44 CSS pixels, the wordmark grows from 22 to 25 CSS pixels, and the complete lockup moves upward by 4 CSS pixels. The existing airy horizontal spacing is retained and increased slightly from 28 to 30 CSS pixels.

## Required Fidelity Surfaces

- Fonts and typography: the approved local `Nunito Variable` wordmark remains unchanged in family, weight, line height, tracking, and copy; only its size increases to 25 px.
- Spacing and layout rhythm: brand row height and bottom margin compensate for one another, so the compose control and following navigation do not shift. The lockup uses a controlled -4 px vertical offset.
- Colors and visual tokens: theme-aware sidebar text color and all material tokens remain unchanged.
- Image quality and asset fidelity: the approved transparent fox PNG is reused directly at 44 × 44 px with no frame, replacement, or generated approximation.
- Copy and content: `Mine Mail` remains the only brand copy; no navigation or mailbox content changes.

## Findings

No actionable P0, P1, or P2 mismatch remains. The requested larger and slightly higher brand treatment is visible without introducing layout overlap.

## Comparison History

- Pass 1: the revised full-view and focused comparison show the requested scale and vertical lift while preserving the compose-button position. No additional visual fix was required after comparison.

## Interaction and Build Checks

- The running Tauri window remained responsive and the sidebar retained its expanded desktop layout.
- React verification: 79 tests passed.
- Production Vite build passed.
- The browser-control runtime was unavailable, so visual evidence was captured from the actual running Tauri window.

## Follow-up Polish

No P3 follow-up is required. The six `--sidebar-brand-*` variables at the top of `web/src/styles.css` provide a single manual adjustment point.

final result: passed

---

# Sidebar Brand Lockup Design QA — 2026-07-18

- Source visual truth: `C:\Users\tantl\.codex\generated_images\019f74e7-709f-7251-8f0f-3f102fc9078b\exec-5629226b-fe45-46be-bece-0cb2e7312d03.png`
- Browser-rendered implementation screenshot: `Z:\mine-mail\web\design\qa\mine-mail-brand-lockup-implemented-final.png`
- Focused implementation crop: `Z:\mine-mail\web\design\qa\mine-mail-brand-lockup-focus-final.png`
- Focused side-by-side comparison: `Z:\mine-mail\web\design\qa\mine-mail-brand-lockup-comparison-final.png`
- Viewport: 1536 × 791 logical pixels, Chrome local desktop preview.
- State: daylight theme, demo desktop shell, theme menu closed.

## Full-view Comparison Evidence

The full desktop capture confirms that the enlarged brand lockup still fits the existing sidebar rail, remains aligned with the compose control, and does not displace the folder navigation or window controls. The approved wallpaper and continuous sidebar material are unchanged.

## Focused Region Comparison Evidence

The side-by-side focused comparison shows the selected rounded wordmark direction, transparent fox treatment, and deliberately airy logo-to-name gap on the actual app background. The implementation scales the concept down to the compact sidebar while preserving the reference's approximate one-logo-width visual separation.

## Required Fidelity Surfaces

- Fonts and typography: `Nunito Variable` is bundled locally and used only for the wordmark at 22 px, weight 800, line-height 1, and tightened display tracking. The exact `Mine Mail` copy remains on one line without clipping or synthetic fallback.
- Spacing and layout rhythm: the fox is 36 × 36 px, the brand row is 38 px high, and the final 28 px CSS gap produces the approved airy separation. The compose button remains aligned below the brand row without overlap.
- Colors and visual tokens: the wordmark continues to inherit the active sidebar foreground color in every theme. No new color surface or non-semantic theme override was introduced.
- Image quality and asset fidelity: the existing approved transparent fox PNG is used directly. The former border, white glass plate, corner radius, and container shadow are absent; the browser reports a transparent background, zero border, and no shadow.
- Copy and content: the only brand text remains exactly `Mine Mail`; no surrounding labels or navigation copy changed.

## Findings

No actionable P0, P1, or P2 mismatch remains.

## Comparison History

- Pass 1: the first implementation used a 16 px gap. The focused comparison showed that it was still noticeably tighter than the user-approved reference.
- Fix: increased the logo-to-wordmark gap to 28 px while keeping the logo, type size, sidebar alignment, and surrounding layout unchanged.
- Pass 2: the focused comparison shows the final visual gap at approximately one visible fox width, matching the selected direction without pushing the wordmark outside the sidebar.

## Interaction and Console Checks

- Opened and closed the theme menu in the browser-rendered app; the sidebar remained stable.
- Browser console warnings/errors: none.
- React verification: 79 tests passed.
- Production Vite build passed.

## Follow-up Polish

No P3 follow-up is required for this focused brand change.

final result: passed

---

# Sidebar Account Selected-State Design QA — 2026-07-17

- Source visual truth: `C:\Users\tantl\Documents\xwechat_files\wxid_0qqrc78n3hvm12_adc0\temp\InputTemp\8b691e37-ef4c-45a0-b84b-7e6e5879af4f.png`
- Running desktop capture: `Z:\mine-mail\design-qa\account-selected-state-after.png`
- Viewport: 1453 × 908 physical pixels at WebView2 device scale factor 2.
- State requested: dusk theme, two connected accounts, 163 active, Gmail inactive.

## Full-view comparison evidence

The supplied screenshot clearly shows the original problem: the active 163 card uses a 0.18 white surface and 0.28 white edge while the inactive Gmail card uses a 0.13 white surface, leaving only a five-point opacity difference and no independent active marker.

The running desktop window was captured after implementation. Its current high-DPI viewport is only about 726 × 454 logical pixels, so the account switcher falls below the visible sidebar region. The full application shell, dusk palette, wallpaper, typography, and surrounding navigation render without regression, but the requested account-card region is not present in the implementation evidence.

## Focused region comparison evidence

Blocked: the in-app browser surface reported no available browser binding, and the running Tauri window's current logical height clips the account switcher. A same-state focused after-capture could not be produced in this session.

## Intended state treatment implemented

- Inactive surface reduced from 0.13 to 0.09 white opacity.
- Hover surface uses 0.17 white opacity, visually between inactive and selected.
- Selected surface combines the active theme primary color at 14% with a 0.28 white glass surface.
- Selected border combines the active theme primary color at 32% with a 0.54 white edge.
- A three-pixel inset theme-colored rail provides a non-opacity-only selection cue.
- Selected email copy is raised to 82% of the sidebar foreground color.
- Focus-visible and `aria-pressed` behavior remain unchanged.

## Required fidelity surfaces

- Fonts and typography: no typography, wrapping, weight, or size changes were made; the source hierarchy is intentionally preserved.
- Spacing and layout rhythm: no dimensions, padding, radius, or account-slot ordering changed.
- Colors and visual tokens: selection now derives from `--color-primary`, so dusk, daylight, night, and forest retain the shared material structure while receiving theme-appropriate weak emphasis.
- Image quality and asset fidelity: avatars, wallpaper, and icon assets are unchanged.
- Copy and content: provider and email copy are unchanged.

## Findings

- [P2] Focused rendered verification is unavailable.
  - Location: sidebar account switcher.
  - Evidence: source region is available, but the post-change desktop capture clips the account region and the in-app browser is unavailable.
  - Impact: the implementation is covered by component tests and production build, but the exact visual contrast cannot be signed off from same-state evidence in this session.
  - Fix: capture the running app on a viewport tall enough to show the account switcher, then compare active, inactive, hover, and keyboard-focus states against the supplied source.

## Comparison history

- Iteration 1: identified insufficient separation between 0.13 inactive and 0.18 active surfaces.
- Fix: introduced quieter inactive glass, theme-tinted selected glass, a stronger semantic edge, an inset active rail, and selected secondary-copy emphasis.
- Post-fix visual evidence: blocked by the current high-DPI viewport and unavailable in-app browser surface.

final result: blocked

---

# Sidebar Account Switcher Design QA

- Source visual truth:
  - `C:\Users\tantl\Documents\xwechat_files\wxid_0qqrc78n3hvm12_adc0\temp\InputTemp\9a3e11e2-6aef-41d5-9249-296f3d3a9649.png`
  - `C:\Users\tantl\Documents\xwechat_files\wxid_0qqrc78n3hvm12_adc0\temp\InputTemp\989c327f-5678-49e4-959d-1b566374253c.png`
  - `C:\Users\tantl\Documents\xwechat_files\wxid_0qqrc78n3hvm12_adc0\temp\InputTemp\cbde1746-527c-4dfa-a55e-dd5da1d5c444.png`
- Implementation screenshot: `Z:\mine-mail\design-qa\sidebar-account-switcher-current-dpi.png`
- Focused implementation crop: `Z:\mine-mail\design-qa\sidebar-account-switcher-focus.png`
- Side-by-side focused comparison: `Z:\mine-mail\design-qa\sidebar-account-switcher-comparison.png`
- Viewport: 2906 × 1815 physical pixels, DPI-aware capture of the running Tauri window.
- State: dusk theme, two connected accounts, 163 active, Gmail inactive, one remaining account slot.

## Full-view comparison evidence

The running Tauri capture preserves the approved three-column shell, wallpaper, sidebar width, bottom anchoring, glass material, theme/settings actions, and existing folder navigation. The old single dropdown card is replaced by a three-slot account region without moving the surrounding navigation.

The supplied mock shows the one-account state with two empty slots; the captured implementation shows the corresponding two-account state with one empty slot. The repeated slot height, ordering, width, gap, and bottom placement are consistent across those states.

## Focused comparison evidence

The side-by-side comparison confirms that empty slots appear above connected accounts, account cards retain the existing avatar/provider/email hierarchy, and the account region remains directly above theme/settings. The implementation uses a subtle dashed edge plus a Phosphor Plus icon and `添加账号` label so the empty area is visibly actionable; this is an intentional accessibility clarification of the annotated target.

## Required fidelity surfaces

- Fonts and typography: existing Inter/system stack, provider weight, and muted email hierarchy are preserved. Labels remain single-line and truncate safely.
- Spacing and layout rhythm: all three positions share a 52 px minimum height, 8 px gap, and 10 px radius. One/two/three-account states keep a constant three-position region until full.
- Colors and visual tokens: cards and empty slots use the existing sidebar glass, muted foreground, focus ring, hover, and selected-state tokens across themes.
- Image quality and asset fidelity: existing local `ProfileAvatar` resolution is reused for every account. No placeholder raster, custom SVG, or remote avatar service was introduced.
- Copy and content: provider name and full account email remain visible. The only new fixed copy is `添加账号`.
- Icons: the add affordance uses the existing Phosphor icon family and matches the sidebar stroke weight.
- Accessibility and states: account cards are buttons with active `aria-pressed`; add slots have unique labels; keyboard focus, hover, active, one/two/three-account counts, switching, and settings navigation are covered.

## Findings

No actionable P0, P1, or P2 mismatch remains.

## Comparison history

### Iteration 1

- Earlier finding: the original UI exposed multiple accounts through a select dropdown and had no persistent empty account positions.
- Fix: replaced the select with visible account buttons, dynamic dashed add slots, active styling, and direct settings/account-form navigation.
- Post-fix evidence: the focused crop and side-by-side comparison show one empty slot plus two connected account cards in the correct bottom-sidebar region.

## Interaction and build checks

- Sidebar component tests verify 1 account → 2 add slots, 2 accounts → 1 add slot, and 3 accounts → 0 add slots.
- Tests verify direct visible-card switching and empty-slot navigation.
- App test verifies that an empty slot opens the Settings dialog at `连接新账户`.
- Full React suite: 76 tests passed.
- Production build passed.
- The in-app browser surface was unavailable in this session; visual evidence was captured from the actual running Tauri window instead.

final result: passed
# Reply-History Local Navigation Design QA — 2026-07-20

- Source visual truth: `C:\Users\tantl\AppData\Local\Temp\347520a3-8692-4c89-85b8-57619e5d5d2f.png`
- Browser-rendered implementation screenshot: `Z:\mine-mail\web\design\qa\quote-navigation-final.png`
- Focused implementation crop: `Z:\mine-mail\web\design\qa\quote-navigation-final-focus.png`
- Focused side-by-side comparison: `Z:\mine-mail\web\design\qa\quote-navigation-comparison.png`
- Navigation destination screenshot: `Z:\mine-mail\web\design\qa\quote-navigation-destination-passed.png`
- Viewport: 4000 × 2400 physical pixels at WebView2 device scale factor 2 (2000 × 1200 logical pixels).
- State: active dark theme, Gmail account, received `Re: test1` selected, cached `test1` ancestor available in Sent.

## Full-view Comparison Evidence

The final running Tauri capture preserves the approved three-column reader, dark frosted material, quote-card radius and edge, left quote mark, subject/route/time hierarchy, and far-right disclosure caret. The new local-navigation affordance uses the existing Phosphor icon family and occupies a reserved 32 px control column immediately left of the caret without changing card height.

## Focused Region Comparison Evidence

The side-by-side comparison places the supplied quote-card source and the final implementation in one image. Subject, sender → recipient, time, card outline, and disclosure direction remain visually consistent. The only intentional addition is the muted envelope control; its transparent default surface keeps the card hierarchy quiet while hover/focus styling uses the active theme accent.

## Required Fidelity Surfaces

- Fonts and typography: the existing Inter/system stack, subject weight, monospaced metadata, line heights, truncation, and single-line route/time behavior are unchanged.
- Spacing and layout rhythm: the card retains its 66 px header, 12 px radius, 32 px quote mark, and 9 px grid gaps. A dedicated 32 px navigation column prevents overlap with both metadata and caret.
- Colors and visual tokens: the control defaults to `--color-text-muted`; hover/focus use `--color-primary`, semantic border mixing, and the existing focus ring. No fixed theme color was introduced.
- Image quality and asset fidelity: no raster or custom SVG asset was added. The control uses the repository's existing Phosphor `EnvelopeOpen` icon at 17 px.
- Copy and content: subject, route, timestamp, and numbered fallback remain unchanged. The accessible label and tooltip say which mailbox will open.

## Findings

No actionable P0, P1, or P2 issue remains.

## Comparison History

### Iteration 1

- Earlier finding [P2]: an absolutely positioned sibling button inside `<details>` used the disclosure content area as its vertical containing block, so the control rendered below the quote header and was not visibly discoverable.
- Fix: moved the button into the summary's dedicated third grid column, kept the caret in the fourth column, and prevented/stopped the button click so it never toggles the disclosure.
- Post-fix evidence: `quote-navigation-final.png`, `quote-navigation-final-focus.png`, and `quote-navigation-comparison.png` show the icon aligned between metadata and caret with no height or text-wrap regression.

## Interaction, Accessibility, and Console Checks

- Before navigation, the quote disclosure was closed and the target resolved to Gmail Sent UID 3.
- Activating the icon switched the folder to `已发送`, opened reader heading `test1`, selected and focused the exact `test1` list row, and removed the previous quote card with the source message view. The result is captured in `quote-navigation-destination-passed.png`.
- React tests verify that activating the nested button does not open the `<details>` card, unresolved targets render no button, and cross-folder navigation selects/focuses the exact row.
- The 32 × 32 px button is keyboard-focusable, has a mailbox-specific accessible name and tooltip, and retains the global focus ring.
- DevTools showed no application error or warning from this flow; only the standard React DevTools advisory and the console paste-safety notice appeared during QA.

## Follow-up Polish

No P3 follow-up is required for this focused feature.

final result: passed

---
