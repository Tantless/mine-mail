import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App.jsx";
import { bundledAppVersion } from "./services/appUpdate.js";
import { mailApi } from "./services/mailApi.js";
import { useProseMirrorTestGeometry } from "./test/proseMirrorTestGeometry.js";

useProseMirrorTestGeometry();

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

function mailboxPage(items, role = "inbox") {
  return {
    items: items.map((item) => ({ ...item, displayed_role: role })),
    next_cursor: null,
    has_more_local: false,
    remote_history_state: "complete",
    end_reached: true,
  };
}

async function setComposeBody(editor, value) {
  editor.textContent = value;
  fireEvent.input(editor, {
    inputType: value ? "insertText" : "deleteContentBackward",
    data: value || null,
  });
  await act(async () => {
    await Promise.resolve();
  });
}

function minimizeComposer(dialog = screen.getByRole("dialog")) {
  fireEvent.pointerDown(dialog.closest(".compose-layer"), {
    button: 0,
    clientX: 2,
    clientY: 2,
  });
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
    expect(
      await screen.findByText(/我们希望它是一间安静的邮件工作室/),
    ).toBeTruthy();
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

  it("keeps themed reply and Rust-prepared forward actions in the reader", async () => {
    render(<App />);

    const reply = await screen.findByRole("button", { name: "回复" });
    const forward = screen.getByRole("button", { name: "转发" });
    const actions = reply.closest(".message-actions");
    const reader = screen.getByRole("region", { name: "邮件阅读区" });

    expect(reader.classList.contains("reader-panel--message")).toBe(true);
    expect(reply.classList.contains("message-action-button")).toBe(true);
    expect(reply.classList.contains("message-action-button--reply")).toBe(true);
    expect(forward.classList.contains("message-forward-button")).toBe(true);
    expect(actions.classList.contains("message-actions--mail")).toBe(true);
    expect(actions.firstElementChild).toBe(reply);
    expect(actions.lastElementChild).toBe(forward);
  });

  it("keeps authored reply text separate from the read-only quoted message", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "回复" }));
    const composer = await screen.findByRole("dialog", { name: "新邮件" });
    const body = await within(composer).findByLabelText("邮件正文");
    expect(body.textContent).toBe("");
    expect(body.textContent).not.toContain("原邮件");

    const quote = within(composer).getByRole("button", {
      name: /欢迎来到 Mine Mail/,
    });
    expect(quote.getAttribute("aria-expanded")).toBe("false");
    await user.click(quote);
    expect(quote.getAttribute("aria-expanded")).toBe("true");
    expect(within(composer).getByText(/我们希望它是一间安静的邮件工作室/)).toBeTruthy();

    await user.type(body, "这是回复内容");
    expect(body.textContent).toBe("这是回复内容");
  });

  it("keeps routine backend health details out of the main interface", async () => {
    render(<App />);

    expect(await screen.findByText("demo@163.com")).toBeTruthy();
    expect(screen.queryByText("已连接")).toBeNull();
    expect(screen.queryByText("本地缓存已就绪")).toBeNull();
    expect(document.querySelector(".account-card__status")).toBeNull();
    expect(document.querySelector(".list-status")).toBeNull();
  });

  it("opens the settings account form from a sidebar add-account slot", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByText("demo@163.com");
    const addSlots = screen.getAllByRole("button", { name: /添加邮箱账户/ });
    expect(addSlots).toHaveLength(1);
    await user.click(addSlots[0]);

    expect(await screen.findByRole("region", { name: "设置" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "选择邮箱服务商" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /163 邮箱/ }));
    expect(await screen.findByRole("heading", { name: "连接163 邮箱" })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "邮箱地址" })).toBeTruthy();
  });

  it("returns from settings to the inbox when the active sidebar account is clicked", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByText("demo@163.com");
    await user.click(screen.getByRole("button", { name: "设置" }));
    expect(screen.getByRole("region", { name: "设置" })).toBeTruthy();

    const accountSwitcher = screen.getByLabelText("已登录邮箱账户");
    await user.click(
      within(accountSwitcher).getByRole("button", {
        name: "当前账户 demo@163.com",
      }),
    );

    expect(screen.queryByRole("region", { name: "设置" })).toBeNull();
    expect(
      screen
        .getByRole("button", { name: /^收件箱/ })
        .getAttribute("aria-current"),
    ).toBe("page");
  });

  it("paints a prewarmed mailbox immediately while the native account switch is pending", async () => {
    const user = userEvent.setup();
    const pendingSwitch = deferred();
    const accountA = {
      accountId: "account-a",
      provider: "163",
      email: "a@163.com",
      authentication: "password",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
    };
    const accountB = {
      accountId: "account-b",
      provider: "gmail",
      email: "b@gmail.com",
      authentication: "google_oauth",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
    };
    const statusA = {
      configured: true,
      ...accountA,
      activeAccountId: accountA.accountId,
      accounts: [accountA, accountB],
      accountCount: 2,
      maxAccounts: 3,
      canAddAccount: true,
    };
    const statusB = {
      ...statusA,
      ...accountB,
      activeAccountId: accountB.accountId,
    };
    const cachedMessage = {
      id: "account-b-message-202",
      displayed_role: "inbox",
      subject: "Gmail 本地缓存即时显示",
      sender: { name: "Google", email: "no-reply@google.com" },
      to: [{ name: null, email: "b@gmail.com" }],
      cc: [],
      sent_at: "2026-07-17T10:00:00Z",
      internal_date: "2026-07-17T10:00:00Z",
      flags: ["\\Seen"],
      size_bytes: 100,
      preview: "切换不等待网络同步",
      body_text: null,
      body_html: null,
      body_render_mode: "plain",
      body_segments: [],
      body_html_available: false,
      body_html_loaded: false,
      has_remote_images: false,
      attachment_names: [],
      body_fetched: false,
      synced_at: "2026-07-17T10:00:00Z",
    };

    vi.spyOn(mailApi, "getAccountStatus").mockResolvedValue(statusA);
    vi.spyOn(mailApi, "getMailboxCapabilities").mockResolvedValue([
      { role: "inbox", status: "available", retryable: false },
      { role: "sent", status: "available", retryable: false },
      { role: "archive", status: "available", retryable: false },
      { role: "trash", status: "available", retryable: false },
    ]);
    const listMailboxPage = vi
      .spyOn(mailApi, "listMailboxPage")
      .mockImplementation(async (accountId, role) =>
        mailboxPage(
          accountId === accountB.accountId && role === "inbox"
            ? [cachedMessage]
            : [],
          role,
        ),
      );
    vi.spyOn(mailApi, "loadOlderMailboxPage").mockImplementation(
      async (accountId, role) =>
        mailboxPage(
          accountId === accountB.accountId && role === "inbox"
            ? [cachedMessage]
            : [],
          role,
        ),
    );
    vi.spyOn(mailApi, "switchAccount").mockReturnValue(pendingSwitch.promise);
    const fetchMailboxMessage = vi
      .spyOn(mailApi, "fetchMailboxMessage")
      .mockResolvedValue({
        ...cachedMessage,
        body_text: "切换不等待网络同步",
        body_fetched: true,
      });

    render(<App />);
    await waitFor(() => {
      expect(listMailboxPage).toHaveBeenCalledWith(
        "account-b",
        "inbox",
        null,
        50,
        null,
      );
    });

    await user.click(screen.getByRole("button", { name: "切换到 b@gmail.com" }));
    expect(screen.getByRole("button", { name: "当前账户 b@gmail.com" })).toBeTruthy();
    expect(screen.getAllByText("Gmail 本地缓存即时显示").length).toBeGreaterThan(0);
    expect(fetchMailboxMessage).not.toHaveBeenCalledWith(
      "account-b-message-202",
    );

    pendingSwitch.resolve(statusB);
    await waitFor(() =>
      expect(fetchMailboxMessage).toHaveBeenCalledWith(
        "account-b-message-202",
      ),
    );
    expect(screen.queryByText("已切换到 b@gmail.com")).toBeNull();
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

  it("dismisses the theme picker before outside controls continue", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: "主题外观" }));
    expect(screen.getByRole("menu", { name: "选择主题" })).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "设置" }));
    expect(screen.queryByRole("menu", { name: "选择主题" })).toBeNull();
    const settings = screen.getByRole("region", { name: "设置" });

    await user.click(screen.getByRole("button", { name: "主题外观" }));
    expect(screen.getByRole("menu", { name: "选择主题" })).toBeTruthy();
    fireEvent.click(settings);
    expect(screen.queryByRole("menu", { name: "选择主题" })).toBeNull();
  });

  it("confirms the exact recipients once and releases the composer while Outbox sends", async () => {
    const delivery = deferred();
    const sendDraft = vi
      .spyOn(mailApi, "sendDraft")
      .mockReturnValue(delivery.promise);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.type(screen.getByLabelText("收件人"), "friend@example.com");
    await user.type(screen.getByLabelText("主题"), "MVP 测试邮件");
    await user.type(screen.getByLabelText("邮件正文"), "这是一封仅用于界面测试的邮件。");
    await user.click(screen.getByRole("button", { name: "发送邮件" }));

    await waitFor(() => expect(sendDraft).toHaveBeenCalledOnce());
    expect(sendDraft).toHaveBeenCalledWith(
      expect.any(String),
      expect.any(Number),
      ["friend@example.com"],
    );
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("dialog", { name: /邮件|草稿/ })).toBeNull();
    expect(
      screen
        .getByRole("button", { name: "发件队列，有邮件正在队列中处理" })
        .querySelector(".folder-nav__activity--outbox"),
    ).toBeTruthy();

    await act(async () => {
      delivery.resolve({ status: "sent" });
      await delivery.promise;
    });

    const sent = await screen.findByRole("button", {
      name: "已发送，有新增邮件",
    });
    expect(sent.querySelector(".folder-nav__new-dot")).toBeTruthy();
    await waitFor(() =>
      expect(
        screen.queryByRole("button", {
          name: "发件队列，有邮件正在队列中处理",
        }),
      ).toBeNull(),
    );

    await user.click(sent);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "已发送" })).toBeTruthy(),
    );
    expect(
      screen
        .getByRole("button", { name: "已发送" })
        .querySelector(".folder-nav__new-dot"),
    ).toBeNull();
  });

  it("keeps the stationery controls in the footer and controls send behavior", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    expect(screen.getByRole("toolbar", { name: "正文格式" })).toBeTruthy();
    expect(screen.getByRole("combobox", { name: "字体" })).toBeTruthy();
    expect(
      screen.queryByRole("radiogroup", { name: "信纸发送方式" }),
    ).toBeNull();

    await user.click(screen.getByRole("button", { name: "启用信纸" }));
    await user.click(screen.getByRole("radio", { name: "方格纸" }));

    const editor = screen.getByRole("textbox", { name: "邮件正文" });
    expect(editor.closest(".compose-editor-shell").dataset.stationery).toBe(
      "grid",
    );
    const sendTheme = screen.getByRole("radio", {
      name: "将信纸随邮件发送",
    });
    await user.click(sendTheme);
    expect(sendTheme.getAttribute("aria-checked")).toBe("true");

    await user.click(screen.getByRole("button", { name: "关闭信纸" }));
    expect(editor.closest(".compose-editor-shell").dataset.stationery).toBe(
      "none",
    );
    expect(
      screen.queryByRole("radiogroup", { name: "信纸发送方式" }),
    ).toBeNull();
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
    expect(screen.getByRole("button", { name: "移除抄送 copy@example.com" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "移除密送 private@example.com" })).toBeTruthy();
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
    expect(screen.getByRole("button", { name: "保存并最小化" })).toBeTruthy();
    expect(screen.queryByText("保存并最小化")).toBeNull();

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

    minimizeComposer(dialog);
    const minimizedDialog = screen.getByRole("dialog", { name: "季度计划" });
    const minimizedLayer = minimizedDialog.closest(".compose-layer");
    const restoreButton = screen.getByRole("button", {
      name: "还原写信窗口：季度计划",
    });

    expect(dialog.dataset.minimized).toBe("true");
    expect(dialog.dataset.windowMotion).toBe("minimizing");
    expect(minimizedLayer.dataset.minimized).toBe("true");
    expect(dialog.style.width).toBe("340px");
    expect(dialog.style.height).toBe("44px");
    expect(restoreButton.textContent).toBe("季度计划");
    expect(screen.getByRole("button", { name: "关闭写信窗口" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "最小化写信窗口" })).toBeNull();

    await user.click(restoreButton);
    expect(dialog.dataset.minimized).toBe("false");
    expect(dialog.dataset.windowMotion).toBe("restoring");
    expect(dialog.style.left).toBe(restoredGeometry.left);
    expect(dialog.style.top).toBe(restoredGeometry.top);
    expect(dialog.style.width).toBe(restoredGeometry.width);
    expect(dialog.style.height).toBe(restoredGeometry.height);

    await user.clear(screen.getByLabelText("主题"));
    minimizeComposer(dialog);
    await user.click(screen.getByRole("button", { name: "关闭写信窗口" }));
    expect(screen.queryByRole("dialog", { name: "新草稿" })).toBeNull();
    await user.click(screen.getByRole("button", { name: /写信/ }));

    const reopened = screen.getByRole("dialog", { name: "新邮件" });
    expect(reopened.style.left).toBe(`${persisted.x}px`);
    expect(reopened.style.top).toBe(`${persisted.y}px`);
    expect(reopened.style.width).toBe(`${persisted.width}px`);
    expect(reopened.style.height).toBe(`${persisted.height}px`);

    minimizeComposer(reopened);
    expect(
      screen.getByRole("button", { name: "还原写信窗口：新草稿" }).textContent,
    ).toBe("新草稿");
  });

  it("saves authored content before minimizing without a success toast", async () => {
    let persistedDraft = null;
    const listDrafts = vi
      .spyOn(mailApi, "listDrafts")
      .mockImplementation(async () => (persistedDraft ? [persistedDraft] : []));
    const saveDraft = vi
      .spyOn(mailApi, "saveDraft")
      .mockImplementation(async (request, draftId, expectedLocalVersion) => {
        const outcome = savedOutcome(
          request,
          draftId,
          expectedLocalVersion,
        );
        persistedDraft = outcome.draft;
        return outcome;
      });
    const syncDrafts = vi
      .spyOn(mailApi, "syncDrafts")
      .mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.type(screen.getByLabelText("主题"), "保存后缩略");
    await user.click(screen.getByRole("button", { name: "保存并最小化" }));

    await waitFor(() => expect(saveDraft).toHaveBeenCalledOnce());
    await waitFor(() => expect(syncDrafts).toHaveBeenCalledOnce());
    await waitFor(() =>
      expect(listDrafts.mock.calls.length).toBeGreaterThan(1),
    );
    const minimizedDialog = await screen.findByRole("dialog", {
      name: "保存后缩略",
    });
    expect(minimizedDialog.dataset.minimized).toBe("true");
    expect(
      screen.getByRole("button", { name: "还原写信窗口：保存后缩略" }),
    ).toBeTruthy();
    expect(screen.queryByText("草稿已保存到本地")).toBeNull();
    expect(document.querySelector(".toast")).toBeNull();
  });

  it("restores the minimized composer from the primary compose action", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: "写信" }));
    await user.type(screen.getByLabelText("主题"), "继续写这封邮件");
    const dialog = screen.getByRole("dialog", { name: "新邮件" });
    minimizeComposer(dialog);

    expect(dialog.dataset.minimized).toBe("true");
    await user.click(screen.getByRole("button", { name: "写信" }));

    expect(dialog.dataset.minimized).toBe("false");
    expect(dialog.dataset.windowMotion).toBe("restoring");
    expect(screen.getByLabelText("主题").value).toBe("继续写这封邮件");
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
  });

  it("minimizes an untouched composer without creating an empty draft", async () => {
    const saveDraft = vi.spyOn(mailApi, "saveDraft");
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.click(screen.getByRole("button", { name: "保存并最小化" }));

    const minimizedDialog = await screen.findByRole("dialog", { name: "新草稿" });
    expect(minimizedDialog.dataset.minimized).toBe("true");
    expect(saveDraft).not.toHaveBeenCalled();
  });

  it("keeps the composer expanded when saving before minimize fails", async () => {
    vi.spyOn(mailApi, "saveDraft").mockRejectedValue(new Error("本地写入失败"));
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.type(screen.getByLabelText("主题"), "保留当前编辑");
    await user.click(screen.getByRole("button", { name: "保存并最小化" }));

    expect((await screen.findByRole("alert")).textContent).toContain(
      "本地写入失败",
    );
    expect(
      screen.getByRole("dialog", { name: "新邮件" }).dataset.minimized,
    ).toBe("false");
    expect(screen.getByLabelText("主题").value).toBe("保留当前编辑");
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

    expect(await screen.findByText(/投递结果未知/)).toBeTruthy();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "已发送，有新增邮件" }),
    ).toBeNull();
  });

  it("debounces local draft persistence and reuses the returned draft id", async () => {
    const staleDraftList = deferred();
    const listDrafts = vi
      .spyOn(mailApi, "listDrafts")
      .mockReturnValueOnce(staleDraftList.promise)
      .mockResolvedValue([]);
    const saveDraft = vi
      .spyOn(mailApi, "saveDraft")
      .mockImplementation(async (request, draftId, expectedLocalVersion) =>
        savedOutcome(request, draftId, expectedLocalVersion),
      );
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");
    await waitFor(() => expect(listDrafts).toHaveBeenCalledTimes(1));

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /写信/ }));
    await screen.findByLabelText("邮件正文");
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

    vi.useRealTimers();
    await act(async () => {
      staleDraftList.resolve([]);
      await Promise.resolve();
    });
    expect(await screen.findByRole("dialog", { name: "编辑草稿" })).toBeTruthy();

    const composeBody = await screen.findByLabelText("邮件正文");
    vi.useFakeTimers();
    await setComposeBody(composeBody, "继续编辑");
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
    const sendDraft = vi
      .spyOn(mailApi, "sendDraft")
      .mockResolvedValue({ status: "sent" });

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /写信/ }));
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("收件人"), {
      target: { value: "friend@example.com" },
    });
    fireEvent.keyDown(screen.getByLabelText("收件人"), { key: "Enter" });
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

    await waitFor(() => expect(sendDraft).toHaveBeenCalledOnce());
    expect(sendDraft).toHaveBeenCalledWith(
      "stable-draft-id",
      expect.any(Number),
      ["friend@example.com"],
    );
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByLabelText("主题")).toBeNull();
  });

  it("syncs only the active semantic mailbox on manual refresh", async () => {
    const syncMailbox = vi
      .spyOn(mailApi, "syncMailbox")
      .mockResolvedValue({ synced: 3 });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: "同步收件箱" }));
    await waitFor(() =>
      expect(syncMailbox).toHaveBeenCalledWith("demo-primary", "inbox"),
    );
    expect(await screen.findByText("同步成功，新增 3 封邮件")).toBeTruthy();
    expect(screen.queryByText("收件箱同步完成")).toBeNull();
    expect(document.querySelector(".toast")).toBeNull();
    await waitFor(
      () =>
        expect(screen.queryByText("同步成功，新增 3 封邮件")).toBeNull(),
      { timeout: 2_500 },
    );
  });

  it("navigates the settings menu and saves function preferences", async () => {
    const updateSettings = vi
      .spyOn(mailApi, "updateDesktopSettings")
      .mockImplementation(async (value) => value);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: "设置" }));
    expect(screen.getByRole("navigation", { name: "设置菜单" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "账户与同步" })).toBeTruthy();
    await user.click(screen.getByRole("combobox", { name: "完整校准间隔" }));
    await user.click(screen.getByRole("option", { name: "3 分钟" }));
    await user.click(screen.getByRole("button", { name: /功能设定/ }));
    expect(
      screen.getByRole("button", { name: "了解自动加载远程图片的隐私风险" }),
    ).toBeTruthy();
    expect(screen.getByRole("tooltip").textContent).toContain("邮件打开时间");
    expect(screen.queryByText("前台也提醒")).toBeNull();
    await user.click(screen.getByRole("checkbox", { name: /桌面通知/ }));
    await user.click(screen.getByRole("checkbox", { name: /桌面通知/ }));
    await user.click(screen.getByRole("combobox", { name: "通知声音类型" }));
    await user.click(screen.getByRole("option", { name: "提醒提示" }));
    await user.click(screen.getByRole("combobox", { name: "远程图片加载方式" }));
    await user.click(screen.getByRole("option", { name: "每次询问" }));
    await user.click(screen.getByRole("checkbox", { name: /开机启动/ }));
    await user.click(screen.getByRole("button", { name: "关于 Mine Mail" }));
    expect(screen.getByText(`v${bundledAppVersion}`)).toBeTruthy();
    expect(screen.getByRole("button", { name: "检查更新" }).disabled).toBe(true);
    expect(screen.queryByRole("button", { name: "保存设置" })).toBeNull();

    await waitFor(() =>
      expect(updateSettings).toHaveBeenCalledWith({
        pollingIntervalMinutes: 3,
        autostartEnabled: true,
        notificationsEnabled: true,
        notificationSoundEnabled: true,
        notificationSound: "reminder",
        remoteImageMode: "ask",
      }),
    );
  });

  it("propagates a saved account remark to visible account provenance", async () => {
    const setAccountRemark = vi
      .spyOn(mailApi, "setAccountRemark")
      .mockResolvedValue({
        configured: true,
        accountId: "demo-primary",
        activeAccountId: "demo-primary",
        provider: "163",
        email: "demo@163.com",
        remark: "工作邮箱",
        backendReady: true,
        credentialAvailable: true,
        networkReady: true,
        accounts: [
          {
            accountId: "demo-primary",
            provider: "163",
            email: "demo@163.com",
            remark: "工作邮箱",
            authentication: "password",
            backendReady: true,
            credentialAvailable: true,
            networkReady: true,
          },
        ],
        accountCount: 1,
        maxAccounts: 3,
        canAddAccount: true,
        googleOauthConfigured: true,
      });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: "设置" }));
    await user.click(
      await screen.findByRole("button", { name: "管理 demo@163.com" }),
    );
    await user.click(await screen.findByRole("menuitem", { name: "添加备注" }));
    const remarkEditor = await screen.findByRole("form", {
      name: "demo@163.com 账户备注",
    });
    await user.type(
      within(remarkEditor).getByRole("textbox", { name: "账户备注" }),
      "工作邮箱",
    );
    await user.click(
      within(remarkEditor).getByRole("button", { name: "保存" }),
    );

    await waitFor(() =>
      expect(setAccountRemark).toHaveBeenCalledWith(
        "demo-primary",
        "工作邮箱",
      ),
    );
    expect(
      screen.getByRole("button", {
        name: "当前账户 工作邮箱 demo@163.com",
      }),
    ).toBeTruthy();
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
    await user.click(await screen.findByText("关于下周的主题评审"));
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

  it("persists stationery delivery when sending an existing plain draft", async () => {
    const user = userEvent.setup();
    const sendDraft = vi
      .spyOn(mailApi, "sendDraft")
      .mockResolvedValue({ status: "sent" });
    const saveDraft = vi
      .spyOn(mailApi, "saveDraft")
      .mockImplementation(async (request, draftId, expectedLocalVersion) =>
        savedOutcome(request, draftId, expectedLocalVersion),
      );
    render(<App />);
    await screen.findAllByText("欢迎来到 Mine Mail");

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(await screen.findByText("关于下周的主题评审"));
    await user.click(screen.getByRole("button", { name: "启用信纸" }));
    await user.click(
      screen.getByRole("radio", { name: "将信纸随邮件发送" }),
    );
    await user.click(screen.getByRole("button", { name: "发送邮件" }));

    await waitFor(() => expect(sendDraft).toHaveBeenCalledOnce());
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(saveDraft).toHaveBeenLastCalledWith(
      expect.objectContaining({
        format: expect.objectContaining({
          stationery: "lined",
          send_stationery: true,
        }),
      }),
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
    await user.click(await screen.findByText("关于下周的主题评审"));
    vi.useFakeTimers();
    fireEvent.click(
      screen.getByRole("button", { name: "移除收件人 linxia@example.com" }),
    );
    fireEvent.change(screen.getByLabelText("主题"), { target: { value: "" } });
    await setComposeBody(screen.getByLabelText("邮件正文"), "");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(901);
    });

    expect(saveDraft).toHaveBeenCalledWith(
      {
        to: [],
        cc: [],
        bcc: [],
        subject: "",
        body_text: "",
        format: {
          body_html: null,
          stationery: "none",
          send_stationery: false,
        },
        reply_context: null,
      },
      "draft-welcome",
      1,
    );
  });

  it("uses the main shell as the unconfigured account state", async () => {
    const user = userEvent.setup();
    vi.spyOn(mailApi, "getAccountStatus").mockResolvedValue({
      configured: false,
      provider: null,
      email: null,
      backendReady: false,
      credentialAvailable: false,
      startupError: null,
    });
    const listMailboxPage = vi.spyOn(mailApi, "listMailboxPage");
    render(<App />);

    const emptyWorkspace = await screen.findByRole("region", {
      name: "尚未连接邮箱",
    });
    expect(emptyWorkspace.classList.contains("account-empty-workspace")).toBe(
      true,
    );
    expect(emptyWorkspace.querySelector(".reader-idle")).toBeTruthy();
    expect(screen.getByLabelText("邮箱导航")).toBeTruthy();
    expect(screen.queryByLabelText("收件箱邮件列表")).toBeNull();
    expect(document.querySelector(".reader-panel")).toBeNull();
    expect(screen.queryByText("先连接你的邮箱")).toBeNull();
    expect(listMailboxPage).not.toHaveBeenCalled();

    await user.click(
      within(emptyWorkspace).getByRole("button", { name: "连接邮箱" }),
    );
    expect(
      screen.getByRole("heading", { name: "选择邮箱服务商" }),
    ).toBeTruthy();
  });

  it("keeps account repair inside the main shell and opens the existing provider", async () => {
    const user = userEvent.setup();
    vi.spyOn(mailApi, "getAccountStatus").mockResolvedValue({
      configured: true,
      accountId: "repair-account",
      activeAccountId: "repair-account",
      provider: "163",
      email: "repair@163.com",
      backendReady: false,
      credentialAvailable: false,
      networkReady: false,
      startupError: "授权信息需要更新。",
      accounts: [
        {
          accountId: "repair-account",
          provider: "163",
          email: "repair@163.com",
          backendReady: false,
          credentialAvailable: false,
          networkReady: false,
        },
      ],
    });
    render(<App />);

    const emptyWorkspace = await screen.findByRole("region", {
      name: "账户需要重新连接",
    });
    await user.click(
      within(emptyWorkspace).getByRole("button", { name: "修复账户" }),
    );

    expect(
      screen.getByRole("heading", { name: "连接163 邮箱" }),
    ).toBeTruthy();
    expect(screen.getByDisplayValue("repair@163.com")).toBeTruthy();
    expect(screen.getByRole("button", { name: "更新账户" })).toBeTruthy();
  });

  it("keeps cached mail visible when credentials or network are unavailable", async () => {
    vi.spyOn(mailApi, "getAccountStatus").mockResolvedValue({
      configured: true,
      accountId: "offline-account",
      activeAccountId: "offline-account",
      provider: "163",
      email: "me@163.com",
      backendReady: true,
      credentialAvailable: false,
      networkReady: false,
      startupError: "系统凭据不可用，请重新连接账户。",
      accounts: [
        {
          accountId: "offline-account",
          provider: "163",
          email: "me@163.com",
          backendReady: true,
          credentialAvailable: false,
          networkReady: false,
        },
      ],
    });
    vi.spyOn(mailApi, "getMailboxCapabilities").mockResolvedValue([
      { role: "inbox", status: "available", retryable: false },
      { role: "sent", status: "available", retryable: false },
      { role: "archive", status: "available", retryable: false },
      { role: "trash", status: "available", retryable: false },
    ]);
    const cachedMessage = {
      id: "offline-message-1",
      displayed_role: "inbox",
      subject: "欢迎来到 Mine Mail",
      sender: { name: "Mine Mail 团队", email: "hello@minemail.app" },
      to: [],
      cc: [],
      sent_at: "2026-07-17T10:00:00Z",
      flags: ["\\Seen"],
      preview: "本地缓存仍可阅读",
      body_fetched: false,
    };
    const listMailboxPage = vi
      .spyOn(mailApi, "listMailboxPage")
      .mockImplementation(async (_, role) =>
        mailboxPage(role === "inbox" ? [cachedMessage] : [], role),
      );
    render(<App />);

    expect(await screen.findAllByText("欢迎来到 Mine Mail")).toHaveLength(2);
    expect(
      await screen.findByText(
        "系统凭据不可用，请重新连接账户。",
        {},
        { timeout: 1600 },
      ),
    ).toBeTruthy();
    expect(screen.queryByText("尚未连接邮箱")).toBeNull();
    expect(listMailboxPage).toHaveBeenCalledWith(
      "offline-account",
      "inbox",
      null,
      50,
      null,
    );
    expect(screen.getByRole("button", { name: "同步收件箱" }).disabled).toBe(true);
  });
});
