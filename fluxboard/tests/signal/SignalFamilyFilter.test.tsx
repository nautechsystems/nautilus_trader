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
    id: 'aapl_tradexyz_maker',
    strategy_family: 'equities_maker',
    running: true,
    tradeable: false,
    blocked: true,
    balances_ok: true,
    params: { bot_on: '0', qty: '1' },
    meta: {
      chain: 'equities',
      strategy_groups: 'equities',
      strategy_family: 'equities_maker',
      strategy_version: 'v4',
      class: 'equities_maker',
      param_set: 'equities_maker',
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
    equities_arb: {
      operator: {
        execution_mode: 'maker_hedge',
        behavior: 'maker',
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

function buildTakerStrategy(): SignalStrategy {
  return {
    ...buildMakerV4Strategy(),
    id: 'aapl_tradexyz_taker',
    strategy_family: 'equities_taker',
    meta: {
      ...buildMakerV4Strategy().meta,
      strategy_family: 'equities_taker',
      class: 'equities_taker',
      param_set: 'equities_taker',
    },
    equities_arb: {
      ...buildMakerV4Strategy().equities_arb,
      operator: {
        ...buildMakerV4Strategy().equities_arb?.operator,
        execution_mode: 'take_take',
        behavior: 'take_take',
      },
    },
  };
}

function buildLegacyMakerV4EquitiesStrategy(): SignalStrategy {
  const legacy = buildMakerV4Strategy();
  return {
    ...legacy,
    id: 'aapl_tradexyz_makerv4',
    strategy_family: 'maker_v4',
    meta: {
      ...legacy.meta,
      strategy_family: 'maker_v4',
      strategy_version: 'v4',
      class: 'maker_v4',
      param_set: 'makerv4',
    },
    maker_v4: legacy.equities_arb as any,
    equities_arb: undefined,
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
    const strategies = [buildMakerV4Strategy(), buildTakerStrategy()];
    (api.getSignalStrategies as any).mockResolvedValue({
      strategies,
      server_time: '2024-01-01 12:00:00',
      server_ts_ms: 1_700_000_001_500,
    });
    initSignalState({
      rows: strategies,
      setRows: vi.fn(),
      mergeStrategy: vi.fn(),
      mergeStrategies: vi.fn(),
    });
  });

  it('shows an equities maker/taker family control on equities signal while keeping the strategy filter', async () => {
    renderSignalTable('/equities/signal');

    await screen.findByTestId('equities-arb-signal-table');

    const familyFilter = screen.getByLabelText('Signal family');
    expect(familyFilter).toBeInTheDocument();
    expect(familyFilter).toHaveValue('all');
    expect(screen.getByRole('option', { name: 'Maker (1)' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Taker (1)' })).toBeInTheDocument();

    fireEvent.click(screen.getByText('Filters'));
    expect(screen.getByPlaceholderText(/Strategy ID/i)).toBeInTheDocument();

    fireEvent.change(familyFilter, { target: { value: 'equities_taker' } });
    expect(screen.getByText('aapl_tradexyz_taker')).toBeInTheDocument();
    expect(screen.queryByText('aapl_tradexyz_maker')).not.toBeInTheDocument();
  });

  it('ignores legacy maker_v4 equities rows on the shared family filter after the split cleanup', async () => {
    const strategies = [buildLegacyMakerV4EquitiesStrategy(), buildTakerStrategy()];
    (api.getSignalStrategies as any).mockResolvedValue({
      strategies,
      server_time: '2024-01-01 12:00:00',
      server_ts_ms: 1_700_000_001_500,
    });
    initSignalState({
      rows: strategies,
      setRows: vi.fn(),
      mergeStrategy: vi.fn(),
      mergeStrategies: vi.fn(),
    });

    renderSignalTable('/equities/signal');

    await screen.findByTestId('equities-arb-signal-table');

    const familyFilter = screen.getByLabelText('Signal family');
    expect(screen.getByRole('option', { name: 'Maker (0)' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Taker (1)' })).toBeInTheDocument();
    expect(screen.queryByText('aapl_tradexyz_makerv4')).not.toBeInTheDocument();

    fireEvent.change(familyFilter, { target: { value: 'equities_maker' } });
    expect(screen.queryByText('aapl_tradexyz_makerv4')).not.toBeInTheDocument();
    expect(screen.queryByText('aapl_tradexyz_taker')).not.toBeInTheDocument();
  });

  it('labels maker_v4 as a legacy family on the default signal route', async () => {
    const strategies = [buildLegacyMakerV4EquitiesStrategy()];
    (api.getSignalStrategies as any).mockResolvedValue({
      strategies,
      server_time: '2024-01-01 12:00:00',
      server_ts_ms: 1_700_000_001_500,
    });
    initSignalState({
      rows: strategies,
      setRows: vi.fn(),
      mergeStrategy: vi.fn(),
      mergeStrategies: vi.fn(),
    });

    renderSignalTable('/signal');

    await screen.findByText('aapl_tradexyz_makerv4');

    expect(
      screen.getByRole('option', { name: 'Maker V4 (legacy) (1)' }),
    ).toBeInTheDocument();
  });
});
