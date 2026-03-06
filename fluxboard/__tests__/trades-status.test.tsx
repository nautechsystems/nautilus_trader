import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';

// Mock sockets with disconnected state for OFFLINE
vi.mock('../sockets', () => ({ socket: { on: () => {}, off: () => {}, connected: false } }));

// Mock store with minimal rows
vi.mock('../stores', async (importOriginal) => {
  const mod = await importOriginal<any>();
  const mockRows: any[] = [];
  return {
    ...mod,
    useTradesStore: (selector?: any) => {
      const state = {
        rows: mockRows,
        byId: new Map(),
        order: [],
        lastSeq: 0,
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

describe('Trades LIVE/STALE/OFFLINE banner', () => {
  it('shows OFFLINE banner when socket disconnected', () => {
    render(<Trades />);
    expect(screen.getByText(/OFFLINE — Reconnecting…/)).toBeInTheDocument();
  });
});
