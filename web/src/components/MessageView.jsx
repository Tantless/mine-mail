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
import { HtmlMessageBody } from "./HtmlMessageBody.jsx";
import { NativeHtmlMessageBody } from "./NativeHtmlMessageBody.jsx";
import { SegmentedMessageBody } from "./SegmentedMessageBody.jsx";
import { EditableProfileAvatar, ProfileAvatar } from "./ProfileAvatar.jsx";
import { formatFullDate, senderLabel } from "../utils/formatters.js";

function fileSizeLabel(bytes) {
  if (!bytes) return "附件";
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function MessageView({
  message,
  isLoading,
  error,
  onRetry,
  onClose,
  onReply,
  onForward,
  onRetryDelivery,
  isRetryingDelivery = false,
  canRetryDelivery = false,
  onPrevious,
  onNext,
  canPrevious,
  canNext,
  remoteImageMode = "automatic",
  onOpenExternalLink,
  senderAvatar,
  onSetSenderAvatar,
  onRemoveSenderAvatar,
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
  const body = message.body_fetched
    ? message.body_text || "这封邮件没有纯文本正文。"
    : message.preview || "这封邮件没有纯文本正文。";
  const bodyRenderMode =
    message.body_render_mode || (message.body_html ? "isolated_html" : "plain");
  const hasBodySegments = Boolean(message.body_segments?.length);

  return (
    <section className="reader-panel reader-panel--message" aria-label="邮件阅读区">
      <header className="reader-toolbar">
        <div className="reader-toolbar__group">
          <IconButton label="返回邮件列表" className="reader-back" onClick={onClose}>
            <ArrowLeft size={20} />
          </IconButton>
          <IconButton label="归档（尚未实现）" disabled>
            <Archive size={19} />
          </IconButton>
          <IconButton label="删除（尚未实现）" disabled>
            <Trash size={19} />
          </IconButton>
          <span className="toolbar-divider" />
          <IconButton label="标记为未读（尚未实现）" disabled>
            <EnvelopeOpen size={19} />
          </IconButton>
          <IconButton label="更多操作（尚未实现）" disabled>
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
          <p className="eyebrow">{message.kind === "outbox" ? "OUTBOX" : "INBOX"}</p>
          <h2>{message.subject || "（无主题）"}</h2>

          <div className="sender-card">
            {message.kind !== "outbox" && message.sender?.email && onSetSenderAvatar ? (
              <EditableProfileAvatar
                className="sender-avatar-picker"
                avatarClassName="sender-card__avatar"
                email={message.sender.email}
                label={sender}
                customSrc={senderAvatar}
                onSelectFile={onSetSenderAvatar}
                onRemove={onRemoveSenderAvatar}
              />
            ) : (
              <ProfileAvatar
                className="sender-card__avatar"
                email={message.sender?.email}
                label={sender}
                customSrc={senderAvatar}
              />
            )}
            <div className="sender-card__identity">
              <strong>{sender}</strong>
              <span>{message.sender?.email}</span>
              <button type="button" className="recipient-toggle" disabled>
                {message.kind === "outbox" ? "查看收件人" : "发送给我"} <CaretDown size={12} />
              </button>
            </div>
            <time dateTime={message.sent_at}>{formatFullDate(message.sent_at)}</time>
          </div>
        </div>

        {message.kind === "outbox" && message.outbox?.status !== "sent" ? (
          <aside className="message-error delivery-status" role="status">
            <strong>投递状态：{message.delivery_status_label}</strong>
            {message.outbox?.last_error ? (
              <span>说明：{message.outbox.last_error}</span>
            ) : null}
            {message.outbox?.status === "delivery_unknown" ? (
              <span>请先到邮箱服务器确认投递结果，不要立即重复发送。</span>
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
          aria-busy={isLoading}
        >
          {isLoading ? (
            <div className="body-skeleton" aria-label="正在加载正文">
              <span />
              <span />
              <span />
              <span />
            </div>
          ) : error ? (
            <div className="message-error" role="alert">
              <strong>正文加载失败</strong>
              <span>{error}</span>
              <button type="button" className="secondary-button" onClick={onRetry}>
                重新加载
              </button>
            </div>
          ) : hasBodySegments ? (
            <SegmentedMessageBody
              key={`${message.mailbox || "INBOX"}:${message.uid}`}
              message={message}
              body={body}
              bodyRenderMode={bodyRenderMode}
              remoteImageMode={remoteImageMode}
              onOpenExternalLink={onOpenExternalLink}
            />
          ) : bodyRenderMode === "native_html" && message.body_html ? (
            <NativeHtmlMessageBody
              key={message.uid}
              html={message.body_html}
              hasRemoteImages={message.has_remote_images}
              remoteImageMode={remoteImageMode}
              onOpenLink={onOpenExternalLink}
            />
          ) : bodyRenderMode === "isolated_html" && message.body_html ? (
            <HtmlMessageBody
              key={message.uid}
              cacheKey={`${message.mailbox || "INBOX"}:${message.uid}`}
              html={message.body_html}
              hasRemoteImages={message.has_remote_images}
              remoteImageMode={remoteImageMode}
              title={message.subject}
              onOpenLink={onOpenExternalLink}
            />
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
              {message.attachment_names.map((name, index) => (
                <button
                  className="attachment-card"
                  type="button"
                  key={`${index}-${name}`}
                  aria-label={`${name}（附件下载尚未实现）`}
                  title="附件下载尚未实现"
                  disabled
                >
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

        {message.kind !== "outbox" ? (
          <div className="message-actions message-actions--mail">
            <button
              type="button"
              className="message-action-button message-action-button--reply"
              onClick={onReply}
            >
              <ArrowBendUpLeft size={18} />
              回复
            </button>
            <IconButton
              label="转发"
              className="message-forward-button"
              onClick={onForward}
            >
              <ArrowBendUpRight size={18} />
            </IconButton>
          </div>
        ) : message.outbox?.status === "retryable" ? (
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
        ) : null}
      </div>
    </section>
  );
}
