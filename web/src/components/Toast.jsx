import { CheckCircle, Info, WarningCircle, X } from "@phosphor-icons/react";
import { userFacingErrorMessage } from "../utils/userFacingError.js";

export function Toast({ toast, onClose }) {
  if (!toast) return null;
  const message =
    toast.tone === "error"
      ? userFacingErrorMessage(toast.message, "该操作没有完成")
      : toast.message;
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
      data-state={toast.exiting ? "exiting" : "visible"}
    >
      <span className="toast__icon" aria-hidden="true">
        <Icon size={18} weight="fill" />
      </span>
      <span className="toast__message">{message}</span>
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
