import { create } from 'zustand';
import { nowInUnixSeconds } from '../utils/time';
import type { Tab, AllTab, AccountTab, FolderTab, EmailTab, FeedTab, ComposeTab, SettingsTab, SettingsSection, AccountType } from '../types/tabs';

interface TabState {
  tabs: Tab[];
  activeTabId: string;
  tabHistory: string[]; // Track tab navigation history for "return to previous"

  // Actions
  addTab: (tab: Tab) => void;
  removeTab: (tabId: string) => void;
  setActiveTab: (tabId: string) => void;
  getTabById: (tabId: string) => Tab | undefined;
  updateTabLastAccessed: (tabId: string) => void;

  // Tab creation helpers
  addAccountTab: (accountId: string, email: string, accountType: AccountType) => void;
  addFolderTab: (accountId: string, folderId: string, folderName: string, accountEmail: string) => void;
  addEmailTab: (accountId: string, emailId: string, subject: string, accountEmail: string, fromAddress: string, fromName?: string, date?: number) => void;
  addFeedTab: (feedId: number, feedName: string, accountScope: string | null) => void;
  addComposeTab: (accountEmail: string, replyTo?: string, replySubject?: string, inReplyTo?: string, replyQuotedBody?: string, replyFromName?: string, replyDate?: number) => void;
  openSettingsTab: (section?: SettingsSection) => void;

  // Find helpers (prevent duplicate tabs)
  findFolderTab: (accountId: string, folderId: string) => FolderTab | undefined;
  findEmailTab: (emailId: string) => EmailTab | undefined;
  findAccountTab: (accountId: string) => AccountTab | undefined;
  findFeedTab: (feedId: number) => FeedTab | undefined;

  // Computed getters
  getCloseableTabs: () => Tab[];
  getAccountTabs: () => AccountTab[];

  // Reordering
  reorderAccountTabs: (orderedAccountIds: string[]) => void;

  // View mode and page persistence
  setTabViewMode: (tabId: string, viewMode: 'list' | 'feeds') => void;
  setTabCurrentPage: (tabId: string, page: number) => void;

  // Navigation
  closeAndFocusPrevious: (tabId: string) => void;

  // Initialize with All tab and any saved accounts
  initialize: (accounts: { id: string; email: string; accountType: AccountType }[], accountTabOrder?: string[]) => void;
}

const ALL_TAB_ID = 'all';
const COMPOSE_TAB_ID_PREFIX = 'compose-';

/**
 * Monotonic sequence for compose tab ids. `Date.now()` was previously used,
 * which collides when two compose tabs are opened within the same millisecond
 * (e.g. a double-pressed shortcut).
 */
let composeTabSequence = 0;
const nextComposeTabId = (): string => {
  composeTabSequence += 1;
  return `${COMPOSE_TAB_ID_PREFIX}${composeTabSequence}`;
};

/**
 * Pick the tab to focus after `tabId` is closed: the most recent entry in the
 * navigation history that is not the closed tab, falling back to "All".
 */
const previousTabIdInHistory = (tabHistory: string[], tabId: string): { previousTabId: string; historyWithoutTab: string[] } => {
  const historyWithoutTab = tabHistory.filter((id) => id !== tabId);
  const previousTabId = historyWithoutTab.length > 0
    ? historyWithoutTab[historyWithoutTab.length - 1]
    : ALL_TAB_ID;
  return { previousTabId, historyWithoutTab };
};

// Create the initial "All" tab
const createAllTab = (): AllTab => ({
  id: ALL_TAB_ID,
  type: 'all',
  title: 'All',
  closeable: false,
  lastAccessed: Date.now(),
});

