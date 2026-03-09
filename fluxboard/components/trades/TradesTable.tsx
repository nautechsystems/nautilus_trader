// Trades blotter view rendered entirely with div-based virtualization and fixed heights.

import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import type { CSSProperties } from 'react';
import {
  Cell,
  flexRender,
  getCoreRowModel,
  Row,
  useReactTable,
} from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { TradeRow } from '../../types';
import { createColumns } from './columns';
import { DecisionModal } from './DecisionModal';
import { colors, typography, spacing } from '@/lib/tokens';
import { ColumnKey, gridTemplateFrom, gridMinWidth } from '@/config/columnMap';
import { useMobileLayout } from '@/hooks/useMobileLayout';

const ROW_HEIGHT = 28;
const OVERSCAN = 8;
const BASE_GRID_COLUMNS: ColumnKey[] = [
  'timeShort',
  'coin',
  'exch',
  'side',
  'px',
  'qty',
  'notional',
  'fee',
  'id', // trade id
  'id', // signal id
  'id', // strategy id
  'id', // order id
  'notes',
];

const MOBILE_GRID_COLUMNS: ColumnKey[] = [
  'timeShort',
  'coin',
  'side',
  'px',
  'qty',
  'notional',
];

const MOBILE_VISIBLE_COLUMNS = ['time', 'coin', 'side', 'px', 'qty', 'notional'];

const COLUMN_ALIGN: Record<string, 'left' | 'right' | 'center'> = {
  time: 'left',
  timeShort: 'left',
  coin: 'left',
  exch: 'left',
  side: 'center',
  px: 'right',
  qty: 'right',
  notional: 'right',
  fee: 'right',
  tx_hash: 'left',
  trd_id: 'left',
  signal: 'left',
  strategy: 'left',
  ord_id: 'left',
  decision: 'center',
  decision_summary: 'left',
  notes: 'left',
};

type StyleWithVars = CSSProperties & Record<string, string | number>;

const cellBaseStyle: CSSProperties = {
  padding: '6px 10px',
  overflow: 'hidden',
  whiteSpace: 'nowrap',
  textOverflow: 'ellipsis',
  color: colors.text.primary,
  fontSize: typography.fontSize.sm,
};

const rowBaseStyle: StyleWithVars = {
  display: 'grid',
  alignItems: 'center',
  backgroundColor: colors.bg.surface,
  borderBottom: `1px solid ${colors.border.DEFAULT}`,
  '--trades-row-hover': colors.bg.hover,
};

type ScrollState = {
  atTop: boolean;
  isScrolling: boolean;
  scrollElement: HTMLDivElement | null;
};

type TradesTableProps = {
  trades: TradeRow[] | undefined | null;
  sortDirection?: 'ts_desc' | 'ts_asc';
  onTimeSortChange?: (dir: 'ts_desc' | 'ts_asc') => void;
  onReachEnd?: () => void;
  onScrollStateChange?: (state: ScrollState) => void;
  enableDecisionDetails?: boolean;
};

const alignForColumn = (columnId: string): 'left' | 'right' | 'center' =>
  COLUMN_ALIGN[columnId] ?? 'left';

const CellRenderer = memo(({ cell }: { cell: Cell<TradeRow, unknown> }) => {
  const align = alignForColumn(cell.column.id);
  return (
    <div
      style={{
        ...cellBaseStyle,
        textAlign: align,
      }}
    >
      {flexRender(cell.column.columnDef.cell, cell.getContext())}
    </div>
  );
}, (prev, next) => {
  if (prev.cell.id !== next.cell.id) return false;
  const prevValue = prev.cell.getValue();
  const nextValue = next.cell.getValue();
  return prevValue === nextValue && prev.cell.column.id === next.cell.column.id;
});

type RowProps = {
  row: Row<TradeRow>;
  style: CSSProperties;
  gridTemplate: string;
};

