import { describe, expect, it, vi } from "vitest";

import { installDesktopContextMenuGuard } from "./desktopContextMenu.js";

function dispatchContextMenu(target) {
  const event = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
  });
  target.dispatchEvent(event);
  return event;
}

describe("desktop context-menu guard", () => {
  it("prevents the WebView menu without blocking app-owned handlers", () => {
    const targetDocument = document.implementation.createHTMLDocument();
    const appHandler = vi.fn();
    targetDocument.body.addEventListener("contextmenu", appHandler);

    const removeGuard = installDesktopContextMenuGuard(targetDocument, {
      __TAURI_INTERNALS__: {},
    });
    const guardedEvent = dispatchContextMenu(targetDocument.body);

    expect(guardedEvent.defaultPrevented).toBe(true);
    expect(appHandler).toHaveBeenCalledOnce();

    removeGuard();
    expect(dispatchContextMenu(targetDocument.body).defaultPrevented).toBe(false);
  });

  it("leaves the browser demo context menu unchanged", () => {
    const targetDocument = document.implementation.createHTMLDocument();

    installDesktopContextMenuGuard(targetDocument, {});

    expect(dispatchContextMenu(targetDocument.body).defaultPrevented).toBe(false);
  });
});
