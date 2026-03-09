import { memo, useMemo, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';

import { colors } from '@/lib/tokens';
import type { OrderViewOpenOrder } from '@/types';

type OrderViewOpenOrdersTableProps = {
  rows: OrderViewOpenOrder[];
  showBids: boolean;
  showAsks: boolean;
};

const ROW_HEIGHT = 30;
const MAX_RENDER_ROWS = 400;
const MAX_SORTED_ROWS = 2_000;

const toFiniteNumber = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const sideRank = (side: string): number => {
  const normalized = String(side || '').toLowerCase();
  if (normalized === 'bid') return 0;
  if (normalized === 'ask') return 1;
  return 2;
};

function OrderViewOpenOrdersTableImpl({ rows, showBids, showAsks }: OrderViewOpenOrdersTableProps) {
  const sortedRows = useMemo(() => {
    const filteredRows = rows.filter((row) => {
      if (row.side === 'bid' && !showBids) return false;
      if (row.side === 'ask' && !showAsks) return false;
      return true;
    });

    return filteredRows
      .map((row, index) => ({ row, index }))
      .sort((lhs, rhs) => {
        const sideDiff = sideRank(lhs.row.side) - sideRank(rhs.row.side);
        if (sideDiff !== 0) return sideDiff;

        const lhsLevel = toFiniteNumber(lhs.row.level);
        const rhsLevel = toFiniteNumber(rhs.row.level);
        if (lhsLevel !== null && rhsLevel !== null && lhsLevel !== rhsLevel) {
          return lhsLevel - rhsLevel;
        }

        const lhsPrice = toFiniteNumber(lhs.row.px);
        const rhsPrice = toFiniteNumber(rhs.row.px);
        if (lhsPrice !== null && rhsPrice !== null && lhsPrice !== rhsPrice) {
          return lhs.row.side === 'bid' ? rhsPrice - lhsPrice : lhsPrice - rhsPrice;
        }

        return lhs.index - rhs.index;
      })
      .slice(0, MAX_SORTED_ROWS)
      .map((entry) => entry.row);
  }, [rows, showAsks, showBids]);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const rowVirtualizer = useVirtualizer({
    count: sortedRows.length,
    getScrollElement: () => containerRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  const virtualRows = rowVirtualizer.getVirtualItems();
  const fallbackRows = useMemo(
    () =>
      sortedRows.slice(0, MAX_RENDER_ROWS).map((_, index) => ({
        key: `fallback-${index}`,
        index,
        start: index * ROW_HEIGHT,
      })),
    [sortedRows]
  );

  const rowsToRender =
    virtualRows.length > 0
      ? virtualRows.map((row) => ({ key: row.key, index: row.index, start: row.start }))
      : fallbackRows;
  const totalHeight =
    virtualRows.length > 0 ? rowVirtualizer.getTotalSize() : fallbackRows.length * ROW_HEIGHT;

  return (
    <div
      data-testid="order-view-open-orders-table"
      className="h-full border rounded overflow-hidden"
      style={{ borderColor: colors.border.DEFAULT }}
    >
      <div
        className="grid grid-cols-[1.2fr_80px_80px_70px_100px_100px_1fr] px-2 py-1 text-xs border-b"
        style={{
          borderColor: colors.border.DEFAULT,
          backgroundColor: colors.bg.hover,
          color: colors.text.muted,
        }}
      >
        <span>Row ID</span>
        <span>Leg</span>
        <span>Side</span>
        <span className="text-right">Lvl</span>
        <span className="text-right">Px</span>
        <span className="text-right">Qty</span>
        <span>Client Order ID</span>
      </div>
      <div ref={containerRef} className="h-[230px] overflow-auto">
        {sortedRows.length === 0 ? (
          <div className="px-2 py-3 text-xs" style={{ color: colors.text.muted }}>
            No open orders
          </div>
        ) : (
          <div style={{ height: `${Math.max(totalHeight, 1)}px`, position: 'relative' }}>
            {rowsToRender.map((virtualRow) => {
              const row = sortedRows[virtualRow.index];
              if (!row) return null;
              return (
                <div
                  key={virtualRow.key}
                  className="absolute left-0 right-0 grid grid-cols-[1.2fr_80px_80px_70px_100px_100px_1fr] px-2 items-center text-xs border-b"
                  style={{
                    transform: `translateY(${virtualRow.start}px)`,
                    height: `${ROW_HEIGHT}px`,
                    borderColor: colors.border.DEFAULT,
                  }}
                >
                  <span style={{ color: colors.text.secondary }}>{row.order_row_id}</span>
                  <span style={{ color: colors.text.secondary }}>{row.leg}</span>
                  <span
                    style={{
                      color:
                        row.side === 'bid'
                          ? colors.semantic.success.light
                          : colors.semantic.danger.light,
                    }}
                  >
                    {row.side}
                  </span>
                  <span className="text-right" style={{ color: colors.text.secondary }}>
                    {row.level}
                  </span>
                  <span className="text-right" style={{ color: colors.text.primary }}>
                    {row.px ?? '--'}
                  </span>
                  <span className="text-right" style={{ color: colors.text.primary }}>
                    {row.rem_qty ?? '--'}
                  </span>
                  <span style={{ color: colors.text.secondary }}>
                    {row.client_order_id || row.order_id || '--'}
                  </span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

export const OrderViewOpenOrdersTable = memo(OrderViewOpenOrdersTableImpl);
