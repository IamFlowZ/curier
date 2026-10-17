import { describe, expect, it, vi } from 'vitest';
import { htmlToPlainText } from './htmlToPlainText';

describe('htmlToPlainText', () => {
  it('returns the visible text of an HTML fragment', () => {
    expect(htmlToPlainText('<p>Hello <b>world</b></p>')).toBe('Hello world');
  });

  it('decodes entities', () => {
    expect(htmlToPlainText('Tom &amp; Jerry &lt;3')).toBe('Tom & Jerry <3');
  });

  it('drops image tags (no text) and never executes inline scripts', () => {
    expect(htmlToPlainText('<img src="https://track.example/p.gif">text')).toBe('text');

    const originalTitle = document.title;
    htmlToPlainText('<script>document.title = "pwned"</script>');
    expect(document.title).toBe(originalTitle);
  });

  it('returns an empty string for empty input', () => {
    expect(htmlToPlainText('')).toBe('');
  });

  it('parses with DOMParser rather than innerHTML', () => {
    const parseSpy = vi.spyOn(DOMParser.prototype, 'parseFromString');
    htmlToPlainText('<p>x</p>');
    expect(parseSpy).toHaveBeenCalledWith('<p>x</p>', 'text/html');
  });
});
