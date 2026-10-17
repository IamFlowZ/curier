const BYTES_PER_UNIT = 1024;
const BYTE_SIZE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;
const DISPLAYED_DECIMAL_PLACES = 1;
const ZERO_BYTES_LABEL = '0 B';

/**
 * Format a byte count for display using binary (1024-based) units with one
 * decimal place, e.g. `1536` -> `"1.5 KB"`. Zero is rendered as `"0 B"`.
 *
 * @throws {RangeError} if `bytes` is negative or not a finite number.
 */
export function formatByteSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) {
    throw new RangeError(`formatByteSize expects a non-negative finite number, received ${bytes}`);
  }
  if (bytes === 0) return ZERO_BYTES_LABEL;

  const largestUnitIndex = BYTE_SIZE_UNITS.length - 1;
  const unitIndex = Math.min(
    Math.floor(Math.log(bytes) / Math.log(BYTES_PER_UNIT)),
    largestUnitIndex,
  );
  const scaled = bytes / Math.pow(BYTES_PER_UNIT, unitIndex);
  return `${scaled.toFixed(DISPLAYED_DECIMAL_PLACES)} ${BYTE_SIZE_UNITS[unitIndex]}`;
}
