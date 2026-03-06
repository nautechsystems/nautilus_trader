import { Fragment, useMemo, useState } from 'react';
import { Tooltip, TooltipProvider } from '../ui/tooltip';
import { formatMoney, formatMoneyNoSign } from '../../utils/balanceFormat';
import { formatMark, formatQty } from '../../lib/assetFormat';
import { RiskGroup } from '../../types';
import { cn } from '../../lib/utils';

export type RiskSortColumn = 'underlying' | 'net_qty' | 'net_mv' | 'long_mv' | 'short_mv' | 'gross_mv' | 'sources';
export type RiskSortDirection = 'asc' | 'desc';

export type RiskSortState = {
  column: RiskSortColumn;
  direction: RiskSortDirection;
};

export type RiskSourceBreakdownRow = {
  venue: string;
  coin: string;
  qty_raw: number;
  mv_raw: number;
  mark_raw?: number | null;
  time_display?: string | null;
  label?: string | null;
  wallet?: string | null;
  address?: string | null;
};

const columnMeta: Record<
  RiskSortColumn,
  { label: string; isNumeric: boolean; align: 'left' | 'right'; minWidth: number }
> = {
  underlying: { label: 'Underlying', isNumeric: false, align: 'left', minWidth: 200 },
  net_qty: { label: 'Net Qty', isNumeric: true, align: 'right', minWidth: 110 },
  net_mv: { label: 'Net MV', isNumeric: true, align: 'right', minWidth: 110 },
  long_mv: { label: 'Long MV', isNumeric: true, align: 'right', minWidth: 110 },
  short_mv: { label: 'Short MV', isNumeric: true, align: 'right', minWidth: 110 },
  gross_mv: { label: 'Gross MV', isNumeric: true, align: 'right', minWidth: 110 },
  sources: { label: 'Sources', isNumeric: false, align: 'right', minWidth: 160 },
};

const DEFAULT_SORT: RiskSortState = { column: 'gross_mv', direction: 'desc' };

