import { create } from 'zustand';
import { useResyncStore } from '@/stores';
import type {
  OrderViewBbo,
  OrderViewCandleRow,
  OrderViewContext,
  OrderViewDelta,
  OrderViewEvent,
  OrderViewLeg,
  OrderViewL2Snapshot,
  OrderViewMarketTradeRow,
  OrderViewOpenOrder,
  OrderViewSnapshot,
  OrderViewStatus,
} from '@/types';

export const ORDER_VIEW_EVENTS_CAP = 10_000;
export const ORDER_VIEW_FILLS_CAP = 10_000;
export const ORDER_VIEW_MARKET_TRADES_CAP = 10_000;
export const ORDER_VIEW_CANDLES_CAP = 14_400;
export const ORDER_VIEW_L2_DEPTH_CAP = 50;
export const ORDER_VIEW_LIFETIME_SEGMENTS_CAP = ORDER_VIEW_EVENTS_CAP;
export const ORDER_VIEW_BUFFER_CAP = ORDER_VIEW_EVENTS_CAP;
export const ORDER_VIEW_PRICE_MAX_POINTS = 20000;
const ORDER_VIEW_PRICE_BUCKET_MS = 1_000;

const ORDER_VIEW_RANGE_MS = {
  '5m': 5 * 60 * 1000,
  '15m': 15 * 60 * 1000,
  '1h': 60 * 60 * 1000,
} as const;

export type OrderViewTimeRange = keyof typeof ORDER_VIEW_RANGE_MS;

export type OrderViewSelectionState = {
  strategyId: string;
  leg: OrderViewLeg;
  timeRange: OrderViewTimeRange;
  paused: boolean;
  showBids: boolean;
  showAsks: boolean;
  showFills: boolean;
};

export type OrderViewFocusSide = 'bid' | 'ask';

export type OrderViewFocusState = {
  orderKey: string | null;
  eventKey: string | null;
  side: OrderViewFocusSide | null;
  price: number | null;
};

type OrderViewFocusInput = {
  orderKey?: unknown;
  eventKey?: unknown;
  side?: unknown;
  price?: unknown;
};

export type OrderViewPricePoint = {
  ts_ms: number;
  maker_mid?: number;
  maker_bid?: number;
  maker_ask?: number;
  hedge_mid?: number;
  hedge_bid?: number;
  hedge_ask?: number;
};

export type OrderViewLifetimeSegmentCloseReason = 'open' | 'fill' | 'cancel' | 'unknown';

export type OrderViewLifetimeSegment = {
  segment_id: string;
  order_key: string;
  side: 'bid' | 'ask' | null;
  price: number;
  start_ts_ms: number;
  end_ts_ms: number | null;
  close_reason: OrderViewLifetimeSegmentCloseReason;
  lifetime_start_unknown: boolean;
};

export type OrderViewApplySnapshotResult = {
  accepted: boolean;
  applied: boolean;
  staleRejected: boolean;
};

export type OrderViewApplyDeltaResult = {
  accepted: boolean;
  applied: boolean;
  staleRejected: boolean;
  seqGap: boolean;
  needsResync: boolean;
  queued: boolean;
};

type OrderViewStoreState = {
  selection: OrderViewSelectionState;
  focus: OrderViewFocusState;
  roomId: string | null;
  context: OrderViewContext;
  snapshotId: string;
  stateRev: string;
  lastSeq: number;
  needsResync: boolean;
  pendingDeltaCount: number;
  appliedResyncId: number;
  openOrdersById: Record<string, OrderViewOpenOrder>;
  openOrderIds: string[];
  events: OrderViewEvent[];
  fills: OrderViewEvent[];
  lifetimeSegments: OrderViewLifetimeSegment[];
  marketTrades: OrderViewMarketTradeRow[];
  candles: OrderViewCandleRow[];
  candleSource: string | null;
  l2: OrderViewL2Snapshot | null;
  priceSeries: OrderViewPricePoint[];
  bbo: {
    maker?: OrderViewBbo;
    hedge?: OrderViewBbo;
  };
  status: OrderViewStatus | null;
  lastServerTsMs: number | null;
  lastSnapshotTsMs: number | null;
  applySnapshot: (snapshot: OrderViewSnapshot, resyncId?: number) => OrderViewApplySnapshotResult;
  applyDelta: (delta: OrderViewDelta, resyncId?: number) => OrderViewApplyDeltaResult;
  clear: () => void;
  setSelection: (patch: Partial<OrderViewSelectionState>) => void;
  setFocus: (focus: OrderViewFocusInput) => void;
  clearFocus: () => void;
  clearNeedsResync: () => void;
};

const defaultContext = (): OrderViewContext => ({
  maker: { exchange: null, symbol: null },
  hedge: { exchange: null, symbol: null },
});

const defaultSelection = (): OrderViewSelectionState => ({
  strategyId: '',
  leg: 'maker',
  timeRange: '15m',
  paused: false,
  showBids: true,
  showAsks: true,
  showFills: true,
});

const defaultFocus = (): OrderViewFocusState => ({
  orderKey: null,
  eventKey: null,
  side: null,
  price: null,
});

const coercePositiveInt = (value: unknown): number | undefined => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return undefined;
  }
  return Math.trunc(parsed);
};

const normalizeFocusText = (value: unknown): string | null => {
  const text = String(value || '').trim();
  return text ? text : null;
};

const normalizeFocusSide = (value: unknown): OrderViewFocusSide | null => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (text === 'bid' || text === 'buy') return 'bid';
  if (text === 'ask' || text === 'sell') return 'ask';
  return null;
};

const normalizeFocusPrice = (value: unknown): number | null => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  return parsed;
};

