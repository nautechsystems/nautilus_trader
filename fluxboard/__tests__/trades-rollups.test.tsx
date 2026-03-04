import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';

// Mock sockets to avoid WS
vi.mock('../sockets', () => ({ socket: { on: () => {}, off: () => {}, connected: true } }));

// Mock store with a single trade to compute rollups
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
      gas_used: 0.001,
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

vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: () => <div data-testid="trades-table" />,
}));

import Trades from '../Trades';

describe('Trades footer rollups', () => {
  it('renders sums for qty, notional, fee, gas', () => {
    render(<Trades />);
    expect(screen.getByText(/Σ qty:/)).toBeInTheDocument();
    expect(screen.getByText(/Σ notional:/)).toBeInTheDocument();
    expect(screen.getByText(/Σ fee:/)).toBeInTheDocument();
    expect(screen.getByText(/Σ gas:/)).toBeInTheDocument();
  });
});

