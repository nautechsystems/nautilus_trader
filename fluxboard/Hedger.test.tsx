import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import Hedger from './Hedger';
import type { HedgerSnapshot, HedgerStatus } from './types';
import { INTERVALS } from './constants';

const mockUsePolling = vi.fn(
  (fn: () => unknown | Promise<unknown>, _interval: number, enabled?: boolean) => {
    if (!enabled) return;
    // Simulate usePolling's "initial fetch" behavior without causing
    // state updates during render (which would trigger React re-render loops).
    setTimeout(() => {
      void fn();
    }, 0);
  }
);

const mockApi = vi.hoisted(() => ({
  listHedgerInstances: vi.fn(),
  getHedgerStatusById: vi.fn(),
  setHedgerJobStateById: vi.fn(),
  getHedgerConfig: vi.fn(),
  patchHedgerConfig: vi.fn(),
  setHedgerGeometryOverridesById: vi.fn(),
  clearHedgerGeometryOverridesById: vi.fn(),
  setHedgerThresholdOverridesById: vi.fn(),
  clearHedgerThresholdOverridesById: vi.fn(),
  setHedgerEnabledById: vi.fn(),
  clearHedgerEventsById: vi.fn(),
  getEthPlumeHedgerStatus: vi.fn(),
  setEthPlumeHedgerJobState: vi.fn(),
  setHedgerGeometryOverrides: vi.fn(),
  clearHedgerGeometryOverrides: vi.fn(),
  setHedgerThresholdOverrides: vi.fn(),
  clearHedgerThresholdOverrides: vi.fn(),
  setHedgerEnabled: vi.fn(),
  clearHedgerEvents: vi.fn(),
  setHedgerBand2GeometryOverrides: vi.fn(),
  clearHedgerBand2GeometryOverrides: vi.fn(),
  setHedgerBand2ThresholdOverrides: vi.fn(),
  clearHedgerBand2ThresholdOverrides: vi.fn(),
  setHedgerBand2Enabled: vi.fn(),
  clearHedgerBand2Events: vi.fn(),
}));

vi.mock('./hooks', () => ({
  usePolling: (fn: () => unknown | Promise<unknown>, interval: number, immediate?: boolean) =>
    mockUsePolling(fn, interval, immediate),
}));

vi.mock('./api', () => ({
  api: mockApi,
}));

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

const buildSnapshot = (overrides: Partial<HedgerSnapshot> = {}): HedgerSnapshot => ({
  timestamp: 1,
  price_plume_per_eth: '0.05',
  price_move_pct: '0',
  price_source: 'oracle',
  lp_eth: '0',
  lp_plume: '0',
  perp_eth: '0',
  perp_plume: '0',
  net_eth: '0',
  net_plume: '0',
  target_net_eth: '0',
  target_net_plume: '0',
  eth_error: '0',
  plume_error: '0',
  eth_mark: '0',
  plume_mark: '0',
  eth_usd_error: '0',
  plume_usd_error: '0',
  lp_eth_usd: '0',
  lp_plume_usd: '0',
  perp_eth_usd: '0',
  perp_plume_usd: '0',
  net_eth_usd: '0',
  net_plume_usd: '0',
  total_lp_value_usd: '0',
  total_perp_notional_usd: '0',
  net_delta_value_usd: '0',
  lp_mix_eth_pct: '0',
  lp_mix_plume_pct: '0',
  range_pct: '0',
  near_lower_bound: false,
  near_upper_bound: false,
  last_hedge_price: '0.05',
  last_net_eth: '0',
  last_net_plume: '0',
  initial_eth_base: '0',
  initial_plume_base: '0',
  price_lower_base: '0',
  price_upper_base: '0',
  initial_eth_effective: '0',
  initial_plume_effective: '0',
  price_lower_effective: '0',
  price_upper_effective: '0',
  ...overrides,
});

