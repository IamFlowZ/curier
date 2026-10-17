/**
 * Decide whether a wheel event on the horizontal tab strip should be
 * translated into horizontal scrolling. Mouse wheels only emit vertical
 * deltas; trackpad gestures with a dominant horizontal component already
 * scroll the strip natively and must be left alone.
 */
export function shouldMapWheelToHorizontalScroll(deltaX: number, deltaY: number): boolean {
  return deltaY !== 0 && Math.abs(deltaY) > Math.abs(deltaX);
}
