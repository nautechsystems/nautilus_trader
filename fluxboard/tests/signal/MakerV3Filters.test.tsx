import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { describe, it, expect, vi, beforeEach } from 'vitest';

import SignalTable from '@/components/domain/signal/SignalTable';
import { useSignalStore, useSuiteStore } from '@/stores';
import * as apiModule from '@/api';
import type { SignalStrategy } from '@/types';

vi.mock('@/api', () => ({
  api: {
    getSignalStrategies: vi.fn(),
  },
}));

vi.mock('@/sockets', () => ({
  socket: {
    on: vi.fn(),
    off: vi.fn(),
    connected: false,
  },
}));

vi.mock('@/components/ui/tooltip', () => ({
  TooltipProvider: ({ children }: any) => children,
  Tooltip: ({ children }: any) => children,
  SimpleTooltip: ({ children }: any) => children,
  IconTooltip: ({ icon }: any) => icon,
}));

vi.mock('@/components/domain/signal/useVisibleNowMs', () => ({
  useVisibleNowMs: () => ({
    nowMs: Date.now(),
    isVisible: true,
    targetRef: () => undefined,
  }),
}));

vi.mock('@/components/shared/FreshnessIndicator', () => ({
  FreshnessIndicator: () => null,
}));

vi.mock('@/stores', async () => {
  const actual = await vi.importActual<any>('@/stores');
  return { ...actual, useSignalStore: vi.fn(), useSuiteStore: vi.fn() };
});

let currentSignalState: any;

