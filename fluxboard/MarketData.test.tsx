import { act, render, screen, fireEvent } from '@testing-library/react';
import { useEffect } from 'react';
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import MarketData from './MarketData';
import { useMarketDataStore } from './stores/marketDataStore';

const snapshotMock = vi.fn();
const pollingState = vi.hoisted(() => ({
  fn: null as null | (() => void | Promise<void>),
  calls: [] as Array<{
    interval: number;
    enabled: boolean;
    options: unknown;
  }>,
}));
const flushPromises = () => new Promise((resolve) => setTimeout(resolve, 0));

vi.mock('./api', () => ({
  api: {
    getMarketDataSnapshot: () => snapshotMock(),
  },
}));

// Disable timers auto-start to control polling in tests
vi.mock('./hooks', async () => {
  const actual = await vi.importActual<any>('./hooks');
  return {
    ...actual,
    usePolling: (
      fn: () => void | Promise<void>,
      interval: number,
      enabled = true,
      options?: unknown
    ) => {
      pollingState.calls.push({ interval, enabled, options });
      useEffect(() => {
        pollingState.fn = enabled ? fn : null;
        if (enabled) {
          void fn();
        }
        return () => {
          if (pollingState.fn === fn) {
            pollingState.fn = null;
          }
        };
      }, [fn, enabled]);
    },
  };
});

vi.mock('./components/shared/Pager', () => ({
  Pager: ({ page, pageSize, total }: { page: number; pageSize: number; total: number }) => (
    <div data-testid="pager">
      {page}–{pageSize} / {total} rows
    </div>
  ),
}));

