import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

interface ImportStepProps {
  accountEmail: string;
  onComplete: () => void;
}

type Strategy = 'skip' | 'days_30' | 'days_60' | 'days_90' | 'count_100' | 'count_1000' | 'count_10000' | 'all';

interface ImportProgress {
  account_email: string;
  imported: number;
  total_estimate: number | null;
  phase: string;
  done: boolean;
}

const TIME_OPTIONS: { value: Strategy; label: string; description: string }[] = [
  { value: 'days_30', label: 'Last 30 days', description: 'Recent emails only' },
  { value: 'days_60', label: 'Last 60 days', description: 'About 2 months' },
  { value: 'days_90', label: 'Last 90 days', description: 'About 3 months' },
];

const COUNT_OPTIONS: { value: Strategy; label: string; description: string }[] = [
  { value: 'count_100', label: '100 emails', description: 'Quick start' },
  { value: 'count_1000', label: '1,000 emails', description: 'Moderate import' },
  { value: 'count_10000', label: '10,000 emails', description: 'Large import' },
];

export function ImportStep({ accountEmail, onComplete }: ImportStepProps) {
  const [mode, setMode] = useState<'choose' | 'importing'>('choose');
  const [tab, setTab] = useState<'time' | 'count'>('time');
  const [importing, setImporting] = useState(false);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [error, setError] = useState('');
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    const unlisten = listen<ImportProgress>('import-progress', (event) => {
      if (event.payload.account_email === accountEmail) {
        setProgress(event.payload);
      }
    });
    return () => {
      unlisten.then(fn => fn());
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [accountEmail]);

  const handleImport = async (strategy: Strategy) => {
    if (strategy === 'skip') {
      onComplete();
      return;
    }

    setImporting(true);
    setMode('importing');
    setError('');
    setElapsedSeconds(0);

    timerRef.current = setInterval(() => {
      setElapsedSeconds((prev) => prev + 1);
    }, 1000);

    try {
      await invoke('import_emails', {
        accountEmail,
        strategy,
      });
      if (timerRef.current) clearInterval(timerRef.current);
      onComplete();
    } catch (err) {
      if (timerRef.current) clearInterval(timerRef.current);
      setError(String(err));
      setImporting(false);
      setMode('choose');
    }
  };

  if (mode === 'importing') {
    return (
      <div className="text-center py-4">
        <div className="animate-spin rounded-full h-10 w-10 border-b-2 border-blue-600 mx-auto mb-4" />
        <h3 className="text-base font-medium text-gray-900 dark:text-gray-100 mb-2">
          {progress?.phase === 'fetching'
            ? 'Fetching emails from server...'
            : 'Importing emails...'}
        </h3>
        {progress && (
          <p className="text-sm text-gray-500 dark:text-gray-400">
            {progress.phase === 'fetching'
              ? 'This may take a moment'
              : (
                <>
                  {progress.imported.toLocaleString()} emails imported
                  {progress.total_estimate != null && progress.total_estimate > 0 && !progress.done
                    ? ` of ${progress.total_estimate.toLocaleString()}`
                    : ''}
                </>
              )}
          </p>
        )}
        <p className="text-xs text-gray-400 dark:text-gray-500 mt-2">
          {elapsedSeconds < 60
            ? `${elapsedSeconds}s elapsed`
            : `${Math.floor(elapsedSeconds / 60)}m ${elapsedSeconds % 60}s elapsed`}
        </p>
        {error && (
          <div className="bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 p-3 rounded-md mt-4 text-sm">
            {error}
          </div>
        )}
      </div>
    );
  }

  return (
    <div>
      <p className="text-sm text-gray-600 dark:text-gray-400 mt-0 mb-4 text-center">
        Would you like to import existing emails?
      </p>

      {/* Tab switcher */}
      <div className="flex mb-4 border border-gray-200 dark:border-gray-600 rounded-md overflow-hidden">
        <button
          onClick={() => setTab('time')}
          className={`flex-1 py-2 text-sm font-medium border-none cursor-pointer transition-colors ${
            tab === 'time'
              ? 'bg-blue-600 text-white'
              : 'bg-white dark:bg-gray-700 text-gray-600 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-600'
          }`}
        >
          By time
        </button>
        <button
          onClick={() => setTab('count')}
          className={`flex-1 py-2 text-sm font-medium border-none cursor-pointer transition-colors ${
            tab === 'count'
              ? 'bg-blue-600 text-white'
              : 'bg-white dark:bg-gray-700 text-gray-600 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-600'
          }`}
        >
          By count
        </button>
      </div>

      {/* Options */}
      <div className="flex flex-col gap-2 mb-3">
        {(tab === 'time' ? TIME_OPTIONS : COUNT_OPTIONS).map((option) => (
          <button
            key={option.value}
            onClick={() => handleImport(option.value)}
            disabled={importing}
            className="flex items-center justify-between p-3 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 cursor-pointer text-left transition-colors hover:border-blue-500 dark:hover:border-blue-400 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <div>
              <div className="text-sm font-medium text-gray-900 dark:text-gray-100">{option.label}</div>
              <div className="text-xs text-gray-500 dark:text-gray-400">{option.description}</div>
            </div>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-gray-400 dark:text-gray-500 flex-shrink-0">
              <polyline points="9 18 15 12 9 6" />
            </svg>
          </button>
        ))}

        {/* All emails option */}
        <button
          onClick={() => handleImport('all')}
          disabled={importing}
          className="flex items-center justify-between p-3 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 cursor-pointer text-left transition-colors hover:border-blue-500 dark:hover:border-blue-400 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <div>
            <div className="text-sm font-medium text-gray-900 dark:text-gray-100">All emails</div>
            <div className="text-xs text-gray-500 dark:text-gray-400">Full inbox import (may take a while)</div>
          </div>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-gray-400 dark:text-gray-500 flex-shrink-0">
            <polyline points="9 18 15 12 9 6" />
          </svg>
        </button>
      </div>

      {error && (
        <div className="bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 p-3 rounded-md mb-3 text-sm">
          {error}
        </div>
      )}

      {/* Skip option */}
      <button
        onClick={() => handleImport('skip')}
        disabled={importing}
        className="w-full py-2.5 bg-transparent text-gray-500 dark:text-gray-400 border-none rounded-md text-sm cursor-pointer hover:text-gray-700 dark:hover:text-gray-200 disabled:cursor-not-allowed"
      >
        Skip — start fresh
      </button>
    </div>
  );
}
