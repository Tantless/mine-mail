const emptyClientRect = {
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

const emptyClientRects = Object.assign([], {
  item: () => null,
});

if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () => emptyClientRects;
}

if (!Range.prototype.getBoundingClientRect) {
  Range.prototype.getBoundingClientRect = () => emptyClientRect;
}

if (!Element.prototype.getClientRects) {
  Element.prototype.getClientRects = () => emptyClientRects;
}

if (!Document.prototype.elementFromPoint) {
  Document.prototype.elementFromPoint = function elementFromPoint() {
    return (
      this.activeElement ||
      this.querySelector('[contenteditable="true"]') ||
      this.body
    );
  };
}

if (!HTMLElement.prototype.scrollIntoView) {
  HTMLElement.prototype.scrollIntoView = () => {};
}
