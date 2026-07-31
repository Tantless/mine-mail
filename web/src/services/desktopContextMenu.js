function isTauriRuntime(runtimeWindow) {
  const candidate =
    runtimeWindow ?? (typeof window === "undefined" ? undefined : window);
  return Boolean(candidate && "__TAURI_INTERNALS__" in candidate);
}

/**
 * Suppress only the WebView-provided menu. Context-menu events still propagate
 * so Mine Mail can attach its own contextual actions later.
 */
export function installDesktopContextMenuGuard(
  targetDocument,
  runtimeWindow,
) {
  if (!targetDocument?.addEventListener || !isTauriRuntime(runtimeWindow)) {
    return () => {};
  }

  const preventWebViewMenu = (event) => event.preventDefault();
  targetDocument.addEventListener("contextmenu", preventWebViewMenu, true);

  return () => {
    targetDocument.removeEventListener("contextmenu", preventWebViewMenu, true);
  };
}
