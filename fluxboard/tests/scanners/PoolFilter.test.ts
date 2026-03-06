import { describe, it, expect } from 'vitest';
import { matchesPoolQuery } from '@/stores/scannersStore';

describe('matchesPoolQuery', () => {
  it('matches substrings ignoring case', () => {
    expect(matchesPoolQuery('WBNB/USDT', 'bnb')).toBe(true);
    expect(matchesPoolQuery('WBNB/USDT', 'USDT')).toBe(true);
  });

  it('matches sanitized tokens without separators', () => {
    expect(matchesPoolQuery('WBNB/USDT', 'bnbusdt')).toBe(true);
    expect(matchesPoolQuery('SEI/USDT', 'sei')).toBe(true);
  });

  it('returns false when query not present', () => {
    expect(matchesPoolQuery('WBNB/USDT', 'eth')).toBe(false);
  });
});
