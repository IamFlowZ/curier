import { useEffect, useRef, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useInfiniteQuery, useQueryClient } from '@tanstack/react-query';
import { useTabStore } from '../../store/tabStore';
import { useFeedsStore } from '../../store/feedsStore';
import { useNavigationStore } from '../../store/navigationStore';
import { FolderSidebar } from '../sidebar/FolderSidebar';
import { EmailViewer } from '../email/EmailViewer';
import { ComposeView } from '../email/ComposeView';
import { FeedContainer } from '../feeds';
import { FeedEmailList } from '../feeds/FeedEmailList';
import { SettingsPage } from '../settings/SettingsModal';
import type { AccountTab, FolderTab, EmailTab, FeedTab, ComposeTab, SettingsTab as SettingsTabType } from '../../types/tabs';
import type { EmailSummary, PaginatedEmailResponse } from '../../types/email';
import type { PaginatedFeedResponse } from '../../types/feeds';
import type { AppSettings } from '../../types/settings';
import { getEmailListRowColor, extractAccountId } from '../../utils/accountColors';
import { formatEmailDate } from '../../utils/formatEmailDate';
import { usePagination } from '../../hooks/usePagination';
import { Pagination } from '../pagination/Pagination';

/** Debounce helper for search input */
function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), delayMs);
    return () => clearTimeout(timer);
  }, [value, delayMs]);
  return debounced;
}

interface TabContentProps {
  onEmailClick: (email: EmailSummary) => void;
  emails?: EmailSummary[];
  fetchingEmails?: boolean;
  emailError?: string;
  onRefresh?: () => void;
  themePreference?: 'system' | 'light' | 'dark';
  onThemeChange?: (pref: 'system' | 'light' | 'dark') => void;
  memoryInfo?: import('../../hooks/useMemoryMonitor').MemoryInfo | null;
  onLogout?: (email?: string) => void;
}

export function TabContent({
  onEmailClick,
  themePreference,
  onThemeChange,
  memoryInfo,
  onLogout,
}: TabContentProps) {
  const { tabs, activeTabId } = useTabStore();
  const activeTab = tabs.find((t) => t.id === activeTabId);

  if (!activeTab) {
    return <div className="p-5">No tab selected</div>;
  }

  switch (activeTab.type) {
    case 'all':
      return (
        <AllTabContent
          onEmailClick={onEmailClick}
        />
      );
    case 'account':
      return (
        <AccountTabContent
          key={activeTab.id}
          tab={activeTab as AccountTab}
          onEmailClick={onEmailClick}
        />
      );
    case 'folder':
      return (
        <FolderTabContent
          key={activeTab.id}
          tab={activeTab as FolderTab}
          onEmailClick={onEmailClick}
        />
      );
    case 'email':
      const emailTab = activeTab as EmailTab;
      return (
        <EmailViewer
          key={activeTab.id}
          emailId={emailTab.emailId}
          subject={emailTab.subject}
          accountEmail={emailTab.accountEmail}
          fromAddress={emailTab.fromAddress}
          fromName={emailTab.fromName}
          date={emailTab.date}
        />
      );
    case 'feed':
      return (
        <FeedTabContent
          key={activeTab.id}
          tab={activeTab as FeedTab}
          onEmailClick={onEmailClick}
        />
      );
    case 'compose': {
      const composeTab = activeTab as ComposeTab;
      return (
        <ComposeView
          key={activeTab.id}
          tabId={activeTab.id}
          accountEmail={composeTab.accountEmail}
          replyTo={composeTab.replyTo}
          replySubject={composeTab.replySubject}
          inReplyTo={composeTab.inReplyTo}
          replyQuotedBody={composeTab.replyQuotedBody}
          replyFromName={composeTab.replyFromName}
          replyDate={composeTab.replyDate}
        />
      );
    }
    case 'settings': {
      const settingsTab = activeTab as SettingsTabType;
      return (
        <SettingsPage
          key={activeTab.id}
          initialTab={settingsTab.section}
          themePreference={themePreference}
          onThemeChange={onThemeChange}
          memoryInfo={memoryInfo}
          onLogout={onLogout}
        />
      );
    }
    default:
      return <div className="p-5">Unknown tab type</div>;
  }
}

