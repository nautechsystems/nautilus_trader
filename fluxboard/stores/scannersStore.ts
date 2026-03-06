import { create } from 'zustand';
import type { ScannerPricingSnapshot, ScannerPricingDelta, SignalLeg } from '@/types';
import type { FilterValues } from '@/components/shared/TableFilter';
import { api } from '@/api';
import { formatUsdCompact, formatEdgeValue } from '@/utils/scannersFormatting';
import {
  isScannersPerfV2Enabled,
  isScannersOptimizeTimersEnabled,
  isScannersOptimizeRafEnabled,
  isScannersMemoryCleanupEnabled,
  isScannersDeltaBufferLimitsEnabled,
} from '@/config/featureFlags';
import { fmtPrice } from '@/utils';

export type EnrichedRow = ScannerPricingSnapshot & {
  pairLabel: string;
  bestEdgeNum: number;
  dexMidNum: number;
  cexBidNum: number;
  cexAskNum: number;
  netEdgeSellNum: number;
  netEdgeBuyNum: number;
  tvlNum: number;
  vol24Num: number;
  last_update_ts: number;
  isMarginable: boolean;
  cexStale: boolean;
  legA: SignalLeg | null;
  legB: SignalLeg | null;
  tvlDisplay: string;
  vol24Display: string;
  edgeDisplay: string;
  pool_address: string;
  // Perf V2: Preformatted display strings (only when perfV2 enabled)
  bestEdgeDisplay?: string;
  cexBidDisplay?: string;
  cexAskDisplay?: string;
  dexMidDisplay?: string;
  netEdgeSellDisplay?: string;
  netEdgeBuyDisplay?: string;
};

type FilterSpec = {
  pairLabel: string;
  dex_name?: string;
  chain?: string;
  bybit_marginable?: 'marginable' | 'manual';
  min_edge_bps?: number | null;
  min_tvl_usd?: number | null;
  exclude_stable?: boolean;
  search?: string;
};

type ScannerStats = {
  applyDurationLastMs: number;
  applyDurationP50Ms: number;
  applyDurationP95Ms: number;
  applyDurationP99Ms: number;
  deltaBufferSize: number;
  deltaBufferHighWater: number;
  updatesPerSec: number;
  lastApplyDurationMs: number;
  lastAppliedAtTs: number;
  virtualRowsRendered: number;
  totalRows: number;
  // Perf V2: Additional metrics
  indexUpdateDurationP50Ms: number;
  indexUpdateDurationP95Ms: number;
  renderDurationP50Ms: number;
  renderDurationP95Ms: number;
  droppedDeltas: number;
  droppedDeltaRatePct: number;
};

export interface ScannersStoreState {
  initialized: boolean;
  scannerId: string;
  setScannerId: (id: string) => void;
  filteredIds: string[];
  hasMore: boolean;
  nextCursor: string | null;
  loading: boolean;
  refreshing: boolean;
  liveEnabled: boolean;
  pendingDeltas: number;
  stats: ScannerStats;
  nowTs: number;
  dataVersion: number; // Incremented when enriched rows are updated to trigger re-renders
  dexOptions: string[];
  chainOptions: string[];
  filterSpec: FilterSpec;
  loadInitial: () => Promise<void>;
  refresh: () => Promise<void>;
  loadMore: () => Promise<void>;
  setFilters: (filters: FilterValues) => void;
  enqueueDelta: (delta: ScannerPricingDelta) => void;
  toggleLive: (enabled?: boolean) => void;
  getRowById: (id: string) => EnrichedRow | undefined;
  getVisibleRows: (start: number, end: number) => EnrichedRow[];
  getTotalCount: () => number;
  initialize: () => void;
  setVirtualRenderStats: (rendered: number) => void;
  recordRenderDuration?: (durationMs: number) => void;
  recordScroll?: () => void;
  noteScroll: () => void;
  startRafApply: () => void;
  stopRafApply: () => void;
}

export type ScannersTableActionsSlice = Pick<
  ScannersStoreState,
  | 'initialize'
  | 'loadInitial'
  | 'refresh'
  | 'loadMore'
  | 'setFilters'
  | 'getRowById'
  | 'setVirtualRenderStats'
  | 'recordRenderDuration'
  | 'recordScroll'
  | 'setScannerId'
  | 'noteScroll'
  | 'stopRafApply'
>;

export const selectScannersTableActions = (
  state: ScannersStoreState,
): ScannersTableActionsSlice => ({
  initialize: state.initialize,
  loadInitial: state.loadInitial,
  refresh: state.refresh,
  loadMore: state.loadMore,
  setFilters: state.setFilters,
  getRowById: state.getRowById,
  setVirtualRenderStats: state.setVirtualRenderStats,
  recordRenderDuration: state.recordRenderDuration,
  recordScroll: state.recordScroll,
  setScannerId: state.setScannerId,
  noteScroll: state.noteScroll,
  stopRafApply: state.stopRafApply,
});

export type ScannersTableDataSlice = Pick<
  ScannersStoreState,
  | 'filteredIds'
  | 'hasMore'
  | 'loading'
  | 'refreshing'
  | 'dexOptions'
  | 'chainOptions'
  | 'nowTs'
  | 'scannerId'
  | 'filterSpec'
>;

export const selectScannersTableData = (
  state: ScannersStoreState,
): ScannersTableDataSlice => ({
  filteredIds: state.filteredIds,
  hasMore: state.hasMore,
  loading: state.loading,
  refreshing: state.refreshing,
  dexOptions: state.dexOptions,
  chainOptions: state.chainOptions,
  nowTs: state.nowTs,
  scannerId: state.scannerId,
  filterSpec: state.filterSpec,
});

export type ScannersTableTelemetrySlice = Pick<
  ScannerStats,
  'lastAppliedAtTs' | 'updatesPerSec' | 'applyDurationP95Ms' | 'deltaBufferSize'
>;

export const selectScannersTableTelemetry = (
  state: ScannersStoreState,
): ScannersTableTelemetrySlice => ({
  lastAppliedAtTs: state.stats.lastAppliedAtTs,
  updatesPerSec: state.stats.updatesPerSec,
  applyDurationP95Ms: state.stats.applyDurationP95Ms,
  deltaBufferSize: state.stats.deltaBufferSize,
});

