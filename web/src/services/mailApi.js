import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  toUserFacingError,
  userFacingErrorMessage,
} from "../utils/userFacingError.js";

export const isTauriRuntime =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function demoAdapterBuildEnabled({ demoFlag, mode }) {
  return demoFlag === "1" || mode === "test";
}

function resolveRuntime({ tauri, demoFlag, mode }) {
  if (tauri) return "tauri";
  if (demoFlag === "1") return "demo";
  // Vitest is an explicitly isolated mock environment. Production browser
  // builds must opt in with VITE_MINE_MAIL_DEMO=1 instead of silently faking mail.
  if (mode === "test") return "demo";
  return "unsupported";
}

const runtimeKind = resolveRuntime({
  tauri: isTauriRuntime,
  demoFlag: import.meta.env.VITE_MINE_MAIL_DEMO,
  mode: import.meta.env.MODE,
});

export const isTauri = runtimeKind === "tauri";
export const isDemoRuntime = runtimeKind === "demo";
export const isUnsupportedRuntime = runtimeKind === "unsupported";

// Keep this as a compile-time conditional. Vite/Rollup removes the import and
// its full fixture graph from ordinary production builds.
const demoAdapterModule =
  import.meta.env.VITE_MINE_MAIL_DEMO === "1" || import.meta.env.MODE === "test"
    ? import("./demoMailAdapter.js")
    : null;

let demoAdapterReady;

function unsupportedRuntimeError() {
  return new Error(
    "Mine Mail 不支持直接在普通浏览器中运行。请启动 Tauri 桌面版，或设置 VITE_MINE_MAIL_DEMO=1 进行界面演示。",
  );
}

async function prepareDemoAdapter() {
  if (!demoAdapterModule) throw unsupportedRuntimeError();
  demoAdapterReady ??= demoAdapterModule.then(({ createDemoMailAdapter }) => {
    return createDemoMailAdapter({
      normalizeSettings,
      normalizeProfileAvatar,
      normalizeContact,
    });
  });
  return demoAdapterReady;
}

async function callDemo(method, ...args) {
  if (!isDemoRuntime) throw unsupportedRuntimeError();
  const adapter = await prepareDemoAdapter();
  const action = adapter[method];
  if (typeof action !== "function") {
    throw new Error("当前运行时没有实现此操作。");
  }
  try {
    return await action(...args);
  } catch (error) {
    throw toUserFacingError(error, "演示操作没有完成");
  }
}

const commandFailureMessages = Object.freeze({
  get_ai_config: "AI 配置读取没有完成",
  get_ai_provider_registry: "AI 渠道配置读取没有完成",
  save_ai_provider_instance: "AI 渠道保存没有完成",
  delete_ai_provider_instance: "AI 渠道删除没有完成",
  reorder_ai_provider_instances: "AI 渠道排序保存没有完成",
  set_default_ai_provider: "默认 AI 模型设置没有完成",
  get_ai_session: "AI 会话读取没有完成",
  get_ai_context_usage: "AI 上下文占用读取没有完成",
  add_draft_attachments: "添加附件没有完成",
  archive_message: "归档邮件没有完成",
  assign_archive_folder: "归档文件夹设置没有完成",
  configure_account: "邮箱账户连接没有完成",
  confirm_permanent_delete: "永久删除没有完成",
  connect_google_account: "Google 登录没有完成",
  create_mailbox_role: "邮箱文件夹创建没有完成",
  delete_draft: "删除草稿没有完成",
  delete_profile_avatar: "移除头像没有完成",
  fetch_mailbox_message: "邮件正文读取没有完成",
  fetch_outbox_message: "发件队列邮件读取没有完成",
  get_account_status: "账户状态读取没有完成",
  get_desktop_settings: "桌面设置读取没有完成",
  list_contact_messages: "往来邮件读取没有完成",
  list_contacts: "联系人读取没有完成",
  list_archive_folder_candidates: "服务器文件夹读取没有完成",
  list_ai_sessions: "AI 会话列表读取没有完成",
  list_ai_models: "可用模型检索没有完成",
  list_mailbox_page: "邮件列表读取没有完成",
  load_older_mailbox_page: "更早邮件读取没有完成",
  move_message_to_inbox: "移回收件箱没有完成",
  move_message_to_trash: "移到废纸篓没有完成",
  open_external_url: "外部链接打开没有完成",
  prepare_forward: "转发邮件准备没有完成",
  prepare_permanent_delete: "永久删除确认没有完成",
  prepare_reply: "回复邮件准备没有完成",
  record_ai_patch_outcome: "AI 草稿结果记录没有完成",
  remove_account: "移除邮箱账户没有完成",
  remove_draft_attachment: "移除附件没有完成",
  resolve_delivery_unknown: "投递结果处理没有完成",
  retry_outbox: "重新发送没有完成",
  run_ai_turn: "AI 请求没有完成",
  cancel_ai_turn: "无法停止 AI 请求",
  resolve_ai_proposal_group: "AI 草稿提案没有完成应用",
  save_ai_config: "AI 配置保存没有完成",
  save_draft: "草稿保存没有完成",
  save_message_attachment: "附件保存没有完成",
  save_profile_avatar: "头像保存没有完成",
  send_draft: "发送邮件没有完成",
  set_account_remark: "邮箱备注保存没有完成",
  set_contact_favorite: "联系人收藏状态保存没有完成",
  set_contact_remark: "联系人备注保存没有完成",
  set_message_seen: "已读状态保存没有完成",
  set_message_starred_by_id: "星标状态保存没有完成",
  switch_account: "邮箱账户切换没有完成",
  test_ai_connection: "AI 连接测试没有完成",
  test_ai_provider_instance: "AI 渠道连接测试没有完成",
  refresh_ai_model_catalog: "可用模型刷新没有完成",
  translate_mail_content: "AI 翻译没有完成",
  sync_all: "邮箱同步没有完成",
  sync_drafts: "草稿同步没有完成",
  sync_mailbox: "邮箱文件夹同步没有完成",
  sync_sent: "已发送邮件同步没有完成",
  update_desktop_settings: "桌面设置保存没有完成",
});