const buildStatus = (overrides: Partial<HedgerStatus> = {}): HedgerStatus => {
  const {
    snapshot: snapshotOverride,
    config_summary: configSummaryOverride,
    threshold_effective: thresholdEffectiveOverride,
    threshold_overrides: thresholdOverridesOverride,
    geometry_effective: geometryEffectiveOverride,
    ...rest
  } = overrides;

  const snapshot = buildSnapshot(snapshotOverride);
  const baseThresholds = {
    eth_exposure_usd_threshold: '1000',
    plume_exposure_usd_threshold: '1200',
    price_move_pct: '5',
  };
  const thresholdEffective = { ...baseThresholds, ...thresholdEffectiveOverride };
  const thresholdOverrides = { ...baseThresholds, ...thresholdOverridesOverride };
  const geometryEffective = {
    initial_eth: '1',
    initial_plume: '100',
    price_lower: '0.04',
    price_upper: '0.07',
    ...geometryEffectiveOverride,
  };

  return {
    id: 'eth_plume_lp',
    job_id: 'job-1',
    job_status: 'running',
    last_tick_ts: 2,
    last_hedge_ts: 2,
    last_hedge_price: snapshot.last_hedge_price,
    last_net_eth: '0',
    last_net_plume: '0',
    snapshot,
    recent_events: [],
    config_summary: {
      price_move_pct: '5',
      eth_exposure_usd_threshold: '1000',
      plume_exposure_usd_threshold: '1200',
      ...configSummaryOverride,
    },
    geometry_overrides: null,
    geometry_effective: geometryEffective,
    threshold_overrides: thresholdOverrides,
    threshold_effective: thresholdEffective,
    hedger_enabled: true,
    dry_run: false,
    ...rest,
  };
};

