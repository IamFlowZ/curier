import { useState, useEffect } from 'react';
import type { Tab as TabType } from '../../types/tabs';
import { getAccountColor } from '../../utils/accountColors';
import { resolveTabAccountEmail, resolveTabTooltip } from '../../utils/tabAccount';

interface TabProps {
  tab: TabType;
  isActive: boolean;
  onSelect: () => void;
  onClose?: () => void;
}

export function Tab({ tab, isActive, onSelect, onClose }: TabProps) {
  // Colors are keyed by raw email address (see configuredColors in settings),
  // so resolve the email per tab type rather than the munged accountId.
  const accountEmail = resolveTabAccountEmail(tab);
  const [, setColorVersion] = useState(0);

  // Re-render when account colors change
  useEffect(() => {
    const handler = () => setColorVersion((v) => v + 1);
    window.addEventListener('curier:account-colors-changed', handler);
    return () => window.removeEventListener('curier:account-colors-changed', handler);
  }, []);

  const accountColor = getAccountColor(accountEmail);
  const handleClose = (e: React.MouseEvent) => {
    e.stopPropagation();
    onClose?.();
  };

  // Get icon based on tab type
  const getTabIcon = () => {
    switch (tab.type) {
      case 'all':
        return (
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z" />
            <polyline points="22,6 12,13 2,6" />
          </svg>
        );
      case 'account':
        return (
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
            <circle cx="12" cy="7" r="4" />
          </svg>
        );
      case 'folder':
        return (
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
          </svg>
        );
      case 'email':
        return (
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <polyline points="14 2 14 8 20 8" />
            <line x1="16" y1="13" x2="8" y2="13" />
            <line x1="16" y1="17" x2="8" y2="17" />
            <polyline points="10 9 9 9 8 9" />
          </svg>
        );
      case 'compose':
        return (
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
            <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
          </svg>
        );
    }
  };

  return (
    <div
      data-tab-id={tab.id}
      onClick={onSelect}
      className={`flex items-center gap-1.5 py-2 px-3 rounded-t-lg cursor-pointer min-w-[100px] max-w-[200px] relative mr-0.5 transition-colors ${
        isActive
          ? 'bg-white dark:bg-gray-800 border-b-2 border-white dark:border-gray-800 -mb-0.5 shadow-[0_-2px_4px_rgba(0,0,0,0.05)]'
          : 'bg-gray-200 dark:bg-gray-700 border-b-2 border-transparent hover:bg-gray-300 dark:hover:bg-gray-600'
      }`}
    >
      <span className={`flex-shrink-0 ${isActive ? 'text-blue-600 dark:text-blue-400' : 'text-gray-500 dark:text-gray-400'}`}>
        {getTabIcon()}
      </span>
      <span
        title={resolveTabTooltip(tab)}
        className={`text-[13px] overflow-hidden text-ellipsis whitespace-nowrap flex-1 ${
          isActive
            ? 'font-medium text-gray-900 dark:text-gray-100'
            : 'font-normal text-gray-600 dark:text-gray-300'
        }`}
      >
        {tab.title}
      </span>
      {tab.closeable && (
        <button
          onClick={handleClose}
          className="flex items-center justify-center w-[18px] h-[18px] rounded border-none bg-transparent cursor-pointer p-0 text-gray-400 dark:text-gray-500 hover:bg-gray-100 dark:hover:bg-gray-600 hover:text-gray-700 dark:hover:text-gray-300 flex-shrink-0 transition-colors"
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      )}
      {accountColor && (
        <div
          className="absolute bottom-0 left-0 right-0 h-[3px]"
          style={{ backgroundColor: accountColor }}
        />
      )}
    </div>
  );
}
