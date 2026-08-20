import {
  Component,
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { WarningCircle } from "@phosphor-icons/react";
import { emptyCompose } from "./models/compose.js";
import {
  isTauri,
  isTauriRuntime,
  isStaleMailboxCursorError,
  isUnsupportedRuntime,
  mailApi,
} from "./services/mailApi.js";
import { createReaderTranslationQueue } from "./services/readerTranslationQueue.js";
import { WindowTitlebar } from "./components/WindowTitlebar.jsx";
import { Sidebar } from "./components/Sidebar.jsx";
import { MailList } from "./components/MailList.jsx";
import { MessageView } from "./components/MessageView.jsx";
import { ComposePanel } from "./components/ComposePanel.jsx";
import { ConsequentialConfirmDialog } from "./components/ConsequentialConfirmDialog.jsx";
import { AccountEmptyWorkspace } from "./components/AccountEmptyWorkspace.jsx";
import { PermanentDeleteDialog } from "./components/PermanentDeleteDialog.jsx";
import { ArchiveFolderDialog } from "./components/ArchiveFolderDialog.jsx";
import { Toast } from "./components/Toast.jsx";
import { UpdateProgressNotice } from "./components/UpdateProgressNotice.jsx";
import { normalizeAvatarEmail } from "./components/ProfileAvatar.jsx";
import { useAppUpdate } from "./hooks/useAppUpdate.js";
import { useThemeSchedule } from "./hooks/useThemeSchedule.js";
import { hasFlag } from "./utils/formatters.js";
import { messageNavigationKey } from "./utils/messageNavigation.js";
import { userFacingErrorMessage } from "./utils/userFacingError.js";
import {
  appearanceFromSavedAppearance,
  applyAppearanceToDocument,
  builtinAppearanceThemes,
} from "./appearanceThemes.js";

const ContactsWorkspace = lazy(() =>
  import("./components/ContactsWorkspace.jsx").then(({ ContactsWorkspace }) => ({
    default: ContactsWorkspace,
  })),
);

function preloadContactsWorkspace() {
  return import("./components/ContactsWorkspace.jsx");
}

const SettingsPanel = lazy(() =>
  import("./components/SettingsPanel.jsx").then(({ SettingsPanel }) => ({
    default: SettingsPanel,
  })),
);

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

const defaultSettings = {
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
};
const supportedAvatarTypes = new Set(["image/png", "image/jpeg", "image/webp"]);
const maxAvatarBytes = 2 * 1024 * 1024;
const accountRepairDelayMs = 750;
const toastVisibleMs = 3800;
const importantToastVisibleMs = 8000;
const toastExitMs = 180;
const manualSyncFeedbackVisibleMs = 2000;
const readerWindowMotionMs = 260;
const readerFastExitMs = 120;
const mailListWindowMotionMs = 260;
const mailListSwitchHalfMs = mailListWindowMotionMs / 2;
const motionFallbackPaddingMs = 80;
const paginatedMailboxRoles = ["inbox", "sent", "archive", "trash"];
const starredMailboxRoles = ["inbox", "sent", "archive"];
const mailboxFolders = [...paginatedMailboxRoles, "drafts", "outbox"];
const mailboxPageSize = 50;
const maxAutomaticEmptyPageContinuations = 3;
const wideMailWorkspaceQuery = "(min-width: 941px)";
const reducedMotionQuery = "(prefers-reduced-motion: reduce)";

function nonNegativeSyncCount(value) {
  return Number.isSafeInteger(value) && value >= 0 ? value : 0;
}

function synchronizedMessageCount(report) {
  if (Array.isArray(report)) {
    return report.reduce(
      (total, item) => total + synchronizedMessageCount(item),
      0,
    );
  }
  if (Number.isSafeInteger(report)) return nonNegativeSyncCount(report);
  if (!report || typeof report !== "object") return 0;
  if (Number.isSafeInteger(report.synced)) {
    return nonNegativeSyncCount(report.synced);
  }
  if (Number.isSafeInteger(report.fetched)) {
    return nonNegativeSyncCount(report.fetched);
  }
  if (
    Number.isSafeInteger(report.pulled) ||
    Number.isSafeInteger(report.pushed)
  ) {
    return (
      nonNegativeSyncCount(report.pulled) +
      nonNegativeSyncCount(report.pushed)
    );
  }
  if (Number.isSafeInteger(report.drafts_synced)) {
    return nonNegativeSyncCount(report.drafts_synced);
  }
  return ["inbox", "sent", "drafts"].reduce(
    (total, key) => total + synchronizedMessageCount(report[key]),
    0,
  );
}

function manualSyncProgressMessage(folder) {
  if (folder === "outbox") return "正在刷新发件队列…";
  return `正在同步${folderLabels[folder] || "邮箱"}…`;
}

function manualSyncSuccessMessage(folder, count) {
  if (folder === "drafts") return `同步成功，处理 ${count} 封草稿`;
  if (folder === "outbox") return `刷新成功，共 ${count} 封队列邮件`;
  return `同步成功，新增 ${count} 封邮件`;
}

function mediaQueryMatches(query, fallback) {
  if (typeof window.matchMedia !== "function") return fallback;
  return window.matchMedia(query).matches;
}

function prefersReducedMotion() {
  // A non-visual runtime cannot complete CSS animation events, so atomic
  // transitions are the safest fallback there as well.
  return mediaQueryMatches(reducedMotionQuery, true);
}

function contactNavigationKey(email, accountId) {
  const normalizedEmail = normalizeAvatarEmail(email);
  if (!normalizedEmail) return "";
  return accountId ? `${accountId}:${normalizedEmail}` : normalizedEmail;
}

function useMediaQuery(query, fallback) {
  const [matches, setMatches] = useState(() =>
    mediaQueryMatches(query, fallback),
  );

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return undefined;
    const mediaQuery = window.matchMedia(query);
    const update = (event) => setMatches(event.matches);
    setMatches(mediaQuery.matches);
    if (typeof mediaQuery.addEventListener === "function") {
      mediaQuery.addEventListener("change", update);
    } else {
      mediaQuery.addListener?.(update);
    }
    return () => {
      if (typeof mediaQuery.removeEventListener === "function") {
        mediaQuery.removeEventListener("change", update);
      } else {
        mediaQuery.removeListener?.(update);
      }
    };
  }, [query]);

  return matches;
}

function useLiveCallback(callback) {
  const callbackRef = useRef(callback);
  callbackRef.current = callback;
  return useCallback((...args) => callbackRef.current?.(...args), []);
}

function SecondaryWorkspaceLoading({ label }) {
  return (
    <main
      className="secondary-workspace-loading"
      role="status"
      aria-live="polite"
      aria-busy="true"
    >
      {label}
    </main>
  );
}

export class SecondaryWorkspaceErrorBoundary extends Component {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <main
        className="secondary-workspace-loading secondary-workspace-loading--error"
        role="alert"
      >
        <span className="secondary-workspace-loading__message">
          <strong>{this.props.label}暂时无法打开</strong>
          <small>
            请返回邮件界面并重启 Mine Mail 后重试，本地邮件和草稿不会受影响。
          </small>
          <button
            type="button"
            className="secondary-button"
            onClick={this.props.onClose}
          >
            返回邮件界面
          </button>
        </span>
      </main>
    );
  }
}

function emptyMailboxPageState(overrides = {}) {
  return {
    query: "",
    nextCursor: null,
    hasMoreLocal: false,
    remoteHistoryState: "not_checked",
    endReached: false,
    loadMorePhase: "idle",
    loadMoreError: null,
    initialized: false,
    ...overrides,
  };
}

function createMailboxPageStates() {
  return Object.fromEntries(
    paginatedMailboxRoles.map((role) => [role, emptyMailboxPageState()]),
  );
}

function createStarredMailboxPageStates() {
  return Object.fromEntries(
    starredMailboxRoles.map((role) => [
      role,
      emptyMailboxPageState({ items: [] }),
    ]),
  );
}

function createMailboxLoadStates(phase = "loading") {
  return Object.fromEntries(
    mailboxFolders.map((folder) => [
      folder,
      { phase, completed: 0, total: null },
    ]),
  );
}

function mailboxProgress(payload) {
  const completed = Number(payload?.completed);
  const total = Number(payload?.total);
  return {
    complete: Boolean(payload?.isComplete ?? payload?.is_complete ?? true),
    completed: Number.isFinite(completed) && completed >= 0 ? completed : 0,
    total: Number.isFinite(total) && total > 0 ? total : null,
  };
}

function mailboxEventHasExplicitProgress(payload) {
  if (!payload || typeof payload !== "object") return false;
  return ["completed", "total", "isComplete", "is_complete", "report"].some(
    (key) => Object.prototype.hasOwnProperty.call(payload, key),
  );
}

function eventAccountId(payload) {
  return payload?.accountId ?? payload?.account_id ?? null;
}

function normalizedMailboxQuery(value) {
  const query = String(value || "").trim();
  return query || null;
}

function localMessageId(message) {
  const id = message?.id;
  if (typeof id === "string") {
    const normalized = id.trim();
    return normalized ? normalized : null;
  }
  return null;
}

function messageRole(message, fallback = "inbox") {
  const role = String(
    message?.displayed_role ||
      message?.mailbox_role ||
      message?.role ||
      message?.kind ||
      fallback,
  )
    .trim()
    .toLowerCase();
  return paginatedMailboxRoles.includes(role) ? role : fallback;
}

function toMailboxDisplayMessage(message, role) {
  if (!message) return message;
  const displayedRole = messageRole(message, role);
  const normalized = {
    ...message,
    displayed_role: displayedRole,
  };
  return displayedRole === "sent" ? toSentMessage(normalized) : normalized;
}

function normalizeMailboxPage(page, role, query = null) {
  return {
    items: (page?.items || []).map((message) =>
      toMailboxDisplayMessage(message, role),
    ),
    state: emptyMailboxPageState({
      query: normalizedMailboxQuery(query) || "",
      nextCursor: page?.next_cursor ?? page?.nextCursor ?? null,
      hasMoreLocal: Boolean(
        page?.has_more_local ?? page?.hasMoreLocal ?? false,
      ),
      remoteHistoryState:
        page?.remote_history_state ??
        page?.remoteHistoryState ??
        "not_checked",
      endReached: Boolean(page?.end_reached ?? page?.endReached ?? false),
      initialized: true,
    }),
  };
}

function appendMailboxItems(current, incoming) {
  const currentItems = current || [];
  const seen = new Set();
  const appended = [];
  for (const message of [...currentItems, ...(incoming || [])]) {
    const id = localMessageId(message);
    if (id === null || seen.has(id)) continue;
    seen.add(id);
    appended.push(message);
  }
  if (
    appended.length === currentItems.length &&
    appended.every((message, index) => message === currentItems[index])
  ) {
    return currentItems;
  }
  return appended;
}

function nextEmptyMailboxPageCursor(
  cursor,
  page,
  appendedItemCount,
  completedContinuations,
) {
  const nextCursor = page?.state?.nextCursor || null;
  if (
    appendedItemCount !== 0 ||
    page?.state?.remoteHistoryState !== "may_have_more" ||
    page?.state?.endReached ||
    !nextCursor ||
    nextCursor === cursor ||
    completedContinuations >= maxAutomaticEmptyPageContinuations
  ) {
    return null;
  }
  return nextCursor;
}

function preserveStarredVisitMessages(snapshotItems, currentItems) {
  if (!snapshotItems.length) {
    return currentItems.filter((message) => hasFlag(message, "\\Flagged"));
  }
  const currentById = new Map(
    currentItems
      .map((message) => [localMessageId(message), message])
      .filter(([id]) => id !== null),
  );
  const snapshotIds = new Set(
    snapshotItems.map(localMessageId).filter((id) => id !== null),
  );
  const stableItems = snapshotItems.map((message) => {
    const messageId = localMessageId(message);
    return messageId === null
      ? message
      : currentById.get(messageId) || message;
  });
  return appendMailboxItems(
    stableItems,
    currentItems.filter(
      (message) =>
        hasFlag(message, "\\Flagged") &&
        !snapshotIds.has(localMessageId(message)),
    ),
  );
}

export function mergeRefreshedMailboxItems(current, refreshed) {
  // The refreshed first page is authoritative for duplicate summaries and
  // order, while already loaded older pages remain mounted behind it.
  return appendMailboxItems(refreshed, current);
}

function capabilityMap(capabilities) {
  return Object.fromEntries(
    (capabilities || [])
      .filter((capability) => capability?.role)
      .map((capability) => [capability.role, capability]),
  );
}

function capabilityAvailable(capabilities, role) {
  if (role === "inbox" || role === "sent") return true;
  return capabilities?.[role]?.status === "available";
}

function mailboxSetupFailureMessage(capability, role) {
  const mailboxLabel = role === "archive" ? "归档文件夹" : "垃圾箱";
  if (capability?.unavailable_reason === "create_not_supported") {
    return `服务器不支持创建${mailboxLabel}`;
  }
  if (capability?.unavailable_reason === "created_mailbox_not_selectable") {
    return `已创建的${mailboxLabel}无法打开`;
  }
  if (capability?.unavailable_reason === "provider_unsupported") {
    return `当前邮箱服务不支持${mailboxLabel}`;
  }
  return role === "archive"
    ? `${mailboxLabel}设置失败，请重试`
    : `${mailboxLabel}创建失败，请重试`;
}

const archiveFolderSelectionCancelledCode =
  "archive_folder_selection_cancelled";

function archiveFolderSelectionCancelledError() {
  const error = new Error("已取消设置归档文件夹");
  error.code = archiveFolderSelectionCancelledCode;
  return error;
}

function isArchiveFolderSelectionCancelled(error) {
  return error?.code === archiveFolderSelectionCancelledCode;
}

function mailboxViewField(role) {
  if (role === "inbox") return "messages";
  if (role === "sent") return "sentMessages";
  if (role === "archive") return "archiveMessages";
  return "trashMessages";
}

function mutationActionState(mutation) {
  if (!mutation) return null;
  return {
    status: mutation.status || "pending",
    operationId: mutation.operation_id ?? mutation.operationId ?? null,
    sourceRole: mutation.source_role ?? mutation.sourceRole ?? null,
    destinationRole:
      mutation.destination_role ?? mutation.destinationRole ?? null,
    flag: mutation.flag ?? null,
    desired: mutation.desired,
    message:
      mutation.error_kind === "retryable"
        ? "等待重新同步"
        : mutation.error_kind
          ? "需要重新同步确认"
          : "",
    retryable: mutation.error_kind === "retryable",
  };
}

function messageActionKey(accountId, messageId, action) {
  return `${accountId}:${messageId}:${action}`;
}

function canUseAccountNetwork(status) {
  return Boolean(
    status?.configured &&
      status?.backendReady &&
      status?.credentialAvailable &&
      status?.networkReady !== false,
  );
}

function shouldRepairAccount(status) {
  return Boolean(
    status?.configured &&
      status?.backendReady &&
      !canUseAccountNetwork(status),
  );
}

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
  "attachments",
  "attachment_names",
  "body_fetched",
];

function messageCacheKey(message, accountId = "unscoped") {
  const id = localMessageId(message);
  return id !== null ? `${accountId}:message:${id}` : null;
}

function bodySnapshot(message) {
  return Object.fromEntries(
    cachedBodyFields.map((field) => [field, message?.[field]]),
  );
}

function messageBodyIsReady(message) {
  if (!message?.body_fetched) return false;
  if (
    message.body_html_available === true &&
    message.body_html_loaded !== true
  ) {
    return false;
  }
  return Boolean(
    message.body_html_loaded === true ||
      typeof message.body_text === "string" ||
      typeof message.body_html === "string" ||
      message.body_segments?.length,
  );
}

function getInitialAppearance() {
  return appearanceFromSavedAppearance();
}

function describeError(error, fallback) {
  return userFacingErrorMessage(error, fallback);
}

