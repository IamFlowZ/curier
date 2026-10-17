import { useState, useCallback, useMemo } from 'react';
import { useInfiniteQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { ask } from '@tauri-apps/plugin-dialog';
import type { Feed, PaginatedFeedResponse } from '../../types/feeds';
import type { EmailSummary } from '../../types/email';
import { FeedHeader } from './FeedHeader';
import { FeedEmailList } from './FeedEmailList';
import { useFeedsStore } from '../../store/feedsStore';
import { useTabStore } from '../../store/tabStore';
import { useNavigationStore } from '../../store/navigationStore';

interface FeedColumnProps {
  feed: Feed;
  columnIndex: number;
  searchQuery?: string;
  onEditFeed: (feed: Feed) => void;
  onMoveLeft?: () => void;
  onMoveRight?: () => void;
}

interface RustPaginatedFeedResponse {
  emails: EmailSummary[];
  total_count: number;
  page: number;
  has_more: boolean;
}

export function FeedColumn({ feed, columnIndex, searchQuery = '', onEditFeed, onMoveLeft, onMoveRight }: FeedColumnProps) {
  const [isDeleting, setIsDeleting] = useState(false);
  const { deleteFeed } = useFeedsStore();
  const { addEmailTab, addFeedTab } = useTabStore();

  const {
    data,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading,
  } = useInfiniteQuery({
    queryKey: ['feed', feed.id],
    queryFn: async ({ pageParam = 1 }): Promise<PaginatedFeedResponse> => {
      const response = await invoke<RustPaginatedFeedResponse>('fetch_feed_emails', {
        feedId: feed.id,
        page: pageParam,
        limit: 20,
      });
      return {
        emails: response.emails,
        totalCount: response.total_count,
        page: response.page,
        hasMore: response.has_more,
      };
    },
    getNextPageParam: (lastPage) => (lastPage.hasMore ? lastPage.page + 1 : undefined),
    initialPageParam: 1,
  });

  const allEmails = data?.pages.flatMap((p) => p.emails) ?? [];
  const totalCount = data?.pages[0]?.totalCount ?? 0;

  const emails = useMemo(() => {
    if (!searchQuery) return allEmails;
    return allEmails.filter((email) => {
      const fromName = (email.from_name ?? '').toLowerCase();
      const fromAddress = (email.from_address ?? '').toLowerCase();
      const subject = (email.subject ?? '').toLowerCase();
      const snippet = (email.snippet ?? '').toLowerCase();
      return (
        fromName.includes(searchQuery) ||
        fromAddress.includes(searchQuery) ||
        subject.includes(searchQuery) ||
        snippet.includes(searchQuery)
      );
    });
  }, [allEmails, searchQuery]);

  const handleLoadMore = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage) {
      fetchNextPage();
    }
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  const handleEmailClick = useCallback(
    (email: EmailSummary) => {
      if (email.message_id) {
        addEmailTab(
          feed.accountScope || 'all',
          email.message_id,
          email.subject,
          email.account_email || '',
          email.from_address,
          email.from_name,
          email.date
        );
      }
    },
    [addEmailTab, feed.accountScope]
  );

  const handleDeleteClick = useCallback(async () => {
    console.log('Delete button clicked for feed:', feed.id, feed.name);
    if (isDeleting) {
      console.log('Already deleting, skipping...');
      return;
    }

    const confirmed = await ask(`Delete feed "${feed.name}"?`, {
      title: 'Confirm Deletion',
      kind: 'warning',
    });
    console.log('Delete confirmed:', confirmed);
    if (!confirmed) return;

    setIsDeleting(true);
    try {
      console.log('Calling deleteFeed for:', feed.id);
      await deleteFeed(feed.id);
      console.log('Feed deleted successfully:', feed.id);
    } catch (error) {
      console.error('Failed to delete feed:', error);
      alert(`Failed to delete feed: ${error}`);
    } finally {
      setIsDeleting(false);
    }
  }, [feed, deleteFeed, isDeleting]);

  const handleOpenInTab = useCallback(() => {
    addFeedTab(feed.id, feed.name, feed.accountScope);
  }, [addFeedTab, feed.id, feed.name, feed.accountScope]);

  const listId = `feed-column-${feed.id}`;
  const focusedFeedColumnIndex = useNavigationStore((s) => s.focusedFeedColumnIndex);
  const selectedEmailListId = useNavigationStore((s) => s.selectedEmailListId);
  const selectedEmailIndex = useNavigationStore((s) => s.selectedEmailIndex);
  const isFocused = focusedFeedColumnIndex === columnIndex;
  const selectedIndex = selectedEmailListId === listId ? selectedEmailIndex : -1;

  return (
    <div
      className={`flex flex-col h-auto max-h-[85vh] bg-white dark:bg-gray-800 border rounded-lg overflow-hidden${isFocused ? ' ring-2 ring-blue-400 border-blue-400 dark:border-blue-400' : ' border-gray-200 dark:border-gray-700'}`}
      style={{
        flex: `1 1 ${feed.columnWidth * 8.33}%`,
        minWidth: Math.max(280, feed.columnWidth * 80)
      }}
    >
      <FeedHeader
        feed={feed}
        emailCount={searchQuery ? emails.length : totalCount}
        onSettingsClick={() => onEditFeed(feed)}
        onDeleteClick={handleDeleteClick}
        onOpenInTab={handleOpenInTab}
        onMoveLeft={onMoveLeft}
        onMoveRight={onMoveRight}
      />
      <FeedEmailList
        emails={emails}
        isLoading={isLoading || isFetchingNextPage}
        hasMore={hasNextPage ?? false}
        onLoadMore={handleLoadMore}
        onEmailClick={handleEmailClick}
        listId={listId}
        selectedIndex={selectedIndex}
      />
    </div>
  );
}