function initSignalState(state: any) {
  currentSignalState = {
    rows: [],
    setRows: vi.fn(),
    mergeStrategy: vi.fn(),
    mergeStrategies: vi.fn(),
    ...state,
  };
  (useSignalStore as any).mockImplementation((selector?: any) =>
    selector ? selector(currentSignalState) : currentSignalState,
  );
  (useSignalStore as any).getState = () => currentSignalState;
  const suiteState = { suite: 'all' as const, setSuite: vi.fn() };
  (useSuiteStore as any).mockImplementation((selector?: any) =>
    selector ? selector(suiteState) : suiteState,
  );
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

function makeMakerV3Strategy(
  id: string,
  overrides: Partial<SignalStrategy> = {},
): SignalStrategy {
  return {
    id,
    strategy_family: 'maker_v3',
    params: { bot_on: '1' } as any,
    balances_ok: true,
    meta: {
      class: 'maker_v3_dual_cex',
      strategy_groups: 'tokenmm',
    },
    legs: {
      'binance_spot:PLUMEUSDT': {
        contract_id: 'binance_spot:PLUMEUSDT',
        exchange: 'binance_spot',
        symbol: 'PLUMEUSDT',
        base_asset: 'PLUME',
        product_type: 'spot',
        update_time: '2025-01-15 12:00:00',
      } as any,
      'okx:PLUMEUSDT-PERP': {
        contract_id: 'okx:PLUMEUSDT-PERP',
        exchange: 'okx',
        symbol: 'PLUMEUSDT-PERP',
        base_asset: 'PLUME',
        product_type: 'perp',
        update_time: '2025-01-15 12:00:00',
      } as any,
    },
    legs_order: ['binance_spot:PLUMEUSDT', 'okx:PLUMEUSDT-PERP'],
    maker_role_map: {
      maker_leg: 'okx:PLUMEUSDT-PERP',
      ref_leg: 'binance_spot:PLUMEUSDT',
    } as any,
    maker_v3: {
      quote_snapshot: {
        maker_exchange: 'okx',
        ref_exchange: 'binance_spot',
      },
    } as any,
    ...overrides,
  } as SignalStrategy;
}

describe('Signal MakerV3 filters', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    (apiModule.api.getSignalStrategies as any).mockImplementation(async () => ({
      strategies: currentSignalState?.rows ?? [],
      server_time: '2025-01-15 12:00:02',
      server_ts_ms: Date.now(),
    }));
    initSignalState({ rows: [] });
  });

  it('locks maker-suite routes to MakerV3 rows and hides the generic family selector', async () => {
    const makerStrategy = makeMakerV3Strategy('maker_v3_live');
    const takerStrategy = {
      id: 'taker_live',
      strategy_family: 'taker',
      params: { bot_on: '1' },
      balances_ok: true,
      meta: { class: 'dex_cex_arb', strategy_groups: 'tokenmm' },
      legs: {},
    } as any;

    initSignalState({ rows: [makerStrategy, takerStrategy] });

    renderSignalTable('/tokenmm/signal');

    await screen.findByText('maker_v3_live');
    expect(screen.queryByText('taker_live')).not.toBeInTheDocument();

    fireEvent.click(screen.getByText('Filters'));

    expect(screen.queryByText('Family')).not.toBeInTheDocument();
    expect(screen.getByText('Maker Venue')).toBeInTheDocument();
    expect(screen.getByText('Reference Venue')).toBeInTheDocument();
  });

  it('filters by maker venue using maker_role_map instead of raw leg order', async () => {
    const makerOnSecondLeg = makeMakerV3Strategy('maker_on_second_leg');
    const makerOnFirstLeg = makeMakerV3Strategy('maker_on_first_leg', {
      legs: {
        'bybit_linear:PLUMEUSDT-PERP': {
          contract_id: 'bybit_linear:PLUMEUSDT-PERP',
          exchange: 'bybit_linear',
          symbol: 'PLUMEUSDT-PERP',
          base_asset: 'PLUME',
          product_type: 'perp',
          update_time: '2025-01-15 12:00:00',
        } as any,
        'binance_spot:PLUMEUSDT': {
          contract_id: 'binance_spot:PLUMEUSDT',
          exchange: 'binance_spot',
          symbol: 'PLUMEUSDT',
          base_asset: 'PLUME',
          product_type: 'spot',
          update_time: '2025-01-15 12:00:00',
        } as any,
      },
      legs_order: ['bybit_linear:PLUMEUSDT-PERP', 'binance_spot:PLUMEUSDT'],
      maker_role_map: {
        maker_leg: 'bybit_linear:PLUMEUSDT-PERP',
        ref_leg: 'binance_spot:PLUMEUSDT',
      } as any,
      maker_v3: {
        quote_snapshot: {
          maker_exchange: 'bybit_linear',
          ref_exchange: 'binance_spot',
        },
      } as any,
    });

    initSignalState({ rows: [makerOnSecondLeg, makerOnFirstLeg] });

    renderSignalTable('/tokenmm/signal');

    await screen.findByText('maker_on_second_leg');
    fireEvent.click(screen.getByText('Filters'));

    const makerVenueLabel = screen.getByText('Maker Venue', { selector: 'label' });
    const makerVenueFilter = makerVenueLabel.parentElement?.querySelector('select') as HTMLSelectElement;
    await userEvent.selectOptions(makerVenueFilter, 'okx');

    await waitFor(() => {
      expect(screen.getByText('maker_on_second_leg')).toBeInTheDocument();
      expect(screen.queryByText('maker_on_first_leg')).not.toBeInTheDocument();
    });
  });

  it('builds maker-specific filter options from live rows instead of static lists', async () => {
    const noChainStrategy = makeMakerV3Strategy('maker_okx', {
      meta: {
        class: 'maker_v3_dual_cex',
        strategy_groups: 'tokenmm',
      },
    });
    const dualCexStrategy = makeMakerV3Strategy('maker_hl', {
      meta: {
        class: 'maker_v3_dual_cex',
        strategy_groups: 'tokenmm',
        chain: 'plume',
      },
      legs: {
        'binance_spot:PLUMEUSDT': {
          contract_id: 'binance_spot:PLUMEUSDT',
          exchange: 'binance_spot',
          symbol: 'PLUMEUSDT',
          base_asset: 'PLUME',
          product_type: 'spot',
          update_time: '2025-01-15 12:00:00',
        } as any,
        'hyperliquid:PLUME-PERP': {
          contract_id: 'hyperliquid:PLUME-PERP',
          exchange: 'hyperliquid',
          symbol: 'PLUME-PERP',
          base_asset: 'PLUME',
          product_type: 'perp',
          update_time: '2025-01-15 12:00:00',
        } as any,
      },
      legs_order: ['binance_spot:PLUMEUSDT', 'hyperliquid:PLUME-PERP'],
      maker_role_map: {
        maker_leg: 'hyperliquid:PLUME-PERP',
        ref_leg: 'binance_spot:PLUMEUSDT',
      } as any,
      maker_v3: {
        quote_snapshot: {
          maker_exchange: 'hyperliquid',
          ref_exchange: 'binance_spot',
        },
      } as any,
    });

    initSignalState({ rows: [noChainStrategy, dualCexStrategy] });

    renderSignalTable('/tokenmm/signal');

    await screen.findByText('maker_okx');
    fireEvent.click(screen.getByText('Filters'));

    const makerVenueLabel = screen.getByText('Maker Venue', { selector: 'label' });
    const makerVenueFilter = makerVenueLabel.parentElement?.querySelector('select') as HTMLSelectElement;
    const makerVenueOptions = within(makerVenueFilter).getAllByRole('option').map((option) => option.textContent);
    expect(makerVenueOptions).toEqual(expect.arrayContaining(['okx', 'hyperliquid']));
    expect(makerVenueOptions).not.toEqual(expect.arrayContaining(['rooster_bybit']));

    expect(screen.getByText('Class')).toBeInTheDocument();
    const classLabel = screen.getByText('Class', { selector: 'label' });
    const classFilter = classLabel.parentElement?.querySelector('select') as HTMLSelectElement;
    const classOptions = within(classFilter).getAllByRole('option').map((option) => option.textContent);
    expect(classOptions).toEqual(expect.arrayContaining(['maker_v3_dual_cex']));

    expect(screen.getByText('Chain')).toBeInTheDocument();
  });
});
