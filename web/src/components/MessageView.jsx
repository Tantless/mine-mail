import { useEffect, useId, useRef, useState } from "react";
import {
  Archive,
  ArrowBendUpLeft,
  ArrowBendUpRight,
  ArrowLeft,
  CaretLeft,
  CaretRight,
  CheckCircle,
  DownloadSimple,
  EnvelopeOpen,
  File,
  FileArchive,
  FileAudio,
  FileCode,
  FileImage,
  FilePdf,
  FileText,
  FileVideo,
  Paperclip,
  SpinnerGap,
  TrayArrowDown,
  Trash,
  Translate,
  WarningCircle,
} from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { HtmlMessageBody } from "./HtmlMessageBody.jsx";
import { NativeHtmlMessageBody } from "./NativeHtmlMessageBody.jsx";
import { ReaderIdleExperience } from "./ReaderIdleExperience.jsx";
import { SegmentedMessageBody } from "./SegmentedMessageBody.jsx";
import { EditableProfileAvatar, ProfileAvatar } from "./ProfileAvatar.jsx";
import { TooltipTarget } from "./Tooltip.jsx";
import { formatFullDate } from "../utils/formatters.js";
import { messageNavigationKey } from "../utils/messageNavigation.js";
import { userFacingErrorMessage } from "../utils/userFacingError.js";

const REMOTE_MAILBOX_ROLES = new Set(["inbox", "sent", "archive", "trash"]);
const MOVE_TO_INBOX_ROLES = new Set(["archive", "trash"]);
const MOVE_TO_TRASH_ROLES = new Set(["inbox", "sent", "archive"]);
const MAX_OUTBOX_ATTEMPTS = 0xffffffff;

function translationPartsForMessage(message, bodyRenderMode) {
  if (Array.isArray(message.body_segments) && message.body_segments.length) {
    return message.body_segments.map((segment, index) => ({
      id: `segment-${index}`,
      format: segment.render_mode === "plain" ? "plain" : "html",
      content: String(segment.content || ""),
    }));
  }
  if (
    (bodyRenderMode === "native_html" || bodyRenderMode === "isolated_html")
    && typeof message.body_html === "string"
  ) {
    return [{ id: "body-html", format: "html", content: message.body_html }];
  }
  return [
    {
      id: "body-text",
      format: "plain",
      content: String(message.body_text || ""),
    },
  ];
}

function applyTranslatedParts(message, translatedParts) {
  const translatedById = new Map(
    translatedParts.map((part) => [part.id, part.content]),
  );
  if (Array.isArray(message.body_segments) && message.body_segments.length) {
    return {
      ...message,
      body_segments: message.body_segments.map((segment, index) => ({
        ...segment,
        content: translatedById.get(`segment-${index}`) ?? segment.content,
      })),
    };
  }
  if (translatedById.has("body-html")) {
    return { ...message, body_html: translatedById.get("body-html") };
  }
  if (translatedById.has("body-text")) {
    return { ...message, body_text: translatedById.get("body-text") };
  }
  return message;
}

const ROLE_LABELS = {
  archive: "ARCHIVE",
  draft: "DRAFT",
  inbox: "INBOX",
  outbox: "OUTBOX",
  sent: "SENT",
  trash: "TRASH",
};

function normalizedMailboxRole(message) {
  const rawRole =
    message?.displayed_role ||
    message?.mailbox_role ||
    message?.role ||
    message?.kind ||
    "inbox";
  const role = String(rawRole).trim().toLowerCase();

  if (role === "drafts") return "draft";
  if (role === "junk" || role === "deleted") return "trash";
  return ROLE_LABELS[role] ? role : "inbox";
}

function normalizeActionState(value) {
  if (typeof value === "string") return { status: value };
  return value && typeof value === "object" ? value : { status: "idle" };
}

function actionStateMessage(state, fallback) {
  const detail =
    state.disabledReason ||
    state.message ||
    state.error_message ||
    state.error ||
    null;
  return detail ? userFacingErrorMessage(detail, fallback) : "";
}