const normalizeFocus = (focus: OrderViewFocusInput): OrderViewFocusState => ({
  orderKey: normalizeFocusText(focus.orderKey),
  eventKey: normalizeFocusText(focus.eventKey),
  side: normalizeFocusSide(focus.side),
  price: normalizeFocusPrice(focus.price),
});

const sameFocus = (lhs: OrderViewFocusState, rhs: OrderViewFocusState): boolean =>
  lhs.orderKey === rhs.orderKey &&
  lhs.eventKey === rhs.eventKey &&
  lhs.side === rhs.side &&
  lhs.price === rhs.price;

const resolveResyncFloor = (appliedResyncId: number): number => {
  const globalId = coercePositiveInt(useResyncStore.getState().resyncId) ?? 0;
  return Math.max(appliedResyncId, globalId);
};

const isStaleResync = (incomingResyncId: number | undefined, floorResyncId: number): boolean =>
  incomingResyncId !== undefined && incomingResyncId < floorResyncId;

const toOrderRowId = (row: OrderViewOpenOrder, index: number): string => {
  const level = Number(row.level);
  const levelToken = Number.isFinite(level) && level > 0 ? `lvl-${Math.trunc(level)}` : '';
  return (
    row.order_row_id ||
    `${row.leg}:${row.side}:${row.level}:${row.client_order_id || row.order_id || levelToken || row.px || index}`
  );
};

const hasFiniteNumericText = (value: unknown): boolean => {
  if (value === null || value === undefined) return false;
  const text = String(value).trim();
  if (!text || text === '--') return false;
  return Number.isFinite(Number(text));
};

const isActionableOpenOrderRow = (row: OrderViewOpenOrder): boolean => {
  const hasOrderIdentity = Boolean(String(row.client_order_id || row.order_id || '').trim());
  const hasPrice = hasFiniteNumericText(row.px);
  const hasQty = hasFiniteNumericText(row.rem_qty);
  return hasOrderIdentity || hasPrice || hasQty;
};

const normalizeOpenOrders = (rows: OrderViewOpenOrder[] | undefined) => {
  const openOrdersById: Record<string, OrderViewOpenOrder> = {};
  const openOrderIds: string[] = [];
  if (!Array.isArray(rows)) {
    return { openOrdersById, openOrderIds };
  }
  for (let index = 0; index < rows.length; index += 1) {
    const row = rows[index];
    if (!row || typeof row !== 'object') {
      continue;
    }
    if (!isActionableOpenOrderRow(row)) {
      continue;
    }
    const rowId = toOrderRowId(row, index);
    if (!rowId || openOrdersById[rowId]) {
      continue;
    }
    openOrdersById[rowId] = { ...row, order_row_id: rowId };
    openOrderIds.push(rowId);
  }
  return { openOrdersById, openOrderIds };
};

const eventKeyOf = (row: OrderViewEvent, index: number): string =>
  String(row.event_key || `${row.type || 'unknown'}:${row.ts_ms ?? 'na'}:${index}`);

const seedEvents = (rows: OrderViewEvent[] | undefined): OrderViewEvent[] => {
  if (!Array.isArray(rows) || rows.length === 0) {
    return [];
  }
  const out: OrderViewEvent[] = [];
  const seen = new Set<string>();
  for (let index = 0; index < rows.length; index += 1) {
    const row = rows[index];
    if (!row || typeof row !== 'object') {
      continue;
    }
    const eventKey = eventKeyOf(row, index);
    if (seen.has(eventKey)) {
      continue;
    }
    seen.add(eventKey);
    out.push({ ...row, event_key: eventKey });
    if (out.length >= ORDER_VIEW_BUFFER_CAP) {
      break;
    }
  }
  return out;
};

const appendEvents = (current: OrderViewEvent[], incoming: OrderViewEvent[] | undefined): OrderViewEvent[] => {
  if (!Array.isArray(incoming) || incoming.length === 0) {
    return current;
  }
  const next = current.slice();
  const seen = new Set<string>(next.map((row, index) => eventKeyOf(row, index)));
  for (let index = 0; index < incoming.length; index += 1) {
    const row = incoming[index];
    if (!row || typeof row !== 'object') {
      continue;
    }
    const eventKey = eventKeyOf(row, index);
    if (seen.has(eventKey)) {
      continue;
    }
    seen.add(eventKey);
    next.unshift({ ...row, event_key: eventKey });
    if (next.length > ORDER_VIEW_EVENTS_CAP) {
      next.length = ORDER_VIEW_EVENTS_CAP;
    }
  }
  return next;
};

const mergeSnapshotEvents = (
  snapshotRows: OrderViewEvent[] | undefined,
  current: OrderViewEvent[],
  cap: number
): OrderViewEvent[] => {
  const merged = seedEvents(snapshotRows);
  if (merged.length >= cap || current.length === 0) {
    return merged.slice(0, cap);
  }
  const seen = new Set<string>(merged.map((row, index) => eventKeyOf(row, index)));
  for (let index = 0; index < current.length; index += 1) {
    const row = current[index];
    if (!row || typeof row !== 'object') {
      continue;
    }
    const eventKey = eventKeyOf(row, index);
    if (seen.has(eventKey)) {
      continue;
    }
    seen.add(eventKey);
    merged.push({ ...row, event_key: eventKey });
    if (merged.length >= cap) {
      break;
    }
  }
  return merged.slice(0, cap);
};

const appendFillEvents = (current: OrderViewEvent[], incoming: OrderViewEvent[] | undefined): OrderViewEvent[] => {
  if (!Array.isArray(incoming) || incoming.length === 0) {
    return current;
  }
  const filtered = incoming.filter((row) => isFillEvent(row as OrderViewEvent)) as OrderViewEvent[];
  if (filtered.length === 0) {
    return current;
  }
  const next = appendEvents(current, filtered);
  if (next.length > ORDER_VIEW_FILLS_CAP) {
    return next.slice(0, ORDER_VIEW_FILLS_CAP);
  }
  return next;
};

