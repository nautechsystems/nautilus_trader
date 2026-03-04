/**
 * ScannersTable Component - Pool pricing snapshots with filtering and manual refresh.
 *
 * Bridges the scanner store (API + indexing) into the UI and renders
 * the DataTable with virtualization and telemetry.
 * Auto-refresh and WebSocket streaming disabled - relies on manual refresh only.
 */

import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { RefreshCw, ChevronDown } from 'lucide-react';
import { type ColumnDef } from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { shallow } from 'zustand/shallow';
import { api } from '@/api';
import type { SignalLeg } from '@/types';
import { DataTable } from '@/components/ui/table/DataTable';
import { PanelHeader } from '@/components/shared/PanelHeader';
import { PanelBody } from '@/components/shared/PanelBody';
import { TableFilter, type FilterValues, type ColumnFilter } from '@/components/shared/TableFilter';
import { StatusPill } from '@/components/shared/StatusPill';
import { Badge } from '@/components/ui/badge';
import { TimeAgo } from '@/components/shared/TimeAgo';
import { colors, spacing, typography, STALE_THRESHOLDS, borderRadius } from '@/lib/tokens';
import { fmtPrice } from '@/utils';
import { formatAbsoluteTime, formatLocal } from '@/utils/time';
import { isScannersVirtualizedEnabled, isScannersPerfV2Enabled } from '@/config/featureFlags';
import {
  selectScannersTableActions,
  selectScannersTableData,
  selectScannersTableTelemetry,
  useScannersStore,
  type EnrichedRow,
} from '@/stores/scannersStore';
import { formatFeeBps, formatEdgeValue, formatUsdCompact } from '@/utils/scannersFormatting';
import { useMobileLayout } from '@/hooks/useMobileLayout';
import { cn } from '@/lib/utils';
import { Select } from '@/components/ui/select';

export const SCANNERS_COLUMN_TITLES = {
  pair: 'Pool',
  dex: 'DEX',
  chain: 'Chain',
  tvl: 'TVL',
  vol: 'Vol 24h',
  legA: 'Leg A',
  legB: 'Leg B',
  dexFee: 'DEX Fee',
  cexFee: 'CEX Fee',
  bestEdge: 'Best Edge',
  edgeBtoA: 'B->A Edge',
  edgeAtoB: 'A->B Edge',
  marginable: 'Marginable',
  lastUpdate: 'Last Update',
  localTime: 'Local Time',
} as const;

export const SCANNERS_COLUMN_SEQUENCE = [
  SCANNERS_COLUMN_TITLES.pair,
  SCANNERS_COLUMN_TITLES.dex,
  SCANNERS_COLUMN_TITLES.chain,
  SCANNERS_COLUMN_TITLES.bestEdge,
  SCANNERS_COLUMN_TITLES.vol,
  SCANNERS_COLUMN_TITLES.tvl,
  SCANNERS_COLUMN_TITLES.legA,
  SCANNERS_COLUMN_TITLES.legB,
  SCANNERS_COLUMN_TITLES.dexFee,
  SCANNERS_COLUMN_TITLES.cexFee,
  SCANNERS_COLUMN_TITLES.edgeBtoA,
  SCANNERS_COLUMN_TITLES.edgeAtoB,
  SCANNERS_COLUMN_TITLES.marginable,
  SCANNERS_COLUMN_TITLES.lastUpdate,
  SCANNERS_COLUMN_TITLES.localTime,
] as const;

const MANUAL_STALE_REMINDER_MS = 2 * 60 * 1000;

const formatManualAge = (ms: number): string => {
  if (ms <= 0) return 'just now';
  const minutes = Math.floor(ms / 60000);
  if (minutes >= 1) {
    return `${minutes}m`;
  }
  const seconds = Math.floor(ms / 1000);
  return `${Math.max(seconds, 1)}s`;
};

// Phase 1.4: Optimized Last Update cell - memoized to reduce re-renders
const LastUpdateCell = React.memo(({ rowId }: { rowId: string }) => {
  // Subscribe to dataVersion to trigger re-renders when enriched rows are updated
  const dataVersion = useScannersStore((s) => s.dataVersion);
  const ts = useScannersStore((s) => {
    const row = s.getRowById(rowId);
    if (!row) return 0;
    const c = Number(row.cex_last_update_ts || 0);
    const d = Number(row.dex_last_update_ts || 0);
    const l = Number(row.last_update_ts || 0);
    return Math.max(c, d, l);
  });
  // Phase 1.4: Throttle nowTs - round to 5s intervals to reduce update frequency
  const nowTsRaw = useScannersStore((s) => s.nowTs);
  const nowTs = Math.floor(nowTsRaw / 5000) * 5000; // Update every 5s instead of every 1s

  if (!ts || ts <= 0) {
    return <span className="text-xs" style={{ color: colors.text.muted }}>—</span>;
  }
  return <TimeAgo timestamp={ts} now={nowTs} className="text-xs" style={{ color: colors.text.muted }} />;
}, (prev, next) => {
  // Only re-render if rowId changes (timestamp updates handled by store subscription)
  return prev.rowId === next.rowId;
});

