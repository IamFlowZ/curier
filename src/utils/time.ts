export const MILLISECONDS_PER_SECOND = 1000;

/**
 * Current time as whole Unix seconds. The Rust backend stores every
 * `created_at` / `updated_at` / email `date` as seconds, so any timestamp
 * the frontend stamps locally must use this helper to stay comparable.
 */
export function nowInUnixSeconds(): number {
  return Math.floor(Date.now() / MILLISECONDS_PER_SECOND);
}
