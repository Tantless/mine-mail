import { useLayoutEffect, useState } from "react";

const viewportEdgeInset = 12;
const dropdownGap = 7;
const composeClearance = 10;

function minimizedComposeTop() {
  if (typeof document === "undefined") return null;

  let nearestTop = Number.POSITIVE_INFINITY;
  document
    .querySelectorAll('.compose-panel[data-minimized="true"]')
    .forEach((panel) => {
      const rect = panel.getBoundingClientRect();
      if (
        rect.width > 0 &&
        rect.height > 0 &&
        rect.bottom > 0 &&
        rect.top < window.innerHeight
      ) {
        nearestTop = Math.min(nearestTop, rect.top);
      }
    });

  return Number.isFinite(nearestTop) ? nearestTop : null;
}

export function boundedDropdownLayout(
  anchorRect,
  {
    preferredMaxHeight,
    viewportHeight,
    obstructionTop = null,
  },
) {
  const viewportBottom = Math.max(viewportEdgeInset, viewportHeight - viewportEdgeInset);
  const lowerBoundary = Math.min(
    viewportBottom,
    obstructionTop == null ? viewportBottom : obstructionTop - composeClearance,
  );
  const roomBelow = Math.max(0, lowerBoundary - anchorRect.bottom - dropdownGap);

  return {
    maxHeight: Math.max(0, Math.floor(Math.min(preferredMaxHeight, roomBelow))),
  };
}

export function useBoundedDropdown({
  open,
  anchorRef,
  preferredMaxHeight,
}) {
  const [layout, setLayout] = useState(() => ({
    maxHeight: preferredMaxHeight,
  }));

  useLayoutEffect(() => {
    if (!open || !anchorRef.current || typeof window === "undefined") return undefined;

    const updateLayout = () => {
      const next = boundedDropdownLayout(anchorRef.current.getBoundingClientRect(), {
        preferredMaxHeight,
        viewportHeight: window.innerHeight,
        obstructionTop: minimizedComposeTop(),
      });
      setLayout((current) =>
        current.maxHeight === next.maxHeight
          ? current
          : next,
      );
    };

    updateLayout();
    window.addEventListener("resize", updateLayout);
    window.addEventListener("scroll", updateLayout, true);

    const panels = Array.from(
      document.querySelectorAll('.compose-panel[data-minimized="true"]'),
    );
    panels.forEach((panel) => panel.addEventListener("transitionend", updateLayout));

    const resizeObserver =
      typeof ResizeObserver === "function" ? new ResizeObserver(updateLayout) : null;
    resizeObserver?.observe(anchorRef.current);
    panels.forEach((panel) => resizeObserver?.observe(panel));

    return () => {
      window.removeEventListener("resize", updateLayout);
      window.removeEventListener("scroll", updateLayout, true);
      panels.forEach((panel) => panel.removeEventListener("transitionend", updateLayout));
      resizeObserver?.disconnect();
    };
  }, [anchorRef, open, preferredMaxHeight]);

  return layout;
}
