export function formatUsdCompact(value: number | null | undefined): string {
  if (value === null || value === undefined) return '—';
  const n = Number(value);
  if (!Number.isFinite(n) || n === 0) return '—';
  const abs = Math.abs(n);
  if (abs >= 1_000_000_000) return `$${(n / 1_000_000_000).toFixed(1)}B`;
  if (abs >= 1_000_000) return `$${(n / 1_000_000).toFixed(1)}M`;
  if (abs >= 1_000) return `$${(n / 1_000).toFixed(1)}K`;
  return `$${n.toFixed(0)}`;
}

export function formatFeeBps(value: number | null | undefined): string {
  if (value === null || value === undefined) return '0';
  const rounded = Math.round(Number(value));
  if (!Number.isFinite(rounded)) return '0';
  return rounded.toString();
}

export function formatEdgeValue(value: number | null | undefined): string {
  if (value === null || value === undefined) return '0.0';
  const n = Number(value);
  if (!Number.isFinite(n)) return '0.0';
  return n.toFixed(1);
}