function actionPresentation(actionLabel, stateValue) {
  const state = normalizeActionState(stateValue);
  const status = state.status || "idle";
  const detail = actionStateMessage(state, `${actionLabel}没有完成`);

  if (state.available === false || status === "unavailable") {
    return {
      disabled: true,
      feedback: `${actionLabel}不可用${detail ? `：${detail}` : ""}`,
      hidden: true,
      label: `${actionLabel}不可用${detail ? `：${detail}` : ""}`,
      status,
    };
  }

  if (status === "pending" || status === "in_flight") {
    return {
      disabled: true,
      feedback: `${actionLabel}正在处理${detail ? `：${detail}` : ""}`,
      hidden: false,
      label: `正在${actionLabel}…`,
      status,
    };
  }

  if (status === "needs_attention") {
    return {
      disabled: true,
      feedback: `${actionLabel}需要处理${detail ? `：${detail}` : "，请先重新同步"}`,
      hidden: false,
      label: `${actionLabel}需要处理${detail ? `：${detail}` : "，请先重新同步"}`,
      status,
    };
  }

  if (status === "outcome_unknown") {
    return {
      disabled: true,
      feedback: `${actionLabel}结果待确认${detail ? `：${detail}` : "，不会自动重试"}`,
      hidden: false,
      label: `${actionLabel}结果待确认${detail ? `：${detail}` : "，不会自动重试"}`,
      status,
    };
  }

  if (status === "error") {
    const retryable = Boolean(state.retryable);
    return {
      disabled: !retryable,
      feedback: `${actionLabel}失败${detail ? `：${detail}` : ""}${
        retryable ? "，可重试" : ""
      }`,
      hidden: false,
      label: retryable
        ? `重试${actionLabel}${detail ? `：${detail}` : ""}`
        : `${actionLabel}失败${detail ? `：${detail}` : ""}`,
      status,
    };
  }

  if (state.disabledReason) {
    return {
      disabled: true,
      feedback: `${actionLabel}不可用：${state.disabledReason}`,
      hidden: false,
      label: `${actionLabel}不可用：${state.disabledReason}`,
      status,
    };
  }

  return {
    disabled: false,
    feedback: status === "confirmed" ? `${actionLabel}已完成` : "",
    hidden: Boolean(state.hidden),
    label: actionLabel,
    status,
  };
}

function formatAttachmentSize(bytes) {
  const size = Number(bytes);
  if (!Number.isFinite(size) || size < 0) return "大小未知";

  const exact = `${Math.round(size).toLocaleString("zh-CN")} 字节`;
  if (size < 1024) return exact;

  const divisor = size < 1024 * 1024 ? 1024 : 1024 * 1024;
  const unit = divisor === 1024 ? "KB" : "MB";
  const value = size / divisor;
  const readable = Number.isInteger(value) ? String(value) : value.toFixed(1);
  return `${readable} ${unit} · ${exact}`;
}

function AttachmentTypeIcon({ mimeType }) {
  const normalized = String(mimeType || "").toLowerCase();
  const sharedProps = { "aria-hidden": true, size: 25, weight: "duotone" };

  if (normalized === "application/pdf") return <FilePdf {...sharedProps} />;
  if (normalized.startsWith("image/")) return <FileImage {...sharedProps} />;
  if (normalized.startsWith("audio/")) return <FileAudio {...sharedProps} />;
  if (normalized.startsWith("video/")) return <FileVideo {...sharedProps} />;
  if (
    normalized.startsWith("text/") ||
    normalized.includes("json") ||
    normalized.includes("javascript") ||
    normalized.includes("xml")
  ) {
    return normalized === "text/plain" ? (
      <FileText {...sharedProps} />
    ) : (
      <FileCode {...sharedProps} />
    );
  }
  if (
    normalized.includes("zip") ||
    normalized.includes("compressed") ||
    normalized.includes("archive") ||
    normalized.includes("tar")
  ) {
    return <FileArchive {...sharedProps} />;
  }
  return <File {...sharedProps} />;
}

function normalizeAttachmentSaveState(value) {
  if (typeof value === "string") return { status: value };
  return value && typeof value === "object" ? value : { status: "idle" };
}

function attachmentErrorLabel(kind) {
  return (
    {
      attachment_not_found: "附件已不存在",
      disk_full: "磁盘空间不足",
      message_unavailable: "完整邮件暂时不可用",
      permission_denied: "没有保存到所选位置的权限",
      write_failed: "写入文件失败",
    }[kind] || ""
  );
}

