# 让邮件列表触底加载保持连续且可感知

Written against: f46316a0906793123c5da074f86ccb41f80a4a4b

## Evidence chain

- Surface: `web/src/App.jsx` 渲染的邮件工作区中栏，运行路径为 `MailList` 的 `.message-list`、`.mail-list` 与 `.mail-pagination-sentinel`。
- Problem: 用户提供的运行时截图显示，滚动到邮件列表末端并等待下一页时，最后一封邮件与面板底边之间没有任何加载反馈；现有 1 px 哨兵虽然已触发异步分页，但 `loading` 状态没有可见结果，滚动因此像突然撞到硬边界。
- Design evidence: 用户明确选择在列表末端加入可继续上拉的有界缓冲区，并让“正在加载”位于该空白区中央；`DESIGN.md` 的滚动规范允许在内容末端使用有界 inset，并要求加载不得替换已经可用的缓存行；`web/src/styles.css` 已声明 `--motion-fast`、`--motion-normal`、语义文字色和共享 `spin` 动画。
- Owner: `web/src/components/MailList.jsx` 拥有滚动容器、分页 phase 归一化、64 px 触发阈值和末端哨兵；`web/src/App.jsx` 已把 `loadMorePhase === "loading"` 映射为 `loadMoreState="loading"`，无需新增分页状态源。
- Scope and affected surfaces: 由同一个 `MailList` 承载且支持自动分页的 Inbox、Sent、Archive、Trash、Starred 聚合列表及分页搜索结果；在宽屏三栏、1250/940 px 重排和 720 px 单栏状态中复用同一表现。Drafts、Outbox、联系人列表、正文阅读区和显式同步状态带不在改动范围内。
- Uncertainty: 精确网络等待时长不可预测，但不影响状态契约；视觉验收应使用可控的延迟分页响应，而不是依赖真实邮箱网络。

## Design decision

把现有 `.mail-pagination-sentinel` 扩展为同一个元素拥有的双态内容末端：空闲时继续保持 1 px 并负责预取触发；下一页请求处于 `loading` 时，在列表内容之后展开为 64 px 高的有界缓冲区，使用现有 `CircleNotch` 和文案 **正在加载更多邮件…** 在区域内水平、垂直居中。缓冲区不使用卡片、边框、独立底色或阴影，只以静默的次要文字色和主题强调色旋转图标表达进行中状态。

缓冲区通过 `--motion-normal` 展开和收回。用户到达旧列表末端后仍可继续向下滚入这 64 px，因此加载反馈会自然进入视口；页面追加完成后，新邮件行占据原缓冲区之后的位置，哨兵回到 1 px。不得调用 `scrollIntoView`、主动改写 `scrollTop`、替换现有行或播放逐行入场动画。`retry`、`offline`、`unavailable` 与 `complete` 继续不显示底部文案，也不增加手动“加载更多”按钮。

## Reuse

- `web/src/components/MailList.jsx` 中已有的 `resolvedPaginationPhase`、`loadMoreSentinelRef`、`CircleNotch` 与 `requestOlderPage`。
- `web/src/styles.css` 中已有的 `--motion-normal`、`--color-text-muted`、`--color-primary`、`spin` 关键帧与全局 `prefers-reduced-motion` 降级。
- Exemplar: `web/src/components/MailList.jsx` 的 `SyncFeedbackRow` 提供一致的 Phosphor 加载图标、`role="status"`、`aria-live="polite"` 和语义颜色用法；分页反馈只复用其语言，不复用顶部全宽状态带的表面与布局。

现有系统可以表达该决定，不新增共享 primitive，也不把分页加载复用成显式同步状态带。

## Changes

1. `web/src/components/MailList.jsx`
   - Change: 将末端哨兵从永远 `aria-hidden` 的空 `span` 调整为可承载状态的末端元素，暴露 `data-state={paginationPhase}`；仅在 `paginationPhase === "loading"` 时渲染 14 px `CircleNotch` 与 **正在加载更多邮件…**，并给该进行中内容设置 `role="status"`、`aria-live="polite"`、`aria-atomic="true"` 和 `aria-busy="true"`。非加载状态不保留可访问名称或隐藏文本。
   - Preserve: 继续使用同一个 ref 作为 `IntersectionObserver` 目标；继续以 `paginationPhase !== "idle"` 和 `autoLoadRequestedRef` 阻止重复请求；保留 64 px 的 observer/root scroll 触发阈值，不引入手动加载入口。
   - Verify: 触底只调用一次 `onLoadMore`；等待期间旧邮件行和选择态仍在；加载内容追加后提示消失且新行出现在列表末端。

2. `web/src/styles.css`
   - Change: 保留 `.mail-pagination-sentinel` 默认 `height: 1px`，增加裁切、居中布局和 `height`/`opacity` 的 `--motion-normal` 过渡；在 `[data-state="loading"]` 时将其高度设为 64 px，使用紧凑的 11 px 状态文字、`--color-text-muted`、6 px 图文间距，并让图标使用 `--color-primary` 与现有 `spin 850ms linear infinite`。
   - Preserve: `.message-list` 仍是唯一纵向滚动面；缓冲区不获得独立背景、边框、圆角或阴影；列表原有底部 padding、面板半径和滚动条样式不变。
   - Verify: 用户能在旧末行之后继续滚入一个不超过 64 px 的加载区，提示在该区中央；追加完成后该空间收回而不推动或重播已有行。全局 reduced-motion 规则必须让高度过渡近乎即时并停止持续旋转，同时保留完整文案。

