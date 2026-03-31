import type { TokenMMBalanceDisplayStatus } from '../../types';

const STATUS_CLASS: Record<TokenMMBalanceDisplayStatus, string> = {
  OK: 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200',
  STALE: 'border-amber-500/30 bg-amber-500/10 text-amber-100',
  PARTIAL: 'border-orange-500/30 bg-orange-500/10 text-orange-100',
  MISSING: 'border-rose-500/30 bg-rose-500/10 text-rose-100',
};

export function BalanceStatusBadge({ status }: { status: TokenMMBalanceDisplayStatus }) {
  return (
    <span className={`inline-flex rounded-full border px-2 py-0.5 text-[11px] font-semibold uppercase tracking-wide ${STATUS_CLASS[status]}`}>
      {status}
    </span>
  );
}
