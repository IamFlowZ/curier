import { describe, expect, it } from 'vitest';
import {
  isAccountTab,
  isAllTab,
  isComposeTab,
  isEmailTab,
  isFeedTab,
  isFolderTab,
  isSettingsTab,
  type Tab,
} from './tabs';

const NOW = 0;

const TABS: Tab[] = [
  { id: 'all', type: 'all', title: 'All', closeable: false, lastAccessed: NOW },
  {
    id: 'account-1',
    type: 'account',
    title: 'a@b.com',
    accountId: '1',
    accountEmail: 'a@b.com',
    accountType: 'gmail',
    closeable: false,
    lastAccessed: NOW,
  },
  {
    id: 'folder-1',
    type: 'folder',
    title: 'Inbox',
    accountId: '1',
    accountEmail: 'a@b.com',
    folderId: 'INBOX',
    folderName: 'Inbox',
    closeable: true,
    lastAccessed: NOW,
  },
  {
    id: 'email-1',
    type: 'email',
    title: 'Hi',
    accountId: '1',
    accountEmail: 'a@b.com',
    emailId: '1',
    subject: 'Hi',
    fromAddress: 'x@y.com',
    date: NOW,
    closeable: true,
    lastAccessed: NOW,
  },
  { id: 'feed-1', type: 'feed', title: 'Feed', feedId: 1, feedName: 'Feed', accountScope: null, closeable: true, lastAccessed: NOW },
  { id: 'compose-1', type: 'compose', title: 'New', accountEmail: 'a@b.com', closeable: true, lastAccessed: NOW },
  { id: 'settings', type: 'settings', title: 'Settings', section: 'app', closeable: true, lastAccessed: NOW },
];

const GUARDS = [
  ['all', isAllTab],
  ['account', isAccountTab],
  ['folder', isFolderTab],
  ['email', isEmailTab],
  ['feed', isFeedTab],
  ['compose', isComposeTab],
  ['settings', isSettingsTab],
] as const;

describe('tab type guards', () => {
  it.each(GUARDS)('%s guard matches exactly one tab of that type', (type, guard) => {
    const matches = TABS.filter((tab) => guard(tab));
    expect(matches).toHaveLength(1);
    expect(matches[0].type).toBe(type);
  });
});
