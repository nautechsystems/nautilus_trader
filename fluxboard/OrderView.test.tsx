import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { bumpGlobalResync, useResyncStore } from './stores';
import { useOrderViewStore } from './stores/orderViewStore';

const getSignalStrategiesMock = vi.hoisted(() => vi.fn());
const getOrderViewSnapshotMock = vi.hoisted(() => vi.fn());
const socketEmitMock = vi.hoisted(() => vi.fn());
const socketOnMock = vi.hoisted(() => vi.fn());
const socketOffMock = vi.hoisted(() => vi.fn());
const socketHandlers = vi.hoisted(() => new Map<string, Set<(...args: any[]) => void>>());
const chartSetDataMock = vi.hoisted(() => vi.fn());
const chartUpdateMock = vi.hoisted(() => vi.fn());
const chartCreatePriceLineMock = vi.hoisted(() => vi.fn(() => ({ applyOptions: vi.fn() })));
const chartRemovePriceLineMock = vi.hoisted(() => vi.fn());
const chartSetMarkersMock = vi.hoisted(() => vi.fn());
const chartFitContentMock = vi.hoisted(() => vi.fn());

vi.mock('./api', () => ({
  api: {
    getSignalStrategies: getSignalStrategiesMock,
    getOrderViewSnapshot: getOrderViewSnapshotMock,
  },
  l2: {
    bids: [
      { px: '30000', qty: '2.0', size: 10 },
      { px: '29999', qty: '1.0', size: 5 },
    ],
    asks: [
      { px: '30001', qty: '2.5', size: 8 },
      { px: '30002', qty: '4.0', size: 4 },
    ],
    top_n: 2,
    spread_abs: 1,
    spread_bps: 3.33,
  },
}));

vi.mock('./sockets', () => ({
  socket: {
    connected: true,
    emit: (...args: any[]) => socketEmitMock(...args),
    on: (...args: any[]) => socketOnMock(...args),
    off: (...args: any[]) => socketOffMock(...args),
  },
}));

vi.mock('lightweight-charts', () => {
  const markersApi = {
    setMarkers: (...args: any[]) => chartSetMarkersMock(...args),
  };
  const seriesApi = {
    setData: (...args: any[]) => chartSetDataMock(...args),
    update: (...args: any[]) => chartUpdateMock(...args),
    createPriceLine: (...args: any[]) => chartCreatePriceLineMock(...args),
    removePriceLine: (...args: any[]) => chartRemovePriceLineMock(...args),
  };
  const chartApi = {
    addSeries: vi.fn(() => seriesApi),
    applyOptions: vi.fn(),
    remove: vi.fn(),
    timeScale: vi.fn(() => ({ fitContent: (...args: any[]) => chartFitContentMock(...args) })),
  };
  return {
    ColorType: { Solid: 'solid' },
    LineStyle: { Solid: 0, Dashed: 2 },
    CandlestickSeries: {},
    LineSeries: {},
    createChart: vi.fn(() => chartApi),
    createSeriesMarkers: vi.fn(() => markersApi),
  };
});

import OrderView from './OrderView';

