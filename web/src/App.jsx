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
import { ContactsWorkspace } from "./components/ContactsWorkspace.jsx";
import { SendConfirmDialog } from "./components/SendConfirmDialog.jsx";
import { SettingsPanel } from "./components/SettingsPanel.jsx";
import { AccountSetupPanel } from "./components/AccountSetup.jsx";
import { Toast } from "./components/Toast.jsx";
import { normalizeAvatarEmail } from "./components/ProfileAvatar.jsx";
import { hasFlag } from "./utils/formatters.js";
import { messageNavigationKey } from "./utils/messageNavigation.js";

const folderLabels = {
  inbox: "收件箱",
  starred: "已收藏",
  sent: "已发送",
  drafts: "草稿",
  outbox: "发件队列",
  archive: "归档",
  trash: "垃圾箱",
  contacts: "通讯录",
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
    reader.addEventListener("error", () =>
      reject(new Error("无法读取所选图片")),
    );
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
  const recipientLabel = recipients.join(", ") || "未知收件人";
  return {
    id: item.id,
    uid: `outbox-${item.id}`,
    kind: "outbox",
    subject: item.subject || draft?.subject || status,
    sender: { name: recipientLabel, email: recipients[0] || "" },
    to: recipients.map((email) => ({ name: null, email })),
    sent_at: item.sent_at || item.created_at,
    flags: ["\\Seen"],
    preview: item.preview || "",
    body_text: null,
    body_fetched: false,
    delivery_status_label: status,
    attachment_names: [],
    outbox: item,
  };
}

function normalizeMessageId(value) {
  const normalized = value?.trim().toLowerCase();
  if (!normalized) return null;
  return normalized.replace(/^</, "").replace(/>$/, "");
}

function toSentMessage(message) {
  const recipients = [...(message.to || []), ...(message.cc || [])];
  const firstRecipient = recipients[0] || null;
  const recipientLabel =
    recipients
      .map((recipient) => recipient.name || recipient.email)
      .filter(Boolean)
      .join(", ") || "未知收件人";
  return {
    ...message,
    kind: "sent",
    sent_from: message.sender,
    sender: {
      name: recipientLabel,
      email: firstRecipient?.email || "",
    },
  };
}

function toContactDisplayMessage(message) {
  const displayMessage =
    message?.direction === "outgoing" ? toSentMessage(message) : message;
  return displayMessage
    ? { ...displayMessage, contactHistory: true }
    : displayMessage;
}

function sentMessageMatchesOutbox(message, item) {
  const remoteMessageId = normalizeMessageId(message.message_id);
  const localMessageId = normalizeMessageId(item.message_id);
  if (remoteMessageId && localMessageId)
    return remoteMessageId === localMessageId;

  // Compatibility for items sent before Mine Mail generated Message-ID. Keep
  // this deliberately strict so two genuinely separate sends are not hidden.
  if ((message.subject || "").trim() !== (item.subject || "").trim())
    return false;
  const remoteRecipients = [...(message.to || []), ...(message.cc || [])]
    .map((recipient) => normalizeAvatarEmail(recipient.email))
    .filter(Boolean);
  const localRecipients = new Set(
    (item.recipients || []).map(normalizeAvatarEmail).filter(Boolean),
  );
  if (
    !remoteRecipients.length ||
    !remoteRecipients.every((email) => localRecipients.has(email))
  ) {
    return false;
  }
  const remoteTime = Date.parse(message.sent_at || message.internal_date || "");
  const localTime = Date.parse(
    item.message_date || item.sent_at || item.created_at || "",
  );
  return (
    Number.isFinite(remoteTime) &&
    Number.isFinite(localTime) &&
    Math.abs(remoteTime - localTime) <= 5_000
  );
}

function withSeenFlag(message) {
  return withSystemFlag(message, "\\Seen", true);
}

function withSystemFlag(message, flag, desired) {
  if (!message || hasFlag(message, flag) === desired) return message;
  const flags = desired
    ? [...(message.flags || []), flag]
    : (message.flags || []).filter(
        (value) => value.toLowerCase() !== flag.toLowerCase(),
      );
  return { ...message, flags };
}

function remoteFlagKey(message) {
  if (!message || message.kind === "draft" || message.kind === "outbox")
    return null;
  const uid = Number(message.uid);
  const mailbox = (message.mailbox || (!message.kind ? "INBOX" : "")).trim();
  if (!mailbox || !Number.isInteger(uid) || uid <= 0) return null;
  return `${mailbox.toLowerCase()}:${uid}`;
}

function scopedRemoteFlagKey(message, accountId = "unscoped") {
  const key = remoteFlagKey(message);
  return key ? `${accountId}:${key}` : null;
}

function hasDraftContent(value) {
  return Boolean(
    value &&
    ([...value.to, ...value.cc, ...value.bcc].length ||
      value.subject.trim() ||
      value.body_text.trim()),
  );
}

function createComposer(
  value = emptyCompose,
  draftId = null,
  persistedDraft = null,
) {
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
    reply_context: draft?.reply_context
      ? structuredClone(draft.reply_context)
      : null,
  };
}

function upsertDraft(items, draft) {
  return [draft, ...items.filter((item) => item.id !== draft.id)];
}