function toDraftMessage(draft, index) {
  return {
    id: draft.id,
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

function accountSenderIdentity(status) {
  const accountId = status?.activeAccountId || status?.accountId || null;
  const account =
    status?.accounts?.find((candidate) => candidate.accountId === accountId) ||
    status ||
    {};
  return {
    name:
      String(account.remark || account.name || account.displayName || "").trim(),
    email: String(account.email || "").trim(),
  };
}

function recipientAddressObjects(values) {
  return (Array.isArray(values) ? values : [])
    .filter((email) => typeof email === "string" && email.trim())
    .map((email) => ({ name: null, email: email.trim() }));
}

function toOutboxMessage(item, drafts, senderIdentity, displayedRole = "outbox") {
  const draft = drafts.find((candidate) => candidate.id === item.draft_id);
  const status = outboxCopy[item.status] || item.status || "状态未知";
  const recipients = item.recipients || [];
  const recipientLabel = recipients.join(", ") || "未知收件人";
  const recipientGroups =
    item.recipient_groups &&
    typeof item.recipient_groups === "object" &&
    !Array.isArray(item.recipient_groups)
      ? item.recipient_groups
      : null;
  return {
    id: item.id,
    kind: "outbox",
    displayed_role: displayedRole,
    subject: item.subject || draft?.subject || status,
    sender: senderIdentity,
    list_sender: { name: recipientLabel, email: recipients[0] || "" },
    to: recipientAddressObjects(recipientGroups?.to),
    cc: recipientAddressObjects(recipientGroups?.cc),
    bcc: recipientAddressObjects(recipientGroups?.bcc),
    recipient_groups: recipientGroups,
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
  const recipients = [
    ...(message.to || []),
    ...(message.cc || []),
    ...(message.bcc || []),
  ];
  const firstRecipient = recipients[0] || null;
  const recipientLabel =
    recipients
      .map((recipient) => recipient.name || recipient.email)
      .filter(Boolean)
      .join(", ") || "未知收件人";
  return {
    ...message,
    kind: "sent",
    list_sender: {
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
  const id = localMessageId(message);
  return id !== null && paginatedMailboxRoles.includes(messageRole(message))
    ? `message:${id}`
    : null;
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
      value.body_text.trim() ||
      value.format?.body_html?.trim() ||
      value.reply_context),
  );
}

function createComposer(
  value = emptyCompose,
  draftId = null,
  persistedDraft = null,
  { forwardWarnings = [] } = {},
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
    startMinimized: false,
    minimized: false,
    optimizationCache: {},
    attachmentOperations: { add: null, remove: {} },
    forwardWarnings: [...forwardWarnings],
  };
}

function draftToRequest(draft) {
  return {
    to: [...(draft?.to || [])],
    cc: [...(draft?.cc || [])],
    bcc: [...(draft?.bcc || [])],
    subject: draft?.subject || "",
    body_text: draft?.body_text || "",
    format: {
      body_html: draft?.format?.body_html || null,
      stationery: draft?.format?.stationery || "none",
      send_stationery: draft?.format?.send_stationery === true,
    },
    reply_context: draft?.reply_context
      ? structuredClone(draft.reply_context)
      : null,
  };
}

function upsertDraft(items, draft) {
  return [draft, ...items.filter((item) => item.id !== draft.id)];
}

export function App() {
  const appUpdate = useAppUpdate();
  const [appearance, setAppearance] = useState(getInitialAppearance);
  const [activeFolder, setActiveFolder] = useState("inbox");
  const [messages, setMessages] = useState([]);
  const [sentMessages, setSentMessages] = useState([]);
  const [archiveMessages, setArchiveMessages] = useState([]);
  const [trashMessages, setTrashMessages] = useState([]);
  const [drafts, setDrafts] = useState([]);
  const [outbox, setOutbox] = useState([]);
  const [sentOutboxFallbacks, setSentOutboxFallbacks] = useState([]);
  const [selectedMessageId, setSelectedMessageId] = useState(null);
  const [selectedMessage, setSelectedMessage] = useState(null);
  const [isReaderOpen, setIsReaderOpen] = useState(false);
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
  const [manualSyncFeedback, setManualSyncFeedback] = useState(null);
  const [mailboxLoadStates, setMailboxLoadStates] = useState(() =>
    createMailboxLoadStates(),
  );
  const [mailboxPageStates, setMailboxPageStates] = useState(
    createMailboxPageStates,
  );
  const [starredMailboxPageStates, setStarredMailboxPageStates] = useState(
    createStarredMailboxPageStates,
  );
  const [starredViewRetention, setStarredViewRetention] = useState({
    accountId: null,
    items: [],
  });
  const [mailboxCapabilities, setMailboxCapabilities] = useState(null);
  const [remoteSearch, setRemoteSearch] = useState(null);
  const [messageActionStates, setMessageActionStates] = useState({});
  const [attachmentSaveStates, setAttachmentSaveStates] = useState({});
  const [forwardPreparationStates, setForwardPreparationStates] = useState({});
  const [permanentDelete, setPermanentDelete] = useState(null);
  const [archiveFolderDialog, setArchiveFolderDialog] = useState(null);
  const [isSidebarOpen, setIsSidebarOpen] = useState(false);
  const [composer, setComposer] = useState(null);
  const [composeRestoreRequest, setComposeRestoreRequest] = useState(0);
  const [backgroundSendCounts, setBackgroundSendCounts] = useState({});
  const [sentAttentionByAccount, setSentAttentionByAccount] = useState({});
  const [retryingOutboxId, setRetryingOutboxId] = useState(null);
  const [deliveryUnknownDecision, setDeliveryUnknownDecision] = useState(null);
  const [settings, setSettings] = useState(defaultSettings);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [settingsFocusTarget, setSettingsFocusTarget] = useState(null);
  const [settingsSaveStatus, setSettingsSaveStatus] = useState("idle");
  const [accountPresets, setAccountPresets] = useState([]);
  const [accountStatus, setAccountStatus] = useState({ configured: null });
  const [accountSubmitStatus, setAccountSubmitStatus] = useState("idle");
  const [accountError, setAccountError] = useState(null);
  const [accountErrorProvider, setAccountErrorProvider] = useState(null);
  const [isAccountRepairVisible, setIsAccountRepairVisible] = useState(false);
  const [profileAvatars, setProfileAvatars] = useState([]);
  const [toast, setToast] = useState(null);
  const [referenceJump, setReferenceJump] = useState(null);
  const [readerMotion, setReaderMotion] = useState("idle");
  const [readerExitSpeed, setReaderExitSpeed] = useState("normal");
  const [contactDetailMotion, setContactDetailMotion] = useState("idle");
  const [contactDetailExitSpeed, setContactDetailExitSpeed] = useState("normal");
  const [mailListMotion, setMailListMotion] = useState("expanded");
  const isWideMailWorkspace = useMediaQuery(wideMailWorkspaceQuery, true);

  const composerRef = useRef(null);
  const composerSessionsRef = useRef(new Map());
  const composeBodySnapshotProviderRef = useRef(null);
  const composeBodyAutosaveTimerRef = useRef(null);
  const switchMinimizedComposerDraftRef = useRef(null);
  const draftSaveRef = useRef(null);
  const exitFlushRef = useRef(null);
  const networkActionsAvailableRef = useRef(false);
  const draftsRef = useRef([]);
  const mailboxLoadStatesRef = useRef(mailboxLoadStates);
  const selectionRequestRef = useRef(0);
  const selectedMessageIdRef = useRef(null);
  const readerOpenRef = useRef(false);
  const messageBodyCacheRef = useRef(new Map());
  const readerTranslationQueueRef = useRef(null);
  if (!readerTranslationQueueRef.current) {
    readerTranslationQueueRef.current = createReaderTranslationQueue({
      maxConcurrent: 2,
      maxEntries: 50,
    });
  }
  const accountViewsRef = useRef(new Map());
  const accountViewLoadsRef = useRef(new Map());
  const accountViewSnapshotLockRef = useRef(null);
  const rememberActiveAccountViewRef = useRef(() => null);
  const mailListScrollPositionsRef = useRef(new Map());
  const starredViewMessagesRef = useRef({ accountId: null, items: [] });
  const draftsRequestRef = useRef(0);
  const outboxRequestRef = useRef(0);
  const mailboxPageRequestsRef = useRef(new Map());
  const mailboxRefreshFlightsRef = useRef(new Map());
  const mailboxProjectionInvalidationsRef = useRef(new Set());
  const mailboxSyncProgressRef = useRef(new Map());
  const mailboxCapabilityRequestRef = useRef(new Map());
  const mailboxRoleCreationRequestsRef = useRef(new Map());
  const archiveFolderRequestRef = useRef(null);
  const remoteSearchRequestRef = useRef(0);
  const accountStatusRef = useRef(accountStatus);
  const activeAccountIdRef = useRef(null);
  const activeFolderRef = useRef("inbox");
  const mailboxQueryRef = useRef("");
  const selectedContactEmailRef = useRef(null);
  const selectedContactAccountIdRef = useRef(null);
  const accountSwitchRequestRef = useRef(0);
  const referenceJumpRequestRef = useRef(0);
  const contactsRequestRef = useRef(0);
  const contactMessagesRequestRef = useRef(0);
  const contactMessageOpenRequestRef = useRef(0);
  const navigationIntentRef = useRef(0);
  const pendingNotificationOpenRef = useRef(null);
  const resumeNotificationOpenRef = useRef(null);
  const replyPreparationRequestRef = useRef(0);
  const forwardPreparationRequestRef = useRef(0);
  const preservedContactContextRef = useRef(null);
  const favoriteOwnershipWarningRef = useRef(false);
  const contactsRefreshTimerRef = useRef(null);
  const contactsInvalidatedAccountsRef = useRef(new Set());
  const starRequestRef = useRef(new Map());
  const starStateRef = useRef(new Map());
  const settingsSaveRequestRef = useRef(0);
  const toastSequenceRef = useRef(0);
  const syncFeedbackSequenceRef = useRef(0);
  const syncFeedbackTimerRef = useRef(null);
  const drawerTriggerRef = useRef(null);
  const consequentialActionRef = useRef(null);
  const readerMotionRef = useRef("idle");
  const readerAfterExitRef = useRef(null);
  const completeReaderMotionRef = useRef(() => {});
  const contactDetailMotionRef = useRef("idle");
  const contactDetailAfterExitRef = useRef(null);
  const completeContactDetailMotionRef = useRef(() => {});
  const mailListMotionRef = useRef("expanded");
  const activeMailListIntentRef = useRef(null);
  const queuedMailListIntentRef = useRef(null);
  const folderMotionRequestRef = useRef(0);
  const startMailListIntentRef = useRef(() => {});
  const completeMailListMotionRef = useRef(() => {});
  const commitFolderChangeRef = useRef(() => {});
  const isWideMailWorkspaceRef = useRef(isWideMailWorkspace);
  isWideMailWorkspaceRef.current = isWideMailWorkspace;
  const invalidatePreparedFolderMotion = useCallback(() => {
    folderMotionRequestRef.current += 1;
  }, []);
  const platform = /Mac|iPhone|iPad/.test(navigator.platform)
    ? "mac"
    : "windows";
  const networkActionsAvailable = canUseAccountNetwork(accountStatus);
  const accountNeedsRepair = shouldRepairAccount(accountStatus);
  const activeAccountId =
    accountStatus.activeAccountId || accountStatus.accountId || null;
  const activeBackgroundSendCount = activeAccountId
    ? backgroundSendCounts[activeAccountId] || 0
    : 0;
  const mailListScrollStateKey = JSON.stringify([
    activeAccountId,
    activeFolder,
  ]);
  networkActionsAvailableRef.current = networkActionsAvailable;
  accountStatusRef.current = accountStatus;
  draftsRef.current = drafts;
  mailboxLoadStatesRef.current = mailboxLoadStates;
  activeAccountIdRef.current = activeAccountId;
  activeFolderRef.current = activeFolder;
  mailboxQueryRef.current = query;
  selectedContactEmailRef.current = selectedContactEmail;
  selectedContactAccountIdRef.current = selectedContactAccountId;

  useEffect(() => {
    const visibleAccountId =
      activeFolder === "starred" && !isSettingsOpen
        ? activeAccountId
        : null;
    setStarredViewRetention((current) => {
      if (
        visibleAccountId !== null &&
        current.accountId === visibleAccountId
      ) {
        return current;
      }
      if (
        visibleAccountId === null &&
        current.accountId === null &&
        current.items.length === 0
      ) {
        return current;
      }
      return { accountId: visibleAccountId, items: [] };
    });
  }, [activeAccountId, activeFolder, isSettingsOpen]);

  useEffect(() => {
    if (activeFolder !== "starred" || mailListMotion !== "collapsed") {
      return;
    }
    setStarredViewRetention((current) =>
      current.items.length
        ? { accountId: activeAccountId, items: [] }
        : current,
    );
  }, [activeAccountId, activeFolder, mailListMotion]);

  useEffect(() => {
    setStarredViewRetention((current) =>
      current.items.length
        ? { accountId: current.accountId, items: [] }
        : current,
    );
  }, [query]);

  const markSentAttention = useCallback((accountId) => {
    if (!accountId) return;
    if (
      activeAccountIdRef.current === accountId &&
      activeFolderRef.current === "sent"
    ) {
      return;
    }
    setSentAttentionByAccount((current) =>
      current[accountId] ? current : { ...current, [accountId]: true },
    );
  }, []);

  useEffect(() => {
    if (!activeAccountId || activeFolder !== "sent") return;
    setSentAttentionByAccount((current) => {
      if (!current[activeAccountId]) return current;
      const next = { ...current };
      delete next[activeAccountId];
      return next;
    });
  }, [activeAccountId, activeFolder]);

  const getMailListScrollTop = useCallback(
    (key) => mailListScrollPositionsRef.current.get(key) ?? 0,
    [],
  );
  const saveMailListScrollTop = useCallback((key, scrollTop) => {
    if (!Number.isFinite(scrollTop)) return;
    mailListScrollPositionsRef.current.set(key, Math.max(0, scrollTop));
  }, []);

  const setReaderMotionPhase = useCallback((phase) => {
    readerMotionRef.current = phase;
    setReaderMotion(phase);
  }, []);

  const setReaderOpenState = useCallback((open) => {
    readerOpenRef.current = open;
    setIsReaderOpen(open);
  }, []);

  const presentReader = useCallback(
    (phase) => {
      readerAfterExitRef.current = null;
      setReaderExitSpeed("normal");
      setReaderOpenState(true);
      setReaderMotionPhase(prefersReducedMotion() ? "open" : phase);
    },
    [setReaderMotionPhase, setReaderOpenState],
  );

  const setContactDetailMotionPhase = useCallback((phase) => {
    contactDetailMotionRef.current = phase;
    setContactDetailMotion(phase);
  }, []);

  const presentContactDetail = useCallback(
    (phase) => {
      contactDetailAfterExitRef.current = null;
      setContactDetailExitSpeed("normal");
      setContactDetailMotionPhase(prefersReducedMotion() ? "open" : phase);
    },
    [setContactDetailMotionPhase],
  );

  const setMailListMotionPhase = useCallback((phase) => {
    mailListMotionRef.current = phase;
    setMailListMotion(phase);
  }, []);

  const ensureMailListVisibleForReader = useCallback(() => {
    if (mailListMotionRef.current === "expanded") return;
    activeMailListIntentRef.current = null;
    queuedMailListIntentRef.current = null;
    setMailListMotionPhase("expanded");
  }, [setMailListMotionPhase]);

  useEffect(() => {
    const accountId = activeAccountIdRef.current;
    if (!accountId) return;
    if (accountViewSnapshotLockRef.current) return;
    const current = accountViewsRef.current.get(accountId) || {};
    accountViewsRef.current.set(accountId, {
      ...current,
      messages,
      sentMessages,
      archiveMessages,
      trashMessages,
      drafts,
      outbox,
      sentOutboxFallbacks,
      selectedMessageId,
      selectedMessage,
      mailboxPageStates,
      starredMailboxPageStates,
      mailboxCapabilities,
    });
  }, [
    archiveMessages,
    drafts,
    mailboxCapabilities,
    mailboxPageStates,
    messages,
    outbox,
    selectedMessage,
    selectedMessageId,
    sentMessages,
    sentOutboxFallbacks,
    starredMailboxPageStates,
    trashMessages,
  ]);

  const showToast = useCallback(
    (message, tone = "success", persistent = false) => {
      toastSequenceRef.current += 1;
      setToast({
        message,
        tone,
        persistent,
        exiting: false,
        id: toastSequenceRef.current,
      });
    },
    [],
  );

  const publishManualSyncFeedback = useCallback(
    (feedback, dismissAfterCompletion = false) => {
      if (syncFeedbackTimerRef.current) {
        window.clearTimeout(syncFeedbackTimerRef.current);
        syncFeedbackTimerRef.current = null;
      }
      setManualSyncFeedback(feedback);
      if (!feedback || !dismissAfterCompletion) return;
      syncFeedbackTimerRef.current = window.setTimeout(() => {
        setManualSyncFeedback((current) =>
          current?.id === feedback.id ? null : current,
        );
        syncFeedbackTimerRef.current = null;
      }, manualSyncFeedbackVisibleMs);
    },
    [],
  );

  const clearForwardPreparationRequest = useCallback(
    (messageId, requestId) => {
      setForwardPreparationStates((current) => {
        if (current[messageId]?.requestId !== requestId) return current;
        const next = { ...current };
        delete next[messageId];
        return next;
      });
    },
    [],
  );

  const invalidateForwardPreparationsForAccount = useCallback((accountId) => {
    if (!accountId) return;
    setForwardPreparationStates((current) => {
      const next = { ...current };
      let changed = false;
      for (const [messageId, state] of Object.entries(current)) {
        if (
          state?.status === "loading" &&
          state.sourceAccountId === accountId
        ) {
          delete next[messageId];
          changed = true;
        }
      }
      return changed ? next : current;
    });
  }, []);

  const updateMailboxLoadState = useCallback((folder, update) => {
    setMailboxLoadStates((current) => ({
      ...current,
      [folder]: {
        ...current[folder],
        ...(typeof update === "function" ? update(current[folder]) : update),
      },
    }));
  }, []);

  const beginMailboxLoading = useCallback((phase = "syncing") => {
    setMailboxLoadStates(createMailboxLoadStates(phase));
  }, []);

  const settleMailboxSnapshot = useCallback((view, { preserveSyncing = true } = {}) => {
    const counts = {
      inbox: view?.messages?.length || 0,
      sent: view?.sentMessages?.length || 0,
      archive: view?.archiveMessages?.length || 0,
      trash: view?.trashMessages?.length || 0,
      drafts: view?.drafts?.length || 0,
      outbox: view?.outbox?.length || 0,
    };
    setMailboxLoadStates((current) =>
      Object.fromEntries(
        mailboxFolders.map((folder) => [
          folder,
          preserveSyncing && current[folder]?.phase === "syncing"
            ? current[folder]
            : { phase: "ready", completed: counts[folder], total: null },
        ]),
      ),
    );
  }, []);

  const dismissToast = useCallback(() => {
    setToast((current) =>
      current && !current.exiting ? { ...current, exiting: true } : current,
    );
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
      } catch (error) {
        showToast(describeError(error, "头像没有移除，请重试"), "error");
      }
    },
    [showToast],
  );

  const commitComposer = useCallback((valueOrUpdater) => {
    const previous = composerRef.current;
    const next =
      typeof valueOrUpdater === "function"
        ? valueOrUpdater(previous)
        : valueOrUpdater;
    if (
      composeBodyAutosaveTimerRef.current !== null &&
      (!next ||
        next.sessionId !== previous?.sessionId ||
        next.dirty !== previous?.dirty ||
        next.locked !== previous?.locked ||
        next.revision !== previous?.revision ||
        next.saveStatus !== previous?.saveStatus)
    ) {
      window.clearTimeout(composeBodyAutosaveTimerRef.current);
      composeBodyAutosaveTimerRef.current = null;
    }
    const accountId = activeAccountIdRef.current;
    if (accountId) {
      if (next) composerSessionsRef.current.set(accountId, next);
      else composerSessionsRef.current.delete(accountId);
    }
    composerRef.current = next;
    setComposer(next);
    if (previous && !next && pendingNotificationOpenRef.current) {
      Promise.resolve().then(() => resumeNotificationOpenRef.current?.());
    }
    return next;
  }, []);

  const activateComposerForAccount = useCallback((accountId) => {
    if (composeBodyAutosaveTimerRef.current !== null) {
      window.clearTimeout(composeBodyAutosaveTimerRef.current);
      composeBodyAutosaveTimerRef.current = null;
    }
    const next = accountId
      ? composerSessionsRef.current.get(accountId) || null
      : null;
    composerRef.current = next;
    setComposer(next);
    return next;
  }, []);

  useEffect(
    () => () => {
      if (composeBodyAutosaveTimerRef.current !== null) {
        window.clearTimeout(composeBodyAutosaveTimerRef.current);
      }
    },
    [],
  );

  const handleComposeBodySnapshotProviderChange = useCallback(
    (sessionId, provider) => {
      if (typeof provider === "function" && sessionId) {
        composeBodySnapshotProviderRef.current = { sessionId, provider };
        return;
      }
      if (
        !sessionId ||
        composeBodySnapshotProviderRef.current?.sessionId === sessionId
      ) {
        composeBodySnapshotProviderRef.current = null;
      }
    },
    [],
  );

  const flushComposeBodySnapshot = useCallback((sessionId) => {
    const registered = composeBodySnapshotProviderRef.current;
    if (!sessionId || registered?.sessionId !== sessionId) return null;
    return registered.provider();
  }, []);

  const openComposer = useCallback(
    (
      value = emptyCompose,
      draftId = null,
      persistedDraft = null,
      options = {},
    ) => {
      if (composerRef.current) return composerRef.current;
      replyPreparationRequestRef.current += 1;
      return commitComposer(
        createComposer(value, draftId, persistedDraft, options),
      );
    },
    [commitComposer],
  );

  const openOrRestoreComposer = useCallback(() => {
    if (composerRef.current) {
      setComposeRestoreRequest((request) => request + 1);
      return composerRef.current;
    }
    return openComposer();
  }, [openComposer]);

  const clearSelection = useCallback((options = {}) => {
    const navigationIntentId = options?.navigationIntentId ?? null;
    if (
      navigationIntentId !== null &&
      navigationIntentRef.current !== navigationIntentId
    ) {
      return false;
    }
    if (navigationIntentId === null) {
      navigationIntentRef.current += 1;
      pendingNotificationOpenRef.current = null;
    }
    if (!options?.preserveContactMessageOpenRequest) {
      contactMessageOpenRequestRef.current += 1;
    }
    replyPreparationRequestRef.current += 1;
    selectionRequestRef.current += 1;
    selectedMessageIdRef.current = null;
    setSelectedMessageId(null);
    setSelectedMessage(null);
    setReaderOpenState(false);
    setMessageError(null);
    setIsMessageLoading(false);
    readerAfterExitRef.current = null;
    setReaderMotionPhase("idle");
    return true;
  }, [setReaderMotionPhase, setReaderOpenState]);

  const clearContactSelection = useCallback(() => {
    selectedContactEmailRef.current = null;
    selectedContactAccountIdRef.current = null;
    setSelectedContactEmail(null);
    setSelectedContactAccountId(null);
    contactDetailAfterExitRef.current = null;
    setContactDetailMotionPhase("idle");
  }, [setContactDetailMotionPhase]);

  const commitRoleItems = useCallback((role, update, accountId) => {
    const targetAccountId =
      accountId || activeAccountIdRef.current || "unscoped";
    const field = mailboxViewField(role);
    const apply = (current) =>
      typeof update === "function" ? update(current || []) : update || [];
    const updateView = (next) => {
      const view = accountViewsRef.current.get(targetAccountId) || {};
      accountViewsRef.current.set(targetAccountId, { ...view, [field]: next });
      return next;
    };

    if (activeAccountIdRef.current !== targetAccountId) {
      const view = accountViewsRef.current.get(targetAccountId) || {};
      return updateView(apply(view[field] || []));
    }

    const setter =
      role === "inbox"
        ? setMessages
        : role === "sent"
          ? setSentMessages
          : role === "archive"
            ? setArchiveMessages
            : setTrashMessages;
    setter((current) => updateView(apply(current)));
    return null;
  }, []);

  const commitMailboxPageState = useCallback(
    (role, update, accountId = activeAccountIdRef.current) => {
      if (!accountId) return;
      const apply = (current) =>
        typeof update === "function"
          ? update(current || emptyMailboxPageState())
          : update;
      if (activeAccountIdRef.current !== accountId) {
        const view = accountViewsRef.current.get(accountId) || {};
        const pages = view.mailboxPageStates || createMailboxPageStates();
        accountViewsRef.current.set(accountId, {
          ...view,
          mailboxPageStates: {
            ...pages,
            [role]: apply(pages[role]),
          },
        });
        return;
      }
      setMailboxPageStates((current) => {
        const next = { ...current, [role]: apply(current[role]) };
        const view = accountViewsRef.current.get(accountId) || {};
        accountViewsRef.current.set(accountId, {
          ...view,
          mailboxPageStates: next,
        });
        return next;
      });
    },
    [],
  );

  const commitStarredMailboxPageState = useCallback(
    (role, update, accountId = activeAccountIdRef.current) => {
      if (!accountId) return;
      const apply = (current) =>
        typeof update === "function"
          ? update(current || emptyMailboxPageState({ items: [] }))
          : update;
      if (activeAccountIdRef.current !== accountId) {
        const view = accountViewsRef.current.get(accountId) || {};
        const pages =
          view.starredMailboxPageStates || createStarredMailboxPageStates();
        accountViewsRef.current.set(accountId, {
          ...view,
          starredMailboxPageStates: {
            ...pages,
            [role]: apply(pages[role]),
          },
        });
        return;
      }
      setStarredMailboxPageStates((current) => {
        const next = { ...current, [role]: apply(current[role]) };
        const view = accountViewsRef.current.get(accountId) || {};
        accountViewsRef.current.set(accountId, {
          ...view,
          starredMailboxPageStates: next,
        });
        return next;
      });
    },
    [],
  );

  const mergeRemoteMessage = useCallback(
    (target, update, accountId = activeAccountIdRef.current) => {
      const targetId = localMessageId(target);
      if (targetId === null || !accountId) return;
      const apply = (message) =>
        localMessageId(message) === targetId ? update(message) : message;
      for (const role of paginatedMailboxRoles) {
        commitRoleItems(role, (items) => items.map(apply), accountId);
      }
      for (const role of starredMailboxRoles) {
        commitStarredMailboxPageState(
          role,
          (state) => ({
            ...state,
            items: (state.items || []).map(apply),
          }),
          accountId,
        );
      }
      setRemoteSearch((current) =>
        current?.accountId === accountId
          ? { ...current, items: current.items.map(apply) }
          : current,
      );
      if (
        activeAccountIdRef.current === accountId ||
        selectedContactAccountIdRef.current === accountId
      ) {
        setContactMessages((current) => current.map(apply));
      }
      if (activeAccountIdRef.current === accountId) {
        setSelectedMessage((current) =>
          localMessageId(current) === targetId ? update(current) : current,
        );
      }
    },
    [commitRoleItems, commitStarredMailboxPageState],
  );

  const handleSelect = useCallback(
    async (message, forceFetch = false, options = {}) => {
      invalidatePreparedFolderMotion();
      const navigationIntentId = options?.navigationIntentId ?? null;
      if (
        navigationIntentId !== null &&
        navigationIntentRef.current !== navigationIntentId
      ) {
        return;
      }
      if (navigationIntentId === null) {
        navigationIntentRef.current += 1;
        pendingNotificationOpenRef.current = null;
      }
      const contactMessageOpenRequestId =
        options?.contactMessageOpenRequestId ?? null;
      if (
        contactMessageOpenRequestId !== null &&
        contactMessageOpenRequestRef.current !== contactMessageOpenRequestId
      ) {
        return;
      }
      if (contactMessageOpenRequestId === null) {
        contactMessageOpenRequestRef.current += 1;
      }
      replyPreparationRequestRef.current += 1;
      if (!message) return;
      if (message.kind === "draft") {
        if (localMessageId(message) === null) return;
        const current = composerRef.current;
        if (current?.draftId === message.draft.id) {
          setComposeRestoreRequest((request) => request + 1);
          return;
        }
        if (current) {
          if (current.minimized) {
            await switchMinimizedComposerDraftRef.current?.(message.draft);
          }
          return;
        }
        openComposer(
          draftToRequest(message.draft),
          message.draft.id,
          message.draft,
        );
        return;
      }

      const accountId = activeAccountIdRef.current || "unscoped";
      const messageId = localMessageId(message);
      const role = messageRole(message);
      const isRemoteMailboxMessage =
        messageId !== null &&
        paginatedMailboxRoles.includes(role) &&
        !message.contactHistory &&
        message.kind !== "outbox";
      const selectedStarKey = scopedRemoteFlagKey(message, accountId);
      if (selectedStarKey && !starStateRef.current.has(selectedStarKey)) {
        starStateRef.current.set(
          selectedStarKey,
          hasFlag(message, "\\Flagged"),
        );
      }
      const shouldMarkRead =
        isRemoteMailboxMessage &&
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
      const bodyIsReady = messageBodyIsReady(displayMessage);
      const needsHtmlHydration =
        displayMessage.body_html_available === true &&
        displayMessage.body_html_loaded !== true;
      const needsAttachmentHydration =
        isRemoteMailboxMessage &&
        !Array.isArray(displayMessage.attachments) &&
        Array.isArray(displayMessage.attachment_names) &&
        displayMessage.attachment_names.length > 0;
      const requestId = selectionRequestRef.current + 1;
      selectionRequestRef.current = requestId;
      const previousSelectionId = selectedMessageIdRef.current;
      const nextSelectionId = localMessageId(message);
      if (nextSelectionId === null) {
        setIsMessageLoading(false);
        setMessageError("这封邮件缺少可用的本地标识，无法打开。");
        return;
      }
      ensureMailListVisibleForReader();
      if (
        previousSelectionId === null ||
        readerMotionRef.current === "idle"
      ) {
        presentReader("entering");
      } else if (previousSelectionId !== nextSelectionId) {
        presentReader("open");
      }
      setIsMessageLoading(forceFetch || !bodyIsReady);
      selectedMessageIdRef.current = nextSelectionId;
      setSelectedMessageId(nextSelectionId);
      setSelectedMessage(displayMessage);
      setMessageError(null);

      if (shouldMarkRead) {
        mergeRemoteMessage(message, withSeenFlag, accountId);
        void mailApi
          .setMessageSeen(messageId, true)
          .catch((error) => {
            mergeRemoteMessage(
              message,
              (current) => withSystemFlag(current, "\\Seen", false),
              accountId,
            );
            showToast(describeError(error, "已读状态保存失败"), "error");
          });
      }

      if (
        !forceFetch &&
        bodyIsReady &&
        !needsHtmlHydration &&
        !needsAttachmentHydration
      ) {
        setIsMessageLoading(false);
        return;
      }

      if (displayMessage.kind === "outbox") {
        setIsMessageLoading(true);
        try {
          const hydrated = {
            ...displayMessage,
            ...(await mailApi.fetchOutboxMessage(message.outbox.id)),
          };
          if (
            selectionRequestRef.current !== requestId ||
            selectedMessageIdRef.current !== nextSelectionId
          ) {
            return;
          }
          if (!messageBodyIsReady(hydrated)) {
            throw new Error("邮件正文加载结果不完整，请重新加载。");
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

      try {
        const fetchedMessage =
          displayMessage.contactHistory || isRemoteMailboxMessage
            ? await mailApi.fetchMailboxMessage(messageId)
            : undefined;
        let fullMessage =
          isRemoteMailboxMessage && fetchedMessage
            ? toMailboxDisplayMessage(
                { ...displayMessage, ...fetchedMessage },
                role,
              )
            : fetchedMessage;
        if (displayMessage.contactHistory && fullMessage) {
          fullMessage = { ...fullMessage, contactHistory: true };
        }
        if (shouldMarkRead && fullMessage) {
          fullMessage = withSeenFlag(fullMessage);
        }
        if (
          selectionRequestRef.current !== requestId ||
          selectedMessageIdRef.current !== nextSelectionId
        ) {
          return;
        }
        if (!messageBodyIsReady(fullMessage)) {
          throw new Error("邮件正文加载结果不完整，请重新加载。");
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
        messageBodyCacheRef.current.set(
          messageCacheKey(fullMessage, accountId),
          bodySnapshot(fullMessage),
        );
        setSelectedMessage(fullMessage);
        if (isRemoteMailboxMessage) {
          commitRoleItems(
            role,
            (items) =>
              items.map((mail) =>
                localMessageId(mail) === messageId
                  ? { ...mail, ...fullMessage }
                  : mail,
              ),
            accountId,
          );
        }
      } catch (error) {
        if (selectionRequestRef.current === requestId) {
          const messageText = describeError(error, "邮件正文加载失败");
          setMessageError(messageText);
        }
      } finally {
        if (selectionRequestRef.current === requestId)
          setIsMessageLoading(false);
      }
    },
    [
      commitRoleItems,
      ensureMailListVisibleForReader,
      invalidatePreparedFolderMotion,
      mergeRemoteMessage,
      openComposer,
      presentReader,
      showToast,
    ],
  );

  const applyMessageStarState = useCallback((target, starred, accountId) => {
    const scopedKey = scopedRemoteFlagKey(target, accountId);
    if (!scopedKey) return;
    starStateRef.current.set(scopedKey, starred);
    mergeRemoteMessage(
      target,
      (message) => withSystemFlag(message, "\\Flagged", starred),
      accountId,
    );
  }, [mergeRemoteMessage]);

  const updateStarredViewRetention = useCallback(
    (message, starred, accountId) => {
      if (
        activeFolderRef.current !== "starred" ||
        activeAccountIdRef.current !== accountId
      ) {
        return;
      }
      const messageId = localMessageId(message);
      if (messageId === null) return;
      setStarredViewRetention((current) => {
        const displayedItems =
          starredViewMessagesRef.current.accountId === accountId
            ? starredViewMessagesRef.current.items
            : [];
        const snapshotItems =
          current.accountId === accountId && current.items.length
            ? current.items
            : displayedItems;
        let found = false;
        const nextItems = snapshotItems.map((item) => {
          if (localMessageId(item) !== messageId) return item;
          found = true;
          return withSystemFlag(item, "\\Flagged", starred);
        });
        return {
          accountId,
          items: found
            ? nextItems
            : [...nextItems, withSystemFlag(message, "\\Flagged", starred)],
        };
      });
    },
    [],
  );

  const handleToggleStar = useCallback(
    async (message) => {
      const accountId = activeAccountIdRef.current || "unscoped";
      const messageId = localMessageId(message);
      const key = scopedRemoteFlagKey(message, accountId);
      if (!key || messageId === null) return;
      const starred = !hasFlag(message, "\\Flagged");
      const requestId = (starRequestRef.current.get(key)?.requestId || 0) + 1;
      starRequestRef.current.set(key, { requestId, starred });
      updateStarredViewRetention(message, starred, accountId);
      applyMessageStarState(message, starred, accountId);
      try {
        const receipt = await mailApi.setMessageStarredById(
          messageId,
          starred,
        );
        if (starRequestRef.current.get(key)?.requestId === requestId) {
          starRequestRef.current.delete(key);
          setMessageActionStates((current) => ({
            ...current,
            [`${accountId}:${messageId}:starred`]:
              mutationActionState(receipt),
          }));
        }
      } catch (error) {
        if (starRequestRef.current.get(key)?.requestId !== requestId) return;
        starRequestRef.current.delete(key);
        updateStarredViewRetention(message, !starred, accountId);
        applyMessageStarState(message, !starred, accountId);
        showToast(describeError(error, "收藏状态保存失败"), "error");
      }
    },
    [applyMessageStarState, showToast, updateStarredViewRetention],
  );

  const refreshMailboxCapabilities = useCallback(async (accountId) => {
    if (!accountId) return {};
    const requestId =
      (mailboxCapabilityRequestRef.current.get(accountId) || 0) + 1;
    mailboxCapabilityRequestRef.current.set(accountId, requestId);
    const next = capabilityMap(
      await mailApi.getMailboxCapabilities(accountId),
    );
    const view = accountViewsRef.current.get(accountId) || {};
    accountViewsRef.current.set(accountId, {
      ...view,
      mailboxCapabilities: next,
    });
    if (
      activeAccountIdRef.current === accountId &&
      mailboxCapabilityRequestRef.current.get(accountId) === requestId
    ) {
      setMailboxCapabilities(next);
    }
    return next;
  }, []);

  const loadMailboxRolePage = useCallback(
    async ({
      accountId = activeAccountIdRef.current,
      role,
      cursor = null,
      query: pageQuery = null,
      append = false,
      mergeExisting = false,
      selectFirst = false,
      preserveSyncing = false,
      recoveringCursor = false,
      settleLoadMore = true,
    }) => {
      if (!accountId || !paginatedMailboxRoles.includes(role)) return null;
      const normalizedQuery = normalizedMailboxQuery(pageQuery);
      const requestKey = `${accountId}:${role}:${normalizedQuery || ""}`;
      const requestToken = Symbol(requestKey);
      mailboxPageRequestsRef.current.set(requestKey, requestToken);
      if (
        !normalizedQuery &&
        activeAccountIdRef.current === accountId &&
        !preserveSyncing
      ) {
        updateMailboxLoadState(role, (current) => ({
          ...current,
          phase: current?.completed ? "syncing" : "loading",
        }));
        if (append) {
          commitMailboxPageState(
            role,
            (current) => ({
              ...current,
              loadMorePhase: "loading",
              loadMoreError: null,
            }),
            accountId,
          );
        }
      }

      try {
        const response = cursor
          ? await mailApi.loadOlderMailboxPage(
              accountId,
              role,
              cursor,
              mailboxPageSize,
              normalizedQuery,
            )
          : await mailApi.listMailboxPage(
              accountId,
              role,
              null,
              mailboxPageSize,
              normalizedQuery,
            );
        if (mailboxPageRequestsRef.current.get(requestKey) !== requestToken) {
          return null;
        }
        const normalized = normalizeMailboxPage(
          response,
          role,
          normalizedQuery,
        );
        normalized.items = normalized.items.map((message) => {
          const cachedBody = messageBodyCacheRef.current.get(
            messageCacheKey(message, accountId),
          );
          let resolved = cachedBody ? { ...message, ...cachedBody } : message;
          const key = scopedRemoteFlagKey(resolved, accountId);
          const pending = key ? starRequestRef.current.get(key) : null;
          const starred = pending?.starred ?? hasFlag(resolved, "\\Flagged");
          if (key) starStateRef.current.set(key, starred);
          if (pending) {
            resolved = withSystemFlag(resolved, "\\Flagged", starred);
          }
          return resolved;
        });

        if (normalizedQuery) {
          return normalized;
        }

        const existingView = accountViewsRef.current.get(accountId) || {};
        const existingItems =
          existingView[mailboxViewField(role)] || [];
        const existingPageState =
          existingView.mailboxPageStates?.[role] || null;
        const shouldMergeRefresh =
          mergeExisting && !append && existingItems.length > 0;
        const committedItems = shouldMergeRefresh
          ? mergeRefreshedMailboxItems(existingItems, normalized.items)
          : append
            ? appendMailboxItems(existingItems, normalized.items)
            : normalized.items;
        const committedPageState =
          shouldMergeRefresh && existingPageState?.initialized
            ? {
                ...normalized.state,
                nextCursor: existingPageState.nextCursor,
                hasMoreLocal: existingPageState.hasMoreLocal,
                remoteHistoryState: existingPageState.remoteHistoryState,
                endReached: existingPageState.endReached,
              }
            : normalized.state;
        const appendedItemCount = Math.max(
          0,
          committedItems.length - existingItems.length,
        );
        commitRoleItems(role, committedItems, accountId);
        commitMailboxPageState(
          role,
          {
            ...committedPageState,
            loadMorePhase: append && !settleLoadMore ? "loading" : "idle",
            loadMoreError: null,
          },
          accountId,
        );
        if (activeAccountIdRef.current === accountId) {
          if (!preserveSyncing) {
            updateMailboxLoadState(role, {
              phase: "ready",
              completed: committedItems.length,
              total: null,
            });
          }
          const currentSelection = selectedMessageIdRef.current;
          if (currentSelection !== null) {
            const current = committedItems.find(
              (message) => localMessageId(message) === currentSelection,
            );
            if (current) {
              setSelectedMessage((previous) => {
                if (
                  !previous ||
                  localMessageId(previous) !== currentSelection ||
                  messageRole(previous) !== role
                ) {
                  return previous;
                }
                const preservedBody = bodySnapshot(previous);
                messageBodyCacheRef.current.set(
                  messageCacheKey(current, accountId),
                  preservedBody,
                );
                return { ...previous, ...current, ...preservedBody };
              });
            }
          } else if (
            selectFirst &&
            role === "inbox" &&
            committedItems.length &&
            window.innerWidth >= 720
          ) {
            void handleSelect(committedItems[0]);
          }
        }
        return {
          ...normalized,
          items: committedItems,
          state: committedPageState,
          appendedItemCount,
        };
      } catch (error) {
        if (
          append &&
          !recoveringCursor &&
          isStaleMailboxCursorError(error)
        ) {
          try {
            const recovered = await loadMailboxRolePage({
              accountId,
              role,
              query: normalizedQuery,
              mergeExisting: !normalizedQuery,
              preserveSyncing: true,
              recoveringCursor: true,
              settleLoadMore,
            });
            return recovered
              ? { ...recovered, cursorRecovered: true }
              : recovered;
          } catch (recoveryError) {
            if (!normalizedQuery && activeAccountIdRef.current === accountId) {
              commitMailboxPageState(
                role,
                (current) => ({
                  ...current,
                  loadMorePhase: "retry",
                  loadMoreError: describeError(
                    recoveryError,
                    "更早邮件暂时无法加载",
                  ),
                }),
                accountId,
              );
            }
            throw recoveryError;
          }
        }
        if (mailboxPageRequestsRef.current.get(requestKey) === requestToken) {
          if (!normalizedQuery && activeAccountIdRef.current === accountId) {
            updateMailboxLoadState(role, (current) => ({
              ...current,
              phase: "error",
            }));
            commitMailboxPageState(
              role,
              (current) => ({
                ...current,
                loadMorePhase: append ? "retry" : current.loadMorePhase,
                loadMoreError: append
                  ? describeError(error, "更早邮件暂时无法加载")
                  : current.loadMoreError,
              }),
              accountId,
            );
          }
        }
        throw error;
      } finally {
        if (mailboxPageRequestsRef.current.get(requestKey) === requestToken) {
          mailboxPageRequestsRef.current.delete(requestKey);
        }
      }
    },
    [
      commitMailboxPageState,
      commitRoleItems,
      handleSelect,
      updateMailboxLoadState,
    ],
  );

  const loadStarredMailboxRolePage = useCallback(
    async ({
      accountId = activeAccountIdRef.current,
      role,
      cursor = null,
      query: pageQuery = null,
      append = false,
      mergeExisting = false,
      recoveringCursor = false,
      settleLoadMore = true,
    }) => {
      if (!accountId || !starredMailboxRoles.includes(role)) return null;
      const normalizedQuery = normalizedMailboxQuery(pageQuery);
      const requestKey = `starred:${accountId}:${role}:${normalizedQuery || ""}`;
      const requestToken = Symbol(requestKey);
      mailboxPageRequestsRef.current.set(requestKey, requestToken);
      if (!normalizedQuery) {
        commitStarredMailboxPageState(
          role,
          (current) => ({
            ...current,
            ...(append ? {} : { nextCursor: null }),
            loadMorePhase: "loading",
            loadMoreError: null,
          }),
          accountId,
        );
      }

      try {
        const response = cursor
          ? await mailApi.loadOlderStarredMailboxPage(
              accountId,
              role,
              cursor,
              mailboxPageSize,
              normalizedQuery,
            )
          : await mailApi.listStarredMailboxPage(
              accountId,
              role,
              null,
              mailboxPageSize,
              normalizedQuery,
            );
        if (mailboxPageRequestsRef.current.get(requestKey) !== requestToken) {
          return null;
        }
        const normalized = normalizeMailboxPage(
          response,
          role,
          normalizedQuery,
        );
        normalized.items = normalized.items.map((message) => {
          const cachedBody = messageBodyCacheRef.current.get(
            messageCacheKey(message, accountId),
          );
          let resolved = cachedBody ? { ...message, ...cachedBody } : message;
          const key = scopedRemoteFlagKey(resolved, accountId);
          const pending = key ? starRequestRef.current.get(key) : null;
          const starred = pending?.starred ?? hasFlag(resolved, "\\Flagged");
          if (key) starStateRef.current.set(key, starred);
          if (pending) {
            resolved = withSystemFlag(resolved, "\\Flagged", starred);
          }
          return resolved;
        });

        if (normalizedQuery) return normalized;

        const existingView = accountViewsRef.current.get(accountId) || {};
        const existingPages =
          existingView.starredMailboxPageStates ||
          createStarredMailboxPageStates();
        const existingItems = existingPages[role]?.items || [];
        const committedItems =
          mergeExisting && !append && existingItems.length > 0
            ? mergeRefreshedMailboxItems(existingItems, normalized.items)
            : append
              ? appendMailboxItems(existingItems, normalized.items)
              : normalized.items;
        const appendedItemCount = Math.max(
          0,
          committedItems.length - existingItems.length,
        );
        commitStarredMailboxPageState(
          role,
          {
            ...normalized.state,
            items: committedItems,
            loadMorePhase: append && !settleLoadMore ? "loading" : "idle",
            loadMoreError: null,
          },
          accountId,
        );
        return { ...normalized, items: committedItems, appendedItemCount };
      } catch (error) {
        if (
          append &&
          !recoveringCursor &&
          isStaleMailboxCursorError(error)
        ) {
          try {
            const recovered = await loadStarredMailboxRolePage({
              accountId,
              role,
              query: normalizedQuery,
              mergeExisting: !normalizedQuery,
              recoveringCursor: true,
              settleLoadMore,
            });
            return recovered
              ? { ...recovered, cursorRecovered: true }
              : recovered;
          } catch (recoveryError) {
            if (!normalizedQuery) {
              commitStarredMailboxPageState(
                role,
                (current) => ({
                  ...current,
                  loadMorePhase: "retry",
                  loadMoreError: describeError(
                    recoveryError,
                    "收藏邮件暂时无法加载",
                  ),
                }),
                accountId,
              );
            }
            throw recoveryError;
          }
        }
        if (
          mailboxPageRequestsRef.current.get(requestKey) === requestToken &&
          !normalizedQuery
        ) {
          commitStarredMailboxPageState(
            role,
            (current) => ({
              ...current,
              loadMorePhase: "retry",
              loadMoreError: describeError(error, "收藏邮件暂时无法加载"),
            }),
            accountId,
          );
        }
        throw error;
      } finally {
        if (mailboxPageRequestsRef.current.get(requestKey) === requestToken) {
          mailboxPageRequestsRef.current.delete(requestKey);
        }
      }
    },
    [commitStarredMailboxPageState],
  );

  const mailboxProjectionKey = useCallback(
    (accountId, role, projection = "main") =>
      `${accountId}:${role}:${projection}`,
    [],
  );

  const mailboxProjectionIsVisible = useCallback(
    (accountId, role, projection = "main") => {
      if (activeAccountIdRef.current !== accountId) return false;
      if (projection === "starred") {
        return (
          activeFolderRef.current === "starred" &&
          starredMailboxRoles.includes(role)
        );
      }
      return activeFolderRef.current === role;
    },
    [],
  );

  const runMailboxProjectionRefresh = useCallback(
    ({
      accountId,
      role,
      projection = "main",
      selectFirst = false,
      preserveSyncing = false,
      mergeExisting = false,
      rerunIfActive = false,
    }) => {
      if (!accountId || !paginatedMailboxRoles.includes(role)) {
        return Promise.resolve(null);
      }
      const key = mailboxProjectionKey(accountId, role, projection);
      const activeFlight = mailboxRefreshFlightsRef.current.get(key);
      const requestedOptions = {
        selectFirst,
        preserveSyncing,
        mergeExisting,
      };
      if (activeFlight) {
        if (rerunIfActive) {
          activeFlight.dirty = true;
          activeFlight.nextOptions = activeFlight.nextOptions
            ? {
                selectFirst:
                  activeFlight.nextOptions.selectFirst || selectFirst,
                preserveSyncing:
                  activeFlight.nextOptions.preserveSyncing && preserveSyncing,
                mergeExisting:
                  activeFlight.nextOptions.mergeExisting && mergeExisting,
              }
            : requestedOptions;
        }
        return activeFlight.promise;
      }

      const flight = {
        dirty: false,
        nextOptions: null,
        promise: null,
      };
      const operation = (async () => {
        let options = requestedOptions;
        let result = null;
        do {
          flight.dirty = false;
          flight.nextOptions = null;
          mailboxProjectionInvalidationsRef.current.delete(key);
          try {
            result =
              projection === "starred"
                ? await loadStarredMailboxRolePage({ accountId, role })
                : await loadMailboxRolePage({
                    accountId,
                    role,
                    selectFirst: options.selectFirst,
                    preserveSyncing: options.preserveSyncing,
                    mergeExisting: options.mergeExisting,
                  });
          } catch (error) {
            mailboxProjectionInvalidationsRef.current.add(key);
            throw error;
          }
          options = flight.nextOptions || requestedOptions;
        } while (flight.dirty);
        return result;
      })().finally(() => {
        if (mailboxRefreshFlightsRef.current.get(key) === flight) {
          mailboxRefreshFlightsRef.current.delete(key);
        }
      });
      flight.promise = operation;
      mailboxRefreshFlightsRef.current.set(key, flight);
      return operation;
    },
    [
      loadMailboxRolePage,
      loadStarredMailboxRolePage,
      mailboxProjectionKey,
    ],
  );

  const invalidateMailboxRole = useCallback(
    ({
      accountId,
      role,
      preserveSyncing = false,
      mergeExisting = false,
    }) => {
      if (!accountId || !paginatedMailboxRoles.includes(role)) return [];
      const projections = ["main"];
      if (starredMailboxRoles.includes(role)) projections.push("starred");
      const visibleRefreshes = [];
      for (const projection of projections) {
        const key = mailboxProjectionKey(accountId, role, projection);
        mailboxProjectionInvalidationsRef.current.add(key);
        if (!mailboxProjectionIsVisible(accountId, role, projection)) {
          continue;
        }
        visibleRefreshes.push(
          runMailboxProjectionRefresh({
            accountId,
            role,
            projection,
            preserveSyncing,
            mergeExisting,
            rerunIfActive: true,
          }),
        );
      }
      return visibleRefreshes;
    },
    [
      mailboxProjectionIsVisible,
      mailboxProjectionKey,
      runMailboxProjectionRefresh,
    ],
  );

  useEffect(() => {
    if (!activeAccountId) return;
    if (paginatedMailboxRoles.includes(activeFolder)) {
      const key = mailboxProjectionKey(
        activeAccountId,
        activeFolder,
        "main",
      );
      if (mailboxProjectionInvalidationsRef.current.has(key)) {
        void runMailboxProjectionRefresh({
          accountId: activeAccountId,
          role: activeFolder,
        }).catch(() => {});
      }
      return;
    }
    if (activeFolder !== "starred") return;
    for (const role of starredMailboxRoles) {
      const key = mailboxProjectionKey(activeAccountId, role, "starred");
      if (!mailboxProjectionInvalidationsRef.current.has(key)) continue;
      void runMailboxProjectionRefresh({
        accountId: activeAccountId,
        role,
        projection: "starred",
      }).catch(() => {});
    }
  }, [
    activeAccountId,
    activeFolder,
    mailboxProjectionKey,
    runMailboxProjectionRefresh,
  ]);

  const refreshInbox = useCallback(
    ({
      selectFirst = false,
      accountId = activeAccountIdRef.current,
      preserveSyncing = false,
      mergeExisting = false,
    } = {}) =>
      runMailboxProjectionRefresh({
        accountId,
        role: "inbox",
        selectFirst,
        preserveSyncing,
        mergeExisting,
      }).then(
        (page) => page?.items || [],
      ),
    [runMailboxProjectionRefresh],
  );

  const refreshSent = useCallback(
    ({
      accountId = activeAccountIdRef.current,
      preserveSyncing = false,
      mergeExisting = false,
    } = {}) =>
      runMailboxProjectionRefresh({
        accountId,
        role: "sent",
        preserveSyncing,
        mergeExisting,
      }).then(
        (page) => page?.items || [],
      ),
    [runMailboxProjectionRefresh],
  );

  const refreshDrafts = useCallback(async ({ preserveSyncing = false } = {}) => {
    const requestId = draftsRequestRef.current + 1;
    draftsRequestRef.current = requestId;
    updateMailboxLoadState("drafts", (current) => ({
      ...current,
      phase:
        preserveSyncing && current?.phase === "syncing"
          ? "syncing"
          : "loading",
    }));
    const accountId = activeAccountIdRef.current || "unscoped";
    let localDrafts;
    try {
      localDrafts = await mailApi.listDrafts();
    } catch (error) {
      if (draftsRequestRef.current === requestId) {
        updateMailboxLoadState("drafts", (current) => ({
          ...current,
          phase: "error",
        }));
      }
      throw error;
    }
    if (draftsRequestRef.current !== requestId) return localDrafts;
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
        showToast("草稿已更新为其他客户端的最新版本", "info");
      }
    }
    updateMailboxLoadState("drafts", (current) =>
      preserveSyncing && current?.phase === "syncing"
        ? current
        : {
            phase: "ready",
            completed: localDrafts.length,
            total: null,
          },
    );
    return localDrafts;
  }, [commitComposer, showToast, updateMailboxLoadState]);

  const refreshOutbox = useCallback(async () => {
    const requestId = outboxRequestRef.current + 1;
    outboxRequestRef.current = requestId;
    updateMailboxLoadState("outbox", (current) => ({
      ...current,
      phase: current?.phase === "syncing" ? "syncing" : "loading",
    }));
    const accountId = activeAccountIdRef.current || "unscoped";
    let items;
    let sentFallbacks;
    try {
      [items, sentFallbacks] = await Promise.all([
        mailApi.listOutbox(),
        mailApi.listSentOutboxFallbacks(),
      ]);
    } catch (error) {
      if (outboxRequestRef.current === requestId) {
        updateMailboxLoadState("outbox", (current) => ({
          ...current,
          phase: "error",
        }));
      }
      throw error;
    }
    if (outboxRequestRef.current !== requestId) return items;
    const existingView = accountViewsRef.current.get(accountId) || {};
    accountViewsRef.current.set(accountId, {
      ...existingView,
      outbox: items,
      sentOutboxFallbacks: sentFallbacks,
    });
    if (activeAccountIdRef.current !== accountId) return items;
    setOutbox(items);
    setSentOutboxFallbacks(sentFallbacks);
    if (
      activeFolderRef.current === "outbox" &&
      selectedMessageIdRef.current &&
      !items.some((item) => item.id === selectedMessageIdRef.current)
    ) {
      clearSelection();
    }
    setSelectedMessage((current) => {
      if (current?.kind !== "outbox") return current;
      const displayedRole =
        current.displayed_role === "sent" ? "sent" : "outbox";
      const freshItem = (
        displayedRole === "sent" ? sentFallbacks : items
      ).find((item) => item.id === current.outbox?.id);
      if (!freshItem) return current;
      const summary = toOutboxMessage(
        freshItem,
        draftsRef.current,
        accountSenderIdentity(accountStatusRef.current),
        displayedRole,
      );
      return current.body_fetched
        ? { ...summary, ...bodySnapshot(current) }
        : summary;
    });
    updateMailboxLoadState("outbox", {
      phase: "ready",
      completed: items.length,
      total: null,
    });
    return items;
  }, [clearSelection, updateMailboxLoadState]);

  const commitAuthoritativeOutboxItem = useCallback((item, accountId) => {
    if (!item?.id || !accountId) return;
    const upsertItem = (items = []) => {
      const index = items.findIndex((candidate) => candidate.id === item.id);
      if (index < 0) return [item, ...items];
      const next = [...items];
      next[index] = item;
      return next;
    };
    const removeItem = (items = []) =>
      items.filter((candidate) => candidate.id !== item.id);
    const itemWasSent = item.status === "sent";
    if (activeAccountIdRef.current !== accountId) {
      const view = accountViewsRef.current.get(accountId) || {};
      accountViewsRef.current.set(accountId, {
        ...view,
        outbox: itemWasSent
          ? removeItem(view.outbox)
          : upsertItem(view.outbox),
        sentOutboxFallbacks: itemWasSent
          ? upsertItem(view.sentOutboxFallbacks)
          : removeItem(view.sentOutboxFallbacks),
      });
      return;
    }
    setOutbox((current) => {
      const next = itemWasSent ? removeItem(current) : upsertItem(current);
      const view = accountViewsRef.current.get(accountId) || {};
      accountViewsRef.current.set(accountId, { ...view, outbox: next });
      return next;
    });
    setSentOutboxFallbacks((current) => {
      const next = itemWasSent ? upsertItem(current) : removeItem(current);
      const view = accountViewsRef.current.get(accountId) || {};
      accountViewsRef.current.set(accountId, {
        ...view,
        sentOutboxFallbacks: next,
      });
      return next;
    });
    if (
      itemWasSent &&
      activeFolderRef.current === "outbox" &&
      selectedMessageIdRef.current === item.id
    ) {
      clearSelection();
      return;
    }
    setSelectedMessage((current) => {
      if (current?.kind !== "outbox" || current.outbox?.id !== item.id) {
        return current;
      }
      const summary = toOutboxMessage(
        item,
        draftsRef.current,
        accountSenderIdentity(accountStatusRef.current),
        itemWasSent ? "sent" : "outbox",
      );
      return current.body_fetched
        ? { ...summary, ...bodySnapshot(current) }
        : summary;
    });
  }, [clearSelection]);

  const loadAccountView = useCallback(
    (accountId, { force = false } = {}) => {
      if (!force && accountViewsRef.current.has(accountId)) {
        return Promise.resolve(accountViewsRef.current.get(accountId));
      }
      if (accountViewLoadsRef.current.has(accountId)) {
        return accountViewLoadsRef.current.get(accountId);
      }
      const operation = (async () => {
        const capabilities = await refreshMailboxCapabilities(accountId);
        const roles = paginatedMailboxRoles.filter((role) =>
          capabilityAvailable(capabilities, role),
        );
        await Promise.allSettled(
          roles.map((role) =>
            runMailboxProjectionRefresh({ accountId, role }),
          ),
        );
        const previous = accountViewsRef.current.get(accountId) || {};
        const view = {
          messages: previous.messages || [],
          sentMessages: previous.sentMessages || [],
          archiveMessages: previous.archiveMessages || [],
          trashMessages: previous.trashMessages || [],
          drafts: previous.drafts || [],
          outbox: previous.outbox || [],
          sentOutboxFallbacks: previous.sentOutboxFallbacks || [],
          selectedMessageId: previous.selectedMessageId ?? null,
          selectedMessage: previous.selectedMessage ?? null,
          navigationState: previous.navigationState || null,
          mailboxPageStates:
            previous.mailboxPageStates || createMailboxPageStates(),
          starredMailboxPageStates:
            previous.starredMailboxPageStates ||
            createStarredMailboxPageStates(),
          mailboxCapabilities: capabilities,
        };
        accountViewsRef.current.set(accountId, view);
        return view;
      })()
        .finally(() => {
          if (accountViewLoadsRef.current.get(accountId) === operation) {
            accountViewLoadsRef.current.delete(accountId);
          }
        });
      accountViewLoadsRef.current.set(accountId, operation);
      return operation;
    },
    [refreshMailboxCapabilities, runMailboxProjectionRefresh],
  );

  const prefetchAccountViews = useCallback(
    async (status) => {
      const accounts = status?.accounts || [];
      const activeAccountId =
        status?.activeAccountId || status?.accountId || null;
      return Promise.allSettled(
        accounts
          .filter((account) => account.accountId !== activeAccountId)
          .map((account) => loadAccountView(account.accountId)),
      );
    },
    [loadAccountView],
  );

  const loadMailboxData = useCallback(
    async ({
      selectFirst = false,
      accountId = activeAccountIdRef.current,
      preserveSyncing = false,
    } = {}) => {
      if (!accountId) return null;
      let capabilities = {};
      try {
        capabilities = await refreshMailboxCapabilities(accountId);
      } catch (error) {
        if (activeAccountIdRef.current === accountId) {
          setMailboxCapabilities({});
        }
      }
      const pageRoles = paginatedMailboxRoles.filter((role) =>
        capabilityAvailable(capabilities, role),
      );
      for (const role of ["archive", "trash"]) {
        if (!capabilityAvailable(capabilities, role)) {
          commitRoleItems(role, [], accountId);
          commitMailboxPageState(
            role,
            emptyMailboxPageState(),
            accountId,
          );
          if (starredMailboxRoles.includes(role)) {
            commitStarredMailboxPageState(
              role,
              emptyMailboxPageState({ items: [] }),
              accountId,
            );
          }
          if (activeAccountIdRef.current === accountId) {
            updateMailboxLoadState(role, {
              phase: "ready",
              completed: 0,
              total: null,
            });
          }
        }
      }
      const pageResults = await Promise.allSettled(
        pageRoles.map((role) =>
          runMailboxProjectionRefresh({
            accountId,
            role,
            selectFirst: role === "inbox" && selectFirst,
            preserveSyncing,
          }),
        ),
      );
      const starredPageResults =
        activeAccountIdRef.current === accountId &&
        activeFolderRef.current === "starred"
          ? await Promise.allSettled(
              starredMailboxRoles
                .filter((role) => capabilityAvailable(capabilities, role))
                .map((role) =>
                  runMailboxProjectionRefresh({
                    accountId,
                    role,
                    projection: "starred",
                  }),
                ),
            )
          : [];
      const activeLocalResults =
        activeAccountIdRef.current === accountId
          ? await Promise.allSettled([
              refreshDrafts({ preserveSyncing: true }),
              refreshOutbox(),
            ])
          : [];
      const previous = accountViewsRef.current.get(accountId) || {};
      const view = {
        messages: previous.messages || [],
        sentMessages: previous.sentMessages || [],
        archiveMessages: previous.archiveMessages || [],
        trashMessages: previous.trashMessages || [],
        drafts: previous.drafts || [],
        outbox: previous.outbox || [],
        sentOutboxFallbacks: previous.sentOutboxFallbacks || [],
        selectedMessageId: previous.selectedMessageId ?? null,
        selectedMessage: previous.selectedMessage ?? null,
        navigationState: previous.navigationState || null,
        mailboxPageStates:
          previous.mailboxPageStates || createMailboxPageStates(),
        starredMailboxPageStates:
          previous.starredMailboxPageStates ||
          createStarredMailboxPageStates(),
        mailboxCapabilities: capabilities,
      };
      accountViewsRef.current.set(accountId, view);
      if (
        [...pageResults, ...starredPageResults, ...activeLocalResults].some(
          (result) => result.status === "rejected",
        ) &&
        activeAccountIdRef.current === accountId
      ) {
        showToast("部分本地邮箱数据没有加载完成", "error");
      }
      return view;
    },
    [
      refreshDrafts,
      refreshOutbox,
      refreshMailboxCapabilities,
      runMailboxProjectionRefresh,
      commitMailboxPageState,
      commitStarredMailboxPageState,
      commitRoleItems,
      showToast,
      updateMailboxLoadState,
    ],
  );

  const loadRemoteSearch = useCallback(
    async ({
      accountId,
      folder,
      searchQuery,
      append = false,
      currentSearch = null,
    }) => {
      const normalizedQuery = normalizedMailboxQuery(searchQuery);
      if (!accountId || !normalizedQuery) return null;
      const requestId = remoteSearchRequestRef.current + 1;
      remoteSearchRequestRef.current = requestId;
      const current =
        currentSearch?.accountId === accountId &&
        currentSearch?.folder === folder &&
        currentSearch?.query === normalizedQuery
          ? currentSearch
          : null;
      const accountView = accountViewsRef.current.get(accountId) || {};
      const fallbackItems =
        folder === "starred"
          ? appendMailboxItems(
              [],
              starredMailboxRoles.flatMap((role) => [
                ...(accountView.starredMailboxPageStates?.[role]?.items || []),
                ...(accountView[mailboxViewField(role)] || []),
              ]),
            ).filter((message) => hasFlag(message, "\\Flagged"))
          : accountView[mailboxViewField(folder)] || [];
      setRemoteSearch((previous) => {
        if (
          previous?.accountId === accountId &&
          previous?.folder === folder &&
          previous?.query === normalizedQuery
        ) {
          return {
            ...previous,
            loadMorePhase: append ? "loading" : previous.loadMorePhase,
            phase: append ? previous.phase : "loading",
            error: null,
          };
        }
        return {
          accountId,
          folder,
          query: normalizedQuery,
          items: fallbackItems,
          sources: {},
          phase: "loading",
          loadMorePhase: "idle",
          loadMoreError: null,
          endReached: false,
          hasMoreLocal: false,
          remoteHistoryState: "not_checked",
        };
      });

      try {
        const roles =
          folder === "starred"
            ? starredMailboxRoles.filter((role) =>
                capabilityAvailable(mailboxCapabilities, role),
              )
            : [folder];
        const pages = await Promise.all(
          roles.map(async (role) => {
            const previousSource = current?.sources?.[role] || null;
            if (append && previousSource?.endReached) {
              return [role, previousSource];
            }
            const cursor = append ? previousSource?.nextCursor : null;
            if (append && !cursor) return [role, previousSource];
            const loadPage =
              folder === "starred"
                ? loadStarredMailboxRolePage
                : loadMailboxRolePage;
            const page = await loadPage({
              accountId,
              role,
              cursor,
              query: normalizedQuery,
              append,
            });
            if (!page) return [role, previousSource];
            return [
              role,
              {
                ...page.state,
                items:
                  append && page.cursorRecovered
                    ? mergeRefreshedMailboxItems(
                        previousSource?.items,
                        page.items,
                      )
                    : append
                      ? appendMailboxItems(previousSource?.items, page.items)
                      : page.items,
              },
            ];
          }),
        );
        if (
          remoteSearchRequestRef.current !== requestId ||
          activeAccountIdRef.current !== accountId ||
          activeFolderRef.current !== folder
        ) {
          return null;
        }
        const sources = Object.fromEntries(
          pages.filter(([, page]) => page).map(([role, page]) => [role, page]),
        );
        const sourceItems = Object.values(sources).flatMap(
          (source) => source.items || [],
        );
        const items = appendMailboxItems(
          [],
          folder === "starred"
            ? sourceItems.filter((message) =>
                hasFlag(message, "\\Flagged"),
              )
            : sourceItems,
        ).sort((left, right) => {
          const leftTime = Date.parse(left.sent_at || left.internal_date || "");
          const rightTime = Date.parse(
            right.sent_at || right.internal_date || "",
          );
          return (Number.isFinite(rightTime) ? rightTime : 0) -
            (Number.isFinite(leftTime) ? leftTime : 0);
        });
        const sourceStates = Object.values(sources);
        const endReached =
          sourceStates.length > 0 &&
          sourceStates.every((source) => source.endReached);
        const hasMoreLocal = sourceStates.some(
          (source) => source.hasMoreLocal,
        );
        const remoteHistoryState = sourceStates.some(
          (source) => source.remoteHistoryState === "may_have_more",
        )
          ? "may_have_more"
          : sourceStates.some(
                (source) => source.remoteHistoryState === "offline",
              )
            ? "offline"
            : endReached
              ? "complete"
              : sourceStates[0]?.remoteHistoryState || "not_checked";
        const next = {
          accountId,
          folder,
          query: normalizedQuery,
          items,
          sources,
          phase: "ready",
          loadMorePhase: "idle",
          loadMoreError: null,
          endReached,
          hasMoreLocal,
          remoteHistoryState,
        };
        setRemoteSearch(next);
        return next;
      } catch (error) {
        if (
          remoteSearchRequestRef.current === requestId &&
          activeAccountIdRef.current === accountId &&
          activeFolderRef.current === folder
        ) {
          setRemoteSearch((previous) => ({
            ...(previous || {
              accountId,
              folder,
              query: normalizedQuery,
              items: [],
              sources: {},
            }),
            phase: append ? previous?.phase || "ready" : "error",
            loadMorePhase: append ? "retry" : "idle",
            loadMoreError: describeError(
              error,
              "已同步邮件搜索暂时不可用",
            ),
            error: describeError(error, "已同步邮件搜索暂时不可用"),
          }));
        }
        throw error;
      }
    },
    [
      loadMailboxRolePage,
      loadStarredMailboxRolePage,
      mailboxCapabilities,
    ],
  );

  const restoreAccountView = useCallback(
    (
      accountId,
      view,
      {
        preserveWorkspaceMotion = false,
        deferReader = false,
        contactContext = null,
      } = {},
    ) => {
      const restored = {
        messages: [],
        sentMessages: [],
        archiveMessages: [],
        trashMessages: [],
        drafts: [],
        outbox: [],
        sentOutboxFallbacks: [],
        selectedMessageId: null,
        selectedMessage: null,
        navigationState: null,
        mailboxPageStates: createMailboxPageStates(),
        starredMailboxPageStates: createStarredMailboxPageStates(),
        mailboxCapabilities: null,
        ...(view || {}),
      };
      const navigationState = restored.navigationState || null;
      const restoredFolder = navigationState?.folder || "inbox";
      const restoredListExpanded =
        !isWideMailWorkspaceRef.current ||
        navigationState?.listExpanded === true;
      const restoredMessageId = navigationState
        ? restored.selectedMessageId ?? null
        : null;
      const restoredMessage =
        restoredMessageId !== null
          ? restored.selectedMessage ?? null
          : null;
      const restoredReaderOpen = Boolean(
        restoredListExpanded &&
        navigationState?.readerOpen &&
        restoredMessageId !== null &&
        restoredMessage,
      );
      activeAccountIdRef.current = accountId;
      activeFolderRef.current = restoredFolder;
      activateComposerForAccount(accountId);
      selectionRequestRef.current += 1;
      setActiveFolder(restoredFolder);
      setMessages(restored.messages);
      setSentMessages(restored.sentMessages || []);
      setArchiveMessages(restored.archiveMessages || []);
      setTrashMessages(restored.trashMessages || []);
      draftsRef.current = restored.drafts;
      setDrafts(restored.drafts);
      setOutbox(restored.outbox);
      setSentOutboxFallbacks(restored.sentOutboxFallbacks || []);
      setMailboxPageStates(
        restored.mailboxPageStates || createMailboxPageStates(),
      );
      setStarredMailboxPageStates(
        restored.starredMailboxPageStates || createStarredMailboxPageStates(),
      );
      setMailboxCapabilities(restored.mailboxCapabilities || null);
      setRemoteSearch(null);
      setQuery("");
      setFilter("all");
      setContactQuery("");
      setContactFilter(contactContext?.filter || "all");
      selectedContactEmailRef.current = contactContext?.email || null;
      selectedContactAccountIdRef.current =
        contactContext?.accountId || null;
      setSelectedContactEmail(contactContext?.email || null);
      setSelectedContactAccountId(contactContext?.accountId || null);
      if (!preserveWorkspaceMotion) {
        activeMailListIntentRef.current = null;
        queuedMailListIntentRef.current = null;
        setMailListMotionPhase(
          restoredListExpanded ? "expanded" : "collapsed",
        );
      }
      readerAfterExitRef.current = null;
      setReaderOpenState(false);
      setReaderMotionPhase("idle");
      selectedMessageIdRef.current = restoredMessageId;
      setSelectedMessageId(restoredMessageId);
      setSelectedMessage(restoredMessage);
      if (restoredReaderOpen && !deferReader) {
        presentReader("entering");
      }
      setMessageError(null);
      setIsMessageLoading(false);
      settleMailboxSnapshot(restored, { preserveSyncing: false });
      return restored;
    },
    [
      activateComposerForAccount,
      presentReader,
      setMailListMotionPhase,
      setReaderOpenState,
      setReaderMotionPhase,
      settleMailboxSnapshot,
    ],
  );

  const loadContacts = useCallback(
    async ({
      accountId = activeAccountIdRef.current,
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
        const favoriteCandidates = (
          Array.isArray(directory) ? [] : directory.favorites || []
        );
        const appFavorites = favoriteCandidates
          .filter(
            (item) =>
              typeof item?.accountId === "string" &&
              Boolean(item.accountId.trim()),
          )
          .map((item) => ({
            ...item,
            accountId: item.accountId.trim(),
          }));
        if (
          appFavorites.length !== favoriteCandidates.length &&
          !favoriteOwnershipWarningRef.current
        ) {
          favoriteOwnershipWarningRef.current = true;
          showToast(
            "部分收藏联系人缺少邮箱账户归属，已暂时忽略；重新收藏后可恢复。",
            "error",
          );
        }
        setContacts(currentContacts);
        setFavoriteContacts(appFavorites);
        setContactsState("ready");
        setContactsError(null);
        return directory;
      } catch (error) {
        if (contactsRequestRef.current === requestId && !silent) {
          setContactsState("error");
          setContactsError(describeError(error, "联系人没有加载完成"));
        }
        throw error;
      }
    },
    [showToast],
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
          selectedContactAccountIdRef.current !== accountId ||
          normalizeAvatarEmail(selectedContactEmailRef.current) !==
            normalizedEmail
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
    const accountId = activeAccountIdRef.current;
    if (!accountId || activeFolderRef.current !== "contacts") return;
    contactsInvalidatedAccountsRef.current.delete(accountId);
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

  const scheduleContactsRefresh = useCallback(() => {
    if (contactsRefreshTimerRef.current !== null) return;
    contactsRefreshTimerRef.current = window.setTimeout(() => {
      contactsRefreshTimerRef.current = null;
      void refreshActiveContactWorkspace().catch(() => {});
    }, 80);
  }, [refreshActiveContactWorkspace]);

  useEffect(() => {
    if (
      activeFolder !== "contacts" ||
      !activeAccountId ||
      !contactsInvalidatedAccountsRef.current.has(activeAccountId)
    ) {
      return;
    }
    scheduleContactsRefresh();
  }, [activeAccountId, activeFolder, scheduleContactsRefresh]);

  useEffect(
    () => () => {
      if (contactsRefreshTimerRef.current !== null) {
        window.clearTimeout(contactsRefreshTimerRef.current);
      }
    },
    [],
  );

  useEffect(() => {
    if (!activeAccountId) return;
    const preserved = preservedContactContextRef.current;
    const shouldPreserve =
      activeFolderRef.current === "contacts" &&
      preserved?.accountId === activeAccountId &&
      preserved?.requestId === contactMessageOpenRequestRef.current;
    if (shouldPreserve) {
      selectedContactEmailRef.current = preserved.email;
      selectedContactAccountIdRef.current = preserved.accountId;
      setSelectedContactEmail(preserved.email);
      setSelectedContactAccountId(preserved.accountId);
    } else {
      clearContactSelection();
    }
    preservedContactContextRef.current = null;
    // Contact remarks are local metadata used by the mail list and reader too,
    // so hydrate them with the active account's cached header activity even
    // before the contacts workspace is opened.
    void loadContacts({ accountId: activeAccountId }).catch(() => {});
  }, [activeAccountId, clearContactSelection, loadContacts]);

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
    const searchQuery = normalizedMailboxQuery(query);
    const remoteFolder =
      paginatedMailboxRoles.includes(activeFolder) ||
      activeFolder === "starred";
    if (!activeAccountId || !remoteFolder || !searchQuery) {
      remoteSearchRequestRef.current += 1;
      setRemoteSearch(null);
      return undefined;
    }
    const accountId = activeAccountId;
    const folder = activeFolder;
    const timer = window.setTimeout(() => {
      void loadRemoteSearch({
        accountId,
        folder,
        searchQuery,
      }).catch(() => {});
    }, 120);
    return () => window.clearTimeout(timer);
  }, [activeAccountId, activeFolder, loadRemoteSearch, query]);

  useEffect(() => {
    if (isUnsupportedRuntime) return undefined;
    let cancelled = false;
    const load = async () => {
      const settingsTask = mailApi
        .getDesktopSettings()
        .then((value) => {
          if (cancelled) return;
          setSettings(value);
          if (value.startupError) {
            showToast(
              describeError(value.startupError, "桌面设置初始化没有完成"),
              "error",
              true,
            );
          }
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
      const appearanceTask = mailApi
        .getAppearanceSettings()
        .then(async (value) => {
          if (cancelled) return;
          const legacyTheme = window.localStorage.getItem("mine-mail-theme");
          const legacyIsBuiltin = builtinAppearanceThemes.some(
            (theme) => theme.id === legacyTheme,
          );
          let resolved = value;
          if (!value.selectionInitialized && legacyIsBuiltin) {
            if (!value.minimalModeEnabled) {
              const legacyPalette = builtinAppearanceThemes.find(
                (theme) => theme.id === legacyTheme,
              )?.paletteId;
              if (legacyPalette) {
                resolved = await mailApi.updateAppearancePreferences({
                  paletteId: legacyPalette,
                });
              }
            }
            resolved = await mailApi.selectAppearanceTheme({
              kind: "builtin",
              id: legacyTheme,
            });
          }
          if (!cancelled) setAppearance(resolved);
        })
        .catch((error) => {
          if (!cancelled)
            showToast(describeError(error, "外观设置读取失败"), "error");
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
          beginMailboxLoading(networkUsable && isTauri ? "syncing" : "loading");
          if (!networkUsable) {
            setAccountError(
              describeError(
                status.startupError,
                "本地邮件仍可阅读，但账户凭据或网络连接不可用。请重新连接账户后再同步或发送。",
              ),
            );
          }
          void loadMailboxData({
            accountId: activeAccountId,
            selectFirst: true,
            preserveSyncing: networkUsable && isTauri,
          });
        } else {
          setAccountError(
            status.startupError
              ? describeError(status.startupError, "邮箱账户初始化没有完成")
              : status.configured && !status.credentialAvailable
                ? "账户信息存在，但系统凭据不可用，请重新输入授权信息。"
                : null,
          );
        }
      } catch (error) {
        if (cancelled) return;
        setAccountStatus({ configured: false, provider: null, email: null });
        setAccountError(describeError(error, "无法读取账户配置"));
      }

      await Promise.allSettled([
        settingsTask,
        presetsTask,
        avatarsTask,
        appearanceTask,
      ]);
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [beginMailboxLoading, loadMailboxData, prefetchAccountViews]);

  useEffect(() => {
    applyAppearanceToDocument(appearance);
  }, [appearance]);

  useEffect(() => {
    if (!toast || toast.exiting) return undefined;
    const timer = window.setTimeout(
      dismissToast,
      toast.persistent ? importantToastVisibleMs : toastVisibleMs,
    );
    return () => window.clearTimeout(timer);
  }, [dismissToast, toast]);

  useEffect(
    () => () => {
      if (syncFeedbackTimerRef.current) {
        window.clearTimeout(syncFeedbackTimerRef.current);
      }
    },
    [],
  );

  useEffect(() => {
    if (!toast?.exiting) return undefined;
    const toastId = toast.id;
    const timer = window.setTimeout(() => {
      setToast((current) => (current?.id === toastId ? null : current));
    }, toastExitMs);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    if (!accountNeedsRepair) {
      setIsAccountRepairVisible(false);
      return undefined;
    }
    const timer = window.setTimeout(
      () => setIsAccountRepairVisible(true),
      accountRepairDelayMs,
    );
    return () => window.clearTimeout(timer);
  }, [accountNeedsRepair]);

  const cacheAuthoritativeDrafts = useCallback(
    (draft, canonical = null) => {
      if (!draft?.id) return;
      const currentDrafts = draftsRef.current;
      const withCanonical = canonical
        ? upsertDraft(currentDrafts, canonical)
        : currentDrafts;
      const nextDrafts = upsertDraft(withCanonical, draft);

      // A list request that began before this accepted write is now stale. Its
      // snapshot may legitimately omit the new draft, so prevent it from
      // replacing the authoritative local cache or closing the composer.
      draftsRequestRef.current += 1;
      draftsRef.current = nextDrafts;
      setDrafts(nextDrafts);

      const accountId = activeAccountIdRef.current;
      if (accountId) {
        const existingView = accountViewsRef.current.get(accountId) || {};
        accountViewsRef.current.set(accountId, {
          ...existingView,
          drafts: nextDrafts,
        });
      }
      updateMailboxLoadState("drafts", (current) => ({
        phase: current?.phase === "syncing" ? "syncing" : "ready",
        completed: nextDrafts.length,
        total: null,
      }));
    },
    [updateMailboxLoadState],
  );

  const saveDraftNow = useCallback(
    async ({ force = false } = {}) => {
      flushComposeBodySnapshot(composerRef.current?.sessionId);
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
            cacheAuthoritativeDrafts(draft, outcome.canonical || null);
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
    [cacheAuthoritativeDrafts, commitComposer, flushComposeBodySnapshot, showToast],
  );

  const switchMinimizedComposerDraft = useCallback(
    async (draft) => {
      if (!draft?.id) return false;
      const initial = composerRef.current;
      if (!initial) {
        openComposer(draftToRequest(draft), draft.id, draft);
        return true;
      }
      if (!initial.minimized) return false;
      if (initial.draftId === draft.id) {
        setComposeRestoreRequest((request) => request + 1);
        return true;
      }
      if (initial.locked) {
        showToast("请等待当前草稿操作完成后再切换草稿。", "error");
        return false;
      }

      const sessionId = initial.sessionId;
      commitComposer((current) =>
        current?.sessionId === sessionId
          ? { ...current, locked: true }
          : current,
      );
      try {
        await saveDraftNow();
      } catch (error) {
        commitComposer((current) =>
          current?.sessionId === sessionId
            ? { ...current, locked: false, saveStatus: "error" }
            : current,
        );
        showToast(
          describeError(error, "切换草稿前未能保存当前编辑，请重试"),
          "error",
        );
        return false;
      }

      if (composerRef.current?.sessionId !== sessionId) return false;
      commitComposer(createComposer(draftToRequest(draft), draft.id, draft));
      return true;
    },
    [commitComposer, openComposer, saveDraftNow, showToast],
  );
  switchMinimizedComposerDraftRef.current = switchMinimizedComposerDraft;

  const prepareComposerForAccountSwitch = useCallback(async () => {
    const initial = composerRef.current;
    if (!initial) return true;
    if (initial.locked) {
      showToast("请等待当前草稿操作完成后再切换邮箱账户。", "error");
      return false;
    }

    const sessionId = initial.sessionId;
    commitComposer((current) =>
      current?.sessionId === sessionId ? { ...current, locked: true } : current,
    );
    try {
      await saveDraftNow();
    } catch (error) {
      commitComposer((current) =>
        current?.sessionId === sessionId
          ? { ...current, locked: false, saveStatus: "error" }
          : current,
      );
      showToast(
        describeError(error, "切换账户前未能保存当前草稿，请重试"),
        "error",
      );
      return false;
    }

    const current = composerRef.current;
    if (!current || current.sessionId !== sessionId) return false;
    commitComposer({
      ...current,
      locked: false,
      startMinimized: true,
      minimized: true,
      saveStatus: current.readOnlyUnsupported
        ? "readonly"
        : current.dirty
          ? "dirty"
          : current.draftId
            ? "saved"
            : "idle",
    });
    return true;
  }, [commitComposer, saveDraftNow, showToast]);

  const applyAttachmentMutationOutcome = useCallback(
    (sessionId, outcome, operation) => {
      const draft = outcome?.draft;
      if (
        !draft?.id ||
        !Number.isInteger(draft.local_version) ||
        draft.local_version < 1
      ) {
        throw new Error("附件操作没有返回可用的草稿版本");
      }
      cacheAuthoritativeDrafts(draft, outcome.canonical || null);
      commitComposer((current) => {
        if (!current || current.sessionId !== sessionId) return current;
        const nextOperations = {
          add: current.attachmentOperations?.add || null,
          remove: { ...(current.attachmentOperations?.remove || {}) },
        };
        const operationState = { status: outcome.kind || "saved" };
        if (operation.kind === "add") {
          nextOperations.add = operationState;
        } else {
          nextOperations.remove[operation.attachmentId] = operationState;
        }
        return {
          ...current,
          draftId: draft.id,
          baseLocalVersion: draft.local_version,
          persistedDraft: draft,
          readOnlyUnsupported: Boolean(draft.has_unsupported_content),
          value: draftToRequest(draft),
          dirty: false,
          revision: current.revision + 1,
          saveStatus: draft.has_unsupported_content ? "readonly" : "saved",
          locked: false,
          attachmentOperations: nextOperations,
        };
      });
      if (outcome.kind === "conflict_copy") {
        showToast(
          "附件已保存在新的冲突副本中，未覆盖其他客户端的最新草稿。",
          "error",
          true,
        );
      } else if (outcome.kind === "stale") {
        showToast(
          "草稿已在其他客户端更新，本次附件操作未生效；已切换到最新版本。",
          "error",
        );
      }
      return draft;
    },
    [cacheAuthoritativeDrafts, commitComposer, showToast],
  );

  useEffect(() => {
    if (!isTauri) return undefined;
    let cancelled = false;
    const disposers = [];
    const reportEventError = (error, fallback) => {
      if (!cancelled) showToast(describeError(error, fallback), "error");
    };
    const handleMailboxUpdate = (
      folder,
      event,
      fallback,
      { refreshContacts = false, refreshOutboxOnComplete = false } = {},
    ) => {
      const payload = event?.payload || {};
      const targetAccountId =
        eventAccountId(payload) || activeAccountIdRef.current;
      const activeAccountId = activeAccountIdRef.current;
      const progress = mailboxProgress(payload);
      const explicitProgress = mailboxEventHasExplicitProgress(payload);
      const progressKey = targetAccountId
        ? `${targetAccountId}:${folder}`
        : null;
      const terminalError =
        progress.complete && typeof payload.error === "string"
          ? payload.error.trim()
          : "";
      let hasNewPersistedBatch = false;

      if (progressKey) {
        if (progress.complete) {
          mailboxSyncProgressRef.current.delete(progressKey);
        } else if (progress.completed > 0) {
          const previous = mailboxSyncProgressRef.current.get(progressKey) || 0;
          hasNewPersistedBatch = progress.completed > previous;
          mailboxSyncProgressRef.current.set(
            progressKey,
            Math.max(previous, progress.completed),
          );
        }
      }

      if (targetAccountId === activeAccountId) {
        updateMailboxLoadState(folder, (current) => {
          if (terminalError) {
            return ["loading", "syncing"].includes(current?.phase)
              ? { ...current, phase: "error" }
              : current;
          }
          if (
            !progress.complete &&
            current?.phase !== "loading" &&
            current?.phase !== "syncing"
          ) {
            return current;
          }
          return {
            phase: progress.complete ? "ready" : "syncing",
            completed: progress.completed,
            total: progress.total,
          };
        });
      }

      const shouldRefresh =
        !terminalError &&
        (!explicitProgress || progress.complete || hasNewPersistedBatch);
      if (targetAccountId && shouldRefresh) {
        const refreshes = invalidateMailboxRole({
          accountId: targetAccountId,
          role: folder,
          preserveSyncing: !progress.complete,
          mergeExisting:
            !Boolean(
              payload.report?.uidValidityReset ??
                payload.report?.uid_validity_reset,
            ) && Number(payload.report?.removed || 0) === 0,
        });
        for (const refresh of refreshes) {
          void refresh.catch((error) => reportEventError(error, fallback));
        }
      }

      if (targetAccountId && progress.complete && !terminalError) {
        if (refreshContacts) {
          contactsInvalidatedAccountsRef.current.add(targetAccountId);
          if (
            targetAccountId === activeAccountId &&
            activeFolderRef.current === "contacts"
          ) {
            scheduleContactsRefresh();
          }
        }
        if (refreshOutboxOnComplete && targetAccountId === activeAccountId) {
          void refreshOutbox().catch(() => {});
        }
      }
    };
    const openNotificationMessage = (request) => {
      const {
        accountId: opaqueAccountId,
        messageId: opaqueMessageId,
        navigationIntentId,
      } = request;
      if (
        cancelled ||
        navigationIntentRef.current !== navigationIntentId
      ) {
        if (
          pendingNotificationOpenRef.current?.navigationIntentId ===
          navigationIntentId
        ) {
          pendingNotificationOpenRef.current = null;
        }
        return;
      }
      void (async () => {
        let status = await mailApi.getAccountStatus();
        if (
          cancelled ||
          navigationIntentRef.current !== navigationIntentId
        ) {
          return;
        }
        const currentAccountId =
          status.activeAccountId || status.accountId || null;
        if (currentAccountId !== opaqueAccountId) {
          const composerReady = await prepareComposerForAccountSwitch();
          if (!composerReady) {
            pendingNotificationOpenRef.current = request;
            return;
          }
          invalidateForwardPreparationsForAccount(
            activeAccountIdRef.current,
          );
          rememberActiveAccountViewRef.current(
            activeAccountIdRef.current,
          );
          forwardPreparationRequestRef.current += 1;
          const accountSwitchRequestId =
            accountSwitchRequestRef.current + 1;
          accountSwitchRequestRef.current = accountSwitchRequestId;
          status = await mailApi.switchAccount(opaqueAccountId);
          if (
            cancelled ||
            navigationIntentRef.current !== navigationIntentId ||
            accountSwitchRequestRef.current !== accountSwitchRequestId
          ) {
            return;
          }
        }

        if (
          pendingNotificationOpenRef.current?.navigationIntentId ===
          navigationIntentId
        ) {
          pendingNotificationOpenRef.current = null;
        }
        accountStatusRef.current = status;
        setAccountStatus(status);
        if (activeAccountIdRef.current !== opaqueAccountId) {
          restoreAccountView(
            opaqueAccountId,
            accountViewsRef.current.get(opaqueAccountId),
          );
        }
        if (
          cancelled ||
          navigationIntentRef.current !== navigationIntentId
        ) {
          return;
        }

        // A notification points at an account-scoped opaque public ID.
        // Hydrate that ID directly; its message may have moved folders or
        // be outside the first Inbox page.
        const fetched = await mailApi.fetchMailboxMessage(opaqueMessageId);
        if (
          cancelled ||
          navigationIntentRef.current !== navigationIntentId ||
          activeAccountIdRef.current !== opaqueAccountId
        ) {
          return;
        }
        const displayMessage = toMailboxDisplayMessage(
          fetched,
          messageRole(fetched),
        );
        if (localMessageId(displayMessage) !== opaqueMessageId) {
          throw new Error("新邮件标识与本地邮件不一致");
        }
        await handleSelect(displayMessage, false, {
          navigationIntentId,
        });
        if (
          navigationIntentRef.current === navigationIntentId &&
          activeAccountIdRef.current === opaqueAccountId
        ) {
          void loadAccountView(opaqueAccountId, { force: true }).catch(
            () => {},
          );
        }
      })().catch((error) => {
        if (navigationIntentRef.current === navigationIntentId) {
          reportEventError(error, "新邮件暂时无法打开");
        }
      });
    };
    const resumeNotificationOpen = () => {
      const pending = pendingNotificationOpenRef.current;
      if (!pending || composerRef.current) return;
      if (
        navigationIntentRef.current !== pending.navigationIntentId
      ) {
        pendingNotificationOpenRef.current = null;
        return;
      }
      openNotificationMessage(pending);
    };
    resumeNotificationOpenRef.current = resumeNotificationOpen;
    const subscribe = async () => {
      try {
        const accountUnlisten = await mailApi.onMailEvent(
          "mail:account-updated",
          (event) => {
            const status = event?.payload;
            if (!status || typeof status.configured !== "boolean") return;
            if (accountViewSnapshotLockRef.current) return;
            const previousStatus = accountStatusRef.current;
            accountStatusRef.current = status;
            activeAccountIdRef.current =
              status.activeAccountId || status.accountId || null;
            setAccountStatus(status);
            if (
              shouldRepairAccount(previousStatus) &&
              canUseAccountNetwork(status)
            ) {
              setAccountError(null);
            }
          },
        );
        if (cancelled) accountUnlisten();
        else disposers.push(accountUnlisten);

        const inboxUnlisten = await mailApi.onMailEvent(
          "mail:inbox-updated",
          (event) => {
            handleMailboxUpdate(
              "inbox",
              event,
              "收件箱刷新失败",
              { refreshContacts: true },
            );
          },
        );
        if (cancelled) inboxUnlisten();
        else disposers.push(inboxUnlisten);

        const sentUnlisten = await mailApi.onMailEvent(
          "mail:sent-updated",
          (event) => {
            const payload = event?.payload || {};
            const progress = mailboxProgress(payload);
            if (
              progress.complete &&
              synchronizedMessageCount(payload.report) > 0
            ) {
              markSentAttention(
                eventAccountId(payload) || activeAccountIdRef.current,
              );
            }
            handleMailboxUpdate(
              "sent",
              event,
              "已发送刷新失败",
              { refreshContacts: true, refreshOutboxOnComplete: true },
            );
          },
        );
        if (cancelled) sentUnlisten();
        else disposers.push(sentUnlisten);

        const mailboxUnlisten = await mailApi.onMailEvent(
          "mail:mailbox-updated",
          (event) => {
            const payload = event?.payload || {};
            const targetAccountId = eventAccountId(payload);
            const role = String(payload.role || "").toLowerCase();
            if (
              !targetAccountId ||
              !paginatedMailboxRoles.includes(role)
            ) {
              return;
            }
            const refreshes = invalidateMailboxRole({
              accountId: targetAccountId,
              role,
            });
            for (const refresh of refreshes) {
              void refresh.catch((error) =>
                reportEventError(
                  error,
                  activeFolderRef.current === "starred"
                    ? "收藏邮件刷新失败"
                    : `${folderLabels[role]}刷新失败`,
                ),
              );
            }
            contactsInvalidatedAccountsRef.current.add(targetAccountId);
            if (
              targetAccountId === activeAccountIdRef.current &&
              activeFolderRef.current === "contacts"
            ) {
              scheduleContactsRefresh();
            }
            if (
              role === "sent" &&
              targetAccountId === activeAccountIdRef.current
            ) {
              void refreshOutbox().catch(() => {});
            }
          },
        );
        if (cancelled) mailboxUnlisten();
        else disposers.push(mailboxUnlisten);

        const capabilitiesUnlisten = await mailApi.onMailEvent(
          "mail:mailbox-capabilities-updated",
          (event) => {
            const targetAccountId = eventAccountId(event?.payload || {});
            if (!targetAccountId) return;
            void refreshMailboxCapabilities(targetAccountId).catch((error) =>
              reportEventError(error, "邮箱能力刷新失败"),
            );
          },
        );
        if (cancelled) capabilitiesUnlisten();
        else disposers.push(capabilitiesUnlisten);

        const openMessageUnlisten = await mailApi.onMailEvent(
          "mail:open-message",
          (event) => {
            const messageId =
              event?.payload?.messageId ?? event?.payload?.message_id ?? null;
            const targetAccountId =
              event?.payload?.accountId ?? event?.payload?.account_id;
            if (
              typeof messageId !== "string" ||
              !messageId.trim() ||
              typeof targetAccountId !== "string" ||
              !targetAccountId.trim()
            ) {
              return;
            }
            const opaqueMessageId = messageId.trim();
            const opaqueAccountId = targetAccountId.trim();
            const navigationIntentId = navigationIntentRef.current + 1;
            navigationIntentRef.current = navigationIntentId;
            pendingNotificationOpenRef.current = null;
            contactMessageOpenRequestRef.current += 1;
            replyPreparationRequestRef.current += 1;
            preservedContactContextRef.current = null;
            openNotificationMessage({
              accountId: opaqueAccountId,
              messageId: opaqueMessageId,
              navigationIntentId,
            });
          },
        );
        if (cancelled) openMessageUnlisten();
        else disposers.push(openMessageUnlisten);

        const draftsUnlisten = await mailApi.onMailEvent(
          "mail:drafts-updated",
          (event) => {
            const progress = mailboxProgress(event?.payload || {});
            updateMailboxLoadState("drafts", (current) => {
              if (
                !progress.complete &&
                current?.phase !== "loading" &&
                current?.phase !== "syncing"
              ) {
                return current;
              }
              return {
                phase: progress.complete ? "ready" : "syncing",
                completed: progress.completed,
                total: progress.total,
              };
            });
            void Promise.all([
              refreshDrafts({ preserveSyncing: !progress.complete }),
              refreshOutbox(),
            ]).catch((error) =>
              reportEventError(error, "草稿或发件队列刷新失败"),
            );
          },
        );
        if (cancelled) draftsUnlisten();
        else disposers.push(draftsUnlisten);

        const syncErrorUnlisten = await mailApi.onMailEvent(
          "mail:sync-error",
          (event) => {
            const operation = event?.payload?.operation;
            setMailboxLoadStates((current) =>
              Object.fromEntries(
                mailboxFolders.map((folder) => [
                  folder,
                  (operation === "all" || operation === folder) &&
                  current[folder]?.phase === "syncing"
                    ? { ...current[folder], phase: "error" }
                    : current[folder],
                ]),
              ),
            );
            const trigger = event?.payload?.trigger;
            if (trigger !== "manual" && trigger !== "tray") {
              setSyncState("idle");
              return;
            }
            setSyncState("error");
            showToast(
              describeError(
                event?.payload?.message,
                "部分邮箱暂时未能同步，请稍后重试",
              ),
              "error",
            );
          },
        );
        if (cancelled) syncErrorUnlisten();
        else disposers.push(syncErrorUnlisten);

        const externalLinkOpenFailedUnlisten = await mailApi.onMailEvent(
          "mail:external-link-open-failed",
          () => {
            showToast(
              "无法打开邮件中的链接，请检查系统默认浏览器设置后重试",
              "error",
            );
          },
        );
        if (cancelled) externalLinkOpenFailedUnlisten();
        else disposers.push(externalLinkOpenFailedUnlisten);

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
      if (resumeNotificationOpenRef.current === resumeNotificationOpen) {
        resumeNotificationOpenRef.current = null;
      }
      disposers.forEach((dispose) => dispose());
    };
  }, [
    commitComposer,
    handleSelect,
    invalidateMailboxRole,
    invalidateForwardPreparationsForAccount,
    loadAccountView,
    markSentAttention,
    prepareComposerForAccountSwitch,
    refreshDrafts,
    refreshMailboxCapabilities,
    refreshOutbox,
    restoreAccountView,
    saveDraftNow,
    scheduleContactsRefresh,
    showToast,
    updateMailboxLoadState,
  ]);

  useEffect(() => {
    if (
      !composer?.dirty ||
      composer.locked ||
      composer.saveStatus === "saving" ||
      composeBodyAutosaveTimerRef.current !== null
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
    composer?.locked,
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
  }, [openComposer]);

  const outboxMessages = useMemo(
    () =>
      outbox.map((item) =>
        toOutboxMessage(
          item,
          drafts,
          accountSenderIdentity(accountStatus),
        ),
      ),
    [accountStatus, drafts, outbox],
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
    const localFallbacks = sentOutboxFallbacks
      .filter(
        (item) =>
          !remote.some((message) => sentMessageMatchesOutbox(message, item)),
      )
      .map((item) =>
        toOutboxMessage(
          item,
          drafts,
          accountSenderIdentity(accountStatus),
          "sent",
        ),
      );
    return [...remote, ...localFallbacks].sort((left, right) => {
      const leftTime = Date.parse(left.sent_at || "") || 0;
      const rightTime = Date.parse(right.sent_at || "") || 0;
      return rightTime - leftTime;
    });
  }, [accountStatus, drafts, sentMessages, sentOutboxFallbacks]);

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
          account.remark?.trim() || account.email || account.accountId,
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
    clearContactSelection();
  }, [
    activeFolder,
    clearContactSelection,
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

  const baseFolderMessages = useMemo(() => {
    if (activeFolder === "inbox") return messages;
    if (activeFolder === "starred") {
      const snapshotItems =
        starredViewRetention.accountId === activeAccountId
          ? starredViewRetention.items
          : [];
      const currentItems = appendMailboxItems([], [
        ...starredMailboxRoles.flatMap(
          (role) => starredMailboxPageStates[role]?.items || [],
        ),
        ...messages,
        ...sentMessages,
        ...archiveMessages,
      ]);
      return preserveStarredVisitMessages(snapshotItems, currentItems);
    }
    if (activeFolder === "drafts") {
      return drafts
        .filter((draft) => draft.status !== "sent")
        .map(toDraftMessage);
    }
    if (activeFolder === "outbox") return outboxMessages;
    if (activeFolder === "sent") return combinedSentMessages;
    if (activeFolder === "archive") return archiveMessages;
    if (activeFolder === "trash") return trashMessages;
    return [];
  }, [
    activeFolder,
    activeAccountId,
    archiveMessages,
    combinedSentMessages,
    drafts,
    messages,
    outboxMessages,
    sentMessages,
    starredMailboxPageStates,
    starredViewRetention,
    trashMessages,
  ]);

  const folderMessages = useMemo(() => {
    const normalizedQuery = normalizedMailboxQuery(query);
    const activeSearch =
      normalizedQuery &&
      remoteSearch?.accountId === activeAccountId &&
      remoteSearch?.folder === activeFolder &&
      remoteSearch?.query === normalizedQuery
        ? remoteSearch
        : null;
    if (!activeSearch) return baseFolderMessages;
    if (activeFolder !== "starred") return activeSearch.items;

    const snapshotItems =
      starredViewRetention.accountId === activeAccountId
        ? starredViewRetention.items
        : [];
    return preserveStarredVisitMessages(
      snapshotItems,
      activeSearch.items,
    );
  }, [
    activeFolder,
    activeAccountId,
    baseFolderMessages,
    query,
    remoteSearch,
    starredViewRetention,
  ]);

  const visibleMessages = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    const queryWasHandledByMailboxApi =
      Boolean(normalizedQuery) &&
      (paginatedMailboxRoles.includes(activeFolder) ||
        activeFolder === "starred");
    return folderMessages.filter((message) => {
      if (filter === "unread" && hasFlag(message, "\\Seen")) return false;
      if (filter === "starred" && !hasFlag(message, "\\Flagged")) return false;
      if (!normalizedQuery || queryWasHandledByMailboxApi) return true;
      return [
        message.subject,
        message.preview,
        contactRemarkForEmail(message.sender?.email),
        message.sender?.name,
        message.sender?.email,
      ].some((value) => value?.toLowerCase().includes(normalizedQuery));
    });
  }, [activeFolder, contactRemarkForEmail, filter, folderMessages, query]);

  const activeMailboxLoadState = useMemo(() => {
    if (mailboxLoadStates[activeFolder]) {
      return mailboxLoadStates[activeFolder];
    }
    if (activeFolder === "starred") {
      const sources = starredMailboxRoles
        .filter((role) => capabilityAvailable(mailboxCapabilities, role))
        .map((role) => mailboxLoadStates[role])
        .filter(Boolean);
      const phase = sources.some((state) => state.phase === "syncing")
        ? "syncing"
        : sources.some((state) => state.phase === "loading")
          ? "loading"
          : sources.some((state) => state.phase === "error")
            ? "error"
            : "ready";
      const totals = sources.map((state) => state.total);
      return {
        phase,
        completed: sources.reduce(
          (sum, state) => sum + (state.completed || 0),
          0,
        ),
        total:
          sources.length > 0 &&
          totals.every((total) => Number.isFinite(total))
            ? totals.reduce((sum, total) => sum + total, 0)
            : null,
      };
    }
    return { phase: "ready", completed: visibleMessages.length, total: null };
  }, [
    activeFolder,
    mailboxCapabilities,
    mailboxLoadStates,
    visibleMessages.length,
  ]);

  useEffect(() => {
    if (activeFolder !== "starred" || !activeAccountId) {
      starredViewMessagesRef.current = { accountId: null, items: [] };
      return;
    }
    starredViewMessagesRef.current = {
      accountId: activeAccountId,
      items: visibleMessages,
    };
    setStarredViewRetention((current) => {
      if (current.accountId !== activeAccountId || !current.items.length) {
        return current;
      }
      const snapshotIds = new Set(
        current.items.map(localMessageId).filter((id) => id !== null),
      );
      const appendedItems = visibleMessages.filter(
        (message) => !snapshotIds.has(localMessageId(message)),
      );
      return appendedItems.length
        ? { ...current, items: [...current.items, ...appendedItems] }
        : current;
    });
  }, [activeAccountId, activeFolder, visibleMessages]);

  const selectedMessageKey = remoteFlagKey(selectedMessage);
  const selectedIndex = visibleMessages.findIndex((message) => {
    const key = remoteFlagKey(message);
    return selectedMessageKey && key
      ? key === selectedMessageKey
      : localMessageId(message) === selectedMessageId;
  });

  const contactDisplayMessages = useMemo(
    () => contactMessages.map(toContactDisplayMessage),
    [contactMessages],
  );
  const contactSelectedIndex = contactDisplayMessages.findIndex((message) => {
    const key = remoteFlagKey(message);
    return selectedMessageKey && key
      ? key === selectedMessageKey
      : localMessageId(message) === selectedMessageId;
  });

  const activeMailboxCapability =
    activeFolder === "archive" || activeFolder === "trash"
      ? mailboxCapabilities?.[activeFolder] || {
          role: activeFolder,
          status: "discovery_pending",
          retryable: false,
        }
      : null;

  const activePagination = useMemo(() => {
    const normalizedQuery = normalizedMailboxQuery(query);
    if (
      normalizedQuery &&
      remoteSearch?.accountId === activeAccountId &&
      remoteSearch?.folder === activeFolder &&
      remoteSearch?.query === normalizedQuery
    ) {
      return remoteSearch;
    }
    if (paginatedMailboxRoles.includes(activeFolder)) {
      return mailboxPageStates[activeFolder] || emptyMailboxPageState();
    }
    if (activeFolder === "starred") {
      const sources = Object.fromEntries(
        starredMailboxRoles
          .filter((role) => capabilityAvailable(mailboxCapabilities, role))
          .map((role) => [
            role,
            starredMailboxPageStates[role] ||
              emptyMailboxPageState({ items: [] }),
          ]),
      );
      const states = Object.values(sources);
      return {
        sources,
        initialized:
          states.length > 0 &&
          states.every((state) => Boolean(state.initialized)),
        endReached:
          states.length > 0 && states.every((state) => state.endReached),
        hasMoreLocal: states.some((state) => state.hasMoreLocal),
        remoteHistoryState: states.some(
          (state) => state.remoteHistoryState === "may_have_more",
        )
          ? "may_have_more"
          : states.some((state) => state.remoteHistoryState === "offline")
            ? "offline"
            : states.every((state) => state.endReached)
              ? "complete"
              : "not_checked",
        loadMorePhase: states.some(
          (state) => state.loadMorePhase === "loading",
        )
          ? "loading"
          : states.some((state) => state.loadMorePhase === "retry")
            ? "retry"
            : "idle",
        loadMoreError:
          states.find((state) => state.loadMoreError)?.loadMoreError || null,
      };
    }
    return null;
  }, [
    activeAccountId,
    activeFolder,
    mailboxCapabilities,
    mailboxPageStates,
    query,
    remoteSearch,
    starredMailboxPageStates,
  ]);

  const canLoadOlder = useMemo(() => {
    if (!activePagination) return false;
    if (activePagination.endReached) return false;
    if (activeFolder === "starred") {
      return Object.values(activePagination.sources || {}).some(
        (source) => source?.nextCursor && !source.endReached,
      );
    }
    if (normalizedMailboxQuery(query)) {
      return Object.values(activePagination.sources || {}).some(
        (source) => source?.nextCursor && !source.endReached,
      );
    }
    return Boolean(activePagination.nextCursor);
  }, [activeFolder, activePagination, query]);

  const loadMoreState = useMemo(() => {
    if (!activePagination) return "complete";
    if (activePagination.loadMorePhase === "loading") return "loading";
    if (activePagination.loadMorePhase === "retry") return "retry";
    if (activePagination.endReached) return "complete";
    if (
      !networkActionsAvailable &&
      !activePagination.hasMoreLocal
    ) {
      return "offline";
    }
    if (
      ["unavailable", "not_supported"].includes(
        activePagination.remoteHistoryState,
      )
    ) {
      return "unavailable";
    }
    if (
      ["retry", "retryable_failure", "failed"].includes(
        activePagination.remoteHistoryState,
      )
    ) {
      return "retry";
    }
    if (activePagination.remoteHistoryState === "offline") return "offline";
    return "idle";
  }, [activePagination, networkActionsAvailable]);

  const handleLoadMore = useCallback(async () => {
    const accountId = activeAccountIdRef.current;
    if (!accountId || !canLoadOlder) return;
    const searchQuery = normalizedMailboxQuery(query);
    try {
      if (searchQuery) {
        await loadRemoteSearch({
          accountId,
          folder: activeFolder,
          searchQuery,
          append: true,
          currentSearch: remoteSearch,
        });
        return;
      }
      if (activeFolder === "starred") {
        const sources = activePagination?.sources || {};
        await Promise.all(
          Object.entries(sources)
            .filter(([, source]) => source?.nextCursor && !source.endReached)
            .map(async ([role, source]) => {
              let cursor = source.nextCursor;
              let completedContinuations = 0;
              let completedWithoutError = false;
              try {
                while (cursor) {
                  const page = await loadStarredMailboxRolePage({
                    accountId,
                    role,
                    cursor,
                    append: true,
                    settleLoadMore: false,
                  });
                  if (!page) return;
                  const nextCursor = nextEmptyMailboxPageCursor(
                    cursor,
                    page,
                    page.appendedItemCount,
                    completedContinuations,
                  );
                  if (
                    !nextCursor ||
                    activeAccountIdRef.current !== accountId ||
                    activeFolderRef.current !== "starred" ||
                    normalizedMailboxQuery(mailboxQueryRef.current)
                  ) {
                    break;
                  }
                  cursor = nextCursor;
                  completedContinuations += 1;
                }
                completedWithoutError = true;
              } finally {
                if (completedWithoutError) {
                  commitStarredMailboxPageState(
                    role,
                    (current) => ({
                      ...current,
                      loadMorePhase: "idle",
                      loadMoreError: null,
                    }),
                    accountId,
                  );
                }
              }
            }),
        );
        return;
      }
      let cursor = activePagination?.nextCursor;
      if (!cursor || !paginatedMailboxRoles.includes(activeFolder)) return;
      let completedContinuations = 0;
      while (cursor) {
        const page = await loadMailboxRolePage({
          accountId,
          role: activeFolder,
          cursor,
          append: true,
          settleLoadMore: false,
        });
        if (!page) return;
        const nextCursor = nextEmptyMailboxPageCursor(
          cursor,
          page,
          page.appendedItemCount,
          completedContinuations,
        );
        if (
          !nextCursor ||
          activeAccountIdRef.current !== accountId ||
          activeFolderRef.current !== activeFolder ||
          normalizedMailboxQuery(mailboxQueryRef.current)
        ) {
          break;
        }
        cursor = nextCursor;
        completedContinuations += 1;
      }
      commitMailboxPageState(
        activeFolder,
        (current) => ({
          ...current,
          loadMorePhase: "idle",
          loadMoreError: null,
        }),
        accountId,
      );
    } catch {
      // The page state retains the silent automatic-pagination failure so a
      // later explicit refresh can reconcile the folder without losing rows.
    }
  }, [
    activeFolder,
    activePagination,
    canLoadOlder,
    commitMailboxPageState,
    commitStarredMailboxPageState,
    loadMailboxRolePage,
    loadStarredMailboxRolePage,
    loadRemoteSearch,
    query,
    remoteSearch,
  ]);

  const folderCounts = useMemo(
    () => ({
      inbox: messages.filter((message) => !hasFlag(message, "\\Seen")).length,
    }),
    [messages],
  );

  const requestArchiveFolderSelection = useCallback(async (accountId) => {
    const candidates = await mailApi.listArchiveFolderCandidates(accountId);
    return new Promise((resolve, reject) => {
      archiveFolderRequestRef.current = { accountId, resolve, reject };
      setArchiveFolderDialog({
        accountId,
        candidates,
        selectedId: candidates[0]?.selectionId || "",
        pending: false,
        error: null,
      });
    });
  }, []);

  const cancelArchiveFolderSelection = useCallback(() => {
    const request = archiveFolderRequestRef.current;
    if (!request || archiveFolderDialog?.pending) return;
    archiveFolderRequestRef.current = null;
    setArchiveFolderDialog(null);
    request.reject(archiveFolderSelectionCancelledError());
  }, [archiveFolderDialog?.pending]);

  const confirmArchiveFolderSelection = useCallback(async () => {
    const request = archiveFolderRequestRef.current;
    const dialog = archiveFolderDialog;
    if (
      !request ||
      !dialog ||
      dialog.pending ||
      !dialog.selectedId ||
      request.accountId !== dialog.accountId
    ) {
      return;
    }
    setArchiveFolderDialog((current) =>
      current ? { ...current, pending: true, error: null } : current,
    );
    try {
      const capability = await mailApi.assignArchiveFolder(
        dialog.accountId,
        dialog.selectedId,
      );
      archiveFolderRequestRef.current = null;
      setArchiveFolderDialog(null);
      request.resolve(capability);
    } catch (error) {
      setArchiveFolderDialog((current) =>
        current
          ? {
              ...current,
              pending: false,
              error: describeError(error, "归档文件夹设置失败，请重试"),
            }
          : current,
      );
    }
  }, [archiveFolderDialog]);

  const ensureMailboxRoleAvailable = useCallback(
    async (role, accountId = activeAccountIdRef.current) => {
      if (!accountId || !["archive", "trash"].includes(role)) {
        throw new Error("邮箱文件夹请求无效");
      }
      const view = accountViewsRef.current.get(accountId) || {};
      const currentCapabilities =
        activeAccountIdRef.current === accountId
          ? mailboxCapabilities || view.mailboxCapabilities
          : view.mailboxCapabilities;
      const currentCapability = currentCapabilities?.[role];
      if (currentCapability?.status === "available") {
        return currentCapability;
      }
      const requestKey = `${accountId}:${role}`;
      const existingRequest =
        mailboxRoleCreationRequestsRef.current.get(requestKey);
      if (existingRequest) return existingRequest;

      const canEnsure =
        role === "archive" ||
        !currentCapability ||
        currentCapability.status === "discovery_pending" ||
        currentCapability.status === "needs_creation_confirmation" ||
        (currentCapability.status === "unavailable" &&
          currentCapability.unavailable_reason === "create_failed" &&
          currentCapability.retryable);
      if (!canEnsure) {
        throw new Error(mailboxSetupFailureMessage(currentCapability, role));
      }

      const request = (async () => {
        const capability =
          role === "archive"
            ? await requestArchiveFolderSelection(accountId)
            : await mailApi.createMailboxRole(accountId, role);
        const latestView = accountViewsRef.current.get(accountId) || {};
        accountViewsRef.current.set(accountId, {
          ...latestView,
          mailboxCapabilities: {
            ...(latestView.mailboxCapabilities || {}),
            [role]: capability,
          },
        });
        if (activeAccountIdRef.current === accountId) {
          setMailboxCapabilities((current) => ({
            ...(current || {}),
            [role]: capability,
          }));
        }
        if (capability.status !== "available") {
          throw new Error(mailboxSetupFailureMessage(capability, role));
        }
        return capability;
      })().finally(() => {
        if (
          mailboxRoleCreationRequestsRef.current.get(requestKey) === request
        ) {
          mailboxRoleCreationRequestsRef.current.delete(requestKey);
        }
      });
      mailboxRoleCreationRequestsRef.current.set(requestKey, request);
      return request;
    },
    [mailboxCapabilities, requestArchiveFolderSelection],
  );

  const handleMailboxCapabilityRetry = useCallback(
    async (role) => {
      const accountId = activeAccountIdRef.current;
      const capability = mailboxCapabilities?.[role];
      if (!accountId || !["archive", "trash"].includes(role)) return;
      if (
        role === "archive" ||
        (capability?.unavailable_reason === "create_failed" &&
          capability.retryable)
      ) {
        try {
          await ensureMailboxRoleAvailable(role, accountId);
          if (activeAccountIdRef.current !== accountId) return;
          setActiveFolder(role);
          setQuery("");
          setFilter("all");
          clearSelection();
          await loadMailboxRolePage({ accountId, role });
        } catch (error) {
          if (isArchiveFolderSelectionCancelled(error)) return;
          showToast(describeError(error, "邮箱文件夹创建失败"), "error");
        }
        return;
      }
      if (!networkActionsAvailableRef.current) {
        showToast("重新连接账户后才能重新确认邮箱能力", "error");
        return;
      }
      try {
        await mailApi.syncAll();
        const next = await refreshMailboxCapabilities(accountId);
        if (next[role]?.status === "available") {
          setActiveFolder(role);
          setQuery("");
          setFilter("all");
          clearSelection();
          await loadMailboxRolePage({ accountId, role });
        }
      } catch (error) {
        showToast(describeError(error, "邮箱能力重新确认失败"), "error");
      }
    },
    [
      clearSelection,
      ensureMailboxRoleAvailable,
      loadMailboxRolePage,
      mailboxCapabilities,
      refreshMailboxCapabilities,
      showToast,
    ],
  );

  const setMessageActionState = useCallback(
    (accountId, messageId, action, state) => {
      setMessageActionStates((current) => ({
        ...current,
        [messageActionKey(accountId, messageId, action)]: state,
      }));
    },
    [],
  );

  const selectAdjacentAfterRemoval = useCallback(
    (target, sourceRole, destinationRole = null, accountId) => {
      const targetId = localMessageId(target);
      if (targetId === null || !accountId) return;
      const sourceView = accountViewsRef.current.get(accountId) || {};
      const sourceItems =
        sourceView[mailboxViewField(sourceRole)] || [];
      const sourceIndex = sourceItems.findIndex(
        (message) => localMessageId(message) === targetId,
      );
      const adjacent =
        sourceIndex >= 0
          ? sourceItems[sourceIndex + 1] ||
            sourceItems[sourceIndex - 1] ||
            null
          : null;
      commitRoleItems(
        sourceRole,
        (items) =>
          items.filter((message) => localMessageId(message) !== targetId),
        accountId,
      );
      if (
        activeAccountIdRef.current !== accountId &&
        sourceView.selectedMessageId === targetId
      ) {
        const updatedView = accountViewsRef.current.get(accountId) || {};
        accountViewsRef.current.set(accountId, {
          ...updatedView,
          selectedMessageId: localMessageId(adjacent),
          selectedMessage: adjacent,
        });
      }
      setRemoteSearch((current) => {
        if (!current || current.accountId !== accountId) return current;
        const shouldLeaveAggregate =
          current.folder !== "starred" || destinationRole === "trash";
        return shouldLeaveAggregate
          ? {
              ...current,
              items: current.items.filter(
                (message) => localMessageId(message) !== targetId,
              ),
            }
          : current;
      });
      if (
        activeAccountIdRef.current === accountId &&
        selectedMessageIdRef.current === targetId &&
        (activeFolderRef.current === sourceRole ||
          (activeFolderRef.current === "starred" &&
            destinationRole === "trash"))
      ) {
        if (adjacent) void handleSelect(adjacent);
        else clearSelection();
      }
    },
    [clearSelection, commitRoleItems, handleSelect],
  );

  const refreshMutationRoles = useCallback(
    (accountId, sourceRole, destinationRole = null) => {
      const roles = [...new Set([sourceRole, destinationRole].filter(Boolean))];
      const targetCapabilities =
        accountViewsRef.current.get(accountId)?.mailboxCapabilities ||
        (activeAccountIdRef.current === accountId
          ? mailboxCapabilities
          : null);
      void Promise.allSettled(
        roles.flatMap((role) => {
          if (!capabilityAvailable(targetCapabilities, role)) return [];
          const refreshes = [loadMailboxRolePage({ accountId, role })];
          if (starredMailboxRoles.includes(role)) {
            refreshes.push(loadStarredMailboxRolePage({ accountId, role }));
          }
          return refreshes;
        }),
      );
    },
    [
      loadMailboxRolePage,
      loadStarredMailboxRolePage,
      mailboxCapabilities,
    ],
  );

  const executeArchiveMessage = useCallback(async (message, accountId) => {
    const messageId = localMessageId(message);
    if (
      !accountId ||
      messageId === null ||
      messageRole(message) !== "inbox"
    ) {
      return;
    }
    setMessageActionState(accountId, messageId, "archive", {
      status: "in_flight",
    });
    try {
      const receipt = await mailApi.archiveMessage(messageId);
      setMessageActionState(
        accountId,
        messageId,
        "archive",
        mutationActionState(receipt),
      );
      selectAdjacentAfterRemoval(message, "inbox", "archive", accountId);
      refreshMutationRoles(accountId, "inbox", "archive");
    } catch (error) {
      if (isArchiveFolderSelectionCancelled(error)) {
        setMessageActionState(accountId, messageId, "archive", {
          status: "idle",
        });
        return;
      }
      setMessageActionState(accountId, messageId, "archive", {
        status: "error",
        retryable: true,
        error,
      });
    }
  }, [
    refreshMutationRoles,
    selectAdjacentAfterRemoval,
    setMessageActionState,
  ]);
  const handleArchiveMessage = useCallback(async () => {
    const message = selectedMessage;
    const accountId = activeAccountIdRef.current;
    const messageId = localMessageId(message);
    if (!accountId || messageId === null || messageRole(message) !== "inbox") {
      return;
    }
    const capability = mailboxCapabilities?.archive;
    if (capability?.status === "available") {
      await executeArchiveMessage(message, accountId);
      return;
    }
    setMessageActionState(accountId, messageId, "archive", {
      status: "in_flight",
    });
    try {
      await ensureMailboxRoleAvailable("archive", accountId);
      if (activeAccountIdRef.current !== accountId) {
        setMessageActionState(accountId, messageId, "archive", {
          status: "idle",
        });
        return;
      }
      await executeArchiveMessage(message, accountId);
    } catch (error) {
      if (isArchiveFolderSelectionCancelled(error)) {
        setMessageActionState(accountId, messageId, "archive", {
          status: "idle",
        });
        return;
      }
      setMessageActionState(accountId, messageId, "archive", {
        status: "error",
        retryable: true,
        error,
      });
    }
  }, [
    ensureMailboxRoleAvailable,
    executeArchiveMessage,
    mailboxCapabilities,
    selectedMessage,
    setMessageActionState,
  ]);

  const handleMoveToTrash = useCallback(async () => {
    const message = selectedMessage;
    const accountId = activeAccountIdRef.current;
    const messageId = localMessageId(message);
    const sourceRole = messageRole(message);
    if (
      !accountId ||
      messageId === null ||
      !["inbox", "sent", "archive"].includes(sourceRole)
    ) {
      return;
    }
    setMessageActionState(accountId, messageId, "move_to_trash", {
      status: "in_flight",
    });
    try {
      await ensureMailboxRoleAvailable("trash", accountId);
      if (activeAccountIdRef.current !== accountId) {
        setMessageActionState(accountId, messageId, "move_to_trash", {
          status: "idle",
        });
        return;
      }
      const receipt = await mailApi.moveMessageToTrash(messageId);
      setMessageActionState(
        accountId,
        messageId,
        "move_to_trash",
        mutationActionState(receipt),
      );
      selectAdjacentAfterRemoval(message, sourceRole, "trash", accountId);
      refreshMutationRoles(accountId, sourceRole, "trash");
    } catch (error) {
      setMessageActionState(accountId, messageId, "move_to_trash", {
        status: "error",
        retryable: true,
        error,
      });
    }
  }, [
    ensureMailboxRoleAvailable,
    refreshMutationRoles,
    selectAdjacentAfterRemoval,
    selectedMessage,
    setMessageActionState,
  ]);

  const handleMoveToInbox = useCallback(async () => {
    const message = selectedMessage;
    const accountId = activeAccountIdRef.current;
    const messageId = localMessageId(message);
    const sourceRole = messageRole(message);
    if (
      !accountId ||
      messageId === null ||
      !["archive", "trash"].includes(sourceRole)
    ) {
      return;
    }
    setMessageActionState(accountId, messageId, "move_to_inbox", {
      status: "in_flight",
    });
    try {
      const receipt = await mailApi.moveMessageToInbox(messageId);
      setMessageActionState(
        accountId,
        messageId,
        "move_to_inbox",
        mutationActionState(receipt),
      );
      selectAdjacentAfterRemoval(message, sourceRole, "inbox", accountId);
      refreshMutationRoles(accountId, sourceRole, "inbox");
    } catch (error) {
      setMessageActionState(accountId, messageId, "move_to_inbox", {
        status: "error",
        retryable: true,
        error,
      });
    }
  }, [
    refreshMutationRoles,
    selectAdjacentAfterRemoval,
    selectedMessage,
    setMessageActionState,
  ]);

  const handleMarkUnread = useCallback(async () => {
    const message = selectedMessage;
    const accountId = activeAccountIdRef.current;
    const messageId = localMessageId(message);
    if (!accountId || messageId === null) return;
    mergeRemoteMessage(
      message,
      (current) => withSystemFlag(current, "\\Seen", false),
      accountId,
    );
    try {
      await mailApi.setMessageSeen(messageId, false);
    } catch {
      mergeRemoteMessage(message, withSeenFlag, accountId);
      showToast("标记未读失败，已恢复为已读状态", "error");
    }
  }, [
    mergeRemoteMessage,
    selectedMessage,
    showToast,
  ]);

  const handlePreparePermanentDelete = useCallback(async () => {
    const message = selectedMessage;
    const accountId = activeAccountIdRef.current;
    const messageId = localMessageId(message);
    if (
      !accountId ||
      messageId === null ||
      messageRole(message) !== "trash"
    ) {
      return;
    }
    consequentialActionRef.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    setMessageActionState(accountId, messageId, "permanent_delete", {
      status: "in_flight",
    });
    try {
      const plan = await mailApi.preparePermanentDelete(messageId);
      if (
        activeAccountIdRef.current !== accountId ||
        selectedMessageIdRef.current !== messageId
      ) {
        return;
      }
      setPermanentDelete({
        accountId,
        message,
        messageId,
        planId: plan.plan_id ?? plan.planId,
        pending: false,
        error: null,
      });
      setMessageActionState(accountId, messageId, "permanent_delete", {
        status: "idle",
      });
    } catch (error) {
      setMessageActionState(accountId, messageId, "permanent_delete", {
        status: "error",
        retryable: true,
        error,
      });
    }
  }, [selectedMessage, setMessageActionState]);

  const handleConfirmPermanentDelete = useCallback(async () => {
    const pending = permanentDelete;
    if (!pending?.planId) return;
    setPermanentDelete((current) =>
      current ? { ...current, pending: true, error: null } : current,
    );
    try {
      const receipt = await mailApi.confirmPermanentDelete(pending.planId);
      setMessageActionState(
        pending.accountId,
        pending.messageId,
        "permanent_delete",
        mutationActionState(receipt),
      );
      selectAdjacentAfterRemoval(
        pending.message,
        "trash",
        null,
        pending.accountId,
      );
      setPermanentDelete((current) =>
        current?.planId === pending.planId ? null : current,
      );
      refreshMutationRoles(pending.accountId, "trash");
    } catch (error) {
      setPermanentDelete((current) =>
        current?.planId === pending.planId
          ? {
              ...current,
              pending: false,
              error: describeError(error, "永久删除失败，请重试"),
            }
          : current,
      );
      setMessageActionState(
        pending.accountId,
        pending.messageId,
        "permanent_delete",
        {
          status: "error",
          retryable: true,
          error,
        },
      );
    }
  }, [
    permanentDelete,
    refreshMutationRoles,
    selectAdjacentAfterRemoval,
    setMessageActionState,
  ]);

  const focusMessageRow = useCallback((navigationKey) => {
    const row = navigationKey
      ? Array.from(
          document.querySelectorAll(".mail-row[data-navigation-key]"),
        ).find(
          (candidate) =>
            candidate.dataset.navigationKey === navigationKey,
        )
      : null;
    const opener = row?.querySelector(".mail-row__open");
    if (opener) {
      opener.focus({ preventScroll: true });
      return;
    }
    const searchInput = document.querySelector("#mail-list-panel input");
    if (searchInput) {
      searchInput.focus({ preventScroll: true });
      return;
    }
    document
      .querySelector('.folder-nav__item[data-selected="true"]')
      ?.focus({ preventScroll: true });
  }, []);

  const focusActiveFolder = useCallback(() => {
    document
      .querySelector('.folder-nav__item[data-selected="true"]')
      ?.focus({ preventScroll: true });
  }, []);

  const focusContactRow = useCallback((key) => {
    const row = Array.from(
      document.querySelectorAll(".contacts-row__select[data-contact-key]"),
    ).find((candidate) => candidate.dataset.contactKey === key);
    if (row) {
      row.focus({ preventScroll: true });
      return;
    }
    document
      .querySelector(".contacts-search input")
      ?.focus({ preventScroll: true });
  }, []);

  const completeReaderMotion = useCallback(() => {
    const phase = readerMotionRef.current;
    if (phase === "entering") {
      setReaderMotionPhase("open");
      return;
    }
    if (phase !== "exiting") return;

    const afterExit = readerAfterExitRef.current;
    readerAfterExitRef.current = null;
    if (afterExit?.restoreFocusKey) {
      focusMessageRow(afterExit.restoreFocusKey);
    }
    clearSelection({ navigationIntentId: afterExit?.navigationIntentId });
    afterExit?.run?.();
  }, [clearSelection, focusMessageRow, setReaderMotionPhase]);
  completeReaderMotionRef.current = completeReaderMotion;

  const requestReaderExit = useCallback(
    ({
      speed = "normal",
      restoreFocusKey = null,
      afterExit = null,
      navigationIntentId = null,
    } = {}) => {
      const run = typeof afterExit === "function" ? afterExit : null;
      if (
        selectedMessageIdRef.current === null ||
        !readerOpenRef.current
      ) {
        run?.();
        return;
      }

      const nextAction = { restoreFocusKey, run, navigationIntentId };
      if (prefersReducedMotion()) {
        if (restoreFocusKey) focusMessageRow(restoreFocusKey);
        clearSelection({ navigationIntentId });
        run?.();
        return;
      }

      readerAfterExitRef.current = nextAction;
      if (readerMotionRef.current === "exiting") {
        if (speed === "fast") setReaderExitSpeed("fast");
        return;
      }
      setReaderExitSpeed(speed);
      setReaderMotionPhase("exiting");
    },
    [clearSelection, focusMessageRow, setReaderMotionPhase],
  );

  useEffect(() => {
    if (!["entering", "exiting"].includes(readerMotion)) {
      return undefined;
    }
    const duration =
      readerMotion === "exiting" && readerExitSpeed === "fast"
        ? readerFastExitMs
        : readerWindowMotionMs;
    const timer = window.setTimeout(
      () => completeReaderMotionRef.current(),
      duration + motionFallbackPaddingMs,
    );
    return () => window.clearTimeout(timer);
  }, [readerExitSpeed, readerMotion]);

  const completeContactDetailMotion = useCallback(() => {
    const phase = contactDetailMotionRef.current;
    if (phase === "entering") {
      setContactDetailMotionPhase("open");
      return;
    }
    if (phase !== "exiting") return;

    const afterExit = contactDetailAfterExitRef.current;
    contactDetailAfterExitRef.current = null;
    if (afterExit?.restoreFocusKey) {
      focusContactRow(afterExit.restoreFocusKey);
    }
    clearContactSelection();
    afterExit?.run?.();
  }, [
    clearContactSelection,
    focusContactRow,
    setContactDetailMotionPhase,
  ]);
  completeContactDetailMotionRef.current = completeContactDetailMotion;

  const requestContactDetailExit = useCallback(
    ({
      speed = "normal",
      restoreFocusKey = null,
      afterExit = null,
    } = {}) => {
      const run = typeof afterExit === "function" ? afterExit : null;
      if (!selectedContactEmailRef.current) {
        run?.();
        return;
      }

      const nextAction = { restoreFocusKey, run };
      if (prefersReducedMotion()) {
        if (restoreFocusKey) focusContactRow(restoreFocusKey);
        clearContactSelection();
        run?.();
        return;
      }

      contactDetailAfterExitRef.current = nextAction;
      if (contactDetailMotionRef.current === "exiting") {
        if (speed === "fast") setContactDetailExitSpeed("fast");
        return;
      }
      setContactDetailExitSpeed(speed);
      setContactDetailMotionPhase("exiting");
    },
    [
      clearContactSelection,
      focusContactRow,
      setContactDetailMotionPhase,
    ],
  );

  useEffect(() => {
    if (!["entering", "exiting"].includes(contactDetailMotion)) {
      return undefined;
    }
    const duration =
      contactDetailMotion === "exiting" &&
      contactDetailExitSpeed === "fast"
        ? readerFastExitMs
        : readerWindowMotionMs;
    const timer = window.setTimeout(
      () => completeContactDetailMotionRef.current(),
      duration + motionFallbackPaddingMs,
    );
    return () => window.clearTimeout(timer);
  }, [contactDetailExitSpeed, contactDetailMotion]);

  const settleMailListMotion = useCallback(() => {
    const queuedIntent = queuedMailListIntentRef.current;
    queuedMailListIntentRef.current = null;
    activeMailListIntentRef.current = null;
    if (queuedIntent) startMailListIntentRef.current(queuedIntent);
  }, []);

  const completeMailListMotion = useCallback(() => {
    const phase = mailListMotionRef.current;
    const activeIntent = activeMailListIntentRef.current;

    if (phase === "switching-out") {
      if (activeIntent?.type === "account-switch") {
        activeIntent.commit?.();
        setMailListMotionPhase("switching-in");
        return;
      }
      if (activeIntent?.type === "switch") {
        if (activeIntent.folder !== activeFolderRef.current) {
          commitFolderChangeRef.current(activeIntent.folder);
        }
        setMailListMotionPhase("switching-in");
        return;
      }
      if (activeIntent?.type === "collapse") {
        setMailListMotionPhase("collapsing");
        return;
      }
      setMailListMotionPhase("switching-in");
      return;
    }

    if (phase === "collapsing") {
      if (activeIntent?.type === "account-switch") {
        activeIntent.commit?.();
      }
      setMailListMotionPhase("collapsed");
      activeIntent?.after?.();
      settleMailListMotion();
      return;
    }
    if (phase === "expanding" || phase === "switching-in") {
      setMailListMotionPhase("expanded");
      activeIntent?.after?.();
      settleMailListMotion();
    }
  }, [setMailListMotionPhase, settleMailListMotion]);
  completeMailListMotionRef.current = completeMailListMotion;

  const startMailListIntent = useCallback(
    (intent) => {
      if (!intent) return;
      if (!isWideMailWorkspaceRef.current || intent.instant) {
        activeMailListIntentRef.current = null;
        queuedMailListIntentRef.current = null;
        if (intent.type === "account-switch") {
          intent.commit?.();
          setMailListMotionPhase(
            intent.targetListExpanded ? "expanded" : "collapsed",
          );
          intent.after?.();
          return;
        }
        if (
          intent.type !== "collapse" &&
          intent.folder &&
          intent.folder !== activeFolderRef.current
        ) {
          commitFolderChangeRef.current(intent.folder);
        }
        setMailListMotionPhase("expanded");
        return;
      }
      const phase = mailListMotionRef.current;
      const isMoving = !["expanded", "collapsed"].includes(phase);

      if (isMoving) {
        if (phase === "switching-out" && intent.type === "switch") {
          // The old surface has not reached its midpoint yet, so retargeting
          // here avoids ever committing an obsolete intermediate folder.
          activeMailListIntentRef.current = intent;
          queuedMailListIntentRef.current = null;
        } else {
          queuedMailListIntentRef.current?.cancel?.();
          queuedMailListIntentRef.current = intent;
        }
        return;
      }

      if (prefersReducedMotion()) {
        activeMailListIntentRef.current = null;
        queuedMailListIntentRef.current = null;
        if (intent.type === "account-switch") {
          intent.commit?.();
          setMailListMotionPhase(
            intent.targetListExpanded ? "expanded" : "collapsed",
          );
          intent.after?.();
        } else if (intent.type === "collapse") {
          setMailListMotionPhase("collapsed");
        } else {
          if (
            intent.folder &&
            intent.folder !== activeFolderRef.current
          ) {
            commitFolderChangeRef.current(intent.folder);
          }
          setMailListMotionPhase("expanded");
        }
        return;
      }

      activeMailListIntentRef.current = intent;
      if (intent.type === "account-switch") {
        if (!intent.targetListExpanded) {
          if (phase === "collapsed") {
            intent.commit?.();
            setMailListMotionPhase("collapsed");
            activeMailListIntentRef.current = null;
            intent.after?.();
          } else {
            setMailListMotionPhase("collapsing");
          }
          return;
        }
        if (phase === "collapsed") {
          intent.commit?.();
          setMailListMotionPhase("expanding");
        } else {
          setMailListMotionPhase("switching-out");
        }
        return;
      }
      if (intent.type === "collapse") {
        if (phase === "collapsed") {
          activeMailListIntentRef.current = null;
          return;
        }
        setMailListMotionPhase("collapsing");
        return;
      }

      if (intent.type === "reveal") {
        if (
          intent.folder &&
          intent.folder !== activeFolderRef.current
        ) {
          commitFolderChangeRef.current(intent.folder);
        }
        if (phase === "collapsed") {
          setMailListMotionPhase("expanding");
        } else {
          setMailListMotionPhase("expanded");
          activeMailListIntentRef.current = null;
        }
        return;
      }

      if (intent.type === "switch") {
        setMailListMotionPhase("switching-out");
      }
    },
    [setMailListMotionPhase],
  );
  startMailListIntentRef.current = startMailListIntent;

  useEffect(() => {
    if (
      ![
        "collapsing",
        "expanding",
        "switching-out",
        "switching-in",
      ].includes(mailListMotion)
    ) {
      return undefined;
    }
    const duration = ["switching-out", "switching-in"].includes(
      mailListMotion,
    )
      ? mailListSwitchHalfMs
      : mailListWindowMotionMs;
    const timer = window.setTimeout(
      () => completeMailListMotionRef.current(),
      duration + motionFallbackPaddingMs,
    );
    return () => window.clearTimeout(timer);
  }, [mailListMotion]);

  const resetMailListMotion = useCallback(() => {
    activeMailListIntentRef.current = null;
    queuedMailListIntentRef.current = null;
    setMailListMotionPhase("expanded");
  }, [setMailListMotionPhase]);

  useEffect(() => {
    if (isWideMailWorkspace) return;
    const latestIntent =
      queuedMailListIntentRef.current ||
      activeMailListIntentRef.current;
    if (
      ["switch", "reveal"].includes(latestIntent?.type) &&
      latestIntent.folder &&
      latestIntent.folder !== activeFolderRef.current
    ) {
      commitFolderChangeRef.current(latestIntent.folder);
    }
    resetMailListMotion();
  }, [isWideMailWorkspace, resetMailListMotion]);

  useEffect(() => {
    if (
      ["collapsing", "collapsed"].includes(mailListMotion) &&
      selectedMessageIdRef.current !== null
    ) {
      clearSelection();
    }
  }, [clearSelection, mailListMotion]);

  const commitFolderChange = async (folder) => {
    const targetAccountId = activeAccountId;
    const shouldEnsureMailboxRole =
      ["archive", "trash"].includes(folder) &&
      !capabilityAvailable(mailboxCapabilities, folder);
    setIsSettingsOpen(false);
    setSettingsFocusTarget(null);

    if (shouldEnsureMailboxRole) {
      try {
        await ensureMailboxRoleAvailable(folder, targetAccountId);
        if (activeAccountIdRef.current !== targetAccountId) return;
      } catch (error) {
        if (isArchiveFolderSelectionCancelled(error)) return;
        showToast(
          describeError(
            error,
            folder === "archive"
              ? "归档文件夹设置失败，请重试"
              : "垃圾箱创建失败，请重试",
          ),
          "error",
        );
        return;
      }
    }
    setActiveFolder(folder);
    if (folder === "contacts") {
      setContactFilter("all");
      setContactQuery("");
    } else {
      setFilter("all");
      setQuery("");
    }
    clearContactSelection();
    clearSelection();
    setIsSidebarOpen(false);
    if (paginatedMailboxRoles.includes(folder) && targetAccountId) {
      const preserveSyncing =
        mailboxLoadStatesRef.current[folder]?.phase === "syncing";
      const cachedRead = runMailboxProjectionRefresh({
        accountId: targetAccountId,
        role: folder,
        preserveSyncing,
      });
      void cachedRead.catch(() => {});
      if (
        ["archive", "trash"].includes(folder) &&
        networkActionsAvailable
      ) {
        void cachedRead
          .catch(() => undefined)
          .then(() => mailApi.syncMailbox(targetAccountId, folder))
          .catch(() => {});
      }
    } else if (folder === "starred" && activeAccountId) {
      const roles = starredMailboxRoles.filter((role) =>
        capabilityAvailable(mailboxCapabilities, role),
      );
      void Promise.allSettled(
        roles.map((role) =>
          runMailboxProjectionRefresh({
            accountId: activeAccountId,
            role,
            projection: "starred",
          }),
        ),
      );
    }
  };
  commitFolderChangeRef.current = commitFolderChange;

  const handleCollapseMailList = () => {
    if (
      !isWideMailWorkspace ||
      !document.getElementById("mail-list-panel")
    ) {
      return;
    }
    invalidatePreparedFolderMotion();
    focusActiveFolder();
    const collapse = () =>
      startMailListIntentRef.current({ type: "collapse" });
    if (selectedMessageIdRef.current !== null) {
      requestReaderExit({ speed: "fast", afterExit: collapse });
    } else {
      collapse();
    }
  };

  const handleFolderChange = (folder) => {
    const folderMotionRequest = folderMotionRequestRef.current + 1;
    folderMotionRequestRef.current = folderMotionRequest;
    if (
      ["archive", "trash"].includes(folder) &&
      !capabilityAvailable(mailboxCapabilities, folder)
    ) {
      void commitFolderChange(folder);
      return;
    }

    const currentFolder = activeFolderRef.current;
    const currentFolderActivated = folder === currentFolder;
    const hasVisibleListWorkspace = Boolean(
      document.querySelector(".mail-workspace .mail-list-motion-frame"),
    );
    const canCoordinate =
      isWideMailWorkspace &&
      !isSettingsOpen &&
      hasVisibleListWorkspace;

    if (!canCoordinate) {
      resetMailListMotion();
      void commitFolderChange(folder);
      return;
    }

    const phase = mailListMotionRef.current;
    const listIsRetractedOrRetracting = [
      "collapsed",
      "collapsing",
    ].includes(phase);
    const intent =
      phase === "switching-out"
        ? { type: "switch", folder }
        : currentFolderActivated
          ? listIsRetractedOrRetracting
            ? { type: "reveal", folder }
            : { type: "collapse" }
          : listIsRetractedOrRetracting
            ? { type: "reveal", folder }
            : { type: "switch", folder };
    const targetWorkspaceReady =
      folder === "contacts" && folder !== currentFolder
        ? preloadContactsWorkspace()
        : null;
    const startIntent = () => {
      const begin = () => {
        if (folderMotionRequestRef.current !== folderMotionRequest) return;
        startMailListIntentRef.current(intent);
      };
      if (targetWorkspaceReady) {
        void targetWorkspaceReady.then(begin, begin);
      } else {
        begin();
      }
    };

    if (intent.type !== "reveal" && selectedMessageIdRef.current !== null) {
      requestReaderExit({
        speed: "fast",
        afterExit: () => {
          if (currentFolder === "contacts") clearContactSelection();
          startIntent();
        },
      });
    } else if (
      intent.type !== "reveal" &&
      currentFolder === "contacts" &&
      selectedContactEmailRef.current
    ) {
      requestContactDetailExit({
        speed: "fast",
        afterExit: startIntent,
      });
    } else {
      startIntent();
    }
  };

  const handleSelectContact = (contact) => {
    invalidatePreparedFolderMotion();
    const previousKey = contactNavigationKey(
      selectedContactEmailRef.current,
      selectedContactAccountIdRef.current,
    );
    const nextEmail = contact?.email || null;
    const nextAccountId = contact?.accountId || null;
    const nextKey = contactNavigationKey(nextEmail, nextAccountId);
    clearSelection();
    if (!nextKey) {
      clearContactSelection();
      return;
    }
    if (previousKey !== nextKey) {
      contactMessagesRequestRef.current += 1;
      setContactMessages([]);
      setContactMessagesState("loading");
      setContactMessagesError(null);
    }
    selectedContactEmailRef.current = nextEmail;
    selectedContactAccountIdRef.current = nextAccountId;
    setSelectedContactEmail(nextEmail);
    setSelectedContactAccountId(nextAccountId);
    if (!previousKey || contactDetailMotionRef.current === "idle") {
      presentContactDetail("entering");
    } else if (
      previousKey !== nextKey ||
      contactDetailMotionRef.current === "exiting"
    ) {
      presentContactDetail("open");
    }
  };

  const handleBackToContacts = () => {
    clearSelection();
    requestContactDetailExit({
      restoreFocusKey: contactNavigationKey(
        selectedContactEmailRef.current,
        selectedContactAccountIdRef.current,
      ),
    });
  };

  const handleOpenContactMessage = async (message) => {
    // Contact history deliberately carries no body/HTML across the Tauri
    // boundary. Force a local hydration even when SQLite reports that the
    // canonical cached message body has already been fetched.
    const messageId = localMessageId(message);
    const contactContext = {
      email: selectedContactEmailRef.current,
      accountId:
        selectedContactAccountIdRef.current || activeAccountIdRef.current,
      filter: contactFilter,
    };
    const targetAccountId =
      contactContext.accountId;
    if (!messageId || !targetAccountId || !contactContext.email) {
      showToast("这封往来邮件缺少可用的本地标识", "error");
      return;
    }
    const requestId = contactMessageOpenRequestRef.current + 1;
    contactMessageOpenRequestRef.current = requestId;
    const navigationIntentId = navigationIntentRef.current + 1;
    navigationIntentRef.current = navigationIntentId;
    replyPreparationRequestRef.current += 1;

    if (activeAccountIdRef.current !== targetAccountId) {
      const switched = await handleSwitchAccount(targetAccountId, {
        navigationIntentId,
        contactMessageOpenRequestId: requestId,
        preserveContactContext: contactContext,
        selectFirst: false,
      });
      if (!switched) return;
    }
    if (
      contactMessageOpenRequestRef.current !== requestId ||
      navigationIntentRef.current !== navigationIntentId ||
      activeAccountIdRef.current !== targetAccountId
    ) {
      return;
    }
    selectedContactEmailRef.current = contactContext.email;
    selectedContactAccountIdRef.current = contactContext.accountId;
    setSelectedContactEmail(contactContext.email);
    setSelectedContactAccountId(contactContext.accountId);
    await handleSelect(toContactDisplayMessage(message), true, {
      navigationIntentId,
      contactMessageOpenRequestId: requestId,
    });
  };

  const navigateContactRelative = (offset) => {
    const next = contactDisplayMessages[contactSelectedIndex + offset];
    if (next) void handleOpenContactMessage(next);
  };

  const handleToggleContactFavorite = async (contact) => {
    const favoriteAccountId =
      typeof contact?.accountId === "string"
        ? contact.accountId.trim() || null
        : null;
    if (!contact?.email || !favoriteAccountId || !activeAccountId) {
      showToast(
        "无法确认联系人所属的邮箱账户，请刷新通讯录后重试。",
        "error",
      );
      return;
    }
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
    const accountId = activeAccountIdRef.current;
    if (!accountId) return;
    const folder = activeFolderRef.current;
    if (folder === "starred") {
      setStarredViewRetention({ accountId, items: [] });
    }
    const feedbackId = syncFeedbackSequenceRef.current + 1;
    syncFeedbackSequenceRef.current = feedbackId;
    publishManualSyncFeedback({
      id: feedbackId,
      accountId,
      folder,
      state: "syncing",
      message: manualSyncProgressMessage(folder),
    });
    setSyncState("syncing");
    if (mailboxFolders.includes(folder)) {
      updateMailboxLoadState(folder, (current) => ({
        ...current,
        phase: "syncing",
      }));
    }
    try {
      let synchronizedCount = 0;
      if (paginatedMailboxRoles.includes(folder)) {
        const report = await mailApi.syncMailbox(accountId, folder);
        synchronizedCount = synchronizedMessageCount(report);
      } else if (folder === "starred") {
        const roles = starredMailboxRoles.filter((role) =>
          capabilityAvailable(mailboxCapabilities, role),
        );
        const reports = await Promise.all(
          roles.map((role) => mailApi.syncMailbox(accountId, role)),
        );
        synchronizedCount = synchronizedMessageCount(reports);
      } else if (folder === "drafts") {
        const report = await mailApi.syncDrafts();
        synchronizedCount = synchronizedMessageCount(report);
        await Promise.all([refreshDrafts(), refreshOutbox()]);
      } else if (folder === "outbox") {
        const [, refreshedOutbox] = await Promise.all([
          refreshDrafts(),
          refreshOutbox(),
        ]);
        synchronizedCount = Array.isArray(refreshedOutbox)
          ? refreshedOutbox.length
          : 0;
      } else {
        const report = await mailApi.syncAll();
        synchronizedCount = synchronizedMessageCount(report);
      }
      if (normalizedMailboxQuery(query)) {
        await loadRemoteSearch({
          accountId,
          folder,
          searchQuery: query,
        });
      }
      if (folder === "contacts") {
        await loadContacts({ accountId, silent: true });
        if (selectedContactEmail) {
          await loadContactMessages(selectedContactEmail, {
            accountId: selectedContactAccountId || accountId,
            silent: true,
          });
        }
      }
      setSyncState("done");
      publishManualSyncFeedback(
        {
          id: feedbackId,
          accountId,
          folder,
          state: "success",
          message: manualSyncSuccessMessage(folder, synchronizedCount),
        },
        true,
      );
    } catch (error) {
      setSyncState("error");
      if (mailboxFolders.includes(folder)) {
        updateMailboxLoadState(folder, (current) => ({
          ...current,
          phase: "error",
        }));
      }
      publishManualSyncFeedback(
        {
          id: feedbackId,
          accountId,
          folder,
          state: "error",
          message: `同步失败：${describeError(error, "请检查网络后重试")}`,
        },
        true,
      );
    }
  };

  const reconcileSentAfterDelivery = useCallback(() => {
    void mailApi
      .syncSent()
      // The local Outbox fallback remains visible if the provider's Sent copy
      // is briefly delayed or the follow-up sync is offline. Scheduled full
      // reconciliation will retry without changing delivery state.
      .catch(() => undefined);
  }, []);

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

  const handleComposeBodyDirty = () => {
    const current = composerRef.current;
    if (!current || current.locked || current.readOnlyUnsupported) return;
    const next = {
      ...current,
      dirty: true,
      revision: current.revision + 1,
      saveStatus: "dirty",
    };
    const accountId = activeAccountIdRef.current;
    if (accountId) composerSessionsRef.current.set(accountId, next);
    composerRef.current = next;

    if (composeBodyAutosaveTimerRef.current !== null) {
      window.clearTimeout(composeBodyAutosaveTimerRef.current);
    }
    const sessionId = next.sessionId;
    composeBodyAutosaveTimerRef.current = window.setTimeout(() => {
      composeBodyAutosaveTimerRef.current = null;
      const latest = composerRef.current;
      if (latest?.sessionId !== sessionId || latest.locked) return;
      void saveDraftNow().catch((error) => {
        showToast(describeError(error, "草稿自动保存失败"), "error");
      });
    }, localDraftDebounceMs);
  };

  const handleComposeBodyChange = (updater, { publish = false } = {}) => {
    const current = composerRef.current;
    if (!current || current.readOnlyUnsupported) return;
    const nextValue =
      typeof updater === "function" ? updater(current.value) : updater;
    const next = { ...current, value: nextValue };
    const accountId = activeAccountIdRef.current;
    if (accountId) composerSessionsRef.current.set(accountId, next);
    composerRef.current = next;
    if (publish) setComposer(next);
  };

  const handleComposeMinimizedChange = useCallback(
    (minimized) => {
      commitComposer((current) =>
        current && current.minimized !== minimized
          ? { ...current, minimized }
          : current,
      );
    },
    [commitComposer],
  );

  const handleAddAttachments = async () => {
    const initial = composerRef.current;
    if (!initial || initial.locked || initial.readOnlyUnsupported) return;
    const sessionId = initial.sessionId;
    let stabilizedDraft = null;
    commitComposer((current) =>
      current?.sessionId === sessionId
        ? {
            ...current,
            locked: true,
            attachmentOperations: {
              add: { status: "saving" },
              remove: { ...(current.attachmentOperations?.remove || {}) },
            },
          }
        : current,
    );

    try {
      let draft = await saveDraftNow({ force: true });
      stabilizedDraft = draft;
      const current = composerRef.current;
      if (!current || current.sessionId !== sessionId) return;

      if (!draft) {
        if (
          current.draftId ||
          current.persistedDraft ||
          hasDraftContent(current.value)
        ) {
          throw new Error("当前内容尚未稳定保存，未打开附件选择器");
        }
        draft = await mailApi.createComposeDraft();
        if (
          !draft?.id ||
          !Number.isInteger(draft.local_version) ||
          draft.local_version < 1
        ) {
          throw new Error("无法创建稳定的空白草稿");
        }
        cacheAuthoritativeDrafts(draft);
        stabilizedDraft = draft;
        commitComposer((latest) =>
          latest?.sessionId === sessionId
            ? {
                ...latest,
                draftId: draft.id,
                baseLocalVersion: draft.local_version,
                persistedDraft: draft,
                value: draftToRequest(draft),
                dirty: false,
                revision: latest.revision + 1,
                saveStatus: "saved",
              }
            : latest,
        );
      }

      const outcome = await mailApi.addDraftAttachments(
        draft.id,
        draft.local_version,
      );
      applyAttachmentMutationOutcome(sessionId, outcome, { kind: "add" });
    } catch (error) {
      commitComposer((current) =>
        current?.sessionId === sessionId
          ? {
              ...current,
              locked: false,
              saveStatus: stabilizedDraft ? "saved" : "error",
              attachmentOperations: {
                add: {
                  status: "error",
                  message: describeError(error, "添加附件失败"),
                },
                remove: { ...(current.attachmentOperations?.remove || {}) },
              },
            }
          : current,
      );
      showToast(describeError(error, "添加附件失败，请重试"), "error");
    }
  };

  const handleRemoveAttachment = async (attachmentId) => {
    const initial = composerRef.current;
    if (
      !initial ||
      initial.locked ||
      initial.readOnlyUnsupported ||
      !initial.draftId ||
      !Number.isInteger(initial.baseLocalVersion) ||
      !attachmentId
    ) {
      return;
    }
    const sessionId = initial.sessionId;
    let stabilizedDraft = null;
    commitComposer((current) =>
      current?.sessionId === sessionId
        ? {
            ...current,
            locked: true,
            attachmentOperations: {
              add: current.attachmentOperations?.add || null,
              remove: {
                ...(current.attachmentOperations?.remove || {}),
                [attachmentId]: { status: "saving" },
              },
            },
          }
        : current,
    );

    try {
      const draft = await saveDraftNow({ force: true });
      stabilizedDraft = draft;
      if (
        !draft?.id ||
        !Number.isInteger(draft.local_version) ||
        composerRef.current?.sessionId !== sessionId
      ) {
        throw new Error("移除附件前无法稳定保存当前草稿");
      }
      const outcome = await mailApi.removeDraftAttachment(
        draft.id,
        attachmentId,
        draft.local_version,
      );
      applyAttachmentMutationOutcome(sessionId, outcome, {
        kind: "remove",
        attachmentId,
      });
    } catch (error) {
      commitComposer((current) =>
        current?.sessionId === sessionId
          ? {
              ...current,
              locked: false,
              saveStatus: stabilizedDraft ? "saved" : "error",
              attachmentOperations: {
                add: current.attachmentOperations?.add || null,
                remove: {
                  ...(current.attachmentOperations?.remove || {}),
                  [attachmentId]: {
                    status: "error",
                    message: describeError(error, "移除附件失败"),
                  },
                },
              },
            }
          : current,
      );
      showToast(describeError(error, "移除附件失败，请重试"), "error");
    }
  };

  const handleSaveDraftAndMinimize = async ({ syncRemote = true } = {}) => {
    const initial = composerRef.current;
    if (!initial || initial.locked) return false;
    const sessionId = initial.sessionId;
    commitComposer((current) =>
      current?.sessionId === sessionId
        ? { ...current, locked: true }
        : current,
    );
    try {
      const draft = await saveDraftNow({ force: true });
      if (composerRef.current?.sessionId !== sessionId) return false;
      commitComposer((current) =>
        current?.sessionId === sessionId
          ? { ...current, locked: false }
          : current,
      );

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
      return true;
    } catch (error) {
      commitComposer((current) =>
        current?.sessionId === sessionId
          ? { ...current, locked: false, saveStatus: "error" }
          : current,
      );
      showToast(describeError(error, "草稿保存失败"), "error");
      return false;
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
      const sendAccountId = activeAccountIdRef.current;
      if (!sendAccountId) throw new Error("当前没有可用的发件账户。");
      const sendRequest = {
        ...draftToRequest(draft),
        draftId: draft.id,
        expectedLocalVersion: draft.local_version,
      };
      const confirmedRecipients = [
        ...sendRequest.to,
        ...sendRequest.cc,
        ...sendRequest.bcc,
      ];

      // The exact draft version is stable now. Release the composer before
      // SMTP finishes; Rust persists and owns the immutable Outbox attempt.
      commitComposer(null);
      setBackgroundSendCounts((current) => ({
        ...current,
        [sendAccountId]: (current[sendAccountId] || 0) + 1,
      }));

      void (async () => {
        try {
          const result = await mailApi.sendDraft(
            sendRequest.draftId,
            sendRequest.expectedLocalVersion,
            confirmedRecipients,
          );
          if (activeAccountIdRef.current === sendAccountId) {
            await Promise.all([refreshDrafts(), refreshOutbox()]);
          }

          if (result.status !== "sent") {
            const deliveryMessages = {
              retryable: "邮件保留在发件队列，请稍后查看状态",
              rejected: "服务器拒绝了这封邮件，请查看发件队列",
              delivery_unknown:
                "投递结果未知，请先到邮箱服务器确认，切勿立即重发",
            };
            showToast(
              deliveryMessages[result.status] ||
                "邮件尚未发送，已保留在发件队列",
              "error",
              result.status === "delivery_unknown",
            );
            return;
          }

          markSentAttention(sendAccountId);
          if (activeAccountIdRef.current === sendAccountId) {
            reconcileSentAfterDelivery();
          }
        } catch (error) {
          if (activeAccountIdRef.current === sendAccountId) {
            await Promise.allSettled([refreshDrafts(), refreshOutbox()]);
          }
          showToast(
            describeError(error, "邮件未能进入发件队列，已保存的草稿仍然保留"),
            "error",
          );
        } finally {
          setBackgroundSendCounts((current) => {
            const remaining = (current[sendAccountId] || 1) - 1;
            if (remaining > 0) {
              return { ...current, [sendAccountId]: remaining };
            }
            const next = { ...current };
            delete next[sendAccountId];
            return next;
          });
        }
      })();
    } catch (error) {
      commitComposer((current) =>
        current ? { ...current, locked: false, saveStatus: "error" } : current,
      );
      showToast(describeError(error, "发送前保存草稿失败"), "error");
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

  const handlePrepareDeliveryUnknownDecision = (decision) => {
    if (!["confirm_delivered", "retry_once"].includes(decision)) return;
    const item = selectedMessage?.outbox;
    const outboxId =
      typeof item?.id === "string" ? item.id.trim() : "";
    const expectedAttempts = item?.attempts;
    if (
      !outboxId ||
      item.status !== "delivery_unknown" ||
      !Number.isSafeInteger(expectedAttempts) ||
      expectedAttempts < 0 ||
      expectedAttempts > 0xffffffff
    ) {
      showToast("发件队列状态已变化，请刷新后重新选择", "error");
      void refreshOutbox().catch(() => {});
      return;
    }
    if (decision === "retry_once" && !networkActionsAvailable) {
      showToast("重新连接账户后才能承担风险重试", "error");
      return;
    }
    consequentialActionRef.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    setDeliveryUnknownDecision({
      accountId: activeAccountIdRef.current,
      outboxId,
      subject: selectedMessage?.subject || "",
      expectedAttempts,
      decision,
      pending: false,
      stale: false,
      error: null,
    });
  };

  const handleConfirmDeliveryUnknownDecision = async () => {
    const pending = deliveryUnknownDecision;
    if (
      !pending ||
      pending.pending ||
      pending.stale ||
      activeAccountIdRef.current !== pending.accountId
    ) {
      return;
    }
    if (
      pending.decision === "retry_once" &&
      !networkActionsAvailableRef.current
    ) {
      setDeliveryUnknownDecision((current) =>
        current
          ? {
              ...current,
              error: "重新连接账户后才能承担重复投递风险并重试。",
            }
          : current,
      );
      return;
    }
    setDeliveryUnknownDecision((current) =>
      current ? { ...current, pending: true, error: null } : current,
    );
    try {
      const result = await mailApi.resolveDeliveryUnknown({
        outboxId: pending.outboxId,
        expectedAttempts: pending.expectedAttempts,
        decision: pending.decision,
        acknowledgeDuplicateRisk: pending.decision === "retry_once",
      });
      const resultId =
        typeof result?.id === "string" ? result.id.trim() : "";
      if (!resultId || resultId !== pending.outboxId) {
        throw new Error("后端返回了不匹配的发件队列记录");
      }
      const authoritativeResult =
        resultId === result.id ? result : { ...result, id: resultId };
      commitAuthoritativeOutboxItem(authoritativeResult, pending.accountId);
      setDeliveryUnknownDecision(null);
      void refreshDrafts().catch(() => {});
      if (authoritativeResult.status === "sent") {
        showToast(
          pending.decision === "confirm_delivered"
            ? "已根据你的核对结果标记为已投递"
            : "邮件已重试并确认投递",
        );
        reconcileSentAfterDelivery();
      } else {
        const outcomeMessage =
          authoritativeResult.status === "delivery_unknown"
            ? "再次投递的结果仍未知，请重新核对服务器后再决定"
            : authoritativeResult.status === "rejected"
              ? "服务器拒绝了这次明确重试"
              : "明确重试已结束，发件队列状态已更新";
        showToast(
          outcomeMessage,
          "error",
          authoritativeResult.status === "delivery_unknown",
        );
      }
    } catch (error) {
      let refreshedItem = null;
      let refreshCompleted = false;
      try {
        const refreshed = await refreshOutbox();
        refreshCompleted = Array.isArray(refreshed);
        refreshedItem = refreshed?.find(
          (item) => item.id === pending.outboxId,
        );
      } catch {
        // Keep the reviewed item and the original actionable failure visible.
      }
      const stale =
        refreshCompleted &&
        (!refreshedItem ||
          refreshedItem.status !== "delivery_unknown" ||
          refreshedItem.attempts !== pending.expectedAttempts);
      setDeliveryUnknownDecision((current) =>
        current?.outboxId === pending.outboxId &&
        current?.expectedAttempts === pending.expectedAttempts
          ? {
              ...current,
              pending: false,
              stale,
              error: stale
                ? "发件队列状态已刷新。请取消后查看最新状态，再重新选择操作。"
                : `${describeError(error, "未能处理投递结果").replace(/[。！？!?]+$/u, "")}。邮件仍保留在发件队列。`,
            }
          : current,
      );
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
      try {
        const restored = await mailApi.getDesktopSettings();
        if (settingsSaveRequestRef.current !== requestId) return;
        setSettings(restored);
      } catch {
        // Keep the attempted values visible when even the persisted snapshot
        // cannot be reloaded; the error state still makes the failure clear.
      }
      setSettingsSaveStatus("error");
      showToast(describeError(error, "桌面设置保存失败"), "error");
    }
  };

  const handleSelectAppearance = async (request) => {
    const updated = await mailApi.selectAppearanceTheme(request);
    setAppearance(updated);
    return updated;
  };

  const handleUpdateAppearancePreferences = async (request) => {
    const updated = await mailApi.updateAppearancePreferences(request);
    setAppearance(updated);
    return updated;
  };

  const handleImportCustomTheme = async (request) => {
    const updated = await mailApi.importCustomTheme(request);
    setAppearance(updated);
    return updated;
  };

  const handleUpdateCustomTheme = async (request) => {
    const updated = await mailApi.updateCustomTheme(request);
    setAppearance(updated);
    return updated;
  };

  const handleDeleteCustomTheme = async (id) => {
    const updated = await mailApi.deleteCustomTheme(id);
    setAppearance(updated);
    return updated;
  };

  useThemeSchedule({
    enabled: settings.themeScheduleEnabled,
    schedule: {
      dayStart: settings.themeScheduleDayStart,
      duskStart: settings.themeScheduleDuskStart,
      nightStart: settings.themeScheduleNightStart,
    },
    activeTheme: appearance?.activeTheme,
    onApply: (themeId) =>
      handleSelectAppearance({ kind: "builtin", id: themeId }),
  });

  const restoreComposerAfterFailedAccountConnection = useCallback(
    (sessionId) => {
      if (!sessionId) return;
      commitComposer((current) =>
        current?.sessionId === sessionId && current.minimized
          ? { ...current, minimized: false, startMinimized: false }
          : current,
      );
    },
    [commitComposer],
  );

  const handleConfigureAccount = async (request) => {
    const previousAccountId = activeAccountIdRef.current;
    const expandedComposerSessionId = composerRef.current?.minimized
      ? null
      : (composerRef.current?.sessionId ?? null);
    if (!(await prepareComposerForAccountSwitch())) {
      return;
    }
    rememberActiveAccountViewRef.current(previousAccountId);
    setAccountSubmitStatus("saving");
    setAccountError(null);
    setAccountErrorProvider(request.provider);
    try {
      const status = await mailApi.configureAccount(request);
      setAccountStatus(status);
      const backendUsable = status.configured && status.backendReady;
      if (!backendUsable) {
        const message = status.startupError
          ? describeError(status.startupError, "邮箱账户初始化没有完成")
          : "账户信息已保存，但邮箱服务尚未就绪，请检查授权信息。";
        setAccountError(message);
        setAccountSubmitStatus("error");
        return;
      }

      const nextAccountId = status.activeAccountId || status.accountId || null;
      activeAccountIdRef.current = nextAccountId;
      accountStatusRef.current = status;
      if (previousAccountId !== nextAccountId) {
        activateComposerForAccount(nextAccountId);
        invalidateForwardPreparationsForAccount(previousAccountId);
        forwardPreparationRequestRef.current += 1;
        clearSelection();
        messageBodyCacheRef.current.clear();
        setMessages([]);
        setSentMessages([]);
        setArchiveMessages([]);
        setTrashMessages([]);
        setDrafts([]);
        setOutbox([]);
        setSentOutboxFallbacks([]);
        setMailboxPageStates(createMailboxPageStates());
        setStarredMailboxPageStates(createStarredMailboxPageStates());
        setMailboxCapabilities(null);
        setRemoteSearch(null);
      }
      const networkUsable =
        status.credentialAvailable && status.networkReady !== false;
      beginMailboxLoading(networkUsable ? "syncing" : "loading");
      await loadMailboxData({
        selectFirst: true,
        preserveSyncing: networkUsable,
      });
      if (!networkUsable) {
        setAccountError(
          describeError(
            status.startupError,
            "本地邮箱已打开，但账户凭据或网络连接不可用。",
          ),
        );
      }
      setAccountSubmitStatus("saved");
    } catch (error) {
      const message = describeError(
        error,
        "账户配置失败，请检查地址和授权信息",
      );
      setAccountError(message);
      setAccountErrorProvider(request.provider);
      setAccountSubmitStatus("error");
      restoreComposerAfterFailedAccountConnection(expandedComposerSessionId);
    }
  };

  const applyActiveAccount = async (
    status,
    previousAccountId = activeAccountIdRef.current,
  ) => {
    setAccountStatus(status);
    const nextAccountId = status.activeAccountId || status.accountId || null;
    activeAccountIdRef.current = nextAccountId;
    accountStatusRef.current = status;
    const networkUsable = canUseAccountNetwork(status);
    beginMailboxLoading(networkUsable ? "syncing" : "loading");
    if (previousAccountId !== nextAccountId) {
      activateComposerForAccount(nextAccountId);
      invalidateForwardPreparationsForAccount(previousAccountId);
      forwardPreparationRequestRef.current += 1;
      clearSelection();
      messageBodyCacheRef.current.clear();
      setMessages([]);
      setSentMessages([]);
      setArchiveMessages([]);
      setTrashMessages([]);
      setDrafts([]);
      setOutbox([]);
      setSentOutboxFallbacks([]);
      setMailboxPageStates(createMailboxPageStates());
      setStarredMailboxPageStates(createStarredMailboxPageStates());
      setMailboxCapabilities(null);
      setRemoteSearch(null);
    }
    if (status.configured && status.backendReady) {
      await loadMailboxData({
        selectFirst: true,
        preserveSyncing: networkUsable,
      });
    }
    void prefetchAccountViews(status);
    setAccountSubmitStatus("saved");
    setAccountError(null);
    setAccountErrorProvider(null);
  };

  const handleConnectGoogle = async () => {
    const previousAccountId = activeAccountIdRef.current;
    const expandedComposerSessionId = composerRef.current?.minimized
      ? null
      : (composerRef.current?.sessionId ?? null);
    if (!(await prepareComposerForAccountSwitch())) {
      return;
    }
    rememberActiveAccountViewRef.current(previousAccountId);
    setAccountSubmitStatus("saving");
    setAccountError(null);
    setAccountErrorProvider("gmail");
    try {
      const status = await mailApi.connectGoogleAccount();
      await applyActiveAccount(status, previousAccountId);
    } catch (error) {
      const message = describeError(error, "Google 登录失败，请重试");
      setAccountError(message);
      setAccountErrorProvider("gmail");
      setAccountSubmitStatus("error");
      restoreComposerAfterFailedAccountConnection(expandedComposerSessionId);
    }
  };

  const rememberActiveAccountView = useCallback(
    (accountId) => {
      if (!accountId) return null;
      const listExpanded =
        !isWideMailWorkspaceRef.current ||
        !["collapsing", "collapsed"].includes(
          mailListMotionRef.current,
        );
      const view = {
        ...(accountViewsRef.current.get(accountId) || {}),
        messages,
        sentMessages,
        archiveMessages,
        trashMessages,
        drafts,
        outbox,
        sentOutboxFallbacks,
        selectedMessageId,
        selectedMessage,
        navigationState: {
          folder: activeFolderRef.current,
          listExpanded,
          readerOpen: Boolean(
            listExpanded &&
            selectedMessageIdRef.current !== null &&
            readerOpenRef.current &&
            readerMotionRef.current !== "exiting",
          ),
        },
        mailboxPageStates,
        starredMailboxPageStates,
        mailboxCapabilities,
      };
      accountViewsRef.current.set(accountId, view);
      return view;
    },
    [
      archiveMessages,
      drafts,
      mailboxCapabilities,
      mailboxPageStates,
      messages,
      outbox,
      selectedMessage,
      selectedMessageId,
      sentMessages,
      sentOutboxFallbacks,
      starredMailboxPageStates,
      trashMessages,
    ],
  );
  rememberActiveAccountViewRef.current = rememberActiveAccountView;

  const handleSwitchAccount = async (accountId, options = {}) => {
    invalidatePreparedFolderMotion();
    const suppliedNavigationIntentId = options.navigationIntentId ?? null;
    if (
      suppliedNavigationIntentId !== null &&
      navigationIntentRef.current !== suppliedNavigationIntentId
    ) {
      return false;
    }
    const navigationIntentId =
      suppliedNavigationIntentId ?? navigationIntentRef.current + 1;
    if (suppliedNavigationIntentId === null) {
      navigationIntentRef.current = navigationIntentId;
      pendingNotificationOpenRef.current = null;
    }
    const suppliedContactRequestId =
      options.contactMessageOpenRequestId ?? null;
    if (
      suppliedContactRequestId !== null &&
      contactMessageOpenRequestRef.current !== suppliedContactRequestId
    ) {
      return false;
    }
    if (suppliedContactRequestId === null) {
      contactMessageOpenRequestRef.current += 1;
      preservedContactContextRef.current = null;
    } else if (options.preserveContactContext) {
      preservedContactContextRef.current = {
        ...options.preserveContactContext,
        requestId: suppliedContactRequestId,
      };
    }
    replyPreparationRequestRef.current += 1;
    if (!accountId || accountId === accountStatus.activeAccountId) return true;
    if (!(await prepareComposerForAccountSwitch())) {
      return false;
    }
    if (navigationIntentRef.current !== navigationIntentId) {
      return false;
    }
    invalidateForwardPreparationsForAccount(activeAccountIdRef.current);
    forwardPreparationRequestRef.current += 1;
    const previousStatus = accountStatus;
    const previousAccountId =
      accountStatus.activeAccountId || accountStatus.accountId || null;
    const previousView = rememberActiveAccountView(previousAccountId);
    const targetAccount = accountStatus.accounts?.find(
      (account) => account.accountId === accountId,
    );
    if (!targetAccount) {
      showToast("邮箱账户不存在，请刷新账户列表", "error");
      return false;
    }
    const optimisticStatus = {
      ...accountStatus,
      ...targetAccount,
      accountId: targetAccount.accountId,
      activeAccountId: targetAccount.accountId,
    };
    const requestId = accountSwitchRequestRef.current + 1;
    accountSwitchRequestRef.current = requestId;
    accountViewSnapshotLockRef.current = {
      accountId: previousAccountId,
      requestId,
    };
    setAccountSubmitStatus("saving");

    let targetView = accountViewsRef.current.get(accountId);
    let confirmedStatus = null;
    let targetCommitted = false;
    let transitionIntent = null;
    let finishTransition = null;
    const transitionDone = new Promise((resolve) => {
      finishTransition = resolve;
    });
    const requestIsCurrent = () =>
      accountSwitchRequestRef.current === requestId &&
      navigationIntentRef.current === navigationIntentId;
    const releaseSnapshotLock = () => {
      if (accountViewSnapshotLockRef.current?.requestId === requestId) {
        accountViewSnapshotLockRef.current = null;
      }
    };
    const abandonTransition = () => {
      releaseSnapshotLock();
      if (activeMailListIntentRef.current === transitionIntent) {
        activeMailListIntentRef.current = null;
      }
      if (queuedMailListIntentRef.current === transitionIntent) {
        queuedMailListIntentRef.current = null;
      }
      transitionIntent?.cancel?.();
    };
    try {
      const viewPromise = targetView
        ? Promise.resolve(targetView)
        : loadAccountView(accountId).catch(() => null);
      const statusPromise = mailApi.switchAccount(accountId).then(
        (status) => ({ ok: true, status }),
        (error) => ({ ok: false, error }),
      );
      targetView =
        (await viewPromise) || accountViewsRef.current.get(accountId) || null;
      if (!requestIsCurrent()) {
        abandonTransition();
        return false;
      }

      if (options.preserveContactContext) {
        targetView = {
          ...(targetView || {}),
          selectedMessageId: null,
          selectedMessage: null,
          navigationState: {
            folder: "contacts",
            listExpanded: true,
            readerOpen: false,
          },
        };
      }

      const targetNavigation = targetView?.navigationState || null;
      const targetListExpanded =
        !isWideMailWorkspaceRef.current ||
        targetNavigation?.listExpanded === true;
      const targetReaderOpen = Boolean(
        targetListExpanded &&
        targetNavigation?.readerOpen &&
        targetView?.selectedMessageId != null &&
        targetView?.selectedMessage,
      );
      const finish = () => {
        if (requestIsCurrent() && targetReaderOpen) {
          presentReader("entering");
        }
        finishTransition?.();
      };
      transitionIntent = {
        type: "account-switch",
        requestId,
        targetListExpanded,
        instant: options.forceAtomic === true || isSettingsOpen,
        commit: () => {
          if (!requestIsCurrent()) return;
          targetCommitted = true;
          releaseSnapshotLock();
          const targetStatus = confirmedStatus || optimisticStatus;
          accountStatusRef.current = targetStatus;
          setAccountStatus(targetStatus);
          restoreAccountView(accountId, targetView, {
            preserveWorkspaceMotion: true,
            deferReader: true,
            contactContext: options.preserveContactContext || null,
          });
        },
        after: finish,
        cancel: () => finishTransition?.(),
      };
      const startTransition = () => {
        if (!requestIsCurrent()) {
          finishTransition?.();
          return;
        }
        startMailListIntentRef.current(transitionIntent);
      };
      if (transitionIntent.instant) {
        startTransition();
      } else if (readerOpenRef.current) {
        requestReaderExit({
          speed: "fast",
          afterExit: startTransition,
          navigationIntentId,
        });
      } else if (
        activeFolderRef.current === "contacts" &&
        selectedContactEmailRef.current
      ) {
        requestContactDetailExit({
          speed: "fast",
          afterExit: startTransition,
        });
      } else {
        startTransition();
      }

      const statusResult = await statusPromise;
      if (!statusResult.ok) throw statusResult.error;
      confirmedStatus = statusResult.status;
      if (!requestIsCurrent()) {
        abandonTransition();
        return false;
      }
      if (targetCommitted) {
        accountStatusRef.current = confirmedStatus;
        setAccountStatus(confirmedStatus);
      }
      await transitionDone;
      if (!requestIsCurrent()) {
        abandonTransition();
        return false;
      }
      if (!targetCommitted) {
        accountStatusRef.current = confirmedStatus;
        setAccountStatus(confirmedStatus);
        restoreAccountView(accountId, targetView, {
          deferReader: true,
          contactContext: options.preserveContactContext || null,
        });
        if (targetReaderOpen) presentReader("entering");
      }
      setAccountSubmitStatus("saved");
      setAccountError(null);
      setAccountErrorProvider(null);
      void loadMailboxData({
        accountId,
        selectFirst: false,
      }).catch(() => {});
      return true;
    } catch (error) {
      if (accountSwitchRequestRef.current !== requestId) return;
      accountSwitchRequestRef.current += 1;
      abandonTransition();
      if (
        preservedContactContextRef.current?.requestId ===
        suppliedContactRequestId
      ) {
        preservedContactContextRef.current = null;
      }
      accountStatusRef.current = previousStatus;
      setAccountStatus(previousStatus);
      if (previousAccountId) {
        restoreAccountView(previousAccountId, previousView);
      }
      setAccountSubmitStatus("error");
      showToast(describeError(error, "邮箱账户切换失败"), "error");
      return false;
    }
  };

  const handleSidebarAccountSwitch = (accountId) => {
    const currentAccountId =
      accountStatus.activeAccountId || accountStatus.accountId || null;
    if (!accountId) return;
    if (accountId === currentAccountId) {
      if (isSettingsOpen) handleFolderChange("inbox");
      return;
    }
    if (isSettingsOpen) {
      setIsSettingsOpen(false);
      setSettingsFocusTarget(null);
    }
    void handleSwitchAccount(accountId, {
      forceAtomic: isSettingsOpen,
    });
  };

  const handleSaveAccountRemark = async (accountId, remark) => {
    const status = await mailApi.setAccountRemark(accountId, remark);
    setAccountStatus(status);
    return status;
  };

  const handleRemoveAccount = async (connectedAccount, options = {}) => {
    if (!connectedAccount?.accountId) return;
    if (composerRef.current) {
      showToast("请先关闭当前写信窗口，再移除邮箱账户。", "error");
      return;
    }
    if (composerSessionsRef.current.has(connectedAccount.accountId)) {
      showToast(
        "请先切换到该账户并关闭正在编辑的邮件，再移除邮箱账户。",
        "error",
      );
      return;
    }
    setAccountSubmitStatus("saving");
    try {
      const result = await mailApi.removeAccount(
        connectedAccount.accountId,
        options,
      );
      composerSessionsRef.current.delete(connectedAccount.accountId);
      await applyActiveAccount(result.status);
      if (result.localDataDeleted) {
        const ownerKey = normalizeAvatarEmail(connectedAccount.email);
        setProfileAvatars((current) =>
          current.filter(
            (avatar) =>
              avatar.ownerType !== "account" || avatar.ownerKey !== ownerKey,
          ),
        );
      }
      if (result.warning) {
        showToast(
          `账户已移除，但本地数据清理未完成：${describeError(
            result.warning,
            "请重启 Mine Mail 后重试清理",
          )}`,
          "error",
        );
      } else if (result.googleAuthorizationRevoked) {
        showToast(
          result.localDataDeleted
            ? "Google 授权、系统凭据和本地邮件缓存均已删除"
            : "Google 授权和系统凭据已删除；本地邮件缓存已保留",
        );
      } else if (options.deleteLocalData) {
        showToast("邮箱账户、系统凭据和本地邮件缓存均已删除");
      } else if (connectedAccount.provider === "gmail") {
        showToast("Gmail 已仅断开；Google 授权和本地邮件缓存仍然保留");
      } else {
        showToast("邮箱账户已移除；本地邮件缓存已保留");
      }
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

  const handleSaveSelectedAttachment = async (attachmentId) => {
    const messageId = localMessageId(selectedMessage);
    if (!messageId || !attachmentId) return;
    setAttachmentSaveStates((current) => ({
      ...current,
      [messageId]: {
        ...(current[messageId] || {}),
        [attachmentId]: { status: "saving" },
      },
    }));
    try {
      const result = await mailApi.saveMessageAttachment(
        messageId,
        attachmentId,
      );
      const status = result?.status;
      if (!["saved", "canceled", "error"].includes(status)) {
        throw new Error("附件保存没有返回可识别的结果");
      }
      setAttachmentSaveStates((current) => ({
        ...current,
        [messageId]: {
          ...(current[messageId] || {}),
          [attachmentId]: result,
        },
      }));
    } catch (error) {
      setAttachmentSaveStates((current) => ({
        ...current,
        [messageId]: {
          ...(current[messageId] || {}),
          [attachmentId]: {
            status: "error",
            message: describeError(error, "附件保存失败"),
            retryable: true,
          },
        },
      }));
    }
  };

  const handlePrepareForward = async () => {
    const messageId = localMessageId(selectedMessage);
    const sourceAccountId = activeAccountIdRef.current;
    if (!messageId || !sourceAccountId) return;
    const requestId = forwardPreparationRequestRef.current + 1;
    forwardPreparationRequestRef.current = requestId;
    setForwardPreparationStates((current) => ({
      ...current,
      [messageId]: {
        status: "loading",
        requestId,
        sourceAccountId,
      },
    }));
    try {
      const outcome = await mailApi.prepareForward(messageId, true);
      if (
        forwardPreparationRequestRef.current !== requestId ||
        activeAccountIdRef.current !== sourceAccountId
      ) {
        clearForwardPreparationRequest(messageId, requestId);
        return;
      }
      if (outcome?.kind === "error") {
        const preparationError = outcome.error || {};
        setForwardPreparationStates((current) => ({
          ...current,
          [messageId]: {
            status: "error",
            errorKind: preparationError.kind,
            retryable: true,
          },
        }));
        return;
      }
      const prepared = outcome?.kind === "prepared" ? outcome.prepared : null;
      const draft = prepared?.draft;
      if (
        !draft?.id ||
        !Number.isInteger(draft.local_version) ||
        draft.local_version < 1
      ) {
        throw new Error("转发准备没有返回可用的稳定草稿");
      }
      cacheAuthoritativeDrafts(draft);
      setForwardPreparationStates((current) => ({
        ...current,
        [messageId]: { status: "idle" },
      }));
      if (selectedMessageIdRef.current !== messageId) return;
      if (composerRef.current) {
        showToast("转发草稿已保存，请先处理当前写信窗口。", "error");
        return;
      }
      openComposer(draftToRequest(draft), draft.id, draft, {
        forwardWarnings: prepared.warnings || [],
      });
    } catch {
      if (
        forwardPreparationRequestRef.current !== requestId ||
        activeAccountIdRef.current !== sourceAccountId
      ) {
        clearForwardPreparationRequest(messageId, requestId);
        return;
      }
      setForwardPreparationStates((current) => ({
        ...current,
        [messageId]: {
          status: "error",
          retryable: true,
        },
      }));
    }
  };

  const openReply = async () => {
    const messageId = localMessageId(selectedMessage);
    const sourceAccountId = activeAccountIdRef.current;
    if (!messageId || !sourceAccountId) {
      showToast("这封邮件缺少可用的本地标识，无法准备回复。", "error");
      return;
    }
    if (isMessageLoading || !selectedMessage.body_fetched) {
      showToast("请等待邮件正文加载完成后再回复", "error");
      return;
    }
    const requestId = replyPreparationRequestRef.current + 1;
    replyPreparationRequestRef.current = requestId;
    try {
      const request = await mailApi.prepareReply(messageId);
      if (
        replyPreparationRequestRef.current !== requestId ||
        activeAccountIdRef.current !== sourceAccountId ||
        selectedMessageIdRef.current !== messageId
      ) {
        return;
      }
      if (composerRef.current) {
        showToast("请先处理当前写信窗口，再准备回复。", "error");
        return;
      }
      openComposer(request);
    } catch (error) {
      if (
        replyPreparationRequestRef.current === requestId &&
        activeAccountIdRef.current === sourceAccountId &&
        selectedMessageIdRef.current === messageId
      ) {
        showToast(describeError(error, "无法准备回复邮件"), "error");
      }
    }
  };

  const navigateRelative = (offset) => {
    const next = visibleMessages[selectedIndex + offset];
    if (next) void handleSelect(next);
  };

  const hasNoConnectedAccount = accountStatus.configured === false;
  const accountBackendUnavailable =
    accountStatus.configured === true && !accountStatus.backendReady;
  const needsAccountWorkspace =
    hasNoConnectedAccount || accountBackendUnavailable;

  const needsAccountRepairBanner =
    accountNeedsRepair && isAccountRepairVisible;

  const openAccountSetup = () => {
    invalidatePreparedFolderMotion();
    setSettingsSaveStatus("idle");
    setAccountError(null);
    setAccountErrorProvider(null);
    setSettingsFocusTarget(`account-form:${Date.now()}`);
    setIsSettingsOpen(true);
  };

  const handleAccountProviderChange = () => {
    setAccountError(null);
    setAccountErrorProvider(null);
  };

  const openAccountRepair = () => {
    invalidatePreparedFolderMotion();
    setSettingsSaveStatus("idle");
    setSettingsFocusTarget(`account-repair:${Date.now()}`);
    setIsSettingsOpen(true);
  };

  const openSidebarDrawer = () => {
    drawerTriggerRef.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    setIsSidebarOpen(true);
  };

  if (isUnsupportedRuntime) {
    return (
      <main className="unsupported-runtime" role="main">
        <div>
          <p className="eyebrow">MINE MAIL DESKTOP</p>
          <h1>请从桌面应用启动 Mine Mail</h1>
          <p>
            当前浏览器构建未启用 WebUI 演示。开发界面时，请在
            <code>web</code> 目录运行 <code>npm run dev</code>。
          </p>
        </div>
      </main>
    );
  }

  const isContactMode = activeFolder === "contacts";
  const selectedLocalMessageId = localMessageId(selectedMessage);
  const selectedMailboxRole = messageRole(selectedMessage);
  const selectedSenderOwnerType = ["sent", "outbox"].includes(
    selectedMailboxRole,
  )
    ? "account"
    : "contact";
  const selectedSenderDisplayName =
    selectedSenderOwnerType === "account"
      ? accountSenderIdentity(accountStatus).name || null
      : contactRemarkForEmail(selectedMessage?.sender?.email);
  const selectedIsMailboxMessage =
    selectedLocalMessageId !== null &&
    paginatedMailboxRoles.includes(selectedMailboxRole) &&
    !selectedMessage?.contactHistory &&
    selectedMessage?.kind !== "outbox";
  const selectedCanUseMailboxContentCommands =
    selectedLocalMessageId !== null &&
    selectedMessage?.kind !== "draft" &&
    selectedMessage?.kind !== "outbox";
  const selectedAttachmentSaveStates = selectedLocalMessageId
    ? attachmentSaveStates[selectedLocalMessageId] || {}
    : {};
  const selectedForwardPreparationState = selectedLocalMessageId
    ? forwardPreparationStates[selectedLocalMessageId]
    : null;
  const selectedPendingMutation = selectedMessage?.pending_mutation || null;
  const actionStateFor = (action) => {
    if (!selectedIsMailboxMessage || !activeAccountId) return "idle";
    const stored =
      messageActionStates[
        messageActionKey(activeAccountId, selectedLocalMessageId, action)
      ];
    if (stored) return stored;
    if (selectedPendingMutation?.kind === action) {
      return mutationActionState(selectedPendingMutation);
    }
    return "idle";
  };
  const mailListIsRetracted = ["collapsing", "collapsed"].includes(
    mailListMotion,
  );
  const mailListIsInteractive = mailListMotion === "expanded";
  const mailListMotionFrameProps = useMemo(
    () => ({
      inert: !mailListIsInteractive ? true : undefined,
      "aria-hidden": mailListIsRetracted || undefined,
      "aria-busy":
        !["expanded", "collapsed"].includes(mailListMotion) || undefined,
      onAnimationEnd: (event) => {
        if (event.target === event.currentTarget) {
          completeMailListMotion();
        }
      },
    }),
    [
      completeMailListMotion,
      mailListIsInteractive,
      mailListIsRetracted,
      mailListMotion,
    ],
  );
  const retryContacts = useLiveCallback(() => {
    if (activeAccountId) void loadContacts({ accountId: activeAccountId });
  });
  const retryContactMessages = useLiveCallback(() => {
    if (selectedContactAccountId && selectedContactEmail) {
      void loadContactMessages(selectedContactEmail, {
        accountId: selectedContactAccountId,
      });
    }
  });
  const openContactsMobileNav = useLiveCallback(openSidebarDrawer);
  const selectContact = useLiveCallback(handleSelectContact);
  const backToContacts = useLiveCallback(handleBackToContacts);
  const toggleContactFavorite = useLiveCallback((contact) => {
    void handleToggleContactFavorite(contact);
  });
  const composeToContact = useLiveCallback(handleComposeToContact);
  const openContactMessage = useLiveCallback(handleOpenContactMessage);
  const saveContactRemark = useLiveCallback(handleSaveContactRemark);
  const setContactAvatar = useLiveCallback((contact, file) =>
    handleSaveProfileAvatar("contact", contact.email, file),
  );
  const removeContactAvatar = useLiveCallback((contact) =>
    handleDeleteProfileAvatar("contact", contact.email),
  );
  const visibleReaderMessage = mailListIsRetracted
    ? null
    : isReaderOpen
      ? selectedMessage
      : null;
  const readerRenderKey =
    messageNavigationKey(visibleReaderMessage) ||
    (!mailListIsRetracted &&
    isReaderOpen &&
    selectedLocalMessageId !== null
      ? `reader-message:${selectedLocalMessageId}`
      : "reader-empty");
  const messageReader = (
    <MessageView
      key={readerRenderKey}
      message={visibleReaderMessage}
      isLoading={isMessageLoading}
      error={messageError}
      onRetry={() =>
        selectedMessage && void handleSelect(selectedMessage, true)
      }
      onClose={() =>
        requestReaderExit({
          speed: "normal",
          restoreFocusKey: messageNavigationKey(selectedMessage),
        })
      }
      backLabel={isContactMode ? "返回联系人详情" : "返回邮件列表"}
      motion={readerMotion}
      exitSpeed={readerExitSpeed}
      onMotionEnd={completeReaderMotion}
      onReply={openReply}
      onPrepareForward={
        selectedCanUseMailboxContentCommands
          ? () => void handlePrepareForward()
          : null
      }
      forwardState={selectedForwardPreparationState}
      onSaveAttachment={
        selectedCanUseMailboxContentCommands
          ? (attachmentId) =>
              void handleSaveSelectedAttachment(attachmentId)
          : null
      }
      attachmentSaveStates={selectedAttachmentSaveStates}
      onArchive={
        selectedIsMailboxMessage &&
        selectedMailboxRole === "inbox"
          ? () => void handleArchiveMessage()
          : null
      }
      archiveState={actionStateFor("archive")}
      onMoveToInbox={
        selectedIsMailboxMessage &&
        ["archive", "trash"].includes(selectedMailboxRole)
          ? () => void handleMoveToInbox()
          : null
      }
      moveToInboxState={actionStateFor("move_to_inbox")}
      onMoveToTrash={
        selectedIsMailboxMessage &&
        ["inbox", "sent", "archive"].includes(selectedMailboxRole)
          ? () => void handleMoveToTrash()
          : null
      }
      moveToTrashState={actionStateFor("move_to_trash")}
      onPermanentDelete={
        selectedIsMailboxMessage && selectedMailboxRole === "trash"
          ? () => void handlePreparePermanentDelete()
          : null
      }
      permanentDeleteState={actionStateFor("permanent_delete")}
      onMarkUnread={
        selectedIsMailboxMessage ? () => void handleMarkUnread() : null
      }
      onRetryDelivery={() =>
        selectedMessage?.outbox &&
        void handleRetryOutbox(selectedMessage.outbox)
      }
      isRetryingDelivery={Boolean(retryingOutboxId)}
      canRetryDelivery={networkActionsAvailable}
      onResolveDeliveryUnknown={
        selectedMessage?.outbox?.status === "delivery_unknown"
          ? handlePrepareDeliveryUnknownDecision
          : null
      }
      isResolvingDeliveryUnknown={Boolean(
        deliveryUnknownDecision?.pending,
      )}
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
      showIdlePoetry={settings.idlePoetryEnabled !== false}
      translationQueue={readerTranslationQueueRef.current}
      onTranslateMessage={(parts, languageId) =>
        mailApi.translateMailContent(parts, languageId)
      }
      onLoadTranslationConfig={mailApi.getAiConfig}
      onAgentConfigurationRequired={() =>
        showToast("请先前往设置界面完成AGENT配置", "warning")
      }
      onOpenExternalLink={(url) => void handleOpenExternalLink(url)}
      resolveReferencedMessage={resolveReferencedMessage}
      onOpenReferencedMessage={handleOpenReferencedMessage}
      senderAvatar={profileAvatarFor(
        selectedSenderOwnerType,
        selectedMessage?.sender?.email,
      )}
      senderDisplayName={selectedSenderDisplayName}
      onSetSenderAvatar={(file) =>
        handleSaveProfileAvatar(
          selectedSenderOwnerType,
          selectedMessage?.sender?.email,
          file,
        )
      }
      onRemoveSenderAvatar={() =>
        handleDeleteProfileAvatar(
          selectedSenderOwnerType,
          selectedMessage?.sender?.email,
        )
      }
    />
  );

  return (
    <div
      className={`app-shell platform-${platform} ${isSidebarOpen ? "sidebar-is-open" : ""} ${isSettingsOpen ? "settings-is-open" : ""} ${(isReaderOpen && selectedMessage) || (isContactMode && selectedContact) ? "has-selection" : ""}`}
      data-runtime={isTauriRuntime ? "tauri" : "web"}
    >
      <div className="app-wallpaper" aria-hidden="true" />
      <WindowTitlebar platform={platform} isDesktop={isTauriRuntime} />

      {needsAccountRepairBanner ? (
        <div className="account-repair-banner" role="alert">
          <span className="account-repair-banner__icon" aria-hidden="true">
            <WarningCircle size={18} weight="fill" />
          </span>
          <span className="account-repair-banner__copy">
            <strong>账户暂时离线</strong>
            <span>
              {accountError
                ? describeError(accountError, "邮箱账户暂时不可用")
                : "已下载的邮件仍可阅读；重新连接账户后才能同步、下载其他正文或发送邮件。"}
            </span>
          </span>
          <button
            type="button"
            className="account-repair-banner__action"
            onClick={openAccountRepair}
          >
            修复账户
          </button>
        </div>
      ) : null}

      <div className="mail-layout">
        <Sidebar
          activeFolder={activeFolder}
          onFolderChange={handleFolderChange}
          isMailListExpanded={!mailListIsRetracted}
          isFolderSelectionVisible={!mailListIsRetracted}
          mailListControlsId="mail-list-panel"
          onCompose={() => {
            invalidatePreparedFolderMotion();
            return needsAccountWorkspace
              ? accountBackendUnavailable
                ? openAccountRepair()
                : openAccountSetup()
              : openOrRestoreComposer();
          }}
          counts={folderCounts}
          outboxActive={outbox.length > 0 || activeBackgroundSendCount > 0}
          sentHasNew={Boolean(
            activeAccountId && sentAttentionByAccount[activeAccountId]
          )}
          accountStatus={accountStatus}
          isSettingsOpen={isSettingsOpen}
          accountAvatarFor={(email) => profileAvatarFor("account", email)}
          onAccountSwitch={handleSidebarAccountSwitch}
          onAddAccount={openAccountSetup}
          onOpenSettings={() => {
            invalidatePreparedFolderMotion();
            setSettingsSaveStatus("idle");
            setSettingsFocusTarget(null);
            setIsSettingsOpen(true);
          }}
          onOpenAppearance={() => {
            invalidatePreparedFolderMotion();
            setSettingsSaveStatus("idle");
            setSettingsFocusTarget("appearance");
            setIsSettingsOpen(true);
          }}
          mailboxCapabilities={mailboxCapabilities}
          onMailboxCapabilityRetry={(role) =>
            void handleMailboxCapabilityRetry(role)
          }
          isDrawerOpen={isSidebarOpen}
          onDrawerClose={() => setIsSidebarOpen(false)}
          drawerTriggerRef={drawerTriggerRef}
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
          <SecondaryWorkspaceErrorBoundary
            label="设置"
            onClose={() => {
              setIsSettingsOpen(false);
              setSettingsFocusTarget(null);
            }}
          >
            <Suspense
              fallback={<SecondaryWorkspaceLoading label="正在打开设置…" />}
            >
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
                accountErrorProvider={accountErrorProvider}
                onConfigureAccount={handleConfigureAccount}
                onConnectGoogle={handleConnectGoogle}
                onAccountProviderChange={handleAccountProviderChange}
                onSwitchAccount={(accountId) =>
                  void handleSwitchAccount(accountId)
                }
                onSaveAccountRemark={handleSaveAccountRemark}
                onRemoveAccount={(connectedAccount, options) =>
                  void handleRemoveAccount(connectedAccount, options)
                }
                onOpenExternalLink={(url) => void handleOpenExternalLink(url)}
                accountAvatarFor={(email) => profileAvatarFor("account", email)}
                onSetAccountAvatar={(email, file) =>
                  handleSaveProfileAvatar("account", email, file)
                }
                onRemoveAccountAvatar={(email) =>
                  handleDeleteProfileAvatar("account", email)
                }
                focusTarget={settingsFocusTarget}
                appearance={appearance}
                onSelectAppearance={handleSelectAppearance}
                onUpdateAppearancePreferences={handleUpdateAppearancePreferences}
                onImportCustomTheme={handleImportCustomTheme}
                onUpdateCustomTheme={handleUpdateCustomTheme}
                onDeleteCustomTheme={handleDeleteCustomTheme}
                appUpdateController={appUpdate}
              />
            </Suspense>
          </SecondaryWorkspaceErrorBoundary>
        ) : needsAccountWorkspace ? (
          <AccountEmptyWorkspace
            needsRepair={accountBackendUnavailable}
            showIdlePoetry={settings.idlePoetryEnabled !== false}
            onConnect={
              accountBackendUnavailable ? openAccountRepair : openAccountSetup
            }
          />
        ) : (
          <div
            className="mail-workspace"
            data-list-motion={mailListMotion}
            data-list-visibility={
              mailListMotion === "collapsed" ? "retracted" : "visible"
            }
          >
            {isContactMode ? (
              <Suspense
                fallback={
                  <>
                    <div
                      {...mailListMotionFrameProps}
                      className="mail-list-motion-frame"
                    >
                      <SecondaryWorkspaceLoading label="正在打开通讯录…" />
                    </div>
                    {messageReader}
                  </>
                }
              >
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
                  readerContent={
                    selectedMessage || !selectedContact
                      ? messageReader
                      : null
                  }
                  detailMotion={contactDetailMotion}
                  detailExitSpeed={contactDetailExitSpeed}
                  onDetailMotionEnd={completeContactDetailMotion}
                  listMotionFrameProps={mailListMotionFrameProps}
                  onRetry={retryContacts}
                  onRetryMessages={retryContactMessages}
                  onOpenMobileNav={openContactsMobileNav}
                  onSearchChange={setContactQuery}
                  onFilterChange={setContactFilter}
                  onSelectContact={selectContact}
                  onBackToContacts={backToContacts}
                  onToggleFavorite={toggleContactFavorite}
                  onCompose={composeToContact}
                  onOpenMessage={openContactMessage}
                  onSaveRemark={saveContactRemark}
                  onSetAvatar={setContactAvatar}
                  onRemoveAvatar={removeContactAvatar}
                />
              </Suspense>
            ) : (
              <>
                <div
                  {...mailListMotionFrameProps}
                  className="mail-list-motion-frame"
                >
                  <MailList
                    folderRole={activeFolder}
                    folderLabel={folderLabels[activeFolder]}
                    messages={visibleMessages}
                    hasFolderMessages={baseFolderMessages.length > 0}
                    selectedMessageId={selectedMessageId}
                    selectedMessage={selectedMessage}
                    onSelect={handleSelect}
                    onToggleStar={(message) =>
                      void handleToggleStar(message)
                    }
                    query={query}
                    onQueryChange={setQuery}
                    filter={filter}
                    onFilterChange={setFilter}
                    onCollapse={
                      isWideMailWorkspace ? handleCollapseMailList : null
                    }
                    onSync={handleSync}
                    syncState={syncState}
                    syncFeedback={
                      manualSyncFeedback?.accountId === activeAccountId &&
                      manualSyncFeedback.folder === activeFolder
                        ? manualSyncFeedback
                        : null
                    }
                    loadState={activeMailboxLoadState}
                    canSync={networkActionsAvailable}
                    syncDisabledReason={
                      networkActionsAvailable
                        ? null
                        : "当前离线，无法同步"
                    }
                    onOpenMobileNav={openSidebarDrawer}
                    avatarForEmail={(email) =>
                      profileAvatarFor("contact", email)
                    }
                    displayNameForEmail={contactRemarkForEmail}
                    referenceJump={referenceJump}
                    mailboxCapability={activeMailboxCapability}
                    loadMoreState={loadMoreState}
                    scrollStateKey={mailListScrollStateKey}
                    getScrollTop={getMailListScrollTop}
                    onScrollTopChange={saveMailListScrollTop}
                    onLoadMore={
                      canLoadOlder &&
                      !["offline", "unavailable", "loading"].includes(
                        loadMoreState,
                      )
                        ? () => void handleLoadMore()
                        : null
                    }
                  />
                </div>
                {messageReader}
              </>
            )}
          </div>
        )}
      </div>

      {composer ? (
        <ComposePanel
          key={composer.sessionId}
          sessionId={composer.sessionId}
          accountId={activeAccountId}
          value={composer.value}
          draft={composer.persistedDraft}
          draftId={composer.draftId}
          saveStatus={composer.saveStatus}
          locked={composer.locked}
          readOnly={composer.readOnlyUnsupported}
          initiallyMinimized={composer.startMinimized}
          restoreRequest={composeRestoreRequest}
          optimizationCache={composer.optimizationCache}
          onMinimizedChange={handleComposeMinimizedChange}
          networkAvailable={networkActionsAvailable}
          onClose={() => void handleCloseComposer()}
          onDiscard={() => void handleDiscardComposer()}
          onChange={handleComposeChange}
          onBodyDirty={handleComposeBodyDirty}
          onBodyChange={handleComposeBodyChange}
          onBodySnapshotProviderChange={handleComposeBodySnapshotProviderChange}
          onSaveDraft={handleSaveDraftAndMinimize}
          onRequestSend={() => void handleRequestSend()}
          sendShortcut={platform === "mac" ? "⌘ ↵" : "Ctrl ↵"}
          contacts={composeContactsWithAvatars}
          remoteImageMode={settings.remoteImageMode}
          defaultAiAssistantOpen={settings.aiAssistantDefaultOpen}
          onOpenExternalLink={handleOpenExternalLink}
          attachmentOperations={composer.attachmentOperations}
          forwardWarnings={composer.forwardWarnings}
          onAddAttachments={() => void handleAddAttachments()}
          onRemoveAttachment={(attachmentId) =>
            void handleRemoveAttachment(attachmentId)
          }
        />
      ) : null}

      <PermanentDeleteDialog
        open={Boolean(permanentDelete)}
        subject={permanentDelete?.message?.subject || ""}
        isPending={Boolean(permanentDelete?.pending)}
        errorMessage={permanentDelete?.error || null}
        returnFocusRef={consequentialActionRef}
        onCancel={() => setPermanentDelete(null)}
        onConfirm={() => void handleConfirmPermanentDelete()}
      />

      <ArchiveFolderDialog
        open={Boolean(archiveFolderDialog)}
        candidates={archiveFolderDialog?.candidates || []}
        selectedId={archiveFolderDialog?.selectedId || ""}
        isPending={Boolean(archiveFolderDialog?.pending)}
        errorMessage={archiveFolderDialog?.error || null}
        onSelectedIdChange={(selectedId) =>
          setArchiveFolderDialog((current) =>
            current ? { ...current, selectedId, error: null } : current,
          )
        }
        onCancel={cancelArchiveFolderSelection}
        onConfirm={() => void confirmArchiveFolderSelection()}
      />

      <ConsequentialConfirmDialog
        open={Boolean(deliveryUnknownDecision)}
        title={
          deliveryUnknownDecision?.decision === "retry_once"
            ? "仍要重试这封邮件？"
            : "确认这封邮件已投递？"
        }
        description={
          deliveryUnknownDecision?.decision === "retry_once"
            ? "上一次 SMTP 投递结果无法确认。继续会再次投递同一封邮件，收件人可能收到重复邮件。"
            : "请仅在登录邮箱服务器并核对“已发送”后确认。Mine Mail 会把这条记录标记为已投递，不会再次发送。"
        }
        icon={<WarningCircle size={23} weight="duotone" />}
        tone={
          deliveryUnknownDecision?.decision === "retry_once"
            ? "danger"
            : "primary"
        }
        closeLabel="取消投递结果处理"
        confirmLabel={
          deliveryUnknownDecision?.decision === "retry_once"
            ? "承担风险并重试"
            : "确认已投递"
        }
        pendingLabel={
          deliveryUnknownDecision?.decision === "retry_once"
            ? "正在明确重试…"
            : "正在确认投递…"
        }
        isPending={Boolean(deliveryUnknownDecision?.pending)}
        confirmDisabled={Boolean(deliveryUnknownDecision?.stale)}
        errorMessage={deliveryUnknownDecision?.error || null}
        returnFocusRef={consequentialActionRef}
        onCancel={() =>
          setDeliveryUnknownDecision((current) =>
            current?.pending ? current : null,
          )
        }
        onConfirm={() => void handleConfirmDeliveryUnknownDecision()}
      >
        <div className="confirm-dialog__subject">
          <small>邮件主题</small>
          <strong>
            {deliveryUnknownDecision?.subject?.trim() || "（无主题）"}
          </strong>
        </div>
        <p className="consequential-confirm-dialog__note">
          {deliveryUnknownDecision?.decision === "retry_once"
            ? "只有你明确承担重复投递风险后，Mine Mail 才会执行这一次重试。"
            : "如果服务器中找不到这封邮件，请取消并选择“仍要重试”。"}
        </p>
      </ConsequentialConfirmDialog>

      {appUpdate.isDownloadActive &&
      (!isSettingsOpen || !appUpdate.isDialogOpen) ? (
        <UpdateProgressNotice
          version={appUpdate.availableUpdate?.version}
          progress={appUpdate.progress}
          isCancelling={appUpdate.status === "cancelling"}
          canCancel={appUpdate.isDownloadCancellable}
          composeMinimized={Boolean(composer?.minimized)}
          onCancel={() => void appUpdate.cancelDownload()}
        />
      ) : null}

      <Toast toast={toast} onClose={dismissToast} />
    </div>
  );
}
