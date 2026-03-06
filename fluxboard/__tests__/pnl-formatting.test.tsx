import { describe, it, expect } from 'vitest';

// Mock the formatting functions from PnL.tsx
const fmtDualPnL = (bps: number, usd?: number) => {
  const bpsStr = `${bps >= 0 ? '+' : ''}${bps.toFixed(2)} bps`;
  if (usd === undefined) return bpsStr;
  const usdStr = `$${Math.abs(usd).toFixed(2)}`;
  return `${bpsStr} (${usdStr})`;
};

const fmtPrimary = (bps: number, usd?: number, unitPrimary: 'bps' | 'usd' = 'bps') => {
  if (unitPrimary === 'usd' && usd !== undefined) {
    return `$${usd.toFixed(2)}`;
  }
  return `${bps.toFixed(2)} bps`;
};

describe('PnL Formatting Functions', () => {
  describe('fmtDualPnL', () => {
    it('formats positive bps only', () => {
      expect(fmtDualPnL(12.5)).toBe('+12.50 bps');
    });

    it('formats negative bps only', () => {
      expect(fmtDualPnL(-5.25)).toBe('-5.25 bps');
    });

    it('formats positive bps and usd', () => {
      expect(fmtDualPnL(12.5, 125.75)).toBe('+12.50 bps ($125.75)');
    });

    it('formats negative bps and positive usd', () => {
      expect(fmtDualPnL(-5.25, 52.50)).toBe('-5.25 bps ($52.50)');
    });

    it('formats negative bps and negative usd', () => {
      expect(fmtDualPnL(-5.25, -52.50)).toBe('-5.25 bps ($52.50)');
    });

    it('handles zero values', () => {
      expect(fmtDualPnL(0, 0)).toBe('+0.00 bps ($0.00)');
    });

    it('handles undefined usd', () => {
      expect(fmtDualPnL(10.5, undefined)).toBe('+10.50 bps');
    });
  });

  describe('fmtPrimary', () => {
    it('returns bps when unitPrimary is bps', () => {
      expect(fmtPrimary(12.5, 125.75, 'bps')).toBe('12.50 bps');
    });

    it('returns usd when unitPrimary is usd', () => {
      expect(fmtPrimary(12.5, 125.75, 'usd')).toBe('$125.75');
    });

    it('returns bps when usd is undefined', () => {
      expect(fmtPrimary(12.5, undefined, 'usd')).toBe('12.50 bps');
    });

    it('handles negative values', () => {
      expect(fmtPrimary(-5.25, -52.50, 'usd')).toBe('$-52.50');
    });

    it('defaults to bps when unitPrimary not specified', () => {
      expect(fmtPrimary(10.5, 105.0)).toBe('10.50 bps');
    });
  });
});
