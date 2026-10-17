import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { useFeedsStore } from './feedsStore';
import { queryClient } from '../lib/queryClient';
import { createStoreReset } from '../test/resetStore';
import type { Feed, FilterCriterion, Tag, TaggingRule } from '../types/feeds';

const DEFAULT_MATCH_MODE = 'all';
const DEFAULT_COLUMN_WIDTH = 4;
const DEFAULT_COLUMN_ORDER = 0;
const DEFAULT_RULE_PRIORITY = 0;
const ACCOUNT_SCOPE = 'alice@example.com';
const BACKEND_ERROR = new Error('backend exploded');

const resetFeedsStore = createStoreReset(useFeedsStore);

function store() {
  return useFeedsStore.getState();
}

const SAMPLE_CRITERIA: FilterCriterion[] = [{ field: 'from', operator: 'contains', value: 'news' }];

interface RustTag {
  id: number;
  name: string;
  color?: string;
  icon?: string;
  created_at: number;
}

interface RustRule {
  id: number;
  name: string;
  tag_id: number;
  criteria: FilterCriterion[];
  match_mode: string;
  priority: number;
  enabled: boolean;
  created_at: number;
  updated_at: number;
}

interface RustFeed {
  id: number;
  name: string;
  account_scope?: string | null;
  criteria: FilterCriterion[];
  match_mode: string;
  column_order: number;
  column_width: number;
  created_at: number;
  updated_at: number;
}

function rustTag(id: number, overrides: Partial<RustTag> = {}): RustTag {
  return { id, name: `tag-${id}`, color: '#ff0000', icon: undefined, created_at: 100 + id, ...overrides };
}

function rustRule(id: number, overrides: Partial<RustRule> = {}): RustRule {
  return {
    id,
    name: `rule-${id}`,
    tag_id: 1,
    criteria: SAMPLE_CRITERIA,
    match_mode: 'any',
    priority: 5,
    enabled: true,
    created_at: 10,
    updated_at: 20,
    ...overrides,
  };
}

function rustFeed(id: number, overrides: Partial<RustFeed> = {}): RustFeed {
  return {
    id,
    name: `feed-${id}`,
    account_scope: null,
    criteria: SAMPLE_CRITERIA,
    match_mode: 'all',
    column_order: id,
    column_width: 6,
    created_at: 10,
    updated_at: 20,
    ...overrides,
  };
}

function tsTag(id: number, overrides: Partial<Tag> = {}): Tag {
  return { id, name: `tag-${id}`, color: '#ff0000', icon: undefined, createdAt: 100 + id, ...overrides };
}

function tsFeed(id: number, overrides: Partial<Feed> = {}): Feed {
  return {
    id,
    name: `feed-${id}`,
    accountScope: null,
    criteria: SAMPLE_CRITERIA,
    matchMode: 'all',
    columnOrder: id,
    columnWidth: 6,
    createdAt: 10,
    updatedAt: 20,
    ...overrides,
  };
}

function tsRule(id: number, overrides: Partial<TaggingRule> = {}): TaggingRule {
  return {
    id,
    name: `rule-${id}`,
    tagId: 1,
    criteria: SAMPLE_CRITERIA,
    matchMode: 'any',
    priority: 5,
    enabled: true,
    createdAt: 10,
    updatedAt: 20,
    ...overrides,
  };
}

