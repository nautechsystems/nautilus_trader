import type { MarketSnapshot } from '@/types';

const toNumber = (value: unknown): number | undefined => {
  if (value === null || value === undefined) return undefined;
  const num = typeof value === 'string' ? Number(value) : Number(value);
  return Number.isFinite(num) ? num : undefined;
};

export function normalizeMarketRow(row: any): MarketSnapshot {
  const ts = toNumber(row?.timestamp_ms ?? row?.timestamp ?? row?.observed_ts);
  const bid = row?.bid ?? row?.bid_px ?? '';
  const ask = row?.ask ?? row?.ask_px ?? '';
  const mid = row?.mid_px ?? row?.mid ?? '';
  const bidQty = row?.bid_qty ?? row?.bid_size ?? '';
  const askQty = row?.ask_qty ?? row?.ask_size ?? '';
  const coin = row?.coin ?? row?.symbol ?? '';
  const exchange = row?.exchange ?? row?.venue ?? '';

  return {
    ...row,
    coin,
    exchange,
    bid,
    ask,
    mid_px: mid,
    bid_qty: bidQty,
    ask_qty: askQty,
    timestamp_ms: ts,
    _normalized: true,
  } as MarketSnapshot;
}
