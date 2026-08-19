// Pure helpers for the time-based built-in theme schedule. The schedule maps
// wall-clock boundaries in minutes since midnight to the Daylight, Dusk, and
// Night built-in themes, with Night crossing midnight back to the Day start.

export const defaultThemeSchedule = Object.freeze({
  dayStart: "06:00",
  duskStart: "18:00",
  nightStart: "21:00",
});

const SCHEDULE_TIME_PATTERN = /^([01]\d|2[0-3]):[0-5]\d$/;

export function scheduleTimePattern() {
  return SCHEDULE_TIME_PATTERN;
}

export function minutesSinceMidnight(time) {
  if (!SCHEDULE_TIME_PATTERN.test(time)) return null;
  const [hours, minutes] = time.split(":").map(Number);
  return hours * 60 + minutes;
}

function scheduleStartsValid(schedule) {
  const day = minutesSinceMidnight(schedule.dayStart);
  const dusk = minutesSinceMidnight(schedule.duskStart);
  const night = minutesSinceMidnight(schedule.nightStart);
  if (day === null || dusk === null || night === null) return false;
  return day < dusk && dusk < night;
}

// Returns a friendly Chinese explanation of why the schedule cannot be
// applied, or null when every boundary is valid and strictly ordered. The UI
// refuses invalid schedules before they reach the save path, so callers never
// have to round-trip an error from the persistence layer.
export function themeScheduleIssue(schedule) {
  const day = minutesSinceMidnight(schedule?.dayStart);
  const dusk = minutesSinceMidnight(schedule?.duskStart);
  const night = minutesSinceMidnight(schedule?.nightStart);
  if (day === null || dusk === null || night === null) {
    return "时间格式不正确，请使用 HH:MM（例如 06:00）。";
  }
  if (day >= dusk) return "日间开始时间需要早于黄昏开始时间。";
  if (dusk >= night) return "黄昏开始时间需要早于夜间开始时间。";
  return null;
}

export function normalizeThemeSchedule(schedule, fallback = defaultThemeSchedule) {
  const candidate = {
    dayStart: minutesSinceMidnight(schedule?.dayStart) === null
      ? fallback.dayStart
      : schedule.dayStart,
    duskStart: minutesSinceMidnight(schedule?.duskStart) === null
      ? fallback.duskStart
      : schedule.duskStart,
    nightStart: minutesSinceMidnight(schedule?.nightStart) === null
      ? fallback.nightStart
      : schedule.nightStart,
  };
  return scheduleStartsValid(candidate) ? candidate : { ...fallback };
}

// Resolves the built-in theme id active at `now` (a Date) for the schedule.
// Day occupies [dayStart, duskStart), Dusk [duskStart, nightStart), and Night
// [nightStart, 24:00) plus [00:00, dayStart) across midnight.
export function resolveScheduledThemeId(schedule, now = new Date()) {
  const { dayStart, duskStart, nightStart } = normalizeThemeSchedule(schedule);
  const minute = now.getHours() * 60 + now.getMinutes();
  if (minute < minutesSinceMidnight(dayStart)) return "night";
  if (minute < minutesSinceMidnight(duskStart)) return "daylight";
  if (minute < minutesSinceMidnight(nightStart)) return "dusk";
  return "night";
}

// Returns the epoch milliseconds of the next schedule boundary strictly after
// `now`, or null when the schedule is invalid.
export function nextScheduledBoundaryMs(schedule, now = new Date()) {
  const normalized = normalizeThemeSchedule(schedule);
  if (normalized.dayStart !== schedule?.dayStart
    || normalized.duskStart !== schedule?.duskStart
    || normalized.nightStart !== schedule?.nightStart) {
    return null;
  }
  const day = minutesSinceMidnight(normalized.dayStart);
  const dusk = minutesSinceMidnight(normalized.duskStart);
  const night = minutesSinceMidnight(normalized.nightStart);
  const minute = now.getHours() * 60 + now.getMinutes();
  const nextMinute = [day, dusk, night]
    .map((boundary) => boundary - minute)
    .filter((delta) => delta > 0)
    .reduce((smallest, delta) => Math.min(smallest, delta), Infinity);
  const resolved =
    nextMinute === Infinity
      ? day + 24 * 60 // next day's dayStart, in minutes since today midnight
      : minute + nextMinute;
  const next = new Date(now);
  next.setHours(0, 0, 0, 0);
  next.setMinutes(resolved);
  return next.getTime();
}
