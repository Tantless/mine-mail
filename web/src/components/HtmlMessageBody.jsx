import { useCallback, useEffect, useMemo, useRef, useState } from "react";

const minimumFrameHeight = 220;
const maximumFrameHeight = 250_000;
const rememberedHeightLimit = 32;
const rememberedHeights = new Map();

function rememberHeight(key, height) {
  if (!key) return;
  rememberedHeights.delete(key);
  rememberedHeights.set(key, height);
  while (rememberedHeights.size > rememberedHeightLimit) {
    rememberedHeights.delete(rememberedHeights.keys().next().value);
  }
}

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
      html, body {
        margin: 0;
        padding: 0;
        background: transparent;
        overflow: hidden !important;
        scrollbar-width: none !important;
      }
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
  <body data-mine-mail-document>${fragment}
    <style>html, body { overflow: hidden !important; scrollbar-width: none !important; }</style>
  </body>
</html>`;
}

export function HtmlMessageBody({
  html,
  hasRemoteImages = false,
  remoteImageMode = "automatic",
  cacheKey,
  title,
  onOpenLink,
}) {
  const frameRef = useRef(null);
  const cleanupRef = useRef(() => {});
  const configuredDocumentRef = useRef(null);
  const initialHeight = rememberedHeights.get(cacheKey) || minimumFrameHeight;
  const [frameHeight, setFrameHeight] = useState(initialHeight);
  const [isFrameReady, setIsFrameReady] = useState(
    rememberedHeights.has(cacheKey),
  );
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

  useEffect(() => {
    configuredDocumentRef.current = null;
    cleanupRef.current();
    const rememberedHeight = rememberedHeights.get(cacheKey);
    setFrameHeight(rememberedHeight || minimumFrameHeight);
    setIsFrameReady(Boolean(rememberedHeight));
  }, [cacheKey, source]);

  const configureFrame = useCallback(() => {
    const frame = frameRef.current;
    const document = frame?.contentDocument;
    if (
      !frame ||
      !document?.body?.hasAttribute("data-mine-mail-document")
    ) {
      return false;
    }
    if (configuredDocumentRef.current === document) return true;

    cleanupRef.current();
    configuredDocumentRef.current = document;
    document.documentElement.style.setProperty("overflow", "hidden", "important");
    document.body.style.setProperty("overflow", "hidden", "important");

    const updateHeight = () => {
      const height = Math.max(
        document.documentElement?.scrollHeight || 0,
        document.body?.scrollHeight || 0,
        minimumFrameHeight,
      );
      const nextHeight = Math.min(height, maximumFrameHeight);
      rememberHeight(cacheKey, nextHeight);
      setFrameHeight(nextHeight);
      setIsFrameReady(true);
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
    return true;
  }, [cacheKey, onOpenLink]);

  // iframe load waits for remote images. Attach sizing as soon as the srcdoc
  // DOM exists so the app owns the only scrollbar from the first visible
  // frame; ResizeObserver then follows late image/font layout changes.
  useEffect(() => {
    let animationFrame = 0;
    let attempts = 0;
    const attachWhenReady = () => {
      if (configureFrame() || attempts >= 30) return;
      attempts += 1;
      animationFrame = window.requestAnimationFrame(attachWhenReady);
    };
    attachWhenReady();
    return () => window.cancelAnimationFrame(animationFrame);
  }, [configureFrame, source]);

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
        data-ready={isFrameReady}
        title={`${title || "邮件"} HTML 正文`}
        sandbox="allow-same-origin"
        scrolling="no"
        srcDoc={source}
        style={{ height: `${frameHeight}px` }}
        onLoad={configureFrame}
      />
    </div>
  );
}
