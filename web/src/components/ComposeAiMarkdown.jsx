import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

export default function ComposeAiMarkdown({ children, onOpenExternalLink }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        a({ href, children: label }) {
          return (
            <a
              href={href}
              onClick={(event) => {
                event.preventDefault();
                if (href) void onOpenExternalLink(href);
              }}
            >
              {label}
            </a>
          );
        },
        img({ alt }) {
          return <span>{alt ? `[图片：${alt}]` : "[图片]"}</span>;
        },
      }}
    >
      {children || ""}
    </ReactMarkdown>
  );
}
