// Trades blotter with server-side pagination, filtering, and live updates

import { useEffect, useState, useCallback, useRef, useMemo } from 'react';
import { api } from './api';
import { socket } from './sockets';
import {
  useTradesStore,
  selectTradesRows,
  selectTradesLastSeq,
  useResyncStore,
  selectResyncId,
  markGlobalResyncApplied,
  shallow,
} from './stores';
import { useResyncStatus } from './hooks/useResyncStatus';
import { TableFilter, type FilterValues, type ColumnFilter } from './components/shared/TableFilter';
import { PanelHeader } from './components/shared/PanelHeader';
import type { TradeRow, TradeEvent } from './types';
import { playTradeClick } from './utils/sound';
import { getSoundMuted, setSoundMuted } from './utils/storage';
import { TradesTable, type TradesTableScrollState } from './components/trades/TradesTable';
import { TradesPerfHarness } from './components/trades/PerfHarness';
import { SOUND } from './constants';
import { Button } from './components/ui/button/Button';
import { colors, spacing, typography, STALE_THRESHOLDS, borderRadius } from './lib/tokens';
import { usePanelHeaderSlots } from './components/layout/PanelWrapper';
import { exportCSV, generateTimestampFilename } from './utils/export';
import { isTradesDecisionDetailsEnabled } from './config/featureFlags';

const PERF_RENDER_ENABLED = typeof import.meta !== 'undefined'
  && Boolean(import.meta.env?.DEV)
  && typeof performance !== 'undefined';

const DEV_TRADES_PERF_HARNESS = typeof import.meta !== 'undefined'
  && Boolean(import.meta.env?.DEV)
  && Boolean(import.meta.env?.VITE_TRADES_PERF);

const TRADE_FILTERS: ColumnFilter[] = [
  { key: 'coin', label: 'Coin', type: 'text', placeholder: 'BTC, ETH...' },
  { key: 'exchange', label: 'Exchange', type: 'text', placeholder: 'bybit, rooster...' },
  { key: 'side', label: 'Side', type: 'select', options: ['buy', 'sell'] },
  { key: 'signal_id', label: 'Signal', type: 'text', placeholder: 'Strategy ID...' },
];

const FILTER_STORAGE_KEY = 'trades_filters';
const PAGE_SIZE_STORAGE_KEY = 'trades_page_size';

const DEBOUNCE_MS = 300;
const POLL_BASE_MS = 1000; // Base when WS connected
const POLL_BASE_MS_DISCONNECTED = 500; // Faster reconciliation when WS disconnected
const POLL_MAX_MS = 3000; // Cap backoff at 3s to reduce UI staleness
// Reduce default delta payload to improve initial load times over WAN.
// The UI requests more when scrolling back via cursor.
const DELTA_LIMIT = 500;
const MAX_EMPTY_POLLS = 3; // Log warning if this many consecutive polls return 0 trades
const RECONNECT_CATCHUP_MIN_MS = 3000;

const coerceFiniteNumber = (value: unknown): number | undefined => {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (trimmed.length === 0) {
      return undefined;
    }
    const parsed = Number(trimmed);
    return Number.isFinite(parsed) ? parsed : undefined;
  }
  return undefined;
};

type TradeTimestampParts = {
  seq?: number;
  tsMs?: number;
  ts?: number;
  hasReliableTimestamp: boolean;
};

const getTimestampParts = (payload: any): TradeTimestampParts => {
  const seq = coerceFiniteNumber(payload?.seq);
  const tsMs = coerceFiniteNumber(payload?.ts_ms);
  const ts = coerceFiniteNumber(payload?.ts);
  const hasReliableTimestamp = seq !== undefined || tsMs !== undefined || ts !== undefined;
  return { seq, tsMs, ts, hasReliableTimestamp };
};

export const hasReliableTradeTimestamp = (payload: any): boolean =>
  getTimestampParts(payload).hasReliableTimestamp;

const PAGE_SIZE_OPTIONS = [50, 100, 200, 500];
const DEFAULT_PAGE_SIZE = 100;

const normalizePageSize = (value: unknown): number => {
  const parsed = parseInt(String(value ?? DEFAULT_PAGE_SIZE), 10);
  return PAGE_SIZE_OPTIONS.includes(parsed) ? parsed : DEFAULT_PAGE_SIZE;
};

const hasActiveFilters = (filters: FilterValues): boolean => {
  return Boolean(
    (filters.coin ?? '').trim()
    || (filters.exchange ?? '').trim()
    || (filters.side ?? '').trim()
    || (filters.signal_id ?? '').trim(),
  );
};

const rowMatchesFilters = (row: any, filters: FilterValues): boolean => {
  if (!filters) return true;
  const coinFilter = (filters.coin ?? '').trim();
  if (coinFilter) {
    const target = coinFilter.toUpperCase();
    const coinValue = String(row?.coin ?? row?.symbol ?? '').toUpperCase();
    const base = coinValue.split('/')[0];
    if (coinValue !== target && base !== target) {
      return false;
    }
  }

  const exchangeFilter = (filters.exchange ?? '').trim().toLowerCase();
  if (exchangeFilter) {
    const exchangeValue = String(row?.exchange ?? '').toLowerCase();
    if (exchangeValue !== exchangeFilter) {
      return false;
    }
  }

  const sideFilter = (filters.side ?? '').trim().toLowerCase();
  if (sideFilter) {
    const sideValue = String(row?.side ?? '').toLowerCase();
    if (sideValue !== sideFilter) {
      return false;
    }
  }

  const signalFilter = (filters.signal_id ?? '').trim().toLowerCase();
  if (signalFilter) {
    const sigValue = String(row?.signal_id ?? '').toLowerCase();
    if (!sigValue.includes(signalFilter)) {
      return false;
    }
  }

  return true;
};

const filterEventsForFilters = (events: TradeEvent[] | undefined | null, filters: FilterValues): TradeEvent[] => {
  if (!events?.length) {
    return [];
  }
  if (!hasActiveFilters(filters)) {
    return events;
  }
  return events.filter((event) => rowMatchesFilters(event, filters));
};