describe('MarketData page', () => {
  const invokePoll = async () => {
    await act(async () => {
      await pollingState.fn?.();
      await flushPromises();
      await flushPromises();
    });
  };

  beforeEach(() => {
    pollingState.fn = null;
    pollingState.calls = [];
    let now = 1700000000000;
    vi.spyOn(Date, 'now').mockImplementation(() => {
      now += 1000;
      return now;
    });
    snapshotMock.mockResolvedValue({
      rows: [
        {
          coin: 'BTC/USDT',
          exchange: 'bybit',
          bid: '100',
          bid_qty: '1',
          mid_px: '101',
          ask: '102',
          ask_qty: '2',
          timestamp_ms: 1700000000000,
        },
      ],
      count: 1,
      freshnessKey: 'default-freshness-key',
    });
  });

  afterEach(() => {
    useMarketDataStore.setState({ rows: [], loading: false, lastUpdate: null });
    vi.restoreAllMocks();
  });

  it('renders snapshot rows and default pager', async () => {
    await act(async () => {
      render(<MarketData />);
      await flushPromises();
      await flushPromises();
    });

    await screen.findByText('BTC/USDT');
    expect(screen.getByText('100')).toBeInTheDocument();
    expect(screen.getByText('102')).toBeInTheDocument();
    expect(screen.getByTestId('pager')).toHaveTextContent('1–50 / 1 rows');
    expect(snapshotMock).toHaveBeenCalled();
  });

  it('filters by exchange selection', async () => {
    snapshotMock.mockResolvedValueOnce({
      rows: [
        {
          coin: 'BTC/USDT',
          exchange: 'bybit',
          bid: '100',
          ask: '101',
          mid_px: '100.5',
          bid_qty: '1',
          ask_qty: '1',
          timestamp_ms: 1,
        },
        {
          coin: 'ETH/USDT',
          exchange: 'dex',
          bid: '',
          ask: '',
          mid_px: '2000',
          bid_qty: '',
          ask_qty: '',
          timestamp_ms: 2,
        },
      ],
      count: 2,
    });

    await act(async () => {
      render(<MarketData />);
      await flushPromises();
      await flushPromises();
    });

    await screen.findByText('BTC/USDT');
    const exchangeTrigger = screen.getByRole('button', { name: /Exchange filter/i });
    await act(async () => {
      fireEvent.click(exchangeTrigger);
    });
    const dexCheckbox = screen.getByLabelText('dex');
    await act(async () => {
      fireEvent.click(dexCheckbox);
    });

    expect(screen.queryByText('BTC/USDT')).not.toBeInTheDocument();
    expect(screen.getByText('ETH/USDT')).toBeInTheDocument();
  });

  it('applies row replacement when freshness marker is unchanged but rows change', async () => {
    snapshotMock.mockResolvedValueOnce({
      rows: [
        {
          coin: 'BTC/USDT',
          exchange: 'bybit',
          bid: '100',
          ask: '101',
          mid_px: '100.5',
          bid_qty: '1',
          ask_qty: '1',
          timestamp_ms: 1700000000000,
        },
      ],
      count: 1,
      freshnessKey: 'md-key-1',
    });

    await act(async () => {
      render(<MarketData />);
      await flushPromises();
      await flushPromises();
    });

    const firstState = useMarketDataStore.getState();
    const firstRowsRef = firstState.rows;
    const firstLastUpdate = firstState.lastUpdate ?? 0;
    expect(firstRowsRef[0]?.bid).toBe('100');

    snapshotMock.mockResolvedValueOnce({
      rows: [
        {
          coin: 'BTC/USDT',
          exchange: 'bybit',
          bid: '999',
          ask: '1001',
          mid_px: '1000',
          bid_qty: '1',
          ask_qty: '1',
          timestamp_ms: 1700000000000,
        },
      ],
      count: 1,
      freshnessKey: 'md-key-1',
    });

    await invokePoll();

    const secondState = useMarketDataStore.getState();
    expect(secondState.rows).not.toBe(firstRowsRef);
    expect(secondState.rows[0]?.bid).toBe('999');
    expect(secondState.rows[0]?.ask).toBe('1001');
    expect(secondState.lastUpdate ?? 0).toBeGreaterThan(firstLastUpdate);
  });

  it('skips row replacement when freshness marker and rows are unchanged', async () => {
    const originalRow = {
      coin: 'BTC/USDT',
      exchange: 'bybit',
      bid: '100',
      ask: '101',
      mid_px: '100.5',
      bid_qty: '1',
      ask_qty: '1',
      timestamp_ms: 1700000000000,
    };

    snapshotMock.mockResolvedValueOnce({
      rows: [originalRow],
      count: 1,
      freshnessKey: 'md-key-1',
    });

    await act(async () => {
      render(<MarketData />);
      await flushPromises();
      await flushPromises();
    });

    const firstState = useMarketDataStore.getState();
    const firstRowsRef = firstState.rows;
    const firstLastUpdate = firstState.lastUpdate ?? 0;
    expect(firstRowsRef[0]?.bid).toBe('100');

    snapshotMock.mockResolvedValueOnce({
      rows: [{ ...originalRow }],
      count: 1,
      freshnessKey: 'md-key-1',
    });

    await invokePoll();

    const secondState = useMarketDataStore.getState();
    expect(secondState.rows).toBe(firstRowsRef);
    expect(secondState.rows[0]?.bid).toBe('100');
    expect(secondState.lastUpdate ?? 0).toBeGreaterThan(firstLastUpdate);
  });

  it('replaces rows when freshness marker changes', async () => {
    snapshotMock.mockResolvedValueOnce({
      rows: [
        {
          coin: 'BTC/USDT',
          exchange: 'bybit',
          bid: '100',
          ask: '101',
          mid_px: '100.5',
          bid_qty: '1',
          ask_qty: '1',
          timestamp_ms: 1700000000000,
        },
      ],
      count: 1,
      freshnessKey: 'md-key-1',
    });

    await act(async () => {
      render(<MarketData />);
      await flushPromises();
      await flushPromises();
    });

    const firstRowsRef = useMarketDataStore.getState().rows;
    expect(firstRowsRef[0]?.coin).toBe('BTC/USDT');

    snapshotMock.mockResolvedValueOnce({
      rows: [
        {
          coin: 'ETH/USDT',
          exchange: 'dex',
          bid: '2000',
          ask: '2001',
          mid_px: '2000.5',
          bid_qty: '1',
          ask_qty: '1',
          timestamp_ms: 1700000000100,
        },
      ],
      count: 1,
      freshnessKey: 'md-key-2',
    });

    await invokePoll();

    const secondRows = useMarketDataStore.getState().rows;
    expect(secondRows).not.toBe(firstRowsRef);
    expect(secondRows[0]?.coin).toBe('ETH/USDT');
  });

  it('configures polling with hidden-tab backoff and visible refresh', async () => {
    await act(async () => {
      render(<MarketData />);
      await flushPromises();
      await flushPromises();
    });

    const pollingCall = pollingState.calls.at(-1);
    expect(pollingCall?.interval).toBe(5000);
    expect(pollingCall?.enabled).toBe(true);
    expect(pollingCall?.options).toEqual({
      hiddenIntervalMs: 15000,
      refreshOnVisible: true,
    });
  });
});