const makeSnapshot = () => ({
  room_id: 'order_view:strat-1:maker:book:1:depth:20:v02:1',
  server_ts_ms: 1_700_000_000_000,
  selection: { strategy_id: 'strat-1', leg: 'maker' as const },
  context: {
    maker: { exchange: 'bybit_linear', symbol: 'BTC_USDT' },
    hedge: { exchange: 'binance_spot', symbol: 'BTC_USDT' },
  },
  state_rev: 'rev-1',
  maker_state_ts_ms: 1_700_000_000_000,
  bbo: {
    maker: { bid: 30000, ask: 30010, mid: 30005, ts_ms: 1_700_000_000_000 },
  },
  open_orders: {
    rows: [
      {
        order_row_id: 'maker:bid:1:cl-1',
        leg: 'maker',
        side: 'bid',
        level: 1,
        px: '30000',
        rem_qty: '1.5',
        client_order_id: 'cl-1',
      },
      {
        order_row_id: 'maker:ask:1:cl-2',
        leg: 'maker',
        side: 'ask',
        level: 1,
        px: '30010',
        rem_qty: '0.7',
        client_order_id: 'cl-2',
      },
    ],
  },
  events: {
    rows: [
      {
        event_key: 'evt-quote-1',
        ts_ms: 1_700_000_000_000,
        type: 'quote.placed',
        side: 'bid',
        level: 1,
        order_id: 'oid-1',
        ack_ms: 8,
      },
      {
        event_key: 'evt-fill-1',
        ts_ms: 1_700_000_000_001,
        type: 'fill',
        side: 'buy',
        px: '30005',
        qty: '0.1',
        order_id: 'oid-2',
        fill_ms: 21,
      },
    ],
  },
  market_trades: {
    rows: [
      {
        trade_id: 'mkt-1',
        ts_ms: 1_700_000_000_222,
        side: 'buy',
        price: '30006',
        qty: '0.25',
      },
      {
        trade_id: 'mkt-2',
        ts_ms: 1_700_000_000_333,
        side: 'sell',
        price: '30007',
        qty: '0.15',
      },
    ],
  },
  candles: {
    source: 'trades',
    rows: [
      { ts_ms: 1_700_000_000_000, open: 30000, high: 30010, low: 29990, close: 30005, volume: 1.2 },
      { ts_ms: 1_700_000_001_000, open: 30005, high: 30012, low: 30001, close: 30008, volume: 0.8 },
    ],
  },
  status: {
    md_ok: true,
    maker_state_ok: true,
    events_ok: true,
    last_md_ts_ms: 1_700_000_000_000,
    last_state_ts_ms: 1_700_000_000_000,
    notes: [],
  },
});

const makeDelta = (seq: number, overrides: Partial<any> = {}) => ({
  room_id: 'order_view:strat-1:maker:book:1:depth:20:v02:1',
  seq,
  state_rev: 'rev-1',
  server_ts_ms: 1_700_000_000_000 + seq,
  selection: { strategy_id: 'strat-1', leg: 'maker' as const },
  context: {
    maker: { exchange: 'bybit_linear', symbol: 'BTC_USDT' },
    hedge: { exchange: 'binance_spot', symbol: 'BTC_USDT' },
  },
  bbo: {
    maker: { bid: 30000 + seq, ask: 30010 + seq, mid: 30005 + seq, ts_ms: 1_700_000_000_000 + seq },
  },
  events: { rows: [] },
  status: {
    md_ok: true,
    maker_state_ok: true,
    events_ok: true,
    last_md_ts_ms: 1_700_000_000_000 + seq,
    last_state_ts_ms: 1_700_000_000_000,
    notes: [],
  },
  ...overrides,
});

const loadReplayFixture = () => {
  const fixtureRoot = path.resolve(process.cwd(), '..', 'tests', 'fixtures', 'order_view_v02');
  const snapshotPath = path.join(fixtureRoot, 'snapshot.json');
  const deltasPath = path.join(fixtureRoot, 'deltas.jsonl');

  const snapshot = JSON.parse(readFileSync(snapshotPath, 'utf-8'));
  const deltas = readFileSync(deltasPath, 'utf-8')
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => JSON.parse(line));

  return { snapshot, deltas };
};

