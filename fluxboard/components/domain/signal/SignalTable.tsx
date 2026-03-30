/**
 * SignalTable Component (DataTable-based)
 *
 * Strategy health and edge monitoring table using DataTable component.
 * Features:
 * - 2-line row layout (strategy header + leg details)
 * - Edge color coding (green/yellow/red)
 * - ON/OFF status badges
 * - Real-time WebSocket updates
 * - Age calculation with staleness indicator
 * - Sortable columns
 * - Filter support
 *
 * Performance optimizations:
 * - useMemo for column definitions and data enrichment
 * - memo for LegCell component
 * - Incremental WebSocket updates via mergeStrategy
 * - Shared viewport age ticking (1s interval)
 * - Invalidate-only recovery instead of steady-state polling overlap
 *
 * Fixes & Improvements:
 * - Safe paramTooltip with optional chaining (handles undefined/null params)
 * - Deep merge for signal_delta legs (prevents leg drops on partial updates)
 * - Numeric sorting for Last Updated column (uses _lastUpdateMs)
 * - Proper fallbacks in LegCell (shows "—" instead of 0 for missing prices)
 * - Correct 0 value handling in balance tooltips (0 ≠ null/undefined)
 * - Tooltip newline rendering via SimpleTooltip with whitespace-pre-wrap
 * - Removed dead code (parseAge, getEdgeColor v1)
 */

import { useEffect, useState, useMemo, memo, useRef, useCallback, type FC } from 'react';
import { type ColumnDef, type SortingFn, type SortingState } from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { isEqual } from 'lodash-es';
import { useLocation } from 'react-router-dom';
import { api } from '@/api';
import { ChevronDown, Info } from 'lucide-react';
import { useSignalStore, selectSignalRows } from '@/stores';
import shallow from 'zustand/shallow';
import { socket } from '@/sockets';
import { fmtPriceSignal, fmtPriceTooltip, fmtAgeSec } from '@/utils';
import { computeStrategyAge } from '@/utils/age';
import { buildLegDeltaPatch, getLegForSlot, getOrderedLegEntries, resolveRoleSlot } from '@/utils/signalLegs';
import { formatAbsoluteTime, formatLocal } from '@/utils/time';
import { useVisibleNowMs } from './useVisibleNowMs';
import MakerV4SignalTable from './MakerV4SignalTable';
import EquitiesArbSignalTable from './EquitiesArbSignalTable';
import type {
  BalanceSummary,
  RealtimeSnapshotLineage,
  MakerRoleMap,
  MakerV2QuoteSnapshot,
  PricingAdjustment,
  SignalLeg,
  SignalStrategy,
  StrategyStatus
} from '@/types';
import { DataTable } from '@/components/ui/table/DataTable';
import { PanelHeader } from '@/components/shared/PanelHeader';
import { PanelBody } from '@/components/shared/PanelBody';
import { TableFilter, applyFilters, type FilterValues, type ColumnFilter } from '@/components/shared/TableFilter';
import { SimpleTooltip } from '@/components/ui/tooltip/Tooltip';
import { StatusPill } from '@/components/shared/StatusPill';
import { Badge, type BadgeVariant } from '@/components/ui/badge';
import { colors, spacing, typography, STALE_THRESHOLDS } from '@/lib/tokens';
import { cn } from '@/lib/utils';
import { useMobileLayout } from '@/hooks/useMobileLayout';
import { useStandardWebSocketSubscription } from '@/hooks/useWebSocket';
import {
  createRealtimeSurfaceController,
  useRealtimeSurfaceController,
  type RealtimeRowDelta,
} from '@/hooks/useRealtimeSurfaceController';
import { useRecoveryScheduler } from '@/hooks/useRecoveryScheduler';
import { deriveStrategyProfile } from '@/config/paramsProfiles';
import { isRealtimeStandardEnabled } from '@/config/featureFlags';
import { resolvePathProfile, type PathProfile } from '@/config/uiProfiles';
import { EMPTY_SNAPSHOT_HOLD_MS, evaluateEmptySnapshotPolicy } from './emptySnapshotPolicy';
import { RealtimeSurfaceState } from '@/lib/realtime/types';
import {
  deriveStrategyStatus,
  describeTradingStatus,
  statusToFilterValue,
  TRADING_FILTER_VALUES,
  type TradingFilterValue,
  type TradingFlagInput,
} from '@/utils/strategyStatus';
import { resolveSignalRunning } from '@/utils/signalRunState';

// =============================================================================
// TYPES
// =============================================================================

type EnrichedRow = SignalStrategy & {
  _strategyFamily: SignalStrategyFamily;
  status: StrategyStatus;
  _netEdge: number;
  _edge2: number | null;
  _spreadNet: number | null;
  _globalQty: number | null;
  _localQty: number | null;
  _riskDelta: number | null;
  _maxAge: number;  // staleness (oldest leg)
  _minAge: number; // recency (newest leg)
  _lastUpdate?: string;
  _lastUpdateMs?: number;
  _legAAge?: number;
  _legBAge?: number;
  _recentSide?: 'A' | 'B';
  trading_enabled: TradingFilterValue;
  exchange: string;  // Combined exchanges from legs
  coin: string;  // Combined coins from legs
  market_type: string;
  asset: string;
  maker_venue: string;
  maker_market: string;
  reference_venue: string;
  reference_market: string;
  strategy_class: string;
  // Flattened classification metadata for filtering/grouping
  class?: string;
  venue_prefix?: string;
  chain?: string;
};

type SignalStrategyFamily =
  | 'maker_v4'
  | 'maker_v3'
  | 'maker_v2'
  | 'taker'
  | 'equities_maker'
  | 'equities_taker';
type SignalFamilyScope = 'all' | SignalStrategyFamily;
type SignalLegResolutionRow = Pick<
  SignalStrategy,
  'legs' | 'legs_order' | 'maker_role_map' | 'maker_v2' | 'maker_v3'
>;

function resolveQuoteSnapshot(row: Pick<SignalStrategy, 'maker_v2' | 'maker_v3'>): MakerV2QuoteSnapshot | undefined {
  return row.maker_v2?.quote_snapshot ?? row.maker_v3?.quote_snapshot;
}

function buildEnrichedSignalRow(row: SignalStrategy, serverNowMs: number): EnrichedRow | null {
  if (!row.legs) {
    return null;
  }

  const status = deriveStrategyStatus({
    running: resolveSignalRunning(row, serverNowMs),
    trading: resolveTradingValue(row as any),
    blocked: resolveTradingBlocked(row as any),
  });
  const tradingFilter = statusToFilterValue(status);
  const orderedLegs = getOrderedLegEntries(row);
  const legA = resolveDisplayedLeg(row, 'A');
  const legB = resolveDisplayedLeg(row, 'B');
  const fallbackEdgeLeg = orderedLegs.find((entry) => {
    const value = entry.leg?.net_edge_bps;
    return typeof value === 'number' && Number.isFinite(value);
  })?.leg;
  const netEdge =
    row.decision_edge_bps
    ?? fallbackEdgeLeg?.net_edge_bps
    ?? legA?.net_edge_bps
    ?? legB?.net_edge_bps
    ?? 0;
  const edge2 = row.edge2_bps ?? null;
  const spreadNet = spreadMarketVsFvBps(row);
  const riskDelta = coerceFiniteNumber(row.risk_delta) ?? null;
  const { globalQty, localQty } = resolveInventoryQuantities(row);
  const { makerLeg, referenceLeg } = resolveMakerV3RoleLegs(row);

  const exchanges = orderedLegs
    .map((entry) => entry.leg?.exchange)
    .filter((value): value is string => typeof value === 'string' && value.length > 0)
    .join(' ');
  const coins = orderedLegs
    .map((entry) => getLegUnderlying(entry.leg))
    .filter((value): value is string => typeof value === 'string' && value.length > 0)
    .join(' ');
  const marketTypes = orderedLegs
    .map((entry) => getLegMarketType(entry.leg))
    .filter((value): value is string => typeof value === 'string' && value.length > 0)
    .join(' ');
  const ageData = computeStrategyAge(row, serverNowMs);
  const lastUpdate = orderedLegs
    .map((entry) => entry.leg?.update_time)
    .find((value): value is string => typeof value === 'string' && value.length > 0);

  const meta = row.meta || {};
  const strategyClass = normalizeFacetValue(meta.class);
  const asset = normalizeAssetFacet(meta.base_asset) || normalizeAssetFacet(getLegUnderlying(makerLeg)) || normalizeAssetFacet(getLegUnderlying(referenceLeg));
  const makerVenue = normalizeFacetValue(makerLeg?.exchange ?? resolveQuoteSnapshot(row)?.maker_exchange);
  const makerMarket = normalizeFacetValue(getLegMarketType(makerLeg));
  const referenceVenue = normalizeFacetValue(referenceLeg?.exchange ?? resolveQuoteSnapshot(row)?.ref_exchange);
  const referenceMarket = normalizeFacetValue(getLegMarketType(referenceLeg));
  const normalizedChain = normalizeFacetValue(meta.chain);

  return {
    ...row,
    _strategyFamily: deriveStrategyFamily(row),
    status,
    _netEdge: netEdge,
    _edge2: typeof edge2 === 'number' ? edge2 : null,
    _spreadNet: typeof spreadNet === 'number' ? spreadNet : null,
    _globalQty: typeof globalQty === 'number' ? globalQty : null,
    _localQty: typeof localQty === 'number' ? localQty : null,
    _riskDelta: typeof riskDelta === 'number' ? riskDelta : null,
    _maxAge: ageData.displayAgeMs,
    _minAge: ageData.recentAgeMs ?? 0,
    _lastUpdate: lastUpdate,
    _lastUpdateMs: ageData.mostRecentTsMs,
    _legAAge: ageData.perLeg.A?.ageMs,
    _legBAge: ageData.perLeg.B?.ageMs,
    _recentSide: ageData.mostRecentSide,
    trading_enabled: tradingFilter,
    exchange: exchanges,
    coin: coins,
    market_type: marketTypes,
    asset,
    maker_venue: makerVenue,
    maker_market: makerMarket,
    reference_venue: referenceVenue,
    reference_market: referenceMarket,
    strategy_class: strategyClass,
    class: meta.class,
    venue_prefix: meta.venue_prefix,
    chain: normalizedChain,
  };
}

function resolveSignalDataFreshnessTsMs(
  rows: readonly SignalStrategy[],
  fallbackTsMs: number,
): number {
  let freshestTsMs: number | null = null;

  for (const row of rows) {
    const candidate = computeStrategyAge(row, fallbackTsMs).mostRecentTsMs;
    if (typeof candidate !== 'number' || !Number.isFinite(candidate)) {
      continue;
    }
    freshestTsMs = freshestTsMs == null ? candidate : Math.max(freshestTsMs, candidate);
  }

  return freshestTsMs ?? fallbackTsMs;
}

const tradingSortingFn: SortingFn<EnrichedRow> = (rowA, rowB, columnId) => {
  const a = TRADING_SORT_ORDER[rowA.getValue(columnId) as TradingFilterValue] ?? -1;
  const b = TRADING_SORT_ORDER[rowB.getValue(columnId) as TradingFilterValue] ?? -1;
  return a - b;
};

// =============================================================================
// CONSTANTS
// =============================================================================

type BalanceStatus = 'OK' | 'WARN' | 'FAIL' | 'UNKNOWN';

const GENERIC_SIGNAL_FILTERS: ColumnFilter[] = [
  { key: 'id', label: 'Strategy', type: 'text', placeholder: 'Strategy ID...' },
  { key: 'trading_enabled', label: 'Trading', type: 'select', options: TRADING_FILTER_VALUES },
  { key: 'exchange', label: 'Exchange', type: 'text', placeholder: 'bybit, rooster...' },
  { key: 'coin', label: 'Coin', type: 'text', placeholder: 'BTC, ETH...' },
  { key: 'market_type', label: 'Market', type: 'select', options: ['spot', 'perp'] },
  { key: 'class', label: 'Class', type: 'select', options: ['dex_cex_arb', 'equity_perp_arb'] },
  {
    key: 'venue_prefix',
    label: 'Venue',
    type: 'select',
    options: ['rooster_bybit', 'sailor_bybit', 'pcsbnb_bybit', 'tron_sunswap_v2_bybit', 'tron_sunswap_v3_bybit', 'hl_futu', 'hl_ibkr'],
  },
  { key: 'chain', label: 'Chain', type: 'select', options: ['plume', 'sei', 'bnb', 'tron', 'equities'] },
];

const MAKER_SUITE_SIGNAL_PROFILES = new Set<PathProfile>(['tokenmm', 'equities']);

const TRADING_SORT_ORDER: Record<TradingFilterValue, number> = {
  Enabled: 2,
  Pending: 1,
  Paused: 0,
};

const BALANCE_STATUS_ORDER: BalanceStatus[] = ['OK', 'WARN', 'FAIL', 'UNKNOWN'];

const BALANCE_STATUS_META: Record<BalanceStatus, { label: string; variant: BadgeVariant; dotClass: string }> = {
  OK: {
    label: 'Ready',
    variant: 'success',
    dotClass: 'bg-success-DEFAULT',
  },
  WARN: {
    label: 'Tight',
    variant: 'warning',
    dotClass: 'bg-warning-DEFAULT',
  },
  FAIL: {
    label: 'Insufficient',
    variant: 'danger',
    dotClass: 'bg-danger-DEFAULT',
  },
  UNKNOWN: {
    label: 'Unknown',
    variant: 'neutral',
    dotClass: 'bg-text-tertiary',
  },
};

function resolveSignalSurfaceStatus(
  state: RealtimeSurfaceState,
  wsConnected: boolean,
  recoveryPending: boolean,
): {
  label: string;
  dotClass: string;
  tooltip: string;
} {
  switch (state) {
    case RealtimeSurfaceState.MANUAL_REFRESH_REQUIRED:
      return {
        label: 'Refresh required',
        dotClass: 'bg-danger-light',
        tooltip: 'Standard signal transport failed closed. Use Refresh after the rollout issue is resolved.',
      };
    case RealtimeSurfaceState.RECOVERING:
      return {
        label: wsConnected ? 'Recovering' : 'Recovering',
        dotClass: 'bg-warning-light',
        tooltip: 'Signal recovery uses one-shot invalidate snapshots instead of steady-state polling.',
      };
    case RealtimeSurfaceState.STALE:
      return {
        label: 'Stale',
        dotClass: 'bg-danger-light',
        tooltip: 'Signal data stopped advancing. Fluxboard is waiting for a fresh recovery snapshot.',
      };
    case RealtimeSurfaceState.LAGGING:
      return {
        label: 'Lagging',
        dotClass: 'bg-warning-light',
        tooltip: 'Signal data is aging beyond the expected live-update budget.',
      };
    case RealtimeSurfaceState.SYNCING:
      return {
        label: 'Syncing',
        dotClass: 'bg-warning-light',
        tooltip: 'Signal is fetching a fresh snapshot before the standard transport resumes.',
      };
    default:
      return {
        label: recoveryPending ? 'Live + Recovering' : 'Live (WS)',
        dotClass: wsConnected ? 'bg-success-light' : 'bg-warning-light',
        tooltip: wsConnected
          ? (
            recoveryPending
              ? 'WebSocket connected. A one-shot recovery snapshot is scheduled from an invalidate or reconnect event.'
              : 'WebSocket connected. Steady state stays on the realtime controller until an invalidate or reconnect event schedules recovery.'
          )
          : 'WebSocket disconnected. Recovery uses scheduled invalidate snapshots instead of steady-state polling.',
      };
  }
}

function resolveSignalFreshnessState(
  lastDataTsMs: number | null | undefined,
  nowMs: number,
  recoveryPending: boolean,
): RealtimeSurfaceState | null {
  if (!lastDataTsMs || !Number.isFinite(lastDataTsMs)) {
    return null;
  }

  const ageMs = Math.max(0, nowMs - lastDataTsMs);
  if (ageMs > STALE_THRESHOLDS.NORMAL) {
    return recoveryPending ? RealtimeSurfaceState.RECOVERING : RealtimeSurfaceState.STALE;
  }
  if (ageMs > STALE_THRESHOLDS.FAST) {
    return RealtimeSurfaceState.LAGGING;
  }
  return RealtimeSurfaceState.LIVE;
}

function resolveSignalTransportState(
  lastActivityTsMs: number | null | undefined,
  nowMs: number,
  recoveryPending: boolean,
): RealtimeSurfaceState | null {
  if (!lastActivityTsMs || !Number.isFinite(lastActivityTsMs)) {
    return null;
  }

  const ageMs = Math.max(0, nowMs - lastActivityTsMs);
  if (ageMs > STALE_THRESHOLDS.NORMAL) {
    return recoveryPending ? RealtimeSurfaceState.RECOVERING : RealtimeSurfaceState.STALE;
  }
  if (ageMs > STALE_THRESHOLDS.FAST) {
    return RealtimeSurfaceState.LAGGING;
  }
  return RealtimeSurfaceState.LIVE;
}

const ColumnHeaderWithTooltip: FC<{ label: string; tooltip: string }> = ({ label, tooltip }) => {
  return (
    <span className="inline-flex items-center gap-1">
      <span className="text-xs uppercase tracking-wide text-neutral-300">{label}</span>
      <SimpleTooltip content={tooltip} delay={250}>
        {/* Stop propagation so clicking the info icon doesn't toggle sorting. */}
        <button
          type="button"
          className="inline-flex items-center justify-center p-0.5 text-neutral-400 hover:text-neutral-200"
          onClick={(e) => e.stopPropagation()}
          onMouseDown={(e) => e.stopPropagation()}
          aria-label={`Help: ${label}`}
        >
          <Info className="h-3 w-3" />
        </button>
      </SimpleTooltip>
    </span>
  );
};

function parseStrategyGroups(rawGroups: unknown): Set<PathProfile> {
  if (rawGroups == null) return new Set();
  const values = String(rawGroups)
    .split(',')
    .map((item) => resolvePathProfile(item.trim().toLowerCase()))
    .filter((item) => item !== 'default');
  return new Set(values);
}