export function RiskTable({
  rows,
  breakdowns,
  search,
  nonZeroOnly,
  sort,
  onSortChange,
  onRowClick,
}: {
  rows: RiskGroup[];
  breakdowns?: Record<string, RiskSourceBreakdownRow[]>;
  search: string;
  nonZeroOnly: boolean;
  sort: RiskSortState;
  onSortChange: (next: RiskSortState) => void;
  onRowClick?: (riskKey: string, label: string) => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());

  const filtered = useMemo(() => {
    const query = (search || '').trim().toLowerCase();
    return (rows || []).filter((g) => {
      const netMv = g.net_mv ?? 0;
      const passesNet = !nonZeroOnly || Math.abs(netMv) > 0;
      const matches = !query
        || g.risk_key.toLowerCase().includes(query)
        || (g.label ?? '').toLowerCase().includes(query);
      return passesNet && matches;
    });
  }, [rows, search, nonZeroOnly]);

  const toggleExpanded = (riskKey: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(riskKey)) next.delete(riskKey);
      else next.add(riskKey);
      return next;
    });
  };

  const sorted = useMemo(() => {
    const data = [...filtered];
    const cmp = (a: RiskGroup, b: RiskGroup) => {
      const col = sort.column;
      const dir = sort.direction === 'asc' ? 1 : -1;

      const valFor = (g: RiskGroup) => {
        switch (col) {
          case 'underlying':
            return (g.label || g.risk_key || '').toLowerCase();
          case 'sources':
            return (g.sources || []).join(',').toLowerCase();
          case 'net_qty':
            return g.net_qty ?? 0;
          case 'net_mv':
            return g.net_mv ?? 0;
          case 'long_mv':
            return g.long_mv ?? 0;
          case 'short_mv':
            return g.short_mv ?? 0;
          case 'gross_mv':
          default:
            return g.gross_mv ?? 0;
        }
      };

      const va = valFor(a);
      const vb = valFor(b);

      if (typeof va === 'number' && typeof vb === 'number') {
        if (va === vb) return 0;
        return va > vb ? dir : -dir;
      }

      return String(va).localeCompare(String(vb)) * dir;
    };

    data.sort((a, b) => {
      const primary = cmp(a, b);
      if (primary !== 0) return primary;
      // Secondary default tie-break: underlying asc
      return (a.label || a.risk_key || '').localeCompare(b.label || b.risk_key || '');
    });

    return data;
  }, [filtered, sort]);

  const cycleSort = (column: RiskSortColumn) => {
    const current = sort.column === column ? sort : null;
    const isNumeric = columnMeta[column].isNumeric;
    const defaultDir: RiskSortDirection = isNumeric ? 'desc' : 'asc';

    if (!current) {
      onSortChange({ column, direction: defaultDir });
      return;
    }

    if (current.direction === defaultDir) {
      onSortChange({ column, direction: current.direction === 'asc' ? 'desc' : 'asc' });
      return;
    }

    // Third click -> reset to default sort
    onSortChange(DEFAULT_SORT);
  };

  const headerCell = (col: RiskSortColumn) => {
    const meta = columnMeta[col];
    const isActive = sort.column === col;
    const dir = isActive ? sort.direction : undefined;
    const justify = meta.align === 'right' ? 'justify-end' : 'justify-start';
    return (
      <th
        key={col}
        className={cn(
          'px-4 py-2',
          meta.align === 'right' ? 'text-right' : 'text-left'
        )}
        style={{ minWidth: meta.minWidth }}
        scope="col"
      >
        <button
          type="button"
          onClick={() => cycleSort(col)}
          className={cn(
            'flex w-full items-center gap-2 text-xs font-semibold uppercase tracking-wide transition-colors',
            justify,
            isActive ? 'text-text-primary' : 'text-text-muted hover:text-text-primary'
          )}
          aria-sort={isActive ? (sort.direction === 'asc' ? 'ascending' : 'descending') : 'none'}
          aria-label={`Sort by ${meta.label}`}
        >
          <span>{meta.label}</span>
          {isActive && <span>{dir === 'asc' ? '▲' : '▼'}</span>}
        </button>
      </th>
    );
  };

  const netMvClass = (v: number | null | undefined) => {
    if (v === null || v === undefined || v === 0) return 'text-text-primary';
    if (v < 0) return 'text-rose-400';
    return 'text-text-primary';
  };

  const handleRowClick = (g: RiskGroup) => {
    onRowClick?.(g.risk_key, g.label ?? g.risk_key);
  };

  const renderTooltipContent = (g: RiskGroup) => {
    const gross = g.gross_mv ?? 0;
    const net = g.net_mv ?? 0;
    const hedge = (() => {
      if (!gross) return 0;
      const hr = g.hedge_ratio ?? (1 - Math.abs(net) / gross);
      if (!Number.isFinite(hr)) return 0;
      return Math.max(0, Math.min(1, hr));
    })();
    const pct = Math.round(hedge * 100);
    return `Net MV: ${formatMoney(net)} on ${formatMoney(gross)} gross (Hedge ${pct}%)`;
  };

  return (
    <TooltipProvider>
      <div className="overflow-x-auto">
        <table className="terminal-table min-w-full text-sm">
          <thead>
            <tr>
              {headerCell('underlying')}
              {headerCell('net_qty')}
              {headerCell('net_mv')}
              {headerCell('long_mv')}
              {headerCell('short_mv')}
              {headerCell('gross_mv')}
              {headerCell('sources')}
            </tr>
          </thead>
          <tbody>
            {sorted.length === 0 ? (
              <tr>
                <td className="px-4 py-3 text-center text-text-muted" colSpan={7}>No risk data yet.</td>
              </tr>
            ) : (
              sorted.map((g) => {
                const netMv = g.net_mv ?? 0;
                const netQty = g.net_qty ?? 0;
                const impliedMark = netQty ? Math.abs(netMv / netQty) : Math.abs(netMv);
                const shortAbs = Math.abs(g.short_mv ?? 0);
                const gross = g.gross_mv ?? 0;
                const sources = (breakdowns?.[g.risk_key] ?? []).slice().sort((a, b) => {
                  const amv = Math.abs(a.mv_raw ?? 0);
                  const bmv = Math.abs(b.mv_raw ?? 0);
                  if (amv === bmv) return String(a.venue).localeCompare(String(b.venue));
                  return bmv - amv;
                });
                const isExpanded = expanded.has(g.risk_key);

                return (
                  <Fragment key={g.risk_key}>
                    <tr className="border-t border-border/60 transition-colors hover:bg-bg-hover/60">
                      <td className="px-4 py-2 text-left font-medium text-text-primary">
                        <div className="flex items-start gap-2">
                          <button
                            type="button"
                            className="mt-0.5 inline-flex h-6 w-6 items-center justify-center rounded border border-border text-text-muted hover:bg-bg-hover hover:text-text-primary"
                            aria-label={isExpanded ? 'Collapse risk sources' : 'Expand risk sources'}
                            aria-expanded={isExpanded}
                            onClick={() => toggleExpanded(g.risk_key)}
                          >
                            {isExpanded ? '▾' : '▸'}
                          </button>
                          <button
                            type="button"
                            className="flex flex-col leading-tight text-left hover:underline"
                            onClick={() => handleRowClick(g)}
                            aria-label="Filter holdings by underlying"
                          >
                            <span>{g.label}</span>
                            <span className="text-xs text-text-muted">{g.risk_key}</span>
                          </button>
                        </div>
                      </td>
                      <td className="px-4 py-2 text-right tabular-nums text-text-primary">
                        {formatQty(g.risk_key, netQty, impliedMark)}
                      </td>
                      <td className="px-4 py-2 text-right tabular-nums">
                        <Tooltip content={renderTooltipContent(g)}>
                          <span className={netMvClass(netMv)}>
                            {netMv > 0 ? `+ ${formatMoney(netMv)}` : netMv < 0 ? `- ${formatMoney(Math.abs(netMv))}` : '$0.00'}
                          </span>
                        </Tooltip>
                      </td>
                      <td className="px-4 py-2 text-right tabular-nums text-text-primary">{formatMoneyNoSign(g.long_mv ?? 0)}</td>
                      <td className="px-4 py-2 text-right tabular-nums text-text-primary">{shortAbs ? `- ${formatMoneyNoSign(shortAbs)}` : '$0.00'}</td>
                      <td className="px-4 py-2 text-right tabular-nums font-semibold text-text-primary">{formatMoneyNoSign(Math.abs(gross))}</td>
                      <td className="px-4 py-2 text-right">
                        <div className="flex flex-wrap justify-end gap-1 text-xs text-text-muted">
                          {(g.sources || []).map((src) => (
                            <span key={src} className="rounded border border-border px-2 py-0.5">{src}</span>
                          ))}
                        </div>
                      </td>
                    </tr>
                    {isExpanded && (
                      <tr className="border-t border-border/60 bg-bg-surface/40">
                        <td colSpan={7} className="px-4 py-3">
                          {sources.length === 0 ? (
                            <div className="text-sm text-text-muted">No source breakdown available.</div>
                          ) : (
                            <div className="overflow-x-auto">
                              <table className="terminal-table w-max table-fixed text-xs" style={{ minWidth: 880 }}>
                                <colgroup>
                                  <col style={{ width: 140 }} />
                                  <col style={{ width: 260 }} />
                                  <col style={{ width: 130 }} />
                                  <col style={{ width: 130 }} />
                                  <col style={{ width: 110 }} />
                                  <col style={{ width: 110 }} />
                                </colgroup>
                                <thead>
                                  <tr>
                                    <th className="border-b border-border px-2 py-1 text-left text-text-muted whitespace-nowrap">Venue</th>
                                    <th className="border-b border-border px-2 py-1 text-left text-text-muted whitespace-nowrap">Instrument</th>
                                    <th className="border-b border-border px-2 py-1 text-right text-text-muted whitespace-nowrap">Qty</th>
                                    <th className="border-b border-border px-2 py-1 text-right text-text-muted whitespace-nowrap">MV</th>
                                    <th className="border-b border-border px-2 py-1 text-right text-text-muted whitespace-nowrap">Mark</th>
                                    <th className="border-b border-border px-2 py-1 text-right text-text-muted whitespace-nowrap">Time</th>
                                  </tr>
                                </thead>
                                <tbody>
                                  {sources.map((s, idx) => {
                                    const implied = (s.mark_raw ?? null) as number | null;
                                    return (
                                      <tr key={`${g.risk_key}:${s.venue}:${s.coin}:${idx}`} className="border-b border-border/60">
                                        <td className="px-2 py-1 text-left text-text-primary whitespace-nowrap">{s.venue}</td>
                                        <td className="px-2 py-1 text-left font-mono text-text-primary whitespace-nowrap">{s.coin}</td>
                                        <td className="px-2 py-1 text-right tabular-nums text-text-primary whitespace-nowrap">
                                          {formatQty(s.coin, s.qty_raw, implied)}
                                        </td>
                                        <td className="px-2 py-1 text-right tabular-nums text-text-primary whitespace-nowrap">
                                          {formatMoney(s.mv_raw)}
                                        </td>
                                        <td className="px-2 py-1 text-right tabular-nums text-text-primary whitespace-nowrap">
                                          {formatMark(s.coin, implied)}
                                        </td>
                                        <td className="px-2 py-1 text-right font-mono tabular-nums text-text-muted whitespace-nowrap">
                                          {s.time_display ?? '—'}
                                        </td>
                                      </tr>
                                    );
                                  })}
                                </tbody>
                              </table>
                            </div>
                          )}
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })
            )}
          </tbody>
        </table>
      </div>
    </TooltipProvider>
  );
}

RiskTable.DEFAULT_SORT = DEFAULT_SORT;
