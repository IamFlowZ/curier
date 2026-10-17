import { useRef, useEffect, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { EmailSummary } from '../../types/email';
import { FeedEmailRow } from './FeedEmailRow';
import { getEmailListRowColor, extractAccountId } from '../../utils/accountColors';
import { useNavigationStore } from '../../store/navigationStore';

interface FeedEmailListProps {
  emails: EmailSummary[];
  isLoading: boolean;
  hasMore: boolean;
  onLoadMore: () => void;
  onEmailClick: (email: EmailSummary) => void;
  listId?: string;
  selectedIndex?: number;
}

export function FeedEmailList({
  emails,
  isLoading,
  hasMore,
  onLoadMore,
  onEmailClick,
  listId,
  selectedIndex = -1,
}: FeedEmailListProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const registerEmailList = useNavigationStore((s) => s.registerEmailList);
  const unregisterEmailList = useNavigationStore((s) => s.unregisterEmailList);

  // Register email list with navigation store
  useEffect(() => {
    if (listId) {
      registerEmailList(listId, emails);
      return () => unregisterEmailList(listId);
    }
  }, [listId, emails, registerEmailList, unregisterEmailList]);

  const virtualizer = useVirtualizer({
    count: emails.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 100,
    overscan: 5,
  });

  const items = virtualizer.getVirtualItems();

  // Scroll selected item into view
  useEffect(() => {
    if (selectedIndex >= 0 && selectedIndex < emails.length) {
      virtualizer.scrollToIndex(selectedIndex, { align: 'auto' });
    }
  }, [selectedIndex, emails.length, virtualizer]);

  // Infinite scroll trigger
  const handleScroll = useCallback(() => {
    if (!parentRef.current || isLoading || !hasMore) return;

    const { scrollTop, scrollHeight, clientHeight } = parentRef.current;
    const scrolledToBottom = scrollTop + clientHeight >= scrollHeight - 200;

    if (scrolledToBottom) {
      onLoadMore();
    }
  }, [isLoading, hasMore, onLoadMore]);

  useEffect(() => {
    const container = parentRef.current;
    if (!container) return;

    container.addEventListener('scroll', handleScroll);
    return () => container.removeEventListener('scroll', handleScroll);
  }, [handleScroll]);

  if (isLoading && emails.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center p-10 text-center text-gray-500 dark:text-gray-400">
        Loading emails...
      </div>
    );
  }

  if (emails.length === 0 && !isLoading) {
    return (
      <div className="flex-1 flex items-center justify-center p-10 text-center text-gray-500 dark:text-gray-400">
        No emails found
      </div>
    );
  }

  return (
    <div
      ref={parentRef}
      className="flex-1 overflow-y-auto relative"
    >
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          width: '100%',
          position: 'relative',
        }}
      >
        {items.map((virtualRow) => {
          const email = emails[virtualRow.index];
          const accountId = extractAccountId(email);
          const rowColor = getEmailListRowColor(accountId, virtualRow.index, email.is_read);

          return (
            <div
              key={virtualRow.key}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                transform: `translateY(${virtualRow.start}px)`,
              }}
            >
              <FeedEmailRow
                email={email}
                rowColor={rowColor}
                isSelected={virtualRow.index === selectedIndex}
                onClick={() => onEmailClick(email)}
              />
            </div>
          );
        })}
      </div>
      {isLoading && (
        <div className="flex justify-center py-4">
          <div className="w-5 h-5 border-2 border-gray-300 dark:border-gray-600 border-t-transparent rounded-full animate-spin" />
        </div>
      )}
    </div>
  );
}
