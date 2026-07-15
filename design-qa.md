# Mine Mail — Frosted Material Design QA

## Evidence

- Source visual truth: `web/design/references/mine-mail-frosted-material-reference.png`
- Final Tauri implementation: `web/design/qa/implementation-frosted-daylight-final.png`
- Full-view comparison: `web/design/qa/comparison-frosted-material-final.png`
- Focused surface comparison: `web/design/qa/comparison-frosted-material-focused.png`
- Source viewport: 1586 × 992.
- Implementation viewport: 3000 × 1872 physical pixels, normalized to the source viewport for comparison.
- State: Daylight theme, inbox, first message selected, real cached mailbox data.

The Product Design browser surface was unavailable. Because this is a desktop-only Tauri app, QA used the current compiled debug executable and Windows `PrintWindow` capture. The captured executable path was `web/src-tauri/target/debug/mine-mail-desktop.exe`.

## Findings

- No actionable P0, P1, or P2 visual mismatch remains for the requested material change.
- The compose action now reads as an accent-tinted glass control rather than a flat saturated rectangle. Its sampled Daylight surface is `#266EC4`, giving white label text sufficient contrast while retaining environmental translucency.
- The message list uses the lighter frosted surface. Wallpaper color and light are perceptible through the panel without interfering with row scanning.
- The reader uses the more opaque paper-glass surface. It belongs to the same material family as the list while preserving long-form text readability.
- Search, selected row, footer, edge highlight, shadow softness, blur, and saturation now derive from the same semantic material system.

## Required fidelity surfaces

- Fonts and typography: existing Inter/Segoe UI fallbacks, weights, sizes, truncation, and reading line height remain unchanged. The material change introduces no wrapping or density regression.
- Spacing and layout rhythm: existing three-column proportions, panel gaps, radii, toolbar heights, and row density are preserved. The source and implementation differ only where live content and Windows DPI affect capture scale.
- Colors and visual tokens: Daylight, Night, Dusk, and Forest define theme-specific list, reader, control, selected-row, compose, edge, highlight, and shadow values. Unknown future themes inherit safe semantic defaults derived from their panel and accent colors.
- Image quality and asset fidelity: the original painterly wallpapers remain continuous beneath all columns. No replacement imagery, placeholder art, CSS illustration, or new icon family was introduced.
- Copy and content: static app copy is unchanged. The implementation uses live mailbox content, so the selected-message body is not expected to reproduce every line of the generated reference.
- Accessibility: reader and list surfaces stay near-opaque; body and secondary text remain high contrast. The Daylight compose control was darkened after measurement so its white label does not lose contrast through transparency.

## Interaction and runtime validation

- The real Tauri debug app launched and displayed the updated CSS through Windows WebView2.
- Inbox content and the selected message rendered successfully on the new surfaces.
- Existing hover, pressed, focus-visible, selected, and theme-switching behavior remains wired to the same components.
- React verification passed: 35 tests across 4 files.
- The React production build and complete Tauri Release/MSI/NSIS build passed.
- No SMTP action was triggered during visual QA.

## Comparison history

### Pass 1

- The first attempted capture was rejected because Tauri single-instance handling focused an older Release process. Pixel sampling confirmed the stale flat colors (`#0878F9` compose and `#FCFCFD` panels), so those screenshots were not accepted as implementation evidence.
- Fix: terminated only Mine Mail processes from this workspace and relaunched `target/debug/mine-mail-desktop.exe` after compilation completed.

### Pass 2

- The current implementation showed the intended shared material system and matched the source hierarchy.
- [P2] The initially translucent Daylight compose surface reduced white-label contrast too far.
- Fix: retained translucency but moved the Daylight, Dusk, and Forest accent glass toward darker theme-derived values. The final Daylight sample is `#266EC4`.

### Pass 3

- Post-fix evidence: `implementation-frosted-daylight-final.png`, `comparison-frosted-material-final.png`, and `comparison-frosted-material-focused.png`.
- No actionable P0, P1, or P2 issue remains.
- Residual P3: the generated source contains a slightly more luminous compose-button highlight than CSS can reproduce without adding a decorative gradient. The implementation intentionally uses a quieter inset highlight to remain consistent with the app's existing visual language.

## Implementation checklist

- [x] Add shared semantic frosted-material tokens.
- [x] Tune all four MVP themes.
- [x] Apply list and reader opacity hierarchy.
- [x] Convert compose, search, selected row, and footer to related materials.
- [x] Preserve readable content and accessible primary-action contrast.
- [x] Validate against the selected mock in the real Tauri desktop runtime.

final result: passed
