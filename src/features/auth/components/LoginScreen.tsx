import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useAuthStore } from '../../../store/authStore';

export default function LoginScreen() {
  const [clientId, setClientId] = useState('');
  const [clientSecret, setClientSecret] = useState('');
  const [gmailAddress, setGmailAddress] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [authUrl, setAuthUrl] = useState('');
  const [waitingForCallback, setWaitingForCallback] = useState(false);
  const { setAuthenticated } = useAuthStore((state) => ({
    setAuthenticated: state.setAuthenticated
  }));

  useEffect(() => {
    // Listen for the OAuth URL event
    const unlisten = listen<string>('oauth-url', (event) => {
      const url = event.payload;
      setAuthUrl(url);
      setWaitingForCallback(true);

      // Auto-open the URL in browser
      window.open(url, '_blank');
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

  const handleLogin = async () => {
    if (!clientId.trim()) {
      setError('Please enter a Google OAuth Client ID');
      return;
    }
    if (!clientSecret.trim()) {
      setError('Please enter a Google OAuth Client Secret');
      return;
    }
    if (!gmailAddress.trim()) {
      setError('Please enter your Gmail address');
      return;
    }
    // Validate email format
    if (!gmailAddress.includes('@')) {
      setError('Please enter a valid email address');
      return;
    }

    setLoading(true);
    setError('');
    setAuthUrl('');

    try {
      console.log('LoginScreen: Starting OAuth flow with email:', gmailAddress);
      const result = await invoke('start_oauth_flow', { clientId, clientSecret, email: gmailAddress });
      console.log('LoginScreen: OAuth flow completed, result:', result);
      // If we get here, authentication succeeded
      setAuthenticated(true, gmailAddress);
      console.log('LoginScreen: Authentication state updated');
    } catch (err) {
      setError(err as string);
      console.error('OAuth error:', err);
      setWaitingForCallback(false);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-gradient-to-br from-blue-50 to-indigo-100">
      <div className="max-w-lg w-full bg-white rounded-lg shadow-xl p-8">
        <div className="text-center mb-8">
          <h1 className="text-3xl font-bold text-gray-900 mb-2">Curier</h1>
          <p className="text-gray-600">Minimal macOS Email Client</p>
        </div>

        {!waitingForCallback ? (
          <div className="space-y-6">
            <div>
              <label
                htmlFor="clientId"
                className="block text-sm font-medium text-gray-700 mb-2"
              >
                Google OAuth Client ID
              </label>
              <input
                id="clientId"
                type="text"
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
                placeholder="xxxxx.apps.googleusercontent.com"
                className="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-transparent outline-none"
                disabled={loading}
              />
            </div>

            <div>
              <label
                htmlFor="clientSecret"
                className="block text-sm font-medium text-gray-700 mb-2"
              >
                Google OAuth Client Secret
              </label>
              <input
                id="clientSecret"
                type="password"
                value={clientSecret}
                onChange={(e) => setClientSecret(e.target.value)}
                placeholder="GOCSPX-xxxxxxxxxxxxxxxxxxxxx"
                className="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-transparent outline-none"
                disabled={loading}
              />
              <p className="mt-2 text-xs text-gray-500">
                Create a <strong>Desktop app</strong> OAuth client in{' '}
                <a
                  href="https://console.cloud.google.com/apis/credentials"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-blue-600 hover:underline"
                >
                  Google Cloud Console
                </a>
              </p>
            </div>

            <div>
              <label
                htmlFor="gmailAddress"
                className="block text-sm font-medium text-gray-700 mb-2"
              >
                Your Gmail Address
              </label>
              <input
                id="gmailAddress"
                type="email"
                value={gmailAddress}
                onChange={(e) => setGmailAddress(e.target.value)}
                placeholder="your.email@gmail.com"
                className="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-transparent outline-none"
                disabled={loading}
              />
            </div>

            {error && (
              <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg text-sm">
                {error}
              </div>
            )}

            <button
              onClick={handleLogin}
              disabled={loading}
              className={`w-full py-3 px-4 rounded-lg font-medium text-white transition-colors ${
                loading
                  ? 'bg-gray-400 cursor-not-allowed'
                  : 'bg-blue-600 hover:bg-blue-700'
              }`}
            >
              {loading ? 'Generating...' : 'Connect Gmail'}
            </button>

            <div className="text-xs text-gray-500 text-center space-y-1">
              <p>This app uses OAuth 2.0 with PKCE for secure authentication.</p>
              <p>Your credentials are stored securely on this device.</p>
            </div>
          </div>
        ) : (
          <div className="space-y-6">
            <div className="bg-blue-50 border border-blue-200 p-6 rounded-lg text-center">
              <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-600 mx-auto mb-4"></div>
              <h3 className="text-lg font-medium text-blue-900 mb-2">
                Waiting for authorization...
              </h3>
              <p className="text-sm text-blue-700 mb-4">
                A browser window should have opened. Please authorize Curier to access your Gmail.
              </p>
              {authUrl && (
                <div className="mt-4">
                  <p className="text-xs text-blue-600 mb-2">
                    If the browser didn't open automatically, click below:
                  </p>
                  <a
                    href={authUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-sm text-blue-600 hover:underline break-all"
                  >
                    Open authorization page →
                  </a>
                </div>
              )}
            </div>

            <div className="bg-yellow-50 border border-yellow-200 p-4 rounded-lg">
              <p className="text-sm text-yellow-800">
                After authorizing in your browser, you'll be redirected to a success page.
                This window will automatically continue.
              </p>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
