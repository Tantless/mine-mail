import { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

const VIEWPORT_MARGIN = 8;
const TOOLTIP_GAP = 8;

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
