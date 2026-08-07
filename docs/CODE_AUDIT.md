# Mine Mail 全模块潜在 Bug / 漏洞审查报告
> 审查范围：核心 crate `mine-mail`（`src/`，约 35K 行）＋ 桌面壳 `mine-mail-desktop`（`web/src-tauri/`，约 15K 行）＋ React 前端（`web/src/`，约 40K 行）。
> 置信度标注：🟢 高 = 有直接代码证据（多数已人工复核）；🟡 中 = 依赖特定触发条件或结构对比推断。
>
> **总体结论**：架构与防御设计相当扎实（CAS 事务、attempts 代际防重放、UIDVALIDITY 校验、ammonia 双层 sanitize、keyring 凭据、日志字段白名单、minisign 更新签名）。**未发现可远程直接利用的任意代码执行级漏洞**，但存在 2 个高危安全缺陷（邮件内容层面的 DoS 与 XSS 传播面）、3 个高危可靠性缺陷、1 个凭据治理缺陷及多个中低危问题。

---

## 修复状态总览（2026-08-07）

按 P0-P3 分级修复，每项独立提交（提交信息遵循 `contributing.md`）。**P0/P1 全部修复，P2 中工作量小的附带修复，P3 与工作量大或需产品决策的项暂不修复。**

### P0（安全，已修复）
| 编号 | 问题 | 提交 |
|---|---|---|
| S-1 | 内联 CID 图片引用替换预算，防内存放大 DoS | `fix: 限制内联图片引用替换预算防止内存放大` |
| S-2 | 外发回复/转发前消毒引用邮件 HTML | `fix: 外发回复前消毒引用邮件 HTML`、`fix: 外发引用消毒允许保留内联 data 图片` |
| P-1 | `password.txt` 明文凭据（已被 gitignore） | 未自动删除（用户文件），建议手动删除并检查 git 历史 |

### P1（高危可靠性/安全纵深，已修复）
| 编号 | 问题 | 提交 |
|---|---|---|
| R-1 | 同步推流读写意图失败不再静默吞错 | `fix: 同步推流读写意图失败时不再静默吞错` |
| R-3 | 移动动作与同步删除竞态防护 | `fix: 修复移动动作与同步删除之间的竞态` |
| M-1/M-2/M-6 | 入站解析大小/引用/主题上限 | `fix: 入站邮件解析增加大小与引用数量上限` |
| B-4 | 超大消息下载前拒绝 | `fix: 超大消息在下载正文前即被拒绝` |
| P-2 | keyring 凭据按认证类型区分（含 account_id 与缓存库文件） | `fix: 密码账户与 OAuth 账户的凭据不再互相覆盖`、`fix: 账户标识与缓存库文件按认证类型区分` |
| P-3 | 本地数据目录完整路径不再暴露给前端 | `fix: 本地数据目录完整路径不再暴露给前端` |
| T-1/T-2 | CSS 远程资源检测增强 + data URL 上限 | `fix: 增强 CSS 远程资源检测并限制内联 data 图片大小` |
| T-11 | 外链拒绝本机/内网地址 | `fix: 拒绝打开本机与内网地址链接` |
| T-12 | 桌面命令标识符边界校验补齐 | `fix: 补齐桌面命令入口的标识符边界校验` |
| F-2 | 原生 HTML 渲染纵深防御 | `fix: 原生 HTML 渲染增加主文档纵深防御` |
| F-8 | 协议相对链接绝对化保存 | `fix: 写入链接时协议相对地址改为绝对化保存` |

### P2（工作量小、附带修复，已修复）
| 编号 | 问题 | 提交 |
|---|---|---|
| M-7 | Windows 保留设备名补 CONIN$/CONOUT$ | `fix: 补齐 Windows 保留设备名清洗` |
| D-5 | reset_mailbox/delete_missing_uids 事务改 Immediate | `fix: 重置邮箱事务改用立即模式` |
| D-8 | enqueue_outbox 冲突不再静默 | `fix: enqueue_outbox 冲突时不再静默成功` |
| B-9 | IDLE 等待出错先发 DONE | `fix: IDLE 等待出错时先退出空闲状态` |
| T-8 | 旧版主凭据迁移后删除残留条目 | `fix: 迁移旧版主凭据后立即删除残留条目` |
| T-10 | 授权密码保存前去除首尾空白 | `fix: 授权密码保存前去除首尾空白` |
| T-13 | 更新失败提示脱敏 | `fix: 更新失败提示不再透传底层错误文本` |
| T-15 | 数据迁移命令互斥 | `fix: 数据迁移准备与取消命令增加互斥` |
| F-1 | 恢复用户主动操作的成功提示 | `fix: 恢复用户主动操作的成功提示` |
| F-5 | 账户配置失败后恢复写信窗口 | `fix: 账户配置失败后恢复被最小化的写信窗口` |
| F-7 | 新邮件通知关闭失败限次重试 | `fix: 新邮件通知关闭失败时限制自动重试次数` |
| F-9 | 收件人邮箱拒绝连续点/首尾点 | `fix: 拒绝连续点或首尾点的畸形邮箱地址` |
| F-10 | 标记未读失败给出提示 | `fix: 标记未读失败时给出恢复提示` |

