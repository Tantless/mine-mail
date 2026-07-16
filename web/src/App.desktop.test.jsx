import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const desktop = vi.hoisted(() => {
  const listeners = new Map();
  return {
    listeners,
    mailApi: {
      listInbox: vi.fn(),
      fetchMessage: vi.fn(),
      openExternalUrl: vi.fn(),
      listDrafts: vi.fn(),
      saveDraft: vi.fn(),
      deleteDraft: vi.fn(),
      syncDrafts: vi.fn(),
      syncAll: vi.fn(),
      completeExit: vi.fn(),
      cancelExit: vi.fn(),
      listOutbox: vi.fn(),
      retryOutbox: vi.fn(),
      sendDraft: vi.fn(),
      checkConnections: vi.fn(),
      getDesktopSettings: vi.fn(),
      updateDesktopSettings: vi.fn(),
      listAccountPresets: vi.fn(),
      getAccountStatus: vi.fn(),
      configureAccount: vi.fn(),
      listProfileAvatars: vi.fn(),
      saveProfileAvatar: vi.fn(),
      deleteProfileAvatar: vi.fn(),
      onMailEvent: vi.fn(async (name, handler) => {
        listeners.set(name, handler);
        return () => listeners.delete(name);
      }),
    },
  };
});

vi.mock("./services/mailApi.js", () => ({
  isTauri: true,
  isTauriRuntime: true,
  isUnsupportedRuntime: false,
  mailApi: desktop.mailApi,
}));

import { App } from "./App.jsx";

