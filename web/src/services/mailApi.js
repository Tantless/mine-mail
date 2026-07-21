import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { mockDrafts, mockMessages } from "../data/mockMail.js";

export const isTauriRuntime =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

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

const wait = (milliseconds) =>
  new Promise((resolve) => window.setTimeout(resolve, milliseconds));

let webMessages = structuredClone(mockMessages);
let webDrafts = structuredClone(mockDrafts);
let webOutbox = [];
let webSettings = {
  pollingIntervalMinutes: 5,
  autostartEnabled: false,
  notificationsEnabled: true,
  foregroundNotificationsEnabled: true,
  notificationSoundEnabled: true,
  notificationSound: "mail",
  remoteImageMode: "automatic",
};
let webAccountStatus = {
  configured: true,
  accountId: "demo-primary",
  activeAccountId: "demo-primary",
  provider: "163",
  email: "demo@163.com",
  backendReady: true,
  credentialAvailable: true,
  networkReady: true,
  startupError: null,
  accounts: [
    {
      accountId: "demo-primary",
      provider: "163",
      email: "demo@163.com",
      authentication: "password",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
    },
  ],
  accountCount: 1,
  maxAccounts: 3,
  canAddAccount: true,
  googleOauthConfigured: true,
};
let webProfileAvatars = [];
let webFavoriteContacts = new Set();
let webContactRemarks = new Map();

const webAccountPresets = [
  { id: "163", label: "163 邮箱", secret_label: "客户端授权密码" },
  { id: "gmail", label: "Gmail", oauth: true, secret_label: "Google OAuth" },
  {
    id: "outlook",
    label: "Outlook",
    secret_label: "Modern Auth",
    disabled: true,
    note: "OAuth / Modern Auth 尚未支持",
  },
  { id: "custom", label: "自定义 IMAP/SMTP", secret_label: "邮箱密码或授权密码" },
];

function webOnly(action) {
  return async (...args) => {
    if (!isDemoRuntime) {
      throw new Error(
        "Mine Mail 不支持直接在普通浏览器中运行。请启动 Tauri 桌面版，或设置 VITE_MINE_MAIL_DEMO=1 进行界面演示。",
      );
    }
    await wait(80);
    return action(...args);
  };
}

function commandError(error) {
  if (error instanceof Error) return error;
  if (typeof error === "string") return new Error(error);
  return new Error("桌面后端没有完成此操作。");
}

