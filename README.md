# Mine Mail

Mine Mail 是一个本地优先的跨平台邮件客户端 MVP。项目由 Rust 邮件核心、SQLite 持久化、React 界面与 Tauri 2 桌面壳组成；当前目标是验证 163 单账户的收信、写信、发信、草稿和同步闭环，以及一套可继续迭代的桌面视觉方向。

> 当前版本仅供本地开发与产品验证，不是可对外分发的生产版本。

## MVP 界面能力

- 三栏邮件布局：左侧导航、中间邮件列表、右侧阅读区，共享一张连续的非写实风景背景；内容面板保留窄边距露出背景。
- 四套可切换主题：`Daylight`、`Night`、`Dusk`、`Forest`，选择会保存在浏览器本地存储中。
- 收件箱列表、关键词搜索、全部/未读/星标筛选、邮件选择与纯文本正文阅读。
- 手动同步收件箱；桌面模式会调用真实 163 IMAP，Web 模式使用内置模拟数据。
- 写信、回复、转发、收件人/抄送/密送输入、本地保存草稿。
- 发信前显示收件人确认；桌面端还会在 Rust 边界再次校验确认集合，只有与 To/Cc/Bcc 完全一致时才允许 SMTP 发送。
- 响应式窄屏布局、键盘快捷键（`N` 写信、`Ctrl/Command + K` 聚焦搜索）。

## MVP 架构与运行模式

```text
React UI
   |
   |-- Web 开发模式 --> 内置模拟邮件/草稿（不连接邮箱）
   |
   `-- Tauri 桌面模式 --> 窄范围 Tauri Commands
                              |
                          MailBackend
                         /     |      \
                      IMAP    SMTP   Repository
                         \     |       /
                        MIME 解析与生成   SQLite
```

- React 不直接访问 IMAP、SMTP、凭据文件或 SQLite。
- Web 模式用于快速确认界面与交互，数据仅为进程内模拟数据，刷新后会重置。
- Tauri 模式复用根目录 Rust crate 的 `MailBackend`，通过命令完成连接检查、收件箱同步/读取、正文按需获取、草稿保存与发送。
- CLI 默认使用 `data/mine-mail.db`；Tauri 桌面端将独立数据库放在操作系统的应用本地数据目录中。
- Tauri 只向 React 返回纯文本正文与附件名称，不向界面暴露 HTML 或原始 RFC822 内容。

## 本地优先与后端安全边界

- 启动先读取 SQLite；网络同步在后端进行，界面层不持有邮箱连接。
- 收件箱先同步邮件摘要、Flags 和 UID，正文仅在打开邮件时按需获取；单封正文缓存上限为 50 MiB。
- 草稿先写入本地数据库，再显式同步到服务器草稿箱。远端草稿带稳定私有标识，降低重复追加风险。
- 发信前先写入本地 Outbox，再调用 SMTP；状态区分 `queued`、`sending`、`sent`、`retryable`、`rejected` 和 `delivery_unknown`。结果不确定时不会盲目重复发送。
- 授权密码不会写入 SQLite、源码或命令输出，仅从本地凭据文件读取，并在内存中使用可清零字符串保存。
- 当前 SQLite 会明文保存邮件正文、草稿和 Outbox 原文；它防止授权码落库，但不防御能够读取本机用户文件的攻击者。
- CLI 的 `--body` 会进入终端历史，不适合输入敏感正文；桌面界面通过进程内调用传递内容。

## 配置 163 邮箱

在项目根目录创建 `password.txt`，严格使用两行格式（不要加标签或引号）：

```text
your-account@163.com
your-163-authorization-code
```

第二行应是 163 邮箱开启 IMAP/SMTP 后生成的**客户端授权密码**，不是示例文本，也不建议使用网页登录密码。默认连接 `imap.163.com:993` 与 `smtp.163.com:465`。

如需改用其他位置，可给 CLI 增加 `--credentials`：

```powershell
cargo run -- --credentials "D:\private\mail-credentials.txt" check
```

桌面调试也可通过环境变量指定凭据文件：

```powershell
$env:MINE_MAIL_CREDENTIALS_FILE = "D:\private\mail-credentials.txt"
cd Z:\mine-mail\web
npm run tauri:dev
```

未设置该变量时，Tauri 的 debug 构建会读取项目根目录的 `password.txt`。

## 凭据与发布安全

- `password.txt`、数据库及 WAL 文件已加入 `.gitignore`；不要提交、上传、截图或分享这些文件，也不要把授权码写进前端环境变量。
- 请限制凭据文件的系统访问权限。当前文件凭据方式只适用于本机 debug 开发。
- release 构建不会回退读取仓库内的 `password.txt`，当前只能通过 `MINE_MAIL_CREDENTIALS_FILE` 显式指定文件；这仍是开发桥接方案。
- **任何打包分发之前，必须接入 Windows Credential Manager、macOS Keychain 与 Linux Secret Service 等系统密钥环**，并完成凭据迁移、删除与错误恢复流程。
- 当前代码不应被视为已经完成生产级凭据保护或发布安全审计。

## Web 与桌面开发运行

### React Web（安全的界面模拟模式）

此模式不读取 `password.txt`，不会连接真实邮箱，也不会发送真实邮件。

```powershell
cd Z:\mine-mail\web
npm install
npm run dev
```

然后打开 Vite 输出的本地地址（默认 `http://localhost:1420`）。

