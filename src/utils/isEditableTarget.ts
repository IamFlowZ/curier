/**
 * Returns true if the keyboard event target is an editable element
 * (input, textarea, select, or contentEditable). Used to suppress
 * letter-key shortcuts while the user is typing.
 */
export function isEditableTarget(event: KeyboardEvent): boolean {
  const el = event.target as HTMLElement;
  if (!el) return false;
  const tag = el.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
  if (el.isContentEditable) return true;
  return false;
}