describe('Hedger layout', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.listHedgerInstances.mockResolvedValue([
      { id: 'eth_plume_lp', label: 'ETH/PLUME LP Hedger' },
      { id: 'eth_plume_lp_band2', label: 'ETH/PLUME LP Band2' },
      { id: 'hype_usdt_lp', label: 'HYPE/USDT LP Hedger' },
      { id: 'plume_weth_lp', label: 'PLUME/WETH LP Hedger' },
      { id: 'third_lp', label: 'Third LP Hedger' },
    ]);
    mockApi.getHedgerStatusById.mockResolvedValue(buildStatus());
  });

  it('keeps recent hedges aligned with the main page padding', async () => {
    render(<Hedger />);

    const recentHedgesHeading = await screen.findByText(/Recent Hedges/i);
    const paddedContainer = recentHedgesHeading.closest('[class*="px-4"]');
    expect(paddedContainer).not.toBeNull();

    const pageTitle = screen.getByRole('heading', { level: 1, name: 'ETH/PLUME LP Hedger' });
    expect(paddedContainer?.contains(pageTitle)).toBe(true);
  });

  it('uses the config label when provided', async () => {
    const customLabel = 'Custom LP Hedger Label';
    mockApi.getHedgerStatusById.mockResolvedValue(
      buildStatus({
        config_summary: {
          price_move_pct: '5',
          eth_exposure_usd_threshold: '1000',
          plume_exposure_usd_threshold: '1200',
          label: customLabel,
        },
      })
    );

    render(<Hedger />);

    const pageTitle = await screen.findByText(customLabel);
    expect(pageTitle).toBeInTheDocument();
  });

  it('renders hedger selector with all instances', async () => {
    render(<Hedger />);
    const selector = await screen.findByLabelText(/Hedger/i);
    const options = (selector as HTMLSelectElement).querySelectorAll('option');
    expect(options.length).toBeGreaterThanOrEqual(5);
    expect(Array.from(options).map(o => o.value)).toEqual(
      expect.arrayContaining(['eth_plume_lp', 'eth_plume_lp_band2', 'hype_usdt_lp', 'plume_weth_lp', 'third_lp'])
    );
  });

  it('calls getHedgerStatusById when a different hedger is selected', async () => {
    const user = userEvent.setup();
    render(<Hedger />);
    const selector = await screen.findByLabelText(/Hedger/i);
    await user.selectOptions(selector, 'hype_usdt_lp');

    await waitFor(() => {
      expect(mockApi.getHedgerStatusById).toHaveBeenCalledWith('hype_usdt_lp');
    });
  });

  it('enables config edit for both eth and non-eth hedgers', async () => {
    const user = userEvent.setup();
    render(<Hedger />);
    const editButton = await screen.findByRole('button', { name: /Edit Config/i });
    expect(editButton).not.toBeDisabled();
    const selector = await screen.findByLabelText(/Hedger/i);
    await user.selectOptions(selector, 'hype_usdt_lp');
    expect(editButton).not.toBeDisabled();
  });

  it('documents the intentional monorepo lp deltas from Chainsaw', () => {
    const contract = readFileSync(resolve(process.cwd(), 'docs/lp_contract.md'), 'utf-8');

    expect(contract).toContain('non-ETH hedgers use the same generic by-ID operator controls');
    expect(contract).toContain('Edit Config remains available for ETH/PLUME Band1 and Band2 on `/lp`');
  });

  it('allows disabling hedging for token1 in the config drawer', async () => {
    const user = userEvent.setup();
    const configResponse = {
      id: 'hype_usdt_lp',
      label: 'HYPE/USDT LP Hedger',
      lp_pool: {
        token0_symbol: 'HYPE',
        token1_symbol: 'USDT',
        initial_token0: '1',
        initial_token1: '1',
        price_lower: '0.1',
        price_upper: '10',
      },
      target: {
        target_net_token0: '0',
        target_net_token1: '0',
      },
      hedge: {
        hedge_token0: true,
        hedge_token1: true,
      },
      bybit: {
        perp_symbol_token0: 'HYPEUSDT',
        perp_symbol_token1: 'USDTUSDT',
      },
    };
    mockApi.getHedgerConfig.mockResolvedValue(configResponse);
    mockApi.patchHedgerConfig.mockResolvedValue(configResponse);

    render(<Hedger />);
    const selector = await screen.findByLabelText(/Hedger/i);
    await user.selectOptions(selector, 'hype_usdt_lp');

    await user.click(await screen.findByRole('button', { name: /Edit Config/i }));
    const token1Checkbox = screen.getByLabelText(/Hedge Token1/i);
    expect(token1Checkbox).toBeChecked();
    await user.click(token1Checkbox);

    await user.click(screen.getByRole('button', { name: /Save & Restart/i }));

    await waitFor(() => {
      expect(mockApi.patchHedgerConfig).toHaveBeenCalledWith('hype_usdt_lp', {
        label: 'HYPE/USDT LP Hedger',
        lp_pool: {
          token0_symbol: 'HYPE',
          token1_symbol: 'USDT',
          initial_token0: '1',
          initial_token1: '1',
          price_lower: '0.1',
          price_upper: '10',
        },
        target: {
          target_net_token0: '0',
          target_net_token1: '0',
        },
        hedge: { hedge_token0: true, hedge_token1: false },
        bybit: {
          perp_symbol_token0: 'HYPEUSDT',
          perp_symbol_token1: 'USDTUSDT',
        },
      });
    });
  }, 10000);

  it('omits pool address and decimals while preserving target values when saving config edits', async () => {
    const user = userEvent.setup();
    const cfg = {
      id: 'hype_usdt_lp',
      label: 'HYPE/USDT LP Hedger',
      lp_pool: {
        token0_symbol: 'HYPE',
        token1_symbol: 'USDT',
        token0_decimals: 18,
        token1_decimals: 6,
      },
      target: {
        target_net_token0: '1.5',
        target_net_token1: '250',
      },
      hedge: { hedge_token0: true, hedge_token1: true },
    };
    mockApi.getHedgerConfig.mockResolvedValue(cfg as any);
    mockApi.patchHedgerConfig.mockResolvedValue(cfg as any);

    render(<Hedger />);
    const selector = await screen.findByLabelText(/Hedger/i);
    await user.selectOptions(selector, 'hype_usdt_lp');
    await user.click(await screen.findByRole('button', { name: /Edit Config/i }));
    await user.click(screen.getByRole('button', { name: /Save & Restart/i }));

    await waitFor(() => {
      const [, payload] = mockApi.patchHedgerConfig.mock.calls.at(-1)!;
      const lpPool = (payload as any).lp_pool ?? {};
      expect(lpPool.pool_address).toBeUndefined();
      expect(lpPool.mode).toBeUndefined();
      expect(lpPool.token0_decimals).toBeUndefined();
      expect(lpPool.token1_decimals).toBeUndefined();
      expect((payload as any).target).toEqual({
        target_net_token0: '1.5',
        target_net_token1: '250',
      });
    });
  });

  it('uses the same generic config payload path for eth hedgers', async () => {
    const user = userEvent.setup();
    const cfg = {
      id: 'eth_plume_lp',
      label: 'ETH/PLUME LP Hedger',
      lp_pool: {
        token0_symbol: 'WETH',
        token1_symbol: 'WPLUME',
        initial_token0: '1.6085',
        initial_token1: '169377',
        price_lower: '85000',
        price_upper: '111000',
      },
      target: {
        target_net_token0: '0.25',
        target_net_token1: '500',
      },
      hedge: { hedge_token0: true, hedge_token1: true },
      bybit: {
        perp_symbol_token0: 'ETHUSDT',
        perp_symbol_token1: 'PLUMEUSDT',
      },
    };
    mockApi.getHedgerConfig.mockResolvedValue(cfg as any);
    mockApi.patchHedgerConfig.mockResolvedValue(cfg as any);

    render(<Hedger />);

    const editButton = await screen.findByRole('button', { name: /Edit Config/i });
    expect(editButton).not.toBeDisabled();
    await user.click(editButton);

    const target0 = await screen.findByLabelText(/Target Net Token0/i);
    const target1 = screen.getByLabelText(/Target Net Token1/i);
    const perp0 = screen.getByLabelText(/Perp Symbol Token0/i);
    const perp1 = screen.getByLabelText(/Perp Symbol Token1/i);

    expect(target0).not.toHaveAttribute('readonly');
    expect(target1).not.toHaveAttribute('readonly');
    expect(perp0).toHaveValue('ETHUSDT');
    expect(perp1).toHaveValue('PLUMEUSDT');
    expect(screen.queryByText(/Derived from initial LP/i)).toBeNull();

    await user.clear(target0);
    await user.type(target0, '0.5');
    await user.clear(target1);
    await user.type(target1, '750');
    await user.click(screen.getByRole('button', { name: /Save & Restart/i }));

    await waitFor(() => {
      expect(mockApi.patchHedgerConfig).toHaveBeenCalledWith('eth_plume_lp', {
        label: 'ETH/PLUME LP Hedger',
        lp_pool: {
          token0_symbol: 'WETH',
          token1_symbol: 'WPLUME',
          initial_token0: '1.6085',
          initial_token1: '169377',
          price_lower: '85000',
          price_upper: '111000',
        },
        target: {
          target_net_token0: '0.5',
          target_net_token1: '750',
        },
        hedge: { hedge_token0: true, hedge_token1: true },
        bybit: {
          perp_symbol_token0: 'ETHUSDT',
          perp_symbol_token1: 'PLUMEUSDT',
        },
      });
    });
  }, 10000);

  it('shows perp inputs for non-ETH hedgers and sends bybit payload on save', async () => {
    const user = userEvent.setup();
    const cfg = {
      id: 'hype_usdt_lp',
      label: 'PLUME/USDT LP Hedger',
      lp_pool: {
        token0_symbol: 'PLUME',
        token1_symbol: 'USDT',
        initial_token0: '1',
        initial_token1: '1',
        price_lower: '0.1',
        price_upper: '10',
      },
      target: { target_net_token0: '0', target_net_token1: '0' },
      hedge: { hedge_token0: true, hedge_token1: false },
      bybit: { perp_symbol_token0: 'PLUMEUSDT', perp_symbol_token1: '' },
    };
    mockApi.getHedgerConfig.mockResolvedValue(cfg as any);
    mockApi.patchHedgerConfig.mockResolvedValue(cfg as any);
    mockApi.getHedgerStatusById.mockResolvedValue(
      buildStatus({
        id: 'hype_usdt_lp',
        config_summary: {
          token0_symbol: 'PLUME',
          token1_symbol: 'USDT',
          hedge_token0: true,
          hedge_token1: false,
          perp_symbol_token0: 'PLUMEUSDT',
          perp_symbol_token1: '',
        },
      })
    );

    render(<Hedger />);
    const selector = await screen.findByLabelText(/Hedger/i);
    await user.selectOptions(selector, 'hype_usdt_lp');

    await user.click(await screen.findByRole('button', { name: /Edit Config/i }));

    const perp0 = await screen.findByLabelText(/Perp Symbol Token0/i);
    expect(perp0).toHaveValue('PLUMEUSDT');
    const perp1 = screen.getByLabelText(/Perp Symbol Token1/i);
    expect(perp1).toHaveValue('');
    expect(screen.getByLabelText(/Target Net Token0/i)).toHaveValue('0');
    expect(screen.getByLabelText(/Target Net Token1/i)).toHaveValue('0');
    expect(screen.queryByText(/Derived from initial LP/i)).toBeNull();

    await user.clear(perp0);
    await user.type(perp0, 'PLUMEUSDT');
    await user.click(screen.getByRole('button', { name: /Save & Restart/i }));

    await waitFor(() => {
      expect(mockApi.patchHedgerConfig).toHaveBeenCalledWith(
        'hype_usdt_lp',
        expect.objectContaining({
          target: { target_net_token0: '0', target_net_token1: '0' },
          bybit: { perp_symbol_token0: 'PLUMEUSDT', perp_symbol_token1: '' },
        })
      );
    });
  }, 15000);
});

