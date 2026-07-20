import { CaretRight, EnvelopeOpen, Quotes } from "@phosphor-icons/react";
import { useState } from "react";
import { HtmlMessageBody } from "./HtmlMessageBody.jsx";
import { NativeHtmlMessageBody } from "./NativeHtmlMessageBody.jsx";

function PlainContent({ text }) {
  return text.split(/\n{2,}/).map((paragraph, index) => {
    const lines = paragraph.split("\n");
    return (
      <p key={`${index}-${paragraph.slice(0, 12)}`}>
        {lines.map((line, lineIndex) => (
          <span key={`${lineIndex}-${line.slice(0, 8)}`}>
            {line}
            {lineIndex < lines.length - 1 ? <br /> : null}
          </span>
        ))}
      </p>
    );
  });
}

function SegmentContent({
  segment,
  message,
  index,
  remoteImageMode,
  onOpenExternalLink,
}) {
  if (segment.render_mode === "native_html") {
    return (
      <NativeHtmlMessageBody
        html={segment.content}
        hasRemoteImages={message.has_remote_images}
        remoteImageMode={remoteImageMode}
        onOpenLink={onOpenExternalLink}
      />
    );
  }
  if (segment.render_mode === "isolated_html") {
    return (
      <HtmlMessageBody
        cacheKey={`${message.mailbox || "INBOX"}:${message.uid}:segment:${index}`}
        html={segment.content}
        hasRemoteImages={message.has_remote_images}
        remoteImageMode={remoteImageMode}
        title={`${message.subject || "邮件"}引用内容`}
        onOpenLink={onOpenExternalLink}
      />
    );
  }
  return <PlainContent text={segment.content} />;
}

function OriginalBody({ message, body, bodyRenderMode, remoteImageMode, onOpenExternalLink }) {
  if (bodyRenderMode === "native_html" && message.body_html) {
    return (
      <NativeHtmlMessageBody
        html={message.body_html}
        hasRemoteImages={message.has_remote_images}
        remoteImageMode={remoteImageMode}
        onOpenLink={onOpenExternalLink}
      />
    );
  }
  if (bodyRenderMode === "isolated_html" && message.body_html) {
    return (
      <HtmlMessageBody
        cacheKey={`${message.mailbox || "INBOX"}:${message.uid}:original`}
        html={message.body_html}
        hasRemoteImages={message.has_remote_images}
        remoteImageMode={remoteImageMode}
        title={message.subject}
        onOpenLink={onOpenExternalLink}
      />
    );
  }
  return <PlainContent text={body} />;
}

function QuotedSegment({
  segment,
  quoteNumber,
  message,
  sourceIndex,
  remoteImageMode,
  onOpenExternalLink,
  resolveReferencedMessage,
  onOpenReferencedMessage,
}) {
  const metadata = segment.quote_metadata || {};
  const subject = metadata.subject || `引用邮件 ${quoteNumber}`;
  const hasRoute = metadata.sender || metadata.recipient;
  const navigationTarget = segment.navigation_target;
  const destination = navigationTarget
    ? resolveReferencedMessage?.(navigationTarget)
    : null;
  const destinationLabel = destination?.folder === "sent" ? "已发送" : "收件箱";

  return (
    <details
      className={`quoted-message${destination ? " quoted-message--navigable" : ""}`}
      open={segment.confidence === "medium" ? true : undefined}
    >
      <summary>
        <span className="quoted-message__icon" aria-hidden="true">
          <Quotes size={16} weight="fill" />
        </span>
        <span className="quoted-message__metadata">
          <strong className="quoted-message__subject" title={subject}>
            {subject}
          </strong>
          {hasRoute ? (
            <span className="quoted-message__route">
              <span title={metadata.sender || undefined}>
                {metadata.sender || "未知发件人"}
              </span>
              <span className="quoted-message__route-arrow" aria-hidden="true">
                →
              </span>
              <span title={metadata.recipient || undefined}>
                {metadata.recipient || "未知收件人"}
              </span>
            </span>
          ) : null}
          {metadata.sent_at ? (
            <time className="quoted-message__time">{metadata.sent_at}</time>
          ) : null}
        </span>
        {destination ? (
          <button
            type="button"
            className="quoted-message__open-source"
            aria-label={`在${destinationLabel}中打开原邮件：${subject}`}
            title={`在${destinationLabel}中打开原邮件`}
            onClick={(event) => {
              event.preventDefault();
              event.stopPropagation();
              onOpenReferencedMessage?.(navigationTarget);
            }}
          >
            <EnvelopeOpen size={17} weight="regular" aria-hidden="true" />
          </button>
        ) : null}
        <CaretRight className="quoted-message__caret" size={16} weight="bold" />
      </summary>
      <div className="quoted-message__content">
        <SegmentContent
          segment={segment}
          message={message}
          index={sourceIndex}
          remoteImageMode={remoteImageMode}
          onOpenExternalLink={onOpenExternalLink}
        />
      </div>
    </details>
  );
}

export function SegmentedMessageBody({
  message,
  body,
  bodyRenderMode,
  remoteImageMode,
  onOpenExternalLink,
  resolveReferencedMessage,
  onOpenReferencedMessage,
}) {
  const [showOriginal, setShowOriginal] = useState(false);
  const segments = message.body_segments || [];
  let quoteNumber = 0;
  const displaySegments = segments.map((segment, sourceIndex) => ({
    ...segment,
    sourceIndex,
    quoteNumber: segment.kind === "quoted" ? (quoteNumber += 1) : null,
  }));

  return (
    <div className="segmented-message-body">
      <div className="segmented-message-body__toolbar">
        <button
          type="button"
          className="segmented-message-body__original-toggle"
          aria-pressed={showOriginal}
          onClick={() => setShowOriginal((current) => !current)}
        >
          {showOriginal ? "返回分段阅读" : "按原始格式查看"}
        </button>
      </div>

      {showOriginal ? (
        <OriginalBody
          message={message}
          body={body}
          bodyRenderMode={bodyRenderMode}
          remoteImageMode={remoteImageMode}
          onOpenExternalLink={onOpenExternalLink}
        />
      ) : (
        <div className="message-segments">
          {displaySegments.map((segment, index) =>
            segment.kind === "quoted" ? (
              <QuotedSegment
                key={`${index}-${segment.content.slice(0, 16)}`}
                segment={segment}
                quoteNumber={segment.quoteNumber}
                message={message}
                sourceIndex={segment.sourceIndex}
                remoteImageMode={remoteImageMode}
                onOpenExternalLink={onOpenExternalLink}
                resolveReferencedMessage={resolveReferencedMessage}
                onOpenReferencedMessage={onOpenReferencedMessage}
              />
            ) : (
              <section
                className="message-segment message-segment--authored"
                key={`${index}-${segment.content.slice(0, 16)}`}
              >
                <SegmentContent
                  segment={segment}
                  message={message}
                  index={index}
                  remoteImageMode={remoteImageMode}
                  onOpenExternalLink={onOpenExternalLink}
                />
              </section>
            ),
          )}
        </div>
      )}
    </div>
  );
}
