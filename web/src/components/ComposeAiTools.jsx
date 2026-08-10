import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import {
  ArrowCounterClockwise,
  ArrowLeft,
  CaretDown,
  CaretUp,
  Check,
  CheckCircle,
  Gear,
  MagicWand,
  PaperPlaneRight,
  SidebarSimple,
  Sparkle,
  SpinnerGap,
  Stop,
  Trash,
} from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { ThemedSelect } from "./ThemedSelect.jsx";
import { ConsequentialConfirmDialog } from "./ConsequentialConfirmDialog.jsx";
import {
  buildOptimizationAnnotations,
  ComposeOptimizationReviewDialog,
  optimizationAnnotationText,
} from "./ComposeOptimizationReviewDialog.jsx";
import { mailApi } from "../services/mailApi.js";

const ComposeAiMarkdown = lazy(() => import("./ComposeAiMarkdown.jsx"));

const agentModeOptions = [
  { value: "auto", label: "自动" },
  { value: "generate", label: "邮件生成" },
  { value: "chat", label: "聊天" },
];

const agentModePlaceholders = {
  auto: "告诉 AI 助理你想完成什么",
  generate: "描述邮件主旨、对象和期望语气",
  chat: "询问当前邮件或相关内容",
};

function displaySubject(subject) {
  return subject?.trim() || "无主题";
}

export function shortDraftDisplayId(value) {
  const source = String(value || "new-draft");
  let hash = 2166136261;
  for (let index = 0; index < source.length; index += 1) {
    hash ^= source.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return `#${(hash >>> 0).toString(16).toUpperCase().padStart(8, "0")}`;
}

function editableDraftFingerprint(value) {
  return JSON.stringify({
    to: value?.to || [],
    cc: value?.cc || [],
    bcc: value?.bcc || [],
    subject: value?.subject || "",
    body_text: value?.body_text || "",
    body_html: value?.format?.body_html || null,
    stationery: value?.format?.stationery || "none",
    send_stationery: Boolean(value?.format?.send_stationery),
  });
}

export function createAiDraftRevision() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }
  return `ai-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`;
}

function copyFormat(format) {
  return { ...(format || {}) };
}

function applyAiDraft(current, generated) {
  return {
    ...current,
    to: generated.to,
    cc: generated.cc,
    bcc: generated.bcc,
    subject: generated.subject,
    body_text: generated.body_text,
    format: {
      ...(current.format || {}),
      ...(generated.format || {}),
    },
  };
}

function recordPatchOutcome(result, aiDraft, outcome) {
  if (!result?.request_id || !aiDraft?.account_id) return;
  void mailApi
    .recordAiPatchOutcome({
      requestId: result.request_id,
      accountId: aiDraft.account_id,
      draftId: aiDraft.draft_id,
      outcome,
      changedFields: result.changed_fields || [],
    })
    .catch(() => undefined);
}

function useOptimizationCacheState(cacheRef, key, initialValue) {
  const [value, setValue] = useState(() => {
    if (Object.prototype.hasOwnProperty.call(cacheRef.current, key)) {
      return cacheRef.current[key];
    }
    const resolved =
      typeof initialValue === "function" ? initialValue() : initialValue;
    cacheRef.current[key] = resolved;
    return resolved;
  });
  const setCachedValue = useCallback(
    (nextValue) => {
      const resolved =
        typeof nextValue === "function"
          ? nextValue(cacheRef.current[key])
          : nextValue;
      cacheRef.current[key] = resolved;
      setValue(resolved);
    },
    [cacheRef, key],
  );
  return [value, setCachedValue];
}

