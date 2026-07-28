import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  CaretDown,
  Check,
  DotsSix,
  File,
  FloppyDisk,
  Notebook,
  Paperclip,
  PaperPlaneTilt,
  Quotes,
  Trash,
  UserPlus,
  X,
} from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { HtmlMessageBody } from "./HtmlMessageBody.jsx";
import { NativeHtmlMessageBody } from "./NativeHtmlMessageBody.jsx";
import { RecipientInput } from "./RecipientInput.jsx";

const RichTextEditor = lazy(() =>
  import("./RichTextEditor.jsx").then(({ RichTextEditor }) => ({
    default: RichTextEditor,
  })),
);

const composeMargin = 22;
const composeTopBoundary = 52;
const composeMinWidth = 680;
const composeMinHeight = 520;
const composeMinimizedWidth = 340;
const composeMinimizedHeight = 44;
const composeMinimizedBottom = 18;
const composeGeometryStorageKey = "mine-mail-compose-geometry-v1";
const resizeDirections = ["n", "ne", "e", "se", "s", "sw", "w", "nw"];
const dialogFocusableSelector = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[contenteditable='true']",
  "[tabindex]:not([tabindex='-1'])",
].join(",");
const stationeryOptions = [
  { value: "none", label: "无", description: "纯净编辑区" },
  { value: "lined", label: "横线纸", description: "适合长信与随笔" },
  { value: "grid", label: "方格纸", description: "适合中文书写" },
];

function formatReplyAddress(address) {
  if (!address?.email) return "未知发件人";
  return address.name?.trim()
    ? `${address.name.trim()} <${address.email}>`
    : address.email;
}

function formatReplyTime(value) {
  if (!value) return "时间未知";
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? value
    : date.toLocaleString("zh-CN", { hour12: false });
}

function formatReplyRecipients(recipients) {
  const value = (recipients || []).map(formatReplyAddress).join(", ");
  return value || "未知收件人";
}

function formatAttachmentSize(value) {
  const size = Number(value);
  if (!Number.isSafeInteger(size) || size < 0) return "大小未知";
  return `${new Intl.NumberFormat("zh-CN").format(size)} 字节`;
}

function attachmentName(attachment) {
  return (
    attachment?.name ||
    attachment?.safe_display_name ||
    attachment?.original_name ||
    "attachment.bin"
  );
}

function operationStatus(operation) {
  if (typeof operation === "string") return operation;
  return operation?.status || operation?.kind || "idle";
}

function isOperationBusy(operation) {
  return ["adding", "removing", "saving", "in_flight"].includes(
    operationStatus(operation),
  );
}

function attachmentOperationCopy(operation, action) {
  const status = operationStatus(operation);
  const copies = {
    saved: action === "add" ? "附件已添加" : "附件已移除",
    canceled: action === "add" ? "已取消添加附件" : "已取消移除附件",
    stale: "草稿已更新，本次操作未生效。请在最新版本重试",
    conflict: "附件已保存到冲突副本，请检查后继续",
    conflict_copy: "附件已保存到冲突副本，请检查后继续",
    error:
      action === "add"
        ? "添加附件失败，请重试"
        : "移除附件失败，请重试",
    adding: "正在添加附件…",
    removing: "正在移除附件…",
    saving: action === "add" ? "正在添加附件…" : "正在移除附件…",
    in_flight: action === "add" ? "正在添加附件…" : "正在移除附件…",
  };
  return copies[status] || "";
}

function ForwardContextBody({
  context,
  remoteImageMode,
  onOpenExternalLink,
}) {
  if (context.quoted_render_mode === "native_html" && context.quoted_html) {
    return (
      <NativeHtmlMessageBody
        html={context.quoted_html}
        remoteImageMode={remoteImageMode}
        onOpenLink={onOpenExternalLink}
      />
    );
  }
  if (context.quoted_render_mode === "isolated_html" && context.quoted_html) {
    return (
      <HtmlMessageBody
        cacheKey={`compose-forward:${context.source_message_id}`}
        html={context.quoted_html}
        remoteImageMode={remoteImageMode}
        title={`${context.original_subject || "原邮件"}转发原文`}
        onOpenLink={onOpenExternalLink}
      />
    );
  }
  return (
    <pre className="compose-reply-context__plain">{context.quoted_text}</pre>
  );
}

function forwardModeCopy(context) {
  if (
    ["native_html", "isolated_html"].includes(context.quoted_render_mode) &&
    context.quoted_html
  ) {
    return "原文以经过安全处理的 HTML 只读呈现，不会进入编辑区";
  }
  return "原文以完整可信文本只读呈现，不会进入编辑区";
}

