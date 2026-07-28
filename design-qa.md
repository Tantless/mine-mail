# Compose editor stationery and footer QA

- Source visual truth: OS temporary files `QQ_1785208429168.png`
  (lined paper), `QQ_1785208452629.png` (grid paper), and
  `QQ_1785218787096.png` (plain editor focus regression).
- Current implementation screenshots: OS temporary files
  `mine-mail-compose-paper-qa-footer.png`,
  `mine-mail-compose-paper-qa-lined.png`, and
  `mine-mail-compose-paper-qa-grid.png`.
- Source full-window pixels: 2884 × 1804.
- Implementation full-window pixels: 1549 × 925.
- Comparison viewport: the compose editor surface, cropped at native capture
  density. The grid focused comparison uses equal 740 × 170 px regions without
  resampling.
- State: Daylight theme, new message, plain paper-off state, lined paper, and
  grid paper with mixed Latin, Han, and whitespace content.

## Full-view evidence

The full-window captures confirm that the compose page remains the existing solid
writing page and that the editor keeps its 12 px rounded inset surface. A
temporary compose-only QA geometry was used to fit the complete editor and footer
inside the current high-DPI native capture; it was removed before verification.
The captured geometry is evidence for component anatomy and alignment, not a new
runtime window size.

## Focused-region evidence

The source and implementation editor regions were inspected together. Plain,
lined, and grid modes retain the same outer editor margin, radius, and focus
treatment. The paper area now has an inset on every edge and extends slightly
beyond the shared writing origin. Lined text and plain text begin at the same
point; the first grid cell centers a Han glyph on that point. The first lined
rule remains below the first writing row, and both paper backgrounds move with
the ProseMirror content.

The mixed grid capture verifies the requested sequence: `asd` is centered in one
cell, `1a` in the next, each following Han character in its own centered cell,
and the space before `的` consumes one blank cell. The generated cell wrappers
are editor decorations and do not enter authored HTML.

The footer capture verifies the neutral icon-only paper-off state. Enabling paper
reveals two pill tracks with sliding accent thumbs: rows/grid for paper type and
pencil/paper-plane for edit-only/send-with-message. Tooltips provide the Chinese
labels, and the radio groups support arrow, Home, and End keys. The 74 px
save-status slot remains constant for every save copy, preventing adjacent
controls from moving.

## Fidelity surfaces

- Typography: unchanged editor font, size, weight, line-height behavior, and
  placeholder copy.
- Spacing and layout: retains the prior 12 px radius, 18 × 20 px writing origin,
  adds 10 × 12 px paper-edge insets, and keeps stable 12 × 22 px outer spacing
  across stationery modes.
- Colors and tokens: uses the existing compose surface, divider, paper, and
  primary-focus tokens; no new palette was introduced.
- Image quality: no raster or decorative image assets are part of this bounded
  editor surface.
- Copy: unchanged.

## Comparison history

1. Earlier P1: the editor shell had been flattened to a borderless rectangle,
   while lined/grid modes introduced a separate frame and different margins.
   Fixed by restoring one shared rounded editor shell for every mode.
2. Earlier P1: the focusable ProseMirror node inherited the global focus shadow,
   creating a nested square blue frame or full-height blue stripe. Fixed by
   keeping focus feedback on the rounded shell and suppressing the inner shadow.
3. Earlier P1: grid paper used document-wide letter spacing, which made Latin
   input consume one cell per character and displaced Han following Latin.
   Fixed with Unicode-aware editor decorations and cell-centered groups.
4. Earlier P2: paper rules touched the editor edge and the three modes did not
   share a dependable writing origin. Fixed with separate paper-edge and
   writing-origin insets.
5. Earlier P2: variable save-status copy moved the stationery controls. Fixed
   with a stable status slot.
6. Post-fix evidence: actual desktop captures show consistent paper margins,
   correct mixed-script grouping, compact animated controls, and the retained
   rounded editor shell.

No actionable P0, P1, or P2 visual mismatch remains in the requested editor
surface.

final result: passed