describe('OrderView', () => {
  beforeEach(() => {
    socketHandlers.clear();
    socketEmitMock.mockReset();
    socketOnMock.mockReset();
    socketOffMock.mockReset();
    chartSetDataMock.mockReset();
    chartUpdateMock.mockReset();
    chartCreatePriceLineMock.mockReset();
    chartRemovePriceLineMock.mockReset();
    chartSetMarkersMock.mockReset();
    chartFitContentMock.mockReset();

    socketOnMock.mockImplementation((event: string, handler: (...args: any[]) => void) => {
      const handlers = socketHandlers.get(event) ?? new Set();
      handlers.add(handler);
      socketHandlers.set(event, handlers);
    });

    socketOffMock.mockImplementation((event: string, handler: (...args: any[]) => void) => {
      const handlers = socketHandlers.get(event);
      handlers?.delete(handler);
    });

    socketEmitMock.mockImplementation((event: string, _payload?: unknown, ack?: (...args: any[]) => void) => {
      if (event === 'order_view_subscribe' && typeof ack === 'function') {
        ack({ ok: true, error: null });
      }
    });

    getSignalStrategiesMock.mockReset();
    getOrderViewSnapshotMock.mockReset();
    getSignalStrategiesMock.mockResolvedValue({
      strategies: [{ id: 'strat-1' }],
      server_time: '2026-02-26 00:00:00',
      server_ts_ms: 1_700_000_000_000,
    });
    getOrderViewSnapshotMock.mockResolvedValue(makeSnapshot());

    useOrderViewStore.getState().clear();
    useOrderViewStore.getState().setSelection({ strategyId: '' });
    useResyncStore.getState().resetResyncState();
  });

  it('fetches snapshot and subscribes, then unsubscribes on unmount', async () => {
    const view = render(<OrderView />);

    await waitFor(() => {
      expect(getOrderViewSnapshotMock).toHaveBeenCalledWith(
        expect.objectContaining({
          strategyId: 'strat-1',
          leg: 'maker',
          candleIntervalMs: 1_000,
          candleRange: '15m',
          orderViewV02: true,
        })
      );
    });

    expect(socketEmitMock).toHaveBeenCalledWith(
      'order_view_subscribe',
      expect.objectContaining({
        strategy_id: 'strat-1',
        leg: 'maker',
        include_book: true,
        book_depth: 20,
        events_limit: 200,
        candle_interval_ms: 1_000,
        order_view_v02: true,
      }),
      expect.any(Function)
    );

    view.unmount();

    expect(socketEmitMock).toHaveBeenCalledWith(
      'order_view_unsubscribe',
      expect.objectContaining({ room_id: 'order_view:strat-1:maker:book:1:depth:20:v02:1' })
    );
  });

  it('does not poison unsubscribe room from stale same-selection deltas', async () => {
    const rafSpy = vi
      .spyOn(window, 'requestAnimationFrame')
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(performance.now());
        return 1 as unknown as number;
      });
    try {
      const view = render(<OrderView />);
      await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

      const deltaHandler = Array.from(socketHandlers.get('order_view_delta') || [])[0];
      expect(typeof deltaHandler).toBe('function');

      await act(async () => {
        deltaHandler(
          makeDelta(1, {
            room_id: 'order_view:strat-1:maker:book:1:depth:20:v02:1:stale',
          })
        );
        await Promise.resolve();
      });

      view.unmount();

      expect(socketEmitMock).toHaveBeenCalledWith(
        'order_view_unsubscribe',
        expect.objectContaining({ room_id: 'order_view:strat-1:maker:book:1:depth:20:v02:1' })
      );
    } finally {
      rafSpy.mockRestore();
    }
  });

  it('applies incoming deltas and allows manual resync', async () => {
    render(<OrderView />);

    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    const deltaHandler = Array.from(socketHandlers.get('order_view_delta') || [])[0];
    expect(typeof deltaHandler).toBe('function');

    act(() => {
      deltaHandler(makeDelta(1));
    });

    await waitFor(() => {
      expect(useOrderViewStore.getState().lastSeq).toBe(1);
    });

    fireEvent.click(screen.getByRole('button', { name: 'Resync' }));
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(2));
  });

  it('refetches snapshot when the range selector changes', async () => {
    render(<OrderView />);

    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByLabelText('Range'), { target: { value: '5m' } });

    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(2));
    expect(getOrderViewSnapshotMock).toHaveBeenLastCalledWith(
      expect.objectContaining({
        candleRange: '5m',
      })
    );
  });

  it('refetches snapshot on resume if deltas arrived while paused', async () => {
    render(<OrderView />);

    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    const deltaHandler = Array.from(socketHandlers.get('order_view_delta') || [])[0];
    expect(typeof deltaHandler).toBe('function');

    fireEvent.click(screen.getByRole('button', { name: 'Pause' }));
    act(() => {
      deltaHandler(makeDelta(1));
    });

    await waitFor(() => {
      expect(useOrderViewStore.getState().pendingDeltaCount).toBe(1);
    });

    fireEvent.click(screen.getByRole('button', { name: 'Resume' }));
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(2));
  });

  it('re-subscribes after manual resync to recover pruned rooms', async () => {
    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    const subscribeCountBefore = socketEmitMock.mock.calls.filter(
      (call) => call[0] === 'order_view_subscribe'
    ).length;

    fireEvent.click(screen.getByRole('button', { name: 'Resync' }));
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(2));
    await waitFor(() => {
      const subscribeCountAfter = socketEmitMock.mock.calls.filter(
        (call) => call[0] === 'order_view_subscribe'
      ).length;
      expect(subscribeCountAfter).toBeGreaterThan(subscribeCountBefore);
    });
  });

  it('renders bybit-lite right and bottom tab layout with order-id search', async () => {
    render(<OrderView />);

    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    expect(screen.getByRole('button', { name: 'Order Book' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Recent Trades' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open Orders' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Our Fills' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Our Order Events' })).toBeInTheDocument();
    expect(screen.getByLabelText('Show BBO Lines')).toBeInTheDocument();

    expect(screen.getByTestId('order-view-open-orders-table')).toBeInTheDocument();
    expect(screen.getByTestId('order-view-l1-widget')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Recent Trades' }));
    expect(screen.getByTestId('order-view-market-trades-table')).toBeInTheDocument();
    expect(screen.getByText('mkt-1')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Our Order Events' }));
    expect(screen.getByTestId('order-view-events-table')).toBeInTheDocument();
    const eventsTable = screen.getByTestId('order-view-events-table');
    expect(within(eventsTable).getByText('Ack ms')).toBeInTheDocument();
    expect(within(eventsTable).getByText('Fill ms')).toBeInTheDocument();
    expect(within(eventsTable).getByText('8')).toBeInTheDocument();
    expect(within(eventsTable).getByText('21')).toBeInTheDocument();
    expect(screen.getByText('evt-quote-1')).toBeInTheDocument();
    expect(screen.getByText('evt-fill-1')).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText('Order ID search'), {
      target: { value: 'oid-2' },
    });
    expect(screen.getByText('evt-fill-1')).toBeInTheDocument();
    expect(screen.queryByText('evt-quote-1')).not.toBeInTheDocument();

    const autoScroll = screen.getByLabelText('Auto-scroll');
    expect(autoScroll).toBeChecked();
    fireEvent.click(autoScroll);
    expect(autoScroll).not.toBeChecked();
  });

  it('links event-row focus across chart/ladder/tape and supports clear focus', async () => {
    getOrderViewSnapshotMock.mockResolvedValueOnce({
      ...makeSnapshot(),
      open_orders: {
        rows: [
          {
            order_row_id: 'maker:bid:1:cl-1',
            leg: 'maker',
            side: 'bid',
            level: 1,
            px: '30000',
            rem_qty: '1.5',
            client_order_id: 'cl-1',
            order_id: 'oid-focus-bid',
          },
          {
            order_row_id: 'maker:ask:1:cl-2',
            leg: 'maker',
            side: 'ask',
            level: 1,
            px: '30001',
            rem_qty: '0.7',
            client_order_id: 'cl-2',
            order_id: 'oid-focus-ask',
          },
        ],
      },
      l2: {
        bids: [
          { px: '30000', qty: '2.0', size: 10 },
          { px: '29999', qty: '1.0', size: 5 },
        ],
        asks: [
          { px: '30001', qty: '2.5', size: 8 },
          { px: '30002', qty: '4.0', size: 4 },
        ],
        top_n: 2,
        spread_abs: 1,
        spread_bps: 3.33,
      },
      events: {
        rows: [
          {
            event_key: 'evt-focus-bid',
            ts_ms: 1_700_000_000_000,
            type: 'quote.placed',
            side: 'bid',
            px: '30000',
            order_id: 'oid-focus-bid',
          },
          {
            event_key: 'evt-focus-ask',
            ts_ms: 1_700_000_000_001,
            type: 'quote.placed',
            side: 'ask',
            px: '30001',
            order_id: 'oid-focus-ask',
          },
        ],
      },
    });

    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole('button', { name: 'Our Order Events' }));
    fireEvent.click(screen.getByTestId('order-view-events-row-evt-focus-bid'));

    expect(screen.getByTestId('order-view-focus-label')).toHaveTextContent(
      'evt:evt-focus-bid ord:oid-focus-bid side:bid px:30000.000000'
    );
    expect(screen.getByTestId('order-view-events-row-evt-focus-bid')).toHaveAttribute(
      'data-focus',
      'focused'
    );
    expect(screen.getByTestId('order-view-events-row-evt-focus-ask')).toHaveAttribute(
      'data-focus',
      'dimmed'
    );
    expect(screen.getByTestId('order-view-ladder-row-bid-0')).toHaveAttribute(
      'data-focus',
      'focused'
    );
    expect(screen.getByRole('button', { name: 'Clear Focus' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Clear Focus' }));
    expect(screen.getByTestId('order-view-focus-label')).toHaveTextContent('--');
    expect(screen.getByTestId('order-view-ladder-row-bid-0')).toHaveAttribute(
      'data-focus',
      'neutral'
    );
  });

  it('links ladder-row focus back into event highlights', async () => {
    getOrderViewSnapshotMock.mockResolvedValueOnce({
      ...makeSnapshot(),
      open_orders: {
        rows: [
          {
            order_row_id: 'maker:bid:1:cl-1',
            leg: 'maker',
            side: 'bid',
            level: 1,
            px: '30000',
            rem_qty: '1.5',
            client_order_id: 'cl-1',
            order_id: 'oid-focus-bid',
          },
          {
            order_row_id: 'maker:ask:1:cl-2',
            leg: 'maker',
            side: 'ask',
            level: 1,
            px: '30001',
            rem_qty: '0.7',
            client_order_id: 'cl-2',
            order_id: 'oid-focus-ask',
          },
        ],
      },
      l2: {
        bids: [
          { px: '30000', qty: '2.0', size: 10 },
          { px: '29999', qty: '1.0', size: 5 },
        ],
        asks: [
          { px: '30001', qty: '2.5', size: 8 },
          { px: '30002', qty: '4.0', size: 4 },
        ],
        top_n: 2,
        spread_abs: 1,
        spread_bps: 3.33,
      },
      events: {
        rows: [
          {
            event_key: 'evt-focus-bid',
            ts_ms: 1_700_000_000_000,
            type: 'quote.placed',
            side: 'bid',
            px: '30000',
            order_id: 'oid-focus-bid',
          },
          {
            event_key: 'evt-focus-ask',
            ts_ms: 1_700_000_000_001,
            type: 'quote.placed',
            side: 'ask',
            px: '30001',
            order_id: 'oid-focus-ask',
          },
        ],
      },
    });

    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByTestId('order-view-ladder-row-bid-0'));
    expect(screen.getByTestId('order-view-focus-label')).toHaveTextContent(
      'ord:oid-focus-bid side:bid px:30000.000000'
    );

    fireEvent.click(screen.getByRole('button', { name: 'Our Order Events' }));
    expect(screen.getByTestId('order-view-events-row-evt-focus-bid')).toHaveAttribute(
      'data-focus',
      'focused'
    );
    expect(screen.getByTestId('order-view-events-row-evt-focus-ask')).toHaveAttribute(
      'data-focus',
      'dimmed'
    );

    fireEvent.click(screen.getByTestId('order-view-ladder-row-bid-0'));
    expect(screen.getByTestId('order-view-focus-label')).toHaveTextContent('--');
  });

  it('matches event focus by side when bid/ask share the same price', async () => {
    getOrderViewSnapshotMock.mockResolvedValueOnce({
      ...makeSnapshot(),
      open_orders: {
        rows: [],
      },
      l2: {
        bids: [{ px: '30000', qty: '2.0', size: 10 }],
        asks: [{ px: '30001', qty: '2.5', size: 8 }],
        top_n: 1,
        spread_abs: 1,
        spread_bps: 3.33,
      },
      events: {
        rows: [
          {
            event_key: 'evt-bid-same-price',
            ts_ms: 1_700_000_000_000,
            type: 'quote.placed',
            side: 'bid',
            px: '30000',
          },
          {
            event_key: 'evt-ask-same-price',
            ts_ms: 1_700_000_000_001,
            type: 'quote.placed',
            side: 'ask',
            px: '30000',
          },
        ],
      },
    });

    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByTestId('order-view-ladder-row-bid-0'));
    fireEvent.click(screen.getByRole('button', { name: 'Our Order Events' }));

    expect(screen.getByTestId('order-view-events-row-evt-bid-same-price')).toHaveAttribute(
      'data-focus',
      'focused'
    );
    expect(screen.getByTestId('order-view-events-row-evt-ask-same-price')).toHaveAttribute(
      'data-focus',
      'dimmed'
    );
  });

  it('shows stale badge when market data timestamp is too old', async () => {
    getOrderViewSnapshotMock.mockResolvedValueOnce({
      ...makeSnapshot(),
      status: {
        ...makeSnapshot().status,
        last_md_ts_ms: 1,
      },
      bbo: {
        maker: { bid: 30000, ask: 30010, mid: 30005, ts_ms: 1 },
      },
    });

    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));
    expect(screen.getByText('STALE DATA')).toBeInTheDocument();
  });

  it('uses explicit v0.2 md_age_ms over timestamp fallback when available', async () => {
    getOrderViewSnapshotMock.mockResolvedValueOnce({
      ...makeSnapshot(),
      status: {
        ...makeSnapshot().status,
        md_age_ms: 250,
      },
      bbo: {
        maker: { bid: 30000, ask: 30010, mid: 30005, ts_ms: 1 },
      },
    });

    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));
    expect(screen.queryByText('STALE DATA')).not.toBeInTheDocument();
    expect(screen.getByText('250ms')).toBeInTheDocument();
  });

  it('treats mixed stream ages as stale when l2/trades/bbo contract ages exceed threshold', async () => {
    getOrderViewSnapshotMock.mockResolvedValueOnce({
      ...makeSnapshot(),
      status: {
        ...makeSnapshot().status,
        l2_age_ms: 4500,
        trades_age_ms: 120,
        bbo_age_ms: 95,
      },
      bbo: {
        maker: { bid: 30000, ask: 30010, mid: 30005, ts_ms: 1_700_000_000_000 },
      },
    });

    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));
    expect(screen.getByText('STALE DATA')).toBeInTheDocument();
  });

  it('shows degraded candle source badge when fallback candles are active', async () => {
    getOrderViewSnapshotMock.mockResolvedValueOnce({
      ...makeSnapshot(),
      candles: {
        source: 'bbo_fallback',
        rows: [
          { ts_ms: 1_700_000_000_000, open: 30000, high: 30000, low: 30000, close: 30000, volume: 0 },
        ],
      },
    });

    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));
    expect(screen.getByText('CANDLES: BBO (DEGRADED)')).toBeInTheDocument();
  });

  it('coalesces sub-second chart deltas to update without full reset', async () => {
    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    const baselineSetDataCalls = chartSetDataMock.mock.calls.length;
    const baselineUpdateCalls = chartUpdateMock.mock.calls.length;

    const deltaHandler = Array.from(socketHandlers.get('order_view_delta') || [])[0];
    expect(typeof deltaHandler).toBe('function');
    act(() => {
      deltaHandler(
        makeDelta(1, {
          candles: {
            source: 'trades',
            candle_current: {
              ts_ms: 1_700_000_001_000,
              open: 30005,
              high: 30015,
              low: 30001,
              close: 30012,
              volume: 1.9,
            },
          },
        })
      );
    });

    await waitFor(() => {
      expect(useOrderViewStore.getState().lastSeq).toBe(1);
      expect(chartUpdateMock.mock.calls.length).toBeGreaterThan(baselineUpdateCalls);
    });
    expect(chartSetDataMock.mock.calls.length).toBeLessThanOrEqual(
      baselineSetDataCalls + 3
    );
  });

  it('uses latest global resync id for reconnect snapshots', async () => {
    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    const connectHandler = Array.from(socketHandlers.get('connect') || [])[0];
    expect(typeof connectHandler).toBe('function');

    await act(async () => {
      bumpGlobalResync('test-reconnect');
      connectHandler();
      await Promise.resolve();
    });

    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(2));
    const latestResyncId = useResyncStore.getState().resyncId;
    await waitFor(() => {
      const state = useOrderViewStore.getState();
      expect(state.appliedResyncId).toBe(latestResyncId);
      expect(state.lastSnapshotTsMs).toBe(1_700_000_000_000);
    });
  });

  it('emits chart fill markers in ascending time order', async () => {
    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    const deltaHandler = Array.from(socketHandlers.get('order_view_delta') || [])[0];
    expect(typeof deltaHandler).toBe('function');
    chartSetMarkersMock.mockClear();

    act(() => {
      deltaHandler(
        makeDelta(1, {
          events: {
            rows: [
              {
                event_key: 'fill-1',
                ts_ms: 1_700_000_000_100,
                type: 'fill',
                side: 'buy',
                px: '30001',
                qty: '0.1',
              },
            ],
          },
        })
      );
      deltaHandler(
        makeDelta(2, {
          events: {
            rows: [
              {
                event_key: 'fill-2',
                ts_ms: 1_700_000_000_200,
                type: 'fill',
                side: 'sell',
                px: '30002',
                qty: '0.2',
              },
            ],
          },
        })
      );
    });

    await waitFor(() => expect(useOrderViewStore.getState().lastSeq).toBe(2));
    const latestCall = chartSetMarkersMock.mock.calls[chartSetMarkersMock.mock.calls.length - 1];
    expect(latestCall).toBeTruthy();
    const markers = latestCall[0] as Array<{ time: number; text?: string }>;
    const times = markers.map((marker) => Number(marker.time));
    const sorted = [...times].sort((lhs, rhs) => lhs - rhs);
    expect(times).toEqual(sorted);
    expect(markers.every((marker) => String(marker.text || '').trim() === '')).toBe(true);
  });

  it('sends periodic subscribe heartbeat while connected', async () => {
    const setIntervalSpy = vi.spyOn(window, 'setInterval');
    try {
      render(<OrderView />);
      await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

      const heartbeatCall = setIntervalSpy.mock.calls.find((call) => Number(call[1]) === 30_000);
      expect(heartbeatCall).toBeTruthy();
      const heartbeatFn = heartbeatCall?.[0];
      expect(typeof heartbeatFn).toBe('function');

      const before = socketEmitMock.mock.calls.filter((call) => call[0] === 'order_view_subscribe').length;
      act(() => {
        (heartbeatFn as () => void)();
      });
      const after = socketEmitMock.mock.calls.filter((call) => call[0] === 'order_view_subscribe').length;
      expect(after).toBeGreaterThan(before);
    } finally {
      setIntervalSpy.mockRestore();
    }
  });

  it('flushes pending deltas when requestAnimationFrame is throttled', async () => {
    const rafSpy = vi
      .spyOn(window, 'requestAnimationFrame')
      .mockImplementation(() => 999 as unknown as number);
    try {
      render(<OrderView />);
      await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

      const deltaHandler = Array.from(socketHandlers.get('order_view_delta') || [])[0];
      expect(typeof deltaHandler).toBe('function');

      act(() => {
        deltaHandler(makeDelta(1));
      });

      await waitFor(() => {
        expect(useOrderViewStore.getState().lastSeq).toBe(1);
      });
    } finally {
      rafSpy.mockRestore();
    }
  });

  it('replays v0.2 fixture stream with deterministic final state', async () => {
    const { snapshot, deltas } = loadReplayFixture();
    getOrderViewSnapshotMock.mockResolvedValueOnce(snapshot);

    render(<OrderView />);
    await waitFor(() => expect(getOrderViewSnapshotMock).toHaveBeenCalledTimes(1));

    const deltaHandler = Array.from(socketHandlers.get('order_view_delta') || [])[0];
    expect(typeof deltaHandler).toBe('function');

    act(() => {
      for (const delta of deltas) {
        deltaHandler(delta);
      }
    });

    await waitFor(() => expect(useOrderViewStore.getState().lastSeq).toBe(3));
    const state = useOrderViewStore.getState();
    expect(state.roomId).toBe(snapshot.room_id);
    expect(state.stateRev).toBe('fixture-rev-2');
    expect(state.events.slice(0, 3).map((row) => row.event_key)).toEqual([
      'evt-replay-3',
      'evt-replay-2',
      'evt-replay-1',
    ]);
    expect(state.fills[0]?.event_key).toBe('evt-replay-3');
    expect(state.status?.md_ok).toBe(false);
    expect(state.status?.notes).toEqual(['replay_health_stale']);
  });
});
