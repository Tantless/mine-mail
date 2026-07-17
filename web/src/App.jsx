import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { emptyCompose } from "./data/mockMail.js";
import {
  isTauri,
  isTauriRuntime,
  isUnsupportedRuntime,
  mailApi,
} from "./services/mailApi.js";
import { WindowTitlebar } from "./components/WindowTitlebar.jsx";
import { Sidebar } from "./components/Sidebar.jsx";
import { MailList } from "./components/MailList.jsx";
import { MessageView } from "./components/MessageView.jsx";
import { ComposePanel } from "./components/ComposePanel.jsx";
import { SendConfirmDialog } from "./components/SendConfirmDialog.jsx";
import { SettingsPanel } from "./components/SettingsPanel.jsx";
import { AccountSetupPanel } from "./components/AccountSetup.jsx";
import { Toast } from "./components/Toast.jsx";
import { normalizeAvatarEmail } from "./components/ProfileAvatar.jsx";
import { hasFlag } from "./utils/formatters.js";

const folderLabels = {
  inbox: "收件箱",
  starred: "已加星标",
  sent: "已发送",
  drafts: "草稿",
  outbox: "发件队列",
  archive: "归档",
  trash: "垃圾箱",
};

const validThemes = new Set(["daylight", "night", "dusk", "forest"]);
const defaultSettings = {
  pollingIntervalMinutes: 5,
  autostartEnabled: false,
  notificationsEnabled: true,
  foregroundNotificationsEnabled: true,
  notificationSoundEnabled: true,
  notificationSound: "mail",
  remoteImageMode: "automatic",
};
const supportedAvatarTypes = new Set(["image/png", "image/jpeg", "image/webp"]);
const maxAvatarBytes = 2 * 1024 * 1024;

function readFileAsDataUrl(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => resolve(reader.result));
    reader.addEventListener("error", () => reject(new Error("无法读取所选图片")));
    reader.readAsDataURL(file);
  });
}
const localDraftDebounceMs = 900;

const cachedBodyFields = [
  "body_text",
  "body_html",
  "body_render_mode",
  "body_segments",
  "body_html_available",
  "body_html_loaded",
  "has_remote_images",
  "attachment_names",
  "body_fetched",
];

function messageCacheKey(message, accountId = "unscoped") {
  return `${accountId}:${message?.mailbox || "INBOX"}:${message?.uid}`;
}

function bodySnapshot(message) {
  return Object.fromEntries(
    cachedBodyFields.map((field) => [field, message?.[field]]),
  );
}

function getInitialTheme() {
  const saved = window.localStorage.getItem("mine-mail-theme");
  return validThemes.has(saved) ? saved : "daylight";
}

function describeError(error, fallback) {
  if (typeof error === "string" && error.trim()) return error;
  if (error?.message) return error.message;
  return fallback;
}

function toDraftMessage(draft, index) {
  return {
    id: draft.id,
    uid: `draft-${draft.id}`,
    kind: "draft",
    subject: draft.subject || "（无主题草稿）",
    sender: { name: "草稿", email: "" },
    to: (draft.to || []).map((email) => ({ name: null, email })),
    sent_at: draft.updated_at,
    flags: ["\\Seen"],
    preview: draft.body_text || "空白草稿",
    body_text: draft.body_text,
    attachment_names: [],
    body_fetched: true,
    draft,
    sortIndex: index,
  };
}

const outboxCopy = {
  queued: "等待发送",
  sending: "正在发送",
  sent: "已发送",
  retryable: "等待处理",
  rejected: "服务器已拒绝",
  delivery_unknown: "投递结果未知",
};

function toOutboxMessage(item, drafts) {
  const draft = drafts.find((candidate) => candidate.id === item.draft_id);
  const status = outboxCopy[item.status] || item.status || "状态未知";
  const recipients = item.recipients || [];
  return {
    id: item.id,
    uid: `outbox-${item.id}`,
    kind: "outbox",
    subject: draft?.subject || status,
    sender: { name: "Mine Mail", email: status },
    to: recipients.map((email) => ({ name: null, email })),
    sent_at: item.sent_at || item.created_at,
    flags: ["\\Seen"],
    preview: `${status} · ${recipients.join(", ")}`,
    body_text: [
      `状态：${status}`,
      `收件人：${recipients.join(", ") || "未知"}`,
      item.last_error ? `说明：${item.last_error}` : null,
      item.status === "delivery_unknown"
        ? "请先到邮箱服务器确认投递结果，不要立即重复发送。"
        : null,
    ]
      .filter(Boolean)
      .join("\n\n"),
    attachment_names: [],
    body_fetched: true,
    outbox: item,
  };
}

function hasDraftContent(value) {
  return Boolean(
    value &&
      ([...value.to, ...value.cc, ...value.bcc].length ||
        value.subject.trim() ||
        value.body_text.trim()),
  );
}

function createComposer(value = emptyCompose, draftId = null, persistedDraft = null) {
  const readOnlyUnsupported = Boolean(persistedDraft?.has_unsupported_content);
  return {
    sessionId: crypto.randomUUID(),
    draftId,
    // Keep the session origin separate from draftId. A new composer can gain a
    // draftId through background autosave, but closing it should still remove
    // that session-created recovery draft. Existing drafts must never be
    // mistaken for those temporary drafts.
    openedDraftId: draftId,
    baseLocalVersion: persistedDraft?.local_version ?? null,
    persistedDraft,
    readOnlyUnsupported,
    value: structuredClone(value),
    dirty: false,
    revision: 0,
    saveStatus: readOnlyUnsupported ? "readonly" : draftId ? "saved" : "idle",
    locked: false,
  };
}

function draftToRequest(draft) {
  return {
    to: [...(draft?.to || [])],
    cc: [...(draft?.cc || [])],
    bcc: [...(draft?.bcc || [])],
    subject: draft?.subject || "",
    body_text: draft?.body_text || "",
  };
}

function upsertDraft(items, draft) {
  return [draft, ...items.filter((item) => item.id !== draft.id)];
}