export function LegCell({ leg, row }: { leg: SignalLeg | null; row?: EnrichedRow }) {
  if (!leg) return <span style={{ color: colors.text.muted }}>N/A</span>;
  const perfV2 = isScannersPerfV2Enabled();

  const bid = Number(leg.decision_bid ?? leg.fv_bid ?? 0) || 0;
  const ask = Number(leg.decision_ask ?? leg.fv_ask ?? 0) || 0;
  const mid = (bid + ask) / 2;

  // Perf V2: Use preformatted strings if available
  let bidDisplay: string;
  let askDisplay: string;
  let midDisplay: string;

  if (perfV2 && row) {
    if (leg.exchange === 'bybit') {
      // CEX: Use preformatted CEX bid/ask
      bidDisplay = row.cexBidDisplay || fmtPrice(bid);
      askDisplay = row.cexAskDisplay || fmtPrice(ask);
      midDisplay = fmtPrice(mid);
    } else {
      // DEX: Use preformatted DEX mid for all (DEX typically has single price)
      const dexMid = row.dexMidDisplay || fmtPrice(mid);
      bidDisplay = dexMid;  // DEX bid ≈ mid
      askDisplay = dexMid;  // DEX ask ≈ mid
      midDisplay = dexMid;
    }
  } else {
    bidDisplay = fmtPrice(bid);
    askDisplay = fmtPrice(ask);
    midDisplay = fmtPrice(mid);
  }

  const marketLabel = `${leg.exchange} ${leg.coin}`;

  return (
    <div className="flex flex-col gap-0.5" title={marketLabel}>
      <div
        className="text-xs flex items-center truncate font-medium"
        style={{ fontSize: typography.fontSize.xs, color: colors.text.muted }}
      >
        {marketLabel}
      </div>
      <div className="flex gap-1.5 tabular-nums font-mono" style={{ fontSize: typography.fontSize.sm }}>
        <span style={{ color: colors.semantic.success.DEFAULT }}>{bidDisplay}</span>
        <span className="opacity-70" style={{ color: colors.text.secondary }}>{midDisplay}</span>
        <span style={{ color: colors.semantic.danger.DEFAULT }}>{askDisplay}</span>
      </div>
    </div>
  );
}

export interface ScannersTableProps {
  onRemove?: () => void;
  showHeader?: boolean;
}