const DEFAULT_SCANNER_ID = 'pcs_bnb_usdt';
const PAGE_SIZE_DEFAULT = 200;
const PCS_PAGE_SIZE = 5000;
const CEX_STALE_THRESHOLD_MS = 60_000;
const BUFFER_WARNING_THRESHOLD = 10_000;
const APPLY_SLOW_THRESHOLD_MS = 12;
const SLOW_FRAME_EXIT_COUNT = 5;
const MERGE_DELAY_STEP_MS = 16;
const MERGE_DELAY_MAX_MS = 32;
const NOW_TICK_MS = 1_000;
const UPDATE_RATE_INTERVAL_MS = 1_000;
// Perf V2 constants
const DELTA_QUEUE_MAX = 1_000; // Coarse mode threshold (reduced from 5k for better memory management)
const THROTTLE_HIDDEN_MS = 5_000; // Age tick when document hidden
const IDLE_TICK_MS = 2_000; // Age tick when idle
const IDLE_DETECTION_MS = 2_000; // Idle detection window
const REDIS_STATS_UPDATE_INTERVAL_MS = 1_500; // Publish stats to Redis interval
const AGE_TICKER_ENABLED = false; // Manual refresh mode: disable continuous age ticker
// Phase 1.1: Timer consolidation constants
const UNIFIED_TICK_INTERVAL_MS = 1_000; // Base interval for unified timer
const MAX_ROWS = 10_000; // Phase 2.2: Maximum rows to keep in memory
const STALE_THRESHOLD_MS = 5 * 60 * 1000; // Phase 2.2: 5 minutes for stale row eviction
const EVICTION_INTERVAL_MS = 30_000; // Phase 2.2: Run eviction every 30s
const MAX_DELTAS_PER_POOL = 10; // Phase 2.3: Keep only latest 10 deltas per pool when buffer is large

const STABLE_SYMBOLS = new Set([
  'USDT', 'USDC', 'BUSD', 'FDUSD', 'USDT.BNB', 'USDC.BNB', 'DAI',
]);

const toNumber = (value: unknown): number => {
  const n = Number(value);
  return Number.isFinite(n) ? n : 0;
};

export const normalizePoolString = (value: string): string =>
  value.replace(/[^a-z0-9]/gi, '').toLowerCase();

export const matchesPoolQuery = (label: string, query: string): boolean => {
  if (!query) return true;
  const rawNeedle = query.toLowerCase();
  const normalizedNeedle = normalizePoolString(query);
  const haystack = label.toLowerCase();
  if (haystack.includes(rawNeedle)) return true;
  const normalizedLabel = normalizePoolString(label);
  return normalizedLabel.includes(normalizedNeedle);
};

// Perf V2: Performance mark helpers
const perfMark = (name: string) => {
  if (typeof performance !== 'undefined' && performance.mark) {
    try {
      performance.mark(name);
    } catch {
      // Ignore mark errors
    }
  }
};

const perfMeasure = (name: string, startMark: string) => {
  if (typeof performance !== 'undefined' && performance.measure) {
    try {
      performance.measure(name, startMark);
      performance.clearMarks(startMark);
    } catch {
      // Ignore measure errors
    }
  }
};

// Perf V2: Check if snapshot fields changed (for cache detection)
// Note: old can be EnrichedRow (which extends ScannerPricingSnapshot), but we compare raw snapshot fields
const snapshotFieldsChanged = (
  old: ScannerPricingSnapshot | undefined,
  new_: ScannerPricingSnapshot
): boolean => {
  if (!old) return true;
  // Compare raw snapshot fields - ensure timestamp comparisons work even if types differ
  const compareTs = (a: any, b: any): boolean => {
    if (a == null && b == null) return false;
    if (a == null || b == null) return true;
    // Convert to numbers for comparison (handles string vs number)
    const aNum = typeof a === 'string' ? parseFloat(a) : Number(a);
    const bNum = typeof b === 'string' ? parseFloat(b) : Number(b);
    return !Number.isFinite(aNum) || !Number.isFinite(bNum) || aNum !== bNum;
  };
  return (
    old.best_edge_bps !== new_.best_edge_bps ||
    old.dex_mid !== new_.dex_mid ||
    old.cex_bid !== new_.cex_bid ||
    old.cex_ask !== new_.cex_ask ||
    old.tvl_usd !== new_.tvl_usd ||
    (old as any).volume_24h_usd !== (new_ as any).volume_24h_usd ||
    old.bybit_marginable !== new_.bybit_marginable ||
    compareTs(old.cex_last_update_ts, new_.cex_last_update_ts) ||
    compareTs(old.dex_last_update_ts, new_.dex_last_update_ts) ||
    compareTs(old.last_update_ts, new_.last_update_ts)
  );
};

function normalizeSymbol(sym?: unknown): string | null {
  if (typeof sym !== 'string') return null;
  const s = sym.trim().toUpperCase();
  if (!s) return null;
  if (STABLE_SYMBOLS.has(s) || s.endsWith('.USDT') || s.endsWith('.USDC') || s.endsWith('.BUSD')) {
    return s;
  }
  return s;
}

function isStablePair(row: EnrichedRow): boolean {
  const token0 = normalizeSymbol(row.token0);
  const token1 = normalizeSymbol(row.token1);
  if (!token0 || !token1) return false;
  return token0 === token1 || (STABLE_SYMBOLS.has(token0) && STABLE_SYMBOLS.has(token1));
}

function compareSortKeys(a: EnrichedRow, b: EnrichedRow): number {
  if (a.bestEdgeNum !== b.bestEdgeNum) return b.bestEdgeNum - a.bestEdgeNum;
  if (a.last_update_ts !== b.last_update_ts) return b.last_update_ts - a.last_update_ts;
  const aKey = a.pool_address || '';
  const bKey = b.pool_address || '';
  return aKey.localeCompare(bKey);
}

