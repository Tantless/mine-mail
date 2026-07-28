import { afterEach, describe, expect, it, vi } from "vitest";
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
import { SettingsPanel } from "./SettingsPanel.jsx";

const settings = {
  pollingIntervalMinutes: 5,
  autostartEnabled: false,
  notificationsEnabled: true,
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
    onOpenExternalLink: vi.fn(),
    accountAvatarFor: vi.fn(),
    onSetAccountAvatar: vi.fn(),
    onRemoveAccountAvatar: vi.fn(),
    focusTarget: null,
    ...overrides,
  };
}

describe("SettingsPanel account flow", () => {
  afterEach(() => cleanup());

  it("connects the remote-image privacy help control for pointer and keyboard users", async () => {
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps()} />);

    await user.click(screen.getByRole("button", { name: "功能设定" }));
    const help = screen.getByRole("button", {
      name: "了解自动加载远程图片的隐私风险",
    });
    const tooltip = screen.getByRole("tooltip");
    expect(help.getAttribute("aria-expanded")).toBe("false");

    await user.click(help);
    expect(help.getAttribute("aria-expanded")).toBe("true");
    expect(tooltip.dataset.open).toBe("true");
    expect(tooltip.textContent).toContain("可能暴露邮件打开时间");

    fireEvent.keyDown(help, { key: "Escape" });
    expect(help.getAttribute("aria-expanded")).toBe("false");
    expect(tooltip.dataset.open).toBeUndefined();
  });

  it("labels an invalid credential and explains how to repair it", async () => {
    const user = userEvent.setup();
    render(
      <SettingsPanel
        {...panelProps({
          accountStatus: {
            ...accountStatus,
            accounts: [
              accountStatus.accounts[0],
              {
                ...accountStatus.accounts[1],
                credentialAvailable: false,
                credentialInvalid: true,
              },
            ],
          },
        })}
      />,
    );

    const gmailCard = screen
      .getByText("second@gmail.com")
      .closest(".settings-account-card");
    const warning = within(gmailCard).getByText("凭证失效");
    await user.hover(warning);
    expect(
      await screen.findByRole("tooltip", {
        name: /需要重新登录.*新的授权凭证/,
      }),
    ).toBeTruthy();
  });

  it("does not collapse Add account because of a stale saved status", async () => {
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps({ accountSubmitStatus: "saved" })} />);

    await user.click(screen.getByRole("button", { name: "添加账户" }));
    expect(screen.getByRole("heading", { name: "选择邮箱服务商" })).toBeTruthy();
  });

  it("hides Outlook from the formal provider list even when a stale preset includes it", async () => {
    const user = userEvent.setup();
    render(
      <SettingsPanel
        {...panelProps({
          accountPresets: [
            { id: "163", label: "163 邮箱" },
            { id: "gmail", label: "Gmail" },
            {
              id: "outlook",
              label: "Outlook",
              disabled: true,
              note: "即将支持",
            },
            { id: "custom", label: "自定义 IMAP/SMTP" },
          ],
        })}
      />,
    );

    await user.click(screen.getByRole("button", { name: "添加账户" }));
    const providerPage = screen
      .getByRole("heading", { name: "选择邮箱服务商" })
      .closest("section");
    expect(within(providerPage).getByText("163 邮箱")).toBeTruthy();
    expect(within(providerPage).getByText("Gmail")).toBeTruthy();
    expect(within(providerPage).getByText("自定义 IMAP/SMTP")).toBeTruthy();
    expect(within(providerPage).queryByText("Outlook")).toBeNull();
    expect(within(providerPage).queryByText("即将支持")).toBeNull();
  });

  it("keeps a legacy Outlook account visible as cache-only without a reconnect form", () => {
    const legacyAccountStatus = {
      ...accountStatus,
      accountId: "outlook-account",
      activeAccountId: "outlook-account",
      email: "legacy@outlook.com",
      provider: "outlook",
      accounts: [
        {
          accountId: "outlook-account",
          email: "legacy@outlook.com",
          provider: "outlook",
          credentialAvailable: false,
          credentialInvalid: true,
        },
      ],
    };
    const view = render(
      <SettingsPanel
        {...panelProps({ accountStatus: legacyAccountStatus })}
      />,
    );

    const outlookCard = screen
      .getByText("已缓存邮件可读 · 当前不支持重新连接")
      .closest(".settings-account-card");
    expect(within(outlookCard).getByText("legacy@outlook.com")).toBeTruthy();
    expect(within(outlookCard).getByText("Outlook")).toBeTruthy();
    expect(
      within(outlookCard).getByText("已缓存邮件可读 · 当前不支持重新连接"),
    ).toBeTruthy();
    expect(within(outlookCard).queryByLabelText("凭证失效")).toBeNull();

    view.rerender(
      <SettingsPanel
        {...panelProps({
          accountStatus: legacyAccountStatus,
          focusTarget: "account-repair:outlook-account",
        })}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "Outlook 账户仅可读取缓存" }),
    ).toBeTruthy();
    expect(screen.getByText(/不能重新连接，也不能新建 Outlook 账户/)).toBeTruthy();
    expect(screen.getByText("此账户暂时无法重新连接")).toBeTruthy();
    expect(screen.queryByRole("textbox")).toBeNull();
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

  it("explains Google revocation separately from local cache deletion", async () => {
    const user = userEvent.setup();
    const onRemoveAccount = vi.fn();
    render(<SettingsPanel {...panelProps({ onRemoveAccount })} />);

    const gmailCard = screen
      .getByText("second@gmail.com")
      .closest(".settings-account-card");
    const cardButtons = within(gmailCard).getAllByRole("button");
    const manageAccount = cardButtons[cardButtons.length - 1];
    await user.click(manageAccount);
    await user.click(
      within(gmailCard).getByRole("menuitem", { name: "移除账户" }),
    );

    expect(
      screen.getByRole("alertdialog", { name: "移除 Gmail 账户？" }),
    ).toBeTruthy();
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "取消" }),
    );
    await user.click(
      screen.getByRole("checkbox", { name: /同时删除本地邮件缓存/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "撤销授权并移除" }),
    );

    expect(onRemoveAccount).toHaveBeenCalledWith(
      expect.objectContaining({ accountId: "gmail-account" }),
      {
        revokeGoogleAuthorization: true,
        deleteLocalData: true,
      },
    );
    expect(document.activeElement).toBe(manageAccount);

    await user.click(cardButtons[cardButtons.length - 1]);
    await user.click(
      within(gmailCard).getByRole("menuitem", { name: "移除账户" }),
    );
    await user.click(
      screen.getByRole("checkbox", { name: /同时删除本地邮件缓存/ }),
    );
    await user.click(screen.getByRole("button", { name: "仅断开" }));
    expect(onRemoveAccount).toHaveBeenLastCalledWith(
      expect.objectContaining({ accountId: "gmail-account" }),
      {
        revokeGoogleAuthorization: false,
        deleteLocalData: false,
      },
    );
  });

  it("edits a connected account remark from the local account menu", async () => {
    const user = userEvent.setup();
    const onSaveAccountRemark = vi.fn().mockResolvedValue(accountStatus);
    render(<SettingsPanel {...panelProps({ onSaveAccountRemark })} />);

    const manageAccount = screen.getByRole("button", {
      name: "管理 first@163.com",
    });
    await user.click(manageAccount);
    await user.click(screen.getByRole("menuitem", { name: "添加备注" }));

    const dialog = screen.getByRole("dialog", { name: "设置邮箱备注" });
    expect(dialog.getAttribute("aria-describedby")).toBe(
      "account-remark-description",
    );
    const cancel = within(dialog).getByRole("button", { name: "取消" });
    const close = within(dialog).getByRole("button", {
      name: "关闭邮箱备注编辑",
    });
    const save = within(dialog).getByRole("button", { name: "保存备注" });
    expect(document.activeElement).toBe(cancel);
    save.focus();
    fireEvent.keyDown(save, { key: "Tab" });
    expect(document.activeElement).toBe(close);
    close.focus();
    fireEvent.keyDown(close, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(save);

    const input = within(dialog).getByRole("textbox", { name: "备注名" });
    await user.type(input, "工作邮箱");
    await user.click(save);

    expect(onSaveAccountRemark).toHaveBeenCalledWith(
      "163-account",
      "工作邮箱",
    );
    expect(screen.queryByRole("dialog", { name: "设置邮箱备注" })).toBeNull();
    expect(document.activeElement).toBe(manageAccount);
  });

  it("locks and announces account-remark saving, then restores the account trigger", async () => {
    let finishSave;
    const onSaveAccountRemark = vi.fn(
      () =>
        new Promise((resolve) => {
          finishSave = resolve;
        }),
    );
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps({ onSaveAccountRemark })} />);

    const manageAccount = screen.getByRole("button", {
      name: "管理 first@163.com",
    });
    await user.click(manageAccount);
    await user.click(screen.getByRole("menuitem", { name: "添加备注" }));
    const dialog = screen.getByRole("dialog", { name: "设置邮箱备注" });
    await user.click(
      within(dialog).getByRole("button", { name: "保存备注" }),
    );

    await waitFor(() => expect(dialog.getAttribute("aria-busy")).toBe("true"));
    expect(document.activeElement).toBe(dialog);
    expect(
      within(dialog).getByText("正在保存邮箱备注…"),
    ).toBeTruthy();
    fireEvent.keyDown(dialog, { key: "Escape" });
    fireEvent.pointerDown(dialog.closest(".confirm-layer"));
    expect(
      screen.getByRole("dialog", { name: "设置邮箱备注" }),
    ).toBeTruthy();

    await act(async () => {
      finishSave(accountStatus);
    });
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "设置邮箱备注" }),
      ).toBeNull(),
    );
    expect(document.activeElement).toBe(manageAccount);
  });

  it("checks GitHub releases and asks before installing an update", async () => {
    let finishInstall;
    const installUpdate = vi.fn(async (_candidate, onEvent) => {
      onEvent({ event: "Started", data: { contentLength: 100 } });
      onEvent({ event: "Progress", data: { chunkLength: 100 } });
      await new Promise((resolve) => {
        finishInstall = resolve;
      });
      onEvent({ event: "Finished", data: {} });
    });
    const updateClient = {
      isSupported: true,
      bundledVersion: "0.1.2",
      getCurrentVersion: vi.fn().mockResolvedValue("0.1.2"),
      checkForUpdate: vi.fn().mockResolvedValue({
        status: "available",
        currentVersion: "0.1.2",
        version: "0.1.3",
        notes: "修复同步问题并改进更新体验。",
        resource: { id: 1 },
      }),
      installUpdate,
    };
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps({ updateClient })} />);

    await user.click(screen.getByRole("button", { name: "关于 Mine Mail" }));
    expect(screen.getByText("v0.1.2")).toBeTruthy();
    const checkUpdate = screen.getByRole("button", { name: "检查更新" });
    await user.click(checkUpdate);

    const dialog = await screen.findByRole("dialog", {
      name: "发现 Mine Mail v0.1.3",
    });
    expect(within(dialog).getByText(/当前为 v0.1.2/)).toBeTruthy();
    expect(within(dialog).getByText(/修复同步问题/)).toBeTruthy();
    expect(installUpdate).not.toHaveBeenCalled();
    expect(dialog.getAttribute("aria-describedby")).toBe(
      "update-confirm-description",
    );
    const postponeButtons = within(dialog).getAllByRole("button", {
      name: "暂不更新",
    });
    const postpone = postponeButtons.at(-1);
    expect(document.activeElement).toBe(postpone);

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: /发现 Mine Mail/ })).toBeNull();
    expect(document.activeElement).toBe(checkUpdate);

    await user.click(checkUpdate);
    const reopenedDialog = await screen.findByRole("dialog", {
      name: "发现 Mine Mail v0.1.3",
    });

    await user.click(
      within(reopenedDialog).getByRole("button", { name: "下载并安装" }),
    );
    expect(installUpdate).toHaveBeenCalledOnce();
    await waitFor(() =>
      expect(reopenedDialog.getAttribute("aria-busy")).toBe("true"),
    );
    expect(document.activeElement).toBe(reopenedDialog);
    expect(
      within(reopenedDialog).getByText("正在下载并安装更新…"),
    ).toBeTruthy();
    fireEvent.keyDown(reopenedDialog, { key: "Escape" });
    expect(
      screen.getByRole("dialog", { name: "发现 Mine Mail v0.1.3" }),
    ).toBeTruthy();

    await act(async () => {
      finishInstall();
    });
    expect(await screen.findByText(/更新已安装/)).toBeTruthy();
    expect(document.activeElement).toBe(checkUpdate);
  });

  it("keeps the legal resources as compact links inside the version card", async () => {
    const user = userEvent.setup();
    const onOpenExternalLink = vi.fn();
    render(<SettingsPanel {...panelProps({ onOpenExternalLink })} />);

    const aboutButton = screen
      .getAllByRole("button")
      .find((button) => button.textContent.includes("Mine Mail"));
    await user.click(aboutButton);
    const privacyLink = screen.getByRole("link", { name: "隐私政策" });
    const termsLink = screen.getByRole("link", { name: "服务条款" });
    const deletionLink = screen.getByRole("link", { name: "数据删除指南" });

    expect(privacyLink.closest(".settings-version-card")).toBeTruthy();
    expect(termsLink.closest(".settings-version-card")).toBeTruthy();
    expect(deletionLink.closest(".settings-version-card")).toBeTruthy();
    expect(screen.queryByText("隐私与数据")).toBeNull();
    expect(
      screen.queryByText("了解 Gmail 数据、本地缓存和凭据的处理方式"),
    ).toBeNull();

    await user.click(privacyLink);

    expect(onOpenExternalLink).toHaveBeenCalledWith(
      "https://minemail.tantless.online/privacy/",
    );
  });

  it("shows storage usage and confirms a cold migration before relaunching", async () => {
    const storageClient = {
      isSupported: true,
      getStatus: vi.fn().mockResolvedValue({
        dataPath: "D:\\Mine Mail\\Data",
        locationKind: "install_directory",
        available: true,
        totalBytes: 1_610_612_736,
        categories: [
          {
            id: "mail",
            label: "邮件与本地资料",
            bytes: 1_073_741_824,
          },
          {
            id: "webview",
            label: "界面与浏览器缓存",
            bytes: 536_870_912,
          },
        ],
        migrationNotice: null,
      }),
      chooseDirectory: vi.fn().mockResolvedValue("E:\\Mine Mail Data"),
      prepareMigration: vi.fn().mockResolvedValue({
        targetPath: "E:\\Mine Mail Data",
        totalBytes: 1_610_612_736,
      }),
      cancelMigration: vi.fn().mockResolvedValue(undefined),
      relaunch: vi.fn().mockResolvedValue(undefined),
    };
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps({ storageClient })} />);

    await user.click(screen.getByRole("button", { name: "关于 Mine Mail" }));
    expect(await screen.findByText("1.5 GiB")).toBeTruthy();
    expect(screen.getByText("D:\\Mine Mail\\Data")).toBeTruthy();
    expect(screen.getByText("随应用安装位置")).toBeTruthy();

    const changeStorage = screen.getByRole("button", { name: "更改位置" });
    await user.click(changeStorage);
    const dialog = await screen.findByRole("dialog", { name: "迁移本地数据" });
    expect(within(dialog).getByText("E:\\Mine Mail Data")).toBeTruthy();
    expect(storageClient.prepareMigration).not.toHaveBeenCalled();
    expect(dialog.getAttribute("aria-describedby")).toBe(
      "storage-migration-description",
    );
    expect(document.activeElement).toBe(
      within(dialog).getByRole("button", { name: "取消" }),
    );

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "迁移本地数据" })).toBeNull();
    expect(document.activeElement).toBe(changeStorage);

    await user.click(changeStorage);
    const reopenedDialog = await screen.findByRole("dialog", {
      name: "迁移本地数据",
    });

    await user.click(
      within(reopenedDialog).getByRole("button", { name: "迁移并重启" }),
    );
    await waitFor(() =>
      expect(storageClient.prepareMigration).toHaveBeenCalledWith(
        "E:\\Mine Mail Data",
      ),
    );
    expect(storageClient.relaunch).toHaveBeenCalledOnce();
    expect(reopenedDialog.getAttribute("aria-busy")).toBe("true");
    expect(document.activeElement).toBe(reopenedDialog);
    expect(
      within(reopenedDialog).getByText("正在准备数据迁移并重启…"),
    ).toBeTruthy();
    fireEvent.keyDown(reopenedDialog, { key: "Escape" });
    expect(
      screen.getByRole("dialog", { name: "迁移本地数据" }),
    ).toBeTruthy();
  });

  it("cancels a queued storage migration when relaunch fails", async () => {
    const storageClient = {
      isSupported: true,
      getStatus: vi.fn().mockResolvedValue({
        dataPath: "D:\\Mine Mail\\Data",
        locationKind: "install_directory",
        available: true,
        totalBytes: 1024,
        categories: [],
        migrationNotice: null,
      }),
      chooseDirectory: vi.fn().mockResolvedValue("E:\\Mine Mail Data"),
      prepareMigration: vi.fn().mockResolvedValue({
        targetPath: "E:\\Mine Mail Data",
        totalBytes: 1024,
      }),
      cancelMigration: vi.fn().mockResolvedValue(undefined),
      relaunch: vi.fn().mockRejectedValue(new Error("restart unavailable")),
    };
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps({ storageClient })} />);

    await user.click(screen.getByRole("button", { name: "关于 Mine Mail" }));
    await screen.findByText("D:\\Mine Mail\\Data");
    await user.click(screen.getByRole("button", { name: "更改位置" }));
    await user.click(
      within(
        await screen.findByRole("dialog", { name: "迁移本地数据" }),
      ).getByRole("button", { name: "迁移并重启" }),
    );

    await waitFor(() =>
      expect(storageClient.cancelMigration).toHaveBeenCalledOnce(),
    );
    expect(await screen.findByText("restart unavailable")).toBeTruthy();
  });

  it("keeps a missing configured data path visible without offering an empty fallback", async () => {
    const storageClient = {
      isSupported: true,
      getStatus: vi.fn().mockResolvedValue({
        dataPath: "E:\\Mine Mail Data",
        locationKind: "custom",
        available: false,
        totalBytes: 0,
        categories: [],
        migrationNotice: null,
      }),
      chooseDirectory: vi.fn(),
      prepareMigration: vi.fn(),
      cancelMigration: vi.fn(),
      relaunch: vi.fn(),
    };
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps({ storageClient })} />);

    await user.click(screen.getByRole("button", { name: "关于 Mine Mail" }));

    expect(await screen.findByText("E:\\Mine Mail Data")).toBeTruthy();
    const unavailablePath = screen.getByText(
      "当前数据目录不可用。请重新连接对应磁盘，然后重启 Mine Mail。",
    );
    expect(unavailablePath.getAttribute("role")).toBe("alert");
    expect(unavailablePath.getAttribute("aria-live")).toBe("assertive");
    expect(unavailablePath.getAttribute("aria-atomic")).toBe("true");
    expect(screen.getByRole("button", { name: "更改位置" }).disabled).toBe(true);
  });
});
