import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useTabStore } from '../store/tabStore';

interface Account {
  id: string;
  email: string;
  account_type: string;
  auth_status?: string;
}

interface AuthState {
  is_authenticated: boolean;
  email: string | null;
}

export function useAuth() {
  const [message, setMessage] = useState('Checking auth...');
  const [isAuthenticated, setIsAuthenticated] = useState(false);
  const [userEmail, setUserEmail] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const { initialize } = useTabStore();

  // Check auth on mount + listen for OAuth URL
  useEffect(() => {
    const checkAuth = async () => {
      try {
        const accounts = await invoke<Account[]>('get_accounts');

        if (accounts.length > 0) {
          const tabAccounts = accounts.map(acc => ({
            id: acc.id,
            email: acc.email,
            accountType: acc.account_type as 'gmail' | 'outlook' | 'imap',
          }));

          // Load saved tab order from settings
          let accountTabOrder: string[] | undefined;
          try {
            const settings = await invoke<{ account_tab_order?: string[] }>('get_settings');
            if (settings.account_tab_order && settings.account_tab_order.length > 0) {
              accountTabOrder = settings.account_tab_order;
            }
          } catch {
            // Settings load failure is non-fatal for tab ordering
          }

          initialize(tabAccounts, accountTabOrder);
          setIsAuthenticated(true);
          setUserEmail(accounts[0].email);
          setMessage(`Loaded ${accounts.length} account(s)`);

          // Auto-run tagging rules on startup
          try {
            const result = await invoke<{ rules_executed: number; emails_processed: number; tags_applied: number }>('run_tagging_rules', { emailIds: null });
            console.log(`Tagging complete: ${result.tags_applied} tags applied to ${result.emails_processed} emails using ${result.rules_executed} rules`);
          } catch (tagError) {
            console.error('Failed to run tagging rules:', tagError);
          }

          // Return accounts that need re-auth
          return accounts
            .filter(acc => acc.auth_status === 'reauth_required')
            .map(acc => acc.email);
        } else {
          const result = await invoke<AuthState>('is_authenticated');
          setIsAuthenticated(result.is_authenticated);
          setUserEmail(result.email);
          setMessage(`Auth: ${result.is_authenticated}, Email: ${result.email || 'none'}`);

          if (result.is_authenticated && result.email) {
            const accountId = result.email.replace(/[^a-zA-Z0-9]/g, '-');
            initialize([{ id: accountId, email: result.email, accountType: 'gmail' }]);
          } else {
            initialize([]);
          }
        }
      } catch (authError) {
        console.error('Auth check error:', authError);
        setMessage(`Error: ${authError}`);
        initialize([]);
      }

      return [];
    };

    checkAuth();

    const unlistenOAuth = listen<string>('oauth-url', (event) => {
      openUrl(event.payload).catch((err) => {
        console.error('Failed to open browser:', err);
        setError('Failed to open browser for authentication.');
      });
    });

    return () => {
      unlistenOAuth.then(fn => fn());
    };
  }, []);

  const handleLogin = async (clientId?: string, clientSecret?: string): Promise<string | null> => {
    setLoading(true);
    setError('');

    try {
      const args: Record<string, string> = {};
      if (clientId && clientSecret) {
        args.clientId = clientId;
        args.clientSecret = clientSecret;
      }
      const result = await invoke<AuthState>('start_google_oauth', args);
      if (result.is_authenticated && result.email) {
        // Don't set authenticated yet — let the caller show the import step first
        setUserEmail(result.email);
        setMessage(`Logged in as ${result.email}`);
        return result.email;
      }
    } catch (err) {
      setError(String(err));
      console.error('OAuth error:', err);
    } finally {
      setLoading(false);
    }
    return null;
  };

  const completeLogin = (email: string) => {
    setIsAuthenticated(true);
    setUserEmail(email);
    setMessage(`Successfully logged in as ${email}`);

    const accountId = email.replace(/[^a-zA-Z0-9]/g, '-');
    initialize([{ id: accountId, email, accountType: 'gmail' }]);
  };

  const handleLogout = async (email?: string) => {
    try {
      await invoke('logout', { email });

      if (email) {
        setMessage(`Logged out of ${email}`);
        const accounts = await invoke<Account[]>('get_accounts');
        const tabAccounts = accounts.map(acc => ({
          id: acc.id,
          email: acc.email,
          accountType: acc.account_type as 'gmail' | 'outlook' | 'imap',
        }));
        initialize(tabAccounts);
      } else {
        setIsAuthenticated(false);
        setUserEmail(null);
        setMessage('Logged out');
        initialize([]);
      }
    } catch (logoutError) {
      console.error('Logout failed:', logoutError);
    }
  };

  return {
    isAuthenticated,
    userEmail,
    loading,
    error,
    message,
    handleLogin,
    completeLogin,
    handleLogout,
  };
}
