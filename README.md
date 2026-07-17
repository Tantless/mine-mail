# Mine Mail

> 一个本地优先、专注邮件本身的跨平台桌面邮箱客户端。

Mine Mail 使用 **Tauri 2 + React 19 + Rust + SQLite** 构建。项目希望在保留 IMAP/SMTP 邮件核心能力的同时，提供更轻、更快、更安静，也更适合个性化主题的桌面体验。

> [!IMPORTANT]
> Mine Mail 目前是开发预览版（MVP），尚未提供经过签名的公开安装包，也未达到生产环境所需的完整兼容性与安全审计标准。当前实机开发和验收平台为 Windows 11；macOS 与 Linux 是目标平台，但仍需要真实设备验证。


## 当前进度

### 已实现

- 163 邮箱 IMAP 增量同步与 SMTP 发信。
- Gmail OAuth 2.0、XOAUTH2 IMAP/SMTP 与令牌刷新。
- 自定义 IMAP/SMTP 账户；最多连接 3 个账户并逐账户同步。
- SQLite 本地缓存，启动时先显示本地邮件，再在 Rust 后台同步。
- 收件箱摘要、按需正文获取、最近正文预取与本地搜索。
- 纯文本阅读，以及经过清理和隔离的 HTML 邮件阅读。
- 简单 HTML 使用主题化原生阅读器；复杂发件人排版使用无脚本隔离 iframe。
- 远程图片自动加载、询问或阻止策略。
- 常见回复格式识别；正文和引用历史分离为同级可折叠卡片。
- 本地草稿、每 5 分钟远端同步、远端草稿导入与版本冲突保护。
- 本地 Outbox、完整收件人确认、SMTP 投递状态与人工安全重试。
- 写信、回复、转发、Cc/Bcc、可拖动和缩放的玻璃拟态写信面板。
- Daylight、Night、Dusk、Forest 四套主题和本地自定义头像。
- IMAP IDLE 实时推送；不支持 IDLE 的服务器使用持久连接轻量探测。
- 1/3/5 分钟完整校准、托盘运行、可选开机启动。
- 可配置的新邮件声音，以及主题化的桌面右下角通知卡片。

### 仍在开发

- Outlook OAuth 2.0 / Modern Auth。
- 多账户统一收件箱。
- 更早邮件的分页回填。
- 富文本写信、内嵌图片和完整附件收发流程。
- 已读、星标、归档、垃圾箱、已发送等服务器操作的完整闭环。
- macOS/Linux 实机适配、签名、公证和发行包验收。

## 架构

```text
React UI
   │
   │ narrow Tauri commands / desktop events
   ▼
Tauri desktop runtime
   ├─ tray / notifications / autostart
   ├─ per-account IDLE / lightweight monitor
   ├─ reconciliation scheduler
   └─ OS credential store
   │
   ▼
Rust MailBackend
   ├─ IMAP synchronization
   ├─ SMTP + Outbox
   ├─ MIME / safe HTML processing
   ├─ bidirectional draft synchronization
   └─ SQLite repositories
```

Rust 与 SQLite 是邮件状态的事实来源。React 只通过窄范围 Tauri command 读取本地状态和发起用户操作，不直接访问 IMAP、SMTP、凭据或数据库。

收件箱采用“推送优先、校准兜底”：运行时根据服务器实际公布的 IMAP 能力选择策略。支持标准 `IDLE` 的服务器由长连接即时唤醒；163 等不公布 `IDLE` 的服务器复用认证连接，在前台约每 15 秒、后台约每 30 秒读取邮箱计数。只有检测到变化才拉取新 UID 并提交 SQLite；用户选择的 1/3/5 分钟间隔用于完整校准删除、旗标和异常状态。启动、手动刷新和托盘刷新仍立即执行同步。

项目没有另一套可连接真实邮箱的 Web 运行时。Vite 页面只用于前端构建、自动化测试和可选的无网络 UI 演示。

## 仓库结构

```text
mine-mail/
├─ src/                       # 独立 Rust 邮件核心与 CLI
├─ web/
│  ├─ src/                   # React UI
│  ├─ src-tauri/             # Tauri 桌面 runtime
│  └─ design/                # 设计参考与历史 QA 图
├─ design-qa/                # 当前产品视觉验收记录
├─ Cargo.toml                # Rust 邮件核心
├─ rust-toolchain.toml       # 项目 Rust 工具链
└─ AGENTS.md                 # 持久架构和产品约束
```

## 快速开始

### 1. 安装开发环境

需要准备：

