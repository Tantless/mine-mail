import { useCallback, useEffect, useMemo, useState } from "react";
import {
  appUpdateApi,
  describeUpdateFailure,
  isUpdateCancelledError,
} from "../services/appUpdate.js";

export function displayAppVersion(version) {
  return `v${String(version || "0.0.0").replace(/^v/i, "")}`;
}

export function useAppUpdate(
  updateClient = appUpdateApi,
  { enabled = true } = {},
) {
  const [appVersion, setAppVersion] = useState(
    updateClient.bundledVersion || "0.0.0",
  );
  const [status, setStatus] = useState("idle");
  const [message, setMessage] = useState(null);
  const [availableUpdate, setAvailableUpdate] = useState(null);
  const [progress, setProgress] = useState(null);
  const [isDialogOpen, setIsDialogOpen] = useState(false);

  useEffect(() => {
    if (!enabled) return undefined;
    let active = true;
    void updateClient
      .getCurrentVersion()
      .then((version) => {
        if (active && version) setAppVersion(version);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [enabled, updateClient]);

  const checkForUpdate = useCallback(async () => {
    if (
      !enabled ||
      !updateClient.isSupported ||
      ["checking", "installing", "cancelling"].includes(status)
    ) {
      return;
    }
    if (availableUpdate) {
      setIsDialogOpen(true);
      return;
    }

    setStatus("checking");
    setMessage(null);
    setProgress(null);
    try {
      const result = await updateClient.checkForUpdate();
      if (result.currentVersion) setAppVersion(result.currentVersion);
      if (result.status === "available") {
        setAvailableUpdate(result);
        setStatus("available");
        setIsDialogOpen(true);
        return;
      }
      if (result.status === "up-to-date") {
        setStatus("up-to-date");
        setMessage("已是最新版本。");
        return;
      }
      setStatus("unsupported");
      setMessage("请在 Mine Mail 桌面应用中检查更新。");
    } catch (error) {
      setStatus("error");
      setMessage(describeUpdateFailure(error, "check").message);
    }
  }, [availableUpdate, enabled, status, updateClient]);

  const installAvailableUpdate = useCallback(async () => {
    if (!enabled || !availableUpdate || status === "installing") return;
    let downloaded = 0;
    let total = null;
    setStatus("installing");
    setMessage(null);
    setProgress({ stage: "starting", downloaded, total, percent: null });
    try {
      await updateClient.installUpdate(availableUpdate, (event) => {
        if (event.event === "Started") {
          total = event.data.contentLength || null;
          setProgress({
            stage: "downloading",
            downloaded,
            total,
            percent: total ? 0 : null,
          });
          return;
        }
        if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setProgress({
            stage: "downloading",
            downloaded,
            total,
            percent: total
              ? Math.min(100, Math.round((downloaded / total) * 100))
              : null,
          });
          return;
        }
        if (["Finished", "Installing"].includes(event.event)) {
          setProgress({
            stage: "installing",
            downloaded,
            total,
            percent: 100,
          });
        }
      });
      setAvailableUpdate(null);
      setIsDialogOpen(false);
      setStatus("installed");
      setMessage("更新已安装，正在重新启动 Mine Mail…");
    } catch (error) {
      if (isUpdateCancelledError(error)) {
        setStatus("available");
        setProgress(null);
        setMessage(
          `已取消 ${displayAppVersion(availableUpdate.version)} 更新下载。`,
        );
        return;
      }
      setStatus("error");
      setProgress(null);
      setMessage(describeUpdateFailure(error, "install").message);
    }
  }, [availableUpdate, enabled, status, updateClient]);

  const cancelDownload = useCallback(async () => {
    if (
      !enabled ||
      status !== "installing" ||
      progress?.stage === "installing"
    ) {
      return false;
    }
    setStatus("cancelling");
    try {
      const cancelled = await updateClient.cancelUpdate?.();
      if (cancelled === false) setStatus("installing");
      return cancelled !== false;
    } catch {
      setStatus("installing");
      return false;
    }
  }, [enabled, progress?.stage, status, updateClient]);

  const closeDialog = useCallback(() => {
    if (!availableUpdate) return;
    if (["installing", "cancelling"].includes(status)) {
      setIsDialogOpen(false);
      return;
    }
    setAvailableUpdate(null);
    setIsDialogOpen(false);
    setStatus("idle");
    setMessage(
      `已暂缓 ${displayAppVersion(availableUpdate.version)} 更新。`,
    );
  }, [availableUpdate, status]);

  const minimizeDialog = useCallback(() => setIsDialogOpen(false), []);

  return useMemo(
    () => ({
      appVersion,
      availableUpdate,
      cancelDownload,
      checkForUpdate,
      closeDialog,
      installAvailableUpdate,
      isDialogOpen,
      isDownloadActive: ["installing", "cancelling"].includes(status),
      isDownloadCancellable:
        status === "installing" && progress?.stage !== "installing",
      message,
      minimizeDialog,
      progress,
      status,
      updateClient,
    }),
    [
      appVersion,
      availableUpdate,
      cancelDownload,
      checkForUpdate,
      closeDialog,
      installAvailableUpdate,
      isDialogOpen,
      message,
      minimizeDialog,
      progress,
      status,
      updateClient,
    ],
  );
}
