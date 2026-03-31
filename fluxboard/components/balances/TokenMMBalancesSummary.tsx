import type { TokenMMBalancesSummary as TokenMMBalancesSummaryData } from './tokenmmBalancesModel';
import { formatMoney } from '../../utils/balanceFormat';

export function TokenMMBalancesSummary({
  summary,
}: {
  summary: TokenMMBalancesSummaryData;
}) {
  const cards = [
    { label: 'Total Inventory', value: formatMoney(summary.totalMv) },
    { label: 'Stable Inventory', value: formatMoney(summary.stableMv) },
    { label: 'Trading Inventory', value: formatMoney(summary.nonStableMv) },
    { label: 'Non-zero Coins', value: String(summary.nonZeroCoinCount) },
    { label: 'Stale Rows', value: String(summary.staleRowCount) },
  ];

  return (
    <div className="grid gap-3 md:grid-cols-5">
      {cards.map((card) => (
        <div key={card.label} className="rounded border border-border bg-bg-base px-3 py-3">
          <div className="text-[11px] font-semibold uppercase tracking-wide text-text-muted">{card.label}</div>
          <div className="mt-1 text-base font-semibold text-text-primary">{card.value}</div>
        </div>
      ))}
    </div>
  );
}
