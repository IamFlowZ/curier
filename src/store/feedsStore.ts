import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type {
  Tag,
  TaggingRule,
  Feed,
  CreateTaggingRuleInput,
  CreateFeedInput,
  UpdateFeedInput,
  RuleExecutionResult,
  FilterCriterion,
} from '../types/feeds';
import { queryClient } from '../lib/queryClient';
import { nowInUnixSeconds } from '../utils/time';

interface FeedsState {
  // State
  tags: Tag[];
  rules: TaggingRule[];
  feeds: Feed[];
  isLoading: boolean;
  error: string | null;

  // Tag actions
  loadTags: () => Promise<void>;
  createTag: (name: string, color?: string, icon?: string) => Promise<Tag>;
  updateTag: (tagId: number, name?: string, color?: string, icon?: string) => Promise<void>;
  deleteTag: (tagId: number) => Promise<void>;

  // Email tag actions
  addTagToEmail: (emailId: number, tagId: number) => Promise<void>;
  removeTagFromEmail: (emailId: number, tagId: number) => Promise<void>;
  getEmailTags: (emailId: number) => Promise<Tag[]>;

  // Rule actions
  loadRules: () => Promise<void>;
  createRule: (input: CreateTaggingRuleInput) => Promise<TaggingRule>;
  updateRule: (rule: TaggingRule) => Promise<void>;
  deleteRule: (ruleId: number) => Promise<void>;
  runRules: (emailIds?: number[]) => Promise<RuleExecutionResult>;

  // Feed actions
  loadFeeds: (accountScope?: string | null) => Promise<void>;
  createFeed: (input: CreateFeedInput) => Promise<Feed>;
  updateFeed: (input: UpdateFeedInput) => Promise<void>;
  deleteFeed: (feedId: number) => Promise<void>;
  reorderFeeds: (feedIds: number[]) => Promise<void>;

  // Helper to get feeds for a specific scope
  getFeedsForScope: (accountScope: string | null) => Feed[];

  // Clear error
  clearError: () => void;
}

// Helper to convert snake_case to camelCase for Rust -> TS
function convertTagFromRust(rustTag: {
  id: number;
  name: string;
  color?: string;
  icon?: string;
  created_at: number;
}): Tag {
  return {
    id: rustTag.id,
    name: rustTag.name,
    color: rustTag.color,
    icon: rustTag.icon,
    createdAt: rustTag.created_at,
  };
}

function convertRuleFromRust(rustRule: {
  id: number;
  name: string;
  tag_id: number;
  criteria: FilterCriterion[];
  match_mode: string;
  priority: number;
  enabled: boolean;
  created_at: number;
  updated_at: number;
}): TaggingRule {
  return {
    id: rustRule.id,
    name: rustRule.name,
    tagId: rustRule.tag_id,
    criteria: rustRule.criteria,
    matchMode: rustRule.match_mode as 'all' | 'any',
    priority: rustRule.priority,
    enabled: rustRule.enabled,
    createdAt: rustRule.created_at,
    updatedAt: rustRule.updated_at,
  };
}

function convertFeedFromRust(rustFeed: {
  id: number;
  name: string;
  account_scope?: string | null;
  criteria: FilterCriterion[];
  match_mode: string;
  column_order: number;
  column_width: number;
  created_at: number;
  updated_at: number;
}): Feed {
  return {
    id: rustFeed.id,
    name: rustFeed.name,
    accountScope: rustFeed.account_scope ?? null,
    criteria: rustFeed.criteria,
    matchMode: rustFeed.match_mode as 'all' | 'any',
    columnOrder: rustFeed.column_order,
    columnWidth: rustFeed.column_width,
    createdAt: rustFeed.created_at,
    updatedAt: rustFeed.updated_at,
  };
}

// Helper to convert camelCase to snake_case for TS -> Rust
function convertRuleToRust(rule: TaggingRule): {
  id: number;
  name: string;
  tag_id: number;
  criteria: FilterCriterion[];
  match_mode: string;
  priority: number;
  enabled: boolean;
  created_at: number;
  updated_at: number;
} {
  return {
    id: rule.id,
    name: rule.name,
    tag_id: rule.tagId,
    criteria: rule.criteria,
    match_mode: rule.matchMode,
    priority: rule.priority,
    enabled: rule.enabled,
    created_at: rule.createdAt,
    updated_at: rule.updatedAt,
  };
}

