import { useEffect, useRef, useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import {
  IFRAME_MESSAGE_TYPE_HEIGHT,
  IFRAME_MESSAGE_TYPE_OPEN_URL,
  OPENABLE_URL_PATTERN,
  blockRemoteImages,
  buildIframeDocument,
  linkifyPlainText,
} from '../../utils/emailHtml';

/**
 * Sandbox flags for the email iframe. `allow-scripts` is needed for the
 * bridge script; `allow-same-origin` is deliberately absent so email content
 * runs in an opaque origin, and `allow-popups` is absent because every link
 * is routed through the parent via postMessage (no popups are ever needed).
 */
const EMAIL_IFRAME_SANDBOX = 'allow-scripts';
const INITIAL_IFRAME_HEIGHT_PX = 300;

interface EmailHtmlRendererProps {
  /** Sanitized HTML string, or undefined if the email is plain text */
  html?: string;
  /** Plain text body, used when html is not provided */
  plainText?: string;
  /** Whether the app is currently in dark mode */
  isDarkMode: boolean;
  /** Domains whose images are allowed to load without being blocked */
  approvedDomains?: string[];
  /** Callback to permanently approve a domain in user settings */
  onApproveImageDomain?: (domain: string) => void;
}

/**
 * Renders email HTML inside a sandboxed iframe with:
 * - Auto-resizing to match content height (no inner scrollbar)
 * - Link click interception → opens in system browser via Tauri opener
 * - Remote image blocking with an opt-in "Load images" banner
 * - Dark mode CSS injection
 * - Plain text URL linkification
 */
export function EmailHtmlRenderer({ html, plainText, isDarkMode, approvedDomains, onApproveImageDomain }: EmailHtmlRendererProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [iframeHeight, setIframeHeight] = useState(INITIAL_IFRAME_HEIGHT_PX);
  const [imagesLoaded, setImagesLoaded] = useState(false);

  // Determine the source HTML
  const rawHtml = html ?? (plainText ? linkifyPlainText(plainText) : '');

  // Rewrite remote images to detect their domains for the banner (unless the
  // user has clicked "Load images"). Enforcement is the iframe CSP below.
  const { html: processedHtml, blockedDomains } = imagesLoaded
    ? { html: rawHtml, blockedDomains: [] as string[] }
    : blockRemoteImages(rawHtml, approvedDomains);

  const srcdoc = buildIframeDocument(processedHtml, {
    isDarkMode,
    allowAllRemoteImages: imagesLoaded,
    approvedDomains,
  });

  /** Listen for postMessage events from this component's own iframe only */
  useEffect(() => {
    const handler = (event: MessageEvent) => {
      // Ignore messages from any window other than our iframe (other email
      // tabs, extensions, or anything else that can reach window.postMessage).
      if (!iframeRef.current || event.source !== iframeRef.current.contentWindow) return;

      const data: unknown = event.data;
      if (!data || typeof data !== 'object') return;
      const message = data as { type?: unknown; href?: unknown; height?: unknown };

      if (message.type === IFRAME_MESSAGE_TYPE_OPEN_URL && typeof message.href === 'string') {
        // Re-validate the scheme here; the iframe script is not a trust boundary.
        if (!OPENABLE_URL_PATTERN.test(message.href)) return;
        openUrl(message.href).catch((err) => console.error('Failed to open URL:', err));
      } else if (message.type === IFRAME_MESSAGE_TYPE_HEIGHT && typeof message.height === 'number') {
        setIframeHeight(message.height);
      }
    };

    window.addEventListener('message', handler);
    return () => window.removeEventListener('message', handler);
  }, []);

  const handleLoadImages = () => {
    setImagesLoaded(true);
  };

  const handleApproveDomain = (domain: string) => {
    onApproveImageDomain?.(domain);
  };

  return (
    <div className="email-html-renderer">
      {/* Remote images warning banner */}
      {blockedDomains.length > 0 && !imagesLoaded && (
        <div className="mb-3 rounded-md bg-amber-50 dark:bg-amber-900/30 border border-amber-200 dark:border-amber-700 text-amber-800 dark:text-amber-300 text-sm">
          <div className="flex items-center gap-2 px-4 py-2">
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              className="flex-shrink-0"
            >
              <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
              <line x1="12" y1="9" x2="12" y2="13" />
              <line x1="12" y1="17" x2="12.01" y2="17" />
            </svg>
            <span>
              Images blocked from {blockedDomains.length === 1
                ? <span className="font-medium font-mono">{blockedDomains[0]}</span>
                : <>{blockedDomains.length} domains</>}
            </span>
            <button
              onClick={handleLoadImages}
              className="ml-auto px-3 py-1 rounded text-xs font-medium bg-amber-200 dark:bg-amber-800 hover:bg-amber-300 dark:hover:bg-amber-700 text-amber-900 dark:text-amber-100 transition-colors cursor-pointer flex-shrink-0"
            >
              Load all images
            </button>
          </div>
          {/* Per-domain list with "Always allow" action */}
          <div className="px-4 pb-2 flex flex-wrap gap-1.5">
            {blockedDomains.map((domain) => (
              <span
                key={domain}
                className="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-amber-100 dark:bg-amber-800/50 text-xs font-mono"
              >
                {domain}
                {onApproveImageDomain && (
                  <button
                    onClick={() => handleApproveDomain(domain)}
                    className="bg-transparent border-none cursor-pointer text-amber-600 dark:text-amber-300 hover:text-amber-800 dark:hover:text-amber-100 p-0 text-xs underline"
                    title={`Always allow images from ${domain}`}
                  >
                    Always allow
                  </button>
                )}
              </span>
            ))}
          </div>
        </div>
      )}

      <iframe
        ref={iframeRef}
        srcDoc={srcdoc}
        sandbox={EMAIL_IFRAME_SANDBOX}
        title="Email content"
        style={{
          width: '100%',
          height: `${iframeHeight}px`,
          border: 'none',
          display: 'block',
          background: 'transparent',
        }}
      />
    </div>
  );
}
