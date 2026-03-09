// Utility functions for deduplication and formatting

import type { MarketSnapshot, Trade, TradeRow, FxPair } from './types';

// Deduplication key generators
export const marketKey = (snap: MarketSnapshot): string =>
  `${snap.exchange}:${snap.symbol}`;

export const tradeKey = (t: Trade | TradeRow): string => (
  (t as TradeRow).row_id ? (t as TradeRow).row_id : t.trade_id
);

// Formatting utilities
export const getSideColor = (side: string): string =>
  side === 'buy' ? 'text-emerald-400' : 'text-red-400';

export const fmtLatency = (ms: number): string => ms.toFixed(1);

// Format price to 4 decimal places (matching legacy GUI)
export const fmtPrice = (price: string | number): string => {
  if (!price || price === '') return '';
  const num = typeof price === 'string' ? parseFloat(price) : price;
  if (isNaN(num)) return String(price);
  return num.toFixed(4);
};

// Format price for Signal table (operator-friendly, stable columns):
// - Fixed decimals per magnitude bucket:
//   - abs(px) >= 1000: 2 dp
//   - abs(px) >= 1:    4 dp
//   - abs(px) <  1:    6 dp
// - Add comma thousands separators when abs(px) >= 1000.
export const fmtPriceSignal = (price: string | number): string => {
  if (price === '' || price === null || price === undefined) return '';
  const num = typeof price === 'string' ? parseFloat(price) : price;
  if (!Number.isFinite(num)) return String(price);

  const abs = Math.abs(num);
  const decimals = abs >= 1000 ? 2 : (abs >= 1 ? 4 : 6);
  const fixed = num.toFixed(decimals);
  if (abs < 1000) return fixed;

  const sign = fixed.startsWith('-') ? '-' : '';
  const [whole, frac] = (sign ? fixed.slice(1) : fixed).split('.');
  const groupedWhole = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return `${sign}${groupedWhole}.${frac ?? ''}`;
};

// Format price for Signal tooltips (a bit higher precision than the table):
// - abs(px) >= 1000: 2 dp
// - abs(px) >= 1:    5 dp
// - abs(px) <  1:    8 dp
export const fmtPriceTooltip = (price: string | number): string => {
  if (!price || price === '') return '';
  const num = typeof price === 'string' ? parseFloat(price) : price;
  if (!Number.isFinite(num)) return String(price);

  const abs = Math.abs(num);
  const decimals = abs >= 1000 ? 2 : (abs >= 1 ? 5 : 8);
  const fixed = num.toFixed(decimals);
  if (abs < 1000) return fixed;

  const sign = fixed.startsWith('-') ? '-' : '';
  const [whole, frac] = (sign ? fixed.slice(1) : fixed).split('.');
  const groupedWhole = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return `${sign}${groupedWhole}.${frac ?? ''}`;
};

// Format trade price with locale string (2-8 decimals)
export const fmtTradePrice = (price: string | number): string => {
  if (!price || price === '') return '';
  const num = typeof price === 'string' ? parseFloat(price) : price;
  if (!Number.isFinite(num)) return '';
  try {
    return num.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 8 });
  } catch (e) {
    return String(num);
  }
};

// Format trade quantity (4-8 decimals)
export const fmtTradeQty = (qty: string | number): string => {
  if (!qty || qty === '') return '';
  const num = typeof qty === 'string' ? parseFloat(qty) : qty;
  if (!Number.isFinite(num)) return '';
  try {
    return num.toLocaleString(undefined, { minimumFractionDigits: 4, maximumFractionDigits: 8 });
  } catch (e) {
    return String(num);
  }
};

// Format market value/notional (2-6 decimals)
export const fmtTradeMV = (mv: string | number): string => {
  if (!mv || mv === '') return '';
  const num = typeof mv === 'string' ? parseFloat(mv) : mv;
  if (!Number.isFinite(num)) return '';
  try {
    return num.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 6 });
  } catch (e) {
    return String(num);
  }
};

// Format fee (2-8 decimals)
export const fmtTradeFee = (fee: string | number): string => {
  if (!fee || fee === '') return '';
  const num = typeof fee === 'string' ? parseFloat(fee) : fee;
  if (!Number.isFinite(num)) return '';
  try {
    return num.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 8 });
  } catch (e) {
    return String(num);
  }
};

