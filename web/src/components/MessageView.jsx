import {
  Archive,
  ArrowBendUpLeft,
  ArrowBendUpRight,
  ArrowLeft,
  CaretDown,
  CaretLeft,
  CaretRight,
  DotsThree,
  DownloadSimple,
  EnvelopeOpen,
  FilePdf,
  Paperclip,
  Trash,
} from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";
import { formatFullDate, initials, senderLabel } from "../utils/formatters.js";

function fileSizeLabel(bytes) {
  if (!bytes) return "附件";
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function MessageView({
  message,
  isLoading,
  onClose,
  onReply,
  onForward,
  onPrevious,
  onNext,
  canPrevious,
  canNext,
}) {
  if (!message) {
    return (
      <section className="reader-panel reader-panel--empty" aria-label="邮件阅读区">
        <div className="reader-empty__art" aria-hidden="true">
          <EnvelopeOpen size={34} weight="duotone" />
        </div>
        <p>选择一封邮件开始阅读</p>
        <small>背景会在这里多留一点呼吸</small>
      </section>
    );
  }

  const sender = senderLabel(message);
  const body = message.body_text || message.preview || "这封邮件没有纯文本正文。";

  return (
    <section className="reader-panel" aria-label="邮件阅读区">
      <header className="reader-toolbar">
        <div className="reader-toolbar__group">
          <IconButton label="返回邮件列表" className="reader-back" onClick={onClose}>
            <ArrowLeft size={20} />
          </IconButton>
          <IconButton label="归档">
            <Archive size={19} />
          </IconButton>
          <IconButton label="删除">
            <Trash size={19} />
          </IconButton>
          <span className="toolbar-divider" />
          <IconButton label="标记为未读">
            <EnvelopeOpen size={19} />
          </IconButton>
          <IconButton label="更多操作">
            <DotsThree size={22} weight="bold" />
          </IconButton>
        </div>
        <div className="reader-toolbar__group">
          <IconButton label="上一封" onClick={onPrevious} disabled={!canPrevious}>
            <CaretLeft size={18} />
          </IconButton>
          <IconButton label="下一封" onClick={onNext} disabled={!canNext}>
            <CaretRight size={18} />
          </IconButton>
        </div>
      </header>

      <div className="reader-scroll">
        <div className="message-header">
          <p className="eyebrow">INBOX</p>
          <h2>{message.subject || "（无主题）"}</h2>

          <div className="sender-card">
            <span className="sender-card__avatar" aria-hidden="true">
              {initials(sender)}
            </span>
            <div className="sender-card__identity">
              <strong>{sender}</strong>
              <span>{message.sender?.email}</span>
              <button type="button" className="recipient-toggle">
                发送给我 <CaretDown size={12} />
              </button>
            </div>
            <time dateTime={message.sent_at}>{formatFullDate(message.sent_at)}</time>
          </div>
        </div>

        <article className="message-body" aria-busy={isLoading}>
          {isLoading ? (
            <div className="body-skeleton" aria-label="正在加载正文">
              <span />
              <span />
              <span />
              <span />
            </div>
          ) : (
            body.split(/\n{2,}/).map((paragraph, index) => (
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

        {message.attachment_names?.length ? (
          <section className="attachments" aria-label="附件">
            <h3>
              <Paperclip size={17} />
              {message.attachment_names.length} 个附件
            </h3>
            <div className="attachment-grid">
              {message.attachment_names.map((name) => (
                <button className="attachment-card" type="button" key={name}>
                  <span className="attachment-card__icon">
                    <FilePdf size={25} weight="duotone" />
                  </span>
                  <span className="attachment-card__copy">
                    <strong>{name}</strong>
                    <small>{fileSizeLabel(message.size_bytes)}</small>
                  </span>
                  <DownloadSimple size={18} />
                </button>
              ))}
            </div>
          </section>
        ) : null}

        <div className="message-actions">
          <button type="button" className="secondary-button" onClick={onReply}>
            <ArrowBendUpLeft size={18} />
            回复
          </button>
          <button type="button" className="secondary-button" onClick={onForward}>
            <ArrowBendUpRight size={18} />
            转发
          </button>
        </div>
      </div>
    </section>
  );
}