const VirtualRowComponent = memo<RowProps>(({ row, style, gridTemplate }) => (
  <div
    className="trades-row"
    style={{
      ...style,
      ...rowBaseStyle,
      gridTemplateColumns: gridTemplate,
    }}
  >
    {row.getVisibleCells().map((cell) => (
      <CellRenderer key={cell.id} cell={cell} />
    ))}
  </div>
), (prev, next) => (
  prev.row.id === next.row.id
  && prev.style?.transform === next.style?.transform
  && prev.style?.top === next.style?.top
  && prev.gridTemplate === next.gridTemplate
));

export function TradesTable({
  trades,
  sortDirection = 'ts_desc',
  onTimeSortChange,
  onReachEnd,
  onScrollStateChange,
  enableDecisionDetails = false,
}: TradesTableProps) {
  const [selectedTrade, setSelectedTrade] = useState<TradeRow | null>(null);
  const { isMobile } = useMobileLayout();
  const scrollRef = useRef<HTMLDivElement>(null);
  const headerGridRef = useRef<HTMLDivElement>(null);
  const scrollStateRef = useRef<ScrollState>({
    atTop: true,
    isScrolling: false,
    scrollElement: null,
  });
  const scrollEndTimer = useRef<number | null>(null);
  const [mounted, setMounted] = useState(false);
  const columns = useMemo(
    () => createColumns(setSelectedTrade, {
      enableDecisionDetails,
      visibleColumns: isMobile ? MOBILE_VISIBLE_COLUMNS : undefined,
    }),
    [enableDecisionDetails, isMobile],
  );
  const gridColumns = useMemo(() => {
    const keys = [...(isMobile ? MOBILE_GRID_COLUMNS : BASE_GRID_COLUMNS)];
    if (!isMobile && enableDecisionDetails) {
      keys.splice(keys.length - 1, 0, 'decision');
    }
    return keys;
  }, [enableDecisionDetails, isMobile]);
  const gridTemplate = useMemo(
    () => gridTemplateFrom(gridColumns),
    [gridColumns],
  );
  const gridMinWidthPx = useMemo(
    () => gridMinWidth(gridColumns),
    [gridColumns],
  );
  const gridWidthStyle = useMemo(() => (
    isMobile
      ? { width: '100%', minWidth: '100%' }
      : { width: `${gridMinWidthPx}px`, minWidth: '100%' }
  ), [gridMinWidthPx, isMobile]);
  const data = useMemo(() => (Array.isArray(trades) ? trades : []), [trades]);

  const table = useReactTable({
    data,
    columns,
    manualSorting: true,
    manualFiltering: true,
    getCoreRowModel: getCoreRowModel(),
    getRowId: (row) => row.row_id,
  });

  const rows = table.getRowModel().rows;
  const reachEndNotifiedRef = useRef(-1);

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: OVERSCAN,
  });

  const virtualRows = mounted ? rowVirtualizer.getVirtualItems() : [];

  useEffect(() => {
    setMounted(true);
  }, []);

  useEffect(() => {
    reachEndNotifiedRef.current = -1;
  }, [rows.length]);

  useEffect(() => {
    if (!onReachEnd || virtualRows.length === 0 || rows.length === 0) return;
    const last = virtualRows[virtualRows.length - 1];
    if (last.index >= rows.length - 5 && last.index !== reachEndNotifiedRef.current) {
      reachEndNotifiedRef.current = last.index;
      onReachEnd();
    }
  }, [virtualRows, rows.length, onReachEnd]);

  const emitScrollState = useCallback((partial: Partial<ScrollState>) => {
    const next: ScrollState = {
      ...scrollStateRef.current,
      ...partial,
    };
    if (
      next.atTop === scrollStateRef.current.atTop
      && next.isScrolling === scrollStateRef.current.isScrolling
      && next.scrollElement === scrollStateRef.current.scrollElement
    ) {
      return;
    }
    scrollStateRef.current = next;
    onScrollStateChange?.(next);
  }, [onScrollStateChange]);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    // Keep header horizontally aligned with rows by translating the header grid
    const header = headerGridRef.current;
    if (header) {
      const x = el.scrollLeft || 0;
      // Translate opposite to scrollLeft so header appears to scroll with content
      header.style.transform = `translateX(${-x}px)`;
      header.style.willChange = 'transform';
    }
    const atTop = el.scrollTop <= 8;
    emitScrollState({ atTop, isScrolling: true, scrollElement: el });
    if (scrollEndTimer.current) {
      window.clearTimeout(scrollEndTimer.current);
    }
    scrollEndTimer.current = window.setTimeout(() => {
      emitScrollState({ isScrolling: false, scrollElement: el });
      scrollEndTimer.current = null;
    }, 120);
  }, [emitScrollState]);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) {
      emitScrollState({ atTop: el.scrollTop <= 8, scrollElement: el });
      // Initialize header transform to current scroll position
      if (headerGridRef.current) {
        const x = el.scrollLeft || 0;
        headerGridRef.current.style.transform = `translateX(${-x}px)`;
        headerGridRef.current.style.willChange = 'transform';
      }
    }
  }, [emitScrollState]);

  useEffect(() => {
    return () => {
      if (scrollEndTimer.current) {
        window.clearTimeout(scrollEndTimer.current);
      }
    };
  }, []);

  const headerGroups = table.getHeaderGroups();
  const headerRow = headerGroups[0]?.headers ?? [];

  return (
    <div className="flex flex-col h-full min-h-0">
      <div
        className="border-b font-mono uppercase tracking-wide"
        style={{
          borderBottomColor: colors.border.DEFAULT,
          backgroundColor: '#111214',
          overflowX: 'hidden', // Prevent header content from overflowing when translated
        }}
      >
        <div
          ref={headerGridRef}
          className="grid"
          style={{
            gridTemplateColumns: gridTemplate,
            fontSize: typography.fontSize['2xs'],
            color: colors.text.muted,
            ...gridWidthStyle,
          }}
        >
          {headerRow.map((header) => {
            const isTime = header.column.id === 'time';
            const align = alignForColumn(header.column.id);
            const arrow = isTime ? (sortDirection === 'ts_desc' ? ' ↓' : ' ↑') : null;
            const onClick = isTime && onTimeSortChange
              ? () => onTimeSortChange(sortDirection === 'ts_desc' ? 'ts_asc' : 'ts_desc')
              : undefined;
            return (
              <div
                key={header.id}
                className={`px-[10px] py-[6px] select-none transition-colors ${
                  align === 'right' ? 'text-right' : ''
                } ${isTime ? 'cursor-pointer hover:bg-bg-hover' : ''}`}
                style={{
                  color: colors.text.muted,
                }}
                onClick={onClick}
                title={isTime ? 'Toggle time sort' : undefined}
              >
                {flexRender(header.column.columnDef.header, header.getContext())}
                {arrow}
              </div>
            );
          })}
        </div>
      </div>

      <div
        ref={scrollRef}
        className="flex-1 min-h-0"
        onScroll={handleScroll}
        style={{
          overflow: 'auto',
          contain: 'strict',
          willChange: 'transform',
          backgroundColor: colors.bg.base,
        }}
      >
        {!mounted && rows.slice(0, 200).map((row) => (
          <VirtualRowComponent
            key={row.id}
            row={row}
            style={{
              position: 'relative',
              top: 0,
              left: 0,
              width: '100%',
              height: ROW_HEIGHT,
              transform: 'none',
            }}
            gridTemplate={gridTemplate}
          />
        ))}

        {mounted && (
          <div
            style={{
              height: rowVirtualizer.getTotalSize(),
              position: 'relative',
              ...gridWidthStyle,
            }}
          >
            {virtualRows.map((virtualRow) => {
              const row = rows[virtualRow.index];
              if (!row) {
                return null;
              }
              return (
                <VirtualRowComponent
                  key={row.id}
                  row={row}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    transform: `translateY(${virtualRow.start}px)`,
                    height: ROW_HEIGHT,
                  }}
                  gridTemplate={gridTemplate}
                />
              );
            })}
          </div>
        )}

        {rows.length === 0 && (
          <div
            className="p-8 text-center"
            style={{
              color: colors.text.muted,
              fontSize: typography.fontSize.sm,
            }}
          >
            No trades in selected filter
          </div>
        )}
      </div>

      {selectedTrade && (
        <DecisionModal trade={selectedTrade} onClose={() => setSelectedTrade(null)} />
      )}
    </div>
  );
}

export type TradesTableScrollState = ScrollState;
