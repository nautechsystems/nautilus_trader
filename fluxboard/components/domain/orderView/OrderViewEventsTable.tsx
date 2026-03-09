import { memo, useEffect, useMemo, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';

import { colors } from '@/lib/tokens';
import type { OrderViewFocusState } from '@/stores/orderViewStore';
import type { OrderViewEvent } from '@/types';

type OrderViewEventsTableProps = {
  rows: OrderViewEvent[];
  autoScroll?: boolean;
  orderSearch?: string;
  focus: OrderViewFocusState;
  onRowClick?: (row: OrderViewEvent) => void;
};

const ROW_HEIGHT = 30;
const MAX_RENDER_ROWS = 400;
const ORDER_VIEW_FOCUS_MATCH_EPSILON = 1e-6;

const toEventTimeMs = (value: unknown): string => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return '--';
  const d = new Date(parsed);
  return `${d.toLocaleTimeString(undefined, { hour12: false })}.${String(d.getMilliseconds()).padStart(3, '0')}`;
};

const toLatencyMs = (value: unknown): string => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) return '--';
  return `${Math.round(parsed)}`;
};

const matchesOrderSearch = (row: OrderViewEvent, searchText: string): boolean => {
  if (!searchText) return true;
  const haystacks = [row.order_id, row.client_order_id, row.event_key, row.fill_id]
    .map((value) => String(value || '').toLowerCase())
    .filter(Boolean);
  return haystacks.some((value) => value.includes(searchText));
};

const normalizeOrderSide = (value: unknown): 'bid' | 'ask' | null => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (text === 'buy' || text === 'bid') return 'bid';
  if (text === 'sell' || text === 'ask') return 'ask';
  return null;
};

const toOrderKey = (row: OrderViewEvent): string | null => {
  const orderId = String(row.order_id || '').trim();
  if (orderId) return orderId;
  const clientOrderId = String(row.client_order_id || '').trim();
  if (clientOrderId) return clientOrderId;
  return null;
};

const toFiniteNumber = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const samePrice = (lhs: number | null, rhs: number | null): boolean => {
  if (lhs === null || rhs === null) return false;
  return Math.abs(lhs - rhs) <= ORDER_VIEW_FOCUS_MATCH_EPSILON;
};

const isFocusActive = (focus: OrderViewFocusState): boolean =>
  Boolean(focus.orderKey || focus.eventKey || focus.side || focus.price !== null);

