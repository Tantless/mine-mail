import { CheckCircle, Info, WarningCircle, X } from "@phosphor-icons/react";

export function Toast({ toast, onClose }) {
  if (!toast) return null;
  const Icon =
    toast.tone === "error"
      ? WarningCircle
      : toast.tone === "info"
        ? Info
        : CheckCircle;
  return (
    <div
      className="toast"
      role={toast.tone === "error" ? "alert" : "status"}
      data-tone={toast.tone || "success"}
    >
      <span className="toast__icon" aria-hidden="true">
        <Icon size={18} weight="fill" />
      </span>
      <span className="toast__message">{toast.message}</span>
      <button
        className="toast__close"
        type="button"
        onClick={onClose}
        aria-label="关闭通知"
      >
        <X size={15} />
      </button>
    </div>
  );
}