const loadStoredFilters = (): FilterValues => {
  if (typeof window === 'undefined' || !window?.sessionStorage) {
    return {};
  }
  try {
    const raw = window.sessionStorage.getItem(FILTER_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') {
      return {};
    }
    const allowed = new Set(TRADE_FILTERS.map((f) => f.key));
    const sanitized: FilterValues = {};
    Object.entries(parsed as Record<string, unknown>).forEach(([key, value]) => {
      if (allowed.has(key) && typeof value === 'string') {
        sanitized[key] = value;
      }
    });
    return sanitized;
  } catch {
    return {};
  }
};

function PageSizeControl({ value, onChange }: { value: number; onChange: (val: number) => void }) {
  return (
    <label
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: spacing.gap.xs,
        color: colors.text.secondary,
        fontSize: typography.fontSize.sm,
      }}
    >
      Page size
      <select
        value={value}
        onChange={(e) => onChange(parseInt(e.target.value, 10))}
        style={{
          backgroundColor: colors.bg.base,
          color: colors.text.primary,
          border: `1px solid ${colors.border.DEFAULT}`,
          borderRadius: borderRadius.md,
          padding: `${spacing.padding.xs} ${spacing.gap.sm}`,
          fontSize: typography.fontSize.sm,
        }}
      >
        {PAGE_SIZE_OPTIONS.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    </label>
  );
}

type FetchOptions = {
  silent?: boolean;
  keepUnread?: boolean;
  cursor?: string | null;
  append?: boolean;
  resyncId?: number;
};