const MAKER_V3_CLASSES = new Set(['maker_v3', 'maker_v3_dual_cex', 'equity_perp_maker_v3']);
const MAKER_V4_CLASSES = new Set(['maker_v4', 'equity_perp_maker_v4']);
const MAKER_V2_CLASSES = new Set(['maker_v2', 'crypto_spot_perp_maker', 'equity_perp_maker']);
const EQUITIES_MAKER_CLASSES = new Set(['equities_maker']);
const EQUITIES_TAKER_CLASSES = new Set(['equities_taker']);

function deriveStrategyFamily(strategy: Pick<SignalStrategy, 'strategy_family' | 'meta'>): SignalStrategyFamily {
  const explicit = String(strategy.strategy_family || '').trim().toLowerCase();
  if (
    explicit === 'maker_v4'
    || explicit === 'maker_v3'
    || explicit === 'maker_v2'
    || explicit === 'taker'
    || explicit === 'equities_maker'
    || explicit === 'equities_taker'
  ) {
    return explicit;
  }
  const metaFamily = String(strategy.meta?.strategy_family || '').trim().toLowerCase();
  const metaVersion = String(strategy.meta?.strategy_version || '').trim().toLowerCase();
  if (
    metaFamily === 'maker_v4'
    || metaFamily === 'maker_v3'
    || metaFamily === 'maker_v2'
    || metaFamily === 'taker'
    || metaFamily === 'equities_maker'
    || metaFamily === 'equities_taker'
  ) {
    return metaFamily;
  }
  if (metaFamily === 'maker' && metaVersion === 'v4') return 'maker_v4';
  if (metaFamily === 'maker' && metaVersion === 'v3') return 'maker_v3';
  if (metaFamily === 'maker' && metaVersion === 'v2') return 'maker_v2';
  const cls = String(strategy.meta?.class || '').trim().toLowerCase();
  if (EQUITIES_MAKER_CLASSES.has(cls)) return 'equities_maker';
  if (EQUITIES_TAKER_CLASSES.has(cls)) return 'equities_taker';
  if (MAKER_V4_CLASSES.has(cls)) return 'maker_v4';
  if (MAKER_V3_CLASSES.has(cls)) return 'maker_v3';
  if (MAKER_V2_CLASSES.has(cls)) return 'maker_v2';
  const profile = deriveStrategyProfile({ meta: strategy.meta });
  if (profile === 'equities_maker') return 'equities_maker';
  if (profile === 'equities_taker') return 'equities_taker';
  if (profile === 'maker_v4') return 'maker_v4';
  if (profile === 'maker_v3') return 'maker_v3';
  if (profile === 'maker_v2') return 'maker_v2';
  return 'taker';
}

function normalizeSignalFamilyForPath(
  family: SignalStrategyFamily,
  pathProfile: PathProfile,
): SignalStrategyFamily {
  if (pathProfile === 'equities' && family === 'maker_v4') {
    return 'equities_maker';
  }
  return family;
}

function resolveSignalFamilyForPath(
  pathProfile: PathProfile,
  strategy: Pick<SignalStrategy, 'strategy_family' | 'meta'>,
): SignalStrategyFamily {
  return normalizeSignalFamilyForPath(deriveStrategyFamily(strategy), pathProfile);
}

function matchesSignalProfile(
  profile: PathProfile,
  strategy: Pick<SignalStrategy, 'strategy_family' | 'meta' | 'params' | 'hot_params' | 'equities_arb'>
): boolean {
  if (profile === 'default') return true;

  const groups = parseStrategyGroups(strategy.meta?.strategy_groups);
  if (profile === 'equities') {
    const inEquitiesProfile = groups.size > 0
      ? groups.has(profile)
      : String(strategy.meta?.chain || '').trim().toLowerCase() === 'equities';
    if (!inEquitiesProfile) return false;

    const family = deriveStrategyFamily(strategy);
    return Boolean(strategy.equities_arb) && (family === 'equities_maker' || family === 'equities_taker');
  }

  if (groups.size > 0) return groups.has(profile);
  return false;
}

function defaultFamilyScopeForProfile(profile: PathProfile): SignalFamilyScope {
  return profile === 'tokenmm' ? 'maker_v3' : 'all';
}

function isMakerSuiteSignalProfile(profile: PathProfile): boolean {
  return MAKER_SUITE_SIGNAL_PROFILES.has(profile);
}

function getLegDisplayLabel(leg: SignalLeg | null | undefined): string {
  if (!leg) return 'N/A';
  return String(
    leg.display_name_long
      ?? ([leg.exchange, leg.display_name_short ?? leg.coin].filter(Boolean).join(' ')),
  ).trim() || 'N/A';
}

function getConfiguredSignalSourceLabel(leg: SignalLeg | null | undefined): string | null {
  if (!leg) return null;
  const explicitRoute = String(leg.route ?? '').trim();
  if (explicitRoute) return explicitRoute.toUpperCase();
  const instrumentId = String(leg.instrument_id ?? '').trim();
  if (!instrumentId.includes('.')) return null;
  const source = instrumentId.split('.').at(-1)?.trim();
  if (!source) return null;
  return source.toUpperCase();
}

function getLegUnderlying(leg: SignalLeg | null | undefined): string {
  return String(leg?.inventory_asset ?? leg?.base_asset ?? leg?.coin ?? '').trim();
}

function getLegMarketType(leg: SignalLeg | null | undefined): string {
  return String(leg?.product_type ?? leg?.market_type ?? '').trim().toLowerCase();
}

function normalizeFacetValue(value: unknown): string {
  return String(value ?? '').trim().toLowerCase();
}

function normalizeAssetFacet(value: unknown): string {
  return String(value ?? '').trim().toUpperCase();
}

function uniqueFilterOptions(values: Array<string | null | undefined>): string[] {
  const normalized = values
    .map((value) => String(value ?? '').trim())
    .filter((value) => value.length > 0);
  return Array.from(new Set(normalized)).sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' }));
}

function legMatchesSnapshotHint(
  leg: SignalLeg | null | undefined,
  {
    exchange,
    symbol,
  }: {
    exchange: string;
    symbol: string;
  },
): boolean {
  if (!leg) return false;
  const legSymbol = (leg as SignalLeg & { symbol?: string }).symbol;
  const legExchange = normalizeFacetValue(leg.exchange);
  if (exchange && legExchange !== exchange) return false;
  if (!symbol) return true;
  const candidates = [
    legSymbol,
    leg.instrument_id,
    leg.contract_id?.split(':').slice(1).join(':'),
    leg.raw_symbol,
    leg.pair?.replace('/', ''),
  ]
    .map((candidate) => normalizeSnapshotSymbol(candidate))
    .filter((candidate) => candidate.length > 0);
  return candidates.includes(symbol);
}

function resolveMakerV3RoleLegs(row: SignalLegResolutionRow): { makerLeg: SignalLeg | null; referenceLeg: SignalLeg | null } {
  const makerSlot = resolveRoleSlot(row.maker_role_map?.maker_leg, row);
  const referenceSlot = resolveRoleSlot(row.maker_role_map?.ref_leg, row);
  let makerLeg = makerSlot ? getLegForSlot(row, makerSlot) : null;
  let referenceLeg = referenceSlot ? getLegForSlot(row, referenceSlot) : null;

  if (makerLeg && referenceLeg) {
    return { makerLeg, referenceLeg };
  }

  const orderedLegs = getOrderedLegEntries(row);
  const quoteSnapshot = resolveQuoteSnapshot(row);
  const makerExchange = normalizeFacetValue(quoteSnapshot?.maker_exchange);
  const makerSymbol = normalizeSnapshotSymbol(quoteSnapshot?.maker_symbol);
  const referenceExchange = normalizeFacetValue(quoteSnapshot?.ref_exchange);
  const referenceSymbol = normalizeSnapshotSymbol(quoteSnapshot?.ref_symbol);

  if (!makerLeg) {
    makerLeg = orderedLegs.find(({ leg }) =>
      legMatchesSnapshotHint(leg, { exchange: makerExchange, symbol: makerSymbol }),
    )?.leg ?? null;
  }
  if (!referenceLeg) {
    referenceLeg = orderedLegs.find(({ leg }) =>
      legMatchesSnapshotHint(leg, { exchange: referenceExchange, symbol: referenceSymbol }),
    )?.leg ?? null;
  }

  if (!makerLeg) makerLeg = orderedLegs[0]?.leg ?? null;
  if (!referenceLeg) {
    referenceLeg = orderedLegs.find(({ leg }) => leg !== makerLeg)?.leg ?? null;
  }

  return { makerLeg, referenceLeg };
}

function getBalanceStatus(row: EnrichedRow) {
  const readiness = row.balance_readiness;
  const status = readiness?.status as BalanceStatus | undefined;
  const derivedStatus: BalanceStatus =
    status && BALANCE_STATUS_META[status]
      ? status
      : row.balances_ok
        ? 'OK'
        : 'FAIL';
  const tooltip = buildBalanceTooltip(
    readiness,
    row.balances_ok ? 'Balances look healthy' : 'Balances missing'
  );
  return {
    status: derivedStatus,
    meta: BALANCE_STATUS_META[derivedStatus],
    tooltip,
  };
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/**
 * Get color for Edge column based on Edge and Edge2 values.
 * - Edge2 >= 0: Green (tradeable)
 * - Edge >= 0 AND Edge2 < 0: Yellow (marginal)
 * - Edge < 0: Red (negative spread)
 */
function getEdgeColor(edge: number, edge2: number | null): string {
  if (edge2 !== null && edge2 !== undefined && edge2 >= 0) {
    return colors.semantic.success.light;  // Green: Edge2 >= 0
  }
  if (edge >= 0) {
    return colors.semantic.warning.light;  // Yellow: Edge >= 0 but Edge2 < 0
  }
  return colors.semantic.danger.light;  // Red: Edge < 0
}

/**
 * Get color for Edge2 column based on Edge and Edge2 values.
 * - Edge2 >= 0: Green (tradeable)
 * - Edge >= 0 AND Edge2 < 0: Red (below threshold)
 * - Edge < 0: Red (negative spread)
 */
function getEdge2Color(edge: number, edge2: number | null): string {
  if (edge2 !== null && edge2 !== undefined && edge2 >= 0) {
    return colors.semantic.success.light;  // Green: Edge2 >= 0
  }
  return colors.semantic.danger.light;  // Red: Edge2 < 0 or Edge < 0
}

function getSpreadNetColor(spreadNet: number | null): string {
  if (spreadNet === null || spreadNet === undefined || !Number.isFinite(spreadNet)) {
    return colors.text.secondary;
  }
  return spreadNet >= 0 ? colors.semantic.success.light : colors.semantic.danger.light;
}

/**
 * Get color for historical values (trade PnL) based on value thresholds.
 * Uses hardcoded thresholds for historical data (not live strategy state).
 */
function getHistoricEdgeColor(edge: number): string {
  if (edge >= 10) return colors.semantic.success.light;
  if (edge >= 5) return colors.semantic.warning.light;
  return colors.semantic.danger.light;
}

/**
 * Get age color based on staleness threshold
 */
function getAgeColor(ageMs: number): string {
  if (ageMs > 10000) return colors.semantic.danger.light;
  if (ageMs > 3000) return colors.semantic.warning.light;
  return colors.text.secondary;
}

/**
 * Deep equality check for legs
 */
function legsEqual(legA: SignalLeg | null, legB: SignalLeg | null): boolean {
  return isEqual(legA, legB);
}

function formatCoveragePercent(coverage?: number | null): string {
  if (coverage === null || coverage === undefined) return '—';
  return `${Math.max(0, Math.min(coverage * 100, 999)).toFixed(1)}%`;
}

function formatRiskDelta(value: number): string {
  return value.toLocaleString(undefined, { minimumFractionDigits: 4, maximumFractionDigits: 4 });
}

function coerceFiniteNumber(value: unknown): number | undefined {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string') {
    const parsed = Number(value.trim());
    if (Number.isFinite(parsed)) return parsed;
  }
  return undefined;
}

function deriveCoinFromSymbol(symbol: unknown): string | undefined {
  const text = String(symbol ?? '').trim().toUpperCase();
  if (!text) return undefined;
  const baseFromVenue = text.split('.')[0] || text;
  const baseFromSlash = baseFromVenue.split('/')[0] || baseFromVenue;
  const base = baseFromSlash.split('-')[0] || baseFromSlash;
  for (const quote of ['USDT', 'USDC', 'USD', 'PERP']) {
    if (base.endsWith(quote) && base.length > quote.length) {
      return base.slice(0, -quote.length);
    }
  }
  return base || undefined;
}

function resolveTradingValue(row: Partial<SignalStrategy> & Record<string, any>): TradingFlagInput {
  const fromParams = row?.params?.bot_on;
  if (fromParams !== undefined && fromParams !== null) return fromParams;
  const fromState = row?.state?.bot_on;
  if (
    typeof fromState === 'string'
    || typeof fromState === 'number'
    || typeof fromState === 'boolean'
    || fromState === null
  ) {
    return fromState;
  }
  if (typeof row?.tradeable === 'boolean') return row.tradeable ? 1 : 0;
  return undefined;
}

function resolveTradingBlocked(row: Partial<SignalStrategy> & Record<string, any>): boolean {
  if (typeof row?.blocked === 'boolean') return row.blocked;
  if (typeof row?.tradeable === 'boolean') return !row.tradeable;
  return false;
}

function normalizeDeltaLegs(legs: unknown): unknown {
  if (!legs || typeof legs !== 'object') return legs;
  const normalized: Record<string, unknown> = {};
  for (const [contractId, value] of Object.entries(legs as Record<string, unknown>)) {
    if (!value || typeof value !== 'object') {
      normalized[contractId] = value;
      continue;
    }
    const leg = { ...(value as Record<string, unknown>) };
    const contractParts = contractId.split(':');
    const contractExchange = contractParts.length > 1 ? contractParts[0].trim().toLowerCase() : '';
    const contractSymbol = contractParts.length > 1 ? contractParts.slice(1).join(':').trim() : '';
    const bid = coerceFiniteNumber(leg.decision_bid ?? leg.fv_bid ?? leg.bid);
    const ask = coerceFiniteNumber(leg.decision_ask ?? leg.fv_ask ?? leg.ask);
    const tsMs = coerceFiniteNumber(leg.update_ts_ms ?? leg.ts_ms ?? leg.timestamp);
    const ageMs = coerceFiniteNumber(leg.md_age_ms ?? leg.age_ms);
    const symbol = String(leg.symbol ?? contractSymbol ?? '').trim();
    if (leg.contract_id == null) leg.contract_id = contractId;
    if (leg.symbol == null && contractSymbol) leg.symbol = contractSymbol;
    if (!String(leg.exchange ?? '').trim() && contractExchange) leg.exchange = contractExchange;
    if (leg.coin == null && symbol) leg.coin = deriveCoinFromSymbol(symbol);
    if (bid !== undefined) {
      if (leg.fv_bid == null) leg.fv_bid = bid;
      if (leg.decision_bid == null) leg.decision_bid = bid;
    }
    if (ask !== undefined) {
      if (leg.fv_ask == null) leg.fv_ask = ask;
      if (leg.decision_ask == null) leg.decision_ask = ask;
    }
    if (leg.mid == null && bid !== undefined && ask !== undefined) {
      leg.mid = (bid + ask) / 2;
    }
    if (leg.update_ts_ms == null && tsMs !== undefined) leg.update_ts_ms = tsMs;
    if (leg.update_time == null && tsMs !== undefined) {
      leg.update_time = new Date(tsMs).toISOString().replace('T', ' ').slice(0, 19);
    }
    if (leg.md_age_ms == null && ageMs !== undefined) leg.md_age_ms = ageMs;
    normalized[contractId] = leg;
  }
  return normalized;
}

type QuoteCounts = {
  source: 'quote_stacks' | 'maker_quote_status' | 'none';
  maker?: { bidOpen: number; bidDepth: number; bidBlocked: number; askOpen: number; askDepth: number; askBlocked: number };
  hedge?: { bidOpen: number; bidDepth: number; bidBlocked: number; askOpen: number; askDepth: number; askBlocked: number };
};

function quoteCount(value?: number | null): number {
  return coerceFiniteNumber(value) ?? 0;
}

function zeroQuoteCounts() {
  return {
    bidOpen: 0,
    bidDepth: 0,
    bidBlocked: 0,
    askOpen: 0,
    askDepth: 0,
    askBlocked: 0,
  };
}

function normalizeMakerOpenCountsAgainstManagedOrders(
  row: EnrichedRow,
  maker?: {
    bidOpen: number;
    bidDepth: number;
    bidBlocked: number;
    askOpen: number;
    askDepth: number;
    askBlocked: number;
  },
) {
  if (!maker) return maker;
  const managedOrders = coerceFiniteNumber((row as any).managed_orders);
  if (managedOrders !== 0) return maker;
  return {
    ...maker,
    bidOpen: 0,
    askOpen: 0,
    bidBlocked: Math.max(0, maker.bidDepth),
    askBlocked: Math.max(0, maker.askDepth),
  };
}

function shouldShowHedgeQuoteCounts(row: EnrichedRow): boolean {
  const cls = String(row.meta?.class ?? '').trim().toLowerCase();
  return cls.includes('maker') || !!resolveQuoteSnapshot(row);
}

