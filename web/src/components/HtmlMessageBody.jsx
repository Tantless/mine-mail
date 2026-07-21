import { useCallback, useEffect, useMemo, useRef, useState } from "react";

const minimumFrameHeight = 220;
const maximumFrameHeight = 250_000;
const rememberedHeightLimit = 32;
const rememberedHeights = new Map();
const horizontalOverflowTolerance = 1;
const maximumDeclaredLayoutWidth = 2_000;
const layoutElements =
  "table, tbody, tr, td, th, div, section, main, article, img";
const declaredWidthElements = "table, td, th, div, section, main, article";
const measuredContentWidths = new WeakMap();

function rememberHeight(key, height) {
  if (!key) return;
  rememberedHeights.delete(key);
  rememberedHeights.set(key, height);
  while (rememberedHeights.size > rememberedHeightLimit) {
    rememberedHeights.delete(rememberedHeights.keys().next().value);
  }
}

function isVisuallyHidden(document, element) {
  if (
    element.hidden ||
    element.getAttribute?.("aria-hidden") === "true" ||
    element.style?.display === "none"
  ) {
    return true;
  }
  const style = document.defaultView?.getComputedStyle?.(element);
  return style?.display === "none" || style?.visibility === "hidden";
}

function parseDeclaredPixelWidth(value) {
  const match = String(value || "").match(/^\s*(\d+(?:\.\d+)?)\s*(?:px)?\s*$/i);
  if (!match) return 0;
  const width = Number(match[1]);
  return Number.isFinite(width) && width <= maximumDeclaredLayoutWidth
    ? width
    : 0;
}

function measureDeclaredLayoutWidth(document, viewportWidth) {
  let width = viewportWidth;
  for (const element of document.querySelectorAll?.(declaredWidthElements) || []) {
    if (isVisuallyHidden(document, element)) continue;
    width = Math.max(
      width,
      parseDeclaredPixelWidth(element.getAttribute?.("width")),
      parseDeclaredPixelWidth(element.style?.width),
      parseDeclaredPixelWidth(element.style?.minWidth),
    );
  }
  return width;
}

function measureRenderedHorizontalExtent(document, viewportWidth) {
  const root = document.documentElement;
  const body = document.body;
  const bodyRect = body.getBoundingClientRect?.();
  const origin = Number.isFinite(bodyRect?.left) ? bodyRect.left : 0;
  let extent = Math.max(
    root.scrollWidth || 0,
    body.scrollWidth || 0,
    viewportWidth,
  );

  if (extent <= viewportWidth + horizontalOverflowTolerance) {
    for (const element of document.querySelectorAll?.(layoutElements) || []) {
      if (isVisuallyHidden(document, element)) continue;
      const rect = element.getBoundingClientRect?.();
      const relativeLeft = Number.isFinite(rect?.left) ? rect.left - origin : 0;
      const relativeRight = Number.isFinite(rect?.right)
        ? rect.right - origin
        : 0;
      extent = Math.max(extent, relativeRight);

      const clientWidth = element.clientWidth || 0;
      const scrollWidth = element.scrollWidth || 0;
      const overflowX =
        document.defaultView?.getComputedStyle?.(element)?.overflowX || "";
      if (
        clientWidth > 0 &&
        scrollWidth > clientWidth + horizontalOverflowTolerance &&
        overflowX !== "auto" &&
        overflowX !== "scroll"
      ) {
        extent = Math.max(extent, Math.max(0, relativeLeft) + scrollWidth);
      }
    }
  }

  return extent;
}

function measureEmailContentWidth(document, viewportWidth) {
  const cached = measuredContentWidths.get(document);
  if (cached?.viewportWidth === viewportWidth) return cached.contentWidth;

  const renderedExtent = measureRenderedHorizontalExtent(document, viewportWidth);
  const contentWidth =
    renderedExtent > viewportWidth + horizontalOverflowTolerance
      ? Math.max(
          renderedExtent,
          measureDeclaredLayoutWidth(document, viewportWidth),
        )
      : viewportWidth;
  measuredContentWidths.set(document, { viewportWidth, contentWidth });
  return contentWidth;
}

export function fitEmailDocumentToWidth(document, availableWidth) {
  const root = document?.documentElement;
  const body = document?.body;
  const viewportWidth = Number(availableWidth);
  if (!root || !body || !Number.isFinite(viewportWidth) || viewportWidth <= 0) {
    return { height: minimumFrameHeight, scale: 1 };
  }

  // Measure the sender document without a previous fit applied. The generated
  // iframe owns this body element, so these inline properties are Mine Mail's.
  body.style.removeProperty("width");
  body.style.removeProperty("transform");
  body.style.removeProperty("transform-origin");
  delete body.dataset.mineMailWidthFit;

  const contentWidth = measureEmailContentWidth(document, viewportWidth);
  const shouldFit =
    contentWidth > viewportWidth + horizontalOverflowTolerance;
  const scale = shouldFit ? viewportWidth / contentWidth : 1;

  if (shouldFit) {
    // Giving the body its intrinsic width before transforming preserves the
    // sender's table/image composition while making the complete document fit.
    body.style.setProperty("width", `${contentWidth}px`, "important");
    body.style.setProperty("transform-origin", "top left", "important");
    body.style.setProperty("transform", `scale(${scale})`, "important");
    body.dataset.mineMailWidthFit = "true";
  }

  const contentHeight = Math.max(
    body.scrollHeight || 0,
    body.offsetHeight || 0,
    shouldFit ? 0 : root.scrollHeight || 0,
  );
  return {
    height: Math.max(minimumFrameHeight, Math.ceil(contentHeight * scale)),
    scale,
  };
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
      const { height } = fitEmailDocumentToWidth(
        document,
        frame.clientWidth || document.documentElement?.clientWidth || 0,
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
