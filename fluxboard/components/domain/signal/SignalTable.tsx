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
 * - Visibility-aware cell-local age ticking (1s interval)
 * - Exponential backoff polling (1s → 2s → 4s → 8s cap)
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
import type {
  BalanceSummary,
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
import { resolvePathProfile, type PathProfile } from '@/config/uiProfiles';
import { EMPTY_SNAPSHOT_HOLD_MS, evaluateEmptySnapshotPolicy } from './emptySnapshotPolicy';
import {
  deriveStrategyStatus,
  describeTradingStatus,
  parseTradingEnabled,
  statusToFilterValue,
  TRADING_FILTER_VALUES,
  type TradingFilterValue,
  type TradingFlagInput,
} from '@/utils/strategyStatus';

// =============================================================================
// TYPES
// =============================================================================

type EnrichedRow = SignalStrategy & {
  _strategyFamily: SignalStrategyFamily;
  status: StrategyStatus;
  _netEdge: number;
  _edge2: number | null;
  _spreadNet: number | null;
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
  // Flattened classification metadata for filtering/grouping
  class?: string;
  venue_prefix?: string;
  chain?: string;
};

type SignalStrategyFamily = 'maker_v3' | 'maker_v2' | 'taker';
type SignalFamilyScope = 'all' | SignalStrategyFamily;

function resolveQuoteSnapshot(row: Pick<SignalStrategy, 'maker_v2' | 'maker_v3'>): MakerV2QuoteSnapshot | undefined {
  return row.maker_v2?.quote_snapshot ?? row.maker_v3?.quote_snapshot;
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

const SIGNAL_FILTERS: ColumnFilter[] = [
  { key: 'id', label: 'Strategy', type: 'text', placeholder: 'Strategy ID...' },
  { key: 'trading_enabled', label: 'Trading', type: 'select', options: TRADING_FILTER_VALUES },
  { key: 'exchange', label: 'Exchange', type: 'text', placeholder: 'bybit, rooster...' },
  { key: 'coin', label: 'Coin', type: 'text', placeholder: 'BTC, ETH...' },
  { key: 'class', label: 'Class', type: 'select', options: ['dex_cex_arb', 'equity_perp_arb'] },
  {
    key: 'venue_prefix',
    label: 'Venue',
    type: 'select',
    options: ['rooster_bybit', 'sailor_bybit', 'pcsbnb_bybit', 'tron_sunswap_v2_bybit', 'tron_sunswap_v3_bybit', 'hl_futu', 'hl_ibkr'],
  },
  { key: 'chain', label: 'Chain', type: 'select', options: ['plume', 'sei', 'bnb', 'tron', 'equities'] },
];

const TRADING_SORT_ORDER: Record<TradingFilterValue, number> = {
  Live: 2,
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
const MAKER_V2_CLASSES = new Set(['maker_v2', 'crypto_spot_perp_maker', 'equity_perp_maker']);

function deriveStrategyFamily(strategy: Pick<SignalStrategy, 'strategy_family' | 'meta'>): SignalStrategyFamily {
  const explicit = String(strategy.strategy_family || '').trim().toLowerCase();
  if (explicit === 'maker_v3' || explicit === 'maker_v2' || explicit === 'taker') {
    return explicit;
  }
  const cls = String(strategy.meta?.class || '').trim().toLowerCase();
  if (MAKER_V3_CLASSES.has(cls)) return 'maker_v3';
  if (MAKER_V2_CLASSES.has(cls)) return 'maker_v2';
  return 'taker';
}

function matchesSignalProfile(
  profile: PathProfile,
  strategy: Pick<SignalStrategy, 'meta'>
): boolean {
  if (profile === 'default') return true;
  const groups = parseStrategyGroups(strategy.meta?.strategy_groups);
  if (groups.size > 0) return groups.has(profile);
  if (profile === 'equities') {
    const chain = String(strategy.meta?.chain || '').trim().toLowerCase();
    if (chain === 'equities') return true;
  }
  return false;
}

function defaultFamilyScopeForProfile(profile: PathProfile): SignalFamilyScope {
  return profile === 'tokenmm' ? 'maker_v3' : 'all';
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
  const base = text.split('.')[0]?.split('-')[0] || text;
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
  if (fromState !== undefined && fromState !== null) return fromState;
  if (typeof row?.tradeable === 'boolean') return row.tradeable ? 1 : 0;
  return undefined;
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
    const bid = coerceFiniteNumber(leg.decision_bid ?? leg.fv_bid ?? leg.bid);
    const ask = coerceFiniteNumber(leg.decision_ask ?? leg.fv_ask ?? leg.ask);
    const tsMs = coerceFiniteNumber(leg.update_ts_ms ?? leg.ts_ms ?? leg.timestamp);
    const ageMs = coerceFiniteNumber(leg.md_age_ms ?? leg.age_ms);
    const symbol = String(leg.symbol ?? '').trim();
    if (leg.contract_id == null) leg.contract_id = contractId;
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

function shouldShowHedgeQuoteCounts(row: EnrichedRow): boolean {
  const cls = String(row.meta?.class ?? '').trim().toLowerCase();
  return cls.includes('maker') || !!resolveQuoteSnapshot(row);
}

function shouldSuppressQuoteCounts(row: EnrichedRow): boolean {
  const tradingEnabled = parseTradingEnabled(resolveTradingValue(row));
  const quoteSnapshot = resolveQuoteSnapshot(row) as any;
  const mode = String(quoteSnapshot?.mode ?? '').trim().toUpperCase();
  const reason = String(quoteSnapshot?.reason ?? '').trim().toLowerCase();
  return !tradingEnabled || mode === 'OFF' || reason === 'bot_off';
}

function zeroCounts(
  counts?: { bidOpen: number; bidDepth: number; bidBlocked: number; askOpen: number; askDepth: number; askBlocked: number }
) {
  if (!counts) return undefined;
  return zeroQuoteCounts();
}

function getQuoteCounts(row: EnrichedRow): QuoteCounts {
  const suppress = shouldSuppressQuoteCounts(row);
  const stacks = (row as any).quote_stacks;
  if (stacks && typeof stacks === 'object') {
    const makerBands = (stacks.maker?.bands ?? []) as any[];
    const hedge = stacks.hedge as any;
    const hasMaker = Array.isArray(makerBands) && makerBands.length > 0;
    const hasHedge = Boolean(hedge && typeof hedge === 'object');
    if (hasMaker || hasHedge) {
      const maker = hasMaker ? {
        bidOpen: makerBands.reduce((s, b) => s + quoteCount(b?.bid?.open), 0),
        bidDepth: makerBands.reduce((s, b) => s + quoteCount(b?.bid?.depth), 0),
        bidBlocked: makerBands.reduce((s, b) => s + quoteCount(b?.bid?.blocked), 0),
        askOpen: makerBands.reduce((s, b) => s + quoteCount(b?.ask?.open), 0),
        askDepth: makerBands.reduce((s, b) => s + quoteCount(b?.ask?.depth), 0),
        askBlocked: makerBands.reduce((s, b) => s + quoteCount(b?.ask?.blocked), 0),
      } : undefined;
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
        maker: suppress ? zeroCounts(maker) : maker,
        hedge: suppress ? zeroCounts(hedgeFallback) : hedgeFallback,
      };
    }
  }

  const qs = row.maker_quote_status as any;
  if (qs && typeof qs === 'object') {
    const maker = {
      bidOpen: quoteCount(qs.bid_open),
      bidDepth: quoteCount(qs.bid_depth),
      bidBlocked: quoteCount(qs.bid_blocked),
      askOpen: quoteCount(qs.ask_open),
      askDepth: quoteCount(qs.ask_depth),
      askBlocked: quoteCount(qs.ask_blocked),
    };
    const hedge = shouldShowHedgeQuoteCounts(row) ? zeroQuoteCounts() : undefined;
    return {
      source: 'maker_quote_status',
      maker: suppress ? zeroCounts(maker) : maker,
      hedge: suppress ? zeroCounts(hedge) : hedge,
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

  // Canonical signed skew from backend (preferred).
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
  // We care about directional shift (translation), not widening/narrowing.
  // Translation is the midpoint move across both legs, i.e. average delta.
  return (deltaAsk + deltaBid) / 2;
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

function buildInventorySkewTooltip(
  adj?: InventorySkewAdjustment,
  params?: Record<string, string | undefined>,
): string | null {
  if (!adj) return null;
  const skewBps = computeInventorySkewBps(adj);
  const desQtyGlobal = params?.des_qty_global ?? params?.des_qty;
  const maxQtyGlobal = params?.max_qty_global ?? params?.max_qty;
  const maxSkewGlobal = params?.max_skew_bps_global ?? params?.max_skew_bps;
  const desQtyLocal = params?.des_qty_local ?? params?.local_des_qty;
  const maxQtyLocal = params?.max_qty_local ?? params?.local_max_qty;
  const maxSkewLocal = params?.max_skew_bps_local ?? params?.local_max_skew_bps;
  const lines: string[] = [
    'Inventory skew (MakerV3 stacked global + local):',
    `Total inv_ratio (r): ${formatSignedRatio(adj.inv_ratio)} (clamped to [-1, +1])`,
    `Total inv_skew (s): ${formatSignedBps(adj.inv_skew)} bps`,
    `Total skew_bps: ${skewBps !== undefined ? formatSignedBps(skewBps) : '—'} (signed; <0 shifts quotes down, >0 shifts quotes up)`,
    '',
    'Global params:',
    `des_qty_global / max_qty_global / max_skew_bps_global: ${desQtyGlobal ?? '—'} / ${maxQtyGlobal ?? '—'} / ${maxSkewGlobal ?? '—'}`,
    `des_qty_local / max_qty_local / max_skew_bps_local: ${desQtyLocal ?? '—'} / ${maxQtyLocal ?? '—'} / ${maxSkewLocal ?? '—'}`,
  ];

  const globalRatio = coerceFiniteNumber(adj.inv_ratio_global);
  const globalSkew = coerceFiniteNumber(adj.inv_skew_global);
  const localRatio = coerceFiniteNumber(adj.inv_ratio_local);
  const localSkew = coerceFiniteNumber(adj.inv_skew_local);
  const localQty = coerceFiniteNumber(adj.local_qty);
  const localMatchedRows = coerceFiniteNumber(adj.local_qty_matched_rows);
  const localMissingSnapshot = coerceFiniteNumber(adj.local_qty_missing_snapshot);
  const localQtyKey = adj.local_qty_key;

  const hasBreakdown =
    globalRatio !== undefined ||
    globalSkew !== undefined ||
    localRatio !== undefined ||
    localSkew !== undefined ||
    localQty !== undefined ||
    localMatchedRows !== undefined ||
    localMissingSnapshot !== undefined ||
    !!localQtyKey;

  if (hasBreakdown) {
    lines.push('', 'Breakdown (if present from backend):');
    if (globalRatio !== undefined || globalSkew !== undefined) {
      lines.push(
        `Global inv_ratio / inv_skew: ${formatSignedRatio(globalRatio)} / ${formatSignedBps(globalSkew)} bps`,
      );
    }
    if (localRatio !== undefined || localSkew !== undefined) {
      lines.push(
        `Local inv_ratio / inv_skew: ${formatSignedRatio(localRatio)} / ${formatSignedBps(localSkew)} bps`,
      );
    }
    if (localQty !== undefined) {
      lines.push(`Local qty: ${localQty}`);
    }
    if (localQtyKey && typeof localQtyKey === 'object') {
      const venueRoot = String(localQtyKey.venue_root ?? '').trim() || '—';
      const instrumentType = String(localQtyKey.instrument_type ?? '').trim() || '—';
      const base = String(localQtyKey.base ?? '').trim() || '—';
      lines.push(`Local key: ${venueRoot}/${instrumentType}/${base}`);
    }
    if (localMatchedRows !== undefined || localMissingSnapshot !== undefined) {
      const matched = localMatchedRows !== undefined ? String(Math.trunc(localMatchedRows)) : '—';
      const missing = localMissingSnapshot !== undefined ? String(Math.trunc(localMissingSnapshot)) : '—';
      lines.push(`Local matched_rows / missing_snapshot: ${matched} / ${missing}`);
    }
  }

  lines.push(
    '',
    'Edges (bps):',
    `base bid/ask: ${formatBps(adj.base_bid_edge_bps)} / ${formatBps(adj.base_ask_edge_bps)}`,
    `eff bid/ask: ${formatBps(adj.eff_bid_edge_bps)} / ${formatBps(adj.eff_ask_edge_bps)}`,
    `delta bid/ask: ${formatSignedBps(adj.delta_bid_edge_bps)} / ${formatSignedBps(adj.delta_ask_edge_bps)}`,
    '',
    'maker quoting adjustment only (hedge side remains global-only)',
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
function buildBalanceTooltip(readiness?: SignalStrategy['balance_readiness'], fallback?: string): string {
  if (!readiness) {
    return fallback || 'No readiness data yet';
  }

  const lines: string[] = [];

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
      lines.push(`  ${req.location} ${req.token}: ${available}/${required} (${coverage})`);
    });
  } else if (readiness.missing && readiness.missing.length > 0) {
    lines.push('');
    lines.push('Top gaps:');
    readiness.missing.forEach(req => {
      lines.push(`  ${req.location} ${req.token} ${formatCoveragePercent(req.coverage)}`);
    });
  }

  // Compact methodology footer for operators who want to understand thresholds.
  lines.push('');
  lines.push('Methodology: Coverage = Avail/Reqd (10× qty buffer)');
  lines.push('OK: ≥100% | WARN: 80-100% | FAIL: <80% | UNKNOWN: No pricing');

  return lines.join('\n');
}

// =============================================================================
// LEG CELL COMPONENT
// =============================================================================

interface LegCellProps {
  leg: SignalLeg | null;
  showQuoted: boolean;
  tooltipBehavior?: 'cell' | 'icon';
  contextHint?: string;
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
const LegCell: FC<LegCellProps> = memo(({ leg, showQuoted, tooltipBehavior = 'cell', contextHint }) => {
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
        label: 'Market (raw)',
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
      label: 'Decision (fees-in)',
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
            {leg.exchange} {leg.coin}
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
            {leg.exchange} {leg.coin}
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

function truthRowTagsForLeg(
  row: EnrichedRow,
  legKey: LegKey,
  roleMap?: MakerRoleMap,
): Array<'Maker' | 'Ref' | 'Hedge'> {
  if (!roleMap) return [];
  const tags: Array<'Maker' | 'Ref' | 'Hedge'> = [];
  if (resolveRoleSlot(roleMap.maker_leg, row) === legKey) tags.push('Maker');
  if (resolveRoleSlot(roleMap.ref_leg, row) === legKey) tags.push('Ref');
  if (roleMap.hedge_leg && resolveRoleSlot(roleMap.hedge_leg, row) === legKey) tags.push('Hedge');
  return tags;
}

function coerceSnapshotPx(v: unknown): number | undefined {
  return coerceFiniteNumber(v);
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
  const roleMap = row.maker_role_map;
  const leg = getLegForSlot(row, legKey);
  const makerSlot = resolveRoleSlot(roleMap?.maker_leg, row);
  const refSlot = resolveRoleSlot(roleMap?.ref_leg, row);

  const isMakerLeg = makerSlot
    ? makerSlot === legKey
    : (leg?.exchange != null && quoteSnapshot.maker_exchange != null
        ? leg.exchange === quoteSnapshot.maker_exchange
        : false);

  const isRefLeg = refSlot
    ? refSlot === legKey
    : (leg?.exchange != null && quoteSnapshot.ref_exchange != null
        ? leg.exchange === quoteSnapshot.ref_exchange
        : false);

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
    tags: truthRowTagsForLeg(row, legKey, roleMap),
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
      <div className="text-text-muted font-semibold">Maker (Row 2 is quoting truth)</div>

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
            { label: 'Place (L1)', value: `${fmtPriceTooltip(placeBid)} .. ${fmtPriceTooltip(placeAsk)}` },
            ...(cancelBid != null || cancelAsk != null
              ? [{ label: 'Cancel', value: `${fmtPriceTooltip(cancelBid)} .. ${fmtPriceTooltip(cancelAsk)}` }]
              : []),
            { label: 'Edges (eff)', value: `bid ${formatBps(effBid)} bps   ask ${formatBps(effAsk)} bps` },
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

const MakerAwareLegCell: FC<{ row: EnrichedRow; legKey: LegKey; showQuoted: boolean; nowMs: number }> = memo(({
  row,
  legKey,
  showQuoted,
  nowMs,
}) => {
  const leg = getLegForSlot(row, legKey);
  const quoteSnapshot = resolveQuoteSnapshot(row);
  const hasMakerOverlay = !!quoteSnapshot;

  if (!hasMakerOverlay) {
    return <LegCell leg={leg} showQuoted={showQuoted} />;
  }

  return (
    <div className="flex flex-col">
      <LegCell
        leg={leg}
        showQuoted={showQuoted}
        tooltipBehavior="icon"
        contextHint="Maker: quoting truth = Row 2 (Our / Ref used)"
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
  const location = useLocation();
  const pathProfile = useMemo<PathProfile>(() => {
    const firstSegment = (location.pathname.split('/').filter(Boolean)[0] || '').toLowerCase();
    return resolvePathProfile(firstSegment);
  }, [location.pathname]);
  const [familyScope, setFamilyScope] = useState<SignalFamilyScope>(() => defaultFamilyScopeForProfile(pathProfile));

  useEffect(() => {
    setFamilyScope(defaultFamilyScopeForProfile(pathProfile));
  }, [pathProfile]);

  const isStrategyVisible = useCallback((strategy: SignalStrategy): boolean => {
    return matchesSignalProfile(pathProfile, strategy);
  }, [pathProfile]);
  const isFamilyVisible = useCallback((strategy: SignalStrategy): boolean => {
    if (familyScope === 'all') return true;
    return deriveStrategyFamily(strategy) === familyScope;
  }, [familyScope]);
  // Select from zustand store with shallow equality to reduce re-renders
  const rows = useSignalStore(selectSignalRows, shallow);
  const setRows = useSignalStore(s => s.setRows);
  const mergeStrategy = useSignalStore(s => s.mergeStrategy);
  const mergeStrategies = useSignalStore(s => s.mergeStrategies);
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
  const [showQuoted, setShowQuoted] = useState(false);
  const [lastUpdate, setLastUpdate] = useState<number>(Date.now());
  // Track lastUpdate via ref to avoid resubscribing effects when checking staleness
  const lastUpdateRef = useRef<number>(Date.now());
  // Sticky rows support: remember time of last non-empty dataset
  const lastNonEmptyRef = useRef<number | null>(null);
  // Track when we first saw an empty snapshot while rows existed.
  const emptySinceRef = useRef<number | null>(null);
  // Keep IDs of currently visible strategies to accept profile-compatible deltas
  // that omit full metadata.
  const visibleIdSetRef = useRef<Set<string>>(new Set());
  // Polling interval tracker for dynamic backoff
  const pollIntervalMsRef = useRef<number>(2000);
  const pollBackoffLevelRef = useRef<number>(0);
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
  const familyCounts = useMemo(() => {
    const base = (rows || []).filter(isStrategyVisible);
    return {
      all: base.length,
      maker_v3: base.filter((r) => deriveStrategyFamily(r) === 'maker_v3').length,
      maker_v2: base.filter((r) => deriveStrategyFamily(r) === 'maker_v2').length,
      taker: base.filter((r) => deriveStrategyFamily(r) === 'taker').length,
    };
  }, [rows, isStrategyVisible]);

  // Refs to track state without triggering dependency cycles
  const pollRef = useRef<NodeJS.Timeout | null>(null);
  const wsConnectedRef = useRef(false);
  const snapshotRefreshThrottleRef = useRef<number>(0);

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
    setVisibilityRoot((prev) => (prev === node ? prev : node));
  }, []);

  // Helper to update both state and ref
  const updateWsConnected = (connected: boolean) => {
    wsConnectedRef.current = connected;
    setWsConnected(connected);
  };

  const markRowsNonEmpty = useCallback((nowMs?: number) => {
    const now = nowMs ?? Date.now();
    lastNonEmptyRef.current = now;
    emptySinceRef.current = null;
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
    emptySinceRef.current = policy.nextEmptySinceMs;
    if (policy.clearRows && currentRows.length > 0) {
      setRows([]);
    }
  }, [setRows]);

  const applyVisibleSnapshotRows = useCallback((incomingRows: SignalStrategy[], requestStartedAtMs?: number) => {
    if (incomingRows.length > 0) {
      setRows(incomingRows);
      markRowsNonEmpty();
      return;
    }
    handleEmptyVisibleSnapshot(requestStartedAtMs);
  }, [handleEmptyVisibleSnapshot, markRowsNonEmpty, setRows]);

  // Manual refresh handler
  const handleRefresh = async () => {
    const requestStartedAtMs = Date.now();
    const requestSeq = ++restRequestSeqRef.current;
    setRefreshing(true);
    try {
      const data = await api.getSignalStrategies();
      if (requestSeq < restAppliedSeqRef.current) return;
      restAppliedSeqRef.current = requestSeq;
      const all = (data.strategies || []) as SignalStrategy[];
      const filtered = all.filter(isStrategyVisible);
      applyVisibleSnapshotRows(filtered, requestStartedAtMs);
      setServerTime(data.server_time || new Date().toISOString().slice(0, 19).replace('T', ' '));
      setServerClock((data as any).server_ts_ms as number | undefined);
      const now = Date.now();
      setLastUpdate(now);
      lastUpdateRef.current = now;
      setBalanceSummary(data.balance_summary ?? null);
    } catch (e) {
      if (import.meta.env?.DEV) {
        console.error('[signal] Refresh failed:', e);
      }
    } finally {
      setRefreshing(false);
    }
  };

  // Initial load, periodic refresh (only when WS disconnected), and WebSocket real-time updates
  useEffect(() => {
    const fetchData = () => {
      const requestStartedAtMs = Date.now();
      const requestSeq = ++restRequestSeqRef.current;
      api.getSignalStrategies()
        .then(data => {
          if (requestSeq < restAppliedSeqRef.current) return;
          restAppliedSeqRef.current = requestSeq;
          const all = (data.strategies || []) as SignalStrategy[];
          const strategies = all.filter(isStrategyVisible);
          applyVisibleSnapshotRows(strategies, requestStartedAtMs);
          // Use server_time if present, otherwise fall back to client time.
          const fetchedServerTime = data.server_time || new Date().toISOString().slice(0, 19).replace('T', ' ');
          setServerTime(fetchedServerTime);
          serverTimeRef.current = fetchedServerTime;
          const fetchedServerTsMs = (typeof (data as any).server_ts_ms === 'number') ? (data as any).server_ts_ms as number : null;
          setServerClock(fetchedServerTsMs);
          const now = Date.now();
          setLastUpdate(now);
          lastUpdateRef.current = now;
          setBalanceSummary(data.balance_summary ?? null);
          setLoading(false);
        })
        .catch(e => {
          if (import.meta.env?.DEV) {
            console.error('[signal] Failed to load:', e);
          }
          setLoading(false);
        });
    };

    const scheduleSnapshotRefresh = () => {
      const now = Date.now();
      if (now - snapshotRefreshThrottleRef.current < 1000) {
        return;
      }
      snapshotRefreshThrottleRef.current = now;
      fetchData();
    };

    const startPolling = (intervalMs?: number) => {
      const ms = intervalMs ?? pollIntervalMsRef.current ?? 2000;
      if (pollRef.current) {
        clearInterval(pollRef.current);
        pollRef.current = null;
      }
      pollIntervalMsRef.current = ms;
      pollRef.current = setInterval(fetchData, ms);
    };

    const startPollingWithBackoff = () => {
      // Exponential backoff: 1s → 2s → 4s → 8s (cap at 8s)
      const backoffIntervals = [1000, 2000, 4000, 8000];
      const level = Math.min(pollBackoffLevelRef.current, backoffIntervals.length - 1);
      const intervalMs = backoffIntervals[level];
      startPolling(intervalMs);
    };

    const resetPollingBackoff = () => {
      pollBackoffLevelRef.current = 0;
      stopPolling();
    };

    const stopPolling = () => {
      if (pollRef.current) {
        clearInterval(pollRef.current);
        pollRef.current = null;
      }
    };

    // Initial load - always fetch on mount
    fetchData();

    // Start polling if socket not connected
    if (!socket.connected) {
      startPolling(2000);
    }

    const handleMarketUpdate = (data: any) => {
      try {
        if (Array.isArray(data.strategies)) {
          // market_update is the authoritative strategy snapshot.
          // Merge first (preserve sticky fields from prior deltas), then prune stale IDs.
          if (data.strategies.length > 0) {
            const allowed = visibleIdSetRef.current;
            const filtered = (data.strategies as SignalStrategy[]).filter((s) => {
              if (matchesSignalProfile(pathProfile, s)) return true;
              const hasMeta = !!(s.meta && typeof s.meta === 'object');
              return !hasMeta && allowed.has(s.id);
            });
            if (filtered.length > 0) {
              mergeStrategies(filtered);
              const incomingIds = new Set(filtered.map((strategy) => strategy.id));
              const latestRows = useSignalStore.getState().rows || [];
              if (latestRows.some((row) => !incomingIds.has(row.id))) {
                setRows(latestRows.filter((row) => incomingIds.has(row.id)));
              }
              markRowsNonEmpty();
            } else {
              handleEmptyVisibleSnapshot();
            }
          } else {
            handleEmptyVisibleSnapshot();
          }
          // Do not keep the table in loading if live snapshot updates are flowing.
          setLoading(false);
          // Keep local server clock anchor in sync when server time changes.
          const newServerTime = data.server_time || new Date().toISOString().slice(0, 19).replace('T', ' ');
          const newServerTsMs = (typeof data.server_ts_ms === 'number') ? (data.server_ts_ms as number) : null;
          if (newServerTime !== serverTimeRef.current || newServerTsMs !== serverTsMsRef.current) {
            setServerTime(newServerTime);
            serverTimeRef.current = newServerTime;
            setServerClock(newServerTsMs);
          }
          const now = Date.now();
          setLastUpdate(now);
          lastUpdateRef.current = now;
          if (data.balance_summary) {
            setBalanceSummary(data.balance_summary);
          }
          // Reset backoff when we receive fresh data
          if (wsConnectedRef.current) {
            resetPollingBackoff();
          }
          return;
        }

        // Newer socket contract sends changed IDs only:
        // { strategies: { changed: [strategy_id...] }, ... }.
        // Pull a fresh snapshot when this arrives so UI cannot stay stale/blank.
        const changed = Array.isArray(data?.strategies?.changed)
          ? data.strategies.changed.filter((item: unknown) => typeof item === 'string')
          : [];
        if (changed.length > 0) {
          scheduleSnapshotRefresh();
          const newServerTime = data.server_time || new Date().toISOString().slice(0, 19).replace('T', ' ');
          const newServerTsMs = (typeof data.server_ts_ms === 'number') ? (data.server_ts_ms as number) : null;
          if (newServerTime !== serverTimeRef.current || newServerTsMs !== serverTsMsRef.current) {
            setServerTime(newServerTime);
            serverTimeRef.current = newServerTime;
            setServerClock(newServerTsMs);
          }
          if (data.balance_summary) {
            setBalanceSummary(data.balance_summary);
          }
          const now = Date.now();
          setLastUpdate(now);
          lastUpdateRef.current = now;
        }
        // Apply param_update payloads immediately so Trading pill reflects saves from Params
        if (data.param_update && data.param_update.strategy_id && data.param_update.parameters) {
          const sid = data.param_update.strategy_id as string;
          if (visibleIdSetRef.current.size > 0 && !visibleIdSetRef.current.has(sid)) return;
          const paramsUpdate = data.param_update.parameters as Record<string, unknown>;
          // Merge a minimal strategy shape containing updated params
          mergeStrategy({ id: sid, params: paramsUpdate } as any as SignalStrategy);
          const now = Date.now();
          if ((useSignalStore.getState().rows || []).length > 0) {
            markRowsNonEmpty(now);
          }
          setLastUpdate(now);
          lastUpdateRef.current = now;
        }
      } catch (err) {
        if (import.meta.env?.DEV) {
          console.error('[signal] Market update handler failed:', err);
        }
      }
    };

    /**
     * Handle signal_delta WebSocket events (single strategy updates).
     *
     * Merge semantics:
     * - Only patches legs that exist in the delta (undefined = no change)
     * - null explicitly deletes a leg
     * - Deep merges leg properties to preserve existing data
     *
     * This prevents leg drops when partial updates arrive.
     */
    const handleSignalDelta = (delta: any) => {
      try {
        const payload = (delta && typeof delta === 'object' && delta.patch && typeof delta.patch === 'object')
          ? {
              id: (delta as any).strategy_id ?? (delta as any).id,
              ...(delta as any).patch,
              ts_ms: (delta as any).server_ts_ms ?? (delta as any).ts_ms,
            }
          : delta;
        const id = payload?.id;
        if (!id) return;
        const hasMeta = !!(payload?.meta && typeof payload.meta === 'object');
        const knownInView = visibleIdSetRef.current.has(id);
        if (!hasMeta && !knownInView) {
          scheduleSnapshotRefresh();
          return;
        }
        const visibleByProfile = matchesSignalProfile(pathProfile, payload as SignalStrategy);
        if (!visibleByProfile && (hasMeta || !visibleIdSetRef.current.has(id))) {
          return;
        }
        const apply: Partial<SignalStrategy> = { id } as any;
        const passThroughKeys = new Set([
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
        // Merge strategy delta
        mergeStrategy(apply as SignalStrategy);
        // If backend provided authoritative server ts, anchor our local ticking to it
        if (typeof payload.ts_ms === 'number' && Number.isFinite(payload.ts_ms)) {
          setServerClock(payload.ts_ms as number);
        }
        const now = Date.now();
        if ((useSignalStore.getState().rows || []).length > 0) {
          markRowsNonEmpty(now);
        }
        setLastUpdate(now);
        lastUpdateRef.current = now;
      } catch (err) {
        if (import.meta.env?.DEV) console.error('[signal] Delta handler failed:', err);
      }
    };

    const handleConnect = () => {
      if (import.meta.env?.DEV) {
        console.log('[signal] WebSocket connected');
      }
      updateWsConnected(true);
      // Fetch fresh data on connection to ensure sync
      fetchData();
      // Stop polling when WS connects
      stopPolling();
    };

    const handleDisconnect = () => {
      if (import.meta.env?.DEV) {
        console.log('[signal] WebSocket disconnected');
      }
      updateWsConnected(false);
      // Resume polling when WS disconnects
      startPolling(1000);
    };

    const handleConnectError = (err: any) => {
      if (import.meta.env?.DEV) {
        console.error('[signal] WebSocket connect_error:', err?.message || err);
      }
      updateWsConnected(false);
      // Kick off polling immediately on connect errors so UI isn’t stuck waiting
      startPolling(500);
    };

    const handleReconnectAttempt = (attempt: number) => {
      if (import.meta.env?.DEV) {
        console.log('[signal] WebSocket reconnect_attempt:', attempt);
      }
      // While attempting to reconnect, ensure polling keeps data warm
      startPolling(500);
    };

    socket.on('connect', handleConnect);
    socket.on('disconnect', handleDisconnect);
    socket.on('market_update', handleMarketUpdate);
    socket.on('signal_delta', handleSignalDelta);
    socket.on('connect_error', handleConnectError);
    socket.on('reconnect_attempt', handleReconnectAttempt);

    // Check if already connected
    if (socket.connected) {
      updateWsConnected(true);
    }

    // Watchdog: if WS is connected but no updates arrive for a while, resume polling as a safety net
    const watchdog = setInterval(() => {
      try {
        const ageMs = Date.now() - (lastUpdateRef.current || 0);
        const isStale = ageMs > STALE_THRESHOLDS.REALTIME; // 2s threshold for real-time channels
        if (isStale) {
          // Increase polling rate when stale
          startPolling(wsConnectedRef.current ? 500 : 1000);
        } else if (wsConnectedRef.current) {
          // WS is live, we can stop polling
          stopPolling();
        } else {
          // WS not connected, keep modest polling
          startPolling(2000);
        }
      } catch {}
    }, 1000);

    return () => {
      socket.off('connect', handleConnect);
      socket.off('disconnect', handleDisconnect);
      socket.off('market_update', handleMarketUpdate);
      socket.off('signal_delta', handleSignalDelta);
      socket.off('connect_error', handleConnectError);
      socket.off('reconnect_attempt', handleReconnectAttempt);
      stopPolling();
      clearInterval(watchdog);
    };
  }, [
    pathProfile,
    setRows,
    mergeStrategy,
    mergeStrategies,
    isStrategyVisible,
    applyVisibleSnapshotRows,
    handleEmptyVisibleSnapshot,
    markRowsNonEmpty,
  ]);

  // Baseline row enrichment. Ages here are snapshot values used for sorting/filtering;
  // live age display is handled in cell-local components.
  const enrichedRows = useMemo<EnrichedRow[]>(() => {
    const serverNowMs = getServerNowMs();
    const visibleRows = (rows || []).filter((row) => isStrategyVisible(row) && isFamilyVisible(row));
    return (visibleRows.map(row => {
      // Defensive check: ensure legs exists (may be missing in WebSocket deltas)
      if (!row.legs) {
        // Skip rows without legs to prevent crashes
        return null;
      }

      const status = deriveStrategyStatus({ trading: resolveTradingValue(row as any) });
      const tradingFilter = statusToFilterValue(status);
      const orderedLegs = getOrderedLegEntries(row);
      const legA = getLegForSlot(row, 'A');
      const legB = getLegForSlot(row, 'B');
      const fallbackEdgeLeg = orderedLegs.find((entry) => {
        const value = entry.leg?.net_edge_bps;
        return typeof value === 'number' && Number.isFinite(value);
      })?.leg;
      // Use strategy-level decision_edge_bps (single source of truth) to prevent jumping
      // during incremental WebSocket updates. Fall back to leg values only if missing.
      const netEdge =
        row.decision_edge_bps
        ?? fallbackEdgeLeg?.net_edge_bps
        ?? legA?.net_edge_bps
        ?? legB?.net_edge_bps
        ?? 0;
      const edge2 = row.edge2_bps ?? null;
      const spreadNet = coerceFiniteNumber((row as any).spread_net_bps) ?? null;
      const riskDelta = coerceFiniteNumber(row.risk_delta) ?? null;

      // Extract filterable fields (exchange and coin from legs)
      const exchanges = orderedLegs
        .map((entry) => entry.leg?.exchange)
        .filter((value): value is string => typeof value === 'string' && value.length > 0)
        .join(' ');
      const coins = orderedLegs
        .map((entry) => entry.leg?.coin)
        .filter((value): value is string => typeof value === 'string' && value.length > 0)
        .join(' ');
      const ageData = computeStrategyAge(row, serverNowMs);
      const lastUpdate = orderedLegs
        .map((entry) => entry.leg?.update_time)
        .find((value): value is string => typeof value === 'string' && value.length > 0);

      const meta = row.meta || {};

      return {
        ...row,
        _strategyFamily: deriveStrategyFamily(row),
        status,
        _netEdge: netEdge,
        _edge2: typeof edge2 === 'number' ? edge2 : null,
        _spreadNet: typeof spreadNet === 'number' ? spreadNet : null,
        _riskDelta: typeof riskDelta === 'number' ? riskDelta : null,
        _maxAge: ageData.displayAgeMs,
        _minAge: ageData.recentAgeMs ?? 0,
        _lastUpdate: lastUpdate,
        _lastUpdateMs: ageData.mostRecentTsMs,
        _legAAge: ageData.perLeg.A?.ageMs,
        _legBAge: ageData.perLeg.B?.ageMs,
        _recentSide: ageData.mostRecentSide,
        // Filterable string fields
        trading_enabled: tradingFilter,
        exchange: exchanges,
        coin: coins,
        class: meta.class,
        venue_prefix: meta.venue_prefix,
        chain: meta.chain,
      };
    }).filter((row) => row !== null)) as EnrichedRow[];
  }, [ageSortTick, getServerNowMs, isFamilyVisible, isStrategyVisible, rows]);

  // Apply filters
  const filteredRows = useMemo(() => {
    return applyFilters(enrichedRows, filters, { columns: SIGNAL_FILTERS });
  }, [enrichedRows, filters]);

  // Column definitions (TanStack Table format)
  const columns = useMemo<ColumnDef<EnrichedRow>[]>(() => {
    const paramTooltip = (row: EnrichedRow) => [
      'Edge thresholds (minimum edge to trade):',
      `  cex_bid_edge: ${row.params?.cex_bid_edge ?? 'N/A'} bps`,
      `  cex_ask_edge: ${row.params?.cex_ask_edge ?? 'N/A'} bps`,
      `  pool_edge: ${row.params?.pool_edge ?? 'N/A'} bps`,
      '',
      'Trading params:',
      `  qty: ${row.params?.qty ?? 'N/A'}`,
      `  slippage: ${row.params?.slippage_bps ?? 'N/A'} bps`,
      '',
      'Decision prices (generic) = fees-in, FX-normalized',
      'Quoted prices (generic) = decision ± edge bias',
      'MakerV2 quoting truth (if present) = Our quotes / Ref used row (planned; not necessarily posted).',
    ].join('\n');

    const buildQuotesInfo = (row: EnrichedRow) => {
      const counts = getQuoteCounts(row);
      const maker = counts.maker;
      const hedge = counts.hedge;
      if (!maker && !hedge) return null;

      const makerSummary = maker ? `B ${maker.bidOpen}/${maker.bidDepth} · A ${maker.askOpen}/${maker.askDepth}` : '—';
      const hedgeSummary = hedge ? `H B ${hedge.bidOpen}/${hedge.bidDepth} · A ${hedge.askOpen}/${hedge.askDepth}` : null;

      const lines = [
        'Maker quoting status (best-effort).',
        `Source: ${counts.source}`,
        '',
      ];
      if (maker) {
        lines.push(`Maker Bid: ${maker.bidOpen}/${maker.bidDepth} (blocked ${maker.bidBlocked})`);
        lines.push(`Maker Ask: ${maker.askOpen}/${maker.askDepth} (blocked ${maker.askBlocked})`);
      }
      if (hedge) {
        lines.push(`Hedge Bid: ${hedge.bidOpen}/${hedge.bidDepth} (blocked ${hedge.bidBlocked})`);
        lines.push(`Hedge Ask: ${hedge.askOpen}/${hedge.askDepth} (blocked ${hedge.askBlocked})`);
      }
      lines.push('');
      lines.push('open = active orders');
      lines.push('depth = unique price levels');
      lines.push('blocked = cooldown');

      return {
        summaryLines: hedgeSummary ? [makerSummary, hedgeSummary] : [makerSummary],
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
              'Strategy id (from configs/strategies.ini).',
              'Hover the id cell to see key params + pricing semantics.',
            ].join('\n')}
          />
        ),
        enableSorting: true,
        cell: ({ row }) => (
          <SimpleTooltip content={<pre className="whitespace-pre-wrap">{paramTooltip(row.original)}</pre>} delay={150}>
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
              'Trading state derived from params.bot_on, state.bot_on, or tradeable.',
              'Live = bot_on=1 (strategy allowed to place orders).',
              'Paused = bot_on=0.',
              'Pending = runner transition/cooldown.',
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
            'Trading status:',
            `- State: ${descriptor.label} (${descriptor.subLabel})`,
            `- Resolved: ${rawStr} (params.bot_on | state.bot_on | tradeable)`,
            '',
            'Semantics:',
            '  Live: bot_on=1 (orders allowed)',
            '  Paused: bot_on=0 (no orders)',
            '  Pending: transition/cooldown (runner syncing)',
            '',
            'Change in Params → bot_on'
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
        accessorFn: (row) => row._riskDelta,
        id: 'risk_delta',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Risk Δ"
            tooltip={[
              'Current net risk delta for the strategy symbol.',
              'Read from maker risk state and updated in real-time.',
              'Positive means net long; negative means net short.',
            ].join('\n')}
          />
        ),
        enableSorting: true,
        cell: ({ row }) => {
          const riskDelta = row.original._riskDelta;
          const riskTsMs = coerceFiniteNumber(row.original.risk_delta_ts_ms);
          const tooltip = [
            'Risk delta (net position):',
            `value: ${riskDelta != null ? formatRiskDelta(riskDelta) : '—'}`,
            `ts: ${riskTsMs != null ? formatAbsoluteTime(riskTsMs) : '—'}`,
          ].join('\n');
          return (
            <SimpleTooltip content={<pre className="whitespace-pre-wrap">{tooltip}</pre>} delay={150}>
              <span className="text-right font-mono inline-flex w-full items-center justify-end cursor-help text-neutral-200">
                {riskDelta != null ? formatRiskDelta(riskDelta) : '—'}
              </span>
            </SimpleTooltip>
          );
        },
      },
      {
        id: 'quotes',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Quotes"
            tooltip={[
              'Maker quoting status (best-effort).',
              'Shows open quotes vs depth (unique price levels) per side.',
              'Supports maker_quote_status and quote_stacks payloads.',
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
            label="Adj/Skew"
            tooltip={[
              'Quote-only pricing adjustments applied by the strategy.',
              'Inventory skew shifts bid/ask edges based on current inventory.',
              'Displayed as a single signed skew (bps): negative shifts quotes down; positive shifts quotes up.',
              'Hover for effective vs base edges (bps).',
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
              'Venue prices for market A (as defined in configs/relations.ini).',
              'For MakerV2 rows: this is typically the market we quote (maker leg).',
              'Top row = Market BBO (raw).',
              'MakerV2 rows include Row 2: Our quotes (maker leg) or Ref used (ref leg).',
              'Hover the info icon in the cell for raw/decision/quoted breakdown.',
            ].join('\n')}
          />
        ),
        enableSorting: false,
        cell: ({ row }) => (
          <MakerAwareLegCell row={row.original} legKey="A" showQuoted={showQuoted} nowMs={getServerNowMs()} />
        ),
      },
      {
        id: 'legB',
        header: () => (
          <ColumnHeaderWithTooltip
            label="FV market"
            tooltip={[
              'Venue prices for market B (as defined in configs/relations.ini).',
              'For MakerV2 rows: this is typically the reference market used for fair value (ref leg).',
              'Top row = Market BBO (raw).',
              'MakerV2 rows include Row 2: Our quotes (maker leg) or Ref used (ref leg).',
              'Hover the info icon in the cell for raw/decision/quoted breakdown.',
            ].join('\n')}
          />
        ),
        enableSorting: false,
        cell: ({ row }) => (
          <MakerAwareLegCell row={row.original} legKey="B" showQuoted={showQuoted} nowMs={getServerNowMs()} />
        ),
      },
      {
        accessorKey: '_maxAge',
        id: 'age_ms',
        header: () => (
          <ColumnHeaderWithTooltip
            label="Age"
            tooltip={[
              'Age = worst freshness (oldest leg).',
              'Computed as max(now - tsA, now - tsB) using server time.',
              'Colors: >10s red, >3s yellow.',
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
              'Timestamp of the newest leg update (server time-derived).',
              'Suffix shows recency for that newest leg (min age).',
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
              'Most recent trade executed for this strategy (from trades.blotter).',
              'Shows notional and realized bps (historical).',
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
      visibilityRoot={visibilityRoot}
    />
  ), [getServerNowMs, showQuoted, visibilityRoot]);

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
              content={
                wsConnected
                  ? 'WebSocket live updates. Watchdog disables polling when fresh.'
                  : 'Polling fallback active (WS disconnected or stale).'
              }
            >
              <span className="flex items-center gap-2 text-[11px]" style={{ color: colors.text.muted }}>
                <span className={cn('h-2 w-2 rounded-full', wsConnected ? 'bg-success-light' : 'bg-warning-light')} />
                <span>{wsConnected ? 'Live (WS)' : 'Polling'}</span>
              </span>
            </SimpleTooltip>
          }
        />
      )}
      <TableFilter
        columns={SIGNAL_FILTERS}
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
            <label className="flex items-center" style={{ gap: spacing.gap.xs }}>
              <span>Family</span>
              <select
                value={familyScope}
                onChange={(e) => setFamilyScope(e.target.value as SignalFamilyScope)}
                className="rounded border px-2 py-1 bg-bg-surface text-text-primary"
                style={{ borderColor: colors.border.DEFAULT }}
              >
                <option value="all">All ({familyCounts.all})</option>
                <option value="maker_v3">Maker V3 ({familyCounts.maker_v3})</option>
                <option value="maker_v2">Maker V2 ({familyCounts.maker_v2})</option>
                <option value="taker">Taker ({familyCounts.taker})</option>
              </select>
            </label>
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
          </div>
        }
      />
      <PanelBody ref={handleVisibilityRootRef}>
        <DataTable
          data={filteredRows}
          columns={columns}
          getRowId={(row) => (row as any).id}
          sortable
          initialSorting={initialSorting}
          sortingState={sortingState}
          onSortingStateChange={setSortingState}
          dense={false}
          loading={loading}
          emptyMessage={loading ? 'Loading strategies...' : (wsConnected ? 'Waiting for pricing…' : 'No strategies found')}
          className={tableClassName}
          widthMode="content"
          columnWidthMode="explicit"
          mobileMode="cards"
          renderMobileRow={renderMobileRow}
        />
      </PanelBody>
    </>
  );

  if (showHeader) {
    // Full-page view: Signal route wraps in its own panel-style container
    return (
      <div className="h-full flex flex-col overflow-hidden" style={{ backgroundColor: colors.bg.base }}>
        {content}
      </div>
    );
  }

  // Dashboard mode: PanelWrapper already provides header + panel chrome.
  // We only render the filter + table content so the panel body can own vertical scrolling.
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
  visibilityRoot?: Element | null;
}

const SignalMobileCard: FC<SignalMobileCardProps> = ({ row, showQuoted, nowProvider, visibilityRoot }) => {
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
  const isMaker = !!resolveQuoteSnapshot(row);
  const spreadNet = row._spreadNet ?? row._netEdge ?? null;
  const spreadText = spreadNet != null && Number.isFinite(spreadNet) ? `${spreadNet.toFixed(1)} bps` : '—';
  const edge2Text = isMaker
    ? 'N/A (Maker)'
    : (row._edge2 != null ? `${row._edge2.toFixed(1)} bps` : '—');
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
            style={{ color: isMaker ? colors.text.secondary : getEdge2Color(row._netEdge, row._edge2) }}
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
        <span className="text-[10px] uppercase text-neutral-500">Adj/Skew</span>
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
            <MakerAwareLegCell row={row} legKey="A" showQuoted={showQuoted} nowMs={nowMs} />
          </div>
          <div>
            <span className="text-[10px] uppercase text-zinc-500">FV market</span>
            <MakerAwareLegCell row={row} legKey="B" showQuoted={showQuoted} nowMs={nowMs} />
          </div>
        </div>
      )}
    </div>
  );
};