// All Tab - Full width, no sidebar, merged view of all accounts
function AllTabContent({
  onEmailClick,
}: {
  onEmailClick: (email: EmailSummary) => void;
}) {
  const [emails, setEmails] = useState<EmailSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [totalCount, setTotalCount] = useState(0);
  const [totalPages, setTotalPages] = useState(0);
  const [itemsPerPage, setItemsPerPage] = useState(20);
  const [searchQuery, setSearchQuery] = useState('');

  const { loadFeeds } = useFeedsStore();
  const allTab = useTabStore((s) => s.tabs.find((t) => t.id === 'all'));
  const setTabViewMode = useTabStore((s) => s.setTabViewMode);
  const setTabCurrentPage = useTabStore((s) => s.setTabCurrentPage);
  const queryClient = useQueryClient();

  const [defaultViewMode, setDefaultViewMode] = useState<'list' | 'feeds'>('feeds');
  const viewMode = allTab?.type === 'all' ? (allTab.viewMode ?? defaultViewMode) : defaultViewMode;
  const savedPage = (allTab?.type === 'all' ? allTab.currentPage : undefined) ?? 1;
  const setViewMode = useCallback((mode: 'list' | 'feeds') => {
    setTabViewMode('all', mode);
  }, [setTabViewMode]);

  const debouncedSearch = useDebouncedValue(searchQuery, 300);
  const pagination = usePagination({ initialPage: savedPage, itemsPerPage });

  // Load feeds on mount
  useEffect(() => {
    loadFeeds(null);
  }, [loadFeeds]);

  // Load settings on mount
  useEffect(() => {
    const loadSettings = async () => {
      try {
        const settings = await invoke<AppSettings>('get_settings');
        if (settings.emails_per_page) {
          setItemsPerPage(settings.emails_per_page);
        }
        if (settings.default_view_mode?.all) {
          setDefaultViewMode(settings.default_view_mode.all as 'list' | 'feeds');
        }
      } catch (err) {
        console.error('Failed to load settings:', err);
      }
    };
    loadSettings();
  }, []);

  // Reset to page 1 when search changes
  useEffect(() => {
    pagination.resetToFirstPage();
  }, [debouncedSearch]);

  // Persist current page to tab store
  useEffect(() => {
    setTabCurrentPage('all', pagination.currentPage);
  }, [pagination.currentPage, setTabCurrentPage]);

  // Fetch emails when page, itemsPerPage, or search changes (list view only)
  useEffect(() => {
    if (viewMode === 'list') {
      fetchAllEmails();
    }
  }, [pagination.currentPage, itemsPerPage, viewMode, debouncedSearch]);

  const fetchAllEmails = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const result = await invoke<PaginatedEmailResponse>('fetch_all_emails', {
        page: pagination.currentPage,
        limit: itemsPerPage,
        searchQuery: debouncedSearch || undefined,
      });
      setEmails(result.emails);
      setTotalCount(result.total_count);
      setTotalPages(result.total_pages);
    } catch (err) {
      setError(String(err));
      console.error('Failed to fetch all emails:', err);
    } finally {
      setLoading(false);
    }
  }, [pagination.currentPage, itemsPerPage, debouncedSearch]);

  const handleRefresh = useCallback(async () => {
    await fetchAllEmails();
    queryClient.invalidateQueries({ queryKey: ['feed'] });
    queryClient.invalidateQueries({ queryKey: ['feed-tab'] });
  }, [fetchAllEmails, queryClient]);

  // Listen for Cmd+R refresh event
  useEffect(() => {
    const handler = () => { handleRefresh(); };
    window.addEventListener('curier:refresh', handler);
    return () => window.removeEventListener('curier:refresh', handler);
  }, [handleRefresh]);

  // Listen for page navigation events
  useEffect(() => {
    if (viewMode !== 'list') return;
    const prevHandler = () => pagination.goToPreviousPage();
    const nextHandler = () => pagination.goToNextPage();
    window.addEventListener('curier:page-prev', prevHandler);
    window.addEventListener('curier:page-next', nextHandler);
    return () => {
      window.removeEventListener('curier:page-prev', prevHandler);
      window.removeEventListener('curier:page-next', nextHandler);
    };
  }, [viewMode, pagination]);

  return (
    <div className="flex flex-col h-full relative">
      {/* Loading Overlay */}
      {viewMode === 'list' && loading && emails.length === 0 && (
        <LoadingOverlay message="Loading all emails..." />
      )}

      {/* Header */}
      <ContentHeader
        searchQuery={searchQuery}
        onSearchChange={setSearchQuery}
      >
        <ViewModeToggle viewMode={viewMode} onChange={setViewMode} />
        <RefreshButton onClick={handleRefresh} loading={loading} />
      </ContentHeader>

      {/* Content based on view mode */}
      {viewMode === 'feeds' ? (
        <FeedContainer accountScope={null} searchQuery={debouncedSearch} />
      ) : (
        <AllTabEmailList
          emails={emails}
          loading={loading}
          error={error}
          onEmailClick={onEmailClick}
          pagination={pagination}
          totalPages={totalPages}
          totalCount={totalCount}
          itemsPerPage={itemsPerPage}
        />
      )}

    </div>
  );
}

