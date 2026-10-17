import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  estimateAllTabsMemory,
  estimateTabMemory,
  getStaleTabs,
  getTotalEstimatedMemory,
} from './memoryEstimator';
import type { AllTab, EmailTab, FolderTab, SettingsTab, Tab } from '../types/tabs';

const BASE_TAB_MEMORY_MB = 0.5;
const EMAIL_BODY_MEMORY_MB = 0.5;
const HTML_BODY_MULTIPLIER = 2;
const MEMORY_PER_LIST_ITEM_MB = 0.01;
const STALE_THRESHOLD_MS = 15 * 60 * 1000;
const FIXED_NOW_MS = 1_700_000_000_000;

function makeAllTab(lastAccessed: number = FIXED_NOW_MS): AllTab {
  return { id: 'all', type: 'all', title: 'All', closeable: false, lastAccessed };
}

function makeEmailTab(id: string, lastAccessed: number = FIXED_NOW_MS): EmailTab {
  return {
    id,
    type: 'email',
    title: 'Subject',
    accountId: 'acc',
    accountEmail: 'a@b.com',
    emailId: id,
    subject: 'Subject',
    fromAddress: 'x@y.com',
    date: 0,
    closeable: true,
    lastAccessed,
  };
}

function makeFolderTab(id: string, lastAccessed: number = FIXED_NOW_MS): FolderTab {
  return {
    id,
    type: 'folder',
    title: 'Folder',
    accountId: 'acc',
    accountEmail: 'a@b.com',
    folderId: 'INBOX',
    folderName: 'Inbox',
    closeable: true,
    lastAccessed,
  };
}

function makeSettingsTab(lastAccessed: number = FIXED_NOW_MS): SettingsTab {
  return { id: 'settings', type: 'settings', title: 'Settings', section: 'app', closeable: true, lastAccessed };
}

describe('memoryEstimator', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(FIXED_NOW_MS);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe('estimateTabMemory', () => {
    it('charges only the base cost for list tabs with no emails', () => {
      expect(estimateTabMemory(makeAllTab(), 0)).toBe(BASE_TAB_MEMORY_MB);
    });

    it('adds a per-item cost for list tabs', () => {
      expect(estimateTabMemory(makeFolderTab('f'), 100)).toBe(
        BASE_TAB_MEMORY_MB + 100 * MEMORY_PER_LIST_ITEM_MB,
      );
    });

    it('ignores the email count for email tabs and charges the body cost', () => {
      expect(estimateTabMemory(makeEmailTab('e'), 1000, false)).toBe(
        BASE_TAB_MEMORY_MB + EMAIL_BODY_MEMORY_MB,
      );
    });

    it('doubles the body cost for HTML emails', () => {
      expect(estimateTabMemory(makeEmailTab('e'), 0, true)).toBe(
        BASE_TAB_MEMORY_MB + EMAIL_BODY_MEMORY_MB * HTML_BODY_MULTIPLIER,
      );
    });

    it('charges only the base cost for tab types without content (settings)', () => {
      expect(estimateTabMemory(makeSettingsTab(), 50, true)).toBe(BASE_TAB_MEMORY_MB);
    });

    it('rounds to two decimal places', () => {
      expect(estimateTabMemory(makeFolderTab('f'), 3)).toBe(0.53);
    });
  });

  describe('estimateAllTabsMemory', () => {
    it('produces one estimate per tab carrying id, title and staleness', () => {
      const freshTab = makeEmailTab('fresh', FIXED_NOW_MS - STALE_THRESHOLD_MS);
      const staleTab = makeEmailTab('stale', FIXED_NOW_MS - STALE_THRESHOLD_MS - 1);

      const estimates = estimateAllTabsMemory([freshTab, staleTab]);

      expect(estimates).toHaveLength(2);
      expect(estimates[0]).toMatchObject({ tabId: 'fresh', tabTitle: 'Subject', isStale: false });
      expect(estimates[1]).toMatchObject({ tabId: 'stale', isStale: true });
    });

    it('treats email tabs as HTML-bodied for estimation', () => {
      const [estimate] = estimateAllTabsMemory([makeEmailTab('e')]);
      expect(estimate.estimatedMB).toBe(BASE_TAB_MEMORY_MB + EMAIL_BODY_MEMORY_MB * HTML_BODY_MULTIPLIER);
    });
  });

  describe('getTotalEstimatedMemory', () => {
    it('returns zero for no tabs', () => {
      expect(getTotalEstimatedMemory([])).toBe(0);
    });

    it('sums the individual estimates', () => {
      const tabs: Tab[] = [makeAllTab(), makeEmailTab('e')];
      const expected = estimateAllTabsMemory(tabs).reduce((sum, e) => sum + e.estimatedMB, 0);
      expect(getTotalEstimatedMemory(tabs)).toBe(expected);
    });
  });

  describe('getStaleTabs', () => {
    it('returns only closeable tabs not accessed within the stale threshold', () => {
      const staleAt = FIXED_NOW_MS - STALE_THRESHOLD_MS - 1;
      const nonCloseableStale = makeAllTab(staleAt);
      const closeableStale = makeEmailTab('stale', staleAt);
      const closeableFresh = makeEmailTab('fresh', FIXED_NOW_MS);
      const exactlyAtThreshold = makeEmailTab('edge', FIXED_NOW_MS - STALE_THRESHOLD_MS);

      const stale = getStaleTabs([nonCloseableStale, closeableStale, closeableFresh, exactlyAtThreshold]);

      expect(stale.map((t) => t.id)).toEqual(['stale']);
    });
  });
});
