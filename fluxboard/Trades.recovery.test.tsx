import { act, cleanup, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import Trades from './Trades';
import { api } from './api';
import { socket } from './sockets';
import { useTradesStore } from './stores';
import type { TradeRow } from './types';

vi.mock('./api', async (importOriginal) => {
  const mod = await importOriginal<any>();
  return {
    ...mod,
    api: {
      ...mod.api,
      getTrades: vi.fn(),
      getTradesDelta: vi.fn(),
    },
    deriveCanonicalNaming: vi.fn(() => ({})),
  };
});

vi.mock('./sockets', () => ({
  socket: {
    on: vi.fn(),
    off: vi.fn(),
    connected: true,
  },
}));

vi.mock('./stores', async (importOriginal) => {
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

let lastTradesTableProps: any = null;
vi.mock('./components/trades/TradesTable', () => ({
  TradesTable: (props: any) => {
    lastTradesTableProps = props;
    return <div data-testid="trades-table-mock">{props.trades?.length ?? 0} rows</div>;
  },
}));

const mockGetTrades = vi.mocked(api.getTrades);
const mockGetTradesDelta = vi.mocked(api.getTradesDelta);

type StoreMock = ReturnType<typeof useTradesStore>;

const makeTradeRow = (overrides: Partial<TradeRow> = {}): TradeRow => ({
  row_id: 'row-default',
  version: 1,
  seq: 1,
  ts: 1,
  time: '2025-01-01T00:00:00.000Z',
  coin: 'BTC',
  exchange: 'bybit',
  side: 'buy',
  price: 100,
  qty: 1,
  mv: 100,
  fee: 0.1,
  exch_id: 'exec-1',
  trade_id: 'trade-1',
  signal_id: 'strat-1',
  order_id: 'order-1',
  decision: '',
  explorer_url: '',
  notes: '',
  ...overrides,
});

const makeSnapshotResponse = (
  rows: TradeRow[],
  overrides: Record<string, unknown> = {},
) => ({
  rows,
  total: rows.length,
  page: 1,
  page_size: 100,
  last_seq: rows.reduce((max, row) => Math.max(max, row.seq), 0),
  has_more: false,
  next_cursor: null,
  stream_id: 'trades-main',
  snapshot_revision: 17,
  ...overrides,
});

const makeDeltaResponse = (
  rows: TradeRow[],
  overrides: Record<string, unknown> = {},
) => ({
  rows,
  last_seq: rows.reduce((max, row) => Math.max(max, row.seq), 0),
  reset_required: false,
  stream_id: 'trades-main',
  snapshot_revision: 17,
  ...overrides,
});

function setupStore(overrides?: Partial<StoreMock>) {
  const setSnapshot = vi.fn();
  const applyDelta = vi.fn().mockReturnValue({ upserts: 0, deletes: 0, changed: false });
  const appendHistorical = vi.fn();

  const store: StoreMock = {
    rows: [],
    order: [],
    setSnapshot,
    appendHistorical,
    applyDelta,
    lastSeq: 0,
    ...overrides,
  } as StoreMock;

  (useTradesStore as unknown as { mockImplementation: (fn?: (state: StoreMock) => unknown) => void }).mockImplementation((selector?: (state: StoreMock) => unknown) => (
    typeof selector === 'function' ? selector(store) : store
  ));

  return { setSnapshot, applyDelta };
}

describe('Trades recovery regressions', () => {
  beforeEach(() => {
    (window.location as any).pathname = '/trades';
    sessionStorage.clear();
    mockGetTrades.mockReset();
    mockGetTradesDelta.mockReset();
    (useTradesStore as unknown as { mockReset: () => void }).mockReset();
    vi.mocked(socket.on).mockClear();
    vi.mocked(socket.off).mockClear();
    lastTradesTableProps = null;
  });

  afterEach(() => {
    cleanup();
  });

  it('keeps existing rows when a snapshot refresh fails transiently', async () => {
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    const existing = makeTradeRow({ row_id: 'existing-row', seq: 5, ts: 1_700_000_000_000 });
    const { setSnapshot } = setupStore({ rows: [existing], lastSeq: 5 });
    mockGetTrades.mockRejectedValueOnce(new Error('temporary failure'));
    mockGetTradesDelta.mockResolvedValue({ rows: [], last_seq: 5, reset_required: false });

    try {
      render(<Trades />);

      await waitFor(() => expect(mockGetTrades).toHaveBeenCalledTimes(1));
      expect(setSnapshot).not.toHaveBeenCalled();
      expect(lastTradesTableProps?.trades.map((row: TradeRow) => row.row_id)).toEqual(['existing-row']);
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it('derives a standard sinceSeq cursor from tokenmm snapshot rows when backend last_seq is zero', async () => {
    (window.location as any).pathname = '/tokenmm/trades';
    setupStore();
    mockGetTrades.mockResolvedValue(makeSnapshotResponse([
      makeTradeRow({ row_id: 'tokenmm-a', seq: 101, ts: 1_700_000_001_000, ts_ms: 1_700_000_001_000 } as Partial<TradeRow>),
      makeTradeRow({ row_id: 'tokenmm-b', seq: 202, ts: 1_700_000_002_000, ts_ms: 1_700_000_002_000 } as Partial<TradeRow>),
    ], {
      last_seq: 0,
      stream_id: 'tokenmm-trades',
      snapshot_revision: 'snap-2',
    }));
    mockGetTradesDelta.mockResolvedValue(makeDeltaResponse([], {
      last_seq: 0,
      stream_id: 'tokenmm-trades',
      snapshot_revision: 'snap-2',
    }));

    render(<Trades />);
    await waitFor(() => expect(mockGetTrades).toHaveBeenCalledTimes(1));

    mockGetTradesDelta.mockClear();
    const disconnectHandler = vi.mocked(socket.on).mock.calls.find(([event]) => event === 'disconnect')?.[1] as ((reason: string) => void) | undefined;
    expect(disconnectHandler).toBeInstanceOf(Function);

    act(() => {
      disconnectHandler?.('transport close');
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1200));
    });

    await waitFor(() => expect(mockGetTradesDelta).toHaveBeenCalledTimes(1));
    expect(mockGetTradesDelta).toHaveBeenCalledWith(
      expect.objectContaining({
        sinceSeq: 202,
        streamId: 'tokenmm-trades',
        snapshotRevision: 'snap-2',
      }),
      500,
    );
    const [cursor] = mockGetTradesDelta.mock.calls[0];
    expect(cursor.afterMs).toBeUndefined();
  });

  it('keeps zero-trade tokenmm snapshots healthy until recovery begins, then replays with sinceSeq 0', async () => {
    (window.location as any).pathname = '/tokenmm/trades';
    setupStore();
    mockGetTrades.mockResolvedValue(makeSnapshotResponse([], {
      last_seq: 0,
      stream_id: 'tokenmm-trades',
      snapshot_revision: 'snap-empty',
    }));
    mockGetTradesDelta.mockResolvedValue(makeDeltaResponse([], {
      last_seq: 0,
      stream_id: 'tokenmm-trades',
      snapshot_revision: 'snap-empty',
    }));

    render(<Trades />);
    await waitFor(() => expect(mockGetTrades).toHaveBeenCalledTimes(1));

    mockGetTradesDelta.mockClear();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1200));
    });

    expect(mockGetTradesDelta).not.toHaveBeenCalled();

    const disconnectHandler = vi.mocked(socket.on).mock.calls.find(([event]) => event === 'disconnect')?.[1] as ((reason: string) => void) | undefined;
    expect(disconnectHandler).toBeInstanceOf(Function);

    act(() => {
      disconnectHandler?.('transport close');
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1200));
    });

    await waitFor(() => expect(mockGetTradesDelta).toHaveBeenCalledTimes(1));
    const [cursor, limit] = mockGetTradesDelta.mock.calls[0];
    expect(cursor).toMatchObject({
      sinceSeq: 0,
      streamId: 'tokenmm-trades',
      snapshotRevision: 'snap-empty',
    });
    expect(cursor.afterMs).toBeUndefined();
    expect(limit).toBe(500);
  });

  it('refreshes the snapshot when a filtered visible row is deleted by a non-matching socket event', async () => {
    sessionStorage.setItem('trades_filters', JSON.stringify({ exchange: 'bybit' }));
    const existing = makeTradeRow({
      row_id: 'existing-row',
      seq: 5,
      exchange: 'bybit',
      coin: 'PLUME',
    });
    setupStore({ rows: [existing], lastSeq: 5 });
    mockGetTrades.mockResolvedValueOnce(makeSnapshotResponse([existing], { last_seq: 5 }));
    mockGetTradesDelta.mockResolvedValue(makeDeltaResponse([], { last_seq: 5 }));

    render(<Trades />);
    await waitFor(() => expect(mockGetTrades).toHaveBeenCalledTimes(1));

    const tradeUpdateHandler = vi.mocked(socket.on).mock.calls.find(([event]) => event === 'trade_update')?.[1] as ((msg: any) => void) | undefined;
    expect(tradeUpdateHandler).toBeInstanceOf(Function);

    mockGetTrades.mockResolvedValueOnce(makeSnapshotResponse([], { last_seq: 6 }));
    mockGetTrades.mockClear();

    act(() => {
      tradeUpdateHandler?.({
        op: 'delete',
        row_id: 'existing-row',
        seq: 6,
        version: 2,
      });
    });

    await waitFor(() => expect(mockGetTrades).toHaveBeenCalledTimes(1));
  });
});