async function desktopInvoke(command, args) {
  try {
    return await invoke(command, args);
  } catch (error) {
    throw commandError(error);
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
  return {
    pollingIntervalMinutes: [1, 3, 5].includes(interval) ? interval : 5,
    autostartEnabled: Boolean(
      settings.autostartEnabled ?? settings.autostart_enabled ?? false,
    ),
    notificationsEnabled: Boolean(
      settings.notificationsEnabled ?? settings.notifications_enabled ?? true,
    ),
    foregroundNotificationsEnabled: Boolean(
      settings.foregroundNotificationsEnabled ??
        settings.foreground_notifications_enabled ??
        true,
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
    startupError: settings.startupError ?? settings.startup_error ?? null,
  };
}

function settingsDto(settings) {
  const normalized = normalizeSettings(settings);
  return {
    poll_interval_minutes: normalized.pollingIntervalMinutes,
    autostart_enabled: normalized.autostartEnabled,
    notifications_enabled: normalized.notificationsEnabled,
    foreground_notifications_enabled:
      normalized.foregroundNotificationsEnabled,
    notification_sound_enabled: normalized.notificationSoundEnabled,
    notification_sound: normalized.notificationSound,
    remote_image_mode: normalized.remoteImageMode,
  };
}

function normalizeAccountStatus(status = {}) {
  const normalizeAccount = (account = {}) => ({
    accountId: account.accountId ?? account.account_id ?? null,
    provider: account.provider ?? null,
    email: account.email ?? null,
    authentication: account.authentication ?? "password",
    backendReady: Boolean(account.backendReady ?? account.backend_ready),
    credentialAvailable: Boolean(
      account.credentialAvailable ?? account.credential_available,
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
            authentication: status.authentication,
            backendReady: status.backendReady ?? status.backend_ready ?? true,
            credentialAvailable:
              status.credentialAvailable ?? status.credential_available ?? true,
            networkReady: status.networkReady ?? status.network_ready ?? true,
          }),
        ]
      : [];
  return {
    configured: Boolean(status.configured),
    accountId: status.accountId ?? status.account_id ?? null,
    activeAccountId:
      status.activeAccountId ?? status.active_account_id ?? status.accountId ?? status.account_id ?? null,
    provider: status.provider ?? status.provider_id ?? null,
    email: status.email ?? null,
    authentication: status.authentication ?? null,
    backendReady: Boolean(status.backendReady ?? status.backend_ready ?? status.configured),
    credentialAvailable: Boolean(
      status.credentialAvailable ?? status.credential_available ?? status.configured,
    ),
    networkReady: Boolean(
      status.networkReady ??
        status.network_ready ??
        status.credentialAvailable ??
        status.credential_available ??
        status.configured,
    ),
    startupError: status.startupError ?? status.startup_error ?? null,
    accounts,
    accountCount: Number(status.accountCount ?? status.account_count ?? accounts.length),
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
    contact.originalName ??
    contact.original_name ??
    contact.displayName ??
    contact.display_name ??
    email;
  const remark = (contact.remark ?? "").trim() || null;
  return {
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

function normalizeContactEmail(value = "") {
  return value.trim().toLowerCase();
}

function webContactItems() {
  const accountEmail = normalizeContactEmail(webAccountStatus.email || "");
  const contacts = new Map();

  for (const message of webMessages) {
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

  const localEmails = new Set([
    ...webFavoriteContacts,
    ...webContactRemarks.keys(),
  ]);
  for (const email of localEmails) {
    const existing = contacts.get(email) || {
      email,
      original_name: email,
      display_name: email,
      message_count: 0,
      last_message_at: null,
      last_subject: null,
    };
    const remark = webContactRemarks.get(email) || null;
    contacts.set(email, {
      ...existing,
      original_name: existing.original_name || existing.display_name || email,
      display_name: remark || existing.original_name || existing.display_name || email,
      remark,
      is_favorite: webFavoriteContacts.has(email),
    });
  }

  return [...contacts.values()].sort((left, right) => {
    if (left.is_favorite !== right.is_favorite) return left.is_favorite ? -1 : 1;
    const rightTime = Date.parse(right.last_message_at || "") || 0;
    const leftTime = Date.parse(left.last_message_at || "") || 0;
    if (rightTime !== leftTime) return rightTime - leftTime;
    return left.display_name.localeCompare(right.display_name, "zh-CN");
  });
}

function profileAvatarRequest(request) {
  return {
    owner_type: request.ownerType,
    owner_key: request.ownerKey,
    ...(request.imageBytes ? { image_bytes: request.imageBytes } : {}),
  };
}

function upsertMockDraft(request, draftId, expectedLocalVersion) {
  const existing = draftId
    ? webDrafts.find((draft) => draft.id === draftId)
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
    };
    webDrafts = [conflictCopy, ...webDrafts];
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
  };
  webDrafts = [draft, ...webDrafts.filter((item) => item.id !== draft.id)];
  return { kind: "saved", draft: structuredClone(draft), canonical: null };
}

export const mailApi = {
  async listInbox(limit = 50) {
    if (isTauri) return desktopInvoke("list_inbox", { limit });
    return webOnly(() => structuredClone(webMessages.slice(0, limit)))();
  },

  async fetchMessage(uid) {
    if (isTauri) return desktopInvoke("fetch_message", { uid });
    return webOnly(() =>
      structuredClone(webMessages.find((mail) => mail.uid === uid)),
    )();
  },

  async markMessageRead(uid) {
    if (isTauri) return desktopInvoke("mark_message_read", { uid });
    return webOnly(() => {
      const message = webMessages.find((mail) => mail.uid === uid);
      if (!message) throw new Error("找不到要标记为已读的邮件");
      if (!(message.flags || []).some((flag) => flag.toLowerCase() === "\\seen")) {
        message.flags = [...(message.flags || []), "\\Seen"];
      }
      return true;
    })();
  },

  async setMessageStarred(mailbox, uid, starred) {
    if (isTauri) {
      return desktopInvoke("set_message_starred", { mailbox, uid, starred });
    }
    // Browser mode is only a visual test/demo surface. Persistent flag state
    // remains owned by the Tauri Rust/SQLite backend.
    return webOnly(() => true)();
  },

  async listSent(limit = 50) {
    if (isTauri) return desktopInvoke("list_sent", { limit });
    return webOnly(() => [])();
  },

  async fetchSentMessage(uid) {
    if (isTauri) return desktopInvoke("fetch_sent_message", { uid });
    return webOnly(() => undefined)();
  },

  async fetchContactMessage(accountId, mailbox, uid) {
    if (isTauri) {
      return desktopInvoke("fetch_contact_message", {
        accountId,
        mailbox,
        uid,
      });
    }
    return webOnly(() => {
      const message = webMessages.find(
        (mail) => mail.uid === uid && (mail.mailbox || "INBOX") === mailbox,
      );
      if (!message) throw new Error("找不到这封联系人往来邮件");
      return structuredClone(message);
    })();
  },

  async prepareReply(messageId) {
    if (isTauri) return desktopInvoke("prepare_reply", { messageId });
    return webOnly(() => {
      const message = webMessages.find((mail) => mail.id === messageId);
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
    })();
  },

  async openExternalUrl(url) {
    if (isTauri) return desktopInvoke("open_external_url", { url });
    return webOnly(() => {
      const parsed = new URL(url);
      if (!["http:", "https:", "mailto:"].includes(parsed.protocol)) {
        throw new Error("不支持打开这种链接");
      }
      window.open(parsed.href, "_blank", "noopener,noreferrer");
      return true;
    })();
  },

  async listDrafts() {
    if (isTauri) return desktopInvoke("list_drafts");
    return webOnly(() => structuredClone(webDrafts))();
  },

  async saveDraft(request, draftId = null, expectedLocalVersion = null) {
    if (isTauri) {
      return desktopInvoke("save_draft", {
        request,
        draftId,
        expectedLocalVersion,
      });
    }
    return webOnly(() =>
      upsertMockDraft(request, draftId, expectedLocalVersion),
    )();
  },

  async deleteDraft(draftId, expectedLocalVersion) {
    if (isTauri) {
      return desktopInvoke("delete_draft", { draftId, expectedLocalVersion });
    }
    return webOnly(() => {
      const existing = webDrafts.find((draft) => draft.id === draftId);
      if (!existing || existing.local_version !== expectedLocalVersion) {
        return { kind: "stale" };
      }
      webDrafts = webDrafts.filter((draft) => draft.id !== draftId);
      return { kind: "deleted" };
    })();
  },

  async syncDrafts() {
    if (isTauri) return desktopInvoke("sync_drafts");
    return webOnly(() => ({ synced: webDrafts.length }))();
  },

  async syncSent() {
    if (isTauri) return desktopInvoke("sync_sent");
    return webOnly(() => ({
      mailbox: "Sent",
      remote_total: 0,
      fetched: 0,
      updated_flags: 0,
      removed: 0,
      cached_total: 0,
      uid_validity_reset: false,
    }))();
  },

  async syncAll() {
    if (isTauri) return desktopInvoke("sync_all");
    return webOnly(() => ({
      inbox: {
        mailbox: "INBOX",
        remote_total: webMessages.length,
        fetched: 0,
        updated_flags: 0,
        removed: 0,
        cached_total: webMessages.length,
        uid_validity_reset: false,
      },
      drafts_synced: webDrafts.length,
    }))();
  },

  async completeExit(requestId) {
    if (isTauri) {
      const completed = await desktopInvoke("complete_exit", { requestId });
      if (completed !== true) {
        throw new Error("退出请求已失效，桌面后端未确认退出。");
      }
      return true;
    }
    return webOnly(() => true)();
  },

  async cancelExit(requestId) {
    if (isTauri) {
      const cancelled = await desktopInvoke("cancel_exit", { requestId });
      if (cancelled !== true) {
        throw new Error("退出请求已失效，桌面后端未确认取消退出。");
      }
      return true;
    }
    return webOnly(() => true)();
  },

  async listOutbox() {
    if (isTauri) return desktopInvoke("list_outbox");
    return webOnly(() => structuredClone(webOutbox))();
  },

  async fetchOutboxMessage(outboxId) {
    if (isTauri) return desktopInvoke("fetch_outbox_message", { outboxId });
    return webOnly(() => {
      const item = webOutbox.find((candidate) => candidate.id === outboxId);
      if (!item) throw new Error("发件队列中的邮件不存在。");
      const draft = webDrafts.find((candidate) => candidate.id === item.draft_id);
      return structuredClone({
        id: item.id,
        subject: item.subject || draft?.subject || "",
        body_text: item.body_text ?? draft?.body_text ?? "",
        body_fetched: true,
      });
    })();
  },

  async getAccountMailboxSnapshot(accountId, limit = 50) {
    if (isTauri) {
      return desktopInvoke("get_account_mailbox_snapshot", { accountId, limit });
    }
    return webOnly(() => ({
      account_id: accountId,
      inbox: structuredClone(webMessages.slice(0, limit)),
      sent: [],
      drafts: structuredClone(webDrafts),
      outbox: structuredClone(webOutbox),
    }))();
  },

  async retryOutbox(outboxId) {
    if (isTauri) return desktopInvoke("retry_outbox", { outboxId });
    return webOnly(() => {
      const item = webOutbox.find((candidate) => candidate.id === outboxId);
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
      webOutbox = [sent, ...webOutbox.filter((candidate) => candidate.id !== outboxId)];
      return structuredClone(sent);
    })();
  },

  async sendDraft(draftId, expectedLocalVersion, confirmedRecipients) {
    if (isTauri) {
      return desktopInvoke("send_draft", {
        draftId,
        expectedLocalVersion,
        confirmedRecipients,
      });
    }
    return webOnly(() => {
      const draft = webDrafts.find((item) => item.id === draftId);
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
      webOutbox = [result, ...webOutbox];
      webDrafts = webDrafts.map((item) =>
        item.id === draftId ? { ...item, status: "sent" } : item,
      );
      return structuredClone(result);
    })();
  },

  async checkConnections() {
    if (isTauri) return desktopInvoke("check_connections");
    return webOnly(() => ({ imap_ok: true, smtp_ok: true }))();
  },

  async getDesktopSettings() {
    if (isTauri) {
      return normalizeSettings(await desktopInvoke("get_desktop_settings"));
    }
    return webOnly(() => structuredClone(webSettings))();
  },

  async updateDesktopSettings(settings) {
    const normalized = normalizeSettings(settings);
    if (isTauri) {
      const updated = await desktopInvoke("update_desktop_settings", {
        settings: settingsDto(normalized),
      });
      return normalizeSettings(updated || normalized);
    }
    return webOnly(() => {
      webSettings = normalized;
      return structuredClone(webSettings);
    })();
  },

  async getNewMailNotification() {
    if (isTauri) return desktopInvoke("get_new_mail_notification");
    return webOnly(() => null)();
  },

  async dismissNewMailNotification(notificationId) {
    if (isTauri) {
      return desktopInvoke("dismiss_new_mail_notification", { notificationId });
    }
    return webOnly(() => true)();
  },

  async openNewMailNotification(notificationId, uid, accountId) {
    if (isTauri) {
      return desktopInvoke("open_new_mail_notification", {
        notificationId,
        uid,
        accountId,
      });
    }
    return webOnly(() => true)();
  },

  async listAccountPresets() {
    if (isTauri) return desktopInvoke("list_account_presets");
    return webOnly(() => structuredClone(webAccountPresets))();
  },

  async getAccountStatus() {
    if (isTauri) {
      return normalizeAccountStatus(await desktopInvoke("get_account_status"));
    }
    return webOnly(() => structuredClone(webAccountStatus))();
  },

  async configureAccount(request) {
    if (isTauri) {
      return normalizeAccountStatus(
        await desktopInvoke("configure_account", { request }),
      );
    }
    return webOnly(() => {
      webAccountStatus = {
        configured: true,
        provider: request.provider,
        email: request.email,
        backendReady: true,
        credentialAvailable: true,
        networkReady: true,
        startupError: null,
      };
      return structuredClone(webAccountStatus);
    })();
  },

  async connectGoogleAccount() {
    if (isTauri) {
      return normalizeAccountStatus(await desktopInvoke("connect_google_account"));
    }
    return webOnly(() => structuredClone(webAccountStatus))();
  },

  async switchAccount(accountId) {
    if (isTauri) {
      return normalizeAccountStatus(
        await desktopInvoke("switch_account", { accountId }),
      );
    }
    return webOnly(() => {
      const selected = webAccountStatus.accounts.find(
        (account) => account.accountId === accountId,
      );
      if (selected) {
        webAccountStatus = {
          ...webAccountStatus,
          ...selected,
          activeAccountId: selected.accountId,
        };
      }
      return structuredClone(webAccountStatus);
    })();
  },

  async removeAccount(accountId) {
    if (isTauri) {
      return normalizeAccountStatus(
        await desktopInvoke("remove_account", { accountId }),
      );
    }
    return webOnly(() => {
      const accounts = webAccountStatus.accounts.filter(
        (account) => account.accountId !== accountId,
      );
      const selected = accounts[0] ?? {};
      webAccountStatus = {
        ...webAccountStatus,
        ...selected,
        configured: accounts.length > 0,
        accounts,
        accountCount: accounts.length,
        activeAccountId: selected.accountId ?? null,
      };
      return structuredClone(webAccountStatus);
    })();
  },

  async listProfileAvatars() {
    if (isTauri) {
      const avatars = await desktopInvoke("list_profile_avatars");
      return avatars.map(normalizeProfileAvatar);
    }
    return webOnly(() => structuredClone(webProfileAvatars))();
  },

  async saveProfileAvatar(request) {
    if (isTauri) {
      return normalizeProfileAvatar(
        await desktopInvoke("save_profile_avatar", {
          request: profileAvatarRequest(request),
        }),
      );
    }
    return webOnly(() => {
      const normalized = normalizeProfileAvatar({
        ...request,
        imageDataUrl: request.imageDataUrl,
      });
      webProfileAvatars = webProfileAvatars.filter(
        (avatar) =>
          avatar.ownerType !== normalized.ownerType || avatar.ownerKey !== normalized.ownerKey,
      );
      webProfileAvatars.push(normalized);
      return structuredClone(normalized);
    })();
  },

  async deleteProfileAvatar(request) {
    if (isTauri) {
      await desktopInvoke("delete_profile_avatar", {
        request: profileAvatarRequest(request),
      });
      return;
    }
    return webOnly(() => {
      const ownerKey = request.ownerKey.trim().toLowerCase();
      webProfileAvatars = webProfileAvatars.filter(
        (avatar) => avatar.ownerType !== request.ownerType || avatar.ownerKey !== ownerKey,
      );
    })();
  },

  async listContacts(accountId) {
    if (isTauri) {
      const contacts = await desktopInvoke("list_contacts", { accountId });
      return contacts.map(normalizeContact);
    }
    return webOnly(() => webContactItems().map(normalizeContact))();
  },

  async listContactMessages(accountId, email, limit = 250) {
    if (isTauri) {
      return desktopInvoke("list_contact_messages", {
        accountId,
        email,
        limit,
      });
    }
    return webOnly(() => {
      const target = normalizeContactEmail(email);
      const accountEmail = normalizeContactEmail(webAccountStatus.email || "");
      return structuredClone(
        webMessages
          .filter((message) => {
            const sender = normalizeContactEmail(message.sender?.email || "");
            if (sender === target) return true;
            if (sender !== accountEmail) return false;
            return [...(message.to || []), ...(message.cc || [])].some(
              (recipient) => normalizeContactEmail(recipient.email || "") === target,
            );
          })
          .slice(0, limit)
          .map((message) => ({
            ...message,
            direction:
              normalizeContactEmail(message.sender?.email || "") === accountEmail
                ? "outgoing"
                : "incoming",
          })),
      );
    })();
  },

  async setContactFavorite(email, favorite) {
    if (isTauri) {
      return desktopInvoke("set_contact_favorite", { email, favorite });
    }
    return webOnly(() => {
      const normalizedEmail = normalizeContactEmail(email || "");
      if (!normalizedEmail) throw new Error("联系人邮箱不能为空");
      if (favorite) webFavoriteContacts.add(normalizedEmail);
      else webFavoriteContacts.delete(normalizedEmail);
      return true;
    })();
  },

  async setContactRemark(email, remark) {
    if (isTauri) {
      return desktopInvoke("set_contact_remark", { email, remark });
    }
    return webOnly(() => {
      const normalizedEmail = normalizeContactEmail(email || "");
      if (!normalizedEmail) throw new Error("联系人邮箱不能为空");
      const normalizedRemark = (remark || "").trim();
      if ([...normalizedRemark].length > 80) throw new Error("联系人备注最多 80 个字符");
      if (/\p{Cc}/u.test(normalizedRemark)) throw new Error("联系人备注不能包含控制字符");
      if (normalizedRemark) webContactRemarks.set(normalizedEmail, normalizedRemark);
      else webContactRemarks.delete(normalizedEmail);
      return true;
    })();
  },

  async onMailEvent(eventName, handler) {
    if (!isTauri) return webOnly(() => () => {})();
    try {
      return await listen(eventName, handler);
    } catch (error) {
      throw commandError(error);
    }
  },
};

export const __testing = {
  resolveRuntime,
  normalizeSettings,
  normalizeAccountStatus,
  normalizeProfileAvatar,
  normalizeContact,
};
