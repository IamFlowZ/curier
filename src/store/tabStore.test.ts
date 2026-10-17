import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useTabStore } from './tabStore';
import { createStoreReset } from '../test/resetStore';
import type { AccountTab, ComposeTab, EmailTab, FeedTab, FolderTab, SettingsTab } from '../types/tabs';

const ALL_TAB_ID = 'all';
const SETTINGS_TAB_ID = 'settings';
const ACCOUNT_ID = 'acc-1';
const ACCOUNT_EMAIL = 'alice@example.com';
const OTHER_ACCOUNT_ID = 'acc-2';
const OTHER_ACCOUNT_EMAIL = 'bob@example.com';
const FOLDER_ID = 'INBOX';
const FOLDER_NAME = 'Inbox';
const EMAIL_ID = '42';
const FEED_ID = 7;
const FIXED_NOW_MS = 1_700_000_000_000;

const resetTabStore = createStoreReset(useTabStore);

function store() {
  return useTabStore.getState();
}

function tabIds(): string[] {
  return store().tabs.map((t) => t.id);
}

function openFolder(): FolderTab {
  store().addFolderTab(ACCOUNT_ID, FOLDER_ID, FOLDER_NAME, ACCOUNT_EMAIL);
  const tab = store().findFolderTab(ACCOUNT_ID, FOLDER_ID);
  if (!tab) throw new Error('folder tab was not created');
  return tab;
}

function openEmail(emailId: string = EMAIL_ID, subject: string = 'Hello'): EmailTab {
  store().addEmailTab(ACCOUNT_ID, emailId, subject, ACCOUNT_EMAIL, 'sender@example.com');
  const tab = store().findEmailTab(emailId);
  if (!tab) throw new Error('email tab was not created');
  return tab;
}

