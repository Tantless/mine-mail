import { demoDrafts, demoMessages } from "../data/demoMail.js";
import { normalizeAppearancePaletteId } from "../appearanceThemes.js";

const demoPageSizeMax = 100;
const demoQueryCharsMax = 256;
const demoAccountId = "demo-primary";
const demoAiProviderId = "11111111-1111-4111-8111-111111111111";
const demoPageRoles = new Set(["inbox", "sent", "archive", "trash"]);
const demoStarredPageRoles = new Set(["inbox", "sent", "archive"]);
const demoSyncRoles = new Set([
  "inbox",
  "sent",
  "drafts",
  "archive",
  "trash",
]);
const creatableDemoRoles = new Set(["trash"]);
const demoProtocolLabels = Object.freeze({
  openai_responses: "OpenAI Responses",
  openai_chat_completions: "OpenAI Chat Completions",
  anthropic_messages: "Anthropic Messages",
});
const demoProviderProtocolIds = Object.freeze({
  custom: ["openai_responses", "openai_chat_completions", "anthropic_messages"],
  deepseek: ["openai_responses", "openai_chat_completions", "anthropic_messages"],
  kimi: ["openai_chat_completions", "anthropic_messages"],
  openai: ["openai_responses", "openai_chat_completions"],
  anthropic: ["anthropic_messages"],
  qwen: ["openai_responses", "openai_chat_completions", "anthropic_messages"],
  mimo: ["openai_responses", "openai_chat_completions", "anthropic_messages"],
  minimax: ["anthropic_messages", "openai_chat_completions", "openai_responses"],
  modelscope: ["openai_chat_completions", "anthropic_messages"],
  doubaoseed: ["openai_responses", "openai_chat_completions"],
  glm: ["openai_chat_completions", "anthropic_messages"],
  openrouter: ["openai_chat_completions", "anthropic_messages", "openai_responses"],
});
const demoAiPresets = [
  ["custom", "自定义", "", "AI_API_KEY", []],
  ["deepseek", "DeepSeek", "https://api.deepseek.com", "DEEPSEEK_API_KEY", ["deepseek-v4-flash", "deepseek-v4-pro"]],
  ["kimi", "Kimi", "https://api.moonshot.ai/v1", "MOONSHOT_API_KEY", ["kimi-k2.6", "kimi-k3"]],
  ["openai", "OpenAI", "https://api.openai.com/v1", "OPENAI_API_KEY", ["gpt-5.6-luna", "gpt-5.6-terra", "gpt-5.6-sol"]],
  ["anthropic", "Anthropic", "https://api.anthropic.com", "ANTHROPIC_API_KEY", ["claude-haiku-4-5", "claude-sonnet-5", "claude-opus-4-8", "claude-fable-5"]],
  ["qwen", "通义千问", "https://dashscope.aliyuncs.com/compatible-mode/v1", "DASHSCOPE_API_KEY", ["qwen3.6-flash", "qwen3.7-plus", "qwen3.7-max"]],
  ["mimo", "Xiaomi MiMo", "https://api.xiaomimimo.com/v1", "MIMO_API_KEY", ["mimo-v2.5", "mimo-v2.5-pro"]],
  ["minimax", "MiniMax", "https://api.minimaxi.com/v1", "MINIMAX_API_KEY", ["MiniMax-M2.7-highspeed", "MiniMax-M2.7"]],
  ["modelscope", "ModelScope", "https://api-inference.modelscope.cn/v1", "MODELSCOPE_SDK_TOKEN", ["Qwen/Qwen3.5-35B-A3B", "Qwen/Qwen3.5-397B-A17B"]],
  ["doubaoseed", "豆包 Seed", "https://ark.cn-beijing.volces.com/api/v3", "ARK_API_KEY", ["doubao-seed-2-0-lite-260428", "doubao-seed-2-0-mini-260428", "doubao-seed-2-0-pro-260215"]],
  ["glm", "智谱 GLM", "https://open.bigmodel.cn/api/paas/v4", "ZAI_API_KEY", ["glm-4.7-flash", "glm-5-turbo", "glm-5.1"]],
  ["openrouter", "OpenRouter", "https://openrouter.ai/api/v1", "OPENROUTER_API_KEY", ["openrouter/auto", "~anthropic/claude-sonnet-latest", "~openai/gpt-latest"]],
].map(([id, label, baseUrl, environmentVariable, models]) => {
  const ids = demoProviderProtocolIds[id];
  const recommendedProtocolId = ids[0];
  return {
    id,
    label,
    baseUrl,
    environmentVariable,
    models,
    protocolId: "auto",
    recommendedProtocolId,
    protocols: ids.map((protocolId) => ({
      id: protocolId,
      label: demoProtocolLabels[protocolId],
      baseUrl:
        id === "deepseek" && protocolId === "anthropic_messages"
          ? "https://api.deepseek.com/anthropic"
          : id === "mimo" && protocolId === "anthropic_messages"
          ? "https://api.xiaomimimo.com/anthropic"
          : id === "minimax" && protocolId === "anthropic_messages"
          ? "https://api.minimaxi.com/anthropic"
          : id === "glm" && protocolId === "anthropic_messages"
            ? "https://open.bigmodel.cn/api/anthropic"
          : id === "kimi" && protocolId === "anthropic_messages"
            ? "https://api.moonshot.ai/anthropic"
          : id === "qwen" && protocolId === "anthropic_messages"
            ? "https://dashscope.aliyuncs.com/apps/anthropic"
          : id === "modelscope" && protocolId === "anthropic_messages"
            ? "https://api-inference.modelscope.cn"
          : id === "openrouter" && protocolId === "anthropic_messages"
            ? "https://openrouter.ai/api"
            : baseUrl,
      recommended: protocolId === recommendedProtocolId,
      recommendationRank: protocolId === recommendedProtocolId ? 100 : 30,
      compatibleModelPrefixes:
        id === "deepseek" && protocolId === "openai_responses"
          ? ["deepseek-v4-flash"]
          : [],
      recommendedBaseUrlHosts:
        id === "custom" && protocolId === "openai_responses"
          ? ["api.xiaomimimo.com", "token-plan-cn.xiaomimimo.com", "token-plan-sgp.xiaomimimo.com", "token-plan-ams.xiaomimimo.com"]
          : [],
      maturity:
        id === "openrouter" && protocolId === "openai_responses"
          ? "beta"
          : ["custom", "kimi", "modelscope"].includes(id)
            && protocolId === "anthropic_messages"
            ? "compatibility"
            : "stable",
      limitation:
        id === "deepseek" && protocolId === "openai_responses"
          ? "当前仅 DeepSeek V4 Flash 支持"
          : id === "openrouter" && protocolId === "openai_responses"
            ? "OpenRouter 当前标记为 Beta"
            : id === "modelscope" && protocolId === "anthropic_messages"
              ? "仅部分模型提供 Anthropic 兼容入口，请先测试连接"
            : null,
      models,
    })),
    configurations: [],
  };
});
const demoAiTranslationLanguages = [
  ["zh-Hans", "中文（简体）"],
  ["zh-Hant", "中文（繁體）"],
  ["en", "English"],
  ["ja", "日本語"],
  ["ko", "한국어"],
  ["ru", "Русский"],
  ["es", "Español"],
  ["fr", "Français"],
  ["de", "Deutsch"],
  ["pt", "Português"],
  ["it", "Italiano"],
  ["ar", "العربية"],
].map(([value, label]) => ({ value, label }));

const wait = (milliseconds) =>
  new Promise((resolve) => window.setTimeout(resolve, milliseconds));

function demoProtocolSupportsModel(protocol, modelName) {
  const model = String(modelName || "").trim().toLowerCase();
  return !model
    || !(protocol.compatibleModelPrefixes || []).length
    || protocol.compatibleModelPrefixes.some(
      (prefix) => model.startsWith(String(prefix).toLowerCase()),
    );
}

function demoResolvedProtocolId(preset, request) {
  if (request.protocolId && request.protocolId !== "auto") return request.protocolId;
  return preset.protocols
    .filter((protocol) => demoProtocolSupportsModel(protocol, request.modelName))
    .reduce((best, protocol) => (
      Number(protocol.recommendationRank || 0) > Number(best.recommendationRank || 0)
        ? protocol
        : best
    ), preset.protocols[0]).id;
}

function demoModelContext(provider, modelName) {
  if (provider?.manualContextWindowTokens) {
    return {
      contextWindowTokens: provider.manualContextWindowTokens,
      contextWindowSource: "manual",
      contextWindowConfidence: 2,
    };
  }
  const model = String(modelName || "").toLowerCase();
  let contextWindowTokens = null;
  if (provider?.providerId === "openai" && /^gpt-5\.(6|4)/.test(model)) {
    contextWindowTokens = 1050000;
  } else if (provider?.providerId === "openai" && model.startsWith("gpt-5")) {
    contextWindowTokens = 400000;
  } else if (provider?.providerId === "deepseek" && model.startsWith("deepseek-v4")) {
    contextWindowTokens = 1000000;
  } else if (provider?.providerId === "anthropic" && model.startsWith("claude-")) {
    contextWindowTokens = 200000;
  } else if (provider?.providerId === "mimo" && /^mimo-v2\.5(?:$|-pro)/.test(model)) {
    contextWindowTokens = 1000000;
  } else if (provider?.providerId === "minimax" && /^minimax-m2(?:$|\.(1|5|7))/.test(model)) {
    contextWindowTokens = 204800;
  } else if (provider?.providerId === "glm" && /^glm-(5|4\.7)/.test(model)) {
    contextWindowTokens = 202752;
  }
  return contextWindowTokens
    ? { contextWindowTokens, contextWindowSource: "official", contextWindowConfidence: 2 }
    : { contextWindowTokens: 128000, contextWindowSource: "default", contextWindowConfidence: 1 };
}

