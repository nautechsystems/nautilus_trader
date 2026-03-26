import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { getTrades, getTradesDelta, realtimeFlags, socketMock } = vi.hoisted(() => ({
  getTrades: vi.fn(),
  getTradesDelta: vi.fn(),
  realtimeFlags: {
    trades: false,
  },
  socketMock: {
    on: vi.fn(),
    off: vi.fn(),
    emit: vi.fn(),
    connected: true,
  },
}));

vi.mock('../api', async (importOriginal) => {
  const mod = await importOriginal<any>();
  return {
    ...mod,
    api: {
      ...mod.api,
      getTrades,
      getTradesDelta,
    },
    deriveCanonicalNaming: vi.fn(() => ({})),
  };
});

vi.mock('../sockets', () => ({
  socket: socketMock,
  standardSocketClient: {
    subscribe: vi.fn(() => () => undefined),
  },
}));

vi.mock('../config/featureFlags', async (importOriginal) => {
  const mod = await importOriginal<any>();
  return {
    ...mod,
    isRealtimeStandardEnabled: (surface: string) => surface === 'trades' && realtimeFlags.trades,
  };
});

vi.mock('../utils/sound', () => ({
  playTradeClick: vi.fn(),
}));

vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: () => <div data-testid="trades-table" />,
}));

import Trades from '../Trades';
import { useResyncStore, useTradesStore } from '../stores';

const baseRows = [
  {
    row_id: 'trade-1',
    seq: 1,
    version: 1,
    ts: 1,
    time: '2025-01-01T00:00:01Z',
    coin: 'PLUME/USDT',
    exchange: 'bybit',
    side: 'buy',
    price: 100,
  },
];

describe('Trades status banner', () => {
  beforeEach(() => {
    window.sessionStorage.clear();
    getTrades.mockReset();
    getTradesDelta.mockReset();
    socketMock.on.mockClear();
    socketMock.off.mockClear();
    socketMock.emit.mockClear();
    socketMock.connected = true;
    realtimeFlags.trades = false;
    useTradesStore.getState().clear();
    useResyncStore.getState().resetResyncState();
    getTrades.mockResolvedValue({
      rows: baseRows,
      total: baseRows.length,
      page: 1,
      page_size: 100,
      last_seq: 1,
      has_more: false,
      next_cursor: null,
    });
    getTradesDelta.mockResolvedValue({
      rows: [],
      last_seq: 1,
      reset_required: false,
    });
  });

  afterEach(() => {
    cleanup();
  });

  it('shows LIVE after a fresh snapshot-only load even when the socket starts disconnected', async () => {
    socketMock.connected = false;

    render(<Trades />);

    await waitFor(() => expect(getTrades).toHaveBeenCalled());
    expect(screen.getByText('LIVE')).toBeInTheDocument();
    expect(screen.queryByText(/OFFLINE - Reconnecting/i)).toBeNull();
  });

  it('shows LIVE for a fresh non-canonical snapshot view without realtime lineage', async () => {
    realtimeFlags.trades = true;
    window.sessionStorage.setItem('trades_filters', JSON.stringify({ exchange: 'bybit' }));
    getTrades.mockResolvedValue({
      rows: baseRows,
      total: baseRows.length,
      page: 1,
      page_size: 50,
      last_seq: 1,
      has_more: false,
      next_cursor: null,
    });

    render(<Trades />);

    await waitFor(() => expect(getTrades).toHaveBeenCalled());
    expect(screen.getByText('LIVE')).toBeInTheDocument();
    expect(screen.queryByText('RECOVERING')).toBeNull();
    expect(screen.queryByText(/RECOVERING - Replaying/i)).toBeNull();
  });
});