export function ComposeOptimizeControl({
  aiDraft,
  cacheRef: providedCacheRef,
  disabled,
  onApply,
  value,
}) {
  const rootRef = useRef(null);
  const optimizeButtonRef = useRef(null);
  const latestValueRef = useRef(value);
  const localCacheRef = useRef({});
  const cacheRef = providedCacheRef || localCacheRef;
  const [promptOpen, setPromptOpen] = useOptimizationCacheState(
    cacheRef,
    "promptOpen",
    false,
  );
  const [instruction, setInstruction] = useOptimizationCacheState(
    cacheRef,
    "instruction",
    "",
  );
  const [status, setStatus] = useOptimizationCacheState(
    cacheRef,
    "status",
    "idle",
  );
  const [errorMessage, setErrorMessage] = useOptimizationCacheState(
    cacheRef,
    "errorMessage",
    "",
  );
  const [reviewResult, setReviewResult] = useOptimizationCacheState(
    cacheRef,
    "reviewResult",
    null,
  );
  const [reviewOpen, setReviewOpen] = useOptimizationCacheState(
    cacheRef,
    "reviewOpen",
    false,
  );
  const [leftAnnotations, setLeftAnnotations] = useOptimizationCacheState(
    cacheRef,
    "leftAnnotations",
    [],
  );
  const [rightAnnotations, setRightAnnotations] = useOptimizationCacheState(
    cacheRef,
    "rightAnnotations",
    [],
  );
  const [leftEdited, setLeftEdited] = useOptimizationCacheState(
    cacheRef,
    "leftEdited",
    false,
  );
  const [rightEdited, setRightEdited] = useOptimizationCacheState(
    cacheRef,
    "rightEdited",
    false,
  );
  const [confirmation, setConfirmation] = useOptimizationCacheState(
    cacheRef,
    "confirmation",
    null,
  );
  const [undoBackup, setUndoBackup] = useOptimizationCacheState(
    cacheRef,
    "undoBackup",
    null,
  );
  const hasContent = Boolean(String(value.body_text || "").trim());

  latestValueRef.current = value;

  useEffect(() => {
    if (!promptOpen) return undefined;
    const closeOnOutsidePointer = (event) => {
      if (!rootRef.current?.contains(event.target)) setPromptOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [promptOpen]);

  const optimize = async () => {
    if (reviewResult) {
      setReviewOpen(true);
      return;
    }
    if (disabled || !hasContent || status === "running" || !aiDraft) return;
    const submitted = {
      body_text: value.body_text || "",
      format: copyFormat(value.format),
    };
    setErrorMessage("");
    setStatus("running");
    try {
      const result = await mailApi.runAiTurn({
        mode: "optimize",
        instruction: instruction.trim() || "优化当前邮件正文",
        session_id: null,
        draft_revision: createAiDraftRevision(),
        draft: { ...aiDraft, compose: value },
      });
      const optimized = {
        body_text: result.draft?.body_text ?? submitted.body_text,
        format: copyFormat(result.draft?.format || submitted.format),
      };
      const annotations = buildOptimizationAnnotations(
        submitted.body_text,
        optimized.body_text,
      );
      setReviewResult({ result, submitted, optimized });
      setLeftAnnotations(annotations.left);
      setRightAnnotations(annotations.right);
      setLeftEdited(false);
      setRightEdited(false);
      setStatus("ready");
    } catch (error) {
      setErrorMessage(error?.message || "邮件优化没有完成，请重试");
      setStatus("idle");
    }
  };

  const undo = () => {
    if (!undoBackup) return;
    onApply((current) => ({
      ...current,
      body_text: undoBackup.body_text,
      format: copyFormat(undoBackup.format),
    }));
    setUndoBackup(null);
  };

  const clearReview = (outcome) => {
    if (reviewResult) recordPatchOutcome(reviewResult.result, aiDraft, outcome);
    setReviewResult(null);
    setReviewOpen(false);
    setLeftAnnotations([]);
    setRightAnnotations([]);
    setLeftEdited(false);
    setRightEdited(false);
    setConfirmation(null);
    setStatus("idle");
  };

  const applyReview = (side) => {
    if (!reviewResult) return;
    const useLeft = side === "left";
    const selectedText = optimizationAnnotationText(
      useLeft ? leftAnnotations : rightAnnotations,
    );
    const selectedFormat = copyFormat(
      useLeft ? reviewResult.submitted.format : reviewResult.optimized.format,
    );
    const wasEdited = useLeft ? leftEdited : rightEdited;
    const live = latestValueRef.current;
    setUndoBackup({
      body_text: live.body_text || "",
      format: copyFormat(live.format),
    });
    onApply((current) => ({
      ...current,
      body_text: selectedText,
      format: {
        ...(current.format || {}),
        ...selectedFormat,
        ...(wasEdited ? { body_html: null } : {}),
      },
    }));
    clearReview("applied");
  };

  return (
    <div ref={rootRef} className="compose-optimize-control">
      <div
        className="compose-optimize-split"
        data-prompt-open={promptOpen}
        data-result-ready={Boolean(reviewResult) || undefined}
      >
        <IconButton
          ref={optimizeButtonRef}
          className="compose-optimize-split__action"
          label={reviewResult ? "查看优化结果" : "优化当前邮件"}
          disabled={
            status === "running" ||
            (!reviewResult && (disabled || !hasContent || !aiDraft))
          }
          aria-busy={status === "running"}
          onClick={() => void optimize()}
        >
          {status === "running" ? (
            <SpinnerGap className="compose-optimize-spinner" size={17} weight="bold" />
          ) : (
            <MagicWand size={17} weight="fill" />
          )}
        </IconButton>
        <IconButton
          className="compose-optimize-split__prompt"
          label={promptOpen ? "收起优化要求" : "填写优化要求"}
          aria-expanded={promptOpen}
          aria-controls="compose-optimize-prompt"
          disabled={disabled}
          onClick={() => setPromptOpen((current) => !current)}
        >
          {promptOpen ? (
            <CaretDown size={13} weight="bold" />
          ) : (
            <CaretUp size={13} weight="bold" />
          )}
        </IconButton>
      </div>

      {promptOpen ? (
        <div
          id="compose-optimize-prompt"
          className="compose-optimize-popover"
          data-no-compose-drag
        >
          <label htmlFor="compose-optimize-instruction">补充优化要求</label>
          <textarea
            id="compose-optimize-instruction"
            autoFocus
            rows={3}
            value={instruction}
            placeholder="例如：更简洁、更正式，保留原有信息"
            onChange={(event) => setInstruction(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                setPromptOpen(false);
              }
              if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
                event.preventDefault();
                void optimize();
              }
            }}
          />
          <small>填写后，点击魔棒开始优化</small>
        </div>
      ) : null}

      <IconButton
        className="compose-optimize-undo"
        label="回退上次优化"
        disabled={disabled || !undoBackup}
        onClick={undo}
      >
        <ArrowCounterClockwise size={16} weight="bold" />
      </IconButton>
      {errorMessage ? (
        <small className="compose-ai-inline-error" role="status">
          {errorMessage}
        </small>
      ) : null}

      {createPortal(
        <>
          <ComposeOptimizationReviewDialog
            open={reviewOpen && Boolean(reviewResult) && !confirmation}
            leftAnnotations={leftAnnotations}
            rightAnnotations={rightAnnotations}
            returnFocusRef={optimizeButtonRef}
            onChangeLeft={(annotations) => {
              setLeftAnnotations(annotations);
              setLeftEdited(true);
            }}
            onChangeRight={(annotations) => {
              setRightAnnotations(annotations);
              setRightEdited(true);
            }}
            onChoose={(side) => setConfirmation({ type: "apply", side })}
            onMinimize={() => setReviewOpen(false)}
            onClose={() => setConfirmation({ type: "close" })}
          />

          <ConsequentialConfirmDialog
            open={confirmation?.type === "apply"}
            title="应用优化结果？"
            description={`您确认选用${confirmation?.side === "left" ? "左侧" : "右侧"}的结果吗？`}
            icon={<CheckCircle size={23} weight="duotone" />}
            confirmLabel="确认应用"
            closeLabel="取消应用优化结果"
            returnFocusRef={optimizeButtonRef}
            onCancel={() => setConfirmation(null)}
            onConfirm={() => applyReview(confirmation.side)}
          />

          <ConsequentialConfirmDialog
            open={confirmation?.type === "close"}
            title="关闭优化结果？"
            description="确认关闭此次优化结果吗？关闭后将无法再次查看或应用。"
            icon={<Trash size={23} weight="duotone" />}
            tone="danger"
            confirmLabel="确认关闭"
            closeLabel="取消关闭优化结果"
            returnFocusRef={optimizeButtonRef}
            onCancel={() => setConfirmation(null)}
            onConfirm={() => clearReview("rejected")}
          />
        </>,
        document.body,
      )}
    </div>
  );
}

