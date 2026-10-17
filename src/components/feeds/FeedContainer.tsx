import { useState, useEffect, useCallback, useMemo } from 'react';
import type { Feed } from '../../types/feeds';
import { useFeedsStore } from '../../store/feedsStore';
import { useNavigationStore } from '../../store/navigationStore';
import { FeedColumn } from './FeedColumn';
import { AddFeedButton } from './AddFeedButton';
import { FeedEditor } from './FeedEditor';

interface FeedContainerProps {
  accountScope: string | null;
  searchQuery?: string;
}

export function FeedContainer({ accountScope, searchQuery: rawSearchQuery = '' }: FeedContainerProps) {
  const { feeds, loadFeeds, reorderFeeds } = useFeedsStore();
  const [editingFeed, setEditingFeed] = useState<Feed | null>(null);
  const [isCreating, setIsCreating] = useState(false);

  const searchQuery = useMemo(
    () => rawSearchQuery.trim().toLowerCase(),
    [rawSearchQuery]
  );

  useEffect(() => {
    loadFeeds(accountScope);
  }, [accountScope, loadFeeds]);

  const setFeedColumnCount = useNavigationStore((s) => s.setFeedColumnCount);

  const scopedFeeds = feeds
    .filter((feed) => feed.accountScope === accountScope)
    .sort((a, b) => a.columnOrder - b.columnOrder);

  // Register column count with navigation store
  useEffect(() => {
    setFeedColumnCount(scopedFeeds.length);
    return () => setFeedColumnCount(0);
  }, [scopedFeeds.length, setFeedColumnCount]);

  const handleAddFeed = () => {
    setIsCreating(true);
    setEditingFeed(null);
  };

  const handleEditFeed = (feed: Feed) => {
    setEditingFeed(feed);
    setIsCreating(false);
  };

  const handleCloseEditor = () => {
    setEditingFeed(null);
    setIsCreating(false);
  };

  const handleMoveFeed = useCallback(
    (feedId: number, direction: -1 | 1) => {
      const currentIndex = scopedFeeds.findIndex((f) => f.id === feedId);
      if (currentIndex < 0) return;

      const targetIndex = currentIndex + direction;
      if (targetIndex < 0 || targetIndex >= scopedFeeds.length) return;

      const newOrder = scopedFeeds.map((f) => f.id);
      // Swap
      [newOrder[currentIndex], newOrder[targetIndex]] = [newOrder[targetIndex], newOrder[currentIndex]];

      reorderFeeds(newOrder).catch((err) => {
        console.error('Failed to reorder feeds:', err);
      });
    },
    [scopedFeeds, reorderFeeds]
  );

  return (
    <div className="flex-1 flex flex-col h-full overflow-hidden">
      <div className="flex-1 flex gap-2 p-2 overflow-x-auto">
        {scopedFeeds.map((feed, index) => (
          <FeedColumn
            key={feed.id}
            feed={feed}
            columnIndex={index}
            searchQuery={searchQuery}
            onEditFeed={handleEditFeed}
            onMoveLeft={index > 0 ? () => handleMoveFeed(feed.id, -1) : undefined}
            onMoveRight={index < scopedFeeds.length - 1 ? () => handleMoveFeed(feed.id, 1) : undefined}
          />
        ))}
        <AddFeedButton onClick={handleAddFeed} />
      </div>

      {(editingFeed || isCreating) && (
        <FeedEditor
          feed={editingFeed}
          accountScope={accountScope}
          onClose={handleCloseEditor}
        />
      )}
    </div>
  );
}