const tradeKeyOf = (row: OrderViewMarketTradeRow): string => {
  const tradeId = row.trade_id == null ? '' : String(row.trade_id).trim();
  if (tradeId) return `id:${tradeId}`;
  return `fp:${row.ts_ms}:${String(row.price)}:${String(row.qty)}:${String(row.side ?? '')}`;
};

const normalizeTradeTsMs = (value: unknown): number | null => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  return Math.trunc(parsed);
};

const normalizeMarketTradeRow = (row: OrderViewMarketTradeRow | Record<string, unknown>): OrderViewMarketTradeRow | null => {
  if (!row || typeof row !== 'object') return null;
  const tsMs = normalizeTradeTsMs((row as Record<string, unknown>).ts_ms);
  if (tsMs == null) return null;
  const price = String((row as Record<string, unknown>).price ?? '').trim();
  const qty = String((row as Record<string, unknown>).qty ?? '').trim();
  if (!price || !qty) return null;
  const sideRaw = (row as Record<string, unknown>).side;
  const side = sideRaw == null ? null : String(sideRaw).trim().toLowerCase();
  const tradeIdRaw = (row as Record<string, unknown>).trade_id;
  const tradeId = tradeIdRaw == null ? null : String(tradeIdRaw).trim();
  return {
    trade_id: tradeId || null,
    ts_ms: tsMs,
    side: side || null,
    price,
    qty,
  };
};

const appendMarketTrades = (
  current: OrderViewMarketTradeRow[],
  incoming: OrderViewMarketTradeRow[] | undefined
): OrderViewMarketTradeRow[] => {
  if (!Array.isArray(incoming) || incoming.length === 0) return current;
  const next = current.slice();
  const seen = new Set<string>(next.map((row) => tradeKeyOf(row)));
  // Iterate backward so prepending preserves server order at the front.
  for (let index = incoming.length - 1; index >= 0; index -= 1) {
    const normalized = normalizeMarketTradeRow(incoming[index] as any);
    if (!normalized) continue;
    const key = tradeKeyOf(normalized);
    if (seen.has(key)) continue;
    seen.add(key);
    next.unshift(normalized);
    if (next.length > ORDER_VIEW_MARKET_TRADES_CAP) {
      next.length = ORDER_VIEW_MARKET_TRADES_CAP;
    }
  }
  return next;
};

const normalizeCandleRow = (row: OrderViewCandleRow | Record<string, unknown>): OrderViewCandleRow | null => {
  if (!row || typeof row !== 'object') return null;
  const tsMs = normalizeTradeTsMs((row as Record<string, unknown>).ts_ms);
  if (tsMs == null) return null;
  const open = Number((row as Record<string, unknown>).open);
  const high = Number((row as Record<string, unknown>).high);
  const low = Number((row as Record<string, unknown>).low);
  const close = Number((row as Record<string, unknown>).close);
  const volumeRaw = Number((row as Record<string, unknown>).volume);
  const volume = Number.isFinite(volumeRaw) ? Math.max(0, volumeRaw) : 0;
  if (!Number.isFinite(open) || !Number.isFinite(high) || !Number.isFinite(low) || !Number.isFinite(close)) {
    return null;
  }
  return {
    ts_ms: tsMs,
    open,
    high,
    low,
    close,
    volume,
  };
};

const reconcileCandleRows = (
  current: OrderViewCandleRow[],
  incoming: OrderViewCandleRow[] | undefined
): OrderViewCandleRow[] => {
  if ((!Array.isArray(incoming) || incoming.length === 0) && current.length === 0) {
    return [];
  }
  const byTsMs = new Map<number, OrderViewCandleRow>();
  for (const row of current) {
    const normalized = normalizeCandleRow(row as any);
    if (!normalized) continue;
    byTsMs.set(normalized.ts_ms, normalized);
  }
  if (Array.isArray(incoming)) {
    for (const row of incoming) {
      const normalized = normalizeCandleRow(row as any);
      if (!normalized) continue;
      // Snapshot/delta rows should win on ts collisions.
      byTsMs.set(normalized.ts_ms, normalized);
    }
  }
  const deduped = Array.from(byTsMs.values()).sort((lhs, rhs) => lhs.ts_ms - rhs.ts_ms);
  if (deduped.length > ORDER_VIEW_CANDLES_CAP) {
    return deduped.slice(deduped.length - ORDER_VIEW_CANDLES_CAP);
  }
  return deduped;
};

