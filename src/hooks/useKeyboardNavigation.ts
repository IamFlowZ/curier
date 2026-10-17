import { useEffect } from 'react';
import { useTabStore } from '../store/tabStore';
import { useNavigationStore } from '../store/navigationStore';
import { isEditableTarget } from '../utils/isEditableTarget';
import type { EmailSummary } from '../types/email';
import type { AccountTab } from '../types/tabs';

type NavigationContext = 'email-list' | 'feeds' | 'email-viewer' | 'compose' | 'none';

interface UseKeyboardNavigationOptions {
  openSettings: () => void;
  onOpenEmail: (email: EmailSummary) => void;
}

export function useKeyboardNavigation({
  openSettings,
  onOpenEmail,
}: UseKeyboardNavigationOptions): void {
  const tabs = useTabStore((s) => s.tabs);
  const activeTabId = useTabStore((s) => s.activeTabId);
  const setActiveTab = useTabStore((s) => s.setActiveTab);
  const closeAndFocusPrevious = useTabStore((s) => s.closeAndFocusPrevious);
  const addComposeTab = useTabStore((s) => s.addComposeTab);

  // Reset selection when active tab changes
  useEffect(() => {
    useNavigationStore.getState().clearSelection();
    useNavigationStore.getState().clearFeedColumnFocus();
  }, [activeTabId]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const meta = e.metaKey || e.ctrlKey;
      const activeTab = tabs.find((t) => t.id === activeTabId);

      // Derive navigation context
      let context: NavigationContext = 'none';
      if (activeTab) {
        switch (activeTab.type) {
          case 'all':
          case 'account': {
            const viewMode = (activeTab as { viewMode?: string }).viewMode ?? 'feeds';
            context = viewMode === 'feeds' ? 'feeds' : 'email-list';
            break;
          }
          case 'folder':
            context = 'email-list';
            break;
          case 'feed':
            context = 'email-list';
            break;
          case 'email':
            context = 'email-viewer';
            break;
          case 'compose':
            context = 'compose';
            break;
        }
      }

      // === MODIFIER SHORTCUTS (always active, even in inputs) ===
      if (meta) {
        // Cmd+W — Close current tab
        if (e.key === 'w') {
          e.preventDefault();
          if (activeTab?.closeable) {
            closeAndFocusPrevious(activeTabId);
          }
          return;
        }

        // Cmd+R — Refresh
        if (e.key === 'r') {
          e.preventDefault();
          window.dispatchEvent(new CustomEvent('curier:refresh'));
          return;
        }

        // Cmd+N — New compose
        if (e.key === 'n') {
          e.preventDefault();
          const accountTabs = tabs.filter((t) => t.type === 'account');
          let defaultAccount = '';
          if (activeTab?.type === 'account') {
            defaultAccount = (activeTab as AccountTab).accountEmail;
          } else if (accountTabs.length > 0) {
            defaultAccount = (accountTabs[0] as AccountTab).accountEmail;
          }
          addComposeTab(defaultAccount);
          return;
        }

        // Cmd+, — Open settings
        if (e.key === ',') {
          e.preventDefault();
          openSettings();
          return;
        }

        // Cmd+1-9 — Switch to tab by position
        const digit = parseInt(e.key, 10);
        if (digit >= 1 && digit <= 9) {
          e.preventDefault();
          const targetIndex = digit - 1;
          if (targetIndex < tabs.length) {
            setActiveTab(tabs[targetIndex].id);
          }
          return;
        }

        // Cmd+Shift+[ / ] — Previous/next tab
        if (e.shiftKey) {
          if (e.code === 'BracketLeft') {
            e.preventDefault();
            const currentIndex = tabs.findIndex((t) => t.id === activeTabId);
            if (currentIndex > 0) {
              setActiveTab(tabs[currentIndex - 1].id);
            }
            return;
          }
          if (e.code === 'BracketRight') {
            e.preventDefault();
            const currentIndex = tabs.findIndex((t) => t.id === activeTabId);
            if (currentIndex < tabs.length - 1) {
              setActiveTab(tabs[currentIndex + 1].id);
            }
            return;
          }
        }

        return;
      }

      // === LETTER/KEY SHORTCUTS (skip when typing in inputs) ===
      if (isEditableTarget(e)) return;

      const navStore = useNavigationStore.getState();

      // Escape — cascading: clear selection > close tab
      if (e.key === 'Escape') {
        if (navStore.selectedEmailIndex >= 0) {
          navStore.clearSelection();
          return;
        }
        if (navStore.focusedFeedColumnIndex >= 0) {
          navStore.clearFeedColumnFocus();
          return;
        }
        if (activeTab?.closeable) {
          closeAndFocusPrevious(activeTabId);
        }
        return;
      }

      // / — Focus search
      if (e.key === '/') {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent('curier:focus-search'));
        return;
      }

      // r — Reply (only in email viewer)
      if (e.key === 'r' && context === 'email-viewer') {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent('curier:reply'));
        return;
      }

      // . — Toggle sidebar
      if (e.key === '.') {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent('curier:toggle-sidebar'));
        return;
      }

      // v — Toggle between list and feeds view
      if (e.key === 'v' && (context === 'email-list' || context === 'feeds')) {
        if (activeTab && (activeTab.type === 'all' || activeTab.type === 'account')) {
          e.preventDefault();
          const currentViewMode = (activeTab as { viewMode?: string }).viewMode ?? 'feeds';
          const newMode = currentViewMode === 'feeds' ? 'list' : 'feeds';
          useTabStore.getState().setTabViewMode(activeTab.id, newMode);
        }
        return;
      }

      // j / ArrowDown — Select next email
      if (e.key === 'j' || e.key === 'ArrowDown') {
        if (context === 'email-list' || context === 'feeds') {
          e.preventDefault();

          if (context === 'feeds') {
            // Auto-focus first column if none focused
            if (navStore.focusedFeedColumnIndex === -1) {
              navStore.moveFeedColumnFocus(1);
            }
          }

          // Ensure we have an active list
          if (!navStore.selectedEmailListId) {
            // Try to activate the appropriate list
            const listId = getActiveListId(activeTab, navStore.focusedFeedColumnIndex);
            if (listId && navStore.emailLists.has(listId)) {
              navStore.setSelectedEmailIndex(0, listId);
              return;
            }
          }

          navStore.moveSelection(1);
        }
        return;
      }

      // k / ArrowUp — Select previous email
      if (e.key === 'k' || e.key === 'ArrowUp') {
        if (context === 'email-list' || context === 'feeds') {
          e.preventDefault();

          if (context === 'feeds') {
            if (navStore.focusedFeedColumnIndex === -1) {
              navStore.moveFeedColumnFocus(1);
            }
          }

          if (!navStore.selectedEmailListId) {
            const listId = getActiveListId(activeTab, navStore.focusedFeedColumnIndex);
            if (listId && navStore.emailLists.has(listId)) {
              const emails = navStore.emailLists.get(listId)!;
              navStore.setSelectedEmailIndex(emails.length - 1, listId);
              return;
            }
          }

          navStore.moveSelection(-1);
        }
        return;
      }

      // Enter / o — Open selected email
      if (e.key === 'Enter' || e.key === 'o') {
        const selectedEmail = navStore.getSelectedEmail();
        if (selectedEmail) {
          e.preventDefault();
          onOpenEmail(selectedEmail);
        }
        return;
      }

      // [ / ] — Previous / next page in list view
      if (e.key === '[' && context === 'email-list') {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent('curier:page-prev'));
        return;
      }
      if (e.key === ']' && context === 'email-list') {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent('curier:page-next'));
        return;
      }

      // h / ArrowLeft — Previous feed column
      if (e.key === 'h' || e.key === 'ArrowLeft') {
        if (context === 'feeds') {
          e.preventDefault();
          navStore.moveFeedColumnFocus(-1);
          // Reset email selection when switching columns
          navStore.clearSelection();
        }
        return;
      }

      // l / ArrowRight — Next feed column
      if (e.key === 'l' || e.key === 'ArrowRight') {
        if (context === 'feeds') {
          e.preventDefault();
          navStore.moveFeedColumnFocus(1);
          navStore.clearSelection();
        }
        return;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [
    tabs,
    activeTabId,
    setActiveTab,
    closeAndFocusPrevious,
    addComposeTab,
    openSettings,
    onOpenEmail,
  ]);
}

