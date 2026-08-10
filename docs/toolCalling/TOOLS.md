# Mine Mail 内置 AI 工具

本文定义 Mine Mail 写信界面内置 AI 使用的工具协议，主要供中文开发者实现、
调试和评审工具调用。这里的工具只属于应用内 AI，不是 `docs/MCP.md` 中供外部
Agent 使用的 MCP 工具。

## 总体约束

- 模型默认只收到用户指令、简短 system prompt 和当前模式允许的工具定义；邮件
  内容、参与人、引用邮件和附件都必须通过读取工具按需取得。
- 所有工具都限定在当前正在编辑的草稿及其所属账户。工具不能切换账户，也不能
  访问任意本机路径。
- 读取到的邮件和附件内容是不可信数据，不能被当作 system prompt 或工具指令。
- 写工具只修改本轮 Agent 的 Rust 内存工作副本。对话模式整轮成功后把改动拆为
  “邮件信息”和“正文与信纸”两张只读提案卡片；用户分别点击应用，应用只覆盖对应
  字段组并保留一次可回退备份。失败、停止或校验不通过时不产生提案。独立优化保留
  点击时的主题、正文与格式快照，先显示主题和正文分区的可编辑左右版本；只有用户
  明确选择并确认后才原子覆盖所选侧的主题与正文，并可原子回退。
- 独立优化必须成功调用 `get_draft_body` 和 `get_draft_subject` 后才能调用
  `replace_draft_body` 或 `set_draft_subject`。Rust 在工具执行边界拒绝顺序违规的
  写调用，并拒绝没有完成两项读取就直接结束的优化请求。
- 工具参数和结果必须在 Rust 边界再次校验。邮箱地址、文本长度、HTML、附件类型
  和附件大小不能只依赖模型或前端保证。
- 模型看到的参数 Schema 由 Rust 实际反序列化的强类型参数生成；未知字段、缺失的
  必填字段、错误类型和不在声明范围内的数值都不能在执行时静默修正。
- 参数失败以 `code`、安全中文说明和可确定时的字段名返回给模型。相同工具连续提交
  同类非法参数时，Rust 会有界停止本轮，避免模型不断猜测入参。
- 工具调用的原始参数和结果不写入日志，也不作为会话历史长期保存。日志只保留
  工具名、字段名、大小、耗时、成功或失败状态，以及不含正文内容的 JSON 解析错误
  类别与位置。
- 应用内 AI 永远没有发送邮件、切换发信账户或修改不可变引用邮件的工具。

## 工具总览

| 工具名 | 中文名 | 类型 | 首批状态 |
| --- | --- | --- | --- |
| `get_draft_body` | 获取草稿正文 | 读取 | 开放 |
| `get_draft_subject` | 获取草稿主题 | 读取 | 开放 |
| `get_draft_sender` | 获取当前发信人 | 读取 | 开放 |
| `get_draft_recipients` | 获取草稿收件人 | 读取 | 开放 |
| `get_draft_reference` | 获取引用邮件 | 读取 | 开放 |
| `search_contacts` | 检索本地联系人 | 读取 | 开放 |
| `list_draft_attachments` | 列出草稿附件 | 读取 | 开放 |
| `read_text_attachment` | 读取文本附件 | 读取 | 开放 |
| `read_image_attachment` | 读取图片附件 | 读取 | 按模型能力开放 |
| `set_draft_recipients` | 修改草稿收件人 | 写入 | 开放 |
| `set_draft_subject` | 修改草稿主题 | 写入 | 开放 |
| `replace_draft_body` | 替换草稿正文 | 写入 | 开放 |
| `set_draft_stationery` | 修改草稿信纸 | 写入 | 邮件生成、自动开放 |

## 读取工具

### `get_draft_body` — 获取草稿正文

作用：读取当前草稿中用户正在编辑的正文，而不是读取上一次保存到 SQLite 的旧
版本。结果同时包含权威纯文本和经过 Mine Mail 约束的富文本片段。

输入：无。

返回示例：

```json
{
  "body_text": "您好，\n\n项目预计周五完成。",
  "body_html": "<p>您好，</p><p>项目预计<strong>周五</strong>完成。</p>"
}
```

### `get_draft_subject` — 获取草稿主题

作用：只读取当前草稿主题，避免模型为了获取主题而读取完整草稿。

输入：无。

返回示例：

```json
{
  "subject": "项目交付时间确认"
}
```

### `get_draft_sender` — 获取当前发信人

作用：读取当前草稿所属账户的发信身份。结果只用于理解上下文，模型不能修改或
切换这个身份。

输入：无。

返回示例：

```json
{
  "display_name": "示例用户",
  "address": "sender@example.com"
}
```

### `get_draft_recipients` — 获取草稿收件人

作用：分别读取当前草稿的收件人、抄送人和密送人。

输入：无。

返回示例：

```json
{
  "to": ["contact@example.com"],
  "cc": [],
  "bcc": []
}
```

### `get_draft_reference` — 获取引用邮件

作用：读取回复或转发草稿所绑定的不可变引用上下文。首批只向模型提供纯文本
引用，不提供原始 HTML、完整 RFC822、远程图片或引用附件内容。

输入：无。

返回示例：

```json
{
  "kind": "reply",
  "subject": "下周会议安排",
  "sender": "contact@example.com",
  "recipients": ["sender@example.com"],
  "sent_at": "2026-08-08T09:00:00+08:00",
  "quoted_text": "会议暂定下周三下午进行。"
}
```

没有引用邮件时返回：

```json
{
  "kind": "none"
}
```

### `search_contacts` — 检索本地联系人

