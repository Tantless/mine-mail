import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";

const isTauriRuntime =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const demoStatus = {
  dataPath: "浏览器预览不使用本地数据目录",
  locationKind: "local_app_data",
  available: false,
  totalBytes: 0,
  reclaimableWebviewBytes: 0,
  categories: [
    { id: "mail", label: "邮件与本地资料", bytes: 0 },
    { id: "webview", label: "界面与浏览器缓存", bytes: 0 },
    { id: "user_assets", label: "用户资源", bytes: 0 },
    { id: "cache", label: "可清理缓存", bytes: 0 },
    { id: "logs", label: "诊断日志", bytes: 0 },
    { id: "other", label: "其他数据", bytes: 0 },
  ],
  migrationNotice: null,
  cacheCleanupNotice: null,
};

export const appStorageApi = {
  isSupported: isTauriRuntime,

  async getStatus() {
    if (!isTauriRuntime) return structuredClone(demoStatus);
    return invoke("get_storage_status");
  },

  async chooseDirectory(currentPath) {
    if (!isTauriRuntime) return null;
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: currentPath || undefined,
      title: "选择新的 Mine Mail 数据目录",
    });
    return typeof selected === "string" ? selected : null;
  },

  async prepareMigration(targetPath) {
    if (!isTauriRuntime) {
      throw new Error("浏览器预览不执行本地数据迁移。");
    }
    return invoke("prepare_storage_migration", { targetPath });
  },

  async cancelMigration() {
    if (!isTauriRuntime) return;
    return invoke("cancel_storage_migration");
  },

  async prepareWebviewCacheCleanup() {
    if (!isTauriRuntime) {
      throw new Error("浏览器预览不执行界面缓存清理。");
    }
    return invoke("prepare_webview_cache_cleanup");
  },

  async cancelWebviewCacheCleanup() {
    if (!isTauriRuntime) return;
    return invoke("cancel_webview_cache_cleanup");
  },

  async relaunch() {
    if (!isTauriRuntime) {
      throw new Error("浏览器预览无法重启桌面应用。");
    }
    await relaunch();
  },
};
