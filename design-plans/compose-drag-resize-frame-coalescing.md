# 让写信窗口拖拽与缩放按显示帧更新

Written against: d354819fa84987fdfe91ccde1e82459ab92b4ff3

## Evidence chain

- Surface: `web/src/App.jsx` 挂载的 `ComposePanel` 展开态写信窗口；交互路径为 `.compose-drag-surface`、八个 `.compose-resize-handle` 和 `.compose-panel` 的内联几何。
- Problem: `window` 上的每个 `pointermove` 都调用 `commitGeometry`，后者立即执行 `setGeometry`。一次拖动或缩放会反复重渲染包含三个 `RecipientInput`、懒加载 `RichTextEditor`、附件与回复/转发上下文的完整 `ComposePanel`；输入事件没有按显示帧合并。
- Design evidence: `DESIGN.md > Compose` 要求写信页可拖拽、可从边缘调整大小、始终位于可见应用边界内，并恢复最近一次有效普通几何；该优化不得改变这些结果。`web/src/styles.css` 已提供交互态 `data-interacting` 和共享 motion 规则。
- Owner: `web/src/components/ComposePanel.jsx` 中的 `geometryLimits`、`constrainGeometry`、`persistGeometry`、`geometryRef`、`interactionRef`、`beginDrag`、`beginResize` 与 `endInteraction`。
- Scope and affected surfaces: 新邮件、回复、转发、打开已有草稿和只读草稿使用的同一个展开态 `ComposePanel`；覆盖鼠标和触控板产生的 Pointer Events。最小化/恢复窗口动画由单独计划负责。
- Uncertainty: 尚未采集 Tauri WebView 的 React Profiler 或 Performance trace；源码已证明更新频率不受显示帧约束，实际收益需用长富文本草稿在目标平台验证。

## Design decision

让 React state 继续拥有稳定、可持久化的写信窗口几何，但不再承接手势中的每一个指针采样。手势开始后把最新指针位置和受约束的目标几何保存在 ref 中，并通过一个 `requestAnimationFrame` 队列把 DOM 写入合并为每帧最多一次：拖动只在 `.compose-panel` 上应用临时 `translate3d`，缩放只在帧回调中直接写入 `left`、`top`、`width`、`height`。`pointerup`、`pointercancel` 或组件卸载时先同步冲刷最后一个待处理采样，再清除临时 transform，最后只提交一次 React 几何并按现有格式持久化。

这一决定保留当前指针跟随、八方向缩放、最小尺寸、窗口边界和最终内联几何；它只减少 React 提交和 DOM 几何写入次数。不要用 `React.memo` 掩盖父组件的高频 state 更新，也不要加入第三方动画库。

## Reuse

- `web/src/components/ComposePanel.jsx` 的 `geometryLimits`、`constrainGeometry`、`persistGeometry`、`geometryRef` 与 `composeGeometryStorageKey`。
- `web/src/styles.css` 的 `.compose-panel[data-interacting]`、现有 cursor、resize handle 和全局 `prefers-reduced-motion` 规则。
- Exemplar: `web/src/components/RecipientInput.jsx` 的单一 RAF 句柄与卸载时 `cancelAnimationFrame` 模式；这里只复用生命周期方式，不复用其弹层定位逻辑。

现有组件可以表达该决定；不新增共享 motion primitive。帧调度 helper 留在 `ComposePanel.jsx`，因为它依赖写信窗口专有几何、边界和持久化规则。

## Changes

1. `web/src/components/ComposePanel.jsx`
   - Change: 扩展 `interactionRef`，保存交互类型、起点、起始几何、最新指针坐标、最后计算的受约束几何和唯一 RAF id；把当前 `pointermove` 内的几何计算抽成一个纯本地 helper，拖动与八方向缩放继续调用原有 `constrainGeometry`/`geometryLimits`。
   - Change: `pointermove` 只更新 ref 并在没有待处理帧时安排一个 RAF。帧回调读取最新采样；拖动时保持 React 的基准 `left/top/width/height` 不变，仅写 `translate3d(dx, dy, 0)`；缩放时一次写入目标 `left/top/width/height`。同一帧内无论收到多少 move，只执行一次样式写入。
   - Change: `endInteraction` 必须在提交前冲刷最新采样，防止紧邻 `pointerup` 的最后一次移动丢失；把最终几何写入 DOM、更新 `geometryRef`、清除拖动 transform，再执行一次 `setGeometry` 和现有 `persistGeometry`。清理 RAF、ref、body cursor 与 `user-select`，卸载和 `pointercancel` 走同一路径。
   - Change: 将 `data-interacting` 从通用的 `true` 调整为 `drag` 或 `resize`，使样式能只在拖动期间声明 transform 合成提示；开始/结束交互仍各只产生一次 React 提交。
   - Preserve: 左键限制、表单控件和 `[data-no-compose-drag]` 排除、八个缩放方向、22 px 外边界、52 px 顶边界、680×520 普通最小值、窄视口降级、普通几何 localStorage 格式以及最小化窗口不可缩放。
   - Verify: 拖动视觉位置和缩放尺寸与当前计算结果逐像素一致；手势结束后 `.compose-panel` 没有残留 transform，内联四项几何等于持久化值。