### P3（未修复，原因）
| 编号 | 问题 | 不修复原因 |
|---|---|---|
| R-2 | body 下载锁跨 await 串行化 | 需重设计锁结构，风险大于收益 |
| M-3/M-4/M-5/M-8/M-9 | MIME 解析/分类/边界若干 | 中危，工作量与回归风险偏高 |
| D-1/D-2/D-3/D-4/D-6/D-7/D-9/D-10/D-11/D-12 | 数据层性能与冗余 | 需改查询/迁移语义，影响面大 |
| B-1 | converge_confirmed_move 吞网络错误 | 有自动恢复机制，改动传播语义风险高 |
| B-2/B-3/B-5/B-6/B-7/B-8 | 状态机/轮询/草稿同步边界 | 跨进程/产品语义，需产品决策 |
| T-3/T-4/T-5/T-6/T-7/T-9/T-14/T-16/T-17 | OAuth 恢复、日志加盐、sandbox 等 | 涉及产品行为或需用户验收的 UI 权衡 |
| F-3/F-4/F-6 | 静默失败提示、ref 无界增长、分页锁 | 复杂流程，改动易引入回归 |
| L 级全部 | 低危项 | 风险低，暂不处理 |

### 验证结果
- 核心库 `cargo test`：245 + 3 + 1 项全部通过
- 桌面壳 `cargo test`：133 项全部通过；`cargo check` 通过
- 前端 `npm test -- --run`：见下方最终验证；`npm run build` 通过
- 前端 UI/交互改动（F 系列）留待用户手动验收

---

## 一、高危安全缺陷（S 级）

### S-1 ｜ 内联 CID 图片重复引用导致内存/CPU 放大 DoS
- **模块**：核心 MIME 解析
- **位置**：`src/mime.rs:2308-2337`（替换循环）、`2359-2376`（`replace_ascii_case_insensitive`）
- **问题**：`extract_renderable_html` 对每个带 Content-ID 的 image part 生成最大 4MB 的 base64 data URL，然后对 HTML 中**所有**出现（不限次数）做大小写不敏感替换；`total_inline_bytes` 只按 part 计一次，不随引用次数增长；每次替换还全量复制输入并做 `to_ascii_lowercase()`。
- **触发**：收到一封 text/html 中对同一 `cid:` 引用数百次的邮件 + 一个 1-4MB 内联图片 part，在同步/读取/渲染任一路径触发解析。
- **影响**：4MB 图片 base64≈5.6MB × 200 处引用 ≈ 1.1GB 输出（且执行两次替换），单封几百 KB 的邮件可致客户端 OOM/卡死。
- **置信度**：🟢 高

### S-2 ｜ 回复邮件引用原文 HTML 未消毒，XSS 可随外发邮件跨用户传播
- **模块**：核心 MIME / 发信链路
- **位置**：`src/mime.rs:1130-1142`（`rich_html_body` 直接把 `context.quoted_html` 拼入 `<blockquote>`）；来源 `backend.rs:3949-3952`（原样克隆原邮件 `body_html`）；外发 `backend.rs:6513-6522`（`send_request` 无消毒）
- **问题**：收到含 `<script>`/`onerror=`/`<iframe>`/远程追踪图的原邮件后，回复草稿的 `quoted_html` 是未消毒的原文 HTML。消毒仅存在于桌面壳 `lib.rs`（`sanitize_compose_request`/`sanitize_reply_html`），核心库与 CLI 路径无防线。
- **影响**：恶意脚本/事件属性/远程引用随回复邮件发送给收件人（受收件端清理能力制约）；远程图片同时造成发件人隐私泄漏。
- **置信度**：🟢 高

---

## 二、高危可靠性缺陷（R 级）

