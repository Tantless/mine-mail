# Mine Mail AI API 协议

本文说明 Agent 配置中的 API 协议选择、供应商预设与 Rust 适配边界。工具定义见
[`TOOLS.md`](TOOLS.md)，各 Agent 模块见 [`AGENT_MODULES.md`](AGENT_MODULES.md)。

## 三种协议

| 配置值 | 界面名称 | 请求端点 | 用途 |
| --- | --- | --- | --- |
| `openai_responses` | OpenAI Responses | `BASE_URL/responses` | Responses input item、函数调用和 SSE 事件 |
| `openai_chat_completions` | OpenAI Chat Completions | `BASE_URL/chat/completions` | Chat messages、tool calls 和 SSE chunks |
| `anthropic_messages` | Anthropic Messages | `BASE_URL/v1/messages` | Anthropic content blocks、tool use 和 SSE 事件 |

**自动**不是第四种传输协议。对话 Agent 与独立优化使用供应商推荐协议；邮件翻译会在
发起请求前比较同一供应商、同一模型下已经配置且具有新鲜能力档案的协议，只有其他
协议的已验证能力严格更好时才改走该协议。请求一旦发出，不会因为失败换协议重试。
显式选择会一直保留，直到用户再次修改。

## 供应商矩阵

| 供应商 | 推荐 | 其他可选协议 | 推荐 BASE_URL |
| --- | --- | --- | --- |
| 自定义 | OpenAI Chat Completions | Responses、Anthropic Messages | 用户填写 |
| DeepSeek | OpenAI Chat Completions | Anthropic Messages | `https://api.deepseek.com`；Anthropic 为 `https://api.deepseek.com/anthropic` |
| Kimi | OpenAI Chat Completions | — | `https://api.moonshot.cn/v1` |
| OpenAI | OpenAI Responses | Chat Completions | `https://api.openai.com/v1` |
| Anthropic | Anthropic Messages | — | `https://api.anthropic.com` |
| 通义千问 | OpenAI Responses | Chat Completions | `https://dashscope.aliyuncs.com/compatible-mode/v1` |
| Xiaomi MiMo | OpenAI Responses | Chat Completions、Anthropic Messages | 按量为 `https://api.xiaomimimo.com/v1`；Anthropic 为 `https://api.xiaomimimo.com/anthropic` |
| MiniMax | Anthropic Messages | Chat Completions | `https://api.minimaxi.com/anthropic` |
| ModelScope | OpenAI Chat Completions | — | `https://api-inference.modelscope.cn/v1` |
| 豆包 Seed | OpenAI Responses | Chat Completions | `https://ark.cn-beijing.volces.com/api/v3` |
| 智谱 GLM | OpenAI Chat Completions | Anthropic Messages | `https://open.bigmodel.cn/api/paas/v4`；Anthropic 为 `https://open.bigmodel.cn/api/anthropic` |
| OpenRouter | OpenAI Chat Completions | Responses（Beta） | `https://openrouter.ai/api/v1` |

MiMo Token Plan 用户把对应区域的地址作为 `BASE_URL`：OpenAI 协议使用地址末尾的
`/v1`，Anthropic 协议使用地址末尾的 `/anthropic`。例如中国区分别是
`https://token-plan-cn.xiaomimimo.com/v1` 和
`https://token-plan-cn.xiaomimimo.com/anthropic`。Mine Mail 不根据 Key 前缀偷偷改写用户
填写的地址或协议。

自定义渠道无法在缺少能力声明时假设任意端点实现了 Responses，因此一般仍以 Chat
Completions 作为 **自动** 的兼容默认值；但当 `BASE_URL` 是 MiMo 官方按量或 Token Plan
地址时，Mine Mail 可可靠识别供应商能力，**自动** 改为推荐并解析到 Responses。用户显式
选择的 Chat Completions 始终保持显式，不会被自动改写。

## 统一运行边界

- 写信对话 Agent、独立优化和邮件翻译都从同一份已解析配置创建 Rust Provider；前端
  不能为某一功能覆盖协议或直接发网络请求。
- 对话 Agent 使用流式适配；独立优化保持非流式；翻译也统一从 Responses SSE、Chat
  Completions SSE 或 Anthropic Messages SSE 适配器读取，再按自身批次、JSON 校验与
  超时规则运行。协议只改变线上请求与响应的编码方式，不改变工具权限和应用规则。
- OpenAI Responses 使用 `store: false` 并手动带回消息、函数调用、工具结果和必要的
  reasoning item。官方 OpenAI Responses 在会话达到已解析上下文窗口的 75% 时，优先
  调用无状态 `/responses/compact`；其返回的完整 canonical output（包括 opaque encrypted
  compaction item）原样保存并用于后续请求，仍保持 `store: false`。兼容 Responses 端点
  不因协议名称被假定支持该能力。Chat Completions 带回供应商要求的思考状态。Anthropic Messages 在
  content blocks 中关联 `tool_use` 与 `tool_result`。
- 所有独立优化请求都由 Rust 在首个 Provider 请求前通过受限工具读取正文和主题，并编码
  为对应协议的标准工具历史；模型无需自行选择这两个必读工具，写入边界仍会验证读取结果。
