# Mine Mail Documentation

The repository keeps a small set of canonical documents:

- [`../AGENTS.md`](../AGENTS.md) — mandatory coding-agent contract, architecture
  and safety invariants, and verification routing.
- [`../DESIGN.md`](../DESIGN.md) — the only visual and interaction specification.
- [`PRODUCT.md`](PRODUCT.md) — durable user-visible behavior.
- [`MAIL_RENDERING.md`](MAIL_RENDERING.md) — mail HTML, MIME, remote content, and
  reply-history safety boundary.
- [`MCP.md`](MCP.md) — local MCP permissions, tools, and supported-agent setup.
- [`toolCalling/TOOLS.md`](toolCalling/TOOLS.md) — 内置 AI 工具名、中文说明、
  权限边界和调用示例。
- [`toolCalling/AGENT_MODULES.md`](toolCalling/AGENT_MODULES.md) — 写信优化、
  邮件生成、聊天和自动模式的工具权限与运行约束。
- [`RELEASE.md`](RELEASE.md) — mutable checklist for the next public release.
- [`../README.md`](../README.md) — human project overview and development setup.

Keep implementation detail beside the code and enforce it with tests. Update a
canonical document only for durable decisions or current operator instructions.
Do not add dated QA journals, chat transcripts, copy inventories, generated
comparison images, absolute local paths, or multiple “final” design documents.
Temporary investigation material belongs in the OS temporary directory.
