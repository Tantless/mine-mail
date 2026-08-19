import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useThemeSchedule } from "./useThemeSchedule.js";
import { defaultThemeSchedule } from "../themeSchedule.js";

function Harness({ enabled, schedule, activeTheme, onApply }) {
  useThemeSchedule({ enabled, schedule, activeTheme, onApply });
  return null;
}

function currentAt(hours, minutes = 0, date = new Date(2026, 0, 15, hours, minutes, 0, 0)) {
  vi.setSystemTime(date);
  return date;
}

describe("useThemeSchedule", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("aligns to the current period when enabled", () => {
    const onApply = vi.fn();
    currentAt(19, 0);
    render(
      <Harness
        enabled
        schedule={defaultThemeSchedule}
        activeTheme={{ kind: "builtin", id: "daylight" }}
        onApply={onApply}
      />,
    );
    expect(onApply).toHaveBeenCalledWith("dusk");
  });

  it("does not apply when the active theme already matches the period", () => {
    const onApply = vi.fn();
    currentAt(19, 0);
    render(
      <Harness
        enabled
        schedule={defaultThemeSchedule}
        activeTheme={{ kind: "builtin", id: "dusk" }}
        onApply={onApply}
      />,
    );
    expect(onApply).not.toHaveBeenCalled();
  });

  it("applies again at the next boundary and keeps scheduling", () => {
    const onApply = vi.fn();
    const now = currentAt(19, 0);
    render(
      <Harness
        enabled
        schedule={defaultThemeSchedule}
        activeTheme={{ kind: "builtin", id: "daylight" }}
        onApply={onApply}
      />,
    );
    expect(onApply).toHaveBeenCalledWith("dusk");

    // Advance just past 21:00. The first boundary (21:00) fires night.
    act(() => {
      vi.setSystemTime(new Date(now.getTime() + 2 * 60 * 60 * 1000 + 1000));
      vi.advanceTimersByTime(2 * 60 * 60 * 1000 + 1000);
    });
    expect(onApply).toHaveBeenLastCalledWith("night");
  });

  it("does not override a manual selection before the next boundary", () => {
    const onApply = vi.fn();
    currentAt(12, 0);
    const { rerender } = render(
      <Harness
        enabled
        schedule={defaultThemeSchedule}
        activeTheme={{ kind: "builtin", id: "daylight" }}
        onApply={onApply}
      />,
    );
    expect(onApply).not.toHaveBeenCalled();

    rerender(
      <Harness
        enabled
        schedule={defaultThemeSchedule}
        activeTheme={{ kind: "builtin", id: "night" }}
        onApply={onApply}
      />,
    );
    // Manual selection stays until the next boundary at 18:00.
    expect(onApply).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(6 * 60 * 60 * 1000 + 1000);
    });
    expect(onApply).toHaveBeenCalledWith("dusk");
  });

  it("stops scheduling when disabled", () => {
    const onApply = vi.fn();
    const now = currentAt(19, 0);
    const { rerender } = render(
      <Harness
        enabled
        schedule={defaultThemeSchedule}
        activeTheme={{ kind: "builtin", id: "daylight" }}
        onApply={onApply}
      />,
    );
    expect(onApply).toHaveBeenCalledWith("dusk");

    rerender(
      <Harness
        enabled={false}
        schedule={defaultThemeSchedule}
        activeTheme={{ kind: "builtin", id: "daylight" }}
        onApply={onApply}
      />,
    );
    act(() => {
      vi.setSystemTime(new Date(now.getTime() + 3 * 60 * 60 * 1000));
      vi.advanceTimersByTime(3 * 60 * 60 * 1000);
    });
    expect(onApply).toHaveBeenCalledTimes(1);
  });

  it("ignores custom themes", () => {
    const onApply = vi.fn();
    currentAt(19, 0);
    render(
      <Harness
        enabled
        schedule={defaultThemeSchedule}
        activeTheme={{ kind: "custom", id: "preset-1" }}
        onApply={onApply}
      />,
    );
    expect(onApply).not.toHaveBeenCalled();
  });
});
