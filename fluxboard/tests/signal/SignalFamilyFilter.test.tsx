import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { api } from '@/api';
import SignalTable from '@/components/domain/signal/SignalTable';
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
      operator: {
        execution_mode: 'take_take',
        behavior: 'take_take',
        hedge_policy: {
          route: 'SMART',
          time_in_force: 'DAY',
          outside_rth: true,
          include_overnight: true,
          cancel_after_ms: 5000,
        },
        fee_assumptions: {
          ibkr_fee_plan: 'tiered',
          ibkr_fee_min_usd: 0.35,
          hl_taker_fee_bps: 4.5,
          hl_maker_fee_bps: 0.25,
          assumed_hedge_fee_bps: 1.0,
        },
      },
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

function renderSignalTable(pathname: string) {
  return render(
    <MemoryRouter
      initialEntries={[pathname]}
      future={{ v7_startTransition: true, v7_relativeSplatPath: true }}
    >
      <SignalTable />
    </MemoryRouter>,
  );
}

describe('Signal family filter', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    const strategy = buildMakerV4Strategy();
    (api.getSignalStrategies as any).mockResolvedValue({
      strategies: [strategy],
      server_time: '2024-01-01 12:00:00',
      server_ts_ms: 1_700_000_001_500,
    });
    initSignalState({
      rows: [strategy],
      setRows: vi.fn(),
      mergeStrategy: vi.fn(),
      mergeStrategies: vi.fn(),
    });
  });

  it('shows a locked maker_v4 family control on equities signal while keeping the strategy filter', async () => {
    renderSignalTable('/equities/signal');

    await screen.findByTestId('maker-v4-signal-table');

    const familyFilter = screen.getByLabelText('Signal family');
    expect(familyFilter).toBeInTheDocument();
    expect(familyFilter).toHaveValue('maker_v4');
    expect(familyFilter).toBeDisabled();

    fireEvent.click(screen.getByText('Filters'));
    expect(screen.getByPlaceholderText(/Strategy ID/i)).toBeInTheDocument();
  });
});