function commandError(error, command) {
  return toUserFacingError(
    error,
    commandFailureMessages[command] || "该操作没有完成",
  );
}

async function desktopInvoke(command, args) {
  try {
    return await invoke(command, args);
  } catch (error) {
    throw commandError(error, command);
  }
}

function normalizeSettings(settings = {}) {
  const interval = Number(
    settings.pollingIntervalMinutes ??
      settings.poll_interval_minutes ??
      settings.polling_interval_minutes ??
      5,
  );
  const remoteImageMode =
    settings.remoteImageMode ?? settings.remote_image_mode ?? "automatic";
  const notificationSound =
    settings.notificationSound ?? settings.notification_sound ?? "mail";
  const notificationDelivery =
    settings.notificationDelivery ?? settings.notification_delivery ?? "mine_mail";
  return {
    pollingIntervalMinutes: [1, 3, 5].includes(interval) ? interval : 5,
    autostartEnabled: Boolean(
      settings.autostartEnabled ?? settings.autostart_enabled ?? false,
    ),
    notificationsEnabled: Boolean(
      settings.notificationsEnabled ?? settings.notifications_enabled ?? true,
    ),
    notificationDelivery: ["mine_mail", "windows"].includes(notificationDelivery)
      ? notificationDelivery
      : "mine_mail",
    windowsNotificationsAvailable: Boolean(
      settings.windowsNotificationsAvailable ??
      settings.windows_notifications_available ??
      false,
    ),
    notificationSoundEnabled: Boolean(
      settings.notificationSoundEnabled ??
      settings.notification_sound_enabled ??
      true,
    ),
    notificationSound: ["default", "mail", "im", "reminder"].includes(
      notificationSound,
    )
      ? notificationSound
      : "mail",
    remoteImageMode: ["automatic", "ask", "blocked"].includes(remoteImageMode)
      ? remoteImageMode
      : "automatic",
    aiAssistantDefaultOpen: Boolean(
      settings.aiAssistantDefaultOpen ??
      settings.ai_assistant_default_open ??
      true,
    ),
    mcpEnabled: Boolean(settings.mcpEnabled ?? settings.mcp_enabled ?? false),
    mcpInformationEnabled: Boolean(
      settings.mcpInformationEnabled ?? settings.mcp_information_enabled ?? true,
    ),
    mcpSendEnabled: Boolean(
      settings.mcpSendEnabled ?? settings.mcp_send_enabled ?? false,
    ),
    mcpEndpoint:
      settings.mcpEndpoint ??
      settings.mcp_endpoint ??
      "http://127.0.0.1:46321/mcp",
    startupError:
      (settings.startupError ?? settings.startup_error)
        ? userFacingErrorMessage(
            settings.startupError ?? settings.startup_error,
            "桌面设置初始化没有完成",
          )
        : null,
  };
}

function settingsDto(settings) {
  const normalized = normalizeSettings(settings);
  return {
    poll_interval_minutes: normalized.pollingIntervalMinutes,
    autostart_enabled: normalized.autostartEnabled,
    notifications_enabled: normalized.notificationsEnabled,
    notification_delivery: normalized.notificationDelivery,
    notification_sound_enabled: normalized.notificationSoundEnabled,
    notification_sound: normalized.notificationSound,
    remote_image_mode: normalized.remoteImageMode,
    ai_assistant_default_open: normalized.aiAssistantDefaultOpen,
    mcp_enabled: normalized.mcpEnabled,
    mcp_information_enabled: normalized.mcpInformationEnabled,
    mcp_send_enabled: normalized.mcpSendEnabled,
  };
}

function normalizeAccountStatus(status = {}) {
  const normalizeAccount = (account = {}) => ({
    accountId: account.accountId ?? account.account_id ?? null,
    provider: account.provider ?? null,
    email: account.email ?? null,
    remark: (account.remark ?? "").trim() || null,
    authentication: account.authentication ?? "password",
    backendReady: Boolean(account.backendReady ?? account.backend_ready),
    credentialAvailable: Boolean(
      account.credentialAvailable ?? account.credential_available,
    ),
    credentialInvalid: Boolean(
      account.credentialInvalid ?? account.credential_invalid,
    ),
    networkReady: Boolean(account.networkReady ?? account.network_ready),
  });
  const accounts = Array.isArray(status.accounts)
    ? status.accounts.map(normalizeAccount)
    : status.configured
      ? [
          normalizeAccount({
            accountId: status.accountId ?? status.account_id ?? "primary",
            provider: status.provider ?? status.provider_id,
            email: status.email,
            remark: status.remark,
            authentication: status.authentication,
            backendReady: status.backendReady ?? status.backend_ready ?? true,
            credentialAvailable:
              status.credentialAvailable ?? status.credential_available ?? true,
            credentialInvalid:
              status.credentialInvalid ?? status.credential_invalid ?? false,
            networkReady: status.networkReady ?? status.network_ready ?? true,
          }),
        ]
      : [];
  return {
    configured: Boolean(status.configured),
    accountId: status.accountId ?? status.account_id ?? null,
    activeAccountId:
      status.activeAccountId ??
      status.active_account_id ??
      status.accountId ??
      status.account_id ??
      null,
    provider: status.provider ?? status.provider_id ?? null,
    email: status.email ?? null,
    remark: (status.remark ?? "").trim() || null,
    authentication: status.authentication ?? null,
    backendReady: Boolean(
      status.backendReady ?? status.backend_ready ?? status.configured,
    ),
    credentialAvailable: Boolean(
      status.credentialAvailable ??
      status.credential_available ??
      status.configured,
    ),
    credentialInvalid: Boolean(
      status.credentialInvalid ?? status.credential_invalid,
    ),
    networkReady: Boolean(
      status.networkReady ??
      status.network_ready ??
      status.credentialAvailable ??
      status.credential_available ??
      status.configured,
    ),
    startupError:
      (status.startupError ?? status.startup_error)
        ? userFacingErrorMessage(
            status.startupError ?? status.startup_error,
            "邮箱账户初始化没有完成",
          )
        : null,
    accounts,
    accountCount: Number(
      status.accountCount ?? status.account_count ?? accounts.length,
    ),
    maxAccounts: Number(status.maxAccounts ?? status.max_accounts ?? 3),
    canAddAccount: Boolean(
      status.canAddAccount ?? status.can_add_account ?? accounts.length < 3,
    ),
    googleOauthConfigured: Boolean(
      status.googleOauthConfigured ?? status.google_oauth_configured ?? true,
    ),
  };
}