function viewportSize() {
  return { width: window.innerWidth, height: window.innerHeight };
}

function clamp(value, minimum, maximum) {
  return Math.min(Math.max(value, minimum), Math.max(minimum, maximum));
}

function geometryLimits() {
  const viewport = viewportSize();
  const availableWidth = Math.max(320, viewport.width - composeMargin * 2);
  const availableHeight = Math.max(260, viewport.height - composeTopBoundary - composeMargin);
  return {
    viewport,
    minWidth: Math.min(composeMinWidth, availableWidth),
    minHeight: Math.min(composeMinHeight, availableHeight),
    maxWidth: availableWidth,
    maxHeight: availableHeight,
  };
}

function constrainGeometry(geometry) {
  const limits = geometryLimits();
  const width = clamp(geometry.width, limits.minWidth, limits.maxWidth);
  const height = clamp(geometry.height, limits.minHeight, limits.maxHeight);
  return {
    x: clamp(geometry.x, composeMargin, limits.viewport.width - composeMargin - width),
    y: clamp(
      geometry.y,
      composeTopBoundary,
      limits.viewport.height - composeMargin - height,
    ),
    width,
    height,
  };
}

function defaultGeometry() {
  const viewport = viewportSize();
  const width = Math.min(980, viewport.width - 64);
  const height = Math.min(800, viewport.height - 120);
  return constrainGeometry({
    x: (viewport.width - width) / 2,
    y: (viewport.height - height) / 2,
    width,
    height,
  });
}

function loadInitialGeometry() {
  try {
    const saved = JSON.parse(window.localStorage.getItem(composeGeometryStorageKey));
    if (
      saved &&
      [saved.x, saved.y, saved.width, saved.height].every(Number.isFinite)
    ) {
      return constrainGeometry(saved);
    }
  } catch {
    // Ignore stale or malformed local UI preferences.
  }
  return defaultGeometry();
}

function persistGeometry(geometry) {
  try {
    window.localStorage.setItem(
      composeGeometryStorageKey,
      JSON.stringify({
        x: Math.round(geometry.x),
        y: Math.round(geometry.y),
        width: Math.round(geometry.width),
        height: Math.round(geometry.height),
      }),
    );
  } catch {
    // Geometry persistence is a non-critical UI preference.
  }
}

function minimizedGeometry() {
  const viewport = viewportSize();
  const width = Math.min(composeMinimizedWidth, viewport.width - composeMargin * 2);
  return {
    x: Math.max(composeMargin, (viewport.width - width) / 2),
    y: Math.max(
      composeTopBoundary,
      viewport.height - composeMinimizedBottom - composeMinimizedHeight,
    ),
    width,
    height: composeMinimizedHeight,
  };
}

function StationeryControl({ format, disabled, onChange }) {
  const rootRef = useRef(null);
  const triggerRef = useRef(null);
  const [open, setOpen] = useState(false);
  const stationery = format?.stationery || "none";
  const selected =
    stationeryOptions.find((option) => option.value === stationery) ||
    stationeryOptions[0];
  const sendStationery = stationery !== "none" && format?.send_stationery === true;

  useEffect(() => {
    if (!open) return undefined;
    const closeOnOutside = (event) => {
      if (!rootRef.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutside);
    return () => document.removeEventListener("pointerdown", closeOnOutside);
  }, [open]);

  const chooseStationery = (next) => {
    onChange({
      stationery: next,
      send_stationery: next === "none" ? false : format?.send_stationery === true,
    });
    setOpen(false);
    triggerRef.current?.focus();
  };

  return (
    <div
      ref={rootRef}
      className="compose-stationery-control"
      data-open={open}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || !open) return;
        event.preventDefault();
        event.stopPropagation();
        setOpen(false);
        triggerRef.current?.focus();
      }}
    >
      <button
        ref={triggerRef}
        className="compose-stationery-trigger"
        type="button"
        aria-label={`信纸主题：${selected.label}`}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
      >
        <Notebook size={17} />
        <span>信纸</span>
        <strong>{selected.label}</strong>
        <CaretDown
          className="compose-stationery-trigger__caret"
          size={11}
          weight="bold"
        />
      </button>

      {open ? (
        <div
          className="compose-stationery-menu"
          role="menu"
          aria-label="选择信纸主题"
        >
          <div className="compose-stationery-menu__heading">
            <strong>信纸主题</strong>
            <span>改变写信时的纸面</span>
          </div>
          {stationeryOptions.map((option) => (
            <button
              key={option.value}
              type="button"
              role="menuitemradio"
              aria-checked={option.value === stationery}
              className="compose-stationery-option"
              data-selected={option.value === stationery}
              onClick={() => chooseStationery(option.value)}
            >
              <span
                className="compose-stationery-option__preview"
                data-stationery={option.value}
                aria-hidden="true"
              />
              <span className="compose-stationery-option__copy">
                <strong>{option.label}</strong>
                <small>{option.description}</small>
              </span>
              <Check size={15} weight="bold" aria-hidden="true" />
            </button>
          ))}
        </div>
      ) : null}

      <div className="compose-stationery-mode" aria-label="信纸发送方式">
        <button
          type="button"
          data-selected={!sendStationery}
          disabled={disabled}
          onClick={() => onChange({ send_stationery: false })}
        >
          仅编辑
        </button>
        <button
          type="button"
          data-selected={sendStationery}
          disabled={disabled || stationery === "none"}
          onClick={() => onChange({ send_stationery: true })}
        >
          随信发送
        </button>
      </div>
    </div>
  );
}

