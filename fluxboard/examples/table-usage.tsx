/**
 * DataTable Usage Examples
 *
 * Demonstrates various use cases for the DataTable component with TanStack Table.
 */

import { useState } from 'react';
import { type ColumnDef } from '@tanstack/react-table';
import { DataTable } from '@/components/ui/table';

// =============================================================================
// EXAMPLE 1: Basic Trade Table
// =============================================================================

interface Trade {
  id: string;
  timestamp: number;
  symbol: string;
  side: 'BUY' | 'SELL';
  qty: number;
  price: number;
  pnl: number;
}

const tradeColumns: ColumnDef<Trade>[] = [
  {
    accessorKey: 'timestamp',
    header: 'Time',
    cell: ({ getValue }) => {
      const ts = getValue<number>();
      return new Date(ts).toLocaleTimeString();
    },
  },
  {
    accessorKey: 'symbol',
    header: 'Symbol',
  },
  {
    accessorKey: 'side',
    header: 'Side',
    cell: ({ getValue }) => {
      const side = getValue<'BUY' | 'SELL'>();
      return (
        <span
          className={side === 'BUY' ? 'text-green-400' : 'text-red-400'}
        >
          {side}
        </span>
      );
    },
  },
  {
    accessorKey: 'qty',
    header: 'Qty',
    cell: ({ getValue }) => getValue<number>().toFixed(2),
  },
  {
    accessorKey: 'price',
    header: 'Price',
    cell: ({ getValue }) => `$${getValue<number>().toFixed(2)}`,
  },
  {
    accessorKey: 'pnl',
    header: 'PnL',
    cell: ({ getValue }) => {
      const pnl = getValue<number>();
      const color = pnl > 0 ? 'text-green-400' : 'text-red-400';
      return (
        <span className={color}>
          {pnl > 0 ? '+' : ''}
          {pnl.toFixed(2)}
        </span>
      );
    },
  },
];

export function BasicTradeTable() {
  const [trades] = useState<Trade[]>([
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
  ]);

  return (
    <DataTable
      data={trades}
      columns={tradeColumns}
      sortable
      dense
      emptyMessage="No trades executed"
    />
  );
}

// =============================================================================
// EXAMPLE 2: Interactive Table with Row Selection
// =============================================================================

interface Position {
  symbol: string;
  qty: number;
  avgPrice: number;
  currentPrice: number;
  unrealizedPnL: number;
}

const positionColumns: ColumnDef<Position>[] = [
  {
    accessorKey: 'symbol',
    header: 'Symbol',
  },
  {
    accessorKey: 'qty',
    header: 'Quantity',
    cell: ({ getValue }) => getValue<number>().toFixed(4),
  },
  {
    accessorKey: 'avgPrice',
    header: 'Avg Price',
    cell: ({ getValue }) => `$${getValue<number>().toFixed(2)}`,
  },
  {
    accessorKey: 'currentPrice',
    header: 'Current',
    cell: ({ getValue }) => `$${getValue<number>().toFixed(2)}`,
  },
  {
    accessorKey: 'unrealizedPnL',
    header: 'Unrealized PnL',
    cell: ({ getValue }) => {
      const pnl = getValue<number>();
      return (
        <span className={pnl > 0 ? 'text-green-400' : 'text-red-400'}>
          {pnl > 0 ? '+' : ''}${pnl.toFixed(2)}
        </span>
      );
    },
  },
];

export function InteractivePositionTable() {
  const [positions] = useState<Position[]>([
    {
      symbol: 'PLUME/USDT',
      qty: 1000,
      avgPrice: 1.20,
      currentPrice: 1.25,
      unrealizedPnL: 50.00,
    },
    {
      symbol: 'WETH/USDT',
      qty: 2.5,
      avgPrice: 2450.00,
      currentPrice: 2500.00,
      unrealizedPnL: 125.00,
    },
  ]);

  const handleRowClick = (position: Position) => {
    console.log('Selected position:', position);
    // Could open a detail modal or navigate to position page
  };

  return (
    <DataTable
      data={positions}
      columns={positionColumns}
      sortable
      onRowClick={handleRowClick}
      emptyMessage="No open positions"
    />
  );
}

// =============================================================================
// EXAMPLE 3: Loading State
// =============================================================================

export function LoadingTable() {
  const [loading] = useState(true);
  const [data] = useState<Trade[]>([]);

  return (
    <DataTable
      data={data}
      columns={tradeColumns}
      loading={loading}
      emptyMessage="No data available"
    />
  );
}

// =============================================================================
// EXAMPLE 4: Dense vs Normal Mode
// =============================================================================

export function DensityComparison() {
  const sampleData: Trade[] = [
    {
      id: '1',
      timestamp: Date.now(),
      symbol: 'PLUME/USDT',
      side: 'BUY',
      qty: 100,
      price: 1.25,
      pnl: 5.43,
    },
  ];

  return (
    <div className="space-y-4">
      <div>
        <h3 className="text-sm font-semibold mb-2">Dense Mode (24px rows)</h3>
        <DataTable
          data={sampleData}
          columns={tradeColumns}
          dense
        />
      </div>

      <div>
        <h3 className="text-sm font-semibold mb-2">Normal Mode (28px rows)</h3>
        <DataTable
          data={sampleData}
          columns={tradeColumns}
          dense={false}
        />
      </div>
    </div>
  );
}

// =============================================================================
// EXAMPLE 5: Custom Cell Rendering
// =============================================================================

interface Alert {
  id: string;
  level: 'CRITICAL' | 'WARNING' | 'INFO';
  message: string;
  timestamp: number;
}

const alertColumns: ColumnDef<Alert>[] = [
  {
    accessorKey: 'level',
    header: 'Level',
    cell: ({ getValue }) => {
      const level = getValue<Alert['level']>();
      const colors = {
        CRITICAL: 'bg-red-900 text-red-400 border-red-700',
        WARNING: 'bg-amber-900 text-amber-400 border-amber-700',
        INFO: 'bg-blue-900 text-blue-400 border-blue-700',
      };
      return (
        <span
          className={`px-2 py-0.5 rounded text-xs border ${colors[level]}`}
        >
          {level}
        </span>
      );
    },
  },
  {
    accessorKey: 'message',
    header: 'Message',
  },
  {
    accessorKey: 'timestamp',
    header: 'Time',
    cell: ({ getValue }) => {
      const ts = getValue<number>();
      return new Date(ts).toLocaleString();
    },
  },
];

export function AlertTable() {
  const [alerts] = useState<Alert[]>([
    {
      id: '1',
      level: 'CRITICAL',
      message: 'Balance check failed for Bybit',
      timestamp: Date.now(),
    },
    {
      id: '2',
      level: 'WARNING',
      message: 'Market data stale for PLUME/USDT',
      timestamp: Date.now() - 5000,
    },
    {
      id: '3',
      level: 'INFO',
      message: 'Strategy enabled: rooster_bybit_pusdplume',
      timestamp: Date.now() - 10000,
    },
  ]);

  return (
    <DataTable
      data={alerts}
      columns={alertColumns}
      sortable
      emptyMessage="No alerts"
    />
  );
}

// =============================================================================
// EXAMPLE 6: Controlled Sorting
// =============================================================================

export function ControlledSortTable() {
  const [data] = useState<Trade[]>([
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
  ]);

  return (
    <div className="space-y-2">
      <div className="text-xs text-gray-400">
        Click column headers to sort (supports multi-column sorting with Shift+Click)
      </div>
      <DataTable
        data={data}
        columns={tradeColumns}
        sortable
        dense
      />
    </div>
  );
}