export function App() {
  const [theme, setTheme] = useState(getInitialTheme);
  const [activeFolder, setActiveFolder] = useState("inbox");
  const [messages, setMessages] = useState([]);
  const [sentMessages, setSentMessages] = useState([]);
  const [drafts, setDrafts] = useState([]);
  const [outbox, setOutbox] = useState([]);
  const [selectedUid, setSelectedUid] = useState(null);
  const [selectedMessage, setSelectedMessage] = useState(null);
  const [isMessageLoading, setIsMessageLoading] = useState(false);
  const [messageError, setMessageError] = useState(null);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [contacts, setContacts] = useState([]);
  const [favoriteContacts, setFavoriteContacts] = useState([]);
  const [contactQuery, setContactQuery] = useState("");
  const [contactFilter, setContactFilter] = useState("all");
  const [contactsState, setContactsState] = useState("idle");
  const [contactsError, setContactsError] = useState(null);
  const [selectedContactEmail, setSelectedContactEmail] = useState(null);
  const [selectedContactAccountId, setSelectedContactAccountId] =
    useState(null);
  const [contactMessages, setContactMessages] = useState([]);
  const [contactMessagesState, setContactMessagesState] = useState("idle");
  const [contactMessagesError, setContactMessagesError] = useState(null);
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
  const [referenceJump, setReferenceJump] = useState(null);

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
  const activeFolderRef = useRef("inbox");
  const selectedContactEmailRef = useRef(null);
  const selectedContactAccountIdRef = useRef(null);
  const accountSwitchRequestRef = useRef(0);
  const referenceJumpRequestRef = useRef(0);
  const contactsRequestRef = useRef(0);
  const contactMessagesRequestRef = useRef(0);
  const starRequestRef = useRef(new Map());
  const starStateRef = useRef(new Map());
  const settingsSaveRequestRef = useRef(0);
  const platform = /Mac|iPhone|iPad/.test(navigator.platform)
    ? "mac"
    : "windows";
  const networkActionsAvailable = Boolean(
    accountStatus.configured &&
    accountStatus.backendReady &&
    accountStatus.credentialAvailable &&
    accountStatus.networkReady !== false,
  );
  const activeAccountId =
    accountStatus.activeAccountId || accountStatus.accountId || null;
  networkActionsAvailableRef.current = networkActionsAvailable;
  draftsRef.current = drafts;
  activeAccountIdRef.current = activeAccountId;
  activeFolderRef.current = activeFolder;
  selectedContactEmailRef.current = selectedContactEmail;
  selectedContactAccountIdRef.current = selectedContactAccountId;

  useEffect(() => {
    const accountId = activeAccountIdRef.current;
    if (!accountId) return;
    accountViewsRef.current.set(accountId, {
      messages,
      sentMessages,
      drafts,
      outbox,
      selectedUid,
      selectedMessage,
    });
  }, [drafts, messages, outbox, selectedMessage, selectedUid, sentMessages]);

  const showToast = useCallback(
    (message, tone = "success", persistent = false) => {
      setToast({ message, tone, persistent, id: Date.now() });
    },
    [],
  );

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
        ? profileAvatarMap.get(`${ownerType}:${normalizeAvatarEmail(email)}`) ||
          null
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
              avatar.ownerType !== saved.ownerType ||
              avatar.ownerKey !== saved.ownerKey,
          ),
          saved,
        ]);
        showToast(
          ownerType === "account" ? "Mine Mail 头像已更新" : "联系人头像已更新",
        );
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
            (avatar) =>
              avatar.ownerType !== ownerType || avatar.ownerKey !== ownerKey,
          ),
        );
        showToast(
          ownerType === "account"
            ? "已恢复默认账户头像"
            : "已恢复默认联系人头像",
        );
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
            reply_context: message.draft.reply_context
              ? structuredClone(message.draft.reply_context)
              : null,
          },
          message.draft.id,
          message.draft,
        );
        return;
      }

      const accountId = activeAccountIdRef.current || "unscoped";
      const selectedStarKey = scopedRemoteFlagKey(message, accountId);
      if (selectedStarKey && !starStateRef.current.has(selectedStarKey)) {
        starStateRef.current.set(
          selectedStarKey,
          hasFlag(message, "\\Flagged"),
        );
      }
      const shouldMarkRead =
        !message.kind &&
        (message.mailbox || "INBOX").toLowerCase() === "inbox" &&
        !hasFlag(message, "\\Seen");
      const cachedBody = messageBodyCacheRef.current.get(
        messageCacheKey(message, accountId),
      );
      const cachedDisplayMessage = cachedBody
        ? { ...message, ...cachedBody }
        : message;
      const displayMessage = shouldMarkRead
        ? withSeenFlag(cachedDisplayMessage)
        : cachedDisplayMessage;
      const requestId = selectionRequestRef.current + 1;
      selectionRequestRef.current = requestId;
      selectedUidRef.current = message.uid;
      setSelectedUid(message.uid);
      setSelectedMessage(displayMessage);
      setMessageError(null);

      if (shouldMarkRead) {
        setMessages((current) => {
          const updated = current.map((mail) =>
            mail.uid === message.uid ? withSeenFlag(mail) : mail,
          );
          const accountView = accountViewsRef.current.get(accountId) || {};
          accountViewsRef.current.set(accountId, {
            ...accountView,
            messages: updated,
          });
          return updated;
        });
        setContactMessages((current) =>
          current.map((mail) =>
            remoteFlagKey(mail) === remoteFlagKey(message)
              ? withSeenFlag(mail)
              : mail,
          ),
        );
        void mailApi.markMessageRead(message.uid).catch((error) => {
          showToast(describeError(error, "已读状态保存失败"), "error");
        });
      }

      const needsHtmlHydration =
        displayMessage.body_html_available === true &&
        displayMessage.body_html_loaded !== true;
      if (!forceFetch && displayMessage.body_fetched && !needsHtmlHydration) {
        setIsMessageLoading(false);
        return;
      }

      if (displayMessage.kind === "outbox") {
        setIsMessageLoading(!displayMessage.preview?.trim());
        try {
          const hydrated = {
            ...displayMessage,
            ...(await mailApi.fetchOutboxMessage(message.outbox.id)),
          };
          if (
            selectionRequestRef.current !== requestId ||
            selectedUidRef.current !== message.uid
          ) {
            return;
          }
          messageBodyCacheRef.current.set(
            messageCacheKey(message, accountId),
            bodySnapshot(hydrated),
          );
          setSelectedMessage(hydrated);
        } catch (error) {
          if (selectionRequestRef.current === requestId) {
            const messageText = describeError(error, "已发送邮件正文加载失败");
            setMessageError(messageText);
            showToast(messageText, "error");
          }
        } finally {
          if (selectionRequestRef.current === requestId)
            setIsMessageLoading(false);
        }
        return;
      }

      if (!networkActionsAvailableRef.current && !displayMessage.body_fetched) {
        setIsMessageLoading(false);
        setMessageError("这封邮件的正文尚未下载。重新连接账户后即可获取。");
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
        const fetchedMessage = displayMessage.contactHistory
          ? await mailApi.fetchContactMessage(
              accountId,
              message.mailbox,
              message.uid,
            )
          : displayMessage.kind === "sent"
            ? await mailApi.fetchSentMessage(message.uid)
            : await mailApi.fetchMessage(message.uid);
        let fullMessage =
          displayMessage.kind === "sent" && fetchedMessage
            ? toSentMessage(fetchedMessage)
            : fetchedMessage;
        if (displayMessage.contactHistory && fullMessage) {
          fullMessage = { ...fullMessage, contactHistory: true };
        }
        if (shouldMarkRead && fullMessage) {
          fullMessage = withSeenFlag(fullMessage);
        }
        const fullMessageStarKey = scopedRemoteFlagKey(fullMessage, accountId);
        if (
          fullMessageStarKey &&
          starStateRef.current.has(fullMessageStarKey)
        ) {
          fullMessage = withSystemFlag(
            fullMessage,
            "\\Flagged",
            starStateRef.current.get(fullMessageStarKey),
          );
        }
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
        if (displayMessage.kind === "sent") {
          setSentMessages((current) =>
            current.map((mail) =>
              mail.uid === fullMessage.uid ? fullMessage : mail,
            ),
          );
        } else {
          setMessages((current) =>
            current.map((mail) =>
              mail.uid === fullMessage.uid ? fullMessage : mail,
            ),
          );
        }
      } catch (error) {
        if (selectionRequestRef.current === requestId) {
          const messageText = describeError(error, "邮件正文加载失败");
          setMessageError(messageText);
          showToast(messageText, "error");
        }
      } finally {
        if (selectionRequestRef.current === requestId)
          setIsMessageLoading(false);
      }
    },
    [openComposer, showToast],
  );

  const applyMessageStarState = useCallback((target, starred, accountId) => {
    const messageKey = remoteFlagKey(target);
    if (!messageKey) return;
    const scopedKey = scopedRemoteFlagKey(target, accountId);
    starStateRef.current.set(scopedKey, starred);
    const update = (mail) =>
      remoteFlagKey(mail) === messageKey
        ? withSystemFlag(mail, "\\Flagged", starred)
        : mail;
    if (activeAccountIdRef.current !== accountId) {
      const accountView = accountViewsRef.current.get(accountId) || {};
      accountViewsRef.current.set(accountId, {
        ...accountView,
        messages: (accountView.messages || []).map(update),
        sentMessages: (accountView.sentMessages || []).map(update),
        selectedMessage: update(accountView.selectedMessage),
      });
      return;
    }
    setMessages((current) => {
      const updated = current.map(update);
      const accountView = accountViewsRef.current.get(accountId) || {};
      accountViewsRef.current.set(accountId, {
        ...accountView,
        messages: updated,
      });
      return updated;
    });
    setSentMessages((current) => {
      const updated = current.map(update);
      const accountView = accountViewsRef.current.get(accountId) || {};
      accountViewsRef.current.set(accountId, {
        ...accountView,
        sentMessages: updated,
      });
      return updated;
    });
    setContactMessages((current) => current.map(update));
    setSelectedMessage((current) => update(current));
  }, []);

  const handleToggleStar = useCallback(
    async (message) => {
      const accountId = activeAccountIdRef.current || "unscoped";
      const key = scopedRemoteFlagKey(message, accountId);
      if (!key) return;
      const mailbox = message.mailbox || "INBOX";
      const starred = !hasFlag(message, "\\Flagged");
      const requestId = (starRequestRef.current.get(key)?.requestId || 0) + 1;
      starRequestRef.current.set(key, { requestId, starred });
      applyMessageStarState(message, starred, accountId);
      try {
        await mailApi.setMessageStarred(mailbox, message.uid, starred);
        if (starRequestRef.current.get(key)?.requestId === requestId) {
          starRequestRef.current.delete(key);
        }
      } catch (error) {
        if (starRequestRef.current.get(key)?.requestId !== requestId) return;
        starRequestRef.current.delete(key);
        applyMessageStarState(message, !starred, accountId);
        showToast(describeError(error, "收藏状态保存失败"), "error");
      }
    },
    [applyMessageStarState, showToast],
  );

  const refreshInbox = useCallback(
    async ({ selectFirst = false } = {}) => {
      const accountId = activeAccountIdRef.current || "unscoped";
      const summaries = await mailApi.listInbox(50);
      const inbox = summaries.map((message) => {
        const cachedBody = messageBodyCacheRef.current.get(
          messageCacheKey(message, accountId),
        );
        let resolved = cachedBody ? { ...message, ...cachedBody } : message;
        const key = scopedRemoteFlagKey(resolved, accountId);
        const pending = key ? starRequestRef.current.get(key) : null;
        const starred = pending?.starred ?? hasFlag(resolved, "\\Flagged");
        if (key) starStateRef.current.set(key, starred);
        if (pending) resolved = withSystemFlag(resolved, "\\Flagged", starred);
        return resolved;
      });
      const existingView = accountViewsRef.current.get(accountId) || {};
      accountViewsRef.current.set(accountId, {
        ...existingView,
        messages: inbox,
      });
      if (activeAccountIdRef.current !== accountId) return inbox;
      setMessages(inbox);
      const currentUid = selectedUidRef.current;
      if (currentUid !== null) {
        const current = inbox.find((message) => message.uid === currentUid);
        if (current) {
          setSelectedMessage((previous) => {
            if (previous?.kind) return previous;
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

  const refreshSent = useCallback(async () => {
    const accountId = activeAccountIdRef.current || "unscoped";
    const summaries = await mailApi.listSent(250);
    const sent = summaries.map((summary) => {
      let message = toSentMessage(summary);
      const cachedBody = messageBodyCacheRef.current.get(
        messageCacheKey(message, accountId),
      );
      message = cachedBody ? { ...message, ...cachedBody } : message;
      const key = scopedRemoteFlagKey(message, accountId);
      const pending = key ? starRequestRef.current.get(key) : null;
      const starred = pending?.starred ?? hasFlag(message, "\\Flagged");
      if (key) starStateRef.current.set(key, starred);
      return pending ? withSystemFlag(message, "\\Flagged", starred) : message;
    });
    const existingView = accountViewsRef.current.get(accountId) || {};
    accountViewsRef.current.set(accountId, {
      ...existingView,
      sentMessages: sent,
    });
    if (activeAccountIdRef.current !== accountId) return sent;
    setSentMessages(sent);
    setSelectedMessage((previous) => {
      if (previous?.kind !== "sent") return previous;
      const current = sent.find((message) => message.uid === previous.uid);
      if (!current) return previous;
      const preservedBody = bodySnapshot(previous);
      messageBodyCacheRef.current.set(
        messageCacheKey(current, accountId),
        preservedBody,
      );
      return { ...previous, ...current, ...preservedBody };
    });
    return sent;
  }, []);

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
      if (!freshItem) return current;
      const summary = toOutboxMessage(freshItem, draftsRef.current);
      return current.body_fetched
        ? { ...summary, ...bodySnapshot(current) }
        : summary;
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
    const sent = (snapshot?.sent || []).map((summary) => {
      const message = toSentMessage(summary);
      const cachedBody = messageBodyCacheRef.current.get(
        messageCacheKey(message, accountId),
      );
      return cachedBody ? { ...message, ...cachedBody } : message;
    });
    const selectedUid = previous.selectedUid ?? null;
    const selectedMessage = selectedUid
      ? (previous.selectedMessage?.kind === "sent"
          ? sent.find((message) => message.uid === selectedUid)
          : inbox.find((message) => message.uid === selectedUid)) ||
        previous.selectedMessage ||
        null
      : null;
    const view = {
      messages: inbox,
      sentMessages: sent,
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
    async ({
      selectFirst = false,
      accountId = activeAccountIdRef.current,
    } = {}) => {
      if (!accountId) return null;
      try {
        const view = await loadAccountView(accountId, { force: true });
        if (activeAccountIdRef.current !== accountId) return view;
        setMessages(view.messages);
        setSentMessages(view.sentMessages);
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
        sentMessages: [],
        drafts: [],
        outbox: [],
        selectedUid: null,
        selectedMessage: null,
      };
      activeAccountIdRef.current = accountId;
      selectionRequestRef.current += 1;
      setMessages(restored.messages);
      setSentMessages(restored.sentMessages || []);
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

  const loadContacts = useCallback(
    async ({
      accountId = activeAccountIdRef.current,
      selectFirst = false,
      silent = false,
    } = {}) => {
      if (!accountId) {
        setContacts([]);
        setFavoriteContacts([]);
        setContactsState("idle");
        return { contacts: [], favorites: [] };
      }
      const requestId = contactsRequestRef.current + 1;
      contactsRequestRef.current = requestId;
      if (!silent) {
        setContactsState("loading");
        setContactsError(null);
      }
      try {
        const directory = await mailApi.listContacts(accountId);
        if (
          contactsRequestRef.current !== requestId ||
          activeAccountIdRef.current !== accountId
        ) {
          return directory;
        }
        const currentContacts = (
          Array.isArray(directory) ? directory : directory.contacts || []
        ).map((item) => ({
          ...item,
          accountId: item.accountId || accountId,
        }));
        const appFavorites = (
          Array.isArray(directory) ? [] : directory.favorites || []
        ).map((item) => ({
          ...item,
          accountId: item.accountId || accountId,
        }));
        setContacts(currentContacts);
        setFavoriteContacts(appFavorites);
        setContactsState("ready");
        setContactsError(null);
        const currentKey = normalizeAvatarEmail(
          selectedContactEmailRef.current,
        );
        const currentAccountId = selectedContactAccountIdRef.current;
        const available = [...currentContacts, ...appFavorites];
        const selectionStillExists =
          currentKey &&
          available.some(
            (item) =>
              normalizeAvatarEmail(item.email) === currentKey &&
              item.accountId === currentAccountId,
          );
        if (!selectionStillExists && selectFirst && window.innerWidth >= 720) {
          const firstVisibleContact =
            currentContacts[0] || appFavorites[0] || null;
          setSelectedContactEmail(firstVisibleContact?.email || null);
          setSelectedContactAccountId(firstVisibleContact?.accountId || null);
        }
        return directory;
      } catch (error) {
        if (contactsRequestRef.current === requestId && !silent) {
          setContactsState("error");
          setContactsError(describeError(error, "联系人没有加载完成"));
        }
        throw error;
      }
    },
    [],
  );

  const loadContactMessages = useCallback(
    async (
      email,
      { accountId = activeAccountIdRef.current, silent = false } = {},
    ) => {
      const normalizedEmail = normalizeAvatarEmail(email);
      if (!accountId || !normalizedEmail) {
        setContactMessages([]);
        setContactMessagesState("idle");
        return [];
      }
      const requestId = contactMessagesRequestRef.current + 1;
      contactMessagesRequestRef.current = requestId;
      if (!silent) {
        setContactMessagesState("loading");
        setContactMessagesError(null);
      }
      try {
        const items = await mailApi.listContactMessages(
          accountId,
          normalizedEmail,
          250,
        );
        if (
          contactMessagesRequestRef.current !== requestId ||
          activeAccountIdRef.current !== accountId
        ) {
          return items;
        }
        setContactMessages(items);
        setContactMessagesState("ready");
        setContactMessagesError(null);
        return items;
      } catch (error) {
        if (contactMessagesRequestRef.current === requestId && !silent) {
          setContactMessagesState("error");
          setContactMessagesError(describeError(error, "往来邮件没有加载完成"));
        }
        throw error;
      }
    },
    [],
  );

  const refreshActiveContactWorkspace = useCallback(async () => {
    if (activeFolderRef.current !== "contacts") return;
    const accountId = activeAccountIdRef.current;
    if (!accountId) return;
    await loadContacts({ accountId, silent: true });
    const email = selectedContactEmailRef.current;
    const contactAccountId = selectedContactAccountIdRef.current || accountId;
    if (email) {
      await loadContactMessages(email, {
        accountId: contactAccountId,
        silent: true,
      });
    }
  }, [loadContactMessages, loadContacts]);

  useEffect(() => {
    if (!activeAccountId) return;
    // Contact remarks are local metadata used by the mail list and reader too,
    // so hydrate them with the active account's cached header activity even
    // before the contacts workspace is opened.
    void loadContacts({ accountId: activeAccountId, selectFirst: true }).catch(
      () => {},
    );
  }, [activeAccountId, loadContacts]);

  useEffect(() => {
    if (
      activeFolder !== "contacts" ||
      !selectedContactAccountId ||
      !selectedContactEmail
    ) {
      contactMessagesRequestRef.current += 1;
      setContactMessages([]);
      setContactMessagesState("idle");
      setContactMessagesError(null);
      return;
    }
    void loadContactMessages(selectedContactEmail, {
      accountId: selectedContactAccountId,
    }).catch(() => {});
  }, [
    activeFolder,
    loadContactMessages,
    selectedContactAccountId,
    selectedContactEmail,
  ]);

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
          if (!cancelled)
            showToast(describeError(error, "桌面设置读取失败"), "error");
        });
      const presetsTask = mailApi
        .listAccountPresets()
        .then((value) => !cancelled && setAccountPresets(value))
        .catch((error) => {
          if (!cancelled)
            showToast(describeError(error, "账户预设读取失败"), "error");
        });
      const avatarsTask = mailApi
        .listProfileAvatars()
        .then((value) => !cancelled && setProfileAvatars(value))
        .catch((error) => {
          if (!cancelled)
            showToast(describeError(error, "本地头像读取失败"), "error");
        });

      try {
        const status = await mailApi.getAccountStatus();
        if (cancelled) return;
        const activeAccountId =
          status.activeAccountId || status.accountId || null;
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
          latest?.sessionId === sessionId &&
          latest.revision === snapshot.revision;
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
            void refreshInbox()
              .then(() => refreshActiveContactWorkspace())
              .catch((error) => reportEventError(error, "收件箱刷新失败"));
          },
        );
        if (cancelled) inboxUnlisten();
        else disposers.push(inboxUnlisten);

        const sentUnlisten = await mailApi.onMailEvent(
          "mail:sent-updated",
          () => {
            void refreshSent()
              .then(() => refreshActiveContactWorkspace())
              .catch((error) => reportEventError(error, "已发送刷新失败"));
          },
        );
        if (cancelled) sentUnlisten();
        else disposers.push(sentUnlisten);

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
                    throw new Error(
                      "请先关闭当前写信窗口，再打开其他账户的新邮件",
                    );
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
              .catch((error) => reportEventError(error, "新邮件暂时无法打开"));
          },
        );
        if (cancelled) openMessageUnlisten();
        else disposers.push(openMessageUnlisten);

        const draftsUnlisten = await mailApi.onMailEvent(
          "mail:drafts-updated",
          () => {
            void Promise.all([refreshDrafts(), refreshOutbox()]).catch(
              (error) => reportEventError(error, "草稿或发件队列刷新失败"),
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
              showToast(
                "桌面退出请求缺少 requestId，已拒绝退出",
                "error",
                true,
              );
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
    refreshActiveContactWorkspace,
    refreshInbox,
    refreshOutbox,
    refreshSent,
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
  }, [
    composer?.dirty,
    composer?.revision,
    composer?.saveStatus,
    composer?.sessionId,
    saveDraftNow,
    showToast,
  ]);

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

  const combinedSentMessages = useMemo(() => {
    const seenRemoteIds = new Set();
    const remote = sentMessages.filter((message) => {
      const messageId = normalizeMessageId(message.message_id);
      if (!messageId) return true;
      if (seenRemoteIds.has(messageId)) return false;
      seenRemoteIds.add(messageId);
      return true;
    });
    const localFallbacks = outbox
      .filter((item) => item.status === "sent")
      .filter(
        (item) =>
          !remote.some((message) => sentMessageMatchesOutbox(message, item)),
      )
      .map((item) => toOutboxMessage(item, drafts));
    return [...remote, ...localFallbacks].sort((left, right) => {
      const leftTime = Date.parse(left.sent_at || "") || 0;
      const rightTime = Date.parse(right.sent_at || "") || 0;
      return rightTime - leftTime;
    });
  }, [drafts, outbox, sentMessages]);

  const referenceNavigationIndex = useMemo(() => {
    const index = new Map();
    for (const message of messages) {
      const key = messageNavigationKey(message);
      if (key) index.set(key, { folder: "inbox", message });
    }
    for (const message of combinedSentMessages) {
      const key = messageNavigationKey(message);
      if (key) index.set(key, { folder: "sent", message });
    }
    return index;
  }, [combinedSentMessages, messages]);

  const resolveReferencedMessage = useCallback(
    (target) => {
      const key = messageNavigationKey(target);
      return key ? referenceNavigationIndex.get(key) || null : null;
    },
    [referenceNavigationIndex],
  );

  const handleOpenReferencedMessage = useCallback(
    (target) => {
      const key = messageNavigationKey(target);
      const destination = key ? referenceNavigationIndex.get(key) : null;
      if (!destination) {
        showToast("原邮件已不在当前列表中", "info");
        return;
      }

      referenceJumpRequestRef.current += 1;
      setReferenceJump({
        key,
        requestId: referenceJumpRequestRef.current,
      });
      setActiveFolder(destination.folder);
      setFilter("all");
      setQuery("");
      setIsSidebarOpen(false);
      void handleSelect(destination.message);
    },
    [handleSelect, referenceNavigationIndex, showToast],
  );

  const contactAccountLabels = useMemo(
    () =>
      new Map(
        (accountStatus.accounts || []).map((account) => [
          account.accountId,
          account.email || account.accountId,
        ]),
      ),
    [accountStatus.accounts],
  );

  const contactsWithAvatars = useMemo(
    () =>
      contacts.map((contact) => ({
        ...contact,
        avatarSrc: profileAvatarFor("contact", contact.email),
        accountLabel:
          contactAccountLabels.get(contact.accountId) || contact.accountId,
      })),
    [contactAccountLabels, contacts, profileAvatarFor],
  );

  const favoriteContactsWithAvatars = useMemo(
    () =>
      favoriteContacts.map((contact) => ({
        ...contact,
        avatarSrc: profileAvatarFor("contact", contact.email),
        accountLabel:
          contactAccountLabels.get(contact.accountId) || contact.accountId,
      })),
    [contactAccountLabels, favoriteContacts, profileAvatarFor],
  );

  const composeContactsWithAvatars = useMemo(() => {
    const byEmail = new Map();
    for (const contact of [
      ...contactsWithAvatars,
      ...favoriteContactsWithAvatars,
    ]) {
      const key = normalizeAvatarEmail(contact.email);
      if (key && !byEmail.has(key)) byEmail.set(key, contact);
    }
    return [...byEmail.values()];
  }, [contactsWithAvatars, favoriteContactsWithAvatars]);

  const contactRemarksByEmail = useMemo(
    () =>
      new Map(
        [...contacts, ...favoriteContacts]
          .filter((contact) => contact.remark?.trim())
          .map((contact) => [
            normalizeAvatarEmail(contact.email),
            contact.remark.trim(),
          ]),
      ),
    [contacts, favoriteContacts],
  );

  const contactRemarkForEmail = useCallback(
    (email) => contactRemarksByEmail.get(normalizeAvatarEmail(email)) || null,
    [contactRemarksByEmail],
  );

  const visibleContacts = useMemo(() => {
    const normalizedQuery = contactQuery.trim().toLowerCase();
    const source =
      contactFilter === "favorite"
        ? favoriteContactsWithAvatars
        : contactsWithAvatars;
    return source
      .filter((contact) => {
        if (!normalizedQuery) return true;
        return [
          contact.displayName,
          contact.originalName,
          contact.remark,
          contact.email,
          contact.lastSubject,
        ].some((value) => value?.toLowerCase().includes(normalizedQuery));
      })
      .sort(
        (left, right) =>
          Number(Boolean(right.isFavorite)) - Number(Boolean(left.isFavorite)),
      );
  }, [
    contactFilter,
    contactQuery,
    contactsWithAvatars,
    favoriteContactsWithAvatars,
  ]);

  useEffect(() => {
    if (activeFolder !== "contacts") return;
    const selectedKey = normalizeAvatarEmail(selectedContactEmail);
    const selectedAccountId = selectedContactAccountId;
    if (
      selectedKey &&
      visibleContacts.some(
        (contact) =>
          normalizeAvatarEmail(contact.email) === selectedKey &&
          contact.accountId === selectedAccountId,
      )
    ) {
      return;
    }
    const firstVisible =
      window.innerWidth >= 720 ? visibleContacts[0] || null : null;
    setSelectedContactEmail(firstVisible?.email || null);
    setSelectedContactAccountId(firstVisible?.accountId || null);
  }, [
    activeFolder,
    selectedContactAccountId,
    selectedContactEmail,
    visibleContacts,
  ]);

  const selectedContact = useMemo(() => {
    const selectedKey = normalizeAvatarEmail(selectedContactEmail);
    return selectedKey
      ? visibleContacts.find(
          (contact) =>
            normalizeAvatarEmail(contact.email) === selectedKey &&
            contact.accountId === selectedContactAccountId,
        ) || null
      : null;
  }, [selectedContactAccountId, selectedContactEmail, visibleContacts]);

  const folderMessages = useMemo(() => {
    if (activeFolder === "inbox") return messages;
    if (activeFolder === "starred") {
      return [...messages, ...sentMessages].filter((message) =>
        hasFlag(message, "\\Flagged"),
      );
    }
    if (activeFolder === "drafts") {
      return drafts
        .filter((draft) => draft.status !== "sent")
        .map(toDraftMessage);
    }
    if (activeFolder === "outbox") return outboxMessages;
    if (activeFolder === "sent") return combinedSentMessages;
    return [];
  }, [
    activeFolder,
    combinedSentMessages,
    drafts,
    messages,
    outboxMessages,
    sentMessages,
  ]);

  const visibleMessages = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return folderMessages.filter((message) => {
      if (filter === "unread" && hasFlag(message, "\\Seen")) return false;
      if (filter === "starred" && !hasFlag(message, "\\Flagged")) return false;
      if (!normalizedQuery) return true;
      return [
        message.subject,
        message.preview,
        contactRemarkForEmail(message.sender?.email),
        message.sender?.name,
        message.sender?.email,
      ].some((value) => value?.toLowerCase().includes(normalizedQuery));
    });
  }, [contactRemarkForEmail, filter, folderMessages, query]);

  const selectedMessageKey = remoteFlagKey(selectedMessage);
  const selectedIndex = visibleMessages.findIndex((message) => {
    const key = remoteFlagKey(message);
    return selectedMessageKey && key
      ? key === selectedMessageKey
      : message.uid === selectedUid;
  });

  const contactDisplayMessages = useMemo(
    () => contactMessages.map(toContactDisplayMessage),
    [contactMessages],
  );
  const contactSelectedIndex = contactDisplayMessages.findIndex((message) => {
    const key = remoteFlagKey(message);
    return selectedMessageKey && key
      ? key === selectedMessageKey
      : message.uid === selectedUid;
  });

  const folderCounts = useMemo(
    () => ({
      inbox: messages.filter((message) => !hasFlag(message, "\\Seen")).length,
      starred: [...messages, ...sentMessages].filter((message) =>
        hasFlag(message, "\\Flagged"),
      ).length,
      drafts: drafts.filter((draft) => draft.status !== "sent").length,
      outbox: outbox.filter((item) => item.status !== "sent").length,
      sent: combinedSentMessages.length,
    }),
    [combinedSentMessages.length, drafts, messages, outbox, sentMessages],
  );

  const handleFolderChange = (folder) => {
    setIsSettingsOpen(false);
    setSettingsFocusTarget(null);
    setActiveFolder(folder);
    if (folder === "contacts") {
      setContactFilter("all");
      setContactQuery("");
    } else {
      setFilter("all");
      setQuery("");
    }
    clearSelection();
    setIsSidebarOpen(false);
  };

  const handleSelectContact = (contact) => {
    clearSelection();
    setSelectedContactEmail(contact?.email || null);
    setSelectedContactAccountId(contact?.accountId || null);
  };

  const handleBackToContacts = () => {
    clearSelection();
    setSelectedContactEmail(null);
    setSelectedContactAccountId(null);
  };

  const handleOpenContactMessage = (message) => {
    // Contact history deliberately carries no body/HTML across the Tauri
    // boundary. Force a local hydration even when SQLite reports that the
    // canonical cached message body has already been fetched.
    void handleSelect(toContactDisplayMessage(message), true);
  };

  const navigateContactRelative = (offset) => {
    const next = contactDisplayMessages[contactSelectedIndex + offset];
    if (next) void handleSelect(next, true);
  };

  const handleToggleContactFavorite = async (contact) => {
    const favoriteAccountId = contact?.accountId || activeAccountId;
    if (!contact?.email || !favoriteAccountId || !activeAccountId) return;
    const email = normalizeAvatarEmail(contact.email);
    const nextFavorite = !contact.isFavorite;
    const updateFavorite = (value) => (current) =>
      current.map((item) =>
        normalizeAvatarEmail(item.email) === email &&
        item.accountId === favoriteAccountId
          ? { ...item, isFavorite: value }
          : item,
      );
    setContacts(updateFavorite(nextFavorite));
    setFavoriteContacts((current) => {
      if (!nextFavorite) {
        return current.filter(
          (item) =>
            normalizeAvatarEmail(item.email) !== email ||
            item.accountId !== favoriteAccountId,
        );
      }
      if (
        current.some(
          (item) =>
            normalizeAvatarEmail(item.email) === email &&
            item.accountId === favoriteAccountId,
        )
      ) {
        return updateFavorite(true)(current);
      }
      return [
        { ...contact, accountId: favoriteAccountId, isFavorite: true },
        ...current,
      ];
    });
    try {
      await mailApi.setContactFavorite(
        favoriteAccountId,
        contact.email,
        nextFavorite,
      );
      await loadContacts({ accountId: activeAccountId, silent: true });
    } catch (error) {
      setContacts(updateFavorite(Boolean(contact.isFavorite)));
      await loadContacts({ accountId: activeAccountId, silent: true }).catch(
        () => {},
      );
      showToast(describeError(error, "联系人收藏状态没有保存"), "error");
    }
  };

  const handleSaveContactRemark = async (contact, remark) => {
    if (!contact?.email || !activeAccountId) return;
    const email = normalizeAvatarEmail(contact.email);
    const nextRemark = remark.trim();
    const previousRemark = contact.remark?.trim() || "";
    const applyRemark = (value) => (current) =>
      current.map((item) => {
        if (normalizeAvatarEmail(item.email) !== email) return item;
        const normalizedRemark = value || null;
        return {
          ...item,
          remark: normalizedRemark,
          displayName: normalizedRemark || item.originalName || item.email,
        };
      });

    setContacts(applyRemark(nextRemark));
    setFavoriteContacts(applyRemark(nextRemark));
    try {
      await mailApi.setContactRemark(contact.email, nextRemark);
      await loadContacts({ accountId: activeAccountId, silent: true });
      showToast(nextRemark ? "联系人备注已保存" : "联系人备注已清除");
    } catch (error) {
      setContacts(applyRemark(previousRemark));
      setFavoriteContacts(applyRemark(previousRemark));
      throw error;
    }
  };

  const handleComposeToContact = (contact) => {
    if (!contact?.email) return;
    openComposer({ ...emptyCompose, to: [contact.email] });
  };

  const handleSync = async () => {
    if (!networkActionsAvailable) {
      showToast("重新连接账户后才能同步邮箱", "error");
      return;
    }
    setSyncState("syncing");
    try {
      const report = await mailApi.syncAll();
      await Promise.all([
        refreshInbox(),
        refreshSent(),
        refreshDrafts(),
        refreshOutbox(),
      ]);
      if (activeFolder === "contacts" && activeAccountId) {
        await loadContacts({ accountId: activeAccountId, silent: true });
        if (selectedContactEmail) {
          await loadContactMessages(selectedContactEmail, {
            accountId: selectedContactAccountId || activeAccountId,
            silent: true,
          });
        }
      }
      setSyncState("done");
      const fetched = report?.inbox?.fetched ?? report?.fetched ?? 0;
      showToast(
        fetched ? `同步完成，收到 ${fetched} 封新邮件` : "邮箱已是最新状态",
      );
    } catch (error) {
      setSyncState("error");
      showToast(describeError(error, "同步失败，请检查网络"), "error");
    }
  };

  const reconcileSentAfterDelivery = useCallback(() => {
    void mailApi
      .syncSent()
      .then(() => refreshSent())
      // The local Outbox fallback remains visible if the provider's Sent copy
      // is briefly delayed or the follow-up sync is offline. Scheduled full
      // reconciliation will retry without changing delivery state.
      .catch(() => undefined);
  }, [refreshSent]);

  const handleComposeChange = (updater) => {
    commitComposer((current) => {
      if (!current || current.locked || current.readOnlyUnsupported)
        return current;
      const nextValue =
        typeof updater === "function" ? updater(current.value) : updater;
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
        current ? { ...current, locked: false, saveStatus: "error" } : current,
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
      showToast(
        describeError(error, "临时草稿清理失败，写信窗口仍保持打开"),
        "error",
      );
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
      reconcileSentAfterDelivery();
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
        reconcileSentAfterDelivery();
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
    const requestId = settingsSaveRequestRef.current + 1;
    settingsSaveRequestRef.current = requestId;
    setSettings(nextSettings);
    setSettingsSaveStatus("saving");
    try {
      const updated = await mailApi.updateDesktopSettings(nextSettings);
      if (settingsSaveRequestRef.current !== requestId) return;
      setSettings(updated);
      setSettingsSaveStatus("saved");
      window.setTimeout(() => {
        if (settingsSaveRequestRef.current === requestId) {
          setSettingsSaveStatus("idle");
        }
      }, 1600);
    } catch (error) {
      if (settingsSaveRequestRef.current !== requestId) return;
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
          status.startupError ||
          "账户信息已保存，但邮箱服务尚未就绪，请检查授权信息。";
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
          status.startupError || "本地邮箱已打开，但账户凭据或网络连接不可用。",
        );
      }
      setAccountSubmitStatus("saved");
      showToast("邮箱账户已安全连接");
    } catch (error) {
      const message = describeError(
        error,
        "账户配置失败，请检查地址和授权信息",
      );
      setAccountError(message);
      setAccountSubmitStatus("error");
    }
  };

  const applyActiveAccount = async (status, successMessage) => {
    setAccountStatus(status);
    activeAccountIdRef.current =
      status.activeAccountId || status.accountId || null;
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
        sentMessages,
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
          if (!loadedView || accountSwitchRequestRef.current !== requestId)
            return;
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
        restoreAccountView(
          previousAccountId,
          accountViewsRef.current.get(previousAccountId),
        );
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

  const openReply = async () => {
    if (!selectedMessage) return;
    if (isMessageLoading || !selectedMessage.body_fetched) {
      showToast("请等待邮件正文加载完成后再回复", "error");
      return;
    }
    try {
      const request = await mailApi.prepareReply(selectedMessage.id);
      openComposer(request);
    } catch (error) {
      showToast(describeError(error, "无法准备回复邮件"), "error");
    }
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
    (accountStatus.configured === true && !accountStatus.backendReady);

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

  const isContactMode = activeFolder === "contacts";
  const messageReader = (
    <MessageView
      message={selectedMessage}
      isLoading={isMessageLoading}
      error={messageError}
      onRetry={() =>
        selectedMessage && void handleSelect(selectedMessage, true)
      }
      onClose={clearSelection}
      backLabel={isContactMode ? "返回联系人详情" : "返回邮件列表"}
      onReply={openReply}
      onForward={openForward}
      onRetryDelivery={() =>
        selectedMessage?.outbox &&
        void handleRetryOutbox(selectedMessage.outbox)
      }
      isRetryingDelivery={Boolean(retryingOutboxId)}
      canRetryDelivery={networkActionsAvailable}
      onPrevious={() =>
        isContactMode ? navigateContactRelative(-1) : navigateRelative(-1)
      }
      onNext={() =>
        isContactMode ? navigateContactRelative(1) : navigateRelative(1)
      }
      canPrevious={isContactMode ? contactSelectedIndex > 0 : selectedIndex > 0}
      canNext={
        isContactMode
          ? contactSelectedIndex >= 0 &&
            contactSelectedIndex < contactDisplayMessages.length - 1
          : selectedIndex >= 0 && selectedIndex < visibleMessages.length - 1
      }
      remoteImageMode={settings.remoteImageMode}
      onOpenExternalLink={(url) => void handleOpenExternalLink(url)}
      resolveReferencedMessage={resolveReferencedMessage}
      onOpenReferencedMessage={handleOpenReferencedMessage}
      senderAvatar={profileAvatarFor("contact", selectedMessage?.sender?.email)}
      senderDisplayName={contactRemarkForEmail(selectedMessage?.sender?.email)}
      onSetSenderAvatar={(file) =>
        handleSaveProfileAvatar("contact", selectedMessage?.sender?.email, file)
      }
      onRemoveSenderAvatar={() =>
        handleDeleteProfileAvatar("contact", selectedMessage?.sender?.email)
      }
    />
  );

  return (
    <div
      className={`app-shell platform-${platform} ${isSidebarOpen ? "sidebar-is-open" : ""} ${isSettingsOpen ? "settings-is-open" : ""} ${selectedMessage || (isContactMode && selectedContact) ? "has-selection" : ""}`}
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
          isSettingsOpen={isSettingsOpen}
          accountAvatarFor={(email) => profileAvatarFor("account", email)}
          onAccountSwitch={(accountId) => void handleSwitchAccount(accountId)}
          onAddAccount={() => {
            setSettingsSaveStatus("idle");
            setSettingsFocusTarget(`account-form:${Date.now()}`);
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

        {isSettingsOpen ? (
          <SettingsPanel
            settings={settings}
            saveStatus={settingsSaveStatus}
            onClose={() => {
              setIsSettingsOpen(false);
              setSettingsFocusTarget(null);
            }}
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
            accountAvatarFor={(email) => profileAvatarFor("account", email)}
            onSetAccountAvatar={(email, file) =>
              handleSaveProfileAvatar("account", email, file)
            }
            onRemoveAccountAvatar={(email) =>
              handleDeleteProfileAvatar("account", email)
            }
            focusTarget={settingsFocusTarget}
          />
        ) : isContactMode ? (
          <ContactsWorkspace
            contacts={visibleContacts}
            selectedContact={selectedContact}
            messages={contactMessages}
            query={contactQuery}
            filter={contactFilter}
            isLoading={contactsState === "loading"}
            error={contactsError}
            isMessagesLoading={contactMessagesState === "loading"}
            messagesError={contactMessagesError}
            readerContent={selectedMessage ? messageReader : null}
            onRetry={() =>
              activeAccountId &&
              void loadContacts({ accountId: activeAccountId })
            }
            onRetryMessages={() =>
              selectedContactAccountId &&
              selectedContactEmail &&
              void loadContactMessages(selectedContactEmail, {
                accountId: selectedContactAccountId,
              })
            }
            onOpenMobileNav={() => setIsSidebarOpen(true)}
            onSearchChange={setContactQuery}
            onFilterChange={setContactFilter}
            onSelectContact={handleSelectContact}
            onBackToContacts={handleBackToContacts}
            onToggleFavorite={(contact) =>
              void handleToggleContactFavorite(contact)
            }
            onCompose={handleComposeToContact}
            onOpenMessage={handleOpenContactMessage}
            onSaveRemark={handleSaveContactRemark}
            onSetAvatar={(contact, file) =>
              handleSaveProfileAvatar("contact", contact.email, file)
            }
            onRemoveAvatar={(contact) =>
              handleDeleteProfileAvatar("contact", contact.email)
            }
          />
        ) : (
          <>
            <MailList
              folderLabel={folderLabels[activeFolder]}
              messages={visibleMessages}
              selectedUid={selectedUid}
              selectedMessage={selectedMessage}
              onSelect={handleSelect}
              onToggleStar={(message) => void handleToggleStar(message)}
              query={query}
              onQueryChange={setQuery}
              filter={filter}
              onFilterChange={setFilter}
              onSync={handleSync}
              syncState={syncState}
              canSync={networkActionsAvailable}
              onOpenMobileNav={() => setIsSidebarOpen(true)}
              avatarForEmail={(email) => profileAvatarFor("contact", email)}
              displayNameForEmail={contactRemarkForEmail}
              referenceJump={referenceJump}
            />
            {messageReader}
          </>
        )}
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
          contacts={composeContactsWithAvatars}
          remoteImageMode={settings.remoteImageMode}
          onOpenExternalLink={handleOpenExternalLink}
        />
      ) : null}

      <SendConfirmDialog
        request={pendingSend}
        isSending={isSending}
        onCancel={handleCancelSend}
        onConfirm={handleConfirmSend}
      />

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
