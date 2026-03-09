import { describe, it, expect } from 'vitest';
import { formatLocal } from '../../utils/time';

describe('formatLocal', () => {
  it('formats ISO with Z', () => {
    const iso = '2025-10-20T14:20:29Z';
    const d = new Date(iso);
    const expected = d.toLocaleString(undefined, {
      year: '2-digit',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
    expect(formatLocal(iso)).toBe(expected);
  });

  it('formats space-separated UTC string by Date parsing', () => {
    const s = '2025-10-20T14:20:29Z';
    const d = new Date(s);
    const expected = d.toLocaleString(undefined, {
      year: '2-digit',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
    expect(formatLocal(s)).toBe(expected);
  });

  it('formats epoch milliseconds', () => {
    const ms = Date.UTC(2025, 9, 20, 14, 20, 29);
    const d = new Date(ms);
    const expected = d.toLocaleString(undefined, {
      year: '2-digit',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
    expect(formatLocal(ms)).toBe(expected);
  });

  it('returns empty string for invalid input', () => {
    expect(formatLocal('not-a-date' as any)).toBe('');
  });
});


