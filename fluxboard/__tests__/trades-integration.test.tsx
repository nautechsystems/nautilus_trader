import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor, act, cleanup } from '@testing-library/react';
import Trades from '../Trades';
import { useTradesStore } from '../stores';

const mockTable = vi.fn(({ trades }: any) => (
  <div data-testid="mock-table">{trades?.map((row: any) => row.row_id).join(',')}</div>
));

vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: (props: any) => mockTable(props),
}));

vi.mock('../utils/sound', () => ({
  playTradeClick: vi.fn(),
}));

const { socketHandlers, socketMock, getTrades, getTradesDelta } = vi.hoisted(() => {
  const socketHandlers: Record<string, (msg: any) => void> = {};
  const socketMock = {
    on: vi.fn((event: string, handler: (msg: any) => void) => {
      socketHandlers[event] = handler;
    }),
    off: vi.fn((event: string) => {
      delete socketHandlers[event];
    }),
    connected: true,
  };
  return {
    socketHandlers,
    socketMock,
    getTrades: vi.fn(),
    getTradesDelta: vi.fn(),
  };
});

vi.mock('../sockets', () => ({ socket: socketMock }));

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

const baseRows = [
  {
    row_id: 'old',
    seq: 1,
    version: 1,
    ts: 1,
    time: '2025-01-01T00:00:01Z',
    coin: 'PLUME/USDT',
    exchange: 'bybit',
    side: 'buy',
    price: 100,
  },
  {
    row_id: 'new',
    seq: 2,
    version: 1,
    ts: 2,
    time: '2025-01-01T00:00:02Z',
    coin: 'PLUME/USDT',
    exchange: 'bybit',
    side: 'sell',
    price: 101,
  },
];

const makeTradeRow = (overrides: Record<string, unknown> = {}) => ({
  row_id: 'trade-row',
  seq: 1,
  version: 1,
  ts: 1,
  time: '2025-01-01T00:00:01Z',
  coin: 'PLUME/USDT',
  exchange: 'bybit',
  side: 'buy',
  price: 100,
  ...overrides,
});

