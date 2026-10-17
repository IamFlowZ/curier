/**
 * Convert an HTML string to its visible text content without fetching any
 * sub-resources.
 *
 * Uses `DOMParser`, which builds an inert document: unlike assigning to
 * `innerHTML` on a detached element, WebKit does not request `<img src>`,
 * `background` or other remote resources for a parsed document, so tracking
 * pixels in an email never fire when the body is converted for quoting.
 */
export function htmlToPlainText(html: string): string {
  const parsedDocument = new DOMParser().parseFromString(html, 'text/html');
  return parsedDocument.body?.textContent ?? '';
}
