import { createServer } from "node:net";

import { describe, expect, it, vi } from "vitest";

import {
  canBindDevPort,
  configuredDevPort,
  createTauriDevOverride,
  parseOptionalDevPort,
  selectDevPort,
} from "./dev-port.mjs";

describe("开发端口配置", () => {
  it("未配置时使用默认端口", () => {
    expect(parseOptionalDevPort(undefined)).toBeUndefined();
    expect(configuredDevPort(undefined)).toBe(1420);
  });

  it("接受合法的显式端口", () => {
    expect(parseOptionalDevPort(" 1430 ")).toBe(1430);
    expect(configuredDevPort("1430")).toBe(1430);
  });

  it.each(["abc", "1420.5", "0", "65536"])("拒绝非法端口 %s", (value) => {
    expect(() => parseOptionalDevPort(value)).toThrow(
      "MINE_MAIL_DEV_PORT 必须是 1 至 65535 之间的整数",
    );
  });
});

describe("开发端口选择", () => {
  it("识别已被本机服务占用的端口", async () => {
    const server = createServer();
    await new Promise((resolve) => {
      server.listen({ host: "0.0.0.0", port: 0 }, resolve);
    });

    try {
      const address = server.address();
      expect(address).not.toBeNull();
      await expect(canBindDevPort(address.port)).resolves.toBe(false);
    } finally {
      await new Promise((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
      });
    }
  });

  it("默认端口被占用时选择后续端口", async () => {
    const probe = vi.fn(async (port) => port === 1422);

    await expect(
      selectDevPort(undefined, { probe, attempts: 3 }),
    ).resolves.toEqual({ port: 1422, source: "fallback" });
    expect(probe.mock.calls.map(([port]) => port)).toEqual([1420, 1421, 1422]);
  });

  it("显式端口被占用时不静默更换", async () => {
    await expect(
      selectDevPort("1430", { probe: async () => false }),
    ).rejects.toThrow("指定的开发端口 1430 已被占用");
  });

  it("找不到端口时给出已检查范围", async () => {
    await expect(
      selectDevPort(undefined, {
        probe: async () => false,
        defaultPort: 65534,
        attempts: 5,
      }),
    ).rejects.toThrow("65534 至 65535");
  });
});

describe("Tauri 开发配置", () => {
  const baseConfig = {
    build: {
      devUrl: "http://localhost:1420",
    },
    app: {
      security: {
        devCsp:
          "default-src 'self'; connect-src ipc: http://ipc.localhost http://localhost:1420 ws://localhost:1420; script-src 'self'",
      },
    },
  };

  it("同步覆盖 devUrl 和 CSP 中的 HTTP、WebSocket 来源", () => {
    expect(createTauriDevOverride(1423, baseConfig)).toEqual({
      build: {
        devUrl: "http://localhost:1423",
      },
      app: {
        security: {
          devCsp:
            "default-src 'self'; connect-src ipc: http://ipc.localhost http://localhost:1423 ws://localhost:1423; script-src 'self'",
        },
      },
    });
  });

  it("CSP 缺少开发来源时拒绝生成不一致配置", () => {
    expect(() =>
      createTauriDevOverride(1423, {
        ...baseConfig,
        app: { security: { devCsp: "default-src 'self'" } },
      }),
    ).toThrow("devCsp 未包含完整的开发服务器来源");
  });
});
