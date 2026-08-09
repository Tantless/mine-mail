import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const ipc = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));
const OPAQUE_MESSAGE_ID = "9f1a7b32-4b55-4d6d-8db7-0e7bf1a32c41";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: ipc.invoke,
  Channel: class MockChannel {
    onmessage = null;
  },
}));
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

  it("keeps AI provider access behind one typed desktop command", async () => {
    ipc.invoke.mockResolvedValue({
      session: null,
      assistant_message: "已完成",
      draft_revision: "revision-1",
      draft: null,
      changed_fields: [],
    });
    const { mailApi } = await import("./mailApi.js");
    const request = {
      mode: "chat",
      instruction: "概括当前草稿",
      session_id: null,
      draft_revision: "revision-1",
      draft: {
        account_id: "account-1",
        draft_id: null,
        local_version: null,
        compose: {
          to: [],
          cc: [],
          bcc: [],
          subject: "",
          body_text: "",
          format: {},
          reply_context: null,
        },
        attachments: [],
        forward_context: null,
      },
    };

    await mailApi.runAiTurn(request, vi.fn());

    expect(ipc.invoke).toHaveBeenCalledWith("run_ai_turn", {
      request,
      onEvent: expect.objectContaining({ onmessage: expect.any(Function) }),
    });
  });

  it("maps Agent configuration through narrow desktop commands", async () => {
    ipc.invoke
      .mockResolvedValueOnce({
        providerId: "deepseek",
        baseUrl: "https://api.deepseek.com",
        modelName: "deepseek-v4-pro",
        useEnvironmentKey: false,
        hasStoredApiKey: true,
        hasEnvironmentApiKey: false,
        environmentVariable: "DEEPSEEK_API_KEY",
        translationLanguage: "zh-Hans",
        translationLanguages: [
          { id: "zh-Hans", label: "中文（简体）" },
          { id: "en", label: "English" },
        ],
        presets: [{
          id: "deepseek",
          label: "DeepSeek",
          base_url: "https://api.deepseek.com",
          environment_variable: "DEEPSEEK_API_KEY",
          models: ["deepseek-v4-flash", "deepseek-v4-pro"],
        }],
      })
      .mockResolvedValueOnce({ models: ["deepseek-v4-pro"] })
      .mockResolvedValueOnce({ latencyMs: 73 })
      .mockResolvedValueOnce({
        providerId: "deepseek",
        baseUrl: "https://api.deepseek.com",
        modelName: "deepseek-v4-pro",
        useEnvironmentKey: false,
        hasStoredApiKey: true,
        hasEnvironmentApiKey: false,
        environmentVariable: "DEEPSEEK_API_KEY",
        translationLanguage: "en",
        translationLanguages: [],
        presets: [],
      });
    const { mailApi } = await import("./mailApi.js");
    const configuration = {
      providerId: "deepseek",
      baseUrl: "https://api.deepseek.com",
      modelName: "deepseek-v4-pro",
      useEnvironmentKey: false,
      apiKey: "test-secret",
      translationLanguage: "en",
    };
    const { translationLanguage: _translationLanguage, ...connectionConfiguration } =
      configuration;

    const loaded = await mailApi.getAiConfig();
    expect(loaded.hasStoredApiKey).toBe(true);
    expect(loaded.presets[0].models).toEqual([
      "deepseek-v4-flash",
      "deepseek-v4-pro",
    ]);
    expect(loaded.translationLanguages).toEqual([
      { value: "zh-Hans", label: "中文（简体）" },
      { value: "en", label: "English" },
    ]);
    expect(await mailApi.listAiModels(configuration)).toEqual([
      "deepseek-v4-pro",
    ]);
    expect(await mailApi.testAiConnection(configuration)).toEqual({
      latencyMs: 73,
    });
    const saved = await mailApi.saveAiConfig(configuration);
    expect(saved).not.toHaveProperty("apiKey");

    expect(ipc.invoke).toHaveBeenNthCalledWith(1, "get_ai_config", undefined);
    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "list_ai_models", {
      request: connectionConfiguration,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(3, "test_ai_connection", {
      request: connectionConfiguration,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(4, "save_ai_config", {
      request: configuration,
    });
  });

  it("sends only bounded render parts through the AI translation command", async () => {
    ipc.invoke.mockResolvedValue({
      language: "zh-Hans",
      parts: [{ id: "body-html", content: "<p>你好</p>" }],
    });
    const { mailApi } = await import("./mailApi.js");
    const parts = [
      { id: "body-html", format: "html", content: "<p>Hello</p>" },
    ];

    await expect(mailApi.translateMailContent(parts)).resolves.toEqual({
      language: "zh-Hans",
      parts: [{ id: "body-html", content: "<p>你好</p>" }],
    });
    expect(ipc.invoke).toHaveBeenCalledWith("translate_mail_content", {
      request: { parts },
    });
  });

  it("updates only the persisted AI translation language", async () => {
    ipc.invoke.mockResolvedValue({
      providerId: "deepseek",
      baseUrl: "https://api.deepseek.com",
      modelName: "deepseek-chat",
      useEnvironmentKey: true,
      translationLanguage: "ja",
      translationLanguages: [{ id: "ja", label: "日本語" }],
      presets: [],
    });
    const { mailApi } = await import("./mailApi.js");

    await expect(mailApi.setAiTranslationLanguage("ja")).resolves.toEqual(
      expect.objectContaining({ translationLanguage: "ja" }),
    );
    expect(ipc.invoke).toHaveBeenCalledWith("set_ai_translation_language", {
      languageId: "ja",
    });
  });

  it("maps one reviewed delivery-unknown generation and its explicit risk decision", async () => {
    const confirmed = {
      id: "outbox-unknown",
      status: "sent",
      attempts: 2,
      sent_at: null,
    };
    ipc.invoke.mockResolvedValue(confirmed);
    const { mailApi } = await import("./mailApi.js");

    await expect(
      mailApi.resolveDeliveryUnknown({
        outboxId: "outbox-unknown",
        expectedAttempts: 2,
        decision: "confirm_delivered",
        acknowledgeDuplicateRisk: false,
      }),
    ).resolves.toBe(confirmed);
    expect(ipc.invoke).toHaveBeenNthCalledWith(
      1,
      "resolve_delivery_unknown",
      {
        outboxId: "outbox-unknown",
        expectedAttempts: 2,
        decision: "confirm_delivered",
        acknowledgeDuplicateRisk: false,
      },
    );

    await mailApi.resolveDeliveryUnknown({
      outboxId: "outbox-unknown",
      expectedAttempts: 2,
      decision: "retry_once",
      acknowledgeDuplicateRisk: true,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(
      2,
      "resolve_delivery_unknown",
      {
        outboxId: "outbox-unknown",
        expectedAttempts: 2,
        decision: "retry_once",
        acknowledgeDuplicateRisk: true,
      },
    );
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

    await expect(mailApi.prepareReply(OPAQUE_MESSAGE_ID)).resolves.toEqual(reply);
    expect(ipc.invoke).toHaveBeenCalledWith("prepare_reply", {
      messageId: OPAQUE_MESSAGE_ID,
    });
  });

  it("maps attachment drafts and forwarding through opaque path-free commands", async () => {
    ipc.invoke
      .mockResolvedValueOnce({
        status: "saved",
        file_name: "invoice.pdf",
        retryable: false,
      })
      .mockResolvedValueOnce({
        id: "draft-1",
        local_version: 1,
        attachments: [],
      })
      .mockResolvedValueOnce({
        kind: "canceled",
        draft: { id: "draft-1", local_version: 1, attachments: [] },
      })
      .mockResolvedValueOnce({
        kind: "saved",
        draft: { id: "draft-1", local_version: 2, attachments: [] },
      })
      .mockResolvedValueOnce({
        kind: "prepared",
        prepared: {
          draft: { id: "draft-2", local_version: 1, attachments: [] },
          warnings: ["attachments_omitted_by_user"],
        },
      });
    const { mailApi } = await import("./mailApi.js");

    await expect(
      mailApi.saveMessageAttachment(OPAQUE_MESSAGE_ID, "opaque-attachment"),
    ).resolves.toEqual({
      status: "saved",
      file_name: "invoice.pdf",
      retryable: false,
    });
    await mailApi.createComposeDraft();
    await mailApi.addDraftAttachments("draft-1", 1);
    await mailApi.removeDraftAttachment("draft-1", "opaque-attachment", 1);
    await mailApi.prepareForward(OPAQUE_MESSAGE_ID, false);

    expect(ipc.invoke.mock.calls).toEqual([
      [
        "save_message_attachment",
        {
          messageId: OPAQUE_MESSAGE_ID,
          attachmentId: "opaque-attachment",
        },
      ],
      ["create_compose_draft", undefined],
      [
        "add_draft_attachments",
        { draftId: "draft-1", expectedLocalVersion: 1 },
      ],
      [
        "remove_draft_attachment",
        {
          draftId: "draft-1",
          attachmentId: "opaque-attachment",
          expectedLocalVersion: 1,
        },
      ],
      [
        "prepare_forward",
        { messageId: OPAQUE_MESSAGE_ID, includeAttachments: false },
      ],
    ]);
    expect(JSON.stringify(ipc.invoke.mock.calls)).not.toContain("path");
    expect(JSON.stringify(ipc.invoke.mock.calls)).not.toContain("bytes");
    expect(JSON.stringify(ipc.invoke.mock.calls)).not.toContain("mailbox");
    expect(JSON.stringify(ipc.invoke.mock.calls)).not.toContain("\"uid\"");
  });

  it("maps semantic mailbox pages and opaque-id actions one to one", async () => {
    const capabilities = [
      { role: "archive", status: "available", retryable: false },
    ];
    const page = {
      items: [{ id: OPAQUE_MESSAGE_ID, displayed_role: "archive" }],
      has_more_local: false,
      remote_history_state: "complete",
      end_reached: true,
    };
    ipc.invoke
      .mockResolvedValueOnce(capabilities)
      .mockResolvedValueOnce([
        { selectionId: "choice-a", displayName: "Mine Archive" },
      ])
      .mockResolvedValueOnce(capabilities[0])
      .mockResolvedValueOnce({
        role: "trash",
        status: "available",
        retryable: false,
      })
      .mockResolvedValueOnce(page)
      .mockResolvedValueOnce(page)
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce({
        id: OPAQUE_MESSAGE_ID,
        body_text: "Full body",
      })
      .mockResolvedValueOnce({
        operation_id: "seen-1",
        status: "pending",
      })
      .mockResolvedValueOnce({
        operation_id: "flagged-1",
        status: "pending",
      })
      .mockResolvedValueOnce({
        operation_id: "archive-1",
        status: "pending",
      })
      .mockResolvedValueOnce({
        operation_id: "trash-1",
        status: "pending",
      })
      .mockResolvedValueOnce({
        plan_id: "plan-1",
        expires_at: "2026-07-28T01:00:00Z",
      })
      .mockResolvedValueOnce({
        operation_id: "delete-1",
        status: "pending",
      });
    const { mailApi } = await import("./mailApi.js");

    await expect(
      mailApi.getMailboxCapabilities("account-a"),
    ).resolves.toEqual(capabilities);
    await expect(
      mailApi.listArchiveFolderCandidates("account-a"),
    ).resolves.toEqual([
      { selectionId: "choice-a", displayName: "Mine Archive" },
    ]);
    await expect(
      mailApi.assignArchiveFolder("account-a", "choice-a"),
    ).resolves.toEqual(capabilities[0]);
    await expect(
      mailApi.createMailboxRole("account-a", "trash"),
    ).resolves.toEqual({
      role: "trash",
      status: "available",
      retryable: false,
    });
    await expect(
      mailApi.listMailboxPage("account-a", "archive", null, 50, "needle"),
    ).resolves.toEqual(page);
    await expect(
      mailApi.loadOlderMailboxPage(
        "account-a",
        "archive",
        "cursor-1",
        50,
        "needle",
      ),
    ).resolves.toEqual(page);
    await mailApi.syncMailbox("account-a", "archive");
    await mailApi.fetchMailboxMessage(OPAQUE_MESSAGE_ID);
    await mailApi.setMessageSeen(OPAQUE_MESSAGE_ID, false);
    await mailApi.setMessageStarredById(OPAQUE_MESSAGE_ID, true);
    await mailApi.archiveMessage(OPAQUE_MESSAGE_ID);
    await mailApi.moveMessageToInbox(OPAQUE_MESSAGE_ID);
    await mailApi.moveMessageToTrash(OPAQUE_MESSAGE_ID);
    await mailApi.preparePermanentDelete(OPAQUE_MESSAGE_ID);
    await mailApi.confirmPermanentDelete("plan-1");

    expect(ipc.invoke.mock.calls).toEqual([
      ["get_mailbox_capabilities", { accountId: "account-a" }],
      ["list_archive_folder_candidates", { accountId: "account-a" }],
      [
        "assign_archive_folder",
        { accountId: "account-a", selectionId: "choice-a" },
      ],
      [
        "create_mailbox_role",
        { accountId: "account-a", role: "trash" },
      ],
      [
        "list_mailbox_page",
        {
          accountId: "account-a",
          role: "archive",
          cursor: null,
          pageSize: 50,
          query: "needle",
        },
      ],
      [
        "load_older_mailbox_page",
        {
          accountId: "account-a",
          role: "archive",
          cursor: "cursor-1",
          pageSize: 50,
          query: "needle",
        },
      ],
      ["sync_mailbox", { accountId: "account-a", role: "archive" }],
      ["fetch_mailbox_message", { messageId: OPAQUE_MESSAGE_ID }],
      ["set_message_seen", { messageId: OPAQUE_MESSAGE_ID, seen: false }],
      [
        "set_message_starred_by_id",
        { messageId: OPAQUE_MESSAGE_ID, starred: true },
      ],
      ["archive_message", { messageId: OPAQUE_MESSAGE_ID }],
      ["move_message_to_inbox", { messageId: OPAQUE_MESSAGE_ID }],
      ["move_message_to_trash", { messageId: OPAQUE_MESSAGE_ID }],
      ["prepare_permanent_delete", { messageId: OPAQUE_MESSAGE_ID }],
      ["confirm_permanent_delete", { planId: "plan-1" }],
    ]);
  });

  it("maps dedicated Starred source pages to their narrow desktop commands", async () => {
    const page = {
      items: [{ id: OPAQUE_MESSAGE_ID, flags: ["\\Flagged"] }],
      end_reached: true,
    };
    ipc.invoke.mockResolvedValue(page);
    const { mailApi } = await import("./mailApi.js");

    await expect(
      mailApi.listStarredMailboxPage(
        "account-a",
        "inbox",
        null,
        25,
        "needle",
      ),
    ).resolves.toEqual(page);
    await expect(
      mailApi.loadOlderStarredMailboxPage(
        "account-a",
        "inbox",
        "starred-cursor-1",
        25,
        "needle",
      ),
    ).resolves.toEqual(page);

    expect(ipc.invoke.mock.calls).toEqual([
      [
        "list_starred_mailbox_page",
        {
          accountId: "account-a",
          role: "inbox",
          cursor: null,
          pageSize: 25,
          query: "needle",
        },
      ],
      [
        "load_older_starred_mailbox_page",
        {
          accountId: "account-a",
          role: "inbox",
          cursor: "starred-cursor-1",
          pageSize: 25,
          query: "needle",
        },
      ],
    ]);
  });

  it("passes mailbox bcc and explicit outbox recipient groups through unchanged", async () => {
    const mailboxPage = {
      items: [
        {
          id: OPAQUE_MESSAGE_ID,
          displayed_role: "sent",
          bcc: [
            { name: "审计留档", email: "archive@example.com" },
            { name: null, email: "second@example.com" },
          ],
        },
      ],
      has_more_local: false,
      remote_history_state: "complete",
      end_reached: true,
    };
    const mailboxMessage = {
      ...mailboxPage.items[0],
      body_text: "Full body",
    };
    const legacyOutbox = {
      id: "outbox-legacy",
      recipients: ["legacy@example.com"],
      recipient_groups: null,
    };
    const groupedOutbox = {
      id: "outbox-grouped",
      recipients: [
        "to@example.com",
        "cc@example.com",
        "bcc@example.com",
      ],
      recipient_groups: {
        to: ["to@example.com"],
        cc: ["cc@example.com"],
        bcc: ["bcc@example.com"],
      },
    };
    const outbox = [legacyOutbox, groupedOutbox];
    ipc.invoke
      .mockResolvedValueOnce(mailboxPage)
      .mockResolvedValueOnce(mailboxMessage)
      .mockResolvedValueOnce(outbox)
      .mockResolvedValueOnce(groupedOutbox);
    const { mailApi } = await import("./mailApi.js");

    await expect(
      mailApi.listMailboxPage("account-a", "sent"),
    ).resolves.toBe(mailboxPage);
    await expect(
      mailApi.fetchMailboxMessage(OPAQUE_MESSAGE_ID),
    ).resolves.toBe(mailboxMessage);
    await expect(mailApi.listOutbox()).resolves.toBe(outbox);
    await expect(
      mailApi.fetchOutboxMessage("outbox-grouped"),
    ).resolves.toBe(groupedOutbox);

    expect(mailboxPage.items[0].bcc).toEqual([
      { name: "审计留档", email: "archive@example.com" },
      { name: null, email: "second@example.com" },
    ]);
    expect(legacyOutbox.recipient_groups).toBeNull();
    expect(legacyOutbox).not.toHaveProperty("to");
    expect(legacyOutbox).not.toHaveProperty("cc");
    expect(legacyOutbox).not.toHaveProperty("bcc");
    expect(groupedOutbox.recipient_groups).toEqual({
      to: ["to@example.com"],
      cc: ["cc@example.com"],
      bcc: ["bcc@example.com"],
    });
  });

  it("hydrates contact history through the shared opaque selected-message command", async () => {
    ipc.invoke.mockResolvedValueOnce({
      id: OPAQUE_MESSAGE_ID,
      subject: "Archived mail",
    });
    const { mailApi } = await import("./mailApi.js");

    expect(mailApi.fetchContactMessage).toBeUndefined();
    await expect(mailApi.fetchMailboxMessage(OPAQUE_MESSAGE_ID)).resolves.toEqual(
      expect.objectContaining({ id: OPAQUE_MESSAGE_ID }),
    );
    expect(ipc.invoke).toHaveBeenCalledWith("fetch_mailbox_message", {
      messageId: OPAQUE_MESSAGE_ID,
    });
    expect(ipc.invoke).not.toHaveBeenCalledWith(
      "fetch_contact_message",
      expect.anything(),
    );
  });

  it("maps desktop settings and account commands without persisting a secret", async () => {
    ipc.invoke
      .mockResolvedValueOnce({
        poll_interval_minutes: 3,
        autostart_enabled: true,
        notifications_enabled: true,
        notification_delivery: "windows",
        windows_notifications_available: true,
        notification_sound_enabled: true,
        notification_sound: "im",
        remote_image_mode: "ask",
        ai_assistant_default_open: true,
      })
      .mockResolvedValueOnce({
        poll_interval_minutes: 1,
        autostart_enabled: false,
        notifications_enabled: false,
        notification_delivery: "mine_mail",
        windows_notifications_available: true,
        notification_sound_enabled: false,
        notification_sound: "reminder",
        remote_image_mode: "blocked",
        ai_assistant_default_open: false,
      })
      .mockResolvedValueOnce({
        configured: true,
        provider: "163",
        email: "me@163.com",
        backend_ready: true,
        credential_available: false,
        credential_invalid: true,
        startup_error: null,
      })
      .mockResolvedValueOnce({
        configured: true,
        backend_ready: true,
        credential_available: true,
      });
    const { mailApi } = await import("./mailApi.js");

    expect(await mailApi.getDesktopSettings()).toEqual({
      pollingIntervalMinutes: 3,
      autostartEnabled: true,
      notificationsEnabled: true,
      notificationDelivery: "windows",
      windowsNotificationsAvailable: true,
      notificationSoundEnabled: true,
      notificationSound: "im",
      remoteImageMode: "ask",
      aiAssistantDefaultOpen: true,
      mcpEnabled: false,
      mcpInformationEnabled: true,
      mcpSendEnabled: false,
      mcpEndpoint: "http://127.0.0.1:46321/mcp",
      startupError: null,
    });
    await mailApi.updateDesktopSettings({
      pollingIntervalMinutes: 1,
      autostartEnabled: false,
      notificationsEnabled: false,
      notificationDelivery: "mine_mail",
      windowsNotificationsAvailable: true,
      notificationSoundEnabled: false,
      notificationSound: "reminder",
      remoteImageMode: "blocked",
      aiAssistantDefaultOpen: false,
    });
    expect(await mailApi.getAccountStatus()).toMatchObject({
      configured: true,
      backendReady: true,
      credentialAvailable: false,
      credentialInvalid: true,
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
        notification_delivery: "mine_mail",
        notification_sound_enabled: false,
        notification_sound: "reminder",
        remote_image_mode: "blocked",
        ai_assistant_default_open: false,
        mcp_enabled: false,
        mcp_information_enabled: true,
        mcp_send_enabled: false,
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

    await mailApi.syncAll();
    await mailApi.syncDrafts();
    await mailApi.completeExit(404);
    await mailApi.cancelExit(405);
    await mailApi.listOutbox();
    await mailApi.listSentOutboxFallbacks();
    await mailApi.fetchOutboxMessage("outbox-4");
    await mailApi.retryOutbox("outbox-4");
    await mailApi.deleteDraft("draft-8", 3);
    await mailApi.syncSent();
    const unlisten = await mailApi.onMailEvent("mail:inbox-updated", handler);

    expect(ipc.invoke).toHaveBeenNthCalledWith(1, "sync_all", undefined);
    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "sync_drafts", undefined);
    expect(ipc.invoke).toHaveBeenNthCalledWith(3, "complete_exit", {
      requestId: 404,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(4, "cancel_exit", {
      requestId: 405,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(5, "list_outbox", undefined);
    expect(ipc.invoke).toHaveBeenNthCalledWith(
      6,
      "list_sent_outbox_fallbacks",
      undefined,
    );
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
    expect(ipc.invoke).toHaveBeenNthCalledWith(10, "sync_sent", undefined);
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
        count: 1,
        webSound: null,
      })
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(true);
    const { mailApi } = await import("./mailApi.js");

    const notification = await mailApi.getNewMailNotification();
    expect(notification).toMatchObject({ notificationId: 7, subject: "Subject" });
    expect(notification).not.toHaveProperty("uid");
    expect(notification).not.toHaveProperty("accountId");
    expect(notification).not.toHaveProperty("messageId");
    await mailApi.dismissNewMailNotification(7);
    await mailApi.openNewMailNotification(7);

    expect(ipc.invoke).toHaveBeenNthCalledWith(
      1,
      "get_new_mail_notification",
      undefined,
    );
    expect(ipc.invoke).toHaveBeenNthCalledWith(
      2,
      "dismiss_new_mail_notification",
      {
        notificationId: 7,
      },
    );
    expect(ipc.invoke).toHaveBeenNthCalledWith(
      3,
      "open_new_mail_notification",
      {
        notificationId: 7,
      },
    );
  });

  it("normalizes and controls a bounded multi-account desktop session", async () => {
    const status = {
      configured: true,
      account_id: "account-a",
      active_account_id: "account-a",
      provider: "163",
      email: "a@163.com",
      remark: "工作邮箱",
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
          remark: "工作邮箱",
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
      remark: "工作邮箱",
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
    await mailApi.setAccountRemark("account-b", "私人邮箱");
    await mailApi.removeAccount("account-a", {
      revokeGoogleAuthorization: true,
      deleteLocalData: true,
    });

    expect(ipc.invoke).toHaveBeenNthCalledWith(
      2,
      "connect_google_account",
      undefined,
    );
    expect(ipc.invoke).toHaveBeenNthCalledWith(3, "switch_account", {
      accountId: "account-b",
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(4, "set_account_remark", {
      accountId: "account-b",
      remark: "私人邮箱",
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(5, "remove_account", {
      request: {
        accountId: "account-a",
        revokeGoogleAuthorization: true,
        deleteLocalData: true,
      },
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
      .mockResolvedValueOnce({
        contacts: [
          {
            account_id: "account-a",
            email: "friend@example.com",
            display_name: "林老师",
            original_name: "Friend",
            remark: "林老师",
            is_favorite: true,
            message_count: 4,
            last_message_at: "2026-07-20T12:00:00Z",
            last_subject: "Hello",
          },
        ],
        favorites: [
          {
            account_id: "account-b",
            email: "favorite@example.com",
            display_name: "Favorite",
            original_name: "Favorite",
            remark: null,
            is_favorite: true,
            message_count: 2,
            last_message_at: "2026-07-19T12:00:00Z",
            last_subject: "Favorite hello",
          },
        ],
      })
      .mockResolvedValueOnce([
        {
          id: "opaque-contact-message",
          mailbox_role: "sent",
          subject: "Hello",
          direction: "outgoing",
        },
      ])
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(true);
    const { mailApi } = await import("./mailApi.js");

    await expect(mailApi.listContacts("account-a")).resolves.toEqual({
      contacts: [
        expect.objectContaining({
          accountId: "account-a",
          email: "friend@example.com",
          displayName: "林老师",
          originalName: "Friend",
          remark: "林老师",
          isFavorite: true,
          messageCount: 4,
        }),
      ],
      favorites: [
        expect.objectContaining({
          accountId: "account-b",
          email: "favorite@example.com",
          isFavorite: true,
          messageCount: 2,
        }),
      ],
    });
    await expect(
      mailApi.listContactMessages("account-a", "friend@example.com", 80),
    ).resolves.toEqual([
      expect.objectContaining({
        id: "opaque-contact-message",
        direction: "outgoing",
        mailbox_role: "sent",
      }),
    ]);
    await mailApi.setContactFavorite("account-a", "friend@example.com", false);
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
      accountId: "account-a",
      email: "friend@example.com",
      favorite: false,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(4, "set_contact_remark", {
      email: "friend@example.com",
      remark: "林同学",
    });
  });

  it("converts Rust diagnostics into categorized Chinese errors", async () => {
    ipc.invoke.mockRejectedValue("Recipient confirmation did not match");
    const { mailApi } = await import("./mailApi.js");

    await expect(mailApi.syncAll()).rejects.toThrow(
      "操作未完成：收件人信息已变化，请重新确认后发送。",
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

    await expect(mailApi.completeExit(501)).rejects.toThrow("退出请求已失效");
    await expect(mailApi.cancelExit(502)).rejects.toThrow("退出请求已失效");
  });

  it("keeps the explicit demo mailbox API deterministic and body-free in pages", async () => {
    delete window.__TAURI_INTERNALS__;
    const { mailApi } = await import("./mailApi.js");

    await expect(
      mailApi.getMailboxCapabilities("demo-primary"),
    ).resolves.toContainEqual({
      role: "archive",
      status: "available",
      retryable: false,
    });
    const inbox = await mailApi.listMailboxPage(
      "demo-primary",
      "inbox",
      null,
      2,
      "mine mail",
    );
    expect(inbox.items).toHaveLength(1);
    expect(inbox.items[0]).toEqual(
      expect.objectContaining({
        id: "demo-message-01",
        displayed_role: "inbox",
      }),
    );
    expect(inbox.items[0]).not.toHaveProperty("mailbox");
    expect(inbox.items[0]).not.toHaveProperty("uid");
    expect(inbox.items[0]).not.toHaveProperty("body_text");

    const contacts = await mailApi.listContacts("demo-primary");
    expect(contacts.contacts).toContainEqual(
      expect.objectContaining({
        email: "chenyu@example.com",
        displayName: "陈屿",
        messageCount: 4,
      }),
    );
    const correspondence = await mailApi.listContactMessages(
      "demo-primary",
      "chenyu@example.com",
      10,
    );
    expect(correspondence).toHaveLength(4);
    expect(correspondence.map((message) => message.direction)).toEqual(
      expect.arrayContaining(["incoming", "outgoing"]),
    );

    const firstStarredPage = await mailApi.listStarredMailboxPage(
      "demo-primary",
      "inbox",
      null,
      1,
      null,
    );
    expect(firstStarredPage.items).toHaveLength(1);
    expect(firstStarredPage.items[0].flags).toContain("\\Flagged");
    expect(firstStarredPage.next_cursor).toBeTruthy();
    const secondStarredPage = await mailApi.loadOlderStarredMailboxPage(
      "demo-primary",
      "inbox",
      firstStarredPage.next_cursor,
      1,
      null,
    );
    expect(secondStarredPage.items).toHaveLength(1);
    expect(secondStarredPage.items[0].flags).toContain("\\Flagged");
    await expect(
      mailApi.loadOlderMailboxPage(
        "demo-primary",
        "inbox",
        firstStarredPage.next_cursor,
        1,
        null,
      ),
    ).rejects.toThrow("分页游标");

    await expect(
      mailApi.setMessageSeen("demo-message-01", true),
    ).resolves.toEqual(
      expect.objectContaining({
        operation_id: "demo-seen-1",
        status: "pending",
        source_role: "inbox",
      }),
    );
    await expect(mailApi.archiveMessage("demo-message-01")).resolves.toEqual(
      expect.objectContaining({
        operation_id: "demo-archive-2",
        source_role: "inbox",
        destination_role: "archive",
      }),
    );
    const archive = await mailApi.listMailboxPage(
      "demo-primary",
      "archive",
      null,
      50,
      null,
    );
    expect(archive.items[0]).toEqual(
      expect.objectContaining({
        id: "demo-message-01",
        displayed_role: "archive",
        pending_mutation: expect.objectContaining({ status: "pending" }),
      }),
    );
    await expect(mailApi.moveMessageToInbox("demo-message-01")).resolves.toEqual(
      expect.objectContaining({
        operation_id: "demo-move_to_inbox-3",
        source_role: "archive",
        destination_role: "inbox",
      }),
    );
    const restoredInbox = await mailApi.listMailboxPage(
      "demo-primary",
      "inbox",
      null,
      50,
      null,
    );
    expect(restoredInbox.items[0]).toEqual(
      expect.objectContaining({
        id: "demo-message-01",
        displayed_role: "inbox",
        pending_mutation: expect.objectContaining({
          kind: "move_to_inbox",
          status: "pending",
        }),
      }),
    );
    const selected = await mailApi.fetchMailboxMessage("demo-message-01");
    expect(selected.body_text).toContain("欢迎来到 Mine Mail");
    expect(selected.attachments).toEqual([]);
    expect(selected).not.toHaveProperty("mailbox");
    expect(selected).not.toHaveProperty("uid");
    await expect(
      mailApi.listMailboxPage("demo-primary", "inbox", null, 101, null),
    ).rejects.toThrow("分页大小");
    await expect(
      mailApi.resolveDeliveryUnknown({
        outboxId: "missing-demo-outbox",
        expectedAttempts: 1,
        decision: "confirm_delivered",
        acknowledgeDuplicateRisk: false,
      }),
    ).rejects.toThrow("投递结果已变化");
  });

  it("requires an explicit demo build outside Tauri and test mode", async () => {
    const { __testing } = await import("./mailApi.js");

    expect(
      __testing.demoAdapterBuildEnabled({
        demoFlag: undefined,
        mode: "production",
      }),
    ).toBe(false);
    expect(
      __testing.demoAdapterBuildEnabled({
        demoFlag: "1",
        mode: "production",
      }),
    ).toBe(true);
    expect(
      __testing.demoAdapterBuildEnabled({
        demoFlag: undefined,
        mode: "test",
      }),
    ).toBe(true);
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
