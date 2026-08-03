import { useId, useRef } from "react";
import { X } from "@phosphor-icons/react";
import {
  ConfirmDialogStatus,
  useConfirmDialogFocus,
} from "./ConfirmDialogPrimitives.jsx";
import { IconButton } from "./IconButton.jsx";
import { userFacingErrorMessage } from "../utils/userFacingError.js";

export function ConsequentialConfirmDialog({
  open,
  title,
  description,
  icon,
  tone = "primary",
  cancelLabel = "取消",
  closeLabel = "取消操作",
  confirmLabel,
  pendingLabel = "正在处理…",
  isPending = false,
  confirmDisabled = false,
  errorMessage = null,
  returnFocusRef = null,
  onCancel,
  onConfirm,
  children = null,
}) {
  const generatedId = useId().replaceAll(":", "");
  const titleId = `consequential-confirm-title-${generatedId}`;
  const descriptionId = `consequential-confirm-description-${generatedId}`;
  const cancelRef = useRef(null);
  const {
    dialogRef,
    onBackdropPointerDown,
    onDialogKeyDown,
  } = useConfirmDialogFocus({
    open,
    isPending,
    initialFocusRef: cancelRef,
    returnFocusRef,
    onCancel,
  });

  if (!open) return null;

  const cancel = () => {
    if (!isPending) onCancel?.();
  };

  const confirm = () => {
    if (!isPending) onConfirm?.();
  };

  return (
    <div
      className="confirm-layer"
      data-pending={isPending}
      onPointerDown={onBackdropPointerDown}
    >
      <section
        ref={dialogRef}
        className={`confirm-dialog consequential-confirm-dialog${
          tone === "danger" ? " confirm-dialog--danger" : ""
        }`}
        role="alertdialog"
        tabIndex={-1}
        aria-modal="true"
        aria-busy={isPending || undefined}
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
        onKeyDown={onDialogKeyDown}
      >
        <header>
          <span className="confirm-dialog__icon" aria-hidden="true">
            {icon}
          </span>
          <IconButton
            label={closeLabel}
            onClick={cancel}
            disabled={isPending}
          >
            <X size={18} />
          </IconButton>
        </header>

        <h2 id={titleId}>{title}</h2>
        <p id={descriptionId}>{description}</p>
        {children}
        <ConfirmDialogStatus>
          {isPending ? pendingLabel : null}
        </ConfirmDialogStatus>
        {errorMessage ? (
          <p
            className="consequential-confirm-dialog__error"
            role="alert"
            aria-live="assertive"
            aria-atomic="true"
          >
            {userFacingErrorMessage(errorMessage, "该操作没有完成")}
          </p>
        ) : null}

        <footer>
          <button
            ref={cancelRef}
            type="button"
            className="secondary-button"
            onClick={cancel}
            disabled={isPending}
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            className={
              tone === "danger"
                ? "danger-button confirm-dialog__danger-action"
                : "send-button"
            }
            onClick={confirm}
            disabled={isPending || confirmDisabled}
          >
            {isPending ? pendingLabel : confirmLabel}
          </button>
        </footer>
      </section>
    </div>
  );
}