function enrichSnapshot(
  snapshot: ScannerPricingSnapshot,
  existing?: EnrichedRow
): EnrichedRow {
  const perfV2 = isScannersPerfV2Enabled();

  // Perf V2: Skip enrichment if snapshot unchanged
  if (perfV2 && existing && !snapshotFieldsChanged(existing, snapshot)) {
    return existing;
  }

  const isMarginable = Boolean(snapshot.bybit_marginable);

  // Normalize timestamps to milliseconds (API may send seconds or ms)
  const normalizeTs = (ts: number | null | undefined): number | null => {
    if (ts == null || ts === 0) return null;
    // Heuristic: if < 1e12, assume seconds and convert to ms
    if (ts < 1_000_000_000_000) {
      return Math.floor(ts * 1000);
    }
    return Math.floor(ts);
  };

  const cexLast = normalizeTs(snapshot.cex_last_update_ts);
  const dexLast = normalizeTs(snapshot.dex_last_update_ts);
  const snapshotTs = normalizeTs(snapshot.last_update_ts);

  const now = Date.now();
  const cexStale = Boolean(
    isMarginable && (!cexLast || now - cexLast > CEX_STALE_THRESHOLD_MS)
  );
  const dexMid = toNumber(snapshot.dex_mid);
  const cexExchange = (snapshot.cex_exchange || 'bybit').toString();
  const bybitSym = String(snapshot.bybit_symbol || '');
  const cexSymbol = String(snapshot.cex_symbol || bybitSym || '').trim();
  const [bybitBase] = (cexSymbol || bybitSym || '').split('/');
  const legA: SignalLeg = {
    exchange: (snapshot as any).dex_name || 'DEX',
    coin: `${snapshot.token0 || '?'}-${snapshot.token1 || '?'}`,
    decision_bid: dexMid || 0,
    decision_ask: dexMid || 0,
  };
  const legB: SignalLeg = {
    exchange: cexExchange,
    coin: cexSymbol || bybitBase || '—',
    decision_bid: toNumber(snapshot.cex_bid),
    decision_ask: toNumber(snapshot.cex_ask),
  };

  // Compute last_update_ts as the maximum of cex/dex timestamps for UI freshness
  const lastUpdateTs = (() => {
    const c = cexLast ?? 0;
    const d = dexLast ?? 0;
    const s = snapshotTs ?? 0;
    return Math.max(c, d, s);
  })();

  const netEdgeSellNum = toNumber(snapshot.net_edge_sell_dex_bps);
  const netEdgeBuyNum = toNumber(snapshot.net_edge_buy_dex_bps);
  const computedBestEdge = Math.max(
    netEdgeSellNum,
    netEdgeBuyNum,
    toNumber(snapshot.best_edge_bps)
  );
  const bestEdgeNum = computedBestEdge;
  const tvlNum = toNumber(snapshot.tvl_usd);
  const vol24Num = toNumber(snapshot.volume_24h_usd);
  const cexBidNum = toNumber(snapshot.cex_bid);
  const cexAskNum = toNumber(snapshot.cex_ask);
  const dexMidNum = toNumber(snapshot.dex_mid);

  const pairLabel = `${snapshot.token0 || '?'}${snapshot.token0 && snapshot.token1 ? '/' : ''}${snapshot.token1 || '?'}`;

  const enriched: EnrichedRow = {
    ...snapshot,
    pairLabel,
    bestEdgeNum,
    dexMidNum,
    cexBidNum,
    cexAskNum,
    netEdgeSellNum,
    netEdgeBuyNum,
    tvlNum,
    vol24Num,
    last_update_ts: lastUpdateTs,
    // CRITICAL: Store normalized timestamps so LastUpdateCell can correctly compute max
    // This ensures timestamps reset when new CEX or DEX data arrives
    cex_last_update_ts: cexLast ?? null,
    dex_last_update_ts: dexLast ?? null,
    isMarginable,
    cexStale,
    legA,
    legB,
    tvlDisplay: formatUsdCompact(tvlNum),
    vol24Display: formatUsdCompact(vol24Num),
    edgeDisplay: formatEdgeValue(bestEdgeNum),
    pool_address: (snapshot.pool_address || '').toLowerCase(),
  };

  // Perf V2: Precompute display strings
  if (perfV2) {
    enriched.bestEdgeDisplay = formatEdgeValue(bestEdgeNum);
    enriched.cexBidDisplay = fmtPrice(cexBidNum);
    enriched.cexAskDisplay = fmtPrice(cexAskNum);
    enriched.dexMidDisplay = fmtPrice(dexMidNum);
    enriched.netEdgeSellDisplay = formatEdgeValue(toNumber(snapshot.net_edge_sell_dex_bps));
    enriched.netEdgeBuyDisplay = formatEdgeValue(toNumber(snapshot.net_edge_buy_dex_bps));
  }

  return enriched;
}

const buildFilterSpec = (values: FilterValues): FilterSpec => {
  const sanitized: FilterSpec = {
    pairLabel: String(values.pairLabel || '').trim(),
    search: '',
  };
  if (values.dex_name) sanitized.dex_name = values.dex_name.trim();
  if (values.chain) sanitized.chain = values.chain.trim();
  if (values.bybit_marginable) {
    const val = String(values.bybit_marginable).toLowerCase();
    if (val === 'marginable') sanitized.bybit_marginable = 'marginable';
    if (val === 'manual') sanitized.bybit_marginable = 'manual';
  }
  if (values.min_edge_bps !== '' && values.min_edge_bps !== null && values.min_edge_bps !== undefined) {
    const num = Number(values.min_edge_bps);
    if (Number.isFinite(num)) sanitized.min_edge_bps = num;
  }
  if (values.min_tvl_usd !== '' && values.min_tvl_usd !== null && values.min_tvl_usd !== undefined) {
    const num = Number(values.min_tvl_usd);
    if (Number.isFinite(num)) sanitized.min_tvl_usd = Math.max(0, num);
  }
  if (values.exclude_stable) {
    const normalized = String(values.exclude_stable).toLowerCase();
    sanitized.exclude_stable = normalized === '1' || normalized === 'true';
  }
  return sanitized;
};

