// Trade table formatting utilities

import dayjs from 'dayjs';
import relativeTime from 'dayjs/plugin/relativeTime';
import { formatLocal } from '../../utils/time';

dayjs.extend(relativeTime);

/**
 * Return the server-provided timestamp string to preserve precision.
 */
export const fmtTime = (value?: string | number | null): string => {
  if (value == null || value === '') return '—';

  try {
    // Normalize to a Date in LOCAL timezone
    let d: Date | null = null;

    if (typeof value === 'number') {
      // Interpret as epoch milliseconds (fallback to seconds if small)
      const ms = value > 1e12 ? value : value > 1e9 ? value * 1000 : value;
      d = new Date(ms);
    } else if (typeof value === 'string') {
      const s = value.trim();
      // If string looks like a number
      if (/^\d{10,}$/.test(s)) {
        const n = Number(s);
        const ms = n > 1e12 ? n : n > 1e9 ? n * 1000 : n;
        d = new Date(ms);
      } else if (s.includes('T') || /Z$|[+-]\d{2}:?\d{2}$/.test(s)) {
        // ISO 8601 with timezone/offset -> Date can parse as UTC -> local
        const parsed = new Date(s);
        if (!Number.isNaN(parsed.getTime())) d = parsed;
      } else {
        // Parse common "YYYY-MM-DD HH:mm:ss[.ffffff]" as LOCAL time
        const m = s.match(
          /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,6}))?$/,
        );
        if (m) {
          const [, Y, M, D, h, mnt, sss, frac] = m;
          const ms = Math.floor(((frac || '').padEnd(3, '0')).slice(0, 3) as unknown as number);
          d = new Date(
            Number(Y),
            Number(M) - 1,
            Number(D),
            Number(h),
            Number(mnt),
            Number(sss),
            Number((frac || '').slice(0, 3).padEnd(3, '0')),
          );
          // Ensure ms is correctly applied (avoid NaN from the earlier Math.floor of string)
          if (!Number.isNaN(ms)) d.setMilliseconds(ms);
        } else {
          const fallback = new Date(s);
          if (!Number.isNaN(fallback.getTime())) d = fallback;
        }
      }
    }

    if (!d || Number.isNaN(d.getTime())) return '—';

    const pad = (n: number, w = 2) => String(n).padStart(w, '0');
    const yyyy = d.getFullYear();
    const MM = pad(d.getMonth() + 1);
    const dd = pad(d.getDate());
    const HH = pad(d.getHours());
    const mm = pad(d.getMinutes());
    const ss = pad(d.getSeconds());
    const SSS = pad(d.getMilliseconds(), 3);
    return `${yyyy}-${MM}-${dd} ${HH}:${mm}:${ss}.${SSS}`;
  } catch {
    return '—';
  }
};

/**
 * Format time tooltip: ISO + relative time
 */
export const fmtTimeTip = (iso: string): string => {
  if (!iso) return '';
  return `${formatLocal(iso)} · ${dayjs(iso).fromNow()}`;
};

/**
 * Format number with standard notation
 */
export const num = (n?: number | null, d = 4): string => {
  if (n == null || !Number.isFinite(n)) return '—';
  return new Intl.NumberFormat('en-US', {
    maximumFractionDigits: d,
  }).format(n);
};

/**
 * Shorten hash to 0x{8}…{6} format
 */
export const shortHash = (hash?: string | null): string => {
  if (!hash || typeof hash !== 'string') return '—';
  if (!hash.startsWith('0x')) return hash;
  if (hash.length < 16) return hash;
  return `${hash.slice(0, 8)}…${hash.slice(-6)}`;
};

/**
 * Truncate ID to 8…6 format
 */
export const shortId = (id?: string | null): string => {
  if (!id) return '—';
  if (id.length <= 14) return id;
  return `${id.slice(0, 8)}…${id.slice(-6)}`;
};

/**
 * Truncate text to max length with ellipsis
 */
export const truncate = (text?: string | null, maxLen = 20): string => {
  if (!text) return '—';
  if (text.length <= maxLen) return text;
  return `${text.slice(0, maxLen)}…`;
};