function normalizeProfileAvatar(avatar = {}) {
  return {
    ownerType: avatar.ownerType ?? avatar.owner_type,
    ownerKey: (avatar.ownerKey ?? avatar.owner_key ?? "").trim().toLowerCase(),
    imageDataUrl: avatar.imageDataUrl ?? avatar.image_data_url ?? null,
  };
}

function normalizeContact(contact = {}) {
  const email = (contact.email ?? "").trim().toLowerCase();
  const originalName =
    String(
      contact.originalName ??
        contact.original_name ??
        contact.displayName ??
        contact.display_name ??
        email,
    ).trim() || email;
  const remark = (contact.remark ?? "").trim() || null;
  return {
    accountId: contact.accountId ?? contact.account_id ?? null,
    email,
    displayName:
      remark || (contact.displayName ?? contact.display_name ?? originalName),
    originalName,
    remark,
    isFavorite: Boolean(contact.isFavorite ?? contact.is_favorite),
    messageCount: Number(contact.messageCount ?? contact.message_count ?? 0),
    lastMessageAt: contact.lastMessageAt ?? contact.last_message_at ?? null,
    lastSubject: contact.lastSubject ?? contact.last_subject ?? null,
  };
}

function relativeAiTime(value) {
  const timestamp = Number(value || 0);
  const elapsed = Math.max(0, Date.now() - timestamp);
  if (!timestamp || elapsed < 60_000) return "刚刚";
  if (elapsed < 60 * 60_000) return `${Math.floor(elapsed / 60_000)} 分钟前`;
  if (elapsed < 24 * 60 * 60_000) {
    return `${Math.floor(elapsed / (60 * 60_000))} 小时前`;
  }
  if (elapsed < 48 * 60 * 60_000) return "昨天";
  return new Date(timestamp).toLocaleDateString("zh-CN", {
    month: "numeric",
    day: "numeric",
  });
}

function normalizeAiSession(session = {}) {
  const summary = session.summary || session;
  return {
    id: summary.id,
    title: summary.title || "新会话",
    updatedAtMs: Number(summary.updated_at_ms ?? summary.updatedAtMs ?? 0),
    lastActive: relativeAiTime(summary.updated_at_ms ?? summary.updatedAtMs),
    drafts: Array.isArray(summary.drafts) ? summary.drafts : [],
    loaded: Array.isArray(session.messages),
    messages: Array.isArray(session.messages)
      ? session.messages.map((message) => ({
          ...message,
          status: message.status || "completed",
          activities: Array.isArray(message.activities)
            ? message.activities.map((activity) => ({
                id: activity.id,
                kind: activity.kind || "thinking",
                label: activity.label || "执行步骤",
                status: activity.status || "completed",
                success: activity.success ?? null,
                detail: "",
              }))
            : [],
          proposal: message.proposal
            ? normalizeAiProposal(message.proposal)
            : null,
        }))
      : [],
  };
}

function normalizeAiProposal(proposal = {}) {
  return {
    id: proposal.id,
    requestId: proposal.requestId ?? proposal.request_id,
    draft: proposal.draft,
    changedFields: proposal.changedFields ?? proposal.changed_fields ?? [],
    headers: {
      changed: Boolean(proposal.headers?.changed),
      status: proposal.headers?.status || "pending",
      canUndo: Boolean(proposal.headers?.canUndo ?? proposal.headers?.can_undo),
    },
    body: {
      changed: Boolean(proposal.body?.changed),
      status: proposal.body?.status || "pending",
      canUndo: Boolean(proposal.body?.canUndo ?? proposal.body?.can_undo),
    },
    expiresAtMs: Number(proposal.expiresAtMs ?? proposal.expires_at_ms ?? 0),
  };
}

function normalizeAiProviderConfiguration(configuration = {}) {
  return {
    protocolId:
      configuration.protocolId
      ?? configuration.protocol_id
      ?? "openai_chat_completions",
    baseUrl: configuration.baseUrl ?? configuration.base_url ?? "",
    modelName: configuration.modelName ?? configuration.model_name ?? "",
    useEnvironmentKey: Boolean(
      configuration.useEnvironmentKey
      ?? configuration.use_environment_key
      ?? false,
    ),
    hasStoredApiKey: Boolean(
      configuration.hasStoredApiKey
      ?? configuration.has_stored_api_key
      ?? false,
    ),
    hasEnvironmentApiKey: Boolean(
      configuration.hasEnvironmentApiKey
      ?? configuration.has_environment_api_key
      ?? false,
    ),
  };
}

