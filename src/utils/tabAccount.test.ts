import { describe, expect, it } from 'vitest';
import type { Tab } from '../types/tabs';
import { describeFeedTab, resolveTabAccountEmail, resolveTabTooltip } from './tabAccount';

const EMAIL = 'alice@example.com';
const NOW = 0;

const TABS: Record<Tab['type'], Tab> = {
  all: { id: 'all', type: 'all', title: 'All', closeable: false, lastAccessed: NOW },
  account: { id: 'account-a', type: 'account', title: EMAIL, accountId: 'a', accountEmail: EMAIL, accountType: 'gmail', closeable: false, lastAccessed: NOW },
  folder: { id: 'folder-a', type: 'folder', title: 'Inbox', accountId: 'a', accountEmail: EMAIL, folderId: 'INBOX', folderName: 'Inbox', closeable: true, lastAccessed: NOW },
  email: { id: 'email-1', type: 'email', title: 'Hi', accountId: 'a', accountEmail: EMAIL, emailId: '1', subject: 'Hi', fromAddress: 'x@y.z', date: NOW, closeable: true, lastAccessed: NOW },
  feed: { id: 'feed-1', type: 'feed', title: 'News', feedId: 1, feedName: 'News', accountScope: EMAIL, closeable: true, lastAccessed: NOW },
  compose: { id: 'compose-1', type: 'compose', title: 'New Email', accountEmail: EMAIL, closeable: true, lastAccessed: NOW },
  settings: { id: 'settings', type: 'settings', title: 'Settings', section: 'app', closeable: true, lastAccessed: NOW },
};

describe('resolveTabAccountEmail', () => {
  it('returns the account email for account, folder, email and compose tabs', () => {
    for (const type of ['account', 'folder', 'email', 'compose'] as const) {
      expect(resolveTabAccountEmail(TABS[type])).toBe(EMAIL);
    }
  });

  it('returns the account scope for feed tabs and null for unscoped feeds', () => {
    expect(resolveTabAccountEmail(TABS.feed)).toBe(EMAIL);
    expect(resolveTabAccountEmail({ ...TABS.feed, accountScope: null } as Tab)).toBeNull();
  });

  it('returns null for the All and Settings tabs', () => {
    expect(resolveTabAccountEmail(TABS.all)).toBeNull();
    expect(resolveTabAccountEmail(TABS.settings)).toBeNull();
  });

  it('treats an empty compose account as no account', () => {
    expect(resolveTabAccountEmail({ ...TABS.compose, accountEmail: '' } as Tab)).toBeNull();
  });
});

describe('describeFeedTab / resolveTabTooltip', () => {
  it('describes scoped feeds with the full address and unscoped feeds with "All"', () => {
    expect(describeFeedTab('News', EMAIL)).toBe(`News (${EMAIL})`);
    expect(describeFeedTab('News', null)).toBe('News (All)');
  });

  it('uses the feed description for feed tabs and the title for everything else', () => {
    expect(resolveTabTooltip(TABS.feed)).toBe(`News (${EMAIL})`);
    expect(resolveTabTooltip(TABS.email)).toBe('Hi');
  });
});
