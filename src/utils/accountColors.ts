import { invoke } from '@tauri-apps/api/core';

// Account color palette - 12 distinct, accessible colors
export const ACCOUNT_COLORS = [
  '#3b82f6', // Blue
  '#10b981', // Green
  '#f59e0b', // Amber
  '#ef4444', // Red
  '#8b5cf6', // Violet
  '#ec4899', // Pink
  '#06b6d4', // Cyan
  '#f97316', // Orange
  '#14b8a6', // Teal
  '#a855f7', // Purple
  '#84cc16', // Lime
  '#f43f5e', // Rose
] as const;

/** Cache for user-configured account colors (loaded from DB) */
let configuredColors: Record<string, string> = {};
let colorsLoaded = false;

/**
 * Load user-configured account colors from the database.
 * Call this on app startup or after changing a color.
 */
export async function loadAccountColors(): Promise<void> {
  try {
    const colors = await invoke<Array<[string, string]>>('get_account_colors');
    configuredColors = {};
    for (const [email, color] of colors) {
      configuredColors[email] = color;
    }
    colorsLoaded = true;
  } catch (err) {
    console.error('Failed to load account colors:', err);
  }
}

/**
 * Set a user-configured color for an account. Updates both local cache and DB.
 */
export async function setAccountColorConfig(accountEmail: string, color: string): Promise<void> {
  await invoke('set_account_color', { accountEmail, color });
  configuredColors[accountEmail] = color;
  window.dispatchEvent(new CustomEvent('curier:account-colors-changed'));
}

/**
 * Get the map of configured colors (for display in settings).
 */
export function getConfiguredColors(): Record<string, string> {
  return { ...configuredColors };
}

/**
 * Whether configured colors have been loaded from DB.
 */
export function areColorsLoaded(): boolean {
  return colorsLoaded;
}

/**
 * Simple hash function to convert a string to a number
 */
function hashString(str: string): number {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    const char = str.charCodeAt(i);
    hash = ((hash << 5) - hash) + char;
    hash = hash & hash; // Convert to 32-bit integer
  }
  return Math.abs(hash);
}

/**
 * Gets the primary color for an account ID.
 * Checks user-configured colors first, then falls back to hash-based auto-assignment.
 */
export function getAccountColor(accountId: string | null | undefined): string | null {
  if (!accountId) return null;

  // Check user-configured colors first
  if (configuredColors[accountId]) {
    return configuredColors[accountId];
  }

  const hash = hashString(accountId);
  const index = hash % ACCOUNT_COLORS.length;
  return ACCOUNT_COLORS[index];
}

/**
 * Gets an rgba color with specified opacity for an account
 */
export function getAccountColorLight(accountId: string | null | undefined, opacity: number): string | null {
  const color = getAccountColor(accountId);
  if (!color) return null;

  // Convert hex to rgb
  const r = parseInt(color.slice(1, 3), 16);
  const g = parseInt(color.slice(3, 5), 16);
  const b = parseInt(color.slice(5, 7), 16);

  return `rgba(${r}, ${g}, ${b}, ${opacity})`;
}

/**
 * Calculates the background color for an email list row with zebra striping
 */
/** Row background that inherits the list background in both light and dark themes. */
export const NEUTRAL_ROW_BACKGROUND = 'transparent';

export function getEmailListRowColor(
  accountId: string | null | undefined,
  index: number,
  isRead: boolean
): string {
  if (!accountId) return NEUTRAL_ROW_BACKGROUND;

  const isEven = index % 2 === 0;

  // Opacity levels for striping
  let opacity: number;
  if (isRead) {
    opacity = isEven ? 0 : 0.03; // Read: even = neutral, odd = 3%
  } else {
    opacity = isEven ? 0.06 : 0.09; // Unread: even = 6%, odd = 9%
  }

  if (opacity === 0) return NEUTRAL_ROW_BACKGROUND;

  return getAccountColorLight(accountId, opacity) || NEUTRAL_ROW_BACKGROUND;
}

/**
 * Extracts account ID from tab or email objects
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function extractAccountId(source: any): string | null {
  if (!source) return null;

  if (source.accountId) {
    return source.accountId;
  }

  if (source.account_email) {
    const parts = source.account_email.split('/');
    return parts[0] || null;
  }

  return null;
}
