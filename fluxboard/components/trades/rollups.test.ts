import { describe, expect, it } from 'vitest';

import { computeTradesRollups } from './rollups';

describe('computeTradesRollups', () => {
  it('sums qty/notional/fee and prefers fee_quote when present', () => {
    const rows = [
      { qty: '1.5', mv: 10, fee: 0.1, fee_quote: 0.5 },
      { qty: 2, mv: '20', fee: '0.2', fee_quote: null },
      { qty: null, mv: null, fee: null, fee_quote: undefined },
    ] as any;

    expect(computeTradesRollups(rows)).toEqual({ qty: 3.5, notional: 30, fee: 0.7 });
  });

  it('treats missing rows as zero', () => {
    expect(computeTradesRollups(undefined)).toEqual({ qty: 0, notional: 0, fee: 0 });
    expect(computeTradesRollups(null)).toEqual({ qty: 0, notional: 0, fee: 0 });
  });
});

