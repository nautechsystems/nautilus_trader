import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api } from './api';
import { OrderViewChart } from './components/domain/orderView/OrderViewChart';
import { OrderViewEventsTable } from './components/domain/orderView/OrderViewEventsTable';
import { OrderViewFillsTable } from './components/domain/orderView/OrderViewFillsTable';
import { OrderViewL1Widget } from './components/domain/orderView/OrderViewL1Widget';
import { OrderViewMarketTradesTable } from './components/domain/orderView/OrderViewMarketTradesTable';
import { OrderViewOpenOrdersTable } from './components/domain/orderView/OrderViewOpenOrdersTable';
import { PageShell } from './components/layout/PageShell';
import { colors, typography } from './lib/tokens';
import { socket } from './sockets';
import {
  bumpGlobalResync,
  markGlobalResyncApplied,
  registerGlobalResyncConsumer,
  selectResyncId,
  shallow,
  unregisterGlobalResyncConsumer,
  useResyncStore,
} from './stores';
import { useOrderViewStore } from './stores/orderViewStore';
import type { OrderViewDelta, OrderViewEvent, OrderViewLeg } from './types';

const ORDER_VIEW_V02 = true;
const ORDER_VIEW_INCLUDE_BOOK = ORDER_VIEW_V02;
const ORDER_VIEW_BOOK_DEPTH = 20;
const ORDER_VIEW_EVENTS_LIMIT = 200;
const ORDER_VIEW_CANDLE_INTERVAL_MS = 1_000;
const ORDER_VIEW_FLUSH_INTERVAL_MS = 50; // 20Hz max apply cadence
const ORDER_VIEW_SUBSCRIBE_HEARTBEAT_MS = 30_000;
const ORDER_VIEW_STALE_MD_MS = 3_000;
const ORDER_VIEW_FOCUS_MATCH_EPSILON = 1e-6;

type ActiveSelection = {
  strategyId: string;
  leg: OrderViewLeg;
};

const normalizeLeg = (value: unknown): OrderViewLeg => {
  const text = String(value || '').trim().toLowerCase();
  if (text === 'hedge') return 'hedge';
  if (text === 'both') return 'both';
  return 'maker';
};

const toSelection = (strategyId: string, leg: OrderViewLeg): ActiveSelection => ({
  strategyId: String(strategyId || '').trim(),
  leg: normalizeLeg(leg),
});

const sameSelection = (lhs: ActiveSelection | null, rhs: ActiveSelection): boolean =>
  !!lhs && lhs.strategyId === rhs.strategyId && lhs.leg === rhs.leg;

const selectionPayload = (selection: ActiveSelection) => ({
  strategy_id: selection.strategyId,
  leg: selection.leg,
  include_book: ORDER_VIEW_INCLUDE_BOOK,
  book_depth: ORDER_VIEW_BOOK_DEPTH,
  events_limit: ORDER_VIEW_EVENTS_LIMIT,
  candle_interval_ms: ORDER_VIEW_CANDLE_INTERVAL_MS,
  order_view_v02: ORDER_VIEW_V02,
});

const toFiniteNumber = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const normalizeResyncId = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return 0;
  }
  return Math.trunc(parsed);
};

const normalizeBookSide = (value: unknown): 'bid' | 'ask' | null => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (text === 'buy' || text === 'bid') return 'bid';
  if (text === 'sell' || text === 'ask') return 'ask';
  return null;
};

const toOrderKey = (row: { order_id?: unknown; client_order_id?: unknown }): string | null => {
  const orderId = String(row.order_id || '').trim();
  if (orderId) return orderId;
  const clientOrderId = String(row.client_order_id || '').trim();
  if (clientOrderId) return clientOrderId;
  return null;
};

const samePrice = (lhs: number | null, rhs: number | null): boolean => {
  if (lhs === null || rhs === null) return false;
  return Math.abs(lhs - rhs) <= ORDER_VIEW_FOCUS_MATCH_EPSILON;
};

