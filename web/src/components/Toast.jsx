import { CheckCircle, WarningCircle, X } from "@phosphor-icons/react";

export function Toast({ toast, onClose }) {
  if (!toast) return null;
  const Icon = toast.tone === "error" ? WarningCircle : CheckCircle;
  return (
    <div
      className="toast"
      role={toast.tone === "error" ? "alert" : "status"}
      data-tone={toast.tone || "success"}
    >
      <Icon size={20} weight="fill" />
      <span>{toast.message}</span>
      <button type="button" onClick={onClose} aria-label="关闭通知">
        <X size={15} />
      </button>
    </div>
  );
}