function attachmentStatusPresentation(attachment, stateValue, canSave) {
  const state = normalizeAttachmentSaveState(stateValue);
  const status = state.status || "idle";
  const name = attachment.safe_display_name || "attachment.bin";
  const rawError =
    state.message ||
    state.error_message ||
    (typeof state.error === "string" ? state.error : state.error?.message) ||
    attachmentErrorLabel(state.error_kind || state.errorKind) ||
    "";
  const error = rawError
    ? userFacingErrorMessage(rawError, "附件保存没有完成")
    : "";

  if (status === "saving") {
    return {
      actionable: false,
      busy: true,
      label: `正在保存附件 ${name}`,
      statusText: "正在保存…",
      status,
    };
  }
  if (status === "saved") {
    const savedName = state.file_name || state.fileName;
    return {
      actionable: false,
      busy: false,
      label: `附件 ${name} 已保存${savedName ? `为 ${savedName}` : ""}`,
      statusText: savedName ? `已保存为 ${savedName}` : "已保存",
      status,
    };
  }
  if (status === "canceled") {
    return {
      actionable: canSave,
      busy: false,
      label: canSave ? `重新保存附件 ${name}` : `附件 ${name} 的保存已取消`,
      statusText: canSave ? "已取消，可重新保存" : "保存已取消",
      status,
    };
  }
  if (status === "error") {
    const retryable = Boolean(state.retryable);
    return {
      actionable: canSave && retryable,
      busy: false,
      label:
        canSave && retryable
          ? `重试保存附件 ${name}${error ? `：${error}` : ""}`
          : `附件 ${name} 保存失败${error ? `：${error}` : ""}`,
      statusText: `保存失败${error ? `：${error}` : ""}${retryable ? "，可重试" : ""}`,
      status,
    };
  }

  return {
    actionable: canSave,
    busy: false,
    label: canSave ? `保存附件 ${name}` : `附件 ${name}，当前无法保存`,
    statusText: `${attachment.mime_type || "未知类型"} · ${
      attachment.size_is_estimate ? "约 " : ""
    }${formatAttachmentSize(attachment.size_bytes)}`,
    status: canSave ? status : "disabled",
  };
}

function AttachmentCard({ attachment, onSave, saveState }) {
  const safeName = attachment.safe_display_name || "attachment.bin";
  const presentation = attachmentStatusPresentation(
    attachment,
    saveState,
    typeof onSave === "function",
  );
  const statusIcon =
    presentation.status === "saving" ? (
      <SpinnerGap aria-hidden="true" size={18} />
    ) : presentation.status === "saved" ? (
      <CheckCircle aria-hidden="true" size={18} />
    ) : presentation.status === "error" ? (
      <WarningCircle aria-hidden="true" size={18} />
    ) : !presentation.actionable ? (
      <WarningCircle aria-hidden="true" size={18} />
    ) : (
      <DownloadSimple aria-hidden="true" size={18} />
    );
  const content = (
    <>
      <span className="attachment-card__icon">
        <AttachmentTypeIcon mimeType={attachment.mime_type} />
      </span>
      <span className="attachment-card__copy">
        <strong title={safeName}>{safeName}</strong>
        <small aria-live="polite">{presentation.statusText}</small>
      </span>
      {statusIcon}
    </>
  );

  if (!presentation.actionable) {
    return (
      <div
        className="attachment-card"
        data-state={presentation.status}
        role="group"
        aria-busy={presentation.busy || undefined}
        aria-label={presentation.label}
      >
        {content}
      </div>
    );
  }

  return (
    <TooltipTarget label={presentation.label}>
      <button
        className="attachment-card"
        data-state={presentation.status}
        type="button"
        aria-busy={presentation.busy || undefined}
        aria-label={presentation.label}
        onClick={() => onSave(attachment.id, attachment)}
      >
        {content}
      </button>
    </TooltipTarget>
  );
}

function normalizedRecipients(value) {
  const recipients = Array.isArray(value)
    ? value
    : value == null
      ? []
      : [value];
  return recipients
    .map((recipient) => {
      if (typeof recipient === "string") {
        return { email: recipient.trim(), name: "" };
      }
      return {
        email: String(recipient?.email || "").trim(),
        name: String(recipient?.name || "").trim(),
      };
    })
    .filter((recipient) => recipient.email || recipient.name);
}

function senderRecipients(message, senderDisplayName) {
  const recipients = normalizedRecipients(message.from ?? message.sender);
  if (!recipients.length || !senderDisplayName?.trim()) return recipients;

  return [
    {
      ...recipients[0],
      name: senderDisplayName.trim(),
    },
    ...recipients.slice(1),
  ];
}

function RecipientGroup({ label, recipients }) {
  if (!recipients.length) return null;

  return (
    <div>
      <dt>{label}</dt>
      <dd>
        <ul>
          {recipients.map((recipient, index) => (
            <li key={`${label}-${index}-${recipient.email}`}>
              {recipient.name ? <strong>{recipient.name}</strong> : null}
              <span>{recipient.email || "地址不可用"}</span>
            </li>
          ))}
        </ul>
      </dd>
    </div>
  );
}

function MessageToolbarAction({ icon, label, onAction, state, tone = "default" }) {
  const presentation = actionPresentation(label, state);
  if (!onAction || presentation.hidden) return null;

  return (
    <IconButton
      label={presentation.label}
      onClick={() => onAction()}
      disabled={presentation.disabled}
      tone={tone}
      aria-busy={
        presentation.status === "pending" || presentation.status === "in_flight"
          ? true
          : undefined
      }
    >
      {icon}
    </IconButton>
  );
}

