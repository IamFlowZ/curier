import { useState } from 'react';
import { useTabStore } from '../../store/tabStore';
import { getStaleTabs } from '../../utils/memoryEstimator';

interface MemoryWarningBannerProps {
  level: string;
  usedMb: number;
}

export function MemoryWarningBanner({ level, usedMb }: MemoryWarningBannerProps) {
  const [dismissed, setDismissed] = useState(false);
  const { tabs, closeAndFocusPrevious } = useTabStore();

  if (dismissed || (level !== 'orange' && level !== 'red')) {
    return null;
  }

  const staleTabs = getStaleTabs(tabs);

  const handleCloseStaleTabs = () => {
    staleTabs.forEach((tab) => {
      closeAndFocusPrevious(tab.id);
    });
  };

  const isRed = level === 'red';

  return (
    <div
      className={`flex items-center justify-between px-4 py-2.5 text-[13px] border-b ${
        isRed
          ? 'bg-red-100 dark:bg-red-900/30 border-red-200 dark:border-red-800 text-red-600 dark:text-red-400'
          : 'bg-orange-100 dark:bg-orange-900/30 border-orange-200 dark:border-orange-800 text-orange-600 dark:text-orange-400'
      }`}
    >
      <div className="flex items-center gap-2">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <circle cx="12" cy="12" r="10" />
          <line x1="12" y1="8" x2="12" y2="12" />
          <line x1="12" y1="16" x2="12.01" y2="16" />
        </svg>
        <span>
          <strong>High memory usage</strong> ({usedMb.toFixed(0)} MB)
          {staleTabs.length > 0 && ` - ${staleTabs.length} tabs not used in 15+ min`}
        </span>
      </div>

      <div className="flex items-center gap-2">
        {staleTabs.length > 0 && (
          <button
            onClick={handleCloseStaleTabs}
            className={`px-2.5 py-1 bg-white dark:bg-gray-800 border rounded text-xs font-medium cursor-pointer ${
              isRed
                ? 'text-red-600 dark:text-red-400 border-red-200 dark:border-red-700'
                : 'text-orange-600 dark:text-orange-400 border-orange-200 dark:border-orange-700'
            }`}
          >
            Close stale tabs
          </button>
        )}
        <button
          onClick={() => setDismissed(true)}
          className="flex items-center justify-center w-5 h-5 bg-transparent border-none cursor-pointer opacity-70"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      </div>
    </div>
  );
}
