import { useCallback, useEffect, useMemo, useRef, useState } from "react";

const minimumFrameHeight = 220;
const maximumFrameHeight = 50_000;

export function buildEmailDocument(fragment, allowRemoteImages = false) {
  const imageSources = allowRemoteImages
    ? "data: blob: http: https:"
    : "data: blob:";
  const policy = [
    "default-src 'none'",
    `img-src ${imageSources}`,
    "style-src 'unsafe-inline'",
    "font-src data:",
    "media-src 'none'",
    "connect-src 'none'",
    "frame-src 'none'",
    "object-src 'none'",
    "script-src 'none'",
    "form-action 'none'",
    "base-uri 'none'",
  ].join("; ");

  return `<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <meta http-equiv="Content-Security-Policy" content="${policy}">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
      :root { color-scheme: light; }
      html, body { margin: 0; padding: 0; background: transparent; }
      body {
        min-width: 0;
        color: #272327;
        font-family: Arial, "Microsoft YaHei", "PingFang SC", sans-serif;
        overflow-wrap: anywhere;
      }
      img { max-width: 100%; height: auto; }
      ${allowRemoteImages ? "" : `
      img[src^="http://"], img[src^="https://"], img[src^="//"] {
        visibility: hidden !important;
      }`}
      table { max-width: 100%; }
      pre { max-width: 100%; overflow: auto; white-space: pre-wrap; }
    </style>
  </head>
  <body>${fragment}</body>
</html>`;
}

export function HtmlMessageBody({
  html,
  hasRemoteImages = false,
  remoteImageMode = "automatic",
  title,
  onOpenLink,
}) {
  const frameRef = useRef(null);
  const cleanupRef = useRef(() => {});
  const [remotePermissionFor, setRemotePermissionFor] = useState(null);
  const normalizedMode = ["automatic", "ask", "blocked"].includes(remoteImageMode)
    ? remoteImageMode
    : "automatic";
  const allowRemoteImages =
    normalizedMode === "automatic" ||
    (normalizedMode === "ask" && remotePermissionFor === html);
  const source = useMemo(
    () => buildEmailDocument(html, allowRemoteImages),
    [allowRemoteImages, html],
  );

  useEffect(() => () => cleanupRef.current(), []);

  const configureFrame = useCallback(() => {
    cleanupRef.current();
    const frame = frameRef.current;
    const document = frame?.contentDocument;
    if (!frame || !document) return;

    const updateHeight = () => {
      const height = Math.max(
        document.documentElement?.scrollHeight || 0,
        document.body?.scrollHeight || 0,
        minimumFrameHeight,
      );
      frame.style.height = `${Math.min(height, maximumFrameHeight)}px`;
    };
    const handleClick = (event) => {
      const anchor = event.target?.closest?.("a[href]");
      if (!anchor) return;
      event.preventDefault();
      event.stopPropagation();
      onOpenLink?.(anchor.getAttribute("href"));
    };

    document.addEventListener("click", handleClick);
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(updateHeight);
    if (document.documentElement) observer?.observe(document.documentElement);
    if (document.body) observer?.observe(document.body);
    frame.contentWindow?.addEventListener("resize", updateHeight);
    updateHeight();

    cleanupRef.current = () => {
      observer?.disconnect();
      document.removeEventListener("click", handleClick);
      frame.contentWindow?.removeEventListener("resize", updateHeight);
    };
  }, [onOpenLink]);

  return (
    <div className="html-message">
      {hasRemoteImages && normalizedMode !== "automatic" ? (
        <div className="html-message__remote-notice" role="note">
          <span>
            {normalizedMode === "blocked"
              ? "已根据设置阻止远程图片"
              : allowRemoteImages
              ? "已允许这封邮件加载远程图片"
              : "为保护隐私，远程图片已被阻止"}
          </span>
          {normalizedMode === "ask" ? (
            <button
              type="button"
              className="secondary-button"
              onClick={() =>
                setRemotePermissionFor((current) =>
                  current === html ? null : html,
                )
              }
            >
              {allowRemoteImages ? "隐藏远程图片" : "加载远程图片"}
            </button>
          ) : null}
        </div>
      ) : null}
      <iframe
        ref={frameRef}
        className="html-message__frame"
        title={`${title || "邮件"} HTML 正文`}
        sandbox="allow-same-origin"
        srcDoc={source}
        onLoad={configureFrame}
      />
    </div>
  );
}