### R-1 ｜ 同步中 pending 读写意图的推流错误被静默吞掉
- **模块**：核心同步
- **位置**：`src/backend.rs:1660-1665`（full sync）、`1861-1866`（增量 sync）、`1332-1334`/`1367-1369`/`1422`（`let _ =`）
- **问题**：`flush_pending_seen_updates` / `flush_pending_flagged_updates` / `flush_pending_message_mutations` 的返回值一律被丢弃。STORE/移动遇网络错误或服务器拒绝时不向上传播，同步继续用远程快照推进。
- **影响**：已读/星标/移动意图长期滞留 pending 队列，跨设备不同步；UI 因 `flags_with_pending_updates` 叠加显示"已应用"，但远程从未生效，且无任何错误提示，同步报告仍显示成功。
- **置信度**：🟢 高

### R-2 ｜ 正文下载连接锁跨 await 持有，单条慢消息阻塞整条下载 lane
- **模块**：核心同步
- **位置**：`src/backend.rs:4853-4952`、`4665-4716`
- **问题**：`body_imap.lock().await` / `body_prefetch_imap.lock().await` 的 guard 跨越 SELECT、fetch、解析、写库等多个 `.await` 网络调用；任一条消息 fetch 超时（45s）会阻塞该 lane 全部后续正文下载。
- **影响**：UI 打开邮件的请求排队、延迟放大（前景与 prefetch 各一连接互不阻塞，但 lane 内无并发）。
- **置信度**：🟢 高

### R-3 ｜ `delete_missing_uids` 只保护 confirmed 动作，in_flight/queued 移动存在竞态
- **模块**：核心数据库/同步
- **位置**：`src/database.rs:1167-1236`（删除 SQL 中 `p.status = 'confirmed'` 的 NOT EXISTS 保护）；`backend.rs:1335` 与 `2921` 分别持不同互斥锁可并发
- **问题**：移动动作处于 queued/in_flight（非 confirmed）时，若同步快照显示源 UID 在远端已不存在，`delete_missing_uids` 会删除本地行并把动作标记为 `needs_attention(source_missing)`。
- **影响**：移动/归档被错误标记失败需人工处理；本地行先消失、后续目标信箱 sync 再拉回，造成短暂"消息消失"。
- **置信度**：🟢 高

---

## 三、凭据与隐私缺陷（P 级）

### P-1 ｜ 工作区存在明文邮箱授权码文件
- **位置**：根目录 `password.txt`（两对真实邮箱授权码）；`.gitignore:14` 已排除
- **问题**：文件虽未入库，但明文存在于工作区，任何通配拷贝的构建/打包脚本都可能带出；若曾进入 git 历史则直接泄露。
- **影响**：邮箱授权码泄露。建议立即删除该文件并检查 git 历史。
- **置信度**：🟢 高

### P-2 ｜ 同一 identity 的 Password 与 OAuth 账户共用同一 keyring entry，凭据互相覆盖
- **模块**：账户/凭据
- **位置**：`web/src-tauri/src/account.rs:239-241`、`293-310`、`2266-2271`、`1205-1216`、`1428-1439`
- **问题**：`keyring_username` 只由 `hash(email+imap+smtp)` 决定，`same_identity` 也只比较该 hash；同邮箱同服务器的密码账户与 OAuth 账户判定为"同一账户"，共用同一 entry 但内容格式不兼容（明文密码 vs OAuthTokenBundle JSON）。
- **影响**：后写入方覆盖先写入方凭据；先写入账户的 `load_network_backend` 反序列化失败、网络工作停止。**与 P-5 叠加**（entry 名可预测）后，同用户任何进程可构造同名 entry 覆盖凭据。
- **置信度**：🟢 高

### P-3 ｜ 完整本地数据目录路径暴露给 React（违反 AGENTS.md）
- **模块**：桌面壳
- **位置**：`web/src-tauri/src/storage.rs:94`、`318-319`（`data_path` → 前端 `dataPath`）；React 侧 `web/src/services/appStorage.js:9,29`；`lib.rs:69-113` 的守卫测试只查 `"path"` 键，是测试盲区
- **问题**：AGENTS.md 明确"Never expose … complete local paths to React"，但 `get_storage_status` 常态调用即把用户数据根目录完整路径送入前端运行时；`targetPath` 同样暴露。
- **影响**：前端一旦存在 XSS，攻击者可获知敏感目录位置辅助本地文件攻击/钓鱼。
- **置信度**：🟢 高

