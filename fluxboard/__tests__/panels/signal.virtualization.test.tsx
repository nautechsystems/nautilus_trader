import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import type { SignalStrategy } from '@/types';

const capturedDataTableProps: any[] = [];

vi.mock('@/components/ui/table/DataTable', () => ({
  DataTable: (props: any) => {
    capturedDataTableProps.push(props);
    return <div data-testid="mock-data-table" />;
  },
}));

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: vi.fn(() => ({
    getVirtualItems: () => [{ index: 0, start: 0, size: 44 }],
    getTotalSize: () => 44,
    measureElement: vi.fn(),
  })),
}));

vi.mock('@/api', () => ({
  api: {
    getSignalStrategies: vi.fn(() => Promise.resolve({
      strategies: [],
      server_time: '2026-03-23 00:00:00',
    })),
  },
}));

vi.mock('@/sockets', () => ({
  socket: {
    on: vi.fn(),
    off: vi.fn(),
    connected: false,
  },
}));

vi.mock('@/hooks/useMobileLayout', () => ({
  useMobileLayout: () => ({
    viewport: 'desktop',
    isMobile: false,
    isMobileViewport: false,
    density: 'desktop',
    isTouch: false,
    width: 1280,
    height: 720,
  }),
  useDensityMode: () => 'desktop',
}));

vi.mock('@/stores', async () => {
  const actual = await vi.importActual<any>('@/stores');
  return { ...actual, useSignalStore: vi.fn() };
});

vi.mock('@/components/ui/popover/Popover', () => ({
  default: ({ children }: any) => <div>{children}</div>,
  Popover: ({ children }: any) => <div>{children}</div>,
  PopoverTrigger: ({ children }: any) => <div>{children}</div>,
  PopoverContent: ({ children }: any) => <div>{children}</div>,
}));

const { default: SignalTable } = await import('@/components/domain/signal/SignalTable');
const { useSignalStore } = await import('@/stores');

let currentSignalState: any;

function initSignalState(state: any) {
  currentSignalState = state;
  const mockedUseSignalStore = useSignalStore as any;
  mockedUseSignalStore.getState = () => currentSignalState;
  mockedUseSignalStore.mockImplementation((selector?: any) =>
    selector ? selector(currentSignalState) : currentSignalState
  );
}

function createMockStrategy(index: number): SignalStrategy {
  const id = `signal-${String(index).padStart(3, '0')}`;
  return {
    id,
    params: {
      bot_on: index % 2 === 0 ? '1' : '0',
      cex_bid_edge: '10',
      cex_ask_edge: '10',
      pool_edge: '10',
      qty: String(100 + index),
      slippage_bps: '50',
    },
    legs: {
      A: {
        exchange: 'bybit',
        coin: 'PLUME',
        decision_bid: 1.0,
        decision_ask: 1.01,
        net_edge_bps: 10,
        update_time: '2026-03-23 00:00:00',
      },
      B: {
        exchange: 'rooster',
        coin: 'WPLUME',
        decision_bid: 1.02,
        decision_ask: 1.03,
        net_edge_bps: 12,
        update_time: '2026-03-23 00:00:00',
      },
    },
    balances_ok: true,
    edge2_bps: 5,
    risk_delta: 100 + index,
  };
}

describe('SignalTable standard virtualization', () => {
  beforeEach(() => {
    capturedDataTableProps.length = 0;
    vi.clearAllMocks();
    initSignalState({
      rows: Array.from({ length: 200 }, (_, index) => createMockStrategy(index)),
      setRows: vi.fn(),
      mergeStrategy: vi.fn(),
    });
  });

  it('passes a virtualizer to the standard desktop DataTable path', async () => {
    render(
      <MemoryRouter initialEntries={['/signal']}>
        <SignalTable />
      </MemoryRouter>
    );

    await waitFor(() => {
      expect(capturedDataTableProps.length).toBeGreaterThan(0);
    });

    const latestProps = capturedDataTableProps.at(-1);
    expect(latestProps.virtualizer).toBeDefined();
  });
});
