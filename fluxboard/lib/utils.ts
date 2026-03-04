/**
 * Utility Functions
 *
 * Common utilities for className composition, type guards, helpers, and trading-specific formatting.
 */

import type { KeyboardEvent } from 'react';
import { type ClassValue, clsx } from 'clsx';
import { twMerge } from 'tailwind-merge';
import type { MarketSnapshot, Trade, TradeRow, FxPair } from '../types';

// =============================================================================
// CLASSNAME UTILITIES
// =============================================================================

/**
 * Combines Tailwind classes with proper conflict resolution
 *
 * Uses clsx for conditional classes and tailwind-merge to resolve conflicts
 *
 * @example
 * cn('px-2 py-1', isActive && 'bg-blue-500', className)
 */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

// =============================================================================
// DEEP MERGE UTILITY
// =============================================================================

/**
 * Deep merge utility for merging objects recursively.
 * - undefined values are ignored (no change)
 * - null values explicitly delete keys
 * - Objects are merged recursively
 * - Arrays are replaced (not merged)
 *
 * @example
 * deepMerge({ a: 1, b: { c: 2 } }, { b: { d: 3 } })
 * // => { a: 1, b: { c: 2, d: 3 } }
 */
export function deepMerge<T>(base: T, patch: Partial<T>): T {
  if (patch === null) return patch as any;
  if (typeof base !== 'object' || typeof patch !== 'object' || !base || !patch) {
    return (patch as T) ?? base;
  }

  const out: any = Array.isArray(base) ? [...base] : { ...base };
  for (const [k, v] of Object.entries(patch)) {
    if (v === undefined) continue; // no change
    if (v === null) {
      delete out[k];
      continue;
    } // explicit delete
    out[k] = deepMerge((out as any)[k], v as any);
  }
  return out;
}

// =============================================================================
// TYPE GUARDS
// =============================================================================

/**
 * Check if value is not null or undefined
 */
export function isDefined<T>(value: T | null | undefined): value is T {
  return value !== null && value !== undefined;
}

/**
 * Check if value is a number and not NaN
 */
export function isValidNumber(value: unknown): value is number {
  return typeof value === 'number' && !isNaN(value) && isFinite(value);
}

/**
 * Check if string is non-empty
 */
export function isNonEmptyString(value: unknown): value is string {
  return typeof value === 'string' && value.trim().length > 0;
}

// =============================================================================
// KEYBOARD UTILITIES
// =============================================================================

/**
 * Check if key event is Enter or Space (standard activation keys)
 */
export function isActivationKey(event: KeyboardEvent): boolean {
  return event.key === 'Enter' || event.key === ' ';
}

/**
 * Check if key event is Escape
 */
export function isEscapeKey(event: KeyboardEvent): boolean {
  return event.key === 'Escape';
}

/**
 * Check if key event is an arrow key
 */
export function isArrowKey(event: KeyboardEvent): boolean {
  return ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(event.key);
}

// =============================================================================
// FOCUS MANAGEMENT
// =============================================================================

/**
 * Trap focus within a container element
 */
export function trapFocus(container: HTMLElement, event: KeyboardEvent) {
  const focusableElements = container.querySelectorAll<HTMLElement>(
    'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
  );

  const firstFocusable = focusableElements[0];
  const lastFocusable = focusableElements[focusableElements.length - 1];

  if (event.key === 'Tab') {
    if (event.shiftKey && document.activeElement === firstFocusable) {
      event.preventDefault();
      lastFocusable?.focus();
    } else if (!event.shiftKey && document.activeElement === lastFocusable) {
      event.preventDefault();
      firstFocusable?.focus();
    }
  }
}

/**
 * Store focus on an element and return a function to restore it
 */
export function preserveFocus(element: HTMLElement | null): () => void {
  const previousActiveElement = document.activeElement as HTMLElement;

  return () => {
    if (previousActiveElement && previousActiveElement !== element) {
      previousActiveElement.focus();
    }
  };
}

