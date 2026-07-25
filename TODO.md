# Mine Mail 发布前 TODO

> 最后核对：2026-07-24  
> 用途：把 Mine Mail 从开发预览推进到可供真实用户安全、顺畅使用的发行版。  
> 优先级：`P0` 为发布阻断项，`P1` 为公开 Beta 前应完成，`P2` 为稳定版前完成。

## 一、当前结论

Mine Mail 目前适合继续开发和小范围内部验收，但不适合把现有 GitHub Release 工作流直接用于公开发行。

当前主要阻断项：

- Windows 应用和安装包没有可信代码签名，可能被 Smart App Control、SmartScreen 或企业策略拦截。
- Gmail 使用 `https://mail.google.com/` 受限 scope；Google Cloud 项目处于 Testing 时只能由测试用户授权，授权及 refresh token 通常在 7 天后失效。
- 当前 release workflow 会自动公开三平台产物，但 Windows 未签名，macOS 只是 ad-hoc 签名且未公证，Linux 也没有发行签名。
- 没有应用内安全更新通道；发布后无法便捷、可靠地给用户修复安全问题。
- 缺少公开的隐私政策、用户数据删除说明、仓库级 LICENSE、SECURITY 和支持入口。
- “移除账户”当前会删除操作系统凭据，但没有撤销 Google token，也没有删除该账户的本地邮件数据库。
- 邮件 SQLite 缓存目前未整库加密；在公开发行前必须确认本地文件权限、风险披露和数据清除行为。
- 当前主要实机验收平台是 Windows 11，不能把未经真实设备验收的 macOS/Linux 包作为正式支持版本公开。

### 建议的发行顺序

1. **封闭 Beta**
   - 仅邀请明确知情的测试用户。
   - Gmail 保持 Testing，并维护测试用户白名单。
   - 即使是封闭测试，发给非开发者的 Windows 包也必须先完成可信签名。
   - 明确说明 Gmail 测试授权约 7 天后可能要求重新登录。

2. **Windows 公开 Beta**
   - 首先完成 Windows 签名、隐私/删除能力、更新通道、干净系统安装验收。
   - Gmail 必须先完成生产项目配置和受限 scope 验证，才能作为“任何 Google 用户可用”的公开功能宣传。
   - 如果 Google 验证尚未完成，需要做一个明确的产品决定：
     - 公开版暂时隐藏/禁用 Gmail，并说明后续开放；或
     - 发行仍标记为邀请制 Beta，不宣称 Gmail 对公众开放。

3. **跨平台稳定版**
   - macOS 完成 Developer ID 签名、公证和 stapling。
   - Linux 选定实际支持的发行版和包格式，并完成签名与真实设备验收。
   - 完成长期升级、数据迁移、回滚和支持流程。

---

## 二、发行范围与责任人

- [ ] **[P0]** 确定首发范围：支持的操作系统、CPU 架构、邮箱提供商和功能边界。
- [ ] **[P0]** 明确首发是“邀请制 Beta”“公开 Beta”还是“稳定版”，并在安装页、应用内和 Release Notes 使用一致表述。
- [ ] **[P0]** 确定发行者主体和对外显示名称；代码签名证书、Google OAuth 品牌、网站和支持邮箱应使用一致身份。
- [ ] **[P0]** 指定安全事件、OAuth 审核、签名密钥、发布审批和用户支持的负责人。
- [ ] **[P0]** 不公开当前 release workflow 生成的三平台产物，直到对应平台的发布门禁通过。
- [ ] **[P1]** 决定 Windows 主分发渠道：
  - Microsoft Store：对用户最省心，Microsoft 会处理 Store 信任；仍需满足 Store 的签名、静默安装和更新要求。
  - 官网/GitHub 直装：必须自行签名、维护 SmartScreen 信誉和安全更新。
- [ ] **[P1]** Windows 只提供一个面向普通用户的主安装包，避免让用户在 MSI、NSIS 和多个架构之间猜测；其他包标记为企业/高级用途。

## 三、Windows 签名、安装和信誉

### 代码签名

- [ ] **[P0]** 获取受信任提供商签发的 RSA Windows 代码签名能力，优先评估 Microsoft Artifact Signing；不要使用自签名证书作为公众发行方案。
- [ ] **[P0]** 固定一个长期使用的 Publisher 身份。频繁更换证书身份会损失已经积累的 SmartScreen 信誉。
- [ ] **[P0]** 在 Tauri `bundle.windows` 中配置 `signCommand`，或配置证书 thumbprint、SHA-256 digest 和 RFC 3161 时间戳服务。
- [ ] **[P0]** 确保以下所有可执行内容都被签名和时间戳：
  - `mine-mail-desktop.exe`
  - NSIS/MSI 安装包
  - 卸载器
  - 自动更新产物
  - 将来新增的 helper、DLL 或其他可执行组件