function getQuoteCounts(row: EnrichedRow): QuoteCounts {
  const stacks = (row as any).quote_stacks;
  if (stacks && typeof stacks === 'object') {
    const makerBands = (stacks.maker?.bands ?? []) as any[];
    const hedge = stacks.hedge as any;
    const hasMaker = Array.isArray(makerBands) && makerBands.length > 0;
    const hasHedge = Boolean(hedge && typeof hedge === 'object');
    if (hasMaker || hasHedge) {
      const rawMaker = hasMaker ? {
        bidOpen: makerBands.reduce((s, b) => s + quoteCount(b?.bid?.open), 0),
        bidDepth: makerBands.reduce((s, b) => s + quoteCount(b?.bid?.depth), 0),
        bidBlocked: makerBands.reduce((s, b) => s + quoteCount(b?.bid?.blocked), 0),
        askOpen: makerBands.reduce((s, b) => s + quoteCount(b?.ask?.open), 0),
        askDepth: makerBands.reduce((s, b) => s + quoteCount(b?.ask?.depth), 0),
        askBlocked: makerBands.reduce((s, b) => s + quoteCount(b?.ask?.blocked), 0),
      } : undefined;
      const maker = normalizeMakerOpenCountsAgainstManagedOrders(row, rawMaker);
      const hedgeCounts = hasHedge ? {
        bidOpen: quoteCount(hedge?.bid?.open),
        bidDepth: quoteCount(hedge?.bid?.depth),
        bidBlocked: quoteCount(hedge?.bid?.blocked),
        askOpen: quoteCount(hedge?.ask?.open),
        askDepth: quoteCount(hedge?.ask?.depth),
        askBlocked: quoteCount(hedge?.ask?.blocked),
      } : undefined;
      const hedgeFallback = shouldShowHedgeQuoteCounts(row) ? (hedgeCounts ?? zeroQuoteCounts()) : hedgeCounts;
      return {
        source: 'quote_stacks',
        maker,
        hedge: hedgeFallback,
      };
    }
  }

  const qs = row.maker_quote_status as any;
  if (qs && typeof qs === 'object') {
    const maker = normalizeMakerOpenCountsAgainstManagedOrders(row, {
      bidOpen: quoteCount(qs.bid_open),
      bidDepth: quoteCount(qs.bid_depth),
      bidBlocked: quoteCount(qs.bid_blocked),
      askOpen: quoteCount(qs.ask_open),
      askDepth: quoteCount(qs.ask_depth),
      askBlocked: quoteCount(qs.ask_blocked),
    });
    const hedge = shouldShowHedgeQuoteCounts(row) ? zeroQuoteCounts() : undefined;
    return {
      source: 'maker_quote_status',
      maker,
      hedge,
    };
  }

  if (shouldShowHedgeQuoteCounts(row)) {
    const zero = zeroQuoteCounts();
    return {
      source: 'none',
      maker: zero,
      hedge: zeroQuoteCounts(),
    };
  }

  return { source: 'none' };
}

type InventorySkewAdjustment = PricingAdjustment & { type: 'inventory_skew' };

function findInventorySkewAdjustment(
  adjustments?: PricingAdjustment[] | null
): InventorySkewAdjustment | undefined {
  return (adjustments || []).find(
    (adj): adj is InventorySkewAdjustment => adj?.type === 'inventory_skew'
  );
}

function formatBps(value?: number | null): string {
  const num = coerceFiniteNumber(value);
  if (num === undefined) return '—';
  return num.toFixed(1);
}

function formatSignedBps(value?: number | null): string {
  const num = coerceFiniteNumber(value);
  if (num === undefined) return '—';
  const sign = num >= 0 ? '+' : '';
  return `${sign}${num.toFixed(1)}`;
}

function formatSignedRatio(value?: number | null): string {
  const num = coerceFiniteNumber(value);
  if (num === undefined) return '—';
  const sign = num >= 0 ? '+' : '';
  return `${sign}${num.toFixed(3)}`;
}

function formatRatio(value?: number | null): string {
  const num = coerceFiniteNumber(value);
  if (num === undefined) return '—';
  return num.toFixed(3);
}

function computeInventorySkewBps(adj?: InventorySkewAdjustment): number | undefined {
  if (!adj) return undefined;

  // Canonical signed skew from backend (preferred). Signal should mirror the
  // strategy-exported signed quote shift instead of re-deriving direction when
  // this field is present.
  const signedSkew = coerceFiniteNumber(adj.skew_bps_signed);
  if (signedSkew !== undefined) return signedSkew;

  // Fallback signed skew.
  const invSkew = coerceFiniteNumber(adj.inv_skew);
  if (invSkew !== undefined) return invSkew;

  // Prefer server-provided deltas; fall back to computing from eff/base when needed.
  let deltaBid = coerceFiniteNumber(adj.delta_bid_edge_bps);
  let deltaAsk = coerceFiniteNumber(adj.delta_ask_edge_bps);

  if (deltaBid === undefined) {
    const effBid = coerceFiniteNumber(adj.eff_bid_edge_bps);
    const baseBid = coerceFiniteNumber(adj.base_bid_edge_bps);
    if (effBid !== undefined && baseBid !== undefined) deltaBid = effBid - baseBid;
  }
  if (deltaAsk === undefined) {
    const effAsk = coerceFiniteNumber(adj.eff_ask_edge_bps);
    const baseAsk = coerceFiniteNumber(adj.base_ask_edge_bps);
    if (effAsk !== undefined && baseAsk !== undefined) deltaAsk = effAsk - baseAsk;
  }

  if (deltaBid === undefined || deltaAsk === undefined) return undefined;

  // Signed convention:
  //   skew_bps < 0 => quotes shift down (buy lower, sell lower)
  //   skew_bps > 0 => quotes shift up (buy higher, sell higher)
  //
  // Edge deltas are not price deltas: moving quotes up reduces bid edge and
  // increases ask edge by the same amount. Recover the signed translation from
  // the edge-space change instead of averaging the deltas.
  return (deltaAsk - deltaBid) / 2;
}

function buildInventorySkewSummary(adj?: InventorySkewAdjustment): string | null {
  if (!adj) return null;

  const skewBps = computeInventorySkewBps(adj);
  if (skewBps !== undefined) return formatSignedBps(skewBps);

  const invRatio = coerceFiniteNumber(adj.inv_ratio);
  if (invRatio !== undefined) return `r ${formatSignedRatio(invRatio)}`;

  const invSkew = coerceFiniteNumber(adj.inv_skew);
  if (invSkew !== undefined) {
    const sign = invSkew >= 0 ? '+' : '';
    return `inv ${sign}${invSkew.toFixed(2)}`;
  }
  return null;
}

function resolveInventoryQuantities(row: SignalStrategy): { globalQty: number | null; localQty: number | null } {
  const adj = findInventorySkewAdjustment(row.pricing_adjustments);
  const globalQty = adj
    ? (coerceFiniteNumber(adj.global_qty ?? adj.curr_qty) ?? null)
    : (coerceFiniteNumber(row.risk_delta) ?? null);
  const localQty = coerceFiniteNumber(adj?.local_qty) ?? null;
  return { globalQty, localQty };
}

export function buildInventorySkewTooltip(
  adj?: InventorySkewAdjustment,
  params?: Record<string, string | undefined>,
): string | null {
  if (!adj) return null;
  const skewBps = computeInventorySkewBps(adj);
  const linearSkew = coerceFiniteNumber(adj.linear_offset_bps ?? params?.linear_offset_bps);
  const desQtyGlobal = params?.des_qty_global ?? params?.des_qty;
  const maxQtyGlobal = params?.max_qty_global ?? params?.max_qty;
  const maxSkewGlobal = params?.max_skew_bps_global ?? params?.max_skew_bps;
  const desQtyLocal = params?.des_qty_local ?? params?.local_des_qty;
  const maxQtyLocal = params?.max_qty_local ?? params?.local_max_qty;
  const maxSkewLocal = params?.max_skew_bps_local ?? params?.local_max_skew_bps;
  const globalRatio = coerceFiniteNumber(adj.inv_ratio_global);
  const globalSkew = coerceFiniteNumber(adj.inv_skew_global);
  const localRatio = coerceFiniteNumber(adj.inv_ratio_local);
  const localSkew = coerceFiniteNumber(adj.inv_skew_local);
  const localQty = coerceFiniteNumber(adj.local_qty);
  const localMatchedRows = coerceFiniteNumber(adj.local_qty_matched_rows);
  const localMissingSnapshot = coerceFiniteNumber(adj.local_qty_missing_snapshot);
  const localQtyKey = adj.local_qty_key;
  const actualBidEdge = coerceFiniteNumber(adj.eff_bid_edge_bps ?? adj.base_bid_edge_bps);
  const actualAskEdge = coerceFiniteNumber(adj.eff_ask_edge_bps ?? adj.base_ask_edge_bps);
  const lines: string[] = [
    'FvAdj (MakerV3)',
    `quoted FV shift: ${skewBps !== undefined ? formatSignedBps(skewBps) : '—'} bps`,
    `linear + global + local: ${formatSignedBps(linearSkew)} + ${formatSignedBps(globalSkew)} + ${formatSignedBps(localSkew)} = ${skewBps !== undefined ? formatSignedBps(skewBps) : '—'}`,
    `inventory ratio: ${formatSignedRatio(adj.inv_ratio)} (clamped to [-1, +1])`,
    '',
    'Global bucket:',
    `target / cap / max shift: ${desQtyGlobal ?? '—'} / ${maxQtyGlobal ?? '—'} / ${maxSkewGlobal ?? '—'}`,
    `ratio / shift: ${formatSignedRatio(globalRatio)} / ${formatSignedBps(globalSkew)} bps`,
    '',
    'Local bucket:',
    `target / cap / max shift: ${desQtyLocal ?? '—'} / ${maxQtyLocal ?? '—'} / ${maxSkewLocal ?? '—'}`,
    `ratio / shift: ${formatSignedRatio(localRatio)} / ${formatSignedBps(localSkew)} bps`,
  ];

  if (localQty !== undefined || !!localQtyKey) {
    const venueRoot = localQtyKey && typeof localQtyKey === 'object'
      ? String(localQtyKey.venue_root ?? '').trim() || '—'
      : '—';
    const instrumentType = localQtyKey && typeof localQtyKey === 'object'
      ? String(localQtyKey.instrument_type ?? '').trim() || '—'
      : '—';
    const base = localQtyKey && typeof localQtyKey === 'object'
      ? String(localQtyKey.base ?? '').trim() || '—'
      : '—';
    lines.push(`local qty / bucket: ${localQty ?? '—'} / ${venueRoot}/${instrumentType}/${base}`);
  }
  if (localMatchedRows !== undefined || localMissingSnapshot !== undefined) {
    const matched = localMatchedRows !== undefined ? String(Math.trunc(localMatchedRows)) : '—';
    const missing = localMissingSnapshot !== undefined ? String(Math.trunc(localMissingSnapshot)) : '—';
    lines.push(`snapshot rows matched / missing: ${matched} / ${missing}`);
  }

  lines.push(
    '',
    'Actual maker edges:',
    `bid / ask: ${formatBps(actualBidEdge)} / ${formatBps(actualAskEdge)} bps`,
    '',
    'Applies to maker quotes only.',
  );
  return lines.join('\n');
}

/**
 * Build balance readiness tooltip text.
 *
 * Handles edge cases:
 * - 0 values are displayed as "0.00" (not "N/A")
 * - null/undefined values are displayed as "N/A"
 *
 * @param readiness - Balance readiness data from strategy
 * @param fallback - Fallback text if readiness is missing
 * @returns Formatted tooltip text with newlines
 */
export function buildBalanceTooltip(readiness?: SignalStrategy['balance_readiness'], fallback?: string): string {
  if (!readiness) {
    return fallback || 'No readiness data yet';
  }

  const lines: string[] = [];
  const qty = String(readiness.qty ?? '').trim() || '—';
  const multiplier = String(readiness.multiplier ?? '').trim() || '—';

  // Status / summary first so operators immediately see the key message.
  if (readiness.summary) {
    lines.push(readiness.summary);
  } else {
    const status = readiness.status || 'UNKNOWN';
    lines.push(`Status: ${status}`);
  }

  // Then show concrete requirements or top gaps (limited list) so the tooltip stays compact.
  if (readiness.requirements && readiness.requirements.length > 0) {
    lines.push('');
    lines.push('Requirements:');
    readiness.requirements.forEach(req => {
      const hasRequired = req.required != null;
      const required = hasRequired ? Number(req.required).toFixed(2) : 'N/A';
      const hasAvail = req.available != null;
      const available = hasAvail ? Number(req.available).toFixed(2) : 'N/A';
      const coverage = formatCoveragePercent(req.coverage);
      const kind = String(req.kind ?? '').trim();
      lines.push(`  ${req.location} ${req.token}${kind ? ` [${kind}]` : ''}: ${available}/${required} (${coverage})`);
    });
  } else if (readiness.missing && readiness.missing.length > 0) {
    lines.push('');
    lines.push('Top gaps:');
    readiness.missing.forEach(req => {
      const kind = String(req.kind ?? '').trim();
      lines.push(`  ${req.location} ${req.token}${kind ? ` [${kind}]` : ''} ${formatCoveragePercent(req.coverage)}`);
    });
  }

  lines.push('');
  lines.push('Coverage = available / required');
  lines.push(`Qty basis: qty ${qty} × multiplier ${multiplier}`);

  return lines.join('\n');
}

export function buildStrategyParamTooltip(
  row: Pick<SignalStrategy, 'params'>,
): string {
  return [
    'Key params:',
    '',
    'Quote/trade edges:',
    `  bid edge: ${row.params?.cex_bid_edge ?? 'N/A'} bps`,
    `  ask edge: ${row.params?.cex_ask_edge ?? 'N/A'} bps`,
    `  pool edge: ${row.params?.pool_edge ?? 'N/A'} bps`,
    '',
    'Order sizing:',
    `  qty: ${row.params?.qty ?? 'N/A'}`,
    `  slippage cap: ${row.params?.slippage_bps ?? 'N/A'} bps`,
  ].join('\n');
}

// =============================================================================
// LEG CELL COMPONENT
// =============================================================================

interface LegCellProps {
  leg: SignalLeg | null;
  showQuoted: boolean;
  tooltipBehavior?: 'cell' | 'icon';
  contextHint?: string;
  sourceLabel?: string | null;
}

interface FrozenState {
  tooltip?: React.ReactNode;
  bid?: number;
  mid?: number;
  ask?: number;
  fxBadgeVisible: boolean;
  fxAgeText?: string;
}

function KvGrid({
  rows,
}: {
  rows: Array<{ label: string; value: React.ReactNode; wrap?: boolean }>;
}) {
  return (
    <div className="grid grid-cols-[auto,1fr] gap-x-3 gap-y-0.5 font-mono text-[11px] leading-4 tabular-nums">
      {rows.map((r, i) => (
        <div className="contents" key={`${r.label}-${i}`}>
          <span className="text-text-muted whitespace-nowrap">{r.label}</span>
          <span className={cn('text-text-secondary', r.wrap ? 'whitespace-normal break-words' : 'whitespace-nowrap')}>
            {r.value}
          </span>
        </div>
      ))}
    </div>
  );
}

/**
 * LegCell - 2-line display of leg data (exchange/coin + bid/mid/ask)
 *
 * Implements hover freezing for stable tooltips and proper fallback handling:
 * - Shows "—" (em dash) instead of 0 when pricing data is missing
 * - Uses SimpleTooltip for consistent multi-line tooltip rendering
 * - Freezes displayed values on hover to prevent tooltip flicker
 *
 * @param leg - Signal leg data (may be null)
 * @param showQuoted - Whether to show quoted prices (with edge bias) vs decision prices
 */