describe('Hedger thresholds UI', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.listHedgerInstances.mockResolvedValue([
      { id: 'eth_plume_lp', label: 'ETH/PLUME LP Hedger' },
      { id: 'eth_plume_lp_band2', label: 'ETH/PLUME LP Band2' },
      { id: 'hype_usdt_lp', label: 'HYPE/USDT LP Hedger' },
      { id: 'plume_weth_lp', label: 'PLUME/WETH LP Hedger' },
      { id: 'third_lp', label: 'Third LP Hedger' },
    ]);
    mockApi.getHedgerStatusById.mockResolvedValue(buildStatus());
    mockApi.getEthPlumeHedgerStatus.mockResolvedValue(buildStatus());
  });

  it('polls the hedger API using the critical interval token', async () => {
    render(<Hedger />);

    await waitFor(() => {
      expect(mockUsePolling).toHaveBeenCalledWith(expect.any(Function), INTERVALS.HEDGER_POLL, true);
    });
  });

  it('treats USD thresholds as legacy and only allows editing price move %', async () => {
    const user = userEvent.setup();
    render(<Hedger />);

    await waitFor(() => {
      expect(mockApi.getHedgerStatusById).toHaveBeenCalled();
    });

    expect(screen.queryByText(/Not used/i)).toBeNull();
    expect(screen.getByText(/Price Move Threshold/i)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /edit thresholds/i }));

    expect(screen.getByLabelText(/Price Move %/i)).toBeInTheDocument();
    expect(screen.queryByLabelText(/ETH Threshold/i)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/PLUME Threshold/i)).not.toBeInTheDocument();
  });
});