3. `web/src/components/MailList.test.jsx`
   - Change: 将“分页始终静默”的用例改为“触底自动加载并仅在请求期间显示有界反馈”。断言临近底部连续滚动仍只请求一次；切到 `loading` 后，原邮件行仍存在，status 的文本为 **正在加载更多邮件…**、`aria-busy` 为 true、哨兵 `data-state` 为 `loading`；追加消息并回到 `idle` 后提示消失。
   - Preserve: 保留没有“加载更早邮件”按钮的断言；`retry`、`offline`、`unavailable` 与 `complete` 仍不显示失败、离线或到底说明。
   - Verify: 状态插入能被可访问查询找到，但非 loading phase 不产生残留 status 名称。

4. `web/src/App.desktop.test.jsx`
   - Change: 把下一页 mock 改为可控 deferred 响应，在响应解决前断言当前列表内出现 **正在加载更多邮件…** 且 `Newest local page` 保持可见；解决后断言旧行、新追加行同时存在且加载提示消失。
   - Preserve: 后端仍以不透明 cursor 和 50 条 page size 调用，缓存列表不被 loading placeholder 替换，也不出现完成提示或手动按钮。
   - Verify: 记录触发前的 `scrollTop`，追加完成后确认没有被代码重置；若 jsdom 的布局属性不会自然反映浏览器滚动锚定，只验证没有显式归零，并把真实滚动连续性保留给界面验收。

5. `web/src/styles.test.js`
   - Change: 将“automatic pagination silent”样式契约改为“bounded loading buffer”。断言哨兵默认高度仍为 1 px，loading selector 高度为 64 px，过渡复用 `--motion-normal`，文字与图标使用语义 token，且没有引入分页卡片/独立表面。
   - Preserve: 已有行不带入场动画，列表不新增另一条滚动面，页面仍不包含持久完成/失败 chrome。
   - Verify: 样式测试能够区分默认哨兵与 loading 缓冲态，而不是只搜索一个新 class 名。

6. `DESIGN.md`
   - Change: 在 `Workspace contracts > Mail and reader` 中替换“自动分页保持完全静默且绝不增加底部 loading line”的旧决定：接近末端仍自动请求；仅在下一页请求进行中，列表末端展开 64 px 有界缓冲并居中显示 **正在加载更多邮件…**；完成后缓冲被追加行接替并收回；不得显示手动按钮、完成行或失败行。
   - Preserve: 缓存行、选择态和滚动位置稳定；显式同步仍只使用 tabs 下方共享状态带；空列表保持安静。
   - Verify: 文档不再同时要求“loading 可见”和“pagination 永远静默”。

7. `docs/PRODUCT.md`
   - Change: 在 `Mail list, bodies, and remote content` 的分页契约中同步说明 transient loading buffer，并保留每页最多 50 条、不透明 keyset cursor、自动触发和无手动按钮的行为。
   - Preserve: `offline`、失败和 confirmed end 不产生持久底部说明；分页状态区分、后台合并与 scroll/selection 稳定规则不变。
   - Verify: 产品行为与 `DESIGN.md`、React 测试对 loading phase 的定义一致。

## Scope

- Inherit: 所有通过 `App.jsx` 向 `MailList` 提供 `onLoadMore` 的邮箱角色、Starred 聚合和分页搜索结果自动获得同一缓冲反馈。
- Verify: Daylight、Night、Dusk、Forest 四主题；>1250 px、1250 px、940 px 与 720 px 边界；鼠标滚轮与触控板连续向下滚动；慢响应、快速响应、返回空页、retry/offline/unavailable/complete、切换文件夹后恢复各自滚动位置。
- Exclude: 新的分页 API、Rust/SQLite/IMAP 逻辑、改变 page size、下拉刷新、顶部显式同步带、联系人往来邮件、正文加载、Drafts/Outbox、失败重试按钮、完成提示以及原生橡皮筋 overscroll。

## Validation

- Product: 使用 deferred 的下一页响应打开一个至少可分页的 Inbox，滚到距底部 64 px 内；预期只发出一次请求，旧行保持可读，继续向下可看到 64 px 缓冲区中央的 **正在加载更多邮件…**，响应后新行无跳闪地接续，提示消失。
- Interface: 在四主题及所有桌面重排边界检查默认、loading、快速完成、空返回、retry/offline/unavailable/complete 和 reduced-motion；确认加载区没有独立卡片感，文字不截断，滚动条只有邮件列表一条，列表顶端没有 rubber-band。
- System: 确认实现只扩展 `.mail-pagination-sentinel` 并复用 `CircleNotch`、语义 token 与现有 pagination phase；不得复制 `SyncFeedbackRow` 或创建并行 loading primitive。
- Repository: `cd web && npm test -- --run` → React、组件、桌面集成和样式契约全部通过。
- Repository: `cd web && npm run build` → Vite 生产构建成功。
- Repository: `git diff --check` → 无空白错误；实现 diff 不覆盖当前工作树中与本计划无关的 `DESIGN.md`、`web/src/styles.css`、`web/src/styles.test.js` 和 `web/src/components/ContactsWorkspace.css` 用户改动。

## Stop conditions

- Stop if `loadMoreState="loading"` 在真实渲染链路中无法持续到异步请求完成，或分页追加会替换整个 messages 数组并主动重置滚动位置；先修正既有状态所有权/列表稳定性，而不是用定时器伪造加载反馈。
- Stop if 实现需要第二个滚动容器、原生 overscroll、手动 load-more 控件或 Rust/IMAP 协议变化；这些都会超出本设计变更。

## Design documentation

- After acceptance and validation: 按 Changes 6–7 更新 `DESIGN.md` 的 Mail and reader 自动分页视觉契约，以及 `docs/PRODUCT.md` 的分页行为契约；本计划所依据的用户请求已经接受“loading 期间显示有界底部缓冲区”这一方向。
