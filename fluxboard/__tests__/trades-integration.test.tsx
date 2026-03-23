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

vi.mock('../api', () => ({
  api: {
    getTrades,
    getTradesDelta,
  },
}));

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
  },
];

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
    });
    getTradesDelta.mockResolvedValue({ rows: [], last_seq: 2, reset_required: false });
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

  it('uses qty_base as the top-row quantity and preserves qty_venue for nested tokenmm trade updates', async () => {
    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'live-okx',
        seq: 100,
        version: 1,
        strategy_id: 'plumeusdt_okx_perp_makerv3',
        server_ts_ms: 1772700209799,
        trade: {
          row_id: 'live-okx',
          version: 1,
          seq: 1772700209804,
          ts_ms: 1772700209799,
          instrument_id: 'PLUME-USDT-SWAP.OKX',
          side: '1',
          price: '0.012736',
          qty: '100',
          qty_base: '1000',
          qty_venue: '100',
          qty_conversion_status: 'exact_multiplier',
          client_order_id: 'O-OKX-1',
          trade_id: 'live-okx',
          strategy_id: 'plumeusdt_okx_perp_makerv3',
        },
      });
    });

    await waitFor(() => {
      const top = useTradesStore.getState().rows[0] as any;
      expect(top.row_id).toBe('live-okx');
      expect(top.qty).toBe(1000);
      expect(top.qty_venue).toBe('100');
    });
  });
});