/**
 * Derives the email list ID for the current context.
 * For regular tabs it's the tab ID; for feeds view it's based on the focused column.
 * Feed tabs use `feed-tab-${feedId}` as their list ID.
 */
function getActiveListId(
  activeTab: { id: string; type: string } | undefined,
  focusedFeedColumnIndex: number,
): string | null {
  if (!activeTab) return null;

  const viewMode = (activeTab as { viewMode?: string }).viewMode;

  // Feeds view — list ID is column-based
  if ((activeTab.type === 'all' || activeTab.type === 'account') && (viewMode ?? 'feeds') === 'feeds') {
    if (focusedFeedColumnIndex === -1) return null;
    // The actual list ID is set by FeedColumn components as `feed-column-${feed.id}`
    // We need to find it from the registered lists
    const navStore = useNavigationStore.getState();
    const feedColumnKeys = Array.from(navStore.emailLists.keys()).filter((k) =>
      k.startsWith('feed-column-'),
    );
    if (focusedFeedColumnIndex < feedColumnKeys.length) {
      return feedColumnKeys[focusedFeedColumnIndex];
    }
    return null;
  }

  // Feed tab — list ID is `feed-tab-${feedId}`
  if (activeTab.type === 'feed') {
    const feedId = (activeTab as { feedId?: number }).feedId;
    return feedId != null ? `feed-tab-${feedId}` : activeTab.id;
  }

  // Regular email list tabs (all, account, folder)
  return activeTab.id;
}
