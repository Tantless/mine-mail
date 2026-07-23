import { useCallback, useEffect, useRef, useState } from "react";
import { Tooltip } from "./Tooltip.jsx";

const TOOLTIP_DELAY_MS = 380;

export function IconButton({
  label,
  children,
  className = "",
  tone = "default",
  title: _nativeTitle,
  onBlur,
  onClick,
  onFocus,
  onKeyDown,
  onPointerEnter,
  onPointerLeave,
  ...props
}) {
  const buttonRef = useRef(null);
  const openTimerRef = useRef(null);
  const [tooltipOpen, setTooltipOpen] = useState(false);

  const cancelPendingOpen = useCallback(() => {
    if (openTimerRef.current !== null) {
      window.clearTimeout(openTimerRef.current);
      openTimerRef.current = null;
    }
  }, []);

  const closeTooltip = useCallback(() => {
    cancelPendingOpen();
    setTooltipOpen(false);
  }, [cancelPendingOpen]);

  const scheduleTooltip = useCallback(() => {
    cancelPendingOpen();
    openTimerRef.current = window.setTimeout(() => {
      openTimerRef.current = null;
      setTooltipOpen(true);
    }, TOOLTIP_DELAY_MS);
  }, [cancelPendingOpen]);

  useEffect(() => () => cancelPendingOpen(), [cancelPendingOpen]);

  useEffect(() => {
    if (!tooltipOpen) return undefined;
    const dismissOnViewportChange = () => closeTooltip();
    window.addEventListener("scroll", dismissOnViewportChange, true);
    return () => window.removeEventListener("scroll", dismissOnViewportChange, true);
  }, [closeTooltip, tooltipOpen]);

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        className={`icon-button icon-button--${tone} ${className}`.trim()}
        aria-label={label}
        onBlur={(event) => {
          closeTooltip();
          onBlur?.(event);
        }}
        onClick={(event) => {
          closeTooltip();
          onClick?.(event);
        }}
        onFocus={(event) => {
          cancelPendingOpen();
          setTooltipOpen(true);
          onFocus?.(event);
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") closeTooltip();
          onKeyDown?.(event);
        }}
        onPointerEnter={(event) => {
          scheduleTooltip();
          onPointerEnter?.(event);
        }}
        onPointerLeave={(event) => {
          closeTooltip();
          onPointerLeave?.(event);
        }}
        {...props}
      >
        {children}
      </button>
      <Tooltip anchorRef={buttonRef} open={tooltipOpen}>
        {label}
      </Tooltip>
    </>
  );
}
