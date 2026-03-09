import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';

vi.mock('../utils/export', async (importOriginal) => {
  const mod = await importOriginal<any>();
  return {
    ...mod,
    exportCSV: vi.fn(),
    generateTimestampFilename: () => 'trades_2000-01-01T00-00-00.csv',
  };
});

// Mock Zustand store selector for Trades page
vi.mock('../stores', async (importOriginal) => {
  const mod = await importOriginal<any>();
  const mockRows = [
    {
      row_id: 'r1',
      time: '2025-11-10T00:00:00Z',
      coin: 'ETH/USDT',
      exchange: 'bybit',
      side: 'buy',
      price: 3500,
      qty: 0.01,
      mv: 35,
      fee: 0.02,
      gas_used: 0,
      seq: 1,
      version: 1,
      ts: 1,
    },
  ];
  return {
    ...mod,
    useTradesStore: (selector?: any) => {
      const state = {
        rows: mockRows,
        byId: new Map(mockRows.map((r) => [r.row_id, r])),
        order: mockRows.map((r) => r.row_id),
        lastSeq: 1,
        lastUpdate: Date.now(),
        setSnapshot: () => {},
        applyDelta: () => ({ upserts: 0, deletes: 0, changed: false }),
        appendHistorical: () => {},
        clear: () => {},
      };
      return selector ? selector(state) : state;
    },
    selectTradesRows: (s: any) => s.rows,
    selectTradesLastSeq: (s: any) => s.lastSeq,
    shallow: (a: any, b: any) => a === b,
  };
});

// Mock sockets to avoid real connection
vi.mock('../sockets', () => ({ socket: { on: () => {}, off: () => {}, connected: true } }));

// Minimal components used by Trades
vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: ({ trades }: any) => (
    <div data-testid="trades-table">{trades?.length ?? 0} rows</div>
  )
}));

import Trades from '../Trades';
import { exportCSV } from '../utils/export';

describe('Trades export CSV', () => {
  beforeEach(() => {
    (exportCSV as any).mockClear?.();
  });

  it('renders Export CSV button and triggers export', async () => {
    render(<Trades />);
    const btn = await screen.findByRole('button', { name: /Export CSV/i });
    expect(btn).toBeInTheDocument();
    fireEvent.click(btn);
    expect(exportCSV).toHaveBeenCalledTimes(1);
    const [data, filename] = (exportCSV as any).mock.calls[0];
    expect(Array.isArray(data)).toBe(true);
    expect(filename).toMatch(/trades_/);
  });
});

