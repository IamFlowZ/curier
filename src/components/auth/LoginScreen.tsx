import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { ImportStep } from './ImportStep';

interface LoginScreenProps {
  message: string;
  error: string;
  loading: boolean;
  onLogin: (clientId?: string, clientSecret?: string) => Promise<string | null>;
  onLoginComplete: (email: string) => void;
}

interface ProviderCredentialStatus {
  exists: boolean;
  client_id: string | null;
}

const GOOGLE_CONSOLE_URL = 'https://console.cloud.google.com/apis/credentials';

export function LoginScreen({ message, error, loading, onLogin, onLoginComplete }: LoginScreenProps) {
  const [clientId, setClientId] = useState('');
  const [clientSecret, setClientSecret] = useState('');
  const [hasExistingCredentials, setHasExistingCredentials] = useState<boolean | null>(null);
  const [localError, setLocalError] = useState('');
  const [importEmail, setImportEmail] = useState<string | null>(null);

  useEffect(() => {
    invoke<ProviderCredentialStatus>('check_provider_credentials', { provider: 'gmail' })
      .then((status) => setHasExistingCredentials(status.exists))
      .catch(() => setHasExistingCredentials(false));
  }, []);

  const handleSubmit = async () => {
    let email: string | null;
    if (hasExistingCredentials) {
      email = await onLogin();
    } else {
      if (!clientId.trim() || !clientSecret.trim()) {
        setLocalError('Both Client ID and Client Secret are required.');
        return;
      }
      setLocalError('');
      email = await onLogin(clientId.trim(), clientSecret.trim());
    }

    if (email) {
      setImportEmail(email);
    }
  };

  const handleImportComplete = () => {
    if (importEmail) {
      onLoginComplete(importEmail);
    }
  };

  const displayError = error || localError;

  // Still checking for existing credentials
  if (hasExistingCredentials === null) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center p-5">
        <h1 className="text-4xl mb-2.5 text-gray-900 dark:text-gray-100">Curier</h1>
        <p className="text-sm text-gray-500 dark:text-gray-400">Loading...</p>
      </div>
    );
  }

  // Import step after successful OAuth
  if (importEmail) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center p-5">
        <h1 className="text-4xl mb-2.5 text-gray-900 dark:text-gray-100">Curier</h1>
        <p className="text-sm mb-5 text-gray-500 dark:text-gray-400">Logged in as {importEmail}</p>

        <div className="bg-white dark:bg-gray-800 p-8 rounded-lg shadow-md max-w-[440px] w-full">
          <h2 className="text-lg font-medium text-gray-900 dark:text-gray-100 mt-0 mb-4 text-center">
            Import Emails
          </h2>
          <ImportStep accountEmail={importEmail} onComplete={handleImportComplete} />
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col items-center justify-center p-5">
      <h1 className="text-4xl mb-2.5 text-gray-900 dark:text-gray-100">Curier</h1>
      <p className="text-sm mb-5 text-gray-500 dark:text-gray-400">{message}</p>

      <div className="bg-white dark:bg-gray-800 p-8 rounded-lg shadow-md max-w-[440px] w-full">
        {!hasExistingCredentials && (
          <>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-0 mb-4 text-center">
              To get started, you'll need your own Google OAuth credentials.
            </p>

            <details className="mb-5 text-[13px] text-gray-500 dark:text-gray-400">
              <summary className="cursor-pointer text-blue-600 dark:text-blue-400 mb-2">
                How to get credentials
              </summary>
              <ol className="pl-4 space-y-1.5 mt-2">
                <li>Go to the{' '}
                  <button
                    onClick={() => openUrl(GOOGLE_CONSOLE_URL)}
                    className="text-blue-600 dark:text-blue-400 underline bg-transparent border-none cursor-pointer p-0 text-[13px] font-inherit"
                  >
                    Google Cloud Console
                  </button>
                </li>
                <li>Create or select a project</li>
                <li>Enable the <strong>Gmail API</strong> under "APIs & Services"</li>
                <li>Go to "Credentials" and create an <strong>OAuth 2.0 Client ID</strong></li>
                <li>Set the application type to <strong>Desktop app</strong></li>
                <li>Add <code className="bg-gray-100 dark:bg-gray-700 px-1 py-0.5 rounded text-xs">http://127.0.0.1:8080</code> as an authorized redirect URI</li>
                <li>Copy the Client ID and Client Secret below</li>
              </ol>
            </details>

            <div className="mb-4">
              <label className="block mb-1.5 text-sm font-medium text-gray-700 dark:text-gray-300">
                Client ID
              </label>
              <input
                type="text"
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
                placeholder="xxxxx.apps.googleusercontent.com"
                disabled={loading}
                className="w-full py-2.5 px-3 border border-gray-300 dark:border-gray-600 rounded-md text-sm box-border bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 font-mono disabled:opacity-50"
              />
            </div>

            <div className="mb-5">
              <label className="block mb-1.5 text-sm font-medium text-gray-700 dark:text-gray-300">
                Client Secret
              </label>
              <input
                type="password"
                value={clientSecret}
                onChange={(e) => setClientSecret(e.target.value)}
                placeholder="GOCSPX-xxxxxxxxxxxxx"
                disabled={loading}
                className="w-full py-2.5 px-3 border border-gray-300 dark:border-gray-600 rounded-md text-sm box-border bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 font-mono disabled:opacity-50"
              />
            </div>
          </>
        )}

        {displayError && (
          <div className="bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 p-3 rounded-md mb-4 text-sm text-left">
            {displayError}
          </div>
        )}

        <button
          onClick={handleSubmit}
          disabled={loading}
          className={`flex items-center justify-center gap-3 w-full py-3 px-6 border border-gray-300 dark:border-gray-600 rounded-md text-base font-medium transition-all shadow-sm ${
            loading
              ? 'bg-gray-100 dark:bg-gray-700 text-gray-400 dark:text-gray-500 cursor-not-allowed'
              : 'bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-200 cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700 hover:shadow-md'
          }`}
        >
          {loading ? (
            <span>Signing in...</span>
          ) : (
            <>
              <svg width="20" height="20" viewBox="0 0 24 24">
                <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"/>
                <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"/>
                <path fill="#FBBC05" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"/>
                <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"/>
              </svg>
              <span>Sign in with Google</span>
            </>
          )}
        </button>

        <p className="mt-4 text-xs text-center text-gray-400 dark:text-gray-500">
          Credentials are encrypted and stored locally on this device.
        </p>
      </div>
    </div>
  );
}