const LegCell: FC<LegCellProps> = memo(({ leg, showQuoted, tooltipBehavior = 'cell', contextHint, sourceLabel }) => {
  const [isHovered, setIsHovered] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const frozenRef = useRef<FrozenState>({ fxBadgeVisible: false });

  // Prices published in fvs.snapshot are already FX-normalized (for cross-quote)
  // so do not re-apply fx_factor here to avoid double-counting.
  const hasDecision = leg?.decision_bid != null && leg?.decision_ask != null;
  const decisionBid = hasDecision ? (leg?.decision_bid ?? leg?.fv_bid) : leg?.fv_bid;
  const decisionAsk = hasDecision ? (leg?.decision_ask ?? leg?.fv_ask) : leg?.fv_ask;
  // mid only if both sides exist
  const decisionMid = (decisionBid != null && decisionAsk != null)
    ? (decisionBid + decisionAsk) / 2
    : undefined;

  // Quoted overlay is already derived off decision prices server-side
  const displayBid = showQuoted ? (leg?.quoted_bid ?? decisionBid) : decisionBid;
  const displayAsk = showQuoted ? (leg?.quoted_ask ?? decisionAsk) : decisionAsk;
  // Keep the displayed mid anchored to decision mid so edge bias only
  // adjusts the quoted sides without visually shifting the midpoint.
  const displayMid = decisionMid;

  // Helper to format price or show em dash if missing
  const fmtMaybe = (n?: number) => n == null ? '—' : fmtPriceSignal(n);

  // Memoize tooltip calculation - only recompute when leg data changes
  const liveTooltip = useMemo<React.ReactNode | undefined>(() => {
    if (!leg) return undefined;
    // Pricing pipeline tooltip: compact, operator-first.
    const hasBreakdown = !!(leg.raw_bid || leg.raw_ask || leg.fee_bps || leg.fx_factor);
    if (!hasBreakdown) return undefined;

    const marketBid = leg.raw_bid;
    const marketAsk = leg.raw_ask;
    const marketMid = (marketBid != null && marketAsk != null) ? (marketBid + marketAsk) / 2 : undefined;

    const rows: Array<{ label: string; value: React.ReactNode }> = [];
    if (marketBid != null && marketAsk != null) {
      rows.push({
        label: 'Raw market',
        value: `${fmtPriceTooltip(marketBid)} / ${fmtPriceTooltip(marketMid)} / ${fmtPriceTooltip(marketAsk)}`,
      });
    }

    if (leg.fee_bps !== undefined && leg.fee_type) {
      rows.push({
        label: 'Fees',
        value: `${leg.fee_type} ${leg.fee_bps.toFixed(1)} bps`,
      });
    }

    rows.push({
      label: 'After fees',
      value: `${fmtPriceTooltip(decisionBid)} / ${fmtPriceTooltip(decisionMid)} / ${fmtPriceTooltip(decisionAsk)}`,
    });

    if (leg.fx_factor !== undefined && leg.fx_pair) {
      const fxAge = leg.fx_age_ms ? (leg.fx_age_ms / 1000).toFixed(2) : '?';
      const fxSource = leg.fx_source || 'unknown';
      const fxImpactBps = (leg.fx_factor - 1.0) * 10000;
      const fxSign = fxImpactBps >= 0 ? '+' : '';
      rows.push({
        label: 'FX',
        value: `${leg.fx_pair} ${leg.fx_factor.toFixed(5)} (${fxSource}, ${fxSign}${fxImpactBps.toFixed(1)} bps, ${fxAge}s)`,
      });
    }

    if (typeof leg.md_age_ms === 'number') {
      const mdAge = (leg.md_age_ms / 1000).toFixed(2);
      const ts = typeof leg.md_ts_ms === 'number' ? formatAbsoluteTime(leg.md_ts_ms) : '—';
      rows.push({
        label: 'MD age',
        value: `${mdAge}s (ts: ${ts})`,
      });
    }

    return (
      <div className="max-w-[360px] flex flex-col gap-2">
        <div className="font-mono text-[11px] leading-4 tabular-nums">
          <div className="text-text-muted">
            {getLegDisplayLabel(leg)}
            <span className="text-text-muted opacity-70"> (Market → Decision)</span>
          </div>
        </div>
        <KvGrid rows={rows} />
        {contextHint ? (
          <div className="font-mono text-[11px] leading-4 text-text-muted">
            {contextHint}
          </div>
        ) : null}
      </div>
    );
  }, [
    leg,
    leg?.raw_bid,
    leg?.raw_ask,
    leg?.fee_bps,
    leg?.fee_type,
    leg?.fx_factor,
    leg?.fx_pair,
    leg?.fx_age_ms,
    leg?.fx_source,
    leg?.md_age_ms,
    leg?.md_ts_ms,
    decisionBid,
    decisionAsk,
    decisionMid,
    contextHint
  ]);

  if (!leg) return <span className="text-text-muted">N/A</span>;

  // Handlers to freeze/unfreeze tooltip
  const handleMouseEnter = () => {
    // Freeze displayed numbers and FX badge state
    frozenRef.current.bid = displayBid;
    frozenRef.current.mid = displayMid;
    frozenRef.current.ask = displayAsk;
    frozenRef.current.tooltip = liveTooltip;
    const liveFxAgeSec = leg.fx_age_ms ? (leg.fx_age_ms/1000).toFixed(1) : undefined;
    frozenRef.current.fxAgeText = liveFxAgeSec;
    frozenRef.current.fxBadgeVisible = !!(leg.fx_age_ms && leg.fx_age_ms > 5000);
    setIsHovered(true);
  };

  const handleMouseLeave = () => {
    setIsHovered(false);
  };

  // Latency badge for FX age (if present and stale) - freeze while hovered
  const liveFxAgeSec = leg.fx_age_ms ? (leg.fx_age_ms/1000).toFixed(1) : undefined;
  const liveFxBadgeVisible = !!(leg.fx_age_ms && leg.fx_age_ms > 5000);
  const showFxBadge = isHovered ? frozenRef.current.fxBadgeVisible : liveFxBadgeVisible;
  const fxAgeText = isHovered ? (frozenRef.current.fxAgeText ?? liveFxAgeSec) : liveFxAgeSec;
  const fxBadge = showFxBadge ? (
    <SimpleTooltip content={fxAgeText ? `FX age: ${fxAgeText}s` : undefined} delay={150}>
      <span className="ml-1 text-warning-light text-[10px] cursor-help">
        FX {fxAgeText}
      </span>
    </SimpleTooltip>
  ) : null;

  return (
    (() => {
      const advancedTooltip =
        liveTooltip
          ? (isHovered ? frozenRef.current.tooltip : liveTooltip)
          : undefined;

      const advancedIcon = advancedTooltip ? (
        <SimpleTooltip content={advancedTooltip} delay={150}>
          <span className="ml-1 inline-flex items-center text-text-muted cursor-help" aria-label="Pricing pipeline details">
            <Info className="h-3 w-3" />
          </span>
        </SimpleTooltip>
      ) : null;

      const content = (
        <div
          ref={containerRef}
          className="flex flex-col gap-1"
          style={{ whiteSpace: 'pre' }}
          onMouseEnter={handleMouseEnter}
          onMouseLeave={handleMouseLeave}
        >
          <div className="text-text-muted text-xs flex items-center">
            {getLegDisplayLabel(leg)}
            {sourceLabel ? (
              <Badge
                variant="neutral"
                size="xs"
                className="ml-1 h-[16px] px-1 py-0 text-[9px] tracking-wide"
                aria-label={`Configured source ${sourceLabel}`}
              >
                {sourceLabel}
              </Badge>
            ) : null}
            {fxBadge}
            {tooltipBehavior === 'icon' ? advancedIcon : null}
          </div>
          <div className="flex gap-2">
            <span className="text-success-light">{fmtMaybe(isHovered ? (frozenRef.current.bid ?? displayBid) : displayBid)}</span>
            <span className="text-text-secondary opacity-70">{fmtMaybe(isHovered ? (frozenRef.current.mid ?? displayMid) : displayMid)}</span>
            <span className="text-danger-light">{fmtMaybe(isHovered ? (frozenRef.current.ask ?? displayAsk) : displayAsk)}</span>
          </div>
        </div>
      );

      if (tooltipBehavior === 'icon') {
        return content;
      }

      return (
        <SimpleTooltip content={advancedTooltip} delay={150} disabled={!advancedTooltip}>
          {content}
        </SimpleTooltip>
      );
    })()
  );
}, (prevProps, nextProps) => {
  // Custom equality: only re-render if leg or showQuoted changed
  return (
    legsEqual(prevProps.leg, nextProps.leg)
    && prevProps.showQuoted === nextProps.showQuoted
    && prevProps.tooltipBehavior === nextProps.tooltipBehavior
    && prevProps.contextHint === nextProps.contextHint
    && prevProps.sourceLabel === nextProps.sourceLabel
  );
});
LegCell.displayName = 'LegCell';

// =============================================================================
// MAKER V2 TRUTH OVERLAY (ROW 2)
// =============================================================================

type LegKey = 'A' | 'B';

function makerModeToVariant(mode?: string): BadgeVariant {
  const m = (mode ?? '').toUpperCase();
  if (!m) return 'neutral';
  if (m === 'QUOTING') return 'success';
  if (m === 'OFF') return 'outline';
  if (m.includes('BLOCKED') || m.includes('FAILED')) return 'danger';
  if (m.includes('STALE') || m.includes('PENDING')) return 'warning';
  return 'neutral';
}

function coerceSnapshotPx(v: unknown): number | undefined {
  return coerceFiniteNumber(v);
}

function stripSnapshotContractSuffix(rawSymbol: string): string {
  const text = rawSymbol.trim().toUpperCase();
  if (!text) return '';
  for (const suffix of ['-LINEAR', '-SWAP', '-INVERSE', '-PERP', '-SPOT']) {
    if (text.endsWith(suffix) && text.length > suffix.length) {
      return text.slice(0, -suffix.length);
    }
  }
  return text;
}

function normalizeSnapshotSymbol(value: unknown): string {
  const text = String(value ?? '').trim().toUpperCase();
  if (!text) return '';
  const contractText = text.includes(':') ? (text.split(':', 2)[1] ?? text) : text;
  const instrumentText = contractText.split('.', 1)[0] ?? contractText;
  return stripSnapshotContractSuffix(instrumentText).replace(/[/_-]/g, '');
}

function extractSnapshotBaseAsset(value: unknown): string {
  const text = String(value ?? '').trim().toUpperCase();
  if (!text) return '';
  const contractText = text.includes(':') ? (text.split(':', 2)[1] ?? text) : text;
  const instrumentText = contractText.split('.', 1)[0] ?? contractText;
  const stripped = stripSnapshotContractSuffix(instrumentText);
  const pairBase = stripped.split(/[\/_-]/, 1)[0] ?? stripped;
  return pairBase.replace(/[/_-]/g, '');
}

function getSnapshotLegMatchScore(
  leg: SignalLeg | null | undefined,
  {
    exchange,
    symbol,
  }: {
    exchange: string | undefined;
    symbol: string | undefined;
  },
): number {
  if (!leg) return 0;
  const legExchange = String(leg.exchange ?? '').trim().toLowerCase();
  const targetExchange = String(exchange ?? '').trim().toLowerCase();
  if (!legExchange || !targetExchange || legExchange !== targetExchange) return 0;

  const legSymbolCandidates = [
    leg.pair,
    leg.raw_symbol,
    leg.instrument_id,
    leg.contract_id,
  ]
    .map((candidate) => normalizeSnapshotSymbol(candidate))
    .filter((candidate) => candidate.length > 0);
  const targetSymbol = normalizeSnapshotSymbol(symbol);
  if (!targetSymbol) return 1;
  if (legSymbolCandidates.includes(targetSymbol)) return 2;

  const targetBase = extractSnapshotBaseAsset(symbol);
  if (!targetBase) return 0;

  const legBaseCandidates = [
    leg.pair,
    leg.base_asset,
    leg.inventory_asset,
    leg.coin,
  ]
    .map((candidate) => extractSnapshotBaseAsset(candidate))
    .filter((candidate) => candidate.length > 0);
  return legBaseCandidates.includes(targetBase) ? 1 : 0;
}

function resolveMakerAwareLeg(
  row: SignalLegResolutionRow,
  legKey: LegKey,
  quoteSnapshot: MakerV2QuoteSnapshot,
): SignalLeg | null {
  const roleMap = row.maker_role_map;
  const targetLegId = legKey === 'A' ? roleMap?.maker_leg : roleMap?.ref_leg;
  if (targetLegId) {
    const mappedLeg = row.legs?.[targetLegId] ?? null;
    if (mappedLeg) return mappedLeg;
  }

  const targetExchange = legKey === 'A' ? quoteSnapshot.maker_exchange : quoteSnapshot.ref_exchange;
  const targetSymbol = legKey === 'A' ? quoteSnapshot.maker_symbol : quoteSnapshot.ref_symbol;
  const looseMatches: SignalLeg[] = [];
  for (const entry of getOrderedLegEntries(row)) {
    const score = getSnapshotLegMatchScore(entry.leg, { exchange: targetExchange, symbol: targetSymbol });
    if (score >= 2) {
      return entry.leg;
    }
    if (score === 1 && entry.leg) {
      looseMatches.push(entry.leg);
    }
  }

  if (looseMatches.length === 1) return looseMatches[0];

  return getLegForSlot(row, legKey);
}

function resolveDisplayedLeg(
  row: SignalLegResolutionRow,
  legKey: LegKey,
): SignalLeg | null {
  const quoteSnapshot = resolveQuoteSnapshot(row);
  return quoteSnapshot ? resolveMakerAwareLeg(row, legKey, quoteSnapshot) : getLegForSlot(row, legKey);
}

function midpointFromValues(bid: unknown, ask: unknown): number | null {
  const bidPx = coerceFiniteNumber(bid);
  const askPx = coerceFiniteNumber(ask);
  if (bidPx == null || askPx == null) return null;
  return (bidPx + askPx) / 2;
}

function resolveDisplayedLegMid(leg: SignalLeg | null | undefined): number | null {
  if (!leg) return null;
  return midpointFromValues(
    leg.decision_bid ?? leg.fv_bid ?? leg.raw_bid,
    leg.decision_ask ?? leg.fv_ask ?? leg.raw_ask
  );
}

function resolveVisibleStrategyMarketMid(row: SignalStrategy): number | null {
  const quoteSnapshot = resolveQuoteSnapshot(row) as any;
  if (quoteSnapshot) {
    const snapshotMid = midpointFromValues(
      quoteSnapshot.maker_top_bid ?? quoteSnapshot.bid,
      quoteSnapshot.maker_top_ask ?? quoteSnapshot.ask,
    );
    if (snapshotMid != null) return snapshotMid;
  }

  return resolveDisplayedLegMid(resolveDisplayedLeg(row, 'A'));
}

function resolveVisibleStrategyFvMid(row: SignalStrategy): number | null {
  const quoteSnapshot = resolveQuoteSnapshot(row);
  if (quoteSnapshot) {
    const snapshotMid = midpointFromValues(quoteSnapshot.ref_bid, quoteSnapshot.ref_ask);
    if (snapshotMid != null) return snapshotMid;
  }

  const displayedMid = resolveDisplayedLegMid(resolveDisplayedLeg(row, 'B'));
  if (displayedMid != null) return displayedMid;

  return coerceFiniteNumber((row as any).fv_row?.fv) ?? null;
}

function spreadMarketVsFvBps(row: SignalStrategy): number | null {
  // Operator-facing spread should represent raw maker market mid versus
  // reference/FV mid, not our translated quoted mid.
  const marketMid = resolveVisibleStrategyMarketMid(row);
  const fvMid = resolveVisibleStrategyFvMid(row);
  if (marketMid == null || fvMid == null || !Number.isFinite(fvMid) || fvMid === 0) return null;
  return ((marketMid - fvMid) / fvMid) * 10_000;
}

function buildMakerTruthRow(
  row: EnrichedRow,
  legKey: LegKey,
  quoteSnapshot: MakerV2QuoteSnapshot,
): {
  label: string;
  bid?: number;
  mid?: number;
  ask?: number;
  tags: Array<'Maker' | 'Ref' | 'Hedge'>;
  isMakerLeg: boolean;
  isRefLeg: boolean;
  mode?: string;
  reason?: string | null;
} | null {
  const leg = resolveMakerAwareLeg(row, legKey, quoteSnapshot);
  const isMakerLeg = legKey === 'A';
  const isRefLeg = legKey === 'B';

  if (!isMakerLeg && !isRefLeg) return null;

  const bid = isMakerLeg ? coerceSnapshotPx(quoteSnapshot.place_bid) : coerceSnapshotPx(quoteSnapshot.ref_bid);
  const ask = isMakerLeg ? coerceSnapshotPx(quoteSnapshot.place_ask) : coerceSnapshotPx(quoteSnapshot.ref_ask);
  const mid = (bid != null && ask != null) ? (bid + ask) / 2 : undefined;

  const snapMode = (quoteSnapshot.mode ?? '').toUpperCase();
  const isLastKnown = snapMode.includes('STALE');

  return {
    // Keep labels terse: Lan-demo mental model is "Market" vs "Our quotes" / "Ref used".
    // Staleness is communicated via the mode pill + snapshot age.
    label: isMakerLeg
      ? (isLastKnown ? 'Our (last-known)' : 'Our')
      : (isLastKnown ? 'Ref (last-known)' : 'Ref'),
    bid,
    mid,
    ask,
    tags: isMakerLeg ? ['Maker'] : ['Ref'],
    isMakerLeg,
    isRefLeg,
    mode: quoteSnapshot.mode,
    reason: (typeof quoteSnapshot.reason === 'string' || quoteSnapshot.reason === null) ? quoteSnapshot.reason : undefined,
  };
}

function buildMakerTruthTooltip(row: EnrichedRow, quoteSnapshot: MakerV2QuoteSnapshot, nowMs: number): React.ReactNode {
  const snapTsMs = coerceFiniteNumber((quoteSnapshot as any).ts_ms);
  const snapAgeMs = snapTsMs != null ? Math.max(0, nowMs - snapTsMs) : null;
  const snapAgeText = snapAgeMs != null ? `${(snapAgeMs / 1000).toFixed(1)}s` : '—';
  const snapTsText = snapTsMs != null ? formatAbsoluteTime(snapTsMs) : '—';

  const mode = (quoteSnapshot.mode ?? '—').toUpperCase();
  const reason = (typeof quoteSnapshot.reason === 'string' && quoteSnapshot.reason.trim().length > 0)
    ? quoteSnapshot.reason.trim()
    : undefined;

  const refBid = coerceSnapshotPx(quoteSnapshot.ref_bid);
  const refAsk = coerceSnapshotPx(quoteSnapshot.ref_ask);
  const refAgeMs = coerceFiniteNumber((quoteSnapshot as any).ref_age_ms);
  const refAgeText = refAgeMs != null ? `${(refAgeMs / 1000).toFixed(2)}s` : '—';

  const makerTopBid = coerceSnapshotPx(quoteSnapshot.maker_top_bid);
  const makerTopAsk = coerceSnapshotPx(quoteSnapshot.maker_top_ask);
  const makerTopAgeMs = coerceFiniteNumber((quoteSnapshot as any).maker_top_age_ms);
  const makerTopAgeText = makerTopAgeMs != null ? `${(makerTopAgeMs / 1000).toFixed(2)}s` : '—';

  const placeBid = coerceSnapshotPx(quoteSnapshot.place_bid);
  const placeAsk = coerceSnapshotPx(quoteSnapshot.place_ask);
  const cancelBid = coerceSnapshotPx(quoteSnapshot.cancel_bid);
  const cancelAsk = coerceSnapshotPx(quoteSnapshot.cancel_ask);

  const effBid = coerceFiniteNumber((quoteSnapshot as any).eff_bid_edge_bps);
  const effAsk = coerceFiniteNumber((quoteSnapshot as any).eff_ask_edge_bps);
  const placeEdge = coerceFiniteNumber((quoteSnapshot as any).place_edge_bps);

  // Fees: derive from legs (best-effort).
  const roleMap = row.maker_role_map;
  const makerLegKey = resolveRoleSlot(roleMap?.maker_leg, row);
  const explicitHedgeKey = resolveRoleSlot(roleMap?.hedge_leg ?? roleMap?.ref_leg, row);
  const hedgeLegKey: LegKey | undefined = explicitHedgeKey
    ?? (makerLegKey === 'A' ? 'B' : (makerLegKey === 'B' ? 'A' : undefined));

  const makerLeg = makerLegKey ? getLegForSlot(row, makerLegKey) : null;
  const hedgeLeg = hedgeLegKey ? getLegForSlot(row, hedgeLegKey) : null;
  const makerFee = (makerLeg?.fee_type && typeof makerLeg.fee_bps === 'number')
    ? `${makerLeg.fee_type} ${makerLeg.fee_bps.toFixed(1)} bps`
    : null;
  const hedgeFee = (hedgeLeg?.fee_type && typeof hedgeLeg.fee_bps === 'number')
    ? `${hedgeLeg.fee_type} ${hedgeLeg.fee_bps.toFixed(1)} bps`
    : null;
  const feesText = makerFee && hedgeFee ? `${makerFee} + hedge ${hedgeFee}` : (makerFee ?? hedgeFee ?? '—');

  return (
    <div className="max-w-[360px] flex flex-col gap-2 font-mono text-[11px] leading-4 tabular-nums">
      <div className="text-text-muted font-semibold">Actual quoting snapshot</div>

      <div className="flex flex-col gap-1">
        <div className="text-text-muted font-semibold">State</div>
        <KvGrid
          rows={[
            { label: 'State', value: <span className="text-text-secondary">{mode}</span> },
            { label: 'Snapshot', value: `${snapAgeText} (ts: ${snapTsText})` },
            ...(reason ? [{ label: 'Reason', value: reason, wrap: true }] : []),
          ]}
        />
      </div>

      <div className="flex flex-col gap-1">
        <div className="text-text-muted font-semibold">Inputs</div>
        <KvGrid
          rows={[
            {
              label: 'Ref used',
              value: `${quoteSnapshot.ref_exchange ?? '—'} ${quoteSnapshot.ref_symbol ?? '—'}   ${fmtPriceTooltip(refBid)} / ${fmtPriceTooltip(refAsk)}   age ${refAgeText}`,
            },
            {
              label: 'Maker top',
              value: `${quoteSnapshot.maker_exchange ?? '—'} ${quoteSnapshot.maker_symbol ?? '—'}   ${fmtPriceTooltip(makerTopBid)} / ${fmtPriceTooltip(makerTopAsk)}   age ${makerTopAgeText}`,
            },
          ]}
        />
      </div>

      <div className="flex flex-col gap-1">
        <div className="text-text-muted font-semibold">Outputs</div>
        <KvGrid
          rows={[
            { label: 'Placed L1', value: `${fmtPriceTooltip(placeBid)} .. ${fmtPriceTooltip(placeAsk)}` },
            ...(cancelBid != null || cancelAsk != null
              ? [{ label: 'Cancel', value: `${fmtPriceTooltip(cancelBid)} .. ${fmtPriceTooltip(cancelAsk)}` }]
              : []),
            { label: 'Live edges', value: `bid ${formatBps(effBid)} bps   ask ${formatBps(effAsk)} bps` },
            { label: 'Fees', value: feesText },
            { label: 'Place edge', value: `${formatBps(placeEdge)} bps` },
          ]}
        />
      </div>
    </div>
  );
}

