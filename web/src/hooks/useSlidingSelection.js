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

    const containerBounds = container.getBoundingClientRect();
    const selectedBounds = selectedElement.getBoundingClientRect();
    const nextSelection = {
      x: selectedBounds.left - containerBounds.left,
      y: selectedBounds.top - containerBounds.top,
      width: selectedBounds.width,
      height: selectedBounds.height,
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
