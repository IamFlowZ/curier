import { describe, expect, it } from 'vitest';
import { formatByteSize } from './formatByteSize';

describe('formatByteSize', () => {
  it('renders zero without a decimal', () => {
    expect(formatByteSize(0)).toBe('0 B');
  });

  it('renders one decimal place in binary units', () => {
    expect(formatByteSize(512)).toBe('512.0 B');
    expect(formatByteSize(1024)).toBe('1.0 KB');
    expect(formatByteSize(1536)).toBe('1.5 KB');
    expect(formatByteSize(5 * 1024 * 1024)).toBe('5.0 MB');
    expect(formatByteSize(2.25 * 1024 ** 3)).toBe('2.3 GB');
  });

  it('clamps to the largest known unit', () => {
    expect(formatByteSize(3 * 1024 ** 5)).toBe('3072.0 TB');
  });

  it('rejects negative and non-finite input', () => {
    expect(() => formatByteSize(-1)).toThrow(RangeError);
    expect(() => formatByteSize(Number.NaN)).toThrow(RangeError);
    expect(() => formatByteSize(Number.POSITIVE_INFINITY)).toThrow(RangeError);
  });
});
