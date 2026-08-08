import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  CaretDown,
  ChatCircleDots,
  DotsSix,
  File,
  FloppyDisk,
  GridFour,
  Notebook,
  Paperclip,
  PaperPlaneTilt,
  PencilSimpleLine,
  Quotes,
  Rows,
  Trash,
  UserPlus,
  X,
} from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import {
  ComposeAiAssistant,
  ComposeOptimizeControl,
} from "./ComposeAiTools.jsx";
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
const composeWindowMotionFallbackMs = 260;
const composeWindowMotionFallbackPaddingMs = 80;
const composeAiAssistantWidth = 372;
const composeAiOverlayBreakpoint = 1200;
const dialogFocusableSelector = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[contenteditable='true']",
  "[tabindex]:not([tabindex='-1'])",
].join(",");
const stationeryTypeOptions = [
  { value: "lined", label: "横线纸", icon: Rows },
  { value: "grid", label: "方格纸", icon: GridFour },
];
const stationeryDeliveryOptions = [
  { value: "edit", label: "仅编辑时显示信纸", icon: PencilSimpleLine },
  { value: "send", label: "将信纸随邮件发送", icon: PaperPlaneTilt },
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

function minimizedComposeTitle(value, contacts) {
  const subject = value.subject.trim();
  const recipientEmail = [...value.to, ...value.cc, ...value.bcc]
    .find((email) => email?.trim())
    ?.trim();
  if (!recipientEmail) return subject || "新草稿";

  const normalizedEmail = recipientEmail.toLowerCase();
  const contact = contacts.find(
    (candidate) => candidate?.email?.trim().toLowerCase() === normalizedEmail,
  );
  const recipientLabel =
    contact?.displayName?.trim() ||
    contact?.remark?.trim() ||
    contact?.originalName?.trim() ||
    recipientEmail;
  return `${subject || "新草稿"}(${recipientLabel})`;
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

function prefersReducedMotion() {
  return (
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

function composeWindowMotionDuration(panel) {
  const rawDuration = getComputedStyle(panel)
    .getPropertyValue("--motion-window")
    .trim();
  if (rawDuration.endsWith("ms")) {
    const milliseconds = Number.parseFloat(rawDuration);
    if (Number.isFinite(milliseconds)) return milliseconds;
  }
  if (rawDuration.endsWith("s")) {
    const seconds = Number.parseFloat(rawDuration);
    if (Number.isFinite(seconds)) return seconds * 1000;
  }
  return composeWindowMotionFallbackMs;
}

function usableMotionRect(rect) {
  return (
    rect &&
    [rect.left, rect.top, rect.width, rect.height].every(Number.isFinite) &&
    rect.width > 0 &&
    rect.height > 0
  );
}

function applyPanelGeometry(panel, geometry) {
  if (!panel) return;
  panel.style.left = `${geometry.x}px`;
  panel.style.top = `${geometry.y}px`;
  panel.style.width = `${geometry.width}px`;
  panel.style.height = `${geometry.height}px`;
}

function geometryForInteraction(interaction) {
  const dx = interaction.clientX - interaction.pointerX;
  const dy = interaction.clientY - interaction.pointerY;
  const origin = interaction.geometry;

  if (interaction.kind === "drag") {
    return constrainGeometry({
      ...origin,
      x: origin.x + dx,
      y: origin.y + dy,
    });
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

  return { x: left, y: top, width: right - left, height: bottom - top };
}

function ComposeIconSegment({
  label,
  value,
  options,
  disabled,
  onChange,
}) {
  const selectedIndex = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  );

  return (
    <div
      className="compose-icon-segment"
      data-position={selectedIndex}
      role="radiogroup"
      aria-label={label}
    >
      <span className="compose-icon-segment__thumb" aria-hidden="true" />
      {options.map((option, optionIndex) => {
        const OptionIcon = option.icon;
        const selected = option.value === value;
        return (
          <IconButton
            key={option.value}
            className="compose-icon-segment__option"
            label={option.label}
            role="radio"
            aria-checked={selected}
            data-selected={selected}
            tabIndex={selected ? 0 : -1}
            disabled={disabled}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => {
              const direction =
                event.key === "ArrowLeft" || event.key === "ArrowUp"
                  ? -1
                  : event.key === "ArrowRight" || event.key === "ArrowDown"
                    ? 1
                    : 0;
              const nextIndex =
                event.key === "Home"
                  ? 0
                  : event.key === "End"
                    ? options.length - 1
                    : direction
                      ? (optionIndex + direction + options.length) %
                        options.length
                      : -1;
              if (nextIndex < 0) return;
              event.preventDefault();
              const group = event.currentTarget.closest(
                ".compose-icon-segment",
              );
              onChange(options[nextIndex].value);
              window.requestAnimationFrame(() =>
                group?.querySelectorAll('[role="radio"]')[nextIndex]?.focus(),
              );
            }}
          >
            <OptionIcon
              size={16}
              weight={selected ? "bold" : "regular"}
            />
          </IconButton>
        );
      })}
    </div>
  );
}

function StationeryControl({ format, disabled, onChange }) {
  const stationery = format?.stationery || "none";
  const enabled = stationery !== "none";
  const sendStationery = stationery !== "none" && format?.send_stationery === true;
  const lastStationeryRef = useRef(enabled ? stationery : "lined");

  useEffect(() => {
    if (enabled) lastStationeryRef.current = stationery;
  }, [enabled, stationery]);

  return (
    <div
      className="compose-stationery-control"
      data-enabled={enabled}
    >
      <IconButton
        className="compose-stationery-toggle"
        label={enabled ? "关闭信纸" : "启用信纸"}
        aria-pressed={enabled}
        disabled={disabled}
        onClick={() =>
          onChange(
            enabled
              ? { stationery: "none", send_stationery: false }
              : {
                  stationery: lastStationeryRef.current,
                  send_stationery: false,
                },
          )
        }
      >
        <Notebook
          size={17}
          weight={enabled ? "fill" : "regular"}
        />
      </IconButton>

      {enabled ? (
        <div className="compose-stationery-control__options">
          <ComposeIconSegment
            label="信纸类型"
            value={stationery}
            options={stationeryTypeOptions}
            disabled={disabled}
            onChange={(next) => onChange({ stationery: next })}
          />
          <ComposeIconSegment
            label="信纸发送方式"
            value={sendStationery ? "send" : "edit"}
            options={stationeryDeliveryOptions}
            disabled={disabled}
            onChange={(next) =>
              onChange({ send_stationery: next === "send" })
            }
          />
        </div>
      ) : null}
    </div>
  );
}

export function ComposePanel({
  accountId = null,
  value,
  draft = null,
  draftId,
  saveStatus,
  locked = false,
  readOnly = false,
  initiallyMinimized = false,
  restoreRequest = 0,
  onMinimizedChange = null,
  networkAvailable = true,
  onClose,
  onDiscard,
  onChange,
  onBodyChange = onChange,
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
  const initialGeometryRef = useRef(null);
  if (initialGeometryRef.current === null) {
    const normal = loadInitialGeometry();
    initialGeometryRef.current = {
      normal,
      current: initiallyMinimized ? minimizedGeometry() : normal,
    };
  }
  const [geometry, setGeometry] = useState(
    () => initialGeometryRef.current.current,
  );
  const [isMinimized, setIsMinimized] = useState(
    Boolean(initiallyMinimized),
  );
  const [hasEntered, setHasEntered] = useState(false);
  const [interactionKind, setInteractionKind] = useState(null);
  const [windowMotion, setWindowMotion] = useState(null);
  const [isReplyExpanded, setIsReplyExpanded] = useState(false);
  const [isForwardExpanded, setIsForwardExpanded] = useState(false);
  const [isAiAssistantOpen, setIsAiAssistantOpen] = useState(false);
  const interactionRef = useRef(null);
  const geometryRef = useRef(geometry);
  const minimizedGeometryRef = useRef(
    initiallyMinimized ? initialGeometryRef.current.normal : null,
  );
  const lastRestoreRequestRef = useRef(restoreRequest);
  const dialogRef = useRef(null);
  const pendingWindowMotionRef = useRef(null);
  const windowMotionRef = useRef(null);
  const windowMotionTokenRef = useRef(0);
  const assistantRestoreXRef = useRef(null);
  const localDraftIdentityRef = useRef(
    draft?.id || draftId || `local-draft-${Date.now().toString(36)}`,
  );
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

  useEffect(() => {
    onMinimizedChange?.(isMinimized);
  }, [isMinimized, onMinimizedChange]);

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

  useEffect(() => {
    if (
      !isAiAssistantOpen ||
      isMinimized ||
      window.innerWidth < composeAiOverlayBreakpoint
    ) {
      return;
    }
    const current = geometryRef.current;
    const rightEdge = current.x + current.width + composeAiAssistantWidth;
    const availableRight = window.innerWidth - composeMargin;
    if (rightEdge <= availableRight) return;
    if (assistantRestoreXRef.current === null) {
      assistantRestoreXRef.current = current.x;
    }
    commitGeometry({
      ...current,
      x: Math.max(composeMargin, current.x - (rightEdge - availableRight)),
    });
  }, [commitGeometry, isAiAssistantOpen, isMinimized]);

  const collapseAiAssistant = useCallback(() => {
    setIsAiAssistantOpen(false);
    if (assistantRestoreXRef.current === null) return;
    const restoreX = assistantRestoreXRef.current;
    assistantRestoreXRef.current = null;
    commitGeometry((current) =>
      constrainGeometry({
        ...current,
        x: restoreX,
      }),
    );
  }, [commitGeometry]);

  const applyInteractionFrame = useCallback(() => {
    const interaction = interactionRef.current;
    const panel = dialogRef.current;
    if (!interaction || !panel) return null;

    interaction.rafId = null;
    const nextGeometry = geometryForInteraction(interaction);

    if (interaction.kind === "drag") {
      panel.style.translate = `${nextGeometry.x - interaction.geometry.x}px ${
        nextGeometry.y - interaction.geometry.y
      }px`;
    } else {
      applyPanelGeometry(panel, nextGeometry);
    }

    return nextGeometry;
  }, []);

  const endInteraction = useCallback(
    ({ commit = true } = {}) => {
      const interaction = interactionRef.current;
      if (interaction?.rafId !== null && interaction?.rafId !== undefined) {
        window.cancelAnimationFrame?.(interaction.rafId);
        interaction.rafId = null;
      }

      if (interaction && commit && !isMinimized) {
        const finalGeometry = applyInteractionFrame() || interaction.geometry;
        const panel = dialogRef.current;
        applyPanelGeometry(panel, finalGeometry);
        panel?.style.removeProperty("translate");
        geometryRef.current = finalGeometry;
        setGeometry(finalGeometry);
        persistGeometry(finalGeometry);
      } else {
        dialogRef.current?.style.removeProperty("translate");
      }

      interactionRef.current = null;
      if (commit) setInteractionKind(null);
      document.body.style.removeProperty("user-select");
      document.body.style.removeProperty("cursor");
    },
    [applyInteractionFrame, isMinimized],
  );

  useEffect(() => {
    const onPointerMove = (event) => {
      const interaction = interactionRef.current;
      if (!interaction) return;
      interaction.clientX = event.clientX;
      interaction.clientY = event.clientY;
      if (interaction.rafId !== null) return;
      if (typeof window.requestAnimationFrame !== "function") {
        applyInteractionFrame();
        return;
      }
      interaction.rafId = window.requestAnimationFrame(applyInteractionFrame);
    };
    const onPointerEnd = () => endInteraction();
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerEnd);
    window.addEventListener("pointercancel", onPointerEnd);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerEnd);
      window.removeEventListener("pointercancel", onPointerEnd);
      endInteraction({ commit: false });
    };
  }, [applyInteractionFrame, endInteraction]);

  const finishWindowMotion = useCallback((token = null, updateState = true) => {
    const motion = windowMotionRef.current;
    if (token !== null && motion?.token !== token) return;
    if (motion?.rafId !== null && motion?.rafId !== undefined) {
      window.cancelAnimationFrame?.(motion.rafId);
    }
    if (motion?.timerId !== null && motion?.timerId !== undefined) {
      window.clearTimeout(motion.timerId);
    }
    pendingWindowMotionRef.current = null;
    windowMotionRef.current = null;

    const panel = dialogRef.current;
    if (panel) {
      panel.style.removeProperty("transform");
      panel.style.removeProperty("transform-origin");
      delete panel.dataset.windowMotionStage;
    }
    if (updateState) setWindowMotion(null);
  }, []);

  const beginWindowMotion = useCallback(
    ({ phase, targetGeometry, targetMinimized }) => {
      const panel = dialogRef.current;
      const startRect = panel?.getBoundingClientRect?.() || null;
      finishWindowMotion();

      geometryRef.current = targetGeometry;
      setGeometry(targetGeometry);
      setIsMinimized(targetMinimized);
      setHasEntered(true);

      if (prefersReducedMotion()) {
        setWindowMotion(null);
        return;
      }

      if (!usableMotionRect(startRect)) {
        const motion = {
          token: windowMotionTokenRef.current + 1,
          phase,
          startRect: null,
          rafId: null,
          timerId: null,
        };
        windowMotionTokenRef.current = motion.token;
        windowMotionRef.current = motion;
        setWindowMotion(phase);
        motion.timerId = window.setTimeout(
          () => finishWindowMotion(motion.token),
          composeWindowMotionFallbackMs + composeWindowMotionFallbackPaddingMs,
        );
        return;
      }

      const motion = {
        token: windowMotionTokenRef.current + 1,
        phase,
        startRect,
        rafId: null,
        timerId: null,
      };
      windowMotionTokenRef.current = motion.token;
      pendingWindowMotionRef.current = motion;
      windowMotionRef.current = motion;
      setWindowMotion(phase);
    },
    [finishWindowMotion],
  );

  useLayoutEffect(() => {
    const motion = pendingWindowMotionRef.current;
    const panel = dialogRef.current;
    if (!motion || !panel || motion.phase !== windowMotion) return;
    pendingWindowMotionRef.current = null;

    const finalRect = panel.getBoundingClientRect();
    if (!usableMotionRect(finalRect)) {
      finishWindowMotion(motion.token);
      return;
    }

    const translateX = motion.startRect.left - finalRect.left;
    const translateY = motion.startRect.top - finalRect.top;
    const scaleX = motion.startRect.width / finalRect.width;
    const scaleY = motion.startRect.height / finalRect.height;
    const effectivelyStatic =
      Math.abs(translateX) < 0.01 &&
      Math.abs(translateY) < 0.01 &&
      Math.abs(scaleX - 1) < 0.001 &&
      Math.abs(scaleY - 1) < 0.001;
    if (effectivelyStatic) {
      finishWindowMotion(motion.token);
      return;
    }

    panel.dataset.windowMotionStage = "inverted";
    panel.style.transformOrigin = "top left";
    panel.style.transform = `translate3d(${translateX}px, ${translateY}px, 0) scale(${scaleX}, ${scaleY})`;
    panel.getBoundingClientRect();

    const startTransition = () => {
      if (windowMotionRef.current?.token !== motion.token) return;
      panel.dataset.windowMotionStage = "running";
      panel.getBoundingClientRect();
      panel.style.transform = "translate3d(0, 0, 0) scale(1, 1)";
      motion.timerId = window.setTimeout(
        () => finishWindowMotion(motion.token),
        composeWindowMotionDuration(panel) + composeWindowMotionFallbackPaddingMs,
      );
    };
    motion.rafId =
      typeof window.requestAnimationFrame === "function"
        ? window.requestAnimationFrame(startTransition)
        : null;
    if (motion.rafId === null) startTransition();
  }, [finishWindowMotion, windowMotion]);

  useEffect(
    () => () => {
      finishWindowMotion(null, false);
    },
    [finishWindowMotion],
  );

  useEffect(() => {
    const onWindowResize = () => {
      finishWindowMotion();
      endInteraction();
      if (isMinimized) {
        commitGeometry(minimizedGeometry());
        return;
      }
      commitGeometry((current) => constrainGeometry(current));
    };
    window.addEventListener("resize", onWindowResize);
    return () => window.removeEventListener("resize", onWindowResize);
  }, [commitGeometry, endInteraction, finishWindowMotion, isMinimized]);

  const beginDrag = (event) => {
    if (
      event.button !== 0 ||
      windowMotionRef.current ||
      event.target.closest(
        "button, input, textarea, [contenteditable], [data-no-compose-drag]",
      )
    ) {
      return;
    }
    event.preventDefault();
    assistantRestoreXRef.current = null;
    interactionRef.current = {
      kind: "drag",
      pointerX: event.clientX,
      pointerY: event.clientY,
      clientX: event.clientX,
      clientY: event.clientY,
      geometry: geometryRef.current,
      rafId: null,
    };
    setInteractionKind("drag");
    document.body.style.userSelect = "none";
    document.body.style.cursor = "grabbing";
  };

  const beginResize = (direction, event) => {
    if (event.button !== 0 || isMinimized || windowMotionRef.current) return;
    event.preventDefault();
    event.stopPropagation();
    assistantRestoreXRef.current = null;
    interactionRef.current = {
      kind: "resize",
      direction,
      pointerX: event.clientX,
      pointerY: event.clientY,
      clientX: event.clientX,
      clientY: event.clientY,
      geometry: geometryRef.current,
      rafId: null,
    };
    setInteractionKind("resize");
    document.body.style.userSelect = "none";
    document.body.style.cursor = getComputedStyle(event.currentTarget).cursor;
  };

  const restoreComposer = useCallback(() => {
    if (!isMinimized) return false;
    endInteraction();
    const targetGeometry = constrainGeometry(
      minimizedGeometryRef.current || loadInitialGeometry(),
    );
    minimizedGeometryRef.current = null;
    beginWindowMotion({
      phase: "restoring",
      targetGeometry,
      targetMinimized: false,
    });
    return true;
  }, [beginWindowMotion, endInteraction, isMinimized]);

  useEffect(() => {
    if (lastRestoreRequestRef.current === restoreRequest) return;
    lastRestoreRequestRef.current = restoreRequest;
    restoreComposer();
  }, [restoreComposer, restoreRequest]);

  const toggleMinimized = () => {
    if (restoreComposer()) return;
    endInteraction();
    minimizedGeometryRef.current = geometryRef.current;
    beginWindowMotion({
      phase: "minimizing",
      targetGeometry: minimizedGeometry(),
      targetMinimized: true,
    });
  };

  const saveAndMinimize = async () => {
    if ((await onSaveDraft()) === true) toggleMinimized();
  };

  const canSend = useMemo(() => {
    const recipients = [...value.to, ...value.cc, ...value.bcc];
    return recipients.length > 0 && recipients.every(Boolean);
  }, [value.bcc, value.cc, value.to]);
  const isBusy = locked;
  const authoritativeDraft = draft || value;
  const attachments = Array.isArray(authoritativeDraft?.attachments)
    ? authoritativeDraft.attachments
    : [];
  const localVersion = authoritativeDraft?.local_version;
  const stableDraftId = authoritativeDraft?.id || draftId;
  const currentDraftForAi = useMemo(
    () => ({
      id: stableDraftId || localDraftIdentityRef.current,
      subject: value.subject,
    }),
    [stableDraftId, value.subject],
  );
  const hasStableDraft =
    Boolean(stableDraftId) &&
    Number.isInteger(localVersion) &&
    localVersion >= 1;
  const aiDraft = useMemo(() => {
    if (!accountId) return null;
    return {
      account_id: accountId,
      draft_id: hasStableDraft ? stableDraftId : null,
      local_version: hasStableDraft ? localVersion : null,
      compose: {
        to: value.to || [],
        cc: value.cc || [],
        bcc: value.bcc || [],
        subject: value.subject || "",
        body_text: value.body_text || "",
        format: value.format || {
          body_html: null,
          stationery: "none",
          send_stationery: false,
        },
        reply_context: value.reply_context || null,
      },
      attachments: hasStableDraft ? attachments : [],
      forward_context: hasStableDraft
        ? authoritativeDraft?.forward_context || null
        : null,
    };
  }, [
    accountId,
    attachments,
    authoritativeDraft?.forward_context,
    hasStableDraft,
    localVersion,
    stableDraftId,
    value,
  ]);
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
  const openLinkedDraft = useCallback((linkedDraft) => {
    if (linkedDraft?.id !== currentDraftForAi.id) return;
    document.getElementById("compose-subject")?.focus();
  }, [currentDraftForAi.id]);

  useEffect(() => {
    const onKeyDown = (event) => {
      if (event.defaultPrevented) return;
      if (event.key === "Escape" && !isBusy) {
        event.preventDefault();
        if (isAiAssistantOpen) {
          collapseAiAssistant();
          return;
        }
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
    collapseAiAssistant,
    controlsDisabled,
    isAiAssistantOpen,
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

  const minimizedTitle = minimizedComposeTitle(value, contacts);
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
      : "保存并最小化";
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
        data-ai-assistant={
          !isMinimized && isAiAssistantOpen ? "open" : undefined
        }
        data-entered={hasEntered || undefined}
        data-interacting={interactionKind || undefined}
        data-window-motion={windowMotion || undefined}
        onKeyDown={trapDialogFocus}
        onAnimationEnd={(event) => {
          if (
            event.target === event.currentTarget &&
            event.animationName === "compose-in"
          ) {
            setHasEntered(true);
          }
        }}
        onTransitionEnd={(event) => {
          if (
            event.target !== event.currentTarget ||
            event.propertyName !== "transform" ||
            event.currentTarget.dataset.windowMotionStage !== "running"
          ) {
            return;
          }
          const expectedDuration =
            composeWindowMotionDuration(event.currentTarget) / 1000;
          if (event.elapsedTime + 0.01 >= expectedDuration) {
            finishWindowMotion();
          }
        }}
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
          <div className="compose-expanded-shell">
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
                  onBodyChange((current) => ({
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
                className="compose-attachments vertical-scroll-surface"
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
                  {locked ? "正在准备…" : readOnly ? "只读" : "发送"}
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
                <ComposeOptimizeControl
                  aiDraft={aiDraft}
                  value={value}
                  disabled={controlsDisabled}
                  onApply={onChange}
                />
                <IconButton
                  className="compose-ai-toggle"
                  label={
                    isAiAssistantOpen ? "收起 AI 助理" : "打开 AI 助理"
                  }
                  aria-pressed={isAiAssistantOpen}
                  disabled={isBusy}
                  onClick={() => {
                    if (isAiAssistantOpen) collapseAiAssistant();
                    else setIsAiAssistantOpen(true);
                  }}
                >
                  <ChatCircleDots
                    size={18}
                    weight={isAiAssistantOpen ? "fill" : "regular"}
                  />
                </IconButton>
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
                  onClick={() => void saveAndMinimize()}
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
          </div>
        )}
        {!isMinimized && isAiAssistantOpen ? (
          <ComposeAiAssistant
            aiDraft={aiDraft}
            value={value}
            currentDraft={currentDraftForAi}
            disabled={isBusy}
            readOnly={readOnly}
            onApplyDraft={onChange}
            onCollapse={collapseAiAssistant}
            onOpenDraft={openLinkedDraft}
          />
        ) : null}
      </section>
    </div>
  );
}
