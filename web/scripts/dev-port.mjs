import { createServer } from "node:net";

export const DEFAULT_DEV_PORT = 1420;
export const AUTO_PORT_ATTEMPTS = 100;

export function parseOptionalDevPort(value) {
  if (value === undefined || value === null || String(value).trim() === "") {
    return undefined;
  }

  const normalized = String(value).trim();
  if (!/^\d+$/.test(normalized)) {
    throw new Error(
      `MINE_MAIL_DEV_PORT 必须是 1 至 65535 之间的整数，当前值为“${normalized}”。`,
    );
  }

  const port = Number(normalized);
  if (!Number.isSafeInteger(port) || port < 1 || port > 65535) {
    throw new Error(
      `MINE_MAIL_DEV_PORT 必须是 1 至 65535 之间的整数，当前值为“${normalized}”。`,
    );
  }

  return port;
}

export function configuredDevPort(value) {
  return parseOptionalDevPort(value) ?? DEFAULT_DEV_PORT;
}

export async function canBindDevPort(port) {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.unref();
    server.once("error", (error) => {
      if (error.code === "EADDRINUSE" || error.code === "EACCES") {
        resolve(false);
        return;
      }
      reject(error);
    });
    server.listen({ host: "0.0.0.0", port, exclusive: true }, () => {
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve(true);
      });
    });
  });
}

export async function selectDevPort(
  configuredValue,
  {
    probe = canBindDevPort,
    defaultPort = DEFAULT_DEV_PORT,
    attempts = AUTO_PORT_ATTEMPTS,
  } = {},
) {
  const configuredPort = parseOptionalDevPort(configuredValue);
  if (configuredPort !== undefined) {
    if (!(await probe(configuredPort))) {
      throw new Error(
        `指定的开发端口 ${configuredPort} 已被占用，请更换 MINE_MAIL_DEV_PORT 后重试。`,
      );
    }
    return { port: configuredPort, source: "configured" };
  }

  const lastPort = Math.min(65535, defaultPort + attempts - 1);
  for (let port = defaultPort; port <= lastPort; port += 1) {
    if (await probe(port)) {
      return {
        port,
        source: port === defaultPort ? "default" : "fallback",
      };
    }
  }

  throw new Error(
    `未能在 ${defaultPort} 至 ${lastPort} 范围内找到可用的开发端口。`,
  );
}

export function createTauriDevOverride(port, baseConfig) {
  const baseDevUrl = baseConfig?.build?.devUrl;
  const baseDevCsp = baseConfig?.app?.security?.devCsp;
  if (typeof baseDevUrl !== "string" || typeof baseDevCsp !== "string") {
    throw new Error("tauri.conf.json 缺少 build.devUrl 或 app.security.devCsp。");
  }

  const devUrl = new URL(baseDevUrl);
  const basePort = devUrl.port;
  if (!basePort) {
    throw new Error("tauri.conf.json 的 build.devUrl 必须包含显式端口。");
  }

  const httpOrigin = `${devUrl.protocol}//${devUrl.hostname}:${basePort}`;
  const websocketProtocol = devUrl.protocol === "https:" ? "wss:" : "ws:";
  const websocketOrigin = `${websocketProtocol}//${devUrl.hostname}:${basePort}`;
  if (!baseDevCsp.includes(httpOrigin) || !baseDevCsp.includes(websocketOrigin)) {
    throw new Error(
      "tauri.conf.json 的 app.security.devCsp 未包含完整的开发服务器来源。",
    );
  }

  devUrl.port = String(port);
  const selectedHttpOrigin = `${devUrl.protocol}//${devUrl.hostname}:${port}`;
  const selectedWebsocketOrigin = `${websocketProtocol}//${devUrl.hostname}:${port}`;

  return {
    build: {
      devUrl: devUrl.toString().replace(/\/$/, ""),
    },
    app: {
      security: {
        devCsp: baseDevCsp
          .replaceAll(httpOrigin, selectedHttpOrigin)
          .replaceAll(websocketOrigin, selectedWebsocketOrigin),
      },
    },
  };
}
