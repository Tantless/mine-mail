# 让写信窗口最小化与恢复走合成层动画

Written against: d354819fa84987fdfe91ccde1e82459ab92b4ff3

## Evidence chain

- Surface: `web/src/App.jsx` 挂载的 `ComposePanel`，从展开写信页切换到窗口底部中央 340×44 摘要条，并通过摘要条或主侧栏“写信”恢复同一会话。
- Problem: `.compose-panel` 当前在 `--motion-window` 内同时 transition `left`、`top`、`width`、`height`，使完整写信内容在几何变化期间逐帧参与布局；`.compose-layer` 同时从全窗口 `backdrop-filter: blur(7px)` 插值到 `none`。恢复时完整 `RichTextEditor` 会在几何 transition 开始时重新挂载。这些工作与动画每帧重叠。
- Design evidence: `DESIGN.md > Compose` 明确要求最小化与恢复使用一个克制的 260 ms 窗口式几何和材质过渡；最小态固定为 340×44、位于底部中央，scrim 与全窗模糊消失，摘要条保留紧凑玻璃；`prefers-reduced-motion` 必须把过渡折叠为完整最终状态。
- Owner: `web/src/components/ComposePanel.jsx` 的 `geometry`、`isMinimized`、`windowMotion`、`minimizedGeometryRef`、`restoreRequest`、`restoreComposer` 与 `toggleMinimized`；`web/src/styles.css` 的 `.compose-layer`、`.compose-panel`、`.compose-expanded-shell` 和 `.compose-minimized-shell`。
- Scope and affected surfaces: 保存并最小化、点击 scrim 最小化、点击摘要条恢复、主“写信”动作恢复、账户切换后恢复各自会话，以及打开时已经最小化的会话。拖拽/缩放高频更新由另一份计划负责。
- Uncertainty: 尚未在 Windows WebView2、macOS WKWebView 和 Linux WebKitGTK 上记录 transition trace；FLIP 的边框、阴影和文字缩放观感必须在三类 WebView 中验证，但首尾几何与现有 timing 已由代码和设计规范确定。

## Design decision

保留现有 React 几何和最小化状态所有权，使用一次性布局提交加 FLIP 变换复现相同的 260 ms 窗口轨迹：在切换前读取当前 `.compose-panel` 矩形，立即提交目标几何和目标内容状态，在 `useLayoutEffect` 中读取目标矩形并施加从旧矩形映射到新矩形的 inverse `translate3d + scale`，下一帧只 transition `transform` 回到 identity。圆角、背景和阴影继续使用当前 `--motion-normal` 材质过渡；首尾尺寸、位置、缓动与内容淡入延迟不变。

把 compose scrim 从会插值 blur 半径的 `.compose-layer` 背景迁到该层的 `::before`：伪元素始终使用当前 7 px blur 和 overlay 材质，最小化时只 transition `opacity`。它不接收 pointer events，因此点击 layer 空白处最小化的现有事件目标不变。`confirm-layer` 继续使用自己的现有模糊表面，不继承 compose 的伪元素结构。

不得使用 View Transitions API、canvas 截图或第三方动画库。实现必须支持动画中反向操作：新请求先读取当前可见矩形、取消旧 RAF/完成计时器，再从该矩形重新 FLIP 到新目标，不能跳回上一阶段的逻辑起点。

## Reuse

- `web/src/styles.css` 的 `--motion-window`、`--motion-normal`、`--motion-fast`、`--motion-window-content-delay`、现有 cubic-bezier、`--overlay`、`--compose-page-surface` 与 `--compose-panel-surface`。
- `web/src/components/ComposePanel.jsx` 的 `constrainGeometry`、`loadInitialGeometry`、`minimizedGeometry`、`geometryRef`、`minimizedGeometryRef`、`restoreRequest` 和 `onMinimizedChange`。
- Exemplar: 当前 `compose-minimized-shell-in` 与 `compose-expanded-shell-in` 关键帧继续负责内容出现节奏；FLIP 只替换外层窗口几何驱动。

现有系统可表达该决定。FLIP 调度和 transition 完成逻辑属于 `ComposePanel` 私有实现，不新增全局窗口动画 primitive，也不把邮件阅读器/列表工作区迁移到本计划。

## Changes

