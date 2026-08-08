import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowCounterClockwise,
  ArrowLeft,
  CaretDown,
  CaretUp,
  Gear,
  MagicWand,
  PaperPlaneRight,
  SidebarSimple,
  Sparkle,
} from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { ThemedSelect } from "./ThemedSelect.jsx";

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

function normalizeBody(body) {
  return String(body || "")
    .replace(/\r\n/g, "\n")
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function mockOptimizeDraft(value, instruction) {
  const prompt = instruction.trim();
  const concise = /简洁|精简|缩短|简短/.test(prompt);
  let body = normalizeBody(value.body_text);

  if (concise) {
    body = body
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean)
      .join("\n\n");
  } else if (body) {
    if (!/^(您好|你好|尊敬的)/.test(body)) body = `您好，\n\n${body}`;
    if (!/(谢谢|感谢|祝好|此致)[！。!]?$/u.test(body)) {
      body = `${body}\n\n感谢您的时间，期待您的回复。`;
    }
  }

  return {
    subject: String(value.subject || "").replace(/\s+/g, " ").trim(),
    body_text: body,
    format: {
      stationery: "none",
      send_stationery: false,
      ...(value.format || {}),
      body_html: null,
    },
  };
}

function editableDraftFingerprint(value) {
  return JSON.stringify({
    subject: value?.subject || "",
    body_text: value?.body_text || "",
    body_html: value?.format?.body_html || null,
  });
}

function mockGeneratedDraft(request, current) {
  const brief = request.trim().replace(/\s+/g, " ");
  const shortBrief = brief.length > 30 ? `${brief.slice(0, 30)}…` : brief;
  return {
    ...current,
    subject:
      String(current.subject || "").trim() ||
      `关于${shortBrief || "相关事项"}的确认`,
    body_text: [
      "您好，",
      "",
      `想就${brief || "相关事项"}与您确认一下。烦请您在方便时告知具体安排，如有需要我补充的信息，也请随时告诉我。`,
      "",
      "感谢您的时间，期待您的回复。",
    ].join("\n"),
    format: {
      stationery: "none",
      send_stationery: false,
      ...(current.format || {}),
      body_html: null,
    },
  };
}

function createInitialSessions(currentDraft) {
  return [
    {
      id: "session-delivery-follow-up",
      title: "确认项目交付时间",
      summary: "整理一封更清晰、语气更自然的项目跟进邮件",
      lastActive: "刚刚",
      drafts: currentDraft ? [currentDraft] : [],
      messages: [
        {
          id: "message-delivery-user",
          role: "user",
          content: "帮我把这封项目交付确认邮件写得更清楚一些。",
        },
        {
          id: "message-delivery-agent",
          role: "assistant",
          content: "已读取当前草稿，并整理了主题和正文。你可以继续告诉我希望调整的语气。",
        },
      ],
    },
    {
      id: "session-meeting-reply",
      title: "回复下周会议邀请",
      summary: "确认参会时间，并询问会议议程",
      lastActive: "2 小时前",
      drafts: [],
      messages: [
        {
          id: "message-meeting-user",
          role: "user",
          content: "我会参加下周的会议，帮我想一下还需要确认什么。",
        },
        {
          id: "message-meeting-agent",
          role: "assistant",
          content: "可以确认具体时间、会议形式、参会人员和需要提前准备的材料。",
        },
      ],
    },
    {
      id: "session-customer-thanks",
      title: "客户感谢信",
      summary: "讨论感谢信的内容和正式程度",
      lastActive: "昨天",
      drafts: [],
      messages: [
        {
          id: "message-thanks-user",
          role: "user",
          content: "感谢客户这次的配合，语气不要太客套。",
        },
        {
          id: "message-thanks-agent",
          role: "assistant",
          content: "可以直接说明对方在哪些环节提供了帮助，再用一句简短的后续期待收尾。",
        },
      ],
    },
  ];
}

function sessionTitleFromInput(value) {
  const normalized = value.trim().replace(/\s+/g, " ");
  if (normalized.length <= 18) return normalized;
  return `${normalized.slice(0, 18)}…`;
}

function shouldAutoWrite(value) {
  return /写|生成|回复|填入|改写|起草|优化|润色/.test(value);
}

function agentReplyFor(mode, didWrite) {
  if (didWrite) return "已生成并填入当前草稿。你可以继续提出修改要求。";
  if (mode === "chat") {
    return "我会保持只读。当前草稿的主题和正文已经可以作为后续讨论的上下文。";
  }
  return "我已经结合当前草稿理解了你的要求，可以继续补充对象、语气或篇幅。";
}

