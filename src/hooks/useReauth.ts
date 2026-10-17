import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

interface Account {
  id: string;
  email: string;
  account_type: string;
  auth_status?: string;
}

export function useReauth() {
  const [reauthAccounts, setReauthAccounts] = useState<Set<string>>(new Set());

  // Listen for re-auth events from the sync service + check on mount
  useEffect(() => {
    const checkInitialReauth = async () => {
      try {
        const accounts = await invoke<Account[]>('get_accounts');
        const needsReauth = accounts
          .filter(acc => acc.auth_status === 'reauth_required')
          .map(acc => acc.email);

        if (needsReauth.length > 0) {
          setReauthAccounts(new Set(needsReauth));
        }
      } catch (error) {
        console.error('Failed to check reauth status:', error);
      }
    };

    checkInitialReauth();

    const unlistenReauth = listen<string>('reauth-required', (event) => {
      setReauthAccounts((prev) => new Set([...prev, event.payload]));
    });

    return () => {
      unlistenReauth.then(fn => fn());
    };
  }, []);

  const handleReauthComplete = (email: string) => {
    setReauthAccounts((prev) => {
      const next = new Set(prev);
      next.delete(email);
      return next;
    });
  };

  return { reauthAccounts, handleReauthComplete };
}
