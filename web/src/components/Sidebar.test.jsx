import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Sidebar } from "./Sidebar.jsx";

const accounts = [
  {
    accountId: "netease",
    provider: "163",
    email: "first@163.com",
  },
  {
    accountId: "google",
    provider: "gmail",
    email: "second@gmail.com",
  },
  {
    accountId: "custom",
    provider: "custom",
    email: "third@example.com",
  },
];

function renderSidebar(accountCount, overrides = {}) {
  const onAccountSwitch = vi.fn();
  const onAddAccount = vi.fn();
  const baseProps = {
    activeFolder: "inbox",
    onFolderChange: vi.fn(),
    onCompose: vi.fn(),
    theme: "dusk",
    onThemeChange: vi.fn(),
    isThemeMenuOpen: false,
    onThemeMenuToggle: vi.fn(),
    counts: {},
    accountStatus: {
      configured: true,
      accounts: accounts.slice(0, accountCount),
      activeAccountId: accounts[0]?.accountId,
      maxAccounts: 3,
    },
    accountAvatarFor: vi.fn(() => null),
    onAccountSwitch,
    onAddAccount,
    onOpenSettings: vi.fn(),
  };
  const renderResult = render(<Sidebar {...baseProps} {...overrides} />);
  const rerenderSidebar = (nextOverrides = {}) =>
    renderResult.rerender(
      <Sidebar {...baseProps} {...overrides} {...nextOverrides} />,
    );
  return { onAccountSwitch, onAddAccount, rerenderSidebar };
}

