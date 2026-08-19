import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ThemedSelect } from "./ThemedSelect.jsx";

const options = [
  { value: 1, label: "1 分钟" },
  { value: 3, label: "3 分钟" },
  { value: 5, label: "5 分钟" },
];

describe("ThemedSelect", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("opens a themed listbox and reports the selected value", async () => {
    const onValueChange = vi.fn();
    const user = userEvent.setup();
    render(
      <ThemedSelect
        label="完整校准间隔"
        value={5}
        options={options}
        onValueChange={onValueChange}
      />,
    );

    const trigger = screen.getByRole("combobox", { name: "完整校准间隔" });
    await user.click(trigger);
    expect(screen.getByRole("listbox", { name: "完整校准间隔" }).style.maxHeight).toBe(
      "166px",
    );
    expect(screen.getByRole("option", { name: "5 分钟" }).getAttribute("aria-selected")).toBe(
      "true",
    );

    await user.click(screen.getByRole("option", { name: "3 分钟" }));
    expect(onValueChange).toHaveBeenCalledWith(3);
    expect(screen.queryByRole("listbox")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it("does not steal focus moved outside before deferred trigger focus", async () => {
    const frames = new Map();
    let nextFrameId = 1;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      const frameId = nextFrameId;
      nextFrameId += 1;
      frames.set(frameId, callback);
      return frameId;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((frameId) => {
      frames.delete(frameId);
    });
    const user = userEvent.setup();
    render(
      <>
        <ThemedSelect
          label="完整校准间隔"
          value={5}
          options={options}
          onValueChange={vi.fn()}
        />
        <input aria-label="后续输入" />
      </>,
    );

    await user.click(screen.getByRole("combobox", { name: "完整校准间隔" }));
    await user.click(screen.getByRole("option", { name: "3 分钟" }));
    const nextInput = screen.getByRole("textbox", { name: "后续输入" });
    nextInput.focus();

    const pendingFrames = [...frames.values()];
    frames.clear();
    act(() => pendingFrames.forEach((callback) => callback(16)));

    expect(document.activeElement).toBe(nextInput);
  });

  it("supports Escape before deferred option focus without changing the value", async () => {
    const onValueChange = vi.fn();
    const user = userEvent.setup();
    vi.spyOn(window, "requestAnimationFrame").mockReturnValue(1);
    render(
      <ThemedSelect
        label="完整校准间隔"
        value={5}
        options={options}
        onValueChange={onValueChange}
      />,
    );

    const trigger = screen.getByRole("combobox", { name: "完整校准间隔" });
    trigger.focus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("listbox")).toBeTruthy();
    expect(document.activeElement).toBe(trigger);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(onValueChange).not.toHaveBeenCalled();
  });

  it("renders an optional font preview without changing the accessible option name", async () => {
    const user = userEvent.setup();
    render(
      <ThemedSelect
        label="字体"
        value="sans"
        options={[
          {
            value: "sans",
            label: "清晰黑体",
            previewFontFamily: "Noto Sans SC Variable, sans-serif",
          },
        ]}
        onValueChange={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("combobox", { name: "字体" }));
    expect(
      screen.getByRole("combobox", { name: "字体" }).querySelector("span")?.style
        .fontFamily,
    ).toContain("Noto Sans SC Variable");
    const option = screen.getByRole("option", { name: "清晰黑体" });
    expect(
      option.querySelector(".themed-select__option-label")?.style.fontFamily,
    ).toContain("Noto Sans SC Variable");
  });

  it("keeps the menu above a minimized compose bar", async () => {
    const user = userEvent.setup();
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function getBoundingClientRect() {
        if (this.matches?.('.compose-panel[data-minimized="true"]')) {
          return {
            top: 300,
            right: 500,
            bottom: 344,
            left: 160,
            width: 340,
            height: 44,
            x: 160,
            y: 340,
            toJSON: () => ({}),
          };
        }
        if (this.matches?.(".themed-select")) {
          return {
            top: 100,
            right: 260,
            bottom: 140,
            left: 60,
            width: 200,
            height: 40,
            x: 60,
            y: 100,
            toJSON: () => ({}),
          };
        }
        return {
          top: 0,
          right: 0,
          bottom: 0,
          left: 0,
          width: 0,
          height: 0,
          x: 0,
          y: 0,
          toJSON: () => ({}),
        };
      },
    );

    render(
      <>
        <div className="compose-panel" data-minimized="true" />
        <ThemedSelect
          label="翻译语言"
          value={1}
          options={options}
          onValueChange={vi.fn()}
        />
      </>,
    );

    await user.click(screen.getByRole("combobox", { name: "翻译语言" }));
    const menu = screen.getByRole("listbox", { name: "翻译语言" });

    await waitFor(() => {
      expect(menu.style.maxHeight).toBe("143px");
    });
  });

  it("uses the available space above for upward menus", async () => {
    const user = userEvent.setup();
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function getBoundingClientRect() {
        if (this.matches?.(".themed-select")) {
          return {
            top: 260,
            right: 260,
            bottom: 300,
            left: 60,
            width: 200,
            height: 40,
            x: 60,
            y: 260,
            toJSON: () => ({}),
          };
        }
        return {
          top: 0,
          right: 0,
          bottom: 0,
          left: 0,
          width: 0,
          height: 0,
          x: 0,
          y: 0,
          toJSON: () => ({}),
        };
      },
    );

    render(
      <ThemedSelect
        label="AI 模型"
        value={1}
        options={options}
        menuPlacement="above"
        preferredMaxHeight={204}
        onValueChange={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("combobox", { name: "AI 模型" }));
    const menu = screen.getByRole("listbox", { name: "AI 模型" });

    await waitFor(() => {
      expect(menu.style.maxHeight).toBe("204px");
    });
  });
});