const findCandleInsertionIndex = (
  candles: OrderViewCandleRow[],
  tsMs: number
): { index: number; found: boolean } => {
  let lo = 0;
  let hi = candles.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >>> 1;
    const midTsMs = candles[mid].ts_ms;
    if (midTsMs === tsMs) {
      return { index: mid, found: true };
    }
    if (midTsMs < tsMs) {
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return { index: lo, found: false };
};

const sameCandle = (lhs: OrderViewCandleRow, rhs: OrderViewCandleRow): boolean =>
  lhs.ts_ms === rhs.ts_ms &&
  lhs.open === rhs.open &&
  lhs.high === rhs.high &&
  lhs.low === rhs.low &&
  lhs.close === rhs.close &&
  lhs.volume === rhs.volume;

const upsertCandleSorted = (
  current: OrderViewCandleRow[],
  candidate: OrderViewCandleRow | null
): OrderViewCandleRow[] => {
  if (!candidate) return current;
  if (current.length === 0) return [candidate];

  const last = current[current.length - 1];
  if (candidate.ts_ms > last.ts_ms) {
    const next = current.concat(candidate);
    if (next.length > ORDER_VIEW_CANDLES_CAP) {
      return next.slice(next.length - ORDER_VIEW_CANDLES_CAP);
    }
    return next;
  }

  if (candidate.ts_ms === last.ts_ms) {
    if (sameCandle(last, candidate)) return current;
    const next = current.slice();
    next[next.length - 1] = candidate;
    return next;
  }

  const { index, found } = findCandleInsertionIndex(current, candidate.ts_ms);
  if (found) {
    if (sameCandle(current[index], candidate)) return current;
    const next = current.slice();
    next[index] = candidate;
    return next;
  }

  if (current.length >= ORDER_VIEW_CANDLES_CAP && index <= 0) {
    // Candidate is older than our oldest candle and would be trimmed immediately.
    return current;
  }
  const next = current.slice();
  next.splice(index, 0, candidate);
  if (next.length > ORDER_VIEW_CANDLES_CAP) {
    return next.slice(next.length - ORDER_VIEW_CANDLES_CAP);
  }
  return next;
};

const appendCandlesFromDelta = (
  current: OrderViewCandleRow[],
  incoming: { rows?: OrderViewCandleRow[]; candle_closed?: OrderViewCandleRow; candle_current?: OrderViewCandleRow } | undefined
): OrderViewCandleRow[] => {
  if (!incoming || typeof incoming !== 'object') return current;
  let next = current;
  if (Array.isArray(incoming.rows) && incoming.rows.length > 0) {
    next = reconcileCandleRows(next, incoming.rows);
  }
  next = upsertCandleSorted(next, normalizeCandleRow(incoming.candle_closed as any));
  next = upsertCandleSorted(next, normalizeCandleRow(incoming.candle_current as any));
  return next;
};

const toL2Level = (row: Record<string, unknown>): { px: string; qty: string; size?: number } | null => {
  if (!row || typeof row !== 'object') return null;
  const px = String(row.px ?? '').trim();
  const qty = String(row.qty ?? '').trim();
  if (!px || !qty) return null;
  const sizeNum = Number(row.size);
  return {
    px,
    qty,
    ...(Number.isFinite(sizeNum) ? { size: sizeNum } : {}),
  };
};

const normalizeL2Snapshot = (l2: OrderViewL2Snapshot | undefined): OrderViewL2Snapshot | null => {
  if (!l2 || typeof l2 !== 'object') return null;
  const topNRaw = Number((l2 as Record<string, unknown>).top_n);
  const topN = Number.isFinite(topNRaw) ? Math.max(1, Math.min(Math.trunc(topNRaw), ORDER_VIEW_L2_DEPTH_CAP)) : ORDER_VIEW_L2_DEPTH_CAP;
  const bidsRaw = Array.isArray((l2 as Record<string, unknown>).bids) ? ((l2 as Record<string, unknown>).bids as Record<string, unknown>[]) : [];
  const asksRaw = Array.isArray((l2 as Record<string, unknown>).asks) ? ((l2 as Record<string, unknown>).asks as Record<string, unknown>[]) : [];
  const bids = bidsRaw.map((row) => toL2Level(row)).filter(Boolean).slice(0, topN) as Array<{ px: string; qty: string; size?: number }>;
  const asks = asksRaw.map((row) => toL2Level(row)).filter(Boolean).slice(0, topN) as Array<{ px: string; qty: string; size?: number }>;
  const spreadAbsRaw = Number((l2 as Record<string, unknown>).spread_abs);
  const spreadBpsRaw = Number((l2 as Record<string, unknown>).spread_bps);
  return {
    bids,
    asks,
    top_n: topN,
    spread_abs: Number.isFinite(spreadAbsRaw) ? spreadAbsRaw : null,
    spread_bps: Number.isFinite(spreadBpsRaw) ? spreadBpsRaw : null,
    semantic: (l2 as Record<string, unknown>).semantic as any,
  };
};

type LifetimeSegmentDraft = {
  orderKey: string;
  side: 'bid' | 'ask' | null;
  price: number | null;
  startTsMs: number | null;
  endTsMs: number | null;
  closeReason: OrderViewLifetimeSegmentCloseReason;
  lifetimeStartUnknown: boolean;
};

const normalizeOrderSide = (value: unknown): 'bid' | 'ask' | null => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (text === 'bid' || text === 'buy') return 'bid';
  if (text === 'ask' || text === 'sell') return 'ask';
  return null;
};

const toOrderEventKey = (event: OrderViewEvent): string => {
  const orderId = String(event.order_id || '').trim();
  if (orderId) return orderId;
  const clientOrderId = String(event.client_order_id || '').trim();
  if (clientOrderId) return clientOrderId;
  return '';
};

const toOrderOpenKey = (row: OrderViewOpenOrder, fallbackRowId: string): string => {
  const orderId = String(row.order_id || '').trim();
  if (orderId) return orderId;
  const clientOrderId = String(row.client_order_id || '').trim();
  if (clientOrderId) return clientOrderId;
  return String(fallbackRowId || '').trim();
};

const isPlacementEventType = (value: unknown): boolean => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  return text.includes('place');
};

const closeReasonFromEventType = (value: unknown): OrderViewLifetimeSegmentCloseReason | null => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (!text) return null;
  if (text === 'fill' || text.includes('fill')) return 'fill';
  if (text.includes('cancel')) return 'cancel';
  return null;
};

