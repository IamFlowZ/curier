import type { EmailSummary } from './email';

/**
 * Custom client-side tag for organizing emails
 */
export interface Tag {
  id: number;
  name: string;
  color?: string;
  icon?: string;
  createdAt: number;
}

/**
 * Input for creating a new tag
 */
export interface CreateTagInput {
  name: string;
  color?: string;
}

/**
 * Filter field types for criteria
 */
export type FilterField =
  | 'from'
  | 'to'
  | 'subject'
  | 'body'
  | 'tag'
  | 'is_read'
  | 'has_attachments'
  | 'date';

/**
 * Filter operator types
 */
export type FilterOperator =
  | 'equals'
  | 'contains'
  | 'starts_with'
  | 'wildcard'
  | 'before'
  | 'after'
  | 'between';

/**
 * Filter criterion for matching emails
 */
export interface FilterCriterion {
  /** Field to match */
  field: FilterField;
  /** Operator for comparison */
  operator: FilterOperator;
  /** Value to match against (can be string, boolean, number, or tuple for 'between') */
  value: string | boolean | number | [number, number];
  /** If true, negates the match (NOT operator) */
  negate?: boolean;
}

/**
 * Auto-tagging rule for automatically applying tags to emails
 */
export interface TaggingRule {
  id: number;
  name: string;
  tagId: number;
  criteria: FilterCriterion[];
  /** 'all' = AND logic, 'any' = OR logic */
  matchMode: 'all' | 'any';
  priority: number;
  enabled: boolean;
  createdAt: number;
  updatedAt: number;
}

/**
 * Input for creating a new tagging rule
 */
export interface CreateTaggingRuleInput {
  name: string;
  tagId: number;
  criteria: FilterCriterion[];
  matchMode?: 'all' | 'any';
  priority?: number;
  enabled?: boolean;
}

/**
 * Input for updating an existing tagging rule
 */
export interface UpdateTaggingRuleInput {
  id: number;
  name?: string;
  tagId?: number;
  criteria?: FilterCriterion[];
  matchMode?: 'all' | 'any';
  priority?: number;
  enabled?: boolean;
}

/**
 * Feed column definition
 */
export interface Feed {
  id: number;
  name: string;
  /** NULL = all accounts, specific email = account-scoped */
  accountScope: string | null;
  criteria: FilterCriterion[];
  /** 'all' = AND logic, 'any' = OR logic */
  matchMode: 'all' | 'any';
  columnOrder: number;
  /** Column width on 1-12 scale (Bootstrap-style). Uses proportional flex sizing. */
  columnWidth: number;
  createdAt: number;
  updatedAt: number;
}

/**
 * Input for creating a new feed
 */
export interface CreateFeedInput {
  name: string;
  accountScope?: string | null;
  criteria: FilterCriterion[];
  matchMode?: 'all' | 'any';
  columnOrder?: number;
  /** Column width on 1-12 scale. Defaults to 4. */
  columnWidth?: number;
}

/**
 * Input for updating an existing feed
 */
export interface UpdateFeedInput {
  id: number;
  name?: string;
  criteria?: FilterCriterion[];
  matchMode?: 'all' | 'any';
  /** Column width on 1-12 scale. */
  columnWidth?: number;
}

/**
 * Result of running tagging rules
 */
export interface RuleExecutionResult {
  rulesExecuted: number;
  emailsProcessed: number;
  tagsApplied: number;
  tagsRemoved: number;
}

/**
 * Paginated feed emails response
 */
export interface PaginatedFeedResponse {
  emails: EmailSummary[];
  totalCount: number;
  page: number;
  hasMore: boolean;
}

/**
 * Email with tags attached
 */
export interface EmailWithTags extends EmailSummary {
  tags: Tag[];
}

/**
 * Default colors for tags (user can choose from these or enter custom)
 */
export const TAG_COLORS = [
  '#ef4444', // red
  '#f97316', // orange
  '#eab308', // yellow
  '#22c55e', // green
  '#14b8a6', // teal
  '#3b82f6', // blue
  '#8b5cf6', // violet
  '#ec4899', // pink
  '#6b7280', // gray
] as const;

/**
 * Human-readable labels for filter fields
 */
export const FILTER_FIELD_LABELS: Record<FilterField, string> = {
  from: 'From',
  to: 'To',
  subject: 'Subject',
  body: 'Body',
  tag: 'Tag',
  is_read: 'Read Status',
  has_attachments: 'Has Attachments',
  date: 'Date',
};

/**
 * Human-readable labels for filter operators
 */
export const FILTER_OPERATOR_LABELS: Record<FilterOperator, string> = {
  equals: 'equals',
  contains: 'contains',
  starts_with: 'starts with',
  wildcard: 'matches pattern',
  before: 'before',
  after: 'after',
  between: 'between',
};

/**
 * Valid operators for each field type
 */
export const FIELD_OPERATORS: Record<FilterField, FilterOperator[]> = {
  from: ['equals', 'contains', 'starts_with', 'wildcard'],
  to: ['equals', 'contains', 'starts_with', 'wildcard'],
  subject: ['equals', 'contains', 'starts_with', 'wildcard'],
  body: ['contains'],
  tag: ['equals'],
  is_read: ['equals'],
  has_attachments: ['equals'],
  date: ['before', 'after', 'between'],
};
