import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  IFRAME_BRIDGE_SCRIPT,
  IFRAME_BRIDGE_SCRIPT_CSP_HASH,
  OPENABLE_URL_PATTERN,
  TRANSPARENT_GIF,
  blockRemoteImages,
  buildApprovedDomainImageSources,
  buildIframeContentSecurityPolicy,
  buildIframeDocument,
  escapeHtml,
  extractHostname,
  isDomainApproved,
  linkifyPlainText,
  DARK_MODE_EMAIL_CARD_BACKGROUND,
} from './emailHtml';

const TAURI_CONFIG_PATH = resolve(__dirname, '../../src-tauri/tauri.conf.json');
const CSP_META_PATTERN = /<meta http-equiv="Content-Security-Policy" content="([^"]*)">/;

interface TauriSecurityConfig {
  app: { security: { csp: Record<string, string[]>; devCsp: Record<string, string[]> } };
}

function readIframeCsp(document: string): string {
  const match = CSP_META_PATTERN.exec(document);
  if (!match) throw new Error('iframe document has no CSP meta tag');
  return match[1];
}

describe('emailHtml', () => {
  describe('escapeHtml', () => {
    it('escapes ampersand, angle brackets, and both quote styles', () => {
      expect(escapeHtml(`<a href="x" title='y'>Tom & Jerry</a>`)).toBe(
        '&lt;a href=&quot;x&quot; title=&#39;y&#39;&gt;Tom &amp; Jerry&lt;/a&gt;',
      );
    });

    it('escapes the ampersand first so existing entities are not double-decoded', () => {
      expect(escapeHtml('&lt;')).toBe('&amp;lt;');
    });

    it('leaves safe text unchanged', () => {
      expect(escapeHtml('plain text 123')).toBe('plain text 123');
    });
  });

  describe('linkifyPlainText', () => {
    it('wraps http and https URLs in anchor tags inside a <pre>', () => {
      const result = linkifyPlainText('see https://example.com/path?a=1 and http://foo.bar');
      expect(result).toMatch(/^<pre[^>]*>.*<\/pre>$/s);
      expect(result).toContain('<a href="https://example.com/path?a=1"');
      expect(result).toContain('<a href="http://foo.bar"');
    });

    it('does not include a trailing closing parenthesis in the URL', () => {
      const result = linkifyPlainText('(https://example.com) done');
      expect(result).toContain('<a href="https://example.com"');
      expect(result).not.toContain('https://example.com)');
    });

    it('stops a URL at a trailing angle bracket and keeps the bracket as an entity', () => {
      const result = linkifyPlainText('<https://x.y>');
      expect(result).toContain('&lt;<a href="https://x.y"');
      expect(result).toContain('</a>&gt;');
    });

    it('does not stop a URL at an ampersand (query strings stay intact, escaped)', () => {
      const result = linkifyPlainText('https://x.y/?a=1&b=2');
      expect(result).toContain('<a href="https://x.y/?a=1&amp;b=2"');
    });

    it('escapes text with no URLs at all', () => {
      expect(linkifyPlainText('a < b')).toContain('a &lt; b');
    });

    it('escapes HTML in the surrounding text so script tags cannot be injected', () => {
      const result = linkifyPlainText('<script>alert(1)</script> https://ok.com');
      expect(result).not.toContain('<script>');
      expect(result).toContain('&lt;script&gt;');
    });

    it('escapes quotes inside a URL so the href attribute cannot be broken out of', () => {
      const result = linkifyPlainText('https://evil.com/"onmouseover="alert(1)');
      expect(result).not.toContain('"onmouseover=');
      expect(result).toContain('&quot;onmouseover=');
    });
  });

  describe('isDomainApproved', () => {
    const APPROVED = ['Example.com', 'cdn.other.org'];

    it('matches the exact domain case-insensitively', () => {
      expect(isDomainApproved('example.com', APPROVED)).toBe(true);
      expect(isDomainApproved('EXAMPLE.COM', APPROVED)).toBe(true);
    });

    it('matches subdomains of an approved domain', () => {
      expect(isDomainApproved('images.example.com', APPROVED)).toBe(true);
      expect(isDomainApproved('a.b.cdn.other.org', APPROVED)).toBe(true);
    });

    it('does not match look-alike domains that merely end with the approved text', () => {
      expect(isDomainApproved('notexample.com', APPROVED)).toBe(false);
      expect(isDomainApproved('other.org', APPROVED)).toBe(false);
    });

    it('returns false when nothing is approved', () => {
      expect(isDomainApproved('example.com', [])).toBe(false);
    });
  });

  describe('extractHostname', () => {
    it('returns the hostname of a valid URL', () => {
      expect(extractHostname('https://Images.Example.com:8080/a.png?x=1')).toBe('images.example.com');
    });

    it('returns null for an invalid URL', () => {
      expect(extractHostname('not a url')).toBeNull();
      expect(extractHostname('')).toBeNull();
    });
  });

  describe('blockRemoteImages', () => {
    it('replaces remote <img> sources with a transparent gif and records the original src', () => {
      const { html, blockedDomains } = blockRemoteImages(
        '<p><img src="https://tracker.example.net/pixel.gif" alt="x"></p>',
      );

      expect(html).toBe(
        `<p><img src="${TRANSPARENT_GIF}" data-original-src="https://tracker.example.net/pixel.gif" alt="x"></p>`,
      );
      expect(blockedDomains).toEqual(['tracker.example.net']);
    });

    it('handles single-quoted src attributes and case-insensitive tags', () => {
      const { html } = blockRemoteImages(`<IMG SRC='http://a.com/i.png'>`);
      expect(html).toBe(`<IMG SRC='${TRANSPARENT_GIF}' data-original-src='http://a.com/i.png'>`);
    });

    it('leaves images from approved domains (including subdomains) untouched', () => {
      const input = '<img src="https://img.approved.com/a.png"><img src="https://other.com/b.png">';
      const { html, blockedDomains } = blockRemoteImages(input, ['approved.com']);

      expect(html).toContain('<img src="https://img.approved.com/a.png">');
      expect(html).not.toContain('https://other.com/b.png"><');
      expect(blockedDomains).toEqual(['other.com']);
    });

    it('leaves data: and relative image sources alone', () => {
      const input = '<img src="data:image/png;base64,AAAA"><img src="/local.png"><img src="cid:part1">';
      const { html, blockedDomains } = blockRemoteImages(input);
      expect(html).toBe(input);
      expect(blockedDomains).toEqual([]);
    });

    it('replaces remote background-image urls in inline styles', () => {
      const input = `<div style="background-image: url('https://bg.example.com/x.jpg')"></div>`;
      const { html, blockedDomains } = blockRemoteImages(input);

      expect(html).toBe(`<div style="background-image:url(${TRANSPARENT_GIF})"></div>`);
      expect(blockedDomains).toEqual(['bg.example.com']);
    });

    it('deduplicates blocked domains across multiple images', () => {
      const input =
        '<img src="https://same.com/1.png"><img src="https://same.com/2.png"><img src="https://diff.com/3.png">';
      const { blockedDomains } = blockRemoteImages(input);
      expect(blockedDomains).toEqual(['same.com', 'diff.com']);
    });

    it('returns the input unchanged when there are no remote images', () => {
      const input = '<p>Hello <b>world</b></p>';
      expect(blockRemoteImages(input)).toEqual({ html: input, blockedDomains: [] });
    });
  });
});

