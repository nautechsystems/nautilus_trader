import { describe, expect, it } from 'vitest';

import { fmtBalanceMV, fmtPriceSignal, fmtPriceTooltip } from './utils';

describe('fmtBalanceMV', () => {
  it('preserves zero exposure so it still renders as $0', () => {
    expect(fmtBalanceMV(0)).toBe('$0');
    expect(fmtBalanceMV('0')).toBe('$0');
  });

  it('keeps the sign for negative exposures', () => {
    expect(fmtBalanceMV(-1250.4)).toBe('-$1,250');
  });
});

describe('fmtPriceSignal', () => {
  it('adds thousands separators for prices >= 1000 and uses 2 dp', () => {
    expect(fmtPriceSignal(65756.6)).toBe('65,756.60');
    expect(fmtPriceSignal('1918.88')).toBe('1,918.88');
  });

  it('does not add thousands separators for prices < 1000', () => {
    expect(fmtPriceSignal(999.1234)).toBe('999.1234');
    expect(fmtPriceSignal('0.2571')).toBe('0.257100');
  });
});

describe('fmtPriceTooltip', () => {
  it('uses higher precision than the table, with stable buckets', () => {
    expect(fmtPriceTooltip(65756.6)).toBe('65,756.60');
    expect(fmtPriceTooltip('1918.88')).toBe('1,918.88');
    expect(fmtPriceTooltip(999.1234)).toBe('999.12340');
    expect(fmtPriceTooltip('0.2571')).toBe('0.25710000');
  });
});
