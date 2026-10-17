import { useEffect, useRef } from 'react';
import type { WheelEvent } from 'react';
import { Tab } from './Tab';
import { useTabStore } from '../../store/tabStore';
import type { AccountTab } from '../../types/tabs';
import { shouldMapWheelToHorizontalScroll } from '../../utils/tabStripScroll';

/**
 * Hides the native scrollbar (which, as a macOS overlay, paints over the
 * 42px strip and blocks clicks until it fades) while keeping the strip
 * scrollable via wheel, trackpad and keyboard.
 */
const HIDDEN_SCROLLBAR_CLASSES = '[scrollbar-width:none] [&::-webkit-scrollbar]:hidden';

export function TabBar() {
  const { tabs, activeTabId, setActiveTab, closeAndFocusPrevious, addComposeTab, openSettingsTab } = useTabStore();
  const stripRef = useRef<HTMLDivElement>(null);

  // Keep the active tab visible when it changes (e.g. opened at the far right).
  useEffect(() => {
    const strip = stripRef.current;
    if (!strip) return;
    const activeTabElement = strip.querySelector<HTMLElement>(`[data-tab-id="${CSS.escape(activeTabId)}"]`);
    activeTabElement?.scrollIntoView({ inline: 'nearest', block: 'nearest' });
  }, [activeTabId]);

  // A mouse wheel only produces vertical deltas; translate them into
  // horizontal scrolling. Trackpad horizontal gestures pass through untouched.
  const handleWheel = (event: WheelEvent<HTMLDivElement>) => {
    const strip = stripRef.current;
    if (!strip || !shouldMapWheelToHorizontalScroll(event.deltaX, event.deltaY)) return;
    event.preventDefault();
    strip.scrollLeft += event.deltaY;
  };

  const handleCompose = () => {
    const activeTab = tabs.find((t) => t.id === activeTabId);
    const accountTabs = tabs.filter((t) => t.type === 'account') as AccountTab[];
    let defaultAccount = '';
    if (activeTab?.type === 'account') {
      defaultAccount = (activeTab as AccountTab).accountEmail;
    } else if (accountTabs.length > 0) {
      defaultAccount = accountTabs[0].accountEmail;
    }
    addComposeTab(defaultAccount);
  };

  return (
    <div
      ref={stripRef}
      onWheel={handleWheel}
      className={`flex items-end bg-gray-100 dark:bg-gray-900 pt-2 px-3 border-b-2 border-white dark:border-gray-800 overflow-x-auto overflow-y-hidden min-h-[42px] ${HIDDEN_SCROLLBAR_CLASSES}`}
    >
      <div className="flex items-end gap-0.5 flex-1">
        {tabs.map((tab) => (
          <Tab
            key={tab.id}
            tab={tab}
            isActive={tab.id === activeTabId}
            onSelect={() => setActiveTab(tab.id)}
            onClose={tab.closeable ? () => closeAndFocusPrevious(tab.id) : undefined}
          />
        ))}
      </div>

      {/* Right side controls */}
      <div className="flex items-center gap-2 mb-1">
        {/* Compose Button */}
        <button
          onClick={handleCompose}
          className="flex items-center justify-center w-7 h-7 rounded-md border-none bg-transparent cursor-pointer text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-700 hover:text-gray-700 dark:hover:text-gray-200 transition-colors"
          title="Compose (Cmd+N)"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
            <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
          </svg>
        </button>
        {/* Settings Button */}
        <button
          onClick={() => openSettingsTab()}
          className="flex items-center justify-center w-7 h-7 rounded-md border-none bg-transparent cursor-pointer text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-700 hover:text-gray-700 dark:hover:text-gray-200 transition-colors"
          title="Settings"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </button>
      </div>
    </div>
  );
}
