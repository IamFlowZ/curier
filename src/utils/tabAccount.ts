import type { Tab } from '../types/tabs';

const UNSCOPED_FEED_ACCOUNT_LABEL = 'All';

/** Tooltip text for a feed tab: "FeedName (user@domain)" or "FeedName (All)". */
export function describeFeedTab(feedName: string, accountScope: string | null): string {
  return `${feedName} (${accountScope ?? UNSCOPED_FEED_ACCOUNT_LABEL})`;
}

/**
 * The email address whose account color a tab should display, or null for
 * tabs that are not tied to a single account (All, unscoped feeds, settings).
 */
export function resolveTabAccountEmail(tab: Tab): string | null {
  switch (tab.type) {
    case 'account':
    case 'folder':
    case 'email':
    case 'compose':
      return tab.accountEmail || null;
    case 'feed':
      return tab.accountScope;
    case 'all':
    case 'settings':
      return null;
  }
}

/**
 * Hover text for a tab label. Feed tabs show the account scope, which is
 * no longer part of the (truncatable) title; every other tab uses its title.
 */
export function resolveTabTooltip(tab: Tab): string {
  return tab.type === 'feed' ? describeFeedTab(tab.feedName, tab.accountScope) : tab.title;
}
