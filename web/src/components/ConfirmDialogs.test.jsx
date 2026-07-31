import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AccountRemovalDialog } from "./AccountRemovalDialog.jsx";

function triggerRef(label) {
  const button = document.createElement("button");
  button.textContent = label;
  document.body.append(button);
  button.focus();
  return {
    button,
    ref: { current: button },
  };
}

afterEach(() => {
  cleanup();
  document.querySelectorAll("[data-dialog-trigger]").forEach((element) => {
    element.remove();
  });
});

describe("AccountRemovalDialog accessibility contract", () => {
  const account = {
    accountId: "gmail-account",
    provider: "gmail",
    email: "owner@gmail.com",
  };

  it("associates its consequence, focuses Cancel, and returns to the account trigger", async () => {
    const user = userEvent.setup();
    const trigger = triggerRef("管理 owner@gmail.com");
    trigger.button.dataset.dialogTrigger = "true";
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    const view = render(
      <AccountRemovalDialog
        account={account}
        returnFocusRef={trigger.ref}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    );

    const dialog = screen.getByRole("alertdialog", {
      name: "移除 Gmail 账户？",
    });
    expect(dialog.getAttribute("aria-describedby")).toBe(
      "account-removal-address account-removal-description",
    );
    expect(screen.getByText("owner@gmail.com")).toBeTruthy();
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "取消" }),
    );

    await user.click(screen.getByRole("button", { name: "仅断开" }));
    expect(onConfirm).toHaveBeenCalledWith({
      revokeGoogleAuthorization: false,
      deleteLocalData: false,
    });

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledOnce();
    view.unmount();
    expect(document.activeElement).toBe(trigger.button);
  });

  it("locks every dismissal path and announces account removal while pending", () => {
    const onCancel = vi.fn();
    const { container } = render(
      <AccountRemovalDialog
        account={account}
        isRemoving
        onCancel={onCancel}
        onConfirm={vi.fn()}
      />,
    );

    const dialog = screen.getByRole("alertdialog");
    expect(dialog.getAttribute("aria-busy")).toBe("true");
    expect(document.activeElement).toBe(dialog);
    expect(screen.getByRole("status").textContent).toBe("正在移除账户…");
    fireEvent.keyDown(dialog, { key: "Escape" });
    fireEvent.pointerDown(container.querySelector(".confirm-layer"));
    expect(onCancel).not.toHaveBeenCalled();
  });
});
