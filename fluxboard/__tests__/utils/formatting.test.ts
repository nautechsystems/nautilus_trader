/**
 * Unit tests for formatting utilities
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { formatRelativeTime, getMarkColor, toLocal } from '../../utils/formatting';

describe('formatting utilities', () => {
  describe('formatRelativeTime', () => {
    beforeEach(() => {
      // Mock Date.now() for consistent testing
      vi.useFakeTimers();
      vi.setSystemTime(new Date('2025-10-20T14:30:00Z'));
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('should return empty string for empty input', () => {
      expect(formatRelativeTime('')).toBe('');
    });

    it('should return "now" for timestamps less than 60 seconds ago', () => {
      const timestamp = new Date('2025-10-20T14:29:45Z').toISOString();
      expect(formatRelativeTime(timestamp)).toBe('now');
    });

    it('should return "now" for current time', () => {
      const timestamp = new Date('2025-10-20T14:30:00Z').toISOString();
      expect(formatRelativeTime(timestamp)).toBe('now');
    });

    it('should return "now" for future timestamps', () => {
      const timestamp = new Date('2025-10-20T14:35:00Z').toISOString();
      expect(formatRelativeTime(timestamp)).toBe('now');
    });

    it('should format minutes ago (1-59 minutes)', () => {
      const timestamp1 = new Date('2025-10-20T14:28:00Z').toISOString();
      expect(formatRelativeTime(timestamp1)).toBe('2m ago');

      const timestamp2 = new Date('2025-10-20T14:00:00Z').toISOString();
      expect(formatRelativeTime(timestamp2)).toBe('30m ago');

      const timestamp3 = new Date('2025-10-20T13:31:00Z').toISOString();
      expect(formatRelativeTime(timestamp3)).toBe('59m ago');
    });

    it('should format hours ago (1-23 hours)', () => {
      const timestamp1 = new Date('2025-10-20T12:30:00Z').toISOString();
      expect(formatRelativeTime(timestamp1)).toBe('2h ago');

      const timestamp2 = new Date('2025-10-20T00:30:00Z').toISOString();
      expect(formatRelativeTime(timestamp2)).toBe('14h ago');

      const timestamp3 = new Date('2025-10-19T15:30:00Z').toISOString();
      expect(formatRelativeTime(timestamp3)).toBe('23h ago');
    });

    it('should format days ago (1-6 days)', () => {
      const timestamp1 = new Date('2025-10-19T14:30:00Z').toISOString();
      expect(formatRelativeTime(timestamp1)).toBe('1d ago');

      const timestamp2 = new Date('2025-10-18T14:30:00Z').toISOString();
      expect(formatRelativeTime(timestamp2)).toBe('2d ago');

      const timestamp3 = new Date('2025-10-14T14:30:00Z').toISOString();
      expect(formatRelativeTime(timestamp3)).toBe('6d ago');
    });

    it('should return date for timestamps older than 7 days', () => {
      const timestamp = new Date('2025-10-10T14:30:00Z').toISOString();
      expect(formatRelativeTime(timestamp)).toBe('2025-10-10');
    });

    it('should handle invalid timestamps', () => {
      expect(formatRelativeTime('invalid')).toBe('invalid');
      expect(formatRelativeTime('not-a-date')).toBe('not-a-date');
    });

    it('should handle edge case at exactly 60 seconds', () => {
      const timestamp = new Date('2025-10-20T14:29:00Z').toISOString();
      expect(formatRelativeTime(timestamp)).toBe('1m ago');
    });
  });

  describe('getMarkColor', () => {
    it('should return neutral color for values near 1.00 (within 5 bps)', () => {
      expect(getMarkColor(1.0)).toBe('text-neutral-400');
      expect(getMarkColor(1.0004)).toBe('text-neutral-400'); // 4 bps
      expect(getMarkColor(0.9996)).toBe('text-neutral-400'); // 4 bps
      expect(getMarkColor(1.00049)).toBe('text-neutral-400'); // 4.9 bps
    });

    it('should return light green for small premium (5-20 bps)', () => {
      expect(getMarkColor(1.0006)).toBe('text-emerald-400/70'); // 6 bps
      expect(getMarkColor(1.0015)).toBe('text-emerald-400/70'); // 15 bps
      expect(getMarkColor(1.00199)).toBe('text-emerald-400/70'); // 19.9 bps
    });

    it('should return medium green for moderate premium (20-50 bps)', () => {
      expect(getMarkColor(1.0021)).toBe('text-emerald-400'); // 21 bps
      expect(getMarkColor(1.0035)).toBe('text-emerald-400'); // 35 bps
      expect(getMarkColor(1.00499)).toBe('text-emerald-400'); // 49.9 bps
    });

    it('should return bright green for large premium (>50 bps)', () => {
      expect(getMarkColor(1.0051)).toBe('text-emerald-300'); // 51 bps
      expect(getMarkColor(1.01)).toBe('text-emerald-300'); // 100 bps
      expect(getMarkColor(1.05)).toBe('text-emerald-300'); // 500 bps
    });

    it('should return light red for small discount (5-20 bps)', () => {
      expect(getMarkColor(0.9994)).toBe('text-red-400/70'); // 6 bps
      expect(getMarkColor(0.9985)).toBe('text-red-400/70'); // 15 bps
      expect(getMarkColor(0.99801)).toBe('text-red-400/70'); // 19.9 bps
    });

    it('should return medium red for moderate discount (20-50 bps)', () => {
      expect(getMarkColor(0.9979)).toBe('text-red-400'); // 21 bps
      expect(getMarkColor(0.9965)).toBe('text-red-400'); // 35 bps
      expect(getMarkColor(0.99501)).toBe('text-red-400'); // 49.9 bps
    });

    it('should return bright red for large discount (>50 bps)', () => {
      expect(getMarkColor(0.9949)).toBe('text-red-300'); // 51 bps
      expect(getMarkColor(0.99)).toBe('text-red-300'); // 100 bps
      expect(getMarkColor(0.95)).toBe('text-red-300'); // 500 bps
    });

    it('should handle edge cases', () => {
      expect(getMarkColor(0)).toBe('text-neutral-400'); // Zero
      expect(getMarkColor(NaN)).toBe('text-neutral-400'); // NaN
      expect(getMarkColor(1.0005)).toBe('text-emerald-400/70'); // Within 5 bps
      expect(getMarkColor(0.9995)).toBe('text-red-400/70'); // Within 5 bps
    });

    it('should handle invalid inputs', () => {
      expect(getMarkColor(null as any)).toBe('text-neutral-400');
      expect(getMarkColor(undefined as any)).toBe('text-neutral-400');
      expect(getMarkColor('not a number' as any)).toBe('text-neutral-400');
    });
  });

  describe('toLocal', () => {
    it('should return empty string for empty input', () => {
      expect(toLocal('')).toBe('');
    });

    it('should handle server timestamp format as UTC', () => {
      // Server timestamp: "2025-10-20 14:20:29" (UTC)
      const result = toLocal('2025-10-20 14:20:29');

      // Should parse as UTC and convert to local
      // We verify it's a valid date string in the expected format
      expect(result).toMatch(/\d{2}\/\d{2}\/\d{4}, \d{2}:\d{2}:\d{2}/);

      // Verify the timestamp is actually treated as UTC by checking the Date object
      const d = new Date('2025-10-20T14:20:29Z');
      const expected = d.toLocaleString(undefined, {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
      });
      expect(result).toBe(expected);
    });

    it('should handle ISO format with Z', () => {
      const result = toLocal('2025-10-20T14:20:29Z');

      const d = new Date('2025-10-20T14:20:29Z');
      const expected = d.toLocaleString(undefined, {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
      });
      expect(result).toBe(expected);
    });

    it('should handle ISO format with T (no Z)', () => {
      const result = toLocal('2025-10-20T14:20:29');

      // Should return a valid date string
      expect(result).toMatch(/\d{2}\/\d{2}\/\d{4}, \d{2}:\d{2}:\d{2}/);
    });

    it('should handle invalid timestamps by returning original string', () => {
      expect(toLocal('invalid')).toBe('invalid');
      expect(toLocal('not-a-date')).toBe('not-a-date');
      expect(toLocal('2025-99-99 25:99:99')).toBe('2025-99-99 25:99:99');
    });

    it('should consistently interpret server format timestamps as UTC', () => {
      // Two identical timestamps in different formats should produce the same result
      const serverFormat = toLocal('2025-10-20 14:20:29');
      const isoFormat = toLocal('2025-10-20T14:20:29Z');

      expect(serverFormat).toBe(isoFormat);
    });
  });
});