// Format quantity with 0 decimals (for market data bid_qty/ask_qty)
export const fmtQty = (qty: string | number): string => {
  if (!qty || qty === '') return '';
  const num = typeof qty === 'string' ? parseFloat(qty) : qty;
  if (!Number.isFinite(num)) return '';
  try {
    return num.toLocaleString(undefined, { minimumFractionDigits: 0, maximumFractionDigits: 0 });
  } catch (e) {
    return String(num);
  }
};

// Format timestamp to match legacy GUI: YYYY-MM-DD HH:MM:SS.MS (2 decimal places)
export const fmtTime19 = (timestamp_ms: number): string => {
  if (!timestamp_ms) return '';

  const d = new Date(timestamp_ms);
  if (isNaN(d.getTime())) return '';

  const pad = (n: number) => n < 10 ? `0${n}` : `${n}`;
  const year = d.getFullYear();
  const month = pad(d.getMonth() + 1);
  const day = pad(d.getDate());
  const hour = pad(d.getHours());
  const minute = pad(d.getMinutes());
  const second = pad(d.getSeconds());
  const centisecond = pad(Math.floor(d.getMilliseconds() / 10));

  return `${year}-${month}-${day} ${hour}:${minute}:${second}.${centisecond}`;
};

// FX utilities
export const bpsFromPar = (priceStr: string): number => {
  const price = parseFloat(priceStr);
  if (!Number.isFinite(price)) return 0;
  return Math.round(10000 * (price - 1));
};

export const formatDecimal = (value: string, decimals: number): string => {
  const num = parseFloat(value);
  if (!Number.isFinite(num)) return value;
  return num.toFixed(decimals);
};

export const fmtAgeSec = (ageMs: number): string => {
  const sec = Math.floor(ageMs / 1000);
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ${sec % 60}s`;
  const hr = Math.floor(min / 60);
  return `${hr}h ${min % 60}m`;
};

export type FxStatus = 'green' | 'yellow' | 'red';

export const deriveFxStatus = (pair: FxPair): FxStatus => {
  if (pair.stale) return 'red';
  if (pair.clamp_breach) return 'red';
  if (pair.source !== 'bybit') return 'yellow';
  if (pair.jump_bps && pair.jump_bps > 0) return 'yellow';
  return 'green';
};

export const fxStatusColor = (status: FxStatus): string => {
  switch (status) {
    case 'green': return 'text-emerald-400';
    case 'yellow': return 'text-yellow-400';
    case 'red': return 'text-red-400';
  }
};

// Simple chain-aware explorer mapping for TX hyperlinks in Alerts
const EXPLORER_BASE: Record<string, string> = {
  'plume-testnet': 'https://testnet-explorer.plumenetwork.xyz/tx/',
  'plume': 'https://explorer.plumenetwork.xyz/tx/'
};

export function txExplorerUrl(txHash: string, chain?: string | number): string {
  const key = typeof chain === 'string' ? chain.toLowerCase() : String(chain ?? '');
  const base = EXPLORER_BASE[key] || EXPLORER_BASE['plume-testnet'];
  return `${base}${txHash}`;
}

// Balance formatting utilities (matching legacy GUI patterns)
export const fmtBalanceQty = (qty: string | number): string => {
  if (!qty || qty === '') return '';
  const num = typeof qty === 'string' ? parseFloat(qty) : qty;
  if (!Number.isFinite(num)) return '';
  try {
    return num.toLocaleString(undefined, { minimumFractionDigits: 3, maximumFractionDigits: 3 });
  } catch (e) {
    return String(num);
  }
};

export const fmtBalanceMV = (mv: string | number): string => {
  if (mv === '' || mv === null || mv === undefined) return '';
  const num = typeof mv === 'string' ? parseFloat(mv) : mv;
  if (!Number.isFinite(num)) return '';
  try {
    const absFormatted = Math.abs(num).toLocaleString(undefined, { minimumFractionDigits: 0, maximumFractionDigits: 0 });
    const sign = num < 0 ? '-' : '';
    return `${sign}$${absFormatted}`;
  } catch (e) {
    return String(num);
  }
};

export const fmtBalanceMark = (mark: string | number): string => {
  if (!mark || mark === '') return '';
  const num = typeof mark === 'string' ? parseFloat(mark) : mark;
  if (!Number.isFinite(num)) return '';
  try {
    return num.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  } catch (e) {
    return String(num);
  }
};