function DraftPills({ drafts, onOpenDraft }) {
  if (!drafts.length) return null;
  return (
    <div className="compose-ai-drafts" aria-label="关联草稿">
      {drafts.map((draft) => {
        const title = displaySubject(draft.subject);
        const shortId = shortDraftDisplayId(draft.id);
        return (
          <button
            key={draft.id}
            className="compose-ai-draft-pill"
            type="button"
            title={`${title} · ${draft.id}`}
            onClick={() => onOpenDraft(draft)}
          >
            <span>{title}</span>
            <small>{shortId}</small>
          </button>
        );
      })}
    </div>
  );
}

function SessionList({ disabled, onOpenSession, sessions }) {
  return (
    <div className="compose-ai-session-list vertical-scroll-surface">
      <h3>会话</h3>
      <div className="compose-ai-session-list__rows">
        {sessions.map((session) => (
          <button
            key={session.id}
            className="compose-ai-session-row"
            type="button"
            disabled={disabled}
            onClick={() => onOpenSession(session.id)}
          >
            <strong className="compose-ai-session-row__title">
              {session.title}
            </strong>
            <time className="compose-ai-session-row__time">
              {session.lastActive}
            </time>
          </button>
        ))}
      </div>
    </div>
  );
}

function SafeMarkdown({ children, onOpenExternalLink }) {
  return (
    <Suspense
      fallback={(
        <p className="compose-ai-message__markdown-loading">
          {children || ""}
        </p>
      )}
    >
      <ComposeAiMarkdown
        onOpenExternalLink={(href) => {
          if (onOpenExternalLink) return onOpenExternalLink(href);
          return mailApi.openExternalUrl(href);
        }}
      >
        {children || ""}
      </ComposeAiMarkdown>
    </Suspense>
  );
}

