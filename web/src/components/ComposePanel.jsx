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
  initialValue,
  isSending,
  onClose,
  onSaveDraft,
  onRequestSend,
  sendShortcut,
}) {
  const [form, setForm] = useState(initialValue);
  const [showCopies, setShowCopies] = useState(
    Boolean(initialValue.cc?.length || initialValue.bcc?.length),
  );

  useEffect(() => {
    setForm(initialValue);
  }, [initialValue]);

  const canSend = useMemo(() => {
    const recipients = [...form.to, ...form.cc, ...form.bcc];
    return recipients.length > 0 && recipients.every(Boolean);
  }, [form.bcc, form.cc, form.to]);

  useEffect(() => {
    const onKeyDown = (event) => {
      if (event.key === "Escape" && !isSending) onClose();
      if (
        (event.metaKey || event.ctrlKey) &&
        event.key === "Enter" &&
        canSend &&
        !isSending
      ) {
        event.preventDefault();
        onRequestSend(form);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [canSend, form, isSending, onClose, onRequestSend]);

  const setAddressField = (field, value) => {
    setForm((current) => ({ ...current, [field]: splitAddresses(value) }));
  };

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
            <h2 id="compose-title">新邮件</h2>
          </div>
          <div className="compose-header__actions">
            <IconButton label="最小化写信窗口">
              <Minus size={17} />
            </IconButton>
            <IconButton label="展开写信窗口">
              <ArrowsOutSimple size={17} />
            </IconButton>
            <IconButton label="关闭写信窗口" onClick={onClose}>
              <X size={18} />
            </IconButton>
          </div>
        </header>

        <div className="compose-fields">
          <label className="compose-field">
            <span>收件人</span>
            <input
              autoFocus
              aria-label="收件人"
              value={form.to.join(", ")}
              onChange={(event) => setAddressField("to", event.target.value)}
              placeholder="name@example.com"
            />
            {!showCopies ? (
              <button type="button" onClick={() => setShowCopies(true)}>
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
                  value={form.cc.join(", ")}
                  onChange={(event) => setAddressField("cc", event.target.value)}
                />
              </label>
              <label className="compose-field">
                <span>密送</span>
                <input
                  aria-label="密送"
                  value={form.bcc.join(", ")}
                  onChange={(event) => setAddressField("bcc", event.target.value)}
                />
              </label>
            </>
          ) : null}
          <label className="compose-field compose-field--subject">
            <span>主题</span>
            <input
              aria-label="主题"
              value={form.subject}
              onChange={(event) =>
                setForm((current) => ({ ...current, subject: event.target.value }))
              }
              placeholder="写一个简洁的主题"
            />
          </label>
        </div>

        <textarea
          className="compose-body"
          value={form.body_text}
          onChange={(event) =>
            setForm((current) => ({ ...current, body_text: event.target.value }))
          }
          placeholder="开始写邮件…"
          aria-label="邮件正文"
        />

        <footer className="compose-footer">
          <div className="compose-footer__left">
            <button
              className="send-button"
              type="button"
              aria-label="发送邮件"
              disabled={!canSend || isSending}
              onClick={() => onRequestSend(form)}
            >
              <PaperPlaneTilt size={18} weight="fill" />
              {isSending ? "正在发送…" : "发送"}
              <kbd>{sendShortcut}</kbd>
            </button>
            <IconButton label="添加附件">
              <Paperclip size={19} />
            </IconButton>
          </div>
          <div className="compose-footer__right">
            <button type="button" className="draft-button" onClick={() => onSaveDraft(form)}>
              保存草稿
            </button>
            <IconButton label="丢弃草稿" tone="danger" onClick={onClose}>
              <Trash size={18} />
            </IconButton>
          </div>
        </footer>
      </section>
    </div>
  );
}
