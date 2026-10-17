import { useEffect, useRef, useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';

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

/** Regex for detecting URLs in plain text */
const URL_REGEX = /https?:\/\/[^\s<>"')\]]+/g;

/**
 * Escape HTML entities in a plain text string so it can be safely
 * injected into an iframe srcdoc.
 */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/**
 * Convert plain text to HTML: escape entities, then wrap detected URLs in
 * `<a>` tags. The result is wrapped in `<pre>` for whitespace preservation.
 */
function linkifyPlainText(text: string): string {
  const escaped = escapeHtml(text);
  const linked = escaped.replace(
    URL_REGEX,
    (url) => `<a href="${url}" style="color:#3b82f6;text-decoration:underline">${url}</a>`,
  );
  return `<pre style="white-space:pre-wrap;word-wrap:break-word;font-family:inherit;margin:0;line-height:1.6;font-size:15px">${linked}</pre>`;
}

/** Transparent 1×1 GIF data URI used as a placeholder for blocked remote images */
const TRANSPARENT_GIF = 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7';

/**
 * Check whether a given hostname matches any of the approved domains.
 * Approving "example.com" also allows "images.example.com" (subdomain match).
 */
function isDomainApproved(hostname: string, approvedDomains: string[]): boolean {
  const lower = hostname.toLowerCase();
  return approvedDomains.some((domain) => {
    const d = domain.toLowerCase();
    return lower === d || lower.endsWith(`.${d}`);
  });
}

/**
 * Extract the hostname from a URL string, or return null on failure.
 */
function extractHostname(url: string): string | null {
  try {
    return new URL(url).hostname;
  } catch {
    return null;
  }
}

/**
 * Replace remote (http/https) image sources with a transparent placeholder,
 * except for images from approved domains.
 * Returns the processed HTML and a deduplicated list of blocked domain names.
 */
function blockRemoteImages(
  html: string,
  approvedDomains: string[] = [],
): { html: string; blockedDomains: string[] } {
  const blockedDomainsSet = new Set<string>();

  // Replace src attributes on <img> tags that point to http(s)
  const processed = html.replace(
    /(<img\b[^>]*?\bsrc\s*=\s*)(["'])(https?:\/\/[^"']*)\2/gi,
    (match, prefix: string, quote: string, url: string) => {
      const hostname = extractHostname(url);
      if (hostname && isDomainApproved(hostname, approvedDomains)) {
        return match; // Leave approved images intact
      }
      if (hostname) blockedDomainsSet.add(hostname);
      return `${prefix}${quote}${TRANSPARENT_GIF}${quote} data-original-src=${quote}${url}${quote}`;
    },
  );

  // Also catch CSS background-image urls in inline styles
  const processedWithBg = processed.replace(
    /background-image\s*:\s*url\(\s*(["']?)(https?:\/\/[^)"']*)\1\s*\)/gi,
    (match, _quote: string, url: string) => {
      const hostname = extractHostname(url);
      if (hostname && isDomainApproved(hostname, approvedDomains)) {
        return match;
      }
      if (hostname) blockedDomainsSet.add(hostname);
      return `background-image:url(${TRANSPARENT_GIF})`;
    },
  );

  return { html: processedWithBg, blockedDomains: Array.from(blockedDomainsSet) };
}


/**
 * Build a full HTML document string to inject into the iframe via srcdoc.
 * Includes a base dark-mode stylesheet when `isDarkMode` is true.
 */
function buildIframeDocument(bodyHtml: string, isDarkMode: boolean): string {
  const darkModeStyles = isDarkMode
    ? `
      <style>
        body:not([style*="background"]) {
          color-scheme: dark;
          color: #e5e7eb;
        }
        a { color: #60a5fa; }
      </style>`
    : '';

  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <style>
    body {
      margin: 0;
      padding: 0;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
      font-size: 15px;
      line-height: 1.6;
      overflow-wrap: break-word;
      word-wrap: break-word;
      background: transparent;
    }
    img { max-width: 100%; height: auto; }
    table { max-width: 100%; }
  </style>
  ${darkModeStyles}
</head>
<body>${bodyHtml}
<script>
  // Intercept link clicks and send to parent via postMessage
  document.addEventListener('click', function(e) {
    var el = e.target;
    while (el && el.tagName !== 'A') el = el.parentElement;
    if (el && el.href) {
      e.preventDefault();
      e.stopPropagation();
      var href = el.getAttribute('href');
      if (href && (href.indexOf('http://') === 0 || href.indexOf('https://') === 0 || href.indexOf('mailto:') === 0)) {
        parent.postMessage({ type: 'open-url', href: href }, '*');
      }
    }
  });

  // Report content height to parent for auto-sizing
  function reportHeight() {
    var h = document.body ? document.body.scrollHeight : 0;
    parent.postMessage({ type: 'iframe-height', height: h }, '*');
  }
  setTimeout(reportHeight, 50);
  if (typeof ResizeObserver !== 'undefined' && document.body) {
    new ResizeObserver(reportHeight).observe(document.body);
  }
</script>
</body>
</html>`;
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
  const [iframeHeight, setIframeHeight] = useState(300);
  const [imagesLoaded, setImagesLoaded] = useState(false);

  // Determine the source HTML
  const rawHtml = html ?? (plainText ? linkifyPlainText(plainText) : '');

  // Block remote images (unless user has clicked "Load images")
  const { html: processedHtml, blockedDomains } = imagesLoaded
    ? { html: rawHtml, blockedDomains: [] as string[] }
    : blockRemoteImages(rawHtml, approvedDomains);

  const srcdoc = buildIframeDocument(processedHtml, isDarkMode);

  /** Listen for postMessage events from the sandboxed iframe */
  useEffect(() => {
    const handler = (event: MessageEvent) => {
      const data = event.data;
      if (!data || typeof data !== 'object') return;

      if (data.type === 'open-url' && typeof data.href === 'string') {
        openUrl(data.href).catch((err) => console.error('Failed to open URL:', err));
      } else if (data.type === 'iframe-height' && typeof data.height === 'number') {
        setIframeHeight(data.height);
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
        sandbox="allow-scripts allow-popups"
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
