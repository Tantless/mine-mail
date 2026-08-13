import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useState,
} from "react";

function sameSelection(current, next) {
  return (
    current &&
    current.x === next.x &&
    current.y === next.y &&
    current.width === next.width &&
    current.height === next.height &&
    current.visible === next.visible
  );
}

function selectionBounds(container, selectedElement) {
  // Every current selection target is a direct child of its positioned
  // container. offset* exposes that stable layout box, while
  // getBoundingClientRect() includes an ancestor's temporary transform. The
  // latter made a selection measured during the mail-list scale-in animation
  // remain permanently short after the animation finished.
  if (
    selectedElement.parentElement === container &&
    selectedElement.offsetWidth > 0 &&
    selectedElement.offsetHeight > 0
  ) {
    return {
      x: selectedElement.offsetLeft,
      y: selectedElement.offsetTop,
      width: selectedElement.offsetWidth,
      height: selectedElement.offsetHeight,
    };
  }

  const containerBounds = container.getBoundingClientRect();
  const selectedBounds = selectedElement.getBoundingClientRect();
  return {
    x: selectedBounds.left - containerBounds.left,
    y: selectedBounds.top - containerBounds.top,
    width: selectedBounds.width,
    height: selectedBounds.height,
  };
}

export function useSlidingSelection({
  containerRef,
  layoutKey,
  selectedKey,
  selectedSelector = '[data-selected="true"]',
}) {
  const [selection, setSelection] = useState(null);
  const [motionReady, setMotionReady] = useState(false);

  const measureSelection = useCallback(() => {
    const container = containerRef.current;
    const selectedElement =
      selectedKey == null
        ? null
        : container?.querySelector(selectedSelector);

    if (!container || !selectedElement) {
      setSelection((current) => {
        if (!current?.visible) return current;
        return { ...current, visible: false };
      });
      return;
    }

    const bounds = selectionBounds(container, selectedElement);
    const nextSelection = {
      ...bounds,
      visible: true,
    };

    setSelection((current) =>
      sameSelection(current, nextSelection) ? current : nextSelection,
    );
  }, [containerRef, selectedKey, selectedSelector]);

  useLayoutEffect(() => {
    measureSelection();

    const container = containerRef.current;
    const selectedElement =
      selectedKey == null
        ? null
        : container?.querySelector(selectedSelector);
    const resizeObserver =
      container && typeof ResizeObserver === "function"
        ? new ResizeObserver(measureSelection)
        : null;

    if (container) resizeObserver?.observe(container);
    if (selectedElement) resizeObserver?.observe(selectedElement);
    window.addEventListener("resize", measureSelection);

    return () => {
      resizeObserver?.disconnect();
      window.removeEventListener("resize", measureSelection);
    };
  }, [
    containerRef,
    layoutKey,
    measureSelection,
    selectedKey,
    selectedSelector,
  ]);

  useEffect(() => {
    if (!selection || motionReady) return undefined;
    const frame = window.requestAnimationFrame(() => {
      setMotionReady(true);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [motionReady, selection]);

  return {
    motionReady,
    selectionStyle: {
      "--sliding-selection-x": `${selection?.x || 0}px`,
      "--sliding-selection-y": `${selection?.y || 0}px`,
      "--sliding-selection-width": `${selection?.width || 0}px`,
      "--sliding-selection-height": `${selection?.height || 0}px`,
    },
    selectionVisible: Boolean(selection?.visible),
  };
}