- [ ] **[P0]** Release CI 在找不到签名身份、签名失败、时间戳失败或签名验证失败时必须直接失败，不能降级生成未签名公开包。
- [ ] **[P0]** 在发布任务中验证签名，例如：

  ```powershell
  Get-AuthenticodeSignature .\Mine-Mail-Setup.exe | Format-List
  signtool verify /pa /all /v .\Mine-Mail-Setup.exe
  Get-FileHash .\Mine-Mail-Setup.exe -Algorithm SHA256
  ```

- [ ] **[P0]** 在开启 Smart App Control enforcement 的干净 Windows 11 设备测试安装、首次启动、托盘、开机启动、更新和卸载；检查 Code Integrity 3077 事件。
- [ ] **[P1]** 在 Microsoft Defender 默认设置、标准用户账户和企业常见策略下各做一次安装验收。
- [ ] **[P1]** 发布页面展示 Publisher 名称和 SHA-256，让早期用户能确认下载来源。有效证书仍可能在信誉积累前触发 SmartScreen，但不应显示“未知发布者”。

### 安装体验

- [ ] **[P0]** 将 `bundle.targets: "all"` 改为明确的发行目标，避免自动公开未经选择和验收的安装格式。
- [ ] **[P0]** 明确最低 Windows 版本和架构；当前至少应按 Windows 11 x64 验收，未验收的平台不能写成已支持。
- [ ] **[P1]** 选择 WebView2 策略：
  - 普通联网安装可使用 `downloadBootstrapper` 或 `embedBootstrapper`。
  - Microsoft Store 的 Win32 安装包按 Tauri 指引使用 offline installer。
  - 不使用 `skip`，除非安装前已可靠验证 WebView2 存在。
- [ ] **[P1]** 普通消费者安装优先使用 per-user 模式，避免无必要的管理员权限请求。
- [ ] **[P1]** 验证安装、覆盖升级、降级拒绝、卸载、重装、路径含中文、用户名含中文以及磁盘空间不足场景。
- [ ] **[P1]** 明确卸载是否保留邮件缓存和设置，并在卸载前给用户清晰选择；默认行为与隐私政策保持一致。

官方参考：