function normalizeAiConfig(config = {}) {
  return {
    providerId: config.providerId ?? config.provider_id ?? "custom",
    protocolId: config.protocolId ?? config.protocol_id ?? "auto",
    resolvedProtocolId:
      config.resolvedProtocolId
      ?? config.resolved_protocol_id
      ?? "openai_chat_completions",
    baseUrl: config.baseUrl ?? config.base_url ?? "",
    modelName: config.modelName ?? config.model_name ?? "",
    useEnvironmentKey: Boolean(
      config.useEnvironmentKey ?? config.use_environment_key ?? false,
    ),
    hasStoredApiKey: Boolean(
      config.hasStoredApiKey ?? config.has_stored_api_key ?? false,
    ),
    hasEnvironmentApiKey: Boolean(
      config.hasEnvironmentApiKey ?? config.has_environment_api_key ?? false,
    ),
    environmentVariable:
      config.environmentVariable ??
      config.environment_variable ??
      "AI_API_KEY",
    translationLanguage:
      config.translationLanguage ?? config.translation_language ?? "zh-Hans",
    translationLanguages: Array.isArray(
      config.translationLanguages ?? config.translation_languages,
    )
      ? (config.translationLanguages ?? config.translation_languages).map(
          (language) => ({
            value: language.value ?? language.id,
            label: language.label,
          }),
        )
      : [],
    presets: Array.isArray(config.presets)
      ? config.presets.map((preset) => ({
          id: preset.id,
          label: preset.label,
          baseUrl: preset.baseUrl ?? preset.base_url ?? "",
          environmentVariable:
            preset.environmentVariable ?? preset.environment_variable ?? "",
          models: Array.isArray(preset.models)
            ? preset.models.filter((model) => typeof model === "string")
            : [],
          protocolId: preset.protocolId ?? preset.protocol_id ?? "auto",
          recommendedProtocolId:
            preset.recommendedProtocolId
            ?? preset.recommended_protocol_id
            ?? "openai_chat_completions",
          protocols: Array.isArray(preset.protocols)
            ? preset.protocols.map((protocol) => ({
                id: protocol.id,
                label: protocol.label,
                baseUrl: protocol.baseUrl ?? protocol.base_url ?? "",
                recommended: Boolean(protocol.recommended),
                compatible: protocol.compatible !== false,
                maturity: protocol.maturity ?? "stable",
                limitation: protocol.limitation ?? null,
                recommendationRank:
                  protocol.recommendationRank
                  ?? protocol.recommendation_rank
                  ?? (protocol.recommended ? 100 : 0),
                compatibleModelPrefixes:
                  protocol.compatibleModelPrefixes
                  ?? protocol.compatible_model_prefixes
                  ?? [],
                recommendedBaseUrlHosts:
                  protocol.recommendedBaseUrlHosts
                  ?? protocol.recommended_base_url_hosts
                  ?? [],
                models: Array.isArray(protocol.models)
                  ? protocol.models.filter((model) => typeof model === "string")
                  : [],
              }))
            : [],
          configurations: Array.isArray(preset.configurations)
            ? preset.configurations.map(normalizeAiProviderConfiguration)
            : [],
          configuration: preset.configuration
            ? normalizeAiProviderConfiguration(preset.configuration)
            : null,
        }))
      : [],
  };
}

function normalizeAiProviderInstance(provider = {}) {
  return {
    id: provider.id || "",
    providerId: provider.providerId ?? provider.provider_id ?? "custom",
    providerLabel:
      provider.providerLabel ?? provider.provider_label ?? "自定义",
    name: provider.name || "未命名渠道",
    protocolId: provider.protocolId ?? provider.protocol_id ?? "auto",
    resolvedProtocolId:
      provider.resolvedProtocolId
      ?? provider.resolved_protocol_id
      ?? "openai_chat_completions",
    protocolLabel:
      provider.protocolLabel
      ?? provider.protocol_label
      ?? "OpenAI Chat Completions",
    protocolMaturity:
      provider.protocolMaturity
      ?? provider.protocol_maturity
      ?? "stable",
    protocolLimitation:
      provider.protocolLimitation
      ?? provider.protocol_limitation
      ?? null,
    capabilityStatus:
      provider.capabilityStatus
      ?? provider.capability_status
      ?? "untested",
    capabilityEvidence:
      provider.capabilityEvidence
      ?? provider.capability_evidence
      ?? "declared",
    baseUrl: provider.baseUrl ?? provider.base_url ?? "",
    modelName: provider.modelName ?? provider.model_name ?? "",
    useEnvironmentKey: Boolean(
      provider.useEnvironmentKey ?? provider.use_environment_key ?? false,
    ),
    hasStoredApiKey: Boolean(
      provider.hasStoredApiKey ?? provider.has_stored_api_key ?? false,
    ),
    hasEnvironmentApiKey: Boolean(
      provider.hasEnvironmentApiKey
      ?? provider.has_environment_api_key
      ?? false,
    ),
    environmentVariable:
      provider.environmentVariable
      ?? provider.environment_variable
      ?? "AI_API_KEY",
    models: Array.isArray(provider.models)
      ? provider.models.filter((model) => typeof model === "string")
      : [],
    sortOrder: Number(provider.sortOrder ?? provider.sort_order ?? 0),
    isDefault: Boolean(provider.isDefault ?? provider.is_default ?? false),
    status: provider.status || "untested",
    latencyMs: Number.isFinite(
      Number(provider.latencyMs ?? provider.latency_ms),
    )
      ? Number(provider.latencyMs ?? provider.latency_ms)
      : null,
    checkedAtMs: Number.isFinite(
      Number(provider.checkedAtMs ?? provider.checked_at_ms),
    )
      ? Number(provider.checkedAtMs ?? provider.checked_at_ms)
      : null,
    manualContextWindowTokens: (() => {
      const value =
        provider.manualContextWindowTokens
        ?? provider.manual_context_window_tokens;
      return value != null && Number.isFinite(Number(value))
        ? Number(value)
        : null;
    })(),
  };
}

function normalizeAiProviderRegistry(registry = {}) {
  const normalizedConfig = normalizeAiConfig(registry);
  return {
    providers: Array.isArray(registry.providers)
      ? registry.providers.map(normalizeAiProviderInstance)
      : [],
    presets: normalizedConfig.presets,
    defaultProviderInstanceId:
      registry.defaultProviderInstanceId
      ?? registry.default_provider_instance_id
      ?? null,
    translationLanguage: normalizedConfig.translationLanguage,
    translationLanguages: normalizedConfig.translationLanguages,
    contextWindowOptions: Array.isArray(
      registry.contextWindowOptions ?? registry.context_window_options,
    )
      ? (registry.contextWindowOptions ?? registry.context_window_options)
      : [128000, 200000, 500000, 1000000, 2000000],
  };
}

