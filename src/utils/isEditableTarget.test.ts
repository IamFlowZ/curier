import { describe, expect, it } from 'vitest';
import { isEditableTarget } from './isEditableTarget';

function keydownOn(element: EventTarget | null): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key: 'j' });
  if (element) {
    Object.defineProperty(event, 'target', { value: element });
  }
  return event;
}

describe('isEditableTarget', () => {
  it.each(['input', 'textarea', 'select'])('returns true when the target is a <%s>', (tagName) => {
    const element = document.createElement(tagName);
    expect(isEditableTarget(keydownOn(element))).toBe(true);
  });

  it('returns true for contentEditable elements', () => {
    const element = document.createElement('div');
    Object.defineProperty(element, 'isContentEditable', { value: true });
    expect(isEditableTarget(keydownOn(element))).toBe(true);
  });

  it('returns false for non-editable elements such as div and button', () => {
    expect(isEditableTarget(keydownOn(document.createElement('div')))).toBe(false);
    expect(isEditableTarget(keydownOn(document.createElement('button')))).toBe(false);
  });

  it('returns false when the event has no target', () => {
    expect(isEditableTarget(keydownOn(null))).toBe(false);
  });

  it('returns false when the target is the document body', () => {
    expect(isEditableTarget(keydownOn(document.body))).toBe(false);
  });
});