function deferred() {
  let resolve;
  const promise = new Promise((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function savedOutcome(request, draftId, expectedLocalVersion = null) {
  return {
    kind: "saved",
    draft: {
      ...request,
      id: draftId || "exit-draft",
      local_version: expectedLocalVersion === null ? 1 : expectedLocalVersion + 1,
      status: "local",
      updated_at: "2026-07-14T09:10:00Z",
    },
    canonical: null,
  };
}

function summary(uid, subject) {
  return {
    id: uid,
    uid,
    subject,
    sender: { name: `Sender ${uid}`, email: `sender${uid}@example.com` },
    to: [],
    cc: [],
    sent_at: "2026-07-14T09:00:00Z",
    flags: [],
    preview: `${subject} preview`,
    body_text: null,
    attachment_names: [],
    body_fetched: false,
  };
}

function draftSnapshot(localVersion, subject, bodyText = "Draft body") {
  return {
    id: "shared-draft",
    local_version: localVersion,
    has_unsupported_content: false,
    to: ["friend@example.com"],
    cc: [],
    bcc: [],
    subject,
    body_text: bodyText,
    status: "synced",
    remote_mailbox: "Drafts",
    remote_uid: 17,
    created_at: "2026-07-14T08:00:00Z",
    updated_at: `2026-07-14T08:0${localVersion}:00Z`,
  };
}

describe("Mine Mail desktop state bridge", () => {
  beforeEach(() => {
    desktop.listeners.clear();
    Object.values(desktop.mailApi).forEach((mock) => mock.mockClear());
    desktop.mailApi.listInbox.mockResolvedValue([summary(1, "First mail")]);
    desktop.mailApi.listDrafts.mockResolvedValue([]);
    desktop.mailApi.listOutbox.mockResolvedValue([
      {
        id: "outbox-1",
        draft_id: null,
        recipients: ["friend@example.com"],
        status: "retryable",
        attempts: 1,
        last_error: "Temporary failure",
        created_at: "2026-07-14T09:00:00Z",
        sent_at: null,
      },
    ]);
    desktop.mailApi.getDesktopSettings.mockResolvedValue({
      pollingIntervalMinutes: 5,
      autostartEnabled: false,
      remoteImageMode: "automatic",
    });
    desktop.mailApi.listAccountPresets.mockResolvedValue([]);
    desktop.mailApi.getAccountStatus.mockResolvedValue({
      configured: true,
      provider: "163",
      email: "me@163.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
      startupError: null,
    });
    desktop.mailApi.listProfileAvatars.mockResolvedValue([]);
    desktop.mailApi.saveProfileAvatar.mockImplementation(async (request) => ({
      ownerType: request.ownerType,
      ownerKey: request.ownerKey,
      imageDataUrl: request.imageDataUrl,
    }));
    desktop.mailApi.deleteProfileAvatar.mockResolvedValue(undefined);
    desktop.mailApi.checkConnections.mockResolvedValue({
      imap_ok: true,
      smtp_ok: true,
    });
    desktop.mailApi.fetchMessage.mockImplementation(async (uid) => ({
      ...summary(uid, "First mail"),
      body_text: "Loaded body",
      body_fetched: true,
    }));
    desktop.mailApi.openExternalUrl.mockResolvedValue(true);
    desktop.mailApi.syncAll.mockResolvedValue({ inbox: { fetched: 0 } });
    desktop.mailApi.deleteDraft.mockResolvedValue({ kind: "deleted" });
    desktop.mailApi.completeExit.mockResolvedValue(true);
    desktop.mailApi.cancelExit.mockResolvedValue(true);
    desktop.mailApi.retryOutbox.mockResolvedValue({
      id: "outbox-1",
      status: "sent",
      attempts: 2,
    });
    desktop.mailApi.saveDraft.mockImplementation(
      async (request, draftId, expectedLocalVersion) =>
        savedOutcome(request, draftId, expectedLocalVersion),
    );
    window.localStorage.clear();
  });

  afterEach(() => cleanup());

  it("subscribes to desktop update events and refreshes local SQLite views", async () => {
    render(<App />);
    await screen.findAllByText("First mail");

    await waitFor(() => {
      expect(desktop.mailApi.onMailEvent).toHaveBeenCalledWith(
        "mail:inbox-updated",
        expect.any(Function),
      );
      expect(desktop.mailApi.onMailEvent).toHaveBeenCalledWith(
        "mail:drafts-updated",
        expect.any(Function),
      );
    });

    await act(async () => {
      desktop.listeners.get("mail:inbox-updated")?.({ payload: {} });
    });
    await waitFor(() => expect(desktop.mailApi.listInbox).toHaveBeenCalledTimes(2));

    await act(async () => {
      desktop.listeners.get("mail:drafts-updated")?.({ payload: {} });
    });
    await waitFor(() => {
      expect(desktop.mailApi.listDrafts).toHaveBeenCalledTimes(2);
      expect(desktop.mailApi.listOutbox).toHaveBeenCalledTimes(2);
    });
  });

  it("hydrates local account and exact-contact avatars across the shell", async () => {
    desktop.mailApi.listProfileAvatars.mockResolvedValue([
      {
        ownerType: "account",
        ownerKey: "me@163.com",
        imageDataUrl: "data:image/png;base64,AAAA",
      },
      {
        ownerType: "contact",
        ownerKey: "sender1@example.com",
        imageDataUrl: "data:image/png;base64,AQID",
      },
    ]);

    render(<App />);

    await screen.findAllByText("First mail");
    await waitFor(() => {
      expect(
        document.querySelectorAll('img[src="data:image/png;base64,AQID"]').length,
      ).toBeGreaterThanOrEqual(2);
    });
    expect(document.querySelector('img[src="data:image/png;base64,AAAA"]')).toBeTruthy();
    expect(screen.getByLabelText("设置 Sender 1 的头像")).toBeTruthy();
  });

  it("ignores a stale body response after the user selects another message", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 600 });
    const first = summary(1, "First mail");
    const second = summary(2, "Second mail");
    desktop.mailApi.listInbox.mockResolvedValue([first, second]);
    let resolveFirst;
    let resolveSecond;
    desktop.mailApi.fetchMessage.mockImplementation(
      (uid) =>
        new Promise((resolve) => {
          if (uid === 1) resolveFirst = resolve;
          else resolveSecond = resolve;
        }),
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("First mail");

    await user.click(screen.getByText("First mail"));
    await user.click(screen.getByText("Second mail"));
    await act(async () => {
      resolveSecond({ ...second, body_text: "Second body", body_fetched: true });
    });
    await screen.findByText("Second body");
    await act(async () => {
      resolveFirst({ ...first, body_text: "Stale first body", body_fetched: true });
    });

    expect(screen.getByRole("heading", { name: "Second mail" })).toBeTruthy();
    expect(screen.queryByText("Stale first body")).toBeNull();
  });

  it("paints the local preview immediately while the full body hydrates", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 600 });
    const localSummary = {
      ...summary(3, "Instant mail"),
      preview: "Immediately visible local copy",
    };
    const bodyResponse = deferred();
    desktop.mailApi.listInbox.mockResolvedValue([localSummary]);
    desktop.mailApi.fetchMessage.mockReturnValue(bodyResponse.promise);
    const user = userEvent.setup();

    render(<App />);
    await user.click(await screen.findByText("Instant mail"));

    const reader = screen.getByLabelText("邮件阅读区");
    expect(within(reader).getByText("Immediately visible local copy")).toBeTruthy();
    expect(within(reader).queryByLabelText("正在加载正文")).toBeNull();

    await act(async () => {
      bodyResponse.resolve({
        ...localSummary,
        body_text: "Canonical full body",
        body_fetched: true,
      });
    });
    expect(await within(reader).findByText("Canonical full body")).toBeTruthy();
  });

  it("hydrates cached HTML on selection and preserves it across summary refreshes", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 600 });
    const richSummary = {
      ...summary(7, "Rich mail"),
      body_text: "Flattened duplicate copy",
      body_fetched: true,
      body_html_available: true,
      body_html_loaded: false,
    };
    desktop.mailApi.listInbox.mockResolvedValue([richSummary]);
    desktop.mailApi.fetchMessage.mockResolvedValue({
      ...richSummary,
      body_html: '<table><tbody><tr><td class="desktop">Rich layout</td></tr></tbody></table>',
      body_render_mode: "isolated_html",
      body_html_loaded: true,
      has_remote_images: false,
    });
    const user = userEvent.setup();

    render(<App />);
    await user.click(await screen.findByText("Rich mail"));

    const frame = await screen.findByTitle("Rich mail HTML 正文");
    expect(desktop.mailApi.fetchMessage).toHaveBeenCalledWith(7);
    expect(frame.getAttribute("sandbox")).toBe("allow-same-origin");
    expect(frame.getAttribute("srcdoc")).toContain("Rich layout");
    expect(screen.queryByText("Flattened duplicate copy")).toBeNull();

    await waitFor(() =>
      expect(desktop.listeners.has("mail:inbox-updated")).toBe(true),
    );
    await act(async () => {
      desktop.listeners.get("mail:inbox-updated")?.({ payload: {} });
    });
    await waitFor(() => {
      expect(screen.getByTitle("Rich mail HTML 正文").getAttribute("srcdoc")).toContain(
        "Rich layout",
      );
    });
  });

  it("renders a reply as native authored text with collapsed quoted history", async () => {
    const replySummary = {
      ...summary(9, "Reply mail"),
      body_text: "My reply preview",
      body_fetched: true,
      body_html_available: true,
      body_html_loaded: false,
    };
    desktop.mailApi.listInbox.mockResolvedValue([replySummary]);
    desktop.mailApi.fetchMessage.mockResolvedValue({
      ...replySummary,
      body_text: "My reply.\n\nOriginal body.",
      body_html: "<div>My reply.</div><table><tr><td>Original body.</td></tr></table>",
      body_render_mode: "isolated_html",
      body_segments: [
        {
          kind: "authored",
          content: "My reply.",
          render_mode: "plain",
          quote_depth: 0,
          confidence: "high",
        },
        {
          kind: "quoted",
          content: "Original body.",
          render_mode: "plain",
          quote_depth: 1,
          confidence: "high",
        },
      ],
      body_html_loaded: true,
      has_remote_images: false,
    });
    const user = userEvent.setup();

    const { container } = render(<App />);
    await user.click(await screen.findByText("Reply mail"));

    expect(await screen.findByText("My reply.")).toBeTruthy();
    expect(screen.getByText("引用的原邮件")).toBeTruthy();
    expect(container.querySelector("details.quoted-message").open).toBe(false);
    expect(container.querySelector("iframe")).toBeNull();
  });

  it("renders bounded semantic HTML directly on the themed reader material", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 600 });
    const nativeSummary = {
      ...summary(8, "Native mail"),
      body_text: "Myo myo@paa.moe",
      body_fetched: true,
      body_html_available: true,
      body_html_loaded: false,
    };
    desktop.mailApi.listInbox.mockResolvedValue([nativeSummary]);
    desktop.mailApi.fetchMessage.mockResolvedValue({
      ...nativeSummary,
      body_html: '<p>Hello <strong>Myo</strong></p><a href="https://paa.moe">Profile</a>',
      body_render_mode: "native_html",
      body_html_loaded: true,
      has_remote_images: false,
    });
    const user = userEvent.setup();

    render(<App />);
    await user.click(await screen.findByText("Native mail"));

    const reader = screen.getByLabelText("邮件阅读区");
    const semanticText = await within(reader).findByText("Myo");
    expect(semanticText.tagName).toBe("STRONG");
    expect(reader.querySelector(".native-html-message__content")).toBeTruthy();
    expect(reader.querySelector("iframe")).toBeNull();
    expect(within(reader).queryByText("Myo myo@paa.moe")).toBeNull();
  });

  it("flushes the final composer revision before completing desktop exit", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");
    await waitFor(() =>
      expect(desktop.listeners.has("mail:before-exit")).toBe(true),
    );

    await user.click(screen.getByRole("button", { name: /写信/ }));
    fireEvent.change(screen.getByLabelText("收件人"), {
      target: { value: "friend@example.com" },
    });
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "退出前必须保存" },
    });

    act(() => {
      desktop.listeners.get("mail:before-exit")?.({
        payload: { requestId: 101 },
      });
    });
    expect(screen.getByLabelText("主题").disabled).toBe(true);

    await waitFor(() => expect(desktop.mailApi.completeExit).toHaveBeenCalledOnce());
    expect(desktop.mailApi.completeExit).toHaveBeenCalledWith(101);
    expect(desktop.mailApi.saveDraft).toHaveBeenCalledWith(
      expect.objectContaining({ subject: "退出前必须保存" }),
      null,
      null,
    );
    expect(
      desktop.mailApi.saveDraft.mock.invocationCallOrder[0],
    ).toBeLessThan(desktop.mailApi.completeExit.mock.invocationCallOrder[0]);
  });

  it("cancels a failed exit flush, unlocks editing, and allows a second exit", async () => {
    desktop.mailApi.saveDraft
      .mockRejectedValueOnce(new Error("SQLite write failed"))
      .mockImplementationOnce(async (request, draftId, expectedLocalVersion) => {
        const outcome = savedOutcome(request, draftId, expectedLocalVersion);
        return {
          ...outcome,
          draft: { ...outcome.draft, id: draftId || "recovered-draft" },
        };
      });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");
    await waitFor(() =>
      expect(desktop.listeners.has("mail:before-exit")).toBe(true),
    );

    await user.click(screen.getByRole("button", { name: /写信/ }));
    fireEvent.change(screen.getByLabelText("收件人"), {
      target: { value: "friend@example.com" },
    });
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "首次退出保存失败" },
    });

    act(() => {
      desktop.listeners.get("mail:before-exit")?.({
        payload: { requestId: 201 },
      });
    });
    await waitFor(() =>
      expect(desktop.mailApi.cancelExit).toHaveBeenCalledWith(201),
    );
    expect(desktop.mailApi.completeExit).not.toHaveBeenCalled();
    expect(screen.getByLabelText("主题").disabled).toBe(false);
    expect(screen.getByRole("alert").textContent).toContain(
      "退出前保存草稿失败",
    );

    act(() => {
      desktop.listeners.get("mail:before-exit")?.({
        payload: { requestId: 202 },
      });
    });
    await waitFor(() =>
      expect(desktop.mailApi.completeExit).toHaveBeenCalledWith(202),
    );
    expect(desktop.mailApi.saveDraft).toHaveBeenCalledTimes(2);
  });

  it("treats a false complete-exit response as stale and unlocks for retry", async () => {
    desktop.mailApi.completeExit
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");
    await waitFor(() =>
      expect(desktop.listeners.has("mail:before-exit")).toBe(true),
    );

    await user.click(screen.getByRole("button", { name: /写信/ }));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "完成退出返回 false" },
    });
    act(() => {
      desktop.listeners.get("mail:before-exit")?.({
        payload: { requestId: 301 },
      });
    });

    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain("无法完成安全退出"),
    );
    expect(screen.getByLabelText("主题").disabled).toBe(false);

    act(() => {
      desktop.listeners.get("mail:before-exit")?.({
        payload: { requestId: 302 },
      });
    });
    await waitFor(() =>
      expect(desktop.mailApi.completeExit).toHaveBeenLastCalledWith(302),
    );
    expect(desktop.mailApi.completeExit).toHaveBeenCalledTimes(2);
  });

  it("adopts a refreshed canonical draft while the composer is clean", async () => {
    const original = draftSnapshot(1, "Original subject");
    const refreshed = draftSnapshot(2, "Edited on another client", "Remote body");
    desktop.mailApi.listDrafts.mockResolvedValue([original]);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");
    await waitFor(() => expect(desktop.listeners.has("mail:drafts-updated")).toBe(true));

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Original subject"));
    expect(screen.getByLabelText("主题").value).toBe("Original subject");

    desktop.mailApi.listDrafts.mockResolvedValue([refreshed]);
    act(() => {
      desktop.listeners.get("mail:drafts-updated")?.({ payload: { reason: "sync" } });
    });

    await waitFor(() =>
      expect(screen.getByLabelText("主题").value).toBe("Edited on another client"),
    );
    expect(screen.getByLabelText("邮件正文").value).toBe("Remote body");
    expect(desktop.mailApi.saveDraft).not.toHaveBeenCalled();
  });

  it("opens unsupported HTML or attachment drafts read-only and closes without saving", async () => {
    const unsupported = {
      ...draftSnapshot(1, "Rich remote draft", "Plain fallback preview"),
      has_unsupported_content: true,
    };
    desktop.mailApi.listDrafts.mockResolvedValue([unsupported]);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Rich remote draft"));

    expect(screen.getByRole("heading", { name: "查看草稿" })).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain(
      "含当前不支持的HTML/附件，未作修改",
    );
    expect(screen.getByLabelText("收件人").disabled).toBe(true);
    expect(screen.getByLabelText("主题").disabled).toBe(true);
    expect(screen.getByLabelText("邮件正文").disabled).toBe(true);
    expect(screen.getByRole("button", { name: "发送邮件" }).disabled).toBe(true);
    expect(screen.getByRole("button", { name: "保存并关闭" }).disabled).toBe(true);
    expect(screen.getByRole("button", { name: "丢弃草稿" }).disabled).toBe(true);

    await user.click(screen.getByRole("button", { name: "关闭写信窗口" }));
    expect(screen.queryByRole("heading", { name: "查看草稿" })).toBeNull();
    expect(desktop.mailApi.saveDraft).not.toHaveBeenCalled();
    expect(desktop.mailApi.deleteDraft).not.toHaveBeenCalled();
    expect(desktop.mailApi.sendDraft).not.toHaveBeenCalled();
  });

  it("closes a new dirty composer without creating a draft", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "不应保存的临时内容" },
    });
    fireEvent.click(screen.getByRole("button", { name: "关闭写信窗口" }));

    expect(screen.queryByRole("dialog", { name: "不应保存的临时内容" })).toBeNull();
    expect(desktop.mailApi.saveDraft).not.toHaveBeenCalled();
    expect(desktop.mailApi.deleteDraft).not.toHaveBeenCalled();
  });

  it("removes a recovery draft when a new composer is closed", async () => {
    desktop.mailApi.saveDraft.mockImplementation(
      async (request, draftId, expectedLocalVersion) =>
        savedOutcome(request, draftId, expectedLocalVersion),
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    vi.useFakeTimers();
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "已自动保存的临时内容" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(901);
    });
    expect(desktop.mailApi.saveDraft).toHaveBeenCalledTimes(1);
    vi.useRealTimers();

    fireEvent.click(screen.getByRole("button", { name: "关闭写信窗口" }));

    await waitFor(() =>
      expect(desktop.mailApi.deleteDraft).toHaveBeenCalledWith("exit-draft", 1),
    );
    expect(desktop.mailApi.saveDraft).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog", { name: "已自动保存的临时内容" })).toBeNull();
  });

  it("closes an existing dirty draft without forcing a save or deleting it", async () => {
    const original = draftSnapshot(1, "Existing draft");
    desktop.mailApi.listDrafts.mockResolvedValue([original]);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Existing draft"));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "尚未自动保存的修改" },
    });
    fireEvent.click(screen.getByRole("button", { name: "关闭写信窗口" }));

    expect(screen.queryByRole("heading", { name: "编辑草稿" })).toBeNull();
    expect(desktop.mailApi.saveDraft).not.toHaveBeenCalled();
    expect(desktop.mailApi.deleteDraft).not.toHaveBeenCalled();
  });

  it("preserves a dirty stale edit as a conflict copy", async () => {
    const original = draftSnapshot(1, "Original subject");
    const canonical = draftSnapshot(2, "New canonical", "Canonical body");
    const conflictCopy = {
      ...draftSnapshot(1, "My offline edit", "Draft body"),
      id: "conflict-copy",
      status: "conflict",
      remote_mailbox: null,
      remote_uid: null,
    };
    desktop.mailApi.listDrafts.mockResolvedValue([original]);
    desktop.mailApi.saveDraft.mockResolvedValue({
      kind: "conflict_copy",
      draft: conflictCopy,
      canonical,
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");
    await waitFor(() => expect(desktop.listeners.has("mail:drafts-updated")).toBe(true));
    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Original subject"));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "My offline edit" },
    });

    desktop.mailApi.listDrafts.mockResolvedValue([canonical]);
    act(() => {
      desktop.listeners.get("mail:drafts-updated")?.({ payload: { reason: "sync" } });
    });
    await waitFor(() => expect(desktop.mailApi.listDrafts).toHaveBeenCalledTimes(2));
    expect(screen.getByLabelText("主题").value).toBe("My offline edit");

    await waitFor(
      () =>
        expect(desktop.mailApi.saveDraft).toHaveBeenCalledWith(
          expect.objectContaining({ subject: "My offline edit" }),
          "shared-draft",
          1,
        ),
      { timeout: 2_000 },
    );
    expect((await screen.findByRole("alert")).textContent).toContain("冲突副本");
    expect(screen.getByLabelText("主题").value).toBe("My offline edit");
  });

  it("preserves a dirty edit when the canonical draft was deleted", async () => {
    const original = draftSnapshot(1, "Original subject");
    const conflictCopy = {
      ...draftSnapshot(1, "Edit after delete"),
      id: "deleted-base-copy",
      status: "conflict",
      remote_mailbox: null,
      remote_uid: null,
    };
    desktop.mailApi.listDrafts.mockResolvedValue([original]);
    desktop.mailApi.saveDraft.mockResolvedValue({
      kind: "conflict_copy",
      draft: conflictCopy,
      canonical: null,
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");
    await waitFor(() => expect(desktop.listeners.has("mail:drafts-updated")).toBe(true));
    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Original subject"));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "Edit after delete" },
    });

    desktop.mailApi.listDrafts.mockResolvedValue([]);
    act(() => {
      desktop.listeners.get("mail:drafts-updated")?.({ payload: { reason: "sync" } });
    });

    await waitFor(
      () =>
        expect(desktop.mailApi.saveDraft).toHaveBeenCalledWith(
          expect.objectContaining({ subject: "Edit after delete" }),
          "shared-draft",
          1,
        ),
      { timeout: 2_000 },
    );
    expect((await screen.findByRole("alert")).textContent).toContain("冲突副本");
  });

  it("closes a stale discard without deleting the newer canonical", async () => {
    const original = draftSnapshot(1, "Original subject");
    const canonical = draftSnapshot(2, "New canonical");
    desktop.mailApi.listDrafts.mockResolvedValue([original]);
    desktop.mailApi.deleteDraft.mockResolvedValue({ kind: "stale" });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");
    await waitFor(() => expect(desktop.listeners.has("mail:drafts-updated")).toBe(true));
    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Original subject"));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "Discard this stale edit" },
    });
    desktop.mailApi.listDrafts.mockResolvedValue([canonical]);

    await user.click(screen.getByRole("button", { name: "丢弃草稿" }));

    await waitFor(() =>
      expect(desktop.mailApi.deleteDraft).toHaveBeenCalledWith("shared-draft", 1),
    );
    expect(screen.queryByRole("heading", { name: "编辑草稿" })).toBeNull();
    expect((await screen.findByRole("alert")).textContent).toContain("没有删除最新版本");
    expect(await screen.findByText("New canonical")).toBeTruthy();
  });

  it("manually retries only the selected retryable Outbox item", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByText("发件队列"));
    await user.click(screen.getByText("等待处理"));
    await user.click(screen.getByRole("button", { name: "重试发送" }));

    await waitFor(() =>
      expect(desktop.mailApi.retryOutbox).toHaveBeenCalledWith("outbox-1"),
    );
  });

  it("disables every retry action while any Outbox retry is in progress", async () => {
    const retry = deferred();
    desktop.mailApi.retryOutbox.mockReturnValueOnce(retry.promise);
    desktop.mailApi.listOutbox.mockResolvedValue([
      {
        id: "outbox-1",
        draft_id: null,
        recipients: ["first@example.com"],
        status: "retryable",
        attempts: 1,
        last_error: "Temporary failure",
        created_at: "2026-07-14T09:00:00Z",
        sent_at: null,
      },
      {
        id: "outbox-2",
        draft_id: null,
        recipients: ["second@example.com"],
        status: "retryable",
        attempts: 1,
        last_error: "Temporary failure",
        created_at: "2026-07-14T08:00:00Z",
        sent_at: null,
      },
    ]);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByText("发件队列"));
    await user.click(screen.getByText("等待处理 · first@example.com"));
    await user.click(screen.getByRole("button", { name: "重试发送" }));
    await user.click(screen.getByText("等待处理 · second@example.com"));

    const retryButton = screen.getByRole("button", { name: "正在重试…" });
    expect(retryButton.disabled).toBe(true);

    await act(async () => {
      retry.resolve({ id: "outbox-1", status: "sent", attempts: 2 });
      await retry.promise;
    });
  });

  it("refreshes the selected Outbox detail instead of leaving a stale DTO", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");
    await waitFor(() =>
      expect(desktop.listeners.has("mail:drafts-updated")).toBe(true),
    );

    await user.click(screen.getByText("发件队列"));
    await user.click(screen.getByText("等待处理"));
    expect(screen.getByRole("button", { name: "重试发送" })).toBeTruthy();

    desktop.mailApi.listOutbox.mockResolvedValue([
      {
        id: "outbox-1",
        draft_id: null,
        recipients: ["friend@example.com"],
        status: "rejected",
        attempts: 2,
        last_error: "Permanent rejection",
        created_at: "2026-07-14T09:00:00Z",
        sent_at: null,
      },
    ]);
    await act(async () => {
      desktop.listeners.get("mail:drafts-updated")?.({ payload: {} });
    });

    expect(
      await screen.findByRole("heading", { name: "服务器已拒绝" }),
    ).toBeTruthy();
    expect(screen.getByText("说明：Permanent rejection")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "重试发送" })).toBeNull();
  });
});
