import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const ipc = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: ipc.listen }));

describe("mailApi desktop IPC contract", () => {
  beforeEach(() => {
    vi.resetModules();
    ipc.invoke.mockReset();
    ipc.listen.mockReset();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  });

  afterEach(() => {
    delete window.__TAURI_INTERNALS__;
  });

  it("reuses a draft id and sends exactly that persisted draft", async () => {
    ipc.invoke
      .mockResolvedValueOnce({
        kind: "saved",
        draft: { id: "draft-7", local_version: 8, status: "local" },
        canonical: null,
      })
      .mockResolvedValueOnce({ id: "outbox-2", status: "sent" });
    const { mailApi } = await import("./mailApi.js");
    const request = {
      to: ["friend@example.com"],
      cc: [],
      bcc: [],
      subject: "Hello",
      body_text: "Body",
    };

    await mailApi.saveDraft(request, "draft-7", 7);
    await mailApi.sendDraft("draft-7", 8, ["friend@example.com"]);

    expect(ipc.invoke).toHaveBeenNthCalledWith(1, "save_draft", {
      request,
      draftId: "draft-7",
      expectedLocalVersion: 7,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "send_draft", {
      draftId: "draft-7",
      expectedLocalVersion: 8,
      confirmedRecipients: ["friend@example.com"],
    });
  });

  it("maps desktop settings and account commands without persisting a secret", async () => {
    ipc.invoke
      .mockResolvedValueOnce({ poll_interval_minutes: 3, autostart_enabled: true })
      .mockResolvedValueOnce({ poll_interval_minutes: 1, autostart_enabled: false })
      .mockResolvedValueOnce({
        configured: true,
        provider: "163",
        email: "me@163.com",
        backend_ready: true,
        credential_available: true,
        startup_error: null,
      })
      .mockResolvedValueOnce({ configured: true, backend_ready: true, credential_available: true });
    const { mailApi } = await import("./mailApi.js");

    expect(await mailApi.getDesktopSettings()).toEqual({
      pollingIntervalMinutes: 3,
      autostartEnabled: true,
      startupError: null,
    });
    await mailApi.updateDesktopSettings({
      pollingIntervalMinutes: 1,
      autostartEnabled: false,
    });
    expect(await mailApi.getAccountStatus()).toMatchObject({
      configured: true,
      backendReady: true,
      credentialAvailable: true,
    });
    const accountRequest = {
      provider: "163",
      email: "me@163.com",
      secret: "ephemeral-test-value",
    };
    await mailApi.configureAccount(accountRequest);

    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "update_desktop_settings", {
      settings: { poll_interval_minutes: 1, autostart_enabled: false },
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(4, "configure_account", {
      request: accountRequest,
    });
    expect(window.localStorage.getItem("secret")).toBeNull();
  });

  it("wires aggregate refresh, outbox, delete and desktop update events", async () => {
    const dispose = vi.fn();
    ipc.invoke.mockResolvedValue(true);
    ipc.listen.mockResolvedValue(dispose);
    const { mailApi } = await import("./mailApi.js");
    const handler = vi.fn();

    await mailApi.listInbox(37);
    await mailApi.syncAll();
    await mailApi.syncDrafts();
    await mailApi.completeExit(404);
    await mailApi.cancelExit(405);
    await mailApi.listOutbox();
    await mailApi.retryOutbox("outbox-4");
    await mailApi.deleteDraft("draft-8", 3);
    const unlisten = await mailApi.onMailEvent("mail:inbox-updated", handler);

    expect(ipc.invoke).toHaveBeenNthCalledWith(1, "list_inbox", { limit: 37 });
    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "sync_all", undefined);
    expect(ipc.invoke).toHaveBeenNthCalledWith(3, "sync_drafts", undefined);
    expect(ipc.invoke).toHaveBeenNthCalledWith(4, "complete_exit", {
      requestId: 404,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(5, "cancel_exit", {
      requestId: 405,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(6, "list_outbox", undefined);
    expect(ipc.invoke).toHaveBeenNthCalledWith(7, "retry_outbox", {
      outboxId: "outbox-4",
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(8, "delete_draft", {
      draftId: "draft-8",
      expectedLocalVersion: 3,
    });
    expect(ipc.listen).toHaveBeenCalledWith("mail:inbox-updated", handler);
    unlisten();
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("preserves safe string errors returned by Rust", async () => {
    ipc.invoke.mockRejectedValue("Recipient confirmation did not match");
    const { mailApi } = await import("./mailApi.js");

    await expect(mailApi.syncAll()).rejects.toThrow(
      "Recipient confirmation did not match",
    );
  });

  it("rejects stale complete and cancel exit handshakes returned as false", async () => {
    ipc.invoke.mockResolvedValue(false);
    const { mailApi } = await import("./mailApi.js");

    await expect(mailApi.completeExit(501)).rejects.toThrow(
      "退出请求已失效",
    );
    await expect(mailApi.cancelExit(502)).rejects.toThrow(
      "退出请求已失效",
    );
  });

  it("requires an explicit demo flag outside Tauri and test mode", async () => {
    const { __testing } = await import("./mailApi.js");

    expect(
      __testing.resolveRuntime({
        tauri: false,
        demoFlag: undefined,
        mode: "production",
      }),
    ).toBe("unsupported");
    expect(
      __testing.resolveRuntime({
        tauri: false,
        demoFlag: "1",
        mode: "production",
      }),
    ).toBe("demo");
    expect(
      __testing.resolveRuntime({
        tauri: true,
        demoFlag: "1",
        mode: "production",
      }),
    ).toBe("tauri");
  });
});