function autoSelectOnlyConfiguredDemoProvider(state, candidateId) {
  if (state.aiProviderRegistry.defaultProviderInstanceId) return;
  const configured = state.aiProviderRegistry.providers.filter(
    (provider) => Boolean(provider.modelName?.trim()),
  );
  if (configured.length !== 1 || configured[0].id !== candidateId) return;
  state.aiProviderRegistry.defaultProviderInstanceId = candidateId;
  state.aiProviderRegistry.providers = state.aiProviderRegistry.providers.map(
    (provider) => ({ ...provider, isDefault: provider.id === candidateId }),
  );
}

function createDemoState() {
  return {
    messages: structuredClone(demoMessages),
    drafts: structuredClone(demoDrafts),
    aiSessions: [],
    aiConfig: {
      providerId: "custom",
      protocolId: "auto",
      resolvedProtocolId: "openai_chat_completions",
      baseUrl: "",
      modelName: "",
      useEnvironmentKey: false,
      hasStoredApiKey: false,
      hasEnvironmentApiKey: false,
      environmentVariable: "AI_API_KEY",
      presets: structuredClone(demoAiPresets),
      translationLanguage: "zh-Hans",
      translationLanguages: structuredClone(demoAiTranslationLanguages),
    },
    aiProviderRegistry: {
      providers: [
        {
          id: demoAiProviderId,
          providerId: "openai",
          providerLabel: "OpenAI",
          name: "OpenAI Official",
          protocolId: "auto",
          resolvedProtocolId: "openai_responses",
          protocolLabel: "OpenAI Responses",
          protocolMaturity: "stable",
          protocolLimitation: null,
          capabilityStatus: "verified",
          structuredOutputStatus: "supported",
          toolCallingStatus: "supported",
          multiTurnToolCallingStatus: "supported",
          capabilityEvidence: "probed",
          baseUrl: "https://api.openai.com/v1",
          modelName: "gpt-5.6-terra",
          useEnvironmentKey: false,
          hasStoredApiKey: true,
          hasEnvironmentApiKey: false,
          environmentVariable: "OPENAI_API_KEY",
          models: ["gpt-5.6-luna", "gpt-5.6-terra", "gpt-5.6-sol"],
          sortOrder: 0,
          isDefault: true,
          status: "available",
          latencyMs: 128,
          checkedAtMs: Date.now(),
          manualContextWindowTokens: null,
        },
      ],
      presets: structuredClone(demoAiPresets),
      defaultProviderInstanceId: demoAiProviderId,
      translationLanguage: "zh-Hans",
      translationLanguages: structuredClone(demoAiTranslationLanguages),
      contextWindowOptions: [128000, 200000, 500000, 1000000, 2000000],
    },
    outbox: [],
    settings: {
      pollingIntervalMinutes: 5,
      autostartEnabled: false,
      notificationsEnabled: true,
      notificationDelivery: "mine_mail",
      windowsNotificationsAvailable: false,
      notificationSoundEnabled: true,
      notificationSound: "mail",
      remoteImageMode: "automatic",
      aiAssistantDefaultOpen: true,
      idlePoetryEnabled: true,
      mcpEnabled: false,
      mcpInformationEnabled: true,
      mcpSendEnabled: false,
      themeScheduleEnabled: false,
      themeScheduleDayStart: "06:00",
      themeScheduleDuskStart: "18:00",
      themeScheduleNightStart: "21:00",
      mcpEndpoint: "http://127.0.0.1:46321/mcp",
    },
    accountStatus: {
      configured: true,
      accountId: demoAccountId,
      activeAccountId: demoAccountId,
      provider: "163",
      email: "demo@163.com",
      remark: null,
      backendReady: true,
      credentialAvailable: true,
      credentialInvalid: false,
      networkReady: true,
      startupError: null,
      accounts: [
        {
          accountId: demoAccountId,
          provider: "163",
          email: "demo@163.com",
          remark: null,
          authentication: "password",
          backendReady: true,
          credentialAvailable: true,
          credentialInvalid: false,
          networkReady: true,
        },
      ],
      accountCount: 1,
      maxAccounts: 3,
      canAddAccount: true,
      googleOauthConfigured: true,
    },
    profileAvatars: [],
    appearance: {
      selectionInitialized: false,
      paletteId: "daylight",
      minimalModeEnabled: true,
      activeTheme: { kind: "builtin", id: "daylight" },
      previousTheme: null,
      customPresets: [],
      activeBackgroundDataUrl: null,
    },
    favoriteContacts: new Set(),
    contactRemarks: new Map(),
    accountPresets: [
      { id: "163", label: "163 邮箱", secret_label: "客户端授权密码" },
      {
        id: "qq",
        label: "QQ 邮箱",
        note: "请使用 QQ 邮箱生成的授权码，而不是 QQ 登录密码。",
        secret_label: "QQ 邮箱授权码",
      },
      {
        id: "gmail",
        label: "Gmail",
        oauth: true,
        note: "请使用 Google 账户生成的应用专用密码，而不是普通登录密码。",
        secret_label: "Google 应用专用密码",
      },
      {
        id: "custom",
        label: "自定义 IMAP/SMTP",
        secret_label: "邮箱密码或授权密码",
      },
    ],
    mailbox: {
      capabilities: [
        { role: "inbox", status: "available", retryable: false },
        { role: "sent", status: "available", retryable: false },
        { role: "drafts", status: "available", retryable: false },
        { role: "archive", status: "available", retryable: false },
        { role: "trash", status: "available", retryable: false },
      ],
      cursorSequence: 0,
      cursors: new Map(),
      mutationSequence: 0,
      deletePlans: new Map(),
    },
  };
}

function requireDemoAccount(accountId) {
  if (accountId !== demoAccountId) {
    throw new Error("演示账户不存在");
  }
}

function requireDemoRole(role, allowedRoles) {
  if (!allowedRoles.has(role)) throw new Error("邮箱角色无效");
}

function normalizeDemoPageInput(pageSize, query) {
  if (
    !Number.isInteger(pageSize) ||
    pageSize < 1 ||
    pageSize > demoPageSizeMax
  ) {
    throw new Error("分页大小必须在 1 到 100 之间");
  }
  const normalizedQuery = (query || "").trim();
  if (
    [...normalizedQuery].length > demoQueryCharsMax ||
    /\p{Cc}/u.test(normalizedQuery)
  ) {
    throw new Error("本地邮件搜索条件无效");
  }
  return normalizedQuery.toLocaleLowerCase();
}

function demoRoleForMessage(message) {
  if (message.demo_role) return message.demo_role;
  const mailbox = String(message.mailbox || "INBOX").toLocaleLowerCase();
  if (mailbox === "sent") return "sent";
  if (mailbox === "archive") return "archive";
  if (mailbox === "trash") return "trash";
  return "inbox";
}

function demoMessageMatchesQuery(message, query) {
  if (!query) return true;
  return [
    message.subject,
    message.sender?.name,
    message.sender?.email,
    ...(message.to || []).flatMap((address) => [address.name, address.email]),
    ...(message.cc || []).flatMap((address) => [address.name, address.email]),
    message.preview,
  ]
    .filter(Boolean)
    .some((value) => String(value).toLocaleLowerCase().includes(query));
}

function demoMessageSummary(message, displayedRole) {
  const {
    account_id: _accountId,
    mailbox: _mailbox,
    uid: _uid,
    message_id: _messageId,
    raw_rfc822: _raw,
    body_text: _bodyText,
    body_html: bodyHtml,
    body_segments: _bodySegments,
    demo_role: _demoRole,
    pending_mutation: pendingMutation,
    ...summary
  } = message;
  return {
    ...structuredClone(summary),
    body_html_available: Boolean(bodyHtml),
    displayed_role: displayedRole,
    ...(pendingMutation
      ? { pending_mutation: structuredClone(pendingMutation) }
      : {}),
  };
}

function issueDemoCursor(state, context) {
  state.mailbox.cursorSequence += 1;
  const cursor = `demo-page-${state.mailbox.cursorSequence}`;
  state.mailbox.cursors.set(cursor, context);
  return cursor;
}

function demoMailboxPage(
  state,
  accountId,
  role,
  cursor = null,
  pageSize = 50,
  query = null,
  flaggedOnly = false,
) {
  requireDemoAccount(accountId);
  requireDemoRole(role, flaggedOnly ? demoStarredPageRoles : demoPageRoles);
  const normalizedQuery = normalizeDemoPageInput(pageSize, query);
  let offset = 0;
  if (cursor) {
    const context = state.mailbox.cursors.get(cursor);
    if (
      !context ||
      context.accountId !== accountId ||
      context.role !== role ||
      context.query !== normalizedQuery ||
      context.flaggedOnly !== flaggedOnly
    ) {
      throw new Error("分页游标无效或已过期");
    }
    offset = context.offset;
  }
  const candidates = state.messages
    .filter((message) => demoRoleForMessage(message) === role)
    .filter(
      (message) =>
        !flaggedOnly ||
        (message.flags || []).some(
          (flag) => flag.toLocaleLowerCase() === "\\flagged".toLocaleLowerCase(),
        ),
    )
    .filter((message) => demoMessageMatchesQuery(message, normalizedQuery));
  const selected = candidates.slice(offset, offset + pageSize);
  const nextOffset = offset + selected.length;
  const hasMoreLocal = nextOffset < candidates.length;
  const nextCursor = hasMoreLocal
    ? issueDemoCursor(state, {
        accountId,
        role,
        query: normalizedQuery,
        flaggedOnly,
        offset: nextOffset,
      })
    : null;
  return {
    items: selected.map((message) => demoMessageSummary(message, role)),
    ...(nextCursor ? { next_cursor: nextCursor } : {}),
    has_more_local: hasMoreLocal,
    remote_history_state: hasMoreLocal ? "not_checked" : "complete",
    end_reached: !hasMoreLocal,
  };
}