### P-4 ｜ CLI 错误输出可能打印 IMAP 服务器响应文本
- **模块**：核心 CLI
- **位置**：`src/main.rs:402-405`；`src/imap_client.rs:395,404,410,436…`（`MailError::Imap(error.to_string())`）
- **问题**：登录失败已专门隔离为 `ConnectionFailureKind::Authentication`（不泄漏），但后续所有命令错误仍透传 `async_imap` 库错误 Display（部分旧式服务器会在 BAD/NO 响应中回显命令，个别回显 LOGIN 参数）。
- **影响**：授权密码/令牌可能被打印到终端/日志。
- **置信度**：🟡 中

---

## 四、中危缺陷（M 级）

### MIME / 邮件处理
- **M-1**｜`src/mime.rs:2434-2485`：`parse_incoming_message` 用 `MessageParser::default()`（无 `with_max_message_size`/`with_max_headers_size`/`with_max_attachments_count`），超大/畸形消息全量解析并落库，内存与 DB 无上限。置信度：🟢 高
- **M-2**｜`src/mime.rs:2537-2550`：References/In-Reply-To 解析无数量与总长上限，入站侧不受出站的 850 字节 `bounded_reference_chain` 保护。置信度：🟢 高
- **M-3**｜`src/mime.rs:2398-2432`：`decode_remote_mime_part` 信任 IMAP 提供的 MIME 头拼装解析，服务器返回与 BODYSTRUCTURE 不符时正文语义漂移，且无字节上限。置信度：🟡 中
- **M-4**｜`src/mime.rs:1754-1757`：只要 part 有 Content-ID 即判 Inline，附件列表出现伪造条目；附件图片被静默内联替换，为 S-1 提供更多入口。置信度：🟢 高
- **M-5**｜`src/mime.rs:2434-2485`：`is_encoding_problem` 只在附件索引路径检查，正文解码错误被静默接受并落库，转发/回复引用时行为不一致。置信度：🟢 高
- **M-6**｜`src/mime.rs:2465`、`323`：入站 subject/显示字段无长度截断（转发路径却有 `MAX_FORWARD_SUBJECT_BYTES`），DB 膨胀 + UI 超长渲染。置信度：🟢 高
- **M-7**｜`src/mime.rs:1804-1836,1883-1895`：Windows 保留名清洗漏 `CONIN$`/`CONOUT$`、Unicode 上标 `COM¹` 及尾部空格变体；跨平台保存歧义。置信度：🟢 高（对比 `save_extracted_file` 路径）
- **M-8**｜`src/mime.rs:335-375,458-478`：回复草稿边界以首个 `At ... wrote:` 行切分，攻击者可在引用首行构造该模式伪造 sender/时间元数据。置信度：🟡 中
- **M-9**｜`src/mime.rs:2429` + `backend.rs:6737-6781`：远程选择性 BODY part 读取时未取内联图片 part，`cid:` 残留导致 HTML 内联图全部丢失（功能回退）。置信度：🟡 中

### 核心数据层（database.rs）
- **D-1**｜`src/database.rs:7593-7640,7674-7736`：分页 `ORDER BY COALESCE(...)` 是表达式，`idx_messages_inbox` 无法命中，大邮箱每页全表排序 + NOT EXISTS，翻页延迟随规模线性恶化。置信度：🟢 高
- **D-2**｜`src/database.rs:3205-3215`（`MESSAGE_COLUMNS` 含 `raw_rfc822`）：归档/删除/标星等"只改标志位"的操作把完整 BLOB 读入内存（数十 MB 级放大）。置信度：🟢 高
- **D-3**｜`src/database.rs:578-596,5686-5692,6529-6550`：迁移无 `user_version` 门控，v10/v18 的全表 UPDATE 每次冷启动重跑（百万级消息库启动时间线性增长）；崩溃会重复扫描。置信度：🟢 高
- **D-4**｜`src/database.rs:611-615,5527-5531,3431-3482`：每次操作新建连接 + 5s busy_timeout 无重试；缓存驱逐持 `BEGIN IMMEDIATE` 长事务，并发时其他写操作直接 `SQLITE_BUSY` 报错。置信度：🟢 高
- **D-5**｜`src/database.rs:1120,1174`：`reset_mailbox`/`delete_missing_uids` 用 deferred 事务，读后升级写锁可能 `SQLITE_BUSY_SNAPSHOT`（busy handler 不等待该类错误）。置信度：🟡 中
- **D-6**｜`src/database.rs:3154-3173` + `backend.rs:2234-2267`：`list_contact_source_messages` 无 LIMIT 全量载入该账号全部消息摘要，联系人页内存随库大小线性增长。置信度：🟢 高
- **D-7**｜`src/database.rs:1158-1240`：每轮同步全量装载 UID 集合并逐 uid 执行 4 条 SQL（重同步时 N+4 次往返）。置信度：🟢 高
- **D-8**｜`src/database.rs:4843-4876`：`enqueue_outbox` 对 id 冲突 `ON CONFLICT DO NOTHING` 后不检查影响行数（对比 `enqueue_new_outbox` 有检查），冲突被静默吞掉。置信度：🟢 高
- **D-9**｜`src/database.rs:5381-5392,5396-5410`：启动恢复语句无账号限定，多账号/多实例场景下 A 账号启动可能改写 B 账号的 queued/sending 状态。置信度：🟢 高（单账号部署无影响）
- **D-10**｜`src/database.rs:3101-3121`：旧列表接口用 OFFSET 分页（深翻页 O(N) + 并发插入重复/跳页）。置信度：🟢 高
- **D-11**｜`src/database.rs:333-366,553-575,526-552`：完整 RFC822/正文/BCC 明文落盘（无磁盘加密；属设计取舍，建议加密层）。置信度：🟢 高
- **D-12**｜`src/database.rs:6220-6321`：`user_version >= 14` 后跳过 public_id 修复，损坏数据导致 `CREATE UNIQUE INDEX` 失败 → 应用无法启动且无降级。置信度：🟡 中（边缘场景）

