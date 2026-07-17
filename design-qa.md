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
