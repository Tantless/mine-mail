# Mine Mail — Simplified Composer Design QA

## Evidence

- Source visual truth: `web/design/qa/implementation-compose-glass-daylight-final.png`
- Final normal composer: `web/design/qa/implementation-compose-refined-full-final.png`
- Final minimized composer: `web/design/qa/implementation-compose-refined-minimized-final.png`
- Full before/after comparison: `web/design/qa/comparison-compose-refined-before-after.png`
- Normal/minimized state comparison: `web/design/qa/comparison-compose-refined-states.png`
- Focused chrome/control comparison: `web/design/qa/comparison-compose-refined-focused.png`
- Collapsed copy-recipient focus: `web/design/qa/implementation-compose-copy-focus-collapsed.png`
- Expanded CC focus: `web/design/qa/implementation-compose-copy-focus-expanded.png`
- Rounded-only input focus: `web/design/qa/implementation-compose-focus-rounded-only.png`
- Source and implementation viewport: 3098 × 1850 physical pixels (1549 × 925 logical pixels at 2× capture scale).
- State: Windows Tauri/WebView2, Daylight theme, configured local mailbox, empty new-message composer.

The implementation evidence comes from the rebuilt Tauri debug executable. The final captures use the live cached mailbox and Windows `PrintWindow`; no parallel browser mail runtime or mock inbox was used.

## Findings

- No actionable P0, P1, or P2 mismatch remains for the requested simplification.
- The visible composer title/status band and maximize action are removed. The top edge is now uninterrupted glass with only minimize and close at the right, while the empty left area remains the drag target.
- **保存并关闭** is now a Phosphor floppy-disk icon button with the same size, material, hover treatment, tooltip, and accessible name as the discard control.
- The minimized state is a 340 × 44 subject-only glass bar centered 18 pixels above the bottom edge. It contains no status dot, draft status, or right-side controls.
- The compose overlay becomes fully transparent and drops its backdrop filter while minimized, so the mailbox below remains sharp and readable.
- Empty subjects render as **新邮件**. A non-empty subject is truncated to one centered line when necessary.
- Drag and resize changes persist as a bounded `{x, y, width, height}` UI preference. New compose sessions and later app launches reuse it; minimize/restore does not overwrite the saved normal geometry.
- Recipient, CC, BCC, and subject focus now sits inside a rounded inset surface rather than highlighting the full rectangular row. The copy-recipient icon remains outside that focus surface and toggles both optional rows without discarding their values.
- Compose inputs suppress the global native-element focus shadow, leaving one rounded focus surface instead of a nested rectangular ring.

## Required fidelity surfaces

- Fonts and typography: existing Inter/Segoe UI fallback, weights, field labels, placeholders, and footer hierarchy are unchanged. The minimized subject uses the existing 12px semibold UI scale with single-line ellipsis.
- Spacing and layout rhythm: the original default 660 × 680 surface and right-side placement are preserved for first use. Removing the visible header tightens the hierarchy without moving the address, body, or footer controls out of alignment.
- Colors and visual tokens: the approved compose glass, wallpaper reflection, borders, shadows, focus ring, and theme tokens are preserved. The compact bar uses the same semantic surface rather than introducing a new solid color.
- Image quality and asset fidelity: the active painterly wallpaper remains the only raster source. No generated filler, CSS illustration, inline SVG, or extra icon family was added.
- Copy and content: minimized copy follows the exact subject-or-**新邮件** rule. Existing recipient, subject, body, send, save, discard, and draft-status copy remains wired to live state.
- Accessibility: the removed visual heading remains available as a clipped semantic heading; both top actions and both footer actions retain accessible names and focus states. Pointer resizing has large edge/corner targets, and the minimized bar is a single keyboard-accessible restore button.

## Interaction validation

- Real Tauri UI Automation invoked the compose action, minimize action, and subject-bar restore action successfully.
- React coverage exercises drag, southeast resize, geometry persistence, close/reopen restoration, subject and empty-subject minimized labels, compact geometry, and restore-to-previous geometry.
- React coverage also exercises CC/BCC expansion, collapse, repeated expansion, and retained address values.
- The minimized layer exposes the sharp mailbox and permits pointer interaction outside the compact bar.
- Mail send, local draft save, remote draft synchronization, discard, and recipient confirmation logic were not changed.

## Comparison history

### Final pass

- The full-view comparison confirms the old titled chrome and maximize button are gone while the glass shell and default placement remain stable.
- The focused comparison confirms the footer text action has become an icon and the top-right control set contains only minimize and close.
- The state comparison confirms the minimized bar is compact, centered, subject-only, and leaves the underlying mailbox unblurred.
- No actionable P0, P1, or P2 issue remains.
- Residual P3: the compact width is fixed at 340 logical pixels, clamped on narrower windows. It can be tuned later if hands-on use favors a slightly shorter or longer subject preview.

## Verification

- Core Rust: 51 tests passed (49 library + 2 send-confirmation).
- React: 41 tests across 5 files passed.
- Tauri: 15 Rust tests and `cargo check` passed.
- Production frontend build: passed.
- Embedded-assets Tauri debug build: passed.
- Real Tauri normal/minimized and collapsed/expanded recipient visual capture: passed.
- No SMTP action was triggered during visual QA.

final result: passed
