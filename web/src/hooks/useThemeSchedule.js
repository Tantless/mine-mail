import { useEffect, useRef } from "react";
import {
  nextScheduledBoundaryMs,
  resolveScheduledThemeId,
} from "../themeSchedule.js";

function scheduleKey(schedule) {
  return `${schedule?.dayStart ?? ""}|${schedule?.duskStart ?? ""}|${schedule?.nightStart ?? ""}`;
}

// Aligns the active theme with the local-time schedule while enabled. Applies
// the current period once on enable and again at every boundary via a
// self-resetting timeout. Manual theme changes are left alone until the next
// boundary: the schedule effect re-runs only when the enable flag or the
// schedule times change, and it skips non-builtin themes entirely.
export function useThemeSchedule({ enabled, schedule, activeTheme, onApply }) {
  const onApplyRef = useRef(onApply);
  useEffect(() => {
    onApplyRef.current = onApply;
  }, [onApply]);

  const activeThemeRef = useRef(activeTheme);
  useEffect(() => {
    activeThemeRef.current = activeTheme;
  }, [activeTheme]);

  const scheduleRef = useRef(schedule);
  useEffect(() => {
    scheduleRef.current = schedule;
  }, [schedule]);

  useEffect(() => {
    if (!enabled) return undefined;
    if (activeTheme?.kind !== "builtin") return undefined;

    let timer = null;
    const applyAt = (now) => {
      const themeId = resolveScheduledThemeId(scheduleRef.current, now);
      const currentThemeId = activeThemeRef.current?.kind === "builtin"
        ? activeThemeRef.current.id
        : null;
      if (themeId !== currentThemeId) {
        onApplyRef.current(themeId);
      }
      const nextBoundary = nextScheduledBoundaryMs(scheduleRef.current, now);
      if (nextBoundary === null) return;
      const delay = Math.max(0, nextBoundary - now.getTime());
      timer = window.setTimeout(() => {
        applyAt(new Date());
      }, delay);
    };

    applyAt(new Date());
    return () => {
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [enabled, scheduleKey(schedule)]);
}
