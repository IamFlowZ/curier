import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTabStore } from '../../store/tabStore';

interface GmailFolder {
  id: string;
  name: string;
  folder_type: string;
  message_count: number | null;
  unread_count: number | null;
}

interface FolderSidebarProps {
  accountId: string;
  accountEmail: string;
}

const CORE_FOLDER_IDS = new Set(['INBOX', 'SENT', 'DRAFT', 'TRASH', 'SPAM', 'STARRED']);

export function FolderSidebar({ accountId, accountEmail }: FolderSidebarProps) {
  const [folders, setFolders] = useState<GmailFolder[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [showAllFolders, setShowAllFolders] = useState(false);
  const { addFolderTab } = useTabStore();

  useEffect(() => {
    const fetchFolders = async () => {
      setLoading(true);
      setError('');
      try {
        const result = await invoke<GmailFolder[]>('get_folders', { accountEmail });
        setFolders(result);
      } catch (err) {
        setError(String(err));
        console.error('Failed to fetch folders:', err);
      } finally {
        setLoading(false);
      }
    };

    fetchFolders();
  }, [accountId]);

  const handleFolderClick = (folder: GmailFolder) => {
    addFolderTab(accountId, folder.id, folder.name, accountEmail);
  };

  const getIconForFolder = (folderId: string) => {
    switch (folderId) {
      case 'INBOX':
        return (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <polyline points="22 12 16 12 14 15 10 15 8 12 2 12" />
            <path d="M5.45 5.11L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z" />
          </svg>
        );
      case 'SENT':
        return (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <line x1="22" y1="2" x2="11" y2="13" />
            <polygon points="22 2 15 22 11 13 2 9 22 2" />
          </svg>
        );
      case 'DRAFT':
        return (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
            <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
          </svg>
        );
      case 'TRASH':
        return (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <polyline points="3 6 5 6 21 6" />
            <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
          </svg>
        );
      case 'SPAM':
        return (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="12" r="10" />
            <line x1="12" y1="8" x2="12" y2="12" />
            <line x1="12" y1="16" x2="12.01" y2="16" />
          </svg>
        );
      case 'STARRED':
        return (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
          </svg>
        );
      default:
        return (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
          </svg>
        );
    }
  };

  const [collapsed, setCollapsed] = useState(true);

  // Listen for keyboard shortcut to toggle sidebar
  useEffect(() => {
    const handler = () => setCollapsed((prev) => !prev);
    window.addEventListener('curier:toggle-sidebar', handler);
    return () => window.removeEventListener('curier:toggle-sidebar', handler);
  }, []);

  return (
    <div
      className={`${collapsed ? 'w-[40px]' : 'w-[200px]'} border-r border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900 flex flex-col overflow-hidden transition-[width] duration-200`}
    >
      <div className="p-2 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between">
        {!collapsed && (
          <span className="pl-2 font-semibold text-sm text-gray-700 dark:text-gray-200">
            Folders
          </span>
        )}
        <button
          onClick={() => setCollapsed((prev) => !prev)}
          className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-500 dark:text-gray-400 transition-colors flex-shrink-0"
          title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            className={`transition-transform duration-200 ${collapsed ? 'rotate-180' : ''}`}
          >
            <polyline points="15 18 9 12 15 6" />
          </svg>
        </button>
      </div>

      {!collapsed && (
        <div className="flex-1 overflow-y-auto">
          {loading ? (
            <div className="p-4 text-gray-500 dark:text-gray-400 text-[13px]">
              Loading folders...
            </div>
          ) : error ? (
            <div className="p-4 text-red-600 dark:text-red-400 text-[13px]">
              {error}
            </div>
          ) : (
            <div className="py-2">
              {folders
                .filter((folder) => showAllFolders || CORE_FOLDER_IDS.has(folder.id))
                .map((folder) => (
                  <button
                    key={folder.id}
                    onClick={() => handleFolderClick(folder)}
                    className="flex items-center gap-2.5 w-full py-2.5 px-4 border-none bg-transparent cursor-pointer text-left text-[13px] text-gray-700 dark:text-gray-300 transition-colors hover:bg-gray-200 dark:hover:bg-gray-700"
                  >
                    <span className="text-gray-500 dark:text-gray-400 flex-shrink-0">
                      {getIconForFolder(folder.id)}
                    </span>
                    <span className="flex-1 overflow-hidden text-ellipsis whitespace-nowrap">
                      {folder.name}
                    </span>
                    {folder.unread_count != null && folder.unread_count > 0 && (
                      <span className="bg-blue-600 text-white text-[11px] px-1.5 py-0.5 rounded-full font-medium">
                        {folder.unread_count}
                      </span>
                    )}
                  </button>
                ))}
              {folders.some((f) => !CORE_FOLDER_IDS.has(f.id)) && (
                <button
                  onClick={() => setShowAllFolders((prev) => !prev)}
                  className="flex items-center gap-2.5 w-full py-2 px-4 border-none bg-transparent cursor-pointer text-left text-[12px] text-gray-400 dark:text-gray-500 transition-colors hover:text-gray-600 dark:hover:text-gray-300"
                >
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    className={`transition-transform duration-200 ${showAllFolders ? 'rotate-180' : ''}`}
                  >
                    <polyline points="6 9 12 15 18 9" />
                  </svg>
                  <span>{showAllFolders ? 'Show less' : 'More folders'}</span>
                </button>
              )}
            </div>
          )}
        </div>
      )}

      {collapsed && (
        <div className="flex-1 overflow-y-auto py-2">
          {!loading && !error && folders
            .filter((folder) => showAllFolders || CORE_FOLDER_IDS.has(folder.id))
            .map((folder) => (
              <button
                key={folder.id}
                onClick={() => handleFolderClick(folder)}
                className="flex items-center justify-center w-full py-2.5 border-none bg-transparent cursor-pointer text-gray-500 dark:text-gray-400 transition-colors hover:bg-gray-200 dark:hover:bg-gray-700 relative group"
                title={folder.name}
              >
                {getIconForFolder(folder.id)}
                {folder.unread_count != null && folder.unread_count > 0 && (
                  <span className="absolute top-1 right-1 w-2 h-2 bg-blue-600 rounded-full" />
                )}
              </button>
            ))}
          {!loading && !error && folders.some((f) => !CORE_FOLDER_IDS.has(f.id)) && (
            <button
              onClick={() => setShowAllFolders((prev) => !prev)}
              className="flex items-center justify-center w-full py-2 border-none bg-transparent cursor-pointer text-gray-400 dark:text-gray-500 transition-colors hover:text-gray-600 dark:hover:text-gray-300"
              title={showAllFolders ? 'Show less' : 'More folders'}
            >
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                className={`transition-transform duration-200 ${showAllFolders ? 'rotate-180' : ''}`}
              >
                <polyline points="6 9 12 15 18 9" />
              </svg>
            </button>
          )}
        </div>
      )}
    </div>
  );
}
