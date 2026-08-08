import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  ArrowCounterClockwise,
  ArrowLeft,
  CaretDown,
  CaretUp,
  CheckCircle,
  Gear,
  MagicWand,
  PaperPlaneRight,
  SidebarSimple,
  Sparkle,
  SpinnerGap,
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

export function ComposeOptimizeControl({ aiDraft, disabled, onApply, value }) {
  const rootRef = useRef(null);
  const optimizeButtonRef = useRef(null);
  const latestValueRef = useRef(value);
  const [promptOpen, setPromptOpen] = useState(false);
  const [instruction, setInstruction] = useState("");
  const [status, setStatus] = useState("idle");
  const [errorMessage, setErrorMessage] = useState("");
  const [reviewResult, setReviewResult] = useState(null);
  const [reviewOpen, setReviewOpen] = useState(false);
  const [leftAnnotations, setLeftAnnotations] = useState([]);
  const [rightAnnotations, setRightAnnotations] = useState([]);
  const [leftEdited, setLeftEdited] = useState(false);
  const [rightEdited, setRightEdited] = useState(false);
  const [confirmation, setConfirmation] = useState(null);
  const [undoBackup, setUndoBackup] = useState(null);
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

function SessionList({ onOpenSession, sessions }) {
  return (
    <div className="compose-ai-session-list vertical-scroll-surface">
      <h3>会话</h3>
      <div className="compose-ai-session-list__rows">
        {sessions.map((session) => (
          <button
            key={session.id}
            className="compose-ai-session-row"
            type="button"
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

function Conversation({ session }) {
  return (
    <div className="compose-ai-conversation vertical-scroll-surface" aria-live="polite">
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
          <p>{message.content}</p>
        </article>
      ))}
    </div>
  );
}

export function ComposeAiAssistant({
  aiDraft,
  currentDraft,
  disabled,
  onApplyDraft,
  onCollapse,
  onOpenDraft,
  readOnly = false,
  value,
}) {
  const inputRef = useRef(null);
  const latestValueRef = useRef(value);
  const [sessions, setSessions] = useState([]);
  const [activeSessionId, setActiveSessionId] = useState(null);
  const [mode, setMode] = useState("auto");
  const [input, setInput] = useState("");
  const [isLoadingSessions, setIsLoadingSessions] = useState(true);
  const [isLoadingActiveSession, setIsLoadingActiveSession] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [activityLabel, setActivityLabel] = useState("");
  const [errorMessage, setErrorMessage] = useState("");
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

  useEffect(() => {
    let cancelled = false;
    setIsLoadingSessions(true);
    mailApi
      .listAiSessions()
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
      !aiDraft
    ) return;
    const baseFingerprint = editableDraftFingerprint(value);
    setIsSubmitting(true);
    setActivityLabel("正在连接 AI…");
    setErrorMessage("");
    try {
      const result = await mailApi.runAiTurn(
        {
          mode,
          instruction: request,
          session_id: activeSession?.id || null,
          draft_revision: createAiDraftRevision(),
          draft: { ...aiDraft, compose: value },
        },
        (event) => {
          if (event?.type === "tool_started") {
            setActivityLabel("正在读取或更新草稿…");
          } else if (event?.type === "content_delta") {
            setActivityLabel("正在整理回答…");
          }
        },
      );
      if (result.draft) {
        if (editableDraftFingerprint(latestValueRef.current) !== baseFingerprint) {
          recordPatchOutcome(result, aiDraft, "rejected");
          setErrorMessage("草稿在 AI 处理过程中发生了变化，生成结果未应用");
        } else {
          onApplyDraft((current) => applyAiDraft(current, result.draft));
          recordPatchOutcome(result, aiDraft, "applied");
        }
      }
      if (result.session) {
        setSessions((current) => [
          result.session,
          ...current.filter((session) => session.id !== result.session.id),
        ]);
        setActiveSessionId(result.session.id);
      }
      setInput("");
    } catch (error) {
      setErrorMessage(error?.message || "AI 请求没有完成，请重试");
    } finally {
      setIsSubmitting(false);
      setActivityLabel("");
    }
  };

  return (
    <aside className="compose-ai-assistant" aria-label="AI 助理">
      <header className="compose-ai-header">
        <div className="compose-ai-header__actions">
          <IconButton label="收起 AI 助理" onClick={onCollapse}>
            <SidebarSimple size={18} />
          </IconButton>
          <IconButton label="AI 助理设置">
            <Gear size={18} />
          </IconButton>
          {activeSession ? (
            <IconButton
              className="compose-ai-header__back"
              label="返回会话列表"
              onClick={() => setActiveSessionId(null)}
            >
              <ArrowLeft size={18} />
            </IconButton>
          ) : null}
        </div>
        <strong title={activeSession?.title || "AI 助理"}>
          {activeSession?.title || "AI 助理"}
        </strong>
      </header>

      {activeSession ? (
        <>
          <DraftPills drafts={activeSession.drafts} onOpenDraft={onOpenDraft} />
          <Conversation session={activeSession} />
        </>
      ) : (
        <SessionList sessions={sessions} onOpenSession={(id) => void openSession(id)} />
      )}

      {isLoadingSessions && !activeSession ? (
        <p className="compose-ai-status" role="status">正在读取会话…</p>
      ) : null}
      {isLoadingActiveSession ? (
        <p className="compose-ai-status" role="status">正在读取会话内容…</p>
      ) : null}
      {activityLabel ? (
        <p className="compose-ai-status" role="status">{activityLabel}</p>
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
            isSubmitting ||
            isLoadingSessions ||
            isLoadingActiveSession ||
            !aiDraft
          }
          placeholder={agentModePlaceholders[mode]}
          onChange={(event) => setInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
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
            onValueChange={setMode}
          />
          <IconButton
            className="compose-ai-send"
            label="发送给 AI 助理"
            disabled={
              disabled ||
              isSubmitting ||
              isLoadingSessions ||
              isLoadingActiveSession ||
              !input.trim() ||
              !aiDraft
            }
            aria-busy={isSubmitting}
            onClick={() => void submit()}
          >
            <PaperPlaneRight size={17} weight="fill" />
          </IconButton>
        </div>
      </div>
    </aside>
  );
}
