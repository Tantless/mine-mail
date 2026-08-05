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
    accountErrorProvider: null,
    onConfigureAccount: vi.fn(),
    onConnectGoogle: vi.fn(),
    onAccountProviderChange: vi.fn(),
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

  it("moves one shared selection surface between settings categories", async () => {
    const user = userEvent.setup();
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
        if (this.classList?.contains("settings-nav")) {
          return rect(100, 20, 182, 186);
        }
        if (this.textContent === "账户") {
          return rect(100, 20, 182, 58);
        }
        if (this.textContent === "功能设定") {
          return rect(164, 20, 182, 58);
        }
        if (this.textContent === "关于 Mine Mail") {
          return rect(228, 20, 182, 58);
        }
        return rect(0, 0, 0, 0);
      });

    try {
      render(<SettingsPanel {...panelProps()} />);
      const navigation = screen.getByRole("navigation", {
        name: "设置菜单",
      });

      expect(navigation.dataset.selectionVisible).toBe("true");
      expect(
        navigation.style.getPropertyValue("--sliding-selection-y"),
      ).toBe("0px");

      await user.click(screen.getByRole("button", { name: "功能设定" }));

      expect(
        navigation.style.getPropertyValue("--sliding-selection-y"),
      ).toBe("64px");
      expect(
        screen
          .getByRole("button", { name: "功能设定" })
          .getAttribute("aria-current"),
      ).toBe("page");
    } finally {
      boundsSpy.mockRestore();
    }
  });

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

  it("keeps a Google timeout error out of another provider form", async () => {
    const user = userEvent.setup();
    const onAccountProviderChange = vi.fn();
    const props = panelProps({ onAccountProviderChange });
    const view = render(<SettingsPanel {...props} />);

    await user.click(screen.getByRole("button", { name: "添加账户" }));
    await user.click(screen.getByRole("button", { name: /Gmail/ }));
    view.rerender(
      <SettingsPanel
        {...props}
        accountSubmitStatus="error"
        accountError="Google 登录等待超时，请重试。"
        accountErrorProvider="gmail"
      />,
    );
    expect(screen.getByRole("alert").textContent).toBe(
      "Google 登录等待超时，请重试。",
    );

    await user.click(
      screen.getByRole("button", { name: "返回选择邮箱服务商" }),
    );
    await user.click(screen.getByRole("button", { name: /QQ 邮箱/ }));

    expect(screen.queryByText("Google 登录等待超时，请重试。")).toBeNull();
    expect(onAccountProviderChange).toHaveBeenLastCalledWith("qq");
  });

  it("keeps Chinese connection headings free of inserted whitespace", async () => {
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps()} />);

    await user.click(screen.getByRole("button", { name: "添加账户" }));
    await user.click(screen.getByRole("button", { name: /其他邮箱/ }));

    expect(
      screen.getByRole("heading", { name: "连接自定义邮箱" }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("heading", { name: "连接 自定义邮箱" }),
    ).toBeNull();
  });

  it("hides Outlook from the formal provider list even when a stale preset includes it", async () => {
    const user = userEvent.setup();
    render(
      <SettingsPanel
        {...panelProps({
          accountPresets: [
            { id: "163", label: "163 邮箱" },
            { id: "qq", label: "QQ 邮箱" },
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
    expect(within(providerPage).getByText("QQ 邮箱")).toBeTruthy();
    expect(within(providerPage).getByText("Gmail")).toBeTruthy();
    expect(within(providerPage).getByText("自定义 IMAP/SMTP")).toBeTruthy();
    expect(within(providerPage).queryByText("Outlook")).toBeNull();
    expect(within(providerPage).queryByText("即将支持")).toBeNull();
  });

  it("opens the bundled authorization-code guide without losing form input", async () => {
    const onOpenExternalLink = vi.fn();
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps({ onOpenExternalLink })} />);

    await user.click(screen.getByRole("button", { name: "添加账户" }));
    await user.click(screen.getByRole("button", { name: /163 邮箱/ }));
    const secretInput = screen.getByLabelText("客户端授权密码");
    const guideButton = screen.getByRole("button", {
      name: "查看 163 邮箱授权码获取教程",
    });
    const connectionPage = secretInput.closest("section");
    await user.type(secretInput, "keep-this-secret");
    await user.click(guideButton);

    expect(
      screen.getByRole("heading", { name: "163 邮箱授权码教程" }),
    ).toBeTruthy();
    const guidePage = screen.getByRole("region", {
      name: "163 邮箱授权码教程",
    });
    await waitFor(() => expect(document.activeElement).toBe(guidePage));
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(
      screen.getByRole("img", {
        name: "163 邮箱设置菜单中的 POP3/SMTP/IMAP 入口",
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole("img", {
        name: "163 邮箱已开启 IMAP/SMTP 服务并显示新增授权密码按钮",
      }),
    ).toBeTruthy();
    expect(connectionPage.hidden).toBe(true);
    expect(secretInput.value).toBe("keep-this-secret");
    expect(onOpenExternalLink).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "返回连接163 邮箱" }));
    expect(
      screen.queryByRole("heading", { name: "163 邮箱授权码教程" }),
    ).toBeNull();
    expect(connectionPage.hidden).toBe(false);
    expect(secretInput.value).toBe("keep-this-secret");
    await waitFor(() => expect(document.activeElement).toBe(guideButton));
    expect(screen.queryByRole("tooltip")).toBeNull();

    await user.click(
      screen.getByRole("button", { name: "返回选择邮箱服务商" }),
    );
    await user.click(screen.getByRole("button", { name: /QQ 邮箱/ }));
    await user.click(
      screen.getByRole("button", { name: "查看 QQ 邮箱授权码获取教程" }),
    );

    expect(
      screen.getByRole("heading", { name: "QQ 邮箱授权码教程" }),
    ).toBeTruthy();
    await waitFor(() =>
      expect(document.activeElement).toBe(
        screen.getByRole("region", { name: "QQ 邮箱授权码教程" }),
      ),
    );
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(
      screen.getByRole("img", {
        name: "QQ 邮箱安全设置中的邮件服务和生成授权码按钮",
      }),
    ).toBeTruthy();
    expect(onOpenExternalLink).not.toHaveBeenCalled();
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
    expect(
      screen.queryByRole("form", { name: "first@163.com 账户备注" }),
    ).toBeNull();

    await user.click(manageAccount);
    await user.click(screen.getByRole("menuitem", { name: "添加备注" }));

    const editor = screen.getByRole("form", {
      name: "first@163.com 账户备注",
    });
    expect(within(editor).getByText("备注")).toBeTruthy();
    expect(screen.queryByRole("dialog", { name: "设置邮箱备注" })).toBeNull();
    const input = within(editor).getByRole("textbox", { name: "账户备注" });
    expect(input.getAttribute("maxLength")).toBe("40");
    expect(input.getAttribute("placeholder")).toBe("输入备注");
    expect(input.getAttribute("aria-describedby")).toBeNull();
    expect(within(editor).queryByText("在账户来源与通知中优先显示；留空可删除。")).toBeNull();
    expect(within(editor).queryByText("账户备注")).toBeNull();
    expect(document.activeElement).toBe(input);

    await user.type(input, "工作邮箱{Enter}");

    expect(onSaveAccountRemark).toHaveBeenCalledWith(
      "163-account",
      "工作邮箱",
    );
    expect(
      screen.queryByRole("form", { name: "first@163.com 账户备注" }),
    ).toBeNull();
    expect(document.activeElement).toBe(manageAccount);
  });

  it("supports keyboard and outside-click dismissal for account action menus", async () => {
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps()} />);

    const gmailCard = screen
      .getByText("second@gmail.com")
      .closest(".settings-account-card");
    const manageAccount = within(gmailCard).getByRole("button", {
      name: "管理 second@gmail.com",
    });

    await user.click(manageAccount);
    const menu = within(gmailCard).getByRole("menu");
    const addRemark = within(menu).getByRole("menuitem", {
      name: "添加备注",
    });
    const removeAccount = within(menu).getByRole("menuitem", {
      name: "移除账户",
    });

    expect(manageAccount.getAttribute("aria-haspopup")).toBe("menu");
    expect(manageAccount.getAttribute("aria-controls")).toBe(menu.id);
    expect(document.activeElement).toBe(addRemark);

    fireEvent.keyDown(addRemark, { key: "ArrowDown" });
    expect(document.activeElement).toBe(removeAccount);

    fireEvent.keyDown(removeAccount, { key: "Escape" });
    expect(within(gmailCard).queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(manageAccount);

    await user.click(manageAccount);
    fireEvent.pointerDown(document.body);
    expect(within(gmailCard).queryByRole("menu")).toBeNull();
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
    const editor = screen.getByRole("form", {
      name: "first@163.com 账户备注",
    });
    await user.click(
      within(editor).getByRole("button", { name: "保存" }),
    );

    await waitFor(() => expect(editor.getAttribute("aria-busy")).toBe("true"));
    expect(
      within(editor).getByRole("textbox", { name: "账户备注" }).disabled,
    ).toBe(true);
    expect(
      within(editor).getByText("正在保存邮箱备注…"),
    ).toBeTruthy();
    expect(
      within(editor).getByRole("button", { name: "保存中…" }),
    ).toBeTruthy();
    fireEvent.keyDown(editor, { key: "Escape" });
    expect(
      within(editor).getByRole("textbox", { name: "账户备注" }),
    ).toBeTruthy();

    await act(async () => {
      finishSave(accountStatus);
    });
    await waitFor(() =>
      expect(
        screen.queryByRole("form", { name: "first@163.com 账户备注" }),
      ).toBeNull(),
    );
    expect(document.activeElement).toBe(manageAccount);
  });

  it("keeps account-remark errors in the edited row and supports Escape cancellation", async () => {
    const onSaveAccountRemark = vi
      .fn()
      .mockRejectedValue(new Error("邮箱备注没有保存，请稍后重试。"));
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps({ onSaveAccountRemark })} />);

    const manageAccount = screen.getByRole("button", {
      name: "管理 first@163.com",
    });
    await user.click(manageAccount);
    await user.click(screen.getByRole("menuitem", { name: "添加备注" }));
    const editor = screen.getByRole("form", {
      name: "first@163.com 账户备注",
    });
    const input = within(editor).getByRole("textbox", { name: "账户备注" });
    await user.type(input, "工作邮箱");
    await user.click(
      within(editor).getByRole("button", { name: "保存" }),
    );

    const error = await within(editor).findByRole("alert");
    expect(error.textContent).toBe("邮箱备注没有保存，请稍后重试。");
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(document.activeElement).toBe(
      within(editor).getByRole("button", { name: "保存" }),
    );

    fireEvent.keyDown(editor, { key: "Escape" });
    expect(
      screen.queryByRole("form", { name: "first@163.com 账户备注" }),
    ).toBeNull();
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
    expect(
      within(reopenedDialog).getByRole("button", { name: "取消更新下载" }),
    ).toBeTruthy();
    fireEvent.keyDown(reopenedDialog, { key: "Escape" });
    expect(
      screen.queryByRole("dialog", { name: "发现 Mine Mail v0.1.3" }),
    ).toBeNull();
    expect(installUpdate).toHaveBeenCalledOnce();

    await act(async () => {
      finishInstall();
    });
    expect(await screen.findByText(/更新已安装/)).toBeTruthy();
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "关闭设置" }),
    );
  });

  it("shows the classified reason when checking for updates fails", async () => {
    const updateClient = {
      isSupported: true,
      bundledVersion: "0.1.2",
      getCurrentVersion: vi.fn().mockResolvedValue("0.1.2"),
      checkForUpdate: vi
        .fn()
        .mockRejectedValue(new Error("request timed out while fetching latest.json")),
      installUpdate: vi.fn(),
    };
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps({ updateClient })} />);

    await user.click(screen.getByRole("button", { name: "关于 Mine Mail" }));
    const checkUpdate = screen.getByRole("button", { name: "检查更新" });
    await user.click(checkUpdate);

    expect(
      await screen.findByText(
        "连接更新服务器超时。请检查系统代理或网络后重试；当前版本和本地数据不受影响。",
      ),
    ).toBeTruthy();
    expect(checkUpdate.disabled).toBe(false);
    expect(screen.queryByText(/request timed out/i)).toBeNull();
  });

  it("shows an expired package URL when update download returns 404", async () => {
    const updateClient = {
      isSupported: true,
      bundledVersion: "0.1.2",
      getCurrentVersion: vi.fn().mockResolvedValue("0.1.2"),
      checkForUpdate: vi.fn().mockResolvedValue({
        status: "available",
        currentVersion: "0.1.2",
        version: "0.1.3",
        notes: null,
        resource: { id: 1 },
      }),
      installUpdate: vi
        .fn()
        .mockRejectedValue(new Error("download request returned HTTP 404")),
    };
    const user = userEvent.setup();
    render(<SettingsPanel {...panelProps({ updateClient })} />);

    await user.click(screen.getByRole("button", { name: "关于 Mine Mail" }));
    await user.click(screen.getByRole("button", { name: "检查更新" }));
    const dialog = await screen.findByRole("dialog", {
      name: "发现 Mine Mail v0.1.3",
    });
    await user.click(
      within(dialog).getByRole("button", { name: "下载并安装" }),
    );

    expect(
      await within(dialog).findByText(
        "更新安装包不存在或下载地址已失效（服务器返回 404）。请稍后重试；当前版本和本地数据不受影响。",
      ),
    ).toBeTruthy();
    expect(
      within(dialog).getByRole("button", { name: "下载并安装" }).disabled,
    ).toBe(false);
    expect(within(dialog).queryByText(/download request returned/i)).toBeNull();
  });

  it("opens the legal resources and source repository as external links", async () => {
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
    const sourceRepositoryLink = screen.getByRole("link", {
      name: "Tantless/mine-mail",
    });

    expect(privacyLink.closest(".settings-version-card")).toBeTruthy();
    expect(termsLink.closest(".settings-version-card")).toBeTruthy();
    expect(deletionLink.closest(".settings-version-card")).toBeTruthy();
    expect(sourceRepositoryLink.closest(".settings-version-note")).toBeTruthy();
    expect(sourceRepositoryLink.getAttribute("href")).toBe(
      "https://github.com/Tantless/mine-mail",
    );
    expect(screen.getByText(/如有问题与反馈，可以在 GitHub Issues 中提出/)).toBeTruthy();
    expect(screen.queryByText("隐私与数据")).toBeNull();
    expect(
      screen.queryByText("了解 Gmail 数据、本地缓存和凭据的处理方式"),
    ).toBeNull();

    await user.click(privacyLink);

    expect(onOpenExternalLink).toHaveBeenCalledWith(
      "https://minemail.tantless.online/privacy/",
    );

    await user.click(sourceRepositoryLink);

    expect(onOpenExternalLink).toHaveBeenLastCalledWith(
      "https://github.com/Tantless/mine-mail",
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

    const storageSection = screen.getByRole("region", {
      name: "本地数据存储",
    });
    expect(within(storageSection).getByText("存储位置")).toBeTruthy();
    expect(within(storageSection).queryByText("数据构成")).toBeNull();
    expect(within(storageSection).queryByText("总计 1.5 GiB")).toBeNull();
    const composition = within(storageSection).getByRole("img", {
      name: /本地数据空间占用构成/,
    });
    const segments = composition.querySelectorAll(
      ".settings-storage-composition__segment",
    );
    expect(segments).toHaveLength(2);
    expect(segments[0].dataset.storageCategory).toBe("mail");
    expect(segments[0].style.flexGrow).toBe("1073741824");
    expect(segments[1].dataset.storageCategory).toBe("webview");
    expect(within(storageSection).queryByRole("progressbar")).toBeNull();
    expect(
      within(storageSection).queryByRole("list", {
        name: "存储分类图例",
      }),
    ).toBeNull();

    await user.hover(segments[0]);
    expect((await screen.findByRole("tooltip")).textContent).toBe(
      "邮件与本地资料 · 1.0 GiB",
    );
    await user.unhover(segments[0]);
    await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());

    const changeStorage = screen.getByRole("button", { name: "更改位置" });
    expect(changeStorage.textContent).toBe("");
    expect(changeStorage.closest(".settings-storage-location__capsule")).toBeTruthy();
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
    expect(
      await screen.findByText(
        "Mine Mail 内部处理失败：数据迁移没有开始，原目录不会改变。请重试；如果仍然失败，请重启应用。",
      ),
    ).toBeTruthy();
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