1. `web/src/components/ComposePanel.jsx`
   - Change: 引入 `useLayoutEffect` 和一个写信窗口私有 motion ref，保存请求 token、起始可见矩形、目标几何、目标 minimized 状态、RAF id 与 fallback timer。集中实现 `startWindowMotion(targetMinimized)`，供 `toggleMinimized`、`restoreComposer` 和 `restoreRequest` 共用。
   - Change: 每次 motion 先从 `dialogRef.current.getBoundingClientRect()` 读取当前可见矩形并取消旧 motion；计算目标普通或 `minimizedGeometry`，同步更新 `geometryRef`、React `geometry`、`isMinimized` 与 `windowMotion`。`useLayoutEffect` 在目标 DOM 提交后计算 inverse translate/scale，设置 `transform-origin: top left`，强制起始变换在首帧生效，再于下一个 RAF 移除 inverse 以触发 `--motion-window` transform transition。
   - Change: 仅接受 `.compose-panel` 自身的 `transitionend` 且 `propertyName === "transform"` 作为完成信号，并保留 `--motion-window` 加小幅安全余量的 timer fallback。完成或被新请求打断时取消 RAF/timer、清除临时 transform 与 transform-origin、把 `windowMotion` 设回 null；旧 token 的回调不得覆盖新 motion。
   - Change: `prefers-reduced-motion: reduce` 时不安排 RAF、不施加 inverse transform、不保留 staged content delay，直接提交最终几何、内容和 minimized 状态。组件卸载、窗口 resize、关闭或账户会话切换必须取消 motion；resize 后用现有约束重新计算最终普通几何或最小化几何。
   - Preserve: `onSaveDraft` 成功后才最小化；保存失败保持展开；`onMinimizedChange` 的状态语义、普通几何记忆、340×44 摘要、底部 18 px、关闭按钮、摘要文案、主“写信”恢复同一会话、焦点陷阱和账户独立 compose session 不变。
   - Verify: 最小化和恢复结束后不存在内联 transform、活动 RAF/timer 或陈旧 `data-window-motion`；快速反向操作从当前可见位置连续运动，不闪回完整窗或摘要条。

2. `web/src/styles.css`
   - Change: 从 `.compose-panel` transition 中移除 `left/top/width/height` 和常驻 `will-change: left, top, width, height`；保留圆角、背景、阴影的 `--motion-normal` 过渡，并在 `[data-window-motion]` 上增加 `transform var(--motion-window)` 与仅活动期间的 `will-change: transform`。静止、完成和 reduced-motion 状态恢复 `will-change: auto`。
   - Change: 把 `.compose-layer` 的背景与 backdrop filter 移到 `.compose-layer::before`，伪元素固定覆盖窗口、复用当前 overlay/7 px blur/saturation，`pointer-events: none`，只 transition `opacity var(--motion-normal) ease-out`；`[data-minimized="true"]::before` opacity 为 0。确保 `.compose-panel` 位于伪元素之上，最小化时 layer 仍 pointer-events none 而 panel 恢复 pointer-events auto。
   - Change: 保留 `compose-in` 的首次打开动画、`compose-minimized-shell-in`、`compose-expanded-shell-in` 和 `--motion-window-content-delay`；仅调整选择器，使 content animation 与新的 `minimizing/restoring` 生命周期对应，并在 motion 完成后不重播。
   - Preserve: expanded page 的实色表面、minimized bar 的 18 px backdrop blur、阴影、边框、圆角、hover/focus 整体高亮和 z-index 层级；`confirm-layer` 的现有背景与 blur 不变。
   - Verify: 动画中的唯一几何 transition 属性是 `transform`；scrim 的唯一动态属性是 opacity，首尾视觉仍分别为当前完整模糊遮罩和完全透明。

3. `web/src/components/ComposePanel.test.jsx`
   - Change: 增加可控 `getBoundingClientRect`、RAF、timer 和 `matchMedia` 夹具。分别验证 minimizing/restoring 的 start rect、target geometry、inverse transform、下一帧 identity、合法 transitionend 清理及 fallback 清理。
   - Change: 覆盖 motion 尚未完成时立即反向，断言旧 token 失效且新 motion 从当前 rect 开始；覆盖 reduced-motion，断言最终状态原子到达且没有 transform/RAF/timer。
   - Change: 验证卸载和 window resize 会清理进行中的 motion，并保留最近普通几何；伪造子元素 transitionend 不得提前结束外层 motion。
   - Preserve: 摘要文案、焦点、Escape、附件、信纸、只读与收件人弹层测试不变。
   - Verify: 测试能够失败于旧的 `left/top/width/height` transition 生命周期，并明确证明 motion 完成后没有资源泄漏。

4. `web/src/App.test.jsx`
   - Change: 保留现有组合用例中的拖动、缩放、持久化、保存并最小化、恢复和重新打开流程；增加 transition 完成/RAF 冲刷，使断言分别覆盖 motion 中和 motion 后状态。若先实施拖拽帧合并计划，复用同一可控 RAF helper，避免两套不一致的测试时钟。
   - Preserve: `App` 只传递 `initiallyMinimized`、`restoreRequest` 和 `onMinimizedChange`，不接管 FLIP 或 DOM rect。
   - Verify: 主“写信”按钮恢复已有 minimized session、保存失败不最小化和关闭摘要条等产品行为没有变化。