function OrderViewEventsTableImpl({
  rows,
  autoScroll = true,
  orderSearch = '',
  focus,
  onRowClick,
}: OrderViewEventsTableProps) {
  const normalizedSearch = orderSearch.trim().toLowerCase();
  const filteredRows = useMemo(
    () => rows.filter((row) => matchesOrderSearch(row, normalizedSearch)),
    [normalizedSearch, rows]
  );

  const containerRef = useRef<HTMLDivElement | null>(null);
  const rowVirtualizer = useVirtualizer({
    count: filteredRows.length,
    getScrollElement: () => containerRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  useEffect(() => {
    if (!autoScroll) return;
    const container = containerRef.current;
    if (!container) return;
    // Store order is newest-first (index 0), so auto-scroll should pin to top.
    container.scrollTop = 0;
  }, [autoScroll, filteredRows]);

  const virtualRows = rowVirtualizer.getVirtualItems();
  const fallbackRows = useMemo(
    () =>
      filteredRows.slice(0, MAX_RENDER_ROWS).map((_, index) => ({
        key: `fallback-${index}`,
        index,
        start: index * ROW_HEIGHT,
      })),
    [filteredRows]
  );

  const rowsToRender =
    virtualRows.length > 0
      ? virtualRows.map((row) => ({ key: row.key, index: row.index, start: row.start }))
      : fallbackRows;
  const totalHeight =
    virtualRows.length > 0 ? rowVirtualizer.getTotalSize() : fallbackRows.length * ROW_HEIGHT;

  return (
    <div
      data-testid="order-view-events-table"
      className="h-full border rounded overflow-hidden"
      style={{ borderColor: colors.border.DEFAULT }}
    >
      <div
        className="grid grid-cols-[120px_130px_80px_90px_90px_80px_80px_1fr_1fr] px-2 py-1 text-xs border-b"
        style={{
          borderColor: colors.border.DEFAULT,
          backgroundColor: colors.bg.hover,
          color: colors.text.muted,
        }}
      >
        <span>Event Key</span>
        <span>Type</span>
        <span>Side</span>
        <span className="text-right">Px</span>
        <span className="text-right">Qty</span>
        <span className="text-right">Ack ms</span>
        <span className="text-right">Fill ms</span>
        <span>Order ID</span>
        <span>Time (ms)</span>
      </div>
      <div ref={containerRef} className="h-[230px] overflow-auto">
        {filteredRows.length === 0 ? (
          <div className="px-2 py-3 text-xs" style={{ color: colors.text.muted }}>
            {normalizedSearch ? 'No matching events' : 'No events'}
          </div>
        ) : (
          <div style={{ height: `${Math.max(totalHeight, 1)}px`, position: 'relative' }}>
            {rowsToRender.map((virtualRow) => {
              const row = filteredRows[virtualRow.index];
              if (!row) return null;
              const side = String(row.side || '').toLowerCase();
              const sideColor =
                side === 'sell' || side === 'ask'
                  ? colors.semantic.danger.light
                  : side === 'buy' || side === 'bid'
                    ? colors.semantic.success.light
                    : colors.text.secondary;
              const focusActive = isFocusActive(focus);
              const rowOrderKey = toOrderKey(row);
              const rowSide = normalizeOrderSide(row.side);
              const rowPrice = toFiniteNumber(row.px);
              const matchesFocus =
                (focus.orderKey !== null && rowOrderKey === focus.orderKey) ||
                (focus.eventKey !== null &&
                  String(row.event_key || '').trim() === focus.eventKey) ||
                (focus.side !== null &&
                  focus.price !== null &&
                  rowSide === focus.side &&
                  samePrice(rowPrice, focus.price)) ||
                (focus.side === null &&
                  focus.price !== null &&
                  samePrice(rowPrice, focus.price));
              const focusState = !focusActive
                ? 'neutral'
                : matchesFocus
                  ? 'focused'
                  : 'dimmed';
              return (
                <div
                  key={virtualRow.key}
                  data-testid={`order-view-events-row-${String(row.event_key || virtualRow.index)}`}
                  data-focus={focusState}
                  className="absolute left-0 right-0 grid grid-cols-[120px_130px_80px_90px_90px_80px_80px_1fr_1fr] px-2 items-center text-xs border-b"
                  style={{
                    transform: `translateY(${virtualRow.start}px)`,
                    height: `${ROW_HEIGHT}px`,
                    borderColor: colors.border.DEFAULT,
                    backgroundColor:
                      focusState === 'focused' ? colors.bg.active : 'transparent',
                    opacity: focusState === 'dimmed' ? 0.4 : 1,
                    cursor: onRowClick ? 'pointer' : 'default',
                  }}
                  onClick={onRowClick ? () => onRowClick(row) : undefined}
                >
                  <span style={{ color: colors.text.secondary }}>{row.event_key || '--'}</span>
                  <span style={{ color: colors.text.primary }}>{row.type || '--'}</span>
                  <span style={{ color: sideColor }}>{row.side || '--'}</span>
                  <span className="text-right" style={{ color: colors.text.secondary }}>
                    {row.px ?? '--'}
                  </span>
                  <span className="text-right" style={{ color: colors.text.secondary }}>
                    {row.qty ?? '--'}
                  </span>
                  <span className="text-right" style={{ color: colors.text.secondary }}>
                    {toLatencyMs(row.ack_ms)}
                  </span>
                  <span className="text-right" style={{ color: colors.text.secondary }}>
                    {toLatencyMs(row.fill_ms)}
                  </span>
                  <span style={{ color: colors.text.secondary }}>
                    {row.order_id || row.client_order_id || '--'}
                  </span>
                  <span style={{ color: colors.text.muted }}>{toEventTimeMs(row.ts_ms)}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

export const OrderViewEventsTable = memo(OrderViewEventsTableImpl);