### 同步 / 状态机（backend.rs / imap_client.rs）
- **B-1**｜`src/backend.rs:3292-3294`：`converge_confirmed_move` 吞掉 IMAP/Timeout/Connection 错误（`Ok(())`），用户无感知、投影延迟。置信度：🟢 高
- **B-2**｜`src/database.rs:5073-5093`：`update_outbox_status` 无条件覆盖（无状态/attempts 代际条件），跨进程双开时状态闪变；同代际保护的 `claim_*` 系列对比明显。置信度：🟡 中
- **B-3**｜`src/database.rs:5396-5410` + `backend.rs:801`：`recover_sending_as_delivery_unknown` 无条件转全部 `sending` 行，双开时进程 B 改写进程 A 进行中的发送（最终一致，中间闪变）。置信度：🟡 中
- **B-4**｜`src/backend.rs:4913-4919` + `imap_client.rs:815-828`：50MiB 上限检查在完整下载之后；服务器省略 `RFC822.SIZE` 时退化为 `raw.len()`，检查失效 → 先全量下载到内存再拒绝。置信度：🟡 中-高
- **B-5**｜`src/backend.rs:7246-7250,336-350`：无 IDLE 服务器轮询模式仅比较 exists/uid_next/uid_validity，仅标志位变化检测不到（跨设备已读/星标只能靠定时 full sync 收敛）。置信度：🟢 高
- **B-6**｜`src/backend.rs:1480-1501`：full sync 要求返回 UID 集合严格等于请求集合，fetch 前远端删除即整轮报错（保守但抖动）。置信度：🟢 高
- **B-7**｜`src/backend.rs:6086-6105`：服务器 Drafts 索引延迟时"本地已同步草稿被判定远程已删除"而删本地行，存在窗口期草稿消失。置信度：🟡 中
- **B-8**｜`src/backend.rs:3360-3373`：远端确定性拒绝（如非法目标信箱名）被归为 `OutcomeUnknown` 需人工介入，无"确定性拒绝"通道。置信度：🟢 高
- **B-9**｜`src/imap_client.rs:658-662`：IDLE wait 出错时不发 DONE 即返回，服务器残留 idle 会话（靠连接关闭清理，无功能错误）。置信度：🟢 高