- Chat Completions 请求携带工具定义时不同时发送 `response_format`，避免兼容实现被 JSON
  Mode 诱导为提前返回简短终态而跳过写入工具；不携带工具的结构化任务仍可使用 JSON
  Mode。独立优化终态由提示词约束并由 Rust 做有界严格校验，首次格式错误只在原协议内
  进行一次格式纠正，不重放已经成功的工具。响应只要包含非空 `tool_calls` 就按工具轮
  处理，即使兼容网关错误返回 `finish_reason: stop`。
- MiMo-compatible Chat Completions 的独立非流式优化请求关闭 thinking，并关闭并行工具
  调用。响应保留首轮 `tool_calls`；`finish_reason` 将 repetition truncation、缺失、`null`、
  非字符串和未知值分别归类到隐私安全日志，只有明确的 stop 才能结束无工具轮次，重复
  截断不能伪装成正常完成。MiMo 当前只实际支持自动 `tool_choice`，不能用 `required` 或
  指定函数假装获得供应商没有实现的强制选择能力。
  优化终态还必须声明 `changed` 或 `unchanged`。明确用户要求却没有形成实际修改时，
  Rust 进行一次定向纠正请求；仍未修改则失败，不能把自动工具选择的短路输出展示成
  “无需改进”。没有额外要求时，第一次 `unchanged` 还要经过一次独立复核才可完成。
- API Key 仍按供应商存入系统凭据库；地址、模型、环境变量选择与模型检索结果按
  “供应商 + 协议”分别保存。切换协议不会清除其他组合。
- 模型检索、连接测试、自动保存和手动保存都只针对当前可见协议。模型列表接口不可用
  时，用户仍可手动填写模型名称。连接测试成功后还会尽力探测该模型是否接受协议原生
  的严格 JSON Schema；探测失败不推翻连接测试结果。
- 模型检索除 `id` 外会识别 `context_window`、`context_length`、
  `max_context_length`、`input_token_limit` 等常见数值字段及受支持的嵌套等价字段；只有
  1,024 到 2,000,000 之间的结构化正整数才可成为三级置信度窗口。缺少该字段时按
  “API 返回值（三级） > 官方资料或自定义手选（二级） > 128K 默认（一级）”解析，不能
  通过最小请求成功或模型列表中存在一个 ID 猜测窗口。
- Chat Completions、Anthropic Messages、不支持原生压缩的 Responses 兼容端点，以及
  官方 compact 调用失败时，使用同一模型生成九段式本地摘要。压缩只覆盖较早完整轮次，
  最近两轮保持原文；完整可见 Session 继续留在本地 SQLite。摘要与 Responses opaque
  state 都严格绑定 Provider 实例、协议、BASE_URL、模型和 Session，不能跨路由复用。
- 能力档案按“供应商 + 协议 + BASE_URL + 模型”保存七天，来源可以是内置预设、连接
  测试探测或真实请求观察，当前记录结构化输出、流式响应与思考控制三类能力。模型列表
  只能声明可用模型，不能替代能力探测。严格 JSON Schema 被兼容端点以 400、404、405、
  415 或 422 拒绝时，同一次翻译只在原协议内降级为 JSON object 或提示词 JSON，不会
  切换端点或协议。
- 翻译执行器把长文本切成不超过 800 UTF-8 字节的语义片段，以四路并发起步并根据批次
  成败在一路到六路之间调整；任何一路完成后立即补入下一批。缺失且可重试的编号只用
  更小批次补一次，仍缺失的位置保留原文。
- 诊断日志记录供应商、协议、模型、阶段、耗时、大小与结果类别，不记录 API Key、
  邮件地址、主题、正文、工具参数或完整路径。

## 预设依据

预设只声明官方已公开且 Mine Mail 已实现的接口，不代表同一供应商的所有模型都支持
每个协议。主要官方资料：

- [OpenAI 模型与 Responses API](https://developers.openai.com/api/docs/models)
- [OpenAI Responses 上下文压缩](https://developers.openai.com/api/docs/guides/compaction)
- [DeepSeek Anthropic API](https://api-docs.deepseek.com/guides/anthropic_api)
- [通义千问 OpenAI Responses](https://help.aliyun.com/zh/model-studio/qwen-api-via-openai-responses)
- [Xiaomi MiMo 工具接入概览](https://mimo.mi.com/docs/integration/tools-overview)
- [Xiaomi MiMo 模型与上下文窗口](https://mimo.mi.com/docs/quick-start/summary/model)
- [MiniMax 文本生成与推荐协议](https://platform.minimaxi.com/docs/guides/text-generation)
- [豆包 Seed Responses 工具调用](https://www.volcengine.com/docs/82379/1958524)
- [智谱 GLM Claude Code 接入](https://docs.bigmodel.cn/cn/guide/develop/claude)
- [OpenRouter Responses API](https://openrouter.ai/docs/api/reference/responses/overview)

供应商文档、模型能力或兼容层发生变化时，应先更新适配和测试，再更新此矩阵；不能只
改下拉选项而让未实现的协议进入生产路径。
