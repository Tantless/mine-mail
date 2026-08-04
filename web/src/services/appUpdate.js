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

function rawUpdateError(error) {
  const messages = [];
  const seen = new Set();
  let current = error;
  while (current && !seen.has(current) && messages.length < 4) {
    if (typeof current === "string") {
      messages.push(current);
      break;
    }
    seen.add(current);
    if (typeof current.message === "string") messages.push(current.message);
    current = current.cause;
  }
  return messages.join(" ").trim();
}

function updateFailure(kind, message) {
  return { kind, message };
}

export function describeUpdateFailure(error, operation = "check") {
  const raw = rawUpdateError(error);
  const isCheck = operation === "check";
  const subject = isCheck ? "更新检查" : "更新下载";

  if (/\b429\b|rate.?limit|too many requests/i.test(raw)) {
    return updateFailure(
      "rate-limit",
      `GitHub 暂时限制了${subject}（服务器返回 429）。请稍后重试；当前版本和本地数据不受影响。`,
    );
  }
  if (!isCheck && /signature|public key|minisign|base64/i.test(raw)) {
    return updateFailure(
      "signature",
      "更新包签名验证失败，Mine Mail 已停止安装。请稍后重试；当前版本和本地数据不受影响。",
    );
  }
  if (/\b404\b/i.test(raw)) {
    return updateFailure(
      "not-found",
      isCheck
        ? "最新版本信息不存在或已失效（服务器返回 404）。请稍后重试；当前版本和本地数据不受影响。"
        : "更新安装包不存在或下载地址已失效（服务器返回 404）。请稍后重试；当前版本和本地数据不受影响。",
    );
  }
  if (/\b403\b|forbidden/i.test(raw)) {
    return updateFailure(
      "forbidden",
      `更新服务器拒绝了${subject}请求（服务器返回 403）。请稍后重试或检查系统代理；当前版本和本地数据不受影响。`,
    );
  }
  const serverStatus = raw.match(/\b(5\d\d)\b/);
  if (serverStatus) {
    return updateFailure(
      "server",
      `更新服务器暂时不可用（服务器返回 ${serverStatus[1]}）。请稍后重试；当前版本和本地数据不受影响。`,
    );
  }
  if (/timed?\s*out|timeout/i.test(raw)) {
    return updateFailure(
      "timeout",
      `${isCheck ? "连接更新服务器" : "下载更新"}超时。请检查系统代理或网络后重试；当前版本和本地数据不受影响。`,
    );
  }
  if (/\bdns\b|resolve|lookup|name or service not known|host not found/i.test(raw)) {
    return updateFailure(
      "dns",
      "无法解析更新服务器地址。请检查 DNS、系统代理或网络后重试；当前版本和本地数据不受影响。",
    );
  }
  if (/certificate|\btls\b|\bssl\b|unknown issuer|handshake/i.test(raw)) {
    return updateFailure(
      "tls",
      "无法建立可信的更新连接。请检查系统时间、代理或证书设置后重试；当前版本和本地数据不受影响。",
    );
  }
  if (!isCheck && /authentication failed|authentication.*cancelled/i.test(raw)) {
    return updateFailure(
      "authentication",
      "安装所需的系统授权未完成或已取消。请重新确认系统授权后重试；当前版本和本地数据不受影响。",
    );
  }
  if (!isCheck && /no space|disk full|not enough space|os error 112/i.test(raw)) {
    return updateFailure(
      "storage",
      "设备可用存储空间不足，无法保存更新包。请释放空间后重试；当前版本和本地数据不受影响。",
    );
  }
  if (
    !isCheck &&
    /permission|access denied|operation not permitted|os error 5/i.test(raw)
  ) {
    return updateFailure(
      "permission",
      "无法写入或启动更新安装程序。请检查系统权限或安全软件拦截后重试；当前版本和本地数据不受影响。",
    );
  }
  if (
    !isCheck &&
    /temporary directory|temp directory|same mount|extract path/i.test(raw)
  ) {
    return updateFailure(
      "temporary-storage",
      "无法准备更新安装所需的临时目录。请检查磁盘空间和临时目录权限后重试；当前版本和本地数据不受影响。",
    );
  }
  if (!isCheck && /relaunch|restart/i.test(raw)) {
    return updateFailure(
      "relaunch",
      "更新已安装，但 Mine Mail 未能自动重新启动。请手动重新打开应用。",
    );
  }
  if (
    /network|connection|connect|failed to fetch|request failed|send(?:ing)? request|error.*request|offline|proxy/i.test(
      raw,
    )
  ) {
    return updateFailure(
      "connection",
      `无法连接更新服务器完成${subject}。请检查系统代理或网络后重试；当前版本和本地数据不受影响。`,
    );
  }
  if (
    isCheck &&
    /could not fetch a valid release json|release json.*remote/i.test(raw)
  ) {
    return updateFailure(
      "release-metadata",
      "更新服务器没有返回可用的最新版本信息。请稍后重试；当前版本和本地数据不受影响。",
    );
  }
  if (
    isCheck &&
    /platform.*not found|fallback platforms|unsupported application architecture|unsupported os/i.test(
      raw,
    )
  ) {
    return updateFailure(
      "platform",
      "最新版本信息不包含当前系统可用的更新包。请稍后重试；当前版本和本地数据不受影响。",
    );
  }
  if (
    isCheck &&
    /secure protocol|insecure transport|endpoint.*https/i.test(raw)
  ) {
    return updateFailure(
      "insecure-endpoint",
      "更新地址未使用安全连接，Mine Mail 已停止检查。请从 GitHub Release 手动安装新版后重试；当前版本和本地数据不受影响。",
    );
  }
  if (
    isCheck &&
    /latest\.json|manifest|json|deserialize|invalid update|expected value|missing field|invalid type|trailing characters/i.test(
      raw,
    )
  ) {
    return updateFailure(
      "manifest",
      "最新版本信息格式无效，Mine Mail 已停止更新。请稍后重试；当前版本和本地数据不受影响。",
    );
  }
  if (!isCheck && /invalid updater|binary.*archive/i.test(raw)) {
    return updateFailure(
      "package-format",
      "更新包格式无效，Mine Mail 已停止安装。请稍后重试；当前版本和本地数据不受影响。",
    );
  }
  if (!isCheck && /install|installer/i.test(raw)) {
    return updateFailure(
      "install",
      "更新包已下载，但安装程序未能完成安装。请检查系统权限后重试；当前版本和本地数据不受影响。",
    );
  }

  return updateFailure(
    "internal",
    isCheck
      ? "Mine Mail 内部处理更新检查失败。请重试；如果仍然失败，请重启应用。当前版本和本地数据不受影响。"
      : "Mine Mail 内部处理更新下载或安装失败。请重试；如果仍然失败，请重启应用。当前版本和本地数据不受影响。",
  );
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

export const __testing = { rawUpdateError, releaseNotes };
