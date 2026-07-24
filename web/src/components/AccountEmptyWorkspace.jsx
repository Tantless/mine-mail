import { EnvelopeSimple, WarningCircle } from "@phosphor-icons/react";
import { ReaderIdleExperience } from "./ReaderIdleExperience.jsx";

export function AccountEmptyWorkspace({ needsRepair = false, onConnect }) {
  const title = needsRepair ? "账户需要重新连接" : "尚未连接邮箱";
  const description = needsRepair
    ? "重新连接后即可继续同步、阅读和发送邮件。"
    : "连接邮箱后即可开始收取、阅读和发送邮件。";
  const actionLabel = needsRepair ? "修复账户" : "连接邮箱";
  const ActionIcon = needsRepair ? WarningCircle : EnvelopeSimple;

  return (
    <section
      className="account-empty-workspace"
      data-repair={needsRepair ? "true" : undefined}
      aria-labelledby="account-empty-workspace-title"
    >
      <ReaderIdleExperience />
      <div className="account-empty-workspace__prompt">
        <span className="account-empty-workspace__copy">
          <strong id="account-empty-workspace-title">{title}</strong>
          <small>{description}</small>
        </span>
        <button
          type="button"
          className="send-button account-empty-workspace__action"
          onClick={onConnect}
        >
          <ActionIcon size={17} weight="fill" aria-hidden="true" />
          {actionLabel}
        </button>
      </div>
    </section>
  );
}
