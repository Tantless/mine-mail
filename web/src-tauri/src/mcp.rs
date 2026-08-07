use std::{path::PathBuf, sync::Arc, time::Duration};

use axum::Router;
use mine_mail::{
    AttachmentDisposition, ComposeRequest, DraftDto as CoreDraftDto, ForwardPreparationOutcome,
    InboxMessage, MailBackend, MailboxRole, MessagePageItem,
};
use rmcp::{
    Json, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::{
    account::{AccountRuntime, BackendState},
    desktop::{DesktopRuntime, DraftsUpdatedEvent},
};

const MCP_BIND_ADDRESS: &str = "127.0.0.1:46321";
const MCP_MAX_REQUEST_BYTES: usize = 256 * 1024;
const MCP_MAX_CONCURRENT_TOOLS: usize = 8;
const MCP_MAX_TOOL_CALLS_PER_SECOND: usize = 30;
const MAX_SEARCH_RESULTS: usize = 100;
const MAX_INDEX_BATCH: usize = 100;
const MAX_BODY_OUTPUT_CHARS: usize = 512 * 1024;

pub(crate) struct McpRuntime {
    server: Mutex<Option<McpServerHandle>>,
    limiter: Arc<Semaphore>,
    rate: Arc<Mutex<ToolRateWindow>>,
}

struct McpServerHandle {
    cancellation: CancellationToken,
    task: tauri::async_runtime::JoinHandle<()>,
}

async fn stop_server(mut server: McpServerHandle) {
    server.cancellation.cancel();
    if tokio::time::timeout(Duration::from_secs(2), &mut server.task)
        .await
        .is_err()
    {
        server.task.abort();
    }
}

struct ToolRateWindow {
    started_at: tokio::time::Instant,
    calls: usize,
}

impl Default for McpRuntime {
    fn default() -> Self {
        Self {
            server: Mutex::new(None),
            limiter: Arc::new(Semaphore::new(MCP_MAX_CONCURRENT_TOOLS)),
            rate: Arc::new(Mutex::new(ToolRateWindow {
                started_at: tokio::time::Instant::now(),
                calls: 0,
            })),
        }
    }
}

impl McpRuntime {
    pub(crate) async fn set_enabled(&self, app: AppHandle, enabled: bool) -> Result<(), String> {
        let mut server = self.server.lock().await;
        if enabled {
            if server
                .as_ref()
                .is_some_and(|server| !server.task.inner().is_finished())
            {
                return Ok(());
            }
            if let Some(previous) = server.take() {
                stop_server(previous).await;
            }
            let listener = tokio::net::TcpListener::bind(MCP_BIND_ADDRESS)
                .await
                .map_err(|_| "MCP 本地端口不可用，请关闭占用端口的程序后重试。".to_owned())?;
            let token = CancellationToken::new();
            let service_token = token.child_token();
            let app_for_factory = app.clone();
            let limiter = self.limiter.clone();
            let rate = self.rate.clone();
            let service: StreamableHttpService<MineMailMcp, LocalSessionManager> =
                StreamableHttpService::new(
                    move || {
                        Ok(MineMailMcp::new(
                            app_for_factory.clone(),
                            limiter.clone(),
                            rate.clone(),
                        ))
                    },
                    Default::default(),
                    StreamableHttpServerConfig::default()
                        .with_sse_keep_alive(None)
                        .with_allowed_hosts([
                            "127.0.0.1",
                            MCP_BIND_ADDRESS,
                            "localhost",
                            "localhost:46321",
                        ])
                        // Agent clients normally omit Origin. Any browser-originated
                        // request is rejected to keep local web pages away from mail.
                        .with_allowed_origins(["http://mine-mail.invalid"])
                        .with_max_request_body_bytes(MCP_MAX_REQUEST_BYTES)
                        .with_cancellation_token(service_token),
                );
            let router = Router::new().nest_service("/mcp", service);
            let shutdown = token.clone();
            let task = tauri::async_runtime::spawn(async move {
                let result = axum::serve(listener, router)
                    .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
                    .await;
                if result.is_err() {
                    crate::diagnostics::limited_failure(
                        "mcp_server_stopped",
                        "local_mcp",
                        None,
                        crate::diagnostics::ErrorKind::Runtime,
                    );
                }
            });
            *server = Some(McpServerHandle {
                cancellation: token,
                task,
            });
        } else if let Some(previous) = server.take() {
            stop_server(previous).await;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct MineMailMcp {
    app: AppHandle,
    limiter: Arc<Semaphore>,
    rate: Arc<Mutex<ToolRateWindow>>,
    tool_router: ToolRouter<Self>,
}

impl MineMailMcp {
    fn new(app: AppHandle, limiter: Arc<Semaphore>, rate: Arc<Mutex<ToolRateWindow>>) -> Self {
        Self {
            app,
            limiter,
            rate,
            tool_router: Self::tool_router(),
        }
    }

    fn require_information(&self) -> Result<(), String> {
        let access = self.app.state::<DesktopRuntime>().mcp_access()?;
        if access.enabled && access.information {
            Ok(())
        } else {
            Err("Mine Mail 未开启 MCP 的“获取信息”权限。".to_owned())
        }
    }

    fn require_send(&self) -> Result<(), String> {
        let access = self.app.state::<DesktopRuntime>().mcp_access()?;
        if access.enabled && access.send {
            Ok(())
        } else {
            Err("Mine Mail 未开启 MCP 的“发送邮件”权限。".to_owned())
        }
    }

    fn local_backend(&self, account_id: &str) -> Result<Arc<MailBackend>, String> {
        self.app.state::<BackendState>().local_for(account_id)
    }

    async fn network_backend(&self, account_id: &str) -> Result<Arc<MailBackend>, String> {
        let accounts = self.app.state::<AccountRuntime>();
        let backends = self.app.state::<BackendState>();
        accounts
            .refresh_oauth_backend_for(&backends, account_id)
            .await?;
        backends.network_for(account_id)
    }

    async fn enter(&self) -> Result<tokio::sync::OwnedSemaphorePermit, String> {
        let permit = self
            .limiter
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "MCP 服务正在停止，请稍后重试。".to_owned())?;
        let now = tokio::time::Instant::now();
        let mut rate = self.rate.lock().await;
        if now.duration_since(rate.started_at) >= std::time::Duration::from_secs(1) {
            rate.started_at = now;
            rate.calls = 0;
        }
        if rate.calls >= MCP_MAX_TOOL_CALLS_PER_SECOND {
            return Err("MCP 请求过于频繁，请稍后重试。".to_owned());
        }
        rate.calls += 1;
        Ok(permit)
    }

    fn emit_drafts_saved(&self) {
        let _ = self
            .app
            .emit("mail:drafts-updated", DraftsUpdatedEvent::saved());
    }

    fn emit_drafts_deleted(&self) {
        let _ = self
            .app
            .emit("mail:drafts-updated", DraftsUpdatedEvent::deleted());
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SyncMailRequest {
    /// Stable account identifier returned by list_accounts.
    account_id: String,
    /// Maximum recent messages fetched for Inbox and Sent. Range: 1-100.
    #[serde(default = "default_sync_limit")]
    limit: usize,
}

fn default_sync_limit() -> usize {
    50
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchMessagesRequest {
    /// Text matched against sender, recipients, subject, preview, and cached full body text.
    query: String,
    /// Account identifiers returned by list_accounts. Empty means all accounts.
    #[serde(default)]
    account_ids: Vec<String>,
    /// Mailbox roles: inbox, sent, archive, trash. Empty means all four.
    #[serde(default)]
    roles: Vec<String>,
    /// Maximum combined results. Range: 1-100.
    #[serde(default = "default_search_limit")]
    limit: usize,
}

fn default_search_limit() -> usize {
    25
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct IndexBodiesRequest {
    account_id: String,
    /// One of inbox, sent, archive, trash.
    role: String,
    /// Opaque continuation token returned by the previous call.
    #[serde(default)]
    cursor: Option<String>,
    /// Maximum bodies to inspect/fetch in this batch. Range: 1-100.
    #[serde(default = "default_index_limit")]
    limit: usize,
}

fn default_index_limit() -> usize {
    25
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MessageRequest {
    account_id: String,
    /// Opaque message identifier returned by search_messages.
    message_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetMessageStateRequest {
    account_id: String,
    message_id: String,
    desired: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DownloadAttachmentRequest {
    account_id: String,
    message_id: String,
    attachment_id: String,
    /// Absolute destination file path. Existing files are never overwritten.
    destination_path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DraftRequest {
    account_id: String,
    draft_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateDraftRequest {
    account_id: String,
    #[serde(default)]
    to: Vec<String>,
    #[serde(default)]
    cc: Vec<String>,
    #[serde(default)]
    bcc: Vec<String>,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    body_text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UpdateDraftRequest {
    account_id: String,
    draft_id: String,
    /// Exact version returned when the draft was last read.
    expected_local_version: u64,
    #[serde(default)]
    to: Option<Vec<String>>,
    #[serde(default)]
    cc: Option<Vec<String>>,
    #[serde(default)]
    bcc: Option<Vec<String>>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    body_text: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DeleteDraftRequest {
    account_id: String,
    draft_id: String,
    expected_local_version: u64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddDraftAttachmentRequest {
    account_id: String,
    draft_id: String,
    expected_local_version: u64,
    /// Absolute local file paths to import into the draft.
    file_paths: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RemoveDraftAttachmentRequest {
    account_id: String,
    draft_id: String,
    expected_local_version: u64,
    attachment_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ForwardDraftRequest {
    account_id: String,
    message_id: String,
    #[serde(default)]
    include_attachments: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SendDraftRequest {
    account_id: String,
    draft_id: String,
    expected_local_version: u64,
    /// Exact To/Cc/Bcc addresses from the reviewed draft.
    confirmed_recipients: Vec<String>,
}

#[tool_router]
impl MineMailMcp {
    #[tool(description = "List connected Mine Mail accounts and their stable account IDs")]
    async fn list_accounts(&self) -> Result<Json<Value>, String> {
        self.require_information()?;
        let _permit = self.enter().await?;
        let accounts = self.app.state::<AccountRuntime>();
        let backends = self.app.state::<BackendState>();
        let active = backends.active_account_id();
        let values = accounts
            .account_ids()
            .into_iter()
            .map(|account_id| {
                let (email, remark) = accounts
                    .account_email_and_remark(&account_id)
                    .unwrap_or_default();
                json!({
                    "account_id": account_id,
                    "email": email,
                    "remark": remark,
                    "active_in_app": active.as_deref() == Some(account_id.as_str()),
                    "network_available": backends.network_ready_for(&account_id),
                })
            })
            .collect::<Vec<_>>();
        Ok(Json(json!({ "accounts": values })))
    }

    #[tool(description = "Synchronize recent Inbox, Sent, and Drafts data for one account")]
    async fn sync_mail(
        &self,
        Parameters(request): Parameters<SyncMailRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_information()?;
        let _permit = self.enter().await?;
        let limit = bounded_limit(request.limit, 100, "limit")?;
        let backend = self.network_backend(&request.account_id).await?;
        let inbox = backend
            .sync_inbox(limit)
            .await
            .map_err(crate::safe_mail_error)?;
        let sent = backend
            .sync_sent(limit)
            .await
            .map_err(crate::safe_mail_error)?;
        let drafts = backend
            .sync_drafts(None)
            .await
            .map_err(crate::safe_mail_error)?;
        Ok(Json(
            json!({ "inbox": inbox, "sent": sent, "drafts": drafts }),
        ))
    }

    #[tool(description = "Search message metadata and cached full body text across accounts")]
    async fn search_messages(
        &self,
        Parameters(request): Parameters<SearchMessagesRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_information()?;
        let _permit = self.enter().await?;
        let query = request.query.trim();
        if query.is_empty() {
            return Err("query 不能为空。".to_owned());
        }
        let limit = bounded_limit(request.limit, MAX_SEARCH_RESULTS, "limit")?;
        let account_ids = if request.account_ids.is_empty() {
            self.app.state::<AccountRuntime>().account_ids()
        } else {
            request.account_ids
        };
        let roles = parse_roles(&request.roles)?;
        let mut matches = Vec::new();
        let mut cached_body_count = 0usize;
        for account_id in account_ids {
            let backend = self.local_backend(&account_id)?;
            for role in &roles {
                let page = backend
                    .list_mailbox_page(&account_id, *role, None, limit, Some(query))
                    .map_err(crate::safe_mail_error)?;
                for item in page.items {
                    if item.message.body_fetched {
                        cached_body_count += 1;
                    }
                    let sort_at = message_sort_at(&item.message).to_owned();
                    matches.push((sort_at, search_item_value(&account_id, item, query)));
                }
            }
        }
        matches.sort_by(|left, right| right.0.cmp(&left.0));
        matches.truncate(limit);
        Ok(Json(json!({
            "messages": matches.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "full_text_scope": "cached_complete_bodies",
            "matched_rows_with_cached_body": cached_body_count,
            "hint": "Call index_message_bodies in batches when complete local body coverage is needed."
        })))
    }

    #[tool(
        description = "Fetch and cache message bodies in bounded batches before full-text search"
    )]
    async fn index_message_bodies(
        &self,
        Parameters(request): Parameters<IndexBodiesRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_information()?;
        let _permit = self.enter().await?;
        let role = parse_role(&request.role)?;
        let limit = bounded_limit(request.limit, MAX_INDEX_BATCH, "limit")?;
        let cursor = request
            .cursor
            .map(|value| {
                serde_json::from_value::<mine_mail::MessagePageCursor>(Value::String(value))
            })
            .transpose()
            .map_err(|_| "cursor 无效，请使用上一批返回的原值。".to_owned())?;
        let local = self.local_backend(&request.account_id)?;
        let page = local
            .list_mailbox_page(&request.account_id, role, cursor.as_ref(), limit, None)
            .map_err(crate::safe_mail_error)?;
        let mut fetched = 0usize;
        let mut already_cached = 0usize;
        let network = if page.items.iter().any(|item| !item.message.body_fetched) {
            Some(self.network_backend(&request.account_id).await?)
        } else {
            None
        };
        for item in &page.items {
            if item.message.body_fetched {
                already_cached += 1;
            } else if let Some(network) = network.as_ref() {
                network
                    .fetch_message_by_id(&item.public_id, false)
                    .await
                    .map_err(crate::safe_mail_error)?;
                fetched += 1;
            }
        }
        let has_more_local = page.has_more_local;
        Ok(Json(json!({
            "fetched": fetched,
            "already_cached": already_cached,
            "next_cursor": if has_more_local { page.next_cursor } else { None },
            "complete": !has_more_local,
        })))
    }

    #[tool(description = "Get one message with plain-text body and attachment metadata")]
    async fn get_message(
        &self,
        Parameters(request): Parameters<MessageRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_information()?;
        let _permit = self.enter().await?;
        let local = self.local_backend(&request.account_id)?;
        let cached = local
            .cached_message_by_id(&request.message_id)
            .map_err(crate::safe_mail_error)?;
        let message = if cached.body_fetched {
            cached
        } else {
            self.network_backend(&request.account_id)
                .await?
                .fetch_message_by_id(&request.message_id, false)
                .await
                .map_err(crate::safe_mail_error)?
        };
        let attachments = if message.body_fetched {
            local
                .cached_message_attachments(&request.message_id)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Json(message_value(
            &request.account_id,
            &request.message_id,
            message,
            attachments,
        )))
    }

    #[tool(description = "Download one received attachment to an absolute local file path")]
    async fn download_attachment(
        &self,
        Parameters(request): Parameters<DownloadAttachmentRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_information()?;
        let _permit = self.enter().await?;
        let destination = PathBuf::from(&request.destination_path);
        if !destination.is_absolute() {
            return Err("destination_path 必须是绝对路径。".to_owned());
        }
        let local = self.local_backend(&request.account_id)?;
        let cached_locally = local
            .cached_message_by_id(&request.message_id)
            .map(|message| !message.raw_rfc822.is_empty())
            .unwrap_or(false);
        let backend = if cached_locally {
            local
        } else {
            self.network_backend(&request.account_id).await?
        };
        let outcome = backend
            .save_message_attachment_to(
                &request.message_id,
                &request.attachment_id,
                Some(&destination),
            )
            .await
            .map_err(crate::safe_mail_error)?;
        let saved_path = outcome.file_name.as_ref().and_then(|name| {
            destination
                .parent()
                .map(|directory| directory.join(name).to_string_lossy().into_owned())
        });
        Ok(Json(json!({
            "status": outcome.status,
            "saved_path": saved_path,
            "error_kind": outcome.error_kind,
            "retryable": outcome.retryable,
        })))
    }

    #[tool(description = "Mark one message read or unread using its stable message ID")]
    async fn set_message_read(
        &self,
        Parameters(request): Parameters<SetMessageStateRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_information()?;
        let _permit = self.enter().await?;
        let result = self
            .local_backend(&request.account_id)?
            .set_message_seen(&request.message_id, request.desired)
            .map_err(crate::safe_mail_error)?;
        Ok(Json(
            serde_json::to_value(result).unwrap_or_else(|_| json!({})),
        ))
    }

    #[tool(description = "Star or unstar one message using its stable message ID")]
    async fn set_message_starred(
        &self,
        Parameters(request): Parameters<SetMessageStateRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_information()?;
        let _permit = self.enter().await?;
        let result = self
            .local_backend(&request.account_id)?
            .set_message_starred_by_id(&request.message_id, request.desired)
            .map_err(crate::safe_mail_error)?;
        Ok(Json(
            serde_json::to_value(result).unwrap_or_else(|_| json!({})),
        ))
    }

    #[tool(description = "Archive one message without permanently deleting it")]
    async fn archive_message(
        &self,
        Parameters(request): Parameters<MessageRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_information()?;
        let _permit = self.enter().await?;
        let result = self
            .local_backend(&request.account_id)?
            .archive_message(&request.message_id)
            .map_err(crate::safe_mail_error)?;
        Ok(Json(
            serde_json::to_value(result).unwrap_or_else(|_| json!({})),
        ))
    }

    #[tool(description = "Move one message to Inbox")]
    async fn move_message_to_inbox(
        &self,
        Parameters(request): Parameters<MessageRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_information()?;
        let _permit = self.enter().await?;
        let result = self
            .local_backend(&request.account_id)?
            .move_message_to_inbox(&request.message_id)
            .map_err(crate::safe_mail_error)?;
        Ok(Json(
            serde_json::to_value(result).unwrap_or_else(|_| json!({})),
        ))
    }

    #[tool(description = "Move one message to Trash; permanent deletion is not exposed")]
    async fn move_message_to_trash(
        &self,
        Parameters(request): Parameters<MessageRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_information()?;
        let _permit = self.enter().await?;
        let result = self
            .local_backend(&request.account_id)?
            .move_message_to_trash(&request.message_id)
            .map_err(crate::safe_mail_error)?;
        Ok(Json(
            serde_json::to_value(result).unwrap_or_else(|_| json!({})),
        ))
    }

    #[tool(description = "List editable drafts for one account")]
    async fn list_drafts(
        &self,
        Parameters(request): Parameters<AccountOnlyRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_send()?;
        let _permit = self.enter().await?;
        let backend = self.local_backend(&request.account_id)?;
        let values = backend
            .list_drafts()
            .map_err(crate::safe_mail_error)?
            .into_iter()
            .map(|draft| {
                backend
                    .draft_dto(&draft.id)
                    .map(draft_value)
                    .map_err(crate::safe_mail_error)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Json(json!({ "drafts": values })))
    }

    #[tool(description = "Get one editable draft and its exact local version")]
    async fn get_draft(
        &self,
        Parameters(request): Parameters<DraftRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_send()?;
        let _permit = self.enter().await?;
        let draft = self
            .local_backend(&request.account_id)?
            .draft_dto(&request.draft_id)
            .map_err(crate::safe_mail_error)?;
        Ok(Json(draft_value(draft)))
    }

    #[tool(description = "Create a plain-text draft; this does not send mail")]
    async fn create_draft(
        &self,
        Parameters(request): Parameters<CreateDraftRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_send()?;
        let _permit = self.enter().await?;
        let backend = self.local_backend(&request.account_id)?;
        let draft = backend
            .save_draft(compose_request(
                request.to,
                request.cc,
                request.bcc,
                request.subject,
                request.body_text,
            ))
            .map_err(crate::safe_mail_error)?;
        self.emit_drafts_saved();
        Ok(Json(draft_value(
            backend
                .draft_dto(&draft.id)
                .map_err(crate::safe_mail_error)?,
        )))
    }

    #[tool(description = "Update an exact draft version; stale edits create a safe conflict copy")]
    async fn update_draft(
        &self,
        Parameters(request): Parameters<UpdateDraftRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_send()?;
        let _permit = self.enter().await?;
        let backend = self.local_backend(&request.account_id)?;
        let current = backend
            .draft_dto(&request.draft_id)
            .map_err(crate::safe_mail_error)?;
        let mut compose = current.draft.compose_request();
        if let Some(value) = request.to {
            compose.to = value;
        }
        if let Some(value) = request.cc {
            compose.cc = value;
        }
        if let Some(value) = request.bcc {
            compose.bcc = value;
        }
        if let Some(value) = request.subject {
            compose.subject = value;
        }
        if let Some(value) = request.body_text {
            compose.body_text = value;
            compose.format = Default::default();
        }
        let outcome = backend
            .save_draft_optimistic(
                Some(&request.draft_id),
                Some(request.expected_local_version),
                compose,
            )
            .map_err(crate::safe_mail_error)?;
        let value = json!({
            "kind": outcome.kind,
            "draft": draft_value(backend.draft_dto(&outcome.draft.id).map_err(crate::safe_mail_error)?),
            "canonical": match outcome.canonical {
                Some(draft) => Some(draft_value(backend.draft_dto(&draft.id).map_err(crate::safe_mail_error)?)),
                None => None,
            },
        });
        self.emit_drafts_saved();
        Ok(Json(value))
    }

    #[tool(
        description = "Delete only the exact draft version; stale deletion cannot remove newer work"
    )]
    async fn delete_draft(
        &self,
        Parameters(request): Parameters<DeleteDraftRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_send()?;
        let _permit = self.enter().await?;
        let kind = self
            .local_backend(&request.account_id)?
            .delete_draft_optimistic(&request.draft_id, request.expected_local_version)
            .map_err(crate::safe_mail_error)?;
        self.emit_drafts_deleted();
        Ok(Json(json!({ "kind": kind })))
    }

    #[tool(description = "Import local files as immutable attachments on an exact draft version")]
    async fn add_draft_attachments(
        &self,
        Parameters(request): Parameters<AddDraftAttachmentRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_send()?;
        let _permit = self.enter().await?;
        if request.file_paths.is_empty() {
            return Err("file_paths 不能为空。".to_owned());
        }
        let paths = request
            .file_paths
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        if paths.iter().any(|path| !path.is_absolute()) {
            return Err("附件路径必须全部为绝对路径。".to_owned());
        }
        let outcome = self
            .local_backend(&request.account_id)?
            .add_draft_attachments(&request.draft_id, request.expected_local_version, &paths)
            .map_err(crate::safe_mail_error)?;
        self.emit_drafts_saved();
        Ok(Json(json!({
            "kind": outcome.kind,
            "draft": draft_value(outcome.draft),
            "canonical": outcome.canonical.map(draft_value),
        })))
    }

    #[tool(description = "Remove one attachment from an exact draft version")]
    async fn remove_draft_attachment(
        &self,
        Parameters(request): Parameters<RemoveDraftAttachmentRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_send()?;
        let _permit = self.enter().await?;
        let outcome = self
            .local_backend(&request.account_id)?
            .remove_draft_attachment(
                &request.draft_id,
                &request.attachment_id,
                request.expected_local_version,
            )
            .map_err(crate::safe_mail_error)?;
        self.emit_drafts_saved();
        Ok(Json(json!({
            "kind": outcome.kind,
            "draft": draft_value(outcome.draft),
            "canonical": outcome.canonical.map(draft_value),
        })))
    }

    #[tool(description = "Create an editable reply draft from one message")]
    async fn create_reply_draft(
        &self,
        Parameters(request): Parameters<MessageRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_send()?;
        let _permit = self.enter().await?;
        let local = self.local_backend(&request.account_id)?;
        let cached = local
            .cached_message_by_id(&request.message_id)
            .map_err(crate::safe_mail_error)?;
        if !cached.body_fetched {
            self.network_backend(&request.account_id)
                .await?
                .fetch_message_by_id(&request.message_id, false)
                .await
                .map_err(crate::safe_mail_error)?;
        }
        let compose = local
            .prepare_reply(&request.message_id)
            .map_err(crate::safe_mail_error)?;
        let draft = local.save_draft(compose).map_err(crate::safe_mail_error)?;
        self.emit_drafts_saved();
        Ok(Json(draft_value(
            local.draft_dto(&draft.id).map_err(crate::safe_mail_error)?,
        )))
    }

    #[tool(description = "Create an editable forward draft, optionally with source attachments")]
    async fn create_forward_draft(
        &self,
        Parameters(request): Parameters<ForwardDraftRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_send()?;
        let _permit = self.enter().await?;
        let outcome = self
            .network_backend(&request.account_id)
            .await?
            .prepare_forward(&request.message_id, request.include_attachments)
            .await
            .map_err(crate::safe_mail_error)?;
        match outcome {
            ForwardPreparationOutcome::Prepared { prepared } => {
                self.emit_drafts_saved();
                Ok(Json(json!({
                    "kind": "prepared",
                    "draft": draft_value(prepared.draft),
                    "warnings": prepared.warnings,
                })))
            }
            ForwardPreparationOutcome::Error { error } => Ok(Json(json!({
                "kind": "error",
                "error": error,
            }))),
        }
    }

    #[tool(description = "Send one exact reviewed draft version to the confirmed recipients")]
    async fn send_draft(
        &self,
        Parameters(request): Parameters<SendDraftRequest>,
    ) -> Result<Json<Value>, String> {
        self.require_send()?;
        let _permit = self.enter().await?;
        let desktop_runtime = self.app.state::<DesktopRuntime>();
        let _smtp = desktop_runtime.begin_smtp_operation()?;
        let item = self
            .network_backend(&request.account_id)
            .await?
            .send_draft(
                &request.draft_id,
                request.expected_local_version,
                &request.confirmed_recipients,
            )
            .await
            .map_err(crate::safe_mail_error)?;
        Ok(Json(json!({
            "outbox_id": item.id,
            "status": item.status,
            "attempts": item.attempts,
            "created_at": item.created_at,
            "sent_at": item.sent_at,
        })))
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AccountOnlyRequest {
    account_id: String,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MineMailMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(rmcp::model::Implementation::new(
                "mine-mail",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Use list_accounts first. Always pass an explicit account_id. Review the exact draft version and recipients before send_draft. Permanent deletion and delivery-unknown retry are intentionally unavailable.",
            )
    }
}

fn bounded_limit(value: usize, maximum: usize, name: &str) -> Result<usize, String> {
    if value == 0 || value > maximum {
        Err(format!("{name} 必须在 1 到 {maximum} 之间。"))
    } else {
        Ok(value)
    }
}

fn parse_roles(values: &[String]) -> Result<Vec<MailboxRole>, String> {
    if values.is_empty() {
        return Ok(vec![
            MailboxRole::Inbox,
            MailboxRole::Sent,
            MailboxRole::Archive,
            MailboxRole::Trash,
        ]);
    }
    values.iter().map(|value| parse_role(value)).collect()
}

fn parse_role(value: &str) -> Result<MailboxRole, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "inbox" => Ok(MailboxRole::Inbox),
        "sent" => Ok(MailboxRole::Sent),
        "archive" => Ok(MailboxRole::Archive),
        "trash" => Ok(MailboxRole::Trash),
        _ => Err("role 仅支持 inbox、sent、archive、trash。".to_owned()),
    }
}

fn compose_request(
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    body_text: String,
) -> ComposeRequest {
    ComposeRequest {
        to,
        cc,
        bcc,
        subject,
        body_text,
        format: Default::default(),
        reply_context: None,
    }
}

fn message_sort_at(message: &InboxMessage) -> &str {
    message
        .internal_date
        .as_deref()
        .or(message.sent_at.as_deref())
        .unwrap_or(&message.synced_at)
}

fn search_item_value(account_id: &str, item: MessagePageItem, query: &str) -> Value {
    let query = query.to_lowercase();
    let matched_in_cached_body = item
        .message
        .body_text
        .as_deref()
        .is_some_and(|body| body.to_lowercase().contains(&query));
    json!({
        "account_id": account_id,
        "message_id": item.public_id,
        "mailbox_role": item.displayed_role,
        "subject": item.message.subject,
        "sender": item.message.sender,
        "to": item.message.to,
        "cc": item.message.cc,
        "sent_at": item.message.sent_at,
        "internal_date": item.message.internal_date,
        "preview": item.message.preview,
        "flags": item.message.flags,
        "body_cached": item.message.body_fetched,
        "matched_in_cached_body": matched_in_cached_body,
        "attachment_names": item.message.attachment_names,
    })
}

fn message_value(
    account_id: &str,
    message_id: &str,
    message: InboxMessage,
    attachments: Vec<mine_mail::AttachmentMeta>,
) -> Value {
    let body = message.body_text.unwrap_or_default();
    let (body_text, body_truncated) = truncate_chars(&body, MAX_BODY_OUTPUT_CHARS);
    json!({
        "account_id": account_id,
        "message_id": message_id,
        "subject": message.subject,
        "sender": message.sender,
        "to": message.to,
        "cc": message.cc,
        "bcc": message.bcc,
        "sent_at": message.sent_at,
        "internal_date": message.internal_date,
        "flags": message.flags,
        "size_bytes": message.size_bytes,
        "body_text": body_text,
        "body_truncated": body_truncated,
        "attachments": attachments.into_iter().map(|attachment| json!({
            "attachment_id": attachment.id,
            "name": attachment.safe_display_name,
            "mime_type": attachment.mime_type,
            "size_bytes": attachment.size_bytes,
            "size_is_estimate": attachment.size_is_estimate,
            "disposition": match attachment.disposition {
                AttachmentDisposition::Attachment => "attachment",
                AttachmentDisposition::Inline => "inline",
            },
        })).collect::<Vec<_>>(),
    })
}

fn truncate_chars(value: &str, maximum: usize) -> (String, bool) {
    let mut characters = value.chars();
    let truncated = characters.by_ref().take(maximum).collect::<String>();
    let has_more = characters.next().is_some();
    (truncated, has_more)
}

fn draft_value(value: CoreDraftDto) -> Value {
    let draft = value.draft;
    json!({
        "draft_id": draft.id,
        "local_version": draft.local_version,
        "has_unsupported_content": draft.has_unsupported_content,
        "to": draft.to,
        "cc": draft.cc,
        "bcc": draft.bcc,
        "subject": draft.subject,
        "body_text": draft.body_text,
        "reply_context": draft.reply_context,
        "status": draft.status,
        "created_at": draft.created_at,
        "updated_at": draft.updated_at,
        "attachments": value.attachments,
        "forward_context": value.forward_context,
    })
}

pub(crate) async fn apply_current_setting(app: AppHandle) -> Result<(), String> {
    let enabled = app.state::<DesktopRuntime>().mcp_access()?.enabled;
    app.state::<McpRuntime>()
        .set_enabled(app.clone(), enabled)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_is_loopback_only() {
        assert_eq!(crate::desktop::MCP_ENDPOINT, "http://127.0.0.1:46321/mcp");
        assert_eq!(MCP_BIND_ADDRESS, "127.0.0.1:46321");
    }

    #[test]
    fn roles_never_include_drafts_or_unknown_mailboxes() {
        assert_eq!(parse_role("inbox"), Ok(MailboxRole::Inbox));
        assert!(parse_role("drafts").is_err());
        assert!(parse_role("custom").is_err());
    }

    #[test]
    fn body_output_is_unicode_safe_and_bounded() {
        let (value, truncated) = truncate_chars("你好世界", 3);
        assert_eq!(value, "你好世");
        assert!(truncated);
    }

    #[test]
    fn discovery_exposes_the_documented_tool_set() {
        let names = MineMailMcp::tool_router()
            .list_all()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "add_draft_attachments",
                "archive_message",
                "create_draft",
                "create_forward_draft",
                "create_reply_draft",
                "delete_draft",
                "download_attachment",
                "get_draft",
                "get_message",
                "index_message_bodies",
                "list_accounts",
                "list_drafts",
                "move_message_to_inbox",
                "move_message_to_trash",
                "remove_draft_attachment",
                "search_messages",
                "send_draft",
                "set_message_read",
                "set_message_starred",
                "sync_mail",
                "update_draft",
            ]
        );
    }
}
