import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { ColumnDef, SortingState } from '@tanstack/react-table';
import { ChevronDown, RefreshCw } from 'lucide-react';
import { PageShell } from './components/layout/PageShell';
import { DataTable } from './components/ui/table';
import { Pager } from './components/shared/Pager';
import { LoadingState } from './components/shared/LoadingState';
import { EmptyState } from './components/shared/EmptyState';
import { DataAgeCell } from './components/shared/DataAgeCell';
import { Checkbox, IconButton, Popover, PopoverContentWrapper, Switch } from './components/ui';
import { colors, typography } from './lib/tokens';
import { api } from './api';
import { usePolling } from './hooks';
import type { MarketSnapshot } from './types';
import { useMarketDataStore, MARKET_DATA_PAGE_SIZE } from './stores/marketDataStore';

type MarketRow = MarketSnapshot & {
  timestamp?: number | string | null;
};

const getTimestampMs = (row: MarketRow): number | null => {
  const raw = row.timestamp_ms ?? row.timestamp;
  if (raw === undefined || raw === null) return null;
  const num = typeof raw === 'string' ? Number(raw) : raw;
  return Number.isFinite(num) ? Number(num) : null;
};

const updateHash = (
  hash: number,
  value: string | number | null | undefined
): number => {
  const text = value == null ? '' : String(value);
  for (let i = 0; i < text.length; i += 1) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return hash;
};

const deriveRowsSignature = (rows: MarketSnapshot[]): string => {
  let hash = 2166136261;
  let maxTimestamp = 0;
  for (const row of rows) {
    const ts = getTimestampMs(row) ?? 0;
    if (ts > maxTimestamp) {
      maxTimestamp = ts;
    }
    hash = updateHash(hash, row.coin);
    hash = updateHash(hash, row.exchange);
    hash = updateHash(hash, ts);
    hash = updateHash(hash, row.bid);
    hash = updateHash(hash, row.mid_px);
    hash = updateHash(hash, row.ask);
    hash = updateHash(hash, row.bid_qty);
    hash = updateHash(hash, row.ask_qty);
  }
  return `rows:${rows.length}:maxTs:${maxTimestamp}:h:${hash >>> 0}`;
};

const deriveFreshnessMarker = (snapshot: {
  freshnessKey?: string;
  etag?: string;
  lastUpdateMs?: number;
  count?: number;
}): string | null => {
  if (snapshot.freshnessKey) {
    return `key:${snapshot.freshnessKey}`;
  }
  if (snapshot.etag) {
    return `etag:${snapshot.etag}`;
  }
  if (typeof snapshot.lastUpdateMs === 'number') {
    return `ts:${snapshot.lastUpdateMs}:count:${snapshot.count}`;
  }
  return null;
};

