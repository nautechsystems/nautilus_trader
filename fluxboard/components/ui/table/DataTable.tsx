/**
 * DataTable Component
 *
 * Generic table component using TanStack Table for data display and manipulation.
 * Supports sorting, row selection, row expansion, loading states, and empty states.
 *
 * @example
 * ```tsx
 * <DataTable
 *   data={balances}
 *   columns={columns}
 *   sortable
 *   dense
 *   enableRowExpansion
 *   renderExpandedRow={(row) => <BalanceDetails balance={row} />}
 * />
 * ```
 */

import {
  memo,
  useMemo,
  useState,
  useEffect,
  useRef,
  useCallback,
  Fragment,
  type ReactNode,
  type CSSProperties,
} from 'react';
import {
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  getExpandedRowModel,
  useReactTable,
  type ColumnDef,
  type SortingState,
  type RowSelectionState,
  type ExpandedState,
  type Row,
} from '@tanstack/react-table';
import type { Virtualizer, VirtualItem } from '@tanstack/react-virtual';
import { ChevronRight, ChevronDown } from 'lucide-react';
import { cn } from '@/lib/utils';
import { colors, spacing, typography, getDensityStyles } from '@/lib/tokens';
import { useMobileLayout } from '@/hooks/useMobileLayout';
import { TableHeader } from './TableHeader';
import { TableRow } from './TableRow';
import { EmptyTableRow } from './EmptyTableRow';
import { SortIndicator } from './SortIndicator';

export interface DataTableProps<T> {
  /** Table data */
  data: T[];
  /** Column definitions (TanStack Table format) */
  columns: ColumnDef<T>[];
  /** Table width behavior */
  widthMode?: 'fill' | 'content';
  /**
   * How to apply column widths.
   * - tanstack: Use TanStack sizing (current behavior; may apply default sizes).
   * - explicit: Only apply `size`/`minSize`/`maxSize` from the column definition passed to DataTable.
   * - none: Never apply width styles; browser auto-sizes columns.
   */
  columnWidthMode?: 'tanstack' | 'explicit' | 'none';
  /** Enable sorting */
  sortable?: boolean;
  /** Dense mode (24px rows) */
  dense?: boolean;
  /** Empty state message */
  emptyMessage?: string;
  /** Loading state */
  loading?: boolean;
  /** Row click handler */
  onRowClick?: (row: T) => void;
  /** Controlled sorting state */
  sortingState?: SortingState;
  /** Sorting change handler */
  onSortingStateChange?: (sorting: SortingState) => void;
  /** Initial sorting state when uncontrolled */
  initialSorting?: SortingState;
  /** Enable row selection */
  enableRowSelection?: boolean;
  /** Row selection state (controlled) */
  rowSelection?: RowSelectionState;
  /** Row selection change handler */
  onRowSelectionChange?: (selection: RowSelectionState) => void;
  /** Enable row expansion */
  enableRowExpansion?: boolean;
  /** Render function for expanded row content */
  renderExpandedRow?: (row: T) => ReactNode;
  /** Controlled expanded state */
  expandedState?: ExpandedState;
  /** Callback when expanded state changes */
  onExpandedStateChange?: (expanded: ExpandedState) => void;
  /** Grouping function (returns group key for each row) */
  getRowGroup?: (row: T) => string;
  /** Render function for group header */
  renderGroupHeader?: (groupKey: string, rows: T[]) => ReactNode;
  /** Optional row id factory */
  getRowId?: (row: T, index: number, parent?: any) => string;
  /** Additional CSS classes */
  className?: string;
  /** Virtualizer controlling row range */
  virtualizer?: Virtualizer<HTMLDivElement, HTMLTableRowElement> | null;
  /** Scrollable container reference used by the virtualizer */
  virtualScrollRef?: React.RefObject<HTMLDivElement>;
  /** Primary columns that should remain visible on mobile */
  primaryColumns?: string[];
  /** Secondary columns that can be hidden on mobile */
  secondaryColumns?: string[];
  /** Mobile rendering mode */
  mobileMode?: 'table' | 'cards';
  /** Custom row renderer for mobile cards */
  renderMobileRow?: (row: T) => React.ReactNode;
  /** Stable live-data version when mutating row objects in place */
  liveDataVersion?: number;
  /** Optional debug callback for live-table verification */
  onDebugMetrics?: (metrics: DataTableDebugMetrics) => void;
}