describe('Hedger pricing card', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.listHedgerInstances.mockResolvedValue([
      { id: 'eth_plume_lp', label: 'ETH/PLUME LP Hedger' },
      { id: 'eth_plume_lp_band2', label: 'ETH/PLUME LP Band2' },
      { id: 'hype_usdt_lp', label: 'HYPE/USDT LP Hedger' },
      { id: 'plume_weth_lp', label: 'PLUME/WETH LP Hedger' },
      { id: 'third_lp', label: 'Third LP Hedger' },
    ]);
    mockApi.getHedgerStatusById.mockResolvedValue(buildStatus());
  });

  it('derives price move percentage from last hedge price and surfaces last hedge price row', async () => {
    const status = buildStatus({
      last_hedge_price: '0.05',
      snapshot: {
        ...buildSnapshot(),
        price_plume_per_eth: '0.0555',
        price_move_pct: '1',
        last_hedge_price: '0.05',
      },
    });
    mockApi.getHedgerStatusById.mockResolvedValue(status);

    render(<Hedger />);

    await waitFor(() => {
      expect(screen.getByText(/Pricing/i)).toBeInTheDocument();
    });

    expect(screen.getByText(/Last Hedge Price/i)).toBeInTheDocument();
    expect(screen.getByText(/0.0555/)).toBeInTheDocument();
    expect(screen.getByText(/11.00%/)).toBeInTheDocument();
  });

  it('uses token0/token1 labels and hides inverse perp row when token1 unhedged', async () => {
    const user = userEvent.setup();
    const status = buildStatus({
      id: 'hype_usdt_lp',
      config_summary: {
        token0_symbol: 'PLUME',
        token1_symbol: 'USDT',
        hedge_token0: true,
        hedge_token1: false,
        perp_symbol_token0: 'PLUMEUSDT',
        perp_symbol_token1: '',
      } as any,
      snapshot: buildSnapshot({
        token0_symbol: 'PLUME',
        token1_symbol: 'USDT',
        lp_token0: '100',
        lp_token1: '0',
        perp_token0: '-100',
        perp_token1: '0',
        net_token0: '0',
        net_token1: '0',
        lp_token0_usd: '1000',
        lp_token1_usd: '0',
        lp_mix_eth_pct: '100',
        lp_mix_plume_pct: '0',
        price_token1_per_token0: '1.2',
        token0_error: '0',
        token1_error: '0',
        token0_usd_error: '0',
        token1_usd_error: '0',
        perp_symbol_token0: 'PLUMEUSDT',
        perp_symbol_token1: '',
      }),
    });
    mockApi.getHedgerStatusById.mockResolvedValue(status);

    render(<Hedger />);
    const selector = await screen.findByLabelText(/Hedger/i);
    await user.selectOptions(selector, 'hype_usdt_lp');

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /^Exposure$/i })).toBeInTheDocument();
    });

    expect(screen.getByText(/PLUME Error/i)).toBeInTheDocument();
    expect(screen.getByText(/USDT Error/i)).toBeInTheDocument();
    expect(screen.getByText(/PLUME 100\.0% · USDT 0\.0%/i)).toBeInTheDocument();
    expect(screen.getByText(/Perp PLUME\/USDT \(Bybit\)/i)).toBeInTheDocument();
    expect(screen.queryByText(/Perp USDT\/PLUME/i)).toBeNull();
  });
});

