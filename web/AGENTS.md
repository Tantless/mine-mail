# Prototype Instructions

Run the local server yourself and open the preview in the browser available to this environment. Do not give the user server-start instructions when you can run it.

Before making substantial visual changes, use the Product Design plugin's `get-context` skill when the visual source is unclear or no longer matches the current goal. When the user gives durable prototype-specific design feedback, preferences, or decisions, record them in `AGENTS.md`.

When implementing from a selected generated mock, treat that image as the source of truth for layout, component anatomy, density, spacing, color, typography, visible content, and hierarchy.

## Mine Mail MVP design decisions

- Source-of-truth references: `design/references/mine-mail-windows-reference.png` and `design/references/mine-mail-macos-reference.png`.
- One painterly landscape wallpaper spans the entire app window beneath all three columns.
- Left navigation sits directly on the wallpaper. The center message list and right reading surface are separate near-opaque panels that nearly fill the window, leaving only narrow 10–14 px gutters where the wallpaper peeks through.
- Desktop proportions target roughly 20.5% navigation, 29% message list, and the remaining 47–49% reading pane.
- Backgrounds are original, non-photorealistic, low-detail landscape paintings rather than photographs.
- The MVP ships four selectable themes: Daylight, Night, Dusk, and Forest.
- Email text is always rendered on an opaque or near-opaque surface; decorative wallpaper must never reduce readability.