2. `web/src/styles.css`
   - Change: 保留 `.compose-panel[data-interacting] { transition: none; }`，并仅在 `[data-interacting="drag"]` 上设置 `will-change: transform`；交互结束立即恢复 `will-change: auto`。缩放仍按帧改变布局尺寸，不声明无效的长期 `will-change: left, top, width, height`。
   - Preserve: 面板表面、阴影、圆角、拖拽条、resize handle 命中区与 cursor 完全不变；本计划不改最小化/恢复的 260 ms transition。
   - Verify: 静止的展开写信窗口不因本计划长期占用额外合成层，拖动开始后只提升运动面板。

3. `web/src/components/ComposePanel.test.jsx`
   - Change: 增加可控 RAF 测试夹具。连续发送多个 `pointermove`，断言只排队一个帧；执行帧后仅最新坐标生效；`pointerup` 前仍有待处理采样时，结束逻辑会同步采用最后坐标。
   - Change: 分别覆盖拖动、东南缩放和会改变 `left/top` 的西北缩放，断言 `constrainGeometry` 约束、最终内联几何、localStorage 四项整数值、RAF 取消和临时 transform 清理。
   - Preserve: 现有焦点陷阱、Escape、附件、信纸和最小化摘要测试不变。
   - Verify: 测试能够失败于“每个 pointermove 直接 setState”的旧实现，而不是只验证最终位置。

4. `web/src/App.test.jsx`
   - Change: 调整现有 “moves, resizes, persists, minimizes, and restores” 集成用例以冲刷 RAF；保留其拖动、缩放、持久化、最小化、恢复和重新打开后恢复几何的完整断言。
   - Preserve: `App` 不获得新的几何状态或回调；`ComposePanel` 仍是唯一窗口几何所有者。
   - Verify: 集成测试证明帧合并没有改变写信入口、保存并最小化、关闭或重新打开流程。

5. `web/src/styles.test.js`
   - Change: 为交互态增加契约断言：drag 使用 `will-change: transform`，resize 不保留长期非合成属性提示，静止面板不因本计划新增永久 transform。
   - Preserve: 本计划不改现有最小化/恢复 width/height transition 断言；该断言由独立的 compositor motion 计划处理。
   - Verify: 样式测试区分 `drag`、`resize` 和静止状态，避免把 `will-change` 再次扩散到常驻面板。

## Scope

- Inherit: 所有账户的新邮件、草稿、回复和转发写信会话自动复用同一交互路径；包含长富文本、信纸、附件及只读上下文的窗口无需分别实现。
- Verify: 1440×900、1250 px、940 px、720 px 防御性布局和 1050×680 原生最小窗口；八个 resize handle；快速/慢速拖动；指针移出面板后释放；`pointercancel`；交互中窗口尺寸改变；四主题。
- Exclude: 最小化/恢复动画实现、scrim 模糊、富文本编辑器内部输入性能、窗口吸附、惯性拖动、触摸手势设计、Tauri 原生窗口移动、邮件保存/发送状态和任何 Rust 代码。

## Validation

- Product: 打开含长 HTML、多个收件人、附件和回复上下文的草稿，连续拖动 3 秒并从四边和四角缩放；预期窗口跟手、内容不闪烁、尺寸约束与当前一致，释放后重新打开仍恢复最后几何。
- Interface: 在四主题和相关重排宽度检查拖动条、resize handle、cursor、焦点、文本选择抑制和释放后的可编辑状态；确保拖动期间没有意外 transition，缩放仍实时重排编辑器。
- System: 用 React Profiler 或等价提交计数验证 pointerdown 和 pointerup 之间没有随每个 `pointermove` 发生的 `ComposePanel`/`RichTextEditor` React commit；用 Performance trace 验证同一显示帧最多一次几何写入。不要把临时 profiling 文件或截图提交仓库。
- Repository: `cd web && npm test -- --run` → 组件、App 集成和样式测试全部通过。
- Repository: `cd web && npm run build` → Vite 生产构建成功。
- Repository: `git diff --check` → 无空白错误，且实现 diff 不覆盖当前工作树中与本计划无关的用户改动。

## Stop conditions

- Stop if 目标 Tauri WebView 在实际设备上不能稳定合成 `translate3d`，或按帧直接写尺寸导致光标、选区、Tiptap 布局或 recipient portal 与窗口脱节；先保存 trace 和最小复现，再重新评估运动宿主，不能用降低视觉刷新率或隐藏内容规避。
- Stop if实现必须改变几何约束、持久化格式、resize handle、拖动命中区或用户可见节奏；这些都超出纯性能优化。

## Design documentation

- After acceptance and validation: none。`DESIGN.md > Compose` 已完整规定可拖拽、可调整大小、边界与几何恢复；本计划只替换内部更新路径，不改变设计决定。