const MakerTruthRow: FC<{ row: EnrichedRow; legKey: LegKey; quoteSnapshot: MakerV2QuoteSnapshot; nowMs: number }> = memo(({
  row,
  legKey,
  quoteSnapshot,
  nowMs,
}) => {
  const data = buildMakerTruthRow(row, legKey, quoteSnapshot);
  // Defensive fallback: if we can't map this leg to Maker/Ref (role-map mismatch),
  // still render a minimal MakerV2 row so operators can see mode + snapshot freshness.
  // This should be rare and indicates backend role-map inference drift.
  const fallbackMode = (quoteSnapshot.mode ?? '—').toUpperCase();
  const fallbackReason = (typeof quoteSnapshot.reason === 'string' && quoteSnapshot.reason.trim().length > 0)
    ? quoteSnapshot.reason.trim()
    : undefined;
  if (!data) {
    const truthTooltip = buildMakerTruthTooltip(row, quoteSnapshot, nowMs);
    return (
      <div className="mt-1 flex flex-col gap-0.5 rounded-sm border-l-2 border-neutral-700 bg-neutral-900/10 pl-2 py-1">
        <div className="flex items-center gap-1.5">
          <span className="text-[10px] text-text-muted font-semibold">Maker:</span>
          <SimpleTooltip
            content={truthTooltip}
            delay={150}
            disabled={!truthTooltip}
          >
            <span className="inline-flex items-center gap-1">
              <Badge
                variant={makerModeToVariant(fallbackMode)}
                size="xs"
                className="h-[16px] px-1 py-0 text-[9px] uppercase tracking-wide cursor-help"
                aria-label={fallbackReason ? `${fallbackMode}: ${fallbackReason}` : fallbackMode}
              >
                {fallbackMode}
              </Badge>
              {(() => {
                const snapTs = coerceFiniteNumber((quoteSnapshot as any).ts_ms);
                if (snapTs == null) return null;
                const ageMs = Math.max(0, nowMs - snapTs);
                const cls = ageMs > 10_000 ? 'text-danger-light' : (ageMs > 2_000 ? 'text-warning-light' : 'text-text-muted');
                return (
                  <span
                    className={cn('text-[10px] font-mono', cls)}
                    aria-label={`snapshot age ${(ageMs / 1000).toFixed(1)}s`}
                  >
                    {fmtAgeSec(ageMs)}
                  </span>
                );
              })()}
              {fallbackReason && fallbackMode !== 'QUOTING' ? (
                <span
                  className="text-[10px] text-text-muted max-w-[140px] truncate"
                  title={fallbackReason}
                >
                  {fallbackReason}
                </span>
              ) : null}
            </span>
          </SimpleTooltip>
        </div>
        <div className="flex gap-2 text-[11px] font-mono opacity-60">
          <span className="text-success-light">—</span>
          <span className="text-danger-light">—</span>
        </div>
      </div>
    );
  }

  const fmtMaybe = (n?: number) => n == null ? '—' : fmtPriceSignal(n);
  const mode = (data.mode ?? '—').toUpperCase();
  const reason = (typeof data.reason === 'string' && data.reason.trim().length > 0) ? data.reason.trim() : undefined;
  const truthTooltip = buildMakerTruthTooltip(row, quoteSnapshot, nowMs);

  const cancelBid = data.isMakerLeg ? coerceSnapshotPx(quoteSnapshot.cancel_bid) : undefined;
  const cancelAsk = data.isMakerLeg ? coerceSnapshotPx(quoteSnapshot.cancel_ask) : undefined;
  const showBalanceFail = (row.balance_readiness?.status as any) === 'FAIL';

  return (
    <div className="mt-1 flex flex-col gap-0.5 rounded-sm border-l-2 border-neutral-700 bg-neutral-900/10 pl-2 py-1">
      <div className="flex items-center gap-1.5">
        <span className="text-[10px] text-text-muted font-semibold">
          {data.label}
        </span>
        <SimpleTooltip
          content={<pre className="whitespace-pre-wrap">{truthTooltip}</pre>}
          delay={150}
          disabled={!truthTooltip}
        >
          <span className="inline-flex items-center gap-1">
            <Badge
              variant={makerModeToVariant(mode)}
              size="xs"
              className="h-[16px] px-1 py-0 text-[9px] uppercase tracking-wide cursor-help"
              aria-label={reason ? `${mode}: ${reason}` : mode}
            >
              {mode}
            </Badge>
            {(() => {
              const snapTs = coerceFiniteNumber((quoteSnapshot as any).ts_ms);
              if (snapTs == null) return null;
              const ageMs = Math.max(0, nowMs - snapTs);
              const cls = ageMs > 10_000 ? 'text-danger-light' : (ageMs > 2_000 ? 'text-warning-light' : 'text-text-muted');
              return (
                <span
                  className={cn('text-[10px] font-mono', cls)}
                  aria-label={`snapshot age ${(ageMs / 1000).toFixed(1)}s`}
                >
                  {fmtAgeSec(ageMs)}
                </span>
              );
            })()}
            {showBalanceFail ? (
              <span className="text-[10px] font-mono text-danger-light" title="Balance readiness FAIL (hedge leg may be constrained)">
                bal!
              </span>
            ) : null}
            {reason && mode !== 'QUOTING' ? (
              <span
                className="text-[10px] text-text-muted max-w-[140px] truncate"
                title={reason}
              >
                {reason}
              </span>
            ) : null}
          </span>
        </SimpleTooltip>
      </div>
      {/* Prices are last-known when mode indicates staleness. */}
      <div className={cn("flex gap-2 text-[11px] font-mono", mode.includes("STALE") && "opacity-60") }>
        <span className="text-success-light">{fmtMaybe(data.bid)}</span>
        <span className="text-danger-light">{fmtMaybe(data.ask)}</span>
      </div>
      {data.isMakerLeg && (cancelBid != null || cancelAsk != null) ? (
        <div className={cn("flex items-center gap-2 text-[10px] font-mono text-text-muted", mode.includes("STALE") && "opacity-60") }>
          <span className="font-semibold">Cancel:</span>
          <span className="text-success-light">{fmtMaybe(cancelBid)}</span>
          <span className="text-danger-light">{fmtMaybe(cancelAsk)}</span>
        </div>
      ) : null}
    </div>
  );
});
MakerTruthRow.displayName = 'MakerTruthRow';

const MakerAwareLegCell: FC<{ row: EnrichedRow; legKey: LegKey; showQuoted: boolean; nowMs: number; pathProfile: PathProfile }> = memo(({
  row,
  legKey,
  showQuoted,
  nowMs,
  pathProfile,
}) => {
  const quoteSnapshot = resolveQuoteSnapshot(row);
  const hasMakerOverlay = !!quoteSnapshot;
  const leg = resolveDisplayedLeg(row, legKey);
  const sourceLabel = pathProfile === 'equities' && legKey === 'B'
    ? getConfiguredSignalSourceLabel(leg)
    : null;

  if (!hasMakerOverlay) {
    return <LegCell leg={leg} showQuoted={showQuoted} sourceLabel={sourceLabel} />;
  }

  return (
    <div className="flex flex-col">
        <LegCell
          leg={leg}
          showQuoted={showQuoted}
          tooltipBehavior="icon"
          contextHint="Maker: Row 2 shows the actual maker snapshot (Our / Ref)"
          sourceLabel={sourceLabel}
        />
      {quoteSnapshot ? (
        <MakerTruthRow row={row} legKey={legKey} quoteSnapshot={quoteSnapshot} nowMs={nowMs} />
      ) : null}
    </div>
  );
});
MakerAwareLegCell.displayName = 'MakerAwareLegCell';

const LiveAgeCell: FC<{ row: EnrichedRow; nowProvider: () => number; visibilityRoot?: Element | null }> = memo(({
  row,
  nowProvider,
  visibilityRoot,
}) => {
  const { nowMs, targetRef } = useVisibleNowMs<HTMLSpanElement>({
    intervalMs: 1000,
    nowProvider,
    root: visibilityRoot,
  });
  const ageInfo = useMemo(() => computeStrategyAge(row, nowMs), [row, nowMs]);
  const ageMs = ageInfo.displayAgeMs;
  const ageSec = (ageMs / 1000).toFixed(1);
  const ageA = ageInfo.perLeg.A?.ageMs;
  const ageB = ageInfo.perLeg.B?.ageMs;
  const recentMs = ageInfo.recentAgeMs ?? ageMs;
  const staleSide = ageA !== undefined && ageB !== undefined ? (ageA >= ageB ? 'A' : 'B') : undefined;
  const tipLines: string[] = [];
  if (ageA !== undefined) tipLines.push(`Strategy market (A): ${fmtAgeSec(ageA)}`);
  if (ageB !== undefined) tipLines.push(`FV market (B): ${fmtAgeSec(ageB)}`);
  if (staleSide) tipLines.push(`Stale leg: ${staleSide}`);
  const tip = tipLines.length > 0 ? tipLines.join(' • ') : undefined;
  const skewedStale = ageMs > 10_000 && recentMs <= 2_000;

  return (
    <SimpleTooltip content={tip} delay={150}>
      <span
        ref={targetRef}
        className="text-right font-mono inline-flex w-full items-center justify-end gap-1"
        style={{ color: getAgeColor(ageMs) }}
      >
        {ageSec}s
        {skewedStale && (
          <span
            className="inline-block h-2 w-2 rounded-full bg-warning-light align-middle"
            aria-label="Potential stale leg"
          />
        )}
      </span>
    </SimpleTooltip>
  );
});
LiveAgeCell.displayName = 'LiveAgeCell';

const LiveLastUpdatedCell: FC<{ row: EnrichedRow; nowProvider: () => number; visibilityRoot?: Element | null }> = memo(({
  row,
  nowProvider,
  visibilityRoot,
}) => {
  const { nowMs, targetRef } = useVisibleNowMs<HTMLSpanElement>({
    intervalMs: 1000,
    nowProvider,
    root: visibilityRoot,
  });
  const ageInfo = useMemo(() => computeStrategyAge(row, nowMs), [row, nowMs]);
  const lastUpdateMs = ageInfo.mostRecentTsMs ?? row._lastUpdateMs;
  const hasMs = typeof lastUpdateMs === 'number' && Number.isFinite(lastUpdateMs);
  const local = hasMs
    ? formatLocal(lastUpdateMs)
    : (row._lastUpdate ? formatLocal(row._lastUpdate) : 'N/A');
  const recentMs = ageInfo.recentAgeMs ?? ageInfo.displayAgeMs ?? row._minAge ?? row._maxAge ?? 0;
  const ageSec = Math.max(0, Math.floor(recentMs / 1000));
  const recentSide = ageInfo.mostRecentSide ?? row._recentSide;
  const legLabel = recentSide ? `Leg ${recentSide}` : 'Newest leg';
  const detail = `${legLabel}: ${local}\nRecency: ${fmtAgeSec(recentMs)}`;

  return (
    <SimpleTooltip content={detail} delay={150}>
      <span ref={targetRef} className="text-neutral-300 text-xs font-mono">
        {local}
        {local !== 'N/A' && <span className="text-neutral-500"> ({ageSec}s ago)</span>}
      </span>
    </SimpleTooltip>
  );
});
LiveLastUpdatedCell.displayName = 'LiveLastUpdatedCell';

// =============================================================================
// MAIN COMPONENT
// =============================================================================

