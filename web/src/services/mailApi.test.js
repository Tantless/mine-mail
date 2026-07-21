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

  it("prepares a structured reply by local message id", async () => {
    const reply = {
      to: ["sender@example.com"],
      cc: [],
      bcc: [],
      subject: "Re: Earlier",
      body_text: "",
      reply_context: { parent_message_id: "parent@example.com" },
    };
    ipc.invoke.mockResolvedValueOnce(reply);
    const { mailApi } = await import("./mailApi.js");

    await expect(mailApi.prepareReply(42)).resolves.toEqual(reply);
    expect(ipc.invoke).toHaveBeenCalledWith("prepare_reply", { messageId: 42 });
  });

  it("marks an Inbox UID read through a narrow desktop command", async () => {
    ipc.invoke.mockResolvedValueOnce(true);
    const { mailApi } = await import("./mailApi.js");

    await expect(mailApi.markMessageRead(42)).resolves.toBe(true);
    expect(ipc.invoke).toHaveBeenCalledWith("mark_message_read", { uid: 42 });
  });

  it("sets a message star through its exact remote mailbox and UID", async () => {
    ipc.invoke.mockResolvedValueOnce(true);
    const { mailApi } = await import("./mailApi.js");

    await expect(mailApi.setMessageStarred("INBOX", 42, true)).resolves.toBe(true);
    expect(ipc.invoke).toHaveBeenCalledWith("set_message_starred", {
      mailbox: "INBOX",
      uid: 42,
      starred: true,
    });
  });

  it("hydrates contact history by its exact account, mailbox, and UID", async () => {
    ipc.invoke.mockResolvedValueOnce({
      uid: 42,
      mailbox: "Archive/2026",
      subject: "Archived mail",
    });
    const { mailApi } = await import("./mailApi.js");

    await expect(
      mailApi.fetchContactMessage("account-a", "Archive/2026", 42),
    ).resolves.toEqual(
      expect.objectContaining({ uid: 42, mailbox: "Archive/2026" }),
    );
    expect(ipc.invoke).toHaveBeenCalledWith("fetch_contact_message", {
      accountId: "account-a",
      mailbox: "Archive/2026",
      uid: 42,
    });
  });

  it("maps desktop settings and account commands without persisting a secret", async () => {
    ipc.invoke
      .mockResolvedValueOnce({
        poll_interval_minutes: 3,
        autostart_enabled: true,
        notifications_enabled: true,
        foreground_notifications_enabled: false,
        notification_sound_enabled: true,
        notification_sound: "im",
        remote_image_mode: "ask",
      })
      .mockResolvedValueOnce({
        poll_interval_minutes: 1,
        autostart_enabled: false,
        notifications_enabled: false,
        foreground_notifications_enabled: true,
        notification_sound_enabled: false,
        notification_sound: "reminder",
        remote_image_mode: "blocked",
      })
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
      notificationsEnabled: true,
      foregroundNotificationsEnabled: false,
      notificationSoundEnabled: true,
      notificationSound: "im",
      remoteImageMode: "ask",
      startupError: null,
    });
    await mailApi.updateDesktopSettings({
      pollingIntervalMinutes: 1,
      autostartEnabled: false,
      notificationsEnabled: false,
      foregroundNotificationsEnabled: true,
      notificationSoundEnabled: false,
      notificationSound: "reminder",
      remoteImageMode: "blocked",
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
      settings: {
        poll_interval_minutes: 1,
        autostart_enabled: false,
        notifications_enabled: false,
        foreground_notifications_enabled: true,
        notification_sound_enabled: false,
        notification_sound: "reminder",
        remote_image_mode: "blocked",
      },
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
    await mailApi.fetchOutboxMessage("outbox-4");
    await mailApi.retryOutbox("outbox-4");
    await mailApi.deleteDraft("draft-8", 3);
    await mailApi.listSent(29);
    await mailApi.fetchSentMessage(73);
    await mailApi.syncSent();
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
    expect(ipc.invoke).toHaveBeenNthCalledWith(7, "fetch_outbox_message", {
      outboxId: "outbox-4",
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(8, "retry_outbox", {
      outboxId: "outbox-4",
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(9, "delete_draft", {
      draftId: "draft-8",
      expectedLocalVersion: 3,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(10, "list_sent", { limit: 29 });
    expect(ipc.invoke).toHaveBeenNthCalledWith(11, "fetch_sent_message", {
      uid: 73,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(12, "sync_sent", undefined);
    expect(ipc.listen).toHaveBeenCalledWith("mail:inbox-updated", handler);
    unlisten();
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("uses narrow commands for the custom new-mail notification surface", async () => {
    ipc.invoke
      .mockResolvedValueOnce({
        notificationId: 7,
        sender: "Sender",
        subject: "Subject",
        uid: 42,
        count: 1,
        webSound: null,
      })
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(true);
    const { mailApi } = await import("./mailApi.js");

    expect(await mailApi.getNewMailNotification()).toMatchObject({ uid: 42 });
    await mailApi.dismissNewMailNotification(7);
    await mailApi.openNewMailNotification(7, 42);

    expect(ipc.invoke).toHaveBeenNthCalledWith(1, "get_new_mail_notification", undefined);
    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "dismiss_new_mail_notification", {
      notificationId: 7,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(3, "open_new_mail_notification", {
      notificationId: 7,
      uid: 42,
    });
  });

  it("normalizes and controls a bounded multi-account desktop session", async () => {
    const status = {
      configured: true,
      account_id: "account-a",
      active_account_id: "account-a",
      provider: "163",
      email: "a@163.com",
      backend_ready: true,
      credential_available: true,
      network_ready: true,
      account_count: 2,
      max_accounts: 3,
      can_add_account: true,
      google_oauth_configured: true,
      accounts: [
        {
          account_id: "account-a",
          provider: "163",
          email: "a@163.com",
          authentication: "password",
          backend_ready: true,
          credential_available: true,
          network_ready: true,
        },
        {
          account_id: "account-b",
          provider: "gmail",
          email: "b@gmail.com",
          authentication: "google_oauth",
          backend_ready: true,
          credential_available: true,
          network_ready: true,
        },
      ],
    };
    ipc.invoke.mockResolvedValue(status);
    const { mailApi } = await import("./mailApi.js");

    const normalized = await mailApi.getAccountStatus();
    expect(normalized).toMatchObject({
      activeAccountId: "account-a",
      accountCount: 2,
      maxAccounts: 3,
      canAddAccount: true,
    });
    expect(normalized.accounts[1]).toMatchObject({
      accountId: "account-b",
      authentication: "google_oauth",
    });
    await mailApi.connectGoogleAccount();
    await mailApi.switchAccount("account-b");
    await mailApi.removeAccount("account-a");

    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "connect_google_account", undefined);
    expect(ipc.invoke).toHaveBeenNthCalledWith(3, "switch_account", {
      accountId: "account-b",
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(4, "remove_account", {
      accountId: "account-a",
    });
  });

  it("loads an inactive account snapshot without changing the active account", async () => {
    ipc.invoke.mockResolvedValue({
      account_id: "account-b",
      inbox: [],
      drafts: [],
      outbox: [],
    });
    const { mailApi } = await import("./mailApi.js");

    await expect(mailApi.getAccountMailboxSnapshot("account-b", 50)).resolves.toMatchObject({
      account_id: "account-b",
    });
    expect(ipc.invoke).toHaveBeenCalledWith("get_account_mailbox_snapshot", {
      accountId: "account-b",
      limit: 50,
    });
  });

  it("maps local avatar commands through the narrow desktop boundary", async () => {
    ipc.invoke
      .mockResolvedValueOnce([
        {
          owner_type: "contact",
          owner_key: "friend@example.com",
          image_data_url: "data:image/png;base64,AQID",
        },
      ])
      .mockResolvedValueOnce({
        owner_type: "account",
        owner_key: "me@example.com",
        image_data_url: "data:image/png;base64,AQID",
      })
      .mockResolvedValueOnce(undefined);
    const { mailApi } = await import("./mailApi.js");

    expect(await mailApi.listProfileAvatars()).toEqual([
      {
        ownerType: "contact",
        ownerKey: "friend@example.com",
        imageDataUrl: "data:image/png;base64,AQID",
      },
    ]);
    await mailApi.saveProfileAvatar({
      ownerType: "account",
      ownerKey: "me@example.com",
      imageBytes: [1, 2, 3],
    });
    await mailApi.deleteProfileAvatar({
      ownerType: "contact",
      ownerKey: "friend@example.com",
    });

    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "save_profile_avatar", {
      request: {
        owner_type: "account",
        owner_key: "me@example.com",
        image_bytes: [1, 2, 3],
      },
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(3, "delete_profile_avatar", {
      request: {
        owner_type: "contact",
        owner_key: "friend@example.com",
      },
    });
  });

  it("maps local contacts and correspondence through narrow desktop commands", async () => {
    ipc.invoke
      .mockResolvedValueOnce([
        {
          email: "friend@example.com",
          display_name: "林老师",
          original_name: "Friend",
          remark: "林老师",
          is_favorite: true,
          message_count: 4,
          last_message_at: "2026-07-20T12:00:00Z",
          last_subject: "Hello",
        },
      ])
      .mockResolvedValueOnce([
        {
          uid: 73,
          mailbox: "Sent",
          subject: "Hello",
          direction: "outgoing",
        },
      ])
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(true);
    const { mailApi } = await import("./mailApi.js");

    await expect(mailApi.listContacts("account-a")).resolves.toEqual([
      expect.objectContaining({
        email: "friend@example.com",
        displayName: "林老师",
        originalName: "Friend",
        remark: "林老师",
        isFavorite: true,
        messageCount: 4,
      }),
    ]);
    await expect(
      mailApi.listContactMessages("account-a", "friend@example.com", 80),
    ).resolves.toEqual([
      expect.objectContaining({ uid: 73, direction: "outgoing" }),
    ]);
    await mailApi.setContactFavorite("friend@example.com", false);
    await mailApi.setContactRemark("friend@example.com", "林同学");

    expect(ipc.invoke).toHaveBeenNthCalledWith(1, "list_contacts", {
      accountId: "account-a",
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "list_contact_messages", {
      accountId: "account-a",
      email: "friend@example.com",
      limit: 80,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(3, "set_contact_favorite", {
      email: "friend@example.com",
      favorite: false,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(4, "set_contact_remark", {
      email: "friend@example.com",
      remark: "林同学",
    });
  });

  it("preserves safe string errors returned by Rust", async () => {
    ipc.invoke.mockRejectedValue("Recipient confirmation did not match");
    const { mailApi } = await import("./mailApi.js");

    await expect(mailApi.syncAll()).rejects.toThrow(
      "Recipient confirmation did not match",
    );
  });

  it("opens mail links through the narrow desktop command", async () => {
    ipc.invoke.mockResolvedValue(undefined);
    const { mailApi } = await import("./mailApi.js");

    await mailApi.openExternalUrl("https://example.com/message");

    expect(ipc.invoke).toHaveBeenCalledWith("open_external_url", {
      url: "https://example.com/message",
    });
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
