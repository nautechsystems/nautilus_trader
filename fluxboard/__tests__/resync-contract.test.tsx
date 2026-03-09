import { act, cleanup, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import Trades from '../Trades';
import { api } from '../api';
import { markGlobalResyncApplied, useResyncStore, useTradesStore } from '../stores';

const { socketMock, getTrades, getTradesDelta } = vi.hoisted(() => {
  const socketMock = {
    on: vi.fn(),
    off: vi.fn(),
    connected: true,
  };
  return {
    socketMock,
    getTrades: vi.fn(),
    getTradesDelta: vi.fn(),
  };
});

vi.mock('../api', async (importOriginal) => {
  const mod = await importOriginal<any>();
  return {
    ...mod,
    api: {
      ...mod.api,
      getTrades,
      getTradesDelta,
    },
  };
});

vi.mock('../sockets', () => ({ socket: socketMock }));

vi.mock('../utils/sound', () => ({
  playTradeClick: vi.fn(),
}));

vi.mock('../stores', async (importOriginal) => {
  const mod = await importOriginal<any>();
  return {
    ...mod,
    useTradesStore: vi.fn(),
    useResyncStore: vi.fn(),
    selectTradesRows: (state: any) => state.rows ?? [],
    selectTradesLastSeq: (state: any) => state.lastSeq ?? 0,
    selectResyncId: (state: any) => state.resyncId ?? 0,
    markGlobalResyncApplied: vi.fn(),
    shallow: () => false,
  };
});

vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: () => <div data-testid="trades-table" />,
}));

type MockStoresConfig = {
  setSnapshotResult?: { accepted: boolean; applied: boolean };
  applyDeltaResult?: {
    upserts: number;
    deletes: number;
    changed: boolean;
    newRows: number;
    staleRejected: number;
    accepted: boolean;
    applied: boolean;
  };
  resyncId?: number;
};

const defaultApplyDeltaResult = {
  upserts: 0,
  deletes: 0,
  changed: false,
  newRows: 0,
  staleRejected: 0,
  accepted: true,
  applied: false,
};

const setupMockStores = (config: MockStoresConfig = {}) => {
  const setSnapshot = vi.fn().mockReturnValue(
    config.setSnapshotResult ?? { accepted: true, applied: true },
  );
  const applyDelta = vi.fn().mockReturnValue(
    config.applyDeltaResult ?? defaultApplyDeltaResult,
  );

  const tradesState = {
    rows: [],
    lastSeq: 0,
    setSnapshot,
    applyDelta,
    appendHistorical: vi.fn(),
    clear: vi.fn(),
  };

  const resyncState = {
    resyncId: config.resyncId ?? 7,
    isResyncing: true,
    lastReason: 'test',
    lastBumpAt: Date.now(),
    appliedBy: {},
    bumpResync: vi.fn(),
    markResyncApplied: vi.fn(),
    resetResyncState: vi.fn(),
  };

  (useTradesStore as unknown as { mockImplementation: (fn?: (...args: any[]) => unknown) => void }).mockImplementation(
    (selector?: (state: typeof tradesState) => unknown) =>
      (typeof selector === 'function' ? selector(tradesState) : tradesState),
  );
  (useResyncStore as unknown as { mockImplementation: (fn?: (...args: any[]) => unknown) => void }).mockImplementation(
    (selector?: (state: typeof resyncState) => unknown) =>
      (typeof selector === 'function' ? selector(resyncState) : resyncState),
  );

  return { setSnapshot, applyDelta, resyncState };
};