export default function Trades({
  dense = false,
  className = '',
  onRemove,
  showHeader = true,
}: {
  dense?: boolean;
  className?: string;
  tableClassName?: string;
  onRemove?: () => void;
  showHeader?: boolean;
} = {}) {
  if (PERF_RENDER_ENABLED) {
    try {
      performance.mark('Trades.render:start');
    } catch {
      // Ignore unsupported environments
    }
  }

  const storeRows = useTradesStore(selectTradesRows, shallow);
  const lastSeq = useTradesStore(selectTradesLastSeq);
  const setSnapshot = useTradesStore((state) => state.setSnapshot);
  const applyDelta = useTradesStore((state) => state.applyDelta);
  const resyncId = useResyncStore(selectResyncId);
  const { isResyncing } = useResyncStatus();
  const decisionDetailsEnabled = useMemo(() => isTradesDecisionDetailsEnabled(), []);

  const [pageSize, setPageSize] = useState(() => {
    if (typeof window === 'undefined' || !window?.sessionStorage) {
      return DEFAULT_PAGE_SIZE;
    }
    const stored = window.sessionStorage.getItem(PAGE_SIZE_STORAGE_KEY);
    return normalizePageSize(stored);
  });
  const [page, setPage] = useState<number>(1);
  const [total, setTotal] = useState(0);
  const [hasMore, setHasMore] = useState<boolean | null>(null);
  const [hasMorePage, setHasMorePage] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [lastUpdate, setLastUpdate] = useState<number>(Date.now());
  const [filters, setFilters] = useState<FilterValues>(() => loadStoredFilters());
  const [sort, setSort] = useState<'ts_desc' | 'ts_asc'>('ts_desc');
  const [soundMuted, setSoundMutedState] = useState(() => getSoundMuted());
  const [unread, setUnread] = useState(0);
  const [pollDelay, setPollDelay] = useState(POLL_BASE_MS);
  const [socketConnected, setSocketConnected] = useState(true);
  const [isViewingLatest, setIsViewingLatest] = useState(true);
  const [perfHarnessActive, setPerfHarnessActive] = useState(false);

  const abortRef = useRef<AbortController | null>(null);
  const mountedRef = useRef<boolean>(true);
  const debounceRef = useRef<number | null>(null);
  const pollTimeoutRef = useRef<number | null>(null);
  const latestSeqRef = useRef<number>(0);
  const mutedRef = useRef<boolean>(soundMuted);
  const pollDelayRef = useRef<number>(POLL_BASE_MS);
  const isViewingLatestRef = useRef<boolean>(true);
  const isAtTopRef = useRef<boolean>(true);
  const isUserScrollingRef = useRef<boolean>(false);
  const scrollElementRef = useRef<HTMLDivElement | null>(null);

  if (DEV_TRADES_PERF_HARNESS && perfHarnessActive) {
    return <TradesPerfHarness onClose={() => setPerfHarnessActive(false)} />;
  }
  const emptyPollCountRef = useRef<number>(0); // Track consecutive empty delta polls
  const socketConnectedRef = useRef<boolean>(true); // Track Socket.IO connection state
  const lastSoundSeqRef = useRef<number>(0); // Deduplicate sound playback by sequence
  const refreshTimeoutRef = useRef<number | null>(null); // Throttle snapshot refreshes
  const lastResetAtRef = useRef<number>(0); // Guard against reset thrash
  const pageSizeRef = useRef<number>(pageSize);
  const pageRef = useRef<number>(page);
  const requestSeqRef = useRef<number>(0);
  const activeForegroundRequestRef = useRef<number | null>(null);
  const filtersRef = useRef<FilterValues>(filters);
  const sortRef = useRef<'ts_desc' | 'ts_asc'>(sort);
  const lastSoundAtRef = useRef<number>(0);
  const applyDeltaRef = useRef(applyDelta);
  const queueSnapshotRefreshRef = useRef<typeof queueSnapshotRefresh | null>(null);
  const playSoundForSeqRef = useRef<typeof playSoundForSeq | null>(null);
  const isActiveRef = useRef<boolean>(true);
  const catchingUpRef = useRef<boolean>(false);
  const resyncIdRef = useRef<number>(resyncId);
  const reconnectCatchupInFlightRef = useRef<boolean>(false);
  const lastReconnectCatchupAtRef = useRef<number>(0);

  useEffect(() => {
    if (!PERF_RENDER_ENABLED) {
      return;
    }
    try {
      performance.measure('Trades.render', 'Trades.render:start');
      performance.clearMarks('Trades.render:start');
    } catch {
      // Ignore unsupported environments
    }
  });

  useEffect(() => {
    mutedRef.current = soundMuted;
  }, [soundMuted]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      abortRef.current?.abort();
      abortRef.current = null;

      if (typeof window !== 'undefined') {
        if (refreshTimeoutRef.current !== null) {
          window.clearTimeout(refreshTimeoutRef.current);
          refreshTimeoutRef.current = null;
        }
        if (pollTimeoutRef.current !== null) {
          window.clearTimeout(pollTimeoutRef.current);
          pollTimeoutRef.current = null;
        }
        if (debounceRef.current !== null) {
          window.clearTimeout(debounceRef.current);
          debounceRef.current = null;
        }
      }
    };
  }, []);

  useEffect(() => {
    latestSeqRef.current = lastSeq;
  }, [lastSeq]);

  useEffect(() => {
    pollDelayRef.current = pollDelay;
  }, [pollDelay]);

  useEffect(() => {
    filtersRef.current = filters;
  }, [filters]);

  useEffect(() => {
    sortRef.current = sort;
  }, [sort]);

  useEffect(() => {
    applyDeltaRef.current = applyDelta;
  }, [applyDelta]);

  useEffect(() => {
    resyncIdRef.current = resyncId;
  }, [resyncId]);

  const rowsToRender = useMemo(() => {
    if (!Array.isArray(storeRows)) {
      return [];
    }
    if (sort === 'ts_desc') {
      return storeRows;
    }
    return [...storeRows].reverse();
  }, [storeRows, sort]);

  const recomputeIsViewingLatest = useCallback(
    (atTopOverride?: boolean) => {
      const atTop = typeof atTopOverride === 'boolean' ? atTopOverride : isAtTopRef.current;
      const latest = atTop && pageRef.current === 1 && sortRef.current === 'ts_desc';
      isViewingLatestRef.current = latest;
      setIsViewingLatest(latest);
      if (latest) {
        setUnread(0);
      }
      return latest;
    },
    [setIsViewingLatest, setUnread],
  );

  const fetchPage = useCallback(
    async (options: FetchOptions = {}) => {
      if (refreshTimeoutRef.current !== null) {
        if (typeof window !== 'undefined') {
          window.clearTimeout(refreshTimeoutRef.current);
        }
        refreshTimeoutRef.current = null;
      }

      abortRef.current?.abort();
      const ac = new AbortController();
      abortRef.current = ac;
      const requestSeq = requestSeqRef.current + 1;
      requestSeqRef.current = requestSeq;
      const isForegroundRequest = !options.silent;

      if (isForegroundRequest) {
        activeForegroundRequestRef.current = requestSeq;
        setLoading(true);
      }

      const params: Record<string, string | number | undefined> = {
        ...filtersRef.current,
        sort: sortRef.current,
      };
      const requestResyncId = options.resyncId ?? resyncIdRef.current;
      const requestPage = pageRef.current;
      // Offset-based pagination: pageRef defines which slice to fetch

      try {
        const response = await api.getTrades(requestPage, pageSizeRef.current, params, { signal: ac.signal });
        if (abortRef.current !== ac) {
          return;
        }
        if (!mountedRef.current) {
          return;
        }

        // Snapshot for the current page slice
        const snapshotResult = setSnapshot(response.rows || [], pageSizeRef.current, requestResyncId);
        if (snapshotResult?.applied) {
          markGlobalResyncApplied('trades', requestResyncId);
        }
        // Update latest-viewing flag based on page and scroll state
        const nowViewingLatest = recomputeIsViewingLatest();

        const totalCount = response.total ?? response.total_records ?? 0;
        setTotal(totalCount);
        setHasMore(typeof response.has_more === 'boolean' ? response.has_more : null);
        setHasMorePage(requestPage);

        if (!options.keepUnread) {
          setUnread(0);
        }

        if (typeof response.last_seq === 'number') {
          latestSeqRef.current = Math.max(latestSeqRef.current, response.last_seq);
        }

        setLastUpdate(Date.now());
      } catch (e) {
        if ((e as any).name !== 'AbortError' && abortRef.current === ac) {
          console.error('[trades] load failed:', e);
          if (!options.append) {
            if (mountedRef.current) {
              setSnapshot([], pageSizeRef.current);
              if (!options.keepUnread) {
                setUnread(0);
              }
              setTotal(0);
              setHasMore(null);
              setHasMorePage(null);
            }
          }
        }
      } finally {
        if (abortRef.current === ac) {
          abortRef.current = null;
        }
        if (
          mountedRef.current
          && isForegroundRequest
          && activeForegroundRequestRef.current === requestSeq
        ) {
          activeForegroundRequestRef.current = null;
          setLoading(false);
        }
      }
    },
    [setSnapshot, setTotal, setUnread, setLoading, recomputeIsViewingLatest],
  );

  const handleTimeSortChange = useCallback((direction: 'ts_desc' | 'ts_asc') => {
    if (sortRef.current === direction) {
      return;
    }
    setSort(direction);
    sortRef.current = direction;
    pageRef.current = 1;
    setPage(1);
    setHasMore(null);
    setHasMorePage(null);
    isAtTopRef.current = true;
    recomputeIsViewingLatest(true);
    fetchPage();
  }, [fetchPage, recomputeIsViewingLatest]);

  const handleScrollStateChange = useCallback(
    ({ atTop, isScrolling, scrollElement }: TradesTableScrollState) => {
      isAtTopRef.current = atTop;
      isUserScrollingRef.current = isScrolling;
      if (scrollElement) {
        scrollElementRef.current = scrollElement;
      }
      recomputeIsViewingLatest(atTop);
    },
    [recomputeIsViewingLatest],
  );

  useEffect(() => {
    pageSizeRef.current = pageSize;
    pageRef.current = page;
    setHasMore(null);
    setHasMorePage(null);
    if (typeof window !== 'undefined' && window?.sessionStorage) {
      try {
        window.sessionStorage.setItem(PAGE_SIZE_STORAGE_KEY, String(pageSize));
      } catch {}
    }
    fetchPage();
  }, [pageSize, page, fetchPage]);

  const queueSnapshotRefresh = useCallback(
    (keepUnread?: boolean) => {
      if (refreshTimeoutRef.current !== null) {
        return;
      }

      const keepUnreadFlag = keepUnread ?? !isViewingLatestRef.current;

      if (!keepUnreadFlag && isViewingLatestRef.current) {
        return;
      }

      refreshTimeoutRef.current = window.setTimeout(() => {
        refreshTimeoutRef.current = null;
        fetchPage({ silent: true, keepUnread: keepUnreadFlag });
      }, 250);
    },
    [fetchPage],
  );

  const playSoundForSeq = useCallback(
    (seq?: number) => {
      if (typeof seq !== 'number') {
        return;
      }
      if (!isActiveRef.current) {
        return;
      }
      if (seq <= lastSoundSeqRef.current) {
        lastSoundSeqRef.current = seq;
        return;
      }
      const now = Date.now();
      if (now - lastSoundAtRef.current < SOUND.TRADE_CLICK_THROTTLE_MS) {
        lastSoundSeqRef.current = seq;
        return;
      }

      if (!mutedRef.current) {
        playTradeClick();
      }
      lastSoundAtRef.current = now;
      lastSoundSeqRef.current = seq;
    },
    [],
  );

  // Keep callback refs in sync with latest values for socket handler
  // These assignments happen on every render to ensure the socket handler
  // (which has a stable reference) always calls the latest callback versions
  queueSnapshotRefreshRef.current = queueSnapshotRefresh;
  playSoundForSeqRef.current = playSoundForSeq;

  const handleFilterChange = useCallback(
    (newFilters: FilterValues) => {
      setFilters(newFilters);
      if (debounceRef.current) window.clearTimeout(debounceRef.current);
      debounceRef.current = window.setTimeout(() => {
        filtersRef.current = newFilters;
        // Reset to first page on filter changes
        pageRef.current = 1;
        setPage(1);
        setHasMore(null);
        setHasMorePage(null);
        isAtTopRef.current = true;
        recomputeIsViewingLatest(true);
        fetchPage();
      }, DEBOUNCE_MS);
    },
    [fetchPage, recomputeIsViewingLatest],
  );

  // Pagination controls
  const totalPages = useMemo(() => Math.max(1, Math.ceil((total || 0) / Math.max(pageSize, 1))), [total, pageSize]);
  const canPrev = useMemo(() => page > 1, [page]);
  const hasFreshHasMore = useMemo(
    () => hasMorePage === page && typeof hasMore === 'boolean',
    [hasMorePage, page, hasMore],
  );
  const canNext = useMemo(() => {
    if (loading) {
      return false;
    }
    if (hasFreshHasMore) {
      return hasMore;
    }
    return page < totalPages;
  }, [loading, hasFreshHasMore, hasMore, page, totalPages]);
  const showUnboundedPageIndicator = useMemo(
    () => hasFreshHasMore && hasMore === true && page >= totalPages,
    [hasFreshHasMore, hasMore, page, totalPages],
  );
  const goPrev = useCallback(() => {
    if (!canPrev) return;
    const nextPage = Math.max(1, page - 1);
    setLoading(true);
    setHasMore(null);
    setHasMorePage(null);
    setPage(nextPage);
    pageRef.current = nextPage;
    recomputeIsViewingLatest();
  }, [page, canPrev, recomputeIsViewingLatest]);
  const goNext = useCallback(() => {
    if (!canNext) return;
    const nextPage = hasFreshHasMore ? page + 1 : Math.min(totalPages, page + 1);
    setLoading(true);
    setHasMore(null);
    setHasMorePage(null);
    setPage(nextPage);
    pageRef.current = nextPage;
    recomputeIsViewingLatest();
  }, [page, canNext, totalPages, hasFreshHasMore, recomputeIsViewingLatest]);

  useEffect(() => {
    if (typeof window === 'undefined' || !window?.sessionStorage) return;
    try {
      window.sessionStorage.setItem(FILTER_STORAGE_KEY, JSON.stringify(filters));
    } catch {
      // Ignore storage errors
    }
  }, [filters]);

  useEffect(
    () => () => {
      if (abortRef.current) abortRef.current.abort();
    },
    [],
  );

  useEffect(
    () => () => {
      if (debounceRef.current) window.clearTimeout(debounceRef.current);
    },
    [],
  );

  useEffect(
    () => () => {
      if (refreshTimeoutRef.current) {
        window.clearTimeout(refreshTimeoutRef.current);
      }
    },
    [],
  );

  const schedulePoll = useCallback(() => {
    if (!isActiveRef.current) {
      return;
    }
    if (pollTimeoutRef.current) {
      window.clearTimeout(pollTimeoutRef.current);
    }
    // Choose dynamic base depending on socket connectivity
    const dynamicBase = socketConnectedRef.current ? POLL_BASE_MS : POLL_BASE_MS_DISCONNECTED;
    const delay = Math.max(pollDelayRef.current || dynamicBase, dynamicBase);
    pollTimeoutRef.current = window.setTimeout(async () => {
      if (!isActiveRef.current) {
        return;
      }
      try {
        const pollResyncId = resyncIdRef.current;
        const requestedSinceSeq = latestSeqRef.current;
        const delta = await api.getTradesDelta(requestedSinceSeq, DELTA_LIMIT);

        if (!isActiveRef.current) {
          return;
        }

        // DEFENSIVE FIX: Validate sequence consistency
        const hasNumericLastSeq = typeof delta.last_seq === 'number';
        const seqIsNonRegressive =
          hasNumericLastSeq && delta.last_seq >= requestedSinceSeq;
        if (hasNumericLastSeq && !seqIsNonRegressive) {
          console.warn(
            `[trades] Delta seq regression detected! Backend last_seq (${delta.last_seq}) < ` +
            `frontend latestSeq (${requestedSinceSeq}). This suggests missed trades. ` +
            `Socket.IO connected: ${socketConnectedRef.current}`
          );
        }

        if (delta.reset_required) {
          const now = Date.now();
          const sinceLast = now - lastResetAtRef.current;
          if (sinceLast >= 10_000) {
            console.log('[trades] Delta poll requires full reset, fetching snapshot');
            lastResetAtRef.current = now;
            catchingUpRef.current = true;
            await fetchPage({
              keepUnread: !isViewingLatestRef.current,
              silent: true,
              resyncId: pollResyncId,
            });
            catchingUpRef.current = false;
          } else {
            console.warn('[trades] Reset requested but throttled to prevent thrash');
          }
          if (!isActiveRef.current) {
            return;
          }
          setPollDelay((prev) => {
            const base = socketConnectedRef.current ? POLL_BASE_MS : POLL_BASE_MS_DISCONNECTED;
            pollDelayRef.current = base;
            return base;
          });
          setUnread(0);
          emptyPollCountRef.current = 0; // Reset empty poll counter
        } else {
          let pollAcknowledgedCurrentEpoch = false;
          let appliedCurrentEpoch = false;
          const rowsForView = filterEventsForFilters(delta.rows, filtersRef.current);
          if (rowsForView.length) {
            const isLiveView = isViewingLatestRef.current && sortRef.current === 'ts_desc';
            let liveNewRows = 0;
            if (isLiveView) {
              const stats = applyDelta(rowsForView, pageSizeRef.current, pollResyncId);
              liveNewRows = stats?.newRows ?? 0;
              appliedCurrentEpoch = Boolean(stats?.applied);
            }

            const filteredUpserts = rowsForView.filter((evt) => evt.op === 'upsert').length;
            if (!isLiveView && filteredUpserts > 0) {
              setUnread((u) => u + filteredUpserts);
            }

            if (typeof delta.last_seq === 'number') {
              latestSeqRef.current = Math.max(latestSeqRef.current, delta.last_seq);
            }

            if (liveNewRows > 0) {
              playSoundForSeq(typeof delta.last_seq === 'number' ? delta.last_seq : undefined);
            }

            queueSnapshotRefreshRef.current?.(!isLiveView);
            emptyPollCountRef.current = 0; // Reset empty poll counter on successful sync
            setLastUpdate(Date.now());
          } else if (delta.rows?.length) {
            // Rows existed but did not match current filters; treat as successful sync for poll timing.
            emptyPollCountRef.current = 0;
            if (typeof delta.last_seq === 'number') {
              latestSeqRef.current = Math.max(latestSeqRef.current, delta.last_seq);
            }
            if (seqIsNonRegressive) {
              pollAcknowledgedCurrentEpoch = true;
            }
          } else {
            // DEFENSIVE FIX: Track consecutive empty polls
            if (seqIsNonRegressive) {
              const lastSeq = delta.last_seq as number;
              // Empty rows with an advancing (or equal) sequence is a successful reconciliation.
              latestSeqRef.current = Math.max(latestSeqRef.current, lastSeq);
              emptyPollCountRef.current = 0;
              pollAcknowledgedCurrentEpoch = true;
            } else {
              // Empty rows without a usable sequence might indicate a problem.
              emptyPollCountRef.current += 1;
              if (emptyPollCountRef.current >= MAX_EMPTY_POLLS) {
                console.warn(
                  `[trades] ${emptyPollCountRef.current} consecutive empty delta polls. ` +
                  `Socket.IO connected: ${socketConnectedRef.current}. ` +
                  `This may indicate Socket.IO missed events or backend is not emitting trades.`
                );
              }
            }
          }
          if (appliedCurrentEpoch || pollAcknowledgedCurrentEpoch) {
            markGlobalResyncApplied('trades', pollResyncId);
          }
          if (!isActiveRef.current) {
            return;
          }
          setPollDelay((prev) => {
            const base = socketConnectedRef.current ? POLL_BASE_MS : POLL_BASE_MS_DISCONNECTED;
            pollDelayRef.current = base;
            return base;
          });
        }
      } catch (err) {
        console.error('[trades] delta poll failed', err);
        if (!isActiveRef.current) {
          return;
        }
        setPollDelay((prev) => {
          const next = Math.min((prev || POLL_BASE_MS) * 2, POLL_MAX_MS);
          pollDelayRef.current = next;
          return next;
        });
      } finally {
        if (!isActiveRef.current) {
          return;
        }
        schedulePoll();
      }
    }, delay);
  }, [applyDelta, fetchPage, queueSnapshotRefresh, playSoundForSeq, setLastUpdate]);

  useEffect(() => {
    schedulePoll();
    return () => {
      if (pollTimeoutRef.current) {
        window.clearTimeout(pollTimeoutRef.current);
        pollTimeoutRef.current = null;
      }
    };
  }, [schedulePoll]);

  useEffect(() => {
    setPollDelay((prev) => {
      pollDelayRef.current = POLL_BASE_MS;
      return POLL_BASE_MS;
    });
  }, [filters, pageSize, sort]);

  useEffect(() => {
    setUnread(0);
  }, [filters]);

  const processTradeMessage = useCallback((msg: any) => {
    if (!msg || typeof msg !== 'object') {
      return;
    }
    try {
      const normalizedMsg = (msg?.trade && typeof msg.trade === 'object')
        ? {
            ...msg.trade,
            op: msg.op,
            row_id: msg.row_id ?? msg.trade?.row_id,
            version: msg.version ?? msg.trade?.version,
            seq: msg.seq ?? msg.trade?.seq,
            ts_ms: msg.ts_ms ?? msg.server_ts_ms ?? msg.trade?.ts_ms,
            strategy_id: msg.strategy_id ?? msg.trade?.strategy_id,
            signal_id: msg.signal_id ?? msg.strategy_id ?? msg.trade?.signal_id,
          }
        : msg;
      const isPubsubEvent =
        normalizedMsg?.op
        && normalizedMsg?.row_id
        && typeof normalizedMsg?.version === 'number'
        && typeof normalizedMsg?.seq === 'number';
      let event: TradeEvent;
      if (isPubsubEvent) {
        event = normalizedMsg as TradeEvent;
      } else {
        const now = Date.now();
        const timestampParts = getTimestampParts(normalizedMsg);
        if (!timestampParts.hasReliableTimestamp) {
          return;
        }
        const seq: number = timestampParts.seq ?? timestampParts.tsMs ?? timestampParts.ts ?? now;
        const rowIdFromMsg: string | undefined =
          typeof normalizedMsg?.row_id === 'string' && normalizedMsg.row_id ? normalizedMsg.row_id : undefined;
        const rowId: string = rowIdFromMsg || (
          (normalizedMsg && (normalizedMsg.exch_id || normalizedMsg.trade_id || normalizedMsg.order_id))
          || `${normalizedMsg?.exchange || ''}:${normalizedMsg?.coin || ''}:${seq}`
        );
        const versionFromMsg: number | undefined =
          typeof normalizedMsg?.version === 'number' && Number.isFinite(normalizedMsg.version)
            ? normalizedMsg.version
            : undefined;
        const parsedPrice = coerceFiniteNumber(normalizedMsg?.price);
        const parsedQty = coerceFiniteNumber(normalizedMsg?.qty);
        const derivedMv =
          parsedPrice !== undefined && parsedQty !== undefined
            ? parsedPrice * parsedQty
            : undefined;
        const rawMv = coerceFiniteNumber(normalizedMsg?.mv ?? normalizedMsg?.notional);
        const normalizedMv =
          (rawMv === undefined || rawMv === 0) && derivedMv !== undefined && derivedMv !== 0
            ? derivedMv
            : rawMv;
        event = {
          op: 'upsert',
          row_id: rowId,
          version: versionFromMsg ?? 1,
          seq,
          ts: seq,
          time: normalizedMsg?.time,
          coin: normalizedMsg?.coin,
          exchange: normalizedMsg?.exchange,
          side: normalizedMsg?.side,
          price: normalizedMsg?.price,
          qty: normalizedMsg?.qty,
          mv: normalizedMv,
          fee: normalizedMsg?.fee,
          exec_id: normalizedMsg?.exch_id,
          trade_id: normalizedMsg?.trade_id,
          order_id: normalizedMsg?.order_id ?? normalizedMsg?.client_order_id,
          signal_id: normalizedMsg?.signal_id ?? normalizedMsg?.strategy_id,
          strategy_id: normalizedMsg?.strategy_id,
          decision: normalizedMsg?.decision,
          gas: normalizedMsg?.gas_used,
          notes: normalizedMsg?.notes,
          explorer_url: normalizedMsg?.explorer_url,
        } as TradeEvent;
      }

      const messageResyncId = resyncIdRef.current;
      const passesFilters = rowMatchesFilters(event, filtersRef.current);
      if (!passesFilters) {
        if (typeof event.seq === 'number') {
          latestSeqRef.current = Math.max(latestSeqRef.current, event.seq);
        }
        return;
      }

      const isLiveView = isViewingLatestRef.current && sortRef.current === 'ts_desc';
      const op = event.op ?? 'upsert';
      let appliedCurrentEpoch = false;
      if (isLiveView) {
        const stats = applyDeltaRef.current([event], pageSizeRef.current, messageResyncId);
        appliedCurrentEpoch = Boolean(stats?.applied);
        if (op === 'upsert' && (stats?.newRows ?? 0) > 0 && typeof event.seq === 'number') {
          playSoundForSeqRef.current?.(event.seq);
        }
      } else if (op === 'upsert') {
        setUnread((u) => u + 1);
      }

      if (typeof event.seq === 'number') {
        latestSeqRef.current = Math.max(latestSeqRef.current, event.seq);
      }
      queueSnapshotRefreshRef.current?.(!isLiveView);
      if (appliedCurrentEpoch) {
        markGlobalResyncApplied('trades', messageResyncId);
      }
      setLastUpdate(Date.now());
    } catch (err) {
      console.error('[trades] socket trade_update error', err);
    }
  }, [setLastUpdate, setUnread]);

  useEffect(() => {
    const pending: any[] = [];
    let rafId: number | null = null;
    let idleTimer: number | null = null;

    const flushPending = () => {
      if (rafId !== null && typeof window !== 'undefined') {
        window.cancelAnimationFrame(rafId);
        rafId = null;
      }
      if (idleTimer !== null && typeof window !== 'undefined') {
        window.clearTimeout(idleTimer);
        idleTimer = null;
      }
      if (!pending.length) {
        return;
      }
      const items = pending.splice(0);
      for (const item of items) {
        processTradeMessage(item);
      }
    };

    const scheduleFrame = () => {
      if (rafId !== null || typeof window === 'undefined') {
        return;
      }
      rafId = window.requestAnimationFrame(() => {
        rafId = null;
        flushPending();
      });
    };

    const scheduleIdleFlush = () => {
      if (idleTimer !== null || typeof window === 'undefined') {
        return;
      }
      idleTimer = window.setTimeout(() => {
        idleTimer = null;
        flushPending();
      }, 150);
    };

    const enqueueTradeMessage = (msg: any) => {
      pending.push(msg);
      if (isUserScrollingRef.current) {
        scheduleIdleFlush();
      } else {
        scheduleFrame();
      }
    };

    const handleTradeUpdate = (msg: any) => {
      enqueueTradeMessage(msg);
    };

    socket.on('trade_update', handleTradeUpdate);
    return () => {
      socket.off('trade_update', handleTradeUpdate);
      flushPending();
      if (rafId !== null && typeof window !== 'undefined') {
        window.cancelAnimationFrame(rafId);
      }
      if (idleTimer !== null && typeof window !== 'undefined') {
        window.clearTimeout(idleTimer);
      }
    };
  }, [processTradeMessage]);

  // DEFENSIVE FIX: Track Socket.IO connection state for debugging
  useEffect(() => {
    const handleConnect = () => {
      console.log('[trades] Socket.IO connected');
      socketConnectedRef.current = true;
      setSocketConnected(true);
      emptyPollCountRef.current = 0; // Reset empty poll counter on reconnect

      const now = Date.now();
      if (reconnectCatchupInFlightRef.current) {
        return;
      }
      if (now - lastReconnectCatchupAtRef.current < RECONNECT_CATCHUP_MIN_MS) {
        return;
      }
      lastReconnectCatchupAtRef.current = now;
      reconnectCatchupInFlightRef.current = true;
      catchingUpRef.current = true;

      const reconnectResyncId = useResyncStore.getState().resyncId;
      resyncIdRef.current = reconnectResyncId;
      fetchPage({
        silent: true,
        keepUnread: !isViewingLatestRef.current,
        resyncId: reconnectResyncId,
      }).finally(() => {
        reconnectCatchupInFlightRef.current = false;
        catchingUpRef.current = false;
      });
    };
    const handleDisconnect = (reason: string) => {
      console.warn(`[trades] Socket.IO disconnected: ${reason}`);
      socketConnectedRef.current = false;
      setSocketConnected(false);
    };

    socket.on('connect', handleConnect);
    socket.on('disconnect', handleDisconnect);

    // Set initial state based on socket.connected
    socketConnectedRef.current = socket.connected;
    setSocketConnected(socket.connected);

    return () => {
      socket.off('connect', handleConnect);
      socket.off('disconnect', handleDisconnect);
    };
  }, [fetchPage]);

  useEffect(() => {
    isActiveRef.current = true;
    return () => {
      isActiveRef.current = false;
    };
  }, []);

  const clearUnreadAndRefresh = useCallback(() => {
    setUnread(0);
    setPage(1);
    pageRef.current = 1;
    isAtTopRef.current = true;
    recomputeIsViewingLatest(true);
    scrollElementRef.current?.scrollTo({ top: 0 });
    fetchPage();
  }, [fetchPage, recomputeIsViewingLatest]);

  const handleToggleSound = useCallback(() => {
    const newMuted = !soundMuted;
    setSoundMutedState(newMuted);
    setSoundMuted(newMuted);
  }, [soundMuted]);

  const soundToggle = useMemo(() => (
    <Button
      variant="secondary"
      size="xs"
      onClick={handleToggleSound}
      title={
        soundMuted
          ? 'Trade sounds muted (click to enable)'
          : 'Trade sounds enabled (click to mute)'
      }
      style={{ fontSize: typography.fontSize['2xs'] }}
    >
      {soundMuted ? '🔇' : '🔊'}
    </Button>
  ), [handleToggleSound, soundMuted]);

  const unreadBadge = useMemo(() => {
    if (unread <= 0) {
      return null;
    }
    return (
      <Button
        variant="secondary"
        size="xs"
        onClick={clearUnreadAndRefresh}
        title="New trades arrived. Jump to latest."
        style={{
          marginLeft: spacing.gap.xs,
          fontSize: typography.fontSize['2xs'],
        }}
      >
        {unread} new
      </Button>
    );
  }, [clearUnreadAndRefresh, unread]);

  const headerActions = useMemo(
    () => {
      const isCatchingUp = catchingUpRef.current;
      const dotColor = socketConnected
        ? (isCatchingUp ? colors.semantic.warning.DEFAULT : colors.semantic.success.DEFAULT)
        : colors.text.muted;
      const handleExport = () => {
        // Map current visible rows to flat export objects
        const data = (rowsToRender || []).map((r) => ({
          time: r.time || '',
          coin: r.coin || '',
          exchange: r.exchange || '',
          side: r.side || '',
          price: r.price ?? '',
          qty: r.qty ?? '',
          notional: r.mv ?? '',
          fee: r.fee ?? '',
          gas_used: (r as any).gas_used ?? '',
          row_id: r.row_id,
          trade_id: r.trade_id || '',
          order_id: r.order_id || '',
          signal_id: r.signal_id || '',
          strategy_id: r.strategy_id || '',
          decision: (r as any).decision || '',
        }));
        const filename = generateTimestampFilename('trades', 'csv');
        exportCSV(data, filename);
      };
      const perfHarnessTrigger = DEV_TRADES_PERF_HARNESS && !perfHarnessActive ? (
        <Button
          variant="secondary"
          size="xs"
          onClick={() => setPerfHarnessActive(true)}
          title="Launch perf harness"
        >
          Perf harness
        </Button>
      ) : null;

      return (
        <div className="flex items-center" style={{ gap: spacing.gap.xs }}>
          <span
            title={socketConnected ? (isCatchingUp ? 'Catching up' : 'LIVE') : 'Offline'}
            style={{ display: 'inline-flex', alignItems: 'center', gap: 6, color: colors.text.muted, fontSize: typography.fontSize.xs }}
          >
            <span
              aria-label="live-status"
              style={{
                width: 8,
                height: 8,
                borderRadius: 9999,
                backgroundColor: dotColor,
                display: 'inline-block',
              }}
            />
            {socketConnected ? (isCatchingUp ? 'CATCHING UP' : 'LIVE') : 'OFFLINE'}
          </span>
          {isResyncing ? (
            <span
              title={`Resync ${resyncId} in progress`}
              style={{ color: colors.semantic.warning.DEFAULT, fontSize: typography.fontSize.xs }}
            >
              RESYNCING #{resyncId}
            </span>
          ) : null}
          {loading ? (
            <span style={{ color: colors.text.muted, fontSize: typography.fontSize.xs }}>Loading…</span>
          ) : null}
          {unreadBadge}
          <Button variant="secondary" size="xs" onClick={handleExport} title="Export current view as CSV">
            Export CSV
          </Button>
          {perfHarnessTrigger}
        </div>
      );
    },
    [loading, unreadBadge, lastUpdate, rowsToRender, perfHarnessActive, isResyncing, resyncId, socketConnected]
  );

  const panelHeaderSlots = usePanelHeaderSlots();

  useEffect(() => {
    if (!panelHeaderSlots) {
      return;
    }

    if (showHeader) {
      panelHeaderSlots.setTitleActions(null);
      panelHeaderSlots.setActions(null);
      return;
    }

    panelHeaderSlots.setTitleActions(soundToggle);
    panelHeaderSlots.setActions(headerActions);

    return () => {
      panelHeaderSlots.setTitleActions(null);
      panelHeaderSlots.setActions(null);
    };
  }, [panelHeaderSlots, showHeader, soundToggle, headerActions]);

  return (
    <div
      className={`flex flex-col h-full overflow-hidden ${className}`}
      style={{ backgroundColor: colors.bg.base }}
    >
      {showHeader && (
        <PanelHeader
          title="Trades"
          onRefresh={clearUnreadAndRefresh}
          refreshing={loading}
          lastUpdate={lastUpdate}
          staleThresholdMs={STALE_THRESHOLDS.REALTIME}
          titleActions={soundToggle}
          actions={headerActions}
          onRemove={onRemove}
        />
      )}

      {/* Live status banner (Phase 1): show OFFLINE or STALE prominently */}
      {(() => {
        const showOffline = !socketConnected;
        const showCatchingUp = catchingUpRef.current;
        if (!showOffline && !showCatchingUp) return null;
        const bg = showOffline ? colors.semantic.danger.bg : colors.semantic.warning.bg;
        const fg = colors.text.secondary;
        const label = showOffline ? 'OFFLINE — Reconnecting…' : 'CATCHING UP…';
        return (
          <div
            className="w-full"
            style={{ backgroundColor: bg, color: fg, padding: `${spacing.gap.xs} ${spacing.gap.md}` }}
            role="status"
            aria-live="polite"
          >
            {label}
          </div>
        );
      })()}

      {/* When embedded in dashboard (showHeader=false), render actions as toolbar */}
      {!showHeader && !panelHeaderSlots && (
        <div
          className="flex items-center justify-end"
          style={{
            padding: `${spacing.gap.sm} ${spacing.gap.md}`,
            backgroundColor: colors.bg.surface,
            borderBottom: `1px solid ${colors.border.DEFAULT}`,
            gap: spacing.gap.sm,
          }}
        >
          {headerActions}
          {soundToggle}
        </div>
      )}

      <div className="flex-1 flex flex-col overflow-hidden">
        <TableFilter
          columns={TRADE_FILTERS}
          onFilterChange={handleFilterChange}
          value={filters}
          dense={dense}
        />

        <div
          className="flex items-center justify-between"
          style={{
            padding: `${spacing.gap.sm} ${spacing.gap.md}`,
            borderBottom: `1px solid ${colors.border.DEFAULT}`,
          }}
        >
          <PageSizeControl value={pageSize} onChange={setPageSize} />
          <span style={{ color: colors.text.muted, fontSize: typography.fontSize.sm }}>
            Loaded {storeRows.length.toLocaleString()} of {total.toLocaleString()}
          </span>
        </div>

        <div className="flex-1 min-h-0">
          <TradesTable
            trades={rowsToRender}
            sortDirection={sort}
            onTimeSortChange={handleTimeSortChange}
            onScrollStateChange={handleScrollStateChange}
            enableDecisionDetails={decisionDetailsEnabled}
          />
        </div>

        <div
          className="flex items-center justify-between"
          style={{
            padding: `${spacing.gap.sm} ${spacing.gap.md}`,
            borderTop: `1px solid ${colors.border.DEFAULT}`,
            gap: spacing.gap.sm,
          }}
        >
          <div className="flex items-center" style={{ gap: spacing.gap.sm }}>
            <Button variant="secondary" size="sm" disabled={!canPrev} onClick={goPrev}>
              ‹ Prev
            </Button>
            <span style={{ color: colors.text.muted, fontSize: typography.fontSize.sm }}>
              {showUnboundedPageIndicator
                ? `Page ${page}`
                : `Page ${Math.min(page, totalPages)} of ${totalPages}`}
            </span>
            <Button variant="secondary" size="sm" disabled={!canNext} onClick={goNext}>
              Next ›
            </Button>
          </div>
          <div className="flex items-center" style={{ gap: spacing.gap.md }}>
            {/* Footer rollups: sums over current view */}
            {(() => {
              const fmt = (v?: number | null) => {
                if (v == null || Number.isNaN(v)) return '0';
                const val = Number(v);
                if (!Number.isFinite(val)) return '0';
                return val.toFixed(val >= 100 ? 2 : 6);
              };
              const sum = (arr: any[], key: 'qty' | 'mv' | 'fee' | 'gas_used'): number => {
                let total = 0;
                for (const r of arr) {
                  const x = (r as any)[key];
                  const n = typeof x === 'number' ? x : (typeof x === 'string' && x.trim() !== '' ? Number(x) : NaN);
                  if (Number.isFinite(n)) total += n as number;
                }
                return total;
              };
              const view = rowsToRender || [];
              const q = sum(view, 'qty');
              const notional = sum(view, 'mv');
              const fees = sum(view, 'fee');
              const gas = sum(view, 'gas_used');
              return (
                <span style={{ color: colors.text.muted, fontSize: typography.fontSize.sm }}>
                  Σ qty: {fmt(q)} • Σ notional: {fmt(notional)} • Σ fee: {fmt(fees)} • Σ gas: {fmt(gas)}
                </span>
              );
            })()}
            {!isViewingLatest && (
              <Button variant="secondary" size="sm" onClick={clearUnreadAndRefresh}>
                Jump to latest
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