5. `web/src/App.desktop.test.jsx`
   - Change: 在已有每账户独立 compose session、切换前保存和失败保留 minimized draft 用例中冲刷新 motion 生命周期；增加一次账户切换返回后通过主“写信”恢复的断言，确保 FLIP 清理不会跨账户遗留。
   - Preserve: 草稿版本、自动保存、账户切换和网络行为断言不变，不为动画引入后端等待。
   - Verify: 每个账户仍最多一个 compose surface，隐藏账户不会留下活动 motion timer 或影响当前账户几何。

6. `web/src/styles.test.js`
   - Change: 把当前要求 `.compose-panel` transition width/height 的断言替换为 compositor motion 契约：静止 panel 不 transition `left/top/width/height`，活动 window motion transition `transform var(--motion-window)` 且只在活动状态声明 `will-change: transform`。
   - Change: 断言 `.compose-layer::before` 持有当前 overlay 和固定 backdrop filter、只 transition opacity，minimized selector 将其 opacity 设为 0；`.compose-layer` 本身不再 transition backdrop-filter，`.confirm-layer` 仍保留原材质。
   - Change: reduced-motion 块必须移除 compose transform、清除 content delay，并让最终 expanded/minimized content 完整可见。
   - Preserve: 现有实色写信页、圆角编辑器、最小摘要 hover 和四主题 token 断言。
   - Verify: 样式测试检查属性类别和现有 token，不硬编码另一套时长或 easing。

## Scope

- Inherit: 新邮件、草稿、回复、转发、只读草稿、账户独立 minimized session、scrim 点击和主“写信”恢复均通过同一个 `ComposePanel` 获得新动画路径。
- Verify: Daylight、Night、Dusk、Forest；1440×900、1250 px、940 px、720 px 和 1050×680；普通长草稿、空草稿、附件、回复/转发上下文、富文本和信纸；minimize、restore、快速反向、窗口 resize、reduced-motion、隐藏/恢复应用窗口。
- Exclude: 拖拽/缩放 pointermove 合并、邮件阅读器窗口动画、列表收起/工作区切换、弹层定位、compose 数据/版本/发送流程、原生系统窗口动画、第三方动画依赖和任何 Rust 代码。

## Validation

- Product: 在含长富文本、附件和回复上下文的写信页连续执行至少 10 次最小化/恢复，并在一次 260 ms 过渡中立即反向；预期窗口轨迹、内容延迟、scrim 淡出、340×44 终态和恢复几何与当前视觉一致，无闪白、裁切、文字跳回或焦点丢失。
- Interface: 四主题及所有相关重排检查展开/最小、hover/focus、保存中、保存失败、只读、快速反向和 reduced-motion；确认 minimized bar 的自身玻璃不受 scrim 迁移影响，confirm dialog 材质没有变化。
- System: 在 Tauri 的 Windows、macOS、Linux 可用目标至少各记录一次 Performance trace；预期 minimize/restore 期间 `.compose-panel` 不再逐帧产生 layout，scrim 不再逐帧插值 blur 半径，主动画由 transform/opacity 合成。不要把 trace 或截图提交仓库。
- Repository: `cd web && npm test -- --run` → 组件、App、桌面集成和样式契约全部通过。
- Repository: `cd web && npm run build` → Vite 生产构建成功。
- Repository: `git diff --check` → 无空白错误，且实现 diff 不覆盖当前工作树中与本计划无关的用户改动。

## Stop conditions

- Stop if 任一目标 WebView 无法在保持当前 260 ms 首尾几何、圆角、阴影和材质观感的同时稳定执行 inverse transform，或 transform 导致 Tiptap 文本、插入光标、recipient portal、附件弹层出现可见缩放/脱节；保存最小 trace 后重新确认方案，不得静默降低效果。
- Stop if scrim 伪元素改变点击空白处最小化、layer pointer-events、confirm dialog 材质或 z-index；先恢复现有交互所有权，不能用额外全屏按钮或新 overlay primitive 扩大范围。
- Stop if实现需要改变草稿保存时机、账户 session、焦点规则、摘要文案或 340×44/260 ms 合同；这些属于产品或设计变更，必须另行批准并更新规范。

## Design documentation

- After acceptance and validation: none。`DESIGN.md > Compose` 已规定 260 ms 窗口式几何与材质结果、最小态尺寸、scrim 和 reduced-motion；本计划只把实现迁移到 compositor-friendly 路径，不改变规范。
