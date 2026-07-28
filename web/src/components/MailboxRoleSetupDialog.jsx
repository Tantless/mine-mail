import { Archive, Trash } from "@phosphor-icons/react";
import { ConsequentialConfirmDialog } from "./ConsequentialConfirmDialog.jsx";

const rolePresentation = {
  archive: {
    mailboxName: "Archive",
    title: "创建 Archive 邮箱？",
    description:
      "Mine Mail 将创建名为 Archive 的固定邮箱，并在服务器确认它可用后再执行归档。",
    confirmLabel: "创建 Archive",
    icon: <Archive size={23} weight="duotone" />,
  },
  trash: {
    mailboxName: "Trash",
    title: "创建 Trash 邮箱？",
    description:
      "Mine Mail 将创建名为 Trash 的固定邮箱，并在服务器确认它可用后再把当前邮件移入垃圾箱。",
    confirmLabel: "创建 Trash",
    icon: <Trash size={23} weight="duotone" />,
  },
};

export function MailboxRoleSetupDialog({
  role,
  isPending = false,
  errorMessage = null,
  returnFocusRef = null,
  onCancel,
  onConfirm,
}) {
  const presentation = rolePresentation[role];
  if (!presentation) return null;

  return (
    <ConsequentialConfirmDialog
      open
      title={presentation.title}
      description={presentation.description}
      icon={presentation.icon}
      cancelLabel="取消"
      closeLabel={`取消创建 ${presentation.mailboxName} 邮箱`}
      confirmLabel={presentation.confirmLabel}
      pendingLabel={`正在创建 ${presentation.mailboxName}…`}
      isPending={isPending}
      errorMessage={errorMessage}
      returnFocusRef={returnFocusRef}
      onCancel={onCancel}
      onConfirm={() => onConfirm?.(role)}
    >
      <div className="consequential-confirm-dialog__fact">
        <small>固定邮箱名称</small>
        <strong>{presentation.mailboxName}</strong>
      </div>
      <p className="consequential-confirm-dialog__note">
        仅在首次创建缺失邮箱时需要此确认。取消不会创建邮箱，也不会移动或改变当前邮件。
      </p>
    </ConsequentialConfirmDialog>
  );
}
