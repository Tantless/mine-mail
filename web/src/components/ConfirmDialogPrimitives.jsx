import { useCallback, useEffect, useLayoutEffect, useRef } from "react";

const focusableSelector = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

function focusableElements(dialog) {
  if (!dialog) return [];
  return Array.from(dialog.querySelectorAll(focusableSelector)).filter(
    (element) =>
      element instanceof HTMLElement &&
      element.getAttribute("aria-hidden") !== "true" &&
      element.tabIndex >= 0,
  );
}

export function useConfirmDialogFocus({
  open,
  isPending = false,
  initialFocusRef = null,
  returnFocusRef = null,
  onCancel,
}) {
  const dialogRef = useRef(null);
  const previousFocusRef = useRef(null);

  useLayoutEffect(() => {
    if (!open) return undefined;
    previousFocusRef.current =
      returnFocusRef?.current ||
      (document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null);
    const initialFocus =
      initialFocusRef?.current || focusableElements(dialogRef.current)[0];
    initialFocus?.focus();

    return () => {
      const previousFocus = previousFocusRef.current;
      previousFocusRef.current = null;
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  }, [initialFocusRef, open, returnFocusRef]);

  useEffect(() => {
    if (open && isPending) dialogRef.current?.focus();
  }, [isPending, open]);

  const onDialogKeyDown = useCallback(
    (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        if (!isPending) onCancel?.();
        return;
      }
      if (event.key !== "Tab") return;

      const focusable = focusableElements(dialogRef.current);
      const first = focusable[0];
      const last = focusable.at(-1);
      const active = document.activeElement;
      if (!first || !last) {
        event.preventDefault();
        dialogRef.current?.focus();
        return;
      }
      if (
        event.shiftKey &&
        (active === first || !dialogRef.current?.contains(active))
      ) {
        event.preventDefault();
        last.focus();
        return;
      }
      if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    },
    [isPending, onCancel],
  );

  const onBackdropPointerDown = useCallback(
    (event) => {
      if (event.target === event.currentTarget && !isPending) onCancel?.();
    },
    [isPending, onCancel],
  );

  return {
    dialogRef,
    onBackdropPointerDown,
    onDialogKeyDown,
  };
}

export function ConfirmDialogStatus({ children, assertive = false }) {
  if (!children) return null;
  return (
    <span
      className="sr-only"
      role={assertive ? "alert" : "status"}
      aria-live={assertive ? "assertive" : "polite"}
      aria-atomic="true"
    >
      {children}
    </span>
  );
}
