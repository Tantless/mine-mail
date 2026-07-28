import { Trash } from "@phosphor-icons/react";
import { ConsequentialConfirmDialog } from "./ConsequentialConfirmDialog.jsx";

export function PermanentDeleteDialog({
  open,
  subject = "",
  isPending = false,
  errorMessage = null,
  returnFocusRef = null,
  onCancel,
  onConfirm,
}) {
  return (
    <ConsequentialConfirmDialog
      open={open}
      title="永久删除这封邮件？"
      description="这封邮件将从垃圾箱永久删除，且无法恢复。此操作每次都需要确认。"
      icon={<Trash size={23} weight="duotone" />}
      tone="danger"
      cancelLabel="取消"
      closeLabel="取消永久删除"
      confirmLabel="永久删除"
      pendingLabel="正在永久删除…"
      isPending={isPending}
      errorMessage={errorMessage}
      returnFocusRef={returnFocusRef}
      onCancel={onCancel}
      onConfirm={onConfirm}
    >
      <div className="confirm-dialog__subject">
        <small>邮件主题</small>
        <strong>{subject?.trim() || "（无主题）"}</strong>
      </div>
      <p className="consequential-confirm-dialog__note">
        取消后，邮件会继续保留在垃圾箱中。
      </p>
    </ConsequentialConfirmDialog>
  );
}