function forwardPresentation(stateValue) {
  const state = normalizeActionState(stateValue);
  const status = state.status || "idle";

  if (status === "loading" || status === "pending" || status === "in_flight") {
    return {
      disabled: true,
      label: "正在准备转发…",
      loading: true,
    };
  }

  if (status === "error") {
    const retryable = state.retryable !== false;
    return {
      disabled: !retryable,
      label: "重试准备转发",
      loading: false,
    };
  }

  return {
    disabled: false,
    label: "转发",
    loading: false,
  };
}

export function MessageView({
  message,
  isLoading,
  error,
  onRetry,
  onClose,
  backLabel = "返回邮件列表",
  motion = "open",
  exitSpeed = "normal",
  onMotionEnd = null,
  onReply,
  onPrepareForward,
  forwardState,
  onArchive,
  archiveState,
  onMoveToInbox,
  moveToInboxState,
  onMoveToTrash,
  moveToTrashState,
  onPermanentDelete,
  permanentDeleteState,
  onMarkUnread,
  onSaveAttachment,
  attachmentSaveStates = {},
  onRetryDelivery,
  isRetryingDelivery = false,
  canRetryDelivery = false,
  onResolveDeliveryUnknown,
  isResolvingDeliveryUnknown = false,
  onPrevious,
  onNext,
  canPrevious,
  canNext,
  remoteImageMode = "automatic",
  onOpenExternalLink,
  resolveReferencedMessage,
  onOpenReferencedMessage,
  senderAvatar,
  senderDisplayName = null,
  onSetSenderAvatar,
  onRemoveSenderAvatar,
  onTranslateMessage,
}) {
  const recipientDetailsId = useId();
  const recipientToggleRef = useRef(null);
  const recipientRegionRef = useRef(null);
  const translationRequestRef = useRef(0);
  const [recipientDetailsOpen, setRecipientDetailsOpen] = useState(false);
  const [translationState, setTranslationState] = useState({
    status: "idle",
    messageKey: null,
    translatedMessage: null,
    showTranslated: false,
    error: null,
  });
  const translationMessageKey = message
    ? messageNavigationKey(message) || String(message.id || "reader-message")
    : null;

  useEffect(() => {
    setRecipientDetailsOpen(false);
  }, [message?.id]);

  useEffect(() => {
    translationRequestRef.current += 1;
    setTranslationState({
      status: "idle",
      messageKey: translationMessageKey,
      translatedMessage: null,
      showTranslated: false,
      error: null,
    });
  }, [translationMessageKey]);

  useEffect(() => {
    if (recipientDetailsOpen) recipientRegionRef.current?.focus();
  }, [recipientDetailsOpen]);

  if (!message) {
    return (
      <section
        className="reader-panel reader-panel--empty"
        aria-label="邮件阅读区，当前未打开邮件"
        data-reader-motion="idle"
      >
        <ReaderIdleExperience />
      </section>
    );
  }

  const role = normalizedMailboxRole(message);
  const fromRecipients = senderRecipients(message, senderDisplayName);
  const primarySender = fromRecipients[0] || { email: "", name: "" };
  const sender = primarySender.name || primarySender.email || "未知发件人";
  const renderKey = messageNavigationKey(message) || "reader-message";
  const isOutgoing = role === "outbox" || role === "sent";
  const remoteMailbox = REMOTE_MAILBOX_ROLES.has(role);
  const bodyPayloadHydrated =
    !(
      message.body_html_available === true &&
      message.body_html_loaded !== true
    ) &&
    (message.body_html_loaded === true ||
      typeof message.body_text === "string" ||
      typeof message.body_html === "string" ||
      Boolean(message.body_segments?.length));
  const bodyIsPending = Boolean(isLoading) || (!error && !bodyPayloadHydrated);
  const bodyRenderMode =
    message.body_render_mode || (message.body_html ? "isolated_html" : "plain");
  const hasBodySegments = Boolean(message.body_segments?.length);
  const translationIsCurrent =
    translationState.messageKey === translationMessageKey;
  const translatedMessage =
    translationIsCurrent && translationState.translatedMessage
      ? translationState.translatedMessage
      : null;
  const showTranslated = Boolean(
    translatedMessage && translationState.showTranslated,
  );
  const displayedMessage = showTranslated ? translatedMessage : message;
  const displayedBody = bodyPayloadHydrated
    ? displayedMessage.body_text || "这封邮件没有纯文本正文。"
    : "";
  const canTranslate =
    !isOutgoing
    && role !== "draft"
    && typeof onTranslateMessage === "function";
  const startTranslation = async () => {
    if (!canTranslate || bodyIsPending || error) return;
    const requestId = translationRequestRef.current + 1;
    translationRequestRef.current = requestId;
    setTranslationState({
      status: "loading",
      messageKey: translationMessageKey,
      translatedMessage: null,
      showTranslated: false,
      error: null,
    });
    try {
      const requestParts = translationPartsForMessage(message, bodyRenderMode);
      const result = await onTranslateMessage(requestParts);
      if (translationRequestRef.current !== requestId) return;
      const resultParts = Array.isArray(result?.parts) ? result.parts : [];
      const returnedIds = new Set(resultParts.map((part) => part.id));
      if (
        resultParts.length !== requestParts.length
        || requestParts.some((part) => !returnedIds.has(part.id))
      ) {
        throw new Error("AI 翻译结果不完整，请重试。");
      }
      const translated = applyTranslatedParts(message, resultParts);
      setTranslationState({
        status: "completed",
        messageKey: translationMessageKey,
        translatedMessage: translated,
        showTranslated: true,
        error: null,
      });
    } catch (translationError) {
      if (translationRequestRef.current !== requestId) return;
      setTranslationState({
        status: "error",
        messageKey: translationMessageKey,
        translatedMessage: null,
        showTranslated: false,
        error: userFacingErrorMessage(
          translationError,
          "AI 翻译失败，请检查 Agent 配置后重试",
        ),
      });
    }
  };
  const outboxRecipientGroupsUnavailable =
    role === "outbox" &&
    (!message.recipient_groups ||
      typeof message.recipient_groups !== "object" ||
      Array.isArray(message.recipient_groups));
  const recipientGroups =
    role === "outbox" && !outboxRecipientGroupsUnavailable
      ? message.recipient_groups
      : null;
  const toRecipients = normalizedRecipients(
    role === "outbox" ? recipientGroups?.to : message.to,
  );
  const ccRecipients = normalizedRecipients(
    role === "outbox" ? recipientGroups?.cc : message.cc,
  );
  const bccRecipients = normalizedRecipients(
    role === "outbox" ? recipientGroups?.bcc : message.bcc,
  );
  const hasRecipientDetails = Boolean(
    fromRecipients.length ||
      toRecipients.length ||
      ccRecipients.length ||
      bccRecipients.length ||
      outboxRecipientGroupsUnavailable,
  );
  const authoritativeAttachments = Array.isArray(message.attachments)
    ? message.attachments
    : null;
  const displayAttachments = authoritativeAttachments?.filter(
    (attachment) =>
      String(attachment?.disposition || "attachment").toLowerCase() !== "inline",
  );
  const legacyAttachmentNames =
    authoritativeAttachments === null && Array.isArray(message.attachment_names)
      ? message.attachment_names
      : [];
  const prepareForward = onPrepareForward;
  const preparedForwardState = forwardPresentation(forwardState);
  const deliveryUnknownItemValid =
    typeof message.outbox?.id === "string" &&
    Boolean(message.outbox.id.trim()) &&
    Number.isSafeInteger(message.outbox?.attempts) &&
    message.outbox.attempts >= 0 &&
    message.outbox.attempts <= MAX_OUTBOX_ATTEMPTS;

  const actionFeedback = [];
  const feedbackCandidates = [
    ["归档", role === "inbox" ? archiveState : undefined],
    [
      "移到收件箱",
      MOVE_TO_INBOX_ROLES.has(role) ? moveToInboxState : undefined,
    ],
    [
      "移到垃圾箱",
      MOVE_TO_TRASH_ROLES.has(role) ? moveToTrashState : undefined,
    ],
    ["永久删除", role === "trash" ? permanentDeleteState : undefined],
  ];
  for (const [label, state] of feedbackCandidates) {
    if (state === undefined) continue;
    const feedback = actionPresentation(label, state).feedback;
    if (feedback) actionFeedback.push(feedback);
  }

  return (
    <section
      className="reader-panel reader-panel--message"
      aria-label="邮件阅读区"
      data-reader-motion={motion}
      data-reader-exit-speed={exitSpeed}
      inert={motion === "exiting" ? true : undefined}
      aria-hidden={motion === "exiting" || undefined}
      onAnimationEnd={(event) => {
        if (
          event.target === event.currentTarget &&
          typeof onMotionEnd === "function"
        ) {
          onMotionEnd();
        }
      }}
    >
      <header className="reader-toolbar">
        <div className="reader-toolbar__group">
          {onClose ? (
            <IconButton
              label={backLabel}
              className="reader-back"
              onClick={() => onClose()}
            >
              <ArrowLeft aria-hidden="true" size={20} />
            </IconButton>
          ) : null}
          {role === "inbox" ? (
            <MessageToolbarAction
              label="归档"
              onAction={onArchive}
              state={archiveState}
              icon={<Archive aria-hidden="true" size={19} />}
            />
          ) : null}
          {MOVE_TO_INBOX_ROLES.has(role) ? (
            <MessageToolbarAction
              label="移到收件箱"
              onAction={onMoveToInbox}
              state={moveToInboxState}
              icon={<TrayArrowDown aria-hidden="true" size={19} />}
            />
          ) : null}
          {MOVE_TO_TRASH_ROLES.has(role) ? (
            <MessageToolbarAction
              label="移到垃圾箱"
              onAction={onMoveToTrash}
              state={moveToTrashState}
              icon={<Trash aria-hidden="true" size={19} />}
            />
          ) : null}
          {role === "trash" ? (
            <MessageToolbarAction
              label="永久删除"
              onAction={onPermanentDelete}
              state={permanentDeleteState}
              tone="danger"
              icon={<Trash aria-hidden="true" size={19} />}
            />
          ) : null}
          {remoteMailbox ? (
            <MessageToolbarAction
              label="标记为未读"
              onAction={onMarkUnread}
              icon={<EnvelopeOpen aria-hidden="true" size={19} />}
            />
          ) : null}
        </div>
        <div className="reader-toolbar__group">
          {canTranslate ? (
            translationIsCurrent && translationState.status === "completed" ? (
              <div
                className="reader-translation-toggle"
                role="group"
                aria-label="邮件正文显示语言"
              >
                <button
                  type="button"
                  aria-pressed={!showTranslated}
                  onClick={() =>
                    setTranslationState((current) => ({
                      ...current,
                      showTranslated: false,
                    }))
                  }
                >
                  原文
                </button>
                <button
                  type="button"
                  aria-pressed={showTranslated}
                  onClick={() =>
                    setTranslationState((current) => ({
                      ...current,
                      showTranslated: true,
                    }))
                  }
                >
                  译文
                </button>
                <span className="sr-only" role="status">
                  AI 翻译已完成
                </span>
              </div>
            ) : (
              <button
                type="button"
                className="reader-translation-action"
                aria-busy={translationState.status === "loading"}
                disabled={bodyIsPending || Boolean(error) || translationState.status === "loading"}
                onClick={() => void startTranslation()}
              >
                {translationState.status === "loading" ? (
                  <SpinnerGap className="spin" size={16} aria-hidden="true" />
                ) : (
                  <Translate size={16} aria-hidden="true" />
                )}
                {translationState.status === "loading"
                  ? "翻译中"
                  : translationState.status === "error"
                    ? "重试翻译"
                    : "AI 翻译"}
              </button>
            )
          ) : null}
          <IconButton
            label="上一封"
            onClick={onPrevious}
            disabled={!onPrevious || !canPrevious}
          >
            <CaretLeft aria-hidden="true" size={18} />
          </IconButton>
          <IconButton label="下一封" onClick={onNext} disabled={!onNext || !canNext}>
            <CaretRight aria-hidden="true" size={18} />
          </IconButton>
        </div>
      </header>

      <div className="reader-scroll vertical-scroll-surface">
        {translationIsCurrent && translationState.error ? (
          <div className="reader-translation-error" role="alert">
            {translationState.error}
          </div>
        ) : null}
        <div className="message-header">
          <p className="eyebrow">
            {role === "outbox" && message.outbox?.status === "sent"
              ? "SENT"
              : ROLE_LABELS[role]}
          </p>
          <h2>{message.subject || "（无主题）"}</h2>

          <div className="sender-card">
            {role !== "outbox" && primarySender.email && onSetSenderAvatar ? (
              <EditableProfileAvatar
                className="sender-avatar-picker"
                avatarClassName="sender-card__avatar"
                email={primarySender.email}
                label={sender}
                customSrc={senderAvatar}
                onSelectFile={onSetSenderAvatar}
                onRemove={onRemoveSenderAvatar}
              />
            ) : (
              <ProfileAvatar
                className="sender-card__avatar"
                email={primarySender.email}
                label={sender}
                customSrc={senderAvatar}
              />
            )}
            <div className="sender-card__content">
              <div className="sender-card__identity">
                <strong>{sender}</strong>
                <span>{primarySender.email}</span>
              </div>
              <div className="sender-card__recipient-disclosure">
                {hasRecipientDetails ? (
                  <button
                    ref={recipientToggleRef}
                    type="button"
                    className="recipient-toggle"
                    aria-controls={recipientDetailsId}
                    aria-expanded={recipientDetailsOpen}
                    onClick={() => setRecipientDetailsOpen((current) => !current)}
                  >
                    {recipientDetailsOpen ? "收起收件人详情" : "查看收件人"}
                    <CaretRight
                      aria-hidden="true"
                      className="recipient-toggle__caret"
                      size={12}
                    />
                  </button>
                ) : (
                  <span>收件人信息不可用</span>
                )}
                {recipientDetailsOpen ? (
                  <section
                    ref={recipientRegionRef}
                    id={recipientDetailsId}
                    className="recipient-details"
                    role="region"
                    tabIndex={-1}
                    aria-label="收件人详情"
                    onKeyDown={(event) => {
                      if (event.key !== "Escape") return;
                      event.preventDefault();
                      setRecipientDetailsOpen(false);
                      recipientToggleRef.current?.focus();
                    }}
                  >
                    <dl>
                      <RecipientGroup label="发件人" recipients={fromRecipients} />
                      <RecipientGroup label="收件人" recipients={toRecipients} />
                      <RecipientGroup label="抄送" recipients={ccRecipients} />
                      <RecipientGroup label="密送" recipients={bccRecipients} />
                    </dl>
                    {outboxRecipientGroupsUnavailable ? (
                      <p className="recipient-groups-unavailable">
                        旧版邮件收件人分组不可用
                      </p>
                    ) : null}
                  </section>
                ) : null}
                {outboxRecipientGroupsUnavailable && !recipientDetailsOpen ? (
                  <span className="recipient-groups-unavailable">
                    旧版邮件收件人分组不可用
                  </span>
                ) : null}
              </div>
            </div>
            <time dateTime={message.sent_at}>{formatFullDate(message.sent_at)}</time>
          </div>
        </div>

        {actionFeedback.length ? (
          <div className="delivery-status" role="status" aria-live="polite">
            {actionFeedback.map((item) => (
              <span key={item}>{item}</span>
            ))}
          </div>
        ) : null}

        {role === "outbox" && message.outbox?.status !== "sent" ? (
          <aside className="message-error delivery-status" role="status">
            <strong>投递状态：{message.delivery_status_label}</strong>
            {message.outbox?.last_error ? (
              <span>
                说明：
                {userFacingErrorMessage(
                  message.outbox.last_error,
                  "邮件投递状态暂时无法确认",
                )}
              </span>
            ) : null}
            {message.outbox?.status === "delivery_unknown" ? (
              <>
                <span>请先到邮箱服务器确认投递结果，不要立即重复发送。</span>
                {!deliveryUnknownItemValid ? (
                  <span>发件队列记录不完整，请刷新后再处理。</span>
                ) : onResolveDeliveryUnknown ? (
                  <div className="delivery-status__actions">
                    <button
                      type="button"
                      className="secondary-button"
                      onClick={() =>
                        onResolveDeliveryUnknown("confirm_delivered")
                      }
                      disabled={isResolvingDeliveryUnknown}
                    >
                      确认已投递
                    </button>
                    <button
                      type="button"
                      className="danger-button"
                      onClick={() => onResolveDeliveryUnknown("retry_once")}
                      disabled={
                        isResolvingDeliveryUnknown || !canRetryDelivery
                      }
                    >
                      仍要重试
                    </button>
                    {!canRetryDelivery ? (
                      <small>重新连接账户后才能重试</small>
                    ) : null}
                  </div>
                ) : null}
              </>
            ) : null}
          </aside>
        ) : null}

        <article
          className={`message-body${
            hasBodySegments
              ? " message-body--segmented"
              : bodyRenderMode === "isolated_html" && message.body_html
                ? " message-body--html"
                : bodyRenderMode === "native_html" && message.body_html
                  ? " message-body--native-html"
                  : ""
          }`}
          aria-busy={bodyIsPending}
        >
          {bodyIsPending ? (
            <div className="body-skeleton" aria-label="正在加载正文">
              <span />
              <span />
              <span />
              <span />
            </div>
          ) : error ? (
            <div className="message-error" role="alert">
              <strong>正文加载失败</strong>
              <span>
                {userFacingErrorMessage(error, "邮件正文暂时无法显示")}
              </span>
              {onRetry ? (
                <button type="button" className="secondary-button" onClick={onRetry}>
                  重新加载
                </button>
              ) : null}
            </div>
          ) : hasBodySegments ? (
            <SegmentedMessageBody
              key={`${renderKey}:${showTranslated ? "translated" : "original"}`}
              message={displayedMessage}
              body={displayedBody}
              bodyRenderMode={bodyRenderMode}
              allowOriginalFormat={!showTranslated}
              remoteImageMode={remoteImageMode}
              onOpenExternalLink={onOpenExternalLink}
              resolveReferencedMessage={resolveReferencedMessage}
              onOpenReferencedMessage={onOpenReferencedMessage}
            />
          ) : bodyRenderMode === "native_html" && displayedMessage.body_html ? (
            <NativeHtmlMessageBody
              key={`${renderKey}:${showTranslated ? "translated" : "original"}`}
              html={displayedMessage.body_html}
              hasRemoteImages={displayedMessage.has_remote_images}
              remoteImageMode={remoteImageMode}
              onOpenLink={onOpenExternalLink}
            />
          ) : bodyRenderMode === "isolated_html" && displayedMessage.body_html ? (
            <HtmlMessageBody
              key={`${renderKey}:${showTranslated ? "translated" : "original"}`}
              cacheKey={`${renderKey}:${showTranslated ? "translated" : "original"}`}
              html={displayedMessage.body_html}
              hasRemoteImages={displayedMessage.has_remote_images}
              remoteImageMode={remoteImageMode}
              title={message.subject}
              onOpenLink={onOpenExternalLink}
            />
          ) : (
            displayedBody.split(/\n{2,}/).map((paragraph, index) => (
              <p key={`${index}-${paragraph.slice(0, 12)}`}>
                {paragraph.split("\n").map((line, lineIndex) => (
                  <span key={`${lineIndex}-${line.slice(0, 8)}`}>
                    {line}
                    {lineIndex < paragraph.split("\n").length - 1 ? <br /> : null}
                  </span>
                ))}
              </p>
            ))
          )}
        </article>

        {displayAttachments?.length ? (
          <section className="attachments" aria-label="附件">
            <h3>
              <Paperclip aria-hidden="true" size={17} />
              {displayAttachments.length} 个附件
            </h3>
            <div className="attachment-grid">
              {displayAttachments.map((attachment) => (
                <AttachmentCard
                  key={attachment.id}
                  attachment={attachment}
                  onSave={onSaveAttachment}
                  saveState={attachmentSaveStates?.[attachment.id]}
                />
              ))}
            </div>
          </section>
        ) : legacyAttachmentNames.length ? (
          <section className="attachments" aria-label="附件">
            <h3>
              <Paperclip aria-hidden="true" size={17} />
              {legacyAttachmentNames.length} 个附件
            </h3>
            <p role="status">
              附件详情尚未加载。重新同步或加载完整邮件后才能保存。
            </p>
            <div className="attachment-grid">
              {legacyAttachmentNames.map((name, index) => (
                <div
                  className="attachment-card"
                  role="group"
                  aria-label={`附件 ${String(name)}，详情尚未加载`}
                  key={`${index}-${String(name)}`}
                >
                  <span className="attachment-card__icon">
                    <File aria-hidden="true" size={25} weight="duotone" />
                  </span>
                  <span className="attachment-card__copy">
                    <strong>{String(name)}</strong>
                    <small>类型和大小未知</small>
                  </span>
                  <WarningCircle aria-hidden="true" size={18} />
                </div>
              ))}
            </div>
          </section>
        ) : null}

        {role !== "outbox" ? (
          onReply || prepareForward ? (
            <div className="message-actions message-actions--mail">
              {onReply ? (
                <button
                  type="button"
                  className="message-action-button message-action-button--reply"
                  onClick={() => onReply()}
                >
                  <ArrowBendUpLeft aria-hidden="true" size={18} />
                  回复
                </button>
              ) : (
                <span />
              )}
              {prepareForward ? (
                <IconButton
                  label={preparedForwardState.label}
                  className="message-forward-button"
                  onClick={() => prepareForward()}
                  disabled={preparedForwardState.disabled}
                  aria-busy={preparedForwardState.loading || undefined}
                >
                  <ArrowBendUpRight aria-hidden="true" size={18} />
                </IconButton>
              ) : null}
            </div>
          ) : null
        ) : message.outbox?.status === "retryable" ? (
          onRetryDelivery ? (
            <div className="message-actions">
              <button
                type="button"
                className="secondary-button"
                onClick={onRetryDelivery}
                disabled={!canRetryDelivery || isRetryingDelivery}
                aria-busy={isRetryingDelivery}
              >
                {isRetryingDelivery ? "正在重试…" : "重试发送"}
              </button>
              {!canRetryDelivery ? <small>重新连接账户后才能重试</small> : null}
            </div>
          ) : null
        ) : null}
      </div>
    </section>
  );
}