function AllTabEmailList({
  emails,
  loading,
  error,
  onEmailClick,
  pagination,
  totalPages,
  totalCount,
  itemsPerPage,
}: {
  emails: EmailSummary[];
  loading: boolean;
  error: string;
  onEmailClick: (email: EmailSummary) => void;
  pagination: ReturnType<typeof usePagination>;
  totalPages: number;
  totalCount: number;
  itemsPerPage: number;
}) {
  const selectedEmailListId = useNavigationStore((s) => s.selectedEmailListId);
  const selectedEmailIndex = useNavigationStore((s) => s.selectedEmailIndex);
  const selectedIndex = selectedEmailListId === 'all' ? selectedEmailIndex : -1;

  return (
    <>
      <EmailList
        emails={emails}
        fetchingEmails={loading}
        emailError={error}
        onEmailClick={onEmailClick}
        showAccountBadge={true}
        scrollKey={pagination.currentPage}
        listId="all"
        selectedIndex={selectedIndex}
      />
      {!loading && emails.length > 0 && (
        <Pagination
          currentPage={pagination.currentPage}
          totalPages={totalPages}
          totalItems={totalCount}
          itemsPerPage={itemsPerPage}
          onPageChange={pagination.goToPage}
          onPreviousPage={pagination.goToPreviousPage}
          onNextPage={pagination.goToNextPage}
          disabled={loading}
        />
      )}
    </>
  );
}

