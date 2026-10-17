import type { Tab } from '../types/tabs';

// Estimated memory usage per tab type (in MB)
const BASE_TAB_MEMORY = 0.5; // Base memory for any tab
const EMAIL_LIST_MEMORY_PER_ITEM = 0.01; // ~10KB per email in list
const EMAIL_BODY_MEMORY = 0.5; // Average email body
const HTML_BODY_MULTIPLIER = 2.0; // HTML is heavier than plain text

interface TabMemoryEstimate {
  tabId: string;
  tabTitle: string;
  estimatedMB: number;
  lastAccessed: number;
  isStale: boolean; // Not accessed in last 15 minutes
}

const STALE_THRESHOLD_MS = 15 * 60 * 1000; // 15 minutes

export function estimateTabMemory(tab: Tab, emailCount: number = 0, hasHtmlBody: boolean = false): number {
  let estimate = BASE_TAB_MEMORY;

  switch (tab.type) {
    case 'all':
    case 'account':
    case 'folder':
      // List view tabs have memory based on email count
      estimate += emailCount * EMAIL_LIST_MEMORY_PER_ITEM;
      break;
    case 'email':
      // Email tabs have the body content
      estimate += EMAIL_BODY_MEMORY * (hasHtmlBody ? HTML_BODY_MULTIPLIER : 1);
      break;
  }

  return Math.round(estimate * 100) / 100; // Round to 2 decimal places
}

export function estimateAllTabsMemory(tabs: Tab[]): TabMemoryEstimate[] {
  const now = Date.now();

  return tabs.map((tab) => {
    // Simplified estimation - in a real app, we'd track actual email counts
    const emailCount = tab.type === 'all' ? 30 : tab.type === 'account' ? 20 : tab.type === 'folder' ? 20 : 0;
    const hasHtmlBody = tab.type === 'email';

    return {
      tabId: tab.id,
      tabTitle: tab.title,
      estimatedMB: estimateTabMemory(tab, emailCount, hasHtmlBody),
      lastAccessed: tab.lastAccessed,
      isStale: now - tab.lastAccessed > STALE_THRESHOLD_MS,
    };
  });
}

export function getTotalEstimatedMemory(tabs: Tab[]): number {
  const estimates = estimateAllTabsMemory(tabs);
  return estimates.reduce((sum, e) => sum + e.estimatedMB, 0);
}

export function getStaleTabs(tabs: Tab[]): Tab[] {
  const now = Date.now();
  return tabs.filter((tab) => tab.closeable && now - tab.lastAccessed > STALE_THRESHOLD_MS);
}
