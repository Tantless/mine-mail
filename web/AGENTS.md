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
- The compose action, message list, and reading pane share semantic frosted-material tokens. The list is lighter than the reading pane, the reading pane stays near-opaque, and the compose action uses a translucent theme accent. New themes must inherit or override these tokens rather than introducing flat opaque cards.
- The theme picker is a theme-tinted frosted popover. Any pointer interaction outside the picker, including another sidebar control, dismisses it before that target action continues.
- The future-letter quotation ink is explicitly tuned per wallpaper: cool blue-black in Daylight, clear off-white in Night, warm wine-brown in Dusk, and deep pine green in Forest. The attribution retains the same ink, and contrast takes priority over decorative tint.
- Settings text-field visuals and their actual input hit area are one surface: the full inset shell uses the text cursor and focuses the contained input when clicked.

## Desktop integration decisions

- After the visual MVP, functional mail updates target the Tauri desktop application only. Do not maintain a parallel Web-mode implementation of new mail sync, notification, draft-sync, background, autostart, or SMTP behavior.
- The desktop app must use the Rust mail core and SQLite as its source of truth; React must not connect directly to IMAP, SMTP, or credential files.