describe("Sidebar account switcher", () => {
  afterEach(cleanup);

  it("keeps the compose button free of keyboard shortcut chrome", () => {
    renderSidebar(1);

    expect(screen.getByRole("button", { name: "写信" }).querySelector("kbd")).toBeNull();
  });

  it("opens the contacts workspace from the primary navigation", async () => {
    const user = userEvent.setup();
    const onFolderChange = vi.fn();
    renderSidebar(1, { onFolderChange });

    await user.click(screen.getByRole("button", { name: "通讯录" }));

    expect(onFolderChange).toHaveBeenCalledWith("contacts");
  });

  it("keeps disclosure semantics while hiding selection for a retracted list", () => {
    const { rerenderSidebar } = renderSidebar(1);
    const inbox = screen.getByRole("button", { name: "收件箱" });
    const selection = screen
      .getByRole("navigation", { name: "邮箱文件夹" })
      .querySelector(".folder-nav__selection");

    expect(inbox.getAttribute("aria-controls")).toBe("mail-list-panel");
    expect(inbox.getAttribute("aria-expanded")).toBe("true");
    expect(inbox.dataset.selected).toBe("true");

    rerenderSidebar({
      isMailListExpanded: false,
      isFolderSelectionVisible: false,
    });

    expect(inbox.getAttribute("aria-controls")).toBe("mail-list-panel");
    expect(inbox.getAttribute("aria-expanded")).toBe("false");
    expect(inbox.dataset.selected).toBe("false");
    expect(inbox.hasAttribute("aria-current")).toBe(false);
    expect(selection.dataset.visible).toBeUndefined();
  });

  it("omits disclosure semantics from settings and includes Contacts", () => {
    const { rerenderSidebar } = renderSidebar(1, { isSettingsOpen: true });
    const inbox = screen.getByRole("button", { name: "收件箱" });
    const settings = screen.getByRole("button", { name: "设置" });

    expect(inbox.hasAttribute("aria-controls")).toBe(false);
    expect(inbox.hasAttribute("aria-expanded")).toBe(false);
    expect(settings.hasAttribute("aria-controls")).toBe(false);
    expect(settings.hasAttribute("aria-expanded")).toBe(false);

    rerenderSidebar({ activeFolder: "contacts", isSettingsOpen: false });

    const contacts = screen.getByRole("button", { name: "通讯录" });
    expect(contacts.getAttribute("aria-controls")).toBe("mail-list-panel");
    expect(contacts.getAttribute("aria-expanded")).toBe("true");
  });

  it("moves one shared selection surface to the active folder", () => {
    const rect = (top, left, width, height) => ({
      bottom: top + height,
      height,
      left,
      right: left + width,
      top,
      width,
      x: left,
      y: top,
      toJSON: () => ({}),
    });
    const boundsSpy = vi
      .spyOn(Element.prototype, "getBoundingClientRect")
      .mockImplementation(function getBoundingClientRect() {
        if (this.classList?.contains("folder-nav")) {
          return rect(80, 20, 232, 373);
        }
        if (this.textContent?.includes("收件箱")) {
          return rect(80, 20, 232, 44);
        }
        if (this.textContent?.includes("已收藏")) {
          return rect(127, 20, 232, 44);
        }
        return rect(0, 0, 0, 0);
      });

    try {
      const { rerenderSidebar } = renderSidebar(1);
      const navigation = screen.getByRole("navigation", {
        name: "邮箱文件夹",
      });
      const selection = navigation.querySelector(".folder-nav__selection");

      expect(
        navigation.querySelectorAll(".folder-nav__selection"),
      ).toHaveLength(1);
      expect(selection.getAttribute("aria-hidden")).toBe("true");
      expect(selection.dataset.visible).toBe("true");
      expect(
        selection.style.getPropertyValue("--sliding-selection-y"),
      ).toBe("0px");

      rerenderSidebar({ activeFolder: "starred" });

      expect(
        selection.style.getPropertyValue("--sliding-selection-y"),
      ).toBe("47px");
      expect(
        screen.getByRole("button", { name: "已收藏" }).dataset.selected,
      ).toBe("true");

      rerenderSidebar({
        activeFolder: "starred",
        isFolderSelectionVisible: false,
      });

      expect(selection.dataset.visible).toBeUndefined();
      expect(
        screen.getByRole("button", { name: "已收藏" }).dataset.selected,
      ).toBe("false");

      rerenderSidebar({
        activeFolder: "starred",
        isFolderSelectionVisible: true,
      });

      expect(selection.dataset.visible).toBe("true");
      expect(
        screen.getByRole("button", { name: "已收藏" }).dataset.selected,
      ).toBe("true");
    } finally {
      boundsSpy.mockRestore();
    }
  });

  it("shows counts only for the inbox and outbox", () => {
    renderSidebar(1, {
      counts: {
        inbox: 5,
        starred: 4,
        contacts: 3,
        sent: 104,
        drafts: 15,
        outbox: 2,
        archive: 6,
        trash: 7,
      },
    });

    expect(
      screen.getByText("收件箱").closest("button").querySelector(".folder-nav__count")
        ?.textContent,
    ).toBe("5");
    expect(
      screen
        .getByText("发件队列")
        .closest("button")
        .querySelector(".folder-nav__count")?.textContent,
    ).toBe("2");

    for (const label of [
      "已收藏",
      "通讯录",
      "已发送",
      "草稿",
      "归档",
      "垃圾箱",
    ]) {
      expect(
        screen.getByText(label).closest("button").querySelector(".folder-nav__count"),
      ).toBeNull();
    }
  });

  it("keeps account and settings controls in a dedicated footer region", () => {
    renderSidebar(1);

    const accountSwitcher = screen.getByLabelText("已登录邮箱账户");
    const footer = accountSwitcher.closest(".sidebar__footer");

    expect(footer).toBeTruthy();
    expect(within(accountSwitcher).getByText("添加账户")).toBeTruthy();
    expect(footer.contains(screen.getByRole("button", { name: "设置" }))).toBe(true);
    expect(footer.contains(screen.getByRole("button", { name: "写信" }))).toBe(false);
  });

  it("keeps the brand and compose action outside the scrollable primary region", () => {
    renderSidebar(1);

    const brand = screen.getByLabelText("Mine Mail");
    const composeButton = screen.getByRole("button", { name: "写信" });
    const content = brand.closest(".sidebar__content");
    const primary = content.querySelector(".sidebar__primary");

    expect(content).toBeTruthy();
    expect(primary).toBeTruthy();
    expect(primary.contains(brand)).toBe(false);
    expect(primary.contains(composeButton)).toBe(false);
    expect(brand.parentElement).toBe(content);
    expect(composeButton.parentElement).toBe(content);
  });

  it("names the folder navigation and connected-account group semantically", () => {
    renderSidebar(1);

    expect(
      screen.getByRole("navigation", { name: "邮箱文件夹" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("group", { name: "已登录邮箱账户" }),
    ).toBeTruthy();
  });

  it("focuses and keyboard-navigates the theme menu, then restores its trigger", () => {
    const onThemeMenuClose = vi.fn();
    const { rerenderSidebar } = renderSidebar(1, { onThemeMenuClose });
    const toggle = screen.getByRole("button", { name: "主题外观" });
    toggle.focus();

    rerenderSidebar({ isThemeMenuOpen: true, onThemeMenuClose });
    const dusk = screen.getByRole("menuitemradio", { name: "黄昏" });
    const forest = screen.getByRole("menuitemradio", { name: "森林" });
    const daylight = screen.getByRole("menuitemradio", { name: "日间" });
    expect(document.activeElement).toBe(dusk);

    fireEvent.keyDown(dusk, { key: "ArrowRight" });
    expect(document.activeElement).toBe(forest);
    fireEvent.keyDown(forest, { key: "ArrowRight" });
    expect(document.activeElement).toBe(daylight);
    fireEvent.keyDown(daylight, { key: "End" });
    expect(document.activeElement).toBe(forest);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(onThemeMenuClose).toHaveBeenCalledOnce();
    rerenderSidebar({ isThemeMenuOpen: false, onThemeMenuClose });
    expect(document.activeElement).toBe(toggle);
  });

  it.each([
    [0, 1],
    [1, 1],
    [2, 1],
    [3, 0],
  ])("renders %i accounts with %i progressive add slot", (accountCount, slotCount) => {
    renderSidebar(accountCount);
    expect(screen.queryAllByRole("button", { name: /添加邮箱账户/ })).toHaveLength(
      slotCount,
    );
    expect(screen.queryByRole("combobox", { name: "切换邮箱账户" })).toBeNull();
  });

  it("switches from a visible account card and opens account setup from an empty slot", async () => {
    const user = userEvent.setup();
    const { onAccountSwitch, onAddAccount } = renderSidebar(2);

    await user.click(screen.getByRole("button", { name: "切换到 second@gmail.com" }));
    expect(onAccountSwitch).toHaveBeenCalledWith("google");

    await user.click(screen.getByRole("button", { name: /添加邮箱账户/ }));
    expect(onAddAccount).toHaveBeenCalledOnce();
  });

  it("uses an account remark as the visible account identity without hiding its address", () => {
    renderSidebar(1, {
      accountStatus: {
        configured: true,
        accounts: [{ ...accounts[0], remark: "工作邮箱" }],
        activeAccountId: "netease",
        maxAccounts: 3,
      },
    });

    const account = screen.getByRole("button", {
      name: "当前账户 工作邮箱 first@163.com",
    });
    expect(account.textContent).toContain("工作邮箱");
    expect(account.textContent).toContain("first@163.com");
  });

  it("moves the selected state between connected accounts", () => {
    const { rerenderSidebar } = renderSidebar(2);
    const firstAccount = screen.getByRole("button", { name: "当前账户 first@163.com" });
    const googleAccount = screen.getByRole("button", { name: "切换到 second@gmail.com" });

    expect(firstAccount.dataset.active).toBe("true");
    expect(googleAccount.dataset.active).toBe("false");
    expect(firstAccount.getAttribute("aria-pressed")).toBe("true");

    rerenderSidebar({
      accountStatus: {
        configured: true,
        accounts: accounts.slice(0, 2),
        activeAccountId: "google",
        maxAccounts: 3,
      },
    });

    expect(firstAccount.dataset.active).toBe("false");
    expect(googleAccount.dataset.active).toBe("true");
    expect(googleAccount.getAttribute("aria-pressed")).toBe("true");
  });

  it("moves one shared selection surface between connected accounts", () => {
    const rect = (top, left, width, height) => ({
      bottom: top + height,
      height,
      left,
      right: left + width,
      top,
      width,
      x: left,
      y: top,
      toJSON: () => ({}),
    });
    const boundsSpy = vi
      .spyOn(Element.prototype, "getBoundingClientRect")
      .mockImplementation(function getBoundingClientRect() {
        if (this.classList?.contains("account-switcher")) {
          return rect(300, 20, 232, 174);
        }
        if (this.textContent?.includes("first@163.com")) {
          return rect(361, 20, 232, 52);
        }
        if (this.textContent?.includes("second@gmail.com")) {
          return rect(422, 20, 232, 52);
        }
        return rect(0, 0, 0, 0);
      });

    try {
      const { rerenderSidebar } = renderSidebar(2);
      const switcher = screen.getByRole("group", {
        name: "已登录邮箱账户",
      });

      expect(switcher.dataset.selectionVisible).toBe("true");
      expect(
        switcher.style.getPropertyValue("--sliding-selection-y"),
      ).toBe("61px");

      rerenderSidebar({
        accountStatus: {
          configured: true,
          accounts: accounts.slice(0, 2),
          activeAccountId: "google",
          maxAccounts: 3,
        },
      });

      expect(
        switcher.style.getPropertyValue("--sliding-selection-y"),
      ).toBe("122px");
      expect(
        screen.getByRole("button", {
          name: "当前账户 second@gmail.com",
        }).getAttribute("aria-pressed"),
      ).toBe("true");
    } finally {
      boundsSpy.mockRestore();
    }
  });

  it("marks the exact account whose credential is invalid", () => {
    renderSidebar(2, {
      accountStatus: {
        configured: true,
        accounts: [
          accounts[0],
          {
            ...accounts[1],
            credentialAvailable: false,
            credentialInvalid: true,
          },
        ],
        activeAccountId: "netease",
        maxAccounts: 3,
      },
    });

    const healthy = screen.getByRole("button", {
      name: "当前账户 first@163.com",
    });
    const invalid = screen.getByRole("button", {
      name: "切换到 second@gmail.com，凭证失效",
    });
    expect(healthy.querySelector(".account-card__credential-warning")).toBeNull();
    expect(
      invalid.querySelector(".account-card__credential-warning"),
    ).toBeTruthy();
  });

  it("keeps Archive and Trash disabled until authoritative capabilities arrive", () => {
    const onFolderChange = vi.fn();
    renderSidebar(1, { onFolderChange });

    const archive = screen.getByRole("button", { name: "归档" });
    const trash = screen.getByRole("button", { name: "垃圾箱" });
    expect(archive.disabled).toBe(true);
    expect(trash.disabled).toBe(true);
    expect(archive.textContent).toBe("归档");
    expect(trash.textContent).toBe("垃圾箱");
    expect(onFolderChange).not.toHaveBeenCalled();
  });

  it("traps drawer focus, closes with Escape, and restores its trigger", () => {
    const trigger = document.createElement("button");
    trigger.textContent = "打开导航";
    document.body.append(trigger);
    trigger.focus();
    const drawerTriggerRef = { current: trigger };
    const onDrawerClose = vi.fn();
    const { rerenderSidebar } = renderSidebar(1, {
      isDrawerOpen: true,
      onDrawerClose,
      drawerTriggerRef,
    });

    const sidebar = screen.getByRole("complementary", { name: "邮箱导航" });
    const focusable = Array.from(
      sidebar.querySelectorAll("button:not([disabled])"),
    );
    const first = focusable[0];
    const last = focusable.at(-1);
    expect(document.activeElement).toBe(first);

    last.focus();
    fireEvent.keyDown(last, { key: "Tab" });
    expect(document.activeElement).toBe(first);

    first.focus();
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(onDrawerClose).toHaveBeenCalledOnce();

    rerenderSidebar({
      isDrawerOpen: false,
      onDrawerClose,
      drawerTriggerRef,
    });
    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });

  it("keeps a missing Archive role neutral and routes to its workspace", async () => {
    const user = userEvent.setup();
    const onFolderChange = vi.fn();
    const onMailboxSetup = vi.fn();
    renderSidebar(1, {
      onFolderChange,
      onMailboxSetup,
      mailboxCapabilities: {
        archive: {
          role: "archive",
          status: "needs_creation_confirmation",
          retryable: true,
        },
        trash: { role: "trash", status: "available", retryable: false },
      },
    });

    const archive = screen.getByRole("button", { name: "归档" });
    expect(archive.disabled).toBe(false);
    expect(archive.textContent).not.toContain("需设置");
    expect(archive.dataset.capabilityStatus).toBeUndefined();

    await user.click(archive);

    expect(onFolderChange).toHaveBeenCalledWith("archive");
    expect(onMailboxSetup).not.toHaveBeenCalled();
  });

  it("keeps discovery non-navigable without exposing its state", () => {
    renderSidebar(1, {
      mailboxCapabilities: {
        archive: {
          role: "archive",
          status: "discovery_pending",
          retryable: false,
        },
        trash: { role: "trash", status: "available", retryable: false },
      },
    });

    const archive = screen.getByRole("button", { name: "归档" });
    expect(archive.disabled).toBe(true);
    expect(archive.textContent).toBe("归档");
  });

  it("keeps unavailable roles neutral while retaining a controlled retry action", async () => {
    const user = userEvent.setup();
    const onMailboxCapabilityRetry = vi.fn();
    renderSidebar(1, {
      onMailboxCapabilityRetry,
      mailboxCapabilities: {
        archive: {
          role: "archive",
          status: "unavailable",
          unavailable_reason: "provider_unsupported",
          retryable: false,
        },
        trash: {
          role: "trash",
          status: "unavailable",
          unavailable_reason: "create_failed",
          retryable: true,
        },
      },
    });

    const archive = screen.getByRole("button", { name: "归档" });
    expect(archive.disabled).toBe(true);
    expect(archive.textContent).toBe("归档");

    const trashRetry = screen.getByRole("button", { name: "重新确认垃圾箱" });
    expect(trashRetry.disabled).toBe(false);
    expect(trashRetry.textContent).toBe("垃圾箱");
    await user.click(trashRetry);
    expect(onMailboxCapabilityRetry).toHaveBeenCalledWith("trash");
  });

  it("does not expose pending Archive work in the sidebar", () => {
    renderSidebar(1, {
      pendingCounts: { archive: 3 },
      mailboxCapabilities: {
        archive: { role: "archive", status: "available", retryable: false },
        trash: { role: "trash", status: "available", retryable: false },
      },
    });

    const archive = screen.getByRole("button", { name: "归档" });
    expect(archive.dataset.pendingCount).toBeUndefined();
    expect(archive.textContent).not.toContain("待同步");
    expect(archive.querySelector(".folder-nav__count")).toBeNull();
    expect(
      document.querySelector('.folder-nav .sr-only[role="status"]'),
    ).toBeNull();
  });
});
