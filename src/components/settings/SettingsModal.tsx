import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  ACCOUNT_COLORS,
  getAccountColor,
  setAccountColorConfig,
  loadAccountColors,
  getConfiguredColors,
} from '../../utils/accountColors';
import { useTabStore } from '../../store/tabStore';
import { FeedSettingsContent } from '../feeds/FeedSettingsPanel';
import { estimateAllTabsMemory, getStaleTabs } from '../../utils/memoryEstimator';
import { formatByteSize } from '../../utils/formatByteSize';
import { AddAccountModal } from '../account/AddAccountModal';
import { CustomSelect } from '../ui/CustomSelect';
import type { ThemePreference } from '../../hooks/useTheme';
import type { MemoryInfo } from '../../hooks/useMemoryMonitor';

interface AppSettings {
  sync_interval_seconds: number;
  enable_background_sync: boolean;
  max_headers_per_folder: number;
  max_bodies_total: number;
  emails_per_page: number;
  approved_image_domains: string[];
  account_tab_order: string[];
  default_view_mode: Record<string, 'feeds' | 'list'>;
  updated_at: number;
}

interface CacheStats {
  email_count: number;
  body_count: number;
  db_size_bytes: number;
}

interface Account {
  email: string;
  display_name: string;
  auth_status: string;
}

export type SettingsTab = 'app' | 'feeds' | 'resources' | 'accounts' | 'keybinds';

interface SettingsPageProps {
  themePreference?: ThemePreference;
  onThemeChange?: (pref: ThemePreference) => void;
  initialTab?: SettingsTab;
  memoryInfo?: MemoryInfo | null;
  onLogout?: (email?: string) => void;
}

const STATUS_COLORS: Record<string, string> = {
  green: 'text-green-600',
  yellow: 'text-amber-600',
  orange: 'text-orange-600',
  red: 'text-red-600',
};

const STATUS_BORDER_COLORS: Record<string, string> = {
  green: 'border-green-600',
  yellow: 'border-amber-600',
  orange: 'border-orange-600',
  red: 'border-red-600',
};