export default function OrderView() {
  const {
    selection,
    focus,
    roomId,
    context,
    bbo,
    status,
    lastSeq,
    lastServerTsMs,
    needsResync,
    pendingDeltaCount,
    openOrderIds,
    openOrdersById,
    events,
    fills,
    marketTrades,
    lifetimeSegments,
    candles,
    candleSource,
    l2,
    priceSeries,
    setSelection,
    setFocus,
    clearFocus,
    applySnapshot,
    applyDelta,
    clearNeedsResync,
  } = useOrderViewStore(
    (state) => ({
      selection: state.selection,
      focus: state.focus,
      roomId: state.roomId,
      context: state.context,
      bbo: state.bbo,
      status: state.status,
      lastSeq: state.lastSeq,
      lastServerTsMs: state.lastServerTsMs,
      needsResync: state.needsResync,
      pendingDeltaCount: state.pendingDeltaCount,
      openOrderIds: state.openOrderIds,
      openOrdersById: state.openOrdersById,
      events: state.events,
      fills: state.fills,
      marketTrades: state.marketTrades,
      lifetimeSegments: state.lifetimeSegments,
      candles: state.candles,
      candleSource: state.candleSource,
      l2: state.l2,
      priceSeries: state.priceSeries,
      setSelection: state.setSelection,
      setFocus: state.setFocus,
      clearFocus: state.clearFocus,
      applySnapshot: state.applySnapshot,
      applyDelta: state.applyDelta,
      clearNeedsResync: state.clearNeedsResync,
    }),
    shallow
  );

  const resyncId = useResyncStore(selectResyncId);

  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [strategyOptions, setStrategyOptions] = useState<string[]>([]);
  const [socketConnected, setSocketConnected] = useState(socket.connected);
  const [lastSyncReason, setLastSyncReason] = useState('init');
  const [activeRightTab, setActiveRightTab] = useState<'order_book' | 'recent_trades'>('order_book');
  const [activeBottomTab, setActiveBottomTab] = useState<'open_orders' | 'our_fills' | 'our_order_events'>('open_orders');
  const [showBboLines, setShowBboLines] = useState(false);
  const [tapeOrderSearch, setTapeOrderSearch] = useState('');
  const [tapeAutoScroll, setTapeAutoScroll] = useState(true);
  const [nowMs, setNowMs] = useState(() => Date.now());

  const mountedRef = useRef(true);
  const resyncIdRef = useRef(resyncId);
  const activeSelectionRef = useRef<ActiveSelection>(toSelection('', 'maker'));
  const subscribedSelectionRef = useRef<ActiveSelection | null>(null);
  const activeRoomRef = useRef<string | null>(null);
  const fetchTokenRef = useRef(0);
  const pendingDeltasRef = useRef<OrderViewDelta[]>([]);
  const rafHandleRef = useRef<number | null>(null);
  const timerHandleRef = useRef<number | null>(null);
  const lastFlushTsRef = useRef(0);

  useEffect(() => {
    resyncIdRef.current = resyncId;
  }, [resyncId]);

  useEffect(() => {
    activeRoomRef.current = roomId || null;
  }, [roomId]);

  const clearScheduledFlush = useCallback(() => {
    if (rafHandleRef.current !== null && typeof window !== 'undefined') {
      window.cancelAnimationFrame(rafHandleRef.current);
      rafHandleRef.current = null;
    }
    if (timerHandleRef.current !== null && typeof window !== 'undefined') {
      window.clearTimeout(timerHandleRef.current);
      timerHandleRef.current = null;
    }
  }, []);

  const emitSubscribe = useCallback((nextSelection: ActiveSelection) => {
    if (!nextSelection.strategyId) return;
    socket.emit(
      'order_view_subscribe',
      selectionPayload(nextSelection),
      (ack?: { ok?: boolean; error?: string }) => {
        if (!mountedRef.current) return;
        if (ack && ack.ok === false) {
          setError(`subscribe_failed:${String(ack.error || 'unknown')}`);
        }
      }
    );
  }, []);

  const emitUnsubscribe = useCallback((prevSelection: ActiveSelection, prevRoomId?: string | null) => {
    if (!prevSelection.strategyId) return;
    if (prevRoomId) {
      socket.emit('order_view_unsubscribe', { room_id: prevRoomId });
      return;
    }
    socket.emit('order_view_unsubscribe', selectionPayload(prevSelection));
  }, []);

  const fetchSnapshot = useCallback(
    async (reason: string, forcedResyncId?: number): Promise<boolean> => {
      const currentSelection = activeSelectionRef.current;
      if (!currentSelection.strategyId) return false;
      const token = ++fetchTokenRef.current;
      const activeResyncId = Math.max(
        normalizeResyncId(forcedResyncId),
        normalizeResyncId(resyncIdRef.current),
        normalizeResyncId(useResyncStore.getState().resyncId)
      );
      resyncIdRef.current = activeResyncId;

      setLoading(true);
      setError(null);
      try {
        const candleRange = useOrderViewStore.getState().selection.timeRange;
        const snapshot = await api.getOrderViewSnapshot({
          strategyId: currentSelection.strategyId,
          leg: currentSelection.leg,
          includeEvents: true,
          eventsLimit: ORDER_VIEW_EVENTS_LIMIT,
          includeBook: ORDER_VIEW_INCLUDE_BOOK,
          bookDepth: ORDER_VIEW_BOOK_DEPTH,
          candleIntervalMs: ORDER_VIEW_CANDLE_INTERVAL_MS,
          candleRange,
          orderViewV02: ORDER_VIEW_V02,
        });
        if (token !== fetchTokenRef.current || !mountedRef.current) {
          return false;
        }
        const result = applySnapshot(snapshot, activeResyncId);
        if (result.applied) {
          activeRoomRef.current = snapshot.room_id || null;
          clearNeedsResync();
          markGlobalResyncApplied('order-view', activeResyncId);
        }
        setLastSyncReason(reason);
        return result.applied;
      } catch (err) {
        if (token === fetchTokenRef.current && mountedRef.current) {
          const message = err instanceof Error ? err.message : 'snapshot_failed';
          setError(message);
        }
        return false;
      } finally {
        if (token === fetchTokenRef.current && mountedRef.current) {
          setLoading(false);
        }
      }
    },
    [applySnapshot, clearNeedsResync]
  );

  const flushDeltas = useCallback(() => {
    clearScheduledFlush();
    if (!pendingDeltasRef.current.length) return;

    const currentResyncId = resyncIdRef.current;
    const batch = pendingDeltasRef.current.splice(0);
    let mustRefetch = false;

    for (const delta of batch) {
      const result = applyDelta(delta, currentResyncId);
      if (result.applied && delta.room_id) {
        activeRoomRef.current = String(delta.room_id);
      }
      if (result.needsResync || result.seqGap) {
        mustRefetch = true;
        break;
      }
    }

    lastFlushTsRef.current = Date.now();
    if (mustRefetch) {
      const nextResyncId = bumpGlobalResync('order-view-seq-gap');
      resyncIdRef.current = nextResyncId;
      void fetchSnapshot('delta_resync', nextResyncId);
      return;
    }
    markGlobalResyncApplied('order-view', currentResyncId);
  }, [applyDelta, clearScheduledFlush, fetchSnapshot]);

  const scheduleDeltaFlush = useCallback(() => {
    if (typeof window === 'undefined') {
      flushDeltas();
      return;
    }
    const elapsed = Date.now() - lastFlushTsRef.current;
    const delay = elapsed >= ORDER_VIEW_FLUSH_INTERVAL_MS ? 0 : ORDER_VIEW_FLUSH_INTERVAL_MS - elapsed;

    if (timerHandleRef.current === null) {
      timerHandleRef.current = window.setTimeout(() => {
        timerHandleRef.current = null;
        if (rafHandleRef.current !== null) {
          window.cancelAnimationFrame(rafHandleRef.current);
          rafHandleRef.current = null;
        }
        flushDeltas();
      }, delay);
    }

    if (rafHandleRef.current !== null) return;
    rafHandleRef.current = window.requestAnimationFrame(() => {
      rafHandleRef.current = null;
      if (delay > 0 && timerHandleRef.current !== null) {
        return;
      }
      flushDeltas();
    });
  }, [flushDeltas]);

  useEffect(() => {
    mountedRef.current = true;
    registerGlobalResyncConsumer('order-view');
    return () => {
      unregisterGlobalResyncConsumer('order-view');
      mountedRef.current = false;
      fetchTokenRef.current += 1;
      pendingDeltasRef.current = [];
      clearScheduledFlush();
      const subscribed = subscribedSelectionRef.current;
      if (subscribed && subscribed.strategyId) {
        emitUnsubscribe(subscribed, activeRoomRef.current);
      }
    };
  }, [clearScheduledFlush, emitUnsubscribe]);

  useEffect(() => {
    let cancelled = false;
    const loadStrategies = async () => {
      try {
        const payload = await api.getSignalStrategies();
        if (cancelled || !mountedRef.current) return;
        const ids = Array.from(
          new Set(
            (payload.strategies || [])
              .map((row) => String(row?.id || '').trim())
              .filter((value) => value.length > 0)
          )
        ).sort((lhs, rhs) => lhs.localeCompare(rhs));
        setStrategyOptions(ids);
        const currentSelection = useOrderViewStore.getState().selection;
        if (!currentSelection.strategyId && ids.length > 0) {
          useOrderViewStore.getState().setSelection({ strategyId: ids[0] });
        }
      } catch (err) {
        if (import.meta.env?.DEV) {
          // eslint-disable-next-line no-console
          console.debug('[order-view] strategy discovery failed', err);
        }
      }
    };
    void loadStrategies();
    return () => {
      cancelled = true;
    };
  }, []);

  const lastRangeRef = useRef(selection.timeRange);

  useEffect(() => {
    const nextSelection = toSelection(selection.strategyId, selection.leg);
    activeSelectionRef.current = nextSelection;
    pendingDeltasRef.current = [];
    clearScheduledFlush();

    const previous = subscribedSelectionRef.current;
    if (previous && !sameSelection(previous, nextSelection)) {
      emitUnsubscribe(previous, activeRoomRef.current);
      activeRoomRef.current = null;
    }

    if (!nextSelection.strategyId) {
      subscribedSelectionRef.current = null;
      return;
    }
    subscribedSelectionRef.current = nextSelection;

    const syncSelection = async () => {
      const applied = await fetchSnapshot('selection_change');
      if (!applied) return;
      if (!sameSelection(activeSelectionRef.current, nextSelection)) return;
      emitSubscribe(nextSelection);
    };
    void syncSelection();
  }, [clearScheduledFlush, emitSubscribe, emitUnsubscribe, fetchSnapshot, selection.leg, selection.strategyId]);

  useEffect(() => {
    const nextRange = selection.timeRange;
    const prevRange = lastRangeRef.current;
    lastRangeRef.current = nextRange;
    if (prevRange === nextRange) return;
    const currentSelection = activeSelectionRef.current;
    if (!currentSelection.strategyId) return;
    void fetchSnapshot('range_change');
  }, [fetchSnapshot, selection.timeRange]);

  useEffect(() => {
    const handleDelta = (payload: OrderViewDelta) => {
      const currentSelection = activeSelectionRef.current;
      if (!currentSelection.strategyId || !payload || typeof payload !== 'object') return;
      const strategyId = String(payload.selection?.strategy_id || '').trim();
      const leg = normalizeLeg(payload.selection?.leg);
      if (strategyId !== currentSelection.strategyId || leg !== currentSelection.leg) {
        return;
      }
      pendingDeltasRef.current.push(payload);
      scheduleDeltaFlush();
    };

    const handleConnect = () => {
      setSocketConnected(true);
      const currentSelection = activeSelectionRef.current;
      if (!currentSelection.strategyId) return;
      const reconnectResyncId = Math.max(
        normalizeResyncId(resyncIdRef.current),
        normalizeResyncId(useResyncStore.getState().resyncId)
      );
      resyncIdRef.current = reconnectResyncId;
      const syncReconnect = async () => {
        await fetchSnapshot('socket_reconnect', reconnectResyncId);
        if (!sameSelection(activeSelectionRef.current, currentSelection)) return;
        emitSubscribe(currentSelection);
      };
      void syncReconnect();
    };

    const handleDisconnect = () => {
      setSocketConnected(false);
    };

    socket.on('order_view_delta', handleDelta as any);
    socket.on('connect', handleConnect);
    socket.on('disconnect', handleDisconnect);
    setSocketConnected(socket.connected);

    return () => {
      socket.off('order_view_delta', handleDelta as any);
      socket.off('connect', handleConnect);
      socket.off('disconnect', handleDisconnect);
    };
  }, [emitSubscribe, fetchSnapshot, scheduleDeltaFlush]);

  useEffect(() => {
    if (typeof window === 'undefined' || !socketConnected) return undefined;
    const intervalId = window.setInterval(() => {
      const currentSelection = activeSelectionRef.current;
      if (!currentSelection.strategyId) return;
      emitSubscribe(currentSelection);
    }, ORDER_VIEW_SUBSCRIBE_HEARTBEAT_MS);
    return () => {
      window.clearInterval(intervalId);
    };
  }, [emitSubscribe, socketConnected]);

  useEffect(() => {
    if (typeof window === 'undefined') return undefined;
    const intervalId = window.setInterval(() => setNowMs(Date.now()), 1_000);
    return () => {
      window.clearInterval(intervalId);
    };
  }, []);

  const runManualResync = useCallback(() => {
    const targetSelection = activeSelectionRef.current;
    if (!targetSelection.strategyId) return;
    const nextResyncId = bumpGlobalResync('order-view-manual-resync');
    resyncIdRef.current = nextResyncId;
    const syncManual = async () => {
      const applied = await fetchSnapshot('manual_resync', nextResyncId);
      if (!applied) return;
      if (!sameSelection(activeSelectionRef.current, targetSelection)) return;
      emitSubscribe(targetSelection);
    };
    void syncManual();
  }, [emitSubscribe, fetchSnapshot]);

  const openOrders = useMemo(
    () =>
      openOrderIds
        .map((orderRowId) => openOrdersById[orderRowId])
        .filter((row): row is NonNullable<typeof row> => Boolean(row)),
    [openOrderIds, openOrdersById]
  );
  const latestMdTsMs = useMemo(() => {
    return Math.max(
      toFiniteNumber(status?.last_md_ts_ms) ?? 0,
      toFiniteNumber(bbo?.maker?.ts_ms) ?? 0,
      toFiniteNumber(bbo?.hedge?.ts_ms) ?? 0
    );
  }, [bbo?.hedge?.ts_ms, bbo?.maker?.ts_ms, status?.last_md_ts_ms]);
  const contractMdAgeMs = useMemo(() => {
    const mdAge = toFiniteNumber(status?.md_age_ms);
    if (mdAge !== null && mdAge >= 0) {
      return mdAge;
    }
    const candidateAges = [status?.l2_age_ms, status?.trades_age_ms, status?.bbo_age_ms]
      .map((value) => toFiniteNumber(value))
      .filter((value): value is number => value !== null && value >= 0);
    if (candidateAges.length === 0) {
      return null;
    }
    return Math.max(...candidateAges);
  }, [status?.bbo_age_ms, status?.l2_age_ms, status?.md_age_ms, status?.trades_age_ms]);
  const mdAgeMs = useMemo(() => {
    if (contractMdAgeMs !== null) {
      return contractMdAgeMs;
    }
    if (latestMdTsMs <= 0) return Number.POSITIVE_INFINITY;
    return Math.max(0, nowMs - latestMdTsMs);
  }, [contractMdAgeMs, latestMdTsMs, nowMs]);
  const staleData = !socketConnected || mdAgeMs > ORDER_VIEW_STALE_MD_MS;
  const visibleFills = useMemo(
    () => (selection.showFills ? fills : []),
    [fills, selection.showFills]
  );
  const selectedLegBbo = useMemo(() => {
    if (selection.leg === 'hedge') {
      return bbo.hedge;
    }
    if (selection.leg === 'both') {
      return bbo.maker || bbo.hedge;
    }
    return bbo.maker || bbo.hedge;
  }, [bbo.hedge, bbo.maker, selection.leg]);
  const selectedBestBid = toFiniteNumber(selectedLegBbo?.bid);
  const selectedBestAsk = toFiniteNumber(selectedLegBbo?.ask);

  const openOrderCount = openOrders.length;
  const eventCount = events.length;
  const fillCount = fills.length;
  const marketTradeCount = marketTrades.length;
  const pricePointCount = priceSeries.length;
  const candleCount = candles.length;
  const statusNotes = useMemo(() => (status?.notes || []).slice(0, 6), [status?.notes]);
  const lastServerTime = useMemo(() => {
    if (!lastServerTsMs) return '--';
    return new Date(lastServerTsMs).toLocaleTimeString();
  }, [lastServerTsMs]);
  const mdAgeLabel = useMemo(() => {
    if (!Number.isFinite(mdAgeMs)) return '--';
    if (mdAgeMs < 1_000) return `${Math.round(mdAgeMs)}ms`;
    return `${(mdAgeMs / 1_000).toFixed(1)}s`;
  }, [mdAgeMs]);
  const candleSourceBadge = useMemo(() => {
    const normalized = String(candleSource || '').trim().toLowerCase();
    if (normalized === 'trades') {
      return {
        text: 'CANDLES: TRADES',
        color: colors.semantic.success.DEFAULT,
      };
    }
    if (normalized === 'bbo_fallback') {
      return {
        text: 'CANDLES: BBO (DEGRADED)',
        color: colors.semantic.warning.DEFAULT,
      };
    }
    return {
      text: 'CANDLES: --',
      color: colors.text.muted,
    };
  }, [candleSource]);
  const hasFocus = Boolean(
    focus.orderKey || focus.eventKey || focus.side || focus.price !== null
  );
  const focusLabel = useMemo(() => {
    if (!hasFocus) return '--';
    return [
      focus.eventKey ? `evt:${focus.eventKey}` : null,
      focus.orderKey ? `ord:${focus.orderKey}` : null,
      focus.side ? `side:${focus.side}` : null,
      focus.price !== null ? `px:${focus.price.toFixed(6)}` : null,
    ]
      .filter(Boolean)
      .join(' ');
  }, [focus.eventKey, focus.orderKey, focus.price, focus.side, hasFocus]);

  const handleLadderRowClick = useCallback(
    (row: { side: 'bid' | 'ask'; price: number }) => {
      const matchedOpenOrder = openOrders.find((openRow) => {
        const openRowSide = normalizeBookSide(openRow.side);
        const openRowPrice = toFiniteNumber(openRow.px);
        return openRowSide === row.side && samePrice(openRowPrice, row.price);
      });
      const orderKey = matchedOpenOrder ? toOrderKey(matchedOpenOrder) : null;
      const isSameFocus =
        focus.eventKey === null &&
        focus.side === row.side &&
        samePrice(focus.price, row.price) &&
        focus.orderKey === orderKey;
      if (isSameFocus) {
        clearFocus();
        return;
      }
      setFocus({
        eventKey: null,
        orderKey,
        side: row.side,
        price: row.price,
      });
    },
    [clearFocus, focus.eventKey, focus.orderKey, focus.price, focus.side, openOrders, setFocus]
  );

  const handleEventRowClick = useCallback(
    (row: OrderViewEvent) => {
      const nextEventKey = String(row.event_key || '').trim() || null;
      const nextOrderKey = toOrderKey(row);
      const nextSide = normalizeBookSide(row.side);
      const nextPrice = toFiniteNumber(row.px);
      const isSameFocus =
        focus.eventKey === nextEventKey &&
        focus.orderKey === nextOrderKey &&
        focus.side === nextSide &&
        (focus.price === nextPrice || samePrice(focus.price, nextPrice));
      if (isSameFocus) {
        clearFocus();
        return;
      }
      setFocus({
        eventKey: nextEventKey,
        orderKey: nextOrderKey,
        side: nextSide,
        price: nextPrice,
      });
      setActiveBottomTab('our_order_events');
    },
    [clearFocus, focus.eventKey, focus.orderKey, focus.price, focus.side, setFocus]
  );

  return (
    <PageShell>
      <div className="flex flex-col h-full overflow-hidden">
        <div
          className="flex items-center justify-between px-4 py-3 border-b gap-3"
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
              Order View
            </h2>
            <span className="text-xs uppercase tracking-wide" style={{ color: colors.text.muted }}>
              Exchange-style debug view
            </span>
            <span
              className="text-[11px] px-2 py-0.5 rounded border"
              style={{
                borderColor: socketConnected
                  ? colors.semantic.success.DEFAULT
                  : colors.border.DEFAULT,
                color: socketConnected
                  ? colors.semantic.success.DEFAULT
                  : colors.text.muted,
              }}
            >
              {socketConnected ? 'LIVE' : 'DISCONNECTED'}
            </span>
            {staleData ? (
              <span
                className="text-[11px] px-2 py-0.5 rounded border"
                style={{
                  borderColor: colors.semantic.warning.DEFAULT,
                  color: colors.semantic.warning.DEFAULT,
                }}
              >
                STALE DATA
              </span>
            ) : null}
            {needsResync ? (
              <span
                className="text-[11px] px-2 py-0.5 rounded border"
                style={{
                  borderColor: colors.semantic.warning.DEFAULT,
                  color: colors.semantic.warning.DEFAULT,
                }}
              >
                RESYNC REQUIRED
              </span>
            ) : null}
            <span
              className="text-[11px] px-2 py-0.5 rounded border"
              style={{
                borderColor: candleSourceBadge.color,
                color: candleSourceBadge.color,
              }}
            >
              {candleSourceBadge.text}
            </span>
          </div>

          <div className="flex items-center gap-2 text-xs flex-wrap justify-end">
            <label className="flex items-center gap-1">
              <span style={{ color: colors.text.muted }}>Strategy</span>
              <select
                value={selection.strategyId}
                onChange={(event) => setSelection({ strategyId: event.target.value })}
                className="px-2 py-1 rounded border bg-transparent min-w-[260px]"
                style={{ borderColor: colors.border.DEFAULT, color: colors.text.primary }}
              >
                <option value="">Select strategy</option>
                {strategyOptions.map((strategyId) => (
                  <option key={strategyId} value={strategyId}>
                    {strategyId}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex items-center gap-1">
              <span style={{ color: colors.text.muted }}>Leg</span>
              <select
                value={selection.leg}
                onChange={(event) =>
                  setSelection({ leg: event.target.value as typeof selection.leg })
                }
                className="px-2 py-1 rounded border bg-transparent"
                style={{ borderColor: colors.border.DEFAULT, color: colors.text.primary }}
              >
                <option value="maker">Maker</option>
                <option value="hedge">Hedge</option>
                <option value="both">Both</option>
              </select>
            </label>
            <label className="flex items-center gap-1">
              <span style={{ color: colors.text.muted }}>Range</span>
              <select
                value={selection.timeRange}
                onChange={(event) =>
                  setSelection({ timeRange: event.target.value as typeof selection.timeRange })
                }
                className="px-2 py-1 rounded border bg-transparent"
                style={{ borderColor: colors.border.DEFAULT, color: colors.text.primary }}
              >
                <option value="5m">5m</option>
                <option value="15m">15m</option>
                <option value="1h">1h</option>
              </select>
            </label>
            <label className="flex items-center gap-1" style={{ color: colors.text.muted }}>
              <input
                type="checkbox"
                checked={selection.showBids}
                onChange={(event) => setSelection({ showBids: event.target.checked })}
              />
              Show Bids
            </label>
            <label className="flex items-center gap-1" style={{ color: colors.text.muted }}>
              <input
                type="checkbox"
                checked={selection.showAsks}
                onChange={(event) => setSelection({ showAsks: event.target.checked })}
              />
              Show Asks
            </label>
            <label className="flex items-center gap-1" style={{ color: colors.text.muted }}>
              <input
                type="checkbox"
                checked={selection.showFills}
                onChange={(event) => setSelection({ showFills: event.target.checked })}
              />
              Show Fills
            </label>
            <label className="flex items-center gap-1" style={{ color: colors.text.muted }}>
              <input
                type="checkbox"
                checked={showBboLines}
                onChange={(event) => setShowBboLines(event.target.checked)}
              />
              Show BBO Lines
            </label>
            <button
              type="button"
              onClick={() => {
                const wasPaused = selection.paused;
                setSelection({ paused: !wasPaused });
                if (wasPaused && (pendingDeltaCount > 0 || needsResync)) {
                  runManualResync();
                }
              }}
              className="px-2 py-1 rounded border"
              style={{ borderColor: colors.border.DEFAULT, color: colors.text.primary }}
            >
              {selection.paused ? 'Resume' : 'Pause'}
            </button>
            <button
              type="button"
              onClick={runManualResync}
              className="px-2 py-1 rounded border"
              style={{ borderColor: colors.border.DEFAULT, color: colors.text.primary }}
            >
              Resync
            </button>
          </div>
        </div>

        <div className="flex-1 min-h-0 flex flex-col p-3 gap-3">
          <section
            className="rounded border px-3 py-2 text-xs flex flex-wrap gap-4"
            style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.surface }}
          >
            <span style={{ color: colors.text.muted }}>
              Room: <span style={{ color: colors.text.primary }}>{roomId || '--'}</span>
            </span>
            <span style={{ color: colors.text.muted }}>
              Seq: <span style={{ color: colors.text.primary }}>{lastSeq}</span>
            </span>
            <span style={{ color: colors.text.muted }}>
              Open: <span style={{ color: colors.text.primary }}>{openOrderCount}</span>
            </span>
            <span style={{ color: colors.text.muted }}>
              Events: <span style={{ color: colors.text.primary }}>{eventCount}</span>
            </span>
            <span style={{ color: colors.text.muted }}>
              Fills: <span style={{ color: colors.text.primary }}>{fillCount}</span>
            </span>
            <span style={{ color: colors.text.muted }}>
              Trades: <span style={{ color: colors.text.primary }}>{marketTradeCount}</span>
            </span>
            <span style={{ color: colors.text.muted }}>
              Points: <span style={{ color: colors.text.primary }}>{pricePointCount}</span>
            </span>
            <span style={{ color: colors.text.muted }}>
              Candles: <span style={{ color: colors.text.primary }}>{candleCount}</span>
            </span>
            <span style={{ color: colors.text.muted }}>
              Pending: <span style={{ color: colors.text.primary }}>{pendingDeltaCount}</span>
            </span>
            <span style={{ color: colors.text.muted }}>
              Last Sync: <span style={{ color: colors.text.primary }}>{lastSyncReason}</span>
            </span>
            <span style={{ color: colors.text.muted }}>
              Server: <span style={{ color: colors.text.primary }}>{lastServerTime}</span>
            </span>
            <span style={{ color: colors.text.muted }}>
              MD Age:{' '}
              <span
                style={{
                  color: staleData ? colors.semantic.warning.DEFAULT : colors.text.primary,
                }}
              >
                {mdAgeLabel}
              </span>
            </span>
            <span style={{ color: colors.text.muted }}>
              Focus:{' '}
              <span
                data-testid="order-view-focus-label"
                style={{
                  color: hasFocus ? colors.accent.DEFAULT : colors.text.primary,
                }}
              >
                {focusLabel}
              </span>
            </span>
            {loading ? (
              <span style={{ color: colors.semantic.warning.DEFAULT }}>Loading snapshot…</span>
            ) : null}
            {error ? (
              <span style={{ color: colors.semantic.danger.DEFAULT }}>Error: {error}</span>
            ) : null}
            {statusNotes.length > 0 ? (
              <span style={{ color: colors.text.muted }}>
                Notes: <span style={{ color: colors.text.primary }}>{statusNotes.join(', ')}</span>
              </span>
            ) : null}
          </section>

          <div className="min-h-0 flex-1 grid gap-3 lg:grid-cols-[2fr_1fr]">
            <section
              className="rounded border min-h-[260px] p-3"
              style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.surface }}
            >
              <h3 className="text-sm font-semibold mb-2" style={{ color: colors.text.primary }}>
                Chart
              </h3>
              <OrderViewChart
                leg={selection.leg}
                priceSeries={priceSeries}
                lifetimeSegments={lifetimeSegments}
                candles={candles}
                candleSource={candleSource}
                fills={visibleFills}
                focus={focus}
                showBids={selection.showBids}
                showAsks={selection.showAsks}
                showFills={selection.showFills}
                showBboLines={showBboLines}
                bestBid={selectedBestBid}
                bestAsk={selectedBestAsk}
              />
            </section>
            <section
              className="rounded border min-h-[260px] p-3"
              style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.surface }}
            >
              <div className="flex items-center justify-between gap-2 flex-wrap mb-2">
                <h3 className="text-sm font-semibold" style={{ color: colors.text.primary }}>
                  Market
                </h3>
                <div className="flex items-center gap-1">
                  <button
                    type="button"
                    onClick={() => setActiveRightTab('order_book')}
                    className="px-2 py-1 text-xs rounded border"
                    style={{
                      borderColor:
                        activeRightTab === 'order_book' ? colors.accent.DEFAULT : colors.border.DEFAULT,
                      color:
                        activeRightTab === 'order_book' ? colors.accent.DEFAULT : colors.text.secondary,
                    }}
                  >
                    Order Book
                  </button>
                  <button
                    type="button"
                    onClick={() => setActiveRightTab('recent_trades')}
                    className="px-2 py-1 text-xs rounded border"
                    style={{
                      borderColor:
                        activeRightTab === 'recent_trades' ? colors.accent.DEFAULT : colors.border.DEFAULT,
                      color:
                        activeRightTab === 'recent_trades' ? colors.accent.DEFAULT : colors.text.secondary,
                    }}
                  >
                    Recent Trades
                  </button>
                </div>
              </div>
              {activeRightTab === 'order_book' ? (
                <OrderViewL1Widget
                  bbo={bbo}
                  l2={l2}
                  openOrders={openOrders}
                  context={context}
                  status={status}
                  nowMs={nowMs}
                  staleThresholdMs={ORDER_VIEW_STALE_MD_MS}
                  showBids={selection.showBids}
                  showAsks={selection.showAsks}
                  focus={focus}
                  onLadderRowClick={handleLadderRowClick}
                />
              ) : null}
              {activeRightTab === 'recent_trades' ? (
                <OrderViewMarketTradesTable rows={marketTrades} autoScroll={tapeAutoScroll} />
              ) : null}
            </section>
          </div>

          <section
            className="rounded border min-h-[220px] p-3 flex flex-col gap-2"
            style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.surface }}
          >
            <div className="flex items-center justify-between gap-2 flex-wrap">
              <h3 className="text-sm font-semibold" style={{ color: colors.text.primary }}>
                Our Orders
              </h3>
              <div className="flex items-center gap-1 flex-wrap">
                <button
                  type="button"
                  onClick={() => setActiveBottomTab('open_orders')}
                  className="px-2 py-1 text-xs rounded border"
                  style={{
                    borderColor:
                      activeBottomTab === 'open_orders' ? colors.accent.DEFAULT : colors.border.DEFAULT,
                    color:
                      activeBottomTab === 'open_orders' ? colors.accent.DEFAULT : colors.text.secondary,
                  }}
                >
                  Open Orders
                </button>
                <button
                  type="button"
                  onClick={() => setActiveBottomTab('our_fills')}
                  className="px-2 py-1 text-xs rounded border"
                  style={{
                    borderColor: activeBottomTab === 'our_fills' ? colors.accent.DEFAULT : colors.border.DEFAULT,
                    color: activeBottomTab === 'our_fills' ? colors.accent.DEFAULT : colors.text.secondary,
                  }}
                >
                  Our Fills
                </button>
                <button
                  type="button"
                  onClick={() => setActiveBottomTab('our_order_events')}
                  className="px-2 py-1 text-xs rounded border"
                  style={{
                    borderColor:
                      activeBottomTab === 'our_order_events' ? colors.accent.DEFAULT : colors.border.DEFAULT,
                    color:
                      activeBottomTab === 'our_order_events' ? colors.accent.DEFAULT : colors.text.secondary,
                  }}
                >
                  Our Order Events
                </button>
                <label className="flex items-center gap-1 text-xs ml-2" style={{ color: colors.text.muted }}>
                  <input
                    type="checkbox"
                    checked={tapeAutoScroll}
                    onChange={(event) => setTapeAutoScroll(event.target.checked)}
                  />
                  Auto-scroll
                </label>
                <input
                  type="text"
                  value={tapeOrderSearch}
                  onChange={(event) => setTapeOrderSearch(event.target.value)}
                  placeholder="Order ID search"
                  className="px-2 py-1 rounded border text-xs min-w-[180px] bg-transparent"
                  style={{
                    borderColor: colors.border.DEFAULT,
                    color: colors.text.primary,
                  }}
                />
                {hasFocus ? (
                  <button
                    type="button"
                    onClick={clearFocus}
                    className="px-2 py-1 text-xs rounded border"
                    style={{
                      borderColor: colors.accent.DEFAULT,
                      color: colors.accent.DEFAULT,
                    }}
                  >
                    Clear Focus
                  </button>
                ) : null}
              </div>
            </div>
            <div className="min-h-0 flex-1">
              {activeBottomTab === 'open_orders' ? (
                <OrderViewOpenOrdersTable
                  rows={openOrders}
                  showBids={selection.showBids}
                  showAsks={selection.showAsks}
                />
              ) : null}
              {activeBottomTab === 'our_fills' ? (
                <OrderViewFillsTable rows={visibleFills} />
              ) : null}
              {activeBottomTab === 'our_order_events' ? (
                <OrderViewEventsTable
                  rows={events}
                  autoScroll={tapeAutoScroll}
                  orderSearch={tapeOrderSearch}
                  focus={focus}
                  onRowClick={handleEventRowClick}
                />
              ) : null}
            </div>
          </section>
        </div>
      </div>
    </PageShell>
  );
}
