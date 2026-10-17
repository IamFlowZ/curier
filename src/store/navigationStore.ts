import { create } from 'zustand';
import type { EmailSummary } from '../types/email';

interface NavigationState {
  /** Index of the selected email in the active list (-1 = no selection) */
  selectedEmailIndex: number;
  /** Identifies which email list owns the current selection (tab ID or feed column ID) */
  selectedEmailListId: string;
  /** Index of the focused feed column (-1 = none focused) */
  focusedFeedColumnIndex: number;
  /** Total number of feed columns in the current view */
  feedColumnCount: number;
  /** Registry of email lists — components register their arrays here */
  emailLists: Map<string, EmailSummary[]>;

  // Email selection actions
  setSelectedEmailIndex: (index: number, listId: string) => void;
  moveSelection: (delta: number) => void;
  clearSelection: () => void;

  // Feed column focus actions
  setFocusedFeedColumnIndex: (index: number) => void;
  moveFeedColumnFocus: (delta: number) => void;
  clearFeedColumnFocus: () => void;
  setFeedColumnCount: (count: number) => void;

  // Email list registry
  registerEmailList: (listId: string, emails: EmailSummary[]) => void;
  unregisterEmailList: (listId: string) => void;

  // Computed
  getSelectedEmail: () => EmailSummary | null;
}

export const useNavigationStore = create<NavigationState>((set, get) => ({
  selectedEmailIndex: -1,
  selectedEmailListId: '',
  focusedFeedColumnIndex: -1,
  feedColumnCount: 0,
  emailLists: new Map(),

  setSelectedEmailIndex: (index: number, listId: string) => {
    set({ selectedEmailIndex: index, selectedEmailListId: listId });
  },

  moveSelection: (delta: number) => {
    const state = get();
    const emails = state.emailLists.get(state.selectedEmailListId);
    if (!emails || emails.length === 0) return;

    const currentIndex = state.selectedEmailIndex;
    let newIndex: number;

    if (currentIndex === -1) {
      // No selection yet — start at beginning or end depending on direction
      newIndex = delta > 0 ? 0 : emails.length - 1;
    } else {
      newIndex = Math.max(0, Math.min(emails.length - 1, currentIndex + delta));
    }

    set({ selectedEmailIndex: newIndex });
  },

  clearSelection: () => {
    set({ selectedEmailIndex: -1, selectedEmailListId: '' });
  },

  setFocusedFeedColumnIndex: (index: number) => {
    set({ focusedFeedColumnIndex: index });
  },

  moveFeedColumnFocus: (delta: number) => {
    const state = get();
    const maxColumns = state.feedColumnCount;
    if (maxColumns === 0) return;

    let newIndex: number;
    if (state.focusedFeedColumnIndex === -1) {
      newIndex = delta > 0 ? 0 : maxColumns - 1;
    } else {
      newIndex = Math.max(0, Math.min(maxColumns - 1, state.focusedFeedColumnIndex + delta));
    }

    set({ focusedFeedColumnIndex: newIndex });
  },

  clearFeedColumnFocus: () => {
    set({ focusedFeedColumnIndex: -1 });
  },

  setFeedColumnCount: (count: number) => {
    set({ feedColumnCount: count });
  },

  registerEmailList: (listId: string, emails: EmailSummary[]) => {
    set((state) => {
      const newMap = new Map(state.emailLists);
      newMap.set(listId, emails);
      return { emailLists: newMap };
    });
  },

  unregisterEmailList: (listId: string) => {
    set((state) => {
      const newMap = new Map(state.emailLists);
      newMap.delete(listId);
      return { emailLists: newMap };
    });
  },

  getSelectedEmail: () => {
    const state = get();
    if (state.selectedEmailIndex === -1) return null;
    const emails = state.emailLists.get(state.selectedEmailListId);
    if (!emails) return null;
    return emails[state.selectedEmailIndex] ?? null;
  },
}));
