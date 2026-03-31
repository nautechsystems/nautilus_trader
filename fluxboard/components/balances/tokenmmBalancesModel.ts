import { STALE_THRESHOLDS } from '../../lib/tokens';
import type {
  BalanceChildRow,
  BalanceParentRow,
  BalancesPayload,
  TokenMMBalanceChildDisplayStatus,
  TokenMMBalanceDisplayStatus,
  TokenMMBalanceRowType,
  TokenMMBalanceSectionKey,
  TokenMMBalancesToolbarState,
  TokenMMBalancesVenueOption,
} from '../../types';
import { DUST_THRESHOLD } from '../../utils/balanceFormat';

export type TokenMMBalanceChildViewModel = {
  id: string;
  row: BalanceChildRow;
  type: TokenMMBalanceRowType;
  status: TokenMMBalanceChildDisplayStatus;
  primaryLabel: string;
  accountLabel: string | null;
  instrumentLabel: string | null;
  searchTokens: string;
};

export type TokenMMBalanceParentViewModel = {
  id: string;
  row: BalanceParentRow;
  coin: string;
  status: TokenMMBalanceDisplayStatus;
  netQty: number;
  netMv: number;
  mark: number | null;
  spotQty: number;
  perpQty: number;
  venueCount: number;
  venues: string[];
  freshestTs: number | null;
  children: TokenMMBalanceChildViewModel[];
};

export type TokenMMBalancesSummary = {
  totalMv: number;
  stableMv: number;
  nonStableMv: number;
  nonZeroCoinCount: number;
  staleRowCount: number;
};

export type TokenMMBalancesViewModel = {
  sections: Record<TokenMMBalanceSectionKey, TokenMMBalanceParentViewModel[]>;
  venueOptions: TokenMMBalancesVenueOption[];
  summary: TokenMMBalancesSummary;
  expandedParentIds: Set<string>;
};

function titleCase(value: string): string {
  if (!value) return '';
  return value.slice(0, 1).toUpperCase() + value.slice(1);
}

