export function TokenMMBalancesStatusStrip({
  source,
  degraded,
  degradedScopeCount,
}: {
  source: string | null;
  degraded: boolean;
  degradedScopeCount: number;
}) {
  const title = degraded || degradedScopeCount > 0
    ? 'Degraded shared snapshot'
    : source === 'portfolio_snapshot'
      ? 'Shared snapshot live'
      : 'Live balances';
  const detail = degradedScopeCount > 0
    ? `${degradedScopeCount} degraded scope${degradedScopeCount === 1 ? '' : 's'}`
    : source === 'portfolio_snapshot'
      ? 'Backend-authored snapshot and grouping are active'
      : 'Live per-strategy balances view';

  return (
    <div className="rounded border border-border bg-bg-base px-4 py-3">
      <div className="text-sm font-semibold text-text-primary">{title}</div>
      <div className="text-xs text-text-muted">{detail}</div>
    </div>
  );
}