// Account Tab - Shows inbox for a specific account with sidebar
function AccountTabContent({
  tab,
  onEmailClick,
}: {
  tab: AccountTab;
  onEmailClick: (email: EmailSummary) => void;
}) {
  const [emails, setEmails] = useState<EmailSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [totalCount, setTotalCount] = useState(0);
  const [totalPages, setTotalPages] = useState(0);
  const [itemsPerPage, setItemsPerPage] = useState(20);
  const [searchQuery, setSearchQuery] = useState('');

  const { loadFeeds } = useFeedsStore();
  const setTabViewMode = useTabStore((s) => s.setTabViewMode);
  const setTabCurrentPage = useTabStore((s) => s.setTabCurrentPage);
  const queryClient = useQueryClient();

  const [defaultViewMode, setDefaultViewMode] = useState<'list' | 'feeds'>('feeds');
  const viewMode = tab.viewMode ?? defaultViewMode;
  const setViewMode = useCallback((mode: 'list' | 'feeds') => {
    setTabViewMode(tab.id, mode);
  }, [setTabViewMode, tab.id]);

  const debouncedSearch = useDebouncedValue(searchQuery, 300);
  const pagination = usePagination({ initialPage: tab.currentPage ?? 1, itemsPerPage });

  // Load feeds on mount
  useEffect(() => {
    loadFeeds(tab.accountEmail);
  }, [loadFeeds, tab.accountEmail]);

  // Load settings on mount
  useEffect(() => {
    const loadSettings = async () => {
      try {
        const settings = await invoke<AppSettings>('get_settings');
        if (settings.emails_per_page) {
          setItemsPerPage(settings.emails_per_page);
        }
        const accountDefault = settings.default_view_mode?.[tab.accountEmail];
        if (accountDefault) {
          setDefaultViewMode(accountDefault as 'list' | 'feeds');
        }
      } catch (err) {
        console.error('Failed to load settings:', err);
      }
    };
    loadSettings();
  }, [tab.accountEmail]);

  // Reset to page 1 when switching accounts or search changes
  useEffect(() => {
    setEmails([]);
    pagination.resetToFirstPage();
  }, [tab.accountEmail, debouncedSearch]);

  // Persist current page to tab store
  useEffect(() => {
    setTabCurrentPage(tab.id, pagination.currentPage);
  }, [pagination.currentPage, setTabCurrentPage, tab.id]);

  // Fetch emails when page, itemsPerPage, account, or search changes (list view only)
  useEffect(() => {
    if (viewMode === 'list') {
      fetchAccountEmails();
    }
  }, [pagination.currentPage, itemsPerPage, tab.accountEmail, viewMode, debouncedSearch]);

  const fetchAccountEmails = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const result = await invoke<PaginatedEmailResponse>('fetch_emails', {
        email: tab.accountEmail,
        page: pagination.currentPage,
        limit: itemsPerPage,
        searchQuery: debouncedSearch || undefined,
      });
      // Add account_email to each email for proper routing when clicked
      const emailsWithAccount = result.emails.map(e => ({ ...e, account_email: tab.accountEmail }));
      setEmails(emailsWithAccount);
      setTotalCount(result.total_count);
      setTotalPages(result.total_pages);
    } catch (err) {
      setError(String(err));
      console.error('Failed to fetch account emails:', err);
    } finally {
      setLoading(false);
    }
  }, [pagination.currentPage, itemsPerPage, tab.accountEmail, debouncedSearch]);

  const handleRefresh = useCallback(async () => {
    await fetchAccountEmails();
    queryClient.invalidateQueries({ queryKey: ['feed'] });
    queryClient.invalidateQueries({ queryKey: ['feed-tab'] });
  }, [fetchAccountEmails, queryClient]);

  // Listen for Cmd+R refresh event
  useEffect(() => {
    const handler = () => { handleRefresh(); };
    window.addEventListener('curier:refresh', handler);
    return () => window.removeEventListener('curier:refresh', handler);
  }, [handleRefresh]);

  // Listen for page navigation events
  useEffect(() => {
    if (viewMode !== 'list') return;
    const prevHandler = () => pagination.goToPreviousPage();
    const nextHandler = () => pagination.goToNextPage();
    window.addEventListener('curier:page-prev', prevHandler);
    window.addEventListener('curier:page-next', nextHandler);
    return () => {
      window.removeEventListener('curier:page-prev', prevHandler);
      window.removeEventListener('curier:page-next', nextHandler);
    };
  }, [viewMode, pagination]);

  return (
    <div className="flex h-full relative">
      {/* Loading Overlay */}
      {viewMode === 'list' && loading && emails.length === 0 && (
        <LoadingOverlay message="Loading emails..." />
      )}

      {/* Folder Sidebar */}
      <FolderSidebar
        accountId={tab.accountId}
        accountEmail={tab.accountEmail}
      />

      {/* Main Content */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* Header */}
        <ContentHeader
          searchQuery={searchQuery}
          onSearchChange={setSearchQuery}
        >
          <ViewModeToggle viewMode={viewMode} onChange={setViewMode} />
          <RefreshButton onClick={handleRefresh} loading={loading} />
        </ContentHeader>

        {/* Content based on view mode */}
        {viewMode === 'feeds' ? (
          <FeedContainer accountScope={tab.accountEmail} searchQuery={debouncedSearch} />
        ) : (
          <AccountTabEmailList
            tabId={tab.id}
            emails={emails}
            loading={loading}
            error={error}
            onEmailClick={onEmailClick}
            pagination={pagination}
            totalPages={totalPages}
            totalCount={totalCount}
            itemsPerPage={itemsPerPage}
          />
        )}
      </div>

    </div>
  );
}

