import { createColumnHelper, type ColumnDef } from '@tanstack/react-table';
import type { MouseEvent } from 'react';
import type { TradeRow } from '../../types';
import { fmtTime, fmtTimeTip, num, shortId, truncate } from './formatters';
import { Button } from '../ui/button/Button';
import { SideCell } from './SidePill';
import { colors, typography } from '@/lib/tokens';
import { COLUMN_MAP } from '@/config/columnMap';
import { useCopyToClipboard } from '@/hooks/useCopyToClipboard';

const columnHelper = createColumnHelper<TradeRow>();

// Copyable ID cell component
function CopyableIdCell({ value, label }: { value?: string; label: string }) {
  const copyToClipboard = useCopyToClipboard();

  if (!value) {
    return (
      <span
        className="block"
        style={{ color: colors.text.muted }}
      >
        —
      </span>
    );
  }

  const handleClick = (e: MouseEvent) => {
    e.stopPropagation();
    copyToClipboard(value, { successMessage: `${label} copied` });
  };

  return (
    <span
      className="block hover:underline transition-colors"
      style={{
        color: colors.text.secondary,
        cursor: 'pointer',
      }}
      onClick={handleClick}
      title={`${value} (click to copy)`}
    >
      {shortId(value)}
    </span>
  );
}

type ColumnOptions = {
  enableDecisionDetails?: boolean;
  visibleColumns?: string[];
};

