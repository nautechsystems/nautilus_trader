import { describe, expect, it } from 'vitest';
import { fmtTime, shortHash } from './formatters';

describe('fmtTime', () => {
  it('returns placeholder for empty or missing timestamps', () => {
    expect(fmtTime('')).toBe('—');
    expect(fmtTime(null)).toBe('—');
    expect(fmtTime(undefined)).toBe('—');
  });

  it('formats ISO Z timestamps to local time with 3 decimals and no T/Z', () => {
    const iso = '2025-11-10T10:36:50.644000Z';
    const out = fmtTime(iso);
    expect(out).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}$/);
    expect(out.includes('T')).toBe(false);
    expect(out.endsWith('.644')).toBe(true);
  });

  it('formats space-separated timestamps to local time with 3 decimals', () => {
    const s = '2025-11-10 10:36:50.123456';
    const out = fmtTime(s);
    expect(out).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}$/);
    // microseconds trimmed to milliseconds
    expect(out.endsWith('.123')).toBe(true);
  });

  it('handles numeric epoch milliseconds and seconds', () => {
    const ms = 1731231410644; // some fixed ms
    const outMs = fmtTime(ms);
    expect(outMs).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}$/);

    const sec = Math.floor(ms / 1000);
    const outSec = fmtTime(sec);
    expect(outSec).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}$/);
  });
});

describe('shortHash', () => {
  it('returns placeholder for missing or non-string values', () => {
    expect(shortHash(undefined)).toBe('—');
    expect(shortHash(null)).toBe('—');
    expect(shortHash({} as any)).toBe('—');
  });

  it('shortens long hex hashes starting with 0x', () => {
    const hash = '0x1234567890abcdef1234567890abcdef1234567890';
    expect(shortHash(hash)).toBe('0x123456…567890');
  });

  it('returns the raw string when the hash does not use 0x prefix', () => {
    expect(shortHash('abc-123')).toBe('abc-123');
  });
});
