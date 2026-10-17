import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useTabStore } from '../../store/tabStore';
import { ImportStep } from '../auth/ImportStep';
import type { AccountType } from '../../types/tabs';

interface AddAccountModalProps {
  isOpen: boolean;
  onClose: () => void;
  onAccountAdded: () => void;
}

type ProviderType = 'gmail' | 'outlook' | 'imap';

interface ProviderCredentialStatus {
  exists: boolean;
  client_id: string | null;
}

const GOOGLE_CONSOLE_URL = 'https://console.cloud.google.com/apis/credentials';
const MICROSOFT_PORTAL_URL = 'https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade';

export function AddAccountModal({ isOpen, onClose, onAccountAdded }: AddAccountModalProps) {
  const [step, setStep] = useState<'select' | 'credentials' | 'oauth' | 'imap' | 'import'>('select');
  const [selectedProvider, setSelectedProvider] = useState<ProviderType | null>(null);

  // Credential input state
  const [clientId, setClientId] = useState('');
  const [clientSecret, setClientSecret] = useState('');
  const [hasExistingCredentials, setHasExistingCredentials] = useState(false);
  const [checkingCredentials, setCheckingCredentials] = useState(false);

  // Import state
  const [importEmail, setImportEmail] = useState<string | null>(null);

  // OAuth state
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  // IMAP form state
  const [imapHost, setImapHost] = useState('');
  const [imapPort, setImapPort] = useState('993');
  const [imapUsername, setImapUsername] = useState('');
  const [imapPassword, setImapPassword] = useState('');

  const { addAccountTab } = useTabStore();

  const handleProviderSelect = async (provider: ProviderType) => {
    setSelectedProvider(provider);
    if (provider === 'imap') {
      setStep('imap');
      return;
    }

    // Check if credentials file exists for this provider
    setCheckingCredentials(true);
    try {
      const status = await invoke<ProviderCredentialStatus>('check_provider_credentials', {
        provider,
      });

      if (status.exists) {
        // Credentials file found, skip to OAuth directly
        setHasExistingCredentials(true);
        setStep('oauth');
      } else {
        // No credentials file, show credential input form
        setHasExistingCredentials(false);
        setStep('credentials');
      }
    } catch {
      // If check fails, show credential input form
      setHasExistingCredentials(false);
      setStep('credentials');
    } finally {
      setCheckingCredentials(false);
    }
  };

  const handleCredentialsSubmit = () => {
    const clientSecretIsRequired = selectedProvider !== 'outlook';
    if (!clientId.trim()) {
      setError('Client ID is required.');
      return;
    }
    if (clientSecretIsRequired && !clientSecret.trim()) {
      setError('Client Secret is required for Google accounts.');
      return;
    }
    setError('');
    setStep('oauth');
  };

  const handleOAuthSubmit = async () => {
    setLoading(true);
    setError('');

    // Listen for OAuth URL
    const unlisten = await listen<string>('oauth-url', (event) => {
      openUrl(event.payload).catch(console.error);
    });

    try {
      const command = selectedProvider === 'outlook' ? 'start_microsoft_oauth' : 'start_google_oauth';

      // Pass credentials if the user entered them manually
      const args: Record<string, string> = {};
      if (!hasExistingCredentials && clientId.trim()) {
        args.clientId = clientId.trim();
        args.clientSecret = clientSecret.trim();
      }

      const result = await invoke<{ is_authenticated: boolean; email: string | null }>(command, args);

      if (result.is_authenticated && result.email) {
        const accountId = result.email.replace(/[^a-zA-Z0-9]/g, '-');
        const accountType: AccountType = selectedProvider === 'outlook' ? 'outlook' : 'gmail';
        addAccountTab(accountId, result.email, accountType);

        // Show import step before closing
        setImportEmail(result.email);
        setStep('import');
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
      unlisten();
    }
  };

  const handleImapSubmit = async () => {
    setError('IMAP support coming soon');
  };

  const handleClose = () => {
    setStep('select');
    setSelectedProvider(null);
    setError('');
    setClientId('');
    setClientSecret('');
    setHasExistingCredentials(false);
    setImportEmail(null);
    setImapHost('');
    setImapPort('993');
    setImapUsername('');
    setImapPassword('');
    onClose();
  };

  const providerDisplayName = selectedProvider === 'outlook' ? 'Microsoft' : 'Google';
  const consoleUrl = selectedProvider === 'outlook' ? MICROSOFT_PORTAL_URL : GOOGLE_CONSOLE_URL;
  const consoleDisplayName = selectedProvider === 'outlook' ? 'Azure Portal' : 'Google Cloud Console';

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 bg-black/50 flex items-center justify-center z-[1000]"
      onClick={handleClose}
    >
      <div
        className="bg-white dark:bg-gray-800 rounded-xl p-6 max-w-[450px] w-[90%] max-h-[80vh] overflow-auto shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex justify-between items-center mb-5">
          <h2 className="m-0 text-xl text-gray-900 dark:text-gray-100">
            {step === 'select' && 'Add Account'}
            {step === 'credentials' && `${providerDisplayName} OAuth Credentials`}
            {step === 'oauth' && `Connect ${selectedProvider === 'gmail' ? 'Gmail' : 'Outlook'}`}
            {step === 'imap' && 'Connect IMAP Account'}
            {step === 'import' && 'Import Emails'}
          </h2>
          <button
            onClick={handleClose}
            className="bg-transparent border-none cursor-pointer p-1 text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        {/* Provider Selection */}
        {step === 'select' && (
          <div className="flex flex-col gap-3">
            <button
              onClick={() => handleProviderSelect('gmail')}
              disabled={checkingCredentials}
              className="flex items-center gap-3 p-4 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 cursor-pointer text-left transition-colors hover:border-blue-600 dark:hover:border-blue-400"
            >
              <div className="w-10 h-10 rounded-lg bg-red-100 dark:bg-red-900/30 flex items-center justify-center text-red-600 dark:text-red-400">
                <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M24 5.457v13.909c0 .904-.732 1.636-1.636 1.636h-3.819V11.73L12 16.64l-6.545-4.91v9.273H1.636A1.636 1.636 0 0 1 0 19.366V5.457c0-2.023 2.309-3.178 3.927-1.964L5.455 4.64 12 9.548l6.545-4.91 1.528-1.145C21.69 2.28 24 3.434 24 5.457z"/>
                </svg>
              </div>
              <div>
                <div className="font-medium text-gray-900 dark:text-gray-100">Gmail</div>
                <div className="text-[13px] text-gray-500 dark:text-gray-400">Connect with Google OAuth</div>
              </div>
            </button>

            <button
              onClick={() => handleProviderSelect('outlook')}
              disabled={checkingCredentials}
              className="flex items-center gap-3 p-4 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 cursor-pointer text-left transition-colors hover:border-blue-600 dark:hover:border-blue-400"
            >
              <div className="w-10 h-10 rounded-lg bg-blue-100 dark:bg-blue-900/30 flex items-center justify-center text-blue-600 dark:text-blue-400">
                <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M7.88 12.04q0 .45-.11.87-.1.41-.33.74-.22.33-.58.52-.37.2-.87.2t-.85-.2q-.35-.21-.57-.55-.22-.33-.33-.75-.1-.42-.1-.86t.1-.87q.1-.43.34-.76.22-.34.59-.54.36-.2.87-.2t.86.2q.35.21.57.55.22.34.31.77.1.43.1.88zM24 12v9.38q0 .46-.33.8-.33.32-.8.32H7.13q-.46 0-.8-.33-.32-.33-.32-.8V18H1q-.41 0-.7-.3-.3-.29-.3-.7V7q0-.41.3-.7Q.58 6 1 6h6.5V2.55q0-.44.3-.75.3-.3.75-.3h12.9q.44 0 .75.3.3.3.3.75V12zm-6-8.25v3h3v-3zm0 4.5v3h3v-3zm0 4.5v1.83l3.05-1.83zm-5.25-9v3h3.75v-3zm0 4.5v3h3.75v-3zm0 4.5v2.03l2.41 1.5 1.34-.8v-2.73zM9 3.75V6h2l.13.01.12.04v-2.3zM5.98 15.98q.9 0 1.6-.3.7-.32 1.19-.86.48-.55.73-1.28.25-.74.25-1.61 0-.83-.25-1.55-.24-.71-.71-1.24t-1.15-.83q-.68-.3-1.55-.3-.92 0-1.64.3-.71.3-1.2.85-.5.54-.75 1.3-.25.74-.25 1.63 0 .85.26 1.56.26.72.74 1.23.48.52 1.17.81.69.3 1.56.3zM7.5 21h12.39L12 16.08V17q0 .41-.3.7-.29.3-.7.3H7.5zm15-.13v-7.24l-5.9 3.54Z"/>
                </svg>
              </div>
              <div>
                <div className="font-medium text-gray-900 dark:text-gray-100">Outlook / Microsoft 365</div>
                <div className="text-[13px] text-gray-500 dark:text-gray-400">Connect with Microsoft OAuth</div>
              </div>
            </button>

            <button
              onClick={() => handleProviderSelect('imap')}
              disabled={checkingCredentials}
              className="flex items-center gap-3 p-4 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 cursor-pointer text-left transition-colors hover:border-blue-600 dark:hover:border-blue-400"
            >
              <div className="w-10 h-10 rounded-lg bg-gray-100 dark:bg-gray-600 flex items-center justify-center text-gray-700 dark:text-gray-300">
                <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <rect x="2" y="4" width="20" height="16" rx="2" />
                  <path d="M22 7l-10 7L2 7" />
                </svg>
              </div>
              <div>
                <div className="font-medium text-gray-900 dark:text-gray-100">Other (IMAP)</div>
                <div className="text-[13px] text-gray-500 dark:text-gray-400">Connect with IMAP credentials</div>
              </div>
            </button>
          </div>
        )}

        {/* OAuth Credentials Input */}
        {step === 'credentials' && (
          <div>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-0 mb-4">
              Curier requires your own {providerDisplayName} OAuth credentials to connect.
              You can create these in the{' '}
              <button
                onClick={() => openUrl(consoleUrl)}
                className="text-blue-600 dark:text-blue-400 underline bg-transparent border-none cursor-pointer p-0 text-sm font-inherit"
              >
                {consoleDisplayName}
              </button>.
            </p>

            {selectedProvider === 'gmail' && (
              <details className="mb-4 text-[13px] text-gray-500 dark:text-gray-400">
                <summary className="cursor-pointer text-blue-600 dark:text-blue-400 mb-2">
                  Setup instructions
                </summary>
                <ol className="pl-4 space-y-1 mt-2">
                  <li>Go to the Google Cloud Console and create or select a project</li>
                  <li>Enable the Gmail API under "APIs & Services"</li>
                  <li>Go to "Credentials" and create an OAuth 2.0 Client ID</li>
                  <li>Set the application type to "Desktop app"</li>
                  <li>Desktop app clients accept any loopback port, so no redirect URI needs to be added</li>
                  <li>Copy the Client ID and Client Secret below</li>
                </ol>
              </details>
            )}

            {selectedProvider === 'outlook' && (
              <details className="mb-4 text-[13px] text-gray-500 dark:text-gray-400">
                <summary className="cursor-pointer text-blue-600 dark:text-blue-400 mb-2">
                  Setup instructions
                </summary>
                <ol className="pl-4 space-y-1 mt-2">
                  <li>Go to the Azure Portal and register a new application</li>
                  <li>Set "Supported account types" to "Accounts in any organizational directory and personal Microsoft accounts"</li>
                  <li>Under "Authentication", add a "Mobile and desktop applications" platform with <code className="bg-gray-100 dark:bg-gray-700 px-1 py-0.5 rounded text-xs">http://127.0.0.1</code> as the redirect URI</li>
                  <li>Copy the Application (client) ID below; a client secret is not required for this platform type</li>
                </ol>
              </details>
            )}

            {error && (
              <div className="bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 p-3 rounded-md mb-4 text-sm">
                {error}
              </div>
            )}

            <div className="mb-4">
              <label className="block mb-1.5 text-sm font-medium text-gray-700 dark:text-gray-300">
                Client ID
              </label>
              <input
                type="text"
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
                placeholder="Enter your OAuth Client ID"
                className="w-full py-2.5 px-3 border border-gray-300 dark:border-gray-600 rounded-md text-sm box-border bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 font-mono"
              />
            </div>

            <div className="mb-5">
              <label className="block mb-1.5 text-sm font-medium text-gray-700 dark:text-gray-300">
                Client Secret{selectedProvider === 'outlook' ? ' (optional)' : ''}
              </label>
              <input
                type="password"
                value={clientSecret}
                onChange={(e) => setClientSecret(e.target.value)}
                placeholder="Enter your OAuth Client Secret"
                className="w-full py-2.5 px-3 border border-gray-300 dark:border-gray-600 rounded-md text-sm box-border bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 font-mono"
              />
            </div>

            <p className="text-xs text-gray-400 dark:text-gray-500 mt-0 mb-4">
              Credentials are encrypted and stored locally on this device. They are never sent to any third-party server.
            </p>

            <div className="flex gap-3">
              <button
                onClick={() => {
                  setStep('select');
                  setError('');
                }}
                className="flex-1 py-3 bg-white dark:bg-gray-700 text-gray-700 dark:text-gray-200 border border-gray-300 dark:border-gray-600 rounded-md text-sm font-medium cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-600 transition-colors"
              >
                Back
              </button>
              <button
                onClick={handleCredentialsSubmit}
                className="flex-1 py-3 bg-blue-600 text-white border-none rounded-md text-sm font-medium cursor-pointer hover:bg-blue-700 transition-colors"
              >
                Continue
              </button>
            </div>
          </div>
        )}

        {/* OAuth - Sign In */}
        {step === 'oauth' && (
          <div className="text-center">
            {error && (
              <div className="bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 p-3 rounded-md mb-4 text-sm text-left">
                {error}
              </div>
            )}

            <button
              onClick={handleOAuthSubmit}
              disabled={loading}
              className={`flex items-center justify-center gap-3 w-full py-3 px-6 border border-gray-300 dark:border-gray-600 rounded-md text-base font-medium mb-4 shadow-sm transition-colors ${
                loading
                  ? 'bg-gray-100 dark:bg-gray-700 text-gray-400 dark:text-gray-500 cursor-not-allowed'
                  : 'bg-white dark:bg-gray-700 text-gray-700 dark:text-gray-200 cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-600 hover:shadow-md'
              }`}
            >
              {loading ? (
                <span>Signing in...</span>
              ) : selectedProvider === 'outlook' ? (
                <>
                  <svg width="20" height="20" viewBox="0 0 23 23" fill="none">
                    <path fill="#f35325" d="M1 1h10v10H1z"/>
                    <path fill="#81bc06" d="M12 1h10v10H12z"/>
                    <path fill="#05a6f0" d="M1 12h10v10H1z"/>
                    <path fill="#ffba08" d="M12 12h10v10H12z"/>
                  </svg>
                  <span>Sign in with Microsoft</span>
                </>
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

            <button
              onClick={() => {
                if (hasExistingCredentials) {
                  setStep('select');
                } else {
                  setStep('credentials');
                }
                setError('');
              }}
              disabled={loading}
              className={`w-full py-2.5 bg-transparent text-gray-500 dark:text-gray-400 border-none rounded-md text-sm ${
                loading ? 'cursor-not-allowed' : 'cursor-pointer hover:text-gray-700 dark:hover:text-gray-200'
              }`}
            >
              Back
            </button>
          </div>
        )}

        {/* IMAP Form */}
        {step === 'imap' && (
          <div>
            {error && (
              <div className="bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 p-3 rounded-md mb-4 text-sm">
                {error}
              </div>
            )}

            <div className="mb-4">
              <label className="block mb-1.5 text-sm font-medium text-gray-700 dark:text-gray-300">
                IMAP Server
              </label>
              <input
                type="text"
                value={imapHost}
                onChange={(e) => setImapHost(e.target.value)}
                placeholder="imap.example.com"
                className="w-full py-2.5 px-3 border border-gray-300 dark:border-gray-600 rounded-md text-sm box-border bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100"
              />
            </div>

            <div className="mb-4">
              <label className="block mb-1.5 text-sm font-medium text-gray-700 dark:text-gray-300">
                Port
              </label>
              <input
                type="text"
                value={imapPort}
                onChange={(e) => setImapPort(e.target.value)}
                placeholder="993"
                className="w-full py-2.5 px-3 border border-gray-300 dark:border-gray-600 rounded-md text-sm box-border bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100"
              />
            </div>

            <div className="mb-4">
              <label className="block mb-1.5 text-sm font-medium text-gray-700 dark:text-gray-300">
                Username
              </label>
              <input
                type="text"
                value={imapUsername}
                onChange={(e) => setImapUsername(e.target.value)}
                placeholder="your.email@example.com"
                className="w-full py-2.5 px-3 border border-gray-300 dark:border-gray-600 rounded-md text-sm box-border bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100"
              />
            </div>

            <div className="mb-5">
              <label className="block mb-1.5 text-sm font-medium text-gray-700 dark:text-gray-300">
                Password
              </label>
              <input
                type="password"
                value={imapPassword}
                onChange={(e) => setImapPassword(e.target.value)}
                placeholder="Enter password"
                className="w-full py-2.5 px-3 border border-gray-300 dark:border-gray-600 rounded-md text-sm box-border bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100"
              />
            </div>

            <div className="flex gap-3">
              <button
                onClick={() => setStep('select')}
                className="flex-1 py-3 bg-white dark:bg-gray-700 text-gray-700 dark:text-gray-200 border border-gray-300 dark:border-gray-600 rounded-md text-sm font-medium cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-600 transition-colors"
              >
                Back
              </button>
              <button
                onClick={handleImapSubmit}
                className="flex-1 py-3 bg-blue-600 text-white border-none rounded-md text-sm font-medium cursor-pointer hover:bg-blue-700 transition-colors"
              >
                Connect
              </button>
            </div>
          </div>
        )}

        {/* Import Step */}
        {step === 'import' && importEmail && (
          <ImportStep
            accountEmail={importEmail}
            onComplete={() => {
              onAccountAdded();
              handleClose();
            }}
          />
        )}
      </div>
    </div>
  );
}
