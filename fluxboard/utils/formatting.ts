/**
 * Formatting utilities for timestamps and mark prices
 * Zero external dependencies - uses native Date APIs
 */

/**
 * Format ISO timestamp to relative time string
 * @param timestamp - ISO 8601 timestamp string
 * @returns Human-readable relative time (e.g., "2m ago", "3h ago", "5d ago")
 */
export function formatRelativeTime(timestamp: string): string {
  if (!timestamp) return '';

  const now = Date.now();
  const time = new Date(timestamp).getTime();

  // Handle invalid timestamps
  if (isNaN(time)) return timestamp;

  const diffMs = now - time;

  // Future timestamp
  if (diffMs < 0) return 'now';

  const diffSec = Math.floor(diffMs / 1000);
  const diffMin = Math.floor(diffSec / 60);
  const diffHour = Math.floor(diffMin / 60);
  const diffDay = Math.floor(diffHour / 24);

  // Less than 60 seconds
  if (diffSec < 60) return 'now';

  // Less than 60 minutes
  if (diffMin < 60) return `${diffMin}m ago`;

  // Less than 24 hours
  if (diffHour < 24) return `${diffHour}h ago`;

  // Less than 7 days
  if (diffDay < 7) return `${diffDay}d ago`;

  // More than 7 days - show date only (YYYY-MM-DD)
  return timestamp.split('T')[0] || timestamp;
}

/**
 * Get Tailwind color class for mark price based on deviation from 1.00
 * Uses subtle text color only for visual refinement
 * @param mark - Mark price (typically around 1.00)
 * @returns Tailwind color class string
 */
export function getMarkColor(mark: number): string {
  // Handle invalid marks
  if (typeof mark !== 'number' || isNaN(mark) || mark === 0) {
    return 'text-neutral-400';
  }

  const deviation = Math.abs(mark - 1.0);
  const rawBps = deviation * 10000; // basis points
  const bps = Math.round(rawBps * 1000) / 1000; // stabilise floating point noise (0.2m bps precision)

  // Near 1.00 (within 5 bps) - neutral gray
  if (bps < 5) return 'text-neutral-400';

  if (mark > 1.0) {
    // Premium - green gradient
    if (bps < 20) return 'text-emerald-400/70';
    if (bps < 50) return 'text-emerald-400';
    return 'text-emerald-300';
  } else {
    // Discount - red gradient
    if (bps < 20) return 'text-red-400/70';
    if (bps < 50) return 'text-red-400';
    return 'text-red-300';
  }
}

/**
 * Convert server timestamp to local time string
 * @param ts - Server timestamp in "YYYY-MM-DD HH:mm:ss" or ISO format (assumes UTC)
 * @returns Localized time string (e.g., "10/20/2025, 02:15:30 PM")
 */
export function toLocal(ts: string): string {
  if (!ts) return '';

  // Server timestamps are in UTC. Explicitly mark as UTC if no timezone indicator present.
  let isoish: string;
  if (ts.includes('T') || ts.endsWith('Z')) {
    isoish = ts;
  } else {
    // "YYYY-MM-DD HH:mm:ss" → "YYYY-MM-DD HH:mm:ssZ" to mark as UTC
    isoish = ts.replace(' ', 'T') + 'Z';
  }

  const d = new Date(isoish);

  return Number.isNaN(d.getTime())
    ? ts  // fallback: show server string as-is
    : d.toLocaleString(undefined, {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
      });
}
