import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  CaretDown,
  DotsSix,
  FloppyDisk,
  Minus,
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
import { splitAddresses } from "../utils/formatters.js";

const composeMargin = 22;
const composeTopBoundary = 52;
const composeMinWidth = 520;
const composeMinHeight = 420;
const composeMinimizedWidth = 340;
const composeMinimizedHeight = 44;
const composeMinimizedBottom = 18;
const composeGeometryStorageKey = "mine-mail-compose-geometry-v1";
const resizeDirections = ["n", "ne", "e", "se", "s", "sw", "w", "nw"];

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
  const width = Math.min(660, viewport.width - 42);
  const height = Math.min(680, viewport.height - 54);
  return constrainGeometry({
    x: viewport.width - width - 26,
    y: viewport.height - height - composeMargin,
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

export function ComposePanel({
  value,
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
  remoteImageMode = "automatic",
  onOpenExternalLink,
}) {
  const [showCopies, setShowCopies] = useState(
    Boolean(value.cc?.length || value.bcc?.length),
  );
  const [geometry, setGeometry] = useState(loadInitialGeometry);
  const [isMinimized, setIsMinimized] = useState(false);
  const [isReplyExpanded, setIsReplyExpanded] = useState(false);
  const interactionRef = useRef(null);
  const geometryRef = useRef(geometry);
  const minimizedGeometryRef = useRef(null);

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
      event.target.closest("button, input, textarea, [data-no-compose-drag]")
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
  const controlsDisabled = isBusy || readOnly;
  const replyContext = value.reply_context || null;

  useEffect(() => {
    const onKeyDown = (event) => {
      if (event.key === "Escape" && !isBusy) onClose();
      if (
        (event.metaKey || event.ctrlKey) &&
        event.key === "Enter" &&
        canSend &&
        networkAvailable &&
        !controlsDisabled
      ) {
        event.preventDefault();
        onRequestSend();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [canSend, controlsDisabled, isBusy, networkAvailable, onClose, onRequestSend]);

  const setAddressField = (field, value) => {
    onChange((current) => ({ ...current, [field]: splitAddresses(value) }));
  };

  const minimizedTitle = value.subject.trim() || "新邮件";
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

  return (
    <div
      className="compose-layer"
      role="presentation"
      data-minimized={isMinimized}
    >
      <section
        className="compose-panel"
        role="dialog"
        aria-modal={isMinimized ? undefined : "true"}
        aria-label={isMinimized ? minimizedTitle : undefined}
        aria-labelledby={isMinimized ? undefined : "compose-title"}
        data-minimized={isMinimized}
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
          <button
            className="compose-minimized-bar"
            type="button"
            aria-label={`还原写信窗口：${minimizedTitle}`}
            onClick={toggleMinimized}
          >
            <span>{minimizedTitle}</span>
          </button>
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
            <div className="compose-window-actions" data-no-compose-drag>
              <IconButton label="最小化写信窗口" onClick={toggleMinimized}>
                <Minus size={17} />
              </IconButton>
              <IconButton label="关闭写信窗口" onClick={onClose} disabled={isBusy}>
                <X size={18} />
              </IconButton>
            </div>

            {readOnly ? (
              <div className="compose-unsupported-notice" role="status">
                含当前不支持的HTML/附件，未作修改
              </div>
            ) : null}

            <div className="compose-fields">
              <div className="compose-field">
                <label htmlFor="compose-to">收件人</label>
                <div className="compose-input-shell inset-input-shell">
                  <input
                    id="compose-to"
                    autoFocus
                    disabled={controlsDisabled}
                    aria-label="收件人"
                    value={value.to.join(", ")}
                    onChange={(event) => setAddressField("to", event.target.value)}
                    placeholder="name@example.com"
                  />
                </div>
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
                    <div className="compose-input-shell inset-input-shell">
                      <input
                        id="compose-cc"
                        aria-label="抄送"
                        disabled={controlsDisabled}
                        value={value.cc.join(", ")}
                        onChange={(event) => setAddressField("cc", event.target.value)}
                      />
                    </div>
                  </div>
                  <div className="compose-field">
                    <label htmlFor="compose-bcc">密送</label>
                    <div className="compose-input-shell inset-input-shell">
                      <input
                        id="compose-bcc"
                        aria-label="密送"
                        disabled={controlsDisabled}
                        value={value.bcc.join(", ")}
                        onChange={(event) => setAddressField("bcc", event.target.value)}
                      />
                    </div>
                  </div>
                </div>
              ) : null}
              <div className="compose-field compose-field--subject">
                <label htmlFor="compose-subject">主题</label>
                <div className="compose-input-shell inset-input-shell">
                  <input
                    id="compose-subject"
                    aria-label="主题"
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

            <textarea
              className="compose-body"
              value={value.body_text}
              onChange={(event) =>
                onChange((current) => ({ ...current, body_text: event.target.value }))
              }
              placeholder="开始写邮件…"
              aria-label="邮件正文"
              disabled={controlsDisabled}
            />

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
                  <div className="compose-reply-context__body">
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

            <footer className="compose-footer">
              <div className="compose-footer__left">
                <button
                  className="send-button"
                  type="button"
                  aria-label="发送邮件"
                  disabled={!canSend || !networkAvailable || controlsDisabled}
                  onClick={onRequestSend}
                >
                  <PaperPlaneTilt size={18} weight="fill" />
                  {isSending ? "正在发送…" : locked ? "正在准备…" : readOnly ? "只读" : "发送"}
                  <kbd>{sendShortcut}</kbd>
                </button>
                <IconButton label="添加附件（尚未实现）" disabled>
                  <Paperclip size={19} />
                </IconButton>
              </div>
              <div className="compose-footer__right">
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
