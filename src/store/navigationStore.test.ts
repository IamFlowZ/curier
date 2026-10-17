import { beforeEach, describe, expect, it } from 'vitest';
import { useNavigationStore } from './navigationStore';
import { createStoreReset } from '../test/resetStore';
import type { EmailSummary } from '../types/email';

const LIST_ID = 'tab-all';
const OTHER_LIST_ID = 'feed-column-1';
const NO_SELECTION = -1;

const resetNavigationStore = createStoreReset(useNavigationStore);

function store() {
  return useNavigationStore.getState();
}

function makeEmails(count: number): EmailSummary[] {
  return Array.from({ length: count }, (_, i) => ({
    uid: i + 1,
    subject: `Email ${i + 1}`,
    from_address: `sender${i + 1}@example.com`,
    date: 0,
    has_attachments: false,
    is_read: false,
  }));
}

describe('navigationStore', () => {
  beforeEach(() => {
    resetNavigationStore();
  });

  describe('email list registry', () => {
    it('registers and unregisters lists without mutating the previous Map', () => {
      const before = store().emailLists;
      store().registerEmailList(LIST_ID, makeEmails(2));

      expect(store().emailLists).not.toBe(before);
      expect(store().emailLists.get(LIST_ID)).toHaveLength(2);

      store().unregisterEmailList(LIST_ID);
      expect(store().emailLists.has(LIST_ID)).toBe(false);
    });
  });

  describe('selection', () => {
    it('setSelectedEmailIndex records both the index and owning list', () => {
      store().setSelectedEmailIndex(2, LIST_ID);
      expect(store()).toMatchObject({ selectedEmailIndex: 2, selectedEmailListId: LIST_ID });
    });

    it('clearSelection resets index and list id', () => {
      store().setSelectedEmailIndex(2, LIST_ID);
      store().clearSelection();
      expect(store()).toMatchObject({ selectedEmailIndex: NO_SELECTION, selectedEmailListId: '' });
    });

    it('moveSelection is a no-op when the selected list is unregistered or empty', () => {
      store().setSelectedEmailIndex(NO_SELECTION, LIST_ID);
      store().moveSelection(1);
      expect(store().selectedEmailIndex).toBe(NO_SELECTION);

      store().registerEmailList(LIST_ID, []);
      store().moveSelection(1);
      expect(store().selectedEmailIndex).toBe(NO_SELECTION);
    });

    it('moveSelection from no selection starts at the first item moving down and the last moving up', () => {
      store().registerEmailList(LIST_ID, makeEmails(3));
      store().setSelectedEmailIndex(NO_SELECTION, LIST_ID);

      store().moveSelection(1);
      expect(store().selectedEmailIndex).toBe(0);

      store().setSelectedEmailIndex(NO_SELECTION, LIST_ID);
      store().moveSelection(-1);
      expect(store().selectedEmailIndex).toBe(2);
    });

    it('moveSelection clamps within list bounds', () => {
      store().registerEmailList(LIST_ID, makeEmails(3));
      store().setSelectedEmailIndex(1, LIST_ID);

      store().moveSelection(10);
      expect(store().selectedEmailIndex).toBe(2);

      store().moveSelection(-10);
      expect(store().selectedEmailIndex).toBe(0);
    });

    it('getSelectedEmail returns the email at the selected index or null', () => {
      const emails = makeEmails(2);
      store().registerEmailList(LIST_ID, emails);

      expect(store().getSelectedEmail()).toBeNull();

      store().setSelectedEmailIndex(1, LIST_ID);
      expect(store().getSelectedEmail()).toBe(emails[1]);

      store().setSelectedEmailIndex(5, LIST_ID);
      expect(store().getSelectedEmail()).toBeNull();

      store().setSelectedEmailIndex(0, OTHER_LIST_ID);
      expect(store().getSelectedEmail()).toBeNull();
    });
  });

  describe('feed column focus', () => {
    it('moveFeedColumnFocus is a no-op when there are no columns', () => {
      store().moveFeedColumnFocus(1);
      expect(store().focusedFeedColumnIndex).toBe(NO_SELECTION);
    });

    it('moveFeedColumnFocus from no focus starts at the first column moving right and the last moving left', () => {
      store().setFeedColumnCount(3);

      store().moveFeedColumnFocus(1);
      expect(store().focusedFeedColumnIndex).toBe(0);

      store().clearFeedColumnFocus();
      store().moveFeedColumnFocus(-1);
      expect(store().focusedFeedColumnIndex).toBe(2);
    });

    it('moveFeedColumnFocus clamps to the column range', () => {
      store().setFeedColumnCount(3);
      store().setFocusedFeedColumnIndex(1);

      store().moveFeedColumnFocus(5);
      expect(store().focusedFeedColumnIndex).toBe(2);

      store().moveFeedColumnFocus(-5);
      expect(store().focusedFeedColumnIndex).toBe(0);
    });

    it('clearFeedColumnFocus resets the focused index', () => {
      store().setFocusedFeedColumnIndex(2);
      store().clearFeedColumnFocus();
      expect(store().focusedFeedColumnIndex).toBe(NO_SELECTION);
    });
  });
});