const buildLifetimeSegments = (
  openOrdersById: Record<string, OrderViewOpenOrder>,
  openOrderIds: string[],
  events: OrderViewEvent[],
  fallbackStartTsMs: number | null = null
): OrderViewLifetimeSegment[] => {
  const drafts = new Map<string, LifetimeSegmentDraft>();
  const ensureDraft = (orderKey: string): LifetimeSegmentDraft => {
    const existing = drafts.get(orderKey);
    if (existing) return existing;
    const next: LifetimeSegmentDraft = {
      orderKey,
      side: null,
      price: null,
      startTsMs: null,
      endTsMs: null,
      closeReason: 'open',
      lifetimeStartUnknown: false,
    };
    drafts.set(orderKey, next);
    return next;
  };

  const chronologicalEvents = events
    .map((event, index) => ({
      event,
      index,
      tsMs: normalizeTradeTsMs(event.ts_ms),
    }))
    .filter((row) => row.tsMs != null)
    .sort((lhs, rhs) => {
      const lhsTs = lhs.tsMs ?? 0;
      const rhsTs = rhs.tsMs ?? 0;
      if (lhsTs !== rhsTs) return lhsTs - rhsTs;
      return lhs.index - rhs.index;
    });

  for (const row of chronologicalEvents) {
    const event = row.event;
    const tsMs = row.tsMs as number;
    const orderKey = toOrderEventKey(event);
    if (!orderKey) continue;
    const draft = ensureDraft(orderKey);
    if (isPlacementEventType(event.type)) {
      if (draft.startTsMs == null || tsMs < draft.startTsMs) {
        draft.startTsMs = tsMs;
      }
      if (draft.closeReason === 'unknown') {
        draft.closeReason = 'open';
      }
    }
    const closeReason = closeReasonFromEventType(event.type);
    if (closeReason && draft.endTsMs == null) {
      if (draft.startTsMs == null) {
        draft.startTsMs = tsMs;
        draft.lifetimeStartUnknown = true;
      }
      draft.endTsMs = tsMs;
      draft.closeReason = closeReason;
    }
    if (draft.side == null) {
      draft.side = normalizeOrderSide(event.side);
    }
    if (draft.price == null) {
      const maybePrice = Number(event.px);
      if (Number.isFinite(maybePrice) && maybePrice > 0) {
        draft.price = maybePrice;
      }
    }
  }

  for (const rowId of openOrderIds) {
    const row = openOrdersById[rowId];
    if (!row) continue;
    const orderKey = toOrderOpenKey(row, rowId);
    if (!orderKey) continue;
    const draft = ensureDraft(orderKey);
    const createdTsMs = normalizeTradeTsMs(row.created_ts_ms);
    if (createdTsMs != null && (draft.startTsMs == null || createdTsMs < draft.startTsMs)) {
      draft.startTsMs = createdTsMs;
    } else if (createdTsMs == null && draft.startTsMs == null && fallbackStartTsMs != null) {
      draft.startTsMs = fallbackStartTsMs;
      draft.lifetimeStartUnknown = true;
    }
    if (row.lifetime_start_unknown === true) {
      draft.lifetimeStartUnknown = true;
    }
    if (draft.side == null) {
      draft.side = normalizeOrderSide(row.side);
    }
    if (draft.price == null) {
      const maybePrice = Number(row.px);
      if (Number.isFinite(maybePrice) && maybePrice > 0) {
        draft.price = maybePrice;
      }
    }
    if (draft.endTsMs == null) {
      draft.closeReason = 'open';
    }
  }

  const segments: OrderViewLifetimeSegment[] = [];
  for (const draft of drafts.values()) {
    if (draft.startTsMs == null || draft.price == null) {
      continue;
    }
    const normalizedEndTsMs =
      draft.endTsMs == null ? null : Math.max(draft.startTsMs, draft.endTsMs);
    segments.push({
      segment_id: `${draft.orderKey}:${draft.startTsMs}:${normalizedEndTsMs ?? 'open'}`,
      order_key: draft.orderKey,
      side: draft.side,
      price: draft.price,
      start_ts_ms: draft.startTsMs,
      end_ts_ms: normalizedEndTsMs,
      close_reason:
        normalizedEndTsMs == null
          ? 'open'
          : draft.closeReason === 'open'
            ? 'unknown'
            : draft.closeReason,
      lifetime_start_unknown: draft.lifetimeStartUnknown,
    });
  }
  segments.sort((lhs, rhs) => {
    if (lhs.start_ts_ms !== rhs.start_ts_ms) return lhs.start_ts_ms - rhs.start_ts_ms;
    return lhs.order_key.localeCompare(rhs.order_key);
  });
  if (segments.length > ORDER_VIEW_LIFETIME_SEGMENTS_CAP) {
    return segments.slice(segments.length - ORDER_VIEW_LIFETIME_SEGMENTS_CAP);
  }
  return segments;
};

const isFillEvent = (event: OrderViewEvent): boolean => String(event.type || '').toLowerCase() === 'fill';

const numberOrUndefined = (value: unknown): number | undefined => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : undefined;
};

const bboTs = (bbo: { maker?: OrderViewBbo; hedge?: OrderViewBbo }, fallbackTs: number): number => {
  const makerTs = numberOrUndefined(bbo?.maker?.ts_ms);
  const hedgeTs = numberOrUndefined(bbo?.hedge?.ts_ms);
  return Math.max(fallbackTs, makerTs ?? 0, hedgeTs ?? 0);
};

const buildPricePoint = (
  bbo: { maker?: OrderViewBbo; hedge?: OrderViewBbo } | undefined,
  serverTsMs: number | null
): OrderViewPricePoint | null => {
  if (!bbo) {
    return null;
  }
  const makerMid = numberOrUndefined(bbo.maker?.mid);
  const hedgeMid = numberOrUndefined(bbo.hedge?.mid);
  if (makerMid === undefined && hedgeMid === undefined) {
    return null;
  }
  const fallbackTs = serverTsMs ?? Date.now();
  return {
    ts_ms: bboTs(bbo, fallbackTs),
    maker_mid: makerMid,
    maker_bid: numberOrUndefined(bbo.maker?.bid),
    maker_ask: numberOrUndefined(bbo.maker?.ask),
    hedge_mid: hedgeMid,
    hedge_bid: numberOrUndefined(bbo.hedge?.bid),
    hedge_ask: numberOrUndefined(bbo.hedge?.ask),
  };
};

