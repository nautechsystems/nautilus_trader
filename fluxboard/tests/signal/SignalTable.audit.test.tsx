import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import SignalTable, { buildBalanceTooltip, buildStrategyParamTooltip } from '@/components/domain/signal/SignalTable';
import { useSignalStore } from '@/stores';
import * as apiModule from '@/api';
import type { SignalStrategy } from '@/types';

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

vi.mock('@/stores', async () => {
  const actual = await vi.importActual<any>('@/stores');
  return { ...actual, useSignalStore: vi.fn() };
});

let currentSignalState: any;
const initSignalState = (state: any) => {
  currentSignalState = {
    rows: [],
    setRows: vi.fn(),
    mergeStrategy: vi.fn(),
    mergeStrategies: vi.fn(),
    ...state,
  };
  (useSignalStore as any).mockImplementation((selector?: any) =>
    selector ? selector(currentSignalState) : currentSignalState
  );
  (useSignalStore as any).getState = () => currentSignalState;
};

function renderSignalTable() {
  return render(
    <MemoryRouter initialEntries={['/tokenmm/signal']}>
      <SignalTable />
    </MemoryRouter>
  );
}

describe('SignalTable audit coverage', () => {
  beforeEach(() => {
    vi.clearAllMocks();

    (apiModule.api.getSignalStrategies as any).mockResolvedValue({
      strategies: [],
      server_time: '2025-01-15 12:00:02',
      server_ts_ms: Date.now(),
    });

    initSignalState({ rows: [] });
  });

  it('renders the canonical backend spread value instead of a local mid-vs-mid proxy', async () => {
    const strategy: SignalStrategy = {
      id: 'spread_strategy',
      params: { bot_on: '1', cex_bid_edge: '5', cex_ask_edge: '5', pool_edge: '2' } as any,
      running: true,
      state: { state: 'running', ts_ms: Date.now(), bot_on: true } as any,
      decision_edge_bps: 14.5,
      edge2_bps: 7.5,
      required_edge_bps: 7,
      spread_net_bps: 14.5,
      spread_net_case1_bps: 14.5,
      spread_net_case2_bps: -26,
      spread_net_best_case: 'case1',
      strategy_family: 'maker_v3',
      meta: {
        class: 'maker_v3',
        strategy_groups: 'tokenmm',
      },
      legs: {
        A: {
          exchange: 'bybit_linear',
          coin: 'PLUME',
          decision_bid: 100,
          decision_ask: 102,
          update_time: '2025-01-15 12:00:00',
        } as any,
        B: {
          exchange: 'binance_spot',
          coin: 'PLUME',
          decision_bid: 103,
          decision_ask: 105,
          update_time: '2025-01-15 12:00:01',
        } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({ rows: [strategy] });
    const { container } = renderSignalTable();

    await waitFor(() => expect(screen.getByText(strategy.id)).toBeInTheDocument());

    const spreadCell = container.querySelector('tbody tr td:nth-child(9)');
    expect(spreadCell?.textContent).toContain('14.5 bps');
    expect(spreadCell?.textContent).not.toContain('-288.5 bps');
  });

  it('builds balance methodology text from payload data instead of a hardcoded 10x rule', () => {
    const tooltip = buildBalanceTooltip({
      status: 'WARN',
      qty: '25',
      multiplier: '3',
      summary: 'Needs more PLUME',
      requirements: [
        {
          location: 'bybit',
          token: 'PLUME',
          required: '75',
          available: '50',
          coverage: 2 / 3,
          kind: 'maker',
        },
      ],
      missing: [],
    });

    expect(tooltip).toContain('Methodology: Coverage = available / required');
    expect(tooltip).toContain('Sizing basis: qty 25 × multiplier 3');
    expect(tooltip).not.toContain('10× qty buffer');
  });

  it('keeps the strategy tooltip focused on params instead of duplicating pricing docs', () => {
    const tooltip = buildStrategyParamTooltip({
      id: 'tooltip_strategy',
      params: {
        cex_bid_edge: '5',
        cex_ask_edge: '6',
        pool_edge: '7',
        qty: '25',
        slippage_bps: '2',
      },
    } as any);

    expect(tooltip).toContain('Edge thresholds (minimum edge to trade):');
    expect(tooltip).toContain('qty: 25');
    expect(tooltip).not.toContain('Decision prices (generic)');
    expect(tooltip).not.toContain('Maker quote snapshot row');
  });

  it('renders inventory qty cells without extra per-cell hover affordances', async () => {
    const strategy: SignalStrategy = {
      id: 'qty_strategy',
      params: { bot_on: '1' } as any,
      running: true,
      state: { state: 'running', ts_ms: Date.now(), bot_on: true } as any,
      pricing_adjustments: [
        {
          type: 'inventory_skew',
          curr_qty: 125,
          local_qty: 45,
        } as any,
      ],
      strategy_family: 'maker_v3',
      meta: {
        class: 'maker_v3',
        strategy_groups: 'tokenmm',
      } as any,
      legs: {
        A: {
          exchange: 'bybit_linear',
          coin: 'PLUME',
          decision_bid: 1,
          decision_ask: 1.01,
          update_time: '2025-01-15 12:00:00',
        } as any,
        B: {
          exchange: 'binance_spot',
          coin: 'PLUME',
          decision_bid: 1.02,
          decision_ask: 1.03,
          update_time: '2025-01-15 12:00:01',
        } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({ rows: [strategy] });
    const { container } = renderSignalTable();

    await waitFor(() => expect(screen.getByText(strategy.id)).toBeInTheDocument());

    const globalQty = container.querySelector('tbody tr td:nth-child(3) span');
    const localQty = container.querySelector('tbody tr td:nth-child(4) span');
    expect(globalQty?.className).not.toContain('cursor-help');
    expect(localQty?.className).not.toContain('cursor-help');
  });
});