export default function SignalTable({
  onRemove,
  showHeader = true,
}: {
  onRemove?: () => void;
  showHeader?: boolean;
} = {}) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const location = useLocation();
  const pathProfile = useMemo<PathProfile>(() => {
    const firstSegment = (location.pathname.split('/').filter(Boolean)[0] || '').toLowerCase();
    return resolvePathProfile(firstSegment);
  }, [location.pathname]);
  const isMakerSuiteProfile = useMemo(() => isMakerSuiteSignalProfile(pathProfile), [pathProfile]);
  const [familyScope, setFamilyScope] = useState<SignalFamilyScope>(() => defaultFamilyScopeForProfile(pathProfile));

  useEffect(() => {
    setFamilyScope(defaultFamilyScopeForProfile(pathProfile));
  }, [pathProfile]);

  const isStrategyVisible = useCallback((strategy: SignalStrategy): boolean => {
    return matchesSignalProfile(pathProfile, strategy);
  }, [pathProfile]);
  const isFamilyVisible = useCallback((strategy: SignalStrategy): boolean => {
    const effectiveFamilyScope: SignalFamilyScope = familyScope;
    if (effectiveFamilyScope === 'all') return true;
    return resolveSignalFamilyForPath(pathProfile, strategy) === effectiveFamilyScope;
  }, [familyScope, pathProfile]);
  // Select from zustand store with shallow equality to reduce re-renders
  const rows = useSignalStore(selectSignalRows, shallow);
  const setRows = useSignalStore(s => s.setRows);
  const mergeStrategy = useSignalStore(s => s.mergeStrategy);
  const mergeStrategies = useSignalStore(s => s.mergeStrategies);
  const signalStandardEnabled = useMemo(() => isRealtimeStandardEnabled('signal'), []);
  const [loading, setLoading] = useState(true);
  const [serverTime, setServerTime] = useState<string | null>(null);
  // Keep a ref for serverTime to avoid effect re-subscriptions on each tick
  const serverTimeRef = useRef<string | null>(null);
  // Numeric server timestamp (ms) to avoid client/server drift in Age
  const [serverTsMs, setServerTsMs] = useState<number | null>(null);
  const serverTsMsRef = useRef<number | null>(null);
  const serverPerfNowRef = useRef<number | null>(null);
  const [filters, setFilters] = useState<FilterValues>({});
  const [wsConnected, setWsConnected] = useState(false);
  const [surfaceState, setSurfaceState] = useState<RealtimeSurfaceState>(() => (
    signalStandardEnabled ? RealtimeSurfaceState.SYNCING : RealtimeSurfaceState.LIVE
  ));
  const [standardLineage, setStandardLineage] = useState<RealtimeSnapshotLineage | null>(null);
  const [showQuoted, setShowQuoted] = useState(false);
  const [lastUpdate, setLastUpdate] = useState<number>(Date.now());
  // Track lastUpdate via ref to avoid resubscribing effects when checking staleness
  const lastUpdateRef = useRef<number>(Date.now());
  const lastStandardActivityRef = useRef<number>(Date.now());
  // Sticky rows support: remember time of last non-empty dataset
  const lastNonEmptyRef = useRef<number | null>(null);
  // Track when we first saw an empty snapshot while rows existed.
  const emptySinceRef = useRef<number | null>(null);
  // Distinguish an acknowledged empty realtime view from a stale non-empty one.
  const emptyViewAcknowledgedRef = useRef<boolean>(false);
  // Keep IDs of currently visible strategies to accept profile-compatible deltas
  // that omit full metadata.
  const visibleIdSetRef = useRef<Set<string>>(new Set());
  // Monotonic sequence IDs prevent older REST responses from overriding newer state.
  const restRequestSeqRef = useRef<number>(0);
  const restAppliedSeqRef = useRef<number>(0);
  const [refreshing, setRefreshing] = useState(false);
  const [balanceSummary, setBalanceSummary] = useState<BalanceSummary | null>(null);
  const { isMobile } = useMobileLayout();
  const tableClassName = isMobile ? 'w-full' : 'min-w-[1200px]';
  const initialSorting = useMemo<SortingState>(() => ([
    { id: 'trading_enabled', desc: true },
    { id: 'id', desc: false },
  ]), []);
  const [sortingState, setSortingState] = useState<SortingState>(() => initialSorting);
  const [ageSortTick, setAgeSortTick] = useState(0);
  const [visibilityRoot, setVisibilityRoot] = useState<HTMLDivElement | null>(null);
  const isAgeSortActive = useMemo(
    () => sortingState.some((sort) => sort.id === 'age_ms'),
    [sortingState]
  );
  const hasCustomSort = useMemo(
    () => !isEqual(sortingState, initialSorting),
    [initialSorting, sortingState],
  );
  const familyCounts = useMemo(() => {
    const base = (rows || []).filter(isStrategyVisible);
    return {
      all: base.length,
      equities_maker: base.filter((r) => resolveSignalFamilyForPath(pathProfile, r) === 'equities_maker').length,
      equities_taker: base.filter((r) => resolveSignalFamilyForPath(pathProfile, r) === 'equities_taker').length,
      maker_v4: base.filter((r) => resolveSignalFamilyForPath(pathProfile, r) === 'maker_v4').length,
      maker_v3: base.filter((r) => resolveSignalFamilyForPath(pathProfile, r) === 'maker_v3').length,
      maker_v2: base.filter((r) => resolveSignalFamilyForPath(pathProfile, r) === 'maker_v2').length,
      taker: base.filter((r) => resolveSignalFamilyForPath(pathProfile, r) === 'taker').length,
    };
  }, [rows, isStrategyVisible, pathProfile]);

  const realtimeController = useMemo(
    () =>
      createRealtimeSurfaceController<EnrichedRow>({
        getRowId: (row) => row.id,
      }),
    [],
  );
  const { rows: realtimeRows, dataVersion: liveDataVersion } = useRealtimeSurfaceController(
    realtimeController,
    (snapshot) => ({
      rows: snapshot.rows as EnrichedRow[],
      dataVersion: snapshot.dataVersion,
    }),
    (left, right) => left.rows === right.rows && left.dataVersion === right.dataVersion,
  );
  const previousDisplayedSourceRowsRef = useRef<Map<string, SignalStrategy>>(new Map());
  const previousDisplayedIdsRef = useRef<string[]>([]);
  const previousAgeSortTickRef = useRef(ageSortTick);
  const wsConnectedRef = useRef(false);
  const manualRefreshRequiredRef = useRef(false);
  const standardLineageRef = useRef<RealtimeSnapshotLineage | null>(null);
  const standardCursorSeqRef = useRef(0);
  const snapshotGenerationRef = useRef(0);
  const resetRecoveryRef = useRef<() => void>(() => undefined);
  const makerV4ChangedRowIdsRef = useRef<readonly string[] | null>(null);

  useEffect(() => {
    return () => {
      realtimeController.destroy();
    };
  }, [realtimeController]);

  useEffect(() => {
    visibleIdSetRef.current = new Set((rows || []).map((r) => r.id));
  }, [rows]);

  const setServerClock = useCallback((tsMs?: number | null) => {
    if (typeof tsMs === 'number' && Number.isFinite(tsMs)) {
      setServerTsMs(tsMs);
      serverTsMsRef.current = tsMs;
      serverPerfNowRef.current = performance.now();
    } else {
      setServerTsMs(null);
      serverTsMsRef.current = null;
      serverPerfNowRef.current = performance.now();
    }
  }, []);

  const getServerNowMs = useCallback(() => {
    const base = serverTsMsRef.current;
    const perfBase = serverPerfNowRef.current;
    if (base != null && perfBase != null) {
      return base + (performance.now() - perfBase);
    }
    return Date.now();
  }, []);

  // Keep age sort aligned with live age ticking without forcing global table re-sorts.
  useEffect(() => {
    if (!isAgeSortActive || rows.length === 0) return;

    const intervalId = window.setInterval(() => {
      setAgeSortTick((tick) => tick + 1);
    }, 1000);

    return () => window.clearInterval(intervalId);
  }, [isAgeSortActive, rows.length]);

  const handleVisibilityRootRef = useCallback((node: HTMLDivElement | null) => {
    scrollRef.current = node;
    setVisibilityRoot((prev) => (prev === node ? prev : node));
  }, []);

  // Helper to update both state and ref
  const updateWsConnected = (connected: boolean) => {
    wsConnectedRef.current = connected;
    setWsConnected(connected);
  };

  const requireManualRefresh = useCallback(() => {
    manualRefreshRequiredRef.current = true;
    snapshotGenerationRef.current += 1;
    standardLineageRef.current = null;
    standardCursorSeqRef.current = 0;
    resetRecoveryRef.current();
    setStandardLineage(null);
    setSurfaceState(RealtimeSurfaceState.MANUAL_REFRESH_REQUIRED);
  }, []);

  const applyStandardSnapshotLineage = useCallback((nextLineage: RealtimeSnapshotLineage) => {
    const previousLineage = standardLineageRef.current;
    const nextLastSeq = Math.max(0, Math.trunc(nextLineage.last_seq));
    const sameEpoch = Boolean(
      previousLineage
      && previousLineage.contract_version === nextLineage.contract_version
      && previousLineage.surface === nextLineage.surface
      && previousLineage.profile === nextLineage.profile
      && previousLineage.surface_query_key === nextLineage.surface_query_key
      && previousLineage.stream_id === nextLineage.stream_id
      && String(previousLineage.snapshot_revision) === String(nextLineage.snapshot_revision),
    );
    standardLineageRef.current = { ...nextLineage };
    standardCursorSeqRef.current = sameEpoch
      ? Math.max(standardCursorSeqRef.current, nextLastSeq)
      : nextLastSeq;
    setStandardLineage({ ...nextLineage });
  }, []);

  const advanceStandardCursor = useCallback((seq: unknown) => {
    if (typeof seq !== 'number' || !Number.isFinite(seq)) {
      return;
    }
    standardCursorSeqRef.current = Math.max(standardCursorSeqRef.current, Math.trunc(seq));
  }, []);

  const markRowsNonEmpty = useCallback((nowMs?: number) => {
    const now = nowMs ?? Date.now();
    lastNonEmptyRef.current = now;
    emptySinceRef.current = null;
    emptyViewAcknowledgedRef.current = false;
  }, []);

  const hasAcknowledgedEmptyView = useCallback((): boolean => {
    const currentRows = useSignalStore.getState().rows || [];
    return emptyViewAcknowledgedRef.current && currentRows.length === 0;
  }, []);

  const handleEmptyVisibleSnapshot = useCallback((requestStartedAtMs?: number) => {
    const currentRows = useSignalStore.getState().rows || [];
    const policy = evaluateEmptySnapshotPolicy({
      hasExistingRows: currentRows.length > 0,
      wsConnected: wsConnectedRef.current,
      nowMs: Date.now(),
      emptySinceMs: emptySinceRef.current,
      holdWindowMs: EMPTY_SNAPSHOT_HOLD_MS,
      requestStartedAtMs,
      lastNonEmptyAtMs: lastNonEmptyRef.current,
    });
    const refreshedAtMs = requestStartedAtMs ?? Date.now();
    const willExposeEmptyView = policy.clearRows || currentRows.length === 0;
    emptySinceRef.current = policy.nextEmptySinceMs;
    emptyViewAcknowledgedRef.current = willExposeEmptyView;
    if (willExposeEmptyView) {
      setLastUpdate(refreshedAtMs);
      lastUpdateRef.current = refreshedAtMs;
    }
    if (policy.clearRows && currentRows.length > 0) {
      setRows([]);
    }
  }, [setLastUpdate, setRows]);

  const applyVisibleSnapshotRows = useCallback((incomingRows: SignalStrategy[], requestStartedAtMs?: number) => {
    if (incomingRows.length > 0) {
      setRows(incomingRows);
      markRowsNonEmpty();
      return;
    }
    handleEmptyVisibleSnapshot(requestStartedAtMs);
  }, [handleEmptyVisibleSnapshot, markRowsNonEmpty, setRows]);

  const fetchSnapshot = useCallback(async ({ markRefreshing = false }: { markRefreshing?: boolean } = {}) => {
    const requestStartedAtMs = Date.now();
    const requestSeq = ++restRequestSeqRef.current;
    const snapshotGeneration = snapshotGenerationRef.current;
    if (markRefreshing) {
      setRefreshing(true);
    }

    try {
      const data = await api.getSignalStrategies(
        signalStandardEnabled ? { contractVersion: 2 } : undefined,
      );
      if (snapshotGeneration !== snapshotGenerationRef.current) return false;
      if (requestSeq < restAppliedSeqRef.current) return false;
      restAppliedSeqRef.current = requestSeq;

      const all = (data.strategies || []) as SignalStrategy[];
      const filtered = all.filter(isStrategyVisible);
      applyVisibleSnapshotRows(filtered, requestStartedAtMs);

      const fetchedServerTsMs = typeof (data as any).server_ts_ms === 'number'
        ? (data as any).server_ts_ms as number
        : requestStartedAtMs;
      const nextLastDataTsMs = resolveSignalDataFreshnessTsMs(filtered, fetchedServerTsMs);
      setLastUpdate(nextLastDataTsMs);
      lastUpdateRef.current = nextLastDataTsMs;

      const fetchedServerTime = data.server_time || new Date().toISOString().slice(0, 19).replace('T', ' ');
      setServerTime(fetchedServerTime);
      serverTimeRef.current = fetchedServerTime;

      setServerClock(fetchedServerTsMs);
      setBalanceSummary(data.balance_summary ?? null);
      if (signalStandardEnabled) {
        if (data.realtime) {
          manualRefreshRequiredRef.current = false;
          lastStandardActivityRef.current = requestStartedAtMs;
          applyStandardSnapshotLineage(data.realtime);
          setSurfaceState(RealtimeSurfaceState.LIVE);
        } else {
          requireManualRefresh();
        }
      } else {
        standardLineageRef.current = null;
        standardCursorSeqRef.current = 0;
        setStandardLineage(null);
        setSurfaceState(RealtimeSurfaceState.LIVE);
      }
      setLoading(false);
      if (!signalStandardEnabled || Boolean(data.realtime)) {
        resetRecoveryRef.current();
        return true;
      }
      return false;
    } catch (error) {
      if (import.meta.env?.DEV) {
        console.error('[signal] Failed to load:', error);
      }
      setLoading(false);
      return false;
    } finally {
      if (markRefreshing) {
        setRefreshing(false);
      }
    }
  }, [
    applyStandardSnapshotLineage,
    applyVisibleSnapshotRows,
    isStrategyVisible,
    requireManualRefresh,
    setServerClock,
    signalStandardEnabled,
  ]);

  const handleRecoveryFetch = useCallback(() => {
    if (manualRefreshRequiredRef.current) {
      return;
    }
    void fetchSnapshot();
  }, [fetchSnapshot]);

  const recovery = useRecoveryScheduler({
    baseDelayMs: 1_000,
    maxDelayMs: 8_000,
    onRecover: handleRecoveryFetch,
  });
  const recoveryPending = recovery.pending;
  const resolveAcknowledgedEmptyViewState = useCallback((): RealtimeSurfaceState | null => {
    if (!hasAcknowledgedEmptyView()) {
      return null;
    }
    return resolveSignalTransportState(
      lastStandardActivityRef.current,
      Date.now(),
      recoveryPending,
    );
  }, [hasAcknowledgedEmptyView, recoveryPending]);
  const scheduleRecovery = useCallback((reason?: string) => {
    recovery.scheduler.schedule(reason);
  }, [recovery.scheduler]);
  const resetRecovery = useCallback(() => {
    recovery.scheduler.reset();
  }, [recovery.scheduler]);
  resetRecoveryRef.current = resetRecovery;

  const handleRefresh = useCallback(async () => {
    const refreshed = await fetchSnapshot({ markRefreshing: true });
    if (refreshed) {
      resetRecovery();
    }
  }, [fetchSnapshot, resetRecovery]);

  const syncRealtimeEnvelope = useCallback((data: any) => {
    const nextServerTime = data?.server_time || new Date().toISOString().slice(0, 19).replace('T', ' ');
    const nextServerTsMs = typeof data?.server_ts_ms === 'number' ? data.server_ts_ms as number : null;
    if (nextServerTime !== serverTimeRef.current || nextServerTsMs !== serverTsMsRef.current) {
      setServerTime(nextServerTime);
      serverTimeRef.current = nextServerTime;
      setServerClock(nextServerTsMs);
    }
    if (data?.balance_summary) {
      setBalanceSummary(data.balance_summary);
    }
  }, [setServerClock]);

  const scheduleInvalidation = useCallback((reason: string) => {
    if (manualRefreshRequiredRef.current) {
      return;
    }
    if (signalStandardEnabled) {
      setSurfaceState(RealtimeSurfaceState.RECOVERING);
    }
    scheduleRecovery(reason);
  }, [scheduleRecovery, signalStandardEnabled]);

  const applySignalDeltaPayload = useCallback((delta: any, fallbackTsMs?: number) => {
    try {
      const payload = (delta && typeof delta === 'object' && delta.patch && typeof delta.patch === 'object')
        ? {
            id: (delta as any).strategy_id ?? (delta as any).id,
            ...(delta as any).patch,
            ts_ms: (delta as any).server_ts_ms ?? (delta as any).ts_ms ?? fallbackTsMs,
          }
        : {
            ...(delta ?? {}),
            ts_ms: (delta as any)?.ts_ms ?? (delta as any)?.server_ts_ms ?? fallbackTsMs,
          };
      const id = payload?.id;
      if (!id) return;

      const hasMeta = !!(payload?.meta && typeof payload.meta === 'object');
      const knownInView = visibleIdSetRef.current.has(id);
      if (!hasMeta && !knownInView) {
        scheduleInvalidation('signal_delta.invalidate');
        return;
      }

      const visibleByProfile = matchesSignalProfile(pathProfile, payload as SignalStrategy);
      if (!visibleByProfile && (hasMeta || !visibleIdSetRef.current.has(id))) {
        return;
      }

      const apply: Partial<SignalStrategy> = { id } as any;
      const passThroughKeys = new Set([
        'meta',
        'strategy_family',
        'decision_edge_bps',
        'edge2_bps',
        'required_edge_bps',
        'spread_net_bps',
        'spread_net_case1_bps',
        'spread_net_case2_bps',
        'spread_net_best_case',
        'risk_delta',
        'risk_delta_ts_ms',
        'maker_v2',
        'maker_v3',
        'maker_role_map',
        'maker_quote_status',
        'quote_stacks',
        'pricing_adjustments',
        'legs_order',
        'state',
        'tradeable',
        'blocked',
        'near_tradeable',
        'managed_orders',
        'params',
        'balance_readiness',
        'balances_ok',
        'last_trade',
        'equities_arb',
        'maker_v4',
      ]);

      for (const [key, value] of Object.entries(payload || {})) {
        if (key === 'id' || key === 'legs' || !passThroughKeys.has(key)) continue;
        (apply as any)[key] = value;
      }

      if (payload && typeof payload === 'object' && 'legs_order' in payload) {
        if ((payload as any).legs_order === null) {
          (apply as any).legs_order = null;
        } else if (Array.isArray((payload as any).legs_order)) {
          (apply as any).legs_order = (payload as any).legs_order.filter((key: unknown) => typeof key === 'string');
        }
      }

      const legPatch = buildLegDeltaPatch(normalizeDeltaLegs((payload as any).legs));
      if (legPatch) {
        (apply as any).legs = legPatch;
      }

      if (typeof mergeStrategy === 'function') {
        mergeStrategy(apply as SignalStrategy);
      }
      if (typeof payload.ts_ms === 'number' && Number.isFinite(payload.ts_ms)) {
        setServerClock(payload.ts_ms as number);
      }
      const nextLastDataTsMs = typeof payload.ts_ms === 'number' && Number.isFinite(payload.ts_ms)
        ? payload.ts_ms as number
        : (typeof fallbackTsMs === 'number' && Number.isFinite(fallbackTsMs) ? fallbackTsMs : Date.now());
      if ((useSignalStore.getState().rows || []).length > 0) {
        markRowsNonEmpty(nextLastDataTsMs);
      }
      setLastUpdate(nextLastDataTsMs);
      lastUpdateRef.current = nextLastDataTsMs;
    } catch (error) {
      if (import.meta.env?.DEV) {
        console.error('[signal] Delta handler failed:', error);
      }
    }
  }, [markRowsNonEmpty, mergeStrategy, pathProfile, scheduleInvalidation, setServerClock]);

  const handleStandardRealtimeEvent = useCallback((event: {
    kind: string;
    seq: number;
    server_ts_ms: number;
    reason?: string;
    payload?: any;
  }) => {
    lastStandardActivityRef.current = Date.now();
    syncRealtimeEnvelope({
      server_ts_ms: event.server_ts_ms,
    });
    advanceStandardCursor(event.seq);

    if (event.kind === 'heartbeat') {
      if (!manualRefreshRequiredRef.current) {
        const emptyViewState = resolveAcknowledgedEmptyViewState();
        if (emptyViewState != null) {
          setSurfaceState(emptyViewState);
          return;
        }
        const nextState = resolveSignalFreshnessState(
          lastUpdateRef.current,
          Date.now(),
          recoveryPending,
        );
        if (nextState != null) {
          setSurfaceState(nextState);
        }
      }
      return;
    }

    if (event.kind === 'invalidate') {
      scheduleInvalidation(`realtime_event.invalidate:${String(event.reason ?? 'invalidate')}`);
      return;
    }

    if (event.kind !== 'delta_batch') {
      return;
    }

    const payload = event.payload ?? {};
    const signalRows = Array.isArray(payload.signals) ? payload.signals : [];
    const changedIds = Array.isArray(payload?.strategies?.changed)
      ? payload.strategies.changed.filter((value: unknown) => typeof value === 'string')
      : [];
    const payloadIds = new Set(
      signalRows
        .map((row: any) => String(row?.id ?? row?.strategy_id ?? '').trim())
        .filter((value: string) => value.length > 0),
    );

    for (const row of signalRows) {
      applySignalDeltaPayload(row, event.server_ts_ms);
    }

    if (changedIds.some((id) => !payloadIds.has(id))) {
      scheduleInvalidation('realtime_event.delta_batch.invalidate');
      return;
    }

    if (!manualRefreshRequiredRef.current) {
      setSurfaceState(RealtimeSurfaceState.LIVE);
    }
  }, [advanceStandardCursor, applySignalDeltaPayload, resolveAcknowledgedEmptyViewState, recoveryPending, scheduleInvalidation, syncRealtimeEnvelope]);

  useStandardWebSocketSubscription({
    enabled: signalStandardEnabled && surfaceState !== RealtimeSurfaceState.MANUAL_REFRESH_REQUIRED,
    lineage: signalStandardEnabled ? standardLineage : null,
    resumeFromSeq: () => standardCursorSeqRef.current,
    onEvent: handleStandardRealtimeEvent,
    onFailure: () => {
      requireManualRefresh();
    },
    onSubscribed: (ack) => {
      lastStandardActivityRef.current = Date.now();
      if (typeof ack.last_seq === 'number' && Number.isFinite(ack.last_seq)) {
        standardCursorSeqRef.current = Math.max(standardCursorSeqRef.current, Math.trunc(ack.last_seq));
      }
      if (!manualRefreshRequiredRef.current) {
        const emptyViewState = resolveAcknowledgedEmptyViewState();
        if (emptyViewState != null) {
          setSurfaceState(emptyViewState);
          return;
        }
        const nextState = resolveSignalFreshnessState(
          lastUpdateRef.current,
          Date.now(),
          recoveryPending,
        );
        if (nextState != null) {
          setSurfaceState(nextState);
        }
      }
    },
  });

  useEffect(() => {
    if (!signalStandardEnabled) {
      return undefined;
    }
    if (surfaceState === RealtimeSurfaceState.MANUAL_REFRESH_REQUIRED) {
      return undefined;
    }

    const intervalId = window.setInterval(() => {
      if (manualRefreshRequiredRef.current) {
        return;
      }
      if (surfaceState === RealtimeSurfaceState.RECOVERING) {
        return;
      }
      const emptyViewState = resolveAcknowledgedEmptyViewState();
      if (emptyViewState != null) {
        setSurfaceState(emptyViewState);
        if (emptyViewState === RealtimeSurfaceState.STALE) {
          if (!recoveryPending) {
            scheduleInvalidation('signal.watchdog.stale');
          } else {
            setSurfaceState(RealtimeSurfaceState.RECOVERING);
          }
        }
        return;
      }

      const lastDataTsMs = lastUpdateRef.current;
      const nextState = resolveSignalFreshnessState(
        lastDataTsMs,
        Date.now(),
        recoveryPending,
      );
      if (nextState == null) {
        return;
      }

      if (nextState === RealtimeSurfaceState.STALE) {
        setSurfaceState(RealtimeSurfaceState.STALE);
        if (!recoveryPending) {
          scheduleInvalidation('signal.watchdog.stale');
        } else {
          setSurfaceState(RealtimeSurfaceState.RECOVERING);
        }
        return;
      }

      setSurfaceState(nextState);
    }, 1_000);

    return () => {
      window.clearInterval(intervalId);
    };
  }, [recoveryPending, resolveAcknowledgedEmptyViewState, scheduleInvalidation, signalStandardEnabled, surfaceState]);

  useEffect(() => {
    void fetchSnapshot();

    const handleMarketUpdate = (data: any) => {
      try {
        if (Array.isArray(data?.strategies)) {
          if (data.strategies.length > 0) {
            const allowed = visibleIdSetRef.current;
            const filtered = (data.strategies as SignalStrategy[]).filter((strategy) => {
              if (matchesSignalProfile(pathProfile, strategy)) return true;
              const hasMeta = !!(strategy.meta && typeof strategy.meta === 'object');
              return !hasMeta && allowed.has(strategy.id);
            });
            if (filtered.length > 0) {
              if (typeof mergeStrategies === 'function') {
                mergeStrategies(filtered);
              } else {
                setRows(filtered);
              }
              const incomingIds = new Set(filtered.map((strategy) => strategy.id));
              const latestRows = useSignalStore.getState().rows || [];
              if (latestRows.some((row) => !incomingIds.has(row.id))) {
                setRows(latestRows.filter((row) => incomingIds.has(row.id)));
              }
              const nextLastDataTsMs = resolveSignalDataFreshnessTsMs(
                filtered,
                typeof data?.server_ts_ms === 'number' ? data.server_ts_ms as number : Date.now(),
              );
              markRowsNonEmpty(nextLastDataTsMs);
              setLastUpdate(nextLastDataTsMs);
              lastUpdateRef.current = nextLastDataTsMs;
            } else {
              handleEmptyVisibleSnapshot();
            }
          } else {
            handleEmptyVisibleSnapshot();
          }

          setLoading(false);
          syncRealtimeEnvelope(data);
          resetRecovery();
          return;
        }

        const changed = Array.isArray(data?.strategies?.changed)
          ? data.strategies.changed.filter((item: unknown) => typeof item === 'string')
          : [];
        if (changed.length > 0) {
          syncRealtimeEnvelope(data);
          scheduleInvalidation('market_update.invalidate');
        }

        if (data?.param_update?.strategy_id && data?.param_update?.parameters) {
          const strategyId = data.param_update.strategy_id as string;
          if (visibleIdSetRef.current.size > 0 && !visibleIdSetRef.current.has(strategyId)) return;
          if (typeof mergeStrategy === 'function') {
            mergeStrategy({ id: strategyId, params: data.param_update.parameters as Record<string, unknown> } as SignalStrategy);
          }
          const now = Date.now();
          if ((useSignalStore.getState().rows || []).length > 0) {
            markRowsNonEmpty(now);
          }
          setLastUpdate(now);
          lastUpdateRef.current = now;
        }
      } catch (error) {
        if (import.meta.env?.DEV) {
          console.error('[signal] Market update handler failed:', error);
        }
      }
    };

    const handleSignalDelta = (delta: any) => {
      applySignalDeltaPayload(delta);
    };

    const handleConnect = () => {
      if (import.meta.env?.DEV) {
        console.log('[signal] WebSocket connected');
      }
      updateWsConnected(true);
      if (manualRefreshRequiredRef.current) {
        return;
      }
      if (signalStandardEnabled && standardLineageRef.current) {
        return;
      }
      resetRecovery();
      void fetchSnapshot();
    };

    const handleDisconnect = () => {
      if (import.meta.env?.DEV) {
        console.log('[signal] WebSocket disconnected');
      }
      updateWsConnected(false);
      scheduleInvalidation('socket.disconnect');
    };

    const handleConnectError = (error: any) => {
      if (import.meta.env?.DEV) {
        console.error('[signal] WebSocket connect_error:', error?.message || error);
      }
      updateWsConnected(false);
      scheduleInvalidation(`socket.connect_error:${String(error?.message || error || 'unknown')}`);
    };

    const handleReconnectAttempt = (attempt: number) => {
      if (import.meta.env?.DEV) {
        console.log('[signal] WebSocket reconnect_attempt:', attempt);
      }
      scheduleInvalidation(`socket.reconnect_attempt:${attempt}`);
    };

    socket.on('connect', handleConnect);
    socket.on('disconnect', handleDisconnect);
    socket.on('connect_error', handleConnectError);
    socket.on('reconnect_attempt', handleReconnectAttempt);
    if (!signalStandardEnabled) {
      socket.on('market_update', handleMarketUpdate);
      socket.on('signal_delta', handleSignalDelta);
    }

    updateWsConnected(Boolean(socket.connected));

    return () => {
      socket.off('connect', handleConnect);
      socket.off('disconnect', handleDisconnect);
      socket.off('connect_error', handleConnectError);
      socket.off('reconnect_attempt', handleReconnectAttempt);
      if (!signalStandardEnabled) {
        socket.off('market_update', handleMarketUpdate);
        socket.off('signal_delta', handleSignalDelta);
      }
    };
  }, [
    applySignalDeltaPayload,
    applyVisibleSnapshotRows,
    fetchSnapshot,
    handleEmptyVisibleSnapshot,
    markRowsNonEmpty,
    mergeStrategies,
    mergeStrategy,
    pathProfile,
    resetRecovery,
    scheduleInvalidation,
    setRows,
    signalStandardEnabled,
    syncRealtimeEnvelope,
  ]);

  useEffect(() => {
    const serverNowMs = getServerNowMs();
    const visibleRows = (rows || []).filter((row) => isStrategyVisible(row) && isFamilyVisible(row) && Boolean(row.legs));
    const nextIds = visibleRows.map((row) => row.id);
    const previousIds = previousDisplayedIdsRef.current;
    const tickChanged = previousAgeSortTickRef.current !== ageSortTick;
    const orderChanged = tickChanged
      || nextIds.length !== previousIds.length
      || nextIds.some((id, index) => id !== previousIds[index]);

    if (orderChanged || hasCustomSort) {
      makerV4ChangedRowIdsRef.current = null;
      realtimeController.applySnapshot(
        visibleRows
          .map((row) => buildEnrichedSignalRow(row, serverNowMs))
          .filter((row): row is EnrichedRow => row !== null),
      );
      setSortingState((current) => (current.length > 0 ? [...current] : current));
    } else {
      const previousRowsById = previousDisplayedSourceRowsRef.current;
      const deltas: RealtimeRowDelta<EnrichedRow>[] = [];

      visibleRows.forEach((row) => {
        if (previousRowsById.get(row.id) === row) {
          return;
        }
        const nextRow = buildEnrichedSignalRow(row, serverNowMs);
        if (nextRow) {
          deltas.push({ kind: 'upsert', row: nextRow });
        }
      });

      if (deltas.length > 0) {
        makerV4ChangedRowIdsRef.current = deltas
          .filter((delta): delta is Extract<RealtimeRowDelta<EnrichedRow>, { kind: 'upsert' }> => delta.kind === 'upsert')
          .map((delta) => delta.row.id);
        realtimeController.applyDelta(deltas);
        setSortingState((current) => (current.length > 0 ? [...current] : current));
      }
    }

    previousDisplayedSourceRowsRef.current = new Map(visibleRows.map((row) => [row.id, row]));
    previousDisplayedIdsRef.current = nextIds;
    previousAgeSortTickRef.current = ageSortTick;
  }, [ageSortTick, getServerNowMs, hasCustomSort, isFamilyVisible, isStrategyVisible, realtimeController, rows]);

  const enrichedRows = realtimeRows;

  const signalFilters = useMemo<ColumnFilter[]>(() => {
    if (!isMakerSuiteProfile) {
      return GENERIC_SIGNAL_FILTERS;
    }

    const assetOptions = uniqueFilterOptions(enrichedRows.map((row) => row.asset));
    const makerVenueOptions = uniqueFilterOptions(enrichedRows.map((row) => row.maker_venue));
    const makerMarketOptions = uniqueFilterOptions(enrichedRows.map((row) => row.maker_market));
    const referenceVenueOptions = uniqueFilterOptions(enrichedRows.map((row) => row.reference_venue));
    const referenceMarketOptions = uniqueFilterOptions(enrichedRows.map((row) => row.reference_market));
    const strategyClassOptions = uniqueFilterOptions(enrichedRows.map((row) => row.strategy_class));
    const chainOptions = uniqueFilterOptions(enrichedRows.map((row) => row.chain));

    const filters: ColumnFilter[] = [
      { key: 'id', label: 'Strategy', type: 'text', placeholder: 'Strategy ID...' },
      { key: 'trading_enabled', label: 'Trading', type: 'select', options: TRADING_FILTER_VALUES },
      { key: 'asset', label: 'Asset', type: 'select', options: assetOptions },
      { key: 'maker_venue', label: 'Maker Venue', type: 'select', options: makerVenueOptions },
      { key: 'maker_market', label: 'Maker Market', type: 'select', options: makerMarketOptions },
      { key: 'reference_venue', label: 'Reference Venue', type: 'select', options: referenceVenueOptions },
      { key: 'reference_market', label: 'Reference Market', type: 'select', options: referenceMarketOptions },
      { key: 'strategy_class', label: 'Class', type: 'select', options: strategyClassOptions },
    ];

    if (chainOptions.length > 0) {
      filters.push({ key: 'chain', label: 'Chain', type: 'select', options: chainOptions });
    }

    return filters;
  }, [enrichedRows, isMakerSuiteProfile, liveDataVersion]);

  // Apply filters
  const filteredRows = useMemo(() => {
    return applyFilters(enrichedRows, filters, { columns: signalFilters });
  }, [enrichedRows, filters, signalFilters, liveDataVersion]);

  const shouldUseEquitiesArbTable = useMemo(() => {
    return pathProfile === 'equities';
  }, [pathProfile]);

  const shouldUseMakerV4Table = useMemo(() => {
    if (shouldUseEquitiesArbTable) return false;
    if (familyScope === 'maker_v4') return true;
    return filteredRows.length > 0 && filteredRows.every((row) => row._strategyFamily === 'maker_v4');
  }, [familyScope, filteredRows, shouldUseEquitiesArbTable]);
  const signalRowVirtualizer = useVirtualizer<HTMLDivElement, HTMLTableRowElement>({
    count: !isMobile && !shouldUseMakerV4Table && !shouldUseEquitiesArbTable ? filteredRows.length : 0,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 44,
    overscan: 8,
  });
  const shouldVirtualizeStandardTable = !isMobile
    && !shouldUseMakerV4Table
    && !shouldUseEquitiesArbTable
    && filteredRows.length > 0
    && signalRowVirtualizer.getVirtualItems().length > 0;

  // Column definitions (TanStack Table format)
  const columns = useMemo<ColumnDef<EnrichedRow>[]>(() => {
    const buildQuotesInfo = (row: EnrichedRow) => {
      const counts = getQuoteCounts(row);
      const maker = counts.maker;
      if (!maker) return null;

      const makerSummary = maker ? `B ${maker.bidOpen}/${maker.bidDepth} · A ${maker.askOpen}/${maker.askDepth}` : '—';

      const lines = [
        'Maker quotes',
        `Source: ${counts.source}`,
        '',
      ];
      if (maker) {
        lines.push(`Bid: ${maker.bidOpen}/${maker.bidDepth} (blocked ${maker.bidBlocked})`);
        lines.push(`Ask: ${maker.askOpen}/${maker.askDepth} (blocked ${maker.askBlocked})`);
      }
      lines.push('');
      lines.push('open = working maker orders');
      lines.push('depth = target quote levels');
      lines.push('blocked = target levels not currently working');

      return {
        summaryLines: [makerSummary],
        tooltip: lines.join('\n'),
      };
    };

    return [
      {
        accessorKey: 'id',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Strategy"
            tooltip={[
              'Strategy id.',
              'Hover the row to see key params and quote semantics.',
            ].join('\n')}
          />
        ),
        enableSorting: true,
        cell: ({ row }) => (
          <SimpleTooltip content={<pre className="whitespace-pre-wrap">{buildStrategyParamTooltip(row.original)}</pre>} delay={150}>
            <span className="font-mono cursor-help">
              {row.original.id}
            </span>
          </SimpleTooltip>
        ),
      },
      {
        accessorKey: 'trading_enabled',
        id: 'trading_enabled',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Trading"
            tooltip={[
              'Can this strategy quote right now?',
              'Combines bot_on intent with live tradeability.',
              'Enabled = quoting allowed. Paused = bot off. Pending = waiting on runner or unblock.',
            ].join('\n')}
          />
        ),
        enableSorting: true,
        sortingFn: tradingSortingFn,
        cell: ({ row }) => {
          const descriptor = describeTradingStatus(row.original.status);
          const raw = resolveTradingValue(row.original as any);
          const rawStr = raw === undefined || raw === null ? 'undefined' : String(raw);
          const tooltip = [
            'Trading status',
            `State: ${descriptor.label}`,
            `Runner: ${descriptor.subLabel}`,
            `Blocked: ${resolveTradingBlocked(row.original as any) ? 'yes' : 'no'}`,
            `Resolved from: ${rawStr} (params.bot_on | state.bot_on | tradeable)`,
            '',
            'Enabled = quoting allowed',
            'Paused = bot off',
            'Pending = bot on, but not ready to quote',
            '',
            'Change Params → bot_on to control this'
          ].join('\n');
          return (
            <StatusPill
              variant={descriptor.variant}
              label={descriptor.label}
              subLabel={descriptor.subLabel}
              tooltip={tooltip}
              size="xs"
              tone="subtle"
              ariaLabel={`Trading is ${descriptor.label.toLowerCase()} for ${row.original.id}`}
            />
          );
        },
      },
      {
        accessorFn: (row) => row._globalQty,
        id: 'global_qty',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Global Qty"
            tooltip={[
              'Global inventory used for the global FV adjustment.',
              'Positive = long. Negative = short.',
              'Usually the shared account/base position for this asset.',
            ].join('\n')}
          />
        ),
        enableSorting: true,
        cell: ({ row }) => (
          <span className="text-right font-mono inline-flex w-full items-center justify-end text-neutral-200">
            {row.original._globalQty != null ? formatRiskDelta(row.original._globalQty) : '—'}
          </span>
        ),
      },
      {
        accessorFn: (row) => row._localQty,
        id: 'local_qty',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Local Qty"
            tooltip={[
              'Local inventory used for the local FV adjustment.',
              'Positive = long. Negative = short.',
              'Scoped to this strategy local bucket.',
            ].join('\n')}
          />
        ),
        enableSorting: true,
        cell: ({ row }) => (
          <span className="text-right font-mono inline-flex w-full items-center justify-end text-neutral-200">
            {row.original._localQty != null ? formatRiskDelta(row.original._localQty) : '—'}
          </span>
        ),
      },
      {
        id: 'quotes',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Quotes"
            tooltip={[
              'Open maker quotes vs target depth.',
              'Format: B open/depth · A open/depth.',
              'Hover for blocked levels and source.',
            ].join('\n')}
          />
        ),
        enableSorting: false,
        cell: ({ row }) => {
          const info = buildQuotesInfo(row.original);
          if (!info) {
            return (
              <span className="text-xs text-neutral-500">—</span>
            );
          }
          return (
            <SimpleTooltip content={<pre className="whitespace-pre-wrap">{info.tooltip}</pre>} delay={150}>
              <span className="font-mono text-xs text-neutral-200 cursor-help inline-flex flex-col items-start gap-0.5 leading-tight">
                {info.summaryLines.map((line, index) => (
                  <span key={`${line}-${index}`}>{line}</span>
                ))}
              </span>
            </SimpleTooltip>
          );
        },
      },
      {
        id: 'adj_skew',
        header: () => (
          <ColumnHeaderWithTooltip
            label="FvAdj"
            tooltip={[
              'Quoted FV shift used for maker quotes.',
              'Built from linear + global + local adjustments.',
              'Positive = quotes shift up. Negative = quotes shift down.',
            ].join('\n')}
          />
        ),
        enableSorting: false,
        cell: ({ row }) => {
          const adj = findInventorySkewAdjustment(row.original.pricing_adjustments);
          const summary = buildInventorySkewSummary(adj);
          if (!summary) {
            return <span className="text-xs text-neutral-500">—</span>;
          }
          const tooltip = buildInventorySkewTooltip(adj, row.original.params);
          return (
            <SimpleTooltip
              content={tooltip ? <pre className="whitespace-pre-wrap">{tooltip}</pre> : undefined}
              delay={150}
            >
              <span className="font-mono text-xs text-neutral-200 cursor-help">
                {summary}
              </span>
            </SimpleTooltip>
          );
        },
      },
      {
        id: 'legA',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Strategy market"
            tooltip={[
              'Strategy-side market.',
              'Top row = visible market/decision prices.',
              'Row 2 = actual maker snapshot (`Our`).',
            ].join('\n')}
          />
        ),
        enableSorting: false,
        cell: ({ row }) => (
          <MakerAwareLegCell row={row.original} legKey="A" showQuoted={showQuoted} nowMs={getServerNowMs()} pathProfile={pathProfile} />
        ),
      },
      {
        id: 'legB',
        header: () => (
          <ColumnHeaderWithTooltip
            label="FV market"
            tooltip={[
              'Reference/FV market.',
              'Top row = visible market/decision prices.',
              'Row 2 = actual reference snapshot (`Ref`).',
            ].join('\n')}
          />
        ),
        enableSorting: false,
        cell: ({ row }) => (
          <MakerAwareLegCell row={row.original} legKey="B" showQuoted={showQuoted} nowMs={getServerNowMs()} pathProfile={pathProfile} />
        ),
      },
      {
        accessorFn: (row) => row._spreadNet,
        id: 'spread_net_bps',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Spread"
            tooltip={[
              'Strategy market mid vs FV/reference mid.',
              'Positive = rich to FV. Negative = cheap to FV.',
              'Hover for the input mids and edge context.',
            ].join('\n')}
          />
        ),
        enableSorting: true,
        cell: ({ row }) => {
          const spreadBps = row.original._spreadNet;
          const quoteSnapshot = resolveQuoteSnapshot(row.original);
          const marketMid = resolveVisibleStrategyMarketMid(row.original);
          const fvMid = resolveVisibleStrategyFvMid(row.original);
          const snapshotMarketMid = quoteSnapshot
            ? midpointFromValues(
                quoteSnapshot.maker_top_bid ?? quoteSnapshot.bid,
                quoteSnapshot.maker_top_ask ?? quoteSnapshot.ask,
              )
            : null;
          const displayedFvMid = resolveDisplayedLegMid(resolveDisplayedLeg(row.original, 'B'));
          const snapshotFvMid = quoteSnapshot
            ? midpointFromValues(quoteSnapshot.ref_bid, quoteSnapshot.ref_ask)
            : null;
          const marketSource = snapshotMarketMid != null
            ? 'quote snapshot maker top'
            : resolveDisplayedLegMid(resolveDisplayedLeg(row.original, 'A')) != null
              ? 'visible maker market'
              : '—';
          const explicitFv = coerceFiniteNumber((row.original as any).fv_row?.fv);
          const fvSource = snapshotFvMid != null
            ? 'quote snapshot ref'
            : displayedFvMid != null
              ? 'visible ref market'
              : explicitFv != null
                ? 'fv_row.fv'
                : '—';
          const resolvedMarketMid = snapshotMarketMid ?? marketMid;
          const resolvedFvMid = snapshotFvMid ?? fvMid;
          const requiredEdge = coerceFiniteNumber(row.original.required_edge_bps);
          const spreadText = spreadBps != null && Number.isFinite(spreadBps) ? `${formatBps(spreadBps)} bps` : '—';
          const tooltip = [
            'Strategy market vs FV',
            `Strategy mid: ${resolvedMarketMid != null ? fmtPriceTooltip(resolvedMarketMid) : '—'} (${marketSource})`,
            `FV mid: ${resolvedFvMid != null ? fmtPriceTooltip(resolvedFvMid) : '—'} (${fvSource})`,
            `Spread: ${spreadText}`,
            '',
            `Required edge: ${requiredEdge != null ? `${requiredEdge.toFixed(1)} bps` : '—'}`,
            `Edge2 surplus: ${row.original._edge2 != null ? `${row.original._edge2.toFixed(1)} bps` : '—'}`,
          ].join('\n');
          return (
            <SimpleTooltip content={<pre className="whitespace-pre-wrap">{tooltip}</pre>} delay={150}>
              <span
                className="font-mono text-xs cursor-help"
                style={{ color: getSpreadNetColor(spreadBps) }}
              >
                {spreadText}
              </span>
            </SimpleTooltip>
          );
        },
      },
      {
        accessorKey: '_maxAge',
        id: 'age_ms',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Age"
            tooltip={[
              'Oldest visible leg age.',
              'Shows the stalest leg in the row.',
              'Red >10s, yellow >3s.',
            ].join('\n')}
          />
        ),
        enableSorting: true,
        cell: ({ row }) => (
          <LiveAgeCell
            row={row.original}
            nowProvider={getServerNowMs}
            visibilityRoot={visibilityRoot}
          />
        ),
      },
      {
        id: 'last_update',
        accessorFn: (row) => row._lastUpdateMs ?? -1,
        header: () => (
          <ColumnHeaderWithTooltip
            label="Last Updated"
            tooltip={[
              'Newest visible leg timestamp.',
              'Suffix shows how long ago that newest update arrived.',
            ].join('\n')}
          />
        ),
        enableSorting: true,
        cell: ({ row }) => (
          <LiveLastUpdatedCell
            row={row.original}
            nowProvider={getServerNowMs}
            visibilityRoot={visibilityRoot}
          />
        ),
      },
      {
        id: 'last_trade',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Last Trade"
            tooltip={[
              'Most recent trade for this strategy.',
              'Shows notional and realized bps.',
            ].join('\n')}
          />
        ),
        enableSorting: false,
        cell: ({ row }) => {
          const trade = row.original.last_trade;
          if (!trade) {
            return <span className="text-neutral-500">-</span>;
          }
          const notional = coerceFiniteNumber(trade.notional);
          const realizedBps = coerceFiniteNumber(trade.realized_bps) ?? 0;
          return (
            <div className="flex flex-col gap-1 text-xs">
              <span className="text-neutral-400">
                {notional !== undefined ? `$${notional.toFixed(2)}` : 'N/A'}
              </span>
              <span style={{ color: getHistoricEdgeColor(realizedBps || 0) }}>
                {realizedBps.toFixed(1)} bps
              </span>
            </div>
          );
        },
      },
    ];
  }, [getServerNowMs, showQuoted, visibilityRoot]);

  const handleFilterChange = (newFilters: FilterValues) => {
    setFilters(newFilters);
  };

  const summaryActions = useMemo(() => {
    if (!balanceSummary) return null;
    return (
      <div className="flex items-center gap-3 text-[11px]" style={{ color: colors.text.muted }}>
        {BALANCE_STATUS_ORDER.map((status) => (
          <span key={status} className="flex items-center gap-1">
            <span className={cn('h-2 w-2 rounded-full', BALANCE_STATUS_META[status].dotClass)} />
            <span>{BALANCE_STATUS_META[status].label}</span>
            <span className="font-mono text-xs">{balanceSummary.counts?.[status] ?? 0}</span>
          </span>
        ))}
      </div>
    );
  }, [balanceSummary]);

  const renderMobileRow = useCallback((row: EnrichedRow) => (
    <SignalMobileCard
      row={row}
      showQuoted={showQuoted}
      nowProvider={getServerNowMs}
      pathProfile={pathProfile}
      visibilityRoot={visibilityRoot}
    />
  ), [getServerNowMs, pathProfile, showQuoted, visibilityRoot]);

  const signalSurfaceStatus = useMemo(
    () => resolveSignalSurfaceStatus(surfaceState, wsConnected, recoveryPending),
    [recoveryPending, surfaceState, wsConnected],
  );

  const content = (
    <>
      {showHeader && (
        <PanelHeader
          title="Signal"
          onRefresh={handleRefresh}
          refreshing={refreshing}
          lastUpdate={lastUpdate}
          staleThresholdMs={STALE_THRESHOLDS.FAST}
          onRemove={onRemove}
          actions={summaryActions}
          titleActions={
            <SimpleTooltip
              delay={250}
              content={signalSurfaceStatus.tooltip}
            >
              <span className="flex items-center gap-2 text-[11px]" style={{ color: colors.text.muted }}>
                <span className={cn('h-2 w-2 rounded-full', signalSurfaceStatus.dotClass)} />
                <span>{signalSurfaceStatus.label}</span>
              </span>
            </SimpleTooltip>
          }
        />
      )}
      <TableFilter
        columns={signalFilters}
        onFilterChange={handleFilterChange}
        dense={true}
        customControls={
          <div
            className="flex items-center flex-wrap"
            style={{
              gap: spacing.gap.sm,
              fontSize: typography.fontSize.xs,
              color: colors.text.muted,
            }}
          >
            {(!isMakerSuiteProfile || shouldUseEquitiesArbTable) && (
              <label className="flex items-center" style={{ gap: spacing.gap.xs }}>
                <span>Family</span>
                <select
                  value={familyScope}
                  onChange={(e) => setFamilyScope(e.target.value as SignalFamilyScope)}
                  aria-label="Signal family"
                  className="rounded border px-2 py-1 bg-bg-surface text-text-primary"
                  style={{ borderColor: colors.border.DEFAULT }}
                >
                  <option value="all">All ({familyCounts.all})</option>
                  {shouldUseEquitiesArbTable ? (
                    <>
                      <option value="equities_maker">Maker ({familyCounts.equities_maker})</option>
                      <option value="equities_taker">Taker ({familyCounts.equities_taker})</option>
                    </>
                  ) : (
                    <>
                      <option value="maker_v4">Maker V4 (legacy) ({familyCounts.maker_v4})</option>
                      <option value="maker_v3">Maker V3 ({familyCounts.maker_v3})</option>
                      <option value="maker_v2">Maker V2 ({familyCounts.maker_v2})</option>
                      <option value="taker">Taker ({familyCounts.taker})</option>
                    </>
                  )}
                </select>
              </label>
            )}
            {!shouldUseMakerV4Table && !shouldUseEquitiesArbTable && (
              <label className="flex items-center select-none cursor-pointer" style={{ gap: spacing.gap.xs }}>
                <input
                  type="checkbox"
                  className="cursor-pointer"
                  style={{ accentColor: colors.semantic.success.DEFAULT }}
                  checked={showQuoted}
                  onChange={(e) => setShowQuoted(e.target.checked)}
                />
                Show quoted prices (edge bias)
              </label>
            )}
          </div>
        }
      />
      <PanelBody ref={handleVisibilityRootRef}>
        {shouldUseEquitiesArbTable ? (
          <EquitiesArbSignalTable
            rows={filteredRows}
            loading={loading}
            nowProvider={getServerNowMs}
          />
        ) : shouldUseMakerV4Table ? (
          <MakerV4SignalTable
            rows={filteredRows}
            loading={loading}
            nowProvider={getServerNowMs}
            clockAnchorMs={serverTsMs}
            liveDataVersion={liveDataVersion}
            changedRowIds={makerV4ChangedRowIdsRef.current}
          />
        ) : (
          <DataTable
            data={filteredRows}
            columns={columns}
            getRowId={(row) => (row as any).id}
            sortable
            liveDataVersion={liveDataVersion}
            initialSorting={initialSorting}
            sortingState={sortingState}
            onSortingStateChange={setSortingState}
            dense={false}
            loading={loading}
            emptyMessage={loading ? 'Loading strategies...' : (wsConnected ? 'Waiting for pricing…' : 'No strategies found')}
            className={tableClassName}
            widthMode="content"
            columnWidthMode="explicit"
            virtualizer={shouldVirtualizeStandardTable ? signalRowVirtualizer : undefined}
            mobileMode="cards"
            renderMobileRow={renderMobileRow}
          />
        )}
      </PanelBody>
    </>
  );
  return (
    <div className="h-full flex flex-col overflow-hidden" style={{ backgroundColor: colors.bg.base }}>
      {content}
    </div>
  );
}

