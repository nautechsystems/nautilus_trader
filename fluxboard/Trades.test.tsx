import { cleanup, fireEvent, render, screen, waitFor, act } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

import Trades from './Trades';
import { useTradesStore } from './stores';
import { api } from './api';
import { socket } from './sockets';
import { playTradeClick } from './utils/sound';
import type { TradeRow } from './types';

vi.mock('./api', () => ({
  api: {
    getTrades: vi.fn(),
    getTradesDelta: vi.fn(),
  },
}));

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

vi.mock('./utils/sound', () => ({
  playTradeClick: vi.fn(),
}));

let lastTradesTableProps: any = null;
vi.mock('./components/trades/TradesTable', () => ({
  TradesTable: (props: any) => {
    lastTradesTableProps = props;
    return (
      <div data-testid="trades-table-mock">
        {props.trades?.length ?? 0} rows
      </div>
    );
  },
}));

const mockGetTrades = vi.mocked(api.getTrades);
const mockGetTradesDelta = vi.mocked(api.getTradesDelta);
const mockPlayTradeClick = vi.mocked(playTradeClick);

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

  (useTradesStore as unknown as { mockImplementation: (fn?: (state: StoreMock) => unknown) => void }).mockImplementation((selector?: (state: StoreMock) => unknown) => {
    return typeof selector === 'function' ? selector(store) : store;
  });

  return { setSnapshot, applyDelta, appendHistorical, store };
}

type TradesApiResponse = Awaited<ReturnType<typeof api.getTrades>>;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

async function renderTrades(options: { apiResponse?: Partial<TradesApiResponse>; storeOverrides?: Partial<StoreMock> } = {}) {
  const { setSnapshot, applyDelta, store } = setupStore(options.storeOverrides);

  const initialPageSize = parseInt(sessionStorage.getItem('trades_page_size') || '100', 10);
  mockGetTrades.mockResolvedValue({
    rows: [],
    total: 0,
    page: 1,
    page_size: initialPageSize,
    last_seq: 0,
    has_more: false,
    next_cursor: null,
    ...options.apiResponse,
  });

  mockGetTradesDelta.mockResolvedValue({ rows: [], last_seq: 0, reset_required: false });

  render(<Trades />);

  await waitFor(() => {
    expect(mockGetTrades).toHaveBeenCalled();
    expect(lastTradesTableProps?.onScrollStateChange).toBeInstanceOf(Function);
  });

  return { setSnapshot, applyDelta, store };
}

