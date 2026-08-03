import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PermanentDeleteDialog } from "./PermanentDeleteDialog.jsx";

function invokingControl(label = "触发操作") {
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
  document.querySelectorAll("[data-confirm-test-trigger]").forEach((element) => {
    element.remove();
  });
});

describe("PermanentDeleteDialog", () => {
  it("states the irreversible consequence and uses the semantic danger action", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(
      <PermanentDeleteDialog
        open
        subject="季度结算"
        onCancel={vi.fn()}
        onConfirm={onConfirm}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "永久删除这封邮件？" }),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "这封邮件将从垃圾箱永久删除，且无法恢复。此操作每次都需要确认。",
      ),
    ).toBeTruthy();
    expect(screen.getByText("季度结算")).toBeTruthy();
    expect(
      screen.getByText("取消后，邮件会继续保留在垃圾箱中。"),
    ).toBeTruthy();

    const confirm = screen.getByRole("button", { name: "永久删除" });
    expect(confirm.classList.contains("confirm-dialog__danger-action")).toBe(
      true,
    );
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "取消" }),
    );

    await user.click(confirm);
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("blocks duplicate confirmation and every dismissal path while pending", () => {
    const trigger = invokingControl("打开永久删除确认");
    trigger.button.dataset.confirmTestTrigger = "true";
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    const view = render(
      <PermanentDeleteDialog
        open
        subject="待删除邮件"
        returnFocusRef={trigger.ref}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "永久删除" }));
    expect(onConfirm).toHaveBeenCalledOnce();

    view.rerender(
      <PermanentDeleteDialog
        open
        subject="待删除邮件"
        isPending
        returnFocusRef={trigger.ref}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    );

    const dialog = screen.getByRole("alertdialog");
    const layer = dialog.closest(".confirm-layer");
    const pendingConfirm = screen.getByRole("button", {
      name: "正在永久删除…",
    });
    expect(dialog.getAttribute("aria-busy")).toBe("true");
    const pendingStatus = screen.getByRole("status");
    expect(pendingStatus.textContent).toBe("正在永久删除…");
    expect(pendingStatus.getAttribute("aria-live")).toBe("polite");
    expect(pendingStatus.getAttribute("aria-atomic")).toBe("true");
    expect(document.activeElement).toBe(dialog);
    expect(pendingConfirm.disabled).toBe(true);
    expect(screen.getByRole("button", { name: "取消" }).disabled).toBe(true);
    expect(
      screen.getByRole("button", { name: "取消永久删除" }).disabled,
    ).toBe(true);

    fireEvent.click(pendingConfirm);
    fireEvent.keyDown(dialog, { key: "Escape" });
    fireEvent.pointerDown(layer);
    fireEvent.keyDown(dialog, { key: "Tab" });

    expect(onConfirm).toHaveBeenCalledOnce();
    expect(onCancel).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(dialog);

    view.unmount();
    expect(document.activeElement).toBe(trigger.button);
  });

  it("announces a failed irreversible action assertively without losing safe focus", () => {
    render(
      <PermanentDeleteDialog
        open
        subject="删除失败邮件"
        errorMessage="服务器拒绝了永久删除，请重试。"
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );

    const error = screen.getByRole("alert");
    expect(error.textContent).toBe("服务器拒绝了永久删除，请重试。");
    expect(error.getAttribute("aria-live")).toBe("assertive");
    expect(error.getAttribute("aria-atomic")).toBe("true");
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "取消" }),
    );
  });
});
