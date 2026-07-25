import { describe, expect, it } from "vitest";
import tauriConfig from "../../src-tauri/tauri.conf.json";
import {
  __testing,
  appUpdateApi,
  bundledAppVersion,
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
});
