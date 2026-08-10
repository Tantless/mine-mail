# Mine Mail AI API 协议

本文说明 Agent 配置中的 API 协议选择、供应商预设与 Rust 适配边界。工具定义见
[`TOOLS.md`](TOOLS.md)，各 Agent 模块见 [`AGENT_MODULES.md`](AGENT_MODULES.md)。

## 三种协议

| 配置值 | 界面名称 | 请求端点 | 用途 |
| --- | --- | --- | --- |
| `openai_responses` | OpenAI Responses | `BASE_URL/responses` | Responses input item、函数调用和 SSE 事件 |
| `openai_chat_completions` | OpenAI Chat Completions | `BASE_URL/chat/completions` | Chat messages、tool calls 和 SSE chunks |
| `anthropic_messages` | Anthropic Messages | `BASE_URL/v1/messages` | Anthropic content blocks、tool use 和 SSE 事件 |

**自动**不是第四种传输协议。它在每次保存配置时解析为供应商的推荐协议；请求过程中
不会因为失败而换用其他协议。显式选择会一直保留，直到用户再次修改。

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

## 统一运行边界

- 写信对话 Agent、独立优化和邮件翻译都从同一份已解析配置创建 Rust Provider；前端
  不能为某一功能覆盖协议或直接发网络请求。
- 对话 Agent 使用流式适配；独立优化保持非流式；翻译按自身批次、JSON 校验与超时
  规则运行。协议只改变线上请求与响应的编码方式，不改变工具权限和应用规则。
- OpenAI Responses 使用 `store: false` 并手动带回消息、函数调用、工具结果和必要的
  reasoning item。Chat Completions 带回供应商要求的思考状态。Anthropic Messages 在
  content blocks 中关联 `tool_use` 与 `tool_result`。
- API Key 仍按供应商存入系统凭据库；地址、模型、环境变量选择与模型检索结果按
  “供应商 + 协议”分别保存。切换协议不会清除其他组合。
- 模型检索、连接测试、自动保存和手动保存都只针对当前可见协议。模型列表接口不可用
  时，用户仍可手动填写模型名称。
- 诊断日志记录供应商、协议、模型、阶段、耗时、大小与结果类别，不记录 API Key、
  邮件地址、主题、正文、工具参数或完整路径。

## 预设依据

预设只声明官方已公开且 Mine Mail 已实现的接口，不代表同一供应商的所有模型都支持
每个协议。主要官方资料：

- [OpenAI 模型与 Responses API](https://developers.openai.com/api/docs/models)
- [DeepSeek Anthropic API](https://api-docs.deepseek.com/guides/anthropic_api)
- [通义千问 OpenAI Responses](https://help.aliyun.com/zh/model-studio/qwen-api-via-openai-responses)
- [Xiaomi MiMo 工具接入概览](https://mimo.mi.com/docs/integration/tools-overview)
- [MiniMax 文本生成与推荐协议](https://platform.minimaxi.com/docs/guides/text-generation)
- [豆包 Seed Responses 工具调用](https://www.volcengine.com/docs/82379/1958524)
- [智谱 GLM Claude Code 接入](https://docs.bigmodel.cn/cn/guide/develop/claude)
- [OpenRouter Responses API](https://openrouter.ai/docs/api/reference/responses/overview)

供应商文档、模型能力或兼容层发生变化时，应先更新适配和测试，再更新此矩阵；不能只
改下拉选项而让未实现的协议进入生产路径。