describe('feedsStore', () => {
  beforeEach(() => {
    resetFeedsStore();
    vi.spyOn(console, 'log').mockImplementation(() => undefined);
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
  });

  describe('tags', () => {
    it('loadTags converts snake_case tags from the backend and clears loading', async () => {
      vi.mocked(invoke).mockResolvedValueOnce([rustTag(1), rustTag(2, { icon: 'star' })]);

      await store().loadTags();

      expect(invoke).toHaveBeenCalledWith('get_tags');
      expect(store().tags).toEqual([tsTag(1), tsTag(2, { icon: 'star' })]);
      expect(store().isLoading).toBe(false);
      expect(store().error).toBeNull();
    });

    it('loadTags records the error, clears loading, and rethrows on failure', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(BACKEND_ERROR);

      await expect(store().loadTags()).rejects.toBe(BACKEND_ERROR);
      expect(store().error).toBe(String(BACKEND_ERROR));
      expect(store().isLoading).toBe(false);
    });

    it('createTag sends the args, appends the converted tag, and returns it', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(rustTag(3, { name: 'Work', color: '#00ff00' }));

      const created = await store().createTag('Work', '#00ff00');

      expect(invoke).toHaveBeenCalledWith('create_tag', { name: 'Work', color: '#00ff00', icon: undefined });
      expect(created).toEqual(tsTag(3, { name: 'Work', color: '#00ff00' }));
      expect(store().tags).toEqual([created]);
    });

    it('updateTag merges only the provided fields into the matching tag', async () => {
      useFeedsStore.setState({ tags: [tsTag(1, { icon: 'old' }), tsTag(2)] });

      await store().updateTag(1, 'Renamed', undefined, undefined);

      expect(invoke).toHaveBeenCalledWith('update_tag', {
        tagId: 1,
        name: 'Renamed',
        color: undefined,
        icon: undefined,
      });
      expect(store().tags[0]).toEqual(tsTag(1, { name: 'Renamed', icon: 'old' }));
      expect(store().tags[1]).toEqual(tsTag(2));
    });

    it('updateTag allows clearing the icon with an empty string', async () => {
      useFeedsStore.setState({ tags: [tsTag(1, { icon: 'old' })] });

      await store().updateTag(1, undefined, undefined, '');

      expect(store().tags[0].icon).toBe('');
    });

    it('deleteTag removes the tag locally after the backend succeeds', async () => {
      useFeedsStore.setState({ tags: [tsTag(1), tsTag(2)] });

      await store().deleteTag(1);

      expect(invoke).toHaveBeenCalledWith('delete_tag', { tagId: 1 });
      expect(store().tags.map((t) => t.id)).toEqual([2]);
    });

    it('deleteTag keeps the tag when the backend fails', async () => {
      useFeedsStore.setState({ tags: [tsTag(1)] });
      vi.mocked(invoke).mockRejectedValueOnce(BACKEND_ERROR);

      await expect(store().deleteTag(1)).rejects.toBe(BACKEND_ERROR);
      expect(store().tags).toHaveLength(1);
      expect(store().error).toBe(String(BACKEND_ERROR));
    });
  });

  describe('email tag association', () => {
    it('addTagToEmail and removeTagFromEmail forward ids to the backend', async () => {
      await store().addTagToEmail(10, 1);
      await store().removeTagFromEmail(10, 1);

      expect(invoke).toHaveBeenNthCalledWith(1, 'add_tag_to_email', { emailId: 10, tagId: 1 });
      expect(invoke).toHaveBeenNthCalledWith(2, 'remove_tag_from_email', { emailId: 10, tagId: 1 });
      expect(store().error).toBeNull();
    });

    it('addTagToEmail records the error and rethrows without toggling isLoading', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(BACKEND_ERROR);

      await expect(store().addTagToEmail(10, 1)).rejects.toBe(BACKEND_ERROR);
      expect(store().error).toBe(String(BACKEND_ERROR));
      expect(store().isLoading).toBe(false);
    });

    it('getEmailTags returns converted tags without touching store state', async () => {
      vi.mocked(invoke).mockResolvedValueOnce([rustTag(5)]);

      const tags = await store().getEmailTags(10);

      expect(invoke).toHaveBeenCalledWith('get_email_tags', { emailId: 10 });
      expect(tags).toEqual([tsTag(5)]);
      expect(store().tags).toEqual([]);
    });
  });

  describe('rules', () => {
    it('loadRules converts rules from the backend', async () => {
      vi.mocked(invoke).mockResolvedValueOnce([rustRule(1)]);

      await store().loadRules();

      expect(invoke).toHaveBeenCalledWith('get_tagging_rules');
      expect(store().rules).toEqual([tsRule(1)]);
    });

    it('createRule applies defaults for matchMode, priority, and enabled when omitted', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(rustRule(1, { match_mode: DEFAULT_MATCH_MODE, priority: 0 }));

      const created = await store().createRule({ name: 'rule-1', tagId: 1, criteria: SAMPLE_CRITERIA });

      expect(invoke).toHaveBeenCalledWith('create_tagging_rule', {
        input: {
          name: 'rule-1',
          tag_id: 1,
          criteria: SAMPLE_CRITERIA,
          match_mode: DEFAULT_MATCH_MODE,
          priority: DEFAULT_RULE_PRIORITY,
          enabled: true,
        },
      });
      expect(created.matchMode).toBe(DEFAULT_MATCH_MODE);
      expect(store().rules).toEqual([created]);
    });

    it('createRule preserves explicit matchMode, priority, and enabled=false', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(rustRule(2, { enabled: false }));

      await store().createRule({
        name: 'rule-2',
        tagId: 1,
        criteria: SAMPLE_CRITERIA,
        matchMode: 'any',
        priority: 9,
        enabled: false,
      });

      expect(invoke).toHaveBeenCalledWith('create_tagging_rule', {
        input: expect.objectContaining({ match_mode: 'any', priority: 9, enabled: false }),
      });
    });

    it('updateRule sends a snake_case payload and replaces the rule locally', async () => {
      useFeedsStore.setState({ rules: [tsRule(1), tsRule(2)] });
      const updated = tsRule(1, { name: 'changed', enabled: false });

      await store().updateRule(updated);

      expect(invoke).toHaveBeenCalledWith('update_tagging_rule', {
        rule: {
          id: 1,
          name: 'changed',
          tag_id: 1,
          criteria: SAMPLE_CRITERIA,
          match_mode: 'any',
          priority: 5,
          enabled: false,
          created_at: 10,
          updated_at: 20,
        },
      });
      expect(store().rules).toEqual([updated, tsRule(2)]);
    });

    it('deleteRule removes the rule locally after the backend succeeds', async () => {
      useFeedsStore.setState({ rules: [tsRule(1), tsRule(2)] });

      await store().deleteRule(2);

      expect(invoke).toHaveBeenCalledWith('delete_tagging_rule', { ruleId: 2 });
      expect(store().rules.map((r) => r.id)).toEqual([1]);
    });

    it('runRules converts the execution summary to camelCase', async () => {
      vi.mocked(invoke).mockResolvedValueOnce({
        rules_executed: 2,
        emails_processed: 30,
        tags_applied: 4,
        tags_removed: 1,
      });

      const result = await store().runRules([1, 2]);

      expect(invoke).toHaveBeenCalledWith('run_tagging_rules', { emailIds: [1, 2] });
      expect(result).toEqual({ rulesExecuted: 2, emailsProcessed: 30, tagsApplied: 4, tagsRemoved: 1 });
      expect(store().isLoading).toBe(false);
    });
  });

  describe('feeds', () => {
    it('loadFeeds passes the scope and converts feeds (missing account_scope becomes null)', async () => {
      vi.mocked(invoke).mockResolvedValueOnce([
        rustFeed(1, { account_scope: undefined }),
        rustFeed(2, { account_scope: ACCOUNT_SCOPE }),
      ]);

      await store().loadFeeds(ACCOUNT_SCOPE);

      expect(invoke).toHaveBeenCalledWith('get_feeds', { accountScope: ACCOUNT_SCOPE });
      expect(store().feeds).toEqual([tsFeed(1), tsFeed(2, { accountScope: ACCOUNT_SCOPE })]);
    });

    it('createFeed applies defaults for matchMode, columnOrder, and columnWidth', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(rustFeed(1, { column_width: DEFAULT_COLUMN_WIDTH, column_order: 0 }));

      const created = await store().createFeed({ name: 'feed-1', criteria: SAMPLE_CRITERIA });

      expect(invoke).toHaveBeenCalledWith('create_feed', {
        input: {
          name: 'feed-1',
          account_scope: undefined,
          criteria: SAMPLE_CRITERIA,
          match_mode: DEFAULT_MATCH_MODE,
          column_order: DEFAULT_COLUMN_ORDER,
          column_width: DEFAULT_COLUMN_WIDTH,
        },
      });
      expect(store().feeds).toEqual([created]);
    });

    it('updateFeed merges provided fields, stamps updatedAt in seconds, and invalidates the feed query', async () => {
      const FIXED_NOW_MS = 1_700_000_000_999;
      const FIXED_NOW_SECONDS = 1_700_000_000;
      vi.useFakeTimers();
      vi.setSystemTime(FIXED_NOW_MS);
      const invalidate = vi.spyOn(queryClient, 'invalidateQueries').mockResolvedValue(undefined);
      useFeedsStore.setState({ feeds: [tsFeed(1), tsFeed(2)] });

      try {
        await store().updateFeed({ id: 1, name: 'Renamed', columnWidth: 12 });
      } finally {
        vi.useRealTimers();
      }

      expect(invoke).toHaveBeenCalledWith('update_feed', {
        input: { id: 1, name: 'Renamed', criteria: undefined, match_mode: undefined, column_width: 12 },
      });
      expect(store().feeds[0]).toEqual(
        tsFeed(1, { name: 'Renamed', columnWidth: 12, updatedAt: FIXED_NOW_SECONDS }),
      );
      expect(store().feeds[1]).toEqual(tsFeed(2));
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ['feed', 1] });
    });

    it('updateFeed does not touch local state or the query cache when the backend fails', async () => {
      const invalidate = vi.spyOn(queryClient, 'invalidateQueries').mockResolvedValue(undefined);
      useFeedsStore.setState({ feeds: [tsFeed(1)] });
      vi.mocked(invoke).mockRejectedValueOnce(BACKEND_ERROR);

      await expect(store().updateFeed({ id: 1, name: 'Renamed' })).rejects.toBe(BACKEND_ERROR);

      expect(store().feeds[0].name).toBe('feed-1');
      expect(invalidate).not.toHaveBeenCalled();
      expect(store().error).toBe(String(BACKEND_ERROR));
    });

    it('deleteFeed removes the feed locally after the backend succeeds', async () => {
      useFeedsStore.setState({ feeds: [tsFeed(1), tsFeed(2)] });

      await store().deleteFeed(1);

      expect(invoke).toHaveBeenCalledWith('delete_feed', { feedId: 1 });
      expect(store().feeds.map((f) => f.id)).toEqual([2]);
    });

    it('reorderFeeds reassigns columnOrder by position and leaves feeds not in the list untouched', async () => {
      useFeedsStore.setState({ feeds: [tsFeed(1), tsFeed(2), tsFeed(3)] });

      await store().reorderFeeds([3, 1]);

      expect(invoke).toHaveBeenCalledWith('reorder_feeds', { feedIds: [3, 1] });
      // Same as the backend: only ids in the list get a new column_order.
      expect(store().feeds).toEqual([tsFeed(1, { columnOrder: 1 }), tsFeed(2), tsFeed(3, { columnOrder: 0 })]);
    });

    it('reorderFeeds ignores unknown ids', async () => {
      useFeedsStore.setState({ feeds: [tsFeed(1)] });

      await store().reorderFeeds([99, 1]);

      expect(store().feeds).toEqual([tsFeed(1, { columnOrder: 1 })]);
    });

    it('reorderFeeds does not touch local state when the backend fails', async () => {
      useFeedsStore.setState({ feeds: [tsFeed(1), tsFeed(2)] });
      vi.mocked(invoke).mockRejectedValueOnce(BACKEND_ERROR);

      await expect(store().reorderFeeds([2, 1])).rejects.toBe(BACKEND_ERROR);

      expect(store().feeds).toEqual([tsFeed(1), tsFeed(2)]);
    });
  });

  describe('getFeedsForScope', () => {
    it('returns only feeds matching the scope, sorted by columnOrder', () => {
      useFeedsStore.setState({
        feeds: [
          tsFeed(1, { accountScope: ACCOUNT_SCOPE, columnOrder: 2 }),
          tsFeed(2, { accountScope: null, columnOrder: 0 }),
          tsFeed(3, { accountScope: ACCOUNT_SCOPE, columnOrder: 1 }),
        ],
      });

      expect(store().getFeedsForScope(ACCOUNT_SCOPE).map((f) => f.id)).toEqual([3, 1]);
      expect(store().getFeedsForScope(null).map((f) => f.id)).toEqual([2]);
      expect(store().getFeedsForScope('nobody@example.com')).toEqual([]);
    });

    it('does not mutate the stored feed order', () => {
      useFeedsStore.setState({
        feeds: [tsFeed(1, { columnOrder: 2 }), tsFeed(2, { columnOrder: 1 })],
      });

      store().getFeedsForScope(null);

      expect(store().feeds.map((f) => f.id)).toEqual([1, 2]);
    });
  });

  describe('clearError', () => {
    it('resets the error to null', () => {
      useFeedsStore.setState({ error: 'boom' });
      store().clearError();
      expect(store().error).toBeNull();
    });
  });
});
