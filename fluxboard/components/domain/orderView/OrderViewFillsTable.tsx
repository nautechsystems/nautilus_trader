import { memo, useMemo, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';

import { colors } from '@/lib/tokens';
import type { OrderViewEvent } from '@/types';

type OrderViewFillsTableProps = {
  rows: OrderViewEvent[];
};

const ROW_HEIGHT = 30;
const MAX_RENDER_ROWS = 400;
const MAX_SORTED_ROWS = 2_000;

const toFillTime = (value: unknown): string => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return '--';
  return new Date(parsed).toLocaleTimeString();
};

function OrderViewFillsTableImpl({ rows }: OrderViewFillsTableProps) {
  const sortedRows = useMemo(
    () =>
      rows
        .map((row, index) => ({ row, index }))
        .sort((lhs, rhs) => {
          const lhsTs = Number(lhs.row.ts_ms) || 0;
          const rhsTs = Number(rhs.row.ts_ms) || 0;
          if (lhsTs !== rhsTs) return rhsTs - lhsTs;
          return lhs.index - rhs.index;
        })
        .slice(0, MAX_SORTED_ROWS)
        .map((entry) => entry.row),
    [rows]
  );

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
  const totalHeight = virtualRows.length > 0 ? rowVirtualizer.getTotalSize() : fallbackRows.length * ROW_HEIGHT;

  return (
    <div data-testid="order-view-fills-table" className="h-full border rounded overflow-hidden" style={{ borderColor: colors.border.DEFAULT }}>
      <div
        className="grid grid-cols-[120px_80px_100px_100px_1fr] px-2 py-1 text-xs border-b"
        style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.hover, color: colors.text.muted }}
      >
        <span>Event Key</span>
        <span>Side</span>
        <span className="text-right">Px</span>
        <span className="text-right">Qty</span>
        <span>Time</span>
      </div>
      <div ref={containerRef} className="h-[230px] overflow-auto">
        {sortedRows.length === 0 ? (
          <div className="px-2 py-3 text-xs" style={{ color: colors.text.muted }}>
            No fills
          </div>
        ) : (
          <div style={{ height: `${Math.max(totalHeight, 1)}px`, position: 'relative' }}>
            {rowsToRender.map((virtualRow) => {
              const row = sortedRows[virtualRow.index];
              if (!row) return null;
              const side = String(row.side || '').toLowerCase();
              const sideColor =
                side === 'sell' || side === 'ask'
                  ? colors.semantic.danger.light
                  : colors.semantic.success.light;
              return (
                <div
                  key={virtualRow.key}
                  className="absolute left-0 right-0 grid grid-cols-[120px_80px_100px_100px_1fr] px-2 items-center text-xs border-b"
                  style={{
                    transform: `translateY(${virtualRow.start}px)`,
                    height: `${ROW_HEIGHT}px`,
                    borderColor: colors.border.DEFAULT,
                  }}
                >
                  <span style={{ color: colors.text.secondary }}>{row.event_key || '--'}</span>
                  <span style={{ color: sideColor }}>{row.side || '--'}</span>
                  <span className="text-right" style={{ color: colors.text.secondary }}>
                    {row.px ?? '--'}
                  </span>
                  <span className="text-right" style={{ color: colors.text.secondary }}>
                    {row.qty ?? '--'}
                  </span>
                  <span style={{ color: colors.text.muted }}>{toFillTime(row.ts_ms)}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

export const OrderViewFillsTable = memo(OrderViewFillsTableImpl);
