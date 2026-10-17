/**
 * Pure helpers for preparing email bodies for display inside the sandboxed
 * iframe (see EmailHtmlRenderer). Kept free of React/Tauri imports so they
 * can be unit tested directly.
 */

/** Regex for detecting URLs in plain text */
export const URL_REGEX = /https?:\/\/[^\s<>"')\]]+/g;

/**
 * Escape HTML entities in a plain text string so it can be safely
 * injected into an iframe srcdoc.
 */
export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

const LINKIFIED_ANCHOR_STYLE = 'color:#3b82f6;text-decoration:underline';
const PLAIN_TEXT_PRE_STYLE =
  'white-space:pre-wrap;word-wrap:break-word;font-family:inherit;margin:0;line-height:1.6;font-size:15px';

/**
 * Convert plain text to HTML: detect URLs on the *raw* text (so URL
 * boundaries such as a trailing `>` are seen before they become entities),
 * then escape every segment and wrap detected URLs in `<a>` tags.
 * The result is wrapped in `<pre>` for whitespace preservation.
 */
export function linkifyPlainText(text: string): string {
  // Capturing group keeps the matched URLs in the split output at odd indexes.
  const segments = text.split(new RegExp(`(${URL_REGEX.source})`, 'g'));
  const linked = segments
    .map((segment, index) => {
      const isUrlSegment = index % 2 === 1;
      const escaped = escapeHtml(segment);
      return isUrlSegment
        ? `<a href="${escaped}" style="${LINKIFIED_ANCHOR_STYLE}">${escaped}</a>`
        : escaped;
    })
    .join('');
  return `<pre style="${PLAIN_TEXT_PRE_STYLE}">${linked}</pre>`;
}

/** Transparent 1×1 GIF data URI used as a placeholder for blocked remote images */
export const TRANSPARENT_GIF = 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7';

/**
 * Check whether a given hostname matches any of the approved domains.
 * Approving "example.com" also allows "images.example.com" (subdomain match).
 */
export function isDomainApproved(hostname: string, approvedDomains: string[]): boolean {
  const lower = hostname.toLowerCase();
  return approvedDomains.some((domain) => {
    const d = domain.toLowerCase();
    return lower === d || lower.endsWith(`.${d}`);
  });
}

/**
 * Extract the hostname from a URL string, or return null on failure.
 */
export function extractHostname(url: string): string | null {
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
 *
 * NOTE: this is a best-effort rewrite used to *detect* remote image domains
 * for the "images blocked" banner. Enforcement is done by the iframe's
 * Content-Security-Policy (see `buildIframeContentSecurityPolicy`), which
 * blocks every remote fetch this regex does not recognise.
 */
export function blockRemoteImages(
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

// ---------------------------------------------------------------------------
// Iframe document + Content-Security-Policy
// ---------------------------------------------------------------------------

/** Message types posted from the sandboxed iframe to the parent window. */
export const IFRAME_MESSAGE_TYPE_OPEN_URL = 'open-url';
export const IFRAME_MESSAGE_TYPE_HEIGHT = 'iframe-height';

/**
 * Schemes the parent window is willing to hand to the system opener.
 * Re-validated in the parent even though the iframe script also filters.
 */
export const OPENABLE_URL_PATTERN = /^(https?:|mailto:)/i;

/**
 * The bridge script injected into every email iframe. It is a fixed string
 * (no interpolation) so its SHA-256 hash can be allow-listed in the main
 * window CSP (`src-tauri/tauri.conf.json`), which `srcdoc` iframes inherit.
 * If you change this string you MUST update the hash in tauri.conf.json;
 * `emailHtml.test.ts` asserts they match.
 */
export const IFRAME_BRIDGE_SCRIPT = `
  // Intercept link clicks and send to parent via postMessage
  document.addEventListener('click', function(e) {
    var el = e.target;
    while (el && el.tagName !== 'A') el = el.parentElement;
    if (el && el.href) {
      e.preventDefault();
      e.stopPropagation();
      var href = el.getAttribute('href');
      if (href && (href.indexOf('http://') === 0 || href.indexOf('https://') === 0 || href.indexOf('mailto:') === 0)) {
        parent.postMessage({ type: '${IFRAME_MESSAGE_TYPE_OPEN_URL}', href: href }, '*');
      }
    }
  });

  // Report content height to parent for auto-sizing
  function reportHeight() {
    var h = document.body ? document.body.scrollHeight : 0;
    parent.postMessage({ type: '${IFRAME_MESSAGE_TYPE_HEIGHT}', height: h }, '*');
  }
  setTimeout(reportHeight, 50);
  if (typeof ResizeObserver !== 'undefined' && document.body) {
    new ResizeObserver(reportHeight).observe(document.body);
  }
`;

/**
 * SHA-256 of `IFRAME_BRIDGE_SCRIPT`, in CSP source-expression form.
 * Allow-listed in both the iframe's own `<meta>` CSP and the main window CSP.
 */
export const IFRAME_BRIDGE_SCRIPT_CSP_HASH =
  "'sha256-FaQ6aZagnvUW6A2sXi6L0RBd7H2lvPK6kVuGdYRTk58='";

/** Hostnames that are safe to interpolate into a CSP source list. */
const CSP_SAFE_HOSTNAME_PATTERN = /^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)*$/i;

/**
 * Build CSP `img-src` source expressions for a list of approved domains.
 * Each domain yields an exact host entry and a wildcard-subdomain entry so
 * that approving `example.com` also loads `images.example.com`, mirroring
 * `isDomainApproved`. Domains with characters that could break the policy
 * string are skipped rather than interpolated.
 */
export function buildApprovedDomainImageSources(approvedDomains: string[]): string[] {
  const sources: string[] = [];
  for (const domain of approvedDomains) {
    const normalized = domain.trim().toLowerCase();
    if (!CSP_SAFE_HOSTNAME_PATTERN.test(normalized)) continue;
    sources.push(`https://${normalized}`, `https://*.${normalized}`);
  }
  return sources;
}

export interface IframeContentSecurityPolicyOptions {
  /** True once the user has clicked "Load images": allow any https image. */
  allowAllRemoteImages: boolean;
  /** Domains the user has permanently approved (loaded even when blocked). */
  approvedDomains?: string[];
}

/**
 * Build the Content-Security-Policy enforced inside the email iframe.
 *
 * `default-src 'none'` blocks every network fetch (CSS `@import`, fonts,
 * `<td background>`, obfuscated `url()`s, ...) that the regex blocker in
 * `blockRemoteImages` cannot see. Images are allowed from `data:` plus the
 * user's approved domains; after "Load images" any `https:` image is allowed.
 * Inline styles are required for email markup; the only script permitted is
 * the bridge script, identified by hash.
 */
export function buildIframeContentSecurityPolicy(options: IframeContentSecurityPolicyOptions): string {
  const imageSources = options.allowAllRemoteImages
    ? ['data:', 'https:']
    : ['data:', ...buildApprovedDomainImageSources(options.approvedDomains ?? [])];

  return [
    "default-src 'none'",
    `img-src ${imageSources.join(' ')}`,
    "style-src 'unsafe-inline'",
    `script-src ${IFRAME_BRIDGE_SCRIPT_CSP_HASH}`,
    "font-src 'none'",
  ].join('; ');
}

export interface IframeDocumentOptions extends IframeContentSecurityPolicyOptions {
  /** Whether the app is currently in dark mode */
  isDarkMode: boolean;
}

/**
 * Build a full HTML document string to inject into the iframe via srcdoc.
 * Includes the per-iframe CSP `<meta>` and a base dark-mode stylesheet when
 * `isDarkMode` is true.
 */
export function buildIframeDocument(bodyHtml: string, options: IframeDocumentOptions): string {
  const contentSecurityPolicy = buildIframeContentSecurityPolicy(options);
  const darkModeStyles = options.isDarkMode
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
  <meta http-equiv="Content-Security-Policy" content="${contentSecurityPolicy}">
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
<script>${IFRAME_BRIDGE_SCRIPT}</script>
</body>
</html>`;
}