const toBucketTsMs = (tsMs: number): number =>
  Math.trunc(Math.max(0, tsMs) / ORDER_VIEW_PRICE_BUCKET_MS) * ORDER_VIEW_PRICE_BUCKET_MS;

const findFirstPointAtOrAfter = (points: OrderViewPricePoint[], cutoffTsMs: number): number => {
  let lo = 0;
  let hi = points.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (points[mid].ts_ms < cutoffTsMs) {
      lo = mid + 1;
    } else {
      hi = mid;
    }
  }
  return lo;
};

const samePricePoint = (lhs: OrderViewPricePoint, rhs: OrderViewPricePoint): boolean =>
  lhs.ts_ms === rhs.ts_ms &&
  lhs.maker_mid === rhs.maker_mid &&
  lhs.maker_bid === rhs.maker_bid &&
  lhs.maker_ask === rhs.maker_ask &&
  lhs.hedge_mid === rhs.hedge_mid &&
  lhs.hedge_bid === rhs.hedge_bid &&
  lhs.hedge_ask === rhs.hedge_ask;

const appendPricePoint = (
  current: OrderViewPricePoint[],
  nextPoint: OrderViewPricePoint | null,
  timeRange: OrderViewTimeRange
): OrderViewPricePoint[] => {
  if (!nextPoint) {
    return current;
  }

  const nextTsMs = numberOrUndefined(nextPoint.ts_ms);
  if (nextTsMs === undefined || nextTsMs <= 0) {
    return current;
  }
  const bucketedPoint: OrderViewPricePoint = {
    ...nextPoint,
    ts_ms: toBucketTsMs(nextTsMs),
  };

  const lastPoint = current[current.length - 1];
  if (lastPoint && bucketedPoint.ts_ms < lastPoint.ts_ms) {
    return current;
  }
  let nextSeries = current;
  if (lastPoint && lastPoint.ts_ms === bucketedPoint.ts_ms) {
    if (samePricePoint(lastPoint, bucketedPoint)) {
      nextSeries = current;
    } else {
      nextSeries = current.slice(0, -1);
      nextSeries.push(bucketedPoint);
    }
  } else {
    nextSeries = current.concat(bucketedPoint);
  }

  if (nextSeries.length === 0) {
    return nextSeries;
  }

  const latestTsMs = nextSeries[nextSeries.length - 1].ts_ms;
  const cutoffTsMs = latestTsMs - ORDER_VIEW_RANGE_MS[timeRange];
  const start = findFirstPointAtOrAfter(nextSeries, cutoffTsMs);
  const needsRangeTrim = start > 0;
  const needsPointTrim = nextSeries.length > ORDER_VIEW_PRICE_MAX_POINTS;
  if (!needsRangeTrim && !needsPointTrim) {
    return nextSeries;
  }

  let trimmed = needsRangeTrim ? nextSeries.slice(start) : nextSeries;
  if (trimmed.length > ORDER_VIEW_PRICE_MAX_POINTS) {
    trimmed = trimmed.slice(trimmed.length - ORDER_VIEW_PRICE_MAX_POINTS);
  }
  return trimmed;
};

const trimPriceSeriesForRange = (
  current: OrderViewPricePoint[],
  timeRange: OrderViewTimeRange
): OrderViewPricePoint[] => {
  if (current.length === 0) {
    return current;
  }
  const latestTs = current[current.length - 1].ts_ms;
  const cutoff = latestTs - ORDER_VIEW_RANGE_MS[timeRange];
  const start = findFirstPointAtOrAfter(current, cutoff);
  const needsRangeTrim = start > 0;
  const needsPointTrim = current.length > ORDER_VIEW_PRICE_MAX_POINTS;
  if (!needsRangeTrim && !needsPointTrim) {
    return current;
  }

  let trimmed = needsRangeTrim ? current.slice(start) : current.slice();
  if (trimmed.length > ORDER_VIEW_PRICE_MAX_POINTS) {
    trimmed = trimmed.slice(trimmed.length - ORDER_VIEW_PRICE_MAX_POINTS);
  }
  return trimmed;
};