function normalizeAiModelCatalog(catalog = {}) {
  return {
    models: Array.isArray(catalog.models)
      ? catalog.models.map((model) => ({
          providerInstanceId:
            model.providerInstanceId ?? model.provider_instance_id ?? "",
          providerId: model.providerId ?? model.provider_id ?? "custom",
          providerName:
            model.providerName ?? model.provider_name ?? "未命名渠道",
          modelName: model.modelName ?? model.model_name ?? "",
          isDefault: Boolean(model.isDefault ?? model.is_default ?? false),
          contextWindowTokens: Number(
            model.contextWindowTokens ?? model.context_window_tokens ?? 128000,
          ),
          contextWindowSource:
            model.contextWindowSource ?? model.context_window_source ?? "default",
          contextWindowConfidence: Number(
            model.contextWindowConfidence ?? model.context_window_confidence ?? 1,
          ),
        })).filter((model) => model.providerInstanceId && model.modelName)
      : [],
    successfulProviderCount: Number(
      catalog.successfulProviderCount
      ?? catalog.successful_provider_count
      ?? 0,
    ),
    totalProviderCount: Number(
      catalog.totalProviderCount ?? catalog.total_provider_count ?? 0,
    ),
  };
}

function aiConnectionRequest(config) {
  return {
    providerId: config.providerId,
    protocolId: config.protocolId || "auto",
    baseUrl: config.baseUrl,
    modelName: config.modelName || "",
    useEnvironmentKey: Boolean(config.useEnvironmentKey),
    apiKey: config.apiKey || null,
  };
}

function profileAvatarRequest(request) {
  return {
    owner_type: request.ownerType,
    owner_key: request.ownerKey,
    ...(request.imageBytes ? { image_bytes: request.imageBytes } : {}),
  };
}