export const useTabStore = create<TabState>((set, get) => ({
  tabs: [createAllTab()],
  activeTabId: ALL_TAB_ID,
  tabHistory: [ALL_TAB_ID],

  addTab: (tab: Tab) => {
    set((state) => ({
      tabs: [...state.tabs, tab],
    }));
  },

  removeTab: (tabId: string) => {
    const state = get();
    const tab = state.tabs.find((t) => t.id === tabId);

    // Cannot remove non-closeable tabs
    if (!tab || !tab.closeable) return;

    set((state) => {
      const { previousTabId, historyWithoutTab } = previousTabIdInHistory(state.tabHistory, tabId);
      return {
        tabs: state.tabs.filter((t) => t.id !== tabId),
        tabHistory: historyWithoutTab,
        // Removing the active tab must not leave focus on a tab that no longer exists
        activeTabId: state.activeTabId === tabId ? previousTabId : state.activeTabId,
      };
    });
  },

  setActiveTab: (tabId: string) => {
    const state = get();
    const tab = state.tabs.find((t) => t.id === tabId);
    if (!tab) return;

    set((state) => {
      // Update tab history - add to end, avoid duplicates at the end
      const newHistory = state.tabHistory.filter((id) => id !== tabId);
      newHistory.push(tabId);

      return {
        activeTabId: tabId,
        tabHistory: newHistory,
        tabs: state.tabs.map((t) =>
          t.id === tabId ? { ...t, lastAccessed: Date.now() } : t
        ),
      };
    });
  },

  getTabById: (tabId: string) => {
    return get().tabs.find((t) => t.id === tabId);
  },

  updateTabLastAccessed: (tabId: string) => {
    set((state) => ({
      tabs: state.tabs.map((t) =>
        t.id === tabId ? { ...t, lastAccessed: Date.now() } : t
      ),
    }));
  },

  addAccountTab: (accountId: string, email: string, accountType: AccountType) => {
    const state = get();
    // Check if account tab already exists
    const existing = state.tabs.find(
      (t) => t.type === 'account' && (t as AccountTab).accountId === accountId
    );
    if (existing) {
      get().setActiveTab(existing.id);
      return;
    }

    const tab: AccountTab = {
      id: `account-${accountId}`,
      type: 'account',
      title: email,
      accountId,
      accountEmail: email, // Store the actual email for API calls
      accountType,
      closeable: false,
      lastAccessed: Date.now(),
    };

    set((state) => ({
      tabs: [...state.tabs, tab],
    }));
  },

  addFolderTab: (accountId: string, folderId: string, folderName: string, accountEmail: string) => {
    const state = get();
    // Check if folder tab already exists
    const existing = state.findFolderTab(accountId, folderId);
    if (existing) {
      get().setActiveTab(existing.id);
      return;
    }

    const tab: FolderTab = {
      id: `folder-${accountId}-${folderId}`,
      type: 'folder',
      title: `${accountEmail.split('@')[0]} - ${folderName}`,
      accountId,
      accountEmail,
      folderId,
      folderName,
      closeable: true,
      lastAccessed: Date.now(),
    };

    set((state) => ({
      tabs: [...state.tabs, tab],
      activeTabId: tab.id,
      tabHistory: [...state.tabHistory, tab.id],
    }));
  },

  addEmailTab: (accountId: string, emailId: string, subject: string, accountEmail: string, fromAddress: string, fromName?: string, date?: number) => {
    const state = get();
    // Check if email tab already exists
    const existing = state.findEmailTab(emailId);
    if (existing) {
      get().setActiveTab(existing.id);
      return;
    }

    // Truncate subject to max 25 chars
    const truncatedSubject = subject.length > 25 ? subject.substring(0, 22) + '...' : subject;

    const tab: EmailTab = {
      id: `email-${emailId}`,
      type: 'email',
      title: truncatedSubject || 'No Subject',
      accountId,
      accountEmail,
      emailId,
      subject,
      fromAddress,
      fromName,
      date: date ?? nowInUnixSeconds(),
      closeable: true,
      lastAccessed: Date.now(),
    };

    set((state) => ({
      tabs: [...state.tabs, tab],
      activeTabId: tab.id,
      tabHistory: [...state.tabHistory, tab.id],
    }));
  },

  openSettingsTab: (section: SettingsSection = 'app') => {
    const state = get();
    // If settings tab already exists, just activate it
    const existing = state.tabs.find((t) => t.type === 'settings');
    if (existing) {
      // Update section if different
      set((s) => ({
        tabs: s.tabs.map((t) =>
          t.type === 'settings' ? { ...t, section } as SettingsTab : t
        ),
        activeTabId: existing.id,
        tabHistory: [...s.tabHistory.filter((id) => id !== existing.id), existing.id],
      }));
      return;
    }

    const tab: SettingsTab = {
      id: 'settings',
      type: 'settings',
      title: 'Settings',
      section,
      closeable: true,
      lastAccessed: Date.now(),
    };

    set((s) => ({
      tabs: [...s.tabs, tab],
      activeTabId: tab.id,
      tabHistory: [...s.tabHistory, tab.id],
    }));
  },

  findFolderTab: (accountId: string, folderId: string) => {
    return get().tabs.find(
      (t) =>
        t.type === 'folder' &&
        (t as FolderTab).accountId === accountId &&
        (t as FolderTab).folderId === folderId
    ) as FolderTab | undefined;
  },

  findEmailTab: (emailId: string) => {
    return get().tabs.find(
      (t) => t.type === 'email' && (t as EmailTab).emailId === emailId
    ) as EmailTab | undefined;
  },

  findAccountTab: (accountId: string) => {
    return get().tabs.find(
      (t) => t.type === 'account' && (t as AccountTab).accountId === accountId
    ) as AccountTab | undefined;
  },

  findFeedTab: (feedId: number) => {
    return get().tabs.find(
      (t) => t.type === 'feed' && (t as FeedTab).feedId === feedId
    ) as FeedTab | undefined;
  },

  addFeedTab: (feedId: number, feedName: string, accountScope: string | null) => {
    const state = get();
    const existing = state.findFeedTab(feedId);
    if (existing) {
      get().setActiveTab(existing.id);
      return;
    }

    // The title is the feed name alone; the account is conveyed by the tab's
    // color bar and the tooltip (see `describeFeedTab`), so truncation never
    // hides the part that identifies the feed.
    const tab: FeedTab = {
      id: `feed-${feedId}`,
      type: 'feed',
      title: feedName,
      feedId,
      feedName,
      accountScope,
      closeable: true,
      lastAccessed: Date.now(),
    };

    set((state) => ({
      tabs: [...state.tabs, tab],
      activeTabId: tab.id,
      tabHistory: [...state.tabHistory, tab.id],
    }));
  },

  addComposeTab: (accountEmail: string, replyTo?: string, replySubject?: string, inReplyTo?: string, replyQuotedBody?: string, replyFromName?: string, replyDate?: number) => {
    const tabId = nextComposeTabId();
    const title = replyTo ? 'Reply' : 'New Email';
    const normalizedSubject = replySubject
      ? `Re: ${replySubject.replace(/^Re:\s*/i, '')}`
      : undefined;

    const tab: ComposeTab = {
      id: tabId,
      type: 'compose',
      title,
      accountEmail,
      replyTo,
      replySubject: normalizedSubject,
      inReplyTo,
      replyQuotedBody,
      replyFromName,
      replyDate,
      closeable: true,
      lastAccessed: Date.now(),
    };

    set((state) => ({
      tabs: [...state.tabs, tab],
      activeTabId: tab.id,
      tabHistory: [...state.tabHistory, tab.id],
    }));
  },

  getCloseableTabs: () => {
    return get().tabs.filter((t) => t.closeable);
  },

  getAccountTabs: () => {
    return get().tabs.filter((t) => t.type === 'account') as AccountTab[];
  },

  reorderAccountTabs: (orderedAccountIds: string[]) => {
    set((state) => {
      const allTab = state.tabs.filter((t) => t.type === 'all');
      const nonAccountTabs = state.tabs.filter((t) => t.type !== 'all' && t.type !== 'account');

      // Sort account tabs according to the provided order
      const accountTabs = state.tabs.filter((t) => t.type === 'account') as AccountTab[];
      const sorted = orderedAccountIds
        .map((id) => accountTabs.find((t) => t.accountId === id))
        .filter((t): t is AccountTab => t !== undefined);

      // Include any account tabs not in the ordered list (shouldn't happen, but safe)
      const remaining = accountTabs.filter((t) => !orderedAccountIds.includes(t.accountId));

      return { tabs: [...allTab, ...sorted, ...remaining, ...nonAccountTabs] };
    });
  },

  setTabViewMode: (tabId: string, viewMode: 'list' | 'feeds') => {
    set((state) => ({
      tabs: state.tabs.map((t) =>
        t.id === tabId ? { ...t, viewMode } : t
      ),
    }));
  },

  setTabCurrentPage: (tabId: string, page: number) => {
    set((state) => ({
      tabs: state.tabs.map((t) =>
        t.id === tabId ? { ...t, currentPage: page } : t
      ),
    }));
  },

  closeAndFocusPrevious: (tabId: string) => {
    const state = get();
    const tab = state.tabs.find((t) => t.id === tabId);

    // Cannot close non-closeable tabs
    if (!tab || !tab.closeable) return;

    set((state) => {
      const { previousTabId, historyWithoutTab } = previousTabIdInHistory(state.tabHistory, tabId);
      return {
        tabs: state.tabs.filter((t) => t.id !== tabId),
        activeTabId: previousTabId,
        tabHistory: historyWithoutTab,
      };
    });
  },

  initialize: (accounts, accountTabOrder) => {
    const allTab = createAllTab();
    const accountTabs: AccountTab[] = accounts.map((account) => ({
      id: `account-${account.id}`,
      type: 'account',
      title: account.email,
      accountId: account.id,
      accountEmail: account.email,
      accountType: account.accountType,
      closeable: false,
      lastAccessed: Date.now(),
    }));

    // Apply saved tab order if available
    let orderedAccountTabs = accountTabs;
    if (accountTabOrder && accountTabOrder.length > 0) {
      const sorted = accountTabOrder
        .map((email) => accountTabs.find((t) => t.accountEmail === email))
        .filter((t): t is AccountTab => t !== undefined);
      const remaining = accountTabs.filter((t) => !accountTabOrder.includes(t.accountEmail));
      orderedAccountTabs = [...sorted, ...remaining];
    }

    set({
      tabs: [allTab, ...orderedAccountTabs],
      activeTabId: ALL_TAB_ID,
      tabHistory: [ALL_TAB_ID],
    });
  },
}));
