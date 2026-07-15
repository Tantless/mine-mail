import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App.jsx";
import { mailApi } from "./services/mailApi.js";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function savedOutcome(request, draftId, expectedLocalVersion = null) {
  return {
    kind: "saved",
    draft: {
      ...request,
      id: draftId || "stable-draft-id",
      local_version: expectedLocalVersion === null ? 1 : expectedLocalVersion + 1,
      status: "local",
      updated_at: new Date().toISOString(),
    },
    canonical: null,
  };
}

describe("Mine Mail MVP", () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    cleanup();
  });

  it("loads the local inbox and opens the first message", async () => {
    render(<App />);

    expect(await screen.findAllByText("欢迎来到 Mine Mail")).toHaveLength(2);
    expect(screen.getByText(/我们希望它是一间安静的邮件工作室/)).toBeTruthy();
  });

  it("renders an integrated draggable titlebar with window controls", () => {
    render(<App />);

    const titlebar = screen.getByTestId("window-titlebar");
    expect(titlebar.getAttribute("data-tauri-drag-region")).toBe("deep");
    const minimizeButton = screen.getByRole("button", { name: "最小化窗口" });
    expect(
      minimizeButton
        .closest(".titlebar-controls")
        .getAttribute("data-tauri-drag-region"),
    ).toBe("false");
    expect(minimizeButton.getAttribute("aria-disabled")).toBe("true");
    expect(minimizeButton.tabIndex).toBe(-1);
    expect(
      screen.getByRole("button", { name: "最大化或还原窗口" }),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "关闭窗口" })).toBeTruthy();
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

    expect(await screen.findByRole("alertdialog")).toBeTruthy();
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
    vi.spyOn(mailApi, "sendDraft").mockResolvedValue({
      status: "delivery_unknown",
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.type(screen.getByLabelText("收件人"), "friend@example.com");
    await user.type(screen.getByLabelText("主题"), "不确定投递测试");
    await user.click(screen.getByRole("button", { name: "发送邮件" }));
    await screen.findByRole("alertdialog");
    await user.click(screen.getByRole("button", { name: "确认发送" }));

    expect(await screen.findByText(/投递结果未知/)).toBeTruthy();
    expect(screen.queryByText("邮件已经发送")).toBeNull();
  });

  it("debounces local draft persistence and reuses the returned draft id", async () => {
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");
    const saveDraft = vi
      .spyOn(mailApi, "saveDraft")
      .mockImplementation(async (request, draftId, expectedLocalVersion) =>
        savedOutcome(request, draftId, expectedLocalVersion),
      );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /写信/ }));
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "自动保存" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(901);
    });

    expect(saveDraft).toHaveBeenCalledTimes(1);
    expect(saveDraft.mock.calls[0][1]).toBeNull();
    expect(screen.getByText("已保存")).toBeTruthy();

    fireEvent.change(screen.getByLabelText("邮件正文"), {
      target: { value: "继续编辑" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(901);
    });

    expect(saveDraft).toHaveBeenCalledTimes(2);
    expect(saveDraft.mock.calls[1][1]).toBe("stable-draft-id");
  });

  it("keeps saving until the locked composer revision is persisted", async () => {
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");
    const firstSave = deferred();
    const secondSave = deferred();
    const saveDraft = vi
      .spyOn(mailApi, "saveDraft")
      .mockImplementationOnce(() => firstSave.promise)
      .mockImplementationOnce(() => secondSave.promise);

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /写信/ }));
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("收件人"), {
      target: { value: "friend@example.com" },
    });
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "第一版主题" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(901);
    });
    expect(saveDraft).toHaveBeenCalledTimes(1);
    vi.useRealTimers();

    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "最终持久化主题" },
    });
    fireEvent.click(screen.getByRole("button", { name: "发送邮件" }));
    expect(screen.getByLabelText("主题").disabled).toBe(true);

    await act(async () => {
      firstSave.resolve({
        ...savedOutcome(saveDraft.mock.calls[0][0], "stable-draft-id"),
      });
      await Promise.resolve();
    });
    await waitFor(() => expect(saveDraft).toHaveBeenCalledTimes(2));
    expect(saveDraft.mock.calls[1][0].subject).toBe("最终持久化主题");
    expect(saveDraft.mock.calls[1][1]).toBe("stable-draft-id");

    await act(async () => {
      secondSave.resolve({
        ...savedOutcome(
          saveDraft.mock.calls[1][0],
          "stable-draft-id",
          saveDraft.mock.calls[1][2],
        ),
      });
      await Promise.resolve();
    });

    expect(await screen.findByRole("alertdialog")).toBeTruthy();
    expect(screen.getByText("最终持久化主题")).toBeTruthy();
  });

  it("uses sync_all for the manual desktop refresh action", async () => {
    const syncAll = vi.spyOn(mailApi, "syncAll").mockResolvedValue({
      inbox: { fetched: 0 },
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: "同步收件箱" }));
    await waitFor(() => expect(syncAll).toHaveBeenCalledOnce());
  });

  it("saves the selected polling interval and opt-in autostart setting", async () => {
    const updateSettings = vi
      .spyOn(mailApi, "updateDesktopSettings")
      .mockImplementation(async (value) => value);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: "设置" }));
    await user.click(screen.getByRole("radio", { name: "3 分钟" }));
    await user.click(screen.getByRole("checkbox", { name: /开机启动/ }));
    await user.click(screen.getByRole("button", { name: "保存设置" }));

    await waitFor(() =>
      expect(updateSettings).toHaveBeenCalledWith({
        pollingIntervalMinutes: 3,
        autostartEnabled: true,
      }),
    );
  });

  it("opens and updates an existing draft with the same id", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");
    const saveDraft = vi
      .spyOn(mailApi, "saveDraft")
      .mockImplementation(async (request, draftId, expectedLocalVersion) =>
        savedOutcome(request, draftId, expectedLocalVersion),
      );

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("关于下周的主题评审"));
    expect(screen.getByRole("heading", { name: "编辑草稿" })).toBeTruthy();

    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "更新后的主题评审" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(901);
    });

    expect(saveDraft).toHaveBeenCalledWith(
      expect.objectContaining({ subject: "更新后的主题评审" }),
      "draft-welcome",
      1,
    );
  });

  it("persists an existing draft even after every field is cleared", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");
    const saveDraft = vi
      .spyOn(mailApi, "saveDraft")
      .mockImplementation(async (request, draftId, expectedLocalVersion) =>
        savedOutcome(request, draftId, expectedLocalVersion),
      );

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("关于下周的主题评审"));
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("收件人"), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText("主题"), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText("邮件正文"), { target: { value: "" } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(901);
    });

    expect(saveDraft).toHaveBeenCalledWith(
      { to: [], cc: [], bcc: [], subject: "", body_text: "" },
      "draft-welcome",
      1,
    );
  });

  it("shows account onboarding before touching mailbox APIs when unconfigured", async () => {
    vi.spyOn(mailApi, "getAccountStatus").mockResolvedValue({
      configured: false,
      provider: null,
      email: null,
      backendReady: false,
      credentialAvailable: false,
      startupError: null,
    });
    const listInbox = vi.spyOn(mailApi, "listInbox");
    render(<App />);

    expect(await screen.findByText("先连接你的邮箱")).toBeTruthy();
    expect(listInbox).not.toHaveBeenCalled();
  });

  it("keeps cached mail visible when credentials or network are unavailable", async () => {
    vi.spyOn(mailApi, "getAccountStatus").mockResolvedValue({
      configured: true,
      provider: "163",
      email: "me@163.com",
      backendReady: true,
      credentialAvailable: false,
      networkReady: false,
      startupError: "系统凭据不可用，请重新连接账户。",
    });
    const listInbox = vi.spyOn(mailApi, "listInbox");
    render(<App />);

    expect(await screen.findAllByText("欢迎来到 Mine Mail")).toHaveLength(2);
    expect(screen.getByRole("alert").textContent).toContain("系统凭据不可用");
    expect(screen.queryByText("先连接你的邮箱")).toBeNull();
    expect(listInbox).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "同步收件箱" }).disabled).toBe(true);
  });
});