export function ComposeOptimizeControl({ disabled, onApply, value }) {
  const rootRef = useRef(null);
  const previousDraftRef = useRef(null);
  const appliedDraftFingerprintRef = useRef(null);
  const awaitingAppliedValueRef = useRef(false);
  const [promptOpen, setPromptOpen] = useState(false);
  const [instruction, setInstruction] = useState("");
  const [status, setStatus] = useState("idle");
  const hasContent = Boolean(
    String(value.subject || "").trim() || String(value.body_text || "").trim(),
  );

  useEffect(() => {
    if (!promptOpen) return undefined;
    const closeOnOutsidePointer = (event) => {
      if (!rootRef.current?.contains(event.target)) setPromptOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [promptOpen]);

  useEffect(() => {
    if (status !== "applied" || !appliedDraftFingerprintRef.current) return;
    const currentFingerprint = editableDraftFingerprint(value);
    if (awaitingAppliedValueRef.current) {
      if (currentFingerprint === appliedDraftFingerprintRef.current) {
        awaitingAppliedValueRef.current = false;
      }
      return;
    }
    if (currentFingerprint !== appliedDraftFingerprintRef.current) {
      previousDraftRef.current = null;
      appliedDraftFingerprintRef.current = null;
      setStatus("idle");
    }
  }, [status, value]);

  const optimize = () => {
    if (disabled || !hasContent) return;
    previousDraftRef.current = {
      subject: value.subject,
      body_text: value.body_text,
      format: value.format,
    };
    const optimized = mockOptimizeDraft(value, instruction);
    appliedDraftFingerprintRef.current = editableDraftFingerprint(optimized);
    awaitingAppliedValueRef.current = true;
    onApply((current) => ({ ...current, ...optimized }));
    setPromptOpen(false);
    setStatus("applied");
  };

  const undo = () => {
    const previous = previousDraftRef.current;
    if (!previous) return;
    onApply((current) => ({ ...current, ...previous }));
    previousDraftRef.current = null;
    appliedDraftFingerprintRef.current = null;
    awaitingAppliedValueRef.current = false;
    setStatus("undone");
  };

  return (
    <div ref={rootRef} className="compose-optimize-control">
      <div className="compose-optimize-split" data-prompt-open={promptOpen}>
        <IconButton
          className="compose-optimize-split__action"
          label="优化当前邮件"
          disabled={disabled || !hasContent}
          onClick={optimize}
        >
          <MagicWand size={17} weight="fill" />
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
                optimize();
              }
            }}
          />
          <small>填写后，点击魔棒开始优化</small>
        </div>
      ) : null}

      {status === "applied" ? (
        <button className="compose-optimize-undo" type="button" onClick={undo}>
          <ArrowCounterClockwise size={14} weight="bold" />
          撤销优化
        </button>
      ) : null}
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
  currentDraft,
  disabled,
  onApplyDraft,
  onCollapse,
  onOpenDraft,
  readOnly = false,
  value,
}) {
  const inputRef = useRef(null);
  const [sessions, setSessions] = useState(() => createInitialSessions(currentDraft));
  const [activeSessionId, setActiveSessionId] = useState(null);
  const [mode, setMode] = useState("auto");
  const [input, setInput] = useState("");
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

  const addDraftBinding = (drafts) => {
    if (!currentDraft || drafts.some((draft) => draft.id === currentDraft.id)) return drafts;
    return [...drafts, currentDraft];
  };

  const submit = () => {
    const request = input.trim();
    if (!request || disabled) return;
    const didWrite =
      !readOnly &&
      (mode === "generate" || (mode === "auto" && shouldAutoWrite(request)));
    if (didWrite) onApplyDraft((current) => mockGeneratedDraft(request, current));

    const userMessage = {
      id: `user-${Date.now()}`,
      role: "user",
      content: request,
    };
    const assistantMessage = {
      id: `assistant-${Date.now()}`,
      role: "assistant",
      content: agentReplyFor(mode, didWrite),
    };

    if (!activeSession) {
      const id = `session-${Date.now()}`;
      const session = {
        id,
        title: sessionTitleFromInput(request),
        summary: request,
        lastActive: "刚刚",
        drafts: addDraftBinding([]),
        messages: [userMessage, assistantMessage],
      };
      setSessions((current) => [session, ...current]);
      setActiveSessionId(id);
    } else {
      setSessions((current) =>
        current.map((session) =>
          session.id === activeSession.id
            ? {
                ...session,
                summary: request,
                lastActive: "刚刚",
                drafts: addDraftBinding(session.drafts),
                messages: [...session.messages, userMessage, assistantMessage],
              }
            : session,
        ),
      );
    }
    setInput("");
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
        <SessionList sessions={sessions} onOpenSession={setActiveSessionId} />
      )}

      <div className="compose-ai-composer">
        <textarea
          ref={inputRef}
          aria-label="向 AI 助理发送消息"
          rows={3}
          value={input}
          disabled={disabled}
          placeholder={agentModePlaceholders[mode]}
          onChange={(event) => setInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              submit();
            }
          }}
        />
        <div className="compose-ai-composer__actions">
          <ThemedSelect
            id="compose-agent-mode"
            label="选择 Agent 模式"
            value={mode}
            options={availableModeOptions}
            disabled={disabled}
            className="compose-ai-mode-select"
            onValueChange={setMode}
          />
          <IconButton
            className="compose-ai-send"
            label="发送给 AI 助理"
            disabled={disabled || !input.trim()}
            onClick={submit}
          >
            <PaperPlaneRight size={17} weight="fill" />
          </IconButton>
        </div>
      </div>
    </aside>
  );
}
