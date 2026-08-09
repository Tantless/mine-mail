import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Toast } from "./Toast.jsx";

describe("Toast", () => {
  afterEach(cleanup);

  it.each([
    ["success", "操作已完成", "status"],
    ["error", "操作没有完成", "alert"],
    ["warning", "请先前往设置界面完成AGENT配置", "alert"],
  ])("omits the %s status icon", (tone, message, role) => {
    const { container } = render(
      <Toast toast={{ tone, message }} onClose={vi.fn()} />,
    );

    const toast = screen.getByRole(role);
    expect(toast.dataset.tone).toBe(tone);
    expect(toast.textContent).toContain(message);
    expect(container.querySelector(".toast__icon")).toBeNull();
  });

  it("keeps the informational icon and dismiss action", async () => {
    const onClose = vi.fn();
    const { container } = render(
      <Toast toast={{ tone: "info", message: "草稿已更新" }} onClose={onClose} />,
    );

    expect(container.querySelector(".toast__icon svg")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "关闭通知" }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