export function SettingsPage({
  themePreference,
  onThemeChange,
  initialTab = 'app',
  memoryInfo,
  onLogout,
}: SettingsPageProps) {
  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [cacheStats, setCacheStats] = useState<CacheStats | null>(null);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [accountColors, setAccountColors] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [error, setError] = useState('');
  const [newDomain, setNewDomain] = useState('');
  const { tabs, getAccountTabs, reorderAccountTabs, closeAndFocusPrevious } = useTabStore();

  // Load data on mount and when section changes
  useEffect(() => {
    setActiveTab(initialTab);
    loadSettings();
    loadCacheStats();
    loadAccounts();
  }, [initialTab]);

  const loadSettings = async () => {
    setLoading(true);
    setError('');
    try {
      const result = await invoke<AppSettings>('get_settings');
      setSettings(result);
    } catch (err) {
      setError(String(err));
      console.error('Failed to load settings:', err);
    } finally {
      setLoading(false);
    }
  };

  const loadCacheStats = async () => {
    try {
      const stats = await invoke<CacheStats>('get_cache_stats');
      setCacheStats(stats);
    } catch (err) {
      console.error('Failed to load cache stats:', err);
    }
  };

  const loadAccounts = async () => {
    try {
      const accts = await invoke<Account[]>('get_accounts');
      setAccounts(accts);

      await loadAccountColors();
      const configured = getConfiguredColors();
      const colorMap: Record<string, string> = {};
      for (const acct of accts) {
        colorMap[acct.email] = configured[acct.email] || getAccountColor(acct.email) || ACCOUNT_COLORS[0];
      }
      setAccountColors(colorMap);
    } catch (err) {
      console.error('Failed to load accounts:', err);
    }
  };

  const handleSave = async () => {
    if (!settings) return;

    setSaving(true);
    setError('');
    try {
      await invoke('update_settings', { settings });
      window.location.reload();
    } catch (err) {
      setError(String(err));
      console.error('Failed to save settings:', err);
      setSaving(false);
    }
  };

  const handleClearCache = async () => {
    const confirmed = window.confirm(
      'Clear all cached emails and bodies? You will need to re-sync your accounts.'
    );
    if (!confirmed) return;

    setClearing(true);
    try {
      await invoke('clear_cache');
      await loadCacheStats();
    } catch (err) {
      setError(String(err));
      console.error('Failed to clear cache:', err);
    } finally {
      setClearing(false);
    }
  };

  const handleColorChange = async (email: string, color: string) => {
    try {
      await setAccountColorConfig(email, color);
      setAccountColors((prev) => ({ ...prev, [email]: color }));
    } catch (err) {
      console.error('Failed to set account color:', err);
    }
  };

  const handleEmailsPerPageChange = (value: number) => {
    if (settings) {
      setSettings({ ...settings, emails_per_page: value });
    }
  };

  const handleDefaultViewModeChange = (inboxKey: string, mode: 'feeds' | 'list') => {
    if (settings) {
      setSettings({
        ...settings,
        default_view_mode: {
          ...settings.default_view_mode,
          [inboxKey]: mode,
        },
      });
    }
  };

  const extractDomain = (input: string): string => {
    let cleaned = input.trim().toLowerCase();
    try {
      if (cleaned.includes('://')) {
        const url = new URL(cleaned);
        return url.hostname;
      }
      if (cleaned.includes('/')) {
        cleaned = cleaned.split('/')[0];
      }
    } catch {
      // Not a valid URL, just use the cleaned string
    }
    return cleaned;
  };

  const handleAddDomain = () => {
    if (!settings || !newDomain.trim()) return;
    const domain = extractDomain(newDomain);
    if (!domain) return;

    if (settings.approved_image_domains.includes(domain)) {
      setNewDomain('');
      return;
    }

    setSettings({
      ...settings,
      approved_image_domains: [...settings.approved_image_domains, domain],
    });
    setNewDomain('');
  };

  const handleRemoveDomain = (domain: string) => {
    if (!settings) return;
    setSettings({
      ...settings,
      approved_image_domains: settings.approved_image_domains.filter((d) => d !== domain),
    });
  };

  const getOrderedAccounts = (): Account[] => {
    if (!settings || accounts.length === 0) return [];
    const order = settings.account_tab_order;
    if (order.length === 0) {
      const currentAccountTabs = getAccountTabs();
      return [...accounts].sort((a, b) => {
        const aIdx = currentAccountTabs.findIndex((t) => t.accountEmail === a.email);
        const bIdx = currentAccountTabs.findIndex((t) => t.accountEmail === b.email);
        return (aIdx === -1 ? Infinity : aIdx) - (bIdx === -1 ? Infinity : bIdx);
      });
    }
    const sorted = order
      .map((email) => accounts.find((a) => a.email === email))
      .filter((a): a is Account => a !== undefined);
    const remaining = accounts.filter((a) => !order.includes(a.email));
    return [...sorted, ...remaining];
  };

  const handleMoveTab = (index: number, direction: 'up' | 'down') => {
    const ordered = getOrderedAccounts();
    const targetIndex = direction === 'up' ? index - 1 : index + 1;
    if (targetIndex < 0 || targetIndex >= ordered.length) return;
    [ordered[index], ordered[targetIndex]] = [ordered[targetIndex], ordered[index]];

    const newOrder = ordered.map((a) => a.email);

    if (settings) {
      setSettings({ ...settings, account_tab_order: newOrder });
    }

    const accountTabs = getAccountTabs();
    const orderedIds = ordered
      .map((acct) => accountTabs.find((t) => t.accountEmail === acct.email)?.accountId)
      .filter((id): id is string => id !== undefined);
    reorderAccountTabs(orderedIds);
  };

  const TAB_ITEMS: { key: SettingsTab; label: string }[] = [
    { key: 'app', label: 'App Settings' },
    { key: 'feeds', label: 'Feed Settings' },
    { key: 'resources', label: 'Resources' },
    { key: 'accounts', label: 'Account Management' },
    { key: 'keybinds', label: 'Keybinds' },
  ];

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Section Tab Bar */}
      <div className="flex border-b border-gray-200 dark:border-gray-700 px-8 pt-3">
        {TAB_ITEMS.map((tab) => (
          <button
            key={tab.key}
            onClick={() => setActiveTab(tab.key)}
            className={`px-4 py-2 text-sm font-medium transition-colors border-none bg-transparent cursor-pointer ${
              activeTab === tab.key
                ? 'text-blue-600 dark:text-blue-400 border-b-2 !border-b-blue-600 dark:!border-b-blue-400'
                : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Tab Content */}
      <div className="flex-1 overflow-y-auto px-8 py-6">
        <div className="max-w-2xl mx-auto">
          {activeTab === 'app' && (
            <AppSettingsTab
              settings={settings}
              loading={loading}
              error={error}
              saving={saving}
              accounts={accounts}
              accountColors={accountColors}
              themePreference={themePreference}
              newDomain={newDomain}
              onThemeChange={onThemeChange}
              onEmailsPerPageChange={handleEmailsPerPageChange}
              onColorChange={handleColorChange}
              onMoveTab={handleMoveTab}
              getOrderedAccounts={getOrderedAccounts}
              onAddDomain={handleAddDomain}
              onRemoveDomain={handleRemoveDomain}
              onNewDomainChange={setNewDomain}
              onDefaultViewModeChange={handleDefaultViewModeChange}
              onSave={handleSave}
            />
          )}
          {activeTab === 'feeds' && <FeedSettingsContent />}
          {activeTab === 'accounts' && (
            <AccountManagementTab
              accounts={accounts}
              onLogout={onLogout}
              onAccountsChanged={loadAccounts}
            />
          )}
          {activeTab === 'resources' && (
            <ResourcesTab
              cacheStats={cacheStats}
              clearing={clearing}
              onClearCache={handleClearCache}
              memoryInfo={memoryInfo ?? null}
              tabs={tabs}
              onCloseTab={closeAndFocusPrevious}
            />
          )}
          {activeTab === 'keybinds' && <KeybindsTab />}
        </div>
      </div>
    </div>
  );
}

// --- App Settings Tab ---

interface AppSettingsTabProps {
  settings: AppSettings | null;
  loading: boolean;
  error: string;
  saving: boolean;
  accounts: Account[];
  accountColors: Record<string, string>;
  themePreference?: ThemePreference;
  newDomain: string;
  onThemeChange?: (pref: ThemePreference) => void;
  onEmailsPerPageChange: (value: number) => void;
  onColorChange: (email: string, color: string) => void;
  onMoveTab: (index: number, direction: 'up' | 'down') => void;
  getOrderedAccounts: () => Account[];
  onAddDomain: () => void;
  onRemoveDomain: (domain: string) => void;
  onNewDomainChange: (value: string) => void;
  onDefaultViewModeChange: (inboxKey: string, mode: 'feeds' | 'list') => void;
  onSave: () => void;
}

function AppSettingsTab({
  settings,
  loading,
  error,
  saving,
  accounts,
  accountColors,
  themePreference,
  newDomain,
  onThemeChange,
  onEmailsPerPageChange,
  onColorChange,
  onMoveTab,
  getOrderedAccounts,
  onAddDomain,
  onRemoveDomain,
  onNewDomainChange,
  onDefaultViewModeChange,
  onSave,
}: AppSettingsTabProps) {
  if (loading) {
    return (
      <div className="text-center py-5 text-gray-500 dark:text-gray-400">
        Loading settings...
      </div>
    );
  }

  if (error) {
    return (
      <div className="bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 p-3 rounded-md mb-4 text-sm">
        {error}
      </div>
    );
  }

  if (!settings) return null;

  return (
    <div className="flex flex-col gap-5">
      {/* Emails Per Page */}
      <div>
        <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
          Emails per page
        </label>
        <CustomSelect
          value={String(settings.emails_per_page)}
          onChange={(val) => onEmailsPerPageChange(Number(val))}
          options={[
            { value: '20', label: '20 emails' },
            { value: '50', label: '50 emails' },
            { value: '100', label: '100 emails' },
          ]}
        />
        <p className="text-xs text-gray-500 dark:text-gray-400 mt-1.5">
          Number of emails to display per page in email lists.
        </p>
      </div>

      {/* Default View Mode */}
      <div>
        <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
          Default View Mode
        </label>
        <div className="space-y-2">
          {/* All Inboxes */}
          <div className="flex items-center justify-between bg-gray-50 dark:bg-gray-900 rounded-md p-2.5">
            <div className="text-sm text-gray-900 dark:text-gray-100">All Inboxes</div>
            <div className="flex bg-gray-200 dark:bg-gray-700 rounded-md p-0.5">
              <button
                type="button"
                onClick={() => onDefaultViewModeChange('all', 'feeds')}
                className={`px-3 py-1 text-xs font-medium border-none rounded cursor-pointer transition-colors ${
                  (settings.default_view_mode?.all ?? 'feeds') === 'feeds'
                    ? 'bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 shadow-sm'
                    : 'bg-transparent text-gray-500 dark:text-gray-400'
                }`}
              >
                Feeds
              </button>
              <button
                type="button"
                onClick={() => onDefaultViewModeChange('all', 'list')}
                className={`px-3 py-1 text-xs font-medium border-none rounded cursor-pointer transition-colors ${
                  (settings.default_view_mode?.all ?? 'feeds') === 'list'
                    ? 'bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 shadow-sm'
                    : 'bg-transparent text-gray-500 dark:text-gray-400'
                }`}
              >
                List
              </button>
            </div>
          </div>
          {/* Per-account */}
          {accounts.map((account) => (
            <div key={account.email} className="flex items-center justify-between bg-gray-50 dark:bg-gray-900 rounded-md p-2.5">
              <div className="text-sm text-gray-900 dark:text-gray-100 truncate mr-3">{account.email}</div>
              <div className="flex bg-gray-200 dark:bg-gray-700 rounded-md p-0.5 flex-shrink-0">
                <button
                  type="button"
                  onClick={() => onDefaultViewModeChange(account.email, 'feeds')}
                  className={`px-3 py-1 text-xs font-medium border-none rounded cursor-pointer transition-colors ${
                    (settings.default_view_mode?.[account.email] ?? 'feeds') === 'feeds'
                      ? 'bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 shadow-sm'
                      : 'bg-transparent text-gray-500 dark:text-gray-400'
                  }`}
                >
                  Feeds
                </button>
                <button
                  type="button"
                  onClick={() => onDefaultViewModeChange(account.email, 'list')}
                  className={`px-3 py-1 text-xs font-medium border-none rounded cursor-pointer transition-colors ${
                    (settings.default_view_mode?.[account.email] ?? 'feeds') === 'list'
                      ? 'bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 shadow-sm'
                      : 'bg-transparent text-gray-500 dark:text-gray-400'
                  }`}
                >
                  List
                </button>
              </div>
            </div>
          ))}
        </div>
        <p className="text-xs text-gray-500 dark:text-gray-400 mt-1.5">
          Choose the default view when opening each inbox. You can still toggle between views at any time.
        </p>
      </div>

      {/* Theme */}
      {onThemeChange && (
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
            Theme
          </label>
          <div className="flex bg-gray-100 dark:bg-gray-700 rounded-md p-0.5">
            {(['system', 'light', 'dark'] as const).map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => onThemeChange(option)}
                className={`flex-1 py-2 text-sm font-medium rounded capitalize border-none cursor-pointer transition-colors ${
                  themePreference === option
                    ? 'bg-white dark:bg-gray-600 text-gray-900 dark:text-white shadow-sm'
                    : 'bg-transparent text-gray-500 dark:text-gray-400'
                }`}
              >
                {option}
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Account Colors */}
      {accounts.length > 0 && (
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
            Account Colors
          </label>
          <div className="space-y-2">
            {accounts.map((account) => (
              <div key={account.email} className="flex items-center gap-3 bg-gray-50 dark:bg-gray-900 rounded-md p-2.5">
                <div className="flex-1 min-w-0">
                  <div className="text-sm text-gray-900 dark:text-gray-100 truncate">{account.email}</div>
                </div>
                <div className="flex gap-1 flex-shrink-0">
                  {ACCOUNT_COLORS.map((color) => (
                    <button
                      key={color}
                      type="button"
                      onClick={() => onColorChange(account.email, color)}
                      className={`w-5 h-5 rounded-full cursor-pointer border-none ${
                        accountColors[account.email] === color
                          ? 'ring-2 ring-offset-1 ring-gray-900 dark:ring-gray-100 ring-offset-white dark:ring-offset-gray-900'
                          : ''
                      }`}
                      style={{ backgroundColor: color }}
                      title={color}
                    />
                  ))}
                </div>
              </div>
            ))}
          </div>
          <p className="text-xs text-gray-500 dark:text-gray-400 mt-1.5">
            Colors are applied to email list rows for easy account identification.
          </p>
        </div>
      )}

      {/* Tab Order */}
      {(() => {
        const orderedAccounts = getOrderedAccounts();
        return orderedAccounts.length > 1 ? (
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Tab Order
            </label>
            <div className="space-y-1">
              <div className="flex items-center gap-3 bg-gray-50 dark:bg-gray-900 rounded-md p-2.5 opacity-60">
                <div className="flex-1 min-w-0">
                  <div className="text-sm text-gray-900 dark:text-gray-100 truncate">All Inboxes</div>
                </div>
                <div className="flex gap-1 flex-shrink-0 w-[52px]" />
              </div>
              {orderedAccounts.map((account, index) => (
                <div key={account.email} className="flex items-center gap-3 bg-gray-50 dark:bg-gray-900 rounded-md p-2.5">
                  <div className="flex-1 min-w-0">
                    <div className="text-sm text-gray-900 dark:text-gray-100 truncate">{account.email}</div>
                  </div>
                  <div className="flex gap-1 flex-shrink-0">
                    <button
                      type="button"
                      onClick={() => onMoveTab(index, 'up')}
                      disabled={index === 0}
                      className={`w-6 h-6 flex items-center justify-center rounded border-none transition-colors ${
                        index === 0
                          ? 'bg-transparent text-gray-300 dark:text-gray-600 cursor-not-allowed'
                          : 'bg-gray-200 dark:bg-gray-700 text-gray-600 dark:text-gray-300 cursor-pointer hover:bg-gray-300 dark:hover:bg-gray-600'
                      }`}
                      title="Move up"
                    >
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                        <polyline points="18 15 12 9 6 15" />
                      </svg>
                    </button>
                    <button
                      type="button"
                      onClick={() => onMoveTab(index, 'down')}
                      disabled={index === orderedAccounts.length - 1}
                      className={`w-6 h-6 flex items-center justify-center rounded border-none transition-colors ${
                        index === orderedAccounts.length - 1
                          ? 'bg-transparent text-gray-300 dark:text-gray-600 cursor-not-allowed'
                          : 'bg-gray-200 dark:bg-gray-700 text-gray-600 dark:text-gray-300 cursor-pointer hover:bg-gray-300 dark:hover:bg-gray-600'
                      }`}
                      title="Move down"
                    >
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                        <polyline points="6 9 12 15 18 9" />
                      </svg>
                    </button>
                  </div>
                </div>
              ))}
            </div>
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1.5">
              Reorder account tabs. Changes are saved with your other settings.
            </p>
          </div>
        ) : null;
      })()}

      {/* Approved Image Domains */}
      <div>
        <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
          Approved Image Domains
        </label>
        <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
          Images from these domains will load automatically without being blocked.
        </p>
        <div className="flex gap-2 mb-2">
          <input
            type="text"
            value={newDomain}
            onChange={(e) => onNewDomainChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                onAddDomain();
              }
            }}
            placeholder="e.g. imgur.com"
            className="flex-1 py-2 px-3 text-sm border border-gray-300 dark:border-gray-600 rounded-md bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 placeholder-gray-400 dark:placeholder-gray-500"
          />
          <button
            type="button"
            onClick={onAddDomain}
            disabled={!newDomain.trim()}
            className={`px-3 py-2 text-sm font-medium rounded-md border-none transition-colors ${
              newDomain.trim()
                ? 'bg-blue-600 text-white cursor-pointer hover:bg-blue-700'
                : 'bg-gray-200 dark:bg-gray-600 text-gray-400 dark:text-gray-500 cursor-not-allowed'
            }`}
          >
            Add
          </button>
        </div>
        {settings.approved_image_domains.length > 0 && (
          <div className="space-y-1">
            {settings.approved_image_domains.map((domain) => (
              <div
                key={domain}
                className="flex items-center justify-between bg-gray-50 dark:bg-gray-900 rounded-md px-3 py-2"
              >
                <span className="text-sm text-gray-900 dark:text-gray-100 font-mono">
                  {domain}
                </span>
                <button
                  type="button"
                  onClick={() => onRemoveDomain(domain)}
                  className="text-gray-400 hover:text-red-500 dark:text-gray-500 dark:hover:text-red-400 bg-transparent border-none cursor-pointer p-0.5"
                  title={`Remove ${domain}`}
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <line x1="18" y1="6" x2="6" y2="18" />
                    <line x1="6" y1="6" x2="18" y2="18" />
                  </svg>
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Save Button */}
      <button
        onClick={onSave}
        disabled={saving}
        className={`w-full py-3 border-none rounded-md text-sm font-medium transition-colors ${
          saving
            ? 'bg-gray-400 text-white cursor-not-allowed'
            : 'bg-blue-600 text-white cursor-pointer hover:bg-blue-700'
        }`}
      >
        {saving ? 'Saving...' : 'Save Changes'}
      </button>
      <p className="text-xs text-gray-500 dark:text-gray-400 text-center m-0">
        The page will reload after saving to apply changes.
      </p>
    </div>
  );
}

// --- Resources Tab ---

interface ResourcesTabProps {
  cacheStats: CacheStats | null;
  clearing: boolean;
  onClearCache: () => void;
  memoryInfo: MemoryInfo | null;
  tabs: ReturnType<typeof useTabStore.getState>['tabs'];
  onCloseTab: (tabId: string) => void;
}

function ResourcesTab({
  cacheStats,
  clearing,
  onClearCache,
  memoryInfo,
  tabs,
  onCloseTab,
}: ResourcesTabProps) {
  const tabEstimates = estimateAllTabsMemory(tabs);
  const sortedEstimates = [...tabEstimates].sort((a, b) => b.estimatedMB - a.estimatedMB);
  const staleTabs = getStaleTabs(tabs);

  const handleCloseStaleTabs = () => {
    staleTabs.forEach((tab) => {
      onCloseTab(tab.id);
    });
  };

  return (
    <div className="flex flex-col gap-5">
      {/* Cache & Disk Usage */}
      <div>
        <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
          Cache & Disk Usage
        </label>
        {cacheStats ? (
          <div className="bg-gray-50 dark:bg-gray-900 rounded-md p-3 text-sm text-gray-600 dark:text-gray-400 space-y-1">
            <div className="flex justify-between">
              <span>Cached emails</span>
              <span className="font-medium text-gray-900 dark:text-gray-100">{cacheStats.email_count.toLocaleString()}</span>
            </div>
            <div className="flex justify-between">
              <span>Cached bodies</span>
              <span className="font-medium text-gray-900 dark:text-gray-100">{cacheStats.body_count.toLocaleString()}</span>
            </div>
            <div className="flex justify-between">
              <span>Database size</span>
              <span className="font-medium text-gray-900 dark:text-gray-100">{formatByteSize(cacheStats.db_size_bytes)}</span>
            </div>
          </div>
        ) : (
          <div className="text-sm text-gray-400 dark:text-gray-500">Loading cache stats...</div>
        )}
        <button
          onClick={onClearCache}
          disabled={clearing}
          className={`mt-2 w-full py-2 border rounded-md text-sm font-medium transition-colors ${
            clearing
              ? 'bg-gray-100 dark:bg-gray-700 text-gray-400 dark:text-gray-500 border-gray-200 dark:border-gray-600 cursor-not-allowed'
              : 'bg-white dark:bg-gray-800 text-red-600 dark:text-red-400 border-red-300 dark:border-red-700 cursor-pointer hover:bg-red-50 dark:hover:bg-red-900/20'
          }`}
        >
          {clearing ? 'Clearing...' : 'Clear Cache'}
        </button>
        <p className="text-xs text-gray-500 dark:text-gray-400 mt-1.5">
          Removes all cached emails and bodies. You will need to re-sync.
        </p>
      </div>

      {/* Memory Usage */}
      {memoryInfo && (
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
            Memory Usage
          </label>
          <div className="flex items-center justify-between p-4 bg-gray-50 dark:bg-gray-900 rounded-lg">
            <div>
              <div className={`text-2xl font-semibold ${STATUS_COLORS[memoryInfo.level] || 'text-gray-500'}`}>
                {memoryInfo.used_mb.toFixed(0)} MB
              </div>
              <div className="text-[13px] text-gray-500 dark:text-gray-400">
                of {memoryInfo.limit_mb} MB limit ({memoryInfo.percentage_used.toFixed(0)}%)
              </div>
            </div>
            <div
              className={`w-[60px] h-[60px] rounded-full border-4 ${STATUS_BORDER_COLORS[memoryInfo.level] || 'border-gray-500'} flex items-center justify-center text-sm font-semibold ${STATUS_COLORS[memoryInfo.level] || 'text-gray-500'}`}
            >
              {memoryInfo.level}
            </div>
          </div>
          <p className="text-xs text-gray-400 mt-2 italic">
            Memory shown is for the Rust backend process only. The webview runs as a separate process and is not included.
          </p>
        </div>
      )}

      {/* Quick Actions */}
      {staleTabs.length > 0 && (
        <button
          onClick={handleCloseStaleTabs}
          className="w-full py-3 bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-200 border-none rounded-md text-sm font-medium cursor-pointer hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors"
        >
          Close {staleTabs.length} stale tab{staleTabs.length > 1 ? 's' : ''} (not used in 15+ min)
        </button>
      )}

      {/* Tab Breakdown */}
      {sortedEstimates.length > 0 && (
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
            Memory by Tab (Estimated)
          </label>
          <div className="flex flex-col gap-2">
            {sortedEstimates.map((estimate) => {
              const tab = tabs.find((t) => t.id === estimate.tabId);
              return (
                <div
                  key={estimate.tabId}
                  className={`flex items-center justify-between py-2.5 px-3 rounded-md ${
                    estimate.isStale ? 'bg-amber-50 dark:bg-amber-900/20' : 'bg-gray-50 dark:bg-gray-900'
                  }`}
                >
                  <div className="flex items-center gap-2 flex-1 min-w-0">
                    <span className="text-[13px] text-gray-700 dark:text-gray-300 overflow-hidden text-ellipsis whitespace-nowrap">
                      {estimate.tabTitle}
                    </span>
                    {estimate.isStale && (
                      <span className="text-[10px] px-1.5 py-0.5 bg-amber-600 text-white rounded flex-shrink-0">
                        Stale
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-gray-500 dark:text-gray-400 whitespace-nowrap">
                      ~{estimate.estimatedMB.toFixed(1)} MB
                    </span>
                    {tab?.closeable && (
                      <button
                        onClick={() => onCloseTab(estimate.tabId)}
                        className="flex items-center justify-center w-5 h-5 bg-transparent border-none cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                          <line x1="18" y1="6" x2="6" y2="18" />
                          <line x1="6" y1="6" x2="18" y2="18" />
                        </svg>
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

// --- Account Management Tab ---

interface AccountManagementTabProps {
  accounts: Account[];
  onLogout?: (email?: string) => void;
  onAccountsChanged: () => void;
}

function AccountManagementTab({
  accounts,
  onLogout,
  onAccountsChanged,
}: AccountManagementTabProps) {
  const [loggingOut, setLoggingOut] = useState<string | null>(null);
  const [showAddAccount, setShowAddAccount] = useState(false);

  const handleLogout = async (email: string) => {
    const confirmed = window.confirm(
      `Log out of ${email}? You will need to re-authenticate to access this account.`
    );
    if (!confirmed) return;

    setLoggingOut(email);
    try {
      await onLogout?.(email);
      await onAccountsChanged();
    } finally {
      setLoggingOut(null);
    }
  };

  const handleLogoutAll = async () => {
    const confirmed = window.confirm(
      'Log out of all accounts? You will need to re-authenticate to access your email.'
    );
    if (!confirmed) return;

    setLoggingOut('all');
    try {
      await onLogout?.();
    } finally {
      setLoggingOut(null);
    }
  };

  const handleAccountAdded = () => {
    onAccountsChanged();
  };

  return (
    <div className="flex flex-col gap-5">
      <div>
        <div className="flex items-center justify-between mb-2">
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
            Connected Accounts
          </label>
          <button
            onClick={() => setShowAddAccount(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md border-none bg-blue-600 text-white cursor-pointer hover:bg-blue-700 transition-colors"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            Add Account
          </button>
        </div>
        {accounts.length === 0 ? (
          <div className="text-sm text-gray-500 dark:text-gray-400 py-4 text-center">
            No accounts connected.
          </div>
        ) : (
          <div className="space-y-2">
            {accounts.map((account) => (
              <div
                key={account.email}
                className="flex items-center justify-between bg-gray-50 dark:bg-gray-900 rounded-md p-3"
              >
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                    {account.email}
                  </div>
                  {account.display_name && account.display_name !== account.email && (
                    <div className="text-xs text-gray-500 dark:text-gray-400 truncate">
                      {account.display_name}
                    </div>
                  )}
                </div>
                <div className="flex items-center gap-2 flex-shrink-0">
                  <span className={`text-xs px-2 py-0.5 rounded-full ${
                    account.auth_status === 'authenticated'
                      ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
                      : 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400'
                  }`}>
                    {account.auth_status === 'authenticated' ? 'Connected' : account.auth_status}
                  </span>
                  <button
                    onClick={() => handleLogout(account.email)}
                    disabled={loggingOut !== null}
                    className={`px-3 py-1.5 text-xs font-medium rounded-md border transition-colors ${
                      loggingOut === account.email
                        ? 'bg-gray-100 dark:bg-gray-700 text-gray-400 dark:text-gray-500 border-gray-200 dark:border-gray-600 cursor-not-allowed'
                        : 'bg-white dark:bg-gray-800 text-red-600 dark:text-red-400 border-red-300 dark:border-red-700 cursor-pointer hover:bg-red-50 dark:hover:bg-red-900/20'
                    }`}
                  >
                    {loggingOut === account.email ? 'Logging out...' : 'Logout'}
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {accounts.length > 1 && (
        <button
          onClick={handleLogoutAll}
          disabled={loggingOut !== null}
          className={`w-full py-2.5 border rounded-md text-sm font-medium transition-colors ${
            loggingOut === 'all'
              ? 'bg-gray-100 dark:bg-gray-700 text-gray-400 dark:text-gray-500 border-gray-200 dark:border-gray-600 cursor-not-allowed'
              : 'bg-white dark:bg-gray-800 text-red-600 dark:text-red-400 border-red-300 dark:border-red-700 cursor-pointer hover:bg-red-50 dark:hover:bg-red-900/20'
          }`}
        >
          {loggingOut === 'all' ? 'Logging out...' : 'Logout All Accounts'}
        </button>
      )}

      <AddAccountModal
        isOpen={showAddAccount}
        onClose={() => setShowAddAccount(false)}
        onAccountAdded={handleAccountAdded}
      />
    </div>
  );
}

// --- Keybinds Tab ---

const KEYBIND_SECTIONS: { title: string; bindings: { keys: string; description: string }[] }[] = [
  {
    title: 'Global',
    bindings: [
      { keys: 'Cmd+1-9', description: 'Switch to tab by position' },
      { keys: 'Cmd+Shift+[', description: 'Previous tab' },
      { keys: 'Cmd+Shift+]', description: 'Next tab' },
      { keys: 'Cmd+W', description: 'Close current tab' },
      { keys: 'Cmd+N', description: 'New compose' },
      { keys: 'Cmd+R', description: 'Refresh' },
      { keys: 'Cmd+,', description: 'Open settings' },
      { keys: 'Escape', description: 'Clear selection / close modal / close tab' },
      { keys: '/', description: 'Focus search' },
    ],
  },
  {
    title: 'Email List',
    bindings: [
      { keys: 'j / ArrowDown', description: 'Select next email' },
      { keys: 'k / ArrowUp', description: 'Select previous email' },
      { keys: 'Enter / o', description: 'Open selected email' },
      { keys: '[ ', description: 'Previous page' },
      { keys: ']', description: 'Next page' },
    ],
  },
  {
    title: 'View',
    bindings: [
      { keys: 'v', description: 'Toggle list / feeds view' },
      { keys: '.', description: 'Toggle sidebar' },
    ],
  },
  {
    title: 'Feeds View',
    bindings: [
      { keys: 'h / ArrowLeft', description: 'Focus previous feed column' },
      { keys: 'l / ArrowRight', description: 'Focus next feed column' },
    ],
  },
  {
    title: 'Email Viewer',
    bindings: [
      { keys: 'r', description: 'Reply' },
    ],
  },
  {
    title: 'Compose',
    bindings: [
      { keys: 'Cmd+Enter', description: 'Send email' },
    ],
  },
];

function KeybindsTab() {
  return (
    <div className="flex flex-col gap-5">
      {KEYBIND_SECTIONS.map((section) => (
        <div key={section.title}>
          <h3 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-2">
            {section.title}
          </h3>
          <table className="w-full text-sm">
            <tbody>
              {section.bindings.map((binding) => (
                <tr key={binding.keys} className="border-b border-gray-100 dark:border-gray-700">
                  <td className="py-2 pr-4 w-[45%]">
                    <div className="flex flex-wrap gap-1">
                      {binding.keys.split(' / ').map((key) => (
                        <kbd
                          key={key}
                          className="inline-block px-2 py-0.5 text-xs font-mono bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300 rounded border border-gray-200 dark:border-gray-600"
                        >
                          {key.trim()}
                        </kbd>
                      ))}
                    </div>
                  </td>
                  <td className="py-2 text-gray-600 dark:text-gray-400">
                    {binding.description}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ))}
    </div>
  );
}