- [Microsoft：SmartScreen reputation](https://learn.microsoft.com/windows/apps/package-and-deploy/smartscreen-reputation)
- [Microsoft：Smart App Control 签名要求](https://learn.microsoft.com/windows/apps/develop/smart-app-control/code-signing-for-smart-app-control)
- [Microsoft：测试 Smart App Control 签名](https://learn.microsoft.com/windows/apps/develop/smart-app-control/test-your-app-with-smart-app-control)
- [Tauri：Windows Code Signing](https://v2.tauri.app/distribute/sign/windows/)
- [Tauri：Windows Installer 与 WebView2](https://v2.tauri.app/distribute/windows-installer/)
- [Tauri：Microsoft Store](https://v2.tauri.app/distribute/microsoft-store/)

## 四、Google OAuth / Gmail 公开使用

### 当前实现与限制

- 当前代码使用系统浏览器、随机 loopback callback、PKCE S256 和 `state` 校验，这些是正确的桌面 OAuth 基础。
- Gmail IMAP/SMTP 使用 `https://mail.google.com/`；Google 明确要求 IMAP、POP、SMTP 使用该 scope。
- `https://mail.google.com/` 是 restricted scope。公开应用需要 OAuth 验证。
- Google 项目处于 Testing 时：
  - 最多配置 100 个测试用户。
  - 非白名单用户不能正常授权。
  - 当前 scope 下的用户授权和 refresh token 通常在 7 天后失效。
- Desktop OAuth client secret 会被编译进客户端，不能被视作真正的机密或客户端身份安全边界；安全性必须依赖 PKCE、`state`、可信发布和 Google 的 OAuth 策略。

### Google Cloud 项目准备

- [ ] **[P0]** 分离开发/测试 Google Cloud 项目与生产项目；不要让日常测试配置改动影响已经提交审核的生产项目。
- [ ] **[P0]** 生产项目使用稳定的 Owner、Project Contact 和支持邮箱，全部管理员开启强 MFA，并准备人员离开后的交接方式。
- [ ] **[P0]** 为生产环境创建 Desktop app OAuth client，确认 loopback redirect 与当前 Authorization Code + PKCE 实现一致。
- [ ] **[P0]** 清理生产项目中不使用或未准备审核的 OAuth client，避免一个 client 的状态拖累整个项目验证。
- [ ] **[P0]** Google Auth Platform 中声明的 scope 与程序实际请求完全一致：
  - `openid`
  - `email`
  - `https://mail.google.com/`
- [ ] **[P0]** 准备 scope 说明：Mine Mail 是用户主动安装的完整桌面邮件客户端，需要通过 IMAP 同步读取邮件、草稿和 flags，通过 SMTP 发送邮件，因此无法用只发送或只读 scope 完成既定功能。
- [ ] **[P0]** 核对 Google 要求的 “full utilization”：提交审核的版本必须真实展示所申请邮件权限对应的读取、同步、草稿和发送能力，不能为尚未实现的未来功能申请权限。

### 品牌、网站和政策材料

- [ ] **[P0]** 准备可公开访问且无需登录的 HTTPS 产品主页，清楚说明 Mine Mail 的用途和为什么需要 Gmail 邮件权限。
- [ ] **[P0]** 准备同一已验证域名下的隐私政策、服务条款、支持页面和用户数据删除说明。
- [ ] **[P0]** 在 Google OAuth 品牌配置中保持应用名称、狐狸图标、主页、隐私政策、支持邮箱和发行包品牌一致。
- [ ] **[P0]** 完成域名所有权验证；不要依赖短链接、会跳转到无关域名的链接或需要登录才能查看的页面。
- [ ] **[P0]** 隐私政策单独说明 Google 用户数据，并加入 Google API Services User Data Policy / Limited Use 的合规声明。
- [ ] **[P0]** 清楚描述：
  - Gmail 邮件、草稿、联系人派生信息和 token 分别存在哪里。
  - 数据是否离开用户电脑。
  - 远程图片会向发件方服务器发起请求并暴露 IP/打开行为的隐私风险。
  - 数据保留多久、如何断开账户、如何撤销授权、如何删除本地缓存。
  - 当前 SQLite 缓存未整库加密的边界。

### 受限 scope 验证

- [ ] **[P0]** 将生产项目置为 In Production 后提交品牌和 restricted-scope verification；Testing 状态不能作为公众 Gmail 登录方案。
- [ ] **[P0]** 提供审核视频，完整录制：
  - 从 Mine Mail 发起 Google 登录。
  - OAuth consent screen 展示的所有 scope。
  - 读取/同步 Inbox。
  - 草稿和发送流程。
  - 断开账户及删除数据流程。
- [ ] **[P0]** 给审核人员提供已签名的可安装版本、逐步操作说明和专用测试账户/白名单方式。
- [ ] **[P0]** 准备回答为什么必须使用 IMAP/SMTP 和 `https://mail.google.com/`，以及为什么更窄的 Gmail API scope 无法保持当前架构与完整功能。
- [ ] **[P0]** 向 Google 准确声明 Mine Mail 的数据流：应用从用户设备直接连接 Google OAuth、IMAP 和 SMTP，不经 Mine Mail 自有服务器。
- [ ] **[P0]** 不预先假设一定免除 CASA。Google 当前规则是：restricted data 如果从或经第三方服务器访问、存储或传输，通常需要年度安全评估；纯本地桌面数据流可能不需要该评估，但最终由 Google 审核确定。
- [ ] **[P1]** 为 restricted-scope verification 预留约 6 周以上的审核时间；这不是 Google 承诺的 SLA，补件会延长。
- [ ] **[P1]** 建立年度复核日历，监控 Google 的重新验证、项目联系人邮件和政策变更。

### 撤权和数据删除

- [ ] **[P0]** “移除 Gmail 账户”时调用 Google token revocation endpoint，不能只删除本机 keyring 中的 refresh token。
- [ ] **[P0]** 给用户提供明确的“断开并删除本地数据”动作，删除：
  - 该账户凭据和 OAuth token
  - 该账户邮件 SQLite 数据库及 WAL/SHM
  - 通知基线和账户级头像
  - 账户级收藏及其他派生数据
  - 无法安全重试的本地草稿/Outbox 前必须二次确认
- [ ] **[P0]** 将“仅断开”“删除本地缓存”“撤销 Google 授权”的差异写清楚；执行失败时不能向用户显示已经全部删除。
- [ ] **[P1]** 提供删除整个 Mine Mail 本地数据的入口和无需应用也能执行的支持说明。
- [ ] **[P1]** 测试 token 撤销、密码修改、Google 授权撤回、refresh token 过期和用户取消授权后的恢复体验。

官方参考：

- [Google：Gmail IMAP/SMTP XOAUTH2 和 scope](https://developers.google.com/workspace/gmail/imap/xoauth2-protocol)
- [Google：管理 OAuth App Audience](https://support.google.com/cloud/answer/15549945)
- [Google：OAuth Verification Help Center](https://support.google.com/cloud/answer/13463073)
- [Google：Restricted Scope Verification](https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification)
- [Google：Verification Requirements](https://support.google.com/cloud/answer/13464321)
- [Google：OAuth 2.0 Policies](https://developers.google.com/identity/protocols/oauth2/policies)

## 五、隐私、安全与本地数据

### 数据边界

- [ ] **[P0]** 建立一份可审核的数据流图：邮箱服务商 → Rust IMAP/SMTP → SQLite/OS credential store → React 摘要/正文边界。
- [ ] **[P0]** 列出所有持久化数据、路径、保留期限、删除入口和访问主体。
- [ ] **[P0]** 确认密码、授权 secret 和 OAuth token 只进入操作系统凭据存储，不进入 SQLite、React、日志、崩溃报告或安装包配置文件。
- [ ] **[P0]** 确认账户数据库和日志目录只允许当前 OS 用户访问；在标准用户、多用户和漫游配置环境验证 Windows ACL。
- [ ] **[P0]** 对“SQLite 未整库加密”作正式风险决定：
  - 若首版接受该边界，隐私政策和产品帮助必须明确依赖 OS 账户/磁盘加密保护。
  - 若目标用户或 Google 审核要求更强保护，先实现可靠的本地加密和密钥恢复方案再发布。
- [ ] **[P0]** 确保异常和诊断日志继续排除邮箱地址、主题、正文、完整 RFC822、token、密码和完整本地路径；用自动测试扫描日志样本。
- [ ] **[P0]** 安全复核 HTML 清理、iframe sandbox、CSP、远程图片和 URL scheme，覆盖恶意邮件语料。
- [ ] **[P0]** 验证所有 IMAP/SMTP/OAuth HTTPS 连接执行正常证书链和主机名校验，不允许用户在普通设置中关闭 TLS 验证。
- [ ] **[P0]** 对草稿、Outbox、`delivery_unknown`、重试和版本冲突做重复投递/数据覆盖专项测试。
- [ ] **[P1]** 检查 Tauri capabilities 和 commands，维持最小权限；React 不获得文件系统、shell、凭据或任意网络能力。
- [ ] **[P1]** 对通知内容、托盘菜单、剪贴板、开机启动和后台同步做隐私复核。
- [ ] **[P1]** 给远程图片“自动/询问/阻止”设置保留就近隐私说明，并验证更改即时生效。

### 供应链与依赖

- [ ] **[P0]** 发布前运行并处理结果：

  ```powershell
  cargo fmt --check
  cargo test --locked
  cargo clippy --all-targets --locked -- -D warnings
  cargo audit

  cd web
  npm ci
  npm test -- --run
  npm run build
  npm audit --omit=dev

  cd src-tauri
  cargo fmt --check
  cargo test --locked
  cargo check --locked
  cargo clippy --all-targets --locked -- -D warnings
  cargo audit
  ```

- [ ] **[P1]** 引入 `cargo-deny` 或等价检查，审查 Rust/Node 依赖许可证、重复依赖、来源和已知漏洞。
- [ ] **[P1]** 生成第三方许可证清单和 SBOM，并随发行保留。
- [ ] **[P1]** GitHub Actions 使用最小权限，关键 action 固定到审核过的 commit SHA，减少 tag 被替换的供应链风险。
- [ ] **[P1]** 为发行产物生成 provenance/attestation 和 SHA-256 清单。

## 六、安全更新能力

- [ ] **[P0]** 在公开发行前决定更新策略。最低要求是应用内能发现新版本并给出可信下载入口；推荐实现 Tauri updater。
- [ ] **[P0]** 如使用 Tauri updater：
  - 生成独立 updater key pair。
  - 公钥内置在应用；私钥只存在于受保护的 CI/密钥服务中。
  - 安全备份私钥和恢复流程；丢失私钥会导致已安装客户端无法接受后续更新。
  - `createUpdaterArtifacts` 开启，更新 endpoint 仅使用 HTTPS。
  - `latest.json` 的版本、URL 和 signature 由发布流水线生成并验证。
  - 更新下载后必须先验证 Tauri 签名，再交给操作系统安装器验证代码签名。
- [ ] **[P0]** 稳定版和 Beta 使用独立更新 channel，避免把试验版本推给稳定用户。
- [ ] **[P0]** 更新失败必须保留当前可运行版本和用户数据，不能留下半安装状态。
- [ ] **[P1]** 测试跨版本数据库 migration、跳版本升级、断网、下载中断、签名错误、磁盘不足和回滚。
- [ ] **[P1]** 制定紧急撤回/停止更新流程；已经签名的恶意或损坏包不能继续由 endpoint 提供。

官方参考：

- [Tauri：Updater](https://v2.tauri.app/plugin/updater/)

## 七、CI/CD 与发布门禁

### 当前 workflow 需要调整

- `.github/workflows/release.yml` 当前有版本一致性检查，这是可保留的基础。
- Windows job 没有代码签名配置。
- macOS job 使用 `APPLE_SIGNING_IDENTITY: "-"`，只是 ad-hoc 签名，不适合公开发行。
- 没有 Apple notarization/stapling、Linux GPG 签名或 updater 签名。
- 所有平台构建完成后会自动把 Draft Release 改为公开，缺少人工批准和平台级安全门禁。

### 待办

- [ ] **[P0]** 将发布流程改为：构建 → 测试 → 签名 → 验证 → 恶意软件扫描 → 生成 hash/SBOM → Draft Release → 人工审批 → 公开。
- [ ] **[P0]** 使用受保护的 GitHub Environment 保存生产签名权限，并要求人工 reviewer；普通 PR 和非保护分支不能调用生产签名。
- [ ] **[P0]** 只有明确支持的平台才能进入公开 Release；未签名/未公证的平台只保留为内部 artifact。
- [ ] **[P0]** OAuth 构建配置缺失时 release 必须失败；任何日志和 artifact 都不得包含原始 JSON、token 或凭据。
- [ ] **[P0]** 在 CI 中扫描最终二进制和安装包，并执行平台原生签名验证。
- [ ] **[P0]** 统一根 Cargo、Tauri Cargo 和 `tauri.conf.json` 版本；现有 tag 检查继续保留。
- [ ] **[P1]** 自动生成 changelog、已知限制、升级说明和 SHA-256。
- [ ] **[P1]** 保护 `main`、发布 tag 和 workflow 文件；要求 CI 通过后才能合并/打 tag。
- [ ] **[P1]** 建立签名密钥轮换、证书到期提醒和应急吊销流程。

### macOS 和 Linux

- [ ] **[P0]** 在准备好前从公开矩阵移除 macOS/Linux，避免自动发布“能构建但不能安全安装”的包。
- [ ] **[P2]** macOS 获取 Apple Developer Program 身份，使用 Developer ID Application 签名，完成 notarization 和 stapling。
- [ ] **[P2]** 在 Intel 和 Apple Silicon 真机验证 Gatekeeper、通知、托盘、钥匙串、开机启动、睡眠唤醒和卸载。
- [ ] **[P2]** Linux 选定实际支持的发行版与包格式，使用 GPG/仓库签名，并验证系统 keyring、托盘和 WebKitGTK 兼容性。

官方参考：

- [Tauri：macOS Code Signing and Notarization](https://v2.tauri.app/distribute/sign/macos/)
- [Tauri：Linux Code Signing](https://v2.tauri.app/distribute/sign/linux/)

## 八、功能与体验验收

### 必测主流程

- [ ] **[P0]** 全新安装 → 首次启动 → 添加账户 → 首次同步 → 关闭到托盘 → 再次打开。
- [ ] **[P0]** 163 授权密码、Gmail OAuth、Custom IMAP/SMTP 分别验证成功、失败和重新认证流程。
- [ ] **[P0]** 最多三个账户下的启动同步、手动刷新、托盘刷新、定时同步、切换账户和通知基线。
- [ ] **[P0]** 在线/离线启动、网络切换、代理/防火墙、DNS 失败、TLS 失败、睡眠唤醒和系统时间变化。
- [ ] **[P0]** 收件、正文 hydration、最近正文预取、搜索、HTML 隔离、回复历史和远程图片三种策略。
- [ ] **[P0]** 新建/已有草稿关闭语义、五分钟远端同步、版本冲突、副本保留和只读 MIME。
- [ ] **[P0]** SMTP 发送成功、明确失败、超时、`delivery_unknown`、人工重试和防重复投递。
- [ ] **[P0]** 账户删除、凭据删除、缓存删除、Google token 撤销和最后一个账户删除。
- [ ] **[P0]** 通知只包含 sender + subject，不泄露正文；首次历史导入不产生大量通知。
- [ ] **[P0]** 升级前后的设置、账户、草稿、Outbox、收藏、备注、头像和窗口几何保持正确。

### 支持矩阵和已知限制

- [ ] **[P0]** 对外明确列出当前未支持能力，例如 Outlook OAuth、完整附件、富文本写信和未闭环的服务器操作。
- [ ] **[P0]** 不为未经完整验收的邮箱服务商使用“兼容所有 IMAP/SMTP”的宣传。
- [ ] **[P1]** 建立 provider × OS × 操作的验收矩阵，并记录服务器 capability 差异。
- [ ] **[P1]** 建立发布候选版冻结期，由真实测试用户连续运行后台同步、托盘和通知至少数天。
- [ ] **[P1]** 完成无障碍、键盘导航、缩放、中文/英文、时区、长地址、长主题和高 DPI 验收。

## 九、法律、隐私页面与支持

- [ ] **[P0]** 添加并提交仓库级 `LICENSE`，与 Cargo 中声明的 MIT 保持一致。
- [ ] **[P0]** 发布公开 `PRIVACY.md` 或网站隐私政策，内容与真实代码和默认设置一致。
- [ ] **[P0]** 添加 `SECURITY.md`，提供私下报告漏洞的渠道和支持版本范围。
- [ ] **[P0]** 提供用户支持入口、最低系统要求、安装/卸载、数据目录、备份/删除和常见登录错误说明。
- [ ] **[P0]** 确认适用地区的隐私、消费者和出口合规要求；必要时进行专业法律审阅。
- [ ] **[P1]** 添加第三方许可证 notices、贡献说明和变更日志。
- [ ] **[P1]** 隐私政策或帮助页说明没有遥测/崩溃上传（若保持当前设计）；未来新增时必须先获得透明同意并更新政策。
- [ ] **[P1]** 为严重安全问题定义响应时间、修复版本、公告和撤销受影响证书/token 的流程。

## 十、每个候选版本的发布清单

- [ ] 从干净、受保护的 release commit/tag 构建。
- [ ] 所有版本号与 tag 一致。
- [ ] Rust core、React、Tauri tests/build/check/clippy 全部通过。
- [ ] 依赖漏洞和许可证检查通过或已有书面风险接受。
- [ ] OAuth 生产配置存在且 scope/品牌状态正确。
- [ ] 支持平台的应用、安装器、卸载器、更新包签名均验证通过。
- [ ] macOS 公证/stapling 或 Linux 包签名通过；不支持的平台无公开产物。
- [ ] 安装、升级、卸载和账户删除在干净设备通过。
- [ ] Gmail、163 和目标 Custom server 的真实账户验收通过。
- [ ] 日志和错误信息抽检无 secret、token、地址、主题、正文或 RFC822 泄漏。
- [ ] Release Notes 列出功能、修复、已知限制、数据 migration 和回滚注意事项。
- [ ] 生成并发布 SHA-256、SBOM/provenance 和 updater metadata。
- [ ] Draft Release 由非构建者复核后再公开。
- [ ] 发布后从公开下载地址重新下载并验证 hash、签名、安装和更新。
- [ ] 监控 OAuth 错误、下载失败、崩溃反馈和安全报告，准备快速补丁。

## 十一、公开 Beta 的完成定义

只有同时满足以下条件，才能把 Mine Mail 描述为“可供普通用户安全便捷使用的公开 Beta”：

- Windows 安装包及所有可执行组件有可信签名和时间戳。
- 干净 Windows 11 上不被 Smart App Control 因签名问题阻止。
- Gmail 已通过所需 OAuth 品牌与 restricted-scope 验证；否则公开构建不展示为公众可用。
- 用户能理解并执行账户撤权、本地缓存删除和完整应用数据删除。
- 隐私政策、Limited Use 说明、支持页、LICENSE 和 SECURITY 可公开访问。
- 有经过签名验证的安全更新方案。
- Release workflow 遇到缺少签名、OAuth 配置或验证失败时会停止，而不是公开降级产物。
- 支持范围内的安装、升级、同步、草稿、发送、通知、托盘和卸载已在真实环境完成验收。