describe('Trades resync apply contract', () => {
  beforeEach(() => {
    sessionStorage.clear();
    localStorage.clear();
    getTrades.mockReset();
    getTradesDelta.mockReset();
    vi.mocked(socketMock.on).mockClear();
    vi.mocked(socketMock.off).mockClear();
    vi.mocked(markGlobalResyncApplied).mockReset();
    (useTradesStore as unknown as { mockReset: () => void }).mockReset();
    (useResyncStore as unknown as { mockReset: () => void }).mockReset();
    getTrades.mockResolvedValue({
      rows: [],
      total: 0,
      page: 1,
      page_size: 100,
      last_seq: 0,
      has_more: false,
      next_cursor: null,
    });
    getTradesDelta.mockResolvedValue({ rows: [], last_seq: 0, reset_required: false });
  });

  afterEach(() => {
    cleanup();
  });

  it('does not mark resync applied when snapshot is rejected', async () => {
    const { setSnapshot } = setupMockStores({
      setSnapshotResult: { accepted: false, applied: false },
    });

    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalled());
    expect(setSnapshot).toHaveBeenCalled();
    expect(markGlobalResyncApplied).not.toHaveBeenCalled();
  });

  it('does not mark resync applied for filtered or no-op socket events', async () => {
    sessionStorage.setItem('trades_filters', JSON.stringify({ exchange: 'rooster' }));
    const { applyDelta } = setupMockStores({
      applyDeltaResult: {
        upserts: 0,
        deletes: 0,
        changed: false,
        newRows: 0,
        staleRejected: 0,
        accepted: true,
        applied: false,
      },
    });

    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalled());

    const tradeUpdateHandler = vi.mocked(socketMock.on).mock.calls.find(
      ([event]) => event === 'trade_update',
    )?.[1] as ((msg: any) => void) | undefined;
    expect(tradeUpdateHandler).toBeTypeOf('function');

    vi.mocked(markGlobalResyncApplied).mockClear();

    act(() => {
      tradeUpdateHandler?.({
        op: 'upsert',
        row_id: 'filtered',
        version: 1,
        seq: 1,
        exchange: 'bybit',
      });
    });

    expect(applyDelta).not.toHaveBeenCalled();
    expect(markGlobalResyncApplied).not.toHaveBeenCalled();

    act(() => {
      tradeUpdateHandler?.({
        op: 'upsert',
        row_id: 'noop',
        version: 1,
        seq: 2,
        exchange: 'rooster',
      });
    });
    await waitFor(() => {
      expect(applyDelta).toHaveBeenCalledTimes(1);
    });
    expect(markGlobalResyncApplied).not.toHaveBeenCalled();
  });

  it('marks resync applied only after accepted current-epoch apply', async () => {
    const { applyDelta, resyncState } = setupMockStores({
      applyDeltaResult: {
        upserts: 1,
        deletes: 0,
        changed: true,
        newRows: 1,
        staleRejected: 0,
        accepted: true,
        applied: true,
      },
      resyncId: 11,
    });

    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalled());

    const tradeUpdateHandler = vi.mocked(socketMock.on).mock.calls.find(
      ([event]) => event === 'trade_update',
    )?.[1] as ((msg: any) => void) | undefined;
    expect(tradeUpdateHandler).toBeTypeOf('function');

    vi.mocked(markGlobalResyncApplied).mockClear();
    act(() => {
      tradeUpdateHandler?.({
        op: 'upsert',
        row_id: 'accepted',
        version: 2,
        seq: 3,
        exchange: 'bybit',
      });
    });

    await waitFor(() => {
      expect(applyDelta).toHaveBeenCalled();
      expect(markGlobalResyncApplied).toHaveBeenCalledWith('trades', resyncState.resyncId);
    });
  });

  it('does not mark resync applied for no-op poll deltas', async () => {
    setupMockStores({
      setSnapshotResult: { accepted: true, applied: false },
      applyDeltaResult: {
        upserts: 0,
        deletes: 0,
        changed: false,
        newRows: 0,
        staleRejected: 0,
        accepted: true,
        applied: false,
      },
    });
    getTradesDelta.mockResolvedValueOnce({
      rows: [{ op: 'upsert', row_id: 'poll-noop', version: 1, seq: 4, exchange: 'bybit' }],
      last_seq: 4,
      reset_required: false,
    });

    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalled());
    vi.mocked(markGlobalResyncApplied).mockClear();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalled());
    expect(markGlobalResyncApplied).not.toHaveBeenCalled();
  });
});
