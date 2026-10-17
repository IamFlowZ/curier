/**
 * Build the quoted block appended to a reply body:
 * a blank line, an attribution header, then every line of the original
 * prefixed with `> `.
 */
export function formatQuotedReply(quotedBody: string, fromName: string, date?: number): string {
  const dateStr = date
    ? new Date(date * 1000).toLocaleString()
    : '';
  const header = dateStr
    ? `On ${dateStr}, ${fromName} wrote:`
    : `${fromName} wrote:`;
  const quoted = quotedBody
    .split('\n')
    .map((line) => `> ${line}`)
    .join('\n');
  return `\n\n${header}\n${quoted}`;
}
