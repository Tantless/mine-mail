import { cleanup, render, screen } from "@testing-library/react";
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
});