function AccountTabEmailList({
  tabId,
  emails,
  loading,
  error,
  onEmailClick,
  pagination,
  totalPages,
  totalCount,
  itemsPerPage,
}: {
  tabId: string;
  emails: EmailSummary[];
  loading: boolean;
  error: string;
  onEmailClick: (email: EmailSummary) => void;
  pagination: ReturnType<typeof usePagination>;
  totalPages: number;
  totalCount: number;
  itemsPerPage: number;
}) {
  const selectedEmailListId = useNavigationStore((s) => s.selectedEmailListId);
  const selectedEmailIndex = useNavigationStore((s) => s.selectedEmailIndex);
  const selectedIndex = selectedEmailListId === tabId ? selectedEmailIndex : -1;

  return (
    <>
      <EmailList
        emails={emails}
        fetchingEmails={loading}
        emailError={error}
        onEmailClick={onEmailClick}
        scrollKey={pagination.currentPage}
        listId={tabId}
        selectedIndex={selectedIndex}
      />
      {!loading && emails.length > 0 && (
        <Pagination
          currentPage={pagination.currentPage}
          totalPages={totalPages}
          totalItems={totalCount}
          itemsPerPage={itemsPerPage}
          onPageChange={pagination.goToPage}
          onPreviousPage={pagination.goToPreviousPage}
          onNextPage={pagination.goToNextPage}
          disabled={loading}
        />
      )}
    </>
  );
}

