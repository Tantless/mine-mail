import { afterEach, beforeEach } from "vitest";

function zeroRect() {
  return {
    bottom: 0,
    height: 0,
    left: 0,
    right: 0,
    top: 0,
    width: 0,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  };
}

function restoreProperty(target, name, descriptor) {
  if (descriptor) {
    Object.defineProperty(target, name, descriptor);
  } else {
    delete target[name];
  }
}

export function useProseMirrorTestGeometry() {
  let elementFromPointDescriptor;
  let rangeClientRectsDescriptor;
  let rangeBoundingRectDescriptor;

  beforeEach(() => {
    elementFromPointDescriptor = Object.getOwnPropertyDescriptor(
      Document.prototype,
      "elementFromPoint",
    );
    rangeClientRectsDescriptor = Object.getOwnPropertyDescriptor(
      Range.prototype,
      "getClientRects",
    );
    rangeBoundingRectDescriptor = Object.getOwnPropertyDescriptor(
      Range.prototype,
      "getBoundingClientRect",
    );

    Object.defineProperty(Document.prototype, "elementFromPoint", {
      configurable: true,
      value() {
        return null;
      },
    });
    Object.defineProperty(Range.prototype, "getClientRects", {
      configurable: true,
      value: () => [],
    });
    Object.defineProperty(Range.prototype, "getBoundingClientRect", {
      configurable: true,
      value: zeroRect,
    });
  });

  afterEach(() => {
    restoreProperty(
      Document.prototype,
      "elementFromPoint",
      elementFromPointDescriptor,
    );
    restoreProperty(
      Range.prototype,
      "getClientRects",
      rangeClientRectsDescriptor,
    );
    restoreProperty(
      Range.prototype,
      "getBoundingClientRect",
      rangeBoundingRectDescriptor,
    );
  });
}
