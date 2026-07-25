import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import tauriConfig from "../../src-tauri/tauri.conf.json";

const isTauriRuntime =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const bundledAppVersion = tauriConfig.version;

function releaseNotes(value) {
  const notes = typeof value === "string" ? value.trim() : "";
  if (!notes) return null;
  return notes.length > 1600 ? `${notes.slice(0, 1600).trimEnd()}…` : notes;
}

export const appUpdateApi = {
  isSupported: isTauriRuntime,
  bundledVersion: bundledAppVersion,

  async getCurrentVersion() {
    if (!isTauriRuntime) return bundledAppVersion;
    return getVersion();
  },

  async checkForUpdate() {
    const currentVersion = await this.getCurrentVersion();
    if (!isTauriRuntime) {
      return { status: "unsupported", currentVersion };
    }

    const update = await check({ timeout: 30_000 });
    if (!update) {
      return { status: "up-to-date", currentVersion };
    }

    return {
      status: "available",
      currentVersion: update.currentVersion || currentVersion,
      version: update.version,
      notes: releaseNotes(update.body),
      date: update.date || null,
      resource: update,
    };
  },

  async installUpdate(candidate, onEvent) {
    if (!candidate?.resource?.downloadAndInstall) {
      throw new Error("没有可安装的 Mine Mail 更新。");
    }
    await candidate.resource.downloadAndInstall(onEvent, {
      timeout: 10 * 60_000,
    });
    await relaunch();
  },
};

export const __testing = { releaseNotes };
