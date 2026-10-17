import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { TabBar } from './components/tabs/TabBar';
import { TabContent } from './components/tabs/TabContent';

import { MemoryWarningBanner } from './components/memory/MemoryWarningBanner';
import { ReauthBanner } from './components/auth/ReauthBanner';
import { LoginScreen } from './components/auth/LoginScreen';
import { useAuth } from './hooks/useAuth';
import { useMemoryMonitor } from './hooks/useMemoryMonitor';
import { useReauth } from './hooks/useReauth';
import { useTheme } from './hooks/useTheme';
import { useKeyboardNavigation } from './hooks/useKeyboardNavigation';
import { useTabStore } from './store/tabStore';
import { loadAccountColors } from './utils/accountColors';
import type { EmailSummary } from './types/email';

function App() {
  const { isAuthenticated, userEmail, loading, error, message, handleLogin, completeLogin, handleLogout } = useAuth();
  const memoryInfo = useMemoryMonitor();
  const { reauthAccounts, handleReauthComplete } = useReauth();
  const { preference: themePreference, setPreference: setThemePreference } = useTheme();

  const { addEmailTab, openSettingsTab } = useTabStore();

  // Email list state
  const [emails, setEmails] = useState<EmailSummary[]>([]);
  const [fetchingEmails, setFetchingEmails] = useState(false);
  const [emailError, setEmailError] = useState('');

  const handleEmailClick = useCallback((email: EmailSummary) => {
    if (!email.message_id) {
      setEmailError('Cannot view email: missing message ID');
      return;
    }

    const emailAccountEmail = email.account_email || userEmail || 'unknown';
    const accountId = emailAccountEmail.replace(/[^a-zA-Z0-9]/g, '-');
    addEmailTab(
      accountId,
      email.message_id,
      email.subject || 'No Subject',
      emailAccountEmail,
      email.from_address || '',
      email.from_name,
      email.date
    );
  }, [userEmail, addEmailTab]);

  // Keyboard navigation
  useKeyboardNavigation({
    openSettings: () => openSettingsTab('app'),
    onOpenEmail: handleEmailClick,
  });

  const fetchEmails = async () => {
    if (!userEmail) return;

    setFetchingEmails(true);
    setEmailError('');

    try {
      const fetchedEmails = await invoke<EmailSummary[]>('fetch_emails', {
        email: userEmail,
        count: 20
      });
      setEmails(fetchedEmails);
    } catch (err) {
      setEmailError(String(err));
      console.error('Failed to fetch emails:', err);
    } finally {
      setFetchingEmails(false);
    }
  };

  // Load account colors and fetch emails when authenticated
  useEffect(() => {
    if (isAuthenticated && userEmail) {
      loadAccountColors().then(() => fetchEmails());
    }
  }, [isAuthenticated, userEmail]);

  if (!isAuthenticated) {
    return (
      <div className="min-h-screen flex flex-col bg-gray-100 dark:bg-gray-900">
        <LoginScreen message={message} error={error} loading={loading} onLogin={handleLogin} onLoginComplete={completeLogin} />
      </div>
    );
  }

  return (
    <div className="h-screen flex flex-col overflow-hidden bg-gray-100 dark:bg-gray-900">
      <TabBar />

      {memoryInfo && (
        <MemoryWarningBanner level={memoryInfo.level} usedMb={memoryInfo.used_mb} />
      )}

      {[...reauthAccounts].map((email) => (
        <ReauthBanner key={email} email={email} onReauthComplete={handleReauthComplete} />
      ))}

      <div className="flex-1 bg-white dark:bg-gray-800 flex flex-col overflow-hidden">
        <TabContent
          emails={emails}
          fetchingEmails={fetchingEmails}
          emailError={emailError}
          onEmailClick={handleEmailClick}
          onRefresh={fetchEmails}
          themePreference={themePreference}
          onThemeChange={setThemePreference}
          memoryInfo={memoryInfo}
          onLogout={handleLogout}
        />
      </div>
    </div>
  );
}

export default App;