function convertCreateRuleToRust(input: CreateTaggingRuleInput): {
  name: string;
  tag_id: number;
  criteria: FilterCriterion[];
  match_mode: string;
  priority: number;
  enabled: boolean;
} {
  return {
    name: input.name,
    tag_id: input.tagId,
    criteria: input.criteria,
    match_mode: input.matchMode ?? 'all',
    priority: input.priority ?? 0,
    enabled: input.enabled ?? true,
  };
}

function convertCreateFeedToRust(input: CreateFeedInput): {
  name: string;
  account_scope?: string | null;
  criteria: FilterCriterion[];
  match_mode: string;
  column_order: number;
  column_width: number;
} {
  return {
    name: input.name,
    account_scope: input.accountScope,
    criteria: input.criteria,
    match_mode: input.matchMode ?? 'all',
    column_order: input.columnOrder ?? 0,
    column_width: input.columnWidth ?? 4,
  };
}

function convertUpdateFeedToRust(input: UpdateFeedInput): {
  id: number;
  name?: string;
  criteria?: FilterCriterion[];
  match_mode?: string;
  column_width?: number;
} {
  return {
    id: input.id,
    name: input.name,
    criteria: input.criteria,
    match_mode: input.matchMode,
    column_width: input.columnWidth,
  };
}