// =============================================================================
// DEBOUNCE / THROTTLE
// =============================================================================

/**
 * Debounce a function call
 */
export function debounce<T extends (...args: any[]) => any>(
  func: T,
  delay: number
): (...args: Parameters<T>) => void {
  let timeoutId: NodeJS.Timeout;

  return function (...args: Parameters<T>) {
    clearTimeout(timeoutId);
    timeoutId = setTimeout(() => func(...args), delay);
  };
}

/**
 * Throttle a function call
 */
export function throttle<T extends (...args: any[]) => any>(
  func: T,
  limit: number
): (...args: Parameters<T>) => void {
  let inThrottle: boolean;

  return function (...args: Parameters<T>) {
    if (!inThrottle) {
      func(...args);
      inThrottle = true;
      setTimeout(() => (inThrottle = false), limit);
    }
  };
}

// =============================================================================
// DATA FORMATTING HELPERS
// =============================================================================

/**
 * Format number with specified precision
 */
export function formatNumber(value: number, precision: number = 2): string {
  if (!isValidNumber(value)) return '—';
  return value.toFixed(precision);
}

/**
 * Format percentage
 */
export function formatPercent(value: number, precision: number = 2): string {
  if (!isValidNumber(value)) return '—';
  return `${(value * 100).toFixed(precision)}%`;
}

/**
 * Format basis points (1 bps = 0.01%)
 */
export function formatBps(value: number, precision: number = 1): string {
  if (!isValidNumber(value)) return '—';
  return `${value.toFixed(precision)} bps`;
}

/**
 * Truncate string with ellipsis
 */
export function truncate(str: string, maxLength: number): string {
  if (str.length <= maxLength) return str;
  return `${str.slice(0, maxLength)}…`;
}

/**
 * Truncate middle of string (useful for hashes)
 */
export function truncateMiddle(str: string, startChars: number = 6, endChars: number = 4): string {
  if (str.length <= startChars + endChars) return str;
  return `${str.slice(0, startChars)}…${str.slice(-endChars)}`;
}

// =============================================================================
// ARRAY UTILITIES
// =============================================================================

/**
 * Get unique values from array
 */
export function unique<T>(array: T[]): T[] {
  return Array.from(new Set(array));
}

/**
 * Group array by key
 */
export function groupBy<T, K extends keyof any>(
  array: T[],
  getKey: (item: T) => K
): Record<K, T[]> {
  return array.reduce((result, item) => {
    const key = getKey(item);
    if (!result[key]) {
      result[key] = [];
    }
    result[key].push(item);
    return result;
  }, {} as Record<K, T[]>);
}

/**
 * Sort array by key (supports nested keys via dot notation)
 */
export function sortBy<T>(
  array: T[],
  key: keyof T | string,
  direction: 'asc' | 'desc' = 'asc'
): T[] {
  return [...array].sort((a, b) => {
    const aVal = typeof key === 'string' && key.includes('.')
      ? getNestedValue(a, key)
      : (a as any)[key];
    const bVal = typeof key === 'string' && key.includes('.')
      ? getNestedValue(b, key)
      : (b as any)[key];

    if (aVal === bVal) return 0;

    const comparison = aVal < bVal ? -1 : 1;
    return direction === 'asc' ? comparison : -comparison;
  });
}

/**
 * Get nested value from object using dot notation
 */
function getNestedValue(obj: any, path: string): any {
  return path.split('.').reduce((acc, part) => acc?.[part], obj);
}

// =============================================================================
// DATE/TIME UTILITIES
// =============================================================================

/**
 * Get relative time string (e.g., "2s ago", "3m ago")
 */
export function getRelativeTime(timestamp: number): string {
  const now = Date.now();
  const diff = now - timestamp;

  const seconds = Math.floor(diff / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);

  if (days > 0) return `${days}d ago`;
  if (hours > 0) return `${hours}h ago`;
  if (minutes > 0) return `${minutes}m ago`;
  if (seconds > 0) return `${seconds}s ago`;
  return 'just now';
}

