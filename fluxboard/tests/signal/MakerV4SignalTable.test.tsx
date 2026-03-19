import { fireEvent, render, screen, waitFor } from '@testing-library/react';
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
    blocked: false,
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
          maker_taker_fee_bps: 4.5,
          maker_maker_fee_bps: 0.25,
          assumed_hedge_fee_bps: 1.0,
        },
      },
      quote_snapshot: {
        ts_ms: 1_700_000_000_500,
        mid_spread_bps: 2.0,
        arb_bid_spread_bps: 14.0,
        arb_ask_spread_bps: -11.0,
        effective_spread_bps: 6.5,
        quoted_spread_bps: 8.0,
        expected_maker_fee_bps: 0.25,
        assumed_hedge_fee_bps: 1.0,
        hedge_ready: true,
        hedge_route: 'BLUEOCEAN',
        hedge_disabled_reason: null,
        fee_snapshot_age_s: 9,
        ibkr_quote_age_ms: 3_000,
        hedge_latency_ms: 45,
        hedge_slippage_bps_vs_mid: 1.5,
        maker_leg: {
          venue: 'HYPERLIQUID',
          instrument_id: 'xyz:AAPL-USD-PERP.HYPERLIQUID',
          feed_state: 'ok',
          quote_state: 'fresh',
          pricing_usable: true,
          hedge_usable: true,
          age_ms: 1_000,
        },
        hedge_leg: {
          venue: 'IBKR',
          instrument_id: 'AAPL.BLUEOCEAN',
          route: 'BLUEOCEAN',
          feed_state: 'ok',
          quote_state: 'fresh',
          pricing_usable: true,
          hedge_usable: true,
          age_ms: 2_000,
        },
        ref_leg: {
          venue: 'IBKR',
          instrument_id: 'AAPL.NASDAQ',
          feed_state: 'ok',
          quote_state: 'fresh',
          pricing_usable: true,
          hedge_usable: true,
          age_ms: 3_000,
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
  it('renders a dedicated maker v4 signal table with both venue legs, route, and strategy-published spreads', () => {
    render(
      <MakerV4SignalTable
        rows={[buildMakerV4Strategy()]}
      />,
    );

    expect(screen.getByText('Maker Market')).toBeInTheDocument();
    expect(screen.getByText('Hedge Market')).toBeInTheDocument();
    expect(screen.getByText('Mode')).toBeInTheDocument();
    expect(screen.getByText('Mid Spread')).toBeInTheDocument();
    expect(screen.getByText('Arb Spread')).toBeInTheDocument();
    expect(screen.getByText('aapl_tradexyz_makerv4')).toBeInTheDocument();
    expect(screen.getByText(/Hyperliquid/i)).toBeInTheDocument();
    expect(screen.getByText(/IBKR/i)).toBeInTheDocument();
    expect(screen.getByText('2.0 bps')).toBeInTheDocument();
    expect(screen.getByText('B 14.0')).toBeInTheDocument();
    expect(screen.getByText('A -11.0')).toBeInTheDocument();
    expect(screen.getByText('Take-Take')).toBeInTheDocument();
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

  it('uses worst-leg age and hides actionable spread and hedge states when quote health is stale', () => {
    const strategy = buildMakerV4Strategy();
    strategy.tradeable = false;
    strategy.blocked = true;
    strategy.maker_v4 = {
      ...strategy.maker_v4,
      quote_snapshot: {
        ...strategy.maker_v4?.quote_snapshot,
        hedge_ready: true,
        hedge_disabled_reason: null,
        ref_leg: {
          ...strategy.maker_v4?.quote_snapshot?.ref_leg,
          quote_state: 'old',
          pricing_usable: false,
          hedge_usable: false,
          reason_code: 'reference_quote_old',
          age_ms: 9_000,
        },
      },
    } as any;

    render(
      <MakerV4SignalTable
        rows={[strategy]}
        nowProvider={() => 1_700_000_010_000}
      />,
    );

    expect(screen.getByRole('button', { name: /Age/i })).toBeInTheDocument();
    expect(screen.getByText('9s')).toBeInTheDocument();
    expect(screen.getByText(/Paused/i)).toBeInTheDocument();
    expect(screen.getByText(/Blocked/i)).toBeInTheDocument();
    expect(screen.getByText(/Quote health/i)).toBeInTheDocument();
    expect(screen.getAllByText(/^stale$/i).length).toBeGreaterThanOrEqual(2);
    expect(screen.queryByText('2.0 bps')).not.toBeInTheDocument();
    expect(screen.queryByText('B 14.0')).not.toBeInTheDocument();
    expect(screen.queryByText('A -11.0')).not.toBeInTheDocument();
  });

  it('shows quote health separately from age and surfaces hedge backlog state', () => {
    const strategy = buildMakerV4Strategy();
    strategy.maker_v4 = {
      ...strategy.maker_v4,
      quote_snapshot: {
        ...strategy.maker_v4?.quote_snapshot,
        maker_leg: {
          ...strategy.maker_v4?.quote_snapshot?.maker_leg,
          quote_state: 'old',
          pricing_usable: false,
          hedge_usable: false,
          reason_code: 'maker_quote_old',
        },
      },
    } as any;
    strategy.maker_v4 = {
      ...strategy.maker_v4,
      operator: {
        ...strategy.maker_v4?.operator,
        hedge_backlog: {
          fill_id: 'take_take:order-1',
          side: 'SELL',
          requested_qty: '1',
          blocked_reason: 'stale_quote',
          fill_ts_ms: 1_700_000_000_450,
          maker_fee_bps: 0.25,
        },
      },
    } as any;

    render(
      <MakerV4SignalTable
        rows={[strategy]}
      />,
    );

    expect(screen.getByText('Feed ok · Quote old')).toBeInTheDocument();
    expect(screen.getByText(/Backlog/i)).toBeInTheDocument();
    expect(screen.getByText(/SELL 1/)).toBeInTheDocument();
  });

  it('shows fee assumptions when hovering the strategy id', async () => {
    render(
      <MakerV4SignalTable
        rows={[buildMakerV4Strategy()]}
      />,
    );

    const strategyId = screen.getByText('aapl_tradexyz_makerv4');
    expect(strategyId).toHaveAttribute('title', expect.stringContaining('IBKR fee plan: tiered'));
    expect(strategyId).toHaveAttribute(
      'title',
      expect.stringContaining('Maker taker fee: 4.50 bps'),
    );
    expect(strategyId).toHaveAttribute('title', expect.stringContaining('Assumed hedge fee: 1.00 bps'));
  });

  it('switches the equities signal route to the dedicated maker v4 table while filters stay available', async () => {
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
    expect(screen.getByText('Mid Spread')).toBeInTheDocument();
    expect(screen.getByText('Arb Spread')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Filters'));
    expect(screen.getByPlaceholderText(/Strategy ID/i)).toBeInTheDocument();
    expect(screen.getByTestId('maker-v4-signal-table')).toBeInTheDocument();
    expect(screen.queryByText('FV market')).not.toBeInTheDocument();
  });
});