interface SignalMobileCardProps {
  row: EnrichedRow;
  showQuoted: boolean;
  nowProvider: () => number;
  pathProfile: PathProfile;
  visibilityRoot?: Element | null;
}

const SignalMobileCard: FC<SignalMobileCardProps> = ({ row, showQuoted, nowProvider, pathProfile, visibilityRoot }) => {
  const [expanded, setExpanded] = useState(false);
  const { nowMs, targetRef } = useVisibleNowMs<HTMLDivElement>({
    intervalMs: 1000,
    nowProvider,
    root: visibilityRoot,
  });
  const ageInfo = useMemo(() => computeStrategyAge(row, nowMs), [row, nowMs]);
  const tradingDescriptor = describeTradingStatus(row.status);
  const balanceInfo = getBalanceStatus(row);
  const ageMs = ageInfo.displayAgeMs;
  const ageDisplay = fmtAgeSec(ageMs);
  const lastUpdateMs = ageInfo.mostRecentTsMs ?? row._lastUpdateMs;
  const lastUpdateLabel = lastUpdateMs
    ? formatLocal(lastUpdateMs)
    : row._lastUpdate ?? 'N/A';
  const lastTrade = row.last_trade;
  const lastTradeNotional = coerceFiniteNumber(lastTrade?.notional);
  const lastTradeBps = coerceFiniteNumber(lastTrade?.realized_bps) ?? 0;
  const spreadNet = row._spreadNet ?? row._netEdge ?? null;
  const spreadText = spreadNet != null && Number.isFinite(spreadNet) ? `${spreadNet.toFixed(1)} bps` : '—';
  const edge2Text = row._edge2 != null ? `${row._edge2.toFixed(1)} bps` : '—';
  const skewAdj = findInventorySkewAdjustment(row.pricing_adjustments);
  const skewSummary = buildInventorySkewSummary(skewAdj) ?? '—';
  const skewTooltip = buildInventorySkewTooltip(skewAdj, row.params);

  return (
    <div ref={targetRef} className="rounded-xl border border-zinc-800 bg-zinc-900/50 p-4 flex flex-col gap-4">
      <div className="flex items-start justify-between gap-2">
        <div className="flex flex-col">
          <span className="font-mono text-sm text-zinc-200">{row.id}</span>
          <span className="text-xs text-zinc-500">{row.exchange}</span>
        </div>
        <div className="flex flex-col items-end gap-1 text-right">
          <StatusPill
            variant={tradingDescriptor.variant}
            label={tradingDescriptor.label}
            subLabel={tradingDescriptor.subLabel}
            tooltip={`Trading is ${tradingDescriptor.label.toLowerCase()} (${tradingDescriptor.subLabel})`}
            layout="inline"
          />
          <SimpleTooltip content={balanceInfo.tooltip} delay={150}>
            <Badge
              variant={balanceInfo.meta.variant}
              size="xs"
              className="justify-center font-semibold uppercase tracking-wide w-[90px]"
            >
              {balanceInfo.meta.label}
            </Badge>
          </SimpleTooltip>
        </div>
      </div>
      <div className="grid grid-cols-3 gap-3">
        <div className="flex flex-col">
          <span className="text-[10px] uppercase text-neutral-500">Spread (net)</span>
          <span
            className="font-mono text-lg"
            style={{ color: getSpreadNetColor(spreadNet) }}
          >
            {spreadText}
          </span>
        </div>
        <div className="flex flex-col">
          <span className="text-[10px] uppercase text-neutral-500">Edge2</span>
          <span
            className="font-mono text-lg"
            style={{ color: row._edge2 != null ? getEdge2Color(row._netEdge, row._edge2) : colors.text.secondary }}
          >
            {edge2Text}
          </span>
        </div>
        <div className="flex flex-col">
          <span className="text-[10px] uppercase text-neutral-500">Age</span>
          <span className="font-mono text-lg" style={{ color: getAgeColor(ageMs) }}>
            {ageDisplay}
          </span>
        </div>
      </div>
      <div className="flex items-center justify-between text-xs text-neutral-400">
        <span className="text-[10px] uppercase text-neutral-500">FvAdj</span>
        {skewTooltip ? (
          <SimpleTooltip
            content={<pre className="whitespace-pre-wrap">{skewTooltip}</pre>}
            delay={150}
          >
            <span className="font-mono text-neutral-200 cursor-help">{skewSummary}</span>
          </SimpleTooltip>
        ) : (
          <span className="font-mono text-neutral-500">{skewSummary}</span>
        )}
      </div>
      <div className="flex flex-col gap-1 text-xs text-neutral-400">
        <span>Last update: {lastUpdateLabel}</span>
        {lastTrade ? (
          <span className="flex items-center gap-2">
            <span>Last trade:</span>
            <span className="font-mono text-neutral-200">
              {lastTradeNotional !== undefined ? `$${lastTradeNotional.toFixed(2)}` : 'N/A'}
            </span>
            <span
              className="font-mono"
              style={{ color: getHistoricEdgeColor(lastTradeBps || 0) }}
            >
              {lastTradeBps.toFixed(1)} bps
            </span>
          </span>
        ) : (
          <span>No trades yet</span>
        )}
      </div>
      <button
        type="button"
        onClick={() => setExpanded((prev) => !prev)}
        className="flex items-center justify-between rounded-lg border border-neutral-800 px-3 py-2 text-xs text-neutral-200"
      >
        <span>{expanded ? 'Hide details' : 'Show details'}</span>
        <ChevronDown className={cn('h-4 w-4 transition-transform', expanded && 'rotate-180')} />
      </button>
      {expanded && (
        <div className="rounded-lg border border-zinc-800 bg-zinc-950 p-3 flex flex-col gap-3">
          <div>
            <span className="text-[10px] uppercase text-zinc-500">Strategy market</span>
            <MakerAwareLegCell row={row} legKey="A" showQuoted={showQuoted} nowMs={nowMs} pathProfile={pathProfile} />
          </div>
          <div>
            <span className="text-[10px] uppercase text-zinc-500">FV market</span>
            <MakerAwareLegCell row={row} legKey="B" showQuoted={showQuoted} nowMs={nowMs} pathProfile={pathProfile} />
          </div>
        </div>
      )}
    </div>
  );
};
