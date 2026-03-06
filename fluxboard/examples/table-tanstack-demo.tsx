/**
 * TanStack Table Integration Demo
 *
 * Demonstrates that DataTable correctly integrates with TanStack Table
 * and exposes all the powerful features of the library.
 */

import { useState } from 'react';
import { type ColumnDef, type SortingState, type RowSelectionState } from '@tanstack/react-table';
import { DataTable } from '@/components/ui/table';

interface Trade {
  id: string;
  timestamp: number;
  symbol: string;
  side: 'BUY' | 'SELL';
  qty: number;
  price: number;
  pnl: number;
}

const mockTrades: Trade[] = [
  {
    id: '1',
    timestamp: Date.now(),
    symbol: 'PLUME/USDT',
    side: 'BUY',
    qty: 100,
    price: 1.25,
    pnl: 5.43,
  },
  {
    id: '2',
    timestamp: Date.now() - 1000,
    symbol: 'WETH/USDT',
    side: 'SELL',
    qty: 0.5,
    price: 2500.00,
    pnl: -2.15,
  },
  {
    id: '3',
    timestamp: Date.now() - 2000,
    symbol: 'SEI/USDC',
    side: 'BUY',
    qty: 500,
    price: 0.45,
    pnl: 12.30,
  },
  {
    id: '4',
    timestamp: Date.now() - 3000,
    symbol: 'PLUME/USDT',
    side: 'SELL',
    qty: 50,
    price: 1.26,
    pnl: 3.21,
  },
  {
    id: '5',
    timestamp: Date.now() - 4000,
    symbol: 'WETH/USDT',
    side: 'BUY',
    qty: 1.0,
    price: 2475.00,
    pnl: -8.45,
  },
];

/**
 * Proof of TanStack Table Integration:
 *
 * 1. Column definitions use TanStack's ColumnDef<T> type
 * 2. Custom cell renderers via cell property
 * 3. Accessor keys for data binding
 * 4. Sort functionality via TanStack's sorting system
 * 5. Row selection via TanStack's selection state
 */
const columns: ColumnDef<Trade>[] = [
  {
    accessorKey: 'timestamp',
    header: 'Time',
    cell: ({ getValue }) => {
      const ts = getValue<number>();
      return new Date(ts).toLocaleTimeString();
    },
    // TanStack Table feature: Enable sorting for this column
    enableSorting: true,
  },
  {
    accessorKey: 'symbol',
    header: 'Symbol',
    enableSorting: true,
  },
  {
    accessorKey: 'side',
    header: 'Side',
    cell: ({ getValue }) => {
      const side = getValue<'BUY' | 'SELL'>();
      return (
        <span className={side === 'BUY' ? 'text-green-400' : 'text-red-400'}>
          {side}
        </span>
      );
    },
  },
  {
    accessorKey: 'qty',
    header: 'Quantity',
    cell: ({ getValue }) => getValue<number>().toFixed(2),
    enableSorting: true,
  },
  {
    accessorKey: 'price',
    header: 'Price',
    cell: ({ getValue }) => `$${getValue<number>().toFixed(2)}`,
    enableSorting: true,
  },
  {
    accessorKey: 'pnl',
    header: 'PnL',
    cell: ({ getValue }) => {
      const pnl = getValue<number>();
      return (
        <span className={pnl > 0 ? 'text-green-400' : 'text-red-400'}>
          {pnl > 0 ? '+' : ''}${pnl.toFixed(2)}
        </span>
      );
    },
    enableSorting: true,
  },
];

/**
 * TanStack Table Features Demonstrated:
 *
 * 1. Sorting - Click column headers to sort
 * 2. Row Selection - Select individual rows (controlled state)
 * 3. Custom Cell Rendering - Colored text, formatted numbers
 * 4. Type Safety - Full TypeScript support with generics
 * 5. Data Transformation - Cell renderers transform raw data
 */
export function TanStackTableDemo() {
  const [rowSelection, setRowSelection] = useState<RowSelectionState>({});

  const handleRowClick = (trade: Trade) => {
    console.log('Clicked trade:', trade);
    console.log('TanStack Table powered this interaction!');
  };

  return (
    <div className="p-4 space-y-4">
      <div>
        <h2 className="text-lg font-semibold mb-2">
          TanStack Table Integration Demo
        </h2>
        <p className="text-sm text-gray-400 mb-4">
          This table is powered by @tanstack/react-table v8.21.3
        </p>
      </div>

      <DataTable
        data={mockTrades}
        columns={columns}
        sortable
        dense
        onRowClick={handleRowClick}
        enableRowSelection
        rowSelection={rowSelection}
        onRowSelectionChange={setRowSelection}
        emptyMessage="No trades to display"
      />

      <div className="text-xs text-gray-400 space-y-1">
        <p>Features:</p>
        <ul className="list-disc list-inside space-y-1">
          <li>Click column headers to sort (TanStack's getSortedRowModel)</li>
          <li>Row selection state managed by TanStack</li>
          <li>Custom cell renderers via TanStack's cell property</li>
          <li>Type-safe column definitions with ColumnDef&lt;Trade&gt;</li>
          <li>Memoized table configuration for performance</li>
        </ul>
      </div>

      <div className="p-3 bg-neutral-900 rounded text-xs">
        <div className="font-semibold mb-2">Selected Rows (TanStack State):</div>
        <pre className="text-gray-400">
          {JSON.stringify(rowSelection, null, 2) || '{}'}
        </pre>
      </div>
    </div>
  );
}

/**
 * TanStack Table API Exposed:
 *
 * The DataTable component uses these TanStack Table APIs:
 *
 * - useReactTable(): Main hook for table setup
 * - getCoreRowModel(): Core row model
 * - getSortedRowModel(): Sorting functionality
 * - flexRender(): Renders cells/headers
 * - getHeaderGroups(): Header row groups
 * - getRowModel(): Data rows
 * - getIsSelected(): Row selection state
 * - toggleSorting(): Sort toggle
 * - getCanSort(): Check if column is sortable
 *
 * All features are properly typed and type-safe!
 */
