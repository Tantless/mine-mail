# Tooltip Design QA

- Source visual truth path: user-provided screenshot in the current conversation; no local file path is available.
- Implementation screenshot path: unavailable because the in-app browser capture runtime is not exposed in this session.
- Viewport: not captured.
- Source pixel dimensions: 1280 × 960 pixels as provided in the conversation.
- Implementation pixel dimensions: not captured.
- CSS size and density normalization: not available.
- State: icon-button hover tooltip, with the mail reader toolbar as the primary target.

## Full-view comparison evidence

Blocked. The implementation preview is running locally, but a browser-rendered screenshot could not be captured in this session. Build output and DOM tests are not substitutes for visual comparison.

## Focused region comparison evidence

Blocked for the same reason. The intended focused region is the reader toolbar's “返回邮件列表” button and its open tooltip.

## Required fidelity surfaces

- Fonts and typography: implemented with the app's inherited font stack, 12px variable weight, and 1.35 line height; visual comparison remains pending.
- Spacing and layout rhythm: implemented with 7px × 10px padding, 8px radius, an 8px trigger gap, viewport clamping, and automatic top/bottom placement; visual comparison remains pending.
- Colors and visual tokens: implemented from the existing theme panel, border, text, highlight, shadow, and motion tokens; visual comparison remains pending across all four themes.
- Image quality and asset fidelity: no image assets are used by this component.
- Copy and content: the tooltip uses the existing IconButton label, including “返回邮件列表”.

## Findings

- No code-level P0/P1/P2 issue is known after automated verification.
- Visual fidelity, clipping, and perceived animation timing cannot be passed without a browser-rendered hover-state capture.

## Primary interactions tested

- Delayed pointer hover open.
- Pointer leave close.
- Immediate keyboard-focus open.
- Escape close.
- Disabled icon-button hover explanation.
- Existing consumer event handlers remain active.

Browser console errors checked: no; browser capture unavailable.

## Comparison history

- Initial implementation: automated component and application tests passed; no visual comparison iteration was possible.

## Implementation checklist

- Capture the reader toolbar hover state in the running preview.
- Compare the source and implementation in one image input.
- Check daylight, night, dusk, and forest theme contrast.
- Confirm edge avoidance at the left, right, top, and bottom of the window.

final result: blocked

Blocker: browser-rendered evidence is unavailable in this session, so the required visual comparison cannot be completed.
