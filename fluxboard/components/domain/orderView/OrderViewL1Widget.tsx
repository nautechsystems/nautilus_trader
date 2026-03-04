import { memo, useMemo } from 'react';

import { colors } from '@/lib/tokens';
import type { OrderViewFocusState } from '@/stores/orderViewStore';
import type {
  OrderViewBbo,
  OrderViewContext,
  OrderViewL2Level,
  OrderViewL2Snapshot,
  OrderViewOpenOrder,
  OrderViewStatus,
} from '@/types';

type OrderViewL1WidgetProps = {
  bbo: {
    maker?: OrderViewBbo;
    hedge?: OrderViewBbo;
  };
  l2: OrderViewL2Snapshot | null;
  openOrders: OrderViewOpenOrder[];
  context: OrderViewContext;
  status: OrderViewStatus | null;
  nowMs: number;
  staleThresholdMs: number;
  showBids: boolean;
  showAsks: boolean;
  focus: OrderViewFocusState;
  onLadderRowClick?: (row: { side: 'bid' | 'ask'; price: number; rank: number }) => void;
};

type LadderRow = {
  side: 'bid' | 'ask';
  rank: number;
  px: number;
  qty: number;
  qtyLabel: string;
  depthValue: number;
  ourQty: number;
  ourCount: number;
  isBest: boolean;
};

const numberOrNull = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const formatPx = (value: unknown): string => {
  const parsed = numberOrNull(value);
  return parsed === null ? '--' : parsed.toFixed(6);
};

const formatQty = (value: number): string => {
  if (!Number.isFinite(value)) return '--';
  return value.toFixed(4);
};

const formatAge = (valueMs: number | null): string => {
  if (valueMs === null || !Number.isFinite(valueMs)) return '--';
  if (valueMs < 1_000) return `${Math.round(valueMs)}ms`;
  return `${(valueMs / 1_000).toFixed(1)}s`;
};

const spreadOf = (entry?: OrderViewBbo): string => {
  const bid = numberOrNull(entry?.bid);
  const ask = numberOrNull(entry?.ask);
  if (bid === null || ask === null) return '--';
  return (ask - bid).toFixed(6);
};

const toPriceKey = (value: unknown): string | null => {
  const parsed = numberOrNull(value);
  if (parsed === null || parsed <= 0) return null;
  return parsed.toFixed(8);
};

const toDepthValue = (level: OrderViewL2Level): number => {
  const size = numberOrNull(level.size);
  if (size !== null && size > 0) return size;
  const qty = numberOrNull(level.qty);
  if (qty !== null && qty > 0) return qty;
  return 0;
};

const buildOverlayByPrice = (rows: OrderViewOpenOrder[]): {
  bids: Map<string, { qty: number; count: number }>;
  asks: Map<string, { qty: number; count: number }>;
} => {
  const bids = new Map<string, { qty: number; count: number }>();
  const asks = new Map<string, { qty: number; count: number }>();

  for (const row of rows) {
    const side = String(row.side || '').trim().toLowerCase();
    if (side !== 'bid' && side !== 'ask') continue;

    const priceKey = toPriceKey(row.px);
    if (!priceKey) continue;

    const qty = numberOrNull(row.rem_qty);
    const bucket = side === 'bid' ? bids : asks;
    const current = bucket.get(priceKey) ?? { qty: 0, count: 0 };
    current.count += 1;
    if (qty !== null && qty > 0) {
      current.qty += qty;
    }
    bucket.set(priceKey, current);
  }

  return { bids, asks };
};

const buildLadderRows = (
  levels: OrderViewL2Level[] | undefined,
  side: 'bid' | 'ask',
  overlayByPrice: Map<string, { qty: number; count: number }>
): LadderRow[] => {
  if (!Array.isArray(levels)) return [];

  const rows: LadderRow[] = [];
  for (let rank = 0; rank < levels.length; rank += 1) {
    const level = levels[rank];
    const px = numberOrNull(level?.px);
    const qty = numberOrNull(level?.qty);
    if (px === null || px <= 0 || qty === null || qty <= 0) continue;

    const overlay = overlayByPrice.get(toPriceKey(px) || '');
    rows.push({
      side,
      rank,
      px,
      qty,
      qtyLabel: String(level.qty),
      depthValue: toDepthValue(level),
      ourQty: overlay?.qty ?? 0,
      ourCount: overlay?.count ?? 0,
      isBest: rank === 0,
    });
  }

  return rows;
};

