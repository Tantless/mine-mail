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
import { useProseMirrorTestGeometry } from "./test/proseMirrorTestGeometry.js";

useProseMirrorTestGeometry();

const desktop = vi.hoisted(() => {
  const listeners = new Map();
  const fixtures = {
    inboxPageSource: vi.fn(),
    sentPageSource: vi.fn(),
    inboxMessageSource: vi.fn(),
    sentMessageSource: vi.fn(),
    recordInboxSeen: vi.fn(),
    recordStarred: vi.fn(),
  };
  return {
    listeners,
    fixtures,
    messageRoles: new Map(),
    mailApi: {
      getMailboxCapabilities: vi.fn(),
      createMailboxRole: vi.fn(),
      listMailboxPage: vi.fn(),
      loadOlderMailboxPage: vi.fn(),
      listStarredMailboxPage: vi.fn(),
      loadOlderStarredMailboxPage: vi.fn(),
      syncMailbox: vi.fn(),
      fetchMailboxMessage: vi.fn(),
      saveMessageAttachment: vi.fn(),
      prepareForward: vi.fn(),
      setMessageSeen: vi.fn(),
      setMessageStarredById: vi.fn(),
      archiveMessage: vi.fn(),
      moveMessageToTrash: vi.fn(),
      preparePermanentDelete: vi.fn(),
      confirmPermanentDelete: vi.fn(),
      prepareReply: vi.fn(),
      openExternalUrl: vi.fn(),
      listDrafts: vi.fn(),
      saveDraft: vi.fn(),
      createComposeDraft: vi.fn(),
      addDraftAttachments: vi.fn(),
      removeDraftAttachment: vi.fn(),
      deleteDraft: vi.fn(),
      syncDrafts: vi.fn(),
      syncSent: vi.fn(),
      syncAll: vi.fn(),
      completeExit: vi.fn(),
      cancelExit: vi.fn(),
      listOutbox: vi.fn(),
      listSentOutboxFallbacks: vi.fn(),
      fetchOutboxMessage: vi.fn(),
      retryOutbox: vi.fn(),
      resolveDeliveryUnknown: vi.fn(),
      sendDraft: vi.fn(),
      getDesktopSettings: vi.fn(),
      updateDesktopSettings: vi.fn(),
      listAccountPresets: vi.fn(),
      getAccountStatus: vi.fn(),
      switchAccount: vi.fn(),
      configureAccount: vi.fn(),
      connectGoogleAccount: vi.fn(),
      listProfileAvatars: vi.fn(),
      saveProfileAvatar: vi.fn(),
      deleteProfileAvatar: vi.fn(),
      listContacts: vi.fn(),
      listContactMessages: vi.fn(),
      setContactFavorite: vi.fn(),
      setContactRemark: vi.fn(),
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
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function savedOutcome(request, draftId, expectedLocalVersion = null) {
  return {
    kind: "saved",
    draft: {
      ...request,
      id: draftId || "exit-draft",
      local_version:
        expectedLocalVersion === null ? 1 : expectedLocalVersion + 1,
      status: "local",
      updated_at: "2026-07-14T09:10:00Z",
    },
    canonical: null,
  };
}

function minimizeComposer(dialog = screen.getByRole("dialog")) {
  fireEvent.pointerDown(dialog.closest(".compose-layer"), {
    button: 0,
    clientX: 2,
    clientY: 2,
  });
}

function summary(uid, subject) {
  return {
    id: String(uid),
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

function mailboxPage(items, role, overrides = {}) {
  return {
    items: items.map((item) => ({
      ...item,
      displayed_role: role,
    })),
    next_cursor: null,
    has_more_local: false,
    remote_history_state: "complete",
    end_reached: true,
    ...overrides,
  };
}

function installWorkspaceMedia({
  width = 600,
  reducedMotion = false,
} = {}) {
  let viewportWidth = width;
  let reduce = reducedMotion;
  const queries = new Map();
  const evaluate = (query) => {
    if (query.includes("prefers-reduced-motion")) return reduce;
    const minimum = query.match(/min-width:\s*(\d+)px/);
    return !minimum || viewportWidth >= Number(minimum[1]);
  };

  vi.stubGlobal(
    "matchMedia",
    vi.fn((media) => {
      if (queries.has(media)) return queries.get(media);
      const listeners = new Set();
      const query = {
        media,
        matches: evaluate(media),
        addEventListener: vi.fn((_, listener) => listeners.add(listener)),
        removeEventListener: vi.fn((_, listener) =>
          listeners.delete(listener),
        ),
        addListener: vi.fn((listener) => listeners.add(listener)),
        removeListener: vi.fn((listener) => listeners.delete(listener)),
        listeners,
      };
      queries.set(media, query);
      return query;
    }),
  );

  const notify = () => {
    for (const query of queries.values()) {
      const matches = evaluate(query.media);
      if (matches === query.matches) continue;
      query.matches = matches;
      query.listeners.forEach((listener) =>
        listener({ matches, media: query.media }),
      );
    }
  };
  const setWidth = (nextWidth) => {
    act(() => {
      viewportWidth = nextWidth;
      Object.defineProperty(window, "innerWidth", {
        configurable: true,
        value: nextWidth,
      });
      notify();
      window.dispatchEvent(new Event("resize"));
    });
  };
  const setReducedMotion = (nextValue) => {
    act(() => {
      reduce = nextValue;
      notify();
    });
  };

  setWidth(width);
  return { setReducedMotion, setWidth };
}

async function advanceWorkspaceMotion(milliseconds) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(milliseconds);
  });
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
    created_at: "2026-07-14T08:00:00Z",
    updated_at: `2026-07-14T08:0${localVersion}:00Z`,
  };
}

function deliveryUnknownOutbox(overrides = {}) {
  return {
    id: "outbox-unknown",
    draft_id: null,
    recipients: ["friend@example.com"],
    recipient_groups: {
      to: ["friend@example.com"],
      cc: [],
      bcc: [],
    },
    subject: "Unknown delivery message",
    preview: "Ambiguous delivery body",
    status: "delivery_unknown",
    attempts: 2,
    last_error: "SMTP connection closed before acknowledgement",
    created_at: "2026-07-14T09:00:00Z",
    sent_at: null,
    ...overrides,
  };
}

describe("Mine Mail desktop state bridge", () => {
  beforeEach(() => {
    desktop.listeners.clear();
    desktop.messageRoles.clear();
    Object.values(desktop.mailApi).forEach((mock) => mock.mockClear());
    Object.values(desktop.fixtures).forEach((mock) => mock.mockReset());
    desktop.fixtures.inboxPageSource.mockResolvedValue([
      summary(1, "First mail"),
    ]);
    desktop.fixtures.sentPageSource.mockResolvedValue([]);
    desktop.mailApi.getMailboxCapabilities.mockResolvedValue([
      { role: "inbox", status: "available", retryable: false },
      { role: "sent", status: "available", retryable: false },
      { role: "archive", status: "available", retryable: false },
      { role: "trash", status: "available", retryable: false },
    ]);
    desktop.mailApi.createMailboxRole.mockImplementation(async (_, role) => ({
      role,
      status: "available",
      retryable: false,
    }));
    desktop.mailApi.listMailboxPage.mockImplementation(
      async (_, role, _cursor, _pageSize, query) => {
        const source =
          role === "inbox"
            ? await desktop.fixtures.inboxPageSource(50)
            : role === "sent"
              ? await desktop.fixtures.sentPageSource(250)
              : [];
        const normalizedQuery = String(query || "").trim().toLowerCase();
        const items = normalizedQuery
          ? source.filter((item) =>
              [
                item.subject,
                item.preview,
                item.sender?.name,
                item.sender?.email,
              ].some((value) =>
                String(value || "").toLowerCase().includes(normalizedQuery),
              ),
            )
          : source;
        items.forEach((item) =>
          desktop.messageRoles.set(String(item.id), role),
        );
        return mailboxPage(items, role);
      },
    );
    desktop.mailApi.loadOlderMailboxPage.mockImplementation(
      async (accountId, role, cursor, pageSize, query) =>
        desktop.mailApi.listMailboxPage(
          accountId,
          role,
          cursor,
          pageSize,
          query,
        ),
    );
    desktop.mailApi.syncMailbox.mockResolvedValue(undefined);
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
    desktop.mailApi.listSentOutboxFallbacks.mockResolvedValue([]);
    desktop.mailApi.getDesktopSettings.mockResolvedValue({
      pollingIntervalMinutes: 5,
      autostartEnabled: false,
      notificationsEnabled: true,
      notificationSoundEnabled: true,
      notificationSound: "mail",
      remoteImageMode: "automatic",
    });
    desktop.mailApi.listAccountPresets.mockResolvedValue([]);
    desktop.mailApi.getAccountStatus.mockResolvedValue({
      configured: true,
      accountId: "desktop-account",
      activeAccountId: "desktop-account",
      provider: "163",
      email: "me@163.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
      startupError: null,
      accounts: [
        {
          accountId: "desktop-account",
          provider: "163",
          email: "me@163.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
      ],
    });
    desktop.mailApi.switchAccount.mockImplementation(async (accountId) => {
      const status = await desktop.mailApi.getAccountStatus();
      const account = status.accounts?.find(
        (candidate) => candidate.accountId === accountId,
      );
      return {
        ...status,
        ...(account || {}),
        accountId,
        activeAccountId: accountId,
      };
    });
    desktop.mailApi.listProfileAvatars.mockResolvedValue([]);
    desktop.mailApi.saveProfileAvatar.mockImplementation(async (request) => ({
      ownerType: request.ownerType,
      ownerKey: request.ownerKey,
      imageDataUrl: request.imageDataUrl,
    }));
    desktop.mailApi.deleteProfileAvatar.mockResolvedValue(undefined);
    desktop.mailApi.listContacts.mockResolvedValue([]);
    desktop.mailApi.listContactMessages.mockResolvedValue([]);
    desktop.mailApi.setContactFavorite.mockResolvedValue(true);
    desktop.mailApi.setContactRemark.mockResolvedValue(true);
    desktop.fixtures.inboxMessageSource.mockImplementation(async (uid) => ({
      ...summary(uid, "First mail"),
      body_text: "Loaded body",
      body_fetched: true,
    }));
    desktop.mailApi.fetchMailboxMessage.mockImplementation(
      async (messageId) =>
        desktop.messageRoles.get(String(messageId)) === "sent"
          ? desktop.fixtures.sentMessageSource(Number(messageId))
          : desktop.fixtures.inboxMessageSource(Number(messageId)),
    );
    desktop.mailApi.saveMessageAttachment.mockResolvedValue({
      status: "saved",
      file_name: "attachment.bin",
      retryable: false,
    });
    desktop.mailApi.prepareForward.mockResolvedValue({
      kind: "error",
      error: {
        kind: "message_unavailable",
        failed_attachment_ids: [],
        retry_without_attachments_allowed: false,
      },
    });
    desktop.fixtures.recordInboxSeen.mockResolvedValue(true);
    desktop.mailApi.setMessageSeen.mockImplementation(
      async (messageId, seen) => {
        if (seen) await desktop.fixtures.recordInboxSeen(Number(messageId));
        return {
          operation_id: `seen-${messageId}`,
          local_revision: 1,
          status: "pending",
          source_role: "inbox",
          flag: "seen",
          desired: seen,
        };
      },
    );
    desktop.fixtures.recordStarred.mockResolvedValue(true);
    desktop.mailApi.setMessageStarredById.mockImplementation(
      async (messageId, starred) => {
        const role = desktop.messageRoles.get(String(messageId));
        await desktop.fixtures.recordStarred(
          role === "sent" ? "Sent" : "INBOX",
          Number(messageId),
          starred,
        );
        return {
          operation_id: `star-${messageId}`,
          local_revision: 1,
          status: "pending",
          source_role: "inbox",
          flag: "flagged",
          desired: starred,
        };
      },
    );
    desktop.mailApi.archiveMessage.mockImplementation(async (messageId) => ({
      operation_id: `archive-${messageId}`,
      local_revision: 1,
      status: "pending",
      source_role: "inbox",
      destination_role: "archive",
    }));
    desktop.mailApi.moveMessageToTrash.mockImplementation(
      async (messageId) => ({
        operation_id: `trash-${messageId}`,
        local_revision: 1,
        status: "pending",
        source_role: "inbox",
        destination_role: "trash",
      }),
    );
    desktop.mailApi.preparePermanentDelete.mockImplementation(
      async (messageId) => ({
        plan_id: `delete-plan-${messageId}`,
        expires_at: "2026-07-14T09:05:00Z",
      }),
    );
    desktop.mailApi.confirmPermanentDelete.mockResolvedValue({
      operation_id: "permanent-delete-1",
      local_revision: 1,
      status: "pending",
      source_role: "trash",
    });
    desktop.fixtures.sentMessageSource.mockImplementation(async (uid) => ({
      ...summary(uid, "Sent mail"),
      mailbox: "Sent",
      to: [{ name: "Friend", email: "friend@example.com" }],
      body_text: "Loaded sent body",
      body_fetched: true,
    }));
    desktop.mailApi.fetchOutboxMessage.mockImplementation(async (outboxId) => ({
      id: outboxId,
      subject: "Outbox subject",
      body_text: "Actual Outbox body",
      body_fetched: true,
    }));
    desktop.mailApi.openExternalUrl.mockResolvedValue(true);
    desktop.mailApi.syncAll.mockResolvedValue({ inbox: { fetched: 0 } });
    desktop.mailApi.syncSent.mockResolvedValue({ mailbox: "Sent", fetched: 0 });
    desktop.mailApi.deleteDraft.mockResolvedValue({ kind: "deleted" });
    desktop.mailApi.completeExit.mockResolvedValue(true);
    desktop.mailApi.cancelExit.mockResolvedValue(true);
    desktop.mailApi.retryOutbox.mockResolvedValue({
      id: "outbox-1",
      status: "sent",
      attempts: 2,
    });
    desktop.mailApi.resolveDeliveryUnknown.mockResolvedValue({
      id: "outbox-1",
      status: "sent",
      attempts: 1,
    });
    desktop.mailApi.saveDraft.mockImplementation(
      async (request, draftId, expectedLocalVersion) =>
        savedOutcome(request, draftId, expectedLocalVersion),
    );
    desktop.mailApi.createComposeDraft.mockResolvedValue({
      id: "blank-compose-draft",
      local_version: 1,
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
      attachments: [],
      forward_context: null,
      status: "local",
    });
    desktop.mailApi.addDraftAttachments.mockImplementation(
      async (draftId, expectedLocalVersion) => ({
        kind: "canceled",
        draft: {
          id: draftId,
          local_version: expectedLocalVersion,
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
          attachments: [],
          forward_context: null,
          status: "local",
        },
      }),
    );
    desktop.mailApi.removeDraftAttachment.mockImplementation(
      async (draftId, _attachmentId, expectedLocalVersion) => ({
        kind: "saved",
        draft: {
          id: draftId,
          local_version: expectedLocalVersion + 1,
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
          attachments: [],
          forward_context: null,
          status: "local",
        },
      }),
    );
    window.localStorage.clear();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    cleanup();
  });

  describe("wide mail workspace motion", () => {
    async function renderWideWorkspace() {
      const media = installWorkspaceMedia({ width: 600 });
      render(<App />);
      const opener = await screen.findByRole("button", {
        name: "打开邮件：Sender 1，First mail",
      });
      media.setWidth(1440);
      await screen.findByRole("button", { name: "收起邮件列表" });
      vi.useFakeTimers();
      return { media, opener };
    }

    function startWideContactsClick() {
      fireEvent.click(
        screen.getByRole("button", { name: "通讯录" }),
      );
      return import("./components/ContactsWorkspace.jsx");
    }

    async function finishContactsPreparation(moduleReady) {
      await act(async () => {
        await moduleReady;
      });
    }

    async function showWideContacts() {
      const contactsReady = startWideContactsClick();
      await finishContactsPreparation(contactsReady);
      await advanceWorkspaceMotion(250);
      await advanceWorkspaceMotion(250);
      await act(async () => {
        await Promise.resolve();
      });
    }

    it("keeps reader content mounted through its reverse window transition and restores row focus", async () => {
      const { opener } = await renderWideWorkspace();

      fireEvent.click(opener);
      let reader = screen.getByLabelText("邮件阅读区");
      expect(reader.dataset.readerMotion).toBe("entering");
      await advanceWorkspaceMotion(400);
      reader = screen.getByLabelText("邮件阅读区");
      expect(reader.dataset.readerMotion).toBe("open");

      const back = within(reader).getByRole("button", {
        name: "返回邮件列表",
      });
      fireEvent.click(back);
      expect(reader.dataset.readerMotion).toBe("exiting");
      expect(document.querySelector(".reader-panel--message")).toBe(reader);

      await advanceWorkspaceMotion(400);
      expect(document.querySelector(".reader-panel--message")).toBeNull();
      expect(screen.getByLabelText("邮件阅读区，当前未打开邮件")).toBeTruthy();
      expect(document.activeElement).toBe(opener);
    });

    it("replaces an already-open message without animating the reader again", async () => {
      desktop.fixtures.inboxPageSource.mockResolvedValue([
        summary(1, "First mail"),
        summary(2, "Second mail"),
      ]);
      desktop.fixtures.inboxMessageSource.mockImplementation(async (uid) => ({
        ...summary(uid, uid === 2 ? "Second mail" : "First mail"),
        body_text: `Loaded body ${uid}`,
        body_fetched: true,
      }));
      const { opener } = await renderWideWorkspace();
      fireEvent.click(opener);
      await advanceWorkspaceMotion(400);

      fireEvent.click(
        screen.getByRole("button", {
          name: "打开邮件：Sender 2，Second mail",
        }),
      );

      const reader = screen.getByLabelText("邮件阅读区");
      expect(reader.dataset.readerMotion).toBe("open");
      expect(
        within(reader).getByRole("heading", { name: "Second mail" }),
      ).toBeTruthy();
    });

    it("animates account workspace changes and restores each account navigation state", async () => {
      const accounts = [
        {
          accountId: "account-a",
          provider: "163",
          email: "a@example.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
        {
          accountId: "account-b",
          provider: "gmail",
          email: "b@example.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
      ];
      let activeAccountId = "account-a";
      const statusFor = (accountId) => ({
        configured: true,
        ...accounts.find((account) => account.accountId === accountId),
        accountId,
        activeAccountId: accountId,
        accounts,
      });
      desktop.mailApi.getAccountStatus.mockImplementation(async () =>
        statusFor(activeAccountId),
      );
      desktop.mailApi.switchAccount.mockImplementation(async (accountId) => {
        activeAccountId = accountId;
        return statusFor(accountId);
      });
      const accountAInbox = summary(1, "First mail");
      const accountBSent = summary("account-b-sent", "B sent state");
      desktop.mailApi.listMailboxPage.mockImplementation(
        async (accountId, role) =>
          mailboxPage(
            accountId === "account-a" && role === "inbox"
              ? [accountAInbox]
              : accountId === "account-b" && role === "sent"
                ? [accountBSent]
                : [],
            role,
          ),
      );
      desktop.mailApi.fetchMailboxMessage.mockImplementation(
        async (messageId) => ({
          ...(messageId === accountAInbox.id
            ? accountAInbox
            : accountBSent),
          body_text:
            messageId === accountAInbox.id ? "A reader body" : "B reader body",
          body_fetched: true,
        }),
      );

      const { opener } = await renderWideWorkspace();
      fireEvent.click(opener);
      await advanceWorkspaceMotion(400);
      expect(screen.getByText("A reader body")).toBeTruthy();

      fireEvent.click(
        screen.getByRole("button", { name: "切换到 b@example.com" }),
      );
      await act(async () => Promise.resolve());
      expect(
        screen.getByLabelText("邮件阅读区").dataset.readerMotion,
      ).toBe("exiting");
      await advanceWorkspaceMotion(240);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("collapsing");
      await advanceWorkspaceMotion(400);
      expect(
        screen.getByRole("button", { name: "当前账户 b@example.com" }),
      ).toBeTruthy();
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("collapsed");
      expect(
        document.querySelectorAll(
          '.folder-nav__item[data-selected="true"]',
        ),
      ).toHaveLength(0);
      expect(screen.queryByText("B reader body")).toBeNull();

      fireEvent.click(screen.getByRole("button", { name: "已发送" }));
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanding");
      await advanceWorkspaceMotion(400);
      fireEvent.click(
        screen.getByRole("button", {
          name: /打开邮件：.*B sent state/,
        }),
      );
      await advanceWorkspaceMotion(400);
      expect(screen.getByText("B reader body")).toBeTruthy();

      fireEvent.click(
        screen.getByRole("button", { name: "切换到 a@example.com" }),
      );
      await act(async () => Promise.resolve());
      await advanceWorkspaceMotion(240);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("switching-out");
      await advanceWorkspaceMotion(250);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("switching-in");
      expect(
        screen.getByRole("button", { name: "收件箱" }).getAttribute(
          "aria-current",
        ),
      ).toBe("page");
      await advanceWorkspaceMotion(250);
      await advanceWorkspaceMotion(400);
      expect(screen.getByText("A reader body")).toBeTruthy();

      fireEvent.click(
        screen.getByRole("button", { name: "切换到 b@example.com" }),
      );
      await act(async () => Promise.resolve());
      await advanceWorkspaceMotion(240);
      await advanceWorkspaceMotion(250);
      expect(
        screen.getByRole("button", { name: "已发送" }).getAttribute(
          "aria-current",
        ),
      ).toBe("page");
      await advanceWorkspaceMotion(250);
      await advanceWorkspaceMotion(400);
      expect(screen.getByText("B reader body")).toBeTruthy();
    });

    it("closes an open reader before the current sidebar folder retracts the list, then reveals another folder", async () => {
      const { opener } = await renderWideWorkspace();
      fireEvent.click(opener);
      const reader = screen.getByLabelText("邮件阅读区");
      const inbox = screen.getByRole("button", { name: "收件箱" });
      const folderSelection = document.querySelector(
        ".folder-nav__selection",
      );
      await advanceWorkspaceMotion(400);

      fireEvent.click(inbox);
      expect(reader.dataset.readerMotion).toBe("exiting");
      expect(reader.dataset.readerExitSpeed).toBe("fast");
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
      expect(inbox.dataset.selected).toBe("true");

      await advanceWorkspaceMotion(240);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("collapsing");
      expect(inbox.dataset.selected).toBe("false");
      expect(inbox.hasAttribute("aria-current")).toBe(false);
      expect(folderSelection.dataset.visible).toBeUndefined();
      expect(
        document.querySelectorAll(
          '.folder-nav__item[data-selected="true"]',
        ),
      ).toHaveLength(0);
      await advanceWorkspaceMotion(400);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("collapsed");
      expect(
        screen.getByRole("button", { name: "收件箱" }).getAttribute(
          "aria-expanded",
        ),
      ).toBe("false");

      fireEvent.click(screen.getByRole("button", { name: "已收藏" }));
      const starred = screen.getByRole("button", { name: "已收藏" });
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanding");
      expect(starred.dataset.selected).toBe("true");
      expect(starred.getAttribute("aria-current")).toBe("page");
      expect(folderSelection.dataset.visible).toBe("true");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("已收藏");
      await advanceWorkspaceMotion(400);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
    });

    it("closes an open reader before switching the visible list at its midpoint", async () => {
      const { opener } = await renderWideWorkspace();
      fireEvent.click(opener);
      const reader = screen.getByLabelText("邮件阅读区");
      await advanceWorkspaceMotion(400);

      fireEvent.click(screen.getByRole("button", { name: "已收藏" }));
      expect(reader.dataset.readerMotion).toBe("exiting");
      expect(reader.dataset.readerExitSpeed).toBe("fast");
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");

      await advanceWorkspaceMotion(240);
      expect(document.querySelector(".reader-panel--message")).toBeNull();
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("switching-out");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("收件箱");

      await advanceWorkspaceMotion(250);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("switching-in");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("已收藏");

      await advanceWorkspaceMotion(250);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
    });

    it("changes folders only at the shrink midpoint and retargets rapid navigation to the latest folder", async () => {
      await renderWideWorkspace();

      fireEvent.click(screen.getByRole("button", { name: "已发送" }));
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("switching-out");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("收件箱");

      fireEvent.click(screen.getByRole("button", { name: "已收藏" }));
      await advanceWorkspaceMotion(250);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("switching-in");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("已收藏");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).not.toBe("已发送");

      await advanceWorkspaceMotion(250);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
    });

    it("retargets a pending switch back to the visible folder instead of retracting it", async () => {
      await renderWideWorkspace();
      const workspace = document.querySelector(".mail-workspace");

      fireEvent.click(screen.getByRole("button", { name: "已发送" }));
      expect(workspace.dataset.listMotion).toBe("switching-out");

      fireEvent.click(screen.getByRole("button", { name: "收件箱" }));
      expect(workspace.dataset.listMotion).toBe("switching-out");

      await advanceWorkspaceMotion(250);
      expect(workspace.dataset.listMotion).toBe("switching-in");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("收件箱");

      await advanceWorkspaceMotion(250);
      expect(workspace.dataset.listMotion).toBe("expanded");
      expect(
        screen.getByRole("button", { name: "收件箱" }).dataset.selected,
      ).toBe("true");
    });

    it("uses the same midpoint scene transition from Starred through Contacts to Sent", async () => {
      await renderWideWorkspace();
      const workspace = document.querySelector(".mail-workspace");

      fireEvent.click(screen.getByRole("button", { name: "已收藏" }));
      await advanceWorkspaceMotion(250);
      await advanceWorkspaceMotion(250);
      expect(workspace.dataset.listMotion).toBe("expanded");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("已收藏");

      const contactsReady = startWideContactsClick();
      expect(workspace.dataset.listMotion).toBe("expanded");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("已收藏");
      await finishContactsPreparation(contactsReady);
      expect(workspace.dataset.listMotion).toBe("switching-out");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("已收藏");
      expect(document.querySelectorAll(".mail-list-panel")).toHaveLength(1);

      await advanceWorkspaceMotion(250);
      expect(workspace.dataset.listMotion).toBe("switching-in");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("通讯录");
      expect(document.querySelectorAll(".mail-list-panel")).toHaveLength(1);

      await advanceWorkspaceMotion(250);
      expect(workspace.dataset.listMotion).toBe("expanded");

      fireEvent.click(screen.getByRole("button", { name: "已发送" }));
      expect(workspace.dataset.listMotion).toBe("switching-out");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("通讯录");

      await advanceWorkspaceMotion(250);
      expect(workspace.dataset.listMotion).toBe("switching-in");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("已发送");

      await advanceWorkspaceMotion(250);
      expect(workspace.dataset.listMotion).toBe("expanded");
    });

    it("retracts Contacts immediately when no contact detail is open", async () => {
      await renderWideWorkspace();
      await showWideContacts();

      const workspace = document.querySelector(".mail-workspace");
      const contacts = screen.getByRole("button", { name: "通讯录" });
      expect(
        document.querySelector(".contacts-detail-panel--selected"),
      ).toBeNull();

      fireEvent.click(contacts);
      expect(workspace.dataset.listMotion).toBe("collapsing");
      expect(contacts.dataset.selected).toBe("false");
      expect(contacts.hasAttribute("aria-current")).toBe(false);

      await advanceWorkspaceMotion(400);
      expect(workspace.dataset.listMotion).toBe("collapsed");
    });

    it("fast-exits contact detail before a repeated Contacts click retracts and reveals the list", async () => {
      desktop.mailApi.listContacts.mockResolvedValue({
        contacts: [
          {
            accountId: "desktop-account",
            email: "friend@example.com",
            displayName: "Friend",
            isFavorite: false,
            messageCount: 0,
          },
        ],
        favorites: [],
      });
      await renderWideWorkspace();
      await showWideContacts();

      const workspace = document.querySelector(".mail-workspace");
      const contacts = screen.getByRole("button", { name: "通讯录" });
      const folderSelection = document.querySelector(
        ".folder-nav__selection",
      );
      expect(document.getElementById("mail-list-panel")).toBeTruthy();

      fireEvent.click(
        screen.getByRole("button", { name: "查看联系人 Friend" }),
      );
      await advanceWorkspaceMotion(400);
      const detail = screen.getByLabelText("Friend 的联系人详情");

      fireEvent.click(contacts);
      expect(detail.dataset.readerMotion).toBe("exiting");
      expect(detail.dataset.readerExitSpeed).toBe("fast");
      expect(workspace.dataset.listMotion).toBe("expanded");
      expect(contacts.dataset.selected).toBe("true");

      await advanceWorkspaceMotion(240);
      expect(
        document.querySelector(".contacts-detail-panel--selected"),
      ).toBeNull();
      expect(workspace.dataset.listMotion).toBe("collapsing");
      expect(contacts.dataset.selected).toBe("false");
      expect(contacts.hasAttribute("aria-current")).toBe(false);
      expect(contacts.getAttribute("aria-expanded")).toBe("false");
      expect(folderSelection.dataset.visible).toBeUndefined();
      expect(
        document.querySelectorAll(
          '.folder-nav__item[data-selected="true"]',
        ),
      ).toHaveLength(0);

      await advanceWorkspaceMotion(400);
      expect(workspace.dataset.listMotion).toBe("collapsed");
      expect(contacts.dataset.selected).toBe("false");

      fireEvent.click(contacts);
      expect(workspace.dataset.listMotion).toBe("expanding");
      expect(contacts.dataset.selected).toBe("true");
      expect(contacts.getAttribute("aria-current")).toBe("page");
      expect(contacts.getAttribute("aria-expanded")).toBe("true");
      expect(folderSelection.dataset.visible).toBe("true");

      await advanceWorkspaceMotion(400);
      expect(workspace.dataset.listMotion).toBe("expanded");
      expect(
        document.querySelector(".contacts-list-panel h1").textContent,
      ).toBe("通讯录");
    });

    it("animates the first contact detail and reverse exit, but swaps an open contact in place", async () => {
      desktop.mailApi.listContacts.mockResolvedValue({
        contacts: [
          {
            accountId: "desktop-account",
            email: "friend@example.com",
            displayName: "Friend",
            isFavorite: false,
            messageCount: 0,
          },
          {
            accountId: "desktop-account",
            email: "colleague@example.com",
            displayName: "Colleague",
            isFavorite: false,
            messageCount: 0,
          },
        ],
        favorites: [],
      });
      await renderWideWorkspace();
      await showWideContacts();

      const friendRow = screen.getByRole("button", {
        name: "查看联系人 Friend",
      });
      fireEvent.click(friendRow);
      let detail = screen.getByLabelText("Friend 的联系人详情");
      expect(detail.dataset.readerMotion).toBe("entering");
      expect(detail.dataset.readerExitSpeed).toBe("normal");

      await advanceWorkspaceMotion(400);
      detail = screen.getByLabelText("Friend 的联系人详情");
      expect(detail.dataset.readerMotion).toBe("open");

      const colleagueRow = screen.getByRole("button", {
        name: "查看联系人 Colleague",
      });
      fireEvent.click(colleagueRow);
      detail = screen.getByLabelText("Colleague 的联系人详情");
      expect(detail.dataset.readerMotion).toBe("open");

      fireEvent.click(
        within(detail).getByRole("button", { name: "返回通讯录" }),
      );
      expect(detail.dataset.readerMotion).toBe("exiting");
      expect(detail.dataset.readerExitSpeed).toBe("normal");
      expect(
        document.querySelector(".contacts-detail-panel--selected"),
      ).toBe(detail);

      await advanceWorkspaceMotion(400);
      expect(
        document.querySelector(".contacts-detail-panel--selected"),
      ).toBeNull();
      expect(
        screen.getByLabelText("邮件阅读区，当前未打开邮件"),
      ).toBeTruthy();
      expect(document.activeElement).toBe(colleagueRow);
    });

    it("fast-exits contact detail before switching away from Contacts", async () => {
      desktop.mailApi.listContacts.mockResolvedValue({
        contacts: [
          {
            accountId: "desktop-account",
            email: "friend@example.com",
            displayName: "Friend",
            isFavorite: false,
            messageCount: 0,
          },
        ],
        favorites: [],
      });
      await renderWideWorkspace();
      await showWideContacts();

      fireEvent.click(
        screen.getByRole("button", { name: "查看联系人 Friend" }),
      );
      await advanceWorkspaceMotion(400);
      const detail = screen.getByLabelText("Friend 的联系人详情");
      const workspace = document.querySelector(".mail-workspace");

      fireEvent.click(screen.getByRole("button", { name: "已发送" }));
      expect(detail.dataset.readerMotion).toBe("exiting");
      expect(detail.dataset.readerExitSpeed).toBe("fast");
      expect(workspace.dataset.listMotion).toBe("expanded");

      await advanceWorkspaceMotion(240);
      expect(
        document.querySelector(".contacts-detail-panel--selected"),
      ).toBeNull();
      expect(workspace.dataset.listMotion).toBe("switching-out");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("通讯录");
    });

    it("does not flash contact detail after a correspondence reader exits for folder navigation", async () => {
      const history = {
        ...summary(33, "Contact history"),
        mailbox_role: "inbox",
        kind: "inbox",
        direction: "incoming",
      };
      desktop.mailApi.listContacts.mockResolvedValue({
        contacts: [
          {
            accountId: "desktop-account",
            email: "friend@example.com",
            displayName: "Friend",
            isFavorite: false,
            messageCount: 1,
          },
        ],
        favorites: [],
      });
      desktop.mailApi.listContactMessages.mockResolvedValue([history]);
      desktop.mailApi.fetchMailboxMessage.mockResolvedValue({
        ...history,
        body_text: "Contact history body",
        body_fetched: true,
      });
      await renderWideWorkspace();
      await showWideContacts();

      fireEvent.click(
        screen.getByRole("button", { name: "查看联系人 Friend" }),
      );
      await advanceWorkspaceMotion(400);
      await act(async () => {
        await Promise.resolve();
      });
      fireEvent.click(
        screen.getByRole("button", { name: "打开邮件：Contact history" }),
      );
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });
      await advanceWorkspaceMotion(400);
      const reader = screen.getByLabelText("邮件阅读区");
      expect(reader.dataset.readerMotion).toBe("open");

      fireEvent.click(screen.getByRole("button", { name: "已发送" }));
      expect(reader.dataset.readerMotion).toBe("exiting");
      expect(reader.dataset.readerExitSpeed).toBe("fast");

      await advanceWorkspaceMotion(240);
      expect(document.querySelector(".reader-panel--message")).toBeNull();
      expect(
        document.querySelector(".contacts-detail-panel--selected"),
      ).toBeNull();
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("switching-out");
    });

    it("lets a later list retraction cancel a prepared Contacts switch", async () => {
      await renderWideWorkspace();
      const contactsReady = startWideContactsClick();

      fireEvent.click(
        screen.getByRole("button", { name: "收起邮件列表" }),
      );
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("collapsing");

      await finishContactsPreparation(contactsReady);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("collapsing");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("收件箱");

      await advanceWorkspaceMotion(400);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("collapsed");
    });

    it("opens Starred details, then closes the reader before switching to Contacts", async () => {
      desktop.fixtures.inboxPageSource.mockResolvedValue([
        summary(1, "First mail"),
        {
          ...summary(11, "Starred entrance"),
          flags: ["\\Seen", "\\Flagged"],
          body_text: "Starred body",
          body_fetched: true,
        },
      ]);
      await renderWideWorkspace();

      fireEvent.click(screen.getByRole("button", { name: "已收藏" }));
      await advanceWorkspaceMotion(250);
      await advanceWorkspaceMotion(250);
      fireEvent.click(
        within(screen.getByLabelText("已收藏邮件列表")).getByRole(
          "button",
          { name: "打开邮件：Sender 11，Starred entrance" },
        ),
      );

      let reader = screen.getByLabelText("邮件阅读区");
      expect(reader.dataset.readerMotion).toBe("entering");
      expect(reader.dataset.readerExitSpeed).toBe("normal");
      expect(
        within(reader).getByRole("heading", { name: "Starred entrance" }),
      ).toBeTruthy();

      await advanceWorkspaceMotion(400);
      reader = screen.getByLabelText("邮件阅读区");
      expect(reader.dataset.readerMotion).toBe("open");

      const contactsReady = startWideContactsClick();
      expect(reader.dataset.readerMotion).toBe("exiting");
      expect(reader.dataset.readerExitSpeed).toBe("fast");
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
      await finishContactsPreparation(contactsReady);

      await advanceWorkspaceMotion(240);
      expect(document.querySelector(".reader-panel--message")).toBeNull();
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("switching-out");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("已收藏");

      await advanceWorkspaceMotion(250);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("switching-in");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("通讯录");
      await advanceWorkspaceMotion(250);
    });

    it("opens Sent message details with the reader entrance motion", async () => {
      desktop.fixtures.sentPageSource.mockResolvedValue([
        {
          ...summary(71, "Sent entrance"),
          to: [{ name: "Friend", email: "friend@example.com" }],
          flags: ["\\Seen"],
          body_text: "Sent body",
          body_fetched: true,
        },
      ]);
      await renderWideWorkspace();

      fireEvent.click(screen.getByRole("button", { name: "已发送" }));
      await advanceWorkspaceMotion(250);
      await advanceWorkspaceMotion(250);
      fireEvent.click(
        within(screen.getByLabelText("已发送邮件列表")).getByRole(
          "button",
          { name: /^打开邮件：.*，Sent entrance$/ },
        ),
      );

      let reader = screen.getByLabelText("邮件阅读区");
      expect(reader.dataset.readerMotion).toBe("entering");
      expect(reader.dataset.readerExitSpeed).toBe("normal");
      expect(
        within(reader).getByRole("heading", { name: "Sent entrance" }),
      ).toBeTruthy();

      await advanceWorkspaceMotion(400);
      reader = screen.getByLabelText("邮件阅读区");
      expect(reader.dataset.readerMotion).toBe("open");
    });

    it("cancels a delayed retraction when the workspace becomes compact during reader exit", async () => {
      const { media, opener } = await renderWideWorkspace();
      fireEvent.click(opener);
      await advanceWorkspaceMotion(400);

      fireEvent.click(
        screen.getByRole("button", { name: "收起邮件列表" }),
      );
      expect(
        screen.getByLabelText("邮件阅读区").dataset.readerMotion,
      ).toBe("exiting");

      media.setWidth(900);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
      await advanceWorkspaceMotion(240);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
      expect(document.querySelector(".reader-panel--message")).toBeNull();

      media.setWidth(1440);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
      expect(
        screen.getByRole("button", { name: "收起邮件列表" }),
      ).toBeTruthy();
    });

    it("commits the latest folder atomically when a switching workspace becomes compact", async () => {
      const { media } = await renderWideWorkspace();

      fireEvent.click(screen.getByRole("button", { name: "已发送" }));
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("switching-out");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("收件箱");

      fireEvent.click(screen.getByRole("button", { name: "收件箱" }));
      fireEvent.click(screen.getByRole("button", { name: "已收藏" }));
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("switching-out");

      media.setWidth(900);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("已收藏");

      media.setWidth(1024);
    });

    it("reveals the list before a desktop notification presents message details", async () => {
      const { media } = await renderWideWorkspace();
      expect(desktop.listeners.has("mail:open-message")).toBe(true);
      fireEvent.click(
        screen.getByRole("button", { name: "收起邮件列表" }),
      );
      await advanceWorkspaceMotion(400);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("collapsed");

      vi.useRealTimers();
      act(() => {
        desktop.listeners.get("mail:open-message")?.({
          payload: {
            message_id: "1",
            account_id: "desktop-account",
          },
        });
      });

      expect(await screen.findByText("Loaded body")).toBeTruthy();
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
      expect(document.querySelector(".reader-panel--message")).toBeTruthy();
      media.setWidth(1024);
    });

    it("uses atomic navigation for reduced motion and keeps retraction out of compact layouts", async () => {
      const { media } = await renderWideWorkspace();
      media.setReducedMotion(true);

      fireEvent.click(screen.getByRole("button", { name: "已收藏" }));
      expect(
        document.querySelector(".mail-list-panel h1").textContent,
      ).toBe("已收藏");
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");

      fireEvent.click(screen.getByRole("button", { name: "已收藏" }));
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("collapsed");
      const starred = screen.getByRole("button", { name: "已收藏" });
      const folderSelection = document.querySelector(
        ".folder-nav__selection",
      );
      expect(starred.dataset.selected).toBe("false");
      expect(starred.hasAttribute("aria-current")).toBe(false);
      expect(folderSelection.dataset.visible).toBeUndefined();

      media.setWidth(900);
      expect(
        screen.queryByRole("button", { name: "收起邮件列表" }),
      ).toBeNull();
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
      expect(starred.dataset.selected).toBe("true");
      expect(starred.getAttribute("aria-current")).toBe("page");
      expect(folderSelection.dataset.visible).toBe("true");

      fireEvent.click(starred);
      expect(
        document.querySelector(".mail-workspace").dataset.listMotion,
      ).toBe("expanded");
      expect(starred.dataset.selected).toBe("true");

      media.setWidth(1024);
    });
  });

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
      expect(desktop.mailApi.onMailEvent).toHaveBeenCalledWith(
        "mail:sent-updated",
        expect.any(Function),
      );
      expect(desktop.mailApi.onMailEvent).toHaveBeenCalledWith(
        "mail:account-updated",
        expect.any(Function),
      );
    });

    await act(async () => {
      desktop.listeners.get("mail:inbox-updated")?.({ payload: {} });
    });
    await waitFor(() =>
      expect(desktop.fixtures.inboxPageSource).toHaveBeenCalledTimes(2),
    );

    await act(async () => {
      desktop.listeners.get("mail:drafts-updated")?.({ payload: {} });
    });
    await waitFor(() => {
      expect(desktop.mailApi.listDrafts).toHaveBeenCalledTimes(2);
      expect(desktop.mailApi.listOutbox).toHaveBeenCalledTimes(2);
    });
  });

  it("shows a synchronized Sent preview without selecting the message", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const sent = {
      ...summary(71, "Backfilled sent mail"),
      mailbox: "Sent",
      to: [{ name: "Friend", email: "friend@example.com" }],
      preview: "",
    };
    desktop.fixtures.inboxPageSource.mockResolvedValue([]);
    desktop.fixtures.sentPageSource.mockResolvedValue([sent]);
    const user = userEvent.setup();

    render(<App />);
    await user.click(await screen.findByRole("button", { name: /已发送/ }));
    expect(await screen.findByText("暂无摘要")).toBeTruthy();
    await waitFor(() =>
      expect(desktop.listeners.has("mail:sent-updated")).toBe(true),
    );

    desktop.fixtures.sentPageSource.mockResolvedValue([
      { ...sent, preview: "同步后直接显示的有界摘要" },
    ]);
    await act(async () => {
      desktop.listeners.get("mail:sent-updated")?.({
        payload: {
          account_id: "desktop-account",
          completed: 1,
          total: 1,
          is_complete: true,
        },
      });
    });

    expect(await screen.findByText("同步后直接显示的有界摘要")).toBeTruthy();
    expect(desktop.fixtures.sentMessageSource).not.toHaveBeenCalled();
  });

  it("uses recipients as the Sent list correspondent while preserving the real sender in the reader", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const sent = {
      ...summary("sent-recipient-groups", "Sent recipient details"),
      uid: undefined,
      sender: { name: "Mine Mail Owner", email: "me@163.com" },
      to: [{ name: "Primary Friend", email: "to@example.com" }],
      cc: [{ name: "Copy Friend", email: "cc@example.com" }],
      bcc: [{ name: "Hidden Friend", email: "bcc@example.com" }],
      body_text: "Sent body",
      body_fetched: true,
      flags: ["\\Seen"],
    };
    desktop.fixtures.inboxPageSource.mockResolvedValue([]);
    desktop.fixtures.sentPageSource.mockResolvedValue([sent]);
    desktop.fixtures.sentMessageSource.mockResolvedValue(sent);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /已发送/ }));
    const sentList = screen.getByLabelText("已发送邮件列表");
    expect(
      within(sentList).getByText(
        "Primary Friend, Copy Friend, Hidden Friend",
      ),
    ).toBeTruthy();
    expect(within(sentList).queryByText("Mine Mail Owner")).toBeNull();
    await user.click(within(sentList).getByText("Sent recipient details"));

    const reader = screen.getByLabelText("邮件阅读区");
    expect(
      within(reader).getByText("Mine Mail Owner", {
        selector: ".sender-card__identity strong",
      }),
    ).toBeTruthy();
    await user.click(
      within(reader).getByRole("button", { name: "查看收件人" }),
    );
    const details = within(reader).getByRole("region", {
      name: "收件人详情",
    });
    expect(within(details).getByText("me@163.com")).toBeTruthy();
    expect(within(details).getByText("to@example.com")).toBeTruthy();
    expect(within(details).getByText("cc@example.com")).toBeTruthy();
    expect(within(details).getByText("bcc@example.com")).toBeTruthy();
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
    });
  });

  it("refreshes persisted mailbox batches without blanking the list", async () => {
    desktop.fixtures.inboxPageSource.mockResolvedValue([]);
    render(<App />);
    await waitFor(() =>
      expect(desktop.listeners.has("mail:inbox-updated")).toBe(true),
    );
    expect(screen.queryByText("没有找到邮件")).toBeNull();
    const contactsCallsBeforeProgress =
      desktop.mailApi.listContacts.mock.calls.length;
    desktop.fixtures.inboxPageSource.mockResolvedValue(
      Array.from({ length: 10 }, (_, index) =>
        summary(index + 1, `Progress mail ${index + 1}`),
      ),
    );
    desktop.mailApi.listStarredMailboxPage.mockImplementation(
      async (accountId, role, cursor, pageSize, query) => {
        const page = await desktop.mailApi.listMailboxPage(
          accountId,
          role,
          cursor,
          pageSize,
          query,
        );
        return {
          ...page,
          items: page.items.filter((item) =>
            (item.flags || []).some(
              (flag) => flag.toLocaleLowerCase() === "\\flagged",
            ),
          ),
        };
      },
    );
    desktop.mailApi.loadOlderStarredMailboxPage.mockImplementation(
      async (accountId, role, cursor, pageSize, query) =>
        desktop.mailApi.listStarredMailboxPage(
          accountId,
          role,
          cursor,
          pageSize,
          query,
        ),
    );

    await act(async () => {
      desktop.listeners.get("mail:inbox-updated")?.({
        payload: {
          account_id: "desktop-account",
          completed: 10,
          total: 100,
          is_complete: false,
        },
      });
    });

    expect(await screen.findByText("Progress mail 10")).toBeTruthy();
    await waitFor(() =>
      expect(desktop.mailApi.listContacts.mock.calls.length).toBeGreaterThan(
        contactsCallsBeforeProgress,
      ),
    );

    await act(async () => {
      desktop.listeners.get("mail:inbox-updated")?.({
        payload: {
          account_id: "desktop-account",
          completed: 100,
          total: 100,
          is_complete: true,
        },
      });
    });
    expect(screen.getByText("Progress mail 10")).toBeTruthy();

    desktop.mailApi.listDrafts.mockResolvedValue([
      draftSnapshot(1, "Progress draft"),
    ]);
    const draftsCallsBeforeProgress =
      desktop.mailApi.listDrafts.mock.calls.length;
    await act(async () => {
      desktop.listeners.get("mail:drafts-updated")?.({
        payload: {
          account_id: "desktop-account",
          completed: 10,
          total: 20,
          is_complete: false,
        },
      });
    });
    await waitFor(() =>
      expect(desktop.mailApi.listDrafts.mock.calls.length).toBeGreaterThan(
        draftsCallsBeforeProgress,
      ),
    );
    await userEvent.click(screen.getByRole("button", { name: /草稿/ }));
    expect(await screen.findByText("Progress draft")).toBeTruthy();
    expect(
      screen.queryByText("正在同步草稿，已加载 10/20 封"),
    ).toBeNull();
  });

  it("keeps scheduled synchronization failures silent but reports an explicit tray refresh", async () => {
    render(<App />);
    await waitFor(() =>
      expect(desktop.listeners.has("mail:sync-error")).toBe(true),
    );

    act(() => {
      desktop.listeners.get("mail:sync-error")?.({
        payload: {
          trigger: "schedule",
          operation: "inbox",
          message: "后台邮箱校准未完成，将在稍后自动重试。",
        },
      });
    });
    expect(
      screen.queryByText("后台邮箱校准未完成，将在稍后自动重试。"),
    ).toBeNull();

    act(() => {
      desktop.listeners.get("mail:sync-error")?.({
        payload: {
          trigger: "tray",
          operation: "all",
          message: "部分账户同步失败，请检查网络或账户凭证。",
        },
      });
    });
    expect(
      await screen.findByText("部分账户同步失败，请检查网络或账户凭证。"),
    ).toBeTruthy();
  });

  it("reports a native external-link launch failure without exposing the URL", async () => {
    render(<App />);
    await waitFor(() =>
      expect(
        desktop.listeners.has("mail:external-link-open-failed"),
      ).toBe(true),
    );

    act(() => {
      desktop.listeners.get("mail:external-link-open-failed")?.({
        payload: {
          url: "https://private.example.test/message?token=secret",
        },
      });
    });

    const message = screen.getByText(
      "无法打开邮件中的链接，请检查系统默认浏览器设置后重试",
    );
    const alert = message.closest('[role="alert"]');
    expect(alert).not.toBeNull();
    expect(alert.textContent).not.toContain("private.example.test");
    expect(alert.textContent).not.toContain("secret");
  });

  it("removes the repair notice when OAuth refresh restores the account backend", async () => {
    const degradedStatus = {
      configured: true,
      accountId: "desktop-account",
      activeAccountId: "desktop-account",
      provider: "gmail",
      email: "me@gmail.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: false,
      startupError: null,
      accounts: [
        {
          accountId: "desktop-account",
          provider: "gmail",
          email: "me@gmail.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: false,
        },
      ],
    };
    desktop.mailApi.getAccountStatus.mockResolvedValue(degradedStatus);

    render(<App />);

    expect(
      await screen.findByText("账户暂时离线", {}, { timeout: 1600 }),
    ).toBeTruthy();
    await waitFor(() =>
      expect(desktop.listeners.has("mail:account-updated")).toBe(true),
    );

    await act(async () => {
      desktop.listeners.get("mail:account-updated")?.({
        payload: {
          ...degradedStatus,
          networkReady: true,
          accounts: [
            {
              ...degradedStatus.accounts[0],
              networkReady: true,
            },
          ],
        },
      });
    });

    await waitFor(() =>
      expect(screen.queryByText("账户暂时离线")).toBeNull(),
    );
    expect(
      screen.getByRole("button", { name: "同步收件箱" }).disabled,
    ).toBe(false);
  });

  it("opens the exact locally synced message selected from a desktop notification", async () => {
    render(<App />);
    await waitFor(() =>
      expect(desktop.listeners.has("mail:open-message")).toBe(true),
    );

    await act(async () => {
      desktop.listeners.get("mail:open-message")?.({
        payload: { message_id: "1", account_id: "desktop-account" },
      });
    });

    await waitFor(() =>
      expect(desktop.mailApi.fetchMailboxMessage).toHaveBeenCalledWith("1"),
    );
    expect(await screen.findByText("Loaded body")).toBeTruthy();
  });

  it("ignores numeric row ids from desktop open-message events", async () => {
    render(<App />);
    await waitFor(() =>
      expect(desktop.listeners.has("mail:open-message")).toBe(true),
    );
    desktop.mailApi.fetchMailboxMessage.mockClear();

    await act(async () => {
      desktop.listeners.get("mail:open-message")?.({
        payload: { message_id: 1, account_id: "desktop-account" },
      });
      await Promise.resolve();
    });

    expect(desktop.mailApi.fetchMailboxMessage).not.toHaveBeenCalled();
  });

  it("opens a notification target directly even when it is moved outside the Inbox page", async () => {
    const moved = {
      ...summary("moved-public-id", "Moved notification target"),
      uid: undefined,
      displayed_role: "archive",
      body_text: "Body loaded directly by public id",
      body_fetched: true,
      flags: ["\\Seen"],
    };
    desktop.mailApi.fetchMailboxMessage.mockImplementation(async (messageId) =>
      messageId === "moved-public-id"
        ? moved
        : desktop.fixtures.inboxMessageSource(Number(messageId)),
    );
    render(<App />);
    await waitFor(() =>
      expect(desktop.listeners.has("mail:open-message")).toBe(true),
    );

    await act(async () => {
      desktop.listeners.get("mail:open-message")?.({
        payload: {
          message_id: "moved-public-id",
          account_id: "desktop-account",
        },
      });
    });

    expect(
      await screen.findByRole("heading", {
        name: "Moved notification target",
      }),
    ).toBeTruthy();
    expect(screen.getByText("Body loaded directly by public id")).toBeTruthy();
    expect(desktop.mailApi.fetchMailboxMessage).toHaveBeenCalledWith(
      "moved-public-id",
    );
  });

  it("keeps only the latest desktop open-message intent when direct fetches resolve out of order", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const firstFetch = deferred();
    desktop.mailApi.fetchMailboxMessage.mockImplementation(async (messageId) => {
      if (messageId === "event-first") return firstFetch.promise;
      if (messageId === "event-second") {
        return {
          ...summary("event-second", "Latest notification target"),
          uid: undefined,
          body_text: "Latest notification body",
          body_fetched: true,
          flags: ["\\Seen"],
        };
      }
      return desktop.fixtures.inboxMessageSource(Number(messageId));
    });
    render(<App />);
    await waitFor(() =>
      expect(desktop.listeners.has("mail:open-message")).toBe(true),
    );

    await act(async () => {
      desktop.listeners.get("mail:open-message")?.({
        payload: {
          message_id: "event-first",
          account_id: "desktop-account",
        },
      });
    });
    await waitFor(() =>
      expect(desktop.mailApi.fetchMailboxMessage).toHaveBeenCalledWith(
        "event-first",
      ),
    );
    await act(async () => {
      desktop.listeners.get("mail:open-message")?.({
        payload: {
          message_id: "event-second",
          account_id: "desktop-account",
        },
      });
    });
    expect(await screen.findByText("Latest notification body")).toBeTruthy();

    await act(async () => {
      firstFetch.resolve({
        ...summary("event-first", "Stale notification target"),
        uid: undefined,
        body_text: "Stale notification body",
        body_fetched: true,
        flags: ["\\Seen"],
      });
    });
    expect(screen.queryByText("Stale notification body")).toBeNull();
    expect(screen.getByText("Latest notification body")).toBeTruthy();
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
    });
  });

  it("does not let a delayed desktop open-message fetch replace manual navigation", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const eventFetch = deferred();
    desktop.mailApi.fetchMailboxMessage.mockImplementation(async (messageId) =>
      messageId === "delayed-event"
        ? eventFetch.promise
        : desktop.fixtures.inboxMessageSource(Number(messageId)),
    );
    const user = userEvent.setup();
    render(<App />);
    await waitFor(() =>
      expect(desktop.listeners.has("mail:open-message")).toBe(true),
    );

    await act(async () => {
      desktop.listeners.get("mail:open-message")?.({
        payload: {
          message_id: "delayed-event",
          account_id: "desktop-account",
        },
      });
    });
    await waitFor(() =>
      expect(desktop.mailApi.fetchMailboxMessage).toHaveBeenCalledWith(
        "delayed-event",
      ),
    );
    await user.click(
      await screen.findByRole("button", {
        name: /打开邮件：.*First mail/,
      }),
    );
    expect(await screen.findByText("Loaded body")).toBeTruthy();

    await act(async () => {
      eventFetch.resolve({
        ...summary("delayed-event", "Delayed event"),
        uid: undefined,
        body_text: "Must not replace manual selection",
        body_fetched: true,
        flags: ["\\Seen"],
      });
    });
    expect(screen.queryByText("Must not replace manual selection")).toBeNull();
    expect(screen.getByText("Loaded body")).toBeTruthy();
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
    });
  });

  it("keeps independent compose sessions while switching accounts from a notification", async () => {
    const status = {
      configured: true,
      accountId: "account-a",
      activeAccountId: "account-a",
      provider: "163",
      email: "a@example.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
      startupError: null,
      accounts: [
        {
          accountId: "account-a",
          provider: "163",
          email: "a@example.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
        {
          accountId: "account-b",
          provider: "gmail",
          email: "b@example.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
      ],
    };
    let testActiveAccountId = "account-a";
    const draftsByAccount = new Map([
      ["account-a", []],
      ["account-b", []],
    ]);
    const statusFor = (accountId) => ({
      ...status,
      ...status.accounts.find((account) => account.accountId === accountId),
      accountId,
      activeAccountId: accountId,
    });
    desktop.mailApi.getAccountStatus.mockImplementation(async () =>
      statusFor(testActiveAccountId),
    );
    desktop.mailApi.switchAccount.mockImplementation(async (accountId) => {
      testActiveAccountId = accountId;
      return statusFor(accountId);
    });
    desktop.mailApi.listDrafts.mockImplementation(
      async () => draftsByAccount.get(testActiveAccountId) || [],
    );
    let createdDrafts = 0;
    desktop.mailApi.saveDraft.mockImplementation(
      async (request, draftId, expectedLocalVersion) => {
        const outcome = savedOutcome(
          request,
          draftId || `account-compose-${(createdDrafts += 1)}`,
          expectedLocalVersion,
        );
        draftsByAccount.set(testActiveAccountId, [outcome.draft]);
        return outcome;
      },
    );
    desktop.mailApi.fetchMailboxMessage.mockImplementation(async (messageId) =>
      messageId === "account-b-notification"
        ? {
            ...summary(
              "account-b-notification",
              "Retried cross-account notification",
            ),
            uid: undefined,
            body_text: "Retried notification body",
            body_fetched: true,
            flags: ["\\Seen"],
          }
        : desktop.fixtures.inboxMessageSource(Number(messageId)),
    );
    const user = userEvent.setup();
    render(<App />);
    await waitFor(() =>
      expect(desktop.listeners.has("mail:open-message")).toBe(true),
    );
    await screen.findByRole("button", { name: "当前账户 a@example.com" });
    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.type(screen.getByLabelText("主题"), "A 账户正在编辑");

    await act(async () => {
      desktop.listeners.get("mail:open-message")?.({
        payload: {
          message_id: "account-b-notification",
          account_id: "account-b",
        },
      });
    });
    expect(await screen.findByText("Retried notification body")).toBeTruthy();
    expect(
      screen.queryByText("请先关闭当前写信窗口，再打开其他账户的新邮件"),
    ).toBeNull();
    expect(desktop.mailApi.switchAccount).toHaveBeenCalledOnce();
    expect(desktop.mailApi.switchAccount).toHaveBeenCalledWith("account-b");
    expect(desktop.mailApi.fetchMailboxMessage).toHaveBeenCalledWith(
      "account-b-notification",
    );
    expect(desktop.mailApi.saveDraft).toHaveBeenCalledOnce();
    expect(desktop.mailApi.saveDraft.mock.calls[0][0].subject).toBe(
      "A 账户正在编辑",
    );
    expect(screen.queryByRole("dialog", { name: /A 账户正在编辑/ })).toBeNull();

    await user.click(screen.getByRole("button", { name: /写信/ }));
    expect(screen.getByLabelText("主题").value).toBe("");
    await user.type(screen.getByLabelText("主题"), "B 账户独立邮件");
    minimizeComposer();
    await user.click(
      screen.getByRole("button", { name: "切换到 a@example.com" }),
    );
    await screen.findByRole("button", { name: "当前账户 a@example.com" });

    const restoreA = await screen.findByRole("button", {
      name: "还原写信窗口：A 账户正在编辑",
    });
    await user.click(restoreA);
    expect(screen.getByLabelText("主题").value).toBe("A 账户正在编辑");

    minimizeComposer();
    await user.click(
      screen.getByRole("button", { name: "切换到 b@example.com" }),
    );
    await screen.findByRole("button", { name: "当前账户 b@example.com" });
    const restoreB = await screen.findByRole("button", {
      name: "还原写信窗口：B 账户独立邮件",
    });
    await user.click(restoreB);
    expect(screen.getByLabelText("主题").value).toBe("B 账户独立邮件");
    expect(desktop.mailApi.saveDraft).toHaveBeenCalledTimes(2);
    expect(desktop.mailApi.saveDraft.mock.calls[1][0].subject).toBe(
      "B 账户独立邮件",
    );
  });

  it("keeps the current compose session when connecting another Gmail account", async () => {
    const initialStatus = {
      configured: true,
      accountId: "desktop-account",
      activeAccountId: "desktop-account",
      provider: "163",
      email: "me@163.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
      startupError: null,
      accounts: [
        {
          accountId: "desktop-account",
          provider: "163",
          email: "me@163.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
      ],
    };
    const connectedStatus = {
      configured: true,
      accountId: "gmail-account",
      activeAccountId: "gmail-account",
      provider: "gmail",
      email: "second@gmail.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
      startupError: null,
      accounts: [
        {
          accountId: "desktop-account",
          provider: "163",
          email: "me@163.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
        {
          accountId: "gmail-account",
          provider: "gmail",
          email: "second@gmail.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
      ],
    };
    let currentAccountId = "desktop-account";
    let isConnected = false;
    const draftsByAccount = new Map([
      ["desktop-account", []],
      ["gmail-account", []],
    ]);
    const statusFor = (accountId) => ({
      ...connectedStatus,
      ...connectedStatus.accounts.find(
        (account) => account.accountId === accountId,
      ),
      accountId,
      activeAccountId: accountId,
    });
    desktop.mailApi.getAccountStatus.mockImplementation(async () =>
      isConnected ? statusFor(currentAccountId) : initialStatus,
    );
    desktop.mailApi.connectGoogleAccount.mockImplementation(async () => {
      isConnected = true;
      currentAccountId = "gmail-account";
      return statusFor(currentAccountId);
    });
    desktop.mailApi.switchAccount.mockImplementation(async (accountId) => {
      currentAccountId = accountId;
      return statusFor(accountId);
    });
    desktop.mailApi.listDrafts.mockImplementation(
      async () => draftsByAccount.get(currentAccountId) || [],
    );
    desktop.mailApi.saveDraft.mockImplementation(
      async (request, draftId, expectedLocalVersion) => {
        const outcome = savedOutcome(request, draftId, expectedLocalVersion);
        draftsByAccount.set(currentAccountId, [outcome.draft]);
        return outcome;
      },
    );
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("button", { name: "当前账户 me@163.com" });
    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.type(screen.getByLabelText("主题"), "登录前正在编辑");
    await user.click(
      screen.getByRole("button", { name: /添加邮箱账户/ }),
    );
    await user.click(
      await screen.findByRole("button", { name: /Gmail/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "使用 Google 登录" }),
    );

    await screen.findByRole("button", {
      name: "当前账户 second@gmail.com",
    });
    expect(desktop.mailApi.saveDraft).toHaveBeenCalledOnce();
    expect(desktop.mailApi.saveDraft.mock.calls[0][0].subject).toBe(
      "登录前正在编辑",
    );
    expect(
      desktop.mailApi.saveDraft.mock.invocationCallOrder[0],
    ).toBeLessThan(
      desktop.mailApi.connectGoogleAccount.mock.invocationCallOrder[0],
    );
    expect(
      screen.queryByText("请先关闭当前写信窗口，再连接其他账户。"),
    ).toBeNull();
    expect(screen.queryByRole("dialog", { name: /登录前正在编辑/ })).toBeNull();

    const accountSwitcher = screen.getByLabelText("已登录邮箱账户");
    await user.click(
      within(accountSwitcher).getByRole("button", {
        name: "切换到 me@163.com",
      }),
    );
    await screen.findByRole("button", { name: "当前账户 me@163.com" });
    const restore = await screen.findByRole("button", {
      name: "还原写信窗口：登录前正在编辑",
    });
    await user.click(restore);
    expect(screen.getByLabelText("主题").value).toBe("登录前正在编辑");
  });

  it("stabilizes the current compose session before connecting a password account", async () => {
    desktop.mailApi.configureAccount.mockResolvedValue({
      configured: true,
      accountId: "qq-account",
      activeAccountId: "qq-account",
      provider: "qq",
      email: "second@qq.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
      startupError: null,
      accounts: [
        {
          accountId: "desktop-account",
          provider: "163",
          email: "me@163.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
        {
          accountId: "qq-account",
          provider: "qq",
          email: "second@qq.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
      ],
    });
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("button", { name: "当前账户 me@163.com" });
    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.type(screen.getByLabelText("主题"), "连接授权码账户前的草稿");
    await user.click(
      screen.getByRole("button", { name: /添加邮箱账户/ }),
    );
    await user.click(
      await screen.findByRole("button", { name: /QQ 邮箱/ }),
    );
    await user.type(screen.getByLabelText("邮箱地址"), "second@qq.com");
    await user.type(screen.getByLabelText("QQ 邮箱授权码"), "app-secret");
    await user.click(screen.getByRole("button", { name: "连接邮箱" }));

    await screen.findByRole("button", { name: "当前账户 second@qq.com" });
    expect(desktop.mailApi.configureAccount).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: "qq",
        email: "second@qq.com",
        secret: "app-secret",
      }),
    );
    expect(desktop.mailApi.saveDraft.mock.calls[0][0].subject).toBe(
      "连接授权码账户前的草稿",
    );
    expect(
      desktop.mailApi.saveDraft.mock.invocationCallOrder[0],
    ).toBeLessThan(desktop.mailApi.configureAccount.mock.invocationCallOrder[0]);
    expect(
      screen.queryByText("请先关闭当前写信窗口，再连接其他账户。"),
    ).toBeNull();
    expect(
      screen.queryByRole("button", {
        name: "还原写信窗口：连接授权码账户前的草稿",
      }),
    ).toBeNull();
  });

  it("does not start account login when the current compose session cannot be saved", async () => {
    desktop.mailApi.saveDraft.mockRejectedValueOnce(
      new Error("本地草稿写入失败"),
    );
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("button", { name: "当前账户 me@163.com" });
    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.type(screen.getByLabelText("主题"), "不能丢失的登录前草稿");
    await user.click(
      screen.getByRole("button", { name: /添加邮箱账户/ }),
    );
    await user.click(
      await screen.findByRole("button", { name: /Gmail/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "使用 Google 登录" }),
    );

    expect(await screen.findByText("本地草稿写入失败")).toBeTruthy();
    expect(desktop.mailApi.connectGoogleAccount).not.toHaveBeenCalled();
    expect(screen.getByLabelText("主题").value).toBe(
      "不能丢失的登录前草稿",
    );
  });

  it("keeps the current account and editor when a switch-time draft save fails", async () => {
    const status = {
      configured: true,
      accountId: "account-a",
      activeAccountId: "account-a",
      provider: "163",
      email: "a@example.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
      startupError: null,
      accounts: [
        {
          accountId: "account-a",
          provider: "163",
          email: "a@example.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
        {
          accountId: "account-b",
          provider: "gmail",
          email: "b@example.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
      ],
    };
    desktop.mailApi.getAccountStatus.mockResolvedValue(status);
    desktop.mailApi.saveDraft.mockRejectedValueOnce(
      new Error("本地草稿写入失败"),
    );
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("button", { name: "当前账户 a@example.com" });
    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.type(screen.getByLabelText("主题"), "不能丢失的内容");
    minimizeComposer();
    await user.click(
      screen.getByRole("button", { name: "切换到 b@example.com" }),
    );

    expect(await screen.findByText("本地草稿写入失败")).toBeTruthy();
    expect(desktop.mailApi.switchAccount).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "当前账户 a@example.com" }),
    ).toBeTruthy();
    await user.click(
      screen.getByRole("button", {
        name: "还原写信窗口：不能丢失的内容",
      }),
    );
    expect(screen.getByLabelText("主题").value).toBe("不能丢失的内容");
  });

  it("marks an unread Inbox message read immediately and requests IMAP persistence", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const unread = summary(14, "Unread mail");
    desktop.fixtures.inboxPageSource.mockResolvedValue([unread]);
    desktop.fixtures.inboxMessageSource.mockResolvedValue({
      ...unread,
      body_text: "Unread body",
      body_fetched: true,
    });
    const user = userEvent.setup();
    render(<App />);

    const subject = await screen.findByText("Unread mail");
    const row = subject.closest(".mail-row");
    expect(row?.dataset.unread).toBe("true");
    await user.click(subject);

    expect(row?.dataset.unread).toBe("false");
    await waitFor(() =>
      expect(desktop.fixtures.recordInboxSeen).toHaveBeenCalledWith(14),
    );
    expect(
      (await screen.findByText("Unread body")).closest(".reader-panel"),
    ).toBeTruthy();
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
    });
  });

  it("marks an opened message unread without reusing the pending mark-read state", async () => {
    const unread = summary(16, "Mark unread again");
    desktop.fixtures.inboxPageSource.mockResolvedValue([unread]);
    desktop.fixtures.inboxMessageSource.mockResolvedValue({
      ...unread,
      body_text: "Unread toggle body",
      body_fetched: true,
    });
    const user = userEvent.setup();
    render(<App />);

    const subject = (await screen.findAllByText("Mark unread again")).find(
      (candidate) => candidate.closest(".mail-row"),
    );
    const row = subject?.closest(".mail-row");
    expect(row).toBeTruthy();
    await user.click(row);
    await waitFor(() =>
      expect(desktop.mailApi.setMessageSeen).toHaveBeenCalledWith("16", true),
    );

    const markUnread = await screen.findByRole("button", {
      name: "标记为未读",
    });
    expect(markUnread.disabled).toBe(false);
    expect(
      screen.queryByText(/标记为未读.*正在处理|正在标记为未读/),
    ).toBeNull();

    await user.click(markUnread);
    await waitFor(() =>
      expect(desktop.mailApi.setMessageSeen).toHaveBeenCalledWith("16", false),
    );
    expect(row?.dataset.unread).toBe("true");
  });

  it("toggles an Inbox star without opening the message and persists both states", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const message = { ...summary(15, "Star this mail"), mailbox: "INBOX" };
    desktop.fixtures.inboxPageSource.mockResolvedValue([message]);
    const user = userEvent.setup();
    render(<App />);

    const addStar = await screen.findByRole("button", {
      name: "添加收藏：Star this mail",
    });
    await user.click(addStar);

    await waitFor(() =>
      expect(desktop.fixtures.recordStarred).toHaveBeenCalledWith(
        "INBOX",
        15,
        true,
      ),
    );
    expect(
      screen
        .getByRole("button", { name: "取消收藏：Star this mail" })
        .getAttribute("aria-pressed"),
    ).toBe("true");
    expect(desktop.fixtures.inboxMessageSource).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("button", { name: "取消收藏：Star this mail" }),
    );
    await waitFor(() =>
      expect(desktop.fixtures.recordStarred).toHaveBeenLastCalledWith(
        "INBOX",
        15,
        false,
      ),
    );
    expect(
      screen
        .getByRole("button", { name: "添加收藏：Star this mail" })
        .getAttribute("aria-pressed"),
    ).toBe("false");
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
    });
  });

  it("keeps unstarred rows until Starred is refreshed or revisited", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const starredInbox = {
      ...summary(21, "Starred Inbox"),
      mailbox: "INBOX",
      flags: ["\\Flagged"],
    };
    const starredSent = {
      ...summary(22, "Starred Sent"),
      mailbox: "Sent",
      to: [{ name: "Friend", email: "friend@example.com" }],
      flags: ["\\Seen", "\\Flagged"],
    };
    desktop.fixtures.inboxPageSource.mockResolvedValue([starredInbox]);
    desktop.fixtures.sentPageSource.mockResolvedValue([starredSent]);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /已收藏/ }));
    expect(await screen.findByText("Starred Inbox")).toBeTruthy();
    expect(await screen.findByText("Starred Sent")).toBeTruthy();
    await waitFor(() =>
      expect(desktop.mailApi.listStarredMailboxPage).toHaveBeenCalledTimes(3),
    );
    expect(
      desktop.mailApi.listStarredMailboxPage.mock.calls.map(([, role]) => role),
    ).toEqual(expect.arrayContaining(["inbox", "sent", "archive"]));
    expect(desktop.mailApi.loadOlderMailboxPage).not.toHaveBeenCalled();
    const starredSubjectOrder = () =>
      Array.from(
        screen
          .getByLabelText("已收藏邮件列表")
          .querySelectorAll(".mail-row__subject"),
        (subject) => subject.textContent,
      );
    expect(starredSubjectOrder()).toEqual([
      "Starred Inbox",
      "Starred Sent",
    ]);

    await user.click(
      screen.getByRole("button", { name: "取消收藏：Starred Inbox" }),
    );
    await waitFor(() =>
      expect(desktop.fixtures.recordStarred).toHaveBeenCalledWith(
        "INBOX",
        21,
        false,
      ),
    );
    expect(screen.getByText("Starred Inbox")).toBeTruthy();
    expect(screen.getByText("Starred Sent")).toBeTruthy();
    expect(
      screen
        .getByRole("button", { name: "添加收藏：Starred Inbox" })
        .getAttribute("aria-pressed"),
    ).toBe("false");
    await user.unhover(
      screen.getByRole("button", { name: "添加收藏：Starred Inbox" }),
    );
    expect(starredSubjectOrder()).toEqual([
      "Starred Inbox",
      "Starred Sent",
    ]);

    await user.click(
      screen.getByRole("button", { name: "添加收藏：Starred Inbox" }),
    );
    await waitFor(() =>
      expect(desktop.fixtures.recordStarred).toHaveBeenLastCalledWith(
        "INBOX",
        21,
        true,
      ),
    );
    await user.unhover(
      screen.getByRole("button", { name: "取消收藏：Starred Inbox" }),
    );
    expect(starredSubjectOrder()).toEqual([
      "Starred Inbox",
      "Starred Sent",
    ]);
    await user.click(
      screen.getByRole("button", { name: "取消收藏：Starred Inbox" }),
    );
    await waitFor(() =>
      expect(desktop.fixtures.recordStarred).toHaveBeenLastCalledWith(
        "INBOX",
        21,
        false,
      ),
    );
    expect(starredSubjectOrder()).toEqual([
      "Starred Inbox",
      "Starred Sent",
    ]);

    desktop.fixtures.inboxPageSource.mockResolvedValue([
      { ...starredInbox, flags: [] },
    ]);
    const inboxPageCallsBeforeBackgroundRefresh =
      desktop.fixtures.inboxPageSource.mock.calls.length;
    await act(async () => {
      desktop.listeners.get("mail:inbox-updated")?.({
        payload: {
          account_id: "desktop-account",
          completed: 1,
          total: 1,
          is_complete: true,
        },
      });
    });
    await waitFor(() =>
      expect(
        desktop.fixtures.inboxPageSource.mock.calls.length,
      ).toBeGreaterThan(inboxPageCallsBeforeBackgroundRefresh),
    );
    expect(starredSubjectOrder()).toEqual([
      "Starred Inbox",
      "Starred Sent",
    ]);

    await user.click(screen.getByRole("button", { name: "已发送" }));
    await user.click(screen.getByRole("button", { name: /已收藏/ }));
    await waitFor(() =>
      expect(screen.queryByText("Starred Inbox")).toBeNull(),
    );
    expect(screen.getByText("Starred Sent")).toBeTruthy();

    await user.click(
      screen.getByRole("button", { name: "取消收藏：Starred Sent" }),
    );
    expect(screen.getByText("Starred Sent")).toBeTruthy();
    desktop.fixtures.sentPageSource.mockResolvedValue([
      { ...starredSent, flags: ["\\Seen"] },
    ]);
    await user.click(
      screen.getByRole("button", { name: "同步已收藏邮件" }),
    );
    await waitFor(() =>
      expect(screen.queryByText("Starred Sent")).toBeNull(),
    );
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
    });
  });

  it("opens an existing Archive directly and shows its cached server mail", async () => {
    const archived = summary("archived-existing", "Already archived mail");
    desktop.mailApi.listMailboxPage.mockImplementation(async (_, role) =>
      mailboxPage(role === "archive" ? [archived] : [], role),
    );
    const user = userEvent.setup();
    render(<App />);

    const sidebar = await screen.findByRole("complementary", {
      name: "邮箱导航",
    });
    const archive = within(sidebar).getByRole("button", { name: "归档" });
    await user.click(archive);

    expect(
      await screen.findByRole("heading", { name: "归档" }),
    ).toBeTruthy();
    expect(await screen.findByText("Already archived mail")).toBeTruthy();
    expect(desktop.mailApi.createMailboxRole).not.toHaveBeenCalled();
    expect(
      desktop.mailApi.listMailboxPage.mock.calls.some(
        ([accountId, role]) =>
          accountId === "desktop-account" && role === "archive",
      ),
    ).toBe(true);
  });

  it("resolves pending Archive and Trash roles from explicit navigation", async () => {
    const archiveEnsure = deferred();
    desktop.mailApi.getMailboxCapabilities.mockResolvedValue([
      { role: "inbox", status: "available", retryable: false },
      { role: "sent", status: "available", retryable: false },
      {
        role: "archive",
        status: "discovery_pending",
        retryable: true,
      },
      {
        role: "trash",
        status: "discovery_pending",
        retryable: true,
      },
    ]);
    desktop.mailApi.createMailboxRole.mockImplementation(async (_, role) => {
      if (role === "archive") return archiveEnsure.promise;
      return { role, status: "available", retryable: false };
    });
    const user = userEvent.setup();
    render(<App />);

    const sidebar = await screen.findByRole("complementary", {
      name: "邮箱导航",
    });
    const archive = within(sidebar).getByRole("button", { name: "归档" });
    await user.click(archive);

    expect(
      await screen.findByRole("heading", { name: "归档" }),
    ).toBeTruthy();
    await user.click(archive);
    expect(desktop.mailApi.createMailboxRole).toHaveBeenCalledTimes(1);

    archiveEnsure.resolve({
      role: "archive",
      status: "available",
      retryable: false,
    });
    await waitFor(() =>
      expect(
        desktop.mailApi.listMailboxPage.mock.calls.some(
          ([accountId, role]) =>
            accountId === "desktop-account" && role === "archive",
        ),
      ).toBe(true),
    );

    await user.click(
      within(sidebar).getByRole("button", { name: "垃圾箱" }),
    );
    await waitFor(() =>
      expect(desktop.mailApi.createMailboxRole).toHaveBeenCalledWith(
        "desktop-account",
        "trash",
      ),
    );
    expect(
      await screen.findByRole("heading", { name: "垃圾箱" }),
    ).toBeTruthy();
  });

  it("creates a missing Archive directly when its workspace is opened", async () => {
    desktop.mailApi.getMailboxCapabilities.mockResolvedValue([
      { role: "inbox", status: "available", retryable: false },
      { role: "sent", status: "available", retryable: false },
      {
        role: "archive",
        status: "needs_creation_confirmation",
        retryable: false,
      },
      { role: "trash", status: "available", retryable: false },
    ]);
    const user = userEvent.setup();
    render(<App />);

    const sidebar = await screen.findByRole("complementary", {
      name: "邮箱导航",
    });
    const archive = within(sidebar).getByRole("button", { name: "归档" });
    expect(archive.dataset.capabilityStatus).toBeUndefined();
    expect(archive.textContent).not.toContain("需设置");
    expect(
      desktop.mailApi.listMailboxPage.mock.calls.some(
        ([, role]) => role === "archive",
      ),
    ).toBe(false);

    await user.click(archive);
    await waitFor(() =>
      expect(desktop.mailApi.createMailboxRole).toHaveBeenCalledWith(
        "desktop-account",
        "archive",
      ),
    );
    expect(
      await screen.findByRole("heading", { name: "归档" }),
    ).toBeTruthy();
    expect(screen.queryByText("尚未设置归档文件夹")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "设置归档文件夹" }),
    ).toBeNull();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("creates a missing Archive without a dialog and continues the message action", async () => {
    desktop.mailApi.getMailboxCapabilities.mockResolvedValue([
      { role: "inbox", status: "available", retryable: false },
      { role: "sent", status: "available", retryable: false },
      {
        role: "archive",
        status: "needs_creation_confirmation",
        retryable: true,
      },
      { role: "trash", status: "available", retryable: false },
    ]);
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole("button", { name: /打开邮件：.*First mail/ }),
    );
    const reader = screen.getByLabelText("邮件阅读区");
    const archive = within(reader).getByRole("button", { name: "归档" });
    await user.click(archive);

    await waitFor(() =>
      expect(desktop.mailApi.createMailboxRole).toHaveBeenCalledWith(
        "desktop-account",
        "archive",
      ),
    );
    await waitFor(() =>
      expect(desktop.mailApi.archiveMessage).toHaveBeenCalledWith("1"),
    );
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("resolves a pending Archive role and continues the triggering message action", async () => {
    desktop.mailApi.getMailboxCapabilities.mockResolvedValue([
      { role: "inbox", status: "available", retryable: false },
      { role: "sent", status: "available", retryable: false },
      {
        role: "archive",
        status: "discovery_pending",
        retryable: true,
      },
      { role: "trash", status: "available", retryable: false },
    ]);
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole("button", { name: /打开邮件：.*First mail/ }),
    );
    const reader = screen.getByLabelText("邮件阅读区");
    await user.click(within(reader).getByRole("button", { name: "归档" }));

    await waitFor(() =>
      expect(desktop.mailApi.createMailboxRole).toHaveBeenCalledWith(
        "desktop-account",
        "archive",
      ),
    );
    await waitFor(() =>
      expect(desktop.mailApi.archiveMessage).toHaveBeenCalledWith("1"),
    );
    expect(screen.queryByText("正在确认归档文件夹，请稍后重试")).toBeNull();
  });

  it("keeps the message intact when a pending Archive role cannot be ensured", async () => {
    desktop.mailApi.getMailboxCapabilities.mockResolvedValue([
      { role: "inbox", status: "available", retryable: false },
      { role: "sent", status: "available", retryable: false },
      {
        role: "archive",
        status: "discovery_pending",
        retryable: true,
      },
      { role: "trash", status: "available", retryable: false },
    ]);
    desktop.mailApi.createMailboxRole.mockResolvedValue({
      role: "archive",
      status: "unavailable",
      unavailable_reason: "create_failed",
      retryable: true,
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole("button", { name: /打开邮件：.*First mail/ }),
    );
    const reader = screen.getByLabelText("邮件阅读区");
    await user.click(within(reader).getByRole("button", { name: "归档" }));

    expect(
      await within(reader).findByRole("button", { name: /重试归档/ }),
    ).toBeTruthy();
    expect(within(reader).getByText("First mail")).toBeTruthy();
    expect(desktop.mailApi.archiveMessage).not.toHaveBeenCalled();
  });

  it("creates a missing Trash directly when its workspace is opened", async () => {
    desktop.mailApi.getMailboxCapabilities.mockResolvedValue([
      { role: "inbox", status: "available", retryable: false },
      { role: "sent", status: "available", retryable: false },
      { role: "archive", status: "available", retryable: false },
      {
        role: "trash",
        status: "needs_creation_confirmation",
        retryable: true,
      },
    ]);
    const user = userEvent.setup();
    render(<App />);

    const sidebar = await screen.findByRole("complementary", {
      name: "邮箱导航",
    });
    await user.click(within(sidebar).getByRole("button", { name: "垃圾箱" }));

    await waitFor(() =>
      expect(desktop.mailApi.createMailboxRole).toHaveBeenCalledWith(
        "desktop-account",
        "trash",
      ),
    );
    expect(
      await screen.findByRole("heading", { name: "垃圾箱" }),
    ).toBeTruthy();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("creates a missing Trash before moving the triggering message", async () => {
    desktop.mailApi.getMailboxCapabilities.mockResolvedValue([
      { role: "inbox", status: "available", retryable: false },
      { role: "sent", status: "available", retryable: false },
      { role: "archive", status: "available", retryable: false },
      {
        role: "trash",
        status: "needs_creation_confirmation",
        retryable: true,
      },
    ]);
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole("button", { name: /打开邮件：.*First mail/ }),
    );
    const reader = screen.getByLabelText("邮件阅读区");
    await user.click(
      within(reader).getByRole("button", { name: "移到垃圾箱" }),
    );

    await waitFor(() =>
      expect(desktop.mailApi.createMailboxRole).toHaveBeenCalledWith(
        "desktop-account",
        "trash",
      ),
    );
    await waitFor(() =>
      expect(desktop.mailApi.moveMessageToTrash).toHaveBeenCalledWith("1"),
    );
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("appends an older mailbox page using the opaque backend cursor", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const newest = {
      ...summary("message-local-newest", "Newest local page"),
      uid: undefined,
      flags: ["\\Seen"],
    };
    const older = {
      ...summary("message-local-older", "Older local page"),
      uid: undefined,
      flags: ["\\Seen"],
    };
    const pendingOlderPage = deferred();
    desktop.mailApi.listMailboxPage.mockImplementation(
      async (_, role, _cursor, _pageSize, query) =>
        role === "inbox" && !query
          ? mailboxPage([newest], role, {
              next_cursor: "opaque-page-cursor",
              has_more_local: true,
              remote_history_state: "not_checked",
              end_reached: false,
            })
          : mailboxPage([], role),
    );
    desktop.mailApi.loadOlderMailboxPage.mockReturnValue(
      pendingOlderPage.promise,
    );
    render(<App />);

    expect(await screen.findByText("Newest local page")).toBeTruthy();
    const list = screen.getByLabelText("收件箱邮件列表");
    const scrollSurface = list.querySelector(".message-list");
    Object.defineProperties(scrollSurface, {
      scrollHeight: { configurable: true, value: 800 },
      clientHeight: { configurable: true, value: 400 },
      scrollTop: { configurable: true, value: 350 },
    });
    fireEvent.scroll(scrollSurface);

    await waitFor(() =>
      expect(desktop.mailApi.loadOlderMailboxPage).toHaveBeenCalledWith(
        "desktop-account",
        "inbox",
        "opaque-page-cursor",
        50,
        null,
      ),
    );
    expect(
      await within(list).findByRole("status", {
        name: "正在加载更多邮件",
      }),
    ).toBeTruthy();
    expect(screen.getByText("Newest local page")).toBeTruthy();
    expect(scrollSurface.scrollTop).toBe(350);

    await act(async () => {
      pendingOlderPage.resolve(mailboxPage([older], "inbox"));
    });
    expect(await screen.findByText("Older local page")).toBeTruthy();
    expect(screen.getByText("Newest local page")).toBeTruthy();
    expect(
      within(list).queryByRole("status", {
        name: "正在加载更多邮件",
      }),
    ).toBeNull();
    expect(scrollSurface.scrollTop).toBe(350);
    expect(screen.queryByText("已加载 1 封")).toBeNull();
    expect(list.querySelector(".mail-pagination-notice")).toBeNull();
    expect(
      screen.queryByRole("button", { name: /加载更早邮件/ }),
    ).toBeNull();
  });

  it("keeps cached rows visible while backend mailbox search is pending", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const pendingSearch = deferred();
    const cached = {
      ...summary("message-cached", "Cached row remains visible"),
      uid: undefined,
      flags: ["\\Seen"],
    };
    const remoteMatch = {
      ...summary("message-search-result", "Server-side only result"),
      uid: undefined,
      flags: ["\\Seen"],
    };
    desktop.mailApi.listMailboxPage.mockImplementation(
      async (_, role, _cursor, _pageSize, query) => {
        if (role === "inbox" && query === "server-side") {
          return pendingSearch.promise;
        }
        return role === "inbox"
          ? mailboxPage([cached], role)
          : mailboxPage([], role);
      },
    );
    const user = userEvent.setup();
    render(<App />);

    expect(await screen.findByText("Cached row remains visible")).toBeTruthy();
    await user.type(
      screen.getByRole("textbox", { name: "搜索邮件" }),
      "server-side",
    );
    await waitFor(() =>
      expect(desktop.mailApi.listMailboxPage).toHaveBeenCalledWith(
        "desktop-account",
        "inbox",
        null,
        50,
        "server-side",
      ),
    );
    expect(screen.getByText("Cached row remains visible")).toBeTruthy();

    await act(async () => {
      pendingSearch.resolve(mailboxPage([remoteMatch], "inbox"));
    });
    expect(await screen.findByText("Server-side only result")).toBeTruthy();
    expect(screen.queryByText("Cached row remains visible")).toBeNull();
  });

  it("uses opaque message IDs for archive and trash mutations", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const archiveTarget = {
      ...summary("message-to-archive", "Archive opaque message"),
      uid: undefined,
      flags: ["\\Seen"],
      body_text: "Archive body",
      body_fetched: true,
    };
    const trashTarget = {
      ...summary("message-to-trash", "Trash opaque message"),
      uid: undefined,
      flags: ["\\Seen"],
      body_text: "Trash body",
      body_fetched: true,
    };
    let inboxRows = [archiveTarget, trashTarget];
    let archiveRows = [];
    let trashRows = [];
    desktop.mailApi.listMailboxPage.mockImplementation(async (_, role) =>
      mailboxPage(
        role === "inbox"
          ? inboxRows
          : role === "archive"
            ? archiveRows
            : role === "trash"
              ? trashRows
              : [],
        role,
      ),
    );
    desktop.mailApi.archiveMessage.mockImplementation(async (messageId) => {
      inboxRows = inboxRows.filter((message) => message.id !== messageId);
      archiveRows = [...archiveRows, archiveTarget];
      return {
        operation_id: `archive-${messageId}`,
        local_revision: 1,
        status: "pending",
        source_role: "inbox",
        destination_role: "archive",
      };
    });
    desktop.mailApi.moveMessageToTrash.mockImplementation(
      async (messageId) => {
        inboxRows = inboxRows.filter((message) => message.id !== messageId);
        trashRows = [...trashRows, trashTarget];
        return {
          operation_id: `trash-${messageId}`,
          local_revision: 1,
          status: "pending",
          source_role: "inbox",
          destination_role: "trash",
        };
      },
    );
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole("button", {
        name: /打开邮件：.*Archive opaque message/,
      }),
    );
    let reader = screen.getByLabelText("邮件阅读区");
    await user.click(
      within(reader).getByRole("button", { name: "归档" }),
    );
    await waitFor(() =>
      expect(desktop.mailApi.archiveMessage).toHaveBeenCalledWith(
        "message-to-archive",
      ),
    );

    expect(
      await screen.findByRole("heading", { name: "Trash opaque message" }),
    ).toBeTruthy();
    reader = screen.getByLabelText("邮件阅读区");
    await user.click(
      within(reader).getByRole("button", { name: "移到垃圾箱" }),
    );
    await waitFor(() =>
      expect(desktop.mailApi.moveMessageToTrash).toHaveBeenCalledWith(
        "message-to-trash",
      ),
    );
    expect(screen.queryByText("Archive opaque message")).toBeNull();
    expect(screen.queryByText("Trash opaque message")).toBeNull();
  });

  it("keeps a delayed mailbox mutation scoped to its captured account", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const accountStatus = {
      configured: true,
      accountId: "account-a",
      activeAccountId: "account-a",
      provider: "163",
      email: "a@example.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
      startupError: null,
      accounts: [
        {
          accountId: "account-a",
          provider: "163",
          email: "a@example.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
        {
          accountId: "account-b",
          provider: "gmail",
          email: "b@example.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
      ],
    };
    desktop.mailApi.getAccountStatus.mockResolvedValue(accountStatus);
    const accountATarget = {
      ...summary("account-a-target", "Account A target"),
      uid: undefined,
      body_text: "Account A target body",
      body_fetched: true,
      flags: ["\\Seen"],
    };
    const accountAAdjacent = {
      ...summary("account-a-adjacent", "Account A adjacent"),
      uid: undefined,
      body_text: "Account A adjacent body",
      body_fetched: true,
      flags: ["\\Seen"],
    };
    const accountBFirst = {
      ...summary("account-b-first", "Account B first"),
      uid: undefined,
      body_text: "Account B first body",
      body_fetched: true,
      flags: ["\\Seen"],
    };
    const accountBSelected = {
      ...summary("account-b-selected", "Account B remains selected"),
      uid: undefined,
      body_text: "Account B body",
      body_fetched: true,
      flags: ["\\Seen"],
    };
    let accountARows = [accountATarget, accountAAdjacent];
    desktop.mailApi.listMailboxPage.mockImplementation(
      async (accountId, role) =>
        mailboxPage(
          role === "inbox"
            ? accountId === "account-a"
              ? accountARows
              : [accountBFirst, accountBSelected]
            : [],
          role,
        ),
    );
    const archiveResult = deferred();
    desktop.mailApi.archiveMessage.mockReturnValue(archiveResult.promise);
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole("button", {
        name: /打开邮件：.*Account A target/,
      }),
    );
    await user.click(
      within(screen.getByLabelText("邮件阅读区")).getByRole("button", {
        name: "归档",
      }),
    );
    await waitFor(() =>
      expect(desktop.mailApi.archiveMessage).toHaveBeenCalledWith(
        "account-a-target",
      ),
    );
    await user.click(
      screen.getByRole("button", { name: "切换到 b@example.com" }),
    );
    await screen.findByRole("button", { name: "当前账户 b@example.com" });
    await user.click(screen.getByRole("button", { name: "收件箱" }));
    await user.click(
      await screen.findByRole("button", {
        name: /打开邮件：.*Account B remains selected/,
      }),
    );
    expect(
      await screen.findByRole("heading", {
        name: "Account B remains selected",
      }),
    ).toBeTruthy();

    accountARows = [accountAAdjacent];
    await act(async () => {
      archiveResult.resolve({
        operation_id: "archive-account-a",
        local_revision: 1,
        status: "pending",
        source_role: "inbox",
        destination_role: "archive",
      });
    });
    expect(
      screen.getByRole("heading", { name: "Account B remains selected" }),
    ).toBeTruthy();
    expect(screen.getByText("Account B body")).toBeTruthy();
    expect(screen.queryByText("Account A adjacent body")).toBeNull();
    expect(
      screen.queryByRole("heading", { name: "Account B first" }),
    ).toBeNull();

    await user.click(
      screen.getByRole("button", { name: "切换到 a@example.com" }),
    );
    await screen.findByRole("button", { name: "当前账户 a@example.com" });
    expect(
      await screen.findByRole("heading", { name: "Account A adjacent" }),
    ).toBeTruthy();
    expect(screen.getByText("Account A adjacent body")).toBeTruthy();
    expect(screen.queryByText("Account A target")).toBeNull();
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
    });
  });

  it("does not replace a newer same-account selection when an earlier mutation completes", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const target = {
      ...summary("race-target", "Mutation target"),
      uid: undefined,
      body_text: "Mutation target body",
      body_fetched: true,
      flags: ["\\Seen"],
    };
    const adjacent = {
      ...summary("race-adjacent", "Automatic adjacent"),
      uid: undefined,
      body_text: "Automatic adjacent body",
      body_fetched: true,
      flags: ["\\Seen"],
    };
    const manual = {
      ...summary("race-manual", "Manual newer selection"),
      uid: undefined,
      body_text: "Manual newer body",
      body_fetched: true,
      flags: ["\\Seen"],
    };
    let inboxRows = [target, adjacent, manual];
    desktop.mailApi.listMailboxPage.mockImplementation(async (_, role) =>
      mailboxPage(role === "inbox" ? inboxRows : [], role),
    );
    const archiveResult = deferred();
    desktop.mailApi.archiveMessage.mockReturnValue(archiveResult.promise);
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole("button", {
        name: /打开邮件：.*Mutation target/,
      }),
    );
    await user.click(
      within(screen.getByLabelText("邮件阅读区")).getByRole("button", {
        name: "归档",
      }),
    );
    await waitFor(() =>
      expect(desktop.mailApi.archiveMessage).toHaveBeenCalledWith("race-target"),
    );
    await user.click(
      screen.getByRole("button", {
        name: /打开邮件：.*Manual newer selection/,
      }),
    );
    expect(
      await screen.findByRole("heading", { name: "Manual newer selection" }),
    ).toBeTruthy();

    inboxRows = [adjacent, manual];
    await act(async () => {
      archiveResult.resolve({
        operation_id: "archive-race-target",
        local_revision: 1,
        status: "pending",
        source_role: "inbox",
        destination_role: "archive",
      });
    });
    expect(
      screen.getByRole("heading", { name: "Manual newer selection" }),
    ).toBeTruthy();
    expect(screen.getByText("Manual newer body")).toBeTruthy();
    expect(screen.queryByText("Automatic adjacent body")).toBeNull();
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
    });
  });

  it("requires a fresh confirmation plan before permanently deleting trash", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const trashMessage = {
      ...summary("message-in-trash", "Delete opaque message"),
      uid: undefined,
      displayed_role: "trash",
      flags: ["\\Seen"],
      body_text: "Trash body",
      body_fetched: true,
    };
    let deleted = false;
    desktop.mailApi.listMailboxPage.mockImplementation(async (_, role) =>
      mailboxPage(
        role === "trash" && !deleted ? [trashMessage] : [],
        role,
      ),
    );
    desktop.mailApi.confirmPermanentDelete.mockImplementation(
      async (planId) => {
        deleted = true;
        return {
          operation_id: `delete-${planId}`,
          local_revision: 1,
          status: "pending",
          source_role: "trash",
        };
      },
    );
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: /^垃圾箱$/ }));
    await user.click(
      await screen.findByRole("button", {
        name: /打开邮件：.*Delete opaque message/,
      }),
    );
    let reader = screen.getByLabelText("邮件阅读区");
    await user.click(
      within(reader).getByRole("button", { name: "永久删除" }),
    );
    let dialog = await screen.findByRole("alertdialog", {
      name: "永久删除这封邮件？",
    });
    expect(desktop.mailApi.preparePermanentDelete).toHaveBeenCalledWith(
      "message-in-trash",
    );
    await user.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(desktop.mailApi.confirmPermanentDelete).not.toHaveBeenCalled();
    expect(screen.getAllByText("Delete opaque message").length).toBeGreaterThan(
      0,
    );

    reader = screen.getByLabelText("邮件阅读区");
    await user.click(
      within(reader).getByRole("button", { name: "永久删除" }),
    );
    dialog = await screen.findByRole("alertdialog", {
      name: "永久删除这封邮件？",
    });
    await user.click(
      within(dialog).getByRole("button", { name: "永久删除" }),
    );
    await waitFor(() =>
      expect(desktop.mailApi.confirmPermanentDelete).toHaveBeenCalledWith(
        "delete-plan-message-in-trash",
      ),
    );
    await waitFor(() =>
      expect(screen.queryByText("Delete opaque message")).toBeNull(),
    );
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
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
        document.querySelectorAll('img[src="data:image/png;base64,AQID"]')
          .length,
      ).toBeGreaterThanOrEqual(2);
    });
    expect(
      document.querySelector('img[src="data:image/png;base64,AAAA"]'),
    ).toBeTruthy();
    expect(screen.getByLabelText("设置 Sender 1 的头像")).toBeTruthy();
  });

  it("separates current contacts from app-wide favorites and reuses the reader", async () => {
    const contact = {
      accountId: "desktop-account",
      email: "friend@example.com",
      displayName: "Friend",
      isFavorite: false,
      messageCount: 1,
      lastMessageAt: "2026-07-14T09:00:00Z",
      lastSubject: "Contact hello",
    };
    const pinnedContact = {
      ...contact,
      email: "pinned@example.com",
      displayName: "Pinned",
      isFavorite: true,
      lastMessageAt: "2026-07-01T09:00:00Z",
    };
    const favoritedContact = { ...contact, isFavorite: true };
    const contactMessage = {
      ...summary(33, "Contact hello"),
      mailbox: "Archive/2026",
      sender: { name: "Friend", email: "friend@example.com" },
      direction: "incoming",
      // Contact summaries intentionally omit bodies even when SQLite already
      // owns one, so opening must still hydrate the canonical local message.
      body_fetched: true,
    };
    desktop.mailApi.listContacts
      .mockResolvedValueOnce({
        contacts: [pinnedContact, contact],
        favorites: [pinnedContact],
      })
      .mockResolvedValue({
        contacts: [favoritedContact, pinnedContact],
        favorites: [favoritedContact, pinnedContact],
      });
    desktop.mailApi.listContactMessages.mockResolvedValue([contactMessage]);
    desktop.mailApi.fetchMailboxMessage.mockResolvedValue({
      ...contactMessage,
      body_text: "Contact message body",
      body_fetched: true,
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "通讯录" }));
    expect(
      await screen.findByRole("button", { name: "查看联系人 Friend" }),
    ).toBeTruthy();
    const contactList = screen.getByRole("list", { name: "联系人" });
    expect(
      within(contactList).getAllByRole("listitem")[0].textContent,
    ).toContain("Pinned");
    expect(
      screen.getByLabelText("邮件阅读区，当前未打开邮件"),
    ).toBeTruthy();
    expect(desktop.mailApi.listContactMessages).not.toHaveBeenCalled();
    await user.click(
      within(contactList).getByRole("button", {
        name: "查看联系人 Pinned",
      }),
    );
    await waitFor(() =>
      expect(desktop.mailApi.listContactMessages).toHaveBeenCalledWith(
        "desktop-account",
        "pinned@example.com",
        250,
      ),
    );

    await user.click(screen.getByRole("tab", { name: "收藏" }));
    expect(within(contactList).getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getAllByLabelText("收藏账号：me@163.com")).toHaveLength(2);
    expect(await screen.findByRole("heading", { name: "Pinned" })).toBeTruthy();
    await user.click(screen.getByRole("tab", { name: "全部" }));

    await user.click(
      within(contactList).getByRole("button", { name: "收藏 Friend" }),
    );
    await waitFor(() =>
      expect(desktop.mailApi.setContactFavorite).toHaveBeenCalledWith(
        "desktop-account",
        "friend@example.com",
        true,
      ),
    );
    await waitFor(() =>
      expect(
        within(contactList).getAllByRole("listitem")[0].textContent,
      ).toContain("Friend"),
    );
    expect(screen.queryByRole("button", { name: "保存联系人" })).toBeNull();

    await user.click(
      screen.getByRole("button", { name: "打开邮件：Contact hello" }),
    );
    expect(await screen.findByText("Contact message body")).toBeTruthy();
    expect(desktop.mailApi.fetchMailboxMessage).toHaveBeenCalledWith("33");

    await user.click(screen.getByRole("button", { name: "返回联系人详情" }));
    expect(
      await screen.findByRole("heading", { name: "往来邮件" }),
    ).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "返回通讯录" }));
    expect(
      screen.getByLabelText("邮件阅读区，当前未打开邮件"),
    ).toBeTruthy();
  });

  it("removes the previous correspondence before loading another contact", async () => {
    const friend = {
      accountId: "desktop-account",
      email: "friend@example.com",
      displayName: "Friend",
      isFavorite: false,
      messageCount: 1,
    };
    const colleague = {
      ...friend,
      email: "colleague@example.com",
      displayName: "Colleague",
    };
    const friendMessage = {
      ...summary(71, "Friend history"),
      mailbox_role: "inbox",
      direction: "incoming",
    };
    const colleagueMessage = {
      ...summary(72, "Colleague history"),
      mailbox_role: "sent",
      direction: "outgoing",
    };
    const colleagueMessages = deferred();
    desktop.mailApi.listContacts.mockResolvedValue({
      contacts: [friend, colleague],
      favorites: [],
    });
    desktop.mailApi.listContactMessages.mockImplementation(
      async (_accountId, email) =>
        email === friend.email
          ? [friendMessage]
          : colleagueMessages.promise,
    );
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "通讯录" }));
    await user.click(
      await screen.findByRole("button", { name: "查看联系人 Friend" }),
    );
    expect(
      await screen.findByRole("button", { name: "打开邮件：Friend history" }),
    ).toBeTruthy();

    await user.click(
      screen.getByRole("button", { name: "查看联系人 Colleague" }),
    );

    expect(
      screen.queryByRole("button", { name: "打开邮件：Friend history" }),
    ).toBeNull();
    expect(screen.getByText("正在加载往来邮件…")).toBeTruthy();
    expect(screen.getByLabelText("Colleague 的联系人详情")).toBeTruthy();

    await act(async () => {
      colleagueMessages.resolve([colleagueMessage]);
      await colleagueMessages.promise;
    });
    expect(
      await screen.findByRole("button", {
        name: "打开邮件：Colleague history",
      }),
    ).toBeTruthy();
  });

  it("shows only the active account in all and labels every app-wide favorite", async () => {
    desktop.mailApi.getAccountStatus.mockResolvedValue({
      configured: true,
      accountId: "account-163",
      activeAccountId: "account-163",
      provider: "163",
      email: "me@163.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
      startupError: null,
      accounts: [
        {
          accountId: "account-163",
          provider: "163",
          email: "me@163.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
        {
          accountId: "account-gmail",
          provider: "gmail",
          email: "me@gmail.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
      ],
    });
    desktop.mailApi.listContacts.mockResolvedValue({
      contacts: [
        {
          accountId: "account-163",
          email: "local@163.com",
          displayName: "163 联系人",
          isFavorite: false,
          messageCount: 2,
        },
      ],
      favorites: [
        {
          accountId: "account-gmail",
          email: "friend@gmail.com",
          displayName: "Gmail 联系人",
          isFavorite: true,
          messageCount: 3,
        },
      ],
    });
    const gmailMessage = {
      ...summary(88, "Gmail history"),
      mailbox_role: "inbox",
      sender: { name: "Gmail 联系人", email: "friend@gmail.com" },
      direction: "incoming",
      body_fetched: true,
    };
    desktop.mailApi.listContactMessages.mockImplementation(
      async (accountId) =>
        accountId === "account-gmail" ? [gmailMessage] : [],
    );
    desktop.mailApi.fetchMailboxMessage.mockImplementation(async (messageId) => {
      if (messageId === "88") {
        return {
          ...gmailMessage,
          body_text: "Cross-account body",
          body_fetched: true,
        };
      }
      return desktop.fixtures.inboxMessageSource(Number(messageId));
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "通讯录" }));
    expect(
      await screen.findByRole("button", { name: "查看联系人 163 联系人" }),
    ).toBeTruthy();
    expect(screen.queryByText("Gmail 联系人")).toBeNull();

    await user.click(screen.getByRole("tab", { name: "收藏" }));
    expect(
      await screen.findByRole("button", {
        name: "查看联系人 Gmail 联系人（me@gmail.com）",
      }),
    ).toBeTruthy();
    expect(screen.getAllByLabelText("收藏账号：me@gmail.com")).toHaveLength(1);
    await user.click(
      screen.getByRole("button", {
        name: "查看联系人 Gmail 联系人（me@gmail.com）",
      }),
    );
    expect(screen.getAllByLabelText("收藏账号：me@gmail.com")).toHaveLength(2);
    await waitFor(() =>
      expect(desktop.mailApi.listContactMessages).toHaveBeenCalledWith(
        "account-gmail",
        "friend@gmail.com",
        250,
      ),
    );
    await user.click(
      await screen.findByRole("button", { name: "打开邮件：Gmail history" }),
    );
    expect(await screen.findByText("Cross-account body")).toBeTruthy();
    expect(desktop.mailApi.switchAccount).toHaveBeenCalledOnce();
    expect(desktop.mailApi.switchAccount).toHaveBeenCalledWith("account-gmail");
    expect(desktop.mailApi.fetchMailboxMessage).toHaveBeenCalledWith("88");
    expect(
      desktop.mailApi.switchAccount.mock.invocationCallOrder.at(-1),
    ).toBeLessThan(
      desktop.mailApi.fetchMailboxMessage.mock.invocationCallOrder.at(-1),
    );
    await user.click(
      screen.getByRole("button", { name: "返回联系人详情" }),
    );
    expect(
      await screen.findByRole("heading", { name: "Gmail 联系人" }),
    ).toBeTruthy();
    expect(screen.getByRole("heading", { name: "往来邮件" })).toBeTruthy();
  });

  it("cancels a delayed cross-account contact open after leaving the contact", async () => {
    desktop.mailApi.getAccountStatus.mockResolvedValue({
      configured: true,
      accountId: "account-163",
      activeAccountId: "account-163",
      provider: "163",
      email: "me@163.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
      startupError: null,
      accounts: [
        {
          accountId: "account-163",
          provider: "163",
          email: "me@163.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
        {
          accountId: "account-gmail",
          provider: "gmail",
          email: "me@gmail.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
      ],
    });
    const contact = {
      accountId: "account-gmail",
      email: "friend@gmail.com",
      displayName: "Delayed Gmail contact",
      isFavorite: true,
      messageCount: 1,
    };
    const history = {
      ...summary("gmail-delayed-message", "Delayed Gmail history"),
      uid: undefined,
      mailbox_role: "inbox",
      sender: { name: "Friend", email: contact.email },
      direction: "incoming",
      body_fetched: true,
    };
    desktop.mailApi.listContacts.mockResolvedValue({
      contacts: [],
      favorites: [contact],
    });
    desktop.mailApi.listContactMessages.mockResolvedValue([history]);
    const switchResult = deferred();
    desktop.mailApi.switchAccount.mockReturnValue(switchResult.promise);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "通讯录" }));
    await user.click(await screen.findByRole("tab", { name: "收藏" }));
    await user.click(
      await screen.findByRole("button", {
        name: "查看联系人 Delayed Gmail contact（me@gmail.com）",
      }),
    );
    await user.click(
      await screen.findByRole("button", {
        name: "打开邮件：Delayed Gmail history",
      }),
    );
    await waitFor(() =>
      expect(desktop.mailApi.switchAccount).toHaveBeenCalledWith(
        "account-gmail",
      ),
    );
    await user.click(
      screen.getByRole("button", { name: "返回通讯录" }),
    );

    await act(async () => {
      switchResult.resolve({
        ...(await desktop.mailApi.getAccountStatus()),
        accountId: "account-gmail",
        activeAccountId: "account-gmail",
        email: "me@gmail.com",
      });
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(desktop.mailApi.fetchMailboxMessage).not.toHaveBeenCalledWith(
      "gmail-delayed-message",
    );
    expect(screen.queryByText("Delayed Gmail history")).toBeNull();
    expect(
      screen.getByLabelText("邮件阅读区，当前未打开邮件"),
    ).toBeTruthy();
  });

  it("filters app-wide favorites without an owning account and reports recovery guidance", async () => {
    desktop.mailApi.listContacts.mockResolvedValue({
      contacts: [
        {
          email: "valid@example.com",
          displayName: "Valid active contact",
          isFavorite: false,
          messageCount: 1,
        },
      ],
      favorites: [
        {
          email: "legacy@example.com",
          displayName: "Legacy favorite without account",
          isFavorite: true,
          messageCount: 2,
        },
      ],
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "通讯录" }));
    await user.click(screen.getByRole("tab", { name: "收藏" }));
    expect(screen.queryByText("Legacy favorite without account")).toBeNull();
    expect(
      screen.getByText(
        "部分收藏联系人缺少邮箱账户归属，已暂时忽略；重新收藏后可恢复。",
      ),
    ).toBeTruthy();
    expect(desktop.mailApi.setContactFavorite).not.toHaveBeenCalled();
  });

  it("keeps visible contact rows mounted during background mailbox refresh", async () => {
    const contact = {
      email: "friend@example.com",
      displayName: "Friend",
      isFavorite: false,
      messageCount: 1,
      lastMessageAt: "2026-07-14T09:00:00Z",
      lastSubject: "Before refresh",
    };
    const backgroundContacts = deferred();
    desktop.mailApi.listContacts
      .mockResolvedValueOnce([contact])
      .mockReturnValueOnce(backgroundContacts.promise);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "通讯录" }));
    expect(
      await screen.findByRole("button", { name: "查看联系人 Friend" }),
    ).toBeTruthy();
    await waitFor(() =>
      expect(desktop.listeners.get("mail:inbox-updated")).toBeTruthy(),
    );

    act(() => desktop.listeners.get("mail:inbox-updated")());
    await waitFor(() =>
      expect(desktop.mailApi.listContacts).toHaveBeenCalledTimes(2),
    );
    expect(
      screen.getByRole("button", { name: "查看联系人 Friend" }),
    ).toBeTruthy();
    expect(screen.queryByText("正在加载联系人…")).toBeNull();

    await act(async () => {
      backgroundContacts.resolve([
        { ...contact, lastSubject: "After refresh" },
      ]);
    });
    expect(await screen.findByText("1 封往来 · After refresh")).toBeTruthy();
  });

  it("saves a contact remark and prioritizes it in mail names", async () => {
    const contact = {
      email: "sender1@example.com",
      displayName: "Sender 1",
      originalName: "Sender 1",
      remark: null,
      isFavorite: false,
      messageCount: 1,
      lastMessageAt: "2026-07-14T09:00:00Z",
      lastSubject: "First mail",
    };
    const remarkedContact = {
      ...contact,
      displayName: "林老师",
      remark: "林老师",
    };
    desktop.mailApi.listContacts
      .mockResolvedValueOnce([contact])
      .mockResolvedValue([remarkedContact]);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "通讯录" }));
    await user.click(
      await screen.findByRole("button", { name: "查看联系人 Sender 1" }),
    );
    await user.click(await screen.findByRole("button", { name: "添加备注" }));
    const input = await screen.findByRole("textbox", { name: "联系人备注名" });
    await user.type(input, "林老师");
    await user.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(desktop.mailApi.setContactRemark).toHaveBeenCalledWith(
        "sender1@example.com",
        "林老师",
      ),
    );
    expect(await screen.findByText("(Sender 1)")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "查看联系人 林老师" }),
    ).toBeTruthy();

    await user.click(screen.getByRole("button", { name: /^收件箱/ }));
    const openMail = await screen.findByRole("button", {
      name: /打开邮件：林老师，/,
    });
    const mailRow = openMail.closest("li");
    expect(mailRow.textContent).toContain("林老师");
    await user.click(openMail);
    await screen.findByText("Loaded body");
    expect(
      document.querySelector(".sender-card__identity strong")?.textContent,
    ).toBe("林老师");
  });

  it("ignores a stale body response after the user selects another message", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const first = summary(1, "First mail");
    const second = summary(2, "Second mail");
    desktop.fixtures.inboxPageSource.mockResolvedValue([first, second]);
    let resolveFirst;
    let resolveSecond;
    desktop.fixtures.inboxMessageSource.mockImplementation(
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
      resolveSecond({
        ...second,
        body_text: "Second body",
        body_fetched: true,
      });
    });
    await screen.findByText("Second body");
    await act(async () => {
      resolveFirst({
        ...first,
        body_text: "Stale first body",
        body_fetched: true,
      });
    });

    expect(screen.getByRole("heading", { name: "Second mail" })).toBeTruthy();
    expect(screen.queryByText("Stale first body")).toBeNull();
  });

  it("keeps authoritative attachment metadata cached across message switches and retries independently", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const first = summary(41, "Attachment mail");
    const second = summary(42, "Other mail");
    desktop.fixtures.inboxPageSource.mockResolvedValue([first, second]);
    desktop.mailApi.fetchMailboxMessage.mockImplementation(async (messageId) => {
      if (messageId === "41") {
        return {
          ...first,
          body_text: "Attachment body",
          body_fetched: true,
          attachments: [
            {
              id: "part-a",
              safe_display_name: "alpha.pdf",
              mime_type: "application/pdf",
              size_bytes: 2048,
              disposition: "attachment",
            },
            {
              id: "part-b",
              safe_display_name: "beta.zip",
              mime_type: "application/zip",
              size_bytes: 4096,
              disposition: "attachment",
            },
          ],
        };
      }
      return {
        ...second,
        body_text: "Other body",
        body_fetched: true,
        attachments: [],
      };
    });
    desktop.mailApi.saveMessageAttachment
      .mockResolvedValueOnce({
        status: "error",
        error_kind: "disk_full",
        retryable: true,
      })
      .mockResolvedValueOnce({
        status: "saved",
        file_name: "alpha (1).pdf",
        retryable: false,
      });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByText("Attachment mail"));
    await user.click(
      await screen.findByRole("button", { name: "保存附件 alpha.pdf" }),
    );
    expect(await screen.findByText(/磁盘空间不足，可重试/)).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Attachment mail" })).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "保存附件 beta.zip" }),
    ).toBeTruthy();

    await user.click(screen.getByText("Other mail"));
    expect(await screen.findByText("Other body")).toBeTruthy();
    await user.click(screen.getByText("Attachment mail"));

    expect(
      await screen.findByRole("button", {
        name: "重试保存附件 alpha.pdf：磁盘空间不足",
      }),
    ).toBeTruthy();
    expect(
      desktop.mailApi.fetchMailboxMessage.mock.calls.filter(
        ([messageId]) => messageId === "41",
      ),
    ).toHaveLength(1);

    await user.click(
      screen.getByRole("button", {
        name: "重试保存附件 alpha.pdf：磁盘空间不足",
      }),
    );
    expect(
      await screen.findByRole("group", {
        name: "附件 alpha.pdf 已保存为 alpha (1).pdf",
      }),
    ).toBeTruthy();
    expect(desktop.mailApi.saveMessageAttachment.mock.calls).toEqual([
      ["41", "part-a"],
      ["41", "part-a"],
    ]);
  });

  it("shows only the reader loading state while the full body hydrates", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const localSummary = {
      ...summary(3, "Instant mail"),
      preview: "Immediately visible local copy",
    };
    const bodyResponse = deferred();
    desktop.fixtures.inboxPageSource.mockResolvedValue([localSummary]);
    desktop.fixtures.inboxMessageSource.mockReturnValue(bodyResponse.promise);
    const user = userEvent.setup();

    render(<App />);
    await user.click(await screen.findByText("Instant mail"));

    const reader = screen.getByLabelText("邮件阅读区");
    expect(within(reader).getByLabelText("正在加载正文")).toBeTruthy();
    expect(
      within(reader).queryByText("Immediately visible local copy"),
    ).toBeNull();

    await act(async () => {
      bodyResponse.resolve({
        ...localSummary,
        body_text: "Canonical full body",
        body_fetched: true,
      });
    });
    expect(await within(reader).findByText("Canonical full body")).toBeTruthy();
    expect(within(reader).queryByLabelText("正在加载正文")).toBeNull();
  });

  it("hydrates cached HTML on selection and preserves it across summary refreshes", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const richSummary = {
      ...summary(7, "Rich mail"),
      body_text: "Flattened duplicate copy",
      body_fetched: true,
      body_html_available: true,
      body_html_loaded: false,
    };
    const bodyResponse = deferred();
    desktop.fixtures.inboxPageSource.mockResolvedValue([richSummary]);
    desktop.fixtures.inboxMessageSource.mockReturnValue(bodyResponse.promise);
    const user = userEvent.setup();

    render(<App />);
    await user.click(await screen.findByText("Rich mail"));

    const reader = screen.getByLabelText("邮件阅读区");
    expect(within(reader).getByLabelText("正在加载正文")).toBeTruthy();
    expect(within(reader).queryByText("Flattened duplicate copy")).toBeNull();

    await act(async () => {
      bodyResponse.resolve({
        ...richSummary,
        body_html:
          '<table><tbody><tr><td class="desktop">Rich layout</td></tr></tbody></table>',
        body_render_mode: "isolated_html",
        body_html_loaded: true,
        has_remote_images: false,
      });
    });
    const frame = await screen.findByTitle("Rich mail HTML 正文");
    expect(desktop.fixtures.inboxMessageSource).toHaveBeenCalledWith(7);
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
      expect(
        screen.getByTitle("Rich mail HTML 正文").getAttribute("srcdoc"),
      ).toContain("Rich layout");
    });
  });

  it("keeps the current reader stable when a new arrival moves it outside the bounded inbox", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    desktop.fixtures.inboxPageSource.mockResolvedValue([
      summary(1, "First mail"),
    ]);
    desktop.fixtures.inboxMessageSource.mockResolvedValue({
      ...summary(1, "First mail"),
      body_text: "Reader content must remain visible",
      body_fetched: true,
    });
    const user = userEvent.setup();

    render(<App />);
    const inbox = await screen.findByLabelText("收件箱邮件列表");
    await user.click(await within(inbox).findByText("First mail"));
    const reader = screen.getByLabelText("邮件阅读区");
    expect(
      await within(reader).findByText("Reader content must remain visible"),
    ).toBeTruthy();

    desktop.fixtures.inboxPageSource.mockResolvedValue([
      summary(2, "New arrival"),
    ]);
    await waitFor(() =>
      expect(desktop.listeners.has("mail:inbox-updated")).toBe(true),
    );
    await act(async () => {
      desktop.listeners.get("mail:inbox-updated")?.({ payload: {} });
    });

    await waitFor(() =>
      expect(desktop.fixtures.inboxPageSource).toHaveBeenCalledTimes(2),
    );
    expect(
      within(reader).getByText("Reader content must remain visible"),
    ).toBeTruthy();
    expect(
      within(reader).getByRole("heading", { name: "First mail" }),
    ).toBeTruthy();
  });

  it("renders a reply as native authored text with collapsed quoted history", async () => {
    const replySummary = {
      ...summary(9, "Reply mail"),
      body_text: "My reply preview",
      body_fetched: true,
      body_html_available: true,
      body_html_loaded: false,
    };
    desktop.fixtures.inboxPageSource.mockResolvedValue([replySummary]);
    desktop.fixtures.inboxMessageSource.mockResolvedValue({
      ...replySummary,
      body_text: "My reply.\n\nOriginal body.",
      body_html:
        "<div>My reply.</div><table><tr><td>Original body.</td></tr></table>",
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
    expect(screen.getByText("引用邮件 1")).toBeTruthy();
    expect(container.querySelector("details.quoted-message").open).toBe(false);
    expect(container.querySelector("iframe")).toBeNull();
  });

  it("opens quoted remote mail only through its opaque id", async () => {
    const replySummary = {
      ...summary(9, "Reply with local ancestor"),
      mailbox: "INBOX",
    };
    const sentAncestor = {
      ...summary(71, "Original sent message"),
      mailbox: "Sent",
      sender: { name: "Me", email: "me@163.com" },
      to: [{ name: "Friend", email: "friend@example.com" }],
    };
    desktop.fixtures.inboxPageSource.mockResolvedValue([replySummary]);
    desktop.fixtures.sentPageSource.mockResolvedValue([sentAncestor]);
    desktop.fixtures.inboxMessageSource.mockResolvedValue({
      ...replySummary,
      body_text: "Current reply.\n\nEarlier body.",
      body_fetched: true,
      body_segments: [
        {
          kind: "authored",
          content: "Current reply.",
          render_mode: "plain",
          quote_depth: 0,
          confidence: "high",
        },
        {
          kind: "quoted",
          content: "Earlier body.",
          render_mode: "plain",
          quote_depth: 1,
          confidence: "high",
          quote_metadata: { subject: "Original sent message" },
          navigation_target: {
            id: "71",
            mailbox: "Sent",
            uid: 71,
          },
        },
      ],
    });
    desktop.fixtures.sentMessageSource.mockResolvedValue({
      ...sentAncestor,
      body_text: "Canonical sent ancestor body",
      body_fetched: true,
    });
    const user = userEvent.setup();
    const { container } = render(<App />);

    await user.click(await screen.findByText("Reply with local ancestor"));
    const details = container.querySelector("details.quoted-message");
    expect(details.open).toBe(false);

    await user.click(
      screen.getByRole("button", {
        name: "在已发送中打开原邮件：Original sent message",
      }),
    );
    expect(await screen.findByText("Canonical sent ancestor body")).toBeTruthy();
    expect(desktop.mailApi.fetchMailboxMessage).toHaveBeenLastCalledWith("71");
    expect(details.open).toBe(false);
  });

  it("does not expose quoted-message navigation into Archive or Trash", async () => {
    const reply = {
      ...summary("reply-with-archived-ancestor", "Reply with archived ancestor"),
      uid: undefined,
    };
    const archivedAncestor = {
      ...summary("archived-ancestor", "Archived ancestor"),
      uid: undefined,
      displayed_role: "archive",
    };
    desktop.mailApi.listMailboxPage.mockImplementation(async (_, role) =>
      mailboxPage(
        role === "inbox"
          ? [reply]
          : role === "archive"
            ? [archivedAncestor]
            : [],
        role,
      ),
    );
    desktop.mailApi.fetchMailboxMessage.mockImplementation(async (messageId) =>
      messageId === "reply-with-archived-ancestor"
        ? {
            ...reply,
            body_text: "Reply body\n\nArchived body",
            body_fetched: true,
            body_segments: [
              {
                kind: "authored",
                content: "Reply body",
                render_mode: "plain",
                quote_depth: 0,
                confidence: "high",
              },
              {
                kind: "quoted",
                content: "Archived body",
                render_mode: "plain",
                quote_depth: 1,
                confidence: "high",
                quote_metadata: { subject: "Archived ancestor" },
                navigation_target: { id: "archived-ancestor" },
              },
            ],
          }
        : archivedAncestor,
    );
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByText("Reply with archived ancestor"));
    expect(await screen.findByText("Archived body")).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: /打开原邮件：Archived ancestor/ }),
    ).toBeNull();
  });

  it("shows the immutable reply subject and recipient for sent mail", async () => {
    desktop.mailApi.listDrafts.mockResolvedValue([]);
    desktop.mailApi.listOutbox.mockResolvedValue([]);
    desktop.mailApi.listSentOutboxFallbacks.mockResolvedValue([
      {
        id: "sent-reply",
        draft_id: null,
        recipients: ["sender1@example.com"],
        subject: "Re: First mail",
        preview: "Thanks for the update.",
        status: "sent",
        attempts: 1,
        last_error: null,
        created_at: "2026-07-14T09:10:00Z",
        sent_at: "2026-07-14T09:10:01Z",
      },
    ]);
    desktop.mailApi.fetchOutboxMessage.mockResolvedValue({
      id: "sent-reply",
      subject: "Re: First mail",
      body_text: "Thanks for the update.\n\n—— 原邮件 ——\nOriginal body.",
      body_render_mode: "plain",
      body_segments: [
        {
          kind: "authored",
          content: "Thanks for the update.",
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
      body_html_available: false,
      body_html_loaded: true,
      has_remote_images: false,
      body_fetched: true,
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /已发送/ }));

    const sentList = screen.getByLabelText("已发送邮件列表");
    expect(within(sentList).getByText("Re: First mail")).toBeTruthy();
    expect(within(sentList).getByText("sender1@example.com")).toBeTruthy();
    expect(within(sentList).queryByText("Mine Mail")).toBeNull();

    await user.click(within(sentList).getByText("Re: First mail"));

    expect(
      await screen.findByText("Thanks for the update.", {
        selector: ".message-body span",
      }),
    ).toBeTruthy();
    expect(screen.getByText("Original body.")).toBeTruthy();
    expect(screen.getByText("引用邮件 1")).toBeTruthy();
    expect(screen.queryByText(/At 2026/)).toBeNull();
    expect(desktop.mailApi.fetchOutboxMessage).toHaveBeenCalledWith(
      "sent-reply",
    );
    const reader = screen.getByLabelText("邮件阅读区");
    expect(within(reader).getByText("SENT")).toBeTruthy();
    expect(within(reader).queryByText(/状态：已发送/)).toBeNull();
    expect(
      within(reader).queryByText(/收件人：sender1@example.com/),
    ).toBeNull();
  });

  it("merges remote Sent copies with local delivery records without duplicates", async () => {
    const remoteExact = {
      ...summary(71, "Exact remote copy"),
      mailbox: "已发送",
      message_id: "mine-71@mine-mail.invalid",
      sender: { name: "Me", email: "me@163.com" },
      to: [{ name: "Friend", email: "friend@example.com" }],
      sent_at: "2026-07-14T09:20:00Z",
    };
    const remoteLegacy = {
      ...summary(72, "Legacy remote copy"),
      mailbox: "已发送",
      message_id: "server-generated-72@163.com",
      sender: { name: "Me", email: "me@163.com" },
      to: [{ name: null, email: "friend@example.com" }],
      sent_at: "2026-07-14T09:21:00Z",
    };
    desktop.fixtures.sentPageSource.mockResolvedValue([
      remoteLegacy,
      remoteExact,
    ]);
    desktop.mailApi.listOutbox.mockResolvedValue([]);
    desktop.mailApi.listSentOutboxFallbacks.mockResolvedValue([
      {
        id: "local-exact",
        recipients: ["friend@example.com"],
        subject: "Exact remote copy",
        preview: "Exact preview",
        status: "sent",
        attempts: 1,
        created_at: "2026-07-14T09:19:59Z",
        sent_at: "2026-07-14T09:20:01Z",
        message_id: "<mine-71@mine-mail.invalid>",
        message_date: "2026-07-14T09:20:00Z",
      },
      {
        id: "local-legacy",
        recipients: ["friend@example.com"],
        subject: "Legacy remote copy",
        preview: "Legacy preview",
        status: "sent",
        attempts: 1,
        created_at: "2026-07-14T09:20:59Z",
        sent_at: "2026-07-14T09:21:01Z",
        message_id: null,
        message_date: "2026-07-14T09:21:00Z",
      },
    ]);
    desktop.fixtures.sentMessageSource.mockResolvedValue({
      ...remoteExact,
      body_text: "Remote sent body",
      body_fetched: true,
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /已发送/ }));
    const sentList = screen.getByLabelText("已发送邮件列表");
    expect(within(sentList).getAllByText("Exact remote copy")).toHaveLength(1);
    expect(within(sentList).getAllByText("Legacy remote copy")).toHaveLength(1);

    await user.click(within(sentList).getByText("Exact remote copy"));
    expect(await screen.findByText("Remote sent body")).toBeTruthy();
    expect(desktop.fixtures.sentMessageSource).toHaveBeenCalledWith(71);
    expect(desktop.mailApi.fetchOutboxMessage).not.toHaveBeenCalledWith(
      "local-exact",
    );
  });

  it("renders bounded semantic HTML directly on the themed reader material", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const nativeSummary = {
      ...summary(8, "Native mail"),
      body_text: "Myo myo@paa.moe",
      body_fetched: true,
      body_html_available: true,
      body_html_loaded: false,
    };
    desktop.fixtures.inboxPageSource.mockResolvedValue([nativeSummary]);
    desktop.fixtures.inboxMessageSource.mockResolvedValue({
      ...nativeSummary,
      body_html:
        '<p>Hello <strong>Myo</strong></p><a href="https://paa.moe">Profile</a>',
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

  it("retries Rust-prepared forwarding without exposing a reader failure card", async () => {
    const source = {
      ...summary(8, "Forward source"),
      body_text: "Complete source body",
      body_fetched: true,
      preview: "Unsafe list preview",
    };
    const preparedDraft = {
      id: "prepared-forward",
      local_version: 1,
      to: [],
      cc: [],
      bcc: [],
      subject: "Fwd: Forward source",
      body_text: "",
      format: {
        body_html: null,
        stationery: "none",
        send_stationery: false,
      },
      attachments: [],
      forward_context: {
        source_message_id: "8",
        original_subject: "Forward source",
        from: source.sender,
        to: [{ name: "Me", email: "me@163.com" }],
        cc: [],
        sent_at: source.sent_at,
        quoted_text: "Complete source body",
        quoted_html: null,
        quoted_render_mode: "plain_text",
        source_attachments: [
          {
            id: "source-part",
            safe_display_name: "source.pdf",
            mime_type: "application/pdf",
            size_bytes: 2048,
            disposition: "attachment",
          },
        ],
      },
      status: "local",
    };
    desktop.fixtures.inboxPageSource.mockResolvedValue([source]);
    desktop.fixtures.inboxMessageSource.mockResolvedValue(source);
    desktop.mailApi.prepareForward
      .mockResolvedValueOnce({
        kind: "error",
        error: {
          kind: "attachment_stage_failed",
          failed_attachment_ids: ["source-part"],
          retry_without_attachments_allowed: true,
        },
      })
      .mockResolvedValueOnce({
        kind: "prepared",
        prepared: {
          draft: preparedDraft,
          warnings: [],
        },
      });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByText("Forward source"));
    await user.click(screen.getByRole("button", { name: "转发" }));
    expect(
      screen.queryByText("一个或多个原邮件附件无法安全加入草稿。"),
    ).toBeNull();
    expect(screen.queryByText("转发准备失败")).toBeNull();
    expect(screen.queryByRole("button", { name: "无附件转发" })).toBeNull();
    await user.click(
      screen.getByRole("button", { name: "重试准备转发" }),
    );

    const composer = await screen.findByRole("dialog", { name: "编辑草稿" });
    expect(within(composer).getByLabelText("主题").value).toBe(
      "Fwd: Forward source",
    );
    expect(
      (await within(composer).findByLabelText("邮件正文")).textContent,
    ).toBe("");
    expect(within(composer).queryByText("Unsafe list preview")).toBeNull();
    expect(
      within(composer).getByLabelText("不可编辑的转发原文"),
    ).toBeTruthy();
    expect(
      within(composer).queryByText("无附件转发：原邮件附件未加入当前草稿。"),
    ).toBeNull();
    expect(desktop.mailApi.prepareForward.mock.calls).toEqual([
      ["8", true],
      ["8", true],
    ]);
  });

  it("prepares a forward immediately while body hydration is still pending", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const source = {
      ...summary("fast-forward-source", "Fast forward source"),
      uid: undefined,
      preview: "不可转发摘要",
      body_text: null,
      body_fetched: false,
    };
    const pendingBody = deferred();
    desktop.fixtures.inboxPageSource.mockResolvedValue([source]);
    desktop.mailApi.fetchMailboxMessage.mockImplementation(async (messageId) =>
      messageId === "fast-forward-source"
        ? pendingBody.promise
        : desktop.fixtures.inboxMessageSource(Number(messageId)),
    );
    desktop.mailApi.prepareForward.mockResolvedValue({
      kind: "prepared",
      prepared: {
        draft: {
          id: "fast-forward-draft",
          local_version: 1,
          to: [],
          cc: [],
          bcc: [],
          subject: "Fwd: Rust authoritative subject",
          body_text: "Rust prepared authored body",
          format: {
            body_html: null,
            stationery: "none",
            send_stationery: false,
          },
          attachments: [],
          forward_context: {
            source_message_id: "fast-forward-source",
            original_subject: "Fast forward source",
            from: source.sender,
            to: [],
            cc: [],
            sent_at: source.sent_at,
            quoted_text: "Rust authoritative quoted body",
            quoted_html: null,
            quoted_render_mode: "plain_text",
            source_attachments: [],
          },
          status: "local",
        },
        warnings: [],
      },
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByText("Fast forward source"));
    await user.click(screen.getByRole("button", { name: "转发" }));
    await waitFor(() =>
      expect(desktop.mailApi.prepareForward).toHaveBeenCalledWith(
        "fast-forward-source",
        true,
      ),
    );
    const composer = await screen.findByRole("dialog", { name: "编辑草稿" });
    expect(within(composer).getByLabelText("主题").value).toBe(
      "Fwd: Rust authoritative subject",
    );
    expect(
      (await within(composer).findByLabelText("邮件正文")).textContent,
    ).toContain("Rust prepared authored body");
    expect(within(composer).queryByText("不可转发摘要")).toBeNull();

    await act(async () => {
      pendingBody.reject(new Error("body hydration failed"));
    });
    expect(within(composer).queryByText("不可转发摘要")).toBeNull();
    expect(within(composer).getByLabelText("主题").value).toBe(
      "Fwd: Rust authoritative subject",
    );
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
    });
  });

  it("drops a pending forward result on account switch and allows a fresh retry after returning", async () => {
    desktop.mailApi.getAccountStatus.mockResolvedValue({
      configured: true,
      accountId: "account-163",
      activeAccountId: "account-163",
      provider: "163",
      email: "me@163.com",
      backendReady: true,
      credentialAvailable: true,
      networkReady: true,
      startupError: null,
      accounts: [
        {
          accountId: "account-163",
          provider: "163",
          email: "me@163.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
        {
          accountId: "account-gmail",
          provider: "gmail",
          email: "me@gmail.com",
          backendReady: true,
          credentialAvailable: true,
          networkReady: true,
        },
      ],
    });
    const pendingForward = deferred();
    desktop.mailApi.prepareForward
      .mockReturnValueOnce(pendingForward.promise)
      .mockResolvedValueOnce({
        kind: "error",
        error: {
          kind: "message_unavailable",
          failed_attachment_ids: [],
          retry_without_attachments_allowed: false,
        },
      });
    const user = userEvent.setup();
    render(<App />);

    const inbox = await screen.findByLabelText("收件箱邮件列表");
    await user.click(await within(inbox).findByText("First mail"));
    await user.click(screen.getByRole("button", { name: "转发" }));
    expect(
      screen.getByRole("button", { name: "正在准备转发…" }).disabled,
    ).toBe(true);
    await user.click(
      screen.getByRole("button", { name: "切换到 me@gmail.com" }),
    );
    await screen.findByRole("button", { name: "当前账户 me@gmail.com" });

    await act(async () => {
      pendingForward.resolve({
        kind: "prepared",
        prepared: {
          draft: {
            id: "late-forward",
            local_version: 1,
            to: [],
            cc: [],
            bcc: [],
            subject: "Late forward must stay in source account",
            body_text: "",
            format: {
              body_html: null,
              stationery: "none",
              send_stationery: false,
            },
            attachments: [],
            forward_context: null,
            status: "local",
          },
          warnings: [],
        },
      });
    });
    expect(screen.queryByRole("dialog")).toBeNull();

    await user.click(
      screen.getByRole("button", { name: "切换到 me@163.com" }),
    );
    await screen.findByRole("button", { name: "当前账户 me@163.com" });
    expect(
      await screen.findByRole("button", { name: "转发" }),
    ).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "转发" }));
    expect(
      await screen.findByRole("button", { name: "重试准备转发" }),
    ).toBeTruthy();
    expect(
      screen.queryByText("完整邮件暂时不可用，请重新同步后重试。"),
    ).toBeNull();
    expect(desktop.mailApi.prepareForward).toHaveBeenCalledTimes(2);
  });

  it("keeps links and inline images in the read-only reply context", async () => {
    const richSummary = {
      ...summary(8, "Hey tantless"),
      body_text: "Hey tantless A mail from paa.moe!",
      body_fetched: true,
    };
    desktop.fixtures.inboxPageSource.mockResolvedValue([richSummary]);
    desktop.fixtures.inboxMessageSource.mockResolvedValue(richSummary);
    desktop.mailApi.prepareReply.mockResolvedValue({
      to: ["sender8@example.com"],
      cc: [],
      bcc: [],
      subject: "Re: Hey tantless",
      body_text: "",
      reply_context: {
        parent_message_id: "hey-tantless@example.com",
        references: [],
        subject: "Hey tantless",
        sender: richSummary.sender,
        recipients: [{ name: "Mine Mail", email: "me@163.com" }],
        sent_at: richSummary.sent_at,
        quoted_text: richSummary.body_text,
        quoted_html:
          '<p>Hey tantless</p><p>A mail from <a href="https://paa.moe">paa.moe</a>!</p><img alt="Myo avatar" src="data:image/png;base64,AQID">',
        quoted_render_mode: "native_html",
        has_remote_images: false,
      },
    });
    const user = userEvent.setup();

    render(<App />);
    const inbox = await screen.findByLabelText("收件箱邮件列表");
    await user.click(await within(inbox).findByText("Hey tantless"));
    await user.click(await screen.findByRole("button", { name: "回复" }));
    const composer = await screen.findByRole("dialog", { name: "新邮件" });
    await user.click(
      within(composer).getByRole("button", { name: /Hey tantless/ }),
    );

    const link = within(composer).getByRole("link", { name: "paa.moe" });
    expect(within(composer).getByAltText("Myo avatar")).toBeTruthy();
    await user.click(link);
    expect(desktop.mailApi.openExternalUrl).toHaveBeenCalledWith(
      "https://paa.moe",
    );
  });

  it("drops a delayed reply preparation after the user selects another message", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    const replySource = {
      ...summary("reply-race-source", "Reply race source"),
      uid: undefined,
      body_text: "Reply source body",
      body_fetched: true,
      flags: ["\\Seen"],
    };
    const newerSelection = {
      ...summary("reply-race-newer", "Newer manual selection"),
      uid: undefined,
      body_text: "Newer selected body",
      body_fetched: true,
      flags: ["\\Seen"],
    };
    desktop.fixtures.inboxPageSource.mockResolvedValue([
      replySource,
      newerSelection,
    ]);
    const pendingReply = deferred();
    desktop.mailApi.prepareReply.mockReturnValue(pendingReply.promise);
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole("button", {
        name: /打开邮件：.*Reply race source/,
      }),
    );
    await user.click(screen.getByRole("button", { name: "回复" }));
    await waitFor(() =>
      expect(desktop.mailApi.prepareReply).toHaveBeenCalledWith(
        "reply-race-source",
      ),
    );
    await user.click(
      screen.getByRole("button", {
        name: /打开邮件：.*Newer manual selection/,
      }),
    );

    await act(async () => {
      pendingReply.resolve({
        to: ["sender@example.com"],
        cc: [],
        bcc: [],
        subject: "Re: must stay stale",
        body_text: "",
        reply_context: null,
      });
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(
      screen.getByRole("heading", { name: "Newer manual selection" }),
    ).toBeTruthy();
    expect(screen.getByText("Newer selected body")).toBeTruthy();
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
    });
  });

  it("never prepares a reply for a mailbox row without a string public id", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 600,
    });
    desktop.fixtures.inboxPageSource.mockResolvedValue([
      {
        ...summary(99, "Malformed numeric message"),
        id: 99,
        body_text: "Malformed body",
        body_fetched: true,
      },
    ]);
    render(<App />);

    expect(await screen.findByText("Malformed numeric message")).toBeTruthy();
    expect(
      screen.queryByRole("button", {
        name: /打开邮件：.*Malformed numeric message/,
      }),
    ).toBeNull();
    expect(desktop.mailApi.prepareReply).not.toHaveBeenCalled();
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1024,
    });
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

    await waitFor(() =>
      expect(desktop.mailApi.completeExit).toHaveBeenCalledOnce(),
    );
    expect(desktop.mailApi.completeExit).toHaveBeenCalledWith(101);
    expect(desktop.mailApi.saveDraft).toHaveBeenCalledWith(
      expect.objectContaining({ subject: "退出前必须保存" }),
      null,
      null,
    );
    expect(desktop.mailApi.saveDraft.mock.invocationCallOrder[0]).toBeLessThan(
      desktop.mailApi.completeExit.mock.invocationCallOrder[0],
    );
  });

  it("cancels a failed exit flush, unlocks editing, and allows a second exit", async () => {
    desktop.mailApi.saveDraft
      .mockRejectedValueOnce(new Error("SQLite write failed"))
      .mockImplementationOnce(
        async (request, draftId, expectedLocalVersion) => {
          const outcome = savedOutcome(request, draftId, expectedLocalVersion);
          return {
            ...outcome,
            draft: { ...outcome.draft, id: draftId || "recovered-draft" },
          };
        },
      );
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
    expect(screen.getByText(/退出前保存草稿失败/)).toBeTruthy();

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
      expect(screen.getByText(/无法完成安全退出/)).toBeTruthy(),
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
    const refreshed = draftSnapshot(
      2,
      "Edited on another client",
      "Remote body",
    );
    desktop.mailApi.listDrafts.mockResolvedValue([original]);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");
    await waitFor(() =>
      expect(desktop.listeners.has("mail:drafts-updated")).toBe(true),
    );

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Original subject"));
    expect(screen.getByLabelText("主题").value).toBe("Original subject");
    expect(document.querySelector(".compose-save-state")?.dataset.state).toBe(
      "saved",
    );

    desktop.mailApi.listDrafts.mockResolvedValue([refreshed]);
    act(() => {
      desktop.listeners.get("mail:drafts-updated")?.({
        payload: { reason: "sync" },
      });
    });

    await waitFor(() =>
      expect(screen.getByLabelText("主题").value).toBe(
        "Edited on another client",
      ),
    );
    expect(screen.getByLabelText("邮件正文").textContent).toBe("Remote body");
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
    expect(
      screen.getByText("含无法安全编辑的 HTML 或附件，已保持只读"),
    ).toBeTruthy();
    expect(screen.getByLabelText("收件人").disabled).toBe(true);
    expect(screen.getByLabelText("主题").disabled).toBe(true);
    expect(screen.getByLabelText("邮件正文").getAttribute("aria-readonly")).toBe(
      "true",
    );
    expect(screen.getByRole("button", { name: "发送邮件" }).disabled).toBe(
      true,
    );
    expect(screen.getByRole("button", { name: "保存并最小化" }).disabled).toBe(
      true,
    );
    expect(screen.getByRole("button", { name: "丢弃草稿" }).disabled).toBe(
      true,
    );

    minimizeComposer();
    await user.click(screen.getByRole("button", { name: "关闭写信窗口" }));
    expect(screen.queryByRole("heading", { name: "查看草稿" })).toBeNull();
    expect(desktop.mailApi.saveDraft).not.toHaveBeenCalled();
    expect(desktop.mailApi.deleteDraft).not.toHaveBeenCalled();
    expect(desktop.mailApi.sendDraft).not.toHaveBeenCalled();
  });

  it("creates a stable blank draft before the first attachment picker and keeps it on cancel", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    await user.click(screen.getByRole("button", { name: "添加附件" }));

    expect(await screen.findByText("已取消添加附件")).toBeTruthy();
    expect(desktop.mailApi.saveDraft).not.toHaveBeenCalled();
    expect(desktop.mailApi.createComposeDraft).toHaveBeenCalledOnce();
    expect(desktop.mailApi.addDraftAttachments).toHaveBeenCalledWith(
      "blank-compose-draft",
      1,
    );
    expect(
      desktop.mailApi.createComposeDraft.mock.invocationCallOrder[0],
    ).toBeLessThan(
      desktop.mailApi.addDraftAttachments.mock.invocationCallOrder[0],
    );
    expect(document.querySelector(".compose-save-state")?.dataset.state).toBe(
      "saved",
    );
  });

  it("force-saves quickly entered content before opening the first attachment picker", async () => {
    const saveBarrier = deferred();
    let stableDraft = null;
    desktop.mailApi.saveDraft.mockImplementation(
      async (request, draftId, expectedLocalVersion) => {
        await saveBarrier.promise;
        const outcome = savedOutcome(request, draftId, expectedLocalVersion);
        stableDraft = {
          ...outcome.draft,
          attachments: [],
          forward_context: null,
        };
        return { ...outcome, draft: stableDraft };
      },
    );
    desktop.mailApi.addDraftAttachments.mockImplementation(
      async (draftId, expectedLocalVersion) => ({
        kind: "saved",
        draft: {
          ...stableDraft,
          id: draftId,
          local_version: expectedLocalVersion + 1,
          attachments: [
            {
              id: "managed-fast",
              name: "fast.txt",
              mime_type: "text/plain",
              size_bytes: 9,
            },
          ],
        },
      }),
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "刚刚输入的主题" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加附件" }));

    await waitFor(() => expect(desktop.mailApi.saveDraft).toHaveBeenCalledOnce());
    expect(screen.getByLabelText("主题").disabled).toBe(true);
    expect(desktop.mailApi.createComposeDraft).not.toHaveBeenCalled();
    expect(desktop.mailApi.addDraftAttachments).not.toHaveBeenCalled();

    await act(async () => {
      saveBarrier.resolve();
    });
    await waitFor(() =>
      expect(desktop.mailApi.addDraftAttachments).toHaveBeenCalledWith(
        "exit-draft",
        1,
      ),
    );
    expect(desktop.mailApi.saveDraft).toHaveBeenCalledWith(
      expect.objectContaining({ subject: "刚刚输入的主题" }),
      null,
      null,
    );
    expect(
      desktop.mailApi.saveDraft.mock.invocationCallOrder[0],
    ).toBeLessThan(
      desktop.mailApi.addDraftAttachments.mock.invocationCallOrder[0],
    );
    expect(screen.getByLabelText("主题").value).toBe("刚刚输入的主题");
    expect(await screen.findByText("fast.txt")).toBeTruthy();
  });

  it("keeps a dirty stable draft in error state when attachment stabilization fails", async () => {
    desktop.mailApi.listDrafts.mockResolvedValue([
      {
        ...draftSnapshot(1, "Stable draft"),
        attachments: [],
        forward_context: null,
      },
    ]);
    desktop.mailApi.saveDraft.mockRejectedValueOnce(
      new Error("local write failed"),
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Stable draft"));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "尚未稳定的新主题" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加附件" }));

    expect(await screen.findByText("添加附件失败，请重试")).toBeTruthy();
    expect(screen.getByLabelText("主题").value).toBe("尚未稳定的新主题");
    expect(screen.getByLabelText("主题").disabled).toBe(false);
    expect(document.querySelector(".compose-save-state")?.dataset.state).toBe(
      "error",
    );
    expect(desktop.mailApi.addDraftAttachments).not.toHaveBeenCalled();
    expect(desktop.mailApi.createComposeDraft).not.toHaveBeenCalled();
    expect(desktop.mailApi.saveDraft).toHaveBeenCalledWith(
      expect.objectContaining({ subject: "尚未稳定的新主题" }),
      "shared-draft",
      1,
    );
  });

  it("uses the force-saved version for removal and switches exactly to a stale returned draft", async () => {
    const attachment = {
      id: "managed-remove",
      name: "keep-me.pdf",
      mime_type: "application/pdf",
      size_bytes: 2048,
    };
    const original = {
      ...draftSnapshot(1, "Attachment draft"),
      attachments: [attachment],
      forward_context: null,
    };
    desktop.mailApi.listDrafts.mockResolvedValue([original]);
    desktop.mailApi.saveDraft.mockImplementation(async (request) => ({
      kind: "saved",
      draft: {
        ...original,
        ...request,
        local_version: 2,
      },
      canonical: null,
    }));
    desktop.mailApi.removeDraftAttachment.mockResolvedValue({
      kind: "stale",
      draft: {
        ...original,
        local_version: 4,
        subject: "其他客户端的最新主题",
      },
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Attachment draft"));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "准备移除附件" },
    });
    await user.click(
      screen.getByRole("button", { name: "移除附件 keep-me.pdf" }),
    );

    await waitFor(() =>
      expect(desktop.mailApi.removeDraftAttachment).toHaveBeenCalledWith(
        "shared-draft",
        "managed-remove",
        2,
      ),
    );
    expect(screen.getByLabelText("主题").value).toBe(
      "其他客户端的最新主题",
    );
    expect(screen.getByText("keep-me.pdf")).toBeTruthy();
    expect(
      screen.getByText("草稿已更新，本次操作未生效。请在最新版本重试"),
    ).toBeTruthy();
    expect(document.querySelector(".compose-save-state")?.dataset.state).toBe(
      "saved",
    );
  });

  it("adopts an attachment conflict copy exactly and binds later operations to that copy", async () => {
    const conflictAttachment = {
      id: "managed-conflict",
      name: "conflict.pdf",
      mime_type: "application/pdf",
      size_bytes: 4096,
    };
    const original = {
      ...draftSnapshot(1, "Original attachment draft"),
      attachments: [],
      forward_context: null,
    };
    const canonical = {
      ...original,
      local_version: 6,
      subject: "Canonical server draft",
      body_text: "Canonical body",
    };
    const conflictCopy = {
      ...original,
      id: "attachment-conflict-copy",
      local_version: 7,
      subject: "Exact conflict copy",
      body_text: "Exact conflict body",
      attachments: [conflictAttachment],
      status: "conflict",
    };
    desktop.mailApi.listDrafts.mockResolvedValue([original]);
    desktop.mailApi.saveDraft.mockImplementation(
      async (request, draftId, expectedLocalVersion) => {
        if (draftId === "attachment-conflict-copy") {
          return {
            kind: "saved",
            draft: {
              ...conflictCopy,
              ...request,
              id: draftId,
              local_version: expectedLocalVersion + 1,
              attachments: [conflictAttachment],
            },
            canonical: null,
          };
        }
        return {
          kind: "saved",
          draft: {
            ...original,
            ...request,
            id: draftId,
            local_version: expectedLocalVersion + 1,
          },
          canonical: null,
        };
      },
    );
    desktop.mailApi.addDraftAttachments.mockResolvedValue({
      kind: "conflict_copy",
      draft: conflictCopy,
      canonical,
    });
    desktop.mailApi.removeDraftAttachment.mockImplementation(
      async (draftId, attachmentId, expectedLocalVersion) => ({
        kind: "saved",
        draft: {
          ...conflictCopy,
          id: draftId,
          local_version: expectedLocalVersion + 1,
          subject: "Conflict copy after removal",
          body_text: "Exact post-removal body",
          attachments: [],
        },
      }),
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Original attachment draft"));
    await user.click(screen.getByRole("button", { name: "添加附件" }));

    await waitFor(() =>
      expect(desktop.mailApi.addDraftAttachments).toHaveBeenCalledWith(
        "shared-draft",
        2,
      ),
    );
    expect(screen.getByLabelText("主题").value).toBe("Exact conflict copy");
    expect(
      (await screen.findByLabelText("邮件正文")).textContent,
    ).toContain("Exact conflict body");
    expect(await screen.findByText("conflict.pdf")).toBeTruthy();
    expect(
      screen.getByText(
        "附件已保存在新的冲突副本中，未覆盖其他客户端的最新草稿。",
      ),
    ).toBeTruthy();

    await user.click(
      screen.getByRole("button", { name: "移除附件 conflict.pdf" }),
    );
    await waitFor(() =>
      expect(desktop.mailApi.saveDraft).toHaveBeenLastCalledWith(
        expect.objectContaining({
          subject: "Exact conflict copy",
          body_text: "Exact conflict body",
        }),
        "attachment-conflict-copy",
        7,
      ),
    );
    await waitFor(() =>
      expect(desktop.mailApi.removeDraftAttachment).toHaveBeenCalledWith(
        "attachment-conflict-copy",
        "managed-conflict",
        8,
      ),
    );
    expect(screen.getByLabelText("主题").value).toBe(
      "Conflict copy after removal",
    );
    expect(screen.queryByText("conflict.pdf")).toBeNull();

    minimizeComposer();
    await user.click(screen.getByRole("button", { name: "关闭写信窗口" }));
    expect(await screen.findByText("Canonical server draft")).toBeTruthy();
    expect(screen.getByText("Conflict copy after removal")).toBeTruthy();
  });

  it("closes a new dirty composer without creating a draft", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /写信/ }));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "不应保存的临时内容" },
    });
    minimizeComposer();
    fireEvent.click(screen.getByRole("button", { name: "关闭写信窗口" }));

    expect(
      screen.queryByRole("dialog", { name: "不应保存的临时内容" }),
    ).toBeNull();
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

    minimizeComposer();
    fireEvent.click(screen.getByRole("button", { name: "关闭写信窗口" }));

    await waitFor(() =>
      expect(desktop.mailApi.deleteDraft).toHaveBeenCalledWith("exit-draft", 1),
    );
    expect(desktop.mailApi.saveDraft).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByRole("dialog", { name: "已自动保存的临时内容" }),
    ).toBeNull();
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
    minimizeComposer();
    fireEvent.click(screen.getByRole("button", { name: "关闭写信窗口" }));

    expect(screen.queryByRole("heading", { name: "编辑草稿" })).toBeNull();
    expect(desktop.mailApi.saveDraft).not.toHaveBeenCalled();
    expect(desktop.mailApi.deleteDraft).not.toHaveBeenCalled();
  });

  it("switches from a minimized draft after preserving its unsaved edit", async () => {
    const first = {
      ...draftSnapshot(1, "First draft", "First body"),
      id: "first-draft",
    };
    const second = {
      ...draftSnapshot(3, "Second draft", "Second body"),
      id: "second-draft",
    };
    desktop.mailApi.listDrafts.mockResolvedValue([first, second]);
    desktop.mailApi.saveDraft.mockImplementation(
      async (request, draftId, expectedLocalVersion) =>
        savedOutcome(request, draftId, expectedLocalVersion),
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(await screen.findByText("First draft"));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "Unsaved first edit" },
    });
    minimizeComposer();
    expect(
      screen.getByRole("dialog", {
        name: "Unsaved first edit(friend@example.com)",
      }).dataset.minimized,
    ).toBe("true");

    await user.click(screen.getByText("Second draft"));

    await waitFor(() =>
      expect(desktop.mailApi.saveDraft).toHaveBeenCalledWith(
        expect.objectContaining({ subject: "Unsaved first edit" }),
        "first-draft",
        1,
      ),
    );
    const composer = await screen.findByRole("dialog", {
      name: "编辑草稿",
    });
    expect(composer.dataset.minimized).toBe("false");
    expect(within(composer).getByLabelText("主题").value).toBe("Second draft");
    expect(
      (await within(composer).findByLabelText("邮件正文")).textContent,
    ).toBe("Second body");
  });

  it("keeps the minimized draft when saving before a switch fails", async () => {
    const first = {
      ...draftSnapshot(1, "First draft", "First body"),
      id: "first-draft",
    };
    const second = {
      ...draftSnapshot(3, "Second draft", "Second body"),
      id: "second-draft",
    };
    desktop.mailApi.listDrafts.mockResolvedValue([first, second]);
    desktop.mailApi.saveDraft.mockRejectedValue(new Error("local write failed"));
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(await screen.findByText("First draft"));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "Keep this edit" },
    });
    minimizeComposer();
    await user.click(screen.getByText("Second draft"));

    expect((await screen.findByRole("alert")).textContent).toContain(
      "Mine Mail 内部处理失败：切换草稿前未能保存当前编辑，请重试；如果仍然失败，请重启应用。",
    );
    const composer = screen.getByRole("dialog", {
      name: "Keep this edit(friend@example.com)",
    });
    expect(composer.dataset.minimized).toBe("true");
    expect(screen.queryByRole("dialog", { name: "编辑草稿" })).toBeNull();
  });

  it("preserves a dirty stale edit as a conflict copy", async () => {
    const original = draftSnapshot(1, "Original subject");
    const canonical = draftSnapshot(2, "New canonical", "Canonical body");
    const conflictCopy = {
      ...draftSnapshot(1, "My offline edit", "Draft body"),
      id: "conflict-copy",
      status: "conflict",
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
    await waitFor(() =>
      expect(desktop.listeners.has("mail:drafts-updated")).toBe(true),
    );
    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Original subject"));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "My offline edit" },
    });

    desktop.mailApi.listDrafts.mockResolvedValue([canonical]);
    act(() => {
      desktop.listeners.get("mail:drafts-updated")?.({
        payload: { reason: "sync" },
      });
    });
    await waitFor(() =>
      expect(desktop.mailApi.listDrafts).toHaveBeenCalledTimes(2),
    );
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
    expect((await screen.findByRole("alert")).textContent).toContain(
      "冲突副本",
    );
    expect(screen.getByLabelText("主题").value).toBe("My offline edit");
  });

  it("preserves a dirty edit when the canonical draft was deleted", async () => {
    const original = draftSnapshot(1, "Original subject");
    const conflictCopy = {
      ...draftSnapshot(1, "Edit after delete"),
      id: "deleted-base-copy",
      status: "conflict",
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
    await waitFor(() =>
      expect(desktop.listeners.has("mail:drafts-updated")).toBe(true),
    );
    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Original subject"));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "Edit after delete" },
    });

    desktop.mailApi.listDrafts.mockResolvedValue([]);
    act(() => {
      desktop.listeners.get("mail:drafts-updated")?.({
        payload: { reason: "sync" },
      });
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
    expect((await screen.findByRole("alert")).textContent).toContain(
      "冲突副本",
    );
  });

  it("closes a stale discard without deleting the newer canonical", async () => {
    const original = draftSnapshot(1, "Original subject");
    const canonical = draftSnapshot(2, "New canonical");
    desktop.mailApi.listDrafts.mockResolvedValue([original]);
    desktop.mailApi.deleteDraft.mockResolvedValue({ kind: "stale" });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");
    await waitFor(() =>
      expect(desktop.listeners.has("mail:drafts-updated")).toBe(true),
    );
    await user.click(screen.getByRole("button", { name: /草稿/ }));
    await user.click(screen.getByText("Original subject"));
    fireEvent.change(screen.getByLabelText("主题"), {
      target: { value: "Discard this stale edit" },
    });
    desktop.mailApi.listDrafts.mockResolvedValue([canonical]);

    await user.click(screen.getByRole("button", { name: "丢弃草稿" }));

    await waitFor(() =>
      expect(desktop.mailApi.deleteDraft).toHaveBeenCalledWith(
        "shared-draft",
        1,
      ),
    );
    expect(screen.queryByRole("heading", { name: "编辑草稿" })).toBeNull();
    expect((await screen.findByRole("alert")).textContent).toContain(
      "没有删除最新版本",
    );
    expect(await screen.findByText("New canonical")).toBeTruthy();
  });

  it("renders Outbox recipient groups exactly without inferring from flat recipients", async () => {
    desktop.mailApi.listOutbox.mockResolvedValue([
      {
        id: "grouped-outbox",
        draft_id: null,
        recipients: ["legacy-flat@example.com"],
        recipient_groups: {
          to: ["to@example.com"],
          cc: ["cc@example.com"],
          bcc: ["bcc@example.com"],
        },
        subject: "Grouped Outbox message",
        status: "retryable",
        attempts: 1,
        last_error: "Temporary failure",
        created_at: "2026-07-14T09:00:00Z",
        sent_at: null,
      },
    ]);
    desktop.mailApi.fetchOutboxMessage.mockResolvedValue({
      id: "grouped-outbox",
      subject: "Grouped Outbox message",
      body_text: "Grouped Outbox body",
      body_fetched: true,
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByText("发件队列"));
    await user.click(screen.getByText("Grouped Outbox message"));
    const reader = screen.getByLabelText("邮件阅读区");
    await user.click(
      within(reader).getByRole("button", { name: "查看收件人" }),
    );
    const details = within(reader).getByRole("region", {
      name: "收件人详情",
    });
    expect(within(details).getByText("me@163.com")).toBeTruthy();
    expect(within(details).getByText("to@example.com")).toBeTruthy();
    expect(within(details).getByText("cc@example.com")).toBeTruthy();
    expect(within(details).getByText("bcc@example.com")).toBeTruthy();
    expect(within(details).queryByText("legacy-flat@example.com")).toBeNull();
    expect(
      within(reader).queryByText("旧版邮件收件人分组不可用"),
    ).toBeNull();
  });

  it("cancels a delivery-unknown decision without changing Outbox state", async () => {
    const unknown = deliveryUnknownOutbox();
    desktop.mailApi.listOutbox.mockResolvedValue([unknown]);
    desktop.mailApi.fetchOutboxMessage.mockResolvedValue({
      id: unknown.id,
      subject: unknown.subject,
      body_text: "Unknown delivery body",
      body_fetched: true,
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByText("发件队列"));
    const list = await screen.findByLabelText("发件队列邮件列表");
    await user.click(await within(list).findByText(unknown.subject));
    await user.click(screen.getByRole("button", { name: "确认已投递" }));
    const dialog = screen.getByRole("alertdialog", {
      name: "确认这封邮件已投递？",
    });
    await user.click(within(dialog).getByRole("button", { name: "取消" }));

    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(desktop.mailApi.resolveDeliveryUnknown).not.toHaveBeenCalled();
    expect(desktop.mailApi.retryOutbox).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "确认已投递" })).toBeTruthy();
  });

  it("confirms an externally verified delivery with the reviewed attempt generation", async () => {
    const unknown = deliveryUnknownOutbox();
    const delivered = {
      ...unknown,
      status: "sent",
      last_error: null,
      sent_at: "2026-07-14T09:01:00Z",
    };
    desktop.mailApi.listOutbox
      .mockResolvedValueOnce([unknown])
      .mockResolvedValue([]);
    desktop.mailApi.listSentOutboxFallbacks
      .mockResolvedValueOnce([])
      .mockResolvedValue([delivered]);
    desktop.mailApi.fetchOutboxMessage.mockResolvedValue({
      id: unknown.id,
      subject: unknown.subject,
      body_text: "Unknown delivery body",
      body_fetched: true,
    });
    desktop.mailApi.resolveDeliveryUnknown.mockResolvedValue(delivered);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByText("发件队列"));
    const list = await screen.findByLabelText("发件队列邮件列表");
    await user.click(await within(list).findByText(unknown.subject));
    await user.click(screen.getByRole("button", { name: "确认已投递" }));
    const dialog = screen.getByRole("alertdialog", {
      name: "确认这封邮件已投递？",
    });
    await user.click(
      within(dialog).getByRole("button", { name: "确认已投递" }),
    );

    await waitFor(() =>
      expect(desktop.mailApi.resolveDeliveryUnknown).toHaveBeenCalledWith({
        outboxId: "outbox-unknown",
        expectedAttempts: 2,
        decision: "confirm_delivered",
        acknowledgeDuplicateRisk: false,
      }),
    );
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(screen.queryByLabelText("邮件阅读区")).toBeNull();
    expect(
      within(screen.getByLabelText("发件队列邮件列表")).queryByText(
        unknown.subject,
      ),
    ).toBeNull();

    await user.click(screen.getByRole("button", { name: /已发送/ }));
    expect(
      within(screen.getByLabelText("已发送邮件列表")).getByText(
        unknown.subject,
      ),
    ).toBeTruthy();
    expect(desktop.mailApi.retryOutbox).not.toHaveBeenCalled();
  });

  it("requires explicit duplicate-risk acknowledgement for one delivery-unknown retry", async () => {
    const unknown = deliveryUnknownOutbox();
    const retryResult = deferred();
    desktop.mailApi.listOutbox.mockResolvedValue([unknown]);
    desktop.mailApi.fetchOutboxMessage.mockResolvedValue({
      id: unknown.id,
      subject: unknown.subject,
      body_text: "Unknown delivery body",
      body_fetched: true,
    });
    desktop.mailApi.resolveDeliveryUnknown.mockReturnValue(retryResult.promise);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByText("发件队列"));
    const list = await screen.findByLabelText("发件队列邮件列表");
    await user.click(await within(list).findByText(unknown.subject));
    await user.click(screen.getByRole("button", { name: "仍要重试" }));
    const dialog = screen.getByRole("alertdialog", {
      name: "仍要重试这封邮件？",
    });
    expect(within(dialog).getByText(/可能收到重复邮件/)).toBeTruthy();
    await user.click(
      within(dialog).getByRole("button", { name: "承担风险并重试" }),
    );

    await waitFor(() =>
      expect(desktop.mailApi.resolveDeliveryUnknown).toHaveBeenCalledWith({
        outboxId: "outbox-unknown",
        expectedAttempts: 2,
        decision: "retry_once",
        acknowledgeDuplicateRisk: true,
      }),
    );
    expect(
      within(dialog).getByRole("button", { name: "正在明确重试…" }).disabled,
    ).toBe(true);
    expect(desktop.mailApi.retryOutbox).not.toHaveBeenCalled();

    await act(async () => {
      retryResult.resolve({
        ...unknown,
        status: "sent",
        attempts: 3,
        last_error: null,
        sent_at: "2026-07-14T09:02:00Z",
      });
    });
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(screen.queryByLabelText("邮件阅读区")).toBeNull();
  });

  it("refreshes a stale delivery-unknown generation and requires a new review", async () => {
    const unknown = deliveryUnknownOutbox();
    const refreshed = deliveryUnknownOutbox({
      attempts: 3,
      last_error: "A newer ambiguous attempt exists",
    });
    desktop.mailApi.listOutbox.mockResolvedValue([unknown]);
    desktop.mailApi.fetchOutboxMessage.mockResolvedValue({
      id: unknown.id,
      subject: unknown.subject,
      body_text: "Unknown delivery body",
      body_fetched: true,
    });
    desktop.mailApi.resolveDeliveryUnknown.mockRejectedValue(
      new Error("refresh before deciding again"),
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByText("发件队列"));
    const list = await screen.findByLabelText("发件队列邮件列表");
    await user.click(await within(list).findByText(unknown.subject));
    desktop.mailApi.listOutbox.mockResolvedValue([refreshed]);
    await user.click(screen.getByRole("button", { name: "确认已投递" }));
    const dialog = screen.getByRole("alertdialog", {
      name: "确认这封邮件已投递？",
    });
    await user.click(
      within(dialog).getByRole("button", { name: "确认已投递" }),
    );

    expect(
      await within(dialog).findByText(
        "发件队列状态已刷新。请取消后查看最新状态，再重新选择操作。",
      ),
    ).toBeTruthy();
    expect(
      within(dialog).getByRole("button", { name: "确认已投递" }).disabled,
    ).toBe(true);
    expect(
      within(screen.getByLabelText("邮件阅读区")).getByText(
        "说明：投递结果仍待确认，请先到邮箱服务商的“已发送”文件夹核对。",
      ),
    ).toBeTruthy();
    expect(desktop.mailApi.resolveDeliveryUnknown).toHaveBeenCalledOnce();
    expect(desktop.mailApi.retryOutbox).not.toHaveBeenCalled();
  });

  it("keeps the unknown Outbox item and confirmation available after a command failure", async () => {
    const unknown = deliveryUnknownOutbox();
    desktop.mailApi.listOutbox.mockResolvedValue([unknown]);
    desktop.mailApi.fetchOutboxMessage.mockResolvedValue({
      id: unknown.id,
      subject: unknown.subject,
      body_text: "Unknown delivery body",
      body_fetched: true,
    });
    desktop.mailApi.resolveDeliveryUnknown.mockRejectedValue(
      new Error("Desktop bridge unavailable"),
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByText("发件队列"));
    const list = await screen.findByLabelText("发件队列邮件列表");
    await user.click(await within(list).findByText(unknown.subject));
    await user.click(screen.getByRole("button", { name: "确认已投递" }));
    const dialog = screen.getByRole("alertdialog", {
      name: "确认这封邮件已投递？",
    });
    await user.click(
      within(dialog).getByRole("button", { name: "确认已投递" }),
    );

    expect(
      await within(dialog).findByText(
        "Mine Mail 内部处理失败：未能处理投递结果。请重试；如果仍然失败，请重启应用。邮件仍保留在发件队列。",
      ),
    ).toBeTruthy();
    expect(
      within(dialog).getByRole("button", { name: "确认已投递" }).disabled,
    ).toBe(false);
    expect(
      within(screen.getByLabelText("邮件阅读区")).getByText(
        "Unknown delivery body",
      ),
    ).toBeTruthy();
    expect(desktop.mailApi.resolveDeliveryUnknown).toHaveBeenCalledOnce();
    expect(desktop.mailApi.retryOutbox).not.toHaveBeenCalled();
  });

  it("never resolves delivery-unknown state from a malformed attempts value", async () => {
    const malformed = deliveryUnknownOutbox({ attempts: "2" });
    desktop.mailApi.listOutbox.mockResolvedValue([malformed]);
    desktop.mailApi.fetchOutboxMessage.mockResolvedValue({
      id: malformed.id,
      subject: malformed.subject,
      body_text: "Malformed unknown body",
      body_fetched: true,
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByText("发件队列"));
    const list = await screen.findByLabelText("发件队列邮件列表");
    await user.click(await within(list).findByText(malformed.subject));

    expect(screen.queryByRole("button", { name: "确认已投递" })).toBeNull();
    expect(screen.queryByRole("button", { name: "仍要重试" })).toBeNull();
    expect(screen.getByText("发件队列记录不完整，请刷新后再处理。")).toBeTruthy();
    expect(desktop.mailApi.resolveDeliveryUnknown).not.toHaveBeenCalled();
    expect(desktop.mailApi.retryOutbox).not.toHaveBeenCalled();
  });

  it("never opens or resolves a delivery-unknown item with a malformed id", async () => {
    const malformed = deliveryUnknownOutbox({ id: "  " });
    desktop.mailApi.listOutbox.mockResolvedValue([malformed]);
    const user = userEvent.setup();
    render(<App />);
    await screen.findAllByText("First mail");

    await user.click(screen.getByText("发件队列"));
    const list = await screen.findByLabelText("发件队列邮件列表");
    expect(await within(list).findByText(malformed.subject)).toBeTruthy();
    expect(
      within(list).queryByRole("button", {
        name: /打开邮件：.*Unknown delivery message/,
      }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "确认已投递" })).toBeNull();
    expect(screen.queryByRole("button", { name: "仍要重试" })).toBeNull();
    expect(desktop.mailApi.resolveDeliveryUnknown).not.toHaveBeenCalled();
    expect(desktop.mailApi.retryOutbox).not.toHaveBeenCalled();
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
    await user.click(screen.getByText("first@example.com"));
    await user.click(screen.getByRole("button", { name: "重试发送" }));
    await user.click(screen.getByText("second@example.com"));

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
    expect(
      screen.getByText(
        "说明：邮箱服务器拒绝了这次投递，请检查收件人和账户设置。",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "重试发送" })).toBeNull();
  });
});
