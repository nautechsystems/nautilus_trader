import { describe, it, expect } from 'vitest';
import {
  formatUsdCompact,
  formatEdgeValue,
  formatFeeBps,
} from '@/utils/scannersFormatting';

describe('Scanners column formatting helpers', () => {
  it('condenses USD values', () => {
    expect(formatUsdCompact(1_500_000)).toBe('$1.5M');
    expect(formatUsdCompact(12_300)).toBe('$12.3K');
    expect(formatUsdCompact(950)).toBe('$950');
    expect(formatUsdCompact(0)).toBe('—');
  });

  it('rounds fee bps to whole numbers', () => {
    expect(formatFeeBps(12.6)).toBe('13');
    expect(formatFeeBps(9.2)).toBe('9');
    expect(formatFeeBps(undefined)).toBe('0');
  });

  it('renders edge values with single decimal place', () => {
    expect(formatEdgeValue(12.34)).toBe('12.3');
    expect(formatEdgeValue(0)).toBe('0.0');
    expect(formatEdgeValue(null)).toBe('0.0');
  });
});
