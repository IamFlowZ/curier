import { invoke } from '@tauri-apps/api/core';
import { useState } from 'react';

interface ReauthBannerProps {
  email: string;
  onReauthComplete: (email: string) => void;
}

export function ReauthBanner({ email, onReauthComplete }: ReauthBannerProps) {
  const [loading, setLoading] = useState(false);

  const handleReauth = async () => {
    setLoading(true);
    try {
      const result = await invoke<{ is_authenticated: boolean; email: string | null }>(
        'start_google_oauth'
      );
      if (result.is_authenticated && result.email) {
        onReauthComplete(result.email);
      }
    } catch (err) {
      console.error('Re-authentication failed:', err);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex items-center justify-between px-4 py-2.5 bg-orange-100 dark:bg-orange-900/30 border-b border-orange-200 dark:border-orange-800 text-orange-600 dark:text-orange-400 text-[13px]">
      <div className="flex items-center gap-2">
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
        >
          <circle cx="12" cy="12" r="10" />
          <line x1="12" y1="8" x2="12" y2="12" />
          <line x1="12" y1="16" x2="12.01" y2="16" />
        </svg>
        <span>
          <strong>{email}</strong> requires re-authentication. Your session has expired.
        </span>
      </div>

      <button
        onClick={handleReauth}
        disabled={loading}
        className={`px-2.5 py-1 bg-white dark:bg-gray-800 text-orange-600 dark:text-orange-400 border border-orange-200 dark:border-orange-700 rounded text-xs font-medium ${
          loading ? 'cursor-not-allowed opacity-70' : 'cursor-pointer'
        }`}
      >
        {loading ? 'Signing in...' : 'Re-authenticate'}
      </button>
    </div>
  );
}
