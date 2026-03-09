import { describe, expect, it } from 'vitest';

import {
  isStable,
  isMajorCrypto,
  formatMark,
  formatQty,
} from './assetFormat';

const parseNumber = (value: string): number => Number(value.replace(/,/g, ''));

describe('formatQty', () => {
  it('formats stablecoin balances with two decimals', () => {
    expect(formatQty('PUSD', 77272.797644, 1)).toBe('77,272.80');
    expect(formatQty('USDC', 6339.921535, 1)).toBe('6,339.92');
  });

  it('formats large alt balances based on MV bands', () => {
    expect(formatQty('HUMA', 802454.868346, 0.0284)).toBe('802,454.87');
    expect(formatQty('PLUME', 1037625.608042, 0.0217)).toBe('1,037,625.61');
    expect(formatQty('ZENT', 1034780.374933, 0.0045)).toBe('1,034,780.375');
  });

  it('formats major crypto with higher precision below 1 unit', () => {
    expect(formatQty('WETH', 0.015931, 3129.155)).toBe('0.015931');
  });

  it('formats equity perps with up to three decimals', () => {
    expect(formatQty('NVDA_PERP', -10.111, 182.095)).toBe('-10.111');
  });

  it('returns em dash for missing qty', () => {
    expect(formatQty('USDC', null, 1)).toBe('—');
    expect(formatQty('USDC', undefined, 1)).toBe('—');
  });
});

describe('formatMark', () => {
  it('formats stablecoins at two decimals', () => {
    expect(formatMark('USDT', 1)).toBe('1.00');
  });

  it('formats crypto marks with bands', () => {
    expect(formatMark('HUMA', 0.0284)).toBe('0.0284');
    expect(formatMark('WETH', 3129.155)).toBe('3,129.16');
  });

  it('returns em dash for missing mark', () => {
    expect(formatMark('USDC', null)).toBe('—');
    expect(formatMark('USDC', undefined)).toBe('—');
  });
});

describe('classification helpers', () => {
  it('detects stable variants and excludes perps', () => {
    expect(isStable('USDC_ETH')).toBe(true);
    expect(isStable('USDC.ARB')).toBe(true);
    expect(isStable('USDT.BNB')).toBe(true);
    expect(isStable('PUSD_LOGICAL')).toBe(true);
    expect(isStable('USDC_PERP')).toBe(false);
  });

  it('detects major cryptos with suffixes and perps', () => {
    expect(isMajorCrypto('WETH')).toBe(true);
    expect(isMajorCrypto('WETH_ARB')).toBe(true);
    expect(isMajorCrypto('BTC.ARB')).toBe(true);
    expect(isMajorCrypto('ETH_PERP')).toBe(true);
  });
});

describe('mark-value error bounds', () => {
  const mvError = (symbol: string, qty: number, mark: number): number => {
    const displayedQty = parseNumber(formatQty(symbol, qty, mark));
    const mvRaw = Math.abs(qty * mark);
    const mvDisplayed = Math.abs(displayedQty * mark);
    return Math.abs(mvRaw - mvDisplayed);
  };

  const bound = (qty: number, mark: number) => {
    const mv = Math.abs(qty * mark);
    return Math.max(0.01, mv * 0.001);
  };

  it('keeps MV error within bound for small positions', () => {
    const qty = 1.2345;
    const mark = 0.005;
    expect(mvError('TINY', qty, mark)).toBeLessThanOrEqual(bound(qty, mark));
  });

  it('keeps MV error within bound for large high-price alts', () => {
    const qty = 12.34;
    const mark = 1234;
    expect(mvError('ALT', qty, mark)).toBeLessThanOrEqual(bound(qty, mark));
  });
});