function nextDemoMutationId(state, prefix) {
  state.mailbox.mutationSequence += 1;
  return `demo-${prefix}-${state.mailbox.mutationSequence}`;
}

function requireDemoMessage(state, messageId) {
  const message = state.messages.find((candidate) => candidate.id === messageId);
  if (!message) throw new Error("找不到这封邮件");
  return message;
}

function demoSystemFlagMutation(state, messageId, flag, desired) {
  const message = requireDemoMessage(state, messageId);
  const flagName = flag === "seen" ? "\\Seen" : "\\Flagged";
  const flags = (message.flags || []).filter(
    (value) => value.toLocaleLowerCase() !== flagName.toLocaleLowerCase(),
  );
  if (desired) flags.push(flagName);
  message.flags = flags;
  const localRevision = state.mailbox.mutationSequence + 1;
  return {
    operation_id: nextDemoMutationId(state, flag),
    local_revision: localRevision,
    status: "pending",
    source_role: demoRoleForMessage(message),
    flag,
    desired,
  };
}

function demoMoveMessage(state, messageId, destinationRole, kind) {
  const message = requireDemoMessage(state, messageId);
  const sourceRole = demoRoleForMessage(message);
  if (
    (kind === "archive" && !["inbox", "sent"].includes(sourceRole)) ||
    (kind === "move_to_inbox" &&
      !["archive", "trash"].includes(sourceRole)) ||
    (kind === "move_to_trash" &&
      !["inbox", "sent", "archive"].includes(sourceRole))
  ) {
    throw new Error("当前邮箱中不能执行此操作");
  }
  const localRevision = state.mailbox.mutationSequence + 1;
  const operationId = nextDemoMutationId(state, kind);
  message.demo_role = destinationRole;
  message.pending_mutation = {
    operation_id: operationId,
    local_revision: localRevision,
    status: "pending",
    kind,
    source_role: sourceRole,
    destination_role: destinationRole,
  };
  return {
    operation_id: operationId,
    local_revision: localRevision,
    status: "pending",
    source_role: sourceRole,
    destination_role: destinationRole,
  };
}

function demoSelectedMessage(state, messageId) {
  const message = requireDemoMessage(state, messageId);
  const {
    account_id: _accountId,
    mailbox: _mailbox,
    uid: _uid,
    message_id: _messageId,
    raw_rfc822: _raw,
    demo_role: _demoRole,
    pending_mutation: _pendingMutation,
    ...selected
  } = message;
  return {
    ...structuredClone(selected),
    body_html_available: Boolean(message.body_html),
    body_html_loaded: true,
    body_render_mode: message.body_html
      ? message.body_render_mode || "isolated_html"
      : "plain",
    body_segments: (message.body_segments || []).map(
      ({ navigation_target: _navigationTarget, ...segment }) =>
        structuredClone(segment),
    ),
    has_remote_images: Boolean(message.has_remote_images),
    attachments: structuredClone(message.attachments || []),
  };
}

function upsertDemoDraft(state, request, draftId, expectedLocalVersion) {
  const existing = draftId
    ? state.drafts.find((draft) => draft.id === draftId)
    : undefined;
  const now = new Date().toISOString();
  if (
    draftId &&
    (!existing || existing.local_version !== expectedLocalVersion)
  ) {
    const conflictCopy = {
      ...structuredClone(request),
      id: crypto.randomUUID(),
      local_version: 1,
      has_unsupported_content: false,
      status: "conflict",
      created_at: now,
      updated_at: now,
      attachments: structuredClone(existing?.attachments || []),
      forward_context: structuredClone(existing?.forward_context || null),
    };
    state.drafts = [conflictCopy, ...state.drafts];
    return {
      kind: "conflict_copy",
      draft: structuredClone(conflictCopy),
      canonical: existing ? structuredClone(existing) : null,
    };
  }
  const draft = {
    ...structuredClone(request),
    id: existing?.id || draftId || crypto.randomUUID(),
    local_version: existing ? existing.local_version + 1 : 1,
    has_unsupported_content: false,
    status: "local",
    created_at: existing?.created_at || now,
    updated_at: now,
    attachments: structuredClone(existing?.attachments || []),
    forward_context: structuredClone(existing?.forward_context || null),
  };
  state.drafts = [
    draft,
    ...state.drafts.filter((item) => item.id !== draft.id),
  ];
  return { kind: "saved", draft: structuredClone(draft), canonical: null };
}

function normalizeContactEmail(value = "") {
  return value.trim().toLowerCase();
}

function demoFavoriteKey(accountId, email) {
  return `${accountId}\u0000${normalizeContactEmail(email)}`;
}

function demoContactItems(state) {
  const activeAccountId =
    state.accountStatus.activeAccountId || state.accountStatus.accountId || "";
  const accountEmail = normalizeContactEmail(state.accountStatus.email || "");
  const contacts = new Map();

  for (const message of state.messages) {
    const senderEmail = normalizeContactEmail(message.sender?.email || "");
    const outgoing = Boolean(accountEmail) && senderEmail === accountEmail;
    const participants = outgoing
      ? [...(message.to || []), ...(message.cc || [])]
      : message.sender
        ? [message.sender]
        : [];
    const seenInMessage = new Set();
    for (const participant of participants) {
      const email = normalizeContactEmail(participant?.email || "");
      if (!email || email === accountEmail || seenInMessage.has(email)) continue;
      seenInMessage.add(email);
      const sentAt = message.sent_at || message.internal_date || null;
      const existing = contacts.get(email);
      const isNewer =
        !existing?.last_message_at ||
        (sentAt && Date.parse(sentAt) > Date.parse(existing.last_message_at));
      contacts.set(email, {
        email,
        original_name:
          (isNewer ? participant?.name : existing?.display_name) ||
          existing?.original_name ||
          email,
        display_name:
          (isNewer ? participant?.name : existing?.display_name) ||
          existing?.display_name ||
          email,
        remark: null,
        is_favorite: false,
        message_count: (existing?.message_count || 0) + 1,
        last_message_at: isNewer ? sentAt : existing?.last_message_at || sentAt,
        last_subject: isNewer
          ? message.subject || null
          : existing?.last_subject || message.subject || null,
      });
    }
  }

  for (const [email, existing] of contacts) {
    const remark = state.contactRemarks.get(email) || null;
    contacts.set(email, {
      ...existing,
      account_id: activeAccountId,
      original_name: existing.original_name || existing.display_name || email,
      display_name:
        remark || existing.original_name || existing.display_name || email,
      remark,
      is_favorite: state.favoriteContacts.has(
        demoFavoriteKey(activeAccountId, email),
      ),
    });
  }

  const byRecent = (left, right) => {
    if (left.is_favorite !== right.is_favorite)
      return left.is_favorite ? -1 : 1;
    const rightTime = Date.parse(right.last_message_at || "") || 0;
    const leftTime = Date.parse(left.last_message_at || "") || 0;
    if (rightTime !== leftTime) return rightTime - leftTime;
    return left.display_name.localeCompare(right.display_name, "zh-CN");
  };
  const favorites = [...state.favoriteContacts].map((key) => {
    const [accountId, email] = key.split("\u0000");
    const existing = accountId === activeAccountId ? contacts.get(email) : null;
    const remark = state.contactRemarks.get(email) || null;
    return {
      account_id: accountId,
      email,
      original_name: existing?.original_name || email,
      display_name: remark || existing?.original_name || email,
      remark,
      is_favorite: true,
      message_count: existing?.message_count || 0,
      last_message_at: existing?.last_message_at || null,
      last_subject: existing?.last_subject || null,
    };
  });
  return {
    contacts: [...contacts.values()].sort(byRecent),
    favorites: favorites.sort(byRecent),
  };
}