export default function MarketData() {
  const rows = useMarketDataStore((s) => s.rows);
  const loading = useMarketDataStore((s) => s.loading);
  const lastUpdate = useMarketDataStore((s) => s.lastUpdate);
  const setSnapshot = useMarketDataStore((s) => s.setSnapshot);
  const setLoading = useMarketDataStore((s) => s.setLoading);
  const setLastUpdate = useMarketDataStore((s) => s.setLastUpdate);

  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(MARKET_DATA_PAGE_SIZE);
  const [symbolFilter, setSymbolFilter] = useState('');
  const [selectedExchanges, setSelectedExchanges] = useState<string[]>([]);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const prevFreshnessRef = useRef<string>('');
  const prevRowsSignatureRef = useRef<string>('');
  const hasLoadedRef = useRef(false);
  const [refreshing, setRefreshing] = useState(false);
  const initialSorting = useMemo<SortingState>(() => [{ id: 'timestamp', desc: true }], []);

  const fetchSnapshot = useCallback(async (opts?: { showLoading?: boolean }) => {
    const shouldShowLoading = opts?.showLoading ?? !hasLoadedRef.current;
    if (shouldShowLoading) {
      setLoading(true);
    }
    try {
      if (opts?.showLoading) {
        setRefreshing(true);
      }
      const snapshot = await api.getMarketDataSnapshot();
      const freshnessMarker = deriveFreshnessMarker(snapshot);

      if (!freshnessMarker) {
        prevFreshnessRef.current = '';
        const nextRowsSignature = deriveRowsSignature(snapshot.rows);
        if (nextRowsSignature !== prevRowsSignatureRef.current) {
          setSnapshot(snapshot.rows, Date.now());
          prevRowsSignatureRef.current = nextRowsSignature;
        } else {
          setLastUpdate(Date.now());
        }
        return;
      }

      if (freshnessMarker !== prevFreshnessRef.current) {
        setSnapshot(snapshot.rows, Date.now());
        prevFreshnessRef.current = freshnessMarker;
        prevRowsSignatureRef.current = '';
      } else {
        const nextRowsSignature = deriveRowsSignature(snapshot.rows);
        let prevRowsSignature = prevRowsSignatureRef.current;
        if (!prevRowsSignature) {
          prevRowsSignature = deriveRowsSignature(useMarketDataStore.getState().rows);
        }

        if (nextRowsSignature !== prevRowsSignature) {
          setSnapshot(snapshot.rows, Date.now());
          prevRowsSignatureRef.current = nextRowsSignature;
        } else {
          prevRowsSignatureRef.current = prevRowsSignature;
          setLastUpdate(Date.now());
        }
      }
    } catch (e) {
      if (import.meta.env?.DEV) {
        // eslint-disable-next-line no-console
        console.warn('[market-data] polling failed', e);
      }
    } finally {
      if (opts?.showLoading) {
        setRefreshing(false);
      }
      if (shouldShowLoading) {
        hasLoadedRef.current = true;
        setLoading(false);
      }
    }
  }, [setLoading, setSnapshot, setLastUpdate]);

  usePolling(fetchSnapshot, 5000, autoRefresh, {
    hiddenIntervalMs: 15000,
    refreshOnVisible: true,
  });

  useEffect(() => {
    setPage(1);
  }, [symbolFilter, selectedExchanges]);

  const exchangeOptions = useMemo(() => {
    const set = new Set<string>();
    rows.forEach((row) => {
      if (row.exchange) {
        set.add(row.exchange);
      }
    });
    return Array.from(set).sort((a, b) => a.localeCompare(b));
  }, [rows]);

  const filteredRows = useMemo(() => {
    const needle = symbolFilter.trim().toLowerCase();
    const activeExchanges =
      selectedExchanges.length === 0 ? exchangeOptions : selectedExchanges;
    const exchangeSet = new Set(activeExchanges.map((ex) => ex.toLowerCase()));

    return rows.filter((row) => {
      const symbol = (row.coin || '').toLowerCase();
      const matchSymbol = needle ? symbol.includes(needle) : true;
      const matchExchange =
        exchangeSet.size === 0
          ? true
          : exchangeSet.has((row.exchange || '').toLowerCase());
      return matchSymbol && matchExchange;
    });
  }, [rows, symbolFilter, selectedExchanges, exchangeOptions]);

  const sortedRows = useMemo(() => {
    const copy = [...filteredRows] as MarketRow[];
    copy.sort((a, b) => (getTimestampMs(b) ?? 0) - (getTimestampMs(a) ?? 0));
    return copy;
  }, [filteredRows]);

  const totalPages = useMemo(
    () => Math.max(1, Math.ceil(sortedRows.length / pageSize)),
    [sortedRows.length, pageSize]
  );

  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    }
  }, [page, totalPages]);

  const pagedRows = useMemo(() => {
    const start = (page - 1) * pageSize;
    return sortedRows.slice(start, start + pageSize);
  }, [sortedRows, page, pageSize]);

  const handlePageChange = (next: number) => setPage(Math.max(1, next));
  const handlePageSizeChange = (nextSize: number) => {
    setPageSize(nextSize);
    setPage(1);
  };

  const handleToggleExchange = useCallback(
    (exchange: string) => {
      setSelectedExchanges((prev) => {
        // When no explicit selection, treat click as selecting only this exchange
        if (prev.length === 0) {
          return [exchange];
        }
        if (prev.includes(exchange)) {
          return prev.filter((ex) => ex !== exchange);
        }
        return [...prev, exchange];
      });
    },
    []
  );

  const exchangeFilterLabel = useMemo(() => {
    if (exchangeOptions.length === 0) {
      return 'All exchanges';
    }
    if (selectedExchanges.length === 0 || selectedExchanges.length === exchangeOptions.length) {
      return 'All exchanges';
    }
    if (selectedExchanges.length === 1) {
      return selectedExchanges[0];
    }
    return `${selectedExchanges.length} exchanges`;
  }, [exchangeOptions, selectedExchanges]);

  const columns = useMemo<ColumnDef<MarketRow>[]>(() => [
    {
      header: 'Symbol',
      accessorKey: 'coin',
      cell: ({ row }) => (
        <span style={{ fontWeight: typography.fontWeight.semibold, color: colors.text.primary }}>
          {row.original.coin || '-'}
        </span>
      ),
    },
    {
      header: 'Exchange',
      accessorKey: 'exchange',
      cell: ({ row }) => (
        <span style={{ color: colors.text.secondary }}>
          {row.original.exchange || '-'}
        </span>
      ),
    },
    {
      header: 'Bid',
      accessorKey: 'bid',
      cell: ({ row }) => (
        <span style={{ fontVariantNumeric: 'tabular-nums', color: colors.text.primary }}>
          {row.original.bid || ''}
        </span>
      ),
    },
    {
      header: 'Mid',
      accessorKey: 'mid_px',
      cell: ({ row }) => (
        <span style={{ fontVariantNumeric: 'tabular-nums', color: colors.text.primary }}>
          {row.original.mid_px || ''}
        </span>
      ),
    },
    {
      header: 'Ask',
      accessorKey: 'ask',
      cell: ({ row }) => (
        <span style={{ fontVariantNumeric: 'tabular-nums', color: colors.text.primary }}>
          {row.original.ask || ''}
        </span>
      ),
    },
    {
      id: 'timestamp',
      header: 'Last Update',
      accessorFn: (row) => getTimestampMs(row) ?? 0,
      cell: ({ row }) => (
        <DataAgeCell timestamp={getTimestampMs(row.original)} />
      ),
    },
  ], []);

  return (
    <PageShell>
      <div className="flex flex-col h-full">
        <div
          className="flex items-center justify-between px-4 py-3 border-b"
          style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.surface }}
        >
          <div className="flex items-center gap-3">
            <h2
              style={{
                fontSize: typography.fontSize.lg,
                fontWeight: typography.fontWeight.semibold,
                color: colors.text.primary,
              }}
            >
              Market Data
            </h2>
            <span style={{ color: colors.text.muted, fontSize: typography.fontSize.sm }}>
              {filteredRows.length} rows
            </span>
          </div>
          <div className="flex items-center gap-3 flex-wrap justify-end">
            <div className="flex items-center gap-2">
              <label className="sr-only" htmlFor="md-symbol-filter">Symbol filter</label>
              <input
                id="md-symbol-filter"
                value={symbolFilter}
                onChange={(e) => setSymbolFilter(e.target.value)}
                placeholder="Filter symbol"
                style={{
                  padding: '6px 8px',
                  border: `1px solid ${colors.border.DEFAULT}`,
                  borderRadius: '6px',
                  backgroundColor: colors.bg.surface,
                  color: colors.text.primary,
                  fontSize: typography.fontSize.sm,
                  minWidth: '150px',
                }}
              />
              <Popover
                trigger={(
                  <button
                    type="button"
                    aria-label="Exchange filter"
                    className="inline-flex items-center justify-between gap-2"
                    style={{
                      padding: '6px 8px',
                      border: `1px solid ${colors.border.DEFAULT}`,
                      borderRadius: '6px',
                      backgroundColor: colors.bg.surface,
                      color: colors.text.primary,
                      fontSize: typography.fontSize.sm,
                      minWidth: '160px',
                    }}
                  >
                    <span className="truncate">
                      {exchangeFilterLabel}
                    </span>
                    <ChevronDown className="w-3 h-3 opacity-60" />
                  </button>
                )}
                side="bottom"
                align="start"
                widthMode="trigger"
              >
                <PopoverContentWrapper padding="sm">
                  <div className="flex flex-col gap-1">
                    {exchangeOptions.map((ex) => (
                      <Checkbox
                        key={ex}
                        checked={
                          selectedExchanges.length === 0 ||
                          selectedExchanges.includes(ex)
                        }
                        onChange={() => handleToggleExchange(ex)}
                        label={ex}
                        dense
                      />
                    ))}
                  </div>
                </PopoverContentWrapper>
              </Popover>
            </div>
            <div className="flex items-center gap-2">
              <span style={{ color: colors.text.secondary, fontSize: typography.fontSize.sm }}>Last update:</span>
              <DataAgeCell timestamp={lastUpdate} />
              <div className="flex items-center gap-1">
                <Switch checked={autoRefresh} onCheckedChange={setAutoRefresh} size="sm" />
                <span style={{ color: colors.text.secondary, fontSize: typography.fontSize.sm }}>
                  Auto (5s)
                </span>
              </div>
              <IconButton
                variant="secondary"
                size="sm"
                onClick={() => fetchSnapshot({ showLoading: true })}
                aria-label="Refresh now"
                title="Refresh now"
                disabled={refreshing}
              >
                <RefreshCw className={`w-4 h-4 ${refreshing ? 'animate-spin' : ''}`} />
              </IconButton>
            </div>
          </div>
        </div>

        <div className="flex-1 min-h-0 overflow-hidden">
          {loading && rows.length === 0 ? (
            <div className="h-full flex items-center justify-center">
              <LoadingState message="Loading market data..." />
            </div>
          ) : rows.length === 0 ? (
            <div className="h-full flex items-center justify-center">
              <EmptyState title="No market data yet" description="Waiting for market updates..." />
            </div>
          ) : (
            <div className="flex flex-col h-full">
              <div className="flex-1 overflow-auto">
                <DataTable
                  data={pagedRows}
                  columns={columns}
                  sortable
                  initialSorting={initialSorting}
                  dense
                  emptyMessage="No market data"
                />
              </div>
              <div className="shrink-0">
                <Pager
                  page={page}
                  pageSize={pageSize}
                  total={sortedRows.length}
                  onPageChange={handlePageChange}
                  onPageSizeChange={handlePageSizeChange}
                  borderPosition="top"
                  itemLabel="rows"
                />
              </div>
            </div>
          )}
        </div>
      </div>
    </PageShell>
  );
}
