import { useState, useEffect } from 'react';
import type { Feed, FilterCriterion, CreateFeedInput, UpdateFeedInput } from '../../types/feeds';
import { useFeedsStore } from '../../store/feedsStore';
import { CriteriaBuilder } from './CriteriaBuilder';

interface FeedEditorProps {
  feed: Feed | null;
  accountScope: string | null;
  onClose: () => void;
}

export function FeedEditor({ feed, accountScope, onClose }: FeedEditorProps) {
  const { createFeed, updateFeed, loadTags } = useFeedsStore();
  const [name, setName] = useState(feed?.name ?? '');
  const [criteria, setCriteria] = useState<FilterCriterion[]>(feed?.criteria ?? []);
  const [matchMode, setMatchMode] = useState<'all' | 'any'>(feed?.matchMode ?? 'all');
  const [columnWidth, setColumnWidth] = useState(feed?.columnWidth ?? 4);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isEditing = feed !== null;

  useEffect(() => {
    loadTags();
  }, [loadTags]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    if (!name.trim()) {
      setError('Feed name is required');
      return;
    }

    setIsSubmitting(true);

    try {
      if (isEditing) {
        const input: UpdateFeedInput = {
          id: feed.id,
          name: name.trim(),
          criteria,
          matchMode,
          columnWidth,
        };
        await updateFeed(input);
      } else {
        const input: CreateFeedInput = {
          name: name.trim(),
          accountScope,
          criteria,
          matchMode,
          columnWidth,
        };
        await createFeed(input);
      }
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-white dark:bg-gray-800 rounded-lg w-full max-w-2xl max-h-[90vh] overflow-hidden flex flex-col">
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-medium text-gray-900 dark:text-white">
            {isEditing ? 'Edit Feed' : 'Create Feed'}
          </h2>
          <button
            onClick={onClose}
            className="p-1 text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white transition-colors"
          >
            <svg
              className="w-5 h-5"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M6 18L18 6M6 6l12 12"
              />
            </svg>
          </button>
        </div>

        <form onSubmit={handleSubmit} className="flex-1 overflow-y-auto p-4 space-y-4">
          {error && (
            <div className="p-3 bg-red-100 dark:bg-red-900/50 border border-red-300 dark:border-red-700 rounded text-sm text-red-700 dark:text-red-200">
              {error}
            </div>
          )}

          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              Feed Name
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g., GitHub Notifications"
              className="w-full px-3 py-2 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded focus:border-blue-500 focus:outline-none text-gray-900 dark:text-white"
              autoFocus
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              Column Width
            </label>
            <div className="flex items-center gap-4">
              <input
                type="range"
                min={1}
                max={12}
                step={1}
                value={columnWidth}
                onChange={(e) => setColumnWidth(Number(e.target.value))}
                className="flex-1"
              />
              <span className="text-sm text-gray-500 dark:text-gray-400 w-16">{columnWidth}/12</span>
            </div>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Filter Criteria
            </label>
            <CriteriaBuilder
              criteria={criteria}
              onChange={setCriteria}
              matchMode={matchMode}
              onMatchModeChange={setMatchMode}
            />
          </div>

          {criteria.length === 0 && (
            <p className="text-sm text-gray-500 dark:text-gray-400">
              No filters added. This feed will show all emails
              {accountScope ? ` for ${accountScope}` : ''}.
            </p>
          )}
        </form>

        <div className="flex justify-end gap-3 px-4 py-3 border-t border-gray-200 dark:border-gray-700">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 text-sm text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-white transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleSubmit}
            disabled={isSubmitting || !name.trim()}
            className="px-4 py-2 text-sm bg-blue-600 text-white rounded hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            {isSubmitting ? 'Saving...' : isEditing ? 'Save Changes' : 'Create Feed'}
          </button>
        </div>
      </div>
    </div>
  );
}
