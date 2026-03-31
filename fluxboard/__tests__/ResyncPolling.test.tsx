import { act, cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import Trades from '../Trades';
import { api } from '../api';
import { markGlobalResyncApplied, useResyncStore, useTradesStore } from '../stores';

const { realtimeFlags } = vi.hoisted(() => ({
  realtimeFlags: {
    trades: false,
  },
}));

vi.mock('../api', () => ({
  api: {
    getTrades: vi.fn(),
    getTradesDelta: vi.fn(),
  },
}));

vi.mock('../sockets', () => ({
  socket: {
    on: vi.fn(),
    off: vi.fn(),
    connected: true,
  },
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

vi.mock('../stores', async (importOriginal) => {
  const mod = await importOriginal<any>();
  return {
    ...mod,
    useTradesStore: vi.fn(),
    selectTradesRows: (state: any) => state.rows ?? [],
    selectTradesLastSeq: (state: any) => state.lastSeq ?? 0,
    markGlobalResyncApplied: vi.fn(),
    shallow: () => false,
  };
});

vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: () => <div data-testid="trades-table-mock">rows</div>,
}));

const mockGetTrades = vi.mocked(api.getTrades);
const mockGetTradesDelta = vi.mocked(api.getTradesDelta);
const mockMarkGlobalResyncApplied = vi.mocked(markGlobalResyncApplied);

const standardRealtimeLineage = {
  contract_version: 2,
  surface: 'trades',
  profile: 'tokenmm',
  surface_query_key: 'tokenmm.trades',
  stream_id: 'trades-main',
  snapshot_revision: 'snap-1',
  last_seq: 0,
};

function setupTradesStore() {
  const setSnapshot = vi.fn().mockReturnValue({
    accepted: true,
    applied: true,
    staleRejected: false,
  });
  const applyDelta = vi.fn().mockReturnValue({
    accepted: true,
    applied: false,
    staleRejected: 0,
    newRows: 0,
    updatedRows: 0,
    deletedRows: 0,
    upserts: 0,
    deletes: 0,
    changed: false,
  });

  const store = {
    rows: [],
    byId: new Map(),
    order: [],
    lastSeq: 0,
    setSnapshot,
    applyDelta,
    appendHistorical: vi.fn(),
    clear: vi.fn(),
  };

  (useTradesStore as unknown as { mockImplementation: (fn?: (state: typeof store) => unknown) => void })
    .mockImplementation((selector?: (state: typeof store) => unknown) =>
      (typeof selector === 'function' ? selector(store) : store));

  return { setSnapshot, applyDelta };
}

describe('Trades resync clearing behavior', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    realtimeFlags.trades = true;
    mockGetTrades.mockReset();
    mockGetTradesDelta.mockReset();
    mockMarkGlobalResyncApplied.mockReset();
    (useTradesStore as unknown as { mockReset: () => void }).mockReset();
    useResyncStore.getState().resetResyncState();

    setupTradesStore();

    mockGetTrades.mockResolvedValue({
      rows: [],
      total: 0,
      page: 1,
      page_size: 100,
      last_seq: 0,
      has_more: false,
      next_cursor: null,
      realtime: standardRealtimeLineage,
    });
    mockGetTradesDelta.mockResolvedValue({
      rows: [],
      last_seq: 0,
      reset_required: false,
    });
  });

  afterEach(() => {
    act(() => {
      vi.runOnlyPendingTimers();
    });
    vi.useRealTimers();
    cleanup();
  });

  it('marks reconnect resync as applied when an empty delta replay advances last_seq', async () => {
    const view = render(<Trades />);

    await act(async () => {
      await Promise.resolve();
    });
    expect(mockGetTrades).toHaveBeenCalled();
    expect(useResyncStore.getState().mountedConsumers.trades).toBe(1);

    mockMarkGlobalResyncApplied.mockClear();

    let reconnectResyncId = 0;
    await act(async () => {
      reconnectResyncId = useResyncStore.getState().bumpResync('socket-reconnect');
    });
    expect(useResyncStore.getState().isResyncing).toBe(true);

    await act(async () => {
      vi.advanceTimersByTime(1200);
      await Promise.resolve();
    });

    expect(mockGetTradesDelta).toHaveBeenCalled();
    expect(mockMarkGlobalResyncApplied).toHaveBeenCalledWith('trades', reconnectResyncId);
    view.unmount();
    expect(useResyncStore.getState().mountedConsumers.trades).toBeUndefined();
  });

  it('does not mark reconnect resync as applied when a no-op replay omits last_seq', async () => {
    mockGetTradesDelta.mockResolvedValue({
      rows: [],
      reset_required: false,
    });

    render(<Trades />);

    await act(async () => {
      await Promise.resolve();
    });
    expect(mockGetTrades).toHaveBeenCalled();

    mockMarkGlobalResyncApplied.mockClear();

    let reconnectResyncId = 0;
    await act(async () => {
      reconnectResyncId = useResyncStore.getState().bumpResync('socket-reconnect');
    });
    expect(useResyncStore.getState().isResyncing).toBe(true);

    await act(async () => {
      vi.advanceTimersByTime(1200);
      await Promise.resolve();
    });

    expect(mockGetTradesDelta).toHaveBeenCalled();
    expect(mockMarkGlobalResyncApplied).not.toHaveBeenCalledWith('trades', reconnectResyncId);
  });
});
