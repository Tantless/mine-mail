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
    const searchInput = screen.getByLabelText("搜索邮件");
    expect(searchInput.closest(".inset-input-shell")).toBeTruthy();
    expect(screen.queryByText("Ctrl K")).toBeNull();
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(document.activeElement).toBe(searchInput);
  });

  it("renders integrated draggable window chrome without a duplicate brand", () => {
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
    expect(titlebar.querySelector(".titlebar-brand")).toBeNull();
    expect(screen.getAllByText("Mine Mail")).toHaveLength(1);
  });

  it("places themed reply and forward actions at opposite sides of the reader", async () => {
    render(<App />);

    const reply = await screen.findByRole("button", { name: "回复" });
    const forward = screen.getByRole("button", { name: "转发" });
    const actions = reply.closest(".message-actions");

    expect(reply.classList.contains("message-action-button")).toBe(true);
    expect(reply.classList.contains("message-action-button--reply")).toBe(true);
    expect(forward.classList.contains("message-forward-button")).toBe(true);
    expect(forward.classList.contains("message-action-button")).toBe(false);
    expect(forward.textContent).toBe("");
    expect(actions.classList.contains("message-actions--mail")).toBe(true);
    expect(actions.lastElementChild).toBe(forward);
  });

  it("keeps routine backend health details out of the main interface", async () => {
    render(<App />);

    expect(await screen.findByText("demo@163.com")).toBeTruthy();
    expect(screen.queryByText("已连接")).toBeNull();
    expect(screen.queryByText("本地缓存已就绪")).toBeNull();
    expect(document.querySelector(".account-card__status")).toBeNull();
    expect(document.querySelector(".list-status")).toBeNull();
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

  it("toggles copy recipients without losing their values", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    const recipient = screen.getByLabelText("收件人");
    const expandCopies = screen.getByRole("button", { name: "展开抄送和密送" });

    expect(recipient.closest(".compose-input-shell.inset-input-shell")).toBeTruthy();
    expect(expandCopies.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByLabelText("抄送")).toBeNull();
    expect(screen.queryByLabelText("密送")).toBeNull();

    await user.click(expandCopies);
    const cc = screen.getByLabelText("抄送");
    const bcc = screen.getByLabelText("密送");
    expect(screen.getByRole("button", { name: "收起抄送和密送" })).toBeTruthy();
    expect(cc.closest(".compose-input-shell")).toBeTruthy();
    expect(bcc.closest(".compose-input-shell")).toBeTruthy();

    await user.type(cc, "copy@example.com");
    await user.type(bcc, "private@example.com");
    await user.click(screen.getByRole("button", { name: "收起抄送和密送" }));
    expect(screen.queryByLabelText("抄送")).toBeNull();
    expect(screen.queryByLabelText("密送")).toBeNull();

    await user.click(screen.getByRole("button", { name: "展开抄送和密送" }));
    expect(screen.getByLabelText("抄送").value).toBe("copy@example.com");
    expect(screen.getByLabelText("密送").value).toBe("private@example.com");
  });

  it("moves, resizes, persists, minimizes, and restores the compose surface", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");
    await user.click(screen.getByRole("button", { name: /写信/ }));

    const dialog = screen.getByRole("dialog", { name: "新邮件" });
    const dragSurface = dialog.querySelector(".compose-drag-surface");
    const initialLeft = Number.parseFloat(dialog.style.left);
    const initialTop = Number.parseFloat(dialog.style.top);

    expect(screen.queryByRole("button", { name: "展开写信窗口" })).toBeNull();
    expect(screen.getByRole("button", { name: "保存并关闭" })).toBeTruthy();
    expect(screen.queryByText("保存并关闭")).toBeNull();

    fireEvent.pointerDown(dragSurface, {
      button: 0,
      clientX: 500,
      clientY: 120,
      pointerId: 1,
    });
    fireEvent.pointerMove(window, {
      clientX: 450,
      clientY: 90,
      pointerId: 1,
    });
    fireEvent.pointerUp(window, { pointerId: 1 });

    expect(Number.parseFloat(dialog.style.left)).toBeLessThan(initialLeft);
    expect(Number.parseFloat(dialog.style.top)).toBeLessThan(initialTop);

    const initialWidth = Number.parseFloat(dialog.style.width);
    const initialHeight = Number.parseFloat(dialog.style.height);
    const resizeHandle = dialog.querySelector('[data-resize-direction="se"]');
    fireEvent.pointerDown(resizeHandle, {
      button: 0,
      clientX: 850,
      clientY: 650,
      pointerId: 2,
    });
    fireEvent.pointerMove(window, {
      clientX: 900,
      clientY: 700,
      pointerId: 2,
    });
    fireEvent.pointerUp(window, { pointerId: 2 });

    expect(Number.parseFloat(dialog.style.width)).toBeGreaterThan(initialWidth);
    expect(Number.parseFloat(dialog.style.height)).toBeGreaterThan(initialHeight);

    const persisted = JSON.parse(
      window.localStorage.getItem("mine-mail-compose-geometry-v1"),
    );
    expect(persisted.width).toBe(Number.parseFloat(dialog.style.width));
    expect(persisted.height).toBe(Number.parseFloat(dialog.style.height));

    await user.type(screen.getByLabelText("主题"), "季度计划");
    const restoredGeometry = {
      left: dialog.style.left,
      top: dialog.style.top,
      width: dialog.style.width,
      height: dialog.style.height,
    };

    await user.click(screen.getByRole("button", { name: "最小化写信窗口" }));
    const minimizedDialog = screen.getByRole("dialog", { name: "季度计划" });
    const minimizedLayer = minimizedDialog.closest(".compose-layer");
    const restoreButton = screen.getByRole("button", {
      name: "还原写信窗口：季度计划",
    });

    expect(dialog.dataset.minimized).toBe("true");
    expect(minimizedLayer.dataset.minimized).toBe("true");
    expect(dialog.style.width).toBe("340px");
    expect(dialog.style.height).toBe("44px");
    expect(restoreButton.textContent).toBe("季度计划");
    expect(screen.queryByRole("button", { name: "关闭写信窗口" })).toBeNull();
    expect(screen.queryByRole("button", { name: "最小化写信窗口" })).toBeNull();

    await user.click(restoreButton);
    expect(dialog.dataset.minimized).toBe("false");
    expect(dialog.style.left).toBe(restoredGeometry.left);
    expect(dialog.style.top).toBe(restoredGeometry.top);
    expect(dialog.style.width).toBe(restoredGeometry.width);
    expect(dialog.style.height).toBe(restoredGeometry.height);

    await user.clear(screen.getByLabelText("主题"));
    await user.click(screen.getByRole("button", { name: "关闭写信窗口" }));
    expect(screen.queryByRole("dialog", { name: "新邮件" })).toBeNull();
    await user.click(screen.getByRole("button", { name: /写信/ }));

    const reopened = screen.getByRole("dialog", { name: "新邮件" });
    expect(reopened.style.left).toBe(`${persisted.x}px`);
    expect(reopened.style.top).toBe(`${persisted.y}px`);
    expect(reopened.style.width).toBe(`${persisted.width}px`);
    expect(reopened.style.height).toBe(`${persisted.height}px`);

    await user.click(screen.getByRole("button", { name: "最小化写信窗口" }));
    expect(
      screen.getByRole("button", { name: "还原写信窗口：新邮件" }).textContent,
    ).toBe("新邮件");
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
    expect(
      screen.getByRole("button", { name: "了解自动加载远程图片的隐私风险" }),
    ).toBeTruthy();
    expect(screen.getByRole("tooltip").textContent).toContain("邮件打开时间");
    await user.click(screen.getByRole("radio", { name: "3 分钟" }));
    await user.click(screen.getByRole("radio", { name: "每次询问" }));
    await user.click(screen.getByRole("checkbox", { name: /开机启动/ }));
    await user.click(screen.getByRole("button", { name: "保存设置" }));

    await waitFor(() =>
      expect(updateSettings).toHaveBeenCalledWith({
        pollingIntervalMinutes: 3,
        autostartEnabled: true,
        remoteImageMode: "ask",
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