describe('tabStore', () => {
  beforeEach(() => {
    resetTabStore();
    vi.useFakeTimers();
    vi.setSystemTime(FIXED_NOW_MS);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe('initial state', () => {
    it('starts with only the non-closeable All tab active', () => {
      expect(tabIds()).toEqual([ALL_TAB_ID]);
      expect(store().activeTabId).toBe(ALL_TAB_ID);
      expect(store().tabHistory).toEqual([ALL_TAB_ID]);
      expect(store().tabs[0].closeable).toBe(false);
    });
  });

  describe('addTab / removeTab', () => {
    it('appends a raw tab without activating it', () => {
      const tab: SettingsTab = {
        id: SETTINGS_TAB_ID,
        type: 'settings',
        title: 'Settings',
        section: 'app',
        closeable: true,
        lastAccessed: FIXED_NOW_MS,
      };

      store().addTab(tab);

      expect(tabIds()).toEqual([ALL_TAB_ID, SETTINGS_TAB_ID]);
      expect(store().activeTabId).toBe(ALL_TAB_ID);
    });

    it('removes a closeable tab and prunes it from history', () => {
      const folder = openFolder();

      store().removeTab(folder.id);

      expect(tabIds()).toEqual([ALL_TAB_ID]);
      expect(store().tabHistory).not.toContain(folder.id);
    });

    it('refuses to remove non-closeable tabs', () => {
      store().addAccountTab(ACCOUNT_ID, ACCOUNT_EMAIL, 'gmail');

      store().removeTab(ALL_TAB_ID);
      store().removeTab(`account-${ACCOUNT_ID}`);

      expect(tabIds()).toEqual([ALL_TAB_ID, `account-${ACCOUNT_ID}`]);
    });

    it('ignores unknown tab ids', () => {
      store().removeTab('does-not-exist');
      expect(tabIds()).toEqual([ALL_TAB_ID]);
    });

    it('focuses the previous tab in history when removing the active tab', () => {
      const email = openEmail();
      const folder = openFolder();
      expect(store().activeTabId).toBe(folder.id);

      store().removeTab(folder.id);

      expect(store().getTabById(folder.id)).toBeUndefined();
      expect(store().activeTabId).toBe(email.id);
      expect(store().tabHistory).toEqual([ALL_TAB_ID, email.id]);
    });

    it('falls back to the All tab when the removed active tab has no history', () => {
      const folder = openFolder();
      useTabStore.setState({ tabHistory: [] });

      store().removeTab(folder.id);

      expect(store().activeTabId).toBe(ALL_TAB_ID);
    });

    it('keeps the active tab when removing a background tab', () => {
      const folder = openFolder();
      const email = openEmail();
      expect(store().activeTabId).toBe(email.id);

      store().removeTab(folder.id);

      expect(store().activeTabId).toBe(email.id);
    });
  });

  describe('setActiveTab', () => {
    it('activates a known tab, moves it to the end of history, and bumps lastAccessed', () => {
      const folder = openFolder();
      vi.setSystemTime(FIXED_NOW_MS + 1000);

      store().setActiveTab(ALL_TAB_ID);
      store().setActiveTab(folder.id);

      expect(store().activeTabId).toBe(folder.id);
      expect(store().tabHistory).toEqual([ALL_TAB_ID, folder.id]);
      expect(store().getTabById(folder.id)?.lastAccessed).toBe(FIXED_NOW_MS + 1000);
    });

    it('ignores unknown tab ids', () => {
      store().setActiveTab('nope');
      expect(store().activeTabId).toBe(ALL_TAB_ID);
      expect(store().tabHistory).toEqual([ALL_TAB_ID]);
    });

    it('never duplicates a tab id in history', () => {
      const folder = openFolder();
      store().setActiveTab(folder.id);
      store().setActiveTab(folder.id);

      expect(store().tabHistory.filter((id) => id === folder.id)).toHaveLength(1);
    });
  });

  describe('closeAndFocusPrevious', () => {
    it('focuses the most recently visited remaining tab', () => {
      const folder = openFolder();
      const email = openEmail();
      expect(store().activeTabId).toBe(email.id);

      store().closeAndFocusPrevious(email.id);

      expect(store().activeTabId).toBe(folder.id);
      expect(tabIds()).toEqual([ALL_TAB_ID, folder.id]);
      expect(store().tabHistory).toEqual([ALL_TAB_ID, folder.id]);
    });

    it('respects navigation order, not open order, when choosing the neighbour', () => {
      const folder = openFolder();
      const email = openEmail();
      store().setActiveTab(ALL_TAB_ID);
      store().setActiveTab(email.id);

      store().closeAndFocusPrevious(email.id);

      expect(store().activeTabId).toBe(ALL_TAB_ID);
      expect(store().getTabById(folder.id)).toBeDefined();
    });

    it('falls back to the All tab when history is exhausted', () => {
      const folder = openFolder();
      useTabStore.setState({ tabHistory: [folder.id] });

      store().closeAndFocusPrevious(folder.id);

      expect(store().activeTabId).toBe(ALL_TAB_ID);
    });

    it('refuses to close non-closeable tabs', () => {
      store().closeAndFocusPrevious(ALL_TAB_ID);
      expect(tabIds()).toEqual([ALL_TAB_ID]);
    });

    it('closes a background tab without changing focus away from the current history head', () => {
      const folder = openFolder();
      const email = openEmail();

      store().closeAndFocusPrevious(folder.id);

      expect(store().activeTabId).toBe(email.id);
      expect(tabIds()).toEqual([ALL_TAB_ID, email.id]);
    });
  });

  describe('addAccountTab', () => {
    it('creates a non-closeable account tab without activating it', () => {
      store().addAccountTab(ACCOUNT_ID, ACCOUNT_EMAIL, 'gmail');

      const tab = store().findAccountTab(ACCOUNT_ID);
      expect(tab).toMatchObject<Partial<AccountTab>>({
        id: `account-${ACCOUNT_ID}`,
        type: 'account',
        title: ACCOUNT_EMAIL,
        accountEmail: ACCOUNT_EMAIL,
        accountType: 'gmail',
        closeable: false,
      });
      expect(store().activeTabId).toBe(ALL_TAB_ID);
    });

    it('activates the existing tab instead of creating a duplicate', () => {
      store().addAccountTab(ACCOUNT_ID, ACCOUNT_EMAIL, 'gmail');
      store().addAccountTab(ACCOUNT_ID, ACCOUNT_EMAIL, 'gmail');

      expect(store().getAccountTabs()).toHaveLength(1);
      expect(store().activeTabId).toBe(`account-${ACCOUNT_ID}`);
    });
  });

  describe('addFolderTab', () => {
    it('creates a closeable, active folder tab with a "user - Folder" title', () => {
      const tab = openFolder();

      expect(tab).toMatchObject<Partial<FolderTab>>({
        id: `folder-${ACCOUNT_ID}-${FOLDER_ID}`,
        type: 'folder',
        title: `alice - ${FOLDER_NAME}`,
        closeable: true,
      });
      expect(store().activeTabId).toBe(tab.id);
      expect(store().tabHistory).toEqual([ALL_TAB_ID, tab.id]);
    });

    it('re-activates an existing folder tab instead of duplicating it', () => {
      const tab = openFolder();
      store().setActiveTab(ALL_TAB_ID);

      store().addFolderTab(ACCOUNT_ID, FOLDER_ID, FOLDER_NAME, ACCOUNT_EMAIL);

      expect(store().tabs.filter((t) => t.type === 'folder')).toHaveLength(1);
      expect(store().activeTabId).toBe(tab.id);
    });

    it('distinguishes the same folder across accounts', () => {
      store().addFolderTab(ACCOUNT_ID, FOLDER_ID, FOLDER_NAME, ACCOUNT_EMAIL);
      store().addFolderTab(OTHER_ACCOUNT_ID, FOLDER_ID, FOLDER_NAME, OTHER_ACCOUNT_EMAIL);

      expect(store().tabs.filter((t) => t.type === 'folder')).toHaveLength(2);
    });
  });

  describe('addEmailTab', () => {
    it('creates a closeable, active email tab', () => {
      const tab = openEmail(EMAIL_ID, 'Hello');

      expect(tab).toMatchObject<Partial<EmailTab>>({
        id: `email-${EMAIL_ID}`,
        type: 'email',
        title: 'Hello',
        subject: 'Hello',
        closeable: true,
        date: Math.floor(FIXED_NOW_MS / 1000),
      });
      expect(store().activeTabId).toBe(tab.id);
    });

    it('truncates long subjects to 22 characters plus an ellipsis, keeping the full subject', () => {
      const longSubject = 'A'.repeat(26);
      const tab = openEmail('1', longSubject);

      expect(tab.title).toBe(`${'A'.repeat(22)}...`);
      expect(tab.title).toHaveLength(25);
      expect(tab.subject).toBe(longSubject);
    });

    it('keeps a subject of exactly 25 characters untruncated', () => {
      const tab = openEmail('1', 'B'.repeat(25));
      expect(tab.title).toBe('B'.repeat(25));
    });

    it('falls back to "No Subject" for an empty subject', () => {
      expect(openEmail('1', '').title).toBe('No Subject');
    });

    it('uses the provided date when given', () => {
      store().addEmailTab(ACCOUNT_ID, '9', 'x', ACCOUNT_EMAIL, 'f@x.com', 'From', 123);
      expect(store().findEmailTab('9')?.date).toBe(123);
    });

    it('re-activates an existing email tab instead of duplicating it', () => {
      const tab = openEmail();
      store().setActiveTab(ALL_TAB_ID);

      openEmail();

      expect(store().tabs.filter((t) => t.type === 'email')).toHaveLength(1);
      expect(store().activeTabId).toBe(tab.id);
    });
  });

  describe('addFeedTab', () => {
    it('titles feed tabs with the feed name alone and keeps the account scope on the tab', () => {
      store().addFeedTab(FEED_ID, 'Newsletters', ACCOUNT_EMAIL);

      expect(store().findFeedTab(FEED_ID)).toMatchObject<Partial<FeedTab>>({
        id: `feed-${FEED_ID}`,
        title: 'Newsletters',
        feedName: 'Newsletters',
        accountScope: ACCOUNT_EMAIL,
        closeable: true,
      });
      expect(store().activeTabId).toBe(`feed-${FEED_ID}`);
    });

    it('titles unscoped feeds with the feed name as well', () => {
      store().addFeedTab(FEED_ID, 'Newsletters', null);
      expect(store().findFeedTab(FEED_ID)?.title).toBe('Newsletters');
      expect(store().findFeedTab(FEED_ID)?.accountScope).toBeNull();
    });

    it('re-activates an existing feed tab instead of duplicating it', () => {
      store().addFeedTab(FEED_ID, 'Newsletters', null);
      store().setActiveTab(ALL_TAB_ID);

      store().addFeedTab(FEED_ID, 'Newsletters', null);

      expect(store().tabs.filter((t) => t.type === 'feed')).toHaveLength(1);
      expect(store().activeTabId).toBe(`feed-${FEED_ID}`);
    });
  });

  describe('addComposeTab', () => {
    it('creates a "New Email" tab when not replying', () => {
      store().addComposeTab(ACCOUNT_EMAIL);

      const tab = store().tabs.find((t) => t.type === 'compose') as ComposeTab;
      expect(tab.id).toMatch(/^compose-\d+$/);
      expect(tab).toMatchObject<Partial<ComposeTab>>({
        title: 'New Email',
        accountEmail: ACCOUNT_EMAIL,
        replySubject: undefined,
        closeable: true,
      });
      expect(store().activeTabId).toBe(tab.id);
    });

    it('creates a "Reply" tab and normalises the subject to a single "Re: " prefix', () => {
      store().addComposeTab(ACCOUNT_EMAIL, 'sender@example.com', 're:  Original', '<msg-id>');

      const tab = store().tabs.find((t) => t.type === 'compose') as ComposeTab;
      expect(tab.title).toBe('Reply');
      expect(tab.replySubject).toBe('Re: Original');
      expect(tab.inReplyTo).toBe('<msg-id>');
    });

    it('prefixes "Re: " to subjects that do not already have it', () => {
      store().addComposeTab(ACCOUNT_EMAIL, 'sender@example.com', 'Original');
      const tab = store().tabs.find((t) => t.type === 'compose') as ComposeTab;
      expect(tab.replySubject).toBe('Re: Original');
    });

    it('gives compose tabs unique ids even when opened in the same millisecond', () => {
      store().addComposeTab(ACCOUNT_EMAIL);
      store().addComposeTab(ACCOUNT_EMAIL);

      const composeIds = store().tabs.filter((t) => t.type === 'compose').map((t) => t.id);
      expect(composeIds).toHaveLength(2);
      expect(new Set(composeIds).size).toBe(2);
    });
  });

  describe('openSettingsTab', () => {
    it('opens the settings tab on the default "app" section', () => {
      store().openSettingsTab();

      const tab = store().getTabById(SETTINGS_TAB_ID) as SettingsTab;
      expect(tab.section).toBe('app');
      expect(store().activeTabId).toBe(SETTINGS_TAB_ID);
    });

    it('re-uses the existing tab and switches its section', () => {
      store().openSettingsTab('app');
      store().setActiveTab(ALL_TAB_ID);

      store().openSettingsTab('feeds');

      const settingsTabs = store().tabs.filter((t) => t.type === 'settings') as SettingsTab[];
      expect(settingsTabs).toHaveLength(1);
      expect(settingsTabs[0].section).toBe('feeds');
      expect(store().activeTabId).toBe(SETTINGS_TAB_ID);
      expect(store().tabHistory).toEqual([ALL_TAB_ID, SETTINGS_TAB_ID]);
    });
  });

  describe('getters', () => {
    it('getCloseableTabs excludes the All and account tabs', () => {
      store().addAccountTab(ACCOUNT_ID, ACCOUNT_EMAIL, 'gmail');
      const folder = openFolder();

      expect(store().getCloseableTabs().map((t) => t.id)).toEqual([folder.id]);
    });

    it('getTabById returns undefined for unknown ids', () => {
      expect(store().getTabById('missing')).toBeUndefined();
    });

    it('updateTabLastAccessed stamps the current time on the tab only', () => {
      const folder = openFolder();
      const allTabLastAccessedBefore = store().getTabById(ALL_TAB_ID)?.lastAccessed;
      vi.setSystemTime(FIXED_NOW_MS + 5000);

      store().updateTabLastAccessed(folder.id);

      expect(store().getTabById(folder.id)?.lastAccessed).toBe(FIXED_NOW_MS + 5000);
      expect(store().getTabById(ALL_TAB_ID)?.lastAccessed).toBe(allTabLastAccessedBefore);
    });
  });

  describe('reorderAccountTabs', () => {
    it('reorders account tabs, keeping All first and other tabs after', () => {
      store().addAccountTab(ACCOUNT_ID, ACCOUNT_EMAIL, 'gmail');
      store().addAccountTab(OTHER_ACCOUNT_ID, OTHER_ACCOUNT_EMAIL, 'imap');
      const folder = openFolder();

      store().reorderAccountTabs([OTHER_ACCOUNT_ID, ACCOUNT_ID]);

      expect(tabIds()).toEqual([
        ALL_TAB_ID,
        `account-${OTHER_ACCOUNT_ID}`,
        `account-${ACCOUNT_ID}`,
        folder.id,
      ]);
    });

    it('appends account tabs missing from the order list and ignores unknown ids', () => {
      store().addAccountTab(ACCOUNT_ID, ACCOUNT_EMAIL, 'gmail');
      store().addAccountTab(OTHER_ACCOUNT_ID, OTHER_ACCOUNT_EMAIL, 'imap');

      store().reorderAccountTabs(['ghost', OTHER_ACCOUNT_ID]);

      expect(tabIds()).toEqual([ALL_TAB_ID, `account-${OTHER_ACCOUNT_ID}`, `account-${ACCOUNT_ID}`]);
    });
  });

  describe('view mode and page persistence', () => {
    it('setTabViewMode and setTabCurrentPage update only the targeted tab', () => {
      store().addAccountTab(ACCOUNT_ID, ACCOUNT_EMAIL, 'gmail');

      store().setTabViewMode(ALL_TAB_ID, 'feeds');
      store().setTabCurrentPage(ALL_TAB_ID, 3);

      expect(store().getTabById(ALL_TAB_ID)).toMatchObject({ viewMode: 'feeds', currentPage: 3 });
      expect(store().getTabById(`account-${ACCOUNT_ID}`)).not.toHaveProperty('viewMode');
    });
  });

  describe('initialize', () => {
    const accounts = [
      { id: ACCOUNT_ID, email: ACCOUNT_EMAIL, accountType: 'gmail' as const },
      { id: OTHER_ACCOUNT_ID, email: OTHER_ACCOUNT_EMAIL, accountType: 'outlook' as const },
    ];

    it('replaces all tabs with All plus one tab per account and resets history', () => {
      openFolder();

      store().initialize(accounts);

      expect(tabIds()).toEqual([ALL_TAB_ID, `account-${ACCOUNT_ID}`, `account-${OTHER_ACCOUNT_ID}`]);
      expect(store().activeTabId).toBe(ALL_TAB_ID);
      expect(store().tabHistory).toEqual([ALL_TAB_ID]);
    });

    it('applies a saved account order by email and appends unlisted accounts', () => {
      store().initialize(accounts, [OTHER_ACCOUNT_EMAIL]);

      expect(tabIds()).toEqual([ALL_TAB_ID, `account-${OTHER_ACCOUNT_ID}`, `account-${ACCOUNT_ID}`]);
    });

    it('ignores an empty saved order', () => {
      store().initialize(accounts, []);
      expect(tabIds()).toEqual([ALL_TAB_ID, `account-${ACCOUNT_ID}`, `account-${OTHER_ACCOUNT_ID}`]);
    });
  });
});