describe('Trades pagination and snapshot loading', () => {
  beforeEach(() => {
    (window.location as any).pathname = '/trades';
    sessionStorage.clear();
    mockGetTrades.mockReset();
    mockGetTradesDelta.mockReset();
    (useTradesStore as unknown as { mockReset: () => void }).mockReset();
    vi.mocked(socket.on).mockClear();
    vi.mocked(socket.off).mockClear();
    mockPlayTradeClick.mockClear();
    lastTradesTableProps = null;
  });

  afterEach(() => {
    cleanup();
  });

  it('loads the initial snapshot using the stored page size', async () => {
    sessionStorage.setItem('trades_page_size', '50');
    await renderTrades();

    const [page, pageSize, params] = mockGetTrades.mock.calls[0];
    expect(page).toBe(1);
    expect(pageSize).toBe(50);
    expect(params).toMatchObject({ sort: 'ts_desc' });
    expect(sessionStorage.getItem('trades_page_size')).toBe('50');
  });

  it('renders rows newest-first from the store by default', async () => {
    const older = makeTradeRow({ row_id: 'older', ts: 1000, seq: 1000 });
    const newer = makeTradeRow({ row_id: 'newer', ts: 2000, seq: 2000 });
    await renderTrades({ storeOverrides: { rows: [newer, older] } });

    expect(lastTradesTableProps?.trades.map((r: TradeRow) => r.row_id)).toEqual(['newer', 'older']);
  });

  it('enables Next when there are more pages and calls API with the next page', async () => {
    await renderTrades({ apiResponse: { total: 250, page: 1, page_size: 100, has_more: true } });

    const nextBtn = screen.getByRole('button', { name: /next/i });
    expect(nextBtn).toBeEnabled();

    mockGetTrades.mockClear();
    mockGetTrades.mockResolvedValueOnce({
      rows: [],
      total: 250,
      page: 2,
      page_size: 100,
      last_seq: 0,
      has_more: true,
      next_cursor: null,
    });

    fireEvent.click(nextBtn);

    await waitFor(() => expect(mockGetTrades).toHaveBeenCalledTimes(1));
    const [calledPage, calledPageSize] = mockGetTrades.mock.calls[0];
    expect(calledPage).toBe(2);
    expect(calledPageSize).toBe(100);
  });

  it('disables Next on the last page and Prev on the first page', async () => {
    await renderTrades({ apiResponse: { total: 100, page: 1, page_size: 100 } });

    const prevBtn = screen.getByRole('button', { name: /prev/i });
    const nextBtn = screen.getByRole('button', { name: /next/i });
    expect(prevBtn).toBeDisabled();
    expect(nextBtn).toBeDisabled();
  });

  it('shows correct page indicator and updates on navigation', async () => {
    await renderTrades({ apiResponse: { total: 250, page: 1, page_size: 100, has_more: true } });

    // Initial indicator: 250 total with 100/page => 3 pages
    expect(await screen.findByText(/Page 1 of 3/i)).toBeInTheDocument();

    // Click Next and ensure indicator updates to page 2
    const nextBtn = screen.getByRole('button', { name: /next/i });
    mockGetTrades.mockClear();
    mockGetTrades.mockResolvedValueOnce({
      rows: [],
      total: 250,
      page: 2,
      page_size: 100,
      last_seq: 0,
      has_more: true,
      next_cursor: null,
    });

    fireEvent.click(nextBtn);
    await waitFor(() => expect(screen.getByText(/Page 2 of 3/i)).toBeInTheDocument());
  });

  it('shows single-page indicator when total equals page size', async () => {
    await renderTrades({ apiResponse: { total: 100, page: 1, page_size: 100 } });
    expect(await screen.findByText(/Page 1 of 1/i)).toBeInTheDocument();
  });

  it('enables Next when has_more is true even when total equals page size', async () => {
    await renderTrades({ apiResponse: { total: 100, page: 1, page_size: 100, has_more: true } });
    const nextBtn = screen.getByRole('button', { name: /next/i });
    expect(nextBtn).toBeEnabled();
  });

  it('does not advance beyond one page while next-page fetch is in flight', async () => {
    setupStore();
    mockGetTradesDelta.mockResolvedValue({ rows: [], last_seq: 0, reset_required: false });

    const pageTwo = deferred<TradesApiResponse>();
    mockGetTrades.mockImplementation((page: number) => {
      if (page === 1) {
        return Promise.resolve({
          rows: [],
          total: 250,
          page: 1,
          page_size: 100,
          last_seq: 0,
          has_more: true,
          next_cursor: null,
        });
      }
      if (page === 2) {
        return pageTwo.promise;
      }
      return Promise.resolve({
        rows: [],
        total: 250,
        page,
        page_size: 100,
        last_seq: 0,
        has_more: false,
        next_cursor: null,
      });
    });

    render(<Trades />);
    await waitFor(() => expect(mockGetTrades).toHaveBeenCalledTimes(1));

    const nextBtn = screen.getByRole('button', { name: /next/i });
    fireEvent.click(nextBtn);
    expect(nextBtn).toBeDisabled();
    fireEvent.click(nextBtn);

    await waitFor(() => expect(mockGetTrades).toHaveBeenCalledTimes(2));
    expect(mockGetTrades.mock.calls.map(([calledPage]) => calledPage)).toEqual([1, 2]);

    pageTwo.resolve({
      rows: [],
      total: 250,
      page: 2,
      page_size: 100,
      last_seq: 0,
      has_more: false,
      next_cursor: null,
    });

    await waitFor(() => expect(screen.getByText(/Page 2 of 3/i)).toBeInTheDocument());
    expect(mockGetTrades.mock.calls.map(([calledPage]) => calledPage)).toEqual([1, 2]);
  });

  it('keeps tokenmm pagination clickable when a silent refresh preempts an in-flight page fetch', async () => {
    (window.location as any).pathname = '/tokenmm/trades';
    setupStore();
    mockGetTradesDelta.mockResolvedValue({ rows: [], last_seq: 0, reset_required: false });

    let pageTwoCalls = 0;
    mockGetTrades.mockImplementation((page: number, _pageSize: number, _params: any, init?: RequestInit) => {
      if (page === 1) {
        return Promise.resolve({
          rows: [],
          total: 300,
          page: 1,
          page_size: 100,
          last_seq: 0,
          has_more: true,
          next_cursor: null,
        });
      }
      if (page === 2) {
        pageTwoCalls += 1;
        if (pageTwoCalls === 1) {
          return new Promise<TradesApiResponse>((_resolve, reject) => {
            const signal = init?.signal as AbortSignal | undefined;
            if (signal?.aborted) {
              reject(new DOMException('Aborted', 'AbortError'));
              return;
            }
            signal?.addEventListener(
              'abort',
              () => reject(new DOMException('Aborted', 'AbortError')),
              { once: true },
            );
          });
        }
        return Promise.resolve({
          rows: [],
          total: 300,
          page: 2,
          page_size: 100,
          last_seq: 0,
          has_more: true,
          next_cursor: null,
        });
      }
      return Promise.resolve({
        rows: [],
        total: 300,
        page,
        page_size: 100,
        last_seq: 0,
        has_more: true,
        next_cursor: null,
      });
    });

    render(<Trades />);
    await waitFor(() => expect(mockGetTrades).toHaveBeenCalledTimes(1));

    const tradeUpdateHandler = vi.mocked(socket.on).mock.calls.find(
      ([event]) => event === 'trade_update',
    )?.[1] as ((msg: any) => void) | undefined;
    expect(tradeUpdateHandler).toBeInstanceOf(Function);

    const nextBtn = screen.getByRole('button', { name: /next/i });
    fireEvent.click(nextBtn);
    await waitFor(() => expect(mockGetTrades).toHaveBeenCalledTimes(2));
    expect(nextBtn).toBeDisabled();

    act(() => {
      tradeUpdateHandler?.({
        op: 'upsert',
        row_id: 'row-live',
        version: 1,
        seq: 501,
        exchange: 'bybit',
        coin: 'PLUME/USDT',
      });
    });

    await waitFor(() => expect(mockGetTrades).toHaveBeenCalledTimes(3), { timeout: 1500 });
    await waitFor(() => expect(nextBtn).toBeEnabled(), { timeout: 1500 });
  });

  it('clamps invalid stored page size to safe default', async () => {
    sessionStorage.setItem('trades_page_size', '5000');
    await renderTrades();

    const [page, pageSize] = mockGetTrades.mock.calls[0];
    expect(page).toBe(1);
    expect(pageSize).toBe(100);
    expect(sessionStorage.getItem('trades_page_size')).toBe('100');
  });

  it('reveals Jump to latest when history view is active and refreshes on click', async () => {
    await renderTrades();

    act(() => {
      lastTradesTableProps?.onScrollStateChange?.({ atTop: false });
    });

    const jumpButton = await screen.findByRole('button', { name: /jump to latest/i });
    mockGetTrades.mockClear();
    mockGetTrades.mockResolvedValueOnce({
      rows: [],
      total: 0,
      page: 1,
      page_size: 100,
      last_seq: 0,
      has_more: false,
      next_cursor: null,
    });

    fireEvent.click(jumpButton);

    await waitFor(() => expect(mockGetTrades).toHaveBeenCalled());
  });

  it('stops applying live deltas while viewing historical pages', async () => {
    const { applyDelta } = await renderTrades({ apiResponse: { total: 250, page: 1, page_size: 100, has_more: true } });

    const tradeUpdateCall = vi.mocked(socket.on).mock.calls.find(
      ([event]) => event === 'trade_update',
    );
    expect(tradeUpdateCall).toBeTruthy();
    const tradeUpdateHandler = tradeUpdateCall?.[1] as ((msg: any) => void) | undefined;
    expect(tradeUpdateHandler).toBeInstanceOf(Function);

    const nextBtn = screen.getByRole('button', { name: /next/i });
    mockGetTrades.mockClear();
    mockGetTrades.mockResolvedValueOnce({
      rows: [],
      total: 250,
      page: 2,
      page_size: 100,
      last_seq: 0,
      has_more: true,
      next_cursor: null,
    });

    fireEvent.click(nextBtn);
    await waitFor(() => expect(mockGetTrades).toHaveBeenCalled());
    applyDelta.mockClear();

    act(() => {
      tradeUpdateHandler?.({
        op: 'upsert',
        row_id: 'row-historical',
        version: 1,
        seq: 999,
      });
    });

    expect(applyDelta).not.toHaveBeenCalled();
  });

  it('refetches snapshot when toggling time sort', async () => {
    await renderTrades();

    expect(lastTradesTableProps).not.toBeNull();
    mockGetTrades.mockClear();
    mockGetTrades.mockResolvedValueOnce({
      rows: [],
      total: 0,
      page: 1,
      page_size: 100,
      last_seq: 0,
      has_more: false,
      next_cursor: null,
    });

    act(() => {
      lastTradesTableProps?.onTimeSortChange?.('ts_asc');
    });

    await waitFor(() => expect(mockGetTrades).toHaveBeenCalled());
    const params = mockGetTrades.mock.calls[0][2];
    expect(params?.sort).toBe('ts_asc');

    await waitFor(() => expect(lastTradesTableProps?.sortDirection).toBe('ts_asc'));
  });

  it('does not play sound when no live upserts are applied', async () => {
    const { applyDelta } = await renderTrades();
    const handler = vi.mocked(socket.on).mock.calls.find(([event]) => event === 'trade_update')?.[1] as ((msg: any) => void) | undefined;
    expect(handler).toBeInstanceOf(Function);
    applyDelta.mockReturnValueOnce({ upserts: 0, deletes: 0, changed: false, newRows: 0 });
    act(() => {
      handler?.({ op: 'upsert', row_id: 'row-live', seq: 1, version: 1 });
    });
    await waitFor(() => expect(applyDelta).toHaveBeenCalled());
    expect(mockPlayTradeClick).not.toHaveBeenCalled();
  });

  it('plays sound when live upserts occur', async () => {
    const { applyDelta } = await renderTrades();
    const handler = vi.mocked(socket.on).mock.calls.find(([event]) => event === 'trade_update')?.[1] as ((msg: any) => void) | undefined;
    expect(handler).toBeInstanceOf(Function);
    applyDelta.mockReturnValueOnce({ upserts: 1, deletes: 0, changed: true, newRows: 1 });
    act(() => {
      handler?.({ op: 'upsert', row_id: 'row-live', seq: 1, version: 1 });
    });
    await waitFor(() => expect(mockPlayTradeClick).toHaveBeenCalled());
  });

  it('does not apply live deltas when time sort is ascending', async () => {
    const { applyDelta } = await renderTrades();
    const tradeUpdateCall = vi.mocked(socket.on).mock.calls.find(([event]) => event === 'trade_update');
    const tradeUpdateHandler = tradeUpdateCall?.[1] as ((msg: any) => void) | undefined;
    expect(tradeUpdateHandler).toBeInstanceOf(Function);

    mockGetTrades.mockClear();
    mockGetTrades.mockResolvedValueOnce({
      rows: [],
      total: 0,
      page: 1,
      page_size: 100,
      last_seq: 0,
      has_more: false,
      next_cursor: null,
    });

    act(() => {
      lastTradesTableProps?.onTimeSortChange?.('ts_asc');
    });

    await waitFor(() => expect(mockGetTrades).toHaveBeenCalled());
    applyDelta.mockClear();

    act(() => {
      tradeUpdateHandler?.({ op: 'upsert', row_id: 'row-live', seq: 2, version: 1 });
    });

    expect(applyDelta).not.toHaveBeenCalled();
  });

  it('ignores socket events that do not satisfy current filters', async () => {
    sessionStorage.setItem('trades_filters', JSON.stringify({ exchange: 'rooster:plume' }));
    const { applyDelta } = await renderTrades();

    const handler = vi.mocked(socket.on).mock.calls.find(([event]) => event === 'trade_update')?.[1] as ((msg: any) => void) | undefined;
    expect(handler).toBeInstanceOf(Function);

    applyDelta.mockClear();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'bybit-row',
        version: 1,
        seq: 123,
        exchange: 'bybit',
        coin: 'PLUME/USDT',
      });
    });

    expect(applyDelta).not.toHaveBeenCalled();
  });

  it('applies socket trade updates on non-default profile surfaces', async () => {
    (window.location as any).pathname = '/tokenmm/trades';
    const { applyDelta } = await renderTrades();

    const handler = vi.mocked(socket.on).mock.calls.find(([event]) => event === 'trade_update')?.[1] as ((msg: any) => void) | undefined;
    expect(handler).toBeInstanceOf(Function);

    applyDelta.mockClear();
    act(() => {
      handler?.({ op: 'upsert', row_id: 'row-live', seq: 444, version: 1, exchange: 'bybit', coin: 'PLUME/USDT' });
    });

    await waitFor(() => expect(applyDelta).toHaveBeenCalled());
  });

});