export function App() {
  const [theme, setTheme] = useState(getInitialTheme);
  const [activeFolder, setActiveFolder] = useState("inbox");
  const [messages, setMessages] = useState([]);
  const [drafts, setDrafts] = useState([]);
  const [outbox, setOutbox] = useState([]);
  const [selectedUid, setSelectedUid] = useState(null);
  const [selectedMessage, setSelectedMessage] = useState(null);
  const [isMessageLoading, setIsMessageLoading] = useState(false);
  const [messageError, setMessageError] = useState(null);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [syncState, setSyncState] = useState("idle");
  const [isThemeMenuOpen, setIsThemeMenuOpen] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(false);
  const [composer, setComposer] = useState(null);
  const [pendingSend, setPendingSend] = useState(null);
  const [isSending, setIsSending] = useState(false);
  const [retryingOutboxId, setRetryingOutboxId] = useState(null);
  const [settings, setSettings] = useState(defaultSettings);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [settingsFocusTarget, setSettingsFocusTarget] = useState(null);
  const [settingsSaveStatus, setSettingsSaveStatus] = useState("idle");
  const [accountPresets, setAccountPresets] = useState([]);
  const [accountStatus, setAccountStatus] = useState({ configured: null });
  const [accountSubmitStatus, setAccountSubmitStatus] = useState("idle");
  const [accountError, setAccountError] = useState(null);
  const [profileAvatars, setProfileAvatars] = useState([]);
  const [toast, setToast] = useState(null);

  const composerRef = useRef(null);
  const draftSaveRef = useRef(null);
  const exitFlushRef = useRef(null);
  const networkActionsAvailableRef = useRef(false);
  const draftsRef = useRef([]);
  const selectionRequestRef = useRef(0);
  const selectedUidRef = useRef(null);
  const messageBodyCacheRef = useRef(new Map());
  const accountViewsRef = useRef(new Map());
  const accountViewLoadsRef = useRef(new Map());
  const activeAccountIdRef = useRef(null);
  const accountSwitchRequestRef = useRef(0);
  const platform = /Mac|iPhone|iPad/.test(navigator.platform) ? "mac" : "windows";
  const networkActionsAvailable = Boolean(
    accountStatus.configured &&
      accountStatus.backendReady &&
      accountStatus.credentialAvailable &&
      accountStatus.networkReady !== false,
  );
  networkActionsAvailableRef.current = networkActionsAvailable;
  draftsRef.current = drafts;
  activeAccountIdRef.current =
    accountStatus.activeAccountId || accountStatus.accountId || null;

  useEffect(() => {
    const accountId = activeAccountIdRef.current;
    if (!accountId) return;
    accountViewsRef.current.set(accountId, {
      messages,
      drafts,
      outbox,
      selectedUid,
      selectedMessage,
    });
  }, [drafts, messages, outbox, selectedMessage, selectedUid]);

  const showToast = useCallback((message, tone = "success", persistent = false) => {
    setToast({ message, tone, persistent, id: Date.now() });
  }, []);

  const profileAvatarMap = useMemo(
    () =>
      new Map(
        profileAvatars.map((avatar) => [
          `${avatar.ownerType}:${normalizeAvatarEmail(avatar.ownerKey)}`,
          avatar.imageDataUrl,
        ]),
      ),
    [profileAvatars],
  );

  const profileAvatarFor = useCallback(
    (ownerType, email) =>
      email
        ? profileAvatarMap.get(`${ownerType}:${normalizeAvatarEmail(email)}`) || null
        : null,
    [profileAvatarMap],
  );

  const handleSaveProfileAvatar = useCallback(
    async (ownerType, email, file) => {
      if (!email) return;
      if (!supportedAvatarTypes.has(file.type)) {
        showToast("请选择 PNG、JPEG 或 WebP 图片", "error");
        return;
      }
      if (!file.size || file.size > maxAvatarBytes) {
        showToast("头像图片不能超过 2 MB", "error");
        return;
      }
      try {
        const [buffer, imageDataUrl] = await Promise.all([
          file.arrayBuffer(),
          readFileAsDataUrl(file),
        ]);
        const saved = await mailApi.saveProfileAvatar({
          ownerType,
          ownerKey: normalizeAvatarEmail(email),
          imageBytes: Array.from(new Uint8Array(buffer)),
          imageDataUrl,
        });
        setProfileAvatars((current) => [
          ...current.filter(
            (avatar) =>
              avatar.ownerType !== saved.ownerType || avatar.ownerKey !== saved.ownerKey,
          ),
          saved,
        ]);
        showToast(ownerType === "account" ? "Mine Mail 头像已更新" : "联系人头像已更新");
      } catch (error) {
        showToast(describeError(error, "头像没有保存，请重试"), "error");
      }
    },
    [showToast],
  );

  const handleDeleteProfileAvatar = useCallback(
    async (ownerType, email) => {
      if (!email) return;
      const ownerKey = normalizeAvatarEmail(email);
      try {
        await mailApi.deleteProfileAvatar({ ownerType, ownerKey });
        setProfileAvatars((current) =>
          current.filter(
            (avatar) => avatar.ownerType !== ownerType || avatar.ownerKey !== ownerKey,
          ),
        );
        showToast(ownerType === "account" ? "已恢复默认账户头像" : "已恢复默认联系人头像");
      } catch (error) {
        showToast(describeError(error, "头像没有移除，请重试"), "error");
      }
    },
    [showToast],
  );

  const commitComposer = useCallback((valueOrUpdater) => {
    const next =
      typeof valueOrUpdater === "function"
        ? valueOrUpdater(composerRef.current)
        : valueOrUpdater;
    composerRef.current = next;
    setComposer(next);
    return next;
  }, []);

  const openComposer = useCallback(
    (value = emptyCompose, draftId = null, persistedDraft = null) => {
      setPendingSend(null);
      commitComposer(createComposer(value, draftId, persistedDraft));
    },
    [commitComposer],
  );

  const clearSelection = useCallback(() => {
    selectionRequestRef.current += 1;
    selectedUidRef.current = null;
    setSelectedUid(null);
    setSelectedMessage(null);
    setMessageError(null);
    setIsMessageLoading(false);
  }, []);

  const handleSelect = useCallback(
    async (message, forceFetch = false) => {
      if (message.kind === "draft") {
        openComposer(
          {
            to: message.draft.to || [],
            cc: message.draft.cc || [],
            bcc: message.draft.bcc || [],
            subject: message.draft.subject || "",
            body_text: message.draft.body_text || "",
          },
          message.draft.id,
          message.draft,
        );
        return;
      }

      const accountId = activeAccountIdRef.current || "unscoped";
      const cachedBody = messageBodyCacheRef.current.get(
        messageCacheKey(message, accountId),
      );
      const displayMessage = cachedBody ? { ...message, ...cachedBody } : message;
      const requestId = selectionRequestRef.current + 1;
      selectionRequestRef.current = requestId;
      selectedUidRef.current = message.uid;
      setSelectedUid(message.uid);
      setSelectedMessage(displayMessage);
      setMessageError(null);

      const needsHtmlHydration =
        displayMessage.body_html_available === true &&
        displayMessage.body_html_loaded !== true;
      if (
        displayMessage.kind === "outbox" ||
        (!forceFetch && displayMessage.body_fetched && !needsHtmlHydration)
      ) {
        setIsMessageLoading(false);
        return;
      }

      if (!networkActionsAvailableRef.current && !displayMessage.body_fetched) {
        setIsMessageLoading(false);
        setMessageError(
          "这封邮件的正文尚未下载。重新连接账户后即可获取。",
        );
        return;
      }

      // The list response always contains either locally cached text or a
      // metadata preview. Paint that immediately while the full body is
      // hydrated in the background instead of replacing it with a skeleton.
      const hasImmediateCopy = Boolean(
        displayMessage.body_html ||
          displayMessage.body_text?.trim() ||
          displayMessage.preview?.trim(),
      );
      setIsMessageLoading(!hasImmediateCopy);
      try {
        const fullMessage = await mailApi.fetchMessage(message.uid);
        if (
          !fullMessage ||
          selectionRequestRef.current !== requestId ||
          selectedUidRef.current !== message.uid
        ) {
          return;
        }
        messageBodyCacheRef.current.set(
          messageCacheKey(fullMessage, accountId),
          bodySnapshot(fullMessage),
        );
        setSelectedMessage(fullMessage);
        setMessages((current) =>
          current.map((mail) =>
            mail.uid === fullMessage.uid ? fullMessage : mail,
          ),
        );
      } catch (error) {
        if (selectionRequestRef.current === requestId) {
          const messageText = describeError(error, "邮件正文加载失败");
          setMessageError(messageText);
          showToast(messageText, "error");
        }
      } finally {
        if (selectionRequestRef.current === requestId) setIsMessageLoading(false);
      }
    },
    [openComposer, showToast],
  );

  const refreshInbox = useCallback(
    async ({ selectFirst = false } = {}) => {
      const accountId = activeAccountIdRef.current || "unscoped";
      const summaries = await mailApi.listInbox(50);
      const inbox = summaries.map((message) => {
        const cachedBody = messageBodyCacheRef.current.get(
          messageCacheKey(message, accountId),
        );
        return cachedBody ? { ...message, ...cachedBody } : message;
      });
      const existingView = accountViewsRef.current.get(accountId) || {};
      accountViewsRef.current.set(accountId, { ...existingView, messages: inbox });
      if (activeAccountIdRef.current !== accountId) return inbox;
      setMessages(inbox);
      const currentUid = selectedUidRef.current;
      if (currentUid !== null) {
        const current = inbox.find((message) => message.uid === currentUid);
        if (current) {
          setSelectedMessage((previous) => {
            if (!previous || previous.uid !== currentUid) return current;
            const preservedBody = bodySnapshot(previous);
            messageBodyCacheRef.current.set(
              messageCacheKey(current, accountId),
              preservedBody,
            );
            return { ...previous, ...current, ...preservedBody };
          });
        }
        // listInbox is deliberately bounded. A selected message can fall just
        // outside that window when a new message arrives, so absence from this
        // refresh is not proof that it was deleted. Keep the reader stable.
      } else if (selectFirst && inbox.length && window.innerWidth >= 720) {
        void handleSelect(inbox[0]);
      }
      return inbox;
    },
    [handleSelect],
  );

  const refreshDrafts = useCallback(async () => {
    const accountId = activeAccountIdRef.current || "unscoped";
    const localDrafts = await mailApi.listDrafts();
    const existingView = accountViewsRef.current.get(accountId) || {};
    accountViewsRef.current.set(accountId, {
      ...existingView,
      drafts: localDrafts,
    });
    if (activeAccountIdRef.current !== accountId) return localDrafts;
    draftsRef.current = localDrafts;
    setDrafts(localDrafts);
    const current = composerRef.current;
    if (
      current?.draftId &&
      !current.dirty &&
      !current.locked &&
      !draftSaveRef.current
    ) {
      const canonical = localDrafts.find(
        (draft) => draft.id === current.draftId,
      );
      if (!canonical) {
        commitComposer(null);
        showToast("草稿已在其他客户端删除，编辑器已关闭", "error", true);
      } else if (canonical.local_version !== current.baseLocalVersion) {
        commitComposer({
          ...current,
          value: draftToRequest(canonical),
          baseLocalVersion: canonical.local_version,
          persistedDraft: canonical,
          readOnlyUnsupported: Boolean(canonical.has_unsupported_content),
          saveStatus: canonical.has_unsupported_content ? "readonly" : "saved",
        });
        showToast("草稿已更新为其他客户端的最新版本");
      }
    }
    return localDrafts;
  }, [commitComposer, showToast]);

  const refreshOutbox = useCallback(async () => {
    const accountId = activeAccountIdRef.current || "unscoped";
    const items = await mailApi.listOutbox();
    const existingView = accountViewsRef.current.get(accountId) || {};
    accountViewsRef.current.set(accountId, { ...existingView, outbox: items });
    if (activeAccountIdRef.current !== accountId) return items;
    setOutbox(items);
    setSelectedMessage((current) => {
      if (current?.kind !== "outbox") return current;
      const freshItem = items.find((item) => item.id === current.outbox?.id);
      return freshItem ? toOutboxMessage(freshItem, draftsRef.current) : current;
    });
    return items;
  }, []);

  const cacheMailboxSnapshot = useCallback((accountId, snapshot) => {
    const previous = accountViewsRef.current.get(accountId) || {};
    const inbox = (snapshot?.inbox || []).map((message) => {
      const cachedBody = messageBodyCacheRef.current.get(
        messageCacheKey(message, accountId),
      );
      return cachedBody ? { ...message, ...cachedBody } : message;
    });
    const selectedUid = previous.selectedUid ?? null;
    const selectedMessage = selectedUid
      ? inbox.find((message) => message.uid === selectedUid) || previous.selectedMessage || null
      : null;
    const view = {
      messages: inbox,
      drafts: snapshot?.drafts || [],
      outbox: snapshot?.outbox || [],
      selectedUid,
      selectedMessage,
    };
    accountViewsRef.current.set(accountId, view);
    return view;
  }, []);

  const loadAccountView = useCallback(
    (accountId, { force = false } = {}) => {
      if (!force && accountViewsRef.current.has(accountId)) {
        return Promise.resolve(accountViewsRef.current.get(accountId));
      }
      if (accountViewLoadsRef.current.has(accountId)) {
        return accountViewLoadsRef.current.get(accountId);
      }
      const operation = mailApi
        .getAccountMailboxSnapshot(accountId, 50)
        .then((snapshot) => cacheMailboxSnapshot(accountId, snapshot))
        .finally(() => {
          if (accountViewLoadsRef.current.get(accountId) === operation) {
            accountViewLoadsRef.current.delete(accountId);
          }
        });
      accountViewLoadsRef.current.set(accountId, operation);
      return operation;
    },
    [cacheMailboxSnapshot],
  );

  const prefetchAccountViews = useCallback(
    async (status) => {
      const accounts = status?.accounts || [];
      return Promise.allSettled(
        accounts.map((account) => loadAccountView(account.accountId)),
      );
    },
    [loadAccountView],
  );

  const loadMailboxData = useCallback(
    async ({ selectFirst = false, accountId = activeAccountIdRef.current } = {}) => {
      if (!accountId) return null;
      try {
        const view = await loadAccountView(accountId, { force: true });
        if (activeAccountIdRef.current !== accountId) return view;
        setMessages(view.messages);
        draftsRef.current = view.drafts;
        setDrafts(view.drafts);
        setOutbox(view.outbox);
        if (
          selectFirst &&
          view.selectedUid === null &&
          view.messages.length &&
          window.innerWidth >= 720
        ) {
          void handleSelect(view.messages[0]);
        }
        return view;
      } catch (error) {
        if (activeAccountIdRef.current === accountId) {
          showToast("部分本地邮箱数据没有加载完成", "error");
        }
        throw error;
      }
    },
    [handleSelect, loadAccountView, showToast],
  );

  const restoreAccountView = useCallback(
    (accountId, view, { selectFirst = true } = {}) => {
      const restored = view || {
        messages: [],
        drafts: [],
        outbox: [],
        selectedUid: null,
        selectedMessage: null,
      };
      activeAccountIdRef.current = accountId;
      selectionRequestRef.current += 1;
      setMessages(restored.messages);
      draftsRef.current = restored.drafts;
      setDrafts(restored.drafts);
      setOutbox(restored.outbox);
      selectedUidRef.current = restored.selectedUid;
      setSelectedUid(restored.selectedUid);
      setSelectedMessage(restored.selectedMessage);
      setMessageError(null);
      setIsMessageLoading(false);
      if (
        selectFirst &&
        restored.selectedUid === null &&
        restored.messages.length &&
        window.innerWidth >= 720
      ) {
        void handleSelect(restored.messages[0]);
      }
      return restored;
    },
    [handleSelect],
  );

  useEffect(() => {
    if (isUnsupportedRuntime) return undefined;
    let cancelled = false;
    const load = async () => {
      const settingsTask = mailApi
        .getDesktopSettings()
        .then((value) => {
          if (cancelled) return;
          setSettings(value);
          if (value.startupError) showToast(value.startupError, "error", true);
        })
        .catch((error) => {
          if (!cancelled) showToast(describeError(error, "桌面设置读取失败"), "error");
        });
      const presetsTask = mailApi
        .listAccountPresets()
        .then((value) => !cancelled && setAccountPresets(value))
        .catch((error) => {
          if (!cancelled) showToast(describeError(error, "账户预设读取失败"), "error");
        });
      const avatarsTask = mailApi
        .listProfileAvatars()
        .then((value) => !cancelled && setProfileAvatars(value))
        .catch((error) => {
          if (!cancelled) showToast(describeError(error, "本地头像读取失败"), "error");
        });

      try {
        const status = await mailApi.getAccountStatus();
        if (cancelled) return;
        const activeAccountId = status.activeAccountId || status.accountId || null;
        activeAccountIdRef.current = activeAccountId;
        setAccountStatus(status);
        void prefetchAccountViews(status);
        const backendUsable = status.configured && status.backendReady;
        if (backendUsable) {
          const networkUsable =
            status.credentialAvailable && status.networkReady !== false;
          if (!networkUsable) {
            setAccountError(
              status.startupError ||
                "本地邮件仍可阅读，但账户凭据或网络连接不可用。请重新连接账户后再同步或发送。",
            );
          }
          void loadMailboxData({
            accountId: activeAccountId,
            selectFirst: true,
          });
        } else {
          setAccountError(
            status.startupError ||
              (status.configured && !status.credentialAvailable
                ? "账户信息存在，但系统凭据不可用，请重新输入授权信息。"
                : null),
          );
        }
      } catch (error) {
        if (cancelled) return;
        setAccountStatus({ configured: false, provider: null, email: null });
        setAccountError(describeError(error, "无法读取账户配置"));
      }

      await Promise.allSettled([settingsTask, presetsTask, avatarsTask]);
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [loadMailboxData, prefetchAccountViews]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    window.localStorage.setItem("mine-mail-theme", theme);
  }, [theme]);

  useEffect(() => {
    if (!toast || toast.persistent) return undefined;
    const timer = window.setTimeout(() => setToast(null), 3800);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const saveDraftNow = useCallback(
    async ({ force = false } = {}) => {
      const initial = composerRef.current;
      if (!initial) return null;
      if (initial.readOnlyUnsupported) return initial.persistedDraft;
      const sessionId = initial.sessionId;
      let mustPersist = force;

      // A forced save is a stabilization barrier: if the editor changed while a
      // previous write was in flight, keep writing snapshots until the saved
      // revision exactly matches the locked editor revision.
      while (true) {
        if (draftSaveRef.current) {
          await draftSaveRef.current;
          if (composerRef.current?.sessionId !== sessionId) return null;
          continue;
        }

        const current = composerRef.current;
        if (!current || current.sessionId !== sessionId) return null;
        const shouldPersist = current.dirty || mustPersist;
        if (!shouldPersist) return current.persistedDraft || null;

        // A brand-new, untouched empty editor is not a draft. Once a draft ID
        // exists, an empty snapshot is meaningful and must overwrite the old data.
        if (!current.draftId && !hasDraftContent(current.value)) return null;

        const snapshot = {
          sessionId,
          revision: current.revision,
          draftId: current.draftId,
          expectedLocalVersion: current.draftId
            ? current.baseLocalVersion
            : null,
          value: structuredClone(current.value),
        };
        commitComposer((latest) =>
          latest?.sessionId === sessionId
            ? { ...latest, saveStatus: "saving" }
            : latest,
        );

        const operation = mailApi
          .saveDraft(
            snapshot.value,
            snapshot.draftId,
            snapshot.expectedLocalVersion,
          )
          .then((outcome) => {
            const draft = outcome.draft;
            setDrafts((items) => {
              const withCanonical = outcome.canonical
                ? upsertDraft(items, outcome.canonical)
                : items;
              const nextDrafts = upsertDraft(withCanonical, draft);
              draftsRef.current = nextDrafts;
              return nextDrafts;
            });
            commitComposer((latest) => {
              if (!latest || latest.sessionId !== sessionId) return latest;
              const unchanged = latest.revision === snapshot.revision;
              return {
                ...latest,
                draftId: draft.id,
                baseLocalVersion: draft.local_version,
                persistedDraft: draft,
                dirty: !unchanged,
                saveStatus: unchanged ? "saved" : "dirty",
              };
            });
            if (outcome.kind === "conflict_copy") {
              showToast(
                "草稿已在其他客户端更新或删除。你的编辑已保留为新的本地冲突副本，未覆盖最新版本。",
                "error",
                true,
              );
            }
            return draft;
          })
          .catch((error) => {
            commitComposer((latest) =>
              latest?.sessionId === sessionId
                ? { ...latest, dirty: true, saveStatus: "error" }
                : latest,
            );
            throw error;
          });
        draftSaveRef.current = operation;

        let draft;
        try {
          draft = await operation;
        } finally {
          if (draftSaveRef.current === operation) draftSaveRef.current = null;
        }

        mustPersist = false;
        const latest = composerRef.current;
        const isStable =
          latest?.sessionId === sessionId && latest.revision === snapshot.revision;
        if (isStable || !force) return draft;
      }
    },
    [commitComposer, showToast],
  );

  useEffect(() => {
    if (!isTauri) return undefined;
    let cancelled = false;
    const disposers = [];
    const reportEventError = (error, fallback) => {
      if (!cancelled) showToast(describeError(error, fallback), "error");
    };
    const subscribe = async () => {
      try {
        const inboxUnlisten = await mailApi.onMailEvent(
          "mail:inbox-updated",
          () => {
            void refreshInbox().catch((error) =>
              reportEventError(error, "收件箱刷新失败"),
            );
          },
        );
        if (cancelled) inboxUnlisten();
        else disposers.push(inboxUnlisten);

        const openMessageUnlisten = await mailApi.onMailEvent(
          "mail:open-message",
          (event) => {
            const uid = Number(event?.payload?.uid);
            const targetAccountId =
              event?.payload?.accountId ?? event?.payload?.account_id;
            if (!Number.isInteger(uid) || uid <= 0) return;
            void mailApi
              .getAccountStatus()
              .then(async (currentStatus) => {
                let status = currentStatus;
                if (
                  targetAccountId &&
                  currentStatus.activeAccountId !== targetAccountId
                ) {
                  if (composerRef.current) {
                    throw new Error("请先关闭当前写信窗口，再打开其他账户的新邮件");
                  }
                  status = await mailApi.switchAccount(targetAccountId);
                }
                setAccountStatus(status);
                clearSelection();
                messageBodyCacheRef.current.clear();
                return refreshInbox();
              })
              .then((inbox) => {
                const message = inbox.find((item) => item.uid === uid);
                if (message) return handleSelect(message, true);
                throw new Error("这封新邮件暂时不在本地收件箱中");
              })
              .catch((error) =>
                reportEventError(error, "新邮件暂时无法打开"),
              );
          },
        );
        if (cancelled) openMessageUnlisten();
        else disposers.push(openMessageUnlisten);

        const draftsUnlisten = await mailApi.onMailEvent(
          "mail:drafts-updated",
          () => {
            void Promise.all([refreshDrafts(), refreshOutbox()]).catch((error) =>
              reportEventError(error, "草稿或发件队列刷新失败"),
            );
          },
        );
        if (cancelled) draftsUnlisten();
        else disposers.push(draftsUnlisten);

        const syncErrorUnlisten = await mailApi.onMailEvent(
          "mail:sync-error",
          (event) => {
            setSyncState("error");
            const message = event?.payload?.message || "邮箱同步失败";
            showToast(message, "error");
          },
        );
        if (cancelled) syncErrorUnlisten();
        else disposers.push(syncErrorUnlisten);

        const exitUnlisten = await mailApi.onMailEvent(
          "mail:before-exit",
          (event) => {
            const requestId =
              event?.payload?.requestId ?? event?.payload?.request_id;
            if (!requestId) {
              showToast("桌面退出请求缺少 requestId，已拒绝退出", "error", true);
              return;
            }
            if (exitFlushRef.current) return;

            const operation = (async () => {
              commitComposer((current) =>
                current ? { ...current, locked: true } : current,
              );
              try {
                await saveDraftNow({ force: true });
              } catch (error) {
                try {
                  const cancelledExit = await mailApi.cancelExit(requestId);
                  if (cancelledExit !== true) {
                    throw new Error("未能取消退出请求");
                  }
                } catch (cancelError) {
                  // The actionable failure remains the local save. Include the
                  // cancellation failure without replacing that root cause.
                  showToast(
                    `退出前保存草稿失败：${describeError(error, "本地保存失败")}；取消退出也失败：${describeError(cancelError, "应用暂时无响应")}`,
                    "error",
                    true,
                  );
                  return;
                } finally {
                  commitComposer((current) =>
                    current
                      ? { ...current, locked: false, saveStatus: "error" }
                      : current,
                  );
                  if (exitFlushRef.current === operation) {
                    exitFlushRef.current = null;
                  }
                }
                showToast(
                  `退出前保存草稿失败：${describeError(error, "本地保存失败")}。已取消退出，请处理后重试。`,
                  "error",
                  true,
                );
                return;
              }

              try {
                const completedExit = await mailApi.completeExit(requestId);
                if (completedExit !== true) {
                  throw new Error("未能完成退出请求");
                }
              } catch (error) {
                commitComposer((current) =>
                  current
                    ? {
                        ...current,
                        locked: false,
                        saveStatus: current.dirty ? "dirty" : "saved",
                      }
                    : current,
                );
                if (exitFlushRef.current === operation) {
                  exitFlushRef.current = null;
                }
                showToast(
                  `无法完成安全退出：${describeError(error, "应用暂时无响应")}。请再次尝试。`,
                  "error",
                  true,
                );
              }
            })();
            exitFlushRef.current = operation;
          },
        );
        if (cancelled) exitUnlisten();
        else disposers.push(exitUnlisten);
      } catch (error) {
        reportEventError(error, "桌面更新事件监听失败");
      }
    };
    void subscribe();
    return () => {
      cancelled = true;
      disposers.forEach((dispose) => dispose());
    };
  }, [
    commitComposer,
    handleSelect,
    refreshDrafts,
    refreshInbox,
    refreshOutbox,
    saveDraftNow,
    showToast,
  ]);

  useEffect(() => {
    if (
      !composer?.dirty ||
      composer.locked ||
      composer.saveStatus === "saving"
    ) {
      return undefined;
    }
    const sessionId = composer.sessionId;
    const timer = window.setTimeout(() => {
      if (
        composerRef.current?.sessionId !== sessionId ||
        composerRef.current?.locked
      ) {
        return;
      }
      void saveDraftNow().catch((error) => {
        showToast(describeError(error, "草稿自动保存失败"), "error");
      });
    }, localDraftDebounceMs);
    return () => window.clearTimeout(timer);
  }, [composer?.dirty, composer?.revision, composer?.saveStatus, composer?.sessionId, saveDraftNow, showToast]);

  useEffect(() => {
    const onKeyDown = (event) => {
      if (
        !composerRef.current &&
        !pendingSend &&
        event.key.toLowerCase() === "n" &&
        !event.metaKey &&
        !event.ctrlKey &&
        !["INPUT", "TEXTAREA"].includes(document.activeElement?.tagName)
      ) {
        event.preventDefault();
        openComposer();
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        document.querySelector(".search-box input")?.focus();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [openComposer, pendingSend]);

  const outboxMessages = useMemo(
    () => outbox.map((item) => toOutboxMessage(item, drafts)),
    [drafts, outbox],
  );

  const folderMessages = useMemo(() => {
    if (activeFolder === "inbox") return messages;
    if (activeFolder === "starred") {
      return messages.filter((message) => hasFlag(message, "\\Flagged"));
    }
    if (activeFolder === "drafts") {
      return drafts.filter((draft) => draft.status !== "sent").map(toDraftMessage);
    }
    if (activeFolder === "outbox") return outboxMessages;
    if (activeFolder === "sent") {
      return outboxMessages.filter((message) => message.outbox.status === "sent");
    }
    return [];
  }, [activeFolder, drafts, messages, outboxMessages]);

  const visibleMessages = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return folderMessages.filter((message) => {
      if (filter === "unread" && hasFlag(message, "\\Seen")) return false;
      if (filter === "starred" && !hasFlag(message, "\\Flagged")) return false;
      if (!normalizedQuery) return true;
      return [
        message.subject,
        message.preview,
        message.sender?.name,
        message.sender?.email,
      ].some((value) => value?.toLowerCase().includes(normalizedQuery));
    });
  }, [filter, folderMessages, query]);

  const selectedIndex = visibleMessages.findIndex(
    (message) => message.uid === selectedUid,
  );

  const folderCounts = useMemo(
    () => ({
      inbox: messages.filter((message) => !hasFlag(message, "\\Seen")).length,
      starred: messages.filter((message) => hasFlag(message, "\\Flagged")).length,
      drafts: drafts.filter((draft) => draft.status !== "sent").length,
      outbox: outbox.filter((item) => item.status !== "sent").length,
      sent: outbox.filter((item) => item.status === "sent").length,
    }),
    [drafts, messages, outbox],
  );

  const handleFolderChange = (folder) => {
    setActiveFolder(folder);
    setFilter("all");
    setQuery("");
    clearSelection();
    setIsSidebarOpen(false);
  };

  const handleSync = async () => {
    if (!networkActionsAvailable) {
      showToast("重新连接账户后才能同步邮箱", "error");
      return;
    }
    setSyncState("syncing");
    try {
      const report = await mailApi.syncAll();
      await Promise.all([refreshInbox(), refreshDrafts(), refreshOutbox()]);
      setSyncState("done");
      const fetched = report?.inbox?.fetched ?? report?.fetched ?? 0;
      showToast(fetched ? `同步完成，收到 ${fetched} 封新邮件` : "邮箱已是最新状态");
    } catch (error) {
      setSyncState("error");
      showToast(describeError(error, "同步失败，请检查网络"), "error");
    }
  };

  const handleComposeChange = (updater) => {
    commitComposer((current) => {
      if (!current || current.locked || current.readOnlyUnsupported) return current;
      const nextValue = typeof updater === "function" ? updater(current.value) : updater;
      return {
        ...current,
        value: nextValue,
        dirty: true,
        revision: current.revision + 1,
        saveStatus: "dirty",
      };
    });
  };

  const handleSaveDraftAndClose = async ({ syncRemote = true } = {}) => {
    commitComposer((current) =>
      current ? { ...current, locked: true } : current,
    );
    try {
      const draft = await saveDraftNow({ force: true });
      commitComposer(null);
      if (draft?.status !== "conflict") {
        showToast(draft ? "草稿已保存到本地" : "空白写信窗口已关闭");
      }

      if (syncRemote && draft && networkActionsAvailable) {
        void mailApi
          .syncDrafts()
          .then(() => refreshDrafts())
          .catch((error) => {
            showToast(
              `${describeError(error, "草稿远端同步失败")}；本地草稿已安全保存`,
              "error",
            );
          });
      }
    } catch (error) {
      commitComposer((current) =>
        current
          ? { ...current, locked: false, saveStatus: "error" }
          : current,
      );
      showToast(describeError(error, "草稿保存失败"), "error");
    }
  };

  const handleCloseComposer = async () => {
    const initial = composerRef.current;
    if (!initial || initial.locked) return;
    if (initial.readOnlyUnsupported) {
      commitComposer(null);
      return;
    }

    // Closing an existing draft means leaving the editor. It must not force a
    // final save, and it must not delete the draft that the user opened.
    if (initial.openedDraftId) {
      commitComposer(null);
      return;
    }

    // A new composer may already have produced a local recovery draft through
    // autosave. Mark the session as locked so no new timer can start, wait for
    // a write already in flight, then remove only the draft created by this
    // compose session.
    const sessionId = initial.sessionId;
    commitComposer((current) =>
      current?.sessionId === sessionId ? { ...current, locked: true } : current,
    );

    try {
      if (draftSaveRef.current) {
        try {
          await draftSaveRef.current;
        } catch {
          // A failed first autosave has nothing to retain. If an older recovery
          // snapshot exists, draftId below still identifies it for cleanup.
        }
      }

      const current = composerRef.current;
      if (!current || current.sessionId !== sessionId) return;
      if (!current.draftId) {
        commitComposer(null);
        return;
      }

      const outcome = await mailApi.deleteDraft(
        current.draftId,
        current.baseLocalVersion,
      );
      commitComposer(null);
      await refreshDrafts();
      if (outcome.kind === "stale") {
        showToast(
          "临时草稿已在其他客户端更新；已关闭当前编辑，没有删除较新的版本。",
          "error",
          true,
        );
      }
    } catch (error) {
      commitComposer((current) =>
        current?.sessionId === sessionId
          ? { ...current, locked: false, saveStatus: "error" }
          : current,
      );
      showToast(describeError(error, "临时草稿清理失败，写信窗口仍保持打开"), "error");
    }
  };

  const handleDiscardComposer = async () => {
    commitComposer((current) =>
      current ? { ...current, locked: true } : current,
    );
    try {
      if (draftSaveRef.current) await draftSaveRef.current;
      const current = composerRef.current;
      const draftId = current?.draftId;
      if (draftId) {
        const outcome = await mailApi.deleteDraft(
          draftId,
          current.baseLocalVersion,
        );
        commitComposer(null);
        await refreshDrafts();
        if (outcome.kind === "stale") {
          showToast(
            "草稿已在其他客户端更新；仅丢弃当前编辑，没有删除最新版本。",
            "error",
            true,
          );
          return;
        }
        showToast("草稿已删除");
        return;
      }
      commitComposer(null);
      showToast("未保存内容已丢弃");
    } catch (error) {
      commitComposer((current) =>
        current ? { ...current, locked: false } : current,
      );
      showToast(describeError(error, "草稿删除失败"), "error");
    }
  };

  const handleRequestSend = async () => {
    if (!networkActionsAvailable) {
      showToast("重新连接账户后才能发送邮件", "error");
      return;
    }
    commitComposer((current) =>
      current ? { ...current, locked: true } : current,
    );
    try {
      const draft = await saveDraftNow({ force: true });
      if (!draft?.id) throw new Error("请先保存草稿再发送。");
      setPendingSend({
        ...draftToRequest(draft),
        draftId: draft.id,
        expectedLocalVersion: draft.local_version,
      });
    } catch (error) {
      commitComposer((current) =>
        current ? { ...current, locked: false, saveStatus: "error" } : current,
      );
      showToast(describeError(error, "发送前保存草稿失败"), "error");
    }
  };

  const handleCancelSend = () => {
    setPendingSend(null);
    commitComposer((current) =>
      current
        ? {
            ...current,
            locked: false,
            saveStatus: current.dirty ? "dirty" : "saved",
          }
        : current,
    );
  };

  const handleConfirmSend = async () => {
    if (!pendingSend) return;
    setIsSending(true);
    try {
      const confirmedRecipients = [
        ...pendingSend.to,
        ...pendingSend.cc,
        ...pendingSend.bcc,
      ];
      const result = await mailApi.sendDraft(
        pendingSend.draftId,
        pendingSend.expectedLocalVersion,
        confirmedRecipients,
      );
      await Promise.all([refreshDrafts(), refreshOutbox()]);
      setPendingSend(null);
      commitComposer(null);

      if (result.status !== "sent") {
        const deliveryMessages = {
          retryable: "邮件保留在发件队列，请稍后查看状态",
          rejected: "服务器拒绝了这封邮件，请查看发件队列",
          delivery_unknown: "投递结果未知，请先到邮箱服务器确认，切勿立即重发",
        };
        showToast(
          deliveryMessages[result.status] || "邮件尚未发送，已保留在发件队列",
          "error",
          result.status === "delivery_unknown",
        );
        return;
      }
      showToast("邮件已经发送");
    } catch (error) {
      showToast(describeError(error, "邮件发送失败"), "error");
      setPendingSend(null);
      commitComposer((current) =>
        current ? { ...current, locked: false } : current,
      );
    } finally {
      setIsSending(false);
    }
  };

  const handleRetryOutbox = async (item) => {
    if (!item || item.status !== "retryable" || retryingOutboxId) return;
    if (!networkActionsAvailable) {
      showToast("重新连接账户后才能重试发送", "error");
      return;
    }
    setRetryingOutboxId(item.id);
    try {
      const result = await mailApi.retryOutbox(item.id);
      await Promise.all([refreshDrafts(), refreshOutbox()]);
      if (result.status === "sent") {
        showToast("邮件重试发送成功");
      } else {
        const message =
          result.status === "delivery_unknown"
            ? "重试后的投递结果未知，请先到邮箱服务器确认，切勿再次重发"
            : result.status === "rejected"
              ? "服务器拒绝了这封邮件"
              : "邮件仍未发出，已更新发件队列状态";
        showToast(message, "error", result.status === "delivery_unknown");
      }
    } catch (error) {
      showToast(describeError(error, "邮件重试失败"), "error");
    } finally {
      setRetryingOutboxId(null);
    }
  };

  const handleSaveSettings = async (nextSettings) => {
    setSettingsSaveStatus("saving");
    try {
      const updated = await mailApi.updateDesktopSettings(nextSettings);
      setSettings(updated);
      setSettingsSaveStatus("saved");
      setIsSettingsOpen(false);
      showToast("桌面设置已保存");
    } catch (error) {
      setSettingsSaveStatus("error");
      showToast(describeError(error, "桌面设置保存失败"), "error");
    }
  };

  const handleConfigureAccount = async (request) => {
    if (composerRef.current) {
      showToast("请先关闭当前写信窗口，再连接其他账户。", "error");
      return;
    }
    setAccountSubmitStatus("saving");
    setAccountError(null);
    try {
      const status = await mailApi.configureAccount(request);
      setAccountStatus(status);
      const backendUsable = status.configured && status.backendReady;
      if (!backendUsable) {
        const message =
          status.startupError || "账户信息已保存，但邮箱服务尚未就绪，请检查授权信息。";
        setAccountError(message);
        setAccountSubmitStatus("error");
        return;
      }

      clearSelection();
      setMessages([]);
      setDrafts([]);
      setOutbox([]);
      const networkUsable =
        status.credentialAvailable && status.networkReady !== false;
      await loadMailboxData({ selectFirst: true });
      if (!networkUsable) {
        setAccountError(
          status.startupError ||
            "本地邮箱已打开，但账户凭据或网络连接不可用。",
        );
      }
      setAccountSubmitStatus("saved");
      showToast("邮箱账户已安全连接");
    } catch (error) {
      const message = describeError(error, "账户配置失败，请检查地址和授权信息");
      setAccountError(message);
      setAccountSubmitStatus("error");
    }
  };

  const applyActiveAccount = async (status, successMessage) => {
    setAccountStatus(status);
    activeAccountIdRef.current = status.activeAccountId || status.accountId || null;
    clearSelection();
    messageBodyCacheRef.current.clear();
    setMessages([]);
    setDrafts([]);
    setOutbox([]);
    if (status.configured && status.backendReady) {
      await loadMailboxData({ selectFirst: true });
    }
    void prefetchAccountViews(status);
    setAccountSubmitStatus("saved");
    setAccountError(null);
    if (successMessage) showToast(successMessage);
  };

  const handleConnectGoogle = async () => {
    if (composerRef.current) {
      showToast("请先关闭当前写信窗口，再连接其他账户。", "error");
      return;
    }
    setAccountSubmitStatus("saving");
    setAccountError(null);
    try {
      const status = await mailApi.connectGoogleAccount();
      await applyActiveAccount(status, "Gmail 已通过 Google 安全连接");
    } catch (error) {
      const message = describeError(error, "Google 登录失败，请重试");
      setAccountError(message);
      setAccountSubmitStatus("error");
    }
  };

  const handleSwitchAccount = async (accountId) => {
    if (!accountId || accountId === accountStatus.activeAccountId) return;
    if (composerRef.current) {
      showToast("请先关闭当前写信窗口，再切换邮箱账户。", "error");
      return;
    }
    const previousStatus = accountStatus;
    const previousAccountId =
      accountStatus.activeAccountId || accountStatus.accountId || null;
    if (previousAccountId) {
      accountViewsRef.current.set(previousAccountId, {
        messages,
        drafts,
        outbox,
        selectedUid,
        selectedMessage,
      });
    }
    const targetAccount = accountStatus.accounts?.find(
      (account) => account.accountId === accountId,
    );
    if (!targetAccount) {
      showToast("邮箱账户不存在，请刷新账户列表", "error");
      return;
    }
    const optimisticStatus = {
      ...accountStatus,
      ...targetAccount,
      accountId: targetAccount.accountId,
      activeAccountId: targetAccount.accountId,
    };
    const requestId = accountSwitchRequestRef.current + 1;
    accountSwitchRequestRef.current = requestId;
    setAccountSubmitStatus("saving");

    let targetView = accountViewsRef.current.get(accountId);
    if (targetView) {
      setAccountStatus(optimisticStatus);
      restoreAccountView(accountId, targetView, { selectFirst: false });
    }
    try {
      const viewPromise = targetView
        ? Promise.resolve(targetView)
        : loadAccountView(accountId).catch(() => null);
      if (!targetView) {
        void viewPromise.then((loadedView) => {
          if (!loadedView || accountSwitchRequestRef.current !== requestId) return;
          setAccountStatus(optimisticStatus);
          restoreAccountView(accountId, loadedView, { selectFirst: false });
        });
      }
      const status = await mailApi.switchAccount(accountId);
      const loadedView = await viewPromise;
      if (accountSwitchRequestRef.current !== requestId) return;
      targetView = loadedView || accountViewsRef.current.get(accountId);
      setAccountStatus(status);
      if (activeAccountIdRef.current !== accountId || !targetView) {
        restoreAccountView(accountId, targetView, { selectFirst: false });
      }
      if (
        targetView?.selectedUid == null &&
        targetView?.messages.length &&
        window.innerWidth >= 720
      ) {
        void handleSelect(targetView.messages[0]);
      }
      setAccountSubmitStatus("saved");
      setAccountError(null);
      showToast(`已切换到 ${status.email}`);
      void loadMailboxData({
        accountId,
        selectFirst: false,
      }).catch(() => {});
    } catch (error) {
      if (accountSwitchRequestRef.current !== requestId) return;
      accountSwitchRequestRef.current += 1;
      setAccountStatus(previousStatus);
      if (previousAccountId) {
        restoreAccountView(previousAccountId, accountViewsRef.current.get(previousAccountId));
      }
      setAccountSubmitStatus("error");
      showToast(describeError(error, "邮箱账户切换失败"), "error");
    }
  };

  const handleRemoveAccount = async (connectedAccount) => {
    if (!connectedAccount?.accountId) return;
    if (composerRef.current) {
      showToast("请先关闭当前写信窗口，再移除邮箱账户。", "error");
      return;
    }
    const confirmed = window.confirm(
      `确定从 Mine Mail 移除 ${connectedAccount.email} 吗？\n\n系统凭据会被移除；本地邮件缓存会保留，重新连接后仍可恢复。`,
    );
    if (!confirmed) return;
    setAccountSubmitStatus("saving");
    try {
      const status = await mailApi.removeAccount(connectedAccount.accountId);
      await applyActiveAccount(status, "邮箱账户已移除");
    } catch (error) {
      setAccountSubmitStatus("error");
      showToast(describeError(error, "邮箱账户移除失败"), "error");
    }
  };

  const handleOpenExternalLink = useCallback(
    async (url) => {
      if (!url) return;
      try {
        await mailApi.openExternalUrl(url);
      } catch (error) {
        showToast(describeError(error, "无法打开邮件中的链接"), "error");
      }
    },
    [showToast],
  );

  const openReply = () => {
    if (!selectedMessage) return;
    openComposer({
      to: selectedMessage.sender?.email ? [selectedMessage.sender.email] : [],
      cc: [],
      bcc: [],
      subject: selectedMessage.subject.startsWith("Re:")
        ? selectedMessage.subject
        : `Re: ${selectedMessage.subject}`,
      body_text: `\n\n—— 原邮件 ——\n${selectedMessage.body_text || selectedMessage.preview}`,
    });
  };

  const openForward = () => {
    if (!selectedMessage) return;
    openComposer({
      to: [],
      cc: [],
      bcc: [],
      subject: selectedMessage.subject.startsWith("Fwd:")
        ? selectedMessage.subject
        : `Fwd: ${selectedMessage.subject}`,
      body_text: `\n\n—— 转发邮件 ——\n${selectedMessage.body_text || selectedMessage.preview}`,
    });
  };

  const navigateRelative = (offset) => {
    const next = visibleMessages[selectedIndex + offset];
    if (next) void handleSelect(next);
  };

  const needsAccountSetup =
    accountStatus.configured === false ||
    (accountStatus.configured === true &&
      !accountStatus.backendReady);

  const needsAccountRepairBanner =
    accountStatus.configured === true &&
    accountStatus.backendReady === true &&
    !networkActionsAvailable;

  if (isUnsupportedRuntime) {
    return (
      <main className="unsupported-runtime" role="main">
        <div>
          <p className="eyebrow">MINE MAIL DESKTOP</p>
          <h1>请从桌面应用启动 Mine Mail</h1>
          <p>
            普通浏览器不会连接邮箱，也不会启用模拟数据。开发界面时可显式设置
            <code>VITE_MINE_MAIL_DEMO=1</code>。
          </p>
        </div>
      </main>
    );
  }

  return (
    <div
      className={`app-shell platform-${platform} ${isSidebarOpen ? "sidebar-is-open" : ""} ${selectedMessage ? "has-selection" : ""}`}
      data-runtime={isTauriRuntime ? "tauri" : "web"}
    >
      <div className="app-wallpaper" aria-hidden="true" />
      <WindowTitlebar platform={platform} isDesktop={isTauriRuntime} />

      {needsAccountRepairBanner ? (
        <div className="account-repair-banner" role="alert">
          <span>
            {accountError ||
              "已下载的邮件仍可阅读；重新连接账户后才能同步、下载其他正文或发送邮件。"}
          </span>
          <button
            type="button"
            className="secondary-button"
            onClick={() => setIsSettingsOpen(true)}
          >
            修复账户
          </button>
        </div>
      ) : null}

      <div className="mail-layout">
        <Sidebar
          activeFolder={activeFolder}
          onFolderChange={handleFolderChange}
          onCompose={() => openComposer()}
          theme={theme}
          onThemeChange={(nextTheme) => {
            setTheme(nextTheme);
            setIsThemeMenuOpen(false);
          }}
          isThemeMenuOpen={isThemeMenuOpen}
          onThemeMenuToggle={() => setIsThemeMenuOpen((open) => !open)}
          counts={folderCounts}
          accountStatus={accountStatus}
          accountAvatarFor={(email) => profileAvatarFor("account", email)}
          onAccountSwitch={(accountId) => void handleSwitchAccount(accountId)}
          onAddAccount={() => {
            setSettingsSaveStatus("idle");
            setSettingsFocusTarget("account-form");
            setIsSettingsOpen(true);
          }}
          onOpenSettings={() => {
            setSettingsSaveStatus("idle");
            setSettingsFocusTarget(null);
            setIsSettingsOpen(true);
          }}
        />

        {isSidebarOpen ? (
          <button
            className="sidebar-backdrop"
            type="button"
            aria-label="关闭导航"
            onClick={() => setIsSidebarOpen(false)}
          />
        ) : null}

        <MailList
          folderLabel={folderLabels[activeFolder]}
          messages={visibleMessages}
          selectedUid={selectedUid}
          onSelect={handleSelect}
          query={query}
          onQueryChange={setQuery}
          filter={filter}
          onFilterChange={setFilter}
          onSync={handleSync}
          syncState={syncState}
          canSync={networkActionsAvailable}
          onOpenMobileNav={() => setIsSidebarOpen(true)}
          avatarForEmail={(email) => profileAvatarFor("contact", email)}
        />

        <MessageView
          message={selectedMessage}
          isLoading={isMessageLoading}
          error={messageError}
          onRetry={() => selectedMessage && void handleSelect(selectedMessage, true)}
          onClose={clearSelection}
          onReply={openReply}
          onForward={openForward}
          onRetryDelivery={() =>
            selectedMessage?.outbox && void handleRetryOutbox(selectedMessage.outbox)
          }
          isRetryingDelivery={Boolean(retryingOutboxId)}
          canRetryDelivery={networkActionsAvailable}
          onPrevious={() => navigateRelative(-1)}
          onNext={() => navigateRelative(1)}
          canPrevious={selectedIndex > 0}
          canNext={selectedIndex >= 0 && selectedIndex < visibleMessages.length - 1}
          remoteImageMode={settings.remoteImageMode}
          onOpenExternalLink={(url) => void handleOpenExternalLink(url)}
          senderAvatar={profileAvatarFor("contact", selectedMessage?.sender?.email)}
          onSetSenderAvatar={(file) =>
            handleSaveProfileAvatar("contact", selectedMessage?.sender?.email, file)
          }
          onRemoveSenderAvatar={() =>
            handleDeleteProfileAvatar("contact", selectedMessage?.sender?.email)
          }
        />
      </div>

      {composer ? (
        <ComposePanel
          key={composer.sessionId}
          value={composer.value}
          draftId={composer.draftId}
          saveStatus={composer.saveStatus}
          isSending={isSending}
          locked={composer.locked}
          readOnly={composer.readOnlyUnsupported}
          networkAvailable={networkActionsAvailable}
          onClose={() => void handleCloseComposer()}
          onDiscard={() => void handleDiscardComposer()}
          onChange={handleComposeChange}
          onSaveDraft={() => void handleSaveDraftAndClose()}
          onRequestSend={() => void handleRequestSend()}
          sendShortcut={platform === "mac" ? "⌘ ↵" : "Ctrl ↵"}
        />
      ) : null}

      <SendConfirmDialog
        request={pendingSend}
        isSending={isSending}
        onCancel={handleCancelSend}
        onConfirm={handleConfirmSend}
      />

      {isSettingsOpen ? (
        <SettingsPanel
          settings={settings}
          saveStatus={settingsSaveStatus}
          onClose={() => setIsSettingsOpen(false)}
          onSave={handleSaveSettings}
          accountPresets={accountPresets}
          accountStatus={accountStatus}
          accountSubmitStatus={accountSubmitStatus}
          accountError={accountError}
          onConfigureAccount={handleConfigureAccount}
          onConnectGoogle={handleConnectGoogle}
          onSwitchAccount={(accountId) => void handleSwitchAccount(accountId)}
          onRemoveAccount={(connectedAccount) =>
            void handleRemoveAccount(connectedAccount)
          }
          accountAvatar={profileAvatarFor("account", accountStatus.email)}
          onSetAccountAvatar={(file) =>
            handleSaveProfileAvatar("account", accountStatus.email, file)
          }
          onRemoveAccountAvatar={() =>
            handleDeleteProfileAvatar("account", accountStatus.email)
          }
          focusTarget={settingsFocusTarget}
        />
      ) : null}

      {needsAccountSetup ? (
        <AccountSetupPanel
          presets={accountPresets}
          status={accountStatus}
          submitStatus={accountSubmitStatus}
          error={accountError}
          onSubmit={handleConfigureAccount}
          onGoogle={handleConnectGoogle}
        />
      ) : null}

      <Toast toast={toast} onClose={() => setToast(null)} />
    </div>
  );
}
