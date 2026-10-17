import { useState, useEffect } from 'react';
import type { TaggingRule, FilterCriterion, CreateTaggingRuleInput } from '../../types/feeds';
import { useFeedsStore } from '../../store/feedsStore';
import { CriteriaBuilder } from './CriteriaBuilder';
import { CustomSelect } from '../ui/CustomSelect';

interface RuleEditorProps {
  rule: TaggingRule | null;
  onClose: () => void;
}

export function RuleEditor({ rule, onClose }: RuleEditorProps) {
  const { tags, loadTags, createRule, updateRule } = useFeedsStore();
  const [name, setName] = useState(rule?.name ?? '');
  const [tagId, setTagId] = useState<number>(rule?.tagId ?? 0);
  const [criteria, setCriteria] = useState<FilterCriterion[]>(rule?.criteria ?? []);
  const [matchMode, setMatchMode] = useState<'all' | 'any'>(rule?.matchMode ?? 'all');
  const [priority, setPriority] = useState(rule?.priority ?? 0);
  const [enabled, setEnabled] = useState(rule?.enabled ?? true);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isEditing = rule !== null;

  useEffect(() => {
    loadTags();
  }, [loadTags]);

  useEffect(() => {
    if (tags.length > 0 && tagId === 0) {
      setTagId(tags[0].id);
    }
  }, [tags, tagId]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    if (!name.trim()) {
      setError('Rule name is required');
      return;
    }

    if (tagId === 0) {
      setError('Please select a tag');
      return;
    }

    if (criteria.length === 0) {
      setError('At least one filter criterion is required');
      return;
    }

    setIsSubmitting(true);

    try {
      if (isEditing) {
        const updatedRule: TaggingRule = {
          ...rule,
          name: name.trim(),
          tagId,
          criteria,
          matchMode,
          priority,
          enabled,
          updatedAt: Date.now(),
        };
        await updateRule(updatedRule);
      } else {
        const input: CreateTaggingRuleInput = {
          name: name.trim(),
          tagId,
          criteria,
          matchMode,
          priority,
          enabled,
        };
        await createRule(input);
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
            {isEditing ? 'Edit Rule' : 'Create Auto-Tagging Rule'}
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
              Rule Name
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g., Tag GitHub notifications"
              className="w-full px-3 py-2 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded focus:border-blue-500 focus:outline-none text-gray-900 dark:text-white"
              autoFocus
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              Apply Tag
            </label>
            <CustomSelect
              value={String(tagId)}
              onChange={(val) => setTagId(Number(val))}
              placeholder="No tags available - create one first"
              options={tags.map((tag) => ({
                value: String(tag.id),
                label: tag.name,
                color: tag.color ?? undefined,
              }))}
            />
          </div>

          <div className="flex gap-4">
            <div className="flex-1">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                Priority
              </label>
              <input
                type="number"
                value={priority}
                onChange={(e) => setPriority(Number(e.target.value))}
                className="w-full px-3 py-2 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded focus:border-blue-500 focus:outline-none text-gray-900 dark:text-white"
              />
              <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                Higher priority rules are evaluated first
              </p>
            </div>

            <div className="flex items-center gap-2 pt-6">
              <input
                type="checkbox"
                id="enabled"
                checked={enabled}
                onChange={(e) => setEnabled(e.target.checked)}
                className="w-4 h-4 rounded border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-blue-600 focus:ring-blue-500"
              />
              <label htmlFor="enabled" className="text-sm text-gray-700 dark:text-gray-300">
                Enabled
              </label>
            </div>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              When an email matches these criteria
            </label>
            <CriteriaBuilder
              criteria={criteria}
              onChange={setCriteria}
              matchMode={matchMode}
              onMatchModeChange={setMatchMode}
            />
          </div>
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
            disabled={isSubmitting || !name.trim() || tagId === 0}
            className="px-4 py-2 text-sm bg-blue-600 text-white rounded hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            {isSubmitting ? 'Saving...' : isEditing ? 'Save Changes' : 'Create Rule'}
          </button>
        </div>
      </div>
    </div>
  );
}
