import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ThemedSelect } from "./ThemedSelect.jsx";

const options = [
  { value: 1, label: "1 分钟" },
  { value: 3, label: "3 分钟" },
  { value: 5, label: "5 分钟" },
];

describe("ThemedSelect", () => {
  afterEach(() => cleanup());

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

    await user.click(screen.getByRole("combobox", { name: "完整校准间隔" }));
    expect(screen.getByRole("listbox", { name: "完整校准间隔" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "5 分钟" }).getAttribute("aria-selected")).toBe(
      "true",
    );

    await user.click(screen.getByRole("option", { name: "3 分钟" }));
    expect(onValueChange).toHaveBeenCalledWith(3);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("supports arrow navigation and Escape without changing the value", async () => {
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
    trigger.focus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("listbox")).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(onValueChange).not.toHaveBeenCalled();
  });
});
