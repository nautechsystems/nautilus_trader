import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import Trades from './Trades';
import { api } from './api';
import { useTradesStore } from './stores';

vi.mock('@/hooks/useIsMobile', () => ({ useIsMobile: () => true }));

vi.mock('./api', async (importOriginal) => {
  const mod = await importOriginal<any>();
  return {
    ...mod,
    api: {
      ...mod.api,
      getTrades: vi.fn(),
      getTradesDelta: vi.fn(),
    },
  };
});

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

vi.mock('./sockets', () => ({
  socket: {
    on: vi.fn(),
    off: vi.fn(),
    connected: true,
  },
}));

vi.mock('./utils/sound', () => ({
  playTradeClick: vi.fn(),
}));

vi.mock('./utils/storage', () => ({
  getSoundMuted: vi.fn(() => false),
  setSoundMuted: vi.fn(),
}));

vi.mock('./components/trades/TradesTable', () => ({
  TradesTable: (props: any) => (
    <div data-testid="desktop-table">
      {String(props.trades?.[0]?.qty ?? 'none')}
    </div>
  ),
}));

const mockGetTrades = vi.mocked(api.getTrades);
const mockGetTradesDelta = vi.mocked(api.getTradesDelta);
const mockUseStore = vi.mocked(useTradesStore as unknown as any);

const sampleRow = {
  row_id: 'row-1',
  version: 1,
  seq: 1,
  ts: 1,
  time: '2025-01-01T00:00:00.000Z',
  coin: 'BTC',
  exchange: 'bybit',
  side: 'buy',
  price: 25000,
  qty: 0.5,
  mv: 12500,
};

function setupStore() {
  const setSnapshot = vi.fn((rows: any[]) => {
    store.rows = rows;
    return { accepted: true, applied: true, staleRejected: false };
  });
  const applyDelta = vi.fn();
  const store: any = {
    rows: [sampleRow],
    setSnapshot,
    applyDelta,
    lastSeq: 1,
  };
  mockUseStore.mockImplementation((selector?: any) => (selector ? selector(store) : store));
  return store;
}

describe('Trades mobile layout', () => {
  beforeEach(() => {
    mockGetTrades.mockResolvedValue({
      rows: [sampleRow],
      total: 1,
      page: 1,
      page_size: 100,
      last_seq: 1,
      has_more: false,
      next_cursor: null,
    } as any);
    mockGetTradesDelta.mockResolvedValue({
      rows: [],
      last_seq: 1,
      reset_required: false,
    } as any);
    mockUseStore.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it('shows compact headers and pagination controls on mobile variant', async () => {
    setupStore();
    render(<Trades variant="mobile" showHeader={false} />);

    await waitFor(() => {
      expect(screen.getByText(/Loaded\s+1\s+of\s+1/i)).toBeInTheDocument();
    });

    expect(screen.getByRole('button', { name: /Filters/i })).toBeInTheDocument();
    expect(screen.getByTestId('desktop-table')).toBeInTheDocument();

    expect(screen.getByRole('button', { name: /Prev/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Next/i })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Jump to latest/i })).not.toBeInTheDocument();
  });

  it('keeps base quantity as the primary row qty on mobile tokenmm trades', async () => {
    const okxRow = {
      ...sampleRow,
      row_id: 'okx-row',
      instrument_id: 'PLUME-USDT-SWAP.OKX',
      exchange: 'okx',
      coin: 'PLUME',
      qty: 100,
      qty_base: '1000',
      qty_venue: '100',
    };
    setupStore();
    mockGetTrades.mockResolvedValueOnce({
      rows: [okxRow],
      total: 1,
      page: 1,
      page_size: 100,
      last_seq: 1,
      has_more: false,
      next_cursor: null,
    } as any);

    render(<Trades variant="mobile" showHeader={false} />);

    await waitFor(() => {
      expect(screen.getByTestId('desktop-table')).toHaveTextContent('1000');
    });
  });
});