function mergeProposalGroup(current, resolved, group) {
  if (group === "headers") {
    return {
      ...current,
      to: resolved.to || [],
      cc: resolved.cc || [],
      bcc: resolved.bcc || [],
      subject: resolved.subject || "",
    };
  }
  return {
    ...current,
    body_text: resolved.body_text || "",
    format: copyFormat(resolved.format),
  };
}

function updateStreamingAssistant(sessions, sessionId, updater) {
  if (!sessionId) return sessions;
  return sessions.map((session) => ({
    ...session,
    messages:
      session.id === sessionId
        ? (session.messages || []).map((message) =>
            message.role === "assistant" && message.status === "streaming"
              ? updater(message)
              : message,
          )
        : session.messages,
  }));
}

function finishRunningActivities(message, status) {
  return {
    ...message,
    status,
    activities: (message.activities || []).map((activity) =>
      activity.status === "running"
        ? {
            ...activity,
            detail: "",
            label:
              activity.kind === "thinking"
                ? status === "stopped"
                  ? "思考已停止"
                  : "思考中断"
                : status === "stopped"
                  ? "工具调用已停止"
                  : "工具调用未完成",
            status,
            success: false,
          }
        : activity,
    ),
  };
}

function beginToolActivity(message, event, completeThinking = false) {
  const displayName = event.display_name || event.name;
  let activities = (message.activities || []).map((activity) => {
    if (
      completeThinking &&
      activity.id === event.thinking_activity_id &&
      activity.kind === "thinking" &&
      activity.status === "running"
    ) {
      return {
        ...activity,
        label: "分析完成",
        detail: "",
        status: "completed",
        success: true,
      };
    }
    return activity;
  });
  const nextTool = {
    id: event.activity_id,
    kind: "tool",
    label: `正在调用「${displayName}」工具…`,
    detail: "",
    status: "running",
    success: null,
  };
  if (activities.some((activity) => activity.id === event.activity_id)) {
    activities = activities.map((activity) =>
      activity.id === event.activity_id ? { ...activity, ...nextTool } : activity,
    );
  } else {
    activities = [...activities, nextTool];
  }
  return { ...message, activities };
}

function ProposalAction({ busy, group, label, proposal, onResolve }) {
  const state = proposal[group];
  if (!state?.changed) return null;
  const undo = state.status === "applied" && state.canUndo;
  return (
    <IconButton
      className="compose-ai-proposal__apply"
      label={`${undo ? "回退" : "应用"}${label}`}
      disabled={busy}
      aria-busy={busy || undefined}
      onClick={() => onResolve(group, undo ? "undo" : "apply")}
    >
      {busy ? (
        <SpinnerGap size={15} weight="bold" />
      ) : undo ? (
        <ArrowCounterClockwise size={15} weight="bold" />
      ) : (
        <Check size={15} weight="bold" />
      )}
    </IconButton>
  );
}

function ProposalCard({ busyGroup, proposal, onResolve }) {
  const draft = proposal.draft || {};
  return (
    <div className="compose-ai-proposals" aria-label="AI 草稿修改提案">
      {proposal.headers?.changed ? (
        <section className="compose-ai-proposal" aria-label="邮件信息修改提案">
          <header>
            <strong>邮件信息</strong>
            <ProposalAction
              busy={busyGroup === "headers"}
              group="headers"
              label="邮件信息提案"
              proposal={proposal}
              onResolve={onResolve}
            />
          </header>
          <dl>
            <div><dt>收件人</dt><dd>{draft.to?.join("、") || "未填写"}</dd></div>
            <div><dt>抄送</dt><dd>{draft.cc?.join("、") || "未填写"}</dd></div>
            <div><dt>密送</dt><dd>{draft.bcc?.join("、") || "未填写"}</dd></div>
            <div><dt>主题</dt><dd>{draft.subject || "无主题"}</dd></div>
          </dl>
        </section>
      ) : null}
      {proposal.body?.changed ? (
        <section className="compose-ai-proposal" aria-label="正文与信纸修改提案">
          <header>
            <strong>正文与信纸</strong>
            <ProposalAction
              busy={busyGroup === "body"}
              group="body"
              label="正文与信纸提案"
              proposal={proposal}
              onResolve={onResolve}
            />
          </header>
          <pre>{draft.body_text || "（空正文）"}</pre>
          <small>
            信纸：{draft.format?.stationery === "lined"
              ? "横线纸"
              : draft.format?.stationery === "grid"
                ? "方格纸"
                : "无"}
            {draft.format?.send_stationery ? " · 随邮件发送" : " · 仅编辑时显示"}
          </small>
        </section>
      ) : null}
    </div>
  );
}

