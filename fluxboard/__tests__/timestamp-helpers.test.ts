import { describe, expect, it } from 'vitest';
import { hasReliableTradeTimestamp } from '../Trades';

describe('hasReliableTradeTimestamp', () => {
  it('returns true when seq is a finite number', () => {
    expect(hasReliableTradeTimestamp({ seq: 123 })).toBe(true);
  });

  it('returns true when ts_ms is a finite number and seq is missing', () => {
    expect(hasReliableTradeTimestamp({ ts_ms: 1_700_000_000 })).toBe(true);
  });

  it('returns true when ts is a numeric string', () => {
    expect(hasReliableTradeTimestamp({ ts: '1700000000' })).toBe(true);
  });

  it('returns false when no numeric timestamps are present', () => {
    expect(hasReliableTradeTimestamp({})).toBe(false);
    expect(hasReliableTradeTimestamp({ seq: 'abc', ts_ms: null, ts: '' })).toBe(false);
  });
});
