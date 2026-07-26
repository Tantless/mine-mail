import {
  cloneElement,
  isValidElement,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";

const VIEWPORT_MARGIN = 8;
const TOOLTIP_GAP = 8;
const TOOLTIP_DELAY_MS = 380;

function clamp(value, minimum, maximum) {
  return Math.min(Math.max(value, minimum), Math.max(minimum, maximum));
}

export function Tooltip({ anchorRef, children, open }) {
  const tooltipRef = useRef(null);
  const [position, setPosition] = useState(null);

  useLayoutEffect(() => {
    if (!open) {
      setPosition(null);
      return undefined;
    }

    const anchor = anchorRef.current;
    const tooltip = tooltipRef.current;
    if (!anchor || !tooltip) return undefined;

    const placeTooltip = () => {
      const anchorRect = anchor.getBoundingClientRect();
      const tooltipRect = tooltip.getBoundingClientRect();
      const roomBelow = window.innerHeight - anchorRect.bottom - TOOLTIP_GAP;
      const roomAbove = anchorRect.top - TOOLTIP_GAP;
      const placement = roomBelow >= tooltipRect.height || roomBelow >= roomAbove ? "bottom" : "top";
      const top =
        placement === "bottom"
          ? anchorRect.bottom + TOOLTIP_GAP
          : anchorRect.top - tooltipRect.height - TOOLTIP_GAP;
      const centeredLeft = anchorRect.left + anchorRect.width / 2 - tooltipRect.width / 2;

      setPosition({
        left: clamp(
          centeredLeft,
          VIEWPORT_MARGIN,
          window.innerWidth - tooltipRect.width - VIEWPORT_MARGIN,
        ),
        placement,
        top: clamp(
          top,
          VIEWPORT_MARGIN,
          window.innerHeight - tooltipRect.height - VIEWPORT_MARGIN,
        ),
      });
    };

    placeTooltip();
    window.addEventListener("resize", placeTooltip);
    return () => window.removeEventListener("resize", placeTooltip);
  }, [anchorRef, children, open]);

  if (!open || typeof document === "undefined") return null;

  return createPortal(
    <span
      ref={tooltipRef}
      className="app-tooltip"
      data-placement={position?.placement || "bottom"}
      data-ready={Boolean(position)}
      role="tooltip"
      style={{
        left: position?.left ?? 0,
        top: position?.top ?? 0,
      }}
    >
      {children}
    </span>,
    document.body,
  );
}

function assignRef(ref, value) {
  if (typeof ref === "function") {
    ref(value);
  } else if (ref && typeof ref === "object") {
    ref.current = value;
  }
}

export function TooltipTarget({ children, label }) {
  const anchorRef = useRef(null);
  const openTimerRef = useRef(null);
  const [tooltipOpen, setTooltipOpen] = useState(false);
  const child = isValidElement(children) ? children : null;
  const childRef = child?.props?.ref;

  const setAnchorRef = useCallback(
    (node) => {
      anchorRef.current = node;
      assignRef(childRef, node);
    },
    [childRef],
  );

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
    if (!label) return;
    cancelPendingOpen();
    openTimerRef.current = window.setTimeout(() => {
      openTimerRef.current = null;
      setTooltipOpen(true);
    }, TOOLTIP_DELAY_MS);
  }, [cancelPendingOpen, label]);

  useEffect(() => () => cancelPendingOpen(), [cancelPendingOpen]);

  useEffect(() => {
    if (!tooltipOpen) return undefined;
    const dismissOnViewportChange = () => closeTooltip();
    window.addEventListener("scroll", dismissOnViewportChange, true);
    return () => window.removeEventListener("scroll", dismissOnViewportChange, true);
  }, [closeTooltip, tooltipOpen]);

  if (!child) return children;

  return (
    <>
      {cloneElement(child, {
        ref: setAnchorRef,
        title: undefined,
        onBlur: (event) => {
          closeTooltip();
          child.props.onBlur?.(event);
        },
        onClick: (event) => {
          closeTooltip();
          child.props.onClick?.(event);
        },
        onFocus: (event) => {
          cancelPendingOpen();
          if (label) setTooltipOpen(true);
          child.props.onFocus?.(event);
        },
        onKeyDown: (event) => {
          if (event.key === "Escape") closeTooltip();
          child.props.onKeyDown?.(event);
        },
        onPointerEnter: (event) => {
          scheduleTooltip();
          child.props.onPointerEnter?.(event);
        },
        onPointerLeave: (event) => {
          closeTooltip();
          child.props.onPointerLeave?.(event);
        },
      })}
      <Tooltip anchorRef={anchorRef} open={tooltipOpen}>
        {label}
      </Tooltip>
    </>
  );
}