export const mailApi = {
  async getAiConfig() {
    if (isTauri) {
      return normalizeAiConfig(await desktopInvoke("get_ai_config"));
    }
    return normalizeAiConfig(await callDemo("getAiConfig"));
  },

  async getAiProviderRegistry() {
    const response = isTauri
      ? await desktopInvoke("get_ai_provider_registry")
      : await callDemo("getAiProviderRegistry");
    return normalizeAiProviderRegistry(response);
  },

  async saveAiProviderInstance(provider) {
    const request = {
      id: provider.id || null,
      providerId: provider.providerId,
      name: provider.name,
      protocolId: provider.protocolId || "auto",
      baseUrl: provider.baseUrl,
      modelName: provider.modelName || "",
      useEnvironmentKey: Boolean(provider.useEnvironmentKey),
      apiKey: provider.useEnvironmentKey ? null : provider.apiKey || null,
      manualContextWindowTokens: provider.manualContextWindowTokens || null,
    };
    const response = isTauri
      ? await desktopInvoke("save_ai_provider_instance", { request })
      : await callDemo("saveAiProviderInstance", request);
    return normalizeAiProviderRegistry(response);
  },

  async deleteAiProviderInstance(providerInstanceId) {
    const response = isTauri
      ? await desktopInvoke("delete_ai_provider_instance", {
          providerInstanceId,
        })
      : await callDemo("deleteAiProviderInstance", providerInstanceId);
    return normalizeAiProviderRegistry(response);
  },

  async reorderAiProviderInstances(ids) {
    const request = { ids };
    const response = isTauri
      ? await desktopInvoke("reorder_ai_provider_instances", { request })
      : await callDemo("reorderAiProviderInstances", request);
    return normalizeAiProviderRegistry(response);
  },

  async setDefaultAiProvider(providerInstanceId) {
    const response = isTauri
      ? await desktopInvoke("set_default_ai_provider", {
          providerInstanceId,
        })
      : await callDemo("setDefaultAiProvider", providerInstanceId);
    return normalizeAiProviderRegistry(response);
  },

  async testAiProviderInstance(providerInstanceId) {
    const response = isTauri
      ? await desktopInvoke("test_ai_provider_instance", {
          providerInstanceId,
        })
      : await callDemo("testAiProviderInstance", providerInstanceId);
    return {
      provider: normalizeAiProviderInstance(response?.provider),
      modelCount: Number(response?.modelCount ?? response?.model_count ?? 0),
    };
  },

  async refreshAiModelCatalog() {
    const response = isTauri
      ? await desktopInvoke("refresh_ai_model_catalog")
      : await callDemo("refreshAiModelCatalog");
    return normalizeAiModelCatalog(response);
  },

  async saveAiConfig(config) {
    const request = {
      ...aiConnectionRequest(config),
      translationLanguage: config.translationLanguage || "zh-Hans",
    };
    if (isTauri) {
      return normalizeAiConfig(
        await desktopInvoke("save_ai_config", { request }),
      );
    }
    return normalizeAiConfig(await callDemo("saveAiConfig", request));
  },

  async setAiTranslationLanguage(languageId) {
    const response = isTauri
      ? await desktopInvoke("set_ai_translation_language", { languageId })
      : await callDemo("setAiTranslationLanguage", languageId);
    return normalizeAiConfig(response);
  },

  async listAiModels(config) {
    const request = aiConnectionRequest(config);
    const response = isTauri
      ? await desktopInvoke("list_ai_models", { request })
      : await callDemo("listAiModels", request);
    return Array.isArray(response?.models) ? response.models : [];
  },

  async testAiConnection(config) {
    const request = aiConnectionRequest(config);
    const response = isTauri
      ? await desktopInvoke("test_ai_connection", { request })
      : await callDemo("testAiConnection", request);
    return {
      latencyMs: Number(response?.latencyMs ?? response?.latency_ms ?? 0),
    };
  },

  async translateMailContent(parts, languageId = null) {
    const request = {
      languageId: typeof languageId === "string" && languageId.trim()
        ? languageId.trim()
        : null,
      parts: Array.isArray(parts)
        ? parts.map((part) => ({
            id: part.id,
            format: part.format,
            content: part.content,
          }))
        : [],
    };
    const response = isTauri
      ? await desktopInvoke("translate_mail_content", { request })
      : await callDemo("translateMailContent", request);
    const result = {
      language: response?.language || "zh-Hans",
      parts: Array.isArray(response?.parts)
        ? response.parts.map((part) => ({
            id: part.id,
            content: part.content,
          }))
        : [],
    };
    const translatedCount = response?.translatedCount ?? response?.translated_count;
    const totalCount = response?.totalCount ?? response?.total_count;
    if (Number.isInteger(translatedCount) && Number.isInteger(totalCount)) {
      result.translatedCount = translatedCount;
      result.totalCount = totalCount;
    }
    return result;
  },

  async listAiSessions() {
    if (isTauri) {
      const sessions = await desktopInvoke("list_ai_sessions");
      return sessions.map(normalizeAiSession);
    }
    return callDemo("listAiSessions");
  },

  async getAiSession(sessionId) {
    if (isTauri) {
      return normalizeAiSession(
        await desktopInvoke("get_ai_session", { sessionId }),
      );
    }
    return callDemo("getAiSession", sessionId);
  },

  async getAiContextUsage(request) {
    const response = isTauri
      ? await desktopInvoke("get_ai_context_usage", { request })
      : await callDemo("getAiContextUsage", request);
    return {
      inputTokens: Number(response?.inputTokens ?? response?.input_tokens ?? 0),
      contextWindowTokens: Number(
        response?.contextWindowTokens ?? response?.context_window_tokens ?? 128000,
      ),
      compactionThresholdTokens: Number(
        response?.compactionThresholdTokens
        ?? response?.compaction_threshold_tokens
        ?? 96000,
      ),
      percent: Number(response?.percent ?? 0),
      contextWindowSource:
        response?.contextWindowSource ?? response?.context_window_source ?? "default",
      contextWindowConfidence: Number(
        response?.contextWindowConfidence ?? response?.context_window_confidence ?? 1,
      ),
      estimated: Boolean(response?.estimated ?? true),
      compactionNeeded: Boolean(
        response?.compactionNeeded ?? response?.compaction_needed ?? false,
      ),
    };
  },

  async runAiTurn(request, onEvent = null) {
    if (isTauri) {
      const onEventChannel = new Channel();
      onEventChannel.onmessage = (event) =>
        onEvent?.({
          ...event,
          session: event?.session ? normalizeAiSession(event.session) : null,
        });
      const result = await desktopInvoke("run_ai_turn", {
        request,
        onEvent: onEventChannel,
      });
      return {
        ...result,
        session: result.session ? normalizeAiSession(result.session) : null,
      };
    }
    return callDemo("runAiTurn", request, onEvent);
  },

  async cancelAiTurn(requestId) {
    if (isTauri) {
      return desktopInvoke("cancel_ai_turn", { requestId });
    }
    return callDemo("cancelAiTurn", requestId);
  },

  async resolveAiProposalGroup(request) {
    if (isTauri) {
      const result = await desktopInvoke("resolve_ai_proposal_group", {
        request,
      });
      return {
        ...result,
        proposal: normalizeAiProposal(result.proposal),
      };
    }
    return callDemo("resolveAiProposalGroup", request);
  },

  async recordAiPatchOutcome({
    requestId,
    accountId,
    draftId = null,
    outcome,
    changedFields = [],
  }) {
    if (isTauri) {
      return desktopInvoke("record_ai_patch_outcome", {
        requestId,
        accountId,
        draftId,
        outcome,
        changedFields,
      });
    }
    return callDemo("recordAiPatchOutcome");
  },

  async getMailboxCapabilities(accountId) {
    if (isTauri) {
      return desktopInvoke("get_mailbox_capabilities", { accountId });
    }
    return callDemo("getMailboxCapabilities", accountId);
  },

  async createMailboxRole(accountId, role) {
    if (isTauri) {
      return desktopInvoke("create_mailbox_role", { accountId, role });
    }
    return callDemo("createMailboxRole", accountId, role);
  },

  async listArchiveFolderCandidates(accountId) {
    if (isTauri) {
      return desktopInvoke("list_archive_folder_candidates", { accountId });
    }
    return callDemo("listArchiveFolderCandidates", accountId);
  },

  async assignArchiveFolder(accountId, selectionId) {
    if (isTauri) {
      return desktopInvoke("assign_archive_folder", {
        accountId,
        selectionId,
      });
    }
    return callDemo("assignArchiveFolder", accountId, selectionId);
  },

  async listMailboxPage(
    accountId,
    role,
    cursor = null,
    pageSize = 50,
    query = null,
  ) {
    if (isTauri) {
      return desktopInvoke("list_mailbox_page", {
        accountId,
        role,
        cursor,
        pageSize,
        query,
      });
    }
    return callDemo(
      "listMailboxPage",
      accountId,
      role,
      cursor,
      pageSize,
      query,
    );
  },

  async loadOlderMailboxPage(
    accountId,
    role,
    cursor,
    pageSize = 50,
    query = null,
  ) {
    if (isTauri) {
      return desktopInvoke("load_older_mailbox_page", {
        accountId,
        role,
        cursor,
        pageSize,
        query,
      });
    }
    return callDemo(
      "loadOlderMailboxPage",
      accountId,
      role,
      cursor,
      pageSize,
      query,
    );
  },

  async listStarredMailboxPage(
    accountId,
    role,
    cursor = null,
    pageSize = 50,
    query = null,
  ) {
    if (isTauri) {
      return desktopInvoke("list_starred_mailbox_page", {
        accountId,
        role,
        cursor,
        pageSize,
        query,
      });
    }
    return callDemo(
      "listStarredMailboxPage",
      accountId,
      role,
      cursor,
      pageSize,
      query,
    );
  },

  async loadOlderStarredMailboxPage(
    accountId,
    role,
    cursor,
    pageSize = 50,
    query = null,
  ) {
    if (isTauri) {
      return desktopInvoke("load_older_starred_mailbox_page", {
        accountId,
        role,
        cursor,
        pageSize,
        query,
      });
    }
    return callDemo(
      "loadOlderStarredMailboxPage",
      accountId,
      role,
      cursor,
      pageSize,
      query,
    );
  },

  async syncMailbox(accountId, role) {
    if (isTauri) {
      return desktopInvoke("sync_mailbox", { accountId, role });
    }
    return callDemo("syncMailbox", accountId, role);
  },

  async fetchMailboxMessage(messageId) {
    if (isTauri) {
      return desktopInvoke("fetch_mailbox_message", { messageId });
    }
    return callDemo("fetchMailboxMessage", messageId);
  },

  async saveMessageAttachment(messageId, attachmentId) {
    if (isTauri) {
      return desktopInvoke("save_message_attachment", {
        messageId,
        attachmentId,
      });
    }
    return callDemo("saveMessageAttachment", messageId, attachmentId);
  },

  async setMessageSeen(messageId, seen) {
    if (isTauri) {
      return desktopInvoke("set_message_seen", { messageId, seen });
    }
    return callDemo("setMessageSeen", messageId, seen);
  },

  async setMessageStarredById(messageId, starred) {
    if (isTauri) {
      return desktopInvoke("set_message_starred_by_id", {
        messageId,
        starred,
      });
    }
    return callDemo("setMessageStarredById", messageId, starred);
  },

  async archiveMessage(messageId) {
    if (isTauri) {
      return desktopInvoke("archive_message", { messageId });
    }
    return callDemo("archiveMessage", messageId);
  },

  async moveMessageToTrash(messageId) {
    if (isTauri) {
      return desktopInvoke("move_message_to_trash", { messageId });
    }
    return callDemo("moveMessageToTrash", messageId);
  },

  async moveMessageToInbox(messageId) {
    if (isTauri) {
      return desktopInvoke("move_message_to_inbox", { messageId });
    }
    return callDemo("moveMessageToInbox", messageId);
  },

  async preparePermanentDelete(messageId) {
    if (isTauri) {
      return desktopInvoke("prepare_permanent_delete", { messageId });
    }
    return callDemo("preparePermanentDelete", messageId);
  },

  async confirmPermanentDelete(planId) {
    if (isTauri) {
      return desktopInvoke("confirm_permanent_delete", { planId });
    }
    return callDemo("confirmPermanentDelete", planId);
  },

  async prepareReply(messageId) {
    if (isTauri) return desktopInvoke("prepare_reply", { messageId });
    return callDemo("prepareReply", messageId);
  },

  async prepareForward(messageId, includeAttachments = true) {
    if (isTauri) {
      return desktopInvoke("prepare_forward", {
        messageId,
        includeAttachments,
      });
    }
    return callDemo("prepareForward", messageId, includeAttachments);
  },

  async openExternalUrl(url) {
    if (isTauri) return desktopInvoke("open_external_url", { url });
    return callDemo("openExternalUrl", url);
  },

  async listDrafts() {
    if (isTauri) return desktopInvoke("list_drafts");
    return callDemo("listDrafts");
  },

  async createComposeDraft() {
    if (isTauri) return desktopInvoke("create_compose_draft");
    return callDemo("createComposeDraft");
  },

  async addDraftAttachments(draftId, expectedLocalVersion) {
    if (isTauri) {
      return desktopInvoke("add_draft_attachments", {
        draftId,
        expectedLocalVersion,
      });
    }
    return callDemo("addDraftAttachments", draftId, expectedLocalVersion);
  },

  async removeDraftAttachment(draftId, attachmentId, expectedLocalVersion) {
    if (isTauri) {
      return desktopInvoke("remove_draft_attachment", {
        draftId,
        attachmentId,
        expectedLocalVersion,
      });
    }
    return callDemo(
      "removeDraftAttachment",
      draftId,
      attachmentId,
      expectedLocalVersion,
    );
  },

  async saveDraft(request, draftId = null, expectedLocalVersion = null) {
    if (isTauri) {
      return desktopInvoke("save_draft", {
        request,
        draftId,
        expectedLocalVersion,
      });
    }
    return callDemo("saveDraft", request, draftId, expectedLocalVersion);
  },

  async deleteDraft(draftId, expectedLocalVersion) {
    if (isTauri) {
      return desktopInvoke("delete_draft", { draftId, expectedLocalVersion });
    }
    return callDemo("deleteDraft", draftId, expectedLocalVersion);
  },

  async syncDrafts() {
    if (isTauri) return desktopInvoke("sync_drafts");
    return callDemo("syncDrafts");
  },

  async syncSent() {
    if (isTauri) return desktopInvoke("sync_sent");
    return callDemo("syncSent");
  },

  async syncAll() {
    if (isTauri) return desktopInvoke("sync_all");
    return callDemo("syncAll");
  },

  async completeExit(requestId) {
    if (isTauri) {
      const completed = await desktopInvoke("complete_exit", { requestId });
      if (completed !== true) {
        throw new Error("退出请求已失效，桌面后端未确认退出。");
      }
      return true;
    }
    return callDemo("completeExit", requestId);
  },

  async cancelExit(requestId) {
    if (isTauri) {
      const cancelled = await desktopInvoke("cancel_exit", { requestId });
      if (cancelled !== true) {
        throw new Error("退出请求已失效，桌面后端未确认取消退出。");
      }
      return true;
    }
    return callDemo("cancelExit", requestId);
  },

  async listOutbox() {
    if (isTauri) return desktopInvoke("list_outbox");
    return callDemo("listOutbox");
  },

  async listSentOutboxFallbacks() {
    if (isTauri) return desktopInvoke("list_sent_outbox_fallbacks");
    return callDemo("listSentOutboxFallbacks");
  },

  async fetchOutboxMessage(outboxId) {
    if (isTauri) return desktopInvoke("fetch_outbox_message", { outboxId });
    return callDemo("fetchOutboxMessage", outboxId);
  },

  async retryOutbox(outboxId) {
    if (isTauri) return desktopInvoke("retry_outbox", { outboxId });
    return callDemo("retryOutbox", outboxId);
  },

  async resolveDeliveryUnknown({
    outboxId,
    expectedAttempts,
    decision,
    acknowledgeDuplicateRisk,
  }) {
    const request = {
      outboxId,
      expectedAttempts,
      decision,
      acknowledgeDuplicateRisk,
    };
    if (isTauri) return desktopInvoke("resolve_delivery_unknown", request);
    return callDemo("resolveDeliveryUnknown", request);
  },

  async sendDraft(draftId, expectedLocalVersion, confirmedRecipients) {
    if (isTauri) {
      return desktopInvoke("send_draft", {
        draftId,
        expectedLocalVersion,
        confirmedRecipients,
      });
    }
    return callDemo(
      "sendDraft",
      draftId,
      expectedLocalVersion,
      confirmedRecipients,
    );
  },

  async getDesktopSettings() {
    if (isTauri) {
      return normalizeSettings(await desktopInvoke("get_desktop_settings"));
    }
    return callDemo("getDesktopSettings");
  },

  async updateDesktopSettings(settings) {
    const normalized = normalizeSettings(settings);
    if (isTauri) {
      const updated = await desktopInvoke("update_desktop_settings", {
        settings: settingsDto(normalized),
      });
      return normalizeSettings(updated || normalized);
    }
    return callDemo("updateDesktopSettings", normalized);
  },

  async getNewMailNotification() {
    if (isTauri) return desktopInvoke("get_new_mail_notification");
    return callDemo("getNewMailNotification");
  },

  async dismissNewMailNotification(notificationId) {
    if (isTauri) {
      return desktopInvoke("dismiss_new_mail_notification", { notificationId });
    }
    return callDemo("dismissNewMailNotification", notificationId);
  },

  async openNewMailNotification(notificationId) {
    if (isTauri) {
      return desktopInvoke("open_new_mail_notification", {
        notificationId,
      });
    }
    return callDemo("openNewMailNotification", notificationId);
  },

  async listAccountPresets() {
    if (isTauri) return desktopInvoke("list_account_presets");
    return callDemo("listAccountPresets");
  },

  async getAccountStatus() {
    if (isTauri) {
      return normalizeAccountStatus(await desktopInvoke("get_account_status"));
    }
    return callDemo("getAccountStatus");
  },

  async configureAccount(request) {
    if (isTauri) {
      return normalizeAccountStatus(
        await desktopInvoke("configure_account", { request }),
      );
    }
    return callDemo("configureAccount", request);
  },

  async connectGoogleAccount() {
    if (isTauri) {
      return normalizeAccountStatus(
        await desktopInvoke("connect_google_account"),
      );
    }
    return callDemo("connectGoogleAccount");
  },

  async switchAccount(accountId) {
    if (isTauri) {
      return normalizeAccountStatus(
        await desktopInvoke("switch_account", { accountId }),
      );
    }
    return callDemo("switchAccount", accountId);
  },

  async setAccountRemark(accountId, remark) {
    if (isTauri) {
      return normalizeAccountStatus(
        await desktopInvoke("set_account_remark", {
          accountId,
          remark,
        }),
      );
    }
    return callDemo("setAccountRemark", accountId, remark);
  },

  async removeAccount(accountId, options = {}) {
    const request = {
      accountId,
      revokeGoogleAuthorization: Boolean(options.revokeGoogleAuthorization),
      deleteLocalData: Boolean(options.deleteLocalData),
    };
    if (isTauri) {
      const result = await desktopInvoke("remove_account", { request });
      return {
        ...result,
        status: normalizeAccountStatus(result.status),
        googleAuthorizationRevoked: Boolean(
          result.googleAuthorizationRevoked ??
            result.google_authorization_revoked,
        ),
        localDataDeleted: Boolean(
          result.localDataDeleted ?? result.local_data_deleted,
        ),
        warning: result.warning
          ? userFacingErrorMessage(result.warning, "本地数据清理没有完成")
          : null,
      };
    }
    return callDemo("removeAccount", accountId, options);
  },

  async listProfileAvatars() {
    if (isTauri) {
      const avatars = await desktopInvoke("list_profile_avatars");
      return avatars.map(normalizeProfileAvatar);
    }
    return callDemo("listProfileAvatars");
  },

  async saveProfileAvatar(request) {
    if (isTauri) {
      return normalizeProfileAvatar(
        await desktopInvoke("save_profile_avatar", {
          request: profileAvatarRequest(request),
        }),
      );
    }
    return callDemo("saveProfileAvatar", request);
  },

  async deleteProfileAvatar(request) {
    if (isTauri) {
      await desktopInvoke("delete_profile_avatar", {
        request: profileAvatarRequest(request),
      });
      return;
    }
    return callDemo("deleteProfileAvatar", request);
  },

  async listContacts(accountId) {
    if (isTauri) {
      const directory = await desktopInvoke("list_contacts", { accountId });
      return {
        contacts: (directory.contacts || []).map(normalizeContact),
        favorites: (directory.favorites || []).map(normalizeContact),
      };
    }
    return callDemo("listContacts", accountId);
  },

  async listContactMessages(accountId, email, limit = 250) {
    if (isTauri) {
      return desktopInvoke("list_contact_messages", {
        accountId,
        email,
        limit,
      });
    }
    return callDemo("listContactMessages", accountId, email, limit);
  },

  async setContactFavorite(accountId, email, favorite) {
    if (isTauri) {
      return desktopInvoke("set_contact_favorite", {
        accountId,
        email,
        favorite,
      });
    }
    return callDemo("setContactFavorite", accountId, email, favorite);
  },

  async setContactRemark(email, remark) {
    if (isTauri) {
      return desktopInvoke("set_contact_remark", { email, remark });
    }
    return callDemo("setContactRemark", email, remark);
  },

  async onMailEvent(eventName, handler) {
    if (!isTauri) return callDemo("onMailEvent", eventName, handler);
    try {
      return await listen(eventName, handler);
    } catch (error) {
      throw commandError(error, `listen:${eventName}`);
    }
  },
};

export const __testing = {
  demoAdapterBuildEnabled,
  resolveRuntime,
  normalizeSettings,
  normalizeAccountStatus,
  normalizeProfileAvatar,
  normalizeContact,
  normalizeAiSession,
};
