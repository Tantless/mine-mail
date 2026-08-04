import { describe, expect, it } from "vitest";
import tauriConfig from "../../src-tauri/tauri.conf.json";
import {
  __testing,
  appUpdateApi,
  bundledAppVersion,
  describeUpdateFailure,
} from "./appUpdate.js";

describe("appUpdate", () => {
  it("uses the Tauri package version as the browser-safe fallback", async () => {
    expect(bundledAppVersion).toBe(tauriConfig.version);
    expect(await appUpdateApi.getCurrentVersion()).toBe(tauriConfig.version);
  });

  it("bounds release notes before rendering them", () => {
    const notes = __testing.releaseNotes("a".repeat(1700));
    expect(notes).toHaveLength(1601);
    expect(notes.endsWith("…")).toBe(true);
  });

  it.each([
    [
      "check",
      new Error("request failed with status 404 for latest.json"),
      "not-found",
      "最新版本信息不存在或已失效（服务器返回 404）。请稍后重试；当前版本和本地数据不受影响。",
    ],
    [
      "install",
      new Error("Download request failed with status: 404 Not Found"),
      "not-found",
      "更新安装包不存在或下载地址已失效（服务器返回 404）。请稍后重试；当前版本和本地数据不受影响。",
    ],
    [
      "install",
      new Error("signature verification failed"),
      "signature",
      "更新包签名验证失败，Mine Mail 已停止安装。请稍后重试；当前版本和本地数据不受影响。",
    ],
    [
      "check",
      new Error("dns lookup failed"),
      "dns",
      "无法解析更新服务器地址。请检查 DNS、系统代理或网络后重试；当前版本和本地数据不受影响。",
    ],
    [
      "install",
      new Error("operation timed out"),
      "timeout",
      "下载更新超时。请检查系统代理或网络后重试；当前版本和本地数据不受影响。",
    ],
  ])("classifies %s failures without exposing raw errors", (operation, error, kind, message) => {
    const presentation = describeUpdateFailure(error, operation);
    expect(presentation).toEqual({ kind, message });
    expect(presentation.message).not.toContain(error.message);
  });

  it.each([
    [
      "Could not fetch a valid release JSON from the remote",
      "release-metadata",
      "更新服务器没有返回可用的最新版本信息。请稍后重试；当前版本和本地数据不受影响。",
    ],
    [
      "the platform `windows-aarch64` was not found in the response `platforms` object",
      "platform",
      "最新版本信息不包含当前系统可用的更新包。请稍后重试；当前版本和本地数据不受影响。",
    ],
    [
      "The configured updater endpoint must use a secure protocol like `https`.",
      "insecure-endpoint",
      "更新地址未使用安全连接，Mine Mail 已停止检查。请从 GitHub Release 手动安装新版后重试；当前版本和本地数据不受影响。",
    ],
  ])("classifies Tauri update-check errors", (raw, kind, message) => {
    expect(describeUpdateFailure(new Error(raw), "check")).toEqual({
      kind,
      message,
    });
  });

  it.each([
    [
      "Authentication failed or was cancelled",
      "authentication",
      "安装所需的系统授权未完成或已取消。请重新确认系统授权后重试；当前版本和本地数据不受影响。",
    ],
    [
      "invalid updater binary format",
      "package-format",
      "更新包格式无效，Mine Mail 已停止安装。请稍后重试；当前版本和本地数据不受影响。",
    ],
    [
      "failed to create temporary directory",
      "temporary-storage",
      "无法准备更新安装所需的临时目录。请检查磁盘空间和临时目录权限后重试；当前版本和本地数据不受影响。",
    ],
    [
      "Failed to install package",
      "install",
      "更新包已下载，但安装程序未能完成安装。请检查系统权限后重试；当前版本和本地数据不受影响。",
    ],
  ])("classifies Tauri package errors", (raw, kind, message) => {
    expect(describeUpdateFailure(new Error(raw), "install")).toEqual({
      kind,
      message,
    });
  });

  it("keeps unknown backend details out of the update error surface", () => {
    const presentation = describeUpdateFailure(
      new Error("token=private-value crashed in updater internals"),
      "install",
    );

    expect(presentation.kind).toBe("internal");
    expect(presentation.message).not.toContain("private-value");
  });
});
