import type { TradeRow } from '../../types';

export type TradesRollups = {
  qty: number;
  notional: number;
  fee: number;
};

function coerceNumber(value: unknown): number | undefined {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (!trimmed) return undefined;
    const parsed = Number(trimmed);
    return Number.isFinite(parsed) ? parsed : undefined;
  }
  return undefined;
}

function sum(rows: readonly TradeRow[], getter: (row: TradeRow) => unknown): number {
  let total = 0;
  for (const row of rows) {
    const n = coerceNumber(getter(row));
    if (n !== undefined) total += n;
  }
  return total;
}

export function computeTradesRollups(rows: readonly TradeRow[] | null | undefined): TradesRollups {
  const resolved = Array.isArray(rows) ? rows : [];
  return {
    qty: sum(resolved, (row) => row.qty),
    notional: sum(resolved, (row) => row.mv),
    // Prefer fee in quote asset units when present so totals match the column display.
    fee: sum(resolved, (row) => (row.fee_quote ?? row.fee)),
  };
}

