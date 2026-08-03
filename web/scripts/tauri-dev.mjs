import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  createTauriDevOverride,
  DEFAULT_DEV_PORT,
  selectDevPort,
} from "./dev-port.mjs";

const tauriConfigUrl = new URL("../src-tauri/tauri.conf.json", import.meta.url);

export function npmInvocation(environment = process.env) {
  if (environment.npm_execpath) {
    return {
      command: process.execPath,
      prefixArgs: [environment.npm_execpath],
    };
  }

  return {
    command: process.platform === "win32" ? "npm.cmd" : "npm",
    prefixArgs: [],
  };
}

function waitForChild(child) {
  return new Promise((resolveChild, rejectChild) => {
    child.once("error", rejectChild);
    child.once("exit", (code, signal) => {
      resolveChild({ code, signal });
    });
  });
}

export async function runTauriDev(args = process.argv.slice(2)) {
  const selected = await selectDevPort(process.env.MINE_MAIL_DEV_PORT);
  const baseConfig = JSON.parse(await readFile(tauriConfigUrl, "utf8"));
  const override = createTauriDevOverride(selected.port, baseConfig);
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "mine-mail-tauri-dev-"));
  const overridePath = join(temporaryDirectory, "tauri.dev.conf.json");
  await writeFile(overridePath, `${JSON.stringify(override, null, 2)}\n`, "utf8");

  if (selected.source === "fallback") {
    console.log(
      `[Mine Mail] 默认端口 ${DEFAULT_DEV_PORT} 已被占用，改用开发端口 ${selected.port}。`,
    );
  } else {
    console.log(`[Mine Mail] 使用开发端口 ${selected.port}。`);
  }

  const invocation = npmInvocation();
  const child = spawn(
    invocation.command,
    [
      ...invocation.prefixArgs,
      "run",
      "tauri",
      "--",
      "dev",
      "--config",
      overridePath,
      ...args,
    ],
    {
      env: {
        ...process.env,
        MINE_MAIL_DEV_PORT: String(selected.port),
      },
      stdio: "inherit",
    },
  );

  const retainParentForCleanup = () => {};
  process.on("SIGINT", retainParentForCleanup);
  process.on("SIGTERM", retainParentForCleanup);

  try {
    const result = await waitForChild(child);
    if (result.signal) {
      return 1;
    }
    return result.code ?? 1;
  } finally {
    process.off("SIGINT", retainParentForCleanup);
    process.off("SIGTERM", retainParentForCleanup);
    await rm(temporaryDirectory, { recursive: true, force: true });
  }
}

const launchedDirectly =
  process.argv[1] &&
  pathToFileURL(resolve(process.argv[1])).href ===
    pathToFileURL(fileURLToPath(import.meta.url)).href;

if (launchedDirectly) {
  runTauriDev()
    .then((exitCode) => {
      process.exitCode = exitCode;
    })
    .catch((error) => {
      console.error(
        `[Mine Mail] ${error instanceof Error ? error.message : String(error)}`,
      );
      process.exitCode = 1;
    });
}
