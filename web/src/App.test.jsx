import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App.jsx";
import { mailApi } from "./services/mailApi.js";

describe("Mine Mail MVP", () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });

  afterEach(() => {
    vi.restoreAllMocks();
    cleanup();
  });

  it("loads the local inbox and opens the first message", async () => {
    render(<App />);

    expect(await screen.findAllByText("欢迎来到 Mine Mail")).toHaveLength(2);
    expect(screen.getByText(/我们希望它是一间安静的邮件工作室/)).toBeTruthy();
  });

  it("switches and persists an MVP theme", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: "主题外观" }));
    await user.click(screen.getByRole("menuitemradio", { name: "夜间" }));

    expect(document.documentElement.dataset.theme).toBe("night");
    expect(window.localStorage.getItem("mine-mail-theme")).toBe("night");
  });

  it("requires recipient confirmation before sending", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.type(screen.getByLabelText("收件人"), "friend@example.com");
    await user.type(screen.getByLabelText("主题"), "MVP 测试邮件");
    await user.type(screen.getByLabelText("邮件正文"), "这是一封仅用于界面测试的邮件。");
    await user.click(screen.getByRole("button", { name: "发送邮件" }));

    expect(screen.getByRole("alertdialog")).toBeTruthy();
    expect(screen.getByText("friend@example.com")).toBeTruthy();
    expect(screen.getByText("MVP 测试邮件")).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "返回修改" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("filters the inbox by a search query", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.type(screen.getByLabelText("搜索邮件"), "Figma");

    await waitFor(() => {
      expect(screen.getByText("Your July receipt")).toBeTruthy();
      expect(screen.queryByText("周五的产品评审")).toBeNull();
    });
  });

  it("does not report an uncertain delivery as sent", async () => {
    vi.spyOn(mailApi, "sendCompose").mockResolvedValue({
      status: "delivery_unknown",
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.type(screen.getByLabelText("收件人"), "friend@example.com");
    await user.type(screen.getByLabelText("主题"), "不确定投递测试");
    await user.click(screen.getByRole("button", { name: "发送邮件" }));
    await user.click(screen.getByRole("button", { name: "确认发送" }));

    expect(await screen.findByText(/投递结果未知/)).toBeTruthy();
    expect(screen.queryByText("邮件已经发送")).toBeNull();
  });
});