### 桌面壳 / Tauri 边界
- **T-1**｜`web/src-tauri/src/mail_html.rs:308-317`：`has_remote_css_image` 只匹配 6 种 `url(...)` 形态，空格/引号/@import/协议相对/制表符换行全漏检 → `hasRemoteImages=false`，`automatic` 模式跟踪像素无提示加载、`ask` 模式授权入口消失。置信度：🟢 高
- **T-2**｜`web/src-tauri/src/mail_html.rs:830-844`：内联 data URL 只校验 MIME 前缀不限字节，数 MB~数十 MB data 图全部保留注入 iframe，渲染卡顿/崩溃。置信度：🟢 高
- **T-3**｜`web/src-tauri/src/account.rs:1852-1994`：OAuth 刷新 `invalid_grant` 后永久停网且全仓无 `force=true` 调用点（自动恢复路径缺失）；断网期每轮 sync 用过期 token 重试失败。置信度：🟢 高
- **T-4**｜`web/src-tauri/src/desktop/mod.rs:1466-1475,1491-1493,1508-1523`：后台同步失败每 poll 周期重复发 `mail:sync-error`（诊断日志有节流，事件通道无），UI 文件夹状态反复闪"同步失败"。置信度：🟢 高
- **T-5**｜`web/src-tauri/src/account.rs:2574-2667`：OAuth 本机回调只 `accept()` 一次且不校验 Host/来源进程，本机进程抢先连接即 DoS 授权流程（PKCE+state 阻断窃取）。置信度：🟢 高
- **T-6**｜`web/src-tauri/src/account.rs:1371-1384`：`connect_google` 不验证 IMAP/SMTP 连通性与 token 作用域即保存账户（密码账户 `configure` 会 verify）。置信度：🟢 高
- **T-7**｜`web/src-tauri/src/account.rs:2266-2271,2305-2320,1686`：修改自定义账户服务器/端口后 identity hash 改变，旧 keyring entry 与旧 DB 永不删除（凭据/数据残留 + 3 账户名额被占）。置信度：🟢 高
- **T-8**｜`web/src-tauri/src/account.rs:2150-2179,1686-1712`：legacy keyring entry 迁移后不删除旧 entry，删除账户也不清理。置信度：🟢 高
- **T-9**｜`web/src-tauri/src/account.rs:2986-2989`：Google `error_code` 原样回显到用户可见文案（当前无敏感值，未来上游变更则透出）。置信度：🟡 中
- **T-10**｜`web/src-tauri/src/account.rs:332-338`：configure 密码仅 trim 判空、原样保存，首尾空白密码导致难排查的认证失败。置信度：🟢 高
- **T-11**｜`web/src-tauri/src/lib.rs:1208-1224`：`validate_external_url` 不限制本机/内网地址（`http://127.0.0.1:8080/admin` 可诱导打开）。置信度：🟢 高
- **T-12**｜`web/src-tauri/src/lib.rs:1847-1859`：`fetch_outbox_message` 缺失 `validate_outbox_id`（同类命令均有）；`2274-2467`：`switch_account`/`set_account_remark`/`remove_account` 未显式 `validate_account_id`；`1292-1517`：draft/attachment id 无边界校验——验证面不一致（core 有兜底，属防御纵深缺口）。置信度：🟢 高（结构对比）
- **T-13**｜`web/src-tauri/src/app_update.rs:81,85,111,115,154`：更新失败 `error.to_string()` 原样发前端，插件错误文本可能含本地路径/URL。置信度：🟡 中
- **T-14**｜`web/src-tauri/src/storage.rs:543-588`：迁移目标校验后仍可在任意可写绝对路径创建空目录+探针临时文件（受限的任意目录创建面）。置信度：🟢 高
- **T-15**｜`web/src-tauri/src/lib.rs:1876-1895`：`prepare_storage_migration`/`cancel_storage_migration` 并发无互斥，任务文件覆盖交错。置信度：🟡 中（UI 串行，风险低）
- **T-16**｜`web/src-tauri/src/diagnostics.rs:489-499`：日志 `account_ref`/`item_ref` 用确定性 SHA256 前 6 字节（48bit）无盐哈希，可离线字典破解还原邮箱、跨会话关联画像。置信度：🟢 高
- **T-17**｜`web/src-tauri/src/mail_html.rs:258-304` + `HtmlMessageBody.jsx:391`：主 sanitizer 允许 `<style>`/`style`/`class`/`id` 原样保留任意 CSS（依赖事后子串扫描兜底）；iframe `sandbox="allow-same-origin"` 使邮件内容与主文档同源（当前无 `allow-scripts` 才安全，是最薄纵深层）。置信度：🟡 中