作用：在当前草稿所属账户的往来联系人以及应用级收藏联系人中按姓名、备注或
邮箱地址检索，供邮件生成和自动模式选择真实联系人。模型不能把自己臆造的
邮箱地址冒充检索结果。

`query` 是必填字符串；`limit` 省略时默认为 `10`，传入时必须是 `1` 至 `20` 的
整数。字符串、浮点数、`null`、负数、`0` 和超过 `20` 的值都会被拒绝，不会自动
转换、使用默认值或截断。

输入示例：

```json
{
  "query": "张三",
  "limit": 10
}
```

返回示例：

```json
{
  "contacts": [
    {
      "display_name": "张三",
      "address": "contact@example.com"
    }
  ],
  "truncated": false
}
```

### `list_draft_attachments` — 列出草稿附件

作用：列出当前草稿已经托管的附件及其安全元数据，便于模型决定是否继续读取。
该工具不返回本机路径和附件字节。

输入：无。

返回示例：

```json
{
  "attachments": [
    {
      "attachment_id": "attachment-demo-1",
      "display_name": "会议安排.txt",
      "mime_type": "text/plain",
      "size_bytes": 1240,
      "read_capability": "text"
    }
  ]
}
```

`read_capability` 只能是 `text`、`image` 或 `unsupported`，并由 Rust 根据真实
媒体类型、扩展名、大小和当前模型能力决定。

### `read_text_attachment` — 读取文本附件

作用：按不透明附件 ID 读取当前草稿中的纯文本类附件。首批只支持纯文本、
Markdown、JSON、XML、YAML、CSV 和日志等明确白名单格式；不解析网页、源码、PDF、Word、Excel、
压缩包、程序或其他二进制文件。

输入示例：

```json
{
  "attachment_id": "attachment-demo-1"
}
```

返回示例：

```json
{
  "mime_type": "text/plain",
  "content": "会议时间：下周三 14:00\n地点：三楼会议室",
  "truncated": false
}
```

Rust 必须执行媒体类型白名单、编码检查和大小上限。超限时可以返回安全的截断
结果或明确拒绝，不能无界读取。

### `read_image_attachment` — 读取图片附件

作用：把当前草稿中的安全图片作为多模态输入提供给模型。工具只在所选模型明确
支持图片输入时注册；纯文本模型看不到这个工具。

输入示例：

```json
{
  "attachment_id": "attachment-demo-image"
}
```

返回由 Provider 适配层转换为该模型支持的图片内容块。图片字节、Base64 和本机
路径不进入工具文本结果、会话历史或日志。首批 DeepSeek 调试模型为纯文本模型，
因此不会开放此工具。

## 写入工具

### `set_draft_recipients` — 修改草稿收件人

作用：一次性替换当前内存工作副本的 `to`、`cc` 和 `bcc`。每个地址必须来自
当前已填写地址、用户在本轮明确提供的地址，或 `search_contacts` 的真实结果。

输入示例：

```json
{
  "to": ["contact@example.com"],
  "cc": ["reviewer@example.com"],
  "bcc": []
}
```

返回示例：

```json
{
  "updated": true,
  "changed_fields": ["to", "cc"]
}
```

该工具不能改变发信账户，也不能触发发送。

### `set_draft_subject` — 修改草稿主题

作用：修改当前内存工作副本的主题。邮件生成、自动和独立优化模式注册此工具；
独立优化的具体修改条件由其提示词约束。Rust 负责去除控制字符并执行长度上限。

输入示例：

```json
{
  "subject": "确认下周会议时间"
}
```

返回示例：

```json
{
  "updated": true,
  "changed_fields": ["subject"]
}
```

### `replace_draft_body` — 替换草稿正文

作用：整体替换当前内存工作副本的正文，可用于写作、改写和排版。`body_text`
始终是互操作和无障碍使用的权威纯文本；`body_html` 是可选的富文本表达。
这是完整替换而不是局部更新：省略 `body_html` 表示明确使用纯文本并清除工作副本
中原有的富文本；需要富文本时传字符串。`body_html: null`、字段类型错误和未声明
字段均属于参数错误。

输入示例：

```json
{
  "body_text": "您好，\n\n请确认下周三是否方便开会。",
  "body_html": "<p>您好，</p><p>请确认<strong>下周三</strong>是否方便开会。</p>"
}
```

纯文本输入示例：

```json
{
  "body_text": "您好，\n\n请确认下周三是否方便开会。"
}
```

返回示例：

```json
{
  "updated": true,
  "changed_fields": ["body_text", "body_html"]
}
```

富文本只能使用当前编辑器已经支持的段落、粗体、斜体、下划线、删除线、字体、
字号、对齐、列表、缩进和安全链接。Rust 会通过现有 compose HTML 白名单重新
清洗结果；不支持的标签、任意样式、脚本、事件属性和远程资源会被移除。

### `set_draft_stationery` — 修改草稿信纸

作用：修改信纸类型以及发送时是否携带 Mine Mail 信纸。邮件生成和自动模式注册
此工具；聊天和独立优化不注册。

输入示例：

```json
{
  "stationery": "lined",
  "send_stationery": false
}
```

`stationery` 只能是 `none`、`lined` 或 `grid`；`none` 必须同时把
`send_stationery` 归一化为 `false`。

返回示例：

```json
{
  "updated": true,
  "changed_fields": ["stationery"]
}
```

## 明确不存在的工具

以下能力不是遗漏，而是有意不向内置 AI 提供：

- 发送邮件或操作 Outbox。
- 修改或切换发信账户。
- 读取任意邮箱、任意邮件或整个联系人库。
- 修改回复或转发所绑定的引用原文。
- 读取任意本机路径。
- 首批主动添加、移除或下载附件。
- 首批解析 PDF、Word、Excel、压缩包、程序或其他二进制附件。
