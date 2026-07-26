import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { IconButton } from "./IconButton.jsx";
import { TooltipTarget } from "./Tooltip.jsx";

describe("IconButton tooltip", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("replaces the native title with a delayed app tooltip", () => {
    render(<IconButton label="返回邮件列表">返回</IconButton>);
    const button = screen.getByRole("button", { name: "返回邮件列表" });

    expect(button.getAttribute("title")).toBeNull();
    fireEvent.pointerEnter(button);
    act(() => vi.advanceTimersByTime(379));
    expect(screen.queryByRole("tooltip")).toBeNull();

    act(() => vi.advanceTimersByTime(1));
    expect(screen.getByRole("tooltip").textContent).toBe("返回邮件列表");

    fireEvent.pointerLeave(button);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("shows immediately for keyboard focus and closes with Escape", () => {
    render(<IconButton label="下一封">下一封</IconButton>);
    const button = screen.getByRole("button", { name: "下一封" });

    fireEvent.focus(button);
    expect(screen.getByRole("tooltip").textContent).toBe("下一封");

    fireEvent.keyDown(button, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("keeps unavailable toolbar actions explainable on pointer hover", () => {
    render(
      <IconButton label="归档（尚未实现）" disabled>
        归档
      </IconButton>,
    );
    const button = screen.getByRole("button", { name: "归档（尚未实现）" });

    fireEvent.pointerEnter(button);
    act(() => vi.advanceTimersByTime(380));

    expect(screen.getByRole("tooltip").textContent).toBe("归档（尚未实现）");
  });

  it("preserves consumer pointer and focus handlers", () => {
    const onPointerEnter = vi.fn();
    const onFocus = vi.fn();
    render(
      <IconButton label="筛选邮件" onPointerEnter={onPointerEnter} onFocus={onFocus}>
        筛选
      </IconButton>,
    );
    const button = screen.getByRole("button", { name: "筛选邮件" });

    fireEvent.pointerEnter(button);
    fireEvent.focus(button);

    expect(onPointerEnter).toHaveBeenCalledOnce();
    expect(onFocus).toHaveBeenCalledOnce();
  });

  it("uses a short themed tooltip without weakening the accessible name", () => {
    render(
      <IconButton
        label="切换到 mine@example.com"
        title="设为当前账户"
      >
        切换
      </IconButton>,
    );
    const button = screen.getByRole("button", {
      name: "切换到 mine@example.com",
    });

    expect(button.getAttribute("title")).toBeNull();
    fireEvent.pointerEnter(button);
    act(() => vi.advanceTimersByTime(380));

    expect(screen.getByRole("tooltip").textContent).toBe("设为当前账户");
  });

  it("replaces native titles on non-icon controls too", () => {
    render(
      <TooltipTarget label="完整时间">
        <time title="系统提示">刚刚</time>
      </TooltipTarget>,
    );
    const time = screen.getByText("刚刚");

    expect(time.getAttribute("title")).toBeNull();
    fireEvent.pointerEnter(time);
    act(() => vi.advanceTimersByTime(380));

    expect(screen.getByRole("tooltip").textContent).toBe("完整时间");
  });
});
