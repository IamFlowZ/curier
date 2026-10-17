import { describe, expect, it } from 'vitest';
import { formatQuotedReply } from './formatQuotedReply';

const FROM_NAME = 'Alice Example';
const TIMESTAMP_SECONDS = 1_700_000_000;

describe('formatQuotedReply', () => {
  it('prefixes every line of the original body with "> "', () => {
    const result = formatQuotedReply('first\nsecond\n\nfourth', FROM_NAME);
    expect(result).toBe(`\n\n${FROM_NAME} wrote:\n> first\n> second\n> \n> fourth`);
  });

  it('includes the localized date in the attribution header when a timestamp is provided', () => {
    const expectedDate = new Date(TIMESTAMP_SECONDS * 1000).toLocaleString();
    const result = formatQuotedReply('body', FROM_NAME, TIMESTAMP_SECONDS);
    expect(result).toBe(`\n\nOn ${expectedDate}, ${FROM_NAME} wrote:\n> body`);
  });

  it('omits the date clause for a zero timestamp (treated as absent)', () => {
    expect(formatQuotedReply('body', FROM_NAME, 0)).toBe(`\n\n${FROM_NAME} wrote:\n> body`);
  });

  it('quotes an empty body as a single empty quoted line', () => {
    expect(formatQuotedReply('', FROM_NAME)).toBe(`\n\n${FROM_NAME} wrote:\n> `);
  });
});