### 前端（React）
- **F-1**｜`App.jsx:1047-1060`：`showToast` 开头 `if (tone === "success") return;` 丢弃**所有**成功提示（草稿删除、重试发送成功、确认投递、账户移除成功等 7+ 处调用点均无 UI 反馈），用户可能误判失败重复操作。置信度：🟢 高
- **F-2**｜`NativeHtmlMessageBody.jsx:7-20,77`：`dangerouslySetInnerHTML` 注入主文档，前端仅处理 `<img src>`，事件属性/`<meta http-equiv>`/iframe/object/link 原样保留；CSP 挡内联脚本但不挡 `meta refresh`、远程 img。信任单一清理层、无纵深防御。置信度：🟢 高（与 T-1/T-2 叠加）
- **F-3**｜`App.jsx:3498-3521,5617,3623,3057`：多处同步/加载失败 `catch(() => {})` 静默吞掉，UI 停留"ready"误导状态。置信度：🟢 高
- **F-4**｜`App.jsx:812,845,767,817`：`messageBodyCacheRef`/`starStateRef`/`messageActionStates`/`mailListScrollPositionsRef` 无界增长（无 LRU/上限，账户移除后不清理），长会话数百 MB 级。置信度：🟢 高
- **F-5**｜`App.jsx:6697-6765,6807-6825` + `3400-3412`：账户配置/Google 登录失败后 composer 被永久最小化且状态未恢复，用户以为内容丢失。置信度：🟢 高
- **F-6**｜`MailList.jsx:293-303,336-369`：分页自动加载互斥锁在"下一页填不满视口"场景不释放，后续更早邮件需手动滚动才能继续加载（低概率可用性缺陷）。置信度：🟢 高
- **F-7**｜`NewMailNotification.jsx:161-181`：dismiss 失败后每 8 秒无限重试循环（无退避/失败计数）。置信度：🟢 高
- **F-8**｜`RichTextEditor.jsx:638-649`：`safeHref` 对协议相对 URL 返回原值，`<a href="//evil.example/x">` 被原样保存（依赖 Rust 再校验）。置信度：🟢 高
- **F-9**｜`ComposePanel.jsx:891-894` + `RecipientInput.jsx:19`：发送前邮箱格式校验仅"非空"，正则接受 `a@b..c` 等畸形地址，完全依赖 Rust 拒绝。置信度：🟢 高
- **F-10**｜`App.jsx:5042-5060`：标记未读失败静默回滚无提示。置信度：🟢 高

---

## 五、低危项（L 级）

| ID | 位置 | 问题 | 置信度 |
|---|---|---|---|
| L-1 | `src/models.rs:473-507` | `ComposeRequest` 的 subject/body_text 无长度上限（仅 body_html ≤512KB），超大正文 DB 膨胀 + base64 内存翻倍 | 🟢 |
| L-2 | `src/managed_attachments.rs:117-148` | managed 附件无账户级累计配额，`stage_forward_attachments` 每次转发复制新 blob，磁盘长期填满 | 🟢 |
| L-3 | `src/managed_attachments.rs:325-367` | 单条异常 blob 条目（如符号链接）导致整个清理/启动失败（本地 DoS） | 🟢 |
| L-4 | `src/atomic_publish.rs:83-90,122-127` | 原子发布未 fsync 父目录，Unix 断电场景 rename 可能不持久 | 🟡 |
| L-5 | `src/managed_attachments.rs:293-323` | 崩溃残留 `.tmp-*` 靠"1 小时无写入"启发式清理，极慢导入可被误删 | 🟡 |
| L-6 | `src/main.rs:27` + `backend.rs:748-752` | CLI 默认数据库路径相对 CWD，不同目录运行产生"数据丢失"假象 | 🟢 |
| L-7 | `src/config.rs:128-137` | 凭据文件无权限/所有者检查（Unix 0644 可被本机其他用户读） | 🟢 |
| L-8 | `src/models.rs:18-61` | `normalize_contact_email` 对 IDN 域名不 punycode 归一化，联系人键重复 | 🟢 |
| L-9 | `src/models.rs:532` | `InboxMessage.size_bytes: u32` 会截断 >4GB 消息（与 AttachmentMeta u64 不一致） | 🟢 |
| L-10 | `src/backend.rs:6548` + `main.rs:334-339` | `last_error` 原始错误字符串透传面（当前 SMTP 已脱敏，属未来风险） | 🟡 |
| L-11 | `src/smtp_client.rs:33` | SMTP 凭据 `to_owned()` 为普通 String 无 zeroize（IMAP 用 `&str` 引用不拷贝） | 🟢 |
| L-12 | `src/backend.rs:6132-6144` | `push_draft_record` 失败走 skipped 分支仍 `pushed += 1`，报告统计失真 | 🟢 |
| L-13 | `src/database.rs:7445-7449` | 每次签发游标执行一次全表 TTL DELETE（每页一次写事务） | 🟢 |
| L-14 | `src/database.rs:7599-7606,7645` | uid_validity 为 NULL 时 pending 动作过滤失效，常规页与 pending 页可能重复显示 | 🟡 |
| L-15 | `src/database.rs:6570-6617` | 联系人触发器使每次消息 UPSERT 额外 4 条语句，写放大 5 倍 | 🟢 |
| L-16 | `web/src-tauri/src/contacts.rs:477-489` | 联系人排序非全序不稳定；active 账户与收藏列表可重复展示同一邮箱 | 🟢 |
| L-17 | `web/src-tauri/src/contacts.rs:203-206` | `set_favorite` 的 account_id 无长度上限（实际由后端生成，触发面小） | 🟢 |
| L-18 | `web/src-tauri/src/mail_html.rs:1633-1643,425-469` | Outlook 后缀/`<title>` 基于字符串 marker 的启发式切分，可被畸形输入打乱（输出仍二次 sanitize） | 🟡 |
| L-19 | `web/src-tauri/src/account.rs:1415-1427,1937-1944` | serde 序列化/反序列化中间明文副本无法零化（业界常见限制） | 🟢 |
| L-20 | `web/src-tauri/src/account.rs:44,2133-2141` | 本地离线后端用常量明文占位 secret（`mine-mail-local-cache-only`）进 Zeroizing | 🟢 |
| L-21 | `web/src-tauri/tauri.conf.json:47` | 主窗口 CSP 允许 `img-src http: https:`（脚本面受控，纵深提示） | 🟡 |
| L-22 | `Cargo.lock` 双份 | 根 crate 锁定 ammonia 4.1.4，桌面构建锁定 4.1.3，两个产物 sanitizer 版本漂移 | 🟢 |
| L-23 | `App.jsx:470-475,870-872` | `navigator.platform`（已废弃）与 localStorage 主题判定 | 🟢 |
| L-24 | `App.jsx:719-8028` | 渲染期写 ref（`isWideMailWorkspaceRef` 等）反模式，StrictMode 双渲染副作用 | 🟡 |
| L-25 | `MailList.jsx:542` | 行 fallback key 用 index，异常数据时 DOM 复用错位 | 🟢 |
| L-26 | `HtmlMessageBody.jsx:7,250-277` | `rememberedHeights` 的 cacheKey 不含账户维度，跨账户污染（有 32 条上限，可自愈） | 🟢 |
| L-27 | `useAppUpdate.js:47-50` | 已有可用更新时点"检查更新"直接弹窗，不向服务器重新核实 | 🟢 |