// Folder Tab - Shows emails for a specific folder
function FolderTabContent({
  tab,
  onEmailClick,
}: {
  tab: FolderTab;
  onEmailClick: (email: EmailSummary) => void;
}) {
  const [emails, setEmails] = useState<EmailSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [totalCount, setTotalCount] = useState(0);
  const [totalPages, setTotalPages] = useState(0);
  const [itemsPerPage, setItemsPerPage] = useState(20);
  const [searchQuery, setSearchQuery] = useState('');

  const debouncedSearch = useDebouncedValue(searchQuery, 300);
  const pagination = usePagination({ initialPage: 1, itemsPerPage });

  // Load settings on mount
  useEffect(() => {
    const loadSettings = async () => {
      try {
        const settings = await invoke<AppSettings>('get_settings');
        if (settings.emails_per_page) {
          setItemsPerPage(settings.emails_per_page);
        }
      } catch (err) {
        console.error('Failed to load settings:', err);
      }
    };
    loadSettings();
  }, []);

  // Reset to page 1 when switching folders or search changes
  useEffect(() => {
    setEmails([]);
    pagination.resetToFirstPage();
  }, [tab.folderId, tab.accountEmail, debouncedSearch]);

  // Fetch emails when page, itemsPerPage, folder, or search changes
  useEffect(() => {
    fetchFolderEmails();
  }, [pagination.currentPage, itemsPerPage, tab.folderId, tab.accountEmail, debouncedSearch]);

  const fetchFolderEmails = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const result = await invoke<PaginatedEmailResponse>('fetch_emails_by_label', {
        labelId: tab.folderId,
        page: pagination.currentPage,
        limit: itemsPerPage,
        accountEmail: tab.accountEmail,
        searchQuery: debouncedSearch || undefined,
      });
      // Add account_email to each email for proper routing when clicked
      const emailsWithAccount = result.emails.map(e => ({ ...e, account_email: tab.accountEmail }));
      setEmails(emailsWithAccount);
      setTotalCount(result.total_count);
      setTotalPages(result.total_pages);
    } catch (err) {
      setError(String(err));
      console.error('Failed to fetch folder emails:', err);
    } finally {
      setLoading(false);
    }
  }, [pagination.currentPage, itemsPerPage, tab.folderId, tab.accountEmail, debouncedSearch]);

  // Listen for Cmd+R refresh event
  useEffect(() => {
    const handler = () => fetchFolderEmails();
    window.addEventListener('curier:refresh', handler);
    return () => window.removeEventListener('curier:refresh', handler);
  }, [fetchFolderEmails]);

  // Listen for page navigation events
  useEffect(() => {
    const prevHandler = () => pagination.goToPreviousPage();
    const nextHandler = () => pagination.goToNextPage();
    window.addEventListener('curier:page-prev', prevHandler);
    window.addEventListener('curier:page-next', nextHandler);
    return () => {
      window.removeEventListener('curier:page-prev', prevHandler);
      window.removeEventListener('curier:page-next', nextHandler);
    };
  }, [pagination]);

  return (
    <div className="flex flex-col h-full relative">
      {/* Loading Overlay */}
      {loading && emails.length === 0 && <LoadingOverlay message={`Loading ${tab.folderName}...`} />}

      {/* Header */}
      <ContentHeader
        searchQuery={searchQuery}
        onSearchChange={setSearchQuery}
      >
        <RefreshButton onClick={fetchFolderEmails} loading={loading} />
      </ContentHeader>

      {/* Email List */}
      <FolderTabEmailList
        tabId={tab.id}
        emails={emails}
        loading={loading}
        error={error}
        onEmailClick={onEmailClick}
        pagination={pagination}
        totalPages={totalPages}
        totalCount={totalCount}
        itemsPerPage={itemsPerPage}
      />
    </div>
  );
}

function FolderTabEmailList({
  tabId,
  emails,
  loading,
  error,
  onEmailClick,
  pagination,
  totalPages,
  totalCount,
  itemsPerPage,
}: {
  tabId: string;
  emails: EmailSummary[];
  loading: boolean;
  error: string;
  onEmailClick: (email: EmailSummary) => void;
  pagination: ReturnType<typeof usePagination>;
  totalPages: number;
  totalCount: number;
  itemsPerPage: number;
}) {
  const selectedEmailListId = useNavigationStore((s) => s.selectedEmailListId);
  const selectedEmailIndex = useNavigationStore((s) => s.selectedEmailIndex);
  const selectedIndex = selectedEmailListId === tabId ? selectedEmailIndex : -1;

  return (
    <>
      <EmailList
        emails={emails}
        fetchingEmails={loading}
        emailError={error}
        onEmailClick={onEmailClick}
        scrollKey={pagination.currentPage}
        listId={tabId}
        selectedIndex={selectedIndex}
      />
      {!loading && emails.length > 0 && (
        <Pagination
          currentPage={pagination.currentPage}
          totalPages={totalPages}
          totalItems={totalCount}
          itemsPerPage={itemsPerPage}
          onPageChange={pagination.goToPage}
          onPreviousPage={pagination.goToPreviousPage}
          onNextPage={pagination.goToNextPage}
          disabled={loading}
        />
      )}
    </>
  );
}

// Feed Tab - Shows emails for a specific feed with infinite scroll
interface RustPaginatedFeedResponse {
  emails: EmailSummary[];
  total_count: number;
  page: number;
  has_more: boolean;
}