describe('Hedger selector switching', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.getHedgerStatusById.mockResolvedValue(buildStatus());
  });

  it('loads Band2 status when Band2 is selected', async () => {
    const user = userEvent.setup();
    render(<Hedger />);
    const selector = await screen.findByLabelText(/Hedger/i);
    await user.selectOptions(selector, 'eth_plume_lp_band2');
    await waitFor(() => {
      expect(mockApi.getHedgerStatusById).toHaveBeenCalledWith('eth_plume_lp_band2');
    });
  });

  it('toggles a non-band hedger through the generic enabled endpoint', async () => {
    const user = userEvent.setup();
    mockApi.listHedgerInstances.mockResolvedValue([
      { id: 'eth_plume_lp', label: 'ETH/PLUME LP Hedger' },
      { id: 'hype_usdt_lp', label: 'HYPE/USDT LP Hedger' },
    ]);
    mockApi.getHedgerStatusById.mockResolvedValue(buildStatus());
    mockApi.setHedgerEnabledById.mockResolvedValue({ hedger_enabled: false });

    render(<Hedger />);
    const selector = await screen.findByLabelText(/Hedger/i);
    await user.selectOptions(selector, 'hype_usdt_lp');
    const disableButton = await screen.findByRole('button', { name: /Disable Hedger/i });
    await user.click(disableButton);

    await waitFor(() => {
      expect(mockApi.setHedgerEnabledById).toHaveBeenCalledWith('hype_usdt_lp', false);
    });
  });
});

