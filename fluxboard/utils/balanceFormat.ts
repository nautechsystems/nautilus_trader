import { isStable } from '../lib/assetFormat';
export { formatMark, formatQty } from '../lib/assetFormat';

export const DUST_THRESHOLD = 0.01;

export function formatMoney(value: number | null | undefined): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '$0.00';
  const abs = Math.abs(value);
  const decimals = abs >= 10000 ? 0 : 2;
  const formatted = abs.toLocaleString(undefined, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
  const sign = value < 0 ? '-' : '';
  return `${sign}$${formatted}`;
}

export function formatMoneyNoSign(value: number | null | undefined): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '$0.00';
  const abs = Math.abs(value);
  const decimals = abs >= 10000 ? 0 : 2;
  const formatted = abs.toLocaleString(undefined, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
  return `$${formatted}`;
}

export function isStableSymbol(symbol?: string | null): boolean {
  if (!symbol) return false;
  return isStable(symbol);
}

export function formatAge(lastTs: number | null | undefined, nowMs = Date.now()): string {
  if (!lastTs || Number.isNaN(lastTs)) return '—';
  const diff = nowMs - lastTs;
  if (diff < 0) return 'just now';
  const seconds = Math.floor(diff / 1000);
  if (seconds < 60) return 'just now';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function shortAddress(address?: string | null): string | null {
  if (!address) return null;
  if (address.length <= 10) return address;
  return `${address.slice(0, 6)}…${address.slice(-4)}`;
}
