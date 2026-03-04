/**
 * COLUMN_MAP
 *
 * Shared column width and alignment definitions used across Fluxboard tables.
 * Centralizing these ensures Params, Balances, and Trades align visually.
 */

export type ColumnSpec = {
  min: number;
  align: 'left' | 'right' | 'center';
};

export const COLUMN_MAP = {
  // Common financial columns
  coin: { min: 160, align: 'left' },
  qty: { min: 96, align: 'right' },
  mv: { min: 96, align: 'right' },
  mark: { min: 88, align: 'right' },
  time: { min: 130, align: 'right' },

  // Trades-specific columns
  // Wide enough for "YYYY-MM-DD HH:mm:ss" (19 chars) + padding in monospace
  // ~19ch * ~8px + 16px padding ≈ 168px; add buffer for fractional seconds
  timeShort: { min: 176, align: 'left' },
  exch: { min: 100, align: 'left' },
  side: { min: 46, align: 'center' },
  px: { min: 100, align: 'right' },
  notional: { min: 100, align: 'right' },
  fee: { min: 64, align: 'right' },
  gas_used: { min: 74, align: 'right' },
  id: { min: 120, align: 'left' },
  decision: { min: 54, align: 'left' },
  decision_summary: { min: 240, align: 'left' },
  notes: { min: 320, align: 'left' },
} as const;

export type ColumnKey = keyof typeof COLUMN_MAP;

/**
 * Utility to generate CSS grid template from a sequence of column keys.
 */
export function gridTemplateFrom(keys: ColumnKey[]): string {
  return keys.map((k) => `${COLUMN_MAP[k].min}px`).join(' ');
}

export function gridMinWidth(keys: ColumnKey[]): number {
  return keys.reduce((acc, key) => acc + COLUMN_MAP[key].min, 0);
}