describe('iframe Content-Security-Policy', () => {
  describe('buildApprovedDomainImageSources', () => {
    it('emits an exact and a wildcard-subdomain https source per domain', () => {
      expect(buildApprovedDomainImageSources(['Example.com'])).toEqual([
        'https://example.com',
        'https://*.example.com',
      ]);
    });

    it('skips domains that could break out of the policy string', () => {
      expect(buildApprovedDomainImageSources(["evil.com; script-src 'unsafe-eval'", 'ok.org'])).toEqual([
        'https://ok.org',
        'https://*.ok.org',
      ]);
    });
  });

  describe('buildIframeContentSecurityPolicy', () => {
    it('blocks every network fetch except data: images while images are blocked', () => {
      const csp = buildIframeContentSecurityPolicy({ allowAllRemoteImages: false });
      expect(csp).toContain("default-src 'none'");
      expect(csp).toContain('img-src data:;');
      expect(csp).toContain("style-src 'unsafe-inline'");
      expect(csp).toContain("font-src 'none'");
      expect(csp).not.toContain('https:');
    });

    it('allows approved domains while images are blocked', () => {
      const csp = buildIframeContentSecurityPolicy({ allowAllRemoteImages: false, approvedDomains: ['cdn.example.com'] });
      expect(csp).toContain('img-src data: https://cdn.example.com https://*.cdn.example.com');
    });

    it('allows any https image once the user loads images', () => {
      const csp = buildIframeContentSecurityPolicy({ allowAllRemoteImages: true, approvedDomains: ['cdn.example.com'] });
      expect(csp).toContain('img-src data: https:;');
    });

    it('permits only the hashed bridge script, never unsafe-inline scripts', () => {
      const csp = buildIframeContentSecurityPolicy({ allowAllRemoteImages: false });
      expect(csp).toContain(`script-src ${IFRAME_BRIDGE_SCRIPT_CSP_HASH}`);
      expect(csp).not.toMatch(/script-src[^;]*'unsafe-inline'/);
    });
  });

  describe('buildIframeDocument', () => {
    it('embeds the CSP meta before any other resource and switches it with the image flag', () => {
      const blocked = buildIframeDocument('<p>hi</p>', { isDarkMode: false, allowAllRemoteImages: false });
      const loaded = buildIframeDocument('<p>hi</p>', { isDarkMode: false, allowAllRemoteImages: true });

      expect(blocked.indexOf('Content-Security-Policy')).toBeLessThan(blocked.indexOf('<style>'));
      expect(readIframeCsp(blocked)).toContain('img-src data:;');
      expect(readIframeCsp(loaded)).toContain('img-src data: https:;');
    });

    it('includes the body, the bridge script and dark-mode styles when requested', () => {
      const document = buildIframeDocument('<p>body</p>', { isDarkMode: true, allowAllRemoteImages: false });
      expect(document).toContain('<body><p>body</p>');
      expect(document).toContain(`<script>${IFRAME_BRIDGE_SCRIPT}</script>`);
      expect(document).toContain(`background: ${DARK_MODE_EMAIL_CARD_BACKGROUND}`);
      expect(document).toContain('body:not([style*="background"])');
      expect(buildIframeDocument('', { isDarkMode: false, allowAllRemoteImages: false })).not.toContain(DARK_MODE_EMAIL_CARD_BACKGROUND);
    });
  });

  describe('bridge script hash', () => {
    const expectedHash = `'sha256-${createHash('sha256').update(IFRAME_BRIDGE_SCRIPT, 'utf8').digest('base64')}'`;

    it('matches the constant used in the iframe CSP', () => {
      expect(IFRAME_BRIDGE_SCRIPT_CSP_HASH).toBe(expectedHash);
    });

    it('is allow-listed in the main window CSP (srcdoc iframes inherit it) without unsafe-inline', () => {
      const config = JSON.parse(readFileSync(TAURI_CONFIG_PATH, 'utf8')) as TauriSecurityConfig;
      const { csp, devCsp } = config.app.security;

      expect(csp['script-src']).toContain(expectedHash);
      expect(csp['script-src']).not.toContain("'unsafe-inline'");
      expect(devCsp['script-src']).toContain(expectedHash);
      // Email markup needs inline styles, data: placeholders and https: images after "Load images".
      expect(csp['style-src']).toContain("'unsafe-inline'");
      expect(csp['img-src']).toEqual(expect.arrayContaining(['data:', 'https:']));
      expect(csp['frame-src']).toContain('about:');
    });
  });

  describe('OPENABLE_URL_PATTERN', () => {
    it('accepts only http, https and mailto', () => {
      expect(OPENABLE_URL_PATTERN.test('https://a.b')).toBe(true);
      expect(OPENABLE_URL_PATTERN.test('HTTP://a.b')).toBe(true);
      expect(OPENABLE_URL_PATTERN.test('mailto:x@y.z')).toBe(true);
      expect(OPENABLE_URL_PATTERN.test('javascript:alert(1)')).toBe(false);
      expect(OPENABLE_URL_PATTERN.test('file:///etc/passwd')).toBe(false);
      expect(OPENABLE_URL_PATTERN.test(' https://a.b')).toBe(false);
    });
  });
});