export interface DataTableDebugMetrics {
  coreRowModelInvalidated: boolean;
  liveCacheReset: boolean;
  rowModelStable: boolean;
  rowCount: number;
}

/**
 * DataTable - Generic table component with TanStack Table
 *
 * @example
 * const columns: ColumnDef<Trade>[] = [
 *   { accessorKey: 'timestamp', header: 'Time' },
 *   { accessorKey: 'symbol', header: 'Symbol' },
 *   { accessorKey: 'qty', header: 'Qty' },
 * ];
 *
 * <DataTable
 *   data={trades}
 *   columns={columns}
 *   sortable
 *   dense
 *   onRowClick={(trade) => console.log(trade)}
 * />
 */
function DataTableInner<T>({
  data,
  columns,
  widthMode = 'fill',
  columnWidthMode = 'tanstack',
  sortable = false,
  dense = false,
  emptyMessage = 'No data',
  loading = false,
  onRowClick,
  sortingState,
  onSortingStateChange,
  initialSorting,
  enableRowSelection = false,
  rowSelection,
  onRowSelectionChange,
  enableRowExpansion = false,
  renderExpandedRow,
  expandedState,
  onExpandedStateChange,
  getRowGroup,
  renderGroupHeader,
  getRowId,
  className,
  virtualizer,
  virtualScrollRef,
  primaryColumns,
  secondaryColumns,
  mobileMode = 'table',
  renderMobileRow,
  liveDataVersion,
  onDebugMetrics,
}: DataTableProps<T>) {
  // Sorting state (controlled or uncontrolled)
  const [internalSorting, setInternalSorting] = useState<SortingState>(
    sortingState ?? initialSorting ?? []
  );

  useEffect(() => {
    if (sortingState) {
      setInternalSorting(sortingState);
    }
  }, [sortingState]);

  const currentSorting = sortingState ?? internalSorting;
  const previousLiveDataVersionRef = useRef(liveDataVersion);
  const liveCacheReset = previousLiveDataVersionRef.current !== liveDataVersion;
  const hasActiveSorting = sortable && currentSorting.length > 0;
  const effectiveData = useMemo(
    () => (liveCacheReset && hasActiveSorting ? [...data] : data),
    [data, hasActiveSorting, liveCacheReset]
  );
  const effectiveSorting = useMemo(
    () => (liveCacheReset && hasActiveSorting
      ? currentSorting.map((entry) => ({ ...entry }))
      : currentSorting),
    [currentSorting, hasActiveSorting, liveCacheReset]
  );

  const handleSortingChange = useCallback(
    (updater: SortingState | ((old: SortingState) => SortingState)) => {
      const previous = sortingState ?? internalSorting;
      const next =
        typeof updater === 'function'
          ? (updater as (old: SortingState) => SortingState)(previous)
          : updater;

      if (!sortingState) {
        setInternalSorting(next);
      }
      onSortingStateChange?.(next);
    },
    [sortingState, internalSorting, onSortingStateChange]
  );
  const [internalExpanded, setInternalExpanded] = useState<ExpandedState>(expandedState ?? {});

  useEffect(() => {
    if (expandedState) {
      setInternalExpanded(expandedState);
    }
  }, [expandedState]);

  const currentExpanded = expandedState ?? internalExpanded;

  const handleExpandedChange = useCallback(
    (updater: ExpandedState | ((old: ExpandedState) => ExpandedState)) => {
      const previous = expandedState ?? internalExpanded;
      const next =
        typeof updater === 'function'
          ? (updater as (old: ExpandedState) => ExpandedState)(previous)
          : updater;

      if (!expandedState) {
        setInternalExpanded(next);
      }
      onExpandedStateChange?.(next);
    },
    [expandedState, internalExpanded, onExpandedStateChange]
  );

  const { density: densityMode, isMobile: isMobileViewport } = useMobileLayout();
  const primarySet = useMemo(() => new Set((primaryColumns ?? []).map((col) => col.toString())), [primaryColumns]);
  const secondarySet = useMemo(() => new Set((secondaryColumns ?? []).map((col) => col.toString())), [secondaryColumns]);

  const resolvedColumns = useMemo(() => {
    if (!isMobileViewport || mobileMode === 'cards') {
      return columns;
    }

    const filterColumns = (colDefs: ColumnDef<T>[]): ColumnDef<T>[] => {
      return colDefs
        .map((col) => {
          const childColumns = (col as ColumnDef<T> & { columns?: ColumnDef<T>[] }).columns;
          if (childColumns && childColumns.length > 0) {
            const filteredChildren = filterColumns(childColumns);
            if (filteredChildren.length === 0) {
              return null;
            }
            return { ...col, columns: filteredChildren } as ColumnDef<T>;
          }

          const columnKeyRaw = (col as ColumnDef<T> & { id?: string; accessorKey?: string | number }).id
            ?? (col as ColumnDef<T> & { accessorKey?: string | number }).accessorKey;
          const columnKey = columnKeyRaw === undefined || columnKeyRaw === null
            ? null
            : String(columnKeyRaw);

          if (!columnKey) {
            return col;
          }

          if (primarySet.size > 0) {
            return primarySet.has(columnKey) ? col : null;
          }
          if (secondarySet.size > 0) {
            return secondarySet.has(columnKey) ? null : col;
          }

          return col;
        })
        .filter(Boolean) as ColumnDef<T>[];
    };

    return filterColumns(columns);
  }, [columns, isMobileViewport, mobileMode, primarySet, secondarySet]);

  // Group data if grouping function provided
  const groupedData = useMemo(() => {
    if (!getRowGroup) return null;

    const groups = new Map<string, T[]>();
    data.forEach(row => {
      const groupKey = getRowGroup(row);
      if (!groups.has(groupKey)) {
        groups.set(groupKey, []);
      }
      groups.get(groupKey)!.push(row);
    });
    return groups;
  }, [data, getRowGroup]);

  // Adjust columns for expansion
  const expandedColumns = useMemo(() => {
    if (!enableRowExpansion) return resolvedColumns;

    return [
      {
        id: 'expander',
        header: () => null,
        cell: ({ row }: any) => (
          <button
            onClick={(e) => {
              e.stopPropagation();
              row.toggleExpanded();
            }}
            style={{ color: colors.text.muted }}
          >
            {row.getIsExpanded() ? (
              <ChevronDown className="w-4 h-4" />
            ) : (
              <ChevronRight className="w-4 h-4" />
            )}
          </button>
        ),
      },
      ...resolvedColumns,
    ];
  }, [resolvedColumns, enableRowExpansion]);

  // For explicit sizing mode, we only respect sizing info actually provided by the caller.
  // TanStack merges defaults (e.g. size=150) into column defs, so reading from `column.columnDef`
  // doesn't tell us whether the caller explicitly set a size.
  const explicitSizingById = useMemo(() => {
    const map = new Map<string, { size?: number; minSize?: number; maxSize?: number }>();

    const visit = (colDefs: ColumnDef<T>[]) => {
      colDefs.forEach((col) => {
        const anyCol = col as any;
        const keyRaw = anyCol.id ?? anyCol.accessorKey;
        const key = keyRaw === undefined || keyRaw === null ? null : String(keyRaw);
        if (key) {
          const size = typeof anyCol.size === 'number' && anyCol.size > 0 ? anyCol.size : undefined;
          const minSize = typeof anyCol.minSize === 'number' && anyCol.minSize > 0 ? anyCol.minSize : undefined;
          const maxSize = typeof anyCol.maxSize === 'number' && anyCol.maxSize > 0 ? anyCol.maxSize : undefined;
          if (size !== undefined || minSize !== undefined || maxSize !== undefined) {
            map.set(key, { size, minSize, maxSize });
          }
        }

        const children = anyCol.columns as ColumnDef<T>[] | undefined;
        if (children && children.length > 0) {
          visit(children);
        }
      });
    };

    visit(expandedColumns);
    return map;
  }, [expandedColumns]);

  const getWidthStyle = useCallback(
    (column: any): Pick<CSSProperties, 'width' | 'minWidth' | 'maxWidth'> | undefined => {
      if (columnWidthMode === 'none') return undefined;

      if (columnWidthMode === 'explicit') {
        const sizing = explicitSizingById.get(String(column.id));
        if (!sizing) return undefined;
        if (sizing.size !== undefined) {
          return {
            width: sizing.size,
            minWidth: sizing.size,
            ...(sizing.maxSize !== undefined ? { maxWidth: sizing.maxSize } : {}),
          };
        }
        return {
          ...(sizing.minSize !== undefined ? { minWidth: sizing.minSize } : {}),
          ...(sizing.maxSize !== undefined ? { maxWidth: sizing.maxSize } : {}),
        };
      }

      const columnSize = column.getSize();
      const minSize = (column.columnDef as any).minSize;
      // Prefer size over minSize, but use minSize as fallback
      const width = columnSize > 0 ? columnSize : (minSize > 0 ? minSize : undefined);
      return width ? { minWidth: width, width: width } : undefined;
    },
    [columnWidthMode, explicitSizingById]
  );

  // Memoize table configuration
  const tableConfig = useMemo(
    () => ({
      data: effectiveData,
      columns: expandedColumns,
      state: {
        sorting: effectiveSorting,
        expanded: currentExpanded,
        ...(rowSelection && { rowSelection }),
      },
      onSortingChange: handleSortingChange,
      onExpandedChange: handleExpandedChange,
      onRowSelectionChange,
      getCoreRowModel: getCoreRowModel(),
      getSortedRowModel: sortable ? getSortedRowModel() : undefined,
      getExpandedRowModel: enableRowExpansion ? getExpandedRowModel() : undefined,
      getRowId,
      enableRowSelection,
      enableSorting: sortable,
      enableExpanding: enableRowExpansion,
    }),
    [effectiveData, expandedColumns, effectiveSorting, currentExpanded, sortable, enableRowSelection, enableRowExpansion, rowSelection, onRowSelectionChange, handleSortingChange, handleExpandedChange, getRowId]
  );

  const table = useReactTable(tableConfig as any);
  const previousDataRef = useRef(effectiveData);
  const previousRowModelRef = useRef<ReturnType<typeof table.getRowModel>>();

  // Extract current sort state for indicators
  const currentSort = currentSorting[0];
  const sortColumn = currentSort?.id ?? null;
  const sortDirection = currentSort?.desc ? 'desc' : 'asc';

  const densityStyles = getDensityStyles(dense, densityMode);

  if (liveCacheReset) {
    previousRowModelRef.current?.rows.forEach((row) => {
      (row as any)._valuesCache = {};
      (row as any)._uniqueValuesCache = {};
    });
  }
  const rowModel = table.getRowModel();
  if (liveCacheReset) {
    previousLiveDataVersionRef.current = liveDataVersion;
  }
  const rows = rowModel.rows;
  const useCardMode = isMobileViewport && mobileMode === 'cards' && Boolean(renderMobileRow);
  const virtualItems: VirtualItem[] = useCardMode ? [] : virtualizer?.getVirtualItems() ?? [];
  const totalVirtualSize = useCardMode ? 0 : virtualizer?.getTotalSize() ?? 0;
  const virtualEnabled = Boolean(
    !useCardMode && virtualizer && !groupedData && rows.length > 0 && virtualItems.length > 0
  );
  const topVirtualHeight = virtualItems[0]?.start ?? 0;
  const lastVirtualItem = virtualItems[virtualItems.length - 1];
  const bottomVirtualHeight = virtualItems.length > 0
    ? Math.max(0, totalVirtualSize - ((lastVirtualItem?.start ?? 0) + (lastVirtualItem?.size ?? 0)))
    : 0;

  // Performance optimization: Create Map for O(1) row lookups in grouped rendering
  const rowDataToRowMap = useMemo(() => {
    if (!groupedData || !renderGroupHeader) return null;
    return new Map(rows.map((row) => [row.original, row]));
  }, [groupedData, renderGroupHeader, rows]);

  useEffect(() => {
    const metrics: DataTableDebugMetrics = {
      coreRowModelInvalidated: previousDataRef.current !== effectiveData,
      liveCacheReset,
      rowModelStable: previousRowModelRef.current === rowModel,
      rowCount: rows.length,
    };
    onDebugMetrics?.(metrics);
    previousDataRef.current = effectiveData;
    previousRowModelRef.current = rowModel;
  }, [effectiveData, liveCacheReset, onDebugMetrics, rowModel, rows.length]);

  const renderTableCells = (row: Row<T>) =>
    row.getVisibleCells().map((cell) => {
      const widthStyle = getWidthStyle(cell.column);

      return (
        <td
          key={cell.id}
          style={{
            padding: '6px 10px',
            fontSize: densityStyles.fontSize,
            fontWeight: typography.fontWeight.normal,
            color: colors.text.primary,
            fontFamily: typography.fontFamily.sans,
            ...(widthStyle ?? {}),
          }}
        >
          {flexRender(cell.column.columnDef.cell, cell.getContext())}
        </td>
      );
    });

  const renderTableRow = (row: Row<T>, virtualRow?: VirtualItem) => (
    <Fragment key={`row-fragment-${row.id}`}>
      {virtualRow ? (
        <tr
          aria-selected={row.getIsSelected()}
          className={cn(
            'transition-colors duration-150 border-b border-border',
            onRowClick && 'cursor-pointer',
            !row.getIsSelected() && 'hover:bg-bg-hover',
            row.getIsSelected() && 'bg-accent/12 border-accent/60',
          )}
          style={{
            height: dense ? spacing.row.compact : spacing.row.normal,
          }}
          onClick={onRowClick ? () => onRowClick(row.original as T) : undefined}
          onKeyDown={(event) => {
            if (onRowClick && (event.key === 'Enter' || event.key === ' ')) {
              event.preventDefault();
              onRowClick(row.original as T);
            }
          }}
          role={onRowClick ? 'button' : undefined}
          tabIndex={onRowClick ? 0 : undefined}
          ref={(node) => {
            if (node) {
              (virtualizer as any)?.measureElement?.(node);
            }
          }}
          data-index={virtualRow.index}
        >
          {renderTableCells(row)}
        </tr>
      ) : (
        <TableRow
          key={row.id}
          dense={dense}
          selected={row.getIsSelected()}
          onClick={onRowClick ? () => onRowClick(row.original as T) : undefined}
        >
          {renderTableCells(row)}
        </TableRow>
      )}
      {enableRowExpansion && row.getIsExpanded() && renderExpandedRow && (
        <tr
          key={`${row.id}-expanded`}
          style={{ backgroundColor: colors.bg.hover }}
        >
          <td
            colSpan={expandedColumns.length}
            style={{
              padding: spacing.gap.md,
              borderTop: `1px solid ${colors.border.DEFAULT}`,
            }}
          >
            {renderExpandedRow(row.original as T)}
          </td>
        </tr>
      )}
    </Fragment>
  );

  if (useCardMode && renderMobileRow) {
    return (
      <div className={cn('flex flex-col gap-3', className)} ref={virtualScrollRef}>
        {loading ? (
          <div className="py-4 text-center text-sm" style={{ color: colors.text.muted }}>
            Loading...
          </div>
        ) : rows.length === 0 ? (
          <div
            className="rounded-lg border border-dashed border-neutral-800 py-6 text-center text-sm"
            style={{ color: colors.text.muted }}
          >
            {emptyMessage}
          </div>
        ) : (
          rows.map((row) => (
            <div key={row.id}>{renderMobileRow(row.original as T)}</div>
          ))
        )}
      </div>
    );
  }

  return (
    <div
      className={cn(className, widthMode === 'content' && 'max-w-full overflow-x-auto')}
      ref={virtualScrollRef}
    >
      <table className={cn(widthMode === 'content' ? 'w-max' : 'w-full', 'border-collapse')}>
        <thead>
          {table.getHeaderGroups().map((headerGroup) => (
            <tr key={headerGroup.id}>
              {headerGroup.headers.map((header) => {
                const canSort = header.column.getCanSort();
                const widthStyle = getWidthStyle(header.column);

                return (
                  <TableHeader
                    key={header.id}
                    sortable={canSort}
                    onSort={() => header.column.toggleSorting()}
                    style={widthStyle}
                  >
                    <div className="flex items-center">
                      {header.isPlaceholder
                        ? null
                        : flexRender(
                            header.column.columnDef.header,
                            header.getContext()
                          )}
                      {canSort && (
                        <SortIndicator
                          column={header.id}
                          sortColumn={sortColumn}
                          sortDirection={sortColumn === header.id ? sortDirection : null}
                        />
                      )}
                    </div>
                  </TableHeader>
                );
              })}
            </tr>
          ))}
        </thead>
        <tbody>
          {loading ? (
            <EmptyTableRow
              colSpan={expandedColumns.length}
              message="Loading..."
            />
          ) : rows.length === 0 ? (
            <EmptyTableRow
              colSpan={expandedColumns.length}
              message={emptyMessage}
            />
          ) : virtualEnabled ? (
            <>
              {topVirtualHeight > 0 && (
                <tr aria-hidden style={{ height: topVirtualHeight, lineHeight: 0 }}>
                  <td colSpan={expandedColumns.length} />
                </tr>
              )}
              {virtualItems.map((virtualRow) => {
                const row = rows[virtualRow.index];
                if (!row) return null;
                return renderTableRow(row as Row<T>, virtualRow);
              })}
              {bottomVirtualHeight > 0 && (
                <tr aria-hidden style={{ height: bottomVirtualHeight, lineHeight: 0 }}>
                  <td colSpan={expandedColumns.length} />
                </tr>
              )}
            </>
          ) : groupedData && renderGroupHeader ? (
            // Grouped rendering
            Array.from(groupedData.entries()).map(([groupKey, groupRows]) => (
              <Fragment key={`group-fragment-${groupKey}`}>
                <tr
                  key={`group-${groupKey}`}
                  style={{
                    backgroundColor: colors.bg.surface,
                    borderTop: `1px solid ${colors.border.DEFAULT}`,
                  }}
                >
                  <td
                    colSpan={expandedColumns.length}
                    style={{
                      padding: densityStyles.padding,
                      fontWeight: typography.fontWeight.semibold,
                      color: colors.text.secondary,
                    }}
                  >
                    {renderGroupHeader(groupKey, groupRows)}
                  </td>
                </tr>
                {groupRows.map((rowData) => {
                  const row = rowDataToRowMap?.get(rowData);
                  if (!row) return null;
                  return renderTableRow(row as Row<T>);
                })}
              </Fragment>
            ))
          ) : (
            // Standard rendering
            rows.map((row) => renderTableRow(row as Row<T>))
          )}
        </tbody>
      </table>
    </div>
  );
}

// Export with memo for performance
export const DataTable = memo(DataTableInner) as typeof DataTableInner;