export default function ScannersTable({ onRemove, showHeader = true }: ScannersTableProps = {}) {
  // Use stable selector slices with shallow equality to avoid rerenders on unrelated store updates.
  const {
    initialize,
    loadInitial,
    refresh,
    loadMore,
    setFilters: setFiltersAction,
    getRowById,
    setVirtualRenderStats,
    recordRenderDuration,
    recordScroll,
    setScannerId,
    noteScroll,
    stopRafApply,
  } = useScannersStore(selectScannersTableActions, shallow);

  const {
    filteredIds,
    hasMore,
    loading,
    refreshing,
    dexOptions,
    chainOptions,
    nowTs,
    filterSpec,
    scannerId,
  } = useScannersStore(selectScannersTableData, shallow);

  const {
    lastAppliedAtTs,
    updatesPerSec,
    applyDurationP95Ms,
    deltaBufferSize,
  } = useScannersStore(selectScannersTableTelemetry, shallow);

  // Initialize store (sets up ticker for nowTs updates)
  // Phase 1.2: RAF loop now starts automatically when deltas are enqueued
  useEffect(() => {
    initialize();
    return () => {
      // Cleanup: stop RAF loop and clear scroll back-off timeout
      stopRafApply();
    };
  }, [initialize, stopRafApply]);

  // Convert filterSpec to FilterValues format for TableFilter component
  const filters = useMemo<FilterValues>(() => ({
    pairLabel: filterSpec.pairLabel || '',
    dex_name: filterSpec.dex_name || '',
    chain: filterSpec.chain || '',
    bybit_marginable: filterSpec.bybit_marginable || '',
    min_edge_bps: filterSpec.min_edge_bps?.toString() || '',
    min_tvl_usd: filterSpec.min_tvl_usd?.toString() || '',
    exclude_stable: filterSpec.exclude_stable ? '1' : '',
  }), [filterSpec]);

  useEffect(() => {
    initialize();
    loadInitial();
  }, [initialize, loadInitial]);

  const [scannerOptions, setScannerOptions] = useState<{ id: string; label: string }[]>([]);
  const [registryLoaded, setRegistryLoaded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const fetchRegistry = async () => {
      try {
        const registry = await api.getScannersRegistry();
        const scanners = registry?.scanners || [];
        if (!scanners.length) return;

        const options = scanners.map((s) => ({
          id: s.scanner_id,
          label: `${s.scanner_id} (${s.dex_name || 'dex'} / ${s.chain || '-'})`,
        }));
        if (!cancelled) {
          setScannerOptions(options);
        }

      } catch (error) {
        if (import.meta.env?.DEV) {
          console.warn('[ScannersTable] Failed to fetch registry', error);
        }
      } finally {
        if (!cancelled) setRegistryLoaded(true);
      }
    };
    fetchRegistry();
    return () => {
      cancelled = true;
    };
  }, [setScannerId]);

  const isRefreshing = refreshing || loading;

  // Auto-refresh and polling disabled - relying on manual refresh only

  const handleFilterChange = useCallback(
    (changes: FilterValues) => {
      setFiltersAction({ ...filters, ...changes });
    },
    [setFiltersAction, filters],
  );

  const updateFilterField = useCallback(
    (key: keyof FilterValues, value: string) => {
      setFiltersAction({ ...filters, [key]: value });
    },
    [setFiltersAction, filters],
  );

  const virtualizationEnabled = isScannersVirtualizedEnabled();
  const { isMobile } = useMobileLayout();
  const scrollRef = useRef<HTMLDivElement>(null);
  const rowHeight = parseInt(spacing.row.normal, 10) || 28;
  const virtualizer = useVirtualizer<HTMLDivElement, HTMLTableRowElement>({
    count: !isMobile && virtualizationEnabled ? filteredIds.length : 0,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: 6,
  });
  const virtualItems = isMobile ? [] : virtualizer.getVirtualItems();
  const shouldVirtualize = !isMobile && virtualizationEnabled && virtualItems.length > 0;

  // P2 Fix: When virtualization is enabled, compute visible IDs for stats tracking only
  // But pass FULL dataset to DataTable - virtualizer will handle slicing internally via virtualRow.index
  const visibleIds = shouldVirtualize
    ? virtualItems.map((item) => filteredIds[item.index]).filter((id): id is string => Boolean(id))
    : filteredIds;

  // Full dataset for DataTable - when virtualized, DataTable will slice via virtualizer
  // When not virtualized, we render all rows anyway
  const visibleRows = useMemo<EnrichedRow[]>(() => {
    if (shouldVirtualize && virtualItems.length > 0) {
      // Only compute visible rows when virtualized
      return virtualItems
        .map((item) => {
          const id = filteredIds[item.index];
          return id ? getRowById(id) : null;
        })
        .filter((row): row is EnrichedRow => Boolean(row));
    }
    // Non-virtualized: compute all filtered rows
    return filteredIds
      .map((id) => getRowById(id))
      .filter((row): row is EnrichedRow => Boolean(row));
  }, [shouldVirtualize, virtualItems, filteredIds, getRowById]);

  const renderMobileRow = useCallback((row: EnrichedRow) => (
    <ScannersMobileCard row={row} nowTs={nowTs} />
  ), [nowTs]);

  useEffect(() => {
    if (!virtualizationEnabled) {
      setVirtualRenderStats(filteredIds.length);
      return;
    }
    setVirtualRenderStats(shouldVirtualize ? virtualItems.length : filteredIds.length);
  }, [virtualizationEnabled, shouldVirtualize, virtualItems.length, filteredIds.length, setVirtualRenderStats]);

  // P1: Load more when virtualized - trigger near bottom of virtualized list
  useEffect(() => {
    if (!virtualizationEnabled || !hasMore || loading) return;
    const threshold = 50;
    const lastItem = virtualItems[virtualItems.length - 1];
    if (!lastItem) return;
    if (lastItem.index >= filteredIds.length - 1 - threshold) {
      loadMore();
    }
  }, [virtualizationEnabled, hasMore, loading, filteredIds.length, virtualItems, loadMore]);

  // Combined scroll handler: P1 (loadMore when not virtualized) + Perf V2 (idle detection) + Zero-flash (back-off)
  useEffect(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) return;

    let loadMoreTimeout: number | null = null;
    const perfV2Enabled = isScannersPerfV2Enabled();

    const handleScroll = () => {
      // Zero-flash: Notify store of scroll for delta back-off
      noteScroll();

      // Perf V2: Track scroll for idle detection
      if (perfV2Enabled && recordScroll) {
        recordScroll();
      }

      // P1: Load more when NOT virtualized - detect scroll near bottom
      // Check conditions inside handler to get latest values
      if (!virtualizationEnabled && hasMore && !loading) {
        // Debounce loadMore to avoid rapid-fire calls
        if (loadMoreTimeout !== null) {
          return;
        }

        const { scrollTop, scrollHeight, clientHeight } = scrollElement;
        const threshold = 200; // pixels from bottom
        if (scrollHeight - scrollTop - clientHeight < threshold) {
          loadMoreTimeout = window.setTimeout(() => {
            loadMoreTimeout = null;
            // Re-check conditions before calling (may have changed during debounce)
            if (!loading && hasMore && !virtualizationEnabled) {
              loadMore();
            }
          }, 100); // 100ms debounce
        }
      }
    };

    scrollElement.addEventListener('scroll', handleScroll, { passive: true });
    return () => {
      scrollElement.removeEventListener('scroll', handleScroll);
      if (loadMoreTimeout !== null) {
        clearTimeout(loadMoreTimeout);
      }
    };
  }, [virtualizationEnabled, hasMore, loading, loadMore, recordScroll, noteScroll]);

  const lastAppliedTs = lastAppliedAtTs || 0;
  const staleAgeMs = Math.max(0, Date.now() - lastAppliedTs);
  const staleAgeLabel = formatManualAge(staleAgeMs);
  const isStaleReminder = staleAgeMs > MANUAL_STALE_REMINDER_MS;

  const telemetryActions = useMemo(() => (
    <div className="flex items-center gap-2 text-[11px] uppercase" style={{ color: colors.text.muted }}>
      <Badge
        variant={isStaleReminder ? 'warning' : 'neutral'}
        size="xs"
      >
        {isStaleReminder ? `Stale ${staleAgeLabel}` : `Refreshed ${staleAgeLabel}`}
      </Badge>
      <span>Updates: {updatesPerSec}/s</span>
      <span>Apply p95: {applyDurationP95Ms.toFixed(1)}ms</span>
      <span>Buffer: {deltaBufferSize}</span>
    </div>
  ), [updatesPerSec, applyDurationP95Ms, deltaBufferSize, isStaleReminder, staleAgeLabel]);

  const manualModeIndicator = useMemo(() => (
    <Badge
      variant="neutral"
      size="xs"
      className="text-[10px]"
    >
      Manual Mode
    </Badge>
  ), []);

  const customControls = (
    <div className="flex flex-wrap items-center gap-3">
      <span className="text-[11px] uppercase tracking-wide font-medium" style={{ color: colors.text.muted }}>
        Loaded: {filteredIds.length}{hasMore ? '+' : ''}
      </span>
      {scannerOptions.length > 0 && (
        <div className="flex items-center gap-2 text-xs font-medium" style={{ color: colors.text.secondary }}>
          <label htmlFor="scanner-select">Scanner</label>
          <select
            id="scanner-select"
            value={scannerId}
            onChange={(e) => {
              setScannerId(e.target.value);
              refresh();
            }}
            className="rounded px-2 py-1 text-xs"
            style={{
              backgroundColor: colors.bg.surface,
              borderColor: colors.border.DEFAULT,
              color: colors.text.primary,
              borderWidth: '1px'
            }}
            disabled={!registryLoaded}
          >
            {scannerOptions.map((opt) => (
              <option key={opt.id} value={opt.id}>{opt.label}</option>
            ))}
          </select>
        </div>
      )}
      <label className="flex items-center gap-2 text-xs font-medium" style={{ color: colors.text.secondary }}>
        Min Edge (bps)
        <input
          type="number"
          value={filters.min_edge_bps || ''}
          onChange={(e) => updateFilterField('min_edge_bps', e.target.value)}
          placeholder="0"
          className="rounded px-2 py-1 w-20 text-xs focus:outline-none focus:border-accent-primary"
          style={{
            backgroundColor: colors.bg.surface,
            borderColor: colors.border.DEFAULT,
            color: colors.text.primary,
            borderWidth: '1px'
          }}
        />
      </label>
      <label className="flex items-center gap-2 text-xs font-medium" style={{ color: colors.text.secondary }}>
        Min TVL (USD)
        <input
          type="number"
          value={filters.min_tvl_usd || ''}
          onChange={(e) => updateFilterField('min_tvl_usd', e.target.value)}
          placeholder="0"
          className="rounded px-2 py-1 w-24 text-xs focus:outline-none focus:border-accent-primary"
          style={{
            backgroundColor: colors.bg.surface,
            borderColor: colors.border.DEFAULT,
            color: colors.text.primary,
            borderWidth: '1px'
          }}
        />
      </label>
      <label className="flex items-center gap-2 text-xs cursor-pointer select-none font-medium" style={{ color: colors.text.secondary }}>
        <input
          type="checkbox"
          checked={(filters.bybit_marginable || '').toLowerCase() === 'marginable'}
          onChange={(e) => updateFilterField('bybit_marginable', e.target.checked ? 'marginable' : '')}
          className="rounded accent-accent-primary"
          style={{
            borderColor: colors.border.DEFAULT,
            backgroundColor: colors.bg.surface,
          }}
        />
        Marginable only
      </label>
      <label className="flex items-center gap-2 text-xs cursor-pointer select-none font-medium" style={{ color: colors.text.secondary }}>
        <input
          type="checkbox"
          checked={(filters.exclude_stable || '').toLowerCase() === '1' || (filters.exclude_stable || '').toLowerCase() === 'true'}
          onChange={(e) => updateFilterField('exclude_stable', e.target.checked ? '1' : '')}
          className="rounded accent-accent-primary"
          style={{
            borderColor: colors.border.DEFAULT,
            backgroundColor: colors.bg.surface,
          }}
        />
        Exclude stable-stable
      </label>
      <button
        type="button"
        onClick={refresh}
        disabled={isRefreshing}
        title="Fetch latest pool snapshots"
        className={cn(
          "inline-flex items-center gap-1.5 rounded px-2.5 py-1 text-xs font-medium transition-colors hover:text-text-primary",
          isRefreshing && "cursor-not-allowed opacity-60"
        )}
        style={{
          backgroundColor: colors.bg.surface,
          borderColor: colors.border.DEFAULT,
          borderWidth: '1px',
          color: colors.text.secondary
        }}
      >
        <RefreshCw className={cn("w-3.5 h-3.5", isRefreshing && "animate-spin")} />
        Refresh
      </button>
    </div>
  );

  const columns = useMemo<ColumnDef<EnrichedRow>[]>(() => [
    {
      id: 'pair',
      header: SCANNERS_COLUMN_TITLES.pair,
      accessorKey: 'pairLabel',
      size: 120,
      minSize: 100,
      cell: ({ row }) => (
        <span
          className="whitespace-nowrap truncate block font-medium"
          style={{ color: colors.text.secondary, fontSize: typography.fontSize.sm }}
          title={row.original.pairLabel}
        >
          {row.original.pairLabel}
        </span>
      ),
    },
    {
      accessorKey: 'dex_name',
      header: SCANNERS_COLUMN_TITLES.dex,
      size: 90,
      minSize: 75,
      cell: ({ row }) => (
        <span
          className="truncate block"
          style={{ color: colors.text.tertiary, fontSize: typography.fontSize.xs }}
          title={row.original.dex_name || '—'}
        >
          {row.original.dex_name || '—'}
        </span>
      ),
    },
    {
      accessorKey: 'chain',
      header: SCANNERS_COLUMN_TITLES.chain,
      size: 55,
      minSize: 45,
      meta: { align: 'center' },
      cell: ({ row }) => (
        <span
          className="uppercase font-mono text-[10px] block text-center font-medium tracking-wider"
          style={{ color: colors.text.tertiary }}
          title={row.original.chain || '—'}
        >
          {(row.original.chain || '—').slice(0, 3)}
        </span>
      ),
    },
    {
      id: 'best_edge',
      header: SCANNERS_COLUMN_TITLES.bestEdge,
      accessorFn: (row) => row.bestEdgeNum,
      size: 80,
      minSize: 70,
      meta: { align: 'right' },
      cell: ({ row }) => {
        const perfV2 = isScannersPerfV2Enabled();
        const displayValue = perfV2 && row.original.bestEdgeDisplay
          ? row.original.bestEdgeDisplay
          : formatEdgeValue(row.original.bestEdgeNum);
        const bestEdge = row.original.bestEdgeNum;
        return (
          <span
            className="text-right tabular-nums block w-full font-mono"
            style={{
              color: bestEdge >= 50
                ? colors.semantic.success.DEFAULT
                : bestEdge >= 20
                  ? colors.semantic.warning.DEFAULT
                  : colors.text.tertiary,
              fontSize: typography.fontSize.sm,
              fontWeight: typography.fontWeight.medium,
            }}
          >
            {displayValue}
          </span>
        );
      },
    },
    {
      id: 'tvl',
      header: SCANNERS_COLUMN_TITLES.tvl,
      accessorFn: (row) => row.tvlNum,
      size: 85,
      minSize: 70,
      meta: { align: 'right' },
      cell: ({ row }) => {
        const displayValue = row.original.tvlDisplay || formatUsdCompact(row.original.tvlNum);
        return (
          <span className="text-right tabular-nums block w-full font-mono text-sm" style={{ color: colors.text.secondary }}>
            {displayValue}
          </span>
        );
      },
    },
    {
      id: 'vol24',
      header: SCANNERS_COLUMN_TITLES.vol,
      accessorFn: (row) => row.vol24Num,
      size: 85,
      minSize: 70,
      meta: { align: 'right' },
      cell: ({ row }) => {
        const displayValue = row.original.vol24Display || formatUsdCompact(row.original.vol24Num);
        return (
          <span className="text-right tabular-nums block w-full font-mono text-sm" style={{ color: colors.text.secondary }}>
            {displayValue}
          </span>
        );
      },
    },
    {
      id: 'legA',
      header: SCANNERS_COLUMN_TITLES.legA,
      size: 165,
      minSize: 150,
      cell: ({ row }) => <LegCell leg={row.original.legA} row={row.original} />,
    },
    {
      id: 'legB',
      header: SCANNERS_COLUMN_TITLES.legB,
      size: 165,
      minSize: 150,
      cell: ({ row }) => <LegCell leg={row.original.legB} row={row.original} />,
    },
    {
      id: 'dex_fee',
      header: SCANNERS_COLUMN_TITLES.dexFee,
      accessorFn: (row) => Number(row.dex_fee_bps ?? 0),
      size: 60,
      minSize: 50,
      meta: { align: 'right' },
      cell: ({ row }) => (
        <span
          className="font-mono tabular-nums text-right block text-sm opacity-80"
          style={{ color: colors.text.secondary }}
        >
          {formatFeeBps(Number(row.original.dex_fee_bps))}
        </span>
      ),
    },
    {
      id: 'cex_fee',
      header: () => (
        <span title="CEX fee (basis points)">{SCANNERS_COLUMN_TITLES.cexFee}</span>
      ),
      accessorFn: (row) => {
        const snap = row.original;
        const bestDir = (snap.best_direction || '').toString();
        const eff = Number(snap.cex_fee_effective_bps);
        if (eff) return eff;
        const sellPath = Number(snap.cex_fee_sell_path_bps);
        const buyPath = Number(snap.cex_fee_buy_path_bps);
        if (bestDir === 'sell_dex_buy_cex' && sellPath) return sellPath;
        if (bestDir === 'buy_dex_sell_cex' && buyPath) return buyPath;
        return Number(snap.cex_fee_bps);
      },
      size: 70,
      minSize: 60,
      meta: { align: 'right' },
      cell: ({ row }) => {
        const snap = row.original;
        const bestDir = (snap.best_direction || '').toString();
        let val = Number(snap.cex_fee_effective_bps);
        if (!val) {
          val = bestDir === 'sell_dex_buy_cex'
            ? (Number(snap.cex_fee_sell_path_bps) || Number(snap.cex_fee_bps))
            : (Number(snap.cex_fee_buy_path_bps) || Number(snap.cex_fee_bps));
        }
        return (
          <span
            className="text-right tabular-nums block w-full font-mono text-sm opacity-80"
            style={{ color: colors.text.secondary }}
          >
            {formatFeeBps(val)}
          </span>
        );
      },
    },
    {
      id: 'case2_edge',
      header: SCANNERS_COLUMN_TITLES.edgeBtoA,
      accessorFn: (row) => row.netEdgeSellNum,
      size: 80,
      minSize: 70,
      meta: { align: 'right' },
      cell: ({ row }) => {
        const perfV2 = isScannersPerfV2Enabled();
        const displayValue = perfV2 && row.original.netEdgeSellDisplay
          ? row.original.netEdgeSellDisplay
          : formatEdgeValue(row.original.netEdgeSellNum);
        return (
          <span
            className="text-right tabular-nums block w-full font-mono text-sm"
            style={{
              color: row.original.netEdgeSellNum >= 50
                ? colors.semantic.success.DEFAULT
                : row.original.netEdgeSellNum >= 20
                  ? colors.semantic.warning.DEFAULT
                  : colors.text.tertiary,
            }}
          >
            {displayValue}
          </span>
        );
      },
    },
    {
      id: 'case1_edge',
      header: SCANNERS_COLUMN_TITLES.edgeAtoB,
      accessorFn: (row) => row.netEdgeBuyNum,
      size: 80,
      minSize: 70,
      meta: { align: 'right' },
      cell: ({ row }) => {
        const perfV2 = isScannersPerfV2Enabled();
        const displayValue = perfV2 && row.original.netEdgeBuyDisplay
          ? row.original.netEdgeBuyDisplay
          : formatEdgeValue(row.original.netEdgeBuyNum);
        return (
          <span
            className="text-right tabular-nums block w-full font-mono text-sm"
            style={{
              color: row.original.netEdgeBuyNum >= 50
                ? colors.semantic.success.DEFAULT
                : row.original.netEdgeBuyNum >= 20
                  ? colors.semantic.warning.DEFAULT
                  : colors.text.tertiary,
            }}
          >
            {displayValue}
          </span>
        );
      },
    },
    {
      id: 'marginable',
      header: SCANNERS_COLUMN_TITLES.marginable,
      accessorKey: 'bybit_marginable',
      size: 110,
      minSize: 95,
      cell: ({ row }) => (
        <StatusPill
          status={row.original.isMarginable ? 'ok' : 'muted'}
          label={row.original.isMarginable ? 'Marginable' : 'Manual'}
          tooltip={row.original.isMarginable
            ? 'Bybit spot margin trading available'
            : 'No Bybit margin hedge'}
          size="xs"
          tone="subtle"
        />
      ),
    },
    {
      id: 'last_update',
      header: SCANNERS_COLUMN_TITLES.lastUpdate,
      accessorFn: (row) => {
        const c = Number(row.cex_last_update_ts || 0);
        const d = Number(row.dex_last_update_ts || 0);
        const l = Number(row.last_update_ts || 0);
        return Math.max(c, d, l);
      },
      size: 90,
      minSize: 80,
      cell: ({ row }) => {
        // Use stable component to prevent row remounts
        return <LastUpdateCell rowId={row.original.pool_address} />;
      },
    },
    {
      id: 'last_update_time',
      header: SCANNERS_COLUMN_TITLES.localTime,
      accessorFn: (row) => row.last_update_ts || 0,
      size: 95,
      minSize: 85,
      cell: ({ row }) => {
        const ts = row.original.last_update_ts;
        if (!ts || ts <= 0) {
          return <span className="text-xs text-text-muted">—</span>;
        }
        const localTime = formatAbsoluteTime(ts);
        const tooltip = formatLocal(ts);
        return (
          <span className="text-xs text-text-muted font-mono tabular-nums opacity-70" title={tooltip}>
            {localTime}
          </span>
        );
      },
    },
  ], [nowTs]);

  const scannerFilters = useMemo<ColumnFilter[]>(() => [
    { key: 'pairLabel', label: 'Pool', type: 'text', placeholder: 'WBNB/USDT, bnb, usdt...' },
    { key: 'dex_name', label: 'DEX', type: 'select', options: dexOptions },
    { key: 'chain', label: 'Chain', type: 'select', options: chainOptions },
    { key: 'bybit_marginable', label: 'Bybit Marginable', type: 'select', options: ['marginable', 'manual'] },
  ], [dexOptions, chainOptions]);

  const perfV2 = isScannersPerfV2Enabled();
  const renderStartRef = useRef<number>(0);

  // Perf V2: Mark render start
  if (perfV2 && typeof performance !== 'undefined' && performance.mark) {
    try {
      performance.mark('scanners.render.table.start');
      renderStartRef.current = performance.now();
    } catch {
      // Ignore mark errors
    }
  }

  // Perf V2: Measure render duration after layout
  useLayoutEffect(() => {
    if (!perfV2 || !recordRenderDuration) return;

    const renderStart = renderStartRef.current;
    if (renderStart > 0) {
      const renderEnd = performance.now();
      const duration = renderEnd - renderStart;
      if (duration > 0) {
        recordRenderDuration(duration);
      }
      if (typeof performance !== 'undefined' && performance.mark) {
        try {
          performance.mark('scanners.render.table.end');
          performance.measure('scanners.render.table', 'scanners.render.table.start');
        } catch {
          // Ignore measure errors
        }
      }
      renderStartRef.current = 0;
    }
  }, [perfV2, recordRenderDuration, visibleRows.length, loading, refreshing]);

  return (
    <div className="h-full flex flex-col overflow-hidden" style={{ backgroundColor: colors.bg.base }}>
      {showHeader && (
        <PanelHeader
          title="Scanners · Pool Pricing"
          onRefresh={refresh}
          refreshing={isRefreshing}
          lastUpdate={lastAppliedAtTs}
          staleThresholdMs={STALE_THRESHOLDS.NORMAL}
          onRemove={onRemove}
          titleActions={manualModeIndicator}
          actions={telemetryActions}
        />
      )}
      <div className="flex-1 flex flex-col overflow-hidden">
        <TableFilter
          columns={scannerFilters}
          onFilterChange={handleFilterChange}
          value={filters}
          dense={false}
          customControls={customControls}
        />
        <PanelBody ref={scrollRef}>
          <DataTable
            data={visibleRows}
            columns={columns}
            sortable
            loading={loading && !refreshing}
            emptyMessage="No pools found matching filters"
            initialSorting={[{ id: 'best_edge', desc: true }]}
            className={isMobile ? 'w-full' : ''}
            virtualizer={shouldVirtualize ? virtualizer : undefined}
            virtualScrollRef={shouldVirtualize ? scrollRef : undefined}
            mobileMode="cards"
            renderMobileRow={renderMobileRow}
          />
        </PanelBody>
      </div>
    </div>
  );
}

