export type TabType = 'all' | 'account' | 'folder' | 'email' | 'feed' | 'compose' | 'settings';
export type AccountType = 'gmail' | 'outlook' | 'imap';

export interface BaseTab {
  id: string;
  type: TabType;
  title: string;
  closeable: boolean;
  lastAccessed: number; // timestamp for memory tracking
}

export interface AllTab extends BaseTab {
  type: 'all';
  closeable: false;
  viewMode?: 'list' | 'feeds';
  currentPage?: number;
}

export interface AccountTab extends BaseTab {
  type: 'account';
  accountId: string;
  accountEmail: string; // The actual email address for API calls
  accountType: AccountType;
  closeable: false;
  viewMode?: 'list' | 'feeds';
  currentPage?: number;
}

export interface FolderTab extends BaseTab {
  type: 'folder';
  accountId: string;
  accountEmail: string; // The actual email address for API calls
  folderId: string;
  folderName: string;
  closeable: true;
}

export interface EmailTab extends BaseTab {
  type: 'email';
  accountId: string;
  accountEmail: string; // The actual email address for API calls
  emailId: string;
  subject: string;
  fromAddress: string;
  fromName?: string;
  date: number;
  closeable: true;
}

export interface FeedTab extends BaseTab {
  type: 'feed';
  feedId: number;
  feedName: string;
  accountScope: string | null;
  closeable: true;
}

export interface ComposeTab extends BaseTab {
  type: 'compose';
  accountEmail: string;
  replyTo?: string;
  replySubject?: string;
  inReplyTo?: string;
  replyQuotedBody?: string;
  replyFromName?: string;
  replyDate?: number;
  closeable: true;
}

export type SettingsSection = 'app' | 'feeds' | 'resources' | 'accounts' | 'keybinds';

export interface SettingsTab extends BaseTab {
  type: 'settings';
  section: SettingsSection;
  closeable: true;
}

export type Tab = AllTab | AccountTab | FolderTab | EmailTab | FeedTab | ComposeTab | SettingsTab;

// Helper type guards
export function isAllTab(tab: Tab): tab is AllTab {
  return tab.type === 'all';
}

export function isAccountTab(tab: Tab): tab is AccountTab {
  return tab.type === 'account';
}

export function isFolderTab(tab: Tab): tab is FolderTab {
  return tab.type === 'folder';
}

export function isEmailTab(tab: Tab): tab is EmailTab {
  return tab.type === 'email';
}

export function isFeedTab(tab: Tab): tab is FeedTab {
  return tab.type === 'feed';
}

export function isComposeTab(tab: Tab): tab is ComposeTab {
  return tab.type === 'compose';
}

export function isSettingsTab(tab: Tab): tab is SettingsTab {
  return tab.type === 'settings';
}
