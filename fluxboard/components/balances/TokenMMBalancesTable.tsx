import { Fragment } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';

import type { TokenMMBalanceParentViewModel } from './tokenmmBalancesModel';
import { formatMoney } from '../../utils/balanceFormat';
import { formatMark, formatQty } from '../../lib/assetFormat';
import { formatAbsoluteTime } from '../../utils/time';
import { BalanceStatusBadge } from './BalanceStatusBadge';

function titleCase(value: string) {
  return value.slice(0, 1).toUpperCase() + value.slice(1);
}

export function TokenMMBalancesTable({
  sectionTitle,
  rows,
  expandedParentIds,
  onToggleExpanded,
}: {
  sectionTitle: string;
  rows: TokenMMBalanceParentViewModel[];
  expandedParentIds: Set<string>;
  onToggleExpanded: (id: string) => void;
}) {
  return (
    <section className="rounded border border-border bg-bg-base">
      <div className="border-b border-border px-4 py-3 text-sm font-semibold text-text-primary">
        {sectionTitle}
      </div>
      <table className="min-w-full text-sm">
        <thead>
          <tr className="border-b border-border text-xs uppercase tracking-wide text-text-muted">
            <th className="px-4 py-2 text-left">Coin</th>
            <th className="px-4 py-2 text-right">Net Qty</th>
            <th className="px-4 py-2 text-right">Net MV</th>
            <th className="px-4 py-2 text-right">Mark</th>
            <th className="px-4 py-2 text-right">Spot Qty</th>
            <th className="px-4 py-2 text-right">Perp Qty</th>
            <th className="px-4 py-2 text-left">Venues</th>
            <th className="px-4 py-2 text-left">Updated</th>
            <th className="px-4 py-2 text-left">Status</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const expanded = expandedParentIds.has(row.id);
            return (
              <Fragment key={row.id}>
                <tr key={row.id} className="border-b border-border/60">
                  <td className="px-4 py-3">
                    <button
                      className="inline-flex items-center gap-2 font-semibold text-text-primary"
                      type="button"
                      onClick={() => onToggleExpanded(row.id)}
                    >
                      {expanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
                      {row.coin}
                    </button>
                  </td>
                  <td className="px-4 py-3 text-right text-text-primary">
                    {formatQty(row.coin, row.netQty, row.mark)}
                  </td>
                  <td className="px-4 py-3 text-right text-text-primary">{formatMoney(row.netMv)}</td>
                  <td className="px-4 py-3 text-right text-text-primary">{formatMark(row.coin, row.mark)}</td>
                  <td className="px-4 py-3 text-right text-text-primary">{formatQty(row.coin, row.spotQty, row.mark)}</td>
                  <td className="px-4 py-3 text-right text-text-primary">{formatQty(row.coin, row.perpQty, row.mark)}</td>
                  <td className="px-4 py-3 text-text-primary">{row.venues.join(', ')}</td>
                  <td className="px-4 py-3 text-text-muted">{formatAbsoluteTime(row.freshestTs)}</td>
                  <td className="px-4 py-3"><BalanceStatusBadge status={row.status} /></td>
                </tr>
                {expanded ? (
                  <>
                    <tr className="border-b border-border/60 bg-bg-surface text-xs uppercase tracking-wide text-text-muted">
                      <th className="px-4 py-2 text-left">Venue</th>
                      <th className="px-4 py-2 text-left">Account</th>
                      <th className="px-4 py-2 text-left">Type</th>
                      <th className="px-4 py-2 text-left">Symbol / Instrument</th>
                      <th className="px-4 py-2 text-right">Qty</th>
                      <th className="px-4 py-2 text-right">MV</th>
                      <th className="px-4 py-2 text-right">Mark</th>
                      <th className="px-4 py-2 text-left">Updated</th>
                      <th className="px-4 py-2 text-left">Status</th>
                    </tr>
                    {row.children.map((child) => (
                      <tr key={child.id} className="border-b border-border/40 bg-bg-surface/70">
                        <td className="px-4 py-2 text-text-primary">{titleCase(child.row.venue ?? '')}</td>
                        <td className="px-4 py-2 text-text-primary">{child.accountLabel ?? '—'}</td>
                        <td className="px-4 py-2 text-text-primary">{titleCase(child.type)}</td>
                        <td className="px-4 py-2 text-text-primary">
                          <div className="font-medium">{child.primaryLabel}</div>
                          <div className="text-xs text-text-muted">{child.instrumentLabel ?? '—'}</div>
                        </td>
                        <td className="px-4 py-2 text-right text-text-primary">
                          {formatQty(child.row.coin, child.row.qty_raw, child.row.mark_raw)}
                        </td>
                        <td className="px-4 py-2 text-right text-text-primary">{formatMoney(child.row.mv_raw)}</td>
                        <td className="px-4 py-2 text-right text-text-primary">{formatMark(child.row.coin, child.row.mark_raw)}</td>
                        <td className="px-4 py-2 text-text-muted">{formatAbsoluteTime(child.row.last_ts)}</td>
                        <td className="px-4 py-2"><BalanceStatusBadge status={child.status} /></td>
                      </tr>
                    ))}
                  </>
                ) : null}
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </section>
  );
}