export function ComposePanel({
  value,
  draft = null,
  draftId,
  saveStatus,
  isSending,
  locked = false,
  readOnly = false,
  networkAvailable = true,
  onClose,
  onDiscard,
  onChange,
  onSaveDraft,
  onRequestSend,
  sendShortcut,
  contacts = [],
  remoteImageMode = "automatic",
  onOpenExternalLink,
  attachmentOperations = {},
  forwardWarnings = [],
  onAddAttachments,
  onRemoveAttachment,
}) {
  const [showCopies, setShowCopies] = useState(
    Boolean(value.cc?.length || value.bcc?.length),
  );
  const [geometry, setGeometry] = useState(loadInitialGeometry);
  const [isMinimized, setIsMinimized] = useState(false);
  const [isReplyExpanded, setIsReplyExpanded] = useState(false);
  const [isForwardExpanded, setIsForwardExpanded] = useState(false);
  const interactionRef = useRef(null);
  const geometryRef = useRef(geometry);
  const minimizedGeometryRef = useRef(null);
  const dialogRef = useRef(null);
  const restoreFocusRef = useRef(
    typeof document !== "undefined" &&
      document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null,
  );

  useEffect(
    () => () => {
      const previousFocus = restoreFocusRef.current;
      if (previousFocus?.isConnected) previousFocus.focus();
    },
    [],
  );

  useEffect(() => {
    if (value.cc?.length || value.bcc?.length) setShowCopies(true);
  }, [value.bcc, value.cc]);

  const commitGeometry = useCallback((valueOrUpdater) => {
    setGeometry((current) => {
      const next =
        typeof valueOrUpdater === "function"
          ? valueOrUpdater(current)
          : valueOrUpdater;
      geometryRef.current = next;
      return next;
    });
  }, []);

  const endInteraction = useCallback(() => {
    if (interactionRef.current && !isMinimized) {
      persistGeometry(geometryRef.current);
    }
    interactionRef.current = null;
    document.body.style.removeProperty("user-select");
    document.body.style.removeProperty("cursor");
  }, [isMinimized]);

  useEffect(() => {
    const onPointerMove = (event) => {
      const interaction = interactionRef.current;
      if (!interaction) return;
      const dx = event.clientX - interaction.pointerX;
      const dy = event.clientY - interaction.pointerY;
      const origin = interaction.geometry;

      if (interaction.kind === "drag") {
        commitGeometry(
          constrainGeometry({
            ...origin,
            x: origin.x + dx,
            y: origin.y + dy,
          }),
        );
        return;
      }

      const limits = geometryLimits();
      const originRight = origin.x + origin.width;
      const originBottom = origin.y + origin.height;
      let left = origin.x;
      let top = origin.y;
      let right = originRight;
      let bottom = originBottom;
      const direction = interaction.direction;

      if (direction.includes("w")) {
        left = clamp(origin.x + dx, composeMargin, originRight - limits.minWidth);
      }
      if (direction.includes("e")) {
        right = clamp(
          originRight + dx,
          origin.x + limits.minWidth,
          limits.viewport.width - composeMargin,
        );
      }
      if (direction.includes("n")) {
        top = clamp(
          origin.y + dy,
          composeTopBoundary,
          originBottom - limits.minHeight,
        );
      }
      if (direction.includes("s")) {
        bottom = clamp(
          originBottom + dy,
          origin.y + limits.minHeight,
          limits.viewport.height - composeMargin,
        );
      }

      commitGeometry({ x: left, y: top, width: right - left, height: bottom - top });
    };
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", endInteraction);
    window.addEventListener("pointercancel", endInteraction);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", endInteraction);
      window.removeEventListener("pointercancel", endInteraction);
      endInteraction();
    };
  }, [commitGeometry, endInteraction]);

  useEffect(() => {
    const onWindowResize = () => {
      if (isMinimized) {
        commitGeometry(minimizedGeometry());
        return;
      }
      commitGeometry((current) => constrainGeometry(current));
    };
    window.addEventListener("resize", onWindowResize);
    return () => window.removeEventListener("resize", onWindowResize);
  }, [commitGeometry, isMinimized]);

  const beginDrag = (event) => {
    if (
      event.button !== 0 ||
      event.target.closest(
        "button, input, textarea, [contenteditable], [data-no-compose-drag]",
      )
    ) {
      return;
    }
    event.preventDefault();
    interactionRef.current = {
      kind: "drag",
      pointerX: event.clientX,
      pointerY: event.clientY,
      geometry,
    };
    document.body.style.userSelect = "none";
    document.body.style.cursor = "grabbing";
  };

  const beginResize = (direction, event) => {
    if (event.button !== 0 || isMinimized) return;
    event.preventDefault();
    event.stopPropagation();
    interactionRef.current = {
      kind: "resize",
      direction,
      pointerX: event.clientX,
      pointerY: event.clientY,
      geometry,
    };
    document.body.style.userSelect = "none";
    document.body.style.cursor = getComputedStyle(event.currentTarget).cursor;
  };

  const toggleMinimized = () => {
    endInteraction();
    if (isMinimized) {
      commitGeometry(
        constrainGeometry(minimizedGeometryRef.current || loadInitialGeometry()),
      );
      minimizedGeometryRef.current = null;
      setIsMinimized(false);
      return;
    }
    minimizedGeometryRef.current = geometryRef.current;
    commitGeometry(minimizedGeometry());
    setIsMinimized(true);
  };

  const canSend = useMemo(() => {
    const recipients = [...value.to, ...value.cc, ...value.bcc];
    return recipients.length > 0 && recipients.every(Boolean);
  }, [value.bcc, value.cc, value.to]);
  const isBusy = locked || isSending;
  const authoritativeDraft = draft || value;
  const attachments = Array.isArray(authoritativeDraft?.attachments)
    ? authoritativeDraft.attachments
    : [];
  const localVersion = authoritativeDraft?.local_version;
  const stableDraftId = authoritativeDraft?.id || draftId;
  const hasStableDraft =
    Boolean(stableDraftId) &&
    Number.isInteger(localVersion) &&
    localVersion >= 1;
  const addOperation = attachmentOperations.add;
  const removeOperations = attachmentOperations.remove || {};
  const attachmentMutationBusy =
    isOperationBusy(addOperation) ||
    Object.values(removeOperations).some(isOperationBusy);
  const controlsDisabled = isBusy || readOnly;
  const addAttachmentsDisabled = controlsDisabled || attachmentMutationBusy;
  const removeAttachmentsDisabled =
    addAttachmentsDisabled || !hasStableDraft;
  const replyContext = value.reply_context || null;
  const forwardContext = authoritativeDraft?.forward_context || null;
  const omittedForwardAttachments = (forwardWarnings || []).includes(
    "attachments_omitted_by_user",
  );
  const downgradedForwardHtml = (forwardWarnings || []).includes(
    "html_downgraded",
  );
  const omittedInlineResources = (forwardWarnings || []).includes(
    "inline_resources_not_forwarded",
  );

  useEffect(() => {
    const onKeyDown = (event) => {
      if (event.defaultPrevented) return;
      if (event.key === "Escape" && !isBusy) {
        event.preventDefault();
        onClose();
        return;
      }
      if (
        (event.metaKey || event.ctrlKey) &&
        event.key === "Enter" &&
        canSend &&
        networkAvailable &&
        !controlsDisabled &&
        !attachmentMutationBusy
      ) {
        event.preventDefault();
        onRequestSend();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    attachmentMutationBusy,
    canSend,
    controlsDisabled,
    isBusy,
    networkAvailable,
    onClose,
    onRequestSend,
  ]);

  const setRecipients = (field, recipients) => {
    onChange((current) => ({ ...current, [field]: recipients }));
  };

  const setFormat = (patch) => {
    onChange((current) => ({
      ...current,
      format: {
        body_html: null,
        stationery: "none",
        send_stationery: false,
        ...(current.format || {}),
        ...patch,
      },
    }));
  };

  const addAttachments = () => {
    if (addAttachmentsDisabled || !onAddAttachments) return;
    onAddAttachments(localVersion);
  };

  const removeAttachment = (attachmentId) => {
    if (
      removeAttachmentsDisabled ||
      !onRemoveAttachment ||
      !attachmentId
    ) {
      return;
    }
    onRemoveAttachment(attachmentId, localVersion);
  };

  const minimizedTitle = value.subject.trim() || "新邮件";
  const composeFormat = {
    body_html: null,
    stationery: "none",
    send_stationery: false,
    ...(value.format || {}),
  };
  const saveCopy = {
    idle: draftId ? "已保存" : "新草稿",
    dirty: "有未保存更改",
    saving: "正在保存…",
    syncing: "正在同步…",
    saved: "已保存",
    readonly: "只读",
    error: "保存失败",
  }[saveStatus] || "新草稿";
  const saveActionLabel =
    saveStatus === "saving" || saveStatus === "syncing"
      ? "正在保存草稿"
      : "保存并关闭";
  const dialogLabel = readOnly ? "查看草稿" : draftId ? "编辑草稿" : "新邮件";
  const addOperationCopy = attachmentOperationCopy(addOperation, "add");
  const trapDialogFocus = (event) => {
    if (event.key !== "Tab" || event.defaultPrevented || isMinimized) return;
    const dialog = dialogRef.current;
    if (!dialog) return;
    const focusable = Array.from(
      dialog.querySelectorAll(dialogFocusableSelector),
    ).filter(
      (element) =>
        element instanceof HTMLElement &&
        element.getAttribute("aria-hidden") !== "true" &&
        element.tabIndex >= 0,
    );
    const first = focusable[0];
    const last = focusable.at(-1);
    const active = document.activeElement;

    if (!first || !last) {
      event.preventDefault();
      dialog.focus();
      return;
    }
    if (event.shiftKey && (active === first || !dialog.contains(active))) {
      event.preventDefault();
      last.focus();
      return;
    }
    if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      className="compose-layer"
      role="presentation"
      data-minimized={isMinimized}
      onPointerDown={(event) => {
        if (!isMinimized && event.target === event.currentTarget) {
          toggleMinimized();
        }
      }}
    >
      <section
        ref={dialogRef}
        className="compose-panel"
        role="dialog"
        tabIndex={-1}
        aria-modal={isMinimized ? undefined : "true"}
        aria-label={isMinimized ? minimizedTitle : undefined}
        aria-labelledby={isMinimized ? undefined : "compose-title"}
        data-minimized={isMinimized}
        onKeyDown={trapDialogFocus}
        style={{
          left: geometry.x,
          top: geometry.y,
          right: "auto",
          bottom: "auto",
          width: geometry.width,
          height: geometry.height,
        }}
      >
        {isMinimized ? (
          <div className="compose-minimized-shell">
            <button
              className="compose-minimized-bar"
              type="button"
              aria-label={`还原写信窗口：${minimizedTitle}`}
              onClick={toggleMinimized}
            >
              <span>{minimizedTitle}</span>
            </button>
            <IconButton
              className="compose-minimized-close"
              label="关闭写信窗口"
              onClick={onClose}
              disabled={isBusy}
            >
              <X size={17} />
            </IconButton>
          </div>
        ) : (
          <>
            <h2 id="compose-title" className="compose-dialog-title">
              {dialogLabel}
            </h2>
            <div
              className="compose-drag-surface"
              aria-hidden="true"
              onPointerDown={beginDrag}
            />

            {readOnly ? (
              <div className="compose-unsupported-notice" role="status">
                含无法安全编辑的 HTML 或附件，已保持只读
              </div>
            ) : null}

            <div className="compose-fields">
              <div className="compose-field">
                <label htmlFor="compose-to">收件人</label>
                <RecipientInput
                  id="compose-to"
                  label="收件人"
                  autoFocus
                  disabled={controlsDisabled}
                  recipients={value.to}
                  contacts={contacts}
                  onChange={(recipients) => setRecipients("to", recipients)}
                />
                <IconButton
                  className="compose-copy-toggle"
                  label={showCopies ? "收起抄送和密送" : "展开抄送和密送"}
                  aria-expanded={showCopies}
                  aria-controls="compose-copy-fields"
                  data-expanded={showCopies}
                  onClick={() => setShowCopies((current) => !current)}
                  disabled={controlsDisabled}
                >
                  <UserPlus size={15} />
                  <CaretDown
                    className="compose-copy-toggle__caret"
                    size={11}
                    weight="bold"
                  />
                </IconButton>
              </div>
              {showCopies ? (
                <div id="compose-copy-fields" className="compose-copy-fields">
                  <div className="compose-field">
                    <label htmlFor="compose-cc">抄送</label>
                    <RecipientInput
                      id="compose-cc"
                      label="抄送"
                      disabled={controlsDisabled}
                      recipients={value.cc}
                      contacts={contacts}
                      onChange={(recipients) => setRecipients("cc", recipients)}
                    />
                  </div>
                  <div className="compose-field">
                    <label htmlFor="compose-bcc">密送</label>
                    <RecipientInput
                      id="compose-bcc"
                      label="密送"
                      disabled={controlsDisabled}
                      recipients={value.bcc}
                      contacts={contacts}
                      onChange={(recipients) => setRecipients("bcc", recipients)}
                    />
                  </div>
                </div>
              ) : null}
              <div className="compose-field compose-field--subject">
                <label htmlFor="compose-subject">主题</label>
                <div className="compose-input-shell inset-input-shell">
                  <input
                    id="compose-subject"
                    aria-label="主题"
                    autoComplete="off"
                    disabled={controlsDisabled}
                    value={value.subject}
                    onChange={(event) =>
                      onChange((current) => ({ ...current, subject: event.target.value }))
                    }
                    placeholder="写一个简洁的主题"
                  />
                </div>
              </div>
            </div>

            <Suspense
              fallback={
                <div
                  className="compose-editor-loading"
                  role="status"
                  aria-live="polite"
                >
                  正在载入编辑器…
                </div>
              }
            >
              <RichTextEditor
                bodyText={value.body_text}
                format={composeFormat}
                stationery={composeFormat.stationery}
                disabled={controlsDisabled}
                onChange={(next) =>
                  onChange((current) => ({
                    ...current,
                    body_text: next.body_text,
                    format: next.format,
                  }))
                }
              />
            </Suspense>

            {attachments.length > 0 ||
            addOperationCopy ||
            Object.values(removeOperations).some((operation) =>
              Boolean(attachmentOperationCopy(operation, "remove")),
            ) ||
            omittedForwardAttachments ||
            downgradedForwardHtml ||
            omittedInlineResources ? (
              <section
                className="compose-attachments"
                aria-labelledby="compose-attachments-title"
                data-add-state={operationStatus(addOperation)}
              >
                <div className="compose-attachments__heading">
                  <strong id="compose-attachments-title">附件</strong>
                  <span>{attachments.length} 个</span>
                </div>

                {attachments.length > 0 ? (
                  <ul className="compose-attachments__list">
                    {attachments.map((attachment, index) => {
                      const name = attachmentName(attachment);
                      const removeOperation = attachment.id
                        ? removeOperations[attachment.id]
                        : null;
                      const removeCopy = attachmentOperationCopy(
                        removeOperation,
                        "remove",
                      );
                      return (
                        <li
                          key={attachment.id || `${name}:${index}`}
                          className="compose-attachment"
                          data-state={operationStatus(removeOperation)}
                        >
                          <span
                            className="compose-attachment__icon"
                            aria-hidden="true"
                          >
                            <File size={18} />
                          </span>
                          <span className="compose-attachment__copy">
                            <strong title={name}>{name}</strong>
                            <small>
                              {attachment.mime_type || "application/octet-stream"}
                              {" · "}
                              {formatAttachmentSize(attachment.size_bytes)}
                              {attachment.source_attachment_id
                                ? " · 原邮件附件"
                                : ""}
                            </small>
                            {removeCopy ? (
                              <span
                                className="compose-attachment__status"
                                role="status"
                              >
                                {removeCopy}
                              </span>
                            ) : null}
                          </span>
                          {onRemoveAttachment && attachment.id ? (
                            <IconButton
                              className="compose-attachment__remove"
                              label={
                                isOperationBusy(removeOperation)
                                  ? `正在移除附件 ${name}`
                                  : readOnly
                                    ? `只读草稿不能移除附件 ${name}`
                                    : !hasStableDraft
                                      ? `无法移除附件 ${name}：请先保存草稿`
                                      : `移除附件 ${name}`
                              }
                              disabled={
                                removeAttachmentsDisabled ||
                                isOperationBusy(removeOperation)
                              }
                              onClick={() => removeAttachment(attachment.id)}
                            >
                              <X size={15} />
                            </IconButton>
                          ) : null}
                        </li>
                      );
                    })}
                  </ul>
                ) : null}

                {addOperationCopy ? (
                  <div
                    className="compose-attachment-operation"
                    role="status"
                    aria-live="polite"
                    data-state={operationStatus(addOperation)}
                  >
                    {addOperationCopy}
                  </div>
                ) : null}

                {Object.entries(removeOperations)
                  .filter(
                    ([attachmentId, operation]) =>
                      !attachments.some(
                        (attachment) => attachment.id === attachmentId,
                      ) && attachmentOperationCopy(operation, "remove"),
                  )
                  .map(([attachmentId, operation]) => (
                    <div
                      key={attachmentId}
                      className="compose-attachment-operation"
                      role="status"
                      aria-live="polite"
                      data-state={operationStatus(operation)}
                    >
                      {attachmentOperationCopy(operation, "remove")}
                    </div>
                  ))}

                {omittedForwardAttachments ? (
                  <div
                    className="compose-forward-attachment-warning"
                    role="status"
                  >
                    无附件转发：原邮件附件未加入当前草稿。
                  </div>
                ) : null}
                {downgradedForwardHtml ? (
                  <div
                    className="compose-forward-attachment-warning"
                    role="status"
                  >
                    原邮件的复杂 HTML 已转换为安全的完整文本后转发。
                  </div>
                ) : null}
                {omittedInlineResources ? (
                  <div
                    className="compose-forward-attachment-warning"
                    role="status"
                  >
                    原邮件的内嵌资源未加入转发；正文仍保留安全文本内容。
                  </div>
                ) : null}
              </section>
            ) : null}

            {replyContext ? (
              <aside className="compose-reply-context" data-expanded={isReplyExpanded}>
                <button
                  className="compose-reply-context__summary"
                  type="button"
                  aria-expanded={isReplyExpanded}
                  onClick={() => setIsReplyExpanded((current) => !current)}
                >
                  <span className="compose-reply-context__icon" aria-hidden="true">
                    <Quotes size={17} weight="fill" />
                  </span>
                  <span className="compose-reply-context__copy">
                    <strong>{replyContext.subject || "原邮件"}</strong>
                    <small>
                      {formatReplyAddress(replyContext.sender)}
                      {" → "}
                      {formatReplyRecipients(replyContext.recipients)}
                      {" · "}
                      {formatReplyTime(replyContext.sent_at)}
                    </small>
                  </span>
                  <CaretDown
                    className="compose-reply-context__caret"
                    size={15}
                    weight="bold"
                  />
                </button>
                {isReplyExpanded ? (
                  <div className="compose-reply-context__body vertical-scroll-surface">
                    {replyContext.quoted_render_mode === "native_html" &&
                    replyContext.quoted_html ? (
                      <NativeHtmlMessageBody
                        html={replyContext.quoted_html}
                        hasRemoteImages={replyContext.has_remote_images}
                        remoteImageMode={remoteImageMode}
                        onOpenLink={onOpenExternalLink}
                      />
                    ) : replyContext.quoted_render_mode === "isolated_html" &&
                      replyContext.quoted_html ? (
                      <HtmlMessageBody
                        cacheKey={`compose-reply:${replyContext.parent_message_id || replyContext.sent_at || replyContext.subject}`}
                        html={replyContext.quoted_html}
                        hasRemoteImages={replyContext.has_remote_images}
                        remoteImageMode={remoteImageMode}
                        title={`${replyContext.subject || "原邮件"}引用内容`}
                        onOpenLink={onOpenExternalLink}
                      />
                    ) : (
                      <pre className="compose-reply-context__plain">
                        {replyContext.quoted_text}
                      </pre>
                    )}
                  </div>
                ) : null}
              </aside>
            ) : null}

            {forwardContext ? (
              <aside
                className="compose-reply-context compose-forward-context"
                data-expanded={isForwardExpanded}
                data-immutable="true"
                aria-label="不可编辑的转发原文"
              >
                <button
                  className="compose-reply-context__summary"
                  type="button"
                  aria-expanded={isForwardExpanded}
                  onClick={() => setIsForwardExpanded((current) => !current)}
                >
                  <span className="compose-reply-context__icon" aria-hidden="true">
                    <Quotes size={17} weight="fill" />
                  </span>
                  <span className="compose-reply-context__copy">
                    <strong>{forwardContext.original_subject || "原邮件"}</strong>
                    <small>
                      {formatReplyAddress(forwardContext.from)}
                      {" → "}
                      {formatReplyRecipients(forwardContext.to)}
                      {" · "}
                      {formatReplyTime(forwardContext.sent_at)}
                    </small>
                  </span>
                  <CaretDown
                    className="compose-reply-context__caret"
                    size={15}
                    weight="bold"
                  />
                </button>
                {isForwardExpanded ? (
                  <div className="compose-reply-context__body vertical-scroll-surface">
                    <dl className="compose-forward-context__identity">
                      <div>
                        <dt>主题</dt>
                        <dd>{forwardContext.original_subject || "（无主题）"}</dd>
                      </div>
                      <div>
                        <dt>发件人</dt>
                        <dd>{formatReplyAddress(forwardContext.from)}</dd>
                      </div>
                      <div>
                        <dt>收件人</dt>
                        <dd>{formatReplyRecipients(forwardContext.to)}</dd>
                      </div>
                      {forwardContext.cc?.length ? (
                        <div>
                          <dt>抄送</dt>
                          <dd>{formatReplyRecipients(forwardContext.cc)}</dd>
                        </div>
                      ) : null}
                      <div>
                        <dt>时间</dt>
                        <dd>{formatReplyTime(forwardContext.sent_at)}</dd>
                      </div>
                    </dl>
                    <p className="compose-forward-context__mode" role="note">
                      {forwardModeCopy(forwardContext)}
                    </p>
                    <ForwardContextBody
                      context={forwardContext}
                      remoteImageMode={remoteImageMode}
                      onOpenExternalLink={onOpenExternalLink}
                    />
                    {forwardContext.source_attachments?.length ? (
                      <section
                        className="compose-forward-context__attachments"
                        aria-label="原邮件附件清单"
                      >
                        <strong>
                          {omittedForwardAttachments
                            ? "原邮件附件（未随信发送）"
                            : "原邮件附件"}
                        </strong>
                        <ul>
                          {forwardContext.source_attachments.map(
                            (attachment, index) => {
                              const name = attachmentName(attachment);
                              return (
                                <li key={attachment.id || `${name}:${index}`}>
                                  <span title={name}>{name}</span>
                                  <small>
                                    {attachment.mime_type ||
                                      "application/octet-stream"}
                                    {" · "}
                                    {formatAttachmentSize(attachment.size_bytes)}
                                  </small>
                                </li>
                              );
                            },
                          )}
                        </ul>
                      </section>
                    ) : null}
                  </div>
                ) : null}
              </aside>
            ) : null}

            <footer className="compose-footer">
              <div className="compose-footer__left">
                <button
                  className="send-button"
                  type="button"
                  aria-label="发送邮件"
                  disabled={
                    !canSend ||
                    !networkAvailable ||
                    controlsDisabled ||
                    attachmentMutationBusy
                  }
                  onClick={onRequestSend}
                >
                  <PaperPlaneTilt size={18} weight="fill" />
                  {isSending ? "正在发送…" : locked ? "正在准备…" : readOnly ? "只读" : "发送"}
                  <kbd>{sendShortcut}</kbd>
                </button>
                {onAddAttachments ? (
                  <IconButton
                    label={
                      isOperationBusy(addOperation)
                        ? "正在添加附件"
                        : readOnly
                          ? "只读草稿不能添加附件"
                          : "添加附件"
                    }
                    disabled={addAttachmentsDisabled}
                    onClick={addAttachments}
                  >
                    <Paperclip size={19} />
                  </IconButton>
                ) : null}
              </div>
              <div className="compose-footer__right">
                <StationeryControl
                  format={composeFormat}
                  disabled={controlsDisabled}
                  onChange={setFormat}
                />
                <span
                  className="compose-save-state"
                  data-state={saveStatus}
                  aria-live="polite"
                >
                  {saveCopy}
                </span>
                <IconButton
                  label={saveActionLabel}
                  onClick={onSaveDraft}
                  disabled={
                    controlsDisabled ||
                    saveStatus === "saving" ||
                    saveStatus === "syncing"
                  }
                >
                  <FloppyDisk size={18} />
                </IconButton>
                <IconButton
                  label="丢弃草稿"
                  tone="danger"
                  onClick={onDiscard}
                  disabled={controlsDisabled}
                >
                  <Trash size={18} />
                </IconButton>
              </div>
            </footer>

            {resizeDirections.map((direction) => (
              <span
                key={direction}
                className={`compose-resize-handle compose-resize-handle--${direction}`}
                data-resize-direction={direction}
                aria-hidden="true"
                onPointerDown={(event) => beginResize(direction, event)}
              >
                {direction === "se" ? <DotsSix size={15} weight="bold" /> : null}
              </span>
            ))}
          </>
        )}
      </section>
    </div>
  );
}