---

## 六、已核实为安全/正确的重点项（排除误报）

- **SMTP 未知结果不会被误判成功并自动重发**：`smtp_client.rs:105-120` 无状态码/超时一律 `DeliveryUnknown`，仅 4xx→Retryable、5xx→Rejected；`retry_outbox` 只接受 retryable，`confirm_delivery_unknown`/`retry_delivery_unknown_once` 用 `expected_attempts` 代际防重放。
- **消息移动状态机**（`mailbox_mutation.rs` + `backend.rs:2967-3556`）：`TransferStarted` 永不重复转移，`source_delete_acknowledged` 前强制 reconcile。
- **pending 标志不被远程快照覆盖**：`flags_with_pending_updates` 写入前叠加未确认意图。
- **SQL 注入**：全库参数化绑定，动态 SQL 表/列均来自硬编码常量，未发现注入面。
- **附件路径安全**：双层校验（UUID 内部名 + SQLite CHECK）+ symlink/reparse 检查 + 打开前后双校验 + SHA-256 比对，无路径穿越。
- **凭据不落 SQLite**（`database.rs:619` 注释 + 字节级测试），keyring 失败无明文回退，诊断日志字段白名单 + 哈希引用。
- **OAuth**：127.0.0.1 随机端口 + PKCE(S256) + state 校验完整。
- **HTML 渲染**：isolated 路径走 iframe CSP `script-src 'none'` + sandbox；ammonia 白名单拒绝 `javascript:`/事件属性/`srcset`/form/iframe/svg/math。
- **更新链路**：minisign 签名验证 + 版本 TOCTOU 检查 + 单实例下载锁。

---

## 七、修复优先级建议

1. **立即（安全）**：S-1（cid 引用计数上限）、S-2（quoted_html 消毒下沉到核心库）、P-1（删除 password.txt 并检查 git 历史）。
2. **短期（可靠性）**：R-1（flush 错误传播 + 重试上报）、R-3（delete_missing_uids 保护 queued/in_flight 动作）、P-2（keyring entry 按认证类型区分）。
3. **中期（纵深防御）**：T-1/T-2（CSS 远程检测与 data URL 上限）、F-2（native HTML 渲染加 meta refresh/事件属性兜底）、T-16（日志哈希加盐）、P-3（dataPath 移除或降级为相对标签）。
4. **性能**：D-1（排序表达式索引或改排序键）、D-2（操作不拉全量 BLOB）、D-3（迁移 user_version 门控）。