function coerceText(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

function toTimestampMs(value: string | null | undefined): number {
  if (!value) return 0;
  const timestamp = Date.parse(value);
  return Number.isFinite(timestamp) ? timestamp : 0;
}

function getGeneratedAtMs(payload: BalancesPayload): number {
  return toTimestampMs(payload.generated_at);
}

function getStaleAfterMs(payload: BalancesPayload): number {
  if (typeof payload.stale_after_ms === 'number' && Number.isFinite(payload.stale_after_ms) && payload.stale_after_ms > 0) {
    return payload.stale_after_ms;
  }
  return STALE_THRESHOLDS.SLOW;
}

function isScopeDegraded(payload: BalancesPayload): boolean {
  if (payload.degraded) {
    return true;
  }

  return (payload.scope_status ?? []).some((scope) => {
    const projection = scope.projection_status;
    if (!projection) {
      return false;
    }
    if (projection.healthy === false) {
      return true;
    }
    if (
      projection.last_attempt_ts_ms != null
      && projection.last_success_ts_ms != null
      && projection.stale_after_ms != null
      && projection.stale_after_ms > 0
    ) {
      return (projection.last_attempt_ts_ms - projection.last_success_ts_ms) > projection.stale_after_ms;
    }
    return false;
  });
}

function normalizeInstrumentFallback(row: BalanceChildRow): string {
  return coerceText(
    row.display_name_short
      ?? row.display_name_long
      ?? row.raw_symbol
      ?? row.instrument_id
      ?? row.contract
      ?? row.coin,
  );
}

function getNonEmptyText(...values: Array<string | null | undefined>): string | null {
  for (const value of values) {
    const text = coerceText(value);
    if (text) {
      return text;
    }
  }
  return null;
}

function getStatusRank(status: TokenMMBalanceDisplayStatus): number {
  switch (status) {
    case 'MISSING':
      return 0;
    case 'PARTIAL':
      return 1;
    case 'STALE':
      return 1;
    case 'OK':
    default:
      return 2;
  }
}

function getTypeRank(type: TokenMMBalanceRowType): number {
  switch (type) {
    case 'spot':
      return 0;
    case 'perp':
      return 1;
    case 'cash':
    default:
      return 2;
  }
}

function compareParents(left: TokenMMBalanceParentViewModel, right: TokenMMBalanceParentViewModel): number {
  const statusRankDiff = getStatusRank(left.status) - getStatusRank(right.status);
  if (statusRankDiff !== 0) {
    return statusRankDiff;
  }

  const absMvDiff = Math.abs(right.netMv) - Math.abs(left.netMv);
  if (absMvDiff !== 0) {
    return absMvDiff;
  }

  return left.coin.localeCompare(right.coin);
}

function compareChildren(left: TokenMMBalanceChildViewModel, right: TokenMMBalanceChildViewModel): number {
  const absMvDiff = Math.abs(right.row.mv_raw) - Math.abs(left.row.mv_raw);
  if (absMvDiff !== 0) {
    return absMvDiff;
  }

  const venueDiff = coerceText(left.row.venue).localeCompare(coerceText(right.row.venue));
  if (venueDiff !== 0) {
    return venueDiff;
  }

  return getTypeRank(left.type) - getTypeRank(right.type);
}

function isHiddenByZero(parent: BalanceParentRow): boolean {
  return Math.abs(parent.qty_raw) <= DUST_THRESHOLD && Math.abs(parent.mv_raw) <= DUST_THRESHOLD;
}

function matchesChildFilters(
  child: TokenMMBalanceChildViewModel,
  filters: TokenMMBalancesToolbarState,
): boolean {
  if (filters.venue !== 'all' && coerceText(child.row.venue).toLowerCase() !== filters.venue.toLowerCase()) {
    return false;
  }

  if (filters.type !== 'all' && child.type !== filters.type) {
    return false;
  }

  if (filters.search) {
    const search = filters.search.trim().toLowerCase();
    if (search && !child.searchTokens.includes(search)) {
      return false;
    }
  }

  return true;
}

function deriveChildStatus(
  child: BalanceChildRow,
  generatedAtMs: number,
  staleAfterMs: number,
): TokenMMBalanceChildDisplayStatus {
  if (!child.last_ts || child.last_ts <= 0) {
    return 'STALE';
  }

  if (generatedAtMs > 0 && staleAfterMs > 0 && (generatedAtMs - child.last_ts) > staleAfterMs) {
    return 'STALE';
  }

  return 'OK';
}

function deriveParentStatus(
  children: TokenMMBalanceChildViewModel[],
  scopeDegraded: boolean,
): TokenMMBalanceDisplayStatus {
  if (children.length === 0) {
    return 'MISSING';
  }

  const staleCount = children.filter((child) => child.status === 'STALE').length;
  if (staleCount === children.length) {
    return 'STALE';
  }

  if (staleCount > 0 || scopeDegraded) {
    return 'PARTIAL';
  }

  return 'OK';
}

export function classifyTokenMMBalanceRowType(row: BalanceChildRow): TokenMMBalanceRowType {
  const productType = coerceText(row.product_type).toLowerCase();
  const marketType = coerceText(row.market_type).toLowerCase();
  const contractType = coerceText(row.contract_type).toLowerCase();
  const venue = coerceText(row.venue).toLowerCase();

  if (
    productType === 'perp'
    || marketType === 'perp'
    || contractType === 'perp'
    || contractType === 'linear'
    || contractType === 'swap'
    || contractType === 'future'
  ) {
    return 'perp';
  }

  if (
    productType === 'spot'
    || marketType === 'spot'
  ) {
    return 'spot';
  }

  if (
    venue === 'wallet'
    || contractType === 'cash'
  ) {
    return 'cash';
  }

  return 'spot';
}

export function getTokenMMBalanceReadableLabel(row: BalanceChildRow): string {
  return normalizeInstrumentFallback(row);
}

export function buildTokenMMBalanceSearchTokens(row: BalanceChildRow): string {
  return [
    row.coin,
    row.venue,
    getTokenMMBalanceReadableLabel(row),
    row.display_name_long,
    row.wallet,
    row.label,
    row.address,
    row.raw_symbol,
    row.instrument_id,
    row.contract,
  ]
    .map((value) => coerceText(value))
    .filter(Boolean)
    .join(' ')
    .toLowerCase();
}

export function groupTokenMMBalanceSections(
  rows: BalanceParentRow[],
): Record<TokenMMBalanceSectionKey, BalanceParentRow[]> {
  return rows.reduce<Record<TokenMMBalanceSectionKey, BalanceParentRow[]>>(
    (acc, row) => {
      if (row.stable) {
        acc.stables.push(row);
      } else {
        acc.trading.push(row);
      }
      return acc;
    },
    { stables: [], trading: [] },
  );
}

export function deriveTokenMMVenueOptions(rows: BalanceParentRow[]): TokenMMBalancesVenueOption[] {
  const uniqueVenues = new Set<string>();

  rows.forEach((row) => {
    row.children.forEach((child) => {
      const venue = coerceText(child.venue).toLowerCase();
      if (venue) {
        uniqueVenues.add(venue);
      }
    });
  });

  return [
    { label: 'All venues', value: 'all' },
    ...Array.from(uniqueVenues)
      .sort((left, right) => left.localeCompare(right))
      .map((venue) => ({ label: titleCase(venue), value: venue })),
  ];
}

export function createDefaultTokenMMBalancesToolbarState(): TokenMMBalancesToolbarState {
  return {
    search: '',
    venue: 'all',
    type: 'all',
    hideZero: true,
  };
}

export function reconcileExpandedTokenMMBalanceIds(
  expandedParentIds: Iterable<string>,
  rows: BalanceParentRow[],
): Set<string> {
  const validIds = new Set(rows.map((row) => row.id));
  return new Set(Array.from(expandedParentIds).filter((id) => validIds.has(id)));
}

export function buildTokenMMBalancesViewModel(
  payload: BalancesPayload,
  filters: TokenMMBalancesToolbarState = createDefaultTokenMMBalancesToolbarState(),
  expandedParentIds: Iterable<string> = [],
): TokenMMBalancesViewModel {
  const generatedAtMs = getGeneratedAtMs(payload);
  const staleAfterMs = getStaleAfterMs(payload);
  const scopeDegraded = isScopeDegraded(payload);

  const parents = payload.rows.reduce<TokenMMBalanceParentViewModel[]>((acc, parent) => {
    if (filters.hideZero && isHiddenByZero(parent)) {
      return acc;
    }

    const children = parent.children
      .map<TokenMMBalanceChildViewModel>((child) => ({
        id: child.id,
        row: child,
        type: classifyTokenMMBalanceRowType(child),
        status: deriveChildStatus(child, generatedAtMs, staleAfterMs),
        primaryLabel: getTokenMMBalanceReadableLabel(child),
        accountLabel: getNonEmptyText(child.wallet, child.label, child.address),
        instrumentLabel: getNonEmptyText(child.raw_symbol, child.instrument_id, child.contract),
        searchTokens: buildTokenMMBalanceSearchTokens(child),
      }))
      .filter((child) => matchesChildFilters(child, filters))
      .sort(compareChildren);

    if (children.length === 0) {
      return acc;
    }

    const venues = Array.from(
      new Set(children.map((child) => coerceText(child.row.venue)).filter(Boolean)),
    ).sort((left, right) => left.localeCompare(right));

    const freshestTs = children.reduce<number | null>((latest, child) => {
      const lastTs = child.row.last_ts ?? null;
      if (lastTs == null) {
        return latest;
      }
      return latest == null || lastTs > latest ? lastTs : latest;
    }, null);

    const parentViewModel: TokenMMBalanceParentViewModel = {
      id: parent.id,
      row: parent,
      coin: parent.canonical,
      status: deriveParentStatus(children, scopeDegraded),
      netQty: parent.qty_raw,
      netMv: parent.mv_raw,
      mark: parent.mark_raw,
      spotQty: children
        .filter((child) => child.type === 'spot' || child.type === 'cash')
        .reduce((sum, child) => sum + child.row.qty_raw, 0),
      perpQty: children
        .filter((child) => child.type === 'perp')
        .reduce((sum, child) => sum + child.row.qty_raw, 0),
      venueCount: venues.length,
      venues,
      freshestTs,
      children,
    };

    acc.push(parentViewModel);
    return acc;
  }, []);

  const sections = parents.reduce<Record<TokenMMBalanceSectionKey, TokenMMBalanceParentViewModel[]>>(
    (acc, parent) => {
      if (parent.row.stable) {
        acc.stables.push(parent);
      } else {
        acc.trading.push(parent);
      }
      return acc;
    },
    { stables: [], trading: [] },
  );

  sections.stables.sort(compareParents);
  sections.trading.sort(compareParents);

  const visibleParents = [...sections.stables, ...sections.trading];
  const summary: TokenMMBalancesSummary = {
    totalMv: visibleParents.reduce((sum, row) => sum + row.netMv, 0),
    stableMv: sections.stables.reduce((sum, row) => sum + row.netMv, 0),
    nonStableMv: sections.trading.reduce((sum, row) => sum + row.netMv, 0),
    nonZeroCoinCount: visibleParents.filter((row) => Math.abs(row.netMv) > DUST_THRESHOLD || Math.abs(row.netQty) > DUST_THRESHOLD).length,
    staleRowCount: visibleParents.filter((row) => row.status !== 'OK').length,
  };

  return {
    sections,
    venueOptions: deriveTokenMMVenueOptions(payload.rows),
    summary,
    expandedParentIds: reconcileExpandedTokenMMBalanceIds(expandedParentIds, visibleParents.map((row) => row.row)),
  };
}
