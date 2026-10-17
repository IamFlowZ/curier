import type {
  FilterCriterion,
  FilterField,
  FilterOperator,
} from '../../types/feeds';
import {
  FILTER_FIELD_LABELS,
  FILTER_OPERATOR_LABELS,
  FIELD_OPERATORS,
} from '../../types/feeds';
import { useFeedsStore } from '../../store/feedsStore';
import { CustomSelect } from '../ui/CustomSelect';

interface CriteriaBuilderProps {
  criteria: FilterCriterion[];
  onChange: (criteria: FilterCriterion[]) => void;
  matchMode: 'all' | 'any';
  onMatchModeChange: (mode: 'all' | 'any') => void;
}

const FIELDS: FilterField[] = [
  'from',
  'to',
  'subject',
  'body',
  'tag',
  'is_read',
  'has_attachments',
  'date',
];

export function CriteriaBuilder({
  criteria,
  onChange,
  matchMode,
  onMatchModeChange,
}: CriteriaBuilderProps) {
  const { tags } = useFeedsStore();

  const handleAddCriterion = () => {
    const newCriterion: FilterCriterion = {
      field: 'tag',
      operator: 'equals',
      value: tags[0]?.id ?? 0,
      negate: false,
    };
    onChange([...criteria, newCriterion]);
  };

  const handleRemoveCriterion = (index: number) => {
    onChange(criteria.filter((_, i) => i !== index));
  };

  const handleCriterionChange = (
    index: number,
    updates: Partial<FilterCriterion>
  ) => {
    onChange(
      criteria.map((c, i) => {
        if (i !== index) return c;

        const updated = { ...c, ...updates };

        // Reset operator and value when field changes
        if (updates.field && updates.field !== c.field) {
          const validOperators = FIELD_OPERATORS[updates.field as FilterField];
          updated.operator = validOperators[0];
          updated.value = getDefaultValue(updates.field as FilterField);
        }

        return updated;
      })
    );
  };

  const getDefaultValue = (field: FilterField): string | boolean | number => {
    switch (field) {
      case 'is_read':
      case 'has_attachments':
        return false;
      case 'tag':
        return tags[0]?.id ?? 0;
      case 'date':
        return Math.floor(Date.now() / 1000);
      default:
        return '';
    }
  };

  const inputClasses = "flex-1 px-2 py-1 text-sm bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded focus:border-blue-500 focus:outline-none text-gray-900 dark:text-white";

  const renderValueInput = (criterion: FilterCriterion, index: number) => {
    const field = criterion.field as FilterField;

    switch (field) {
      case 'is_read':
      case 'has_attachments':
        return (
          <CustomSelect
            compact
            value={criterion.value === true ? 'true' : 'false'}
            onChange={(val) =>
              handleCriterionChange(index, { value: val === 'true' })
            }
            options={[
              { value: 'true', label: 'Yes' },
              { value: 'false', label: 'No' },
            ]}
            className="flex-1"
          />
        );

      case 'tag':
        return (
          <CustomSelect
            compact
            value={String(criterion.value)}
            onChange={(val) =>
              handleCriterionChange(index, { value: Number(val) })
            }
            placeholder="No tags available"
            options={tags.map((tag) => ({
              value: String(tag.id),
              label: tag.name,
              color: tag.color ?? undefined,
            }))}
            className="flex-1"
          />
        );

      case 'date':
        if (criterion.operator === 'between') {
          const values = Array.isArray(criterion.value)
            ? criterion.value
            : [Date.now() / 1000, Date.now() / 1000];
          return (
            <div className="flex-1 flex gap-2 items-center">
              <input
                type="date"
                value={new Date(values[0] * 1000).toISOString().split('T')[0]}
                onChange={(e) => {
                  const timestamp = new Date(e.target.value).getTime() / 1000;
                  handleCriterionChange(index, {
                    value: [timestamp, values[1]],
                  });
                }}
                className={inputClasses}
              />
              <span className="text-gray-500 dark:text-gray-400 text-sm">to</span>
              <input
                type="date"
                value={new Date(values[1] * 1000).toISOString().split('T')[0]}
                onChange={(e) => {
                  const timestamp = new Date(e.target.value).getTime() / 1000;
                  handleCriterionChange(index, {
                    value: [values[0], timestamp],
                  });
                }}
                className={inputClasses}
              />
            </div>
          );
        }
        return (
          <input
            type="date"
            value={
              new Date((criterion.value as number) * 1000)
                .toISOString()
                .split('T')[0]
            }
            onChange={(e) => {
              const timestamp = new Date(e.target.value).getTime() / 1000;
              handleCriterionChange(index, { value: timestamp });
            }}
            className={inputClasses}
          />
        );

      default:
        return (
          <input
            type="text"
            value={String(criterion.value)}
            onChange={(e) =>
              handleCriterionChange(index, { value: e.target.value })
            }
            placeholder={
              criterion.operator === 'wildcard'
                ? 'e.g., *@github.com'
                : 'Enter value...'
            }
            className={inputClasses}
          />
        );
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <label className="text-sm font-medium text-gray-700 dark:text-gray-300">Match Mode</label>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => onMatchModeChange('all')}
            className={`px-3 py-1 text-sm rounded ${
              matchMode === 'all'
                ? 'bg-blue-600 text-white'
                : 'bg-gray-200 dark:bg-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-600'
            }`}
          >
            All (AND)
          </button>
          <button
            type="button"
            onClick={() => onMatchModeChange('any')}
            className={`px-3 py-1 text-sm rounded ${
              matchMode === 'any'
                ? 'bg-blue-600 text-white'
                : 'bg-gray-200 dark:bg-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-600'
            }`}
          >
            Any (OR)
          </button>
        </div>
      </div>

      <div className="space-y-2">
        {criteria.map((criterion, index) => (
          <div
            key={index}
            className="flex items-center gap-2 p-2 bg-gray-50 dark:bg-gray-700/50 rounded border border-gray-200 dark:border-gray-600"
          >
            <button
              type="button"
              onClick={() =>
                handleCriterionChange(index, { negate: !criterion.negate })
              }
              className={`px-2 py-1 text-xs rounded ${
                criterion.negate
                  ? 'bg-red-600 text-white'
                  : 'bg-gray-200 dark:bg-gray-600 text-gray-600 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-500'
              }`}
              title={criterion.negate ? 'NOT (negated)' : 'Click to negate'}
            >
              {criterion.negate ? 'NOT' : '✓'}
            </button>

            <CustomSelect
              compact
              value={criterion.field}
              onChange={(val) =>
                handleCriterionChange(index, {
                  field: val as FilterField,
                })
              }
              options={FIELDS.map((field) => ({
                value: field,
                label: FILTER_FIELD_LABELS[field],
              }))}
            />

            <CustomSelect
              compact
              value={criterion.operator}
              onChange={(val) =>
                handleCriterionChange(index, {
                  operator: val as FilterOperator,
                })
              }
              options={FIELD_OPERATORS[criterion.field as FilterField].map((op) => ({
                value: op,
                label: FILTER_OPERATOR_LABELS[op],
              }))}
            />

            {renderValueInput(criterion, index)}

            <button
              type="button"
              onClick={() => handleRemoveCriterion(index)}
              className="p-1 text-gray-400 dark:text-gray-400 hover:text-red-500 dark:hover:text-red-400 transition-colors"
              title="Remove criterion"
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
                  d="M6 18L18 6M6 6l12 12"
                />
              </svg>
            </button>
          </div>
        ))}
      </div>

      <button
        type="button"
        onClick={handleAddCriterion}
        className="flex items-center gap-2 px-3 py-2 text-sm text-gray-600 dark:text-gray-300 bg-gray-200 dark:bg-gray-700 hover:bg-gray-300 dark:hover:bg-gray-600 rounded transition-colors"
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
            d="M12 4v16m8-8H4"
          />
        </svg>
        Add Filter
      </button>
    </div>
  );
}