### Tauri 桌面（连接真实开发邮箱）

```powershell
cd Z:\mine-mail\web
npm install
npm run tauri:dev
```

桌面模式会使用真实 IMAP/SMTP 和独立的应用数据目录。同步、打开未缓存正文与确认发信都会访问真实邮箱服务；发信前请再次核对所有 To/Cc/Bcc 地址。

## 数据库

CLI 默认数据库位置为：

```text
data/mine-mail.db
```

SQLite 使用 WAL、外键约束和事务，保存账户公开配置、邮箱同步状态、邮件缓存、草稿与 Outbox；不保存授权密码。可通过 CLI 全局参数覆盖位置：

```powershell
cargo run -- --database "D:\mail-data\mine-mail.db" init
```

Tauri 桌面端不会使用这个 CLI 默认文件，而是在操作系统的应用本地数据目录创建 `mine-mail.sqlite3`。

## CLI 快速验收

所有命令返回结构化 JSON。以下示例均使用虚构地址，不代表真实收件人。

```powershell
# 初始化或迁移数据库
cargo run -- init

# 验证 IMAP 与 SMTP 登录
cargo run -- check

# 查看服务器邮箱文件夹
cargo run -- folders

# 首次最多同步最近 50 封摘要；后续执行为增量同步
cargo run -- sync-inbox --initial-limit 50

# 查看本地缓存摘要（不输出正文）
cargo run -- list-inbox --limit 20

# 按 IMAP UID 获取并缓存一封正文，输出仍只显示元数据与正文长度
cargo run -- fetch-message 123

# 保存本地草稿
cargo run -- draft-save --to recipient@example.com --subject "示例草稿" --body "示例正文"

# 查看草稿，再使用返回的 ID 同步到服务器草稿箱
cargo run -- draft-list
cargo run -- draft-sync <DRAFT_ID>

# 发信：确认地址必须与 To/Cc/Bcc 的完整集合完全一致
cargo run -- send --to recipient@example.com --subject "示例邮件" --body "示例正文" --confirm-recipient recipient@example.com

# 发送已有草稿
cargo run -- send-draft <DRAFT_ID> --confirm-recipient recipient@example.com

# 查看本地发送结果与安全状态（不输出原始邮件）
cargo run -- outbox
```

## 测试与构建

后端检查：

```powershell
cd Z:\mine-mail
cargo fmt --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

React 检查与 Web 构建：

```powershell
cd Z:\mine-mail\web
npm install
npm test
npm run build
```

Tauri 桌面编译检查与本地打包：

```powershell
cd Z:\mine-mail\web
cargo check --manifest-path src-tauri\Cargo.toml
npm run tauri:build
```

自动化测试使用临时数据库或模拟前端数据，不会自行向真实地址发信。只有显式执行 CLI 发信命令，或在 Tauri 界面确认收件人并点击发送时，才会调用 SMTP。`tauri:build` 产物仍是开发阶段构建，不代表具备分发条件。

## 当前已实现

- 163 邮箱 IMAP/SMTP TLS 登录与连接探测。
- 远端文件夹发现、INBOX 增量同步、UIDVALIDITY 重置、Flags 更新与本地删除对账。
- 邮件摘要优先、正文按需获取、MIME 解析及 SQLite 缓存。
- 本地草稿保存、草稿列表和远端 Drafts 同步。
- 纯文本写信、发信、精确收件人确认，以及本地 Outbox 状态记录。
- React 三栏 MVP、四主题、搜索/筛选/阅读、写信与草稿交互。
- Tauri 2 桌面壳及连接 `MailBackend` 的窄范围命令边界。
- 不暴露密码、原始邮件和正文内容的 JSON 验收 CLI。

## 当前 MVP 限制

- 只支持一个 163 IMAP/SMTP 账户；尚无多账户、OAuth、POP3 或通用邮箱配置向导。
- 邮件只以纯文本显示和撰写；尚无安全 HTML 渲染、远程图片控制、富文本或附件上传/下载。
- 收件箱同步与按需正文读取已接入；星标、已读、归档、垃圾箱、已发送等部分文件夹操作仍以界面原型或本地状态为主，未完整回写服务器。
- 草稿可本地保存；服务器草稿同步仍主要通过后端 CLI 能力验收，前端尚未覆盖完整生命周期。
- 没有后台 IMAP IDLE、定时自动同步、系统通知、自动重试调度或后台常驻策略；同步需要用户手动触发。
- Web 模式完全使用模拟数据；只有 Tauri 模式会连接真实后端与 SQLite。
- 尚未完成系统密钥环、安装包签名/公证、自动更新、发布渠道与跨平台真实设备测试。
- 当前 UI 是第一版视觉 MVP，尚未完成完整无障碍审计、国际化和大规模邮件性能验证。

## 里程碑状态

项目已进入 **React + Tauri MVP 集成阶段**：后端与 SQLite 邮件闭环可用，Web 模式用于界面验证，Tauri 模式用于真实本地联调。下一阶段应优先完成真实设备验收、服务器状态回写、HTML/附件安全方案与系统密钥环，再讨论可分发版本。