export const useOrderViewStore = create<OrderViewStoreState>((set) => ({
  selection: defaultSelection(),
  focus: defaultFocus(),
  roomId: null,
  context: defaultContext(),
  snapshotId: '',
  stateRev: '',
  lastSeq: 0,
  needsResync: false,
  pendingDeltaCount: 0,
  appliedResyncId: 0,
  openOrdersById: {},
  openOrderIds: [],
  events: [],
  fills: [],
  lifetimeSegments: [],
  marketTrades: [],
  candles: [],
  candleSource: null,
  l2: null,
  priceSeries: [],
  bbo: {},
  status: null,
  lastServerTsMs: null,
  lastSnapshotTsMs: null,

  clear: () =>
    set((state) => ({
      ...state,
      focus: defaultFocus(),
      roomId: null,
      context: defaultContext(),
      snapshotId: '',
      stateRev: '',
      lastSeq: 0,
      needsResync: false,
      pendingDeltaCount: 0,
      appliedResyncId: 0,
      openOrdersById: {},
      openOrderIds: [],
      events: [],
      fills: [],
      lifetimeSegments: [],
      marketTrades: [],
      candles: [],
      candleSource: null,
      l2: null,
      priceSeries: [],
      bbo: {},
      status: null,
      lastServerTsMs: null,
      lastSnapshotTsMs: null,
    })),

  setSelection: (patch) =>
    set((state) => {
      const nextSelection = { ...state.selection, ...patch };
      const selectionChanged =
        nextSelection.strategyId !== state.selection.strategyId ||
        nextSelection.leg !== state.selection.leg;
      if (selectionChanged) {
        return {
          ...state,
          selection: nextSelection,
          focus: defaultFocus(),
          roomId: null,
          context: defaultContext(),
          snapshotId: '',
          stateRev: '',
          lastSeq: 0,
          needsResync: false,
          pendingDeltaCount: 0,
          openOrdersById: {},
          openOrderIds: [],
          events: [],
          fills: [],
          lifetimeSegments: [],
          marketTrades: [],
          candles: [],
          candleSource: null,
          l2: null,
          priceSeries: [],
          bbo: {},
          status: null,
          lastServerTsMs: null,
          lastSnapshotTsMs: null,
        };
      }
      if (nextSelection.timeRange !== state.selection.timeRange) {
        return {
          ...state,
          selection: nextSelection,
          priceSeries: trimPriceSeriesForRange(state.priceSeries, nextSelection.timeRange),
        };
      }
      return { ...state, selection: nextSelection };
    }),

  setFocus: (focus) =>
    set((state) => {
      const nextFocus = normalizeFocus(focus);
      if (sameFocus(state.focus, nextFocus)) {
        return state;
      }
      return { ...state, focus: nextFocus };
    }),

  clearFocus: () =>
    set((state) => {
      const cleared = defaultFocus();
      if (sameFocus(state.focus, cleared)) {
        return state;
      }
      return { ...state, focus: cleared };
    }),

  clearNeedsResync: () => set((state) => ({ ...state, needsResync: false, pendingDeltaCount: 0 })),

  applySnapshot: (snapshot, resyncId) => {
    const result: OrderViewApplySnapshotResult = {
      accepted: false,
      applied: false,
      staleRejected: false,
    };
    set((state) => {
      const incomingResyncId = coercePositiveInt(resyncId);
      const floorResyncId = resolveResyncFloor(state.appliedResyncId);
      if (isStaleResync(incomingResyncId, floorResyncId)) {
        result.staleRejected = true;
        return state;
      }
      const appliedResyncId =
        incomingResyncId !== undefined
          ? Math.max(state.appliedResyncId, incomingResyncId)
          : state.appliedResyncId;
      result.accepted = true;
      result.applied = true;

      const normalized = normalizeOpenOrders(snapshot.open_orders?.rows);
      const mergedEvents = mergeSnapshotEvents(snapshot.events?.rows, state.events, ORDER_VIEW_EVENTS_CAP);
      const mergedFills = mergeSnapshotEvents(
        Array.isArray(snapshot.events?.rows)
          ? (snapshot.events?.rows.filter((row) => isFillEvent(row as OrderViewEvent)) as OrderViewEvent[])
          : [],
        state.fills,
        ORDER_VIEW_FILLS_CAP
      );
      const serverTsMs =
        coercePositiveInt(snapshot.server_time_ms) ??
        coercePositiveInt(snapshot.server_ts_ms) ??
        Date.now();
      const mergedLifetimeSegments = buildLifetimeSegments(
        normalized.openOrdersById,
        normalized.openOrderIds,
        mergedEvents,
        serverTsMs
      );
      const mergedMarketTrades = appendMarketTrades(state.marketTrades, snapshot.market_trades?.rows);
      const mergedCandles = appendCandlesFromDelta(state.candles, snapshot.candles);
      const normalizedL2 = normalizeL2Snapshot(snapshot.l2);
      const snapshotId = String(snapshot.snapshot_id || '').trim();
      const snapshotSeq = coercePositiveInt(snapshot.last_seq) ?? coercePositiveInt(snapshot.seq) ?? 0;
      const nextPriceSeries = appendPricePoint(
        state.priceSeries,
        buildPricePoint(snapshot.bbo, serverTsMs),
        state.selection.timeRange
      );

      return {
        ...state,
        selection: {
          ...state.selection,
          strategyId: snapshot.selection?.strategy_id || state.selection.strategyId,
          leg: snapshot.selection?.leg || state.selection.leg,
        },
        roomId: snapshot.room_id || null,
        context: snapshot.context || defaultContext(),
        snapshotId,
        stateRev: snapshot.state_rev || '',
        lastSeq: snapshotSeq,
        needsResync: false,
        pendingDeltaCount: 0,
        appliedResyncId,
        openOrdersById: normalized.openOrdersById,
        openOrderIds: normalized.openOrderIds,
        events: mergedEvents,
        fills: mergedFills,
        lifetimeSegments: mergedLifetimeSegments,
        marketTrades: mergedMarketTrades,
        candles: mergedCandles,
        candleSource: snapshot.candles?.source ?? state.candleSource,
        l2: normalizedL2,
        priceSeries: nextPriceSeries,
        bbo: snapshot.bbo || {},
        status: snapshot.status || state.status,
        lastServerTsMs: serverTsMs,
        lastSnapshotTsMs: serverTsMs,
      };
    });
    return result;
  },

  applyDelta: (delta, resyncId) => {
    const result: OrderViewApplyDeltaResult = {
      accepted: false,
      applied: false,
      staleRejected: false,
      seqGap: false,
      needsResync: false,
      queued: false,
    };
    set((state) => {
      const incomingResyncId = coercePositiveInt(resyncId);
      const floorResyncId = resolveResyncFloor(state.appliedResyncId);
      if (isStaleResync(incomingResyncId, floorResyncId)) {
        result.staleRejected = true;
        return state;
      }
      const appliedResyncId =
        incomingResyncId !== undefined
          ? Math.max(state.appliedResyncId, incomingResyncId)
          : state.appliedResyncId;

      const activeStrategyId = String(state.selection.strategyId || '').trim();
      if (activeStrategyId) {
        const deltaStrategyId = String(delta.selection?.strategy_id || '').trim();
        const deltaLeg = String(delta.selection?.leg || '')
          .trim()
          .toLowerCase();
        const selectedLeg = String(state.selection.leg || '')
          .trim()
          .toLowerCase();
        if (!deltaStrategyId || deltaStrategyId !== activeStrategyId || deltaLeg !== selectedLeg) {
          return {
            ...state,
            appliedResyncId,
          };
        }
      }

      result.accepted = true;

      if (state.selection.paused) {
        result.queued = true;
        return {
          ...state,
          appliedResyncId,
          pendingDeltaCount: state.pendingDeltaCount + 1,
        };
      }

      const nextSeq = coercePositiveInt(delta.seq);
      const incomingSnapshotId = String(delta.snapshot_id || '').trim();
      const currentSnapshotId = String(state.snapshotId || '').trim();
      const epochChanged = incomingSnapshotId.length > 0 && incomingSnapshotId !== currentSnapshotId;
      if (nextSeq === undefined || nextSeq <= 0) {
        if (epochChanged) {
          result.needsResync = true;
          return {
            ...state,
            appliedResyncId,
            needsResync: true,
          };
        }
        return {
          ...state,
          appliedResyncId,
        };
      }
      const roomId = delta.room_id || state.roomId;
      if (state.roomId && roomId && roomId !== state.roomId) {
        return {
          ...state,
          appliedResyncId,
        };
      }

      const seqBase = epochChanged ? 0 : state.lastSeq;
      if (epochChanged && nextSeq > 1) {
        result.seqGap = true;
        result.needsResync = true;
        return {
          ...state,
          appliedResyncId,
          needsResync: true,
        };
      }
      if (seqBase > 0 && nextSeq > seqBase + 1) {
        result.seqGap = true;
        result.needsResync = true;
        return {
          ...state,
          appliedResyncId,
          needsResync: true,
        };
      }
      if (!epochChanged && nextSeq < state.lastSeq) {
        result.needsResync = true;
        return {
          ...state,
          appliedResyncId,
          needsResync: true,
        };
      }
      if (!epochChanged && nextSeq === state.lastSeq) {
        return {
          ...state,
          appliedResyncId,
        };
      }

      const incomingStateRev = String(delta.state_rev || '');
      const hasRefreshOpenOrders = delta.open_orders?.full_refresh === 1;
      if (state.stateRev && incomingStateRev && incomingStateRev !== state.stateRev && !hasRefreshOpenOrders) {
        result.needsResync = true;
        return {
          ...state,
          appliedResyncId,
          needsResync: true,
        };
      }

      const normalizedOpenOrders = hasRefreshOpenOrders
        ? normalizeOpenOrders(delta.open_orders?.rows)
        : {
            openOrdersById: state.openOrdersById,
            openOrderIds: state.openOrderIds,
          };
      const nextEvents = appendEvents(state.events, delta.events?.rows);
      const nextFills = appendFillEvents(state.fills, delta.events?.rows);
      const nextLifetimeSegments =
        hasRefreshOpenOrders || nextEvents !== state.events
          ? buildLifetimeSegments(
              normalizedOpenOrders.openOrdersById,
              normalizedOpenOrders.openOrderIds,
              nextEvents,
              state.lastSnapshotTsMs ?? state.lastServerTsMs
            )
          : state.lifetimeSegments;
      const nextMarketTrades = appendMarketTrades(state.marketTrades, delta.market_trades?.rows);
      const nextCandles = appendCandlesFromDelta(state.candles, delta.candles);
      const nextL2 = normalizeL2Snapshot(delta.l2) || state.l2;
      const serverTsMs =
        coercePositiveInt(delta.server_time_ms) ??
        coercePositiveInt(delta.server_ts_ms) ??
        Date.now();
      const nextPriceSeries = appendPricePoint(
        state.priceSeries,
        buildPricePoint(delta.bbo, serverTsMs),
        state.selection.timeRange
      );

      result.applied = true;
      return {
        ...state,
        roomId,
        context: delta.context || state.context,
        snapshotId: incomingSnapshotId || state.snapshotId,
        stateRev: incomingStateRev || state.stateRev,
        lastSeq: nextSeq,
        needsResync: false,
        pendingDeltaCount: 0,
        appliedResyncId,
        openOrdersById: normalizedOpenOrders.openOrdersById,
        openOrderIds: normalizedOpenOrders.openOrderIds,
        events: nextEvents,
        fills: nextFills,
        lifetimeSegments: nextLifetimeSegments,
        marketTrades: nextMarketTrades,
        candles: nextCandles,
        candleSource: delta.candles?.source ?? state.candleSource,
        l2: nextL2,
        priceSeries: nextPriceSeries,
        bbo: delta.bbo || state.bbo,
        status: delta.status || state.status,
        lastServerTsMs: serverTsMs,
      };
    });
    return result;
  },
}));

export const selectOrderViewSelection = (state: OrderViewStoreState) => state.selection;
export const selectOrderViewFocus = (state: OrderViewStoreState) => state.focus;
export const selectOrderViewNeedsResync = (state: OrderViewStoreState) => state.needsResync;
export const selectOrderViewPendingCount = (state: OrderViewStoreState) => state.pendingDeltaCount;
export const selectOrderViewOpenOrders = (state: OrderViewStoreState) => state.openOrderIds.map((rowId) => state.openOrdersById[rowId]).filter(Boolean);
export const selectOrderViewEvents = (state: OrderViewStoreState) => state.events;
export const selectOrderViewFills = (state: OrderViewStoreState) => state.fills;
export const selectOrderViewLifetimeSegments = (state: OrderViewStoreState) => state.lifetimeSegments;
export const selectOrderViewMarketTrades = (state: OrderViewStoreState) => state.marketTrades;
export const selectOrderViewCandles = (state: OrderViewStoreState) => state.candles;
export const selectOrderViewL2 = (state: OrderViewStoreState) => state.l2;
export const selectOrderViewPriceSeries = (state: OrderViewStoreState) => state.priceSeries;