function createDemoActions(
  state,
  { normalizeSettings, normalizeProfileAvatar, normalizeContact },
) {
  const activeAiTurns = new Map();
  return {
    getAiConfig() {
      return structuredClone(state.aiConfig);
    },

    getAiProviderRegistry() {
      return structuredClone(state.aiProviderRegistry);
    },

    saveAiProviderInstance(request) {
      const preset = state.aiProviderRegistry.presets.find(
        (candidate) => candidate.id === request.providerId,
      );
      if (!preset) throw new Error("AI 供应商配置无效");
      if (!request.name?.trim()) throw new Error("请输入渠道名称");
      if (!request.baseUrl?.trim()) throw new Error("请输入 BASE_URL");
      const existing = state.aiProviderRegistry.providers.find(
        (provider) => provider.id === request.id,
      );
      if (
        !request.useEnvironmentKey
        && !request.apiKey?.trim()
        && !existing?.hasStoredApiKey
      ) {
        throw new Error("请输入 API Key，或改为从系统环境变量读取");
      }
      const resolvedProtocolId = demoResolvedProtocolId(preset, request);
      const protocol = preset.protocols.find(
        (candidate) => candidate.id === resolvedProtocolId,
      );
      if (!protocol) throw new Error("AI 协议配置无效");
      const id = existing?.id || crypto.randomUUID();
      const provider = {
        id,
        providerId: preset.id,
        providerLabel: preset.label,
        name: request.name.trim(),
        protocolId: request.protocolId || "auto",
        resolvedProtocolId,
        protocolLabel: protocol.label,
        protocolMaturity: protocol.maturity,
        protocolLimitation: protocol.limitation,
        capabilityStatus: existing?.capabilityStatus || "untested",
        structuredOutputStatus: existing?.structuredOutputStatus || "unknown",
        toolCallingStatus: existing?.toolCallingStatus || "unknown",
        multiTurnToolCallingStatus: existing?.multiTurnToolCallingStatus || "unknown",
        capabilityEvidence: existing?.capabilityEvidence || "declared",
        baseUrl: request.baseUrl.trim(),
        modelName: request.modelName?.trim() || "",
        useEnvironmentKey: Boolean(request.useEnvironmentKey),
        hasStoredApiKey:
          Boolean(existing?.hasStoredApiKey) || Boolean(request.apiKey?.trim()),
        hasEnvironmentApiKey: false,
        environmentVariable: preset.environmentVariable,
        models: existing?.models || [],
        sortOrder:
          existing?.sortOrder ?? state.aiProviderRegistry.providers.length,
        isDefault: Boolean(existing?.isDefault),
        status: existing ? "untested" : "untested",
        latencyMs: null,
        checkedAtMs: null,
        manualContextWindowTokens:
          preset.id === "custom"
            ? Number(request.manualContextWindowTokens || 128000)
            : null,
      };
      state.aiProviderRegistry.providers = [
        ...state.aiProviderRegistry.providers.filter(
          (candidate) => candidate.id !== id,
        ),
        provider,
      ].sort((left, right) => left.sortOrder - right.sortOrder);
      autoSelectOnlyConfiguredDemoProvider(state, id);
      return structuredClone(state.aiProviderRegistry);
    },

    deleteAiProviderInstance(providerInstanceId) {
      const existing = state.aiProviderRegistry.providers.find(
        (provider) => provider.id === providerInstanceId,
      );
      if (!existing) throw new Error("要删除的 AI 渠道不存在");
      state.aiProviderRegistry.providers = state.aiProviderRegistry.providers
        .filter((provider) => provider.id !== providerInstanceId)
        .map((provider, index) => ({ ...provider, sortOrder: index }));
      if (state.aiProviderRegistry.defaultProviderInstanceId === providerInstanceId) {
        state.aiProviderRegistry.defaultProviderInstanceId = null;
      }
      return structuredClone(state.aiProviderRegistry);
    },

    reorderAiProviderInstances(request) {
      const byId = new Map(
        state.aiProviderRegistry.providers.map((provider) => [provider.id, provider]),
      );
      if (request.ids.length !== byId.size || request.ids.some((id) => !byId.has(id))) {
        throw new Error("AI 渠道排序已变化");
      }
      state.aiProviderRegistry.providers = request.ids.map((id, index) => ({
        ...byId.get(id),
        sortOrder: index,
      }));
      return structuredClone(state.aiProviderRegistry);
    },

    setDefaultAiProvider(providerInstanceId) {
      const selected = state.aiProviderRegistry.providers.find(
        (provider) => provider.id === providerInstanceId,
      );
      if (!selected?.modelName) throw new Error("请先为该渠道设置一个首选模型");
      state.aiProviderRegistry.defaultProviderInstanceId = providerInstanceId;
      state.aiProviderRegistry.providers = state.aiProviderRegistry.providers.map(
        (provider) => ({
          ...provider,
          isDefault: provider.id === providerInstanceId,
        }),
      );
      return structuredClone(state.aiProviderRegistry);
    },

    testAiProviderInstance(providerInstanceId) {
      const provider = state.aiProviderRegistry.providers.find(
        (candidate) => candidate.id === providerInstanceId,
      );
      if (!provider) throw new Error("要测试的 AI 渠道不存在");
      const preset = state.aiProviderRegistry.presets.find(
        (candidate) => candidate.id === provider.providerId,
      );
      const models = preset?.models?.length
        ? preset.models
        : ["demo-model-fast", "demo-model-pro"];
      const next = {
        ...provider,
        models,
        modelName: provider.modelName || models[0],
        status: "available",
        latencyMs: 128,
        checkedAtMs: Date.now(),
      };
      state.aiProviderRegistry.providers = state.aiProviderRegistry.providers.map(
        (candidate) => candidate.id === providerInstanceId ? next : candidate,
      );
      autoSelectOnlyConfiguredDemoProvider(state, providerInstanceId);
      const saved = state.aiProviderRegistry.providers.find(
        (candidate) => candidate.id === providerInstanceId,
      );
      return { provider: structuredClone(saved), modelCount: models.length };
    },

    testAiProviderCapabilities(providerInstanceId) {
      const provider = state.aiProviderRegistry.providers.find(
        (candidate) => candidate.id === providerInstanceId,
      );
      if (!provider) throw new Error("要测试的 AI 渠道不存在");
      if (!provider.modelName) throw new Error("请先测试连接并选择首选模型，再测试能力");
      const next = {
        ...provider,
        capabilityStatus: "verified",
        structuredOutputStatus: "supported",
        toolCallingStatus: "supported",
        multiTurnToolCallingStatus: "supported",
        capabilityEvidence: "probed",
      };
      state.aiProviderRegistry.providers = state.aiProviderRegistry.providers.map(
        (candidate) => candidate.id === providerInstanceId ? next : candidate,
      );
      return { provider: structuredClone(next), modelCount: next.models.length };
    },

    refreshAiModelCatalog() {
      const seen = new Set();
      const models = [];
      for (const provider of state.aiProviderRegistry.providers) {
        const preset = state.aiProviderRegistry.presets.find(
          (candidate) => candidate.id === provider.providerId,
        );
        const available = provider.models.length
          ? provider.models
          : preset?.models || [];
        for (const modelName of available) {
          if (seen.has(modelName)) continue;
          seen.add(modelName);
          const context = demoModelContext(provider, modelName);
          models.push({
            providerInstanceId: provider.id,
            providerId: provider.providerId,
            providerName: provider.name,
            modelName,
            isDefault: provider.isDefault && provider.modelName === modelName,
            ...context,
          });
        }
      }
      return {
        models,
        successfulProviderCount: state.aiProviderRegistry.providers.length,
        totalProviderCount: state.aiProviderRegistry.providers.length,
      };
    },

    getAiContextUsage(request) {
      const provider = state.aiProviderRegistry.providers.find(
        (candidate) => candidate.id === request.providerInstanceId,
      );
      const context = demoModelContext(provider, request.modelName);
      const windowTokens = context.contextWindowTokens;
      const session = state.aiSessions.find((candidate) => candidate.id === request.sessionId);
      const text = [
        ...(session?.messages || []).map((message) => message.content || ""),
        request.pendingInstruction || "",
      ].join("\n");
      const inputTokens = Math.max(1, Math.ceil(text.length / 3.2) + 2048);
      return {
        inputTokens,
        contextWindowTokens: windowTokens,
        compactionThresholdTokens: Math.floor(windowTokens * 0.75),
        percent: Math.ceil((inputTokens * 100) / windowTokens),
        contextWindowSource: context.contextWindowSource,
        contextWindowConfidence: context.contextWindowConfidence,
        estimated: true,
        compactionNeeded: inputTokens >= windowTokens * 0.75,
      };
    },

    saveAiConfig(request) {
      const preset = state.aiConfig.presets.find(
        (candidate) => candidate.id === request.providerId,
      );
      if (!preset) throw new Error("AI 供应商配置无效");
      if (!request.baseUrl?.trim()) throw new Error("请输入 BASE_URL");
      if (!request.modelName?.trim()) throw new Error("请输入 MODEL_NAME");
      if (
        !request.useEnvironmentKey
        && !request.apiKey?.trim()
        && !preset.configuration?.hasStoredApiKey
      ) {
        throw new Error("请输入 API Key，或改为从系统环境变量读取");
      }
      const providerConfiguration = {
        protocolId: demoResolvedProtocolId(preset, request),
        baseUrl: request.baseUrl.trim(),
        modelName: request.modelName.trim(),
        useEnvironmentKey: Boolean(request.useEnvironmentKey),
        hasStoredApiKey:
          Boolean(preset.configuration?.hasStoredApiKey)
          || Boolean(request.apiKey?.trim()),
        hasEnvironmentApiKey: false,
      };
      state.aiConfig = {
        ...state.aiConfig,
        providerId: request.providerId,
        protocolId: request.protocolId || "auto",
        resolvedProtocolId: providerConfiguration.protocolId,
        ...providerConfiguration,
        translationLanguage: demoAiTranslationLanguages.some(
          (language) => language.value === request.translationLanguage,
        )
          ? request.translationLanguage
          : "zh-Hans",
        environmentVariable: preset.environmentVariable,
        presets: state.aiConfig.presets.map((candidate) =>
          candidate.id === request.providerId
            ? {
                ...candidate,
                protocolId: request.protocolId || "auto",
                configuration: providerConfiguration,
                configurations: [
                  ...candidate.configurations.filter(
                    (configuration) =>
                      configuration.protocolId !== providerConfiguration.protocolId,
                  ),
                  providerConfiguration,
                ],
              }
            : candidate,
        ),
      };
      return structuredClone(state.aiConfig);
    },

    setAiTranslationLanguage(languageId) {
      if (
        !demoAiTranslationLanguages.some(
          (language) => language.value === languageId,
        )
      ) {
        throw new Error("请选择有效的 AI 翻译语言");
      }
      state.aiConfig.translationLanguage = languageId;
      state.aiProviderRegistry.translationLanguage = languageId;
      return structuredClone(state.aiConfig);
    },

    listAiModels(request) {
      if (!request.baseUrl?.trim()) throw new Error("请输入 BASE_URL");
      const models =
        request.providerId === "deepseek"
          ? ["deepseek-v4-flash", "deepseek-v4-pro"]
          : ["demo-model-fast", "demo-model-pro"];
      state.aiConfig.presets = state.aiConfig.presets.map((preset) =>
        preset.id === request.providerId
          ? {
              ...preset,
              models,
              protocols: preset.protocols.map((protocol) =>
                protocol.id === (
                  request.protocolId === "auto"
                    ? preset.recommendedProtocolId
                    : request.protocolId
                )
                  ? { ...protocol, models }
                  : protocol,
              ),
            }
          : preset,
      );
      return {
        models,
      };
    },

    testAiConnection(request) {
      if (!request.modelName?.trim()) throw new Error("请输入 MODEL_NAME");
      return { latencyMs: 128 };
    },

    translateMailContent(request) {
      const language = request.languageId || state.aiConfig.translationLanguage || "zh-Hans";
      let translatedCount = 0;
      const parts = (request.parts || []).map((part) => {
        if (part.format !== "html") {
          if (part.content.trim()) translatedCount += 1;
          return {
            id: part.id,
            content: part.id === "message-subject"
              ? `【AI 译文】${part.content}`
              : `【AI 译文】\n${part.content}`,
          };
        }
        const template = document.createElement("template");
        template.innerHTML = part.content;
        const walker = document.createTreeWalker(template.content, 4);
        let node = walker.nextNode();
        while (node) {
          const parentTag = node.parentElement?.tagName?.toLowerCase();
          if (
            node.textContent.trim()
            && !["script", "style", "title", "template", "noscript"].includes(
              parentTag,
            )
          ) {
            const source = node.textContent;
            const leading = source.slice(0, source.length - source.trimStart().length);
            const trailing = source.slice(source.trimEnd().length);
            node.textContent = `${leading}【译】${source.trim()}${trailing}`;
            translatedCount += 1;
          }
          node = walker.nextNode();
        }
        return { id: part.id, content: template.innerHTML };
      });
      return {
        language,
        parts,
        translatedCount,
        totalCount: translatedCount,
      };
    },

    listAiSessions() {
      return structuredClone(state.aiSessions).map(
        ({ messages: _messages, ...session }) => ({ ...session, loaded: false }),
      );
    },

    getAiSession(sessionId) {
      const session = state.aiSessions.find(
        (candidate) => candidate.id === sessionId,
      );
      if (!session) throw new Error("找不到这个 AI 会话");
      return { ...structuredClone(session), loaded: true };
    },

    deleteAiSession(sessionId) {
      const existing = state.aiSessions.some(
        (candidate) => candidate.id === sessionId,
      );
      if (!existing) throw new Error("找不到这个 AI 会话");
      state.aiSessions = state.aiSessions.filter(
        (candidate) => candidate.id !== sessionId,
      );
    },

    async runAiTurn(request, onEvent) {
      const requestId = crypto.randomUUID();
      const initial = structuredClone(request.draft.compose);
      let draft = null;
      const changedFields = [];
      const instruction = String(request.instruction || "").trim();
      const shouldWrite =
        request.mode === "optimize" ||
        request.mode === "generate" ||
        (request.mode === "auto" &&
          /写|生成|回复|填入|改写|优化|润色/.test(instruction));
      if (shouldWrite) {
        draft = structuredClone(initial);
        const source = String(draft.body_text || "").trim();
        if (!draft.subject.trim()) {
          draft.subject =
            request.mode === "optimize"
              ? source
                  .split(/[。！？!?\n]/)
                  .map((part) => part.trim())
                  .find(Boolean)
                  ?.slice(0, 24) || "邮件内容"
              : `关于${instruction.slice(0, 18) || "相关事项"}的确认`;
          changedFields.push("subject");
        }
        draft.body_text =
          request.mode === "optimize"
            ? `${source || "您好，"}\n\n感谢您的时间，期待您的回复。`
            : `您好，\n\n想就${instruction || "相关事项"}与您确认一下。烦请您在方便时回复。\n\n感谢您的时间。`;
        draft.format = { ...(draft.format || {}), body_html: null };
        changedFields.push("body_text");
      }
      const assistantMessage = shouldWrite
        ? "已生成邮件修改提案，请在下方分别检查并应用。"
        : "这是离线界面演示；桌面版会按需读取当前草稿后回答。";
      let session = null;
      let assistantEntry = null;
      if (request.mode !== "optimize") {
        const current = request.session_id
          ? state.aiSessions.find(
              (candidate) => candidate.id === request.session_id,
            )
          : null;
        const binding = request.draft.draft_id
          ? {
              id: request.draft.draft_id,
              subject: draft?.subject || initial.subject || "无主题",
            }
          : null;
        if (current) {
          current.lastActive = "刚刚";
          current.updatedAtMs = Date.now();
          assistantEntry = {
            id: crypto.randomUUID(),
            role: "assistant",
            content: "",
            status: "streaming",
            activities: [],
            proposal: null,
          };
          current.messages.push(
            { id: crypto.randomUUID(), role: "user", content: instruction, status: "completed" },
            assistantEntry,
          );
          if (
            binding &&
            !current.drafts.some((item) => item.id === binding.id)
          ) {
            current.drafts.push(binding);
          }
          session = { ...structuredClone(current), loaded: true };
        } else {
          session = {
            id: crypto.randomUUID(),
            title: instruction.slice(0, 18) || "新会话",
            lastActive: "刚刚",
            updatedAtMs: Date.now(),
            drafts: binding ? [binding] : [],
            messages: [
              { id: crypto.randomUUID(), role: "user", content: instruction, status: "completed" },
              (assistantEntry = {
                id: crypto.randomUUID(),
                role: "assistant",
                content: "",
                status: "streaming",
                activities: [],
                proposal: null,
              }),
            ],
            loaded: true,
          };
          state.aiSessions.unshift(structuredClone(session));
        }
        session = current
          ? { ...structuredClone(current), loaded: true }
          : session;
      }
      onEvent?.({
        type: "started",
        request_id: requestId,
        mode: request.mode,
        session: session ? structuredClone(session) : null,
      });
      if (request.mode !== "optimize") {
        const turn = { cancelled: false };
        activeAiTurns.set(requestId, turn);
        const firstThinkingId = `${requestId}:thinking:1`;
        assistantEntry.activities.push({
          id: firstThinkingId,
          kind: "thinking",
          label: "正在思考…",
          status: "running",
          success: null,
          detail: "",
        });
        onEvent?.({
          type: "thinking_started",
          request_id: requestId,
          activity_id: firstThinkingId,
        });
        for (const delta of ["正在理解你的要求，", "并检查需要使用的草稿工具。"]) {
          await wait(28);
          onEvent?.({
            type: "reasoning_delta",
            request_id: requestId,
            activity_id: firstThinkingId,
            delta,
          });
        }
        if (shouldWrite) {
          assistantEntry.activities[0] = {
            ...assistantEntry.activities[0],
            label: "分析完成",
            status: "completed",
            success: true,
          };
          onEvent?.({
            type: "thinking_finished",
            request_id: requestId,
            activity_id: firstThinkingId,
            summary: "分析完成",
            success: true,
          });
          const toolActivityId = `${requestId}:tool:1:0`;
          assistantEntry.activities.push({
            id: toolActivityId,
            kind: "tool",
            label: "正在调用「修改草稿正文」工具…",
            status: "running",
            success: null,
            detail: "",
          });
          onEvent?.({
            type: "tool_started",
            request_id: requestId,
            activity_id: toolActivityId,
            name: "replace_draft_body",
            display_name: "修改草稿正文",
          });
          await wait(80);
          onEvent?.({
            type: "tool_finished",
            request_id: requestId,
            activity_id: toolActivityId,
            name: "replace_draft_body",
            display_name: "修改草稿正文",
            success: true,
          });
          assistantEntry.activities.at(-1).label = "已调用「修改草稿正文」工具";
          assistantEntry.activities.at(-1).status = "completed";
          assistantEntry.activities.at(-1).success = true;
        }
        const finalThinkingId = shouldWrite
          ? `${requestId}:thinking:2`
          : firstThinkingId;
        if (shouldWrite) {
          assistantEntry.activities.push({
            id: finalThinkingId,
            kind: "thinking",
            label: "正在思考…",
            status: "running",
            success: null,
            detail: "",
          });
          onEvent?.({
            type: "thinking_started",
            request_id: requestId,
            activity_id: finalThinkingId,
          });
        }
        onEvent?.({
          type: "reasoning_delta",
          request_id: requestId,
          activity_id: finalThinkingId,
          delta: "正在整理最终回答。",
        });
        if (!shouldWrite) {
          await wait(28);
          const thinking = assistantEntry.activities.find(
            (activity) => activity.id === finalThinkingId,
          );
          if (thinking) {
            thinking.label = "分析完成";
            thinking.status = "completed";
            thinking.success = true;
          }
          onEvent?.({
            type: "thinking_finished",
            request_id: requestId,
            activity_id: finalThinkingId,
            summary: "分析完成",
            success: true,
          });
          const auditActivityId = `${requestId}:audit`;
          assistantEntry.activities.push({
            id: auditActivityId,
            kind: "audit",
            label: "正在复核回答…",
            status: "running",
            success: null,
            detail: "",
          });
          onEvent?.({
            type: "audit_started",
            request_id: requestId,
            activity_id: auditActivityId,
          });
          await wait(36);
          assistantEntry.activities.at(-1).label = "回答已复核";
          assistantEntry.activities.at(-1).status = "completed";
          assistantEntry.activities.at(-1).success = true;
          onEvent?.({
            type: "audit_finished",
            request_id: requestId,
            activity_id: auditActivityId,
            summary: "回答已复核",
            success: true,
          });
        }
        const chunks = assistantMessage.match(/.{1,6}/gu) || [assistantMessage];
        for (const chunk of chunks) {
          await wait(24);
          if (turn.cancelled) break;
          assistantEntry.content += chunk;
          onEvent?.({ type: "content_delta", request_id: requestId, delta: chunk });
        }
        if (turn.cancelled) {
          assistantEntry.status = "stopped";
          const thinking = assistantEntry.activities.find(
            (activity) => activity.id === finalThinkingId,
          );
          if (thinking) {
            thinking.label = "思考已停止";
            thinking.status = "stopped";
            thinking.success = false;
          }
          onEvent?.({
            type: "thinking_finished",
            request_id: requestId,
            activity_id: finalThinkingId,
            summary: "思考已停止",
            success: false,
          });
          onEvent?.({ type: "stopped", request_id: requestId });
        } else {
          assistantEntry.status = "completed";
          const thinking = assistantEntry.activities.find(
            (activity) => activity.id === finalThinkingId,
          );
          if (thinking && shouldWrite) {
            thinking.label = "答案整理完毕";
            thinking.status = "completed";
            thinking.success = true;
          }
          if (shouldWrite) {
            onEvent?.({
              type: "thinking_finished",
              request_id: requestId,
              activity_id: finalThinkingId,
              summary: "答案整理完毕",
              success: true,
            });
          }
          if (draft && changedFields.length) {
            const headerFields = new Set(["to", "cc", "bcc", "subject"]);
            const headerChanged = changedFields.some((field) => headerFields.has(field));
            const bodyChanged = changedFields.some((field) => !headerFields.has(field));
            assistantEntry.proposal = {
              id: crypto.randomUUID(),
              requestId,
              draft: structuredClone(draft),
              changedFields: [...changedFields],
              headers: { changed: headerChanged, status: "pending", canUndo: false },
              body: { changed: bodyChanged, status: "pending", canUndo: false },
              expiresAtMs: Date.now() + 7 * 24 * 60 * 60 * 1000,
              backups: {},
            };
            onEvent?.({
              type: "draft_patch",
              request_id: requestId,
              changed_fields: changedFields,
            });
          }
          onEvent?.({ type: "completed", request_id: requestId });
        }
        activeAiTurns.delete(requestId);
        const stored = state.aiSessions.find((item) => item.id === session.id);
        if (stored) Object.assign(stored, structuredClone(session));
        session = { ...structuredClone(session), loaded: true };
      }
      return {
        request_id: requestId,
        session,
        assistant_message: assistantMessage,
        optimization_decision:
          request.mode === "optimize"
            ? changedFields.length
              ? "changed"
              : "unchanged"
            : undefined,
        draft_revision: request.draft_revision,
        draft,
        changed_fields: changedFields,
        status: assistantEntry?.status || "completed",
      };
    },

    cancelAiTurn(requestId) {
      const turn = activeAiTurns.get(requestId);
      if (!turn) return false;
      turn.cancelled = true;
      return true;
    },

    resolveAiProposalGroup(request) {
      const message = state.aiSessions
        .flatMap((session) => session.messages)
        .find((entry) => entry.proposal?.id === request.proposal_id);
      if (!message) throw new Error("找不到这个 AI 草稿提案，或提案已过期");
      const proposal = message.proposal;
      const group = request.group;
      const current = structuredClone(request.draft.compose);
      if (request.action === "apply") {
        proposal.backups[group] = structuredClone(current);
        if (group === "headers") {
          current.to = structuredClone(proposal.draft.to || []);
          current.cc = structuredClone(proposal.draft.cc || []);
          current.bcc = structuredClone(proposal.draft.bcc || []);
          current.subject = proposal.draft.subject || "";
        } else {
          current.body_text = proposal.draft.body_text || "";
          current.format = structuredClone(proposal.draft.format || {});
        }
        proposal[group] = { ...proposal[group], status: "applied", canUndo: true };
      } else {
        const backup = proposal.backups[group];
        if (!backup) throw new Error("这组提案没有可回退的应用记录");
        if (group === "headers") {
          current.to = backup.to;
          current.cc = backup.cc;
          current.bcc = backup.bcc;
          current.subject = backup.subject;
        } else {
          current.body_text = backup.body_text;
          current.format = backup.format;
        }
        delete proposal.backups[group];
        proposal[group] = { ...proposal[group], status: "pending", canUndo: false };
      }
      return { proposal: structuredClone(proposal), draft: current };
    },

    recordAiPatchOutcome() {},

    getMailboxCapabilities(accountId) {
      requireDemoAccount(accountId);
      return structuredClone(state.mailbox.capabilities);
    },

    createMailboxRole(accountId, role) {
      requireDemoAccount(accountId);
      requireDemoRole(role, creatableDemoRoles);
      const capability = { role, status: "available", retryable: false };
      state.mailbox.capabilities = state.mailbox.capabilities.map((candidate) =>
        candidate.role === role ? capability : candidate,
      );
      return structuredClone(capability);
    },

    listArchiveFolderCandidates(accountId) {
      requireDemoAccount(accountId);
      return [
        {
          selectionId: "demo-archive-folder",
          displayName: "Mine Mail 归档",
        },
      ];
    },

    assignArchiveFolder(accountId, selectionId) {
      requireDemoAccount(accountId);
      if (selectionId !== "demo-archive-folder") {
        throw new Error("归档文件夹选择已失效，请重试。");
      }
      const capability = {
        role: "archive",
        status: "available",
        retryable: false,
      };
      state.mailbox.capabilities = state.mailbox.capabilities.map((candidate) =>
        candidate.role === "archive" ? capability : candidate,
      );
      return structuredClone(capability);
    },

    listMailboxPage(accountId, role, cursor, pageSize, query) {
      return demoMailboxPage(
        state,
        accountId,
        role,
        cursor,
        pageSize,
        query,
      );
    },

    loadOlderMailboxPage(accountId, role, cursor, pageSize, query) {
      return demoMailboxPage(
        state,
        accountId,
        role,
        cursor,
        pageSize,
        query,
      );
    },

    listStarredMailboxPage(accountId, role, cursor, pageSize, query) {
      return demoMailboxPage(
        state,
        accountId,
        role,
        cursor,
        pageSize,
        query,
        true,
      );
    },

    loadOlderStarredMailboxPage(accountId, role, cursor, pageSize, query) {
      return demoMailboxPage(
        state,
        accountId,
        role,
        cursor,
        pageSize,
        query,
        true,
      );
    },

    syncMailbox(accountId, role) {
      requireDemoAccount(accountId);
      requireDemoRole(role, demoSyncRoles);
      const capability = state.mailbox.capabilities.find(
        (candidate) => candidate.role === role,
      );
      if (!capability || capability.status !== "available") {
        throw new Error("此邮箱角色当前不可同步");
      }
      return { synced: 0 };
    },

    fetchMailboxMessage(messageId) {
      return demoSelectedMessage(state, messageId);
    },

    saveMessageAttachment(messageId, attachmentId) {
      const message = requireDemoMessage(state, messageId);
      const attachment = (message.attachments || []).find(
        (candidate) => candidate.id === attachmentId,
      );
      if (!attachment) {
        return {
          status: "error",
          error_kind: "attachment_not_found",
          retryable: false,
        };
      }
      return { status: "canceled", retryable: false };
    },

    setMessageSeen(messageId, seen) {
      return demoSystemFlagMutation(state, messageId, "seen", seen);
    },

    setMessageStarredById(messageId, starred) {
      return demoSystemFlagMutation(state, messageId, "flagged", starred);
    },

    archiveMessage(messageId) {
      return demoMoveMessage(state, messageId, "archive", "archive");
    },

    moveMessageToTrash(messageId) {
      return demoMoveMessage(state, messageId, "trash", "move_to_trash");
    },

    moveMessageToInbox(messageId) {
      return demoMoveMessage(state, messageId, "inbox", "move_to_inbox");
    },

    preparePermanentDelete(messageId) {
      const message = requireDemoMessage(state, messageId);
      if (demoRoleForMessage(message) !== "trash") {
        throw new Error("只有废纸篓中的邮件可以永久删除");
      }
      const planId = nextDemoMutationId(state, "delete-plan");
      state.mailbox.deletePlans.set(planId, messageId);
      return {
        plan_id: planId,
        expires_at: new Date(Date.now() + 5 * 60_000).toISOString(),
      };
    },

    confirmPermanentDelete(planId) {
      const messageId = state.mailbox.deletePlans.get(planId);
      if (!messageId) throw new Error("永久删除确认已失效");
      const message = requireDemoMessage(state, messageId);
      const sourceRole = demoRoleForMessage(message);
      state.mailbox.deletePlans.delete(planId);
      state.messages = state.messages.filter(
        (candidate) => candidate.id !== messageId,
      );
      const localRevision = state.mailbox.mutationSequence + 1;
      return {
        operation_id: nextDemoMutationId(state, "permanent-delete"),
        local_revision: localRevision,
        status: "pending",
        source_role: sourceRole,
      };
    },

    prepareReply(messageId) {
      const message = state.messages.find((mail) => mail.id === messageId);
      if (!message) throw new Error("找不到要回复的邮件");
      const subject = /^re:/i.test(message.subject || "")
        ? message.subject
        : `Re: ${message.subject || ""}`;
      return {
        to: message.sender?.email ? [message.sender.email] : [],
        cc: [],
        bcc: [],
        subject,
        body_text: "",
        format: {
          body_html: null,
          stationery: "none",
          send_stationery: false,
        },
        reply_context: {
          parent_message_id: message.message_id || null,
          references: [...(message.references || [])],
          subject: message.subject || "",
          sender: message.sender || null,
          recipients: [...(message.to || [])],
          sent_at: message.sent_at || message.internal_date || null,
          quoted_text: message.body_text || message.preview || "",
          quoted_html: message.body_html || null,
          quoted_render_mode: message.body_html
            ? message.body_render_mode || "isolated_html"
            : null,
          has_remote_images: message.has_remote_images === true,
        },
      };
    },

    prepareForward(messageId, includeAttachments = true) {
      const message = requireDemoMessage(state, messageId);
      const sourceAttachments = structuredClone(message.attachments || []);
      const outcome = upsertDemoDraft(
        state,
        {
          to: [],
          cc: [],
          bcc: [],
          subject: /^fwd:/i.test(message.subject || "")
            ? message.subject
            : `Fwd: ${message.subject || ""}`,
          body_text: "",
          format: {
            body_html: null,
            stationery: "none",
            send_stationery: false,
          },
          reply_context: null,
        },
        null,
        null,
      );
      const draft = {
        ...outcome.draft,
        attachments: includeAttachments
          ? sourceAttachments.map((attachment) => ({
              id: crypto.randomUUID(),
              name: attachment.safe_display_name || "attachment.bin",
              mime_type: attachment.mime_type || "application/octet-stream",
              size_bytes: attachment.size_bytes || 0,
              source_attachment_id: attachment.id,
            }))
          : [],
        forward_context: {
          source_message_id: messageId,
          original_subject: message.subject || "",
          from: message.sender || null,
          to: structuredClone(message.to || []),
          cc: structuredClone(message.cc || []),
          sent_at: message.sent_at || message.internal_date || null,
          quoted_text: message.body_text || message.preview || "",
          quoted_html: message.body_html || null,
          quoted_render_mode: message.body_html
            ? message.body_render_mode || "isolated_html"
            : null,
          source_attachments: sourceAttachments,
        },
      };
      state.drafts = state.drafts.map((candidate) =>
        candidate.id === draft.id ? structuredClone(draft) : candidate,
      );
      return {
        kind: "prepared",
        prepared: {
          draft: structuredClone(draft),
          warnings: includeAttachments ? [] : ["attachments_omitted_by_user"],
        },
      };
    },

    openExternalUrl(url) {
      const parsed = new URL(url);
      if (!["http:", "https:", "mailto:"].includes(parsed.protocol)) {
        throw new Error("不支持打开这种链接");
      }
      window.open(parsed.href, "_blank", "noopener,noreferrer");
      return true;
    },

    listDrafts() {
      return structuredClone(state.drafts);
    },

    createComposeDraft() {
      return upsertDemoDraft(
        state,
        {
          to: [],
          cc: [],
          bcc: [],
          subject: "",
          body_text: "",
          format: {
            body_html: null,
            stationery: "none",
            send_stationery: false,
          },
          reply_context: null,
        },
        null,
        null,
      ).draft;
    },

    addDraftAttachments(draftId) {
      const draft = state.drafts.find((candidate) => candidate.id === draftId);
      if (!draft) throw new Error("找不到要添加附件的草稿");
      return {
        kind: "canceled",
        draft: structuredClone(draft),
        canonical: null,
      };
    },

    removeDraftAttachment(draftId, attachmentId, expectedLocalVersion) {
      const draft = state.drafts.find((candidate) => candidate.id === draftId);
      if (!draft) throw new Error("找不到要移除附件的草稿");
      if (draft.local_version !== expectedLocalVersion) {
        return {
          kind: "stale",
          draft: structuredClone(draft),
          canonical: null,
        };
      }
      if (!(draft.attachments || []).some((item) => item.id === attachmentId)) {
        throw new Error("找不到要移除的草稿附件");
      }
      const updated = {
        ...draft,
        local_version: draft.local_version + 1,
        updated_at: new Date().toISOString(),
        attachments: draft.attachments.filter(
          (item) => item.id !== attachmentId,
        ),
      };
      state.drafts = state.drafts.map((candidate) =>
        candidate.id === draftId ? updated : candidate,
      );
      return {
        kind: "saved",
        draft: structuredClone(updated),
        canonical: null,
      };
    },

    saveDraft(request, draftId, expectedLocalVersion) {
      return upsertDemoDraft(
        state,
        request,
        draftId,
        expectedLocalVersion,
      );
    },

    deleteDraft(draftId, expectedLocalVersion) {
      const existing = state.drafts.find((draft) => draft.id === draftId);
      if (!existing || existing.local_version !== expectedLocalVersion) {
        return { kind: "stale" };
      }
      state.drafts = state.drafts.filter((draft) => draft.id !== draftId);
      state.aiSessions = state.aiSessions.map((session) => ({
        ...session,
        drafts: session.drafts.filter((draft) => draft.id !== draftId),
      }));
      return { kind: "deleted" };
    },

    syncDrafts() {
      return { synced: state.drafts.length };
    },

    syncSent() {
      return {
        mailbox: "Sent",
        remote_total: 0,
        fetched: 0,
        updated_flags: 0,
        removed: 0,
        cached_total: 0,
        uid_validity_reset: false,
      };
    },

    syncAll() {
      return {
        inbox: {
          mailbox: "INBOX",
          remote_total: state.messages.length,
          fetched: 0,
          updated_flags: 0,
          removed: 0,
          cached_total: state.messages.length,
          uid_validity_reset: false,
        },
        drafts_synced: state.drafts.length,
      };
    },

    completeExit() {
      return true;
    },

    cancelExit() {
      return true;
    },

    listOutbox() {
      return structuredClone(
        state.outbox.filter((candidate) => candidate.status !== "sent"),
      );
    },

    listSentOutboxFallbacks() {
      return structuredClone(
        state.outbox.filter((candidate) => candidate.status === "sent"),
      );
    },

    fetchOutboxMessage(outboxId) {
      const item = state.outbox.find((candidate) => candidate.id === outboxId);
      if (!item) throw new Error("发件队列中的邮件不存在。");
      const draft = state.drafts.find(
        (candidate) => candidate.id === item.draft_id,
      );
      return structuredClone({
        id: item.id,
        subject: item.subject || draft?.subject || "",
        body_text: item.body_text ?? draft?.body_text ?? "",
        body_fetched: true,
      });
    },

    retryOutbox(outboxId) {
      const item = state.outbox.find((candidate) => candidate.id === outboxId);
      if (!item || item.status !== "retryable") {
        throw new Error("只有等待重试的邮件可以再次发送。");
      }
      const sent = {
        ...item,
        status: "sent",
        attempts: item.attempts + 1,
        last_error: null,
        sent_at: new Date().toISOString(),
      };
      state.outbox = [
        sent,
        ...state.outbox.filter((candidate) => candidate.id !== outboxId),
      ];
      return structuredClone(sent);
    },

    resolveDeliveryUnknown({
      outboxId,
      expectedAttempts,
      decision,
      acknowledgeDuplicateRisk,
    }) {
      const item = state.outbox.find((candidate) => candidate.id === outboxId);
      if (
        !item ||
        item.status !== "delivery_unknown" ||
        item.attempts !== expectedAttempts
      ) {
        throw new Error("投递结果已变化，请刷新后重新决定。");
      }
      if (
        decision === "confirm_delivered" &&
        acknowledgeDuplicateRisk === false
      ) {
        const sent = {
          ...item,
          status: "sent",
          last_error: null,
          // The demo mirrors the formal contract: a user decision is not an
          // SMTP delivery timestamp.
          sent_at: item.sent_at ?? null,
        };
        state.outbox = state.outbox.map((candidate) =>
          candidate.id === outboxId ? sent : candidate,
        );
        return structuredClone(sent);
      }
      if (decision === "retry_once" && acknowledgeDuplicateRisk === true) {
        const sent = {
          ...item,
          status: "sent",
          attempts: item.attempts + 1,
          last_error: null,
          sent_at: new Date().toISOString(),
        };
        state.outbox = state.outbox.map((candidate) =>
          candidate.id === outboxId ? sent : candidate,
        );
        return structuredClone(sent);
      }
      throw new Error("投递结果处理请求缺少明确且匹配的风险确认。");
    },

    sendDraft(draftId, expectedLocalVersion, confirmedRecipients) {
      const draft = state.drafts.find((item) => item.id === draftId);
      if (!draft) throw new Error("草稿不存在，无法发送。");
      if (draft.local_version !== expectedLocalVersion) {
        throw new Error("草稿已更新，请重新确认收件人后再发送。");
      }
      const result = {
        id: crypto.randomUUID(),
        draft_id: draftId,
        recipients: [...confirmedRecipients],
        status: "sent",
        attempts: 1,
        last_error: null,
        created_at: new Date().toISOString(),
        sent_at: new Date().toISOString(),
      };
      state.outbox = [result, ...state.outbox];
      state.drafts = state.drafts.map((item) =>
        item.id === draftId ? { ...item, status: "sent" } : item,
      );
      state.aiSessions = state.aiSessions.map((session) => ({
        ...session,
        drafts: session.drafts.filter((draft) => draft.id !== draftId),
      }));
      return structuredClone(result);
    },

    getDesktopSettings() {
      return structuredClone(state.settings);
    },

    updateDesktopSettings(settings) {
      state.settings = normalizeSettings(settings);
      return structuredClone(state.settings);
    },

    getAppearanceSettings() {
      return structuredClone(state.appearance);
    },

    selectAppearanceTheme(request) {
      const preset = state.appearance.customPresets.find(
        (item) => item.id === request.id,
      );
      if (request.kind === "custom" && !preset) {
        throw new Error("自定义主题不存在");
      }
      const unchanged =
        state.appearance.activeTheme.kind === request.kind &&
        state.appearance.activeTheme.id === request.id;
      state.appearance = {
        ...state.appearance,
        selectionInitialized: true,
        ...(request.kind === "builtin" && !state.appearance.minimalModeEnabled
          ? { paletteId: request.id }
          : {}),
        previousTheme: unchanged
          ? state.appearance.previousTheme
          : structuredClone(state.appearance.activeTheme),
        activeTheme: structuredClone(request),
        activeBackgroundDataUrl:
          request.kind === "custom" ? preset?.imageDataUrl || null : null,
      };
      return structuredClone(state.appearance);
    },

    updateAppearancePreferences(request) {
      const enablingImageMode =
        request.minimalModeEnabled === false &&
        state.appearance.minimalModeEnabled &&
        state.appearance.activeTheme.kind === "builtin";
      state.appearance = {
        ...state.appearance,
        ...(request.paletteId != null
          ? { paletteId: normalizeAppearancePaletteId(request.paletteId) }
          : enablingImageMode
            ? { paletteId: state.appearance.activeTheme.id }
            : {}),
        ...(request.minimalModeEnabled != null
          ? { minimalModeEnabled: Boolean(request.minimalModeEnabled) }
          : {}),
      };
      return structuredClone(state.appearance);
    },

    importCustomTheme(request) {
      const id = crypto.randomUUID();
      const names = new Set(
        state.appearance.customPresets.map((preset) => preset.name),
      );
      let nextNumber = 1;
      while (names.has(`自定义主题 ${nextNumber}`)) nextNumber += 1;
      const preset = {
        id,
        name: request.name?.trim() || `自定义主题 ${nextNumber}`,
        focalX: 0.5,
        focalY: 0.5,
        thumbnailDataUrl: request.imageDataUrl,
        imageDataUrl: request.imageDataUrl,
      };
      state.appearance.customPresets.push(preset);
      state.appearance.previousTheme = structuredClone(
        state.appearance.activeTheme,
      );
      state.appearance.activeTheme = { kind: "custom", id };
      state.appearance.activeBackgroundDataUrl = request.imageDataUrl;
      state.appearance.selectionInitialized = true;
      return structuredClone(state.appearance);
    },

    updateCustomTheme(request) {
      const index = state.appearance.customPresets.findIndex(
        (item) => item.id === request.id,
      );
      if (index < 0) throw new Error("自定义主题不存在");
      const current = state.appearance.customPresets[index];
      const next = {
        ...current,
        ...(request.name != null ? { name: request.name.trim() } : {}),
        ...(request.focalX != null ? { focalX: request.focalX } : {}),
        ...(request.focalY != null ? { focalY: request.focalY } : {}),
        ...(request.imageDataUrl
          ? {
              imageDataUrl: request.imageDataUrl,
              thumbnailDataUrl: request.imageDataUrl,
            }
          : {}),
      };
      state.appearance.customPresets[index] = next;
      if (state.appearance.activeTheme.id === request.id) {
        state.appearance.activeBackgroundDataUrl = next.imageDataUrl;
      }
      return structuredClone(state.appearance);
    },

    deleteCustomTheme(id) {
      state.appearance.customPresets = state.appearance.customPresets.filter(
        (item) => item.id !== id,
      );
      if (state.appearance.activeTheme.id === id) {
        const previous = state.appearance.previousTheme;
        const previousPreset = state.appearance.customPresets.find(
          (item) => item.id === previous?.id,
        );
        const fallback =
          previous?.kind === "builtin" || previousPreset
            ? previous
            : { kind: "builtin", id: "daylight" };
        state.appearance.activeTheme = structuredClone(fallback);
        state.appearance.activeBackgroundDataUrl =
          fallback.kind === "custom" ? previousPreset.imageDataUrl : null;
      }
      return structuredClone(state.appearance);
    },

    getNewMailNotification() {
      return null;
    },

    dismissNewMailNotification() {
      return true;
    },

    openNewMailNotification() {
      return true;
    },

    previewNotificationSound(sound) {
      return sound;
    },

    listAccountPresets() {
      return structuredClone(state.accountPresets);
    },

    getAccountStatus() {
      return structuredClone(state.accountStatus);
    },

    configureAccount(request) {
      state.accountStatus = {
        configured: true,
        provider: request.provider,
        email: request.email,
        backendReady: true,
        credentialAvailable: true,
        credentialInvalid: false,
        networkReady: true,
        startupError: null,
      };
      return structuredClone(state.accountStatus);
    },

    connectGoogleAccount() {
      return structuredClone(state.accountStatus);
    },

    switchAccount(accountId) {
      const selected = state.accountStatus.accounts.find(
        (account) => account.accountId === accountId,
      );
      if (selected) {
        state.accountStatus = {
          ...state.accountStatus,
          ...selected,
          activeAccountId: selected.accountId,
        };
      }
      return structuredClone(state.accountStatus);
    },

    setAccountRemark(accountId, remark) {
      const normalizedRemark = (remark || "").trim();
      if ([...normalizedRemark].length > 40) {
        throw new Error("邮箱备注最多 40 个字符");
      }
      if (/\p{Cc}/u.test(normalizedRemark)) {
        throw new Error("邮箱备注不能包含控制字符");
      }
      const accounts = state.accountStatus.accounts.map((account) =>
        account.accountId === accountId
          ? { ...account, remark: normalizedRemark || null }
          : account,
      );
      const active = accounts.find(
        (account) =>
          account.accountId === state.accountStatus.activeAccountId,
      );
      state.accountStatus = {
        ...state.accountStatus,
        ...(active || {}),
        accounts,
        remark: active?.remark || null,
      };
      return structuredClone(state.accountStatus);
    },

    removeAccount(accountId, options = {}) {
      const accounts = state.accountStatus.accounts.filter(
        (account) => account.accountId !== accountId,
      );
      const selected = accounts[0] ?? {};
      state.accountStatus = {
        ...state.accountStatus,
        ...selected,
        configured: accounts.length > 0,
        accounts,
        accountCount: accounts.length,
        activeAccountId: selected.accountId ?? null,
      };
      return {
        status: structuredClone(state.accountStatus),
        googleAuthorizationRevoked: Boolean(
          options.revokeGoogleAuthorization,
        ),
        localDataDeleted: Boolean(options.deleteLocalData),
        warning: null,
      };
    },

    listProfileAvatars() {
      return structuredClone(state.profileAvatars);
    },

    saveProfileAvatar(request) {
      const normalized = normalizeProfileAvatar({
        ...request,
        imageDataUrl: request.imageDataUrl,
      });
      state.profileAvatars = state.profileAvatars.filter(
        (avatar) =>
          avatar.ownerType !== normalized.ownerType ||
          avatar.ownerKey !== normalized.ownerKey,
      );
      state.profileAvatars.push(normalized);
      return structuredClone(normalized);
    },

    deleteProfileAvatar(request) {
      const ownerKey = request.ownerKey.trim().toLowerCase();
      state.profileAvatars = state.profileAvatars.filter(
        (avatar) =>
          avatar.ownerType !== request.ownerType ||
          avatar.ownerKey !== ownerKey,
      );
    },

    listContacts() {
      const directory = demoContactItems(state);
      return {
        contacts: directory.contacts.map(normalizeContact),
        favorites: directory.favorites.map(normalizeContact),
      };
    },

    listContactMessages(_accountId, email, limit = 250) {
      const target = normalizeContactEmail(email);
      const accountEmail = normalizeContactEmail(
        state.accountStatus.email || "",
      );
      return structuredClone(
        state.messages
          .filter((message) => {
            const sender = normalizeContactEmail(message.sender?.email || "");
            if (sender === target) return true;
            if (sender !== accountEmail) return false;
            return [...(message.to || []), ...(message.cc || [])].some(
              (recipient) =>
                normalizeContactEmail(recipient.email || "") === target,
            );
          })
          .slice(0, limit)
          .map((message) => {
            const direction =
              normalizeContactEmail(message.sender?.email || "") ===
              accountEmail
                ? "outgoing"
                : "incoming";
            const mailbox = (message.mailbox || "INBOX").trim();
            return {
              id: message.id,
              direction,
              mailbox_role:
                mailbox.toLowerCase() === "inbox"
                  ? "inbox"
                  : direction === "outgoing"
                    ? "sent"
                    : null,
              subject: message.subject || "",
              sender: message.sender || null,
              to: message.to || [],
              cc: message.cc || [],
              sent_at: message.sent_at || null,
              internal_date: message.internal_date || null,
              flags: message.flags || [],
              size_bytes: message.size_bytes || 0,
              preview: message.preview || "",
              body_html_available: Boolean(message.body_html),
              attachment_names: message.attachment_names || [],
              body_fetched: Boolean(message.body_fetched),
              synced_at: message.synced_at || "",
            };
          }),
      );
    },

    setContactFavorite(accountId, email, favorite) {
      const normalizedEmail = normalizeContactEmail(email || "");
      if (!normalizedEmail) throw new Error("联系人邮箱不能为空");
      const key = demoFavoriteKey(accountId, normalizedEmail);
      if (favorite) state.favoriteContacts.add(key);
      else state.favoriteContacts.delete(key);
      return true;
    },

    setContactRemark(email, remark) {
      const normalizedEmail = normalizeContactEmail(email || "");
      if (!normalizedEmail) throw new Error("联系人邮箱不能为空");
      const normalizedRemark = (remark || "").trim();
      if ([...normalizedRemark].length > 80) {
        throw new Error("联系人备注最多 80 个字符");
      }
      if (/\p{Cc}/u.test(normalizedRemark)) {
        throw new Error("联系人备注不能包含控制字符");
      }
      if (normalizedRemark) {
        state.contactRemarks.set(normalizedEmail, normalizedRemark);
      } else {
        state.contactRemarks.delete(normalizedEmail);
      }
      return true;
    },

    onMailEvent() {
      return () => {};
    },
  };
}

export function createDemoMailAdapter(normalizers) {
  const actions = createDemoActions(createDemoState(), normalizers);
  return Object.fromEntries(
    Object.entries(actions).map(([name, action]) => [
      name,
      async (...args) => {
        await wait(80);
        return action(...args);
      },
    ]),
  );
}