function ActivityTimeline({ activities = [] }) {
  if (!activities.length) return null;
  return (
    <ol className="compose-ai-activity-list" aria-label="Agent 执行过程">
      {activities.map((activity) => (
        <li
          key={activity.id}
          className="compose-ai-activity-step"
          data-kind={activity.kind}
          data-status={activity.status}
        >
          <span className="compose-ai-activity-step__icon" aria-hidden="true">
            {activity.status === "running" ? (
              <SpinnerGap size={13} weight="bold" />
            ) : activity.status === "completed" ? (
              <CheckCircle size={13} weight="fill" />
            ) : (
              <span>•</span>
            )}
          </span>
          <div>
            <strong>{activity.label}</strong>
            {activity.detail ? (
              <p className="compose-ai-activity-step__detail">{activity.detail}</p>
            ) : null}
          </div>
        </li>
      ))}
    </ol>
  );
}

function Conversation({
  busyProposal,
  onOpenExternalLink,
  onResolveProposal,
  session,
}) {
  const scrollRef = useRef(null);
  const followRef = useRef(true);
  const contentKey = session.messages
    .map((message) => {
      const activityKey = (message.activities || [])
        .map((activity) =>
          `${activity.id}:${activity.status}:${activity.detail?.length || 0}`,
        )
        .join(",");
      return `${message.id}:${message.content?.length || 0}:${message.status}:${activityKey}`;
    })
    .join("|");

  useEffect(() => {
    if (!followRef.current || !scrollRef.current) return;
    scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
  }, [contentKey]);

  return (
    <div
      ref={scrollRef}
      className="compose-ai-conversation vertical-scroll-surface"
      aria-live="polite"
      onScroll={(event) => {
        const element = event.currentTarget;
        followRef.current =
          element.scrollHeight - element.scrollTop - element.clientHeight < 36;
      }}
    >
      {session.messages.map((message) => (
        <article
          key={message.id}
          className="compose-ai-message"
          data-role={message.role}
        >
          {message.role === "assistant" ? (
            <span className="compose-ai-message__mark" aria-hidden="true">
              <Sparkle size={14} weight="fill" />
            </span>
          ) : null}
          <div className="compose-ai-message__content">
            {message.role === "assistant" ? (
              <ActivityTimeline activities={message.activities} />
            ) : null}
            {message.content ? (
              message.role === "assistant" ? (
                <SafeMarkdown onOpenExternalLink={onOpenExternalLink}>
                  {message.content}
                </SafeMarkdown>
              ) : (
                <p>{message.content}</p>
              )
            ) : null}
            {message.proposal ? (
              <ProposalCard
                proposal={message.proposal}
                busyGroup={busyProposal?.messageId === message.id
                  ? busyProposal.group
                  : null}
                onResolve={(group, action) =>
                  onResolveProposal(message.id, message.proposal, group, action)
                }
              />
            ) : null}
            {message.status === "stopped" ? (
              <small className="compose-ai-message__state">已停止</small>
            ) : null}
            {message.status === "failed" ? (
              <small className="compose-ai-message__state compose-ai-message__state--error">
                生成中断，可重新发送
              </small>
            ) : null}
          </div>
        </article>
      ))}
    </div>
  );
}

