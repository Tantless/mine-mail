import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SettingsPanel } from "./SettingsPanel.jsx";

const settings = {
  pollingIntervalMinutes: 5,
  autostartEnabled: false,
  notificationsEnabled: true,
  foregroundNotificationsEnabled: true,
  notificationSoundEnabled: true,
  notificationSound: "mail",
  remoteImageMode: "automatic",
};

const accountStatus = {
  configured: true,
  canAddAccount: true,
  maxAccounts: 3,
  accountId: "163-account",
  activeAccountId: "163-account",
  email: "first@163.com",
  provider: "163",
  accounts: [
    { accountId: "163-account", email: "first@163.com", provider: "163" },
    { accountId: "gmail-account", email: "second@gmail.com", provider: "gmail" },
  ],
};

function panelProps(overrides = {}) {
  return {
    settings,
    saveStatus: "idle",
    onClose: vi.fn(),
    onSave: vi.fn(),
    accountPresets: [],
    accountStatus,
    accountSubmitStatus: "idle",
    accountError: null,
    onConfigureAccount: vi.fn(),
    onConnectGoogle: vi.fn(),
    onSwitchAccount: vi.fn(),
    onSaveAccountRemark: vi.fn().mockResolvedValue(accountStatus),
    onRemoveAccount: vi.fn(),
    accountAvatarFor: vi.fn(),
    onSetAccountAvatar: vi.fn(),
    onRemoveAccountAvatar: vi.fn(),
    focusTarget: null,
    ...overrides,
  };
}

describe("SettingsPanel account flow", () => {
  afterEach(() => cleanup());

  it("does not collapse Add account because of a stale saved status", async () => {
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps({ accountSubmitStatus: "saved" })} />);

    await user.click(screen.getByRole("button", { name: "添加账户" }));
    expect(screen.getByRole("heading", { name: "选择邮箱服务商" })).toBeTruthy();
  });

  it("returns to the overview only after the current connection finishes", async () => {
    const user = userEvent.setup();
    const props = panelProps();
    const view = render(<SettingsPanel {...props} />);
    await user.click(screen.getByRole("button", { name: "添加账户" }));
    expect(screen.getByRole("heading", { name: "选择邮箱服务商" })).toBeTruthy();

    view.rerender(<SettingsPanel {...props} accountSubmitStatus="saving" />);
    view.rerender(<SettingsPanel {...props} accountSubmitStatus="saved" />);
    expect(screen.getByRole("heading", { name: "账户与同步" })).toBeTruthy();
  });

  it("confirms account removal in a themed app dialog", async () => {
    const user = userEvent.setup();
    const onRemoveAccount = vi.fn();
    render(<SettingsPanel {...panelProps({ onRemoveAccount })} />);

    await user.click(
      screen.getByRole("button", { name: "管理 first@163.com" }),
    );
    await user.click(screen.getByRole("menuitem", { name: "移除账户" }));

    const dialog = screen.getByRole("alertdialog", {
      name: "移除这个邮箱账户？",
    });
    expect(within(dialog).getByText("first@163.com")).toBeTruthy();
    expect(onRemoveAccount).not.toHaveBeenCalled();

    await user.click(within(dialog).getByRole("button", { name: "移除账户" }));
    expect(onRemoveAccount).toHaveBeenCalledWith(accountStatus.accounts[0]);
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("edits a connected account remark from the local account menu", async () => {
    const user = userEvent.setup();
    const onSaveAccountRemark = vi.fn().mockResolvedValue(accountStatus);
    render(<SettingsPanel {...panelProps({ onSaveAccountRemark })} />);

    await user.click(
      screen.getByRole("button", { name: "管理 first@163.com" }),
    );
    await user.click(screen.getByRole("menuitem", { name: "添加备注" }));

    const dialog = screen.getByRole("dialog", { name: "设置邮箱备注" });
    const input = within(dialog).getByRole("textbox", { name: "备注名" });
    await user.type(input, "工作邮箱");
    await user.click(within(dialog).getByRole("button", { name: "保存备注" }));

    expect(onSaveAccountRemark).toHaveBeenCalledWith(
      "163-account",
      "工作邮箱",
    );
    expect(screen.queryByRole("dialog", { name: "设置邮箱备注" })).toBeNull();
  });
});