const formatSpreadAbs = (value: number | null): string => {
  if (value === null || !Number.isFinite(value)) return '--';
  return value.toFixed(6);
};

const formatSpreadBps = (value: number | null): string => {
  if (value === null || !Number.isFinite(value)) return '--';
  return `${value.toFixed(2)} bps`;
};

function L1Status({
  row,
  fallbackTsMs,
  nowMs,
  staleThresholdMs,
}: {
  row?: OrderViewBbo;
  fallbackTsMs: number | null;
  nowMs: number;
  staleThresholdMs: number;
}) {
  const hasRowData =
    row !== undefined &&
    row !== null &&
    [row.bid, row.ask, row.mid].some((value) => numberOrNull(value) !== null);
  if (!hasRowData) {
    return <span style={{ color: colors.semantic.warning.DEFAULT }}>missing (--)</span>;
  }
  const tsMs = numberOrNull(row?.ts_ms) ?? fallbackTsMs;
  const ageMs = tsMs !== null ? Math.max(0, nowMs - tsMs) : null;
  const stale = ageMs === null || ageMs > staleThresholdMs;
  return (
    <span style={{ color: stale ? colors.semantic.warning.DEFAULT : colors.text.secondary }}>
      {stale ? 'stale' : 'live'} ({formatAge(ageMs)})
    </span>
  );
}

