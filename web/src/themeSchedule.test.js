import { describe, expect, it } from "vitest";
import {
  defaultThemeSchedule,
  minutesSinceMidnight,
  nextScheduledBoundaryMs,
  normalizeThemeSchedule,
  resolveScheduledThemeId,
  themeScheduleIssue,
} from "./themeSchedule.js";

const defaultSchedule = { ...defaultThemeSchedule };

function at(hours, minutes = 0) {
  return new Date(2026, 0, 15, hours, minutes, 0, 0);
}

describe("theme schedule pure helpers", () => {
  it("parses and rejects HH:MM wall-clock values", () => {
    expect(minutesSinceMidnight("06:00")).toBe(360);
    expect(minutesSinceMidnight("00:00")).toBe(0);
    expect(minutesSinceMidnight("23:59")).toBe(1439);
    expect(minutesSinceMidnight("6:00")).toBeNull();
    expect(minutesSinceMidnight("24:00")).toBeNull();
    expect(minutesSinceMidnight("06:60")).toBeNull();
    expect(minutesSinceMidnight("06:0")).toBeNull();
    expect(minutesSinceMidnight("")).toBeNull();
  });

  it("resolves the active built-in theme across every boundary", () => {
    const schedule = { ...defaultSchedule };
    expect(resolveScheduledThemeId(schedule, at(0))).toBe("night");
    expect(resolveScheduledThemeId(schedule, at(5, 59))).toBe("night");
    expect(resolveScheduledThemeId(schedule, at(6))).toBe("daylight");
    expect(resolveScheduledThemeId(schedule, at(12))).toBe("daylight");
    expect(resolveScheduledThemeId(schedule, at(17, 59))).toBe("daylight");
    expect(resolveScheduledThemeId(schedule, at(18))).toBe("dusk");
    expect(resolveScheduledThemeId(schedule, at(20, 59))).toBe("dusk");
    expect(resolveScheduledThemeId(schedule, at(21))).toBe("night");
    expect(resolveScheduledThemeId(schedule, at(23, 59))).toBe("night");
  });

  it("falls back to safe defaults for invalid schedule values", () => {
    const normalized = normalizeThemeSchedule({
      dayStart: "bad",
      duskStart: "18:00",
      nightStart: "21:00",
    });
    expect(normalized).toEqual(defaultSchedule);

    const unordered = normalizeThemeSchedule({
      dayStart: "21:00",
      duskStart: "18:00",
      nightStart: "06:00",
    });
    expect(unordered).toEqual(defaultSchedule);

    expect(
      normalizeThemeSchedule({
        dayStart: "07:00",
        duskStart: "18:00",
        nightStart: "21:00",
      }),
    ).toEqual({ dayStart: "07:00", duskStart: "18:00", nightStart: "21:00" });
  });

  it("computes the next boundary strictly after now", () => {
    const schedule = { ...defaultSchedule };
    expect(nextScheduledBoundaryMs(schedule, at(0))).toBe(at(6).getTime());
    expect(nextScheduledBoundaryMs(schedule, at(6))).toBe(at(18).getTime());
    expect(nextScheduledBoundaryMs(schedule, at(18))).toBe(at(21).getTime());
    expect(nextScheduledBoundaryMs(schedule, at(21))).toBe(
      at(6).getTime() + 24 * 60 * 60 * 1000,
    );
    expect(nextScheduledBoundaryMs(schedule, at(23, 59))).toBe(
      at(6).getTime() + 24 * 60 * 60 * 1000,
    );
  });

  it("returns null when the schedule is invalid", () => {
    expect(nextScheduledBoundaryMs({ dayStart: "x", duskStart: "18:00", nightStart: "21:00" }, at(12))).toBeNull();
  });

  it("explains unordered or malformed schedules in Chinese", () => {
    expect(
      themeScheduleIssue({ dayStart: "06:00", duskStart: "18:00", nightStart: "21:00" }),
    ).toBeNull();
    expect(
      themeScheduleIssue({ dayStart: "17:00", duskStart: "18:00", nightStart: "21:00" }),
    ).toBeNull();
    expect(
      themeScheduleIssue({ dayStart: "21:00", duskStart: "18:00", nightStart: "06:00" }),
    ).toMatch(/日间开始时间需要早于黄昏开始时间/);
    expect(
      themeScheduleIssue({ dayStart: "06:00", duskStart: "22:00", nightStart: "21:00" }),
    ).toMatch(/黄昏开始时间需要早于夜间开始时间/);
    expect(
      themeScheduleIssue({ dayStart: "06:00", duskStart: "bad", nightStart: "21:00" }),
    ).toMatch(/HH:MM/);
  });
});
