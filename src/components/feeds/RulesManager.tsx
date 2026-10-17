import { useState, useEffect } from 'react';
import type { TaggingRule } from '../../types/feeds';
import { useFeedsStore } from '../../store/feedsStore';
import { RuleEditor } from './RuleEditor';

export function RulesManager() {
  const { rules, tags, loadRules, loadTags, updateRule, deleteRule, runRules, isLoading } =
    useFeedsStore();
  const [editingRule, setEditingRule] = useState<TaggingRule | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [runResult, setRunResult] = useState<{
    rulesExecuted: number;
    emailsProcessed: number;
    tagsApplied: number;
  } | null>(null);
  const [isRunning, setIsRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadRules();
    loadTags();
  }, [loadRules, loadTags]);

  const handleToggleEnabled = async (rule: TaggingRule) => {
    try {
      await updateRule({ ...rule, enabled: !rule.enabled });
    } catch (err) {
      setError(String(err));
    }
  };

  const handleDeleteRule = async (ruleId: number, ruleName: string) => {
    const confirmed = window.confirm(`Delete rule "${ruleName}"?`);
    if (!confirmed) return;

    try {
      await deleteRule(ruleId);
    } catch (err) {
      setError(String(err));
    }
  };

  const handleRunRules = async () => {
    setIsRunning(true);
    setError(null);
    setRunResult(null);

    try {
      const result = await runRules();
      setRunResult(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setIsRunning(false);
    }
  };

  const getTagName = (tagId: number): string => {
    return tags.find((t) => t.id === tagId)?.name ?? 'Unknown';
  };

  const getTagColor = (tagId: number): string | undefined => {
    return tags.find((t) => t.id === tagId)?.color;
  };

  return (
    <div className="space-y-4">
      {error && (
        <div className="p-3 bg-red-100 dark:bg-red-900/50 border border-red-300 dark:border-red-700 rounded text-sm text-red-700 dark:text-red-200">
          {error}
        </div>
      )}

      {runResult && (
        <div className="p-3 bg-green-100 dark:bg-green-900/50 border border-green-300 dark:border-green-700 rounded text-sm text-green-700 dark:text-green-200">
          Processed {runResult.emailsProcessed} emails with {runResult.rulesExecuted} rules.
          Applied {runResult.tagsApplied} tags.
        </div>
      )}

      <div className="flex justify-between items-center">
        <button
          onClick={() => {
            setIsCreating(true);
            setEditingRule(null);
          }}
          className="flex items-center gap-2 px-3 py-2 text-sm bg-blue-600 text-white rounded hover:bg-blue-700 transition-colors"
        >
          <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M12 4v16m8-8H4"
            />
          </svg>
          New Rule
        </button>

        <button
          onClick={handleRunRules}
          disabled={isRunning || rules.filter((r) => r.enabled).length === 0}
          className="flex items-center gap-2 px-3 py-2 text-sm bg-gray-200 dark:bg-gray-700 text-gray-700 dark:text-white rounded hover:bg-gray-300 dark:hover:bg-gray-600 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          {isRunning ? (
            <>
              <div className="w-4 h-4 border-2 border-gray-700 dark:border-white border-t-transparent rounded-full animate-spin" />
              Running...
            </>
          ) : (
            <>
              <svg
                className="w-4 h-4"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z"
                />
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                />
              </svg>
              Run All Rules
            </>
          )}
        </button>
      </div>

      <div className="space-y-2">
        {rules.length === 0 && !isLoading && (
          <p className="text-sm text-gray-500 dark:text-gray-400 text-center py-4">
            No auto-tagging rules created yet. Create your first rule above.
          </p>
        )}

        {rules.map((rule) => (
          <div
            key={rule.id}
            className={`p-3 rounded border ${
              rule.enabled
                ? 'bg-gray-50 dark:bg-gray-700/50 border-gray-200 dark:border-gray-600'
                : 'bg-gray-100/50 dark:bg-gray-800/50 border-gray-200 dark:border-gray-700 opacity-60'
            }`}
          >
            <div className="flex items-start justify-between">
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <h4 className="text-sm font-medium text-gray-900 dark:text-white">{rule.name}</h4>
                  {!rule.enabled && (
                    <span className="px-1.5 py-0.5 text-xs bg-gray-200 dark:bg-gray-600 text-gray-600 dark:text-gray-300 rounded">
                      Disabled
                    </span>
                  )}
                </div>
                <div className="flex items-center gap-2 mt-1">
                  <span className="text-xs text-gray-500 dark:text-gray-400">
                    {rule.criteria.length} filter{rule.criteria.length !== 1 ? 's' : ''} →
                  </span>
                  <span
                    className="px-1.5 py-0.5 text-xs rounded"
                    style={{
                      backgroundColor: getTagColor(rule.tagId)
                        ? `${getTagColor(rule.tagId)}20`
                        : '#6b728020',
                      color: getTagColor(rule.tagId) || '#9ca3af',
                    }}
                  >
                    {getTagName(rule.tagId)}
                  </span>
                  <span className="text-xs text-gray-400 dark:text-gray-500">
                    ({rule.matchMode === 'all' ? 'AND' : 'OR'})
                  </span>
                </div>
              </div>

              <div className="flex items-center gap-1">
                <button
                  onClick={() => handleToggleEnabled(rule)}
                  className={`p-1 rounded transition-colors ${
                    rule.enabled
                      ? 'text-green-600 dark:text-green-400 hover:text-green-700 dark:hover:text-green-300'
                      : 'text-gray-400 dark:text-gray-400 hover:text-gray-600 dark:hover:text-gray-300'
                  }`}
                  title={rule.enabled ? 'Disable rule' : 'Enable rule'}
                >
                  {rule.enabled ? (
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
                        d="M5 13l4 4L19 7"
                      />
                    </svg>
                  ) : (
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
                        d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636"
                      />
                    </svg>
                  )}
                </button>
                <button
                  onClick={() => {
                    setEditingRule(rule);
                    setIsCreating(false);
                  }}
                  className="p-1 text-gray-400 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white transition-colors"
                  title="Edit rule"
                >
                  <svg
                    className="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z"
                    />
                  </svg>
                </button>
                <button
                  onClick={() => handleDeleteRule(rule.id, rule.name)}
                  className="p-1 text-gray-400 dark:text-gray-400 hover:text-red-500 dark:hover:text-red-400 transition-colors"
                  title="Delete rule"
                >
                  <svg
                    className="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
                    />
                  </svg>
                </button>
              </div>
            </div>
          </div>
        ))}
      </div>

      {(editingRule || isCreating) && (
        <RuleEditor
          rule={editingRule}
          onClose={() => {
            setEditingRule(null);
            setIsCreating(false);
          }}
        />
      )}
    </div>
  );
}
