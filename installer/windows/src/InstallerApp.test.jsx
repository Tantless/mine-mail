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
    expect(screen.getByText("让重要邮件，安静抵达。")).toBeTruthy();
    expect(screen.getByText(/AppData\\Local\\Mine Mail/)).toBeTruthy();
    expect(screen.getByRole("button", { name: /立即安装/ }).disabled).toBe(false);
  });

  it("moves through the real lifecycle states in one window", async () => {
    vi.useFakeTimers();
    render(<InstallerApp />);

    fireEvent.click(screen.getByRole("button", { name: /立即安装/ }));
    expect(
      screen.getByRole("heading", { name: "安装 Mine Mail" }),
    ).toBeTruthy();
    expect(screen.getByRole("progressbar", { name: "安装进行中" })).toBeTruthy();

    await act(async () => {
      await vi.runAllTimersAsync();
    });

    expect(screen.getByRole("heading", { name: "安装完成" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /打开 Mine Mail/ })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: /桌面图标/ }).checked).toBe(
      true,
    );
    expect(screen.getByRole("checkbox", { name: /开机自启/ }).checked).toBe(
      false,
    );
    expect(screen.queryByText("打开前，再决定两件小事")).toBeNull();
    expect(screen.queryByText(/^安装于/)).toBeNull();

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

    fireEvent.click(screen.getByRole("button", { name: /立即安装/ }));
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));

    expect(
      screen.getByRole("dialog", { name: "安装正在进行" }),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "继续安装" })).toBeTruthy();
  });
});