describe('Hedger recent hedges controls', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('keeps clear button available even when no events exist', async () => {
    const status = buildStatus({ recent_events: [] });
    mockApi.getHedgerStatusById.mockResolvedValue(status);

    render(<Hedger />);

    await waitFor(() => {
      expect(screen.getByText(/Recent Hedges/i)).toBeInTheDocument();
    });

    const clearButton = screen.getByRole('button', { name: /clear/i });
    expect(clearButton).not.toBeDisabled();
  });

  it('clears events when confirmed', async () => {
    const status = buildStatus({
      recent_events: [
        {
          timestamp: 1700000000,
          asset: 'ETH',
          side: 'buy',
          qty: '0.1',
          net_eth_after: '0.2',
          usd_notional: '100',
          net_after_usd: '80',
          trigger_reason: 'price',
          price_source: 'rooster',
        },
      ],
    });
    expect(status.recent_events).toHaveLength(1);
    mockApi.getHedgerStatusById.mockResolvedValue(status);
    mockApi.clearHedgerEventsById.mockResolvedValue({ cleared: 1 });
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);

    const user = userEvent.setup();
    render(<Hedger />);

    await waitFor(() => {
      expect(screen.getByText(/Recent Hedges/i)).toBeInTheDocument();
    });
    await waitFor(() => {
      expect(screen.queryByText(/No hedges recorded yet/i)).not.toBeInTheDocument();
    });

    const clearButton = screen.getByRole('button', { name: /clear/i });
    await user.click(clearButton);

    await waitFor(() => {
      expect(mockApi.clearHedgerEventsById).toHaveBeenCalledWith('eth_plume_lp');
    });
    expect(confirmSpy).toHaveBeenCalled();
    confirmSpy.mockRestore();
  });

  it('does not clear events when confirmation is rejected', async () => {
    const status = buildStatus({
      recent_events: [
        {
          timestamp: 1700000000,
          asset: 'ETH',
          side: 'buy',
          qty: '0.1',
          net_eth_after: '0.2',
          usd_notional: '100',
          net_after_usd: '80',
          trigger_reason: 'price',
          price_source: 'rooster',
        },
      ],
    });
    expect(status.recent_events).toHaveLength(1);
    mockApi.getHedgerStatusById.mockResolvedValue(status);
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);

    const user = userEvent.setup();
    render(<Hedger />);

    await waitFor(() => {
      expect(screen.getByText(/Recent Hedges/i)).toBeInTheDocument();
    });

    const clearButton = screen.getByRole('button', { name: /clear/i });
    await user.click(clearButton);
    expect(mockApi.clearHedgerEventsById).not.toHaveBeenCalled();
    expect(confirmSpy).toHaveBeenCalled();
    confirmSpy.mockRestore();
  });

  it('clears events for non-band hedgers through the generic endpoint', async () => {
    const status = buildStatus({
      id: 'hype_usdt_lp',
      recent_events: [
        {
          timestamp: 1700000000,
          asset: 'PLUME',
          side: 'sell',
          qty: '10',
        },
      ],
    });
    mockApi.getHedgerStatusById.mockResolvedValue(status);
    mockApi.clearHedgerEventsById.mockResolvedValue({ cleared: 1 });
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    const user = userEvent.setup();

    render(<Hedger />);
    const selector = await screen.findByLabelText(/Hedger/i);
    await user.selectOptions(selector, 'hype_usdt_lp');
    const clearButton = await screen.findByRole('button', { name: /clear/i });
    await user.click(clearButton);

    await waitFor(() => {
      expect(mockApi.clearHedgerEventsById).toHaveBeenCalledWith('hype_usdt_lp');
    });
    confirmSpy.mockRestore();
  });
});
