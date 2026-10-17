import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { formatEmailDate } from './formatEmailDate';

const SECONDS_PER_DAY = 24 * 60 * 60;
const MILLISECONDS_PER_SECOND = 1000;

/** Fixed "now": Wednesday, 2026-03-18 15:30:00 local time. */
const FIXED_NOW = new Date(2026, 2, 18, 15, 30, 0);

function toUnixSeconds(date: Date): number {
  return Math.floor(date.getTime() / MILLISECONDS_PER_SECOND);
}

function localDate(year: number, monthIndex: number, day: number, hour = 9, minute = 0): Date {
  return new Date(year, monthIndex, day, hour, minute, 0);
}

describe('formatEmailDate', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(FIXED_NOW);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('formats a timestamp from today as a 12-hour time with minutes', () => {
    const todayMorning = localDate(2026, 2, 18, 9, 5);
    const expected = todayMorning.toLocaleTimeString(undefined, {
      hour: 'numeric',
      minute: '2-digit',
      hour12: true,
    });

    expect(formatEmailDate(toUnixSeconds(todayMorning))).toBe(expected);
    expect(formatEmailDate(toUnixSeconds(todayMorning))).toMatch(/9:05/);
  });

  it('treats a timestamp just after local midnight today as today, not yesterday', () => {
    const justAfterMidnight = localDate(2026, 2, 18, 0, 1);
    expect(formatEmailDate(toUnixSeconds(justAfterMidnight))).not.toBe('Yesterday');
  });

  it('returns "Yesterday" for any time on the previous calendar day', () => {
    const lateYesterday = localDate(2026, 2, 17, 23, 59);
    const earlyYesterday = localDate(2026, 2, 17, 0, 0);

    expect(formatEmailDate(toUnixSeconds(lateYesterday))).toBe('Yesterday');
    expect(formatEmailDate(toUnixSeconds(earlyYesterday))).toBe('Yesterday');
  });

  it('returns the weekday name for dates 2 to 5 days ago', () => {
    const twoDaysAgo = localDate(2026, 2, 16); // Monday
    const fiveDaysAgo = localDate(2026, 2, 13); // Friday

    expect(formatEmailDate(toUnixSeconds(twoDaysAgo))).toBe(
      twoDaysAgo.toLocaleDateString(undefined, { weekday: 'long' }),
    );
    expect(formatEmailDate(toUnixSeconds(fiveDaysAgo))).toBe(
      fiveDaysAgo.toLocaleDateString(undefined, { weekday: 'long' }),
    );
  });

  it('returns a short month/day for dates older than 5 days in the current year', () => {
    const sixDaysAgo = localDate(2026, 2, 12);
    const expected = sixDaysAgo.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });

    expect(formatEmailDate(toUnixSeconds(sixDaysAgo))).toBe(expected);
    expect(formatEmailDate(toUnixSeconds(sixDaysAgo))).not.toMatch(/2026/);
  });

  it('includes the year for dates in a different calendar year', () => {
    const lastYear = localDate(2025, 11, 25);
    const expected = lastYear.toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
    });

    expect(formatEmailDate(toUnixSeconds(lastYear))).toBe(expected);
    expect(formatEmailDate(toUnixSeconds(lastYear))).toMatch(/2025/);
  });

  it('formats a future timestamp in the current year as a short date rather than a weekday', () => {
    const nextWeek = toUnixSeconds(FIXED_NOW) + 7 * SECONDS_PER_DAY;
    const expected = new Date(nextWeek * MILLISECONDS_PER_SECOND).toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
    });

    expect(formatEmailDate(nextWeek)).toBe(expected);
  });
});
