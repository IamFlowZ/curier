/**
 * Smart date formatting for email timestamps.
 *
 * - Today: time only (e.g., "2:30 PM")
 * - Yesterday: "Yesterday"
 * - 2–5 days ago: day name (e.g., "Monday")
 * - Older (same year): short date (e.g., "Feb 12")
 * - Different year: short date with year (e.g., "Feb 12, 2025")
 */
export function formatEmailDate(timestampSeconds: number): string {
  const date = new Date(timestampSeconds * 1000);
  const now = new Date();

  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const startOfDateDay = new Date(date.getFullYear(), date.getMonth(), date.getDate());

  const diffMs = startOfToday.getTime() - startOfDateDay.getTime();
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

  if (diffDays === 0) {
    // Today: show time only
    return date.toLocaleTimeString(undefined, {
      hour: 'numeric',
      minute: '2-digit',
      hour12: true,
    });
  }

  if (diffDays === 1) {
    return 'Yesterday';
  }

  if (diffDays >= 2 && diffDays <= 5) {
    // Show day name
    return date.toLocaleDateString(undefined, { weekday: 'long' });
  }

  if (date.getFullYear() === now.getFullYear()) {
    // Same year: "Feb 12"
    return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  }

  // Different year: "Feb 12, 2025"
  return date.toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  });
}
