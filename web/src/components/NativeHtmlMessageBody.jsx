import { useMemo, useState } from "react";

function isRemoteImageSource(value) {
  return /^\s*https?:\/\//i.test(value || "");
}

export function prepareNativeHtml(html, allowRemoteImages) {
  const template = document.createElement("template");
  template.innerHTML = html;

  if (!allowRemoteImages) {
    template.content.querySelectorAll("img[src]").forEach((image) => {
      if (!isRemoteImageSource(image.getAttribute("src"))) return;
      image.removeAttribute("src");
      image.setAttribute("data-mine-mail-image-blocked", "true");
    });
  }

  return template.innerHTML;
}

export function NativeHtmlMessageBody({
  html,
  hasRemoteImages = false,
  remoteImageMode = "automatic",
  onOpenLink,
}) {
  const [remotePermissionFor, setRemotePermissionFor] = useState(null);
  const normalizedMode = ["automatic", "ask", "blocked"].includes(remoteImageMode)
    ? remoteImageMode
    : "automatic";
  const allowRemoteImages =
    normalizedMode === "automatic" ||
    (normalizedMode === "ask" && remotePermissionFor === html);
  const renderedHtml = useMemo(
    () => prepareNativeHtml(html, allowRemoteImages),
    [allowRemoteImages, html],
  );

  const handleClick = (event) => {
    const anchor = event.target?.closest?.("a[href]");
    if (!anchor) return;
    event.preventDefault();
    event.stopPropagation();
    onOpenLink?.(anchor.getAttribute("href"));
  };

  return (
    <div className="native-html-message">
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
      <div
        className="native-html-message__content"
        onClick={handleClick}
        dangerouslySetInnerHTML={{ __html: renderedHtml }}
      />
    </div>
  );
}
