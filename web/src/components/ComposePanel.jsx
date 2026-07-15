import { useEffect, useMemo, useState } from "react";
import {
  ArrowsOutSimple,
  Minus,
  Paperclip,
  PaperPlaneTilt,
  Trash,
  X,
} from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { splitAddresses } from "../utils/formatters.js";

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
}) {
  const [showCopies, setShowCopies] = useState(
    Boolean(value.cc?.length || value.bcc?.length),
  );

  useEffect(() => {
    if (value.cc?.length || value.bcc?.length) setShowCopies(true);
  }, [value.bcc, value.cc]);

  const canSend = useMemo(() => {
    const recipients = [...value.to, ...value.cc, ...value.bcc];
    return recipients.length > 0 && recipients.every(Boolean);
  }, [value.bcc, value.cc, value.to]);
  const isBusy = locked || isSending;
  const controlsDisabled = isBusy || readOnly;

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

  const saveCopy = {
    idle: draftId ? "已保存" : "新草稿",
    dirty: "有未保存更改",
    saving: "正在保存…",
    syncing: "正在同步…",
    saved: "已保存",
    readonly: "只读",
    error: "保存失败",
  }[saveStatus] || "新草稿";

  return (
    <div className="compose-layer" role="presentation">
      <section
        className="compose-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="compose-title"
      >
        <header className="compose-header">
          <div>
            <span className="compose-header__indicator" />
            <h2 id="compose-title">
              {readOnly ? "查看草稿" : draftId ? "编辑草稿" : "新邮件"}
            </h2>
            <span className="compose-save-state" data-state={saveStatus} aria-live="polite">
              {saveCopy}
            </span>
          </div>
          <div className="compose-header__actions">
            <IconButton label="最小化写信窗口（尚未实现）" disabled>
              <Minus size={17} />
            </IconButton>
            <IconButton label="展开写信窗口（尚未实现）" disabled>
              <ArrowsOutSimple size={17} />
            </IconButton>
            <IconButton label="关闭写信窗口" onClick={onClose} disabled={isBusy}>
              <X size={18} />
            </IconButton>
          </div>
        </header>

        {readOnly ? (
          <div className="compose-unsupported-notice" role="status">
            含当前不支持的HTML/附件，未作修改
          </div>
        ) : null}

        <div className="compose-fields">
          <label className="compose-field">
            <span>收件人</span>
            <input
              autoFocus
              disabled={controlsDisabled}
              aria-label="收件人"
              value={value.to.join(", ")}
              onChange={(event) => setAddressField("to", event.target.value)}
              placeholder="name@example.com"
            />
            {!showCopies ? (
              <button
                type="button"
                onClick={() => setShowCopies(true)}
                disabled={controlsDisabled}
              >
                抄送 / 密送
              </button>
            ) : null}
          </label>
          {showCopies ? (
            <>
              <label className="compose-field">
                <span>抄送</span>
                <input
                  aria-label="抄送"
                  disabled={controlsDisabled}
                  value={value.cc.join(", ")}
                  onChange={(event) => setAddressField("cc", event.target.value)}
                />
              </label>
              <label className="compose-field">
                <span>密送</span>
                <input
                  aria-label="密送"
                  disabled={controlsDisabled}
                  value={value.bcc.join(", ")}
                  onChange={(event) => setAddressField("bcc", event.target.value)}
                />
              </label>
            </>
          ) : null}
          <label className="compose-field compose-field--subject">
            <span>主题</span>
            <input
              aria-label="主题"
              disabled={controlsDisabled}
              value={value.subject}
              onChange={(event) =>
                onChange((current) => ({ ...current, subject: event.target.value }))
              }
              placeholder="写一个简洁的主题"
            />
          </label>
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
            <button
              type="button"
              className="draft-button"
              onClick={onSaveDraft}
              disabled={controlsDisabled || saveStatus === "saving" || saveStatus === "syncing"}
            >
              {saveStatus === "saving" || saveStatus === "syncing"
                ? saveCopy
                : "保存并关闭"}
            </button>
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
      </section>
    </div>
  );
}
