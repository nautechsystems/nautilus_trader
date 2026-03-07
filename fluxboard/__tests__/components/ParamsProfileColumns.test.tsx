import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import Params from '../../Params';
import * as api from '../../api';

const { mockSetActiveProfile, paramsStoreState } = vi.hoisted(() => ({
  mockSetActiveProfile: vi.fn(),
  paramsStoreState: {
    activeProfile: 'maker_v2' as const,
  },
}));

vi.mock('../../api', () => ({
  api: {
    getParamSchema: vi.fn(),
    getParams: vi.fn(),
    patchStrategyParams: vi.fn(),
  },
}));

vi.mock('../../hooks/index', () => ({
  usePolling: vi.fn(),
}));

vi.mock('../../stores', () => {
  const baseStore = {
    auto: false,
    setAuto: vi.fn(),
    viewMode: 'compact' as const,
    setViewMode: vi.fn(),
    get activeProfile() {
      return paramsStoreState.activeProfile;
    },
    setActiveProfile: (profile: 'taker' | 'maker_v2' | 'maker_v3') => {
      paramsStoreState.activeProfile = profile;
      mockSetActiveProfile(profile);
    },
    columnPrefs: { order: [] as string[], visibility: {} as Record<string, boolean> },
    setColumnOrder: vi.fn(),
    setColumnVisibility: vi.fn(),
    resetColumnVisibility: vi.fn(),
    sortState: { key: null as string | null, direction: null as 'asc' | 'desc' | null },
    setSortState: vi.fn(),
    clearSort: vi.fn(),
    selectedStrategies: [] as string[],
    setSelectedStrategies: vi.fn(),
    clearSelection: vi.fn(),
    lastFocusedCell: null as { strategyId: string; paramKey: string } | null,
    setLastFocusedCell: vi.fn(),
    lastUpdate: Date.now(),
    setLastUpdate: vi.fn(),
  };
  const mockedHook = vi.fn((selector?: any) => {
    if (typeof selector === 'function') {
      return selector(baseStore);
    }
    return baseStore;
  });
  return { useParamsStore: mockedHook };
});

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}));

describe('Params profile column filtering', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSetActiveProfile.mockReset();
    paramsStoreState.activeProfile = 'maker_v2';

    vi.mocked(api.api.getParamSchema).mockResolvedValue({
      params: {
        bot_on: {
          key: 'bot_on',
          label: 'bot_on',
          description: 'toggle',
          type: 'select',
          default: '0',
          options: [['0', 'Off'], ['1', 'On']],
        },
        qty: {
          key: 'qty',
          label: 'qty',
          description: 'size',
          type: 'float',
          default: 1,
        },
        cex_bid_edge: {
          key: 'cex_bid_edge',
          label: 'bid_edge',
          description: 'bid edge',
          type: 'float',
          default: 10,
          applies_to: ['TakerArbitrageTask'],
        },
        cex_ask_edge: {
          key: 'cex_ask_edge',
          label: 'ask_edge',
          description: 'ask edge',
          type: 'float',
          default: 10,
          applies_to: ['TakerArbitrageTask'],
        },
        slippage_bps: {
          key: 'slippage_bps',
          label: 'slippage',
          description: 'slippage',
          type: 'float',
          default: 10,
          applies_to: ['TakerArbitrageTask'],
        },
      },
      deprecated: {},
    } as any);

    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'maker_equity_1',
        running: true,
        meta: { class: 'equity_perp_maker' },
        hot_params: ['bot_on', 'qty', 'cex_bid_edge', 'cex_ask_edge'],
        params: {
          bot_on: '1',
          qty: '5',
          cex_bid_edge: '25',
          cex_ask_edge: '25',
        },
      },
      {
        strategy_id: 'taker_spot_1',
        running: true,
        meta: { class: 'dex_cex_arb' },
        hot_params: ['bot_on', 'qty', 'cex_bid_edge', 'cex_ask_edge'],
        params: {
          bot_on: '1',
          qty: '2',
          cex_bid_edge: '11',
          cex_ask_edge: '12',
        },
      },
    ] as any);
  });

  it('keeps cex edge columns visible in maker_v2 profile even when schema applies_to is taker', async () => {
    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('maker_equity_1')).toBeInTheDocument();
    });
    expect(screen.queryByText('taker_spot_1')).not.toBeInTheDocument();

    expect(screen.getByRole('button', { name: 'Sort by bid_edge' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sort by ask_edge' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Sort by slippage' })).not.toBeInTheDocument();
  });

  it('uses Family dropdown to change active profile', async () => {
    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('maker_equity_1')).toBeInTheDocument();
    });

    expect(screen.getByRole('option', { name: 'Maker V2 (1)' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Taker (1)' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Maker V3 (0)' })).toBeInTheDocument();

    const familySelect = screen.getByLabelText('Params family');
    fireEvent.change(familySelect, { target: { value: 'maker_v3' } });

    expect(mockSetActiveProfile).toHaveBeenCalledWith('maker_v3');
  });

  it('renders MakerV3 short headers for equities params when rows are MakerV3-only', async () => {
    window.history.pushState({}, '', '/equities/params');
    paramsStoreState.activeProfile = 'maker_v3';
    vi.mocked(api.api.getParamSchema).mockResolvedValue({
      params: {
        bot_on: {
          key: 'bot_on',
          label: 'bot_on',
          description: 'toggle',
          type: 'select',
          default: '0',
          options: [['0', 'Off'], ['1', 'On']],
        },
        bid_edge1: {
          key: 'bid_edge1',
          label: 'bid_edge1',
          description: 'bid edge',
          type: 'float',
          default: 10,
        },
        ask_edge1: {
          key: 'ask_edge1',
          label: 'ask_edge1',
          description: 'ask edge',
          type: 'float',
          default: 10,
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'aapl_tradexyz_makerv3',
        running: true,
        meta: { class: 'maker_v3' },
        hot_params: ['bot_on', 'bid_edge1', 'ask_edge1'],
        params: {
          bot_on: '1',
          bid_edge1: '25',
          ask_edge1: '25',
        },
      },
    ] as any);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('aapl_tradexyz_makerv3')).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(api.api.getParamSchema).toHaveBeenCalledWith({ preferKeyLabel: true });
    });
    expect(screen.getByRole('button', { name: 'Sort by bid_edge1' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sort by ask_edge1' })).toBeInTheDocument();
  });

});