export const useFeedsStore = create<FeedsState>((set, get) => ({
  tags: [],
  rules: [],
  feeds: [],
  isLoading: false,
  error: null,

  // ========== Tag Actions ==========

  loadTags: async () => {
    set({ isLoading: true, error: null });
    try {
      const rustTags = await invoke<
        Array<{ id: number; name: string; color?: string; icon?: string; created_at: number }>
      >('get_tags');
      const tags = rustTags.map(convertTagFromRust);
      set({ tags, isLoading: false });
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  createTag: async (name: string, color?: string, icon?: string) => {
    set({ isLoading: true, error: null });
    try {
      const rustTag = await invoke<{
        id: number;
        name: string;
        color?: string;
        icon?: string;
        created_at: number;
      }>('create_tag', { name, color, icon });
      const tag = convertTagFromRust(rustTag);
      set((state) => ({ tags: [...state.tags, tag], isLoading: false }));
      return tag;
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  updateTag: async (tagId: number, name?: string, color?: string, icon?: string) => {
    set({ isLoading: true, error: null });
    try {
      await invoke('update_tag', { tagId, name, color, icon });
      set((state) => ({
        tags: state.tags.map((tag) =>
          tag.id === tagId
            ? { ...tag, name: name ?? tag.name, color: color ?? tag.color, icon: icon !== undefined ? icon : tag.icon }
            : tag
        ),
        isLoading: false,
      }));
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  deleteTag: async (tagId: number) => {
    set({ isLoading: true, error: null });
    try {
      await invoke('delete_tag', { tagId });
      set((state) => ({
        tags: state.tags.filter((tag) => tag.id !== tagId),
        isLoading: false,
      }));
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  // ========== Email Tag Actions ==========

  addTagToEmail: async (emailId: number, tagId: number) => {
    try {
      await invoke('add_tag_to_email', { emailId, tagId });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  removeTagFromEmail: async (emailId: number, tagId: number) => {
    try {
      await invoke('remove_tag_from_email', { emailId, tagId });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  getEmailTags: async (emailId: number) => {
    try {
      const rustTags = await invoke<
        Array<{ id: number; name: string; color?: string; icon?: string; created_at: number }>
      >('get_email_tags', { emailId });
      return rustTags.map(convertTagFromRust);
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  // ========== Rule Actions ==========

  loadRules: async () => {
    set({ isLoading: true, error: null });
    try {
      const rustRules = await invoke<
        Array<{
          id: number;
          name: string;
          tag_id: number;
          criteria: FilterCriterion[];
          match_mode: string;
          priority: number;
          enabled: boolean;
          created_at: number;
          updated_at: number;
        }>
      >('get_tagging_rules');
      const rules = rustRules.map(convertRuleFromRust);
      set({ rules, isLoading: false });
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  createRule: async (input: CreateTaggingRuleInput) => {
    set({ isLoading: true, error: null });
    try {
      const rustRule = await invoke<{
        id: number;
        name: string;
        tag_id: number;
        criteria: FilterCriterion[];
        match_mode: string;
        priority: number;
        enabled: boolean;
        created_at: number;
        updated_at: number;
      }>('create_tagging_rule', { input: convertCreateRuleToRust(input) });
      const rule = convertRuleFromRust(rustRule);
      set((state) => ({ rules: [...state.rules, rule], isLoading: false }));
      return rule;
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  updateRule: async (rule: TaggingRule) => {
    set({ isLoading: true, error: null });
    try {
      await invoke('update_tagging_rule', { rule: convertRuleToRust(rule) });
      set((state) => ({
        rules: state.rules.map((r) => (r.id === rule.id ? rule : r)),
        isLoading: false,
      }));
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  deleteRule: async (ruleId: number) => {
    set({ isLoading: true, error: null });
    try {
      await invoke('delete_tagging_rule', { ruleId });
      set((state) => ({
        rules: state.rules.filter((rule) => rule.id !== ruleId),
        isLoading: false,
      }));
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  runRules: async (emailIds?: number[]) => {
    set({ isLoading: true, error: null });
    try {
      const result = await invoke<{
        rules_executed: number;
        emails_processed: number;
        tags_applied: number;
        tags_removed: number;
      }>('run_tagging_rules', { emailIds });
      set({ isLoading: false });
      return {
        rulesExecuted: result.rules_executed,
        emailsProcessed: result.emails_processed,
        tagsApplied: result.tags_applied,
        tagsRemoved: result.tags_removed,
      };
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  // ========== Feed Actions ==========

  loadFeeds: async (accountScope?: string | null) => {
    set({ isLoading: true, error: null });
    try {
      const rustFeeds = await invoke<
        Array<{
          id: number;
          name: string;
          account_scope?: string | null;
          criteria: FilterCriterion[];
          match_mode: string;
          column_order: number;
          column_width: number;
          created_at: number;
          updated_at: number;
        }>
      >('get_feeds', { accountScope });
      const feeds = rustFeeds.map(convertFeedFromRust);
      set({ feeds, isLoading: false });
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  createFeed: async (input: CreateFeedInput) => {
    set({ isLoading: true, error: null });
    try {
      const rustFeed = await invoke<{
        id: number;
        name: string;
        account_scope?: string | null;
        criteria: FilterCriterion[];
        match_mode: string;
        column_order: number;
        column_width: number;
        created_at: number;
        updated_at: number;
      }>('create_feed', { input: convertCreateFeedToRust(input) });
      const feed = convertFeedFromRust(rustFeed);
      set((state) => ({ feeds: [...state.feeds, feed], isLoading: false }));
      return feed;
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  updateFeed: async (input: UpdateFeedInput) => {
    set({ isLoading: true, error: null });
    try {
      await invoke('update_feed', { input: convertUpdateFeedToRust(input) });
      set((state) => ({
        feeds: state.feeds.map((feed) =>
          feed.id === input.id
            ? {
                ...feed,
                name: input.name ?? feed.name,
                criteria: input.criteria ?? feed.criteria,
                matchMode: input.matchMode ?? feed.matchMode,
                columnWidth: input.columnWidth ?? feed.columnWidth,
                updatedAt: nowInUnixSeconds(),
              }
            : feed
        ),
        isLoading: false,
      }));
      // Invalidate the feed query to refresh email list with new criteria
      queryClient.invalidateQueries({ queryKey: ['feed', input.id] });
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  deleteFeed: async (feedId: number) => {
    console.log('[feedsStore] deleteFeed called for:', feedId);
    set({ isLoading: true, error: null });
    try {
      console.log('[feedsStore] Invoking backend delete_feed...');
      await invoke('delete_feed', { feedId });
      console.log('[feedsStore] Backend delete successful, updating state...');
      set((state) => {
        const newFeeds = state.feeds.filter((feed) => feed.id !== feedId);
        console.log('[feedsStore] Feeds before:', state.feeds.length, 'after:', newFeeds.length);
        return {
          feeds: newFeeds,
          isLoading: false,
        };
      });
      console.log('[feedsStore] State updated successfully');
    } catch (error) {
      console.error('[feedsStore] Delete failed:', error);
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  reorderFeeds: async (feedIds: number[]) => {
    set({ isLoading: true, error: null });
    try {
      await invoke('reorder_feeds', { feedIds });
      // Mirror the backend: only the supplied feeds get a new column_order
      // (by position in the list); every other feed is left untouched.
      const columnOrderByFeedId = new Map<number, number>(feedIds.map((id, index) => [id, index]));
      set((state) => ({
        feeds: state.feeds.map((feed) => {
          const columnOrder = columnOrderByFeedId.get(feed.id);
          return columnOrder === undefined ? feed : { ...feed, columnOrder };
        }),
        isLoading: false,
      }));
    } catch (error) {
      set({ error: String(error), isLoading: false });
      throw error;
    }
  },

  getFeedsForScope: (accountScope: string | null) => {
    const { feeds } = get();
    return feeds
      .filter((feed) => feed.accountScope === accountScope)
      .sort((a, b) => a.columnOrder - b.columnOrder);
  },

  clearError: () => set({ error: null }),
}));