export const useScannersStore = create<ScannersStoreState>((set, get) => {
  let rowsById = new Map<string, ScannerPricingSnapshot>();
  let enrichedById = new Map<string, EnrichedRow>();
  let sortedIdsByEdge: string[] = [];
  let filteredSet = new Set<string>();
  let deltaBuffer = new Map<string, ScannerPricingSnapshot>();
  let scheduledHandle: number | null = null;
  let scheduledType: 'raf' | 'timeout' | null = null;
  let mergeDelay = 0;
  let slowFrameCount = 0;
  let updateRateInterval: number | null = null;
  let nowTicker: number | null = null;
  let initialized = false;
  let deltaBufferHighWater = 0;
  let applyDurations: number[] = [];
  let filterSpec: FilterSpec = {
    pairLabel: '',
    search: '',
    // Defaults: no client-side min TVL or min edge so we can hit zset fast-path
    min_tvl_usd: undefined,
    min_edge_bps: undefined,
  };
  let dexSet = new Set<string>();
  let chainSet = new Set<string>();
  let filteredIdsList: string[] = [];
  let filteredDirty = false;
  let updatesThisWindow = 0;
  // Perf V2: Additional tracking
  let indexUpdateDurations: number[] = [];
  let renderDurations: number[] = [];
  let droppedDeltas = 0;
  let totalDeltasEnqueued = 0;
  let lastDeltaTime = 0;
  let lastScrollTime = 0;
  let isUserScrolling = false;
  let scrollBackoffTimeout: number | null = null;
  let rafLoopHandle: number | null = null;
  let lastApplyTime = 0;
  const SCROLL_BACKOFF_MS = 200;
  const MIN_APPLY_INTERVAL_MS = 16; // ~60fps max
  const SCROLL_BACKOFF_THRESHOLD = 5000; // Force apply if buffer > 5k
  let redisStatsUpdateHandle: number | null = null;
  let visibilityChangeHandler: (() => void) | null = null;
  let evictionTimer: number | null = null; // Phase 2.2: Memory cleanup timer

  const statsUpdater = (partial: Partial<ScannerStats>) => {
    set((state) => ({ stats: { ...state.stats, ...partial } }));
  };

  const recordApplyDuration = (durationMs: number) => {
    applyDurations.push(durationMs);
    if (applyDurations.length > 250) {
      applyDurations.shift();
    }
    const sorted = [...applyDurations].sort((a, b) => a - b);
    const p50 = sorted[Math.floor(sorted.length * 0.5)] ?? durationMs;
    const p95 = sorted[Math.floor(sorted.length * 0.95)] ?? durationMs;
    const p99 = sorted[Math.floor(sorted.length * 0.99)] ?? durationMs;
    statsUpdater({
      applyDurationLastMs: durationMs,
      applyDurationP50Ms: p50,
      applyDurationP95Ms: p95,
      applyDurationP99Ms: p99,
      lastApplyDurationMs: durationMs,
      // Note: lastAppliedAtTs is set separately in drainDeltaBuffer/loadPage with actual data timestamps
    });
    if (durationMs > APPLY_SLOW_THRESHOLD_MS) {
      slowFrameCount += 1;
      if (slowFrameCount >= SLOW_FRAME_EXIT_COUNT) {
        mergeDelay = Math.min(MERGE_DELAY_MAX_MS, mergeDelay + MERGE_DELAY_STEP_MS);
      }
    } else {
      slowFrameCount = 0;
      mergeDelay = 0;
    }
  };

  // Perf V2: Record index update duration
  const recordIndexUpdateDuration = (durationMs: number) => {
    if (!isScannersPerfV2Enabled()) return;
    indexUpdateDurations.push(durationMs);
    if (indexUpdateDurations.length > 250) {
      indexUpdateDurations.shift();
    }
    const sorted = [...indexUpdateDurations].sort((a, b) => a - b);
    const p50 = sorted[Math.floor(sorted.length * 0.5)] ?? durationMs;
    const p95 = sorted[Math.floor(sorted.length * 0.95)] ?? durationMs;
    statsUpdater({
      indexUpdateDurationP50Ms: p50,
      indexUpdateDurationP95Ms: p95,
    });
  };

  // Perf V2: Record render duration (called from component)
  const recordRenderDuration = (durationMs: number) => {
    if (!isScannersPerfV2Enabled()) return;
    renderDurations.push(durationMs);
    if (renderDurations.length > 250) {
      renderDurations.shift();
    }
    const sorted = [...renderDurations].sort((a, b) => a - b);
    const p50 = sorted[Math.floor(sorted.length * 0.5)] ?? durationMs;
    const p95 = sorted[Math.floor(sorted.length * 0.95)] ?? durationMs;
    statsUpdater({
      renderDurationP50Ms: p50,
      renderDurationP95Ms: p95,
    });
  };

  const getPoolKey = (snapshot: ScannerPricingSnapshot): string | null => {
    if (!snapshot.pool_address) return null;
    return snapshot.pool_address.toLowerCase();
  };

  const matchesFilters = (row: EnrichedRow, spec: FilterSpec): boolean => {
    if (spec.pairLabel && !matchesPoolQuery(row.pairLabel, spec.pairLabel)) {
      return false;
    }
    if (spec.dex_name && row.dex_name) {
      if (spec.dex_name !== row.dex_name) return false;
    }
    if (spec.chain && row.chain) {
      if (spec.chain !== row.chain) return false;
    }
    if (spec.bybit_marginable && spec.bybit_marginable === 'marginable' && !row.isMarginable) return false;
    if (spec.bybit_marginable && spec.bybit_marginable === 'manual' && row.isMarginable) return false;
    if (spec.min_edge_bps !== undefined && spec.min_edge_bps !== null) {
      if (row.bestEdgeNum < spec.min_edge_bps) return false;
    }
    if (spec.min_tvl_usd !== undefined && spec.min_tvl_usd !== null) {
      if (row.tvlNum < spec.min_tvl_usd) return false;
    }
    if (spec.exclude_stable && isStablePair(row)) return false;
    return true;
  };

  const rebuildFilteredIds = (spec: FilterSpec) => {
    const next: string[] = [];
    filteredSet = new Set();
    for (const id of sortedIdsByEdge) {
      const row = enrichedById.get(id);
      if (!row) continue;
      if (matchesFilters(row, spec)) {
        filteredSet.add(id);
        next.push(id);
      }
    }
    filteredIdsList = next;
    filteredDirty = false;
    set({ filteredIds: next });
  };

  const updateSortedIndex = (id: string) => {
    const perfV2 = isScannersPerfV2Enabled();
    const startTime = perfV2 ? performance.now() : 0;
    if (perfV2) {
      perfMark(`scanners.index.update.start.${id}`);
    }

    const row = enrichedById.get(id);
    if (!row) return;
    const existingIndex = sortedIdsByEdge.indexOf(id);
    if (existingIndex >= 0) {
      sortedIdsByEdge.splice(existingIndex, 1);
    }
    let low = 0;
    let high = sortedIdsByEdge.length;
    while (low < high) {
      const mid = (low + high) >> 1;
      const midRow = enrichedById.get(sortedIdsByEdge[mid]);
      if (!midRow) {
        low = mid + 1;
        continue;
      }
      if (compareSortKeys(midRow, row) < 0) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }
    sortedIdsByEdge.splice(low, 0, id);

    if (perfV2) {
      const endTime = performance.now();
      perfMark(`scanners.index.update.end.${id}`);
      perfMeasure(`scanners.index.update.${id}`, `scanners.index.update.start.${id}`);
      const duration = endTime - startTime;
      if (duration > 0) {
        recordIndexUpdateDuration(duration);
      }
    }
  };

  const updateFilteredSetForId = (id: string, spec: FilterSpec) => {
    const row = enrichedById.get(id);
    if (!row) {
      if (filteredSet.has(id)) {
        filteredSet.delete(id);
        filteredIdsList = filteredIdsList.filter((entry) => entry !== id);
        filteredDirty = true;
      }
      return;
    }
    const matches = matchesFilters(row, spec);
    const currentlyIncluded = filteredSet.has(id);
    if (matches === currentlyIncluded) return;
    if (matches) {
      const insertIndex = filteredIdsList.findIndex((entry) => {
        const entryIndex = sortedIdsByEdge.indexOf(entry);
        const currentIndex = sortedIdsByEdge.indexOf(id);
        if (entryIndex === -1 || currentIndex === -1) return false;
        return entryIndex > currentIndex;
      });
      if (insertIndex === -1) {
        filteredIdsList.push(id);
      } else {
        filteredIdsList.splice(insertIndex, 0, id);
      }
      filteredSet.add(id);
    } else {
      filteredSet.delete(id);
      const removalIndex = filteredIdsList.indexOf(id);
      if (removalIndex >= 0) {
        filteredIdsList.splice(removalIndex, 1);
      }
    }
    filteredDirty = true;
  };

  const updateFilterOptions = (snapshot: ScannerPricingSnapshot) => {
    let changed = false;
    if (snapshot.dex_name) {
      const normalized = snapshot.dex_name;
      if (!dexSet.has(normalized)) {
        dexSet.add(normalized);
        changed = true;
      }
    }
    if (snapshot.chain) {
      const normalized = snapshot.chain;
      if (!chainSet.has(normalized)) {
        chainSet.add(normalized);
        changed = true;
      }
    }
    if (changed) {
      const sortedDex = Array.from(dexSet).sort((a, b) => a.localeCompare(b));
      const sortedChain = Array.from(chainSet).sort((a, b) => a.localeCompare(b));
      set({ dexOptions: sortedDex, chainOptions: sortedChain });
    }
  };

  const applySnapshot = (snapshot: ScannerPricingSnapshot) => {
    const key = getPoolKey(snapshot);
    if (!key) return;
    // Perf V2: Compare raw snapshots (not enriched rows) to detect changes
    const existingRaw = rowsById.get(key);
    const existingEnriched = enrichedById.get(key);

    // CRITICAL: Always update the raw snapshot to ensure fresh timestamps reset
    // When new market data arrives (CEX or DEX), the timestamp should reset, not just increment
    rowsById.set(key, snapshot);

    // Perf V2: Use cache-aware enrichment - compare raw snapshots, not enriched rows
    // Always re-enrich to ensure fresh timestamps from new market data reset the display
    const enriched = enrichSnapshot(snapshot, existingEnriched);
    enrichedById.set(key, enriched);

    updateSortedIndex(key);
    updateFilterOptions(snapshot);
    updateFilteredSetForId(key, filterSpec);
  };

  const drainDeltaBuffer = () => {
    const perfV2 = isScannersPerfV2Enabled();
    if (perfV2) {
      perfMark('scanners.delta.apply.start');
    }

    const bufferEntries = Array.from(deltaBuffer.values());
    deltaBuffer.clear();
    set({ pendingDeltas: 0 });
    if (!bufferEntries.length) {
      if (perfV2) {
        perfMark('scanners.delta.apply.end');
        perfMeasure('scanners.delta.apply', 'scanners.delta.apply.start');
      }
      return;
    }
    const start = performance.now();
    let maxDataTs = 0;
    for (const snapshot of bufferEntries) {
      applySnapshot(snapshot);
      // Track max timestamp from enriched row (normalized, matches what's displayed)
      const key = getPoolKey(snapshot);
      if (key) {
        const enriched = enrichedById.get(key);
        if (enriched && enriched.last_update_ts > 0) {
          if (enriched.last_update_ts > maxDataTs) {
            maxDataTs = enriched.last_update_ts;
          }
        }
      }
    }
    const duration = performance.now() - start;
    recordApplyDuration(duration);
    updatesThisWindow += bufferEntries.length;
    if (filteredDirty) {
      set({ filteredIds: [...filteredIdsList] });
      filteredDirty = false;
    }
    // Update lastAppliedAtTs with actual data timestamp (not client time)
    if (maxDataTs > 0) {
      statsUpdater({ lastAppliedAtTs: maxDataTs });
    }

    if (perfV2) {
      perfMark('scanners.delta.apply.end');
      perfMeasure('scanners.delta.apply', 'scanners.delta.apply.start');
    }

    // Update dropped delta rate
    const droppedRate = totalDeltasEnqueued > 0
      ? (droppedDeltas / totalDeltasEnqueued) * 100
      : 0;
    statsUpdater({
      deltaBufferSize: deltaBuffer.size,
      totalRows: sortedIdsByEdge.length,
      droppedDeltas,
      droppedDeltaRatePct: droppedRate,
    });
  };

  const shouldApplyNow = (): boolean => {
    const now = Date.now();
    // Force apply if buffer exceeds threshold
    if (deltaBuffer.size > SCROLL_BACKOFF_THRESHOLD) {
      return true;
    }
    // Defer during active scrolling unless buffer is large
    if (isUserScrolling && deltaBuffer.size < 100) {
      return false;
    }
    // Rate limit: don't apply more than once per frame (~16ms)
    if (now - lastApplyTime < MIN_APPLY_INTERVAL_MS) {
      return false;
    }
    return true;
  };

  const scheduleApply = () => {
    if (scheduledHandle !== null) return;
    const run = () => {
      scheduledHandle = null;
      scheduledType = null;
      const state = get();
      if (!state.liveEnabled) {
        if (import.meta.env?.DEV) {
          console.log('[ScannersStore] scheduleApply skipped - liveEnabled=false');
        }
        return;
      }
      if (shouldApplyNow()) {
        lastApplyTime = Date.now();
        drainDeltaBuffer();
      } else if (deltaBuffer.size > 0) {
        // Reschedule if we deferred
        scheduleApply();
      }
    };
    if (mergeDelay > 0) {
      if (typeof window !== 'undefined') {
        scheduledHandle = window.setTimeout(run, mergeDelay);
        scheduledType = 'timeout';
      } else {
        run();
      }
    } else if (typeof window !== 'undefined' && window.requestAnimationFrame) {
      scheduledHandle = window.requestAnimationFrame(run);
      scheduledType = 'raf';
    } else if (typeof window !== 'undefined') {
      scheduledHandle = window.setTimeout(run, MERGE_DELAY_STEP_MS);
      scheduledType = 'timeout';
    } else {
      run();
    }
  };

  // Phase 1.2: RAF Loop Optimization - only run when deltas exist
  const startRafApply = () => {
    if (rafLoopHandle !== null) return;
    if (typeof window === 'undefined' || !window.requestAnimationFrame) return;
    // Don't start if no deltas to process
    if (deltaBuffer.size === 0) return;

    const tick = () => {
      const state = get();
      // Stop RAF loop when buffer is empty and no pending work
      if (!state.liveEnabled || deltaBuffer.size === 0) {
        stopRafApply();
        return;
      }
      if (shouldApplyNow()) {
        lastApplyTime = Date.now();
        drainDeltaBuffer();
        // Continue loop if more deltas arrived during processing
        if (deltaBuffer.size > 0) {
          rafLoopHandle = window.requestAnimationFrame(tick);
        } else {
          stopRafApply();
        }
      } else {
        rafLoopHandle = window.requestAnimationFrame(tick);
      }
    };

    rafLoopHandle = window.requestAnimationFrame(tick);
  };

  const stopRafApply = () => {
    if (rafLoopHandle !== null && typeof window !== 'undefined') {
      window.cancelAnimationFrame(rafLoopHandle);
      rafLoopHandle = null;
    }
    // Also clear scroll back-off timeout
    if (scrollBackoffTimeout !== null && typeof window !== 'undefined') {
      window.clearTimeout(scrollBackoffTimeout);
      scrollBackoffTimeout = null;
    }
    isUserScrolling = false;
  };

  const noteScroll = () => {
    isUserScrolling = true;
    lastScrollTime = Date.now();

    // Clear existing timeout
    if (scrollBackoffTimeout !== null && typeof window !== 'undefined') {
      window.clearTimeout(scrollBackoffTimeout);
    }

    // Clear scrolling flag after idle period
    if (typeof window !== 'undefined') {
      scrollBackoffTimeout = window.setTimeout(() => {
        isUserScrolling = false;
        scrollBackoffTimeout = null;
        // Trigger apply if we have buffered deltas
        if (deltaBuffer.size > 0) {
          scheduleApply();
        }
      }, SCROLL_BACKOFF_MS);
    }
  };

  const enqueueDelta = (delta: ScannerPricingDelta) => {
    if (!delta || !delta.snapshot || !delta.snapshot.pool_address) {
      if (import.meta.env?.DEV) {
        console.warn('[ScannersStore] enqueueDelta: invalid delta', { delta });
      }
      return;
    }

    const perfV2 = isScannersPerfV2Enabled();
    if (perfV2) {
      perfMark('scanners.delta.enqueue');
    }

    const key = delta.snapshot.pool_address.toLowerCase();
    const pending = deltaBuffer.size;

    // Phase 2.3: Adaptive rate limiting - check BEFORE adding delta
    let dropRate = 0;
    if (pending > 5000) {
      dropRate = 0.5; // Drop 50% when buffer > 5k
    } else if (pending > 2000) {
      dropRate = 0.3; // Drop 30% when buffer > 2k
    } else if (pending > DELTA_QUEUE_MAX) {
      dropRate = 0.1; // Drop 10% when buffer > 1k
    }

    // Random drop based on rate (before adding to buffer)
    if (dropRate > 0 && Math.random() < dropRate) {
      droppedDeltas += 1;
      totalDeltasEnqueued += 1; // Count as attempted but dropped
      return; // Drop this delta early
    }

    // Delta passed rate limiting, proceed to add it
    totalDeltasEnqueued += 1;
    lastDeltaTime = Date.now();

    if (import.meta.env?.DEV && totalDeltasEnqueued % 10 === 0) {
      console.log('[ScannersStore] enqueueDelta', {
        total_enqueued: totalDeltasEnqueued,
        buffer_size: pending,
        live_enabled: get().liveEnabled,
      });
    }

    // Coalesce: merge with existing delta for same pool (keep latest fields)
    const existing = deltaBuffer.get(key);
    if (existing) {
      // Merge: combine fields from existing and new snapshot
      const merged: ScannerPricingSnapshot = {
        ...existing,
        ...delta.snapshot,
        // Preserve timestamps - use max for freshness
        last_update_ts: Math.max(
          existing.last_update_ts || 0,
          delta.snapshot.last_update_ts || 0
        ),
        cex_last_update_ts: delta.snapshot.cex_last_update_ts ?? existing.cex_last_update_ts,
        dex_last_update_ts: delta.snapshot.dex_last_update_ts ?? existing.dex_last_update_ts,
      };
      deltaBuffer.set(key, merged);
    } else {
      deltaBuffer.set(key, delta.snapshot);
    }

    // Perf V2: Coarse mode - drop intermediate updates when buffer > threshold
    if (perfV2 && pending >= DELTA_QUEUE_MAX) {
      // Already handled above via coalescing
      if (pending >= DELTA_QUEUE_MAX * 2) {
        droppedDeltas += 1;
      }
    }

    // Phase 2.3: Aggressive buffer management - keep only latest N deltas per pool when buffer is very large
    if (bufferSize > DELTA_QUEUE_MAX * 2) {
      // Group deltas by pool
      const poolDeltas = new Map<string, Array<{ key: string; snapshot: ScannerPricingSnapshot }>>();
      for (const [key, snap] of deltaBuffer.entries()) {
        const poolKey = key; // pool_address is already the key
        if (!poolDeltas.has(poolKey)) {
          poolDeltas.set(poolKey, []);
        }
        poolDeltas.get(poolKey)!.push({ key, snapshot: snap });
      }

      // Keep only latest MAX_DELTAS_PER_POOL per pool
      deltaBuffer.clear();
      let droppedCount = 0;
      for (const [pool, deltas] of poolDeltas.entries()) {
        // Sort by timestamp (newest first) and keep only latest N
        const sorted = deltas.sort((a, b) => {
          const aTs = a.snapshot.last_update_ts || 0;
          const bTs = b.snapshot.last_update_ts || 0;
          return bTs - aTs;
        });
        const latest = sorted.slice(0, MAX_DELTAS_PER_POOL);
        for (const { key, snapshot } of latest) {
          deltaBuffer.set(key, snapshot);
        }
        droppedCount += Math.max(0, sorted.length - MAX_DELTAS_PER_POOL);
      }
      droppedDeltas += droppedCount;
    }

    const newPending = deltaBuffer.size;
    deltaBufferHighWater = Math.max(deltaBufferHighWater, newPending);
    statsUpdater({ deltaBufferSize: newPending, deltaBufferHighWater });
    set({ pendingDeltas: newPending });
    if (newPending > BUFFER_WARNING_THRESHOLD) {
      console.warn('[ScannersStore] delta buffer exceeded 10k, retaining latest per pool');
    }
    if (get().liveEnabled) {
      // Phase 1.2: Start RAF loop when deltas are enqueued
      startRafApply();
      scheduleApply();
    } else {
      if (import.meta.env?.DEV && totalDeltasEnqueued % 10 === 0) {
        console.log('[ScannersStore] enqueueDelta skipped - liveEnabled=false');
      }
    }
  };

  const loadPage = async (params: { reset?: boolean } = {}) => {
    const state = get();
    if (state.loading || state.refreshing) return;
    const loaderState = params.reset ? { refreshing: true } : { loading: true };
    set(loaderState);
    try {
      // Track previous data hash to prevent unnecessary redraws
      const previousDataHash = params.reset ? null : JSON.stringify(
        Array.from(rowsById.values())
          .map(s => ({ pool: s.pool_address, ts: s.last_update_ts }))
          .sort((a, b) => (a.pool || '').localeCompare(b.pool || ''))
      );
      if (params.reset) {
        rowsById = new Map();
        enrichedById = new Map();
        sortedIdsByEdge = [];
        filteredSet = new Set();
        deltaBuffer.clear();
        filteredIdsList = [];
        filteredDirty = false;
        dexSet = new Set();
        chainSet = new Set();
        set({ filteredIds: [], dexOptions: [], chainOptions: [] });
      }
      const pageSize = state.scannerId === 'pcs_bnb_usdt' ? PCS_PAGE_SIZE : PAGE_SIZE_DEFAULT;
      let apiResponse: Awaited<ReturnType<typeof api.getScannerPricingSnapshots>> | null;
      try {
        const baseParams: any = {
          cursor: params.reset ? null : state.nextCursor,
          limit: pageSize,
          sort_by: 'last_update_ts',
          sort_dir: 'desc',
        };
        if (typeof state.filterSpec.min_edge_bps === 'number') {
          baseParams.min_edge_bps = state.filterSpec.min_edge_bps;
        }
        if (typeof state.filterSpec.min_tvl_usd === 'number') {
          baseParams.min_tvl_usd = state.filterSpec.min_tvl_usd;
        }
        if (state.filterSpec.search && state.filterSpec.search.trim()) {
          baseParams.search = state.filterSpec.search.trim();
        }

        apiResponse = await api.getScannerPricingSnapshots(state.scannerId, baseParams);

        // Fallback: on initial/filtered loads, retry once with no filters to avoid empty UI
        if (params.reset && (!apiResponse?.snapshots || apiResponse.snapshots.length === 0)) {
          apiResponse = await api.getScannerPricingSnapshots(state.scannerId, {
            cursor: null,
            limit: pageSize,
            sort_by: 'last_update_ts',
            sort_dir: 'desc',
          });
        }
      } catch (error) {
        apiResponse = null;
        if (import.meta.env?.DEV) {
          console.error('[ScannersStore] pricing fetch failed', error);
        }
      }
      const response = apiResponse ?? { snapshots: [], pageInfo: { has_more: false, next_cursor: null } };
      const snapshots = Array.isArray(response.snapshots) ? response.snapshots : [];
      if (params.reset && snapshots.length === 0 && rowsById.size === 0) {
        const aggParams: { dex_name?: string; chain?: string; bybit_marginable?: boolean; min_edge?: number } = {
          bybit_marginable: state.filterSpec.bybit_marginable === 'marginable' ? true : undefined,
          min_edge: state.filterSpec.min_edge_bps ?? undefined,
        };
        if (state.filterSpec.dex_name) aggParams.dex_name = state.filterSpec.dex_name;
        if (state.filterSpec.chain) aggParams.chain = state.filterSpec.chain;
        let aggSnapshots: ScannerPricingSnapshot[] = [];
        try {
          const agg = await api.getScannerAggregatePricingSnapshots(aggParams);
          if (agg && Array.isArray(agg.snapshots)) {
            aggSnapshots = agg.snapshots as ScannerPricingSnapshot[];
          }
        } catch (err) {
          if (import.meta.env?.DEV) {
            console.warn('[ScannersStore] aggregate fetch failed', err);
          }
        }
        if (aggSnapshots.length > 0) {
          for (const snapshot of aggSnapshots) {
            applySnapshot(snapshot as ScannerPricingSnapshot);
          }
        }
        // P1: Rebuild filteredIds after applying snapshots so initial load displays data
        rebuildFilteredIds(state.filterSpec);
        set({ hasMore: false, nextCursor: null });
      } else {
        for (const snapshot of snapshots) {
          applySnapshot(snapshot);
        }
        // Check if data actually changed to prevent unnecessary redraws
        const currentDataHash = JSON.stringify(
          Array.from(rowsById.values())
            .map(s => ({ pool: s.pool_address, ts: s.last_update_ts }))
            .sort((a, b) => (a.pool || '').localeCompare(b.pool || ''))
        );
        const dataChanged = previousDataHash !== currentDataHash;

        // Only rebuild filteredIds if data actually changed (prevents unnecessary redraws)
        // rebuildFilteredIds already calls set() internally, so we skip if unchanged
        if (dataChanged) {
          rebuildFilteredIds(state.filterSpec);
        }
        set({
          hasMore: Boolean(response.pageInfo?.has_more),
          nextCursor: response.pageInfo?.next_cursor ?? null,
        });
      }

      // Manual mode: run stale eviction after each full refresh
      evictStaleRows();

      // Update lastAppliedAtTs with actual data timestamp (max of all enriched rows)
      // This ensures "Last Update" column reflects reality, not client time
      let maxDataTs = 0;
      for (const enriched of enrichedById.values()) {
        if (enriched && enriched.last_update_ts > 0) {
          if (enriched.last_update_ts > maxDataTs) {
            maxDataTs = enriched.last_update_ts;
          }
        }
      }
      if (maxDataTs > 0) {
        statsUpdater({ lastAppliedAtTs: maxDataTs });
      } else {
        // Fallback to current time only if no data timestamps available
        statsUpdater({ lastAppliedAtTs: Date.now() });
      }
    } catch (error) {
      if (import.meta.env?.DEV) console.error('[ScannersStore] pricing fetch failed', error);
    } finally {
      set({ loading: false, refreshing: false, nowTs: Date.now() });
    }
  };

  // Phase 2.2: Memory cleanup - evict stale rows
  const evictStaleRows = () => {
    const now = Date.now();
    const toEvict: string[] = [];

    // First pass: collect stale rows (not updated in last N minutes)
    for (const [id, row] of enrichedById.entries()) {
      const age = now - (row.last_update_ts || 0);
      if (age > STALE_THRESHOLD_MS) {
        toEvict.push(id);
      }
    }

    // Second pass: if still over limit, evict lowest edge rows
    const currentSize = enrichedById.size - toEvict.length;
    if (currentSize > MAX_ROWS) {
      const remaining = Array.from(enrichedById.entries())
        .filter(([id]) => !toEvict.includes(id))
        .sort((a, b) => a[1].bestEdgeNum - b[1].bestEdgeNum); // Sort by edge ascending

      const extra = remaining.slice(0, currentSize - MAX_ROWS);
      toEvict.push(...extra.map(([id]) => id));
    }

    // Remove evicted rows
    if (toEvict.length > 0) {
      for (const id of toEvict) {
        rowsById.delete(id);
        enrichedById.delete(id);
        const idx = sortedIdsByEdge.indexOf(id);
        if (idx >= 0) {
          sortedIdsByEdge.splice(idx, 1);
        }
        filteredSet.delete(id);
        const removalIndex = filteredIdsList.indexOf(id);
        if (removalIndex >= 0) {
          filteredIdsList.splice(removalIndex, 1);
        }
      }

      // Rebuild filteredIds if needed
      if (filteredDirty || toEvict.length > 0) {
        rebuildFilteredIds(filterSpec);
      }

      if (import.meta.env?.DEV) {
        console.log(`[ScannersStore] Evicted ${toEvict.length} stale rows, ${enrichedById.size} remaining`);
      }
    }
  };

  // Perf V2: Publish stats to Redis (via API endpoint)
  const publishStatsToRedis = async () => {
    if (!isScannersPerfV2Enabled()) return;
    try {
      const state = get();
      const stats = state.stats;
      await api.publishScannerPerfStats({
        updates_per_sec: stats.updatesPerSec,
        apply_duration_p50_ms: stats.applyDurationP50Ms,
        apply_duration_p95_ms: stats.applyDurationP95Ms,
        index_update_p50_ms: stats.indexUpdateDurationP50Ms,
        index_update_p95_ms: stats.indexUpdateDurationP95Ms,
        render_duration_p50_ms: stats.renderDurationP50Ms,
        render_duration_p95_ms: stats.renderDurationP95Ms,
        visible_rows: stats.virtualRowsRendered,
        total_rows: stats.totalRows,
        dropped_delta_rate_pct: stats.droppedDeltaRatePct,
        delta_buffer_size: stats.deltaBufferSize,
        delta_buffer_high_water: stats.deltaBufferHighWater,
        last_update_ts: stats.lastAppliedAtTs || Date.now(),
        last_apply_duration_ms: stats.lastApplyDurationMs,
        last_applied_at_ts: stats.lastAppliedAtTs || Date.now(),
      });
    } catch (error) {
      // Silently fail - stats publishing is non-critical
      if (import.meta.env?.DEV) {
        console.debug('[ScannersStore] Failed to publish stats to Redis', error);
      }
    }
  };

  // Perf V2: Optimized age ticker with visibility/viewport throttling
  const setupAgeTicker = () => {
    if (typeof window === 'undefined' || !AGE_TICKER_ENABLED) return;

    // Clear any existing ticker first
    if (nowTicker) {
      window.clearInterval(nowTicker);
      nowTicker = null;
    }

    const perfV2 = isScannersPerfV2Enabled();
    let currentTickInterval = NOW_TICK_MS;
    let lastActivityTime = Date.now();
    let isHidden = false;

    const tick = () => {
      const now = Date.now();
      const state = get();

      // Perf V2: Idle detection - if virtualization enabled + 0 deltas + no scroll in last 2s
      // Only apply idle detection if we've had at least one delta or scroll event
      if (perfV2) {
        // Only check idle if we've had activity; otherwise use normal interval
        if (lastDeltaTime > 0 || lastScrollTime > 0) {
          const timeSinceLastDelta = lastDeltaTime > 0 ? now - lastDeltaTime : Infinity;
          const timeSinceLastScroll = lastScrollTime > 0 ? now - lastScrollTime : Infinity;
          const isIdle = timeSinceLastDelta > IDLE_DETECTION_MS && timeSinceLastScroll > IDLE_DETECTION_MS;

          if (isIdle && currentTickInterval !== IDLE_TICK_MS) {
            currentTickInterval = IDLE_TICK_MS;
            if (nowTicker) {
              window.clearInterval(nowTicker);
              nowTicker = window.setInterval(tick, currentTickInterval);
            }
          } else if (!isIdle && currentTickInterval !== NOW_TICK_MS) {
            currentTickInterval = NOW_TICK_MS;
            if (nowTicker) {
              window.clearInterval(nowTicker);
              nowTicker = window.setInterval(tick, currentTickInterval);
            }
          }
        }
      }

      // Always update nowTs to keep timestamps fresh
      set({ nowTs: now });
    };

    // Perf V2: Visibility change handler
    if (perfV2) {
      visibilityChangeHandler = () => {
        isHidden = document.hidden;
        if (isHidden) {
          // Pause or slow down ticker when hidden
          if (nowTicker) {
            window.clearInterval(nowTicker);
            nowTicker = window.setInterval(tick, THROTTLE_HIDDEN_MS);
          }
        } else {
          // Resume normal ticker when visible
          if (nowTicker) {
            window.clearInterval(nowTicker);
            currentTickInterval = NOW_TICK_MS;
            nowTicker = window.setInterval(tick, currentTickInterval);
          }
        }
      };
      document.addEventListener('visibilitychange', visibilityChangeHandler);
    }

    nowTicker = window.setInterval(tick, currentTickInterval);
  };

  const initialize = () => {
    if (initialized) return;
    initialized = true;
    set({ initialized: true });

    // WebSocket streaming disabled - relying on manual refresh only
    // Socket listener registration removed

    if (typeof window !== 'undefined') {
      if (AGE_TICKER_ENABLED) {
        setupAgeTicker();
        updateRateInterval = window.setInterval(() => {
          statsUpdater({ updatesPerSec: updatesThisWindow });
          updatesThisWindow = 0;
        }, UPDATE_RATE_INTERVAL_MS);

        // Perf V2: Start Redis stats publishing
        if (isScannersPerfV2Enabled()) {
          redisStatsUpdateHandle = window.setInterval(() => {
            publishStatsToRedis();
          }, REDIS_STATS_UPDATE_INTERVAL_MS);
        }

        // Phase 2.2: Start memory cleanup timer
        evictionTimer = window.setInterval(() => {
          evictStaleRows();
        }, EVICTION_INTERVAL_MS);
      } else {
        set({ nowTs: Date.now() });
      }

      // Start RAF loop for smooth delta application (only if deltas exist)
      // Don't start here - let enqueueDelta start it when needed
    }
  };

  return {
    initialized: false,
    scannerId: DEFAULT_SCANNER_ID,
    dataVersion: 0,
    setScannerId: (id: string) => {
      set({ scannerId: id });
      loadPage({ reset: true });
    },
    filteredIds: [],
    hasMore: false,
    nextCursor: null,
    loading: false,
    refreshing: false,
    liveEnabled: false,
    pendingDeltas: 0,
    stats: {
      applyDurationLastMs: 0,
      applyDurationP50Ms: 0,
      applyDurationP95Ms: 0,
      applyDurationP99Ms: 0,
      deltaBufferSize: 0,
      deltaBufferHighWater: 0,
      updatesPerSec: 0,
      lastApplyDurationMs: 0,
      lastAppliedAtTs: 0,
      virtualRowsRendered: 0,
      totalRows: 0,
      indexUpdateDurationP50Ms: 0,
      indexUpdateDurationP95Ms: 0,
      renderDurationP50Ms: 0,
      renderDurationP95Ms: 0,
      droppedDeltas: 0,
      droppedDeltaRatePct: 0,
    },
    nowTs: Date.now(),
    dexOptions: [],
    chainOptions: [],
    filterSpec,
    loadInitial: () => loadPage({ reset: true }),
    refresh: () => loadPage({ reset: true }),
    loadMore: () => loadPage({ reset: false }),
    setFilters: (filters: FilterValues) => {
      const spec = buildFilterSpec(filters);
      filterSpec = spec;
      set({ filterSpec: spec });
      rebuildFilteredIds(spec);
      loadPage({ reset: true });
    },
    enqueueDelta,
    toggleLive: (enabled?: boolean) => {
      const previous = get().liveEnabled;
      const next = enabled ?? !previous;
      set({ liveEnabled: next });
      if (next && !previous) {
        scheduleApply();
      }
    },
    getRowById: (id: string) => enrichedById.get(id.toLowerCase()),
    getVisibleRows: (start: number, end: number) => {
      const rows: EnrichedRow[] = [];
      for (let i = start; i < end; i += 1) {
        const id = get().filteredIds[i];
        if (!id) break;
        const row = enrichedById.get(id);
        if (row) rows.push(row);
      }
      return rows;
    },
    getTotalCount: () => sortedIdsByEdge.length,
    initialize,
    setVirtualRenderStats: (rendered: number) => {
      statsUpdater({ virtualRowsRendered: rendered, totalRows: sortedIdsByEdge.length });
    },
    // Perf V2: Expose recordRenderDuration for component use
    recordRenderDuration,
    // Perf V2: Expose recordScroll for idle detection
    recordScroll: () => {
      lastScrollTime = Date.now();
    },
    noteScroll,
    startRafApply,
    stopRafApply,
  };
});
