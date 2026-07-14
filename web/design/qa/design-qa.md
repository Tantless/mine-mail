# Mine Mail MVP — Design QA

## Evidence

- Source reference: `design/references/mine-mail-windows-reference.png`
- Implemented desktop state: `design/qa/implementation-windows-final.png`
- Source/implementation comparison: `design/qa/comparison-windows.png`
- Compose state: `design/qa/implementation-compose.png`
- Actual Windows Tauri window capture: 3098 × 1850 physical pixels, maximized.

The Product Design in-app browser runtime reported no available browser in this session. Visual QA therefore used the actual compiled Tauri/WebView2 window, captured with the Windows window API, rather than a substitute browser. The React interaction suite and production build were run separately.

## Visual comparison

- One continuous painterly landscape spans the complete application surface.
- Left navigation remains on the wallpaper; center and reader surfaces are fully opaque.
- Measured visual proportions are approximately 20.5% navigation, 29% message list, and 48% reader, matching the selected reference.
- Background remains visible at the top, right edge, bottom edge, and between the two content panels.
- Panel radii, border weight, shadows, row density, search placement, toolbar hierarchy, attachment cards, and selection state match the reference family.
- The implementation intentionally localizes content to Chinese and uses Mine Mail's own generated Daylight landscape rather than copying the mock's background.
- Compose is a focused bottom-right work surface over a subdued full-window scrim; fields, primary send action, local draft action, and destructive discard action remain visually distinct.

## Theme and asset checks

- Daylight, Night, Dusk, and Forest are original 16:10 raster paintings and were individually opened and inspected.
- All theme changes flow through semantic CSS tokens; mail text never sits directly on wallpaper.
- The app icon is an original 1024 × 1024 raster asset and generated Tauri icons were rebuilt from it.
- UI symbols use Phosphor Icons; there are no handcrafted interface SVGs, emoji icons, or placeholder image boxes.

## Interaction and accessibility checks

- React tests cover initial inbox/read state, theme selection/persistence, search filtering, the send-confirmation gate, and uncertain-delivery handling.
- The real Tauri shell starts with the backend-backed local database without triggering a network call.
- Sending is unavailable until a recipient exists and always presents a final recipient review before the Rust command boundary.
- Primary controls have accessible names, visible keyboard focus, and at least 40 px pointer targets.
- `N`, `Ctrl/Command + K`, and `Ctrl/Command + Enter` adapt to the host platform.
- Narrow-window behavior has explicit icon-rail, drawer, and single-pane breakpoints; reduced-motion preferences disable movement animations.

## Validation status

- React production build: passed
- React interaction tests: 5 passed
- Tauri Rust build, format, clippy, and unit test: passed
- Desktop startup and actual-window visual inspection: passed
- No real email was sent during UI QA

final result: passed
