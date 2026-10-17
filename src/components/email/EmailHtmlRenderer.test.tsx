import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { openUrl } from '@tauri-apps/plugin-opener';
import { EmailHtmlRenderer } from './EmailHtmlRenderer';
import { IFRAME_MESSAGE_TYPE_HEIGHT, IFRAME_MESSAGE_TYPE_OPEN_URL } from '../../utils/emailHtml';

const TRUSTED_URL = 'https://example.com/page';

describe('EmailHtmlRenderer message bridge', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(async () => {
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
    await act(async () => {
      root.render(<EmailHtmlRenderer html="<p>hi</p>" isDarkMode={false} />);
    });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
  });

  function renderedIframe(): HTMLIFrameElement {
    const iframe = container.querySelector('iframe');
    if (!iframe) throw new Error('iframe not rendered');
    return iframe;
  }

  async function postToParent(data: unknown, source: Window | null): Promise<void> {
    await act(async () => {
      window.dispatchEvent(new MessageEvent('message', { data, source }));
    });
  }

  it('sandboxes the iframe with scripts only (no popups, no same-origin)', () => {
    expect(renderedIframe().getAttribute('sandbox')).toBe('allow-scripts');
  });

  it('embeds the per-iframe CSP in the srcdoc', () => {
    expect(renderedIframe().getAttribute('srcdoc')).toContain('http-equiv="Content-Security-Policy"');
  });

  it('opens a URL only when the message comes from its own iframe', async () => {
    const ownWindow = renderedIframe().contentWindow;
    expect(ownWindow).not.toBeNull();

    await postToParent({ type: IFRAME_MESSAGE_TYPE_OPEN_URL, href: TRUSTED_URL }, null);
    await postToParent({ type: IFRAME_MESSAGE_TYPE_OPEN_URL, href: TRUSTED_URL }, window);
    expect(openUrl).not.toHaveBeenCalled();

    await postToParent({ type: IFRAME_MESSAGE_TYPE_OPEN_URL, href: TRUSTED_URL }, ownWindow);
    expect(openUrl).toHaveBeenCalledTimes(1);
    expect(openUrl).toHaveBeenCalledWith(TRUSTED_URL);
  });

  it('re-validates the scheme even for messages from its own iframe', async () => {
    const ownWindow = renderedIframe().contentWindow;

    await postToParent({ type: IFRAME_MESSAGE_TYPE_OPEN_URL, href: 'javascript:alert(1)' }, ownWindow);
    await postToParent({ type: IFRAME_MESSAGE_TYPE_OPEN_URL, href: 'file:///etc/passwd' }, ownWindow);

    expect(openUrl).not.toHaveBeenCalled();
  });

  it('applies height reports only from its own iframe', async () => {
    const ownWindow = renderedIframe().contentWindow;

    await postToParent({ type: IFRAME_MESSAGE_TYPE_HEIGHT, height: 999 }, window);
    expect(renderedIframe().style.height).toBe('300px');

    await postToParent({ type: IFRAME_MESSAGE_TYPE_HEIGHT, height: 999 }, ownWindow);
    expect(renderedIframe().style.height).toBe('999px');
  });

  it('ignores malformed messages', async () => {
    const ownWindow = renderedIframe().contentWindow;
    vi.mocked(openUrl).mockClear();

    await postToParent('open-url', ownWindow);
    await postToParent({ type: IFRAME_MESSAGE_TYPE_OPEN_URL, href: 42 }, ownWindow);

    expect(openUrl).not.toHaveBeenCalled();
  });
});
