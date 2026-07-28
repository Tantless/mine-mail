import { Archive, Trash } from "@phosphor-icons/react";
import { ConsequentialConfirmDialog } from "./ConsequentialConfirmDialog.jsx";

const rolePresentation = {
  archive: {
    mailboxName: "Archive",
    title: "设置归档文件夹？",
    description:
      "Mine Mail 将在当前邮箱账户中创建名为 Archive 的服务器文件夹。不会创建新邮箱地址，也不会删除邮件。",
    confirmLabel: "创建归档文件夹",
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
  continueAction = false,
  onCancel,
  onConfirm,
}) {
  const presentation = rolePresentation[role];
  if (!presentation) return null;
  const title =
    role === "archive" && continueAction
      ? "创建归档文件夹并归档这封邮件？"
      : presentation.title;
  const description =
    role === "archive" && continueAction
      ? "Mine Mail 将在当前邮箱账户中创建名为 Archive 的服务器文件夹，确认可用后继续归档这封邮件。不会创建新邮箱地址，也不会删除邮件。"
      : presentation.description;
  const confirmLabel =
    role === "archive" && continueAction
      ? "创建并归档"
      : presentation.confirmLabel;

  return (
    <ConsequentialConfirmDialog
      open
      title={title}
      description={description}
      icon={presentation.icon}
      cancelLabel="取消"
      closeLabel={`取消创建 ${presentation.mailboxName} 文件夹`}
      confirmLabel={confirmLabel}
      pendingLabel={`正在创建 ${presentation.mailboxName}…`}
      isPending={isPending}
      errorMessage={errorMessage}
      returnFocusRef={returnFocusRef}
      onCancel={onCancel}
      onConfirm={() => onConfirm?.(role)}
    >
      <div className="consequential-confirm-dialog__fact">
        <small>固定文件夹名称</small>
        <strong>{presentation.mailboxName}</strong>
      </div>
      <p className="consequential-confirm-dialog__note">
        仅在首次创建缺失文件夹时需要此确认。取消不会创建文件夹，也不会移动或改变当前邮件。
      </p>
    </ConsequentialConfirmDialog>
  );
}
