import type { TokenMMBalancesToolbarState, TokenMMBalancesVenueOption } from '../../types';

export function TokenMMBalancesToolbar({
  filters,
  venueOptions,
  allExpanded,
  onSearchChange,
  onVenueChange,
  onTypeChange,
  onHideZeroChange,
  onToggleExpandAll,
}: {
  filters: TokenMMBalancesToolbarState;
  venueOptions: TokenMMBalancesVenueOption[];
  allExpanded: boolean;
  onSearchChange: (value: string) => void;
  onVenueChange: (value: string) => void;
  onTypeChange: (value: TokenMMBalancesToolbarState['type']) => void;
  onHideZeroChange: (value: boolean) => void;
  onToggleExpandAll: () => void;
}) {
  return (
    <div className="flex flex-wrap items-end gap-3 rounded border border-border bg-bg-base px-4 py-3">
      <label className="flex min-w-[14rem] flex-col gap-1 text-xs text-text-muted">
        <span className="font-semibold uppercase tracking-wide">Search</span>
        <input
          aria-label="Search holdings"
          className="rounded border border-border bg-bg-surface px-3 py-2 text-sm text-text-primary"
          value={filters.search}
          onChange={(event) => onSearchChange(event.target.value)}
        />
      </label>
      <label className="flex min-w-[10rem] flex-col gap-1 text-xs text-text-muted">
        <span className="font-semibold uppercase tracking-wide">Venue</span>
        <select
          aria-label="Venue filter"
          className="rounded border border-border bg-bg-surface px-3 py-2 text-sm text-text-primary"
          value={filters.venue}
          onChange={(event) => onVenueChange(event.target.value)}
        >
          {venueOptions.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
      <label className="flex min-w-[10rem] flex-col gap-1 text-xs text-text-muted">
        <span className="font-semibold uppercase tracking-wide">Type</span>
        <select
          aria-label="Type filter"
          className="rounded border border-border bg-bg-surface px-3 py-2 text-sm text-text-primary"
          value={filters.type}
          onChange={(event) => onTypeChange(event.target.value as TokenMMBalancesToolbarState['type'])}
        >
          <option value="all">All types</option>
          <option value="spot">Spot</option>
          <option value="perp">Perp</option>
          <option value="cash">Cash</option>
        </select>
      </label>
      <label className="flex items-center gap-2 text-sm text-text-primary">
        <input
          aria-label="Hide zero balances"
          checked={filters.hideZero}
          type="checkbox"
          onChange={(event) => onHideZeroChange(event.target.checked)}
        />
        Hide zero
      </label>
      <button
        className="rounded border border-border bg-bg-surface px-3 py-2 text-sm font-semibold text-text-primary"
        type="button"
        onClick={onToggleExpandAll}
      >
        {allExpanded ? 'Collapse all' : 'Expand all'}
      </button>
    </div>
  );
}
