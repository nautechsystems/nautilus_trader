// FX table with sorting

import { useMemo } from 'react';
import { type ColumnDef } from '@tanstack/react-table';
import type { FxPair } from './types';
import { bpsFromPar, formatDecimal, fmtAgeSec } from './utils';
import { DataTable } from './components/ui/table/DataTable';
import { Badge } from './components/ui/badge';
import StatusDot from './components/ui/badge/StatusDot';
import FxStatusPill from './FxStatusPill';
import { useMobileLayout } from '@/hooks/useMobileLayout';

type Props = {
  pairs: FxPair[];
};

export default function FxTable({ pairs }: Props) {
  const { isMobile } = useMobileLayout();
  // Define columns for DataTable
  const columns = useMemo<ColumnDef<FxPair>[]>(
    () => [
      {
        accessorKey: 'pair',
        header: 'Pair',
        enableSorting: true,
        cell: ({ row }) => (
          <span className="font-medium">{row.original.pair}</span>
        ),
      },
      {
        accessorKey: 'price',
        header: 'Rate',
        enableSorting: true,
        cell: ({ row }) => (
          <span className="font-mono">{formatDecimal(row.original.price, 6)}</span>
        ),
        meta: {
          align: 'right',
        },
      },
      {
        id: 'bps',
        header: 'Δ from 1.0000 (bps)',
        enableSorting: true,
        accessorFn: (row) => bpsFromPar(row.price),
        cell: ({ row }) => {
          const bps = bpsFromPar(row.original.price);
          return (
            <span className="font-mono">
              {bps >= 0 ? '+' : ''}{bps}
            </span>
          );
        },
        meta: {
          align: 'right',
        },
      },
      {
        accessorKey: 'source',
        header: 'Source',
        enableSorting: true,
        cell: ({ row }) => {
          const source = row.original.source;
          return (
            <Badge
              variant={source === 'bybit' ? 'success' : 'warning'}
              size="xs"
            >
              {source}
            </Badge>
          );
        },
      },
      {
        accessorKey: 'age_ms',
        header: 'Age',
        enableSorting: true,
        cell: ({ row }) => {
          const age = row.original.age_ms;
          const isStale = age > 10000; // 10s threshold

          return (
            <div className="flex items-center justify-end gap-2">
              <StatusDot
                status={isStale ? 'stale' : 'live'}
                size="xs"
              />
              <span className="font-mono">{fmtAgeSec(age)}</span>
            </div>
          );
        },
        meta: {
          align: 'right',
        },
      },
      {
        id: 'status',
        header: 'Status',
        enableSorting: false,
        cell: ({ row }) => <FxStatusPill pair={row.original} />,
      },
    ],
    []
  );

  return (
    <div className="overflow-x-auto">
      <DataTable
        data={pairs}
        columns={columns}
        sortable
        dense
        emptyMessage="No pairs reported"
        primaryColumns={isMobile ? ['pair', 'price', 'age_ms'] : undefined}
      />
    </div>
  );
}
