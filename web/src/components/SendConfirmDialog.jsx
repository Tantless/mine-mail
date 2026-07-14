import { PaperPlaneTilt, ShieldCheck, X } from "@phosphor-icons/react";
import { IconButton } from "./IconButton.jsx";

export function SendConfirmDialog({ request, isSending, onCancel, onConfirm }) {
  if (!request) return null;
  const recipients = [...request.to, ...request.cc, ...request.bcc];

  return (
    <div className="confirm-layer">
      <section
        className="confirm-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="send-confirm-title"
      >
        <header>
          <span className="confirm-dialog__icon">
            <ShieldCheck size={23} weight="duotone" />
          </span>
          <IconButton label="取消发送" onClick={onCancel} disabled={isSending}>
            <X size={18} />
          </IconButton>
        </header>
        <h2 id="send-confirm-title">确认发送这封邮件？</h2>
        <p>邮件将通过已连接的 163 邮箱发送，请最后确认收件人。</p>
        <div className="recipient-review">
          {recipients.map((recipient) => (
            <span key={recipient}>{recipient}</span>
          ))}
        </div>
        <div className="confirm-dialog__subject">
          <small>主题</small>
          <strong>{request.subject || "（无主题）"}</strong>
        </div>
        <footer>
          <button type="button" className="secondary-button" onClick={onCancel} disabled={isSending}>
            返回修改
          </button>
          <button type="button" className="send-button" onClick={onConfirm} disabled={isSending}>
            <PaperPlaneTilt size={18} weight="fill" />
            {isSending ? "正在发送…" : "确认发送"}
          </button>
        </footer>
      </section>
    </div>
  );
}