describe('Trades integration flows', () => {
  beforeEach(() => {
    getTrades.mockReset();
    getTradesDelta.mockReset();
    mockTable.mockClear();
    Object.keys(socketHandlers).forEach((key) => delete socketHandlers[key]);
    useTradesStore.getState().clear();
    getTrades.mockResolvedValue({
      rows: baseRows,
      total: baseRows.length,
      page: 1,
      page_size: 100,
      last_seq: 2,
      stream_id: 'trades-main',
      snapshot_revision: 17,
    });
    getTradesDelta.mockResolvedValue({
      rows: [],
      last_seq: 2,
      reset_required: false,
      stream_id: 'trades-main',
      snapshot_revision: 17,
    });
  });

  afterEach(() => {
    cleanup();
  });

  it('loads newest-first snapshot using ts_desc by default', async () => {
    render(<Trades />);

    await waitFor(() => expect(getTrades).toHaveBeenCalled());
    const firstCall = getTrades.mock.calls[0];
    expect(firstCall[0]).toBe(1);
    expect(firstCall[1]).toBe(100);
    expect(firstCall[2]).toMatchObject({ sort: 'ts_desc' });
  });

  it('applies live trade_update events to the top of the table', async () => {
    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'live',
        seq: 99,
        version: 1,
        ts: 99,
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
      });
    });

    await waitFor(() => {
      const lastCall = mockTable.mock.calls[mockTable.mock.calls.length - 1];
      expect(lastCall).toBeTruthy();
      const props = lastCall[0];
      expect(props.trades?.[0].row_id).toBe('live');
    });
  });

  it('does not replay over HTTP while the standard cursor is healthy, then replays with that cursor after recovery starts', async () => {
    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalledTimes(1));

    getTradesDelta.mockClear();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    expect(getTradesDelta).not.toHaveBeenCalled();

    act(() => {
      socketHandlers.disconnect?.('transport close');
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    expect(getTradesDelta).toHaveBeenCalledWith(
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );
  });

  it('enters recovery and replays from the last acknowledged seq when a socket seq gap is detected', async () => {
    getTradesDelta.mockResolvedValueOnce({
      rows: [
        makeTradeRow({
          row_id: 'gap-3',
          seq: 3,
          ts: 3,
          time: '2025-01-01T00:00:03Z',
          side: 'sell',
        }),
        makeTradeRow({
          row_id: 'gap-4',
          seq: 4,
          ts: 4,
          time: '2025-01-01T00:00:04Z',
        }),
        makeTradeRow({
          row_id: 'gap-5',
          seq: 5,
          ts: 5,
          time: '2025-01-01T00:00:05Z',
        }),
      ],
      last_seq: 5,
      reset_required: false,
      stream_id: 'trades-main',
      snapshot_revision: 17,
    });

    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'gap-5',
        seq: 5,
        version: 1,
        ts: 5,
        time: '2025-01-01T00:00:05Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
    });

    const propsAfterGap = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
    expect(propsAfterGap.trades.map((row: any) => row.row_id)).not.toContain('gap-5');

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    expect(getTradesDelta).toHaveBeenCalledWith(
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );

  });

  it('keeps the rendered trades array stable for in-place live updates', async () => {
    render(<Trades />);
    await waitFor(() => {
      const latestProps = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
      expect(latestProps?.trades).toHaveLength(2);
    });

    const initialProps = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
    expect(initialProps?.trades).toBeTruthy();
    const initialTrades = initialProps.trades;
    const initialOldRow = initialTrades.find((row: any) => row.row_id === 'old');

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'old',
        seq: 1,
        version: 2,
        ts: 1,
        time: '2025-01-01T00:00:01Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        price: 999,
      });
    });

    await waitFor(() => {
      const latestProps = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
      const updatedOldRow = latestProps.trades.find((row: any) => row.row_id === 'old');

      expect(latestProps.trades).toBe(initialTrades);
      expect(updatedOldRow).toBe(initialOldRow);
      expect(updatedOldRow.price).toBe(999);
    });
  });

  it('keeps zero-seq snapshots on the standard sinceSeq cursor and only replays after recovery begins', async () => {
    getTrades.mockResolvedValueOnce({
      rows: [],
      total: 0,
      page: 1,
      page_size: 100,
      last_seq: 0,
      stream_id: 'tokenmm-trades',
      snapshot_revision: 'snap-empty',
    });
    getTradesDelta.mockResolvedValueOnce({
      rows: [],
      last_seq: 0,
      reset_required: false,
      stream_id: 'tokenmm-trades',
      snapshot_revision: 'snap-empty',
    });
    (window.location as any).pathname = '/tokenmm/trades';

    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalledTimes(1));

    getTradesDelta.mockClear();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    expect(getTradesDelta).not.toHaveBeenCalled();

    act(() => {
      socketHandlers.disconnect?.('transport close');
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    const [cursor, limit] = getTradesDelta.mock.calls[0];
    expect(cursor).toMatchObject({
      sinceSeq: 0,
      streamId: 'tokenmm-trades',
      snapshotRevision: 'snap-empty',
    });
    expect(cursor.afterMs).toBeUndefined();
    expect(limit).toBe(500);
  });

  it('normalizes nested FluxAPI trade_update payloads (trade object) into full blotter rows', async () => {
    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'live-nested',
        seq: 99, // socket seq (not trade seq)
        version: 1,
        strategy_id: 'bybit_binance_plumeusdt_makerv3',
        server_ts_ms: 1772700209799,
        trade: {
          row_id: 'live-nested',
          version: 1,
          seq: 1772700209804, // trade stream seq
          ts_ms: 1772700209799,
          instrument_id: 'PLUMEUSDT-LINEAR.BYBIT',
          side: '1',
          price: '0.009974',
          qty: '1000',
          client_order_id: 'O-20260305-084321-001-000-932',
          trade_id: 'live-nested',
          strategy_id: 'bybit_binance_plumeusdt_makerv3',
        },
      });
    });

    await waitFor(() => {
      const top = useTradesStore.getState().rows[0];
      expect(top.row_id).toBe('live-nested');
      expect(top.coin).toBe('PLUME');
      expect(top.exchange).toBe('bybit');
      expect(top.side).toBe('buy');
      expect(top.order_id).toBe('O-20260305-084321-001-000-932');
      expect(top.time).toMatch(/T/);
      expect(top.mv).toBeCloseTo(9.974, 6);
    });
  });
});
