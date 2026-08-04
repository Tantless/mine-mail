import { useId, useRef } from "react";
import { Archive, X } from "@phosphor-icons/react";
import {
  ConfirmDialogStatus,
  useConfirmDialogFocus,
} from "./ConfirmDialogPrimitives.jsx";
import { IconButton } from "./IconButton.jsx";
import { ThemedSelect } from "./ThemedSelect.jsx";
import { userFacingErrorMessage } from "../utils/userFacingError.js";

export function ArchiveFolderDialog({
  open,
  candidates = [],
  selectedId = "",
  isPending = false,
  errorMessage = null,
  returnFocusRef = null,
  onSelectedIdChange,
  onCancel,
  onConfirm,
}) {
  const generatedId = useId().replaceAll(":", "");
  const titleId = `archive-folder-title-${generatedId}`;
  const descriptionId = `archive-folder-description-${generatedId}`;
  const cancelRef = useRef(null);
  const { dialogRef, onBackdropPointerDown, onDialogKeyDown } =
    useConfirmDialogFocus({
      open,
      isPending,
      initialFocusRef: cancelRef,
      returnFocusRef,
      onCancel,
    });

  if (!open) return null;

  const options = candidates.map((candidate) => ({
    value: candidate.selectionId,
    label: candidate.displayName,
  }));
  const hasCandidates = options.length > 0;
  const handleDialogKeyDown = (event) => {
    const themedSelect =
      event.target instanceof Element
        ? event.target.closest(".themed-select")
        : null;
    if (event.key === "Escape" && themedSelect?.dataset.open === "true") {
      return;
    }
    onDialogKeyDown(event);
  };

  return (
    <div
      className="confirm-layer"
      data-pending={isPending || undefined}
      onPointerDown={onBackdropPointerDown}
    >
      <section
        ref={dialogRef}
        className="confirm-dialog archive-folder-dialog"
        role="dialog"
        tabIndex={-1}
        aria-modal="true"
        aria-busy={isPending || undefined}
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
        onKeyDown={handleDialogKeyDown}
      >
        <header>
          <span className="confirm-dialog__icon" aria-hidden="true">
            <Archive size={23} weight="duotone" />
          </span>
          <IconButton
            label="取消设置归档文件夹"
            onClick={onCancel}
            disabled={isPending}
          >
            <X size={18} />
          </IconButton>
        </header>

        <h2 id={titleId}>选择归档文件夹</h2>
        <p id={descriptionId}>
          此邮箱没有提供官方归档文件夹。请选择服务器上已有的文件夹，今后的归档邮件会移动到这里。
        </p>

        {hasCandidates ? (
          <div className="archive-folder-dialog__field">
            <span>已有服务器文件夹</span>
            <ThemedSelect
              id={`archive-folder-select-${generatedId}`}
              label="已有服务器文件夹"
              value={selectedId}
              options={options}
              disabled={isPending}
              onValueChange={onSelectedIdChange}
            />
          </div>
        ) : (
          <p className="archive-folder-dialog__empty">
            服务器上没有可用文件夹。请先在邮箱网页中创建一个普通文件夹，然后重试。
          </p>
        )}

        <ConfirmDialogStatus>
          {isPending ? "正在保存归档文件夹…" : null}
        </ConfirmDialogStatus>
        {errorMessage ? (
          <p
            className="archive-folder-dialog__error"
            role="alert"
            aria-live="assertive"
            aria-atomic="true"
          >
            {userFacingErrorMessage(errorMessage, "归档文件夹没有保存")}
          </p>
        ) : null}

        <footer>
          <button
            ref={cancelRef}
            type="button"
            className="secondary-button"
            disabled={isPending}
            onClick={onCancel}
          >
            取消
          </button>
          <button
            type="button"
            className="primary-button"
            disabled={isPending || !hasCandidates || !selectedId}
            onClick={onConfirm}
          >
            {isPending ? "正在保存…" : "使用此文件夹"}
          </button>
        </footer>
      </section>
    </div>
  );
}
