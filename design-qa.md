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
