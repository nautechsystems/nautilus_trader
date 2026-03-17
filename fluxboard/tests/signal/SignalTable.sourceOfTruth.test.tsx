import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import SignalTable, { buildInventorySkewTooltip } from '@/components/domain/signal/SignalTable';
import * as apiModule from '@/api';
import { useSignalStore } from '@/stores';
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

function initSignalState(state: any) {
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
}

function renderSignalTable() {
  return render(
    <MemoryRouter initialEntries={['/tokenmm/signal']}>
      <SignalTable />
    </MemoryRouter>
  );
}

describe('SignalTable source-of-truth contract', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    (apiModule.api.getSignalStrategies as any).mockResolvedValue({
      strategies: [],
      server_time: '2025-01-15 12:00:02',
      server_ts_ms: Date.now(),
    });
    initSignalState({ rows: [] });
  });

  it('renders maker spread from the same maker quote snapshot that powers the Our/Ref rows', async () => {
    const strategy: SignalStrategy = {
      id: 'maker_truth_contract',
      params: { bot_on: '0' } as any,
      running: true,
      state: { state: 'bot_off', ts_ms: Date.now(), bot_on: false } as any,
      spread_net_bps: 0.0,
      spread_net_case1_bps: 0.0,
      spread_net_case2_bps: 0.0,
      spread_net_best_case: 'case1',
      required_edge_bps: 45,
      edge2_bps: -333.5,
      strategy_family: 'maker_v3',
      meta: {
        class: 'maker_v3',
        strategy_groups: 'tokenmm',
      } as any,
      maker_role_map: { maker_leg: 'A', ref_leg: 'B' } as any,
      maker_v3: {
        quote_snapshot: {
          ts_ms: Date.now(),
          mode: 'OFF',
          reason: 'bot_off',
          maker_exchange: 'bybit_linear',
          maker_symbol: 'PLUME/USDT',
          ref_exchange: 'binance_spot',
          ref_symbol: 'PLUME/USDT',
          place_bid: 100,
          place_ask: 102,
          ref_bid: 103,
          ref_ask: 105,
        },
      } as any,
      legs: {
        A: {
          exchange: 'bybit_linear',
          coin: 'PLUME',
          decision_bid: 103,
          decision_ask: 105,
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
    expect(spreadCell?.textContent).toContain('-288.5 bps');
    expect(spreadCell?.textContent).not.toContain('0.0 bps');
  });

  it('prefers backend skew_bps_signed over edge-delta reconstruction when both are present', async () => {
    const strategy: SignalStrategy = {
      id: 'signed_skew_contract',
      params: { bot_on: '1' } as any,
      running: true,
      state: { state: 'running', ts_ms: Date.now(), bot_on: true } as any,
      pricing_adjustments: [
        {
          type: 'inventory_skew',
          skew_bps_signed: -3.86,
          inv_skew: -3.86,
          base_bid_edge_bps: 10,
          base_ask_edge_bps: 10,
          eff_bid_edge_bps: 8,
          eff_ask_edge_bps: 12,
        } as any,
      ],
      strategy_family: 'maker_v3',
      meta: {
        class: 'maker_v3',
        strategy_groups: 'tokenmm',
      } as any,
      legs: {
        A: {
          exchange: 'okx_swap',
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
    renderSignalTable();

    await waitFor(() => expect(screen.getByText(strategy.id)).toBeInTheDocument());
    expect(screen.getByText('-3.9')).toBeInTheDocument();
    expect(screen.queryByText('+2.0')).not.toBeInTheDocument();
  });

  it('treats linear plus global plus local as the quoted-FV skew source of truth', () => {
    const tooltip = buildInventorySkewTooltip(
      {
        type: 'inventory_skew',
        skew_bps_signed: 4,
        inv_skew: 4,
        inv_skew_global: 2.5,
        inv_skew_local: 0.5,
        eff_bid_edge_bps: 8,
        eff_ask_edge_bps: 12,
      } as any,
      {
        linear_offset_bps: '1',
        des_qty_global: '0',
        max_qty_global: '40000',
        max_skew_bps_global: '5',
        des_qty_local: '0',
        max_qty_local: '25000',
        max_skew_bps_local: '5',
      }
    );

    expect(tooltip).toContain('linear + global + local = total');
    expect(tooltip).toContain('+1.0 + +2.5 + +0.5 = +4.0');
  });
});
