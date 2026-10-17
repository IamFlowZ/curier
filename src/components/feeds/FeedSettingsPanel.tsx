import { useState } from 'react';
import { TagManager } from './TagManager';
import { RulesManager } from './RulesManager';

type SettingsTab = 'tags' | 'rules';

/**
 * Content-only component for feed settings (tags + auto-tagging rules).
 * Renders sub-tab toggle and the active sub-tab content without any modal wrapper.
 */
export function FeedSettingsContent() {
  const [activeTab, setActiveTab] = useState<SettingsTab>('tags');

  return (
    <div className="flex flex-col h-full">
      <div className="flex border-b border-gray-200 dark:border-gray-700">
        <button
          onClick={() => setActiveTab('tags')}
          className={`flex-1 px-4 py-2 text-sm font-medium transition-colors border-none bg-transparent cursor-pointer ${
            activeTab === 'tags'
              ? 'text-blue-600 dark:text-blue-400 border-b-2 !border-b-blue-600 dark:!border-b-blue-400'
              : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'
          }`}
        >
          Tags
        </button>
        <button
          onClick={() => setActiveTab('rules')}
          className={`flex-1 px-4 py-2 text-sm font-medium transition-colors border-none bg-transparent cursor-pointer ${
            activeTab === 'rules'
              ? 'text-blue-600 dark:text-blue-400 border-b-2 !border-b-blue-600 dark:!border-b-blue-400'
              : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'
          }`}
        >
          Auto-Tagging Rules
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-4">
        {activeTab === 'tags' && <TagManager />}
        {activeTab === 'rules' && <RulesManager />}
      </div>
    </div>
  );
}

interface FeedSettingsPanelProps {
  onClose: () => void;
}

/** @deprecated Use FeedSettingsContent inside the unified SettingsModal instead. */
export function FeedSettingsPanel({ onClose }: FeedSettingsPanelProps) {
  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-white dark:bg-gray-800 rounded-lg w-full max-w-2xl max-h-[90vh] overflow-hidden flex flex-col">
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-medium text-gray-900 dark:text-white">Feed Settings</h2>
          <button
            onClick={onClose}
            className="p-1 text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white transition-colors"
          >
            <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
        <FeedSettingsContent />
      </div>
    </div>
  );
}
