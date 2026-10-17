import { describe, expect, it } from 'vitest';
import { shouldMapWheelToHorizontalScroll } from './tabStripScroll';

describe('shouldMapWheelToHorizontalScroll', () => {
  it('maps purely vertical wheel deltas (mouse wheel) to horizontal scrolling', () => {
    expect(shouldMapWheelToHorizontalScroll(0, 40)).toBe(true);
    expect(shouldMapWheelToHorizontalScroll(0, -40)).toBe(true);
  });

  it('leaves dominant horizontal gestures (trackpad) to native scrolling', () => {
    expect(shouldMapWheelToHorizontalScroll(30, 5)).toBe(false);
    expect(shouldMapWheelToHorizontalScroll(10, 10)).toBe(false);
  });

  it('ignores events with no vertical movement', () => {
    expect(shouldMapWheelToHorizontalScroll(0, 0)).toBe(false);
  });
});
