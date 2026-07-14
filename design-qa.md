# Mine Mail — Themed Titlebar Design QA

## Evidence

- Source visual truth: `web/design/references/mine-mail-windows-reference.png`
- User-reported before state: the Windows 11 native white titlebar shown in the current conversation.
- Final Daylight implementation: `web/design/qa/implementation-titlebar-daylight-final.png`
- Final Night implementation: `web/design/qa/implementation-titlebar-night-final.png`
- Full-view comparison: `web/design/qa/comparison-titlebar-full.png`
- Focused titlebar comparison: `web/design/qa/comparison-titlebar-focused.png`
- Viewport: maximized Windows 11 Tauri/WebView2 window, 3072 × 1824 physical pixels at 200% DPI.
- State: empty cached inbox, no network sync and no SMTP operation triggered.

The Product Design in-app browser was unavailable for this desktop-only window-chrome change. QA therefore used the compiled Tauri application and Windows `PrintWindow` capture, with DWM frame bounds applied so the invisible resize border was excluded.

## Full-view comparison

- The app wallpaper now extends behind the complete window surface instead of beginning below a native white strip.
- The titlebar uses the active theme's semantic tint, text, divider, and hover tokens while preserving the three-panel layout and its existing top spacing.
- Center and reader panels retain their previous dimensions; no above-the-fold content was lost.
- Daylight retains the pale painterly sky treatment from the source reference. Night uses the same structure with a dark blue translucent tint rather than an unrelated system color.

## Focused comparison

- The 38 px titlebar matches the compact density of the selected reference and the former Windows titlebar.
- App identity remains on the left; minimize, maximize/restore, and close remain aligned to the right.
- The close hover state uses the Windows destructive red treatment. Other controls use theme-aware translucent hover states.
- A thin theme-aware divider separates window chrome from content without introducing a second visual frame.

## Required fidelity surfaces

- Fonts and typography: the titlebar reuses Inter/Segoe UI fallbacks, 12 px sizing, restrained weight, and single-line app copy. Controls use Phosphor icons already established by the MVP.
- Spacing and layout rhythm: the titlebar is 38 px high, controls are 46 px wide, and content begins 14 px below it. Panel proportions, radii, and gaps remain unchanged.
- Colors and visual tokens: Daylight, Night, Dusk, and Forest each define titlebar foreground, tint, border, and hover tokens. Contrast remains readable in the two captured extremes.
- Image quality and asset fidelity: the original full-resolution painterly wallpapers remain continuous beneath the titlebar. No placeholder or CSS-drawn product asset was introduced.
- Copy and content: `Mine Mail` and the existing Chinese mailbox copy are unchanged.

## Interaction validation

- Drag region moved the real frameless Tauri window.
- The maximize/restore control worked in both directions.
- Double-clicking the drag region maximized the window.
- Minimize and close controls worked in the real Windows window.
- React interaction suite: 6 tests passed, including drag-region depth and non-draggable control-region assertions.
- React production build: passed.
- Tauri debug no-bundle build: passed.

## Comparison history

### Pass 1

- [P2] The first theme tint was visually denser than the selected reference and hid too much of the wallpaper at the top edge.
- Evidence: `web/design/qa/implementation-titlebar-night-full.png`.
- Fix: reduced titlebar tint opacity for all four themes and reduced backdrop blur from 18 px to 12 px.

### Pass 2

- Post-fix evidence: `web/design/qa/implementation-titlebar-daylight-final.png`, `web/design/qa/implementation-titlebar-night-final.png`, and `web/design/qa/comparison-titlebar-focused.png`.
- No actionable P0, P1, or P2 mismatch remains.
- Residual P3: the small titlebar app label intentionally coexists with the larger sidebar brand, matching the current product structure and keeping app identity visible when the sidebar collapses.

## Implementation checklist

- [x] Disable native window decorations and retain system shadow/resizing.
- [x] Add a deep Tauri drag region with explicit non-draggable controls.
- [x] Add least-required window command permissions.
- [x] Apply semantic titlebar tokens to all four MVP themes.
- [x] Verify real Windows window controls and visual output.

final result: passed
