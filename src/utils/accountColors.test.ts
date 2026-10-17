import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import {
  ACCOUNT_COLORS,
  areColorsLoaded,
  extractAccountId,
  getAccountColor,
  getAccountColorLight,
  getConfiguredColors,
  getEmailListRowColor,
  loadAccountColors,
  setAccountColorConfig,
  NEUTRAL_ROW_BACKGROUND,
} from './accountColors';

const HEX_COLOR_PATTERN = /^#[0-9a-f]{6}$/;
const ACCOUNT_A = 'alice@example.com';
const ACCOUNT_B = 'bob@example.com';
const CUSTOM_COLOR = '#123456';

describe('accountColors', () => {
  beforeEach(async () => {
    // Clear any configured colors cached by a prior test via the public API.
    vi.mocked(invoke).mockResolvedValueOnce([]);
    await loadAccountColors();
  });

  describe('getAccountColor', () => {
    it('returns null for null, undefined, and empty account ids', () => {
      expect(getAccountColor(null)).toBeNull();
      expect(getAccountColor(undefined)).toBeNull();
      expect(getAccountColor('')).toBeNull();
    });

    it('returns a color from the palette and is deterministic for the same id', () => {
      const first = getAccountColor(ACCOUNT_A);
      const second = getAccountColor(ACCOUNT_A);

      expect(first).toBe(second);
      expect(first).toMatch(HEX_COLOR_PATTERN);
      expect(ACCOUNT_COLORS).toContain(first);
    });

    it('maps every id into the palette (collisions are allowed, out-of-range is not)', () => {
      const ids = Array.from({ length: 100 }, (_, i) => `user${i}@example.com`);
      for (const id of ids) {
        expect(ACCOUNT_COLORS).toContain(getAccountColor(id));
      }
    });

    it('produces different colors for ids that hash to different buckets', () => {
      const distinctColors = new Set(
        Array.from({ length: 50 }, (_, i) => getAccountColor(`account-${i}`)),
      );
      expect(distinctColors.size).toBeGreaterThan(1);
    });

    it('prefers a user-configured color over the hash-based one', async () => {
      vi.mocked(invoke).mockResolvedValueOnce([[ACCOUNT_A, CUSTOM_COLOR]]);
      await loadAccountColors();

      expect(getAccountColor(ACCOUNT_A)).toBe(CUSTOM_COLOR);
      expect(ACCOUNT_COLORS).toContain(getAccountColor(ACCOUNT_B));
    });
  });

  describe('loadAccountColors', () => {
    it('invokes get_account_colors, caches the result, and marks colors as loaded', async () => {
      vi.mocked(invoke).mockResolvedValueOnce([
        [ACCOUNT_A, CUSTOM_COLOR],
        [ACCOUNT_B, '#abcdef'],
      ]);

      await loadAccountColors();

      expect(invoke).toHaveBeenCalledWith('get_account_colors');
      expect(getConfiguredColors()).toEqual({ [ACCOUNT_A]: CUSTOM_COLOR, [ACCOUNT_B]: '#abcdef' });
      expect(areColorsLoaded()).toBe(true);
    });

    it('swallows backend errors and logs them instead of throwing', async () => {
      const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
      vi.mocked(invoke).mockRejectedValueOnce(new Error('db unavailable'));

      await expect(loadAccountColors()).resolves.toBeUndefined();
      expect(consoleError).toHaveBeenCalled();
    });

    it('returns a defensive copy from getConfiguredColors', async () => {
      vi.mocked(invoke).mockResolvedValueOnce([[ACCOUNT_A, CUSTOM_COLOR]]);
      await loadAccountColors();

      const copy = getConfiguredColors();
      copy[ACCOUNT_A] = '#000000';

      expect(getAccountColor(ACCOUNT_A)).toBe(CUSTOM_COLOR);
    });
  });

  describe('setAccountColorConfig', () => {
    it('persists via set_account_color, updates the cache, and dispatches a change event', async () => {
      const listener = vi.fn();
      window.addEventListener('curier:account-colors-changed', listener);

      await setAccountColorConfig(ACCOUNT_A, CUSTOM_COLOR);

      expect(invoke).toHaveBeenCalledWith('set_account_color', {
        accountEmail: ACCOUNT_A,
        color: CUSTOM_COLOR,
      });
      expect(getAccountColor(ACCOUNT_A)).toBe(CUSTOM_COLOR);
      expect(listener).toHaveBeenCalledTimes(1);

      window.removeEventListener('curier:account-colors-changed', listener);
    });

    it('does not update the cache when the backend call rejects', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('write failed'));
      const hashColor = getAccountColor(ACCOUNT_A);

      await expect(setAccountColorConfig(ACCOUNT_A, CUSTOM_COLOR)).rejects.toThrow('write failed');
      expect(getAccountColor(ACCOUNT_A)).toBe(hashColor);
    });
  });

  describe('getAccountColorLight', () => {
    it('returns null when no account id is given', () => {
      expect(getAccountColorLight(null, 0.5)).toBeNull();
    });

    it('converts the hex color to an rgba string with the given opacity', async () => {
      vi.mocked(invoke).mockResolvedValueOnce([[ACCOUNT_A, '#ff8000']]);
      await loadAccountColors();

      expect(getAccountColorLight(ACCOUNT_A, 0.25)).toBe('rgba(255, 128, 0, 0.25)');
    });
  });

  describe('getEmailListRowColor', () => {
    const READ = true;
    const UNREAD = false;

    beforeEach(async () => {
      vi.mocked(invoke).mockResolvedValueOnce([[ACCOUNT_A, '#ff0000']]);
      await loadAccountColors();
    });

    it('returns white when there is no account id', () => {
      expect(getEmailListRowColor(null, 0, UNREAD)).toBe(NEUTRAL_ROW_BACKGROUND);
    });

    it('returns white for read emails on even rows', () => {
      expect(getEmailListRowColor(ACCOUNT_A, 0, READ)).toBe(NEUTRAL_ROW_BACKGROUND);
      expect(getEmailListRowColor(ACCOUNT_A, 2, READ)).toBe(NEUTRAL_ROW_BACKGROUND);
    });

    it('uses a 3% tint for read emails on odd rows', () => {
      expect(getEmailListRowColor(ACCOUNT_A, 1, READ)).toBe('rgba(255, 0, 0, 0.03)');
    });

    it('uses 6% and 9% tints for unread emails on even and odd rows', () => {
      expect(getEmailListRowColor(ACCOUNT_A, 0, UNREAD)).toBe('rgba(255, 0, 0, 0.06)');
      expect(getEmailListRowColor(ACCOUNT_A, 1, UNREAD)).toBe('rgba(255, 0, 0, 0.09)');
    });
  });

  describe('extractAccountId', () => {
    it('returns null for falsy sources', () => {
      expect(extractAccountId(null)).toBeNull();
      expect(extractAccountId(undefined)).toBeNull();
    });

    it('prefers an explicit accountId property', () => {
      expect(extractAccountId({ accountId: 'acc-1', account_email: 'x@y.com/INBOX' })).toBe('acc-1');
    });

    it('takes the segment before the first slash of account_email', () => {
      expect(extractAccountId({ account_email: 'alice@example.com/INBOX' })).toBe('alice@example.com');
      expect(extractAccountId({ account_email: 'alice@example.com' })).toBe('alice@example.com');
    });

    it('returns null when neither property is usable', () => {
      expect(extractAccountId({})).toBeNull();
      expect(extractAccountId({ account_email: '' })).toBeNull();
      expect(extractAccountId({ account_email: '/INBOX' })).toBeNull();
    });
  });
});
