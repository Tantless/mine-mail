import { CaretRight, Quotes } from "@phosphor-icons/react";
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

export function buildSegmentTree(segments) {
  const roots = [];
  const quoteStack = [];

  segments.forEach((segment, sourceIndex) => {
    const node = { ...segment, sourceIndex, children: [] };
    if (segment.kind !== "quoted") {
      roots.push(node);
      quoteStack.length = 0;
      return;
    }

    const parsedDepth = Number(segment.quote_depth);
    const depth = Number.isFinite(parsedDepth)
      ? Math.max(1, Math.trunc(parsedDepth))
      : 1;
    while (quoteStack.length >= depth) {
      quoteStack.pop();
    }

    const parent = depth > 1 ? quoteStack[depth - 2] : null;
    if (parent) {
      parent.children.push(node);
    } else {
      roots.push(node);
    }
    quoteStack[depth - 1] = node;
    quoteStack.length = depth;
  });

  return roots;
}

function QuotedSegment({
  node,
  message,
  path,
  remoteImageMode,
  onOpenExternalLink,
}) {
  return (
    <details
      className="quoted-message"
      defaultOpen={node.confidence === "medium"}
    >
      <summary>
        <span className="quoted-message__icon" aria-hidden="true">
          <Quotes size={16} weight="fill" />
        </span>
        <span>
          <strong>引用的原邮件</strong>
          <small>
            {node.confidence === "high" ? "点击展开" : "已展开供你确认"}
          </small>
        </span>
        <CaretRight className="quoted-message__caret" size={16} weight="bold" />
      </summary>
      <div className="quoted-message__content">
        <SegmentContent
          segment={node}
          message={message}
          index={node.sourceIndex}
          remoteImageMode={remoteImageMode}
          onOpenExternalLink={onOpenExternalLink}
        />
        {node.children.length > 0 ? (
          <div className="quoted-message__children">
            {node.children.map((child, childIndex) => (
              <QuotedSegment
                key={`${path}.${childIndex}-${child.content.slice(0, 16)}`}
                node={child}
                message={message}
                path={`${path}.${childIndex}`}
                remoteImageMode={remoteImageMode}
                onOpenExternalLink={onOpenExternalLink}
              />
            ))}
          </div>
        ) : null}
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
}) {
  const [showOriginal, setShowOriginal] = useState(false);
  const segments = message.body_segments || [];
  const segmentTree = buildSegmentTree(segments);

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
          {segmentTree.map((segment, index) =>
            segment.kind === "quoted" ? (
              <QuotedSegment
                key={`${index}-${segment.content.slice(0, 16)}`}
                node={segment}
                message={message}
                path={`${index}`}
                remoteImageMode={remoteImageMode}
                onOpenExternalLink={onOpenExternalLink}
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