function FeedTabContent({
  tab,
  onEmailClick,
}: {
  tab: FeedTab;
  onEmailClick: (email: EmailSummary) => void;
}) {
  const {
    data,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading,
  } = useInfiniteQuery({
    queryKey: ['feed-tab', tab.feedId],
    queryFn: async ({ pageParam = 1 }): Promise<PaginatedFeedResponse> => {
      const response = await invoke<RustPaginatedFeedResponse>('fetch_feed_emails', {
        feedId: tab.feedId,
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

  const emails = data?.pages.flatMap((p) => p.emails) ?? [];
  const listId = `feed-tab-${tab.feedId}`;
  const selectedEmailListId = useNavigationStore((s) => s.selectedEmailListId);
  const selectedEmailIndex = useNavigationStore((s) => s.selectedEmailIndex);
  const selectedIndex = selectedEmailListId === listId ? selectedEmailIndex : -1;

  const handleLoadMore = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage) {
      fetchNextPage();
    }
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  return (
    <div className="flex flex-col h-full relative">
      {/* Loading Overlay */}
      {isLoading && emails.length === 0 && <LoadingOverlay message={`Loading ${tab.feedName}...`} />}

      {/* Header */}
      <ContentHeader />

      {/* Email List with Infinite Scroll */}
      <FeedEmailList
        emails={emails}
        isLoading={isLoading || isFetchingNextPage}
        hasMore={hasNextPage ?? false}
        onLoadMore={handleLoadMore}
        onEmailClick={onEmailClick}
        listId={listId}
        selectedIndex={selectedIndex}
      />
    </div>
  );
}

// Shared Components

function ContentHeader({
  children,
  searchQuery,
  onSearchChange,
}: {
  children?: React.ReactNode;
  searchQuery?: string;
  onSearchChange?: (query: string) => void;
}) {
  const searchInputRef = useRef<HTMLInputElement>(null);

  // Listen for keyboard shortcut to focus search
  useEffect(() => {
    const handler = () => searchInputRef.current?.focus();
    window.addEventListener('curier:focus-search', handler);
    return () => window.removeEventListener('curier:focus-search', handler);
  }, []);

  return (
    <div className="px-8 py-3 border-b border-gray-200 dark:border-gray-700 flex justify-between items-center gap-4">
      {onSearchChange && (
        <div className="flex-1 max-w-xs">
          <input
            ref={searchInputRef}
            type="text"
            value={searchQuery ?? ''}
            onChange={(e) => onSearchChange(e.target.value)}
            placeholder="Search emails..."
            className="w-full px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-md bg-white dark:bg-gray-700 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          />
        </div>
      )}
      {children && (
        <div className="flex gap-2 items-center flex-shrink-0 ml-auto">
          {children}
        </div>
      )}
    </div>
  );
}

function EmailList({
  emails,
  fetchingEmails,
  emailError,
  onEmailClick,
  showAccountBadge = false,
  scrollKey,
  listId,
  selectedIndex = -1,
}: {
  emails: EmailSummary[];
  fetchingEmails: boolean;
  emailError: string;
  onEmailClick: (email: EmailSummary) => void;
  showAccountBadge?: boolean;
  scrollKey?: number;
  listId?: string;
  selectedIndex?: number;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  const selectedRowRef = useRef<HTMLDivElement>(null);
  const registerEmailList = useNavigationStore((s) => s.registerEmailList);
  const unregisterEmailList = useNavigationStore((s) => s.unregisterEmailList);

  // Register email list with navigation store
  useEffect(() => {
    if (listId) {
      registerEmailList(listId, emails);
      return () => unregisterEmailList(listId);
    }
  }, [listId, emails, registerEmailList, unregisterEmailList]);

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = 0;
    }
  }, [scrollKey]);

  // Scroll selected row into view
  useEffect(() => {
    if (selectedIndex >= 0 && selectedRowRef.current) {
      selectedRowRef.current.scrollIntoView({ block: 'nearest' });
    }
  }, [selectedIndex]);

  return (
    <div ref={listRef} className="flex-1 overflow-y-auto">
      {emailError && (
        <div className="bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400 py-3 px-8 text-sm">
          {emailError}
        </div>
      )}

      {fetchingEmails && emails.length === 0 ? (
        <div className="p-10 text-center text-gray-500 dark:text-gray-400">
          Loading emails...
        </div>
      ) : emails.length === 0 ? (
        <div className="p-10 text-center text-gray-500 dark:text-gray-400">
          No emails found
        </div>
      ) : (
        emails.map((email, index) => {
          const accountId = extractAccountId(email);
          const rowColor = getEmailListRowColor(accountId, index, email.is_read);
          const isSelected = index === selectedIndex;

          return (
            <div
              key={email.uid}
              ref={isSelected ? selectedRowRef : undefined}
              onClick={() => onEmailClick(email)}
              className={`py-4 px-8 border-b border-gray-100 dark:border-gray-700 cursor-pointer transition-colors hover:!bg-gray-100 dark:hover:!bg-gray-700${isSelected ? ' ring-2 ring-blue-500 ring-inset' : ''}`}
              style={{ backgroundColor: rowColor }}
            >
            <div className="flex justify-between items-start mb-1">
              <div className="flex items-center gap-2">
                <div
                  className="text-gray-900 dark:text-gray-100 text-[15px]"
                  style={{ fontWeight: email.is_read ? 400 : 600 }}
                >
                  {email.from_name || email.from_address}
                </div>
                {showAccountBadge && (
                  <span className="text-[10px] px-1.5 py-0.5 rounded bg-red-100 text-red-600 font-medium">
                    Gmail
                  </span>
                )}
              </div>
              <div className="text-[13px] text-gray-500 dark:text-gray-400 whitespace-nowrap ml-4">
                {formatEmailDate(email.date)}
              </div>
            </div>
            <div
              className="text-sm text-gray-700 dark:text-gray-300 mb-1"
              style={{ fontWeight: email.is_read ? 400 : 600 }}
            >
              {email.subject}
            </div>
            {email.snippet && (
              <div className="text-[13px] text-gray-500 dark:text-gray-400 overflow-hidden text-ellipsis whitespace-nowrap">
                {email.snippet}
              </div>
            )}
            </div>
          );
        })
      )}
    </div>
  );
}

function RefreshButton({ onClick, loading }: { onClick: () => void; loading: boolean }) {
  return (
    <button
      onClick={onClick}
      disabled={loading}
      className={`px-4 py-2 border-none rounded-md text-sm font-medium transition-colors ${
        loading
          ? 'bg-gray-200 dark:bg-gray-700 text-gray-500 dark:text-gray-400 cursor-not-allowed'
          : 'bg-blue-600 text-white cursor-pointer hover:bg-blue-700'
      }`}
    >
      {loading ? 'Refreshing...' : 'Refresh'}
    </button>
  );
}

function LoadingOverlay({ message }: { message: string }) {
  return (
    <div className="absolute inset-0 bg-white/90 dark:bg-gray-800/90 flex flex-col items-center justify-center z-10">
      <div className="w-10 h-10 border-[3px] border-gray-200 dark:border-gray-600 border-t-blue-600 rounded-full animate-spin" />
      <p className="mt-4 text-gray-500 dark:text-gray-400 text-sm font-medium">
        {message}
      </p>
    </div>
  );
}

function ViewModeToggle({
  viewMode,
  onChange,
}: {
  viewMode: 'list' | 'feeds';
  onChange: (mode: 'list' | 'feeds') => void;
}) {
  return (
    <div className="flex bg-gray-200 dark:bg-gray-700 rounded-md p-0.5">
      <button
        onClick={() => onChange('list')}
        className={`px-3 py-1.5 text-[13px] font-medium border-none rounded cursor-pointer transition-colors ${
          viewMode === 'list'
            ? 'bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 shadow-sm'
            : 'bg-transparent text-gray-500 dark:text-gray-400'
        }`}
      >
        List
      </button>
      <button
        onClick={() => onChange('feeds')}
        className={`px-3 py-1.5 text-[13px] font-medium border-none rounded cursor-pointer transition-colors ${
          viewMode === 'feeds'
            ? 'bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 shadow-sm'
            : 'bg-transparent text-gray-500 dark:text-gray-400'
        }`}
      >
        Feeds
      </button>
    </div>
  );
}
