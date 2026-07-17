# Mine Mail

Mine Mail 是一个本地优先的跨平台桌面邮件客户端 MVP，技术栈为 **Rust + React + Tauri 2 + SQLite**。当前版本已经打通 163 邮箱的收件箱增量同步、双向草稿同步、SMTP 发信、后台轮询、系统通知、托盘和系统密钥环。

> 当前版本用于本机产品验证，尚未达到可公开分发的生产质量。

## 当前 MVP

### 收件箱与后台运行

- 启动时先从 SQLite 显示缓存，再在 Rust 后台立即同步。
- 后续按 UID 增量同步 INBOX，并更新 Flags、处理 UIDVALIDITY 变化和远端删除。
- 单封畸形邮件头会保存安全占位摘要，不会阻断后续邮件；按需下载正文前会再次校验 UIDVALIDITY，防止 UID 被复用后读错邮件。
- 用户可选择 `1 / 3 / 5` 分钟轮询，默认 `5` 分钟。
- 启动、托盘“刷新”、手动刷新、窗口重新唤起时立即同步。
- 第一次历史导入只建立通知基线，不弹出大量旧邮件通知。
- 后续后台收到新未读邮件时发送系统通知，只显示发件人和主题，不显示正文。
- 关闭窗口时隐藏到托盘；托盘菜单固定为 **打开 / 刷新 / 退出**。
- 开机自启是用户设置，默认关闭。

### 草稿与发信

- 编辑内容先保存到本地 SQLite，React 停止输入约 900 ms 后执行本地保存。
- Rust 每五分钟将草稿与服务器 Drafts 文件夹双向同步。
- 本地和远端草稿使用稳定 ID；支持远端草稿导入、删除同步及确定性的冲突副本。
- 编辑器通过独立的 SQLite `local_version` 做乐观并发控制；远端变化不会静默覆盖正在编辑的内容，过期编辑会保留为冲突副本，过期删除不会误删新版本。
- 含 HTML、附件、inline 内容或无法可靠解析的远端草稿会保守显示为只读，当前版本不会用纯文本内容覆盖它们。
- 手动保存会立即尝试远端同步。
- 发信前要求确认完整的 To/Cc/Bcc 收件人集合。
- 完整 MIME 邮件先进入本地 Outbox，再通过 SMTP 发送。
- 发送状态区分 `queued`、`sending`、`sent`、`retryable`、`rejected` 和 `delivery_unknown`；不自动重试结果不确定的邮件。
- `retryable` 可由用户在发件队列中明确点击重试；重试复用已经落盘的原始 MIME 和完整 SMTP envelope，不读取后来变化的草稿。
- 收件人确认、邮件内容和 Outbox 绑定同一个草稿版本；发送期间产生的新编辑会保留。旧的安全重试版本被新版发送取代后不可再次投递，`delivery_unknown` 会阻止任何隐式重发。

### 账户与凭据

- 授权密码与 Google OAuth 令牌只保存在按账户身份隔离的操作系统密钥环条目中，不写入 SQLite、React 状态、本地存储或账户配置文件。
- 密钥环临时不可用时仍可读取本地收件箱、草稿与发件队列；同步、未缓存正文、发送和重试会保持禁用并提示修复账户。
- 内置 163、Gmail、Outlook 和自定义 IMAP/SMTP 入口。
- 163 预填服务器地址并使用客户端授权密码；Gmail 通过系统浏览器完成 Google OAuth 2.0 登录，IMAP 与 SMTP 使用 XOAUTH2。
- 自定义账户可填写 IMAP、SMTP 主机/端口，并选择 SMTP 隐式 TLS 或 STARTTLS。
- Outlook 入口目前会说明并阻止配置，因为正式支持需要 OAuth 2.0 / Modern Auth，不能安全地只依赖账号密码。
- 最多可连接 3 个账户。界面一次显示一个活动账户，启动、托盘刷新、手动刷新与定时轮询会同步全部已连接账户；切换账户时立即读取各自 SQLite 缓存。
- Google 登录使用桌面应用 Authorization Code + PKCE 与随机 loopback 回调；短期 access token 自动刷新，refresh token 不会跨越 Rust/React 边界。

### 配置 Google 登录

1. 在 Google Cloud Console 配置 OAuth consent screen，并创建类型为 **Desktop app** 的 OAuth client。
2. 为 consent screen 加入 `openid`、`email` 与 `https://mail.google.com/` scope。后者是 Gmail IMAP/SMTP 所需的受限 scope；面向测试用户以外发布前需要按 Google 要求完成验证。
3. Mine Mail 已内置项目的 Desktop OAuth client ID；最终用户无需设置环境变量或提供 client secret，点击 **使用 Google 登录** 即会打开系统默认浏览器。

Google Cloud 项目处于 Testing 状态时，只有 Audience 中列出的测试用户可以授权；包含 Gmail scope 的测试授权及 refresh token 通常会在 7 天后失效，需要重新登录。

### 界面

- 三栏桌面布局，共享一张连续的非写实风景背景。
- Daylight、Night、Dusk、Forest 四套主题。
- 无边框标题栏融入主题，保留最小化、最大化和关闭操作。
- 本地搜索/筛选、纯文本正文阅读、写信、回复、转发、草稿与 Outbox 状态。

## 架构

```text
React UI
   |
   | narrow Tauri commands / desktop events
   v
Tauri desktop runtime
   |
   +-- background scheduler / tray / notifications / autostart
   +-- OS keyring and non-secret account metadata
   v
Rust MailBackend
   |
   +-- IMAP sync and MIME parsing
   +-- SMTP and Outbox
   +-- bidirectional Drafts sync
   `-- SQLite repository
