import {
  ArrowsOutSimple,
  EnvelopeSimple,
  Minus,
  Square,
  X,
} from "@phosphor-icons/react";
import { getCurrentWindow } from "@tauri-apps/api/window";

function runWindowAction(action, isDesktop) {
  if (!isDesktop) return;

  const appWindow = getCurrentWindow();
  appWindow[action]().catch((error) => {
    console.error(`Window action failed: ${action}`, error);
  });
}

export function WindowTitlebar({ platform, isDesktop }) {
  const isMac = platform === "mac";
  const webOnlyControlProps = isDesktop
    ? {}
    : { "aria-disabled": true, tabIndex: -1 };

  return (
    <header
      className="app-titlebar"
      data-testid="window-titlebar"
      data-tauri-drag-region="deep"
    >
      {isMac ? (
        <div
          className="titlebar-controls titlebar-controls--mac"
          data-tauri-drag-region="false"
        >
          <button
            className="titlebar-control titlebar-control--close"
            type="button"
            aria-label="关闭窗口"
            title="关闭"
            {...webOnlyControlProps}
            onClick={() => runWindowAction("close", isDesktop)}
          >
            <span className="titlebar-control__mac-face">
              <X size={8} weight="bold" />
            </span>
          </button>
          <button
            className="titlebar-control titlebar-control--minimize"
            type="button"
            aria-label="最小化窗口"
            title="最小化"
            {...webOnlyControlProps}
            onClick={() => runWindowAction("minimize", isDesktop)}
          >
            <span className="titlebar-control__mac-face">
              <Minus size={8} weight="bold" />
            </span>
          </button>
          <button
            className="titlebar-control titlebar-control--maximize"
            type="button"
            aria-label="最大化或还原窗口"
            title="最大化或还原"
            {...webOnlyControlProps}
            onClick={() => runWindowAction("toggleMaximize", isDesktop)}
          >
            <span className="titlebar-control__mac-face">
              <ArrowsOutSimple size={7} weight="bold" />
            </span>
          </button>
        </div>
      ) : null}

      <div className="titlebar-brand">
        <EnvelopeSimple size={15} weight="regular" aria-hidden="true" />
        <span>Mine Mail</span>
      </div>

      {!isMac ? (
        <div
          className="titlebar-controls titlebar-controls--windows"
          data-tauri-drag-region="false"
        >
          <button
            className="titlebar-control"
            type="button"
            aria-label="最小化窗口"
            title="最小化"
            {...webOnlyControlProps}
            onClick={() => runWindowAction("minimize", isDesktop)}
          >
            <Minus size={14} />
          </button>
          <button
            className="titlebar-control"
            type="button"
            aria-label="最大化或还原窗口"
            title="最大化或还原"
            {...webOnlyControlProps}
            onClick={() => runWindowAction("toggleMaximize", isDesktop)}
          >
            <Square size={11} />
          </button>
          <button
            className="titlebar-control titlebar-control--windows-close"
            type="button"
            aria-label="关闭窗口"
            title="关闭"
            {...webOnlyControlProps}
            onClick={() => runWindowAction("close", isDesktop)}
          >
            <X size={14} />
          </button>
        </div>
      ) : null}
    </header>
  );
}
