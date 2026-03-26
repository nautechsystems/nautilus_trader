import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { TradesTable } from './TradesTable';
import type { TradeRow } from '../../types';

const { cellRenderCounts } = vi.hoisted(() => ({
  cellRenderCounts: new Map<string, number>(),
}));

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getVirtualItems: () => Array.from({ length: count }, (_, index) => ({
      index,
      start: index * 28,
      size: 28,
    })),
    getTotalSize: () => count * 28,
  }),
}));

vi.mock('./columns', () => ({
  createColumns: () => [
    {
      id: 'price',
      accessorFn: (row: TradeRow) => row.price,
      header: 'price',
      cell: (info: { row: { original: TradeRow }; getValue: () => unknown }) => {
        const rowId = info.row.original.row_id;
        cellRenderCounts.set(rowId, (cellRenderCounts.get(rowId) ?? 0) + 1);
        return <span>{String(info.getValue() ?? '')}</span>;
      },
    },
  ],
}));

vi.mock('./DecisionModal', () => ({
  DecisionModal: () => null,
}));

vi.mock('@/hooks/useMobileLayout', () => ({
  useMobileLayout: () => ({
    viewport: 'desktop',
    isMobile: false,
    isMobileViewport: false,
    density: 'desktop',
    isTouch: false,
    width: 1280,
    height: 720,
  }),
}));

const makeTradeRow = (overrides: Partial<TradeRow> = {}): TradeRow => ({
  time: '2025-01-01T00:00:00.000Z',
  coin: 'PLUME',
  exchange: 'bybit',
  side: 'buy',
  price: 100,
  qty: 10,
  mv: 1000,
  fee: 0.1,
  trade_id: 'trade-1',
  exch_id: 'exec-1',
  order_id: 'order-1',
  signal_id: 'signal-1',
  row_id: 'row-1',
  version: 1,
  seq: 1,
  ts: 1,
  ...overrides,
});

afterEach(() => {
  cellRenderCounts.clear();
});

describe('TradesTable live update rendering', () => {
  it('renders when decision details are enabled', () => {
    expect(() => {
      render(<TradesTable trades={[]} enableDecisionDetails />);
    }).not.toThrow();
  });

  it('rerenders only the changed visible row for in-place live updates', () => {
    const trades = [
      makeTradeRow({ row_id: 'alpha', version: 1, seq: 1, ts: 1, price: 101 }),
      makeTradeRow({ row_id: 'beta', version: 1, seq: 2, ts: 2, price: 202 }),
      makeTradeRow({ row_id: 'gamma', version: 1, seq: 3, ts: 3, price: 303 }),
    ];

    const { rerender } = render(<TradesTable trades={trades} liveDataVersion={0} />);
    const baselineCounts = Object.fromEntries(cellRenderCounts);

    trades[1].price = 999;
    trades[1].version = 2;
    trades[1].seq = 4;

    rerender(<TradesTable trades={trades} liveDataVersion={1} />);

    expect(Object.fromEntries(cellRenderCounts)).toEqual({
      alpha: baselineCounts.alpha,
      beta: baselineCounts.beta + 1,
      gamma: baselineCounts.gamma,
    });
  });
});