```

- Rust 与 SQLite 是邮件状态的唯一事实来源；React 不直接访问 IMAP、SMTP、凭据文件或数据库。
- UI 启动不等待网络，网络错误不会阻止读取已有缓存。
- 邮件摘要优先同步，正文打开时按需获取并缓存。
- Tauri 只向 React 返回界面需要的数据，不返回授权密码或完整原始 RFC822 邮件。
- Web/Vite 仅用于前端构建和自动化测试，不再维护一套可连接邮箱的 Web 运行时。

## 本地开发账户迁移

仓库已有的 `password.txt` 使用两行格式：

```text
your-account@163.com
your-163-authorization-code
```

首次启动桌面应用且尚无账户配置时，开发版本会读取该文件，验证账户并把授权密码导入系统密钥环；后续运行从密钥环读取。文件不会被程序自动删除。确认迁移成功后可自行移走该明文文件。

`password.txt`、SQLite 数据库、WAL 文件和构建产物均应保持在 Git 之外。不要把授权密码写进源码、前端环境变量、日志、截图或 issue。

## 运行桌面应用

需要 Rust、Node.js 和 Tauri 对应平台的系统依赖。

```powershell
cd Z:\mine-mail\web
npm install
npm run tauri:dev
```

桌面端账户配置保存在操作系统应用数据目录；公开账户元数据与密钥环凭据分离，每个账户使用不含明文邮箱地址的哈希数据库文件名。根目录 CLI 的 `data/mine-mail.db` 与桌面数据库相互独立。

本地构建：

```powershell
cd Z:\mine-mail\web
npm run tauri:build
```

构建成功不代表安装包已经完成 Windows 签名、macOS 签名/公证或 Linux 发行版兼容验收。

## 后端 CLI 验收

CLI 使用根目录 `password.txt` 和 `data/mine-mail.db`，所有常规输出都会省略凭据、正文和原始邮件。

```powershell
cd Z:\mine-mail

# 验证 IMAP 与 SMTP 登录
cargo run -- check

# 增量同步收件箱
cargo run -- sync-inbox --initial-limit 50

# 只查看本地缓存摘要
cargo run -- list-inbox --limit 20

# 按 IMAP UID 获取并缓存正文
cargo run -- fetch-message 123

# 本地保存并同步一封草稿
cargo run -- draft-save --to recipient@example.com --subject "测试草稿" --body "测试正文"
cargo run -- draft-sync <DRAFT_ID>

# 发信时确认集合必须与 To/Cc/Bcc 完全一致
cargo run -- send --to recipient@example.com --subject "测试邮件" --body "测试正文" --confirm-recipient recipient@example.com

# 查看安全的 Outbox 状态摘要
cargo run -- outbox

# 仅人工重试状态为 retryable 的已落盘邮件
cargo run -- retry-outbox <OUTBOX_ID>
```

命令行 `--body` 参数可能进入终端历史，不应用于敏感正文。桌面端通过进程内命令传递正文。

## 验证命令

Rust 邮件核心：

```powershell
cd Z:\mine-mail
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

React：

```powershell
cd Z:\mine-mail\web
npm test -- --run
npm run build
```

Tauri runtime：

```powershell
cd Z:\mine-mail\web\src-tauri
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

自动化测试不会自行向真实地址发信。真实 SMTP 验收必须使用明确的测试主题，并仅发送到用户授权的地址。

## 已完成的真实验收

- 163 IMAP 与 SMTP 鉴权均成功。
- 当前服务器 INBOX 为 761 封，本地缓存 103 封摘要；连续两次同步均获取 0 封新摘要且 UIDVALIDITY 未变化，证明增量同步幂等。
- 一封带唯一时间戳主题的草稿已成功上传服务器 Drafts，随后发送成功并从草稿同步状态中清理。
- Tauri 桌面运行时成功读取系统密钥环；非秘密账户配置中未出现授权密码。
- 关闭窗口后进程继续存活且窗口隐藏；启动第二实例会唤起已有窗口。
- 开机自启默认关闭；默认轮询为 5 分钟；通知历史基线已经建立。
- 自动化回归当前为 Rust 49 项、React 35 项、Tauri 13 项；生产前端构建以及两套 Rust 严格 Clippy 均通过。

## 当前限制

- 目前最多连接 3 个账户，并逐账户同步；尚无把多个账户混排在一起的统一收件箱。
- Outlook OAuth 2.0 / Modern Auth 尚未实现。
- 首次同步当前只缓存最近 100 封摘要，尚无向更早邮件翻页/回填的界面。
- 邮件仅支持纯文本显示和撰写；尚无安全 HTML 渲染、远程图片控制、富文本、附件上传/下载。含这些内容的远端草稿会只读保护。
- 已读、星标、归档、垃圾箱、已发送等服务器文件夹操作尚未形成完整闭环。
- 当前使用定时轮询而非 IMAP IDLE。
- `retryable` 只支持用户明确触发的人工重试，尚无自动重试 worker；`delivery_unknown` 按设计永不自动重试。
- 后台新邮件通知的代码路径、权限、基线和窗口生命周期已有测试；本轮没有为了触发通知而额外向 163 测试账户发送一封入站邮件。
- 当前只在 Windows 11 上完成实机联调；macOS 与 Linux 尚需真实设备、密钥环、通知、托盘和安装包验收。
- 当前 SQLite 明文保存邮件正文、草稿和 Outbox MIME；它防止授权密码落库，但不防御能够读取本机用户文件的攻击者。