function OrderViewL1WidgetImpl({
  bbo,
  l2,
  openOrders,
  context,
  status,
  nowMs,
  staleThresholdMs,
  showBids,
  showAsks,
  focus,
  onLadderRowClick,
}: OrderViewL1WidgetProps) {
  const rows: Array<{
    key: 'maker' | 'hedge';
    label: string;
    entry?: OrderViewBbo;
    venue: string;
  }> = [
    {
      key: 'maker',
      label: 'Maker',
      entry: bbo.maker,
      venue: `${context.maker.exchange || '--'} ${context.maker.symbol || '--'}`,
    },
    {
      key: 'hedge',
      label: 'Hedge',
      entry: bbo.hedge,
      venue: `${context.hedge.exchange || '--'} ${context.hedge.symbol || '--'}`,
    },
  ];

  const overlayByPrice = useMemo(() => buildOverlayByPrice(openOrders), [openOrders]);
  const bidRows = useMemo(
    () => buildLadderRows(l2?.bids, 'bid', overlayByPrice.bids),
    [l2?.bids, overlayByPrice.bids]
  );
  const askRowsRaw = useMemo(
    () => buildLadderRows(l2?.asks, 'ask', overlayByPrice.asks),
    [l2?.asks, overlayByPrice.asks]
  );
  const askRows = useMemo(() => [...askRowsRaw].reverse(), [askRowsRaw]);

  const maxDepth = useMemo(() => {
    const values = [...bidRows, ...askRowsRaw].map((row) => row.depthValue).filter((row) => row > 0);
    if (values.length === 0) return 0;
    return Math.max(...values);
  }, [askRowsRaw, bidRows]);

  const bestBid = bidRows.find((row) => row.isBest)?.px ?? numberOrNull(bbo.maker?.bid);
  const bestAsk = askRowsRaw.find((row) => row.isBest)?.px ?? numberOrNull(bbo.maker?.ask);
  const spreadAbsFallback =
    bestBid !== null && bestAsk !== null ? Math.max(0, bestAsk - bestBid) : null;
  const spreadAbs = numberOrNull(l2?.spread_abs) ?? spreadAbsFallback;
  const spreadBpsFallback =
    spreadAbs !== null && bestBid !== null && bestAsk !== null && bestBid + bestAsk > 0
      ? (spreadAbs / ((bestBid + bestAsk) / 2)) * 10_000
      : null;
  const spreadBps = numberOrNull(l2?.spread_bps) ?? spreadBpsFallback;

  const hasL2Rows = bidRows.length > 0 || askRows.length > 0;
  const topN = Number.isFinite(Number(l2?.top_n)) ? Number(l2?.top_n) : Math.max(bidRows.length, askRows.length);
  const focusedPriceKeys = useMemo(() => {
    const bids = new Set<string>();
    const asks = new Set<string>();

    if (focus.side && focus.price !== null) {
      const key = toPriceKey(focus.price);
      if (key) {
        (focus.side === 'bid' ? bids : asks).add(key);
      }
    }

    const focusOrderKey = String(focus.orderKey || '').trim();
    if (focusOrderKey) {
      for (const row of openOrders) {
        const orderId = String(row.order_id || '').trim();
        const clientOrderId = String(row.client_order_id || '').trim();
        if (orderId !== focusOrderKey && clientOrderId !== focusOrderKey) continue;
        const side = String(row.side || '').trim().toLowerCase();
        if (side !== 'bid' && side !== 'ask') continue;
        const key = toPriceKey(row.px);
        if (!key) continue;
        (side === 'bid' ? bids : asks).add(key);
      }
    }

    return { bids, asks };
  }, [focus.orderKey, focus.price, focus.side, openOrders]);
  const hasLadderFocus =
    focusedPriceKeys.bids.size > 0 || focusedPriceKeys.asks.size > 0;
  const rowFocusState = (row: LadderRow): 'focused' | 'dimmed' | 'neutral' => {
    if (!hasLadderFocus) return 'neutral';
    const priceKey = toPriceKey(row.px);
    if (!priceKey) return 'dimmed';
    const bucket = row.side === 'bid' ? focusedPriceKeys.bids : focusedPriceKeys.asks;
    return bucket.has(priceKey) ? 'focused' : 'dimmed';
  };

  return (
    <div data-testid="order-view-l1-widget" className="h-full flex flex-col text-xs gap-2">
      <div className="grid grid-cols-[80px_1fr_90px_90px_90px_90px_120px] gap-2 px-2 py-1 border-b" style={{ borderColor: colors.border.DEFAULT, color: colors.text.muted }}>
        <span>Leg</span>
        <span>Venue</span>
        <span className="text-right">Bid</span>
        <span className="text-right">Ask</span>
        <span className="text-right">Mid</span>
        <span className="text-right">Spread</span>
        <span>Status</span>
      </div>
      <div>
        {rows.map((row) => (
          <div key={row.key} className="grid grid-cols-[80px_1fr_90px_90px_90px_90px_120px] gap-2 px-2 py-2 border-b items-center" style={{ borderColor: colors.border.DEFAULT }}>
            <span style={{ color: colors.text.secondary }}>{row.label}</span>
            <span style={{ color: colors.text.muted }}>{row.venue}</span>
            <span className="text-right" style={{ color: colors.semantic.success.light }}>
              {formatPx(row.entry?.bid)}
            </span>
            <span className="text-right" style={{ color: colors.semantic.danger.light }}>
              {formatPx(row.entry?.ask)}
            </span>
            <span className="text-right" style={{ color: colors.text.primary }}>
              {formatPx(row.entry?.mid)}
            </span>
            <span className="text-right" style={{ color: colors.text.secondary }}>
              {spreadOf(row.entry)}
            </span>
            <L1Status
              row={row.entry}
              fallbackTsMs={numberOrNull(status?.last_md_ts_ms)}
              nowMs={nowMs}
              staleThresholdMs={staleThresholdMs}
            />
          </div>
        ))}
      </div>

      <div
        data-testid="order-view-ladder"
        className="flex-1 min-h-0 border rounded overflow-auto"
        style={{ borderColor: colors.border.DEFAULT }}
      >
        <div
          className="sticky top-0 z-10 grid grid-cols-[90px_110px_120px_52px] gap-2 px-2 py-1 border-b"
          style={{
            borderColor: colors.border.DEFAULT,
            backgroundColor: colors.bg.hover,
            color: colors.text.muted,
          }}
        >
          <span className="text-right">Qty</span>
          <span className="text-right">Px</span>
          <span className="text-right">Our</span>
          <span className="text-right">Best</span>
        </div>

        {!hasL2Rows ? (
          <div className="px-2 py-3" style={{ color: colors.text.muted }}>
            No L2 ladder data
          </div>
        ) : (
          <>
            {showAsks
              ? askRows.map((row) => {
                  const depthWidthPct = maxDepth > 0 ? (row.depthValue / maxDepth) * 100 : 0;
                  return (
                    <div
                      key={`ask-${row.rank}-${row.px}`}
                      data-testid={`order-view-ladder-row-ask-${row.rank}`}
                      data-focus={rowFocusState(row)}
                      className="relative grid grid-cols-[90px_110px_120px_52px] gap-2 px-2 py-1 border-b items-center"
                      style={{
                        borderColor: colors.border.DEFAULT,
                        backgroundColor:
                          rowFocusState(row) === 'focused'
                            ? colors.bg.active
                            : row.isBest
                              ? colors.bg.active
                              : 'transparent',
                        opacity: rowFocusState(row) === 'dimmed' ? 0.35 : 1,
                        cursor: onLadderRowClick ? 'pointer' : 'default',
                      }}
                      onClick={
                        onLadderRowClick
                          ? () => onLadderRowClick({ side: row.side, price: row.px, rank: row.rank })
                          : undefined
                      }
                    >
                      <div
                        data-testid={`order-view-ladder-depth-ask-${row.rank}`}
                        style={{
                          position: 'absolute',
                          inset: 0,
                          width: `${Math.max(0, Math.min(100, depthWidthPct)).toFixed(2)}%`,
                          backgroundColor: colors.semantic.danger.bg,
                          pointerEvents: 'none',
                        }}
                      />
                      <span className="text-right" style={{ color: colors.text.secondary, position: 'relative', zIndex: 1 }}>
                        {row.qtyLabel}
                      </span>
                      <span className="text-right" style={{ color: colors.semantic.danger.light, position: 'relative', zIndex: 1 }}>
                        {row.px.toFixed(6)}
                      </span>
                      <span
                        className="text-right"
                        data-testid={`order-view-ladder-our-ask-${row.rank}`}
                        style={{ color: colors.text.secondary, position: 'relative', zIndex: 1 }}
                      >
                        {row.ourCount > 0 ? `${row.ourCount} / ${formatQty(row.ourQty)}` : '--'}
                      </span>
                      <span className="text-right" style={{ color: colors.semantic.danger.light, position: 'relative', zIndex: 1 }}>
                        {row.isBest ? 'BEST' : ''}
                      </span>
                    </div>
                  );
                })
              : null}

            <div
              data-testid="order-view-ladder-spread-row"
              className="px-2 py-1 border-b flex items-center justify-between"
              style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.hover }}
            >
              <span style={{ color: colors.text.muted }}>Spread (top {topN || '--'})</span>
              <span style={{ color: colors.text.primary }}>
                {formatSpreadAbs(spreadAbs)} ({formatSpreadBps(spreadBps)})
              </span>
            </div>

            {showBids
              ? bidRows.map((row) => {
                  const depthWidthPct = maxDepth > 0 ? (row.depthValue / maxDepth) * 100 : 0;
                  return (
                    <div
                      key={`bid-${row.rank}-${row.px}`}
                      data-testid={`order-view-ladder-row-bid-${row.rank}`}
                      data-focus={rowFocusState(row)}
                      className="relative grid grid-cols-[90px_110px_120px_52px] gap-2 px-2 py-1 border-b items-center"
                      style={{
                        borderColor: colors.border.DEFAULT,
                        backgroundColor:
                          rowFocusState(row) === 'focused'
                            ? colors.bg.active
                            : row.isBest
                              ? colors.bg.active
                              : 'transparent',
                        opacity: rowFocusState(row) === 'dimmed' ? 0.35 : 1,
                        cursor: onLadderRowClick ? 'pointer' : 'default',
                      }}
                      onClick={
                        onLadderRowClick
                          ? () => onLadderRowClick({ side: row.side, price: row.px, rank: row.rank })
                          : undefined
                      }
                    >
                      <div
                        data-testid={`order-view-ladder-depth-bid-${row.rank}`}
                        style={{
                          position: 'absolute',
                          inset: 0,
                          width: `${Math.max(0, Math.min(100, depthWidthPct)).toFixed(2)}%`,
                          backgroundColor: colors.semantic.success.bg,
                          pointerEvents: 'none',
                        }}
                      />
                      <span className="text-right" style={{ color: colors.text.secondary, position: 'relative', zIndex: 1 }}>
                        {row.qtyLabel}
                      </span>
                      <span className="text-right" style={{ color: colors.semantic.success.light, position: 'relative', zIndex: 1 }}>
                        {row.px.toFixed(6)}
                      </span>
                      <span
                        className="text-right"
                        data-testid={`order-view-ladder-our-bid-${row.rank}`}
                        style={{ color: colors.text.secondary, position: 'relative', zIndex: 1 }}
                      >
                        {row.ourCount > 0 ? `${row.ourCount} / ${formatQty(row.ourQty)}` : '--'}
                      </span>
                      <span className="text-right" style={{ color: colors.semantic.success.light, position: 'relative', zIndex: 1 }}>
                        {row.isBest ? 'BEST' : ''}
                      </span>
                    </div>
                  );
                })
              : null}
          </>
        )}
      </div>
    </div>
  );
}

export const OrderViewL1Widget = memo(OrderViewL1WidgetImpl);
