export const toNum = (v: unknown): number => {
  const n = typeof v === 'number' ? v : Number(v);
  return Number.isFinite(n) ? n : NaN;
};

export const fmtFixed = (v: unknown, d = 2, dash = '—') => {
  const n = toNum(v);
  if (!Number.isFinite(n)) return dash;
  try {
    return n.toLocaleString(undefined, {
      minimumFractionDigits: d,
      maximumFractionDigits: d,
    });
  } catch (_e) {
    return n.toFixed(d);
  }
};

export const fmtMoney = (v: unknown, d = 2, dash = '—') => {
  const n = toNum(v);
  if (!Number.isFinite(n)) return dash;
  try {
    const formatted = Math.abs(n).toLocaleString(undefined, {
      minimumFractionDigits: d,
      maximumFractionDigits: d,
    });
    return `$${n < 0 ? '-' : ''}${formatted}`;
  } catch (_e) {
    return `$${n < 0 ? '-' : ''}${Math.abs(n).toFixed(d)}`;
  }
};

// Format quantity with thousands separators and fixed decimals
export const fmtQty = (v: unknown, d = 2, dash = '—') => {
  const n = toNum(v);
  if (!Number.isFinite(n)) return dash;
  try {
    return n.toLocaleString(undefined, {
      minimumFractionDigits: d,
      maximumFractionDigits: d,
    });
  } catch (_e) {
    return n.toFixed(d);
  }
};

export const fmtDualPnL = (bps: unknown, usd?: unknown) => {
  const b = toNum(bps), u = toNum(usd);
  const bStr = Number.isFinite(b) ? `${b >= 0 ? '+' : ''}${b.toFixed(2)} bps` : '—';
  if (!Number.isFinite(u)) return bStr;
  try {
    const formatted = Math.abs(u).toLocaleString(undefined, {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    });
    const uStr = `$${formatted}`;
    return `${bStr} (${uStr})`;
  } catch (_e) {
    const uStr = `$${Math.abs(u).toFixed(2)}`;
    return `${bStr} (${uStr})`;
  }
};

export const toLocal = (s?: string) => (s ? new Date(s).toLocaleString() : '—');

// Compatibility helpers: some panels expect these names.
export const fmtUsd = (v: unknown, d = 2, dash = '—') => fmtMoney(v, d, dash);
export const fmtNumber = (v: unknown, d = 2, dash = '—') => fmtFixed(v, d, dash);