export const createColumns = (
  onViewDecision: (t: TradeRow) => void,
  options?: ColumnOptions,
) => {
  const cols: ColumnDef<TradeRow, any>[] = [
    columnHelper.accessor('time', {
    id: 'time',
    header: 'time',
    cell: (info) => {
      const iso = info.getValue();
      return (
        <span title={fmtTimeTip(iso)} className="whitespace-nowrap">
          {fmtTime(iso)}
        </span>
      );
    },
    size: COLUMN_MAP.timeShort.min,
    minSize: COLUMN_MAP.timeShort.min,
    enableSorting: false,
  }),
  columnHelper.accessor((row) => {
    const venue = (row.venue || '').toLowerCase();
    const exch = (row.exchange || '').toLowerCase();
    if (venue === 'cex' && ['bybit', 'bitget', 'binance', 'kraken', 'coinbase'].includes(exch)) {
      return row.symbol || row.coin;
    }
    return row.coin || row.symbol;
  }, {
    id: 'coin',
    header: 'coin',
    cell: (info) => (
      <span className="whitespace-nowrap">
        {info.getValue() || '—'}
      </span>
    ),
    size: COLUMN_MAP.coin.min,
    minSize: COLUMN_MAP.coin.min,
    enableSorting: false,
  }),
  columnHelper.accessor('exchange', {
    id: 'exch',
    header: 'exch',
    cell: (info) => {
      const exchange = info.getValue();
      return (
        <span className="whitespace-nowrap">
          {exchange || '—'}
        </span>
      );
    },
    size: COLUMN_MAP.exch.min,
    minSize: COLUMN_MAP.exch.min,
    enableSorting: false,
  }),
  columnHelper.accessor('side', {
    id: 'side',
    header: 'side',
    cell: (info) => {
      const side = info.getValue();
      return <SideCell v={side || ''} />;
    },
    size: COLUMN_MAP.side.min,
    minSize: COLUMN_MAP.side.min,
    enableSorting: false,
  }),
  columnHelper.accessor('price', {
    id: 'px',
    header: 'px',
    cell: (info) => {
      const v = info.getValue();
      const n = typeof v === 'string' ? parseFloat(v) : v;
      return (
        <span
          className="text-right tabular-nums block w-full font-mono"
          style={{
            fontSize: typography.fontSize.sm,
            color: colors.text.secondary,
          }}
        >
          {num(n, 6)}
        </span>
      );
    },
    size: COLUMN_MAP.px.min,
    minSize: COLUMN_MAP.px.min,
    enableSorting: false,
  }),
  columnHelper.accessor('qty', {
    id: 'qty',
    header: 'qty',
    cell: (info) => {
      const v = info.getValue();
      const n = typeof v === 'string' ? parseFloat(v) : v;
      return (
        <span
          className="text-right tabular-nums block w-full font-mono"
          style={{
            fontSize: typography.fontSize.sm,
            color: colors.text.secondary,
          }}
        >
          {num(n, 3)}
        </span>
      );
    },
    size: COLUMN_MAP.qty.min,
    minSize: COLUMN_MAP.qty.min,
    enableSorting: false,
  }),
  columnHelper.accessor('mv', {
    id: 'notional',
    header: 'notional',
    cell: (info) => {
      const v = info.getValue();
      const n = typeof v === 'string' ? parseFloat(v) : v;
      return (
        <span
          className="text-right tabular-nums block w-full font-mono"
          style={{
            fontSize: typography.fontSize.sm,
            color: colors.text.secondary,
          }}
        >
          {num(n, 3)}
        </span>
      );
    },
    size: COLUMN_MAP.notional.min,
    minSize: COLUMN_MAP.notional.min,
    enableSorting: false,
  }),
  // Fee column – prefer fee_quote (fee in quote asset) and show raw asset in tooltip
  columnHelper.accessor((row) => row.fee_quote ?? row.fee, {
    id: 'fee',
    header: 'fee (quote)',
    cell: (info) => {
      const row = info.row.original;
      const v = info.getValue();
      const n = typeof v === 'string' ? parseFloat(v) : v;
      const asset = row.fee_asset_raw || '';
      const label = Number.isFinite(n as number) ? num(n as number, 8) : '—';
      const fullValue = (() => {
        if (typeof v === 'string') {
          const s = v.trim();
          return s || '—';
        }
        if (typeof n === 'number' && Number.isFinite(n)) {
          return String(n);
        }
        return '—';
      })();
      const tooltip = fullValue === '—' ? '—' : (asset ? `${fullValue} ${asset}` : fullValue);
      return (
        <span
          className="text-right tabular-nums block w-full font-mono"
          style={{
            fontSize: typography.fontSize.sm,
            color: colors.text.secondary,
          }}
          title={tooltip}
        >
          {label}
        </span>
      );
    },
    size: COLUMN_MAP.fee.min,
    minSize: COLUMN_MAP.fee.min,
    enableSorting: false,
  }),
  columnHelper.accessor('trade_id', {
    id: 'trd_id',
    header: 'trd_id',
    cell: (info) => <CopyableIdCell value={info.getValue()} label="Trade ID" />,
    size: COLUMN_MAP.id.min,
    minSize: COLUMN_MAP.id.min,
    enableSorting: false,
  }),
  columnHelper.accessor('signal_id', {
    id: 'signal',
    header: 'signal',
    cell: (info) => <CopyableIdCell value={info.getValue()} label="Signal ID" />,
    size: COLUMN_MAP.id.min,
    minSize: COLUMN_MAP.id.min,
    enableSorting: false,
  }),
  columnHelper.accessor('strategy_id', {
    id: 'strategy',
    header: 'strategy',
    cell: (info) => <CopyableIdCell value={info.getValue()} label="Strategy ID" />,
    size: COLUMN_MAP.id.min,
    minSize: COLUMN_MAP.id.min,
    enableSorting: false,
  }),
    columnHelper.accessor('order_id', {
    id: 'ord_id',
    header: 'ord_id',
    cell: (info) => <CopyableIdCell value={info.getValue()} label="Order ID" />,
    size: 120,
    minSize: 120,
    enableSorting: false,
  }),
  ];

  if (options?.enableDecisionDetails) {
    cols.push(
      columnHelper.display({
        id: 'decision',
        header: 'decision',
        cell: (info) => {
          const hasDecision = !!info.row.original.decision;
          if (!hasDecision) {
            return <span style={{ color: colors.text.muted }}>—</span>;
          }
          return (
            <Button
              variant="ghost"
              size="xs"
              onClick={(e) => {
                e.stopPropagation();
                onViewDecision(info.row.original);
              }}
              style={{ color: colors.semantic.info.light }}
            >
              View
            </Button>
          );
        },
        size: COLUMN_MAP.decision.min,
        minSize: COLUMN_MAP.decision.min,
        enableSorting: false,
      })
    );
  }

  cols.push(
    columnHelper.accessor('notes', {
    id: 'notes',
    header: 'notes',
    cell: (info) => {
      const v = info.getValue();
      return (
        <span
          className="block overflow-hidden text-ellipsis"
          style={{
            color: colors.text.muted,
            maxWidth: '320px',
            whiteSpace: 'nowrap',
          }}
          title={v || ''}
        >
          {v ? truncate(v, 50) : '—'}
        </span>
      );
    },
    size: COLUMN_MAP.notes.min,
    minSize: 200,
    enableSorting: false,
    })
  );

  if (options?.visibleColumns && options.visibleColumns.length > 0) {
    const allowed = new Set(options.visibleColumns);
    return cols.filter((col) => typeof col.id === 'string' && allowed.has(col.id as string));
  }

  return cols;
};
