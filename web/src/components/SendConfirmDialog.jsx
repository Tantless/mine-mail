import { PaperPlaneTilt, ShieldCheck, X } from "@phosphor-icons/react";
import { useRef } from "react";
import {
  ConfirmDialogStatus,
  useConfirmDialogFocus,
} from "./ConfirmDialogPrimitives.jsx";
import { IconButton } from "./IconButton.jsx";

export function SendConfirmDialog({
  request,
  isSending,
  returnFocusRef = null,
  onCancel,
  onConfirm,
}) {
  const cancelRef = useRef(null);
  const {
    dialogRef,
    onBackdropPointerDown,
    onDialogKeyDown,
  } = useConfirmDialogFocus({
    open: Boolean(request),
    isPending: isSending,
    initialFocusRef: cancelRef,
    returnFocusRef,
    onCancel,
  });

  if (!request) return null;
  const recipientGroups = [
    { id: "to", label: "收件人", recipients: request.to || [] },
    { id: "cc", label: "抄送", recipients: request.cc || [] },
    { id: "bcc", label: "密送", recipients: request.bcc || [] },
  ].filter((group) => group.recipients.length);

  return (
    <div
      className="confirm-layer"
      data-pending={isSending || undefined}
      onPointerDown={onBackdropPointerDown}
    >
      <section
        ref={dialogRef}
        className="confirm-dialog"
        role="alertdialog"
        tabIndex={-1}
        aria-modal="true"
        aria-busy={isSending || undefined}
        aria-labelledby="send-confirm-title"
        aria-describedby="send-confirm-description"
        onKeyDown={onDialogKeyDown}
      >
        <header>
          <span className="confirm-dialog__icon" aria-hidden="true">
            <ShieldCheck size={23} weight="duotone" />
          </span>
          <IconButton label="取消发送" onClick={onCancel} disabled={isSending}>
            <X size={18} />
          </IconButton>
        </header>
        <h2 id="send-confirm-title">确认发送这封邮件？</h2>
        <p id="send-confirm-description">
          邮件将通过已连接的邮箱账户发送，请最后确认所有收件人。
        </p>
        <div className="recipient-review">
          {recipientGroups.map((group) => (
            <section className="recipient-review__group" key={group.id}>
              <strong>{group.label}</strong>
              <div>
                {group.recipients.map((recipient, index) => (
                  <span key={`${group.id}-${index}-${recipient}`}>{recipient}</span>
                ))}
              </div>
            </section>
          ))}
        </div>
        <div className="confirm-dialog__subject">
          <small>主题</small>
          <strong>{request.subject || "（无主题）"}</strong>
        </div>
        <ConfirmDialogStatus>
          {isSending ? "正在发送邮件…" : null}
        </ConfirmDialogStatus>
        <footer>
          <button
            ref={cancelRef}
            type="button"
            className="secondary-button"
            onClick={onCancel}
            disabled={isSending}
          >
            返回修改
          </button>
          <button
            type="button"
            className="send-button"
            onClick={onConfirm}
            disabled={isSending}
          >
            <PaperPlaneTilt size={18} weight="fill" />
            {isSending ? "正在发送…" : "确认发送"}
          </button>
        </footer>
      </section>
    </div>
  );
}
