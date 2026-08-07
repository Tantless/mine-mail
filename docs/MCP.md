# Mine Mail MCP

Mine Mail exposes a local Streamable HTTP MCP server so an AI agent on the same
computer can work with the user's configured mail accounts. The endpoint is:

```text
http://127.0.0.1:46321/mcp
```

This document is both the user setup guide and the instruction source for an
agent asked to “帮我接上 Mine Mail 的 MCP”. It currently supports Codex,
ChatGPT Desktop, Claude Code, OpenClaw, and Hermes only.

## Before configuration

1. Start the full Mine Mail desktop application. The React-only demo does not
   host MCP.
2. Open **设置 → 功能设定 → 开启 MCP**, read the single confirmation, and
   choose the permissions needed by the agent.
3. Keep Mine Mail open or in the system tray. Fully exiting it stops MCP.

The server listens only on this computer and has no connection key. Enabling it
therefore authorizes other local processes to use the selected capabilities.
Mine Mail rejects browser-originated requests, bounds requests and concurrent
tool calls, and never exposes credentials or raw RFC822 messages. Turn the
parent switch off when local-agent access is no longer wanted.

## Agent configuration contract

When an agent configures Mine Mail on behalf of a user, it must:

1. Read this file before editing any client configuration.
2. Detect the current client and operating system; do not configure unsupported
   clients.
3. Inspect the existing entry named `mine-mail` first. Preserve every unrelated
   server and setting, and do not create a duplicate.
4. Add the endpoint exactly as shown above, or replace only a stale `mine-mail`
   endpoint after explaining the change.
5. Verify connection and tool discovery only. Setup must not call a mail-reading,
   mutation, draft, attachment, or sending tool.
6. If discovery fails, check that Mine Mail is running, **开启 MCP** remains on,
   and local port `46321` is not occupied. Do not expose logs containing mail or
   complete local paths.

On Windows, `~` below means `%USERPROFILE%`; on macOS and Linux it means the
current user's home directory. Restart the client after changing a file-based
configuration if it does not reload automatically.

## Codex and ChatGPT Desktop

Codex CLI and the Codex surface in ChatGPT Desktop use the same local MCP
configuration. Prefer the CLI because it merges one named entry safely:

```bash
codex mcp get mine-mail
codex mcp add mine-mail --url http://127.0.0.1:46321/mcp
codex mcp list
```

Run `add` only when `get` confirms that the entry is absent. If the named entry
exists with an old URL, remove and recreate only `mine-mail`; never overwrite
the rest of `~/.codex/config.toml`. Reopen ChatGPT Desktop after adding the
entry, then confirm that the Mine Mail tools appear.

Official reference: [OpenAI MCP documentation](https://learn.chatgpt.com/docs/extend/mcp).

## Claude Code

Use a user-scoped HTTP server so the connection is available in future projects:

```bash
claude mcp get mine-mail
claude mcp add --transport http --scope user mine-mail http://127.0.0.1:46321/mcp
claude mcp list
```

Run `add` only when the named entry is absent. If an old `mine-mail` entry must
be replaced, change only that entry. Reopen Claude Code if tool discovery does
not refresh.

Official reference: [Claude Code MCP documentation](https://code.claude.com/docs/en/mcp).

## OpenClaw

Inspect the named connection, add it once, then probe it without invoking a mail
tool:

```bash
openclaw mcp get mine-mail
openclaw mcp add mine-mail --url http://127.0.0.1:46321/mcp --transport streamable-http
openclaw mcp doctor mine-mail --probe
```

Keep all unrelated OpenClaw MCP entries unchanged. A successful probe is enough
for setup verification.

Official reference: [OpenClaw MCP CLI documentation](https://docs.openclaw.ai/cli/mcp).

## Hermes

Merge the following named entry into `~/.hermes/config.yaml`. Do not replace an
existing `mcp_servers` mapping:

```yaml
mcp_servers:
  mine-mail:
    url: "http://127.0.0.1:46321/mcp"
```

Use `/reload-mcp` or restart Hermes, then inspect its MCP/tool list. Discovery is
the end of setup; do not test by reading or sending mail.

Official reference: [Hermes MCP documentation](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/mcp.md).

## Permissions and tools

The parent **开启 MCP** switch controls whether the server runs. Child choices
remain saved while the parent is off. The first enabled state defaults to
**获取信息** on and **发送邮件** off.

### 获取信息

- `list_accounts` — list safe account identities and stable account IDs.
- `sync_mail` — synchronize recent Inbox, Sent, and Drafts data for one account.
- `search_messages` — search synchronized metadata and complete bodies already
  present in the bounded local body cache.
- `index_message_bodies` — fetch/cache synchronized message bodies in bounded
  batches when broader full-text coverage is needed.
- `get_message` — fetch one message and return bounded plain text plus attachment
  metadata; HTML and raw RFC822 are not returned.
- `download_attachment` — save one received attachment to an absolute path chosen
  by the agent. Existing files are not overwritten.
- `set_message_read`, `set_message_starred` — queue read/star state changes.
- `archive_message`, `move_message_to_inbox`, `move_message_to_trash` — queue
  reversible mailbox organization. Permanent deletion is not exposed.

### 发送邮件

- `list_drafts`, `get_draft` — inspect editable drafts and their exact local
  versions.
- `create_draft`, `update_draft`, `delete_draft` — manage versioned plain-text
  drafts. Stale writes preserve work as conflict copies; stale deletion cannot
  remove a newer version.
- `add_draft_attachments`, `remove_draft_attachment` — import absolute local file
  paths into managed draft storage or remove an attachment from an exact version.
- `create_reply_draft`, `create_forward_draft` — create editable drafts from a
  hydrated source message.
- `send_draft` — send one exact reviewed draft version to an exact confirmed
  recipient list. Unknown SMTP delivery is never retried automatically, and no
  unknown-delivery retry tool is exposed.

Every mail tool uses an explicit stable `account_id`; it never depends on the
account currently selected in the Mine Mail interface. File reading after a
download, and access to paths supplied for outgoing attachments, remain subject
to the agent client's own local-file permissions.
