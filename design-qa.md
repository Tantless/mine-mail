# Design QA — Integrated Window Chrome

- Source visual truth: `C:\Users\tantl\Documents\xwechat_files\wxid_0qqrc78n3hvm12_adc0\temp\InputTemp\0caddea4-c2d6-4fb4-9828-1003a8a72356.png`
- Implementation screenshot: `Z:\mine-mail\design-qa\titlebar-integrated-current.png`
- Full-view comparison: `Z:\mine-mail\design-qa\titlebar-reference-comparison.png`
- Focused top-chrome comparison: `Z:\mine-mail\design-qa\titlebar-focused-comparison.png`
- Viewport: 1453 × 908 logical pixels / 2906 × 1815 physical capture pixels
- State: Windows, Dusk theme, inbox with a selected message

## Comparison Scope

The WeChat reference is directional rather than a full-screen clone. The comparison therefore evaluates the requested window-chrome behavior: a visually quiet draggable safe area, platform controls in their standard corner, no separate titlebar band, and one product identity at the top-left. Message content and navigation anatomy intentionally remain Mine Mail.

## Full-view Comparison Evidence

- The implementation keeps the three-column Mine Mail structure and its approved layered material system.
- The former full-width titlebar surface, divider, blur band, duplicate envelope, and duplicate `Mine Mail` text are absent.
- The painterly shell now continues uninterrupted behind the top draggable area, matching the reference's weak titlebar hierarchy without copying its flat palette.
- Windows minimize, maximize, and close controls remain at the top-right and clear of mail controls.
- The single Mine Mail brand is aligned with the left navigation, so it reads as part of the app rather than operating-system chrome.

## Focused Region Comparison Evidence

- Both screens reserve a shallow top safe area without presenting it as a separate content card.
- Both keep window controls visually lightweight and anchored at the window edge.
- The implementation's brand begins below the drag-safe strip, with spacing comparable to the reference avatar/search start while respecting Mine Mail's larger sidebar identity.

## Required Fidelity Surfaces

- Fonts and typography: Existing Inter/Segoe UI hierarchy is unchanged; removing the 12 px titlebar copy eliminates the duplicate, weaker brand treatment. The remaining sidebar brand has appropriate weight and scale.
- Spacing and layout rhythm: Top gutter, panel starts, and window controls do not collide. The three columns retain their approved proportions and rounded material edges.
- Colors and visual tokens: No titlebar surface or divider remains. Theme-specific foreground and hover tokens still keep controls legible across Daylight, Night, Dusk, and Forest.
- Image quality and asset fidelity: Existing wallpaper and icon-library assets are preserved without replacements or generated approximations.
- Copy and content: Exactly one visible `Mine Mail` label remains; mail labels and user content are unchanged.

## Findings

No actionable P0, P1, or P2 differences remain for the scoped titlebar integration.

## Comparison History

- Pass 1: The full and focused comparisons confirmed that the separate titlebar band and duplicate brand were removed, the platform controls stayed in position, and the three-column shell retained its hierarchy. No post-comparison P0/P1/P2 fix was required.

## Interaction Checks

- The full top safe area remains a deep Tauri drag region.
- Window controls remain excluded from dragging.
- Minimize, maximize/restore, and close controls remain present with their existing desktop actions and accessible labels.
- Responsive sidebar and mail selection behavior are unchanged.

## Follow-up Polish

- P3: A future pass could tune control hover opacity per theme after broader use, but the current treatment is already consistent and readable.

final result: passed
