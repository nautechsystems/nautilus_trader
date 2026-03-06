export function formatLocal(dt: string | number | Date): string {
  let d: Date;
  try {
    if (typeof dt === 'string') {
      const s = dt.trim();
      // Treat canonical server timestamps ("YYYY-MM-DD HH:MM:SS") as UTC and render in local time
      const re = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/;
      if (re.test(s)) {
        const iso = s.replace(' ', 'T') + 'Z';
        d = new Date(iso);
      } else if (/Z$/.test(s) || /[+-]\d{2}:?\d{2}$/.test(s)) {
        // Already ISO with timezone
        d = new Date(s);
      } else {
        // Fallback: let browser parse; will assume local timezone
        d = new Date(s);
      }
    } else {
      d = new Date(dt);
    }
  } catch {
    return '';
  }

  if (Number.isNaN(d.getTime())) return '';
  return d.toLocaleString(undefined, {
    year: '2-digit',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

export function formatRelativeTime(dt: string | number | Date): string {
  const d = new Date(dt);
  if (Number.isNaN(d.getTime())) return '';

  const now = Date.now();
  const diff = now - d.getTime();

  // Handle future dates (clock skew, bad data)
  if (diff < 0) return 'just now';

  const seconds = Math.floor(diff / 1000);

  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

/**
 * Format timestamp as absolute local time (no tooltip)
 * Shows HH:mm:ss if same day, else MM/DD HH:mm:ss
 * Uses browser locale + local timezone
 */
export function formatAbsoluteTime(ts: number | null | undefined): string {
  if (!ts || ts <= 0) return '—';

  try {
    const d = new Date(ts);
    if (Number.isNaN(d.getTime())) return '—';

    const now = new Date();
    const sameDay = d.toDateString() === now.toDateString();

    if (sameDay) {
      // HH:mm:ss (24-hour format)
      return d.toLocaleTimeString([], {
        hour12: false,
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
      });
    } else {
      // MM/DD HH:mm:ss
      return d.toLocaleString([], {
        month: '2-digit',
        day: '2-digit',
        hour12: false,
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
      });
    }
  } catch {
    return '—';
  }
}

