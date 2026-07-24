# Mine Mail Windows Installer Design QA

- Source visual truth:
  `C:\Users\tantl\.codex\generated_images\019f91fb-d608-7180-ae38-f00def2d3b7e\call_Afr44bomlIJRCqE5170ewbXu.png`
- Supporting approved cute artwork:
  `C:\Users\tantl\.codex\generated_images\019f91fb-d608-7180-ae38-f00def2d3b7e\call_maWhiqX3PwgXONOTelJfFIad.png`
- Implementation URL: `http://127.0.0.1:1431`
- Intended desktop viewport: 940 × 610 CSS px at device scale factor 1
- States: ready, installing, close-blocked, success, error
- Implementation screenshots:
  - ready: `%TEMP%\mine-mail-installer-qa\ready-settled.png`
  - success: `%TEMP%\mine-mail-installer-qa\success-final.png`
- Source pixels: 1536 × 1024 for the full visual reference
- Density normalization: implementation captures use the native 940 × 610
  installer viewport.

## Full-view comparison evidence

The ready and success surfaces were captured at the target viewport after the
entry animation settled. The fixed-size shell remains inside the viewport with
no clipping. The brand lockup now keeps the smaller fox and `Mine Mail` on one
line. The completion view fits its editorial line, both real setting controls,
install path and launch action without reintroducing the former oversized
completion symbol.

## Focused region comparison evidence

1. Frameless top-right minimize and close controls remain visually integrated
   into the body.
2. The smaller logo and brand title fit on one line with balanced spacing.
3. Completed rail items retain their envelope/package icons; a pale green row
   background communicates completion without substituting checkmarks.
4. The success surface has no large checkmark and uses contrasting display,
   editorial and utility typography.
5. Desktop shortcut and autostart are compact switch controls, not decorative
   checkboxes, and stay inside the completion layout.

## Findings

- No open P0 or P1 issue was found in the ready or success capture.
- P2 — native-window verification still belongs to the release runner.
  - Evidence: local Windows Application Control can block a newly linked,
    unsigned native installer shell.
  - Impact: the WebView-rendered pixels are verified, while final executable
    launch, shortcut mutation and registry mutation still need the packaged
    Windows artifact.
  - Fix: run the installer job on GitHub's Windows runner and smoke-test the
    downloaded setup executable before publishing the release.

## Required fidelity surfaces

- Fonts and typography: bundled Nunito plus Chinese system fallbacks are
  rendered cleanly; the completion line uses a restrained Chinese editorial
  fallback for hierarchy.
- Spacing and layout rhythm: fixed 940 × 610 single-window composition is
  implemented and confirmed in the ready and success states.
- Colors and visual tokens: approved warm white, coral orange, deep navy and
  blue accents remain consistent; green is reserved for completed rail rows and
  enabled switches.
- Image quality and asset fidelity: the repository's official transparent fox
  asset is reused without approximation and fits the one-line brand lockup.
- Copy and content: the visible interface does not describe itself as
  "futuristic"; the tone is product-focused and all installation states use
  concise Chinese copy.

## Interaction checks completed

- Ready state exposes the real install location.
- Installing state replaces the ready state in the same window.
- Close during installation opens a branded in-window notice.
- Success state appears only after the install task resolves.
- Static step artwork is not used; step state is derived from the real
  lifecycle.
- Completed steps retain their original icons and use green background state.
- Desktop shortcut and autostart switches call real Rust commands; they are not
  presentation-only controls.
- Frontend unit and interaction tests: 6 passed.
- Browser-preview HTTP response: 200.

## Comparison history

- Iteration 1: reduced the fox lockup to 56 px so the brand remains on one line.
- Iteration 2: removed completion checkmark artwork, retained stable rail icons
  and added green completed-row state.
- Iteration 3: added the two completion switches, shortened helper copy and
  confirmed the final 940 × 610 composition without clipping.

## Implementation checklist

- Capture installing, close-blocked and error states from the packaged native
  shell.
- Smoke-test desktop shortcut removal/recreation and the `--background`
  autostart entry.
- Re-run the native setup build on GitHub's Windows runner.

final result: pass with native packaging follow-up