export function ComposeAiAssistant({
  aiDraft,
  currentDraft,
  disabled,
  hidden = false,
  onApplyDraft,
  onCollapse,
  onOpenDraft,
  onOpenExternalLink,
  readOnly = false,
  value,
}) {
  const inputRef = useRef(null);
  const latestValueRef = useRef(value);
  const streamingSessionIdRef = useRef(null);
  const [sessions, setSessions] = useState([]);
  const [activeSessionId, setActiveSessionId] = useState(null);
  const [mode, setMode] = useState("auto");
  const [input, setInput] = useState("");
  const [isLoadingSessions, setIsLoadingSessions] = useState(true);
  const [isLoadingActiveSession, setIsLoadingActiveSession] = useState(false);
  const [activeRequest, setActiveRequest] = useState(null);
  const [errorMessage, setErrorMessage] = useState("");
  const [busyProposal, setBusyProposal] = useState(null);
  const [modelCatalog, setModelCatalog] = useState({
    models: [],
    successfulProviderCount: 0,
    totalProviderCount: 0,
  });
  const [modelCatalogState, setModelCatalogState] = useState("loading");
  const [selectedModelValue, setSelectedModelValue] = useState("");
  const isSubmitting = Boolean(activeRequest);
  latestValueRef.current = value;
  const availableModeOptions = useMemo(
    () =>
      agentModeOptions.map((option) => ({
        ...option,
        disabled: readOnly && option.value !== "chat",
      })),
    [readOnly],
  );
  const activeSession = useMemo(
    () => sessions.find((session) => session.id === activeSessionId) || null,
    [activeSessionId, sessions],
  );
  const modelOptions = useMemo(
    () => modelCatalog.models.map((model) => ({
      value: `${model.providerInstanceId}\u001f${model.modelName}`,
      label: `${model.modelName} · ${model.providerName}`,
    })),
    [modelCatalog.models],
  );
  const selectedModel = useMemo(
    () => modelCatalog.models.find(
      (model) => `${model.providerInstanceId}\u001f${model.modelName}` === selectedModelValue,
    ) || null,
    [modelCatalog.models, selectedModelValue],
  );

  useEffect(() => {
    let cancelled = false;
    setIsLoadingSessions(true);
    const listSessions =
      typeof mailApi.listAiSessions === "function"
        ? mailApi.listAiSessions()
        : Promise.resolve([]);
    listSessions
      .then((items) => {
        if (!cancelled) setSessions(items);
      })
      .catch((error) => {
        if (!cancelled) {
          setErrorMessage(error?.message || "AI 会话读取没有完成");
        }
      })
      .finally(() => {
        if (!cancelled) setIsLoadingSessions(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    setModelCatalogState("loading");
    Promise.resolve()
      .then(() => mailApi.refreshAiModelCatalog())
      .then((catalog) => {
        if (cancelled) return;
        setModelCatalog(catalog);
        const defaultModel = catalog.models.find((model) => model.isDefault);
        const initial = defaultModel || catalog.models[0] || null;
        setSelectedModelValue(
          initial
            ? `${initial.providerInstanceId}\u001f${initial.modelName}`
            : "",
        );
        setModelCatalogState(catalog.models.length ? "ready" : "empty");
      })
      .catch(() => {
        if (cancelled) return;
        setModelCatalog({
          models: [],
          successfulProviderCount: 0,
          totalProviderCount: 0,
        });
        setSelectedModelValue("");
        setModelCatalogState("empty");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!currentDraft) return;
    setSessions((current) =>
      current.map((session) => ({
        ...session,
        drafts: session.drafts.map((draft) =>
          draft.id === currentDraft.id ? currentDraft : draft,
        ),
      })),
    );
  }, [currentDraft?.id, currentDraft?.subject]);

  useEffect(() => {
    if (readOnly && mode !== "chat") setMode("chat");
  }, [mode, readOnly]);

  const openSession = async (sessionId) => {
    if (isSubmitting) return;
    setActiveSessionId(sessionId);
    setErrorMessage("");
    const existing = sessions.find(
      (session) => session.id === sessionId && session.loaded,
    );
    if (existing) return;
    setIsLoadingActiveSession(true);
    try {
      const detail = await mailApi.getAiSession(sessionId);
      setSessions((current) =>
        current.map((session) => (session.id === detail.id ? detail : session)),
      );
    } catch (error) {
      setErrorMessage(error?.message || "AI 会话读取没有完成");
    } finally {
      setIsLoadingActiveSession(false);
    }
  };

  const submit = async () => {
    const request = input.trim();
    if (
      !request ||
      disabled ||
      isSubmitting ||
      isLoadingSessions ||
      isLoadingActiveSession ||
      !aiDraft ||
      !selectedModel
    ) return;
    setActiveRequest({ id: null });
    setErrorMessage("");
    try {
      const result = await mailApi.runAiTurn(
        {
          mode,
          instruction: request,
          session_id: activeSession?.id || null,
          provider_instance_id: selectedModel.providerInstanceId,
          model_name: selectedModel.modelName,
          draft_revision: createAiDraftRevision(),
          draft: { ...aiDraft, compose: value },
        },
        (event) => {
          if (event?.type === "started") {
            setActiveRequest({ id: event.request_id });
            if (event.session) {
              streamingSessionIdRef.current = event.session.id;
              setSessions((current) => [
                event.session,
                ...current.filter((session) => session.id !== event.session.id),
              ]);
              setActiveSessionId(event.session.id);
              setInput("");
            }
          } else if (event?.type === "thinking_started") {
            setSessions((current) =>
              updateStreamingAssistant(
                current,
                streamingSessionIdRef.current,
                (message) => ({
                  ...message,
                  activities: [
                    ...(message.activities || []),
                    {
                      id: event.activity_id,
                      kind: "thinking",
                      label: "正在思考…",
                      detail: "",
                      status: "running",
                      success: null,
                    },
                  ],
                }),
              ),
            );
          } else if (event?.type === "reasoning_delta") {
            setSessions((current) =>
              updateStreamingAssistant(
                current,
                streamingSessionIdRef.current,
                (message) => ({
                  ...message,
                  activities: (message.activities || []).map((activity) =>
                    activity.id === event.activity_id &&
                    activity.status === "running"
                      ? {
                          ...activity,
                          detail: `${activity.detail || ""}${event.delta || ""}`,
                        }
                      : activity,
                  ),
                }),
              ),
            );
          } else if (event?.type === "thinking_finished") {
            setSessions((current) =>
              updateStreamingAssistant(
                current,
                streamingSessionIdRef.current,
                (message) => ({
                  ...message,
                  activities: (message.activities || []).map((activity) =>
                    activity.id === event.activity_id
                      ? {
                          ...activity,
                          label: event.summary || "分析完成",
                          detail: "",
                          status: event.success ? "completed" : "failed",
                          success: Boolean(event.success),
                        }
                      : activity,
                  ),
                }),
              ),
            );
          } else if (event?.type === "tool_preparing") {
            setSessions((current) =>
              updateStreamingAssistant(
                current,
                streamingSessionIdRef.current,
                (message) => beginToolActivity(message, event, true),
              ),
            );
          } else if (event?.type === "tool_started") {
            setSessions((current) =>
              updateStreamingAssistant(
                current,
                streamingSessionIdRef.current,
                (message) => beginToolActivity(message, event),
              ),
            );
          } else if (event?.type === "tool_finished") {
            setSessions((current) =>
              updateStreamingAssistant(
                current,
                streamingSessionIdRef.current,
                (message) => ({
                  ...message,
                  activities: (message.activities || []).map((activity) =>
                    activity.id === event.activity_id
                      ? {
                          ...activity,
                          label: event.success
                            ? `已调用「${event.display_name || event.name}」工具`
                            : `「${event.display_name || event.name}」工具调用未完成`,
                          status: event.success ? "completed" : "failed",
                          success: Boolean(event.success),
                        }
                      : activity,
                  ),
                }),
              ),
            );
          } else if (event?.type === "content_reset") {
            setSessions((current) =>
              updateStreamingAssistant(
                current,
                streamingSessionIdRef.current,
                (message) => ({
                  ...message,
                  activities: (message.activities || []).map((activity) =>
                    activity.kind === "thinking" && activity.status === "running"
                      ? {
                          ...activity,
                          detail: activity.detail || message.content || "",
                        }
                      : activity,
                  ),
                  content: "",
                }),
              ),
            );
          } else if (event?.type === "content_delta") {
            setSessions((current) =>
              updateStreamingAssistant(
                current,
                streamingSessionIdRef.current,
                (message) => ({
                  ...message,
                  content: `${message.content || ""}${event.delta || ""}`,
                }),
              ),
            );
          } else if (event?.type === "failed") {
            setSessions((current) =>
              updateStreamingAssistant(
                current,
                streamingSessionIdRef.current,
                (message) => finishRunningActivities(message, "failed"),
              ),
            );
          } else if (event?.type === "stopped") {
            setSessions((current) =>
              updateStreamingAssistant(
                current,
                streamingSessionIdRef.current,
                (message) => finishRunningActivities(message, "stopped"),
              ),
            );
          }
        },
      );
      if (result.session) {
        setSessions((current) => [
          result.session,
          ...current.filter((session) => session.id !== result.session.id),
        ]);
        setActiveSessionId(result.session.id);
      }
    } catch (error) {
      setErrorMessage(error?.message || "AI 请求没有完成，请重试");
    } finally {
      setActiveRequest(null);
      streamingSessionIdRef.current = null;
    }
  };

  const stop = async () => {
    if (!activeRequest?.id) return;
    setSessions((current) =>
      updateStreamingAssistant(
        current,
        streamingSessionIdRef.current,
        (message) => ({
          ...message,
          activities: (message.activities || []).map((activity) =>
            activity.status === "running"
              ? { ...activity, label: "正在停止…" }
              : activity,
          ),
        }),
      ),
    );
    try {
      await mailApi.cancelAiTurn(activeRequest.id);
    } catch (error) {
      setErrorMessage(error?.message || "无法停止 AI 请求");
    }
  };

  const resolveProposal = async (messageId, proposal, group, action) => {
    if (!aiDraft || busyProposal) return;
    setBusyProposal({ messageId, group });
    setErrorMessage("");
    try {
      const result = await mailApi.resolveAiProposalGroup({
        proposal_id: proposal.id,
        group,
        action,
        draft: { ...aiDraft, compose: latestValueRef.current },
      });
      onApplyDraft((current) => mergeProposalGroup(current, result.draft, group));
      setSessions((current) =>
        current.map((session) => ({
          ...session,
          messages: (session.messages || []).map((message) =>
            message.id === messageId
              ? { ...message, proposal: result.proposal }
              : message,
          ),
        })),
      );
    } catch (error) {
      setErrorMessage(error?.message || "AI 草稿提案没有完成应用");
    } finally {
      setBusyProposal(null);
    }
  };

  return (
    <aside className="compose-ai-assistant" aria-label="AI 助理" hidden={hidden}>
      <header
        className="compose-ai-header"
        data-view={activeSession ? "conversation" : "sessions"}
      >
        <div className="compose-ai-header__actions">
          <IconButton label="收起 AI 助理" onClick={onCollapse}>
            <SidebarSimple size={18} />
          </IconButton>
          <IconButton label="AI 助理设置" disabled={isSubmitting}>
            <Gear size={18} />
          </IconButton>
          {activeSession ? (
            <IconButton
              className="compose-ai-header__back"
              label="返回会话列表"
              disabled={isSubmitting}
              onClick={() => setActiveSessionId(null)}
            >
              <ArrowLeft size={18} />
            </IconButton>
          ) : null}
        </div>
        <strong
          className="compose-ai-header__title"
          title={activeSession?.title || "AI 助理"}
        >
          {activeSession?.title || "AI 助理"}
        </strong>
      </header>

      {activeSession ? (
        <>
          <DraftPills drafts={activeSession.drafts} onOpenDraft={onOpenDraft} />
          <Conversation
            session={activeSession}
            busyProposal={busyProposal}
            onOpenExternalLink={onOpenExternalLink}
            onResolveProposal={(...args) => void resolveProposal(...args)}
          />
        </>
      ) : (
        <SessionList
          disabled={isSubmitting}
          sessions={sessions}
          onOpenSession={(id) => void openSession(id)}
        />
      )}

      {isLoadingSessions && !activeSession ? (
        <p className="compose-ai-status" role="status">正在读取会话…</p>
      ) : null}
      {isLoadingActiveSession ? (
        <p className="compose-ai-status" role="status">正在读取会话内容…</p>
      ) : null}
      {errorMessage ? (
        <p className="compose-ai-status compose-ai-status--error" role="status">
          {errorMessage}
        </p>
      ) : null}

      <div className="compose-ai-composer">
        <textarea
          ref={inputRef}
          aria-label="向 AI 助理发送消息"
          rows={3}
          value={input}
          disabled={
            disabled ||
            isLoadingSessions ||
            isLoadingActiveSession ||
            !aiDraft
          }
          placeholder={agentModePlaceholders[mode]}
          onChange={(event) => setInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !isSubmitting) {
              event.preventDefault();
              void submit();
            }
          }}
        />
        <div className="compose-ai-composer__actions">
          <ThemedSelect
            id="compose-agent-mode"
            label="选择 Agent 模式"
            value={mode}
            options={availableModeOptions}
            disabled={
              disabled ||
              isSubmitting ||
              isLoadingSessions ||
              isLoadingActiveSession ||
              !aiDraft
            }
            className="compose-ai-mode-select"
            menuPlacement="above"
            onValueChange={setMode}
          />
          <ThemedSelect
            id="compose-agent-model"
            label="选择 AI 模型"
            value={selectedModelValue}
            options={modelOptions}
            disabled={
              disabled
              || isSubmitting
              || isLoadingSessions
              || isLoadingActiveSession
              || modelCatalogState !== "ready"
              || !aiDraft
            }
            placeholder={
              modelCatalogState === "loading"
                ? "正在获取模型…"
                : "没有可用模型"
            }
            className="compose-ai-model-select"
            menuPlacement="above"
            preferredMaxHeight={204}
            onValueChange={setSelectedModelValue}
          />
          <IconButton
            className="compose-ai-send"
            label={isSubmitting ? "停止 AI 助理" : "发送给 AI 助理"}
            disabled={
              disabled ||
              isLoadingSessions ||
              isLoadingActiveSession ||
              (!isSubmitting && !input.trim()) ||
              (!isSubmitting && !selectedModel) ||
              (isSubmitting && !activeRequest?.id) ||
              !aiDraft
            }
            aria-busy={isSubmitting}
            onClick={() => (isSubmitting ? void stop() : void submit())}
          >
            {isSubmitting ? (
              <Stop size={16} weight="fill" />
            ) : (
              <PaperPlaneRight size={17} weight="fill" />
            )}
          </IconButton>
        </div>
      </div>
    </aside>
  );
}