interface ScannersMobileCardProps {
  row: EnrichedRow;
  nowTs: number;
}

const ScannersMobileCard: React.FC<ScannersMobileCardProps> = ({ row, nowTs }) => {
  const [expanded, setExpanded] = useState(false);
  const edgeDisplay = row.bestEdgeDisplay ?? formatEdgeValue(row.bestEdgeNum);
  const tvlDisplay = row.tvlDisplay ?? formatUsdCompact(row.tvlNum);
  const volDisplay = row.vol24Display ?? formatUsdCompact(row.vol24Num);
  const ageMs = Math.max(0, nowTs - (row.last_update_ts || 0));
  const ageDisplay = row.last_update_ts ? formatManualAge(ageMs) : '—';
  const poolAddress = row.pool_address || '';

  return (
    <div className="rounded-lg border p-3 flex flex-col gap-3" style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.surface }}>
      <div className="flex items-start justify-between gap-2">
        <div className="flex flex-col">
          <span className="text-sm font-semibold" style={{ color: colors.text.primary }}>{row.pairLabel}</span>
          <span className="text-xs" style={{ color: colors.text.muted }}>
            {row.dex_name || '—'} · {(row.chain || '').toUpperCase()}
          </span>
        </div>
        <StatusPill
          status={row.isMarginable ? 'ok' : 'muted'}
          label={row.isMarginable ? 'Marginable' : 'Manual'}
          size="xs"
          tone="subtle"
        />
      </div>
      <div className="grid grid-cols-3 gap-2">
        <div className="flex flex-col">
          <span className="text-[10px] uppercase" style={{ color: colors.text.muted }}>Best Edge</span>
          <span className="font-mono text-base font-medium" style={{ color: colors.semantic.success.light }}>{edgeDisplay}</span>
        </div>
        <div className="flex flex-col">
          <span className="text-[10px] uppercase" style={{ color: colors.text.muted }}>TVL</span>
          <span className="font-mono text-sm" style={{ color: colors.text.secondary }}>{tvlDisplay}</span>
        </div>
        <div className="flex flex-col">
          <span className="text-[10px] uppercase" style={{ color: colors.text.muted }}>Vol 24h</span>
          <span className="font-mono text-sm" style={{ color: colors.text.secondary }}>{volDisplay}</span>
        </div>
      </div>
      <div className="flex items-center justify-between text-xs border-t pt-2" style={{ borderColor: colors.border.DEFAULT, color: colors.text.muted }}>
        <span>Last update: {ageDisplay}</span>
        {poolAddress ? (
          <span className="font-mono text-[10px] opacity-70">
            {poolAddress.slice(0, 6)}…{poolAddress.slice(-4)}
          </span>
        ) : (
          <span />
        )}
      </div>
      <button
        type="button"
        onClick={() => setExpanded((prev) => !prev)}
        className="flex items-center justify-between rounded border px-2 py-1.5 text-xs transition-colors hover:bg-bg-hover hover:text-text-primary"
        style={{ borderColor: colors.border.DEFAULT, color: colors.text.secondary }}
      >
        <span>{expanded ? 'Hide legs' : 'View legs'}</span>
        <ChevronDown className={`h-3 w-3 transition-transform ${expanded ? 'rotate-180' : ''}`} />
      </button>
      {expanded && (
        <div className="rounded border p-2 flex flex-col gap-3" style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.base }}>
          <div>
            <span className="text-[10px] uppercase mb-1 block" style={{ color: colors.text.muted }}>Leg A</span>
            <LegCell leg={row.legA} row={row} />
          </div>
          <div>
            <span className="text-[10px] uppercase mb-1 block" style={{ color: colors.text.muted }}>Leg B</span>
            <LegCell leg={row.legB} row={row} />
          </div>
        </div>
      )}
    </div>
  );
};
