// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import InstallerApp from "./InstallerApp";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("Mine Mail installer experience", () => {
  it("starts with the branded ready state and a custom install path", () => {
    render(<InstallerApp />);

    expect(
      screen.getByRole("heading", { name: "安装 Mine Mail" }),
    ).toBeTruthy();
    expect(screen.getByText("多账户邮件客户端")).toBeTruthy();
    expect(
      screen.getByRole("list", { name: "安装进度" }).textContent,
    ).toBe("准备安装完成");
    expect(screen.getByText(/AppData\\Local\\Mine Mail/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "安装" }).disabled).toBe(false);

    [
      "本地安装 · 安全可控",
      "让重要邮件，安静抵达。",
      "轻巧、专注的桌面邮件客户端。安装完成后即可连接你的邮箱。",
      "不修改系统级设置",
      "确认位置",
      "写入文件",
      "开始使用",
    ].forEach((copy) => expect(screen.queryByText(copy)).toBeNull());
  });

  it("moves through the real lifecycle states in one window", async () => {
    vi.useFakeTimers();
    render(<InstallerApp />);

    fireEvent.click(screen.getByRole("button", { name: "安装" }));
    expect(
      screen.getByRole("heading", { name: "正在安装 Mine Mail" }),
    ).toBeTruthy();
    expect(screen.getByText("正在检查安装环境…")).toBeTruthy();
    expect(
      screen
        .getByRole("progressbar", { name: "安装进行中" })
        .getAttribute("aria-valuetext"),
    ).toBe("正在检查安装环境…");
    expect(screen.queryByText("准备安装文件")).toBeNull();
    expect(screen.queryByText("正在为你准备")).toBeNull();
    expect(
      screen.queryByText("安装窗口可以最小化，请保持程序运行直到完成。"),
    ).toBeNull();

    await act(async () => {
      await vi.runAllTimersAsync();
    });

    expect(screen.getByRole("heading", { name: "安装完成" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /打开 Mine Mail/ })).toBeTruthy();
    expect(
      screen.getByRole("checkbox", { name: "桌面快捷方式" }).checked,
    ).toBe(true);
    expect(
      screen.getByRole("checkbox", { name: "开机时启动" }).checked,
    ).toBe(false);

    [
      "一切已经安放妥当",
      "从这里开始，每一封来信都有一个安静的归处。",
      "在桌面留下快捷入口",
      "登录 Windows 后静默启动",
    ].forEach((copy) => expect(screen.queryByText(copy)).toBeNull());

    const stepIcons = Array.from(
      document.querySelectorAll("[data-step-icon]"),
      (icon) => icon.getAttribute("data-step-icon"),
    );
    expect(stepIcons).toEqual(["ready", "installing", "success"]);
    expect(document.querySelector(".success-symbol")).toBeNull();
  });

  it("keeps close behavior inside the branded surface while installing", () => {
    vi.useFakeTimers();
    render(<InstallerApp />);

    fireEvent.click(screen.getByRole("button", { name: "安装" }));
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));

    expect(
      screen.getByRole("dialog", { name: "安装正在进行" }),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "继续安装" })).toBeTruthy();
  });
});