/**
 * Check if timestamp is stale (older than threshold)
 */
export function isStale(timestamp: number, thresholdMs: number): boolean {
  return Date.now() - timestamp > thresholdMs;
}

// =============================================================================
// PERFORMANCE UTILITIES
// =============================================================================

/**
 * Measure execution time of a function
 *
 * @param fn - Function to measure
 * @param label - Optional label for logging (only logs in development)
 * @returns Object with result and duration in milliseconds
 */
export async function measure<T>(
  fn: () => Promise<T> | T,
  label?: string
): Promise<{ result: T; duration: number }> {
  const start = performance.now();
  const result = await fn();
  const duration = performance.now() - start;

  // Only log in development mode
  if (label && import.meta.env?.DEV) {
    console.log(`[Performance] ${label}: ${duration.toFixed(2)}ms`);
  }

  return { result, duration };
}

/**
 * Create a stable ID for React keys (better than index)
 */
export function createStableId(...parts: (string | number | undefined)[]): string {
  return parts.filter(isDefined).join('-');
}

// =============================================================================
// TRADING-SPECIFIC UTILITIES
// =============================================================================

/**
 * Generate deduplication key for market snapshot
 */
export const marketKey = (snap: MarketSnapshot): string =>
  `${snap.exchange}:${snap.symbol}`;

/**
 * Generate deduplication key for trade
 */
export const tradeKey = (t: Trade | TradeRow): string => (
  (t as TradeRow).row_id ? (t as TradeRow).row_id : t.trade_id
);

/**
 * Get color class for trade side
 */
export const getSideColor = (side: string): string =>
  side === 'buy' ? 'text-emerald-400' : 'text-red-400';

// =============================================================================
// FX UTILITIES
// =============================================================================

/**
 * Calculate basis points from par (1.0)
 */
export const bpsFromPar = (priceStr: string): number => {
  const price = parseFloat(priceStr);
  if (!Number.isFinite(price)) return 0;
  return Math.round(10000 * (price - 1));
};

/**
 * Format decimal with specified number of decimals
 */
export const formatDecimal = (value: string, decimals: number): string => {
  const num = parseFloat(value);
  if (!Number.isFinite(num)) return value;
  return num.toFixed(decimals);
};

export type FxStatus = 'green' | 'yellow' | 'red';

/**
 * Derive FX status color from FX pair data
 */
export const deriveFxStatus = (pair: FxPair): FxStatus => {
  if (pair.stale) return 'red';
  if (pair.clamp_breach) return 'red';
  if (pair.source !== 'bybit') return 'yellow';
  if (pair.jump_bps && pair.jump_bps > 0) return 'yellow';
  return 'green';
};

/**
 * Get Tailwind color class for FX status
 */
export const fxStatusColor = (status: FxStatus): string => {
  switch (status) {
    case 'green': return 'text-emerald-400';
    case 'yellow': return 'text-yellow-400';
    case 'red': return 'text-red-400';
  }
};

// =============================================================================
// BLOCKCHAIN UTILITIES
// =============================================================================

/**
 * Simple chain-aware explorer mapping for TX hyperlinks in Alerts
 */
const EXPLORER_BASE: Record<string, string> = {
  'plume-testnet': 'https://testnet-explorer.plumenetwork.xyz/tx/',
  'plume': 'https://explorer.plumenetwork.xyz/tx/'
};

/**
 * Get block explorer URL for transaction hash
 */
export function txExplorerUrl(txHash: string, chain?: string | number): string {
  const key = typeof chain === 'string' ? chain.toLowerCase() : String(chain ?? '');
  const base = EXPLORER_BASE[key] || EXPLORER_BASE['plume-testnet'];
  return `${base}${txHash}`;
}

// NOTE: Formatting utilities (fmtLatency, fmtPrice, fmtTradePrice, fmtBalanceMV, etc.)
// are located in @/utils (fluxboard/utils.ts) to avoid duplication.
