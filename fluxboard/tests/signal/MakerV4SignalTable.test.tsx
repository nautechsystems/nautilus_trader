import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import { api } from '@/api';
import SignalTable from '@/components/domain/signal/SignalTable';
import MakerV4SignalTable from '@/components/domain/signal/MakerV4SignalTable';
import { useSignalStore } from '@/stores';
import type { SignalStrategy } from '@/types';

vi.mock('@/api', () => ({
  api: {
    getSignalStrategies: vi.fn(() => Promise.resolve({ strategies: [], server_time: '2024-01-01 12:00:00' })),
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

let currentSignalState: any;

function initSignalState(state: any) {
  currentSignalState = state;
  const mockedUseSignalStore = useSignalStore as any;
  mockedUseSignalStore.getState = () => currentSignalState;
  mockedUseSignalStore.mockImplementation((selector?: any) =>
    selector ? selector(currentSignalState) : currentSignalState,
  );
}

function buildMakerV4Strategy(): SignalStrategy {
  return {
    id: 'aapl_tradexyz_makerv4',
    strategy_family: 'maker_v4',
    running: true,
    tradeable: false,
    blocked: true,
    balances_ok: true,
    params: { bot_on: '0', qty: '1' },
    meta: {
      chain: 'equities',
      strategy_groups: 'equities',
      strategy_family: 'maker_v4',
      strategy_version: 'v4',
      class: 'maker_v4',
      param_set: 'makerv4',
      base_asset: 'AAPL',
      quote_asset: 'USD',
    },
    maker_role_map: {
      maker_leg: 'maker',
      hedge_leg: 'hedge-blueocean',
      ref_leg: 'hedge',
    },
    legs_order: ['maker', 'hedge', 'hedge-blueocean'],
    legs: {
      maker: {
        exchange: 'hyperliquid',
        instrument_id: 'xyz:AAPL-USD-PERP.HYPERLIQUID',
        update_ts_ms: 1_700_000_000_000,
      },
      hedge: {
        exchange: 'ibkr',
        instrument_id: 'AAPL.NASDAQ',
        update_ts_ms: 1_700_000_000_500,
      },
      'hedge-blueocean': {
        exchange: 'ibkr',
        instrument_id: 'AAPL.BLUEOCEAN',
        update_ts_ms: 1_700_000_000_550,
      },
    },
    maker_v4: {
      quote_snapshot: {
        ts_ms: 1_700_000_000_500,
        effective_spread_bps: 6.5,
        quoted_spread_bps: 8.0,
        expected_maker_fee_bps: 0.25,
        assumed_hedge_fee_bps: 1.0,
        hedge_ready: false,
        hedge_route: 'BLUEOCEAN',
        hedge_disabled_reason: 'stale_quote',
        fee_snapshot_age_s: 9,
        hedge_latency_ms: 45,
        hedge_slippage_bps_vs_mid: 1.5,
        maker_leg: {
          venue: 'HYPERLIQUID',
          instrument_id: 'xyz:AAPL-USD-PERP.HYPERLIQUID',
        },
        hedge_leg: {
          venue: 'IBKR',
          instrument_id: 'AAPL.BLUEOCEAN',
          route: 'BLUEOCEAN',
        },
        ref_leg: {
          venue: 'IBKR',
          instrument_id: 'AAPL.NASDAQ',
        },
      },
    },
    state: {
      ts_ms: 1_700_000_000_500,
      state: 'hedge_paused',
    },
  };
}

describe('MakerV4SignalTable', () => {
  it('renders a dedicated maker v4 signal table with both venue legs, route, and effective spread', () => {
    render(
      <MakerV4SignalTable
        rows={[buildMakerV4Strategy()]}
      />,
    );

    expect(screen.getByText('Maker Market')).toBeInTheDocument();
    expect(screen.getByText('Hedge Market')).toBeInTheDocument();
    expect(screen.getByText('Effective Spread')).toBeInTheDocument();
    expect(screen.getByText('aapl_tradexyz_makerv4')).toBeInTheDocument();
    expect(screen.getByText(/Hyperliquid/i)).toBeInTheDocument();
    expect(screen.getByText(/IBKR/i)).toBeInTheDocument();
    expect(screen.getByText('6.5 bps')).toBeInTheDocument();
    expect(screen.getAllByText(/BLUEOCEAN/i).length).toBeGreaterThan(0);
    expect(screen.getByText('Paused')).toBeInTheDocument();
  });

  it('renders the routed hedge identity and visible hedge latency from the quote snapshot', () => {
    render(
      <MakerV4SignalTable
        rows={[buildMakerV4Strategy()]}
      />,
    );

    expect(screen.getByText('IBKR AAPL.BLUEOCEAN')).toBeInTheDocument();
    expect(screen.getByText(/45 ms/i)).toBeInTheDocument();
  });

  it('switches the equities signal route to the dedicated maker v4 table', async () => {
    vi.clearAllMocks();
    (api.getSignalStrategies as any).mockResolvedValue({
      strategies: [buildMakerV4Strategy()],
      server_time: '2024-01-01 12:00:00',
      server_ts_ms: 1_700_000_001_500,
    });
    initSignalState({
      rows: [buildMakerV4Strategy()],
      setRows: vi.fn(),
      mergeStrategy: vi.fn(),
      mergeStrategies: vi.fn(),
    });

    render(
      <MemoryRouter
        initialEntries={['/equities/signal']}
        future={{ v7_startTransition: true, v7_relativeSplatPath: true }}
      >
        <SignalTable />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('maker-v4-signal-table')).toBeInTheDocument();
    });

    expect(screen.getByText('Maker Market')).toBeInTheDocument();
    expect(screen.getByText('Hedge Market')).toBeInTheDocument();
    expect(screen.getByText('Effective Spread')).toBeInTheDocument();
    expect(screen.queryByText('FV market')).not.toBeInTheDocument();
  });
});
