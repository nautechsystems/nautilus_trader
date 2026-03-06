import { memo, useEffect, useMemo, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';

import { colors } from '@/lib/tokens';
import type { OrderViewMarketTradeRow } from '@/types';

type OrderViewMarketTradesTableProps = {
  rows: OrderViewMarketTradeRow[];
  autoScroll?: boolean;
};

const ROW_HEIGHT = 30;
const MAX_RENDER_ROWS = 400;

const toTradeTimeMs = (value: unknown): string => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return '--';
  const d = new Date(parsed);
  return `${d.toLocaleTimeString(undefined, { hour12: false })}.${String(d.getMilliseconds()).padStart(3, '0')}`;
};

function OrderViewMarketTradesTableImpl({ rows, autoScroll = true }: OrderViewMarketTradesTableProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const rowVirtualizer = useVirtualizer({
    count: rows.length,
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
  }, [autoScroll, rows]);

  const virtualRows = rowVirtualizer.getVirtualItems();
  const fallbackRows = useMemo(
    () =>
      rows.slice(0, MAX_RENDER_ROWS).map((_, index) => ({
        key: `fallback-${index}`,
        index,
        start: index * ROW_HEIGHT,
      })),
    [rows]
  );

  const rowsToRender =
    virtualRows.length > 0
      ? virtualRows.map((row) => ({ key: row.key, index: row.index, start: row.start }))
      : fallbackRows;
  const totalHeight =
    virtualRows.length > 0 ? rowVirtualizer.getTotalSize() : fallbackRows.length * ROW_HEIGHT;

  return (
    <div
      data-testid="order-view-market-trades-table"
      className="h-full border rounded overflow-hidden"
      style={{ borderColor: colors.border.DEFAULT }}
    >
      <div
        className="grid grid-cols-[120px_80px_100px_100px_1fr] px-2 py-1 text-xs border-b"
        style={{
          borderColor: colors.border.DEFAULT,
          backgroundColor: colors.bg.hover,
          color: colors.text.muted,
        }}
      >
        <span>Trade ID</span>
        <span>Side</span>
        <span className="text-right">Px</span>
        <span className="text-right">Qty</span>
        <span>Time (ms)</span>
      </div>
      <div ref={containerRef} className="h-[230px] overflow-auto">
        {rows.length === 0 ? (
          <div className="px-2 py-3 text-xs" style={{ color: colors.text.muted }}>
            No market trades
          </div>
        ) : (
          <div style={{ height: `${Math.max(totalHeight, 1)}px`, position: 'relative' }}>
            {rowsToRender.map((virtualRow) => {
              const row = rows[virtualRow.index];
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
                  <span style={{ color: colors.text.secondary }}>{row.trade_id || '--'}</span>
                  <span style={{ color: sideColor }}>{row.side || '--'}</span>
                  <span className="text-right" style={{ color: colors.text.secondary }}>
                    {row.price ?? '--'}
                  </span>
                  <span className="text-right" style={{ color: colors.text.secondary }}>
                    {row.qty ?? '--'}
                  </span>
                  <span style={{ color: colors.text.muted }}>{toTradeTimeMs(row.ts_ms)}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

export const OrderViewMarketTradesTable = memo(OrderViewMarketTradesTableImpl);
