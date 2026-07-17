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
  render(
    <Sidebar
      activeFolder="inbox"
      onFolderChange={vi.fn()}
      onCompose={vi.fn()}
      theme="dusk"
      onThemeChange={vi.fn()}
      isThemeMenuOpen={false}
      onThemeMenuToggle={vi.fn()}
      counts={{}}
      accountStatus={{
        configured: true,
        accounts: accounts.slice(0, accountCount),
        activeAccountId: accounts[0].accountId,
        maxAccounts: 3,
      }}
      accountAvatarFor={vi.fn(() => null)}
      onAccountSwitch={onAccountSwitch}
      onAddAccount={onAddAccount}
      onOpenSettings={vi.fn()}
      {...overrides}
    />,
  );
  return { onAccountSwitch, onAddAccount };
}

describe("Sidebar account switcher", () => {
  afterEach(cleanup);

  it.each([
    [1, 2],
    [2, 1],
    [3, 0],
  ])("renders %i accounts with %i remaining add slots", (accountCount, slotCount) => {
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
});
