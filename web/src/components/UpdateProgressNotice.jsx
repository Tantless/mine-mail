import { StopCircle } from "@phosphor-icons/react";
import { displayAppVersion } from "../hooks/useAppUpdate.js";
import { IconButton } from "./IconButton.jsx";

export function UpdateProgressNotice({
  version,
  progress,
  isCancelling = false,
  canCancel = true,
  composeMinimized = false,
  onCancel,
}) {
  const installing = progress?.stage === "installing";
  return (
    <aside
      className="app-update-progress"
      data-compose-minimized={composeMinimized || undefined}
      role="status"
      aria-live="polite"
      aria-label={`下载更新 ${displayAppVersion(version)}`}
    >
      <div className="app-update-progress__body">
        <span>下载更新 {displayAppVersion(version)}</span>
        <progress
          aria-label="更新下载进度"
          max="100"
          value={progress?.percent ?? undefined}
        />
      </div>
      <IconButton
        className="app-update-progress__cancel"
        label={
          installing
            ? "更新已下载，无法取消"
            : isCancelling
              ? "正在取消更新下载"
              : "取消更新下载"
        }
        onClick={onCancel}
        disabled={!canCancel || isCancelling || installing}
      >
        <StopCircle size={18} weight="regular" />
      </IconButton>
    </aside>
  );
}
