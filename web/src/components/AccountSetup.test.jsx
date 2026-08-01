import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AccountSetupForm } from "./AccountSetup.jsx";

const presets = [
  { id: "163", label: "163 邮箱", secretLabel: "客户端授权密码", availableInMvp: true },
  { id: "qq", label: "QQ 邮箱", secretLabel: "QQ 邮箱授权码", availableInMvp: true },
  { id: "gmail", label: "Gmail", oauth: true, secretLabel: "Google OAuth", availableInMvp: true },
  {
    id: "outlook",
    label: "Outlook",
    availableInMvp: false,
    authenticationNote: "OAuth / Modern Auth 尚未支持",
  },
  { id: "custom", label: "自定义 IMAP/SMTP", availableInMvp: true },
];

describe("AccountSetupForm", () => {
  afterEach(() => cleanup());

  it("makes each visible text-field shell the input interaction surface", () => {
    render(
      <AccountSetupForm
        presets={presets}
        status={{ configured: false }}
        submitStatus="idle"
        error={null}
        onSubmit={vi.fn()}
      />,
    );

    const email = screen.getByLabelText("邮箱地址");
    const secret = screen.getByPlaceholderText("请输入授权密码");
    expect(
      email.closest(".settings-input-shell--text"),
    ).toBe(email.parentElement);
    expect(
      secret.closest(".settings-input-shell--text"),
    ).toBe(secret.parentElement);
  });

  it("silently focuses empty fields instead of showing redundant browser prompts", async () => {
    const onSubmit = vi.fn();
    const user = userEvent.setup();
    const { container } = render(
      <AccountSetupForm
        presets={presets}
        status={{ configured: false }}
        submitStatus="idle"
        error={null}
        onSubmit={onSubmit}
      />,
    );

    const form = container.querySelector("form");
    const email = screen.getByLabelText("邮箱地址");
    const secret = screen.getByPlaceholderText("请输入授权密码");
    expect(form.noValidate).toBe(true);
    expect(email.required).toBe(false);
    expect(secret.required).toBe(false);

    await user.click(screen.getByRole("button", { name: "连接邮箱" }));

    expect(screen.queryByRole("alert")).toBeNull();
    expect(email.getAttribute("aria-invalid")).toBe("true");
    expect(document.activeElement).toBe(email);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("keeps useful validation feedback inside the app theme", async () => {
    const onSubmit = vi.fn();
    const user = userEvent.setup();
    render(
      <AccountSetupForm
        presets={presets}
        status={{ configured: false }}
        submitStatus="idle"
        error={null}
        onSubmit={onSubmit}
      />,
    );

    const email = screen.getByLabelText("邮箱地址");
    await user.type(email, "not-an-email");
    await user.click(screen.getByRole("button", { name: "连接邮箱" }));

    expect(screen.getByRole("alert").textContent).toBe("邮箱地址格式不正确。");
    expect(email.getAttribute("aria-describedby")).toBe("account-setup-error");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("clears the uncontrolled secret input immediately after submit", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(
      <AccountSetupForm
        presets={presets}
        status={{ configured: false }}
        submitStatus="idle"
        error={null}
        onSubmit={onSubmit}
      />,
    );

    await user.type(screen.getByLabelText("邮箱地址"), "me@163.com");
    const secretInput = screen.getByPlaceholderText("请输入授权密码");
    await user.type(secretInput, "temporary-secret");
    await user.click(screen.getByRole("button", { name: "连接邮箱" }));

    expect(secretInput.value).toBe("");
    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: "163",
        email: "me@163.com",
        secret: "temporary-secret",
      }),
    );
  });

  it("omits Outlook when an old preset response still includes it", () => {
    render(
      <AccountSetupForm
        presets={presets}
        status={{ configured: false }}
        submitStatus="idle"
        error={null}
        onSubmit={vi.fn()}
      />,
    );

    expect(
      screen.getAllByRole("radio").map((option) => option.textContent),
    ).toEqual(["163 邮箱", "QQ 邮箱", "Gmail", "自定义 IMAP/SMTP"]);
    expect(screen.queryByRole("radio", { name: "Outlook" })).toBeNull();
    expect(screen.queryByText(/OAuth \/ Modern Auth 尚未支持/)).toBeNull();
  });

  it.each([
    ["missing", undefined],
    ["empty", []],
  ])("uses only formal fallback providers when presets are %s", (_label, emptyPresets) => {
    render(
      <AccountSetupForm
        presets={emptyPresets}
        status={{ configured: false }}
        submitStatus="idle"
        error={null}
        onSubmit={vi.fn()}
      />,
    );

    expect(
      screen.getAllByRole("radio").map((option) => option.textContent),
    ).toEqual(["163 邮箱", "QQ 邮箱", "Gmail", "自定义 IMAP/SMTP"]);
    expect(screen.queryByText("Outlook")).toBeNull();
  });

  it("starts Google OAuth without asking React for a password", async () => {
    const onGoogle = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(
      <AccountSetupForm
        presets={presets}
        status={{ configured: false }}
        submitStatus="idle"
        error={null}
        onSubmit={vi.fn()}
        onGoogle={onGoogle}
      />,
    );

    await user.click(screen.getByRole("radio", { name: "Gmail" }));
    expect(screen.queryByLabelText("邮箱地址")).toBeNull();
    expect(screen.queryByPlaceholderText("请输入授权密码")).toBeNull();
    await user.click(screen.getByRole("button", { name: "使用 Google 登录" }));
    expect(onGoogle).toHaveBeenCalledOnce();
  });

  it("submits QQ accounts with the provider-issued authorization code", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(
      <AccountSetupForm
        presets={presets}
        status={{ configured: false }}
        submitStatus="idle"
        error={null}
        onSubmit={onSubmit}
      />,
    );

    await user.click(screen.getByRole("radio", { name: "QQ 邮箱" }));
    await user.type(screen.getByLabelText("邮箱地址"), "mine@qq.com");
    await user.type(screen.getByLabelText("QQ 邮箱授权码"), "qq-app-secret");
    await user.click(screen.getByRole("button", { name: "连接邮箱" }));

    expect(onSubmit).toHaveBeenCalledWith({
      provider: "qq",
      email: "mine@qq.com",
      secret: "qq-app-secret",
    });
  });

  it("uses the themed selector for custom SMTP security", async () => {
    const user = userEvent.setup();
    render(
      <AccountSetupForm
        presets={presets}
        status={{ configured: false }}
        submitStatus="idle"
        error={null}
        onSubmit={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("radio", { name: "自定义 IMAP/SMTP" }));
    const security = screen.getByRole("combobox", { name: "SMTP 安全" });
    await user.click(security);
    await user.click(screen.getByRole("option", { name: "STARTTLS" }));
    expect(security.textContent).toContain("STARTTLS");
    expect(document.querySelector("select")).toBeNull();
  });
});
