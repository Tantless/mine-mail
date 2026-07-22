import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AccountSetupForm } from "./AccountSetup.jsx";

const presets = [
  { id: "163", label: "163 邮箱", secretLabel: "客户端授权密码", availableInMvp: true },
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

  it("explains and blocks Outlook until Modern Auth is implemented", async () => {
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

    await user.click(screen.getByRole("radio", { name: "Outlook" }));
    expect(screen.getByText(/OAuth \/ Modern Auth 尚未支持/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "连接邮箱" }).disabled).toBe(true);
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