- [Git](https://git-scm.com/)
- [Node.js 24 LTS](https://nodejs.org/)
- [Rustup](https://rust-lang.org/tools/install/)
- 当前操作系统对应的 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)

Windows 需要 Microsoft C++ Build Tools 和 Microsoft Edge WebView2；macOS 需要 Xcode Command Line Tools；Linux 所需的 WebKitGTK、编译工具和系统库随发行版而异，请以 Tauri 官方依赖清单为准。

本仓库固定使用 Rust 1.97，并包含 `rustfmt` 和 `clippy`。首次开发前建议显式安装：

```powershell
rustup toolchain install 1.97.0 --profile minimal
rustup component add rustfmt clippy --toolchain 1.97.0
```

### 2. 克隆并安装前端依赖

```powershell
git clone https://github.com/Tantless/mine-mail.git
cd mine-mail
cd web
npm ci
```

使用 `npm ci` 可以严格按照 `package-lock.json` 安装依赖。首次安装与首次 Rust 编译需要联网。

### 3. 启动桌面开发版

在 `web` 目录运行：

```powershell
npm run tauri:dev
```

Tauri 会启动 Vite、编译 Rust 桌面 runtime 并打开 Mine Mail。第一次编译可能需要较长时间，并产生数 GB 的 Cargo 构建缓存；后续增量构建会明显更快。

应用可以在没有 `password.txt` 的情况下启动。首次进入后可从账户设置添加 163、Gmail 或自定义 IMAP/SMTP 账户。

### 4. 只开发 React 界面（可选）

该模式不连接真实邮箱，只提供演示数据：

```powershell
cd web
$env:VITE_MINE_MAIL_DEMO = "1"
npm run dev
```

打开终端中显示的本地地址即可。需要验证托盘、通知、密钥环、窗口和真实邮件功能时，必须使用 `npm run tauri:dev`。

## 邮箱联调

### 163 邮箱

推荐直接从应用的账户设置添加 163 邮箱，填写邮箱地址和客户端授权密码。不要使用网页登录密码。

项目仍保留开发账户的一次性迁移入口：在仓库根目录创建不会被 Git 追踪的 `password.txt`：

```text
your-account@163.com
your-163-authorization-code
```

桌面应用首次启动且没有账户配置时，会验证该账户并把授权密码导入操作系统凭据存储。确认迁移成功后应移走明文文件。

### Gmail

仓库不包含 Google OAuth client secret。没有本地 OAuth JSON 时项目仍然可以编译和开发，但 Gmail 登录入口不可用。

有权使用 Mine Mail Google Cloud 项目的协作者，可将 Desktop app OAuth JSON 保存为：

```text
web/src-tauri/google-oauth-client.json
```

该文件已被 `.gitignore` 排除，严禁提交、粘贴到 issue 或写入日志。当前构建脚本会校验它是否匹配项目内置的 OAuth client ID；普通 fork 如需使用自己的 Google Cloud 项目，需要同步调整 OAuth 构建配置。

Google Cloud 项目处于 Testing 状态时，还需要把测试邮箱加入 OAuth consent screen 的测试用户列表。

### Outlook 与自定义服务器

Outlook 目前只展示能力说明，不允许使用不安全的传统账号密码配置；正式接入需要完成 Microsoft OAuth 2.0 / Modern Auth。

自定义账户可以填写 IMAP/SMTP 主机、端口和 TLS 模式。服务器兼容性尚未经过完整矩阵测试。

## 常用开发命令

### React

```powershell
cd web
npm test -- --run
npm run build
```

### Rust 邮件核心

```powershell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

### Tauri runtime

```powershell
cd web/src-tauri
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo check
```

### 构建桌面安装包

```powershell
cd web
npm run tauri:build
```

产物位于 `web/src-tauri/target/release/bundle/`。本地构建成功不代表安装包已经完成 Windows 签名、macOS 签名/公证或 Linux 发行版兼容验收。

## 后端 CLI

根目录还提供不依赖 React 的邮件核心 CLI，使用根目录 `password.txt` 和 `data/mine-mail.db`：

```powershell
# 验证 IMAP/SMTP 登录
cargo run -- check

# 增量同步并读取本地摘要
cargo run -- sync-inbox --initial-limit 50
cargo run -- list-inbox --limit 20

# 获取并缓存指定 IMAP UID 的正文
cargo run -- fetch-message 123

# 查看本地 Outbox
cargo run -- outbox
```

CLI 的 `--body` 参数会进入终端历史，不应用于敏感正文。自动化测试不会自行向真实地址发送邮件。

## 本地数据与安全边界

- 邮箱授权密码和 OAuth token 保存在操作系统凭据存储中。
- 非秘密账户元数据、邮件摘要、正文缓存、草稿和 Outbox 保存在桌面应用数据目录的 SQLite 数据库中。
- 当前 SQLite 数据库未做整库加密，无法防御能够读取本机用户文件的攻击者。
- HTML 邮件按不可信输入处理：清理危险内容、禁止脚本，并对复杂结构使用隔离 iframe。
- `password.txt`、OAuth JSON、数据库、日志、构建目录和前端依赖目录均不应提交 Git。

提交代码前请检查：

```powershell
git status
git diff --check
```

## 磁盘空间

GitHub 仓库本身主要由代码、主题资源和设计 QA 图片组成。开发目录变大通常来自两套 Cargo `target`、前端依赖和打包产物，而不是 Git 下载内容。

需要释放本地编译缓存时可以分别执行：

```powershell
# 根目录 Rust 核心
cargo clean

# Tauri runtime
cd web/src-tauri
cargo clean
```

这不会删除源码、账户数据库或 Git 历史，但下一次编译会重新下载/构建必要依赖。

## 开发约束

- 保持本地优先：React 先显示 SQLite，再由 Rust 后台同步。
- 不在 React、日志、错误信息或 Git 中暴露凭据和完整 RFC822 原文。
- 不让 UI 直接等待 IMAP/SMTP。
- 邮件 HTML 必须经过 Rust 清理和结构判定。
- 同步、草稿和发送逻辑必须支持失败恢复，并避免隐式重复投递。
- 修改产品的持久架构或交互约束前，请先阅读 `AGENTS.md`。

## 发布状态与许可证

Mine Mail 目前没有正式 Release，仓库也尚未提供面向终端用户的安装支持。两个 Cargo package 当前声明为 MIT；正式对外分发前仍需补充仓库级 `LICENSE`、贡献规范、CI 和平台签名流程。
