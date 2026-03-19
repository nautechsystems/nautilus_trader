import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import Params from '../../Params';
import * as api from '../../api';

const { mockSetActiveProfile, paramsStoreState } = vi.hoisted(() => ({
  mockSetActiveProfile: vi.fn(),
  paramsStoreState: {
    activeProfile: 'maker_v2' as const,
    viewMode: 'compact' as const,
  },
}));

vi.mock('../../api', () => ({
  api: {
    getParamSchema: vi.fn(),
    getParams: vi.fn(),
    patchStrategyParams: vi.fn(),
  },
}));

afterEach(() => {
  cleanup();
});

vi.mock('../../hooks/index', () => ({
  usePolling: vi.fn(),
}));

vi.mock('../../stores', () => {
  const baseStore = {
    auto: false,
    setAuto: vi.fn(),
    get viewMode() {
      return paramsStoreState.viewMode;
    },
    setViewMode: (mode: 'compact' | 'full') => {
      paramsStoreState.viewMode = mode;
    },
    get activeProfile() {
      return paramsStoreState.activeProfile;
    },
    setActiveProfile: (
      profile:
        | 'taker'
        | 'maker_v2'
        | 'maker_v3'
        | 'maker_v4'
        | 'equities_maker'
        | 'equities_taker',
    ) => {
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
    paramsStoreState.viewMode = 'compact';

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

  it('shows split equities family options and routes schema selection through the selected strategy family', async () => {
    (window.location as any).pathname = '/equities/params';
    paramsStoreState.activeProfile = 'equities_maker' as any;
    vi.mocked(api.api.getParamSchema).mockResolvedValue({
      params: {
        hedge_style: {
          key: 'hedge_style',
          label: 'hedge_style',
          description: 'hedge style',
          type: 'select',
          default: 'ioc_through_mid',
          options: [['ioc_through_mid', 'IOC Through Mid']],
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'aapl_tradexyz_maker',
        running: true,
        meta: {
          class: 'equities_maker',
          param_set: 'equities_maker',
          strategy_family: 'equities_maker',
        },
        hot_params: ['hedge_style'],
        params: {
          hedge_style: 'ioc_through_mid',
        },
      },
      {
        strategy_id: 'aapl_tradexyz_taker',
        running: true,
        meta: {
          class: 'equities_taker',
          param_set: 'equities_taker',
          strategy_family: 'equities_taker',
        },
        hot_params: ['bid_edge_take_bps'],
        params: {
          bid_edge_take_bps: '5',
        },
      },
    ] as any);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('aapl_tradexyz_maker')).toBeInTheDocument();
    });

    const familySelect = screen.getByLabelText('Params family');
    expect(within(familySelect).getByRole('option', { name: 'Maker (1)' })).toBeInTheDocument();
    expect(within(familySelect).getByRole('option', { name: 'Taker (1)' })).toBeInTheDocument();
    expect(within(familySelect).queryByRole('option', { name: /Maker V4/i })).not.toBeInTheDocument();
    expect(within(familySelect).queryByRole('option', { name: /Maker V3/i })).not.toBeInTheDocument();

    fireEvent.change(familySelect, { target: { value: 'equities_taker' } });

    expect(mockSetActiveProfile).toHaveBeenCalledWith('equities_taker');
  });

  it('uses a deterministic strategy id when fetching split-family equities schemas', async () => {
    (window.location as any).pathname = '/equities/params';
    paramsStoreState.activeProfile = 'equities_maker' as any;
    vi.mocked(api.api.getParamSchema).mockResolvedValue({
      params: {
        hedge_style: {
          key: 'hedge_style',
          label: 'hedge_style',
          description: 'hedge style',
          type: 'select',
          default: 'ioc_through_mid',
          options: [['ioc_through_mid', 'IOC Through Mid']],
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'msft_tradexyz_maker',
        running: true,
        meta: {
          class: 'equities_maker',
          param_set: 'equities_maker',
          strategy_family: 'equities_maker',
        },
        hot_params: ['hedge_style'],
        params: {
          hedge_style: 'ioc_through_mid',
        },
      },
      {
        strategy_id: 'aapl_tradexyz_maker',
        running: true,
        meta: {
          class: 'equities_maker',
          param_set: 'equities_maker',
          strategy_family: 'equities_maker',
        },
        hot_params: ['hedge_style'],
        params: {
          hedge_style: 'ioc_through_mid',
        },
      },
    ] as any);

    render(<Params />);

    await waitFor(() => {
      expect(api.api.getParamSchema).toHaveBeenCalled();
    });

    expect(vi.mocked(api.api.getParamSchema).mock.calls[0]?.[0]).toEqual({
      preferKeyLabel: true,
      strategyId: 'aapl_tradexyz_maker',
    });
  });

  it('keeps legacy maker_v4 equities rows visible on the shared equities params route during mixed rollout', async () => {
    (window.location as any).pathname = '/equities/params';
    paramsStoreState.activeProfile = 'equities_maker' as any;
    vi.mocked(api.api.getParamSchema).mockResolvedValue({
      params: {
        hedge_style: {
          key: 'hedge_style',
          label: 'hedge_style',
          description: 'hedge style',
          type: 'select',
          default: 'ioc_through_mid',
          options: [['ioc_through_mid', 'IOC Through Mid']],
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'aapl_tradexyz_makerv4',
        running: true,
        meta: {
          class: 'maker_v4',
          param_set: 'makerv4',
          strategy_family: 'maker_v4',
          strategy_groups: 'equities',
          chain: 'equities',
        },
        hot_params: ['hedge_style'],
        params: {
          hedge_style: 'ioc_through_mid',
        },
      },
    ] as any);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('aapl_tradexyz_makerv4')).toBeInTheDocument();
    });

    const familySelect = screen.getByLabelText('Params family');
    expect(within(familySelect).getByRole('option', { name: 'Maker (1)' })).toBeInTheDocument();
    expect(within(familySelect).queryByRole('option', { name: /Maker V4/i })).not.toBeInTheDocument();
    expect(vi.mocked(api.api.getParamSchema).mock.calls[0]?.[0]).toEqual({
      preferKeyLabel: true,
      strategyId: 'aapl_tradexyz_makerv4',
    });
  });

  it('hides blended MakerV4-only params when the shared equities maker surface falls back to a legacy maker_v4 schema', async () => {
    (window.location as any).pathname = '/equities/params';
    paramsStoreState.activeProfile = 'equities_maker' as any;
    vi.mocked(api.api.getParamSchema).mockResolvedValue({
      params: {
        hedge_style: {
          key: 'hedge_style',
          label: 'hedge_style',
          description: 'hedge style',
          type: 'select',
          default: 'ioc_through_mid',
          options: [['ioc_through_mid', 'IOC Through Mid']],
        },
        execution_mode: {
          key: 'execution_mode',
          label: 'execution_mode',
          description: 'legacy execution mode',
          type: 'select',
          default: 'maker_hedge',
          options: [['maker_hedge', 'Maker Hedge']],
        },
        bid_edge_take_bps: {
          key: 'bid_edge_take_bps',
          label: 'bid_edge_take_bps',
          description: 'legacy bid take edge',
          type: 'float',
          default: 5,
        },
        ask_edge_take_bps: {
          key: 'ask_edge_take_bps',
          label: 'ask_edge_take_bps',
          description: 'legacy ask take edge',
          type: 'float',
          default: 5,
        },
        take_cooldown_ms: {
          key: 'take_cooldown_ms',
          label: 'take_cooldown_ms',
          description: 'legacy take cooldown',
          type: 'int',
          default: 250,
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'aapl_tradexyz_makerv4',
        running: true,
        meta: {
          class: 'maker_v4',
          param_set: 'makerv4',
          strategy_family: 'maker_v4',
          strategy_groups: 'equities',
          chain: 'equities',
        },
        hot_params: ['hedge_style', 'execution_mode', 'bid_edge_take_bps', 'ask_edge_take_bps', 'take_cooldown_ms'],
        params: {
          hedge_style: 'ioc_through_mid',
          execution_mode: 'maker_hedge',
          bid_edge_take_bps: '5',
          ask_edge_take_bps: '6',
          take_cooldown_ms: '250',
        },
      },
    ] as any);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('aapl_tradexyz_makerv4')).toBeInTheDocument();
    });

    expect(screen.getByRole('button', { name: 'Sort by hedge_style' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Sort by execution_mode' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Sort by bid_edge_take_bps' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Sort by ask_edge_take_bps' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Sort by take_cooldown_ms' })).not.toBeInTheDocument();
  });

  it('recovers a migrated equities maker selection back to maker_v4 on the default params route when only legacy rows exist', async () => {
    (window.location as any).pathname = '/params';
    paramsStoreState.activeProfile = 'equities_maker' as any;
    vi.mocked(api.api.getParamSchema).mockResolvedValue({
      params: {
        hedge_style: {
          key: 'hedge_style',
          label: 'hedge_style',
          description: 'hedge style',
          type: 'select',
          default: 'ioc_through_mid',
          options: [['ioc_through_mid', 'IOC Through Mid']],
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'aapl_tradexyz_makerv4',
        running: true,
        meta: {
          class: 'maker_v4',
          param_set: 'makerv4',
          strategy_family: 'maker_v4',
          strategy_groups: 'equities',
          chain: 'equities',
        },
        hot_params: ['hedge_style'],
        params: {
          hedge_style: 'ioc_through_mid',
        },
      },
    ] as any);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('aapl_tradexyz_makerv4')).toBeInTheDocument();
    });

    expect(mockSetActiveProfile).toHaveBeenCalledWith('maker_v4');
  });

  it('uses the recovered maker_v4 profile for the first schema request on the default params route when legacy maker rows coexist with other families', async () => {
    (window.location as any).pathname = '/params';
    paramsStoreState.activeProfile = 'equities_maker' as any;
    vi.mocked(api.api.getParamSchema).mockResolvedValue({
      params: {
        hedge_style: {
          key: 'hedge_style',
          label: 'hedge_style',
          description: 'hedge style',
          type: 'select',
          default: 'ioc_through_mid',
          options: [['ioc_through_mid', 'IOC Through Mid']],
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'aapl_tradexyz_makerv4',
        running: true,
        meta: {
          class: 'maker_v4',
          param_set: 'makerv4',
          strategy_family: 'maker_v4',
          strategy_groups: 'equities',
          chain: 'equities',
        },
        hot_params: ['hedge_style'],
        params: {
          hedge_style: 'ioc_through_mid',
        },
      },
      {
        strategy_id: 'msft_tradexyz_taker',
        running: true,
        meta: {
          class: 'dex_cex_arb',
        },
        hot_params: ['qty'],
        params: {
          qty: '2',
        },
      },
    ] as any);

    render(<Params />);

    await waitFor(() => {
      expect(api.api.getParamSchema).toHaveBeenCalled();
    });

    expect(vi.mocked(api.api.getParamSchema).mock.calls[0]?.[0]).toEqual({
      preferKeyLabel: true,
      strategyId: 'aapl_tradexyz_makerv4',
    });
  });

  it('recovers an unavailable equities taker selection on the default params route to the first available legacy family', async () => {
    (window.location as any).pathname = '/params';
    paramsStoreState.activeProfile = 'equities_taker' as any;
    vi.mocked(api.api.getParamSchema).mockResolvedValue({
      params: {
        qty: {
          key: 'qty',
          label: 'qty',
          description: 'size',
          type: 'float',
          default: 1,
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'aapl_tradexyz_makerv4',
        running: true,
        meta: {
          class: 'maker_v4',
          param_set: 'makerv4',
          strategy_family: 'maker_v4',
          strategy_groups: 'equities',
          chain: 'equities',
        },
        hot_params: ['qty'],
        params: {
          qty: '1',
        },
      },
      {
        strategy_id: 'taker_spot_1',
        running: true,
        meta: {
          class: 'dex_cex_arb',
        },
        hot_params: ['qty'],
        params: {
          qty: '2',
        },
      },
    ] as any);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('taker_spot_1')).toBeInTheDocument();
    });

    expect(mockSetActiveProfile).toHaveBeenCalledWith('taker');
    expect(vi.mocked(api.api.getParamSchema).mock.calls[0]?.[0]).toEqual({
      preferKeyLabel: false,
      strategyId: 'taker_spot_1',
    });
  });

  it('uses the route-corrected equities maker profile on the first schema request', async () => {
    (window.location as any).pathname = '/equities/params';
    paramsStoreState.activeProfile = 'taker' as any;
    vi.mocked(api.api.getParamSchema).mockResolvedValue({
      params: {
        hedge_style: {
          key: 'hedge_style',
          label: 'hedge_style',
          description: 'hedge style',
          type: 'select',
          default: 'ioc_through_mid',
          options: [['ioc_through_mid', 'IOC Through Mid']],
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'msft_tradexyz_taker',
        running: true,
        meta: {
          class: 'equities_taker',
          param_set: 'equities_taker',
          strategy_family: 'equities_taker',
        },
        hot_params: ['bid_edge_take_bps'],
        params: {
          bid_edge_take_bps: '6',
        },
      },
      {
        strategy_id: 'aapl_tradexyz_maker',
        running: true,
        meta: {
          class: 'equities_maker',
          param_set: 'equities_maker',
          strategy_family: 'equities_maker',
        },
        hot_params: ['hedge_style'],
        params: {
          hedge_style: 'ioc_through_mid',
        },
      },
    ] as any);

    render(<Params />);

    await waitFor(() => {
      expect(api.api.getParamSchema).toHaveBeenCalled();
    });

    expect(vi.mocked(api.api.getParamSchema).mock.calls[0]?.[0]).toEqual({
      preferKeyLabel: true,
      strategyId: 'aapl_tradexyz_maker',
    });
  });

  it('prefers a split equities maker row over a legacy maker_v4 row when choosing the shared equities maker schema', async () => {
    (window.location as any).pathname = '/equities/params';
    paramsStoreState.activeProfile = 'equities_maker' as any;
    vi.mocked(api.api.getParamSchema).mockResolvedValue({
      params: {
        hedge_style: {
          key: 'hedge_style',
          label: 'hedge_style',
          description: 'hedge style',
          type: 'select',
          default: 'ioc_through_mid',
          options: [['ioc_through_mid', 'IOC Through Mid']],
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'aapl_tradexyz_makerv4',
        running: true,
        meta: {
          class: 'maker_v4',
          param_set: 'makerv4',
          strategy_family: 'maker_v4',
          strategy_groups: 'equities',
          chain: 'equities',
        },
        hot_params: ['hedge_style'],
        params: {
          hedge_style: 'ioc_through_mid',
        },
      },
      {
        strategy_id: 'msft_tradexyz_maker',
        running: true,
        meta: {
          class: 'equities_maker',
          param_set: 'equities_maker',
          strategy_family: 'equities_maker',
        },
        hot_params: ['hedge_style'],
        params: {
          hedge_style: 'ioc_through_mid',
        },
      },
    ] as any);

    render(<Params />);

    await waitFor(() => {
      expect(api.api.getParamSchema).toHaveBeenCalled();
    });

    expect(vi.mocked(api.api.getParamSchema).mock.calls[0]?.[0]).toEqual({
      preferKeyLabel: true,
      strategyId: 'msft_tradexyz_maker',
    });
  });

  it('renders MakerV3 short headers for tokenmm params when rows are MakerV3-only', async () => {
    (window.location as any).pathname = '/tokenmm/params';
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
        strategy_id: 'aapl_tradexyz_makerv4',
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
      expect(screen.getByText('aapl_tradexyz_makerv4')).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(api.api.getParamSchema).toHaveBeenCalledWith({
        preferKeyLabel: true,
        strategyId: 'aapl_tradexyz_makerv4',
      });
    });
    expect(screen.getByRole('button', { name: 'Sort by bid_edge1' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sort by ask_edge1' })).toBeInTheDocument();
  });

  it('hides advanced bounded-convergence params by default and shows them in Advanced Params mode', async () => {
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
        qty: {
          key: 'qty',
          label: 'qty',
          description: 'size',
          type: 'float',
          default: 1,
        },
        max_cancels_per_side_per_cycle: {
          key: 'max_cancels_per_side_per_cycle',
          label: 'max_cancels_per_side_per_cycle',
          description: 'cancel budget',
          type: 'int',
          default: 1,
          advanced: true,
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'plumeusdt_bybit_perp_makerv3',
        running: true,
        meta: { class: 'maker_v3' },
        hot_params: ['bot_on', 'qty'],
        params: {
          bot_on: '1',
          qty: '1000',
          max_cancels_per_side_per_cycle: '1',
        },
      },
    ] as any);

    const { rerender } = render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('plumeusdt_bybit_perp_makerv3')).toBeInTheDocument();
    });

    expect(screen.getByRole('button', { name: 'Sort by qty' })).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Sort by max_cancels_per_side_per_cycle' }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Advanced Params/i }));
    rerender(<Params />);

    await waitFor(() => {
      expect(
        screen.getByRole('button', { name: 'Sort by max_cancels_per_side_per_cycle' }),
      ).toBeInTheDocument();
    });
  });

});
