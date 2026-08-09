import { demoDrafts, demoMessages } from "../data/demoMail.js";

const demoPageSizeMax = 100;
const demoQueryCharsMax = 256;
const demoAccountId = "demo-primary";
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
const demoAiPresets = [
  ["custom", "自定义", "", "AI_API_KEY", []],
  ["deepseek", "DeepSeek", "https://api.deepseek.com", "DEEPSEEK_API_KEY", ["deepseek-v4-flash", "deepseek-v4-pro"]],
  ["kimi", "Kimi", "https://api.moonshot.cn/v1", "MOONSHOT_API_KEY", ["kimi-k2.6", "kimi-k3"]],
  ["openai", "OpenAI", "https://api.openai.com/v1", "OPENAI_API_KEY", ["gpt-5.6-luna", "gpt-5.6-terra", "gpt-5.6-sol"]],
  ["anthropic", "Anthropic", "https://api.anthropic.com", "ANTHROPIC_API_KEY", ["claude-haiku-4-5", "claude-sonnet-5", "claude-opus-4-8", "claude-fable-5"]],
  ["qwen", "通义千问", "https://dashscope.aliyuncs.com/compatible-mode/v1", "DASHSCOPE_API_KEY", ["qwen3.6-flash", "qwen3.7-plus", "qwen3.7-max"]],
  ["mimo", "Xiaomi MiMo", "https://api.xiaomimimo.com/v1", "MIMO_API_KEY", ["mimo-v2.5", "mimo-v2.5-pro"]],
  ["minimax", "MiniMax", "https://api.minimaxi.com/v1", "MINIMAX_API_KEY", ["MiniMax-M2.7-highspeed", "MiniMax-M2.7"]],
  ["modelscope", "ModelScope", "https://api-inference.modelscope.cn/v1", "MODELSCOPE_SDK_TOKEN", ["Qwen/Qwen3.5-35B-A3B", "Qwen/Qwen3.5-397B-A17B"]],
  ["doubaoseed", "豆包 Seed", "https://ark.cn-beijing.volces.com/api/v3", "ARK_API_KEY", ["doubao-seed-2-0-lite-260428", "doubao-seed-2-0-mini-260428", "doubao-seed-2-0-pro-260215"]],
  ["glm", "智谱 GLM", "https://open.bigmodel.cn/api/paas/v4", "ZAI_API_KEY", ["glm-4.7-flash", "glm-5-turbo", "glm-5.1"]],
  ["openrouter", "OpenRouter", "https://openrouter.ai/api/v1", "OPENROUTER_API_KEY", ["openrouter/auto", "~anthropic/claude-sonnet-latest", "~openai/gpt-latest"]],
].map(([id, label, baseUrl, environmentVariable, models]) => ({
  id,
  label,
  baseUrl,
  environmentVariable,
  models,
}));
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

function createDemoState() {
  return {
    messages: structuredClone(demoMessages),
    drafts: structuredClone(demoDrafts),
    aiSessions: [],
    aiConfig: {
      providerId: "custom",
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
      mcpEnabled: false,
      mcpInformationEnabled: true,
      mcpSendEnabled: false,
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
        secret_label: "Google OAuth",
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
  return {
    getAiConfig() {
      return structuredClone(state.aiConfig);
    },

    saveAiConfig(request) {
      const preset = demoAiPresets.find(
        (candidate) => candidate.id === request.providerId,
      );
      if (!preset) throw new Error("AI 供应商配置无效");
      if (!request.baseUrl?.trim()) throw new Error("请输入 BASE_URL");
      if (!request.modelName?.trim()) throw new Error("请输入 MODEL_NAME");
      if (!request.useEnvironmentKey && !request.apiKey?.trim()) {
        throw new Error("请输入 API Key，或改为从系统环境变量读取");
      }
      state.aiConfig = {
        ...state.aiConfig,
        providerId: request.providerId,
        baseUrl: request.baseUrl.trim(),
        modelName: request.modelName.trim(),
        useEnvironmentKey: Boolean(request.useEnvironmentKey),
        translationLanguage: demoAiTranslationLanguages.some(
          (language) => language.value === request.translationLanguage,
        )
          ? request.translationLanguage
          : "zh-Hans",
        hasStoredApiKey:
          state.aiConfig.hasStoredApiKey || Boolean(request.apiKey?.trim()),
        environmentVariable: preset.environmentVariable,
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
      return structuredClone(state.aiConfig);
    },

    listAiModels(request) {
      if (!request.baseUrl?.trim()) throw new Error("请输入 BASE_URL");
      const models =
        request.providerId === "deepseek"
          ? ["deepseek-v4-flash", "deepseek-v4-pro"]
          : ["demo-model-fast", "demo-model-pro"];
      state.aiConfig.presets = state.aiConfig.presets.map((preset) =>
        preset.id === request.providerId ? { ...preset, models } : preset,
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
      const language = state.aiConfig.translationLanguage || "zh-Hans";
      const parts = (request.parts || []).map((part) => {
        if (part.format !== "html") {
          return {
            id: part.id,
            content: `【AI 译文】\n${part.content}`,
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
          }
          node = walker.nextNode();
        }
        return { id: part.id, content: template.innerHTML };
      });
      return { language, parts };
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

    runAiTurn(request, onEvent) {
      const requestId = crypto.randomUUID();
      onEvent?.({ type: "started", request_id: requestId, mode: request.mode });
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
        if (request.mode !== "optimize" && !draft.subject.trim()) {
          draft.subject = `关于${instruction.slice(0, 18) || "相关事项"}的确认`;
          changedFields.push("subject");
        }
        const source = String(draft.body_text || "").trim();
        draft.body_text =
          request.mode === "optimize"
            ? `${source || "您好，"}\n\n感谢您的时间，期待您的回复。`
            : `您好，\n\n想就${instruction || "相关事项"}与您确认一下。烦请您在方便时回复。\n\n感谢您的时间。`;
        draft.format = { ...(draft.format || {}), body_html: null };
        changedFields.push("body_text");
        onEvent?.({
          type: "draft_patch",
          request_id: requestId,
          changed_fields: changedFields,
        });
      }
      const assistantMessage = shouldWrite
        ? "已更新当前草稿。"
        : "这是离线界面演示；桌面版会按需读取当前草稿后回答。";
      let session = null;
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
          current.messages.push(
            { id: crypto.randomUUID(), role: "user", content: instruction },
            {
              id: crypto.randomUUID(),
              role: "assistant",
              content: assistantMessage,
            },
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
              { id: crypto.randomUUID(), role: "user", content: instruction },
              {
                id: crypto.randomUUID(),
                role: "assistant",
                content: assistantMessage,
              },
            ],
            loaded: true,
          };
          state.aiSessions.unshift(structuredClone(session));
        }
      }
      onEvent?.({
        type: "content_delta",
        request_id: requestId,
        delta: assistantMessage,
      });
      onEvent?.({ type: "completed", request_id: requestId });
      return {
        request_id: requestId,
        session,
        assistant_message: assistantMessage,
        draft_revision: request.draft_revision,
        draft,
        changed_fields: changedFields,
      };
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

    getNewMailNotification() {
      return null;
    },

    dismissNewMailNotification() {
      return true;
    },

    openNewMailNotification() {
      return true;
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
