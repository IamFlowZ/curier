import { afterEach, describe, expect, it, vi } from 'vitest';
import { MILLISECONDS_PER_SECOND, nowInUnixSeconds } from './time';

describe('nowInUnixSeconds', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns the current time in whole seconds', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_700_000_000_999);
    expect(nowInUnixSeconds()).toBe(1_700_000_000);
  });

  it('uses a 1000 ms second', () => {
    expect(MILLISECONDS_PER_SECOND).toBe(1000);
  });
});
