import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

import SignalTable from '@/components/domain/signal/SignalTable';
import { useSignalStore, useSuiteStore } from '@/stores';
import * as apiModule from '@/api';
import * as socketsModule from '@/sockets';
import type { SignalStrategy } from '@/types';

// Mock API
vi.mock('@/api', () => ({
  api: {
    getSignalStrategies: vi.fn()
  }
}));

// Mock sockets (SignalTable uses polling fallback when not connected)
vi.mock('@/sockets', () => ({
  socket: {
    on: vi.fn(),
    off: vi.fn(),
    connected: false
  }
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

// Mock stores (merge with actual exports to avoid breaking other imports)
vi.mock('@/stores', async () => {
  const actual = await vi.importActual<any>('@/stores');
  return { ...actual, useSignalStore: vi.fn(), useSuiteStore: vi.fn() };
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
  const suiteState = { suite: 'all' as const, setSuite: vi.fn() };
  (useSuiteStore as any).mockImplementation((selector?: any) =>
    selector ? selector(suiteState) : suiteState
  );
};

const renderSignalTable = () =>
  render(
    <MemoryRouter
      initialEntries={['/signal']}
      future={{ v7_startTransition: true, v7_relativeSplatPath: true }}
    >
      <SignalTable />
    </MemoryRouter>
  );

describe('Signal MakerV2 truth overlay', () => {
  const mockSetRows = vi.fn();
  const mockMergeStrategy = vi.fn();
  const mockMergeStrategies = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();

    (apiModule.api.getSignalStrategies as any).mockImplementation(async () => ({
      strategies: currentSignalState?.rows ?? [],
      server_time: '2025-01-15 12:00:02',
      server_ts_ms: Date.now(),
    }));

    initSignalState({
      rows: [],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });
  });

  const renderSignalTable = () =>
    render(
      <MemoryRouter future={{ v7_startTransition: true, v7_relativeSplatPath: true }}>
        <SignalTable />
      </MemoryRouter>
    );

  it('renders Row 2 (Our / Ref used + mode + snapshot age) in desktop table', async () => {
    const makerStrategy: SignalStrategy = {
      id: 'bybit_binance_xrpusdt_maker',
      params: { bot_on: '0' } as any,
      legs: {
        A: { coin: 'XRP/USDT', exchange: 'bybit_linear', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'XRP/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
      maker_role_map: { maker_leg: 'A', ref_leg: 'B' } as any,
      maker_v2: {
        quote_snapshot: {
          ts_ms: Date.now(),
          mode: 'OFF',
          maker_exchange: 'bybit_linear',
          maker_symbol: 'XRP/USDT',
          ref_exchange: 'binance_spot',
          ref_symbol: 'XRP/USDT',
          ref_bid: '0.55',
          ref_ask: '0.56',
          place_bid: '0.54',
          place_ask: '0.57',
          cancel_bid: '0.545',
          cancel_ask: '0.565',
        }
      } as any,
    } as any;

    initSignalState({
      rows: [makerStrategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    await waitFor(() => expect(screen.getByText('bybit_binance_xrpusdt_maker')).toBeInTheDocument());

    expect(screen.getByText(/^Our(?: \(last-known\))?$/)).toBeInTheDocument();
    expect(screen.getByText(/^Ref(?: \(last-known\))?$/)).toBeInTheDocument();

    // Mode pill should exist (OFF).
    expect(screen.getAllByText('OFF').length).toBeGreaterThan(0);
  });

  it('does not emit React act warnings while the overlay stays mounted', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    const makerStrategy: SignalStrategy = {
      id: 'overlay_act_warning_check',
      params: { bot_on: '0' } as any,
      legs: {
        A: { coin: 'XRP/USDT', exchange: 'bybit_linear', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'XRP/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
      maker_role_map: { maker_leg: 'A', ref_leg: 'B' } as any,
      maker_v2: {
        quote_snapshot: {
          ts_ms: Date.now(),
          mode: 'OFF',
          maker_exchange: 'bybit_linear',
          ref_exchange: 'binance_spot',
        },
      } as any,
    } as any;

    initSignalState({
      rows: [makerStrategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    try {
      renderSignalTable();
      await screen.findByText('overlay_act_warning_check');
      await new Promise((resolve) => window.setTimeout(resolve, 1100));

      const actWarnings = errorSpy.mock.calls.filter(([message]) =>
        typeof message === 'string' && message.includes('not wrapped in act')
      );
      expect(actWarnings).toEqual([]);
    } finally {
      errorSpy.mockRestore();
    }
  });

  it('renders quote truth row from maker_v3.quote_snapshot fallback', async () => {
    const makerStrategy: SignalStrategy = {
      id: 'bybit_binance_plumeusdt_makerv3',
      params: { bot_on: '0' } as any,
      legs: {
        A: { coin: 'PLUME/USDT', exchange: 'bybit_linear', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'PLUME/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
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
          ref_bid: '0.00928',
          ref_ask: '0.00929',
          place_bid: '0.00949',
          place_ask: '0.00952',
        }
      } as any,
    } as any;

    initSignalState({
      rows: [makerStrategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    await waitFor(() => expect(screen.getByText('bybit_binance_plumeusdt_makerv3')).toBeInTheDocument());
    expect(screen.getByText(/^Our(?: \(last-known\))?$/)).toBeInTheDocument();
    expect(screen.getByText(/^Ref(?: \(last-known\))?$/)).toBeInTheDocument();
    expect(screen.getAllByText('OFF').length).toBeGreaterThan(0);
  });

  it('keeps mixed spot/perp strategies when filtering by market type', async () => {
    const mixedStrategy: SignalStrategy = {
      id: 'mixed_market_strategy',
      params: { bot_on: '1' } as any,
      legs: {
        A: {
          exchange: 'bybit',
          coin: 'PLUME',
          product_type: 'perp',
          decision_bid: 1.0,
          decision_ask: 1.01,
          update_time: '2025-01-15 12:00:00',
        } as any,
        B: {
          exchange: 'binance_spot',
          coin: 'PLUME',
          product_type: 'spot',
          decision_bid: 1.0,
          decision_ask: 1.01,
          update_time: '2025-01-15 12:00:00',
        } as any,
      },
      balances_ok: true,
    } as any;
    const spotOnlyStrategy: SignalStrategy = {
      id: 'spot_only_strategy',
      params: { bot_on: '1' } as any,
      legs: {
        A: {
          exchange: 'binance_spot',
          coin: 'PLUME',
          product_type: 'spot',
          decision_bid: 1.0,
          decision_ask: 1.01,
          update_time: '2025-01-15 12:00:00',
        } as any,
        B: null,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [mixedStrategy, spotOnlyStrategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    await waitFor(() => expect(screen.getByText('mixed_market_strategy')).toBeInTheDocument());

    const filtersToggle = screen.getByText('Filters');
    fireEvent.click(filtersToggle);
    await waitFor(() => {
      expect(screen.getByText('Market')).toBeInTheDocument();
    });
    const marketFilterLabel = screen.getByText('Market', { selector: 'label' });
    const marketFilter = marketFilterLabel.parentElement?.querySelector('select') as HTMLSelectElement;
    fireEvent.change(marketFilter, { target: { value: 'perp' } });

    await waitFor(() => {
      expect(screen.getByText('mixed_market_strategy')).toBeInTheDocument();
      expect(screen.queryByText('spot_only_strategy')).not.toBeInTheDocument();
    });
  });

  it('passes through maker_quote_status dict updates in signal_delta', async () => {
    const makerStrategy: SignalStrategy = {
      id: 'bybit_binance_xrpusdt_maker',
      params: { bot_on: '0' } as any,
      legs: {
        A: { coin: 'XRP/USDT', exchange: 'bybit_linear', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'XRP/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
      maker_role_map: { maker_leg: 'A', ref_leg: 'B' } as any,
      maker_v2: { quote_snapshot: { ts_ms: Date.now(), mode: 'OFF', maker_exchange: 'bybit_linear', ref_exchange: 'binance_spot' } } as any,
    } as any;

    initSignalState({
      rows: [makerStrategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    // Grab the registered signal_delta handler
    const deltaHandler = (socketsModule.socket.on as any).mock.calls.find(
      (call: any[]) => call[0] === 'signal_delta'
    )?.[1];
    expect(typeof deltaHandler).toBe('function');

    await act(async () => {
      deltaHandler({
        id: 'bybit_binance_xrpusdt_maker',
        maker_quote_status: {
          bid_open: 1,
          ask_open: 2,
          bid_blocked: 0,
          ask_blocked: 0,
          bid_depth: 1,
          ask_depth: 1,
        }
      });
    });

    expect(mockMergeStrategy).toHaveBeenCalled();
    const merged = mockMergeStrategy.mock.calls.at(-1)?.[0];
    expect(merged?.id).toBe('bybit_binance_xrpusdt_maker');
    expect(merged?.maker_quote_status?.bid_open).toBe(1);
    expect(merged?.maker_quote_status?.ask_open).toBe(2);
  });

  it('passes through quote_stacks updates in signal_delta', async () => {
    const makerStrategy: SignalStrategy = {
      id: 'bybit_binance_plumeusdt_makerv3',
      params: { bot_on: '1' } as any,
      legs: {
        A: { coin: 'PLUME/USDT', exchange: 'bybit_linear', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'PLUME/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
      maker_role_map: { maker_leg: 'A', ref_leg: 'B' } as any,
      maker_v3: {
        quote_snapshot: {
          ts_ms: Date.now(),
          mode: 'QUOTING',
          maker_exchange: 'bybit_linear',
          ref_exchange: 'binance_spot',
        }
      } as any,
    } as any;

    initSignalState({
      rows: [makerStrategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    const deltaHandler = (socketsModule.socket.on as any).mock.calls.find(
      (call: any[]) => call[0] === 'signal_delta'
    )?.[1];
    expect(typeof deltaHandler).toBe('function');

    await act(async () => {
      deltaHandler({
        id: 'bybit_binance_plumeusdt_makerv3',
        quote_stacks: {
          maker: {
            bands: [
              {
                band: 1,
                bid: { open: 1, depth: 2, blocked: 0, rows: [] },
                ask: { open: 2, depth: 3, blocked: 1, rows: [] },
              }
            ],
          },
          hedge: {
            bid: { open: 3, depth: 4, blocked: 1, rows: [] },
            ask: { open: 4, depth: 5, blocked: 2, rows: [] },
          },
        },
      });
    });

    expect(mockMergeStrategy).toHaveBeenCalled();
    const merged = mockMergeStrategy.mock.calls.at(-1)?.[0];
    expect(merged?.id).toBe('bybit_binance_plumeusdt_makerv3');
    expect(merged?.quote_stacks?.maker?.bands?.[0]?.bid?.open).toBe(1);
    expect(merged?.quote_stacks?.hedge?.ask?.depth).toBe(5);
  });

  it('does not render a hedge summary row in Quotes', async () => {
    const makerStrategy: SignalStrategy = {
      id: 'plumeusdt_bybit_perp_makerv3',
      params: { bot_on: '1' } as any,
      state: {
        state: 'running',
        ts_ms: Date.now(),
      } as any,
      legs: {
        A: { coin: 'PLUME/USDT', exchange: 'bybit_perp', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'PLUME/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
      quote_stacks: {
        maker: {
          bands: [
            {
              band: 1,
              bid: { open: 1, depth: 2, blocked: 0, rows: [] },
              ask: { open: 2, depth: 3, blocked: 1, rows: [] },
            },
          ],
        },
        hedge: {
          bid: { open: 3, depth: 4, blocked: 1, rows: [] },
          ask: { open: 4, depth: 5, blocked: 2, rows: [] },
        },
      } as any,
      maker_role_map: { maker_leg: 'A', ref_leg: 'B' } as any,
    } as any;

    initSignalState({
      rows: [makerStrategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    const { container } = renderSignalTable();
    await screen.findByText('plumeusdt_bybit_perp_makerv3');

    const quotesCell = container.querySelector('tbody tr td:nth-child(5)');
    expect(quotesCell?.textContent).toContain('B 1/2 · A 2/3');
    expect(quotesCell?.textContent).not.toContain('H B');
  });

  it('renders explicit global and local inventory quantities', async () => {
    const strategy: SignalStrategy = {
      id: 'plumeusdt_bybit_perp_makerv3',
      params: { bot_on: '1' } as any,
      state: { state: 'running', ts_ms: Date.now() } as any,
      pricing_adjustments: [
        {
          type: 'inventory_skew',
          curr_qty: 41689,
          local_qty: 150000,
          local_qty_key: {
            venue_root: 'bybit',
            instrument_type: 'linear',
            base: 'PLUME',
          },
          local_qty_matched_rows: 2,
          local_qty_missing_snapshot: 0,
        },
      ],
      legs: {
        A: { coin: 'PLUME/USDT', exchange: 'bybit_perp', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'PLUME/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    const { container } = renderSignalTable();
    await screen.findByText('plumeusdt_bybit_perp_makerv3');

    const globalQtyCell = container.querySelector('tbody tr td:nth-child(3)');
    const localQtyCell = container.querySelector('tbody tr td:nth-child(4)');
    expect(globalQtyCell?.textContent).toContain('41,689.0000');
    expect(localQtyCell?.textContent).toContain('150,000.0000');
  });

  it('keeps live quote counts visible when bot_on is off', async () => {
    const strategy: SignalStrategy = {
      id: 'plumeusdt_okx_perp_makerv3',
      params: { bot_on: '0' } as any,
      state: { state: 'bot_off', ts_ms: Date.now(), bot_on: false } as any,
      maker_quote_status: {
        bid_open: 1,
        ask_open: 2,
        bid_depth: 3,
        ask_depth: 3,
        bid_blocked: 2,
        ask_blocked: 1,
      } as any,
      maker_v3: {
        quote_snapshot: {
          ts_ms: Date.now(),
          mode: 'OFF',
          reason: 'bot_off',
        },
      } as any,
      legs: {
        A: { coin: 'PLUME/USDT', exchange: 'okx', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'PLUME/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    const { container } = renderSignalTable();
    await screen.findByText('plumeusdt_okx_perp_makerv3');

    const quotesCell = container.querySelector('tbody tr td:nth-child(5)');
    expect(quotesCell?.textContent).toContain('B 1/3 · A 2/3');
  });

  it('prefers explicit global inventory over risk_delta fallback', async () => {
    const strategy: SignalStrategy = {
      id: 'plumeusdt_bybit_spot_makerv3',
      params: { bot_on: '0' } as any,
      state: { state: 'bot_off', ts_ms: Date.now(), bot_on: false } as any,
      risk_delta: -231161.2 as any,
      pricing_adjustments: [
        {
          type: 'inventory_skew',
          global_qty: 134961.863,
          local_qty: -62145.1373,
        } as any,
      ],
      legs: {
        A: { coin: 'PLUME/USDT', exchange: 'bybit_spot', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'PLUME/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    const { container } = renderSignalTable();
    await screen.findByText('plumeusdt_bybit_spot_makerv3');

    const globalQtyCell = container.querySelector('tbody tr td:nth-child(3)');
    const localQtyCell = container.querySelector('tbody tr td:nth-child(4)');
    expect(globalQtyCell?.textContent).toContain('134,961.8630');
    expect(globalQtyCell?.textContent).not.toContain('-231,161.2000');
    expect(localQtyCell?.textContent).toContain('-62,145.1373');
  });

  it('renders spread from backend spread_net_bps fields', async () => {
    const strategy: SignalStrategy = {
      id: 'spread_mid_strategy',
      params: { bot_on: '1' } as any,
      state: { state: 'running', ts_ms: Date.now() } as any,
      spread_net_bps: -288.5 as any,
      spread_net_case1_bps: -288.5 as any,
      spread_net_case2_bps: -476.2 as any,
      spread_net_best_case: 'case1' as any,
      required_edge_bps: 45 as any,
      edge2_bps: -333.5 as any,
      legs: {
        A: {
          coin: 'PLUME/USDT',
          exchange: 'bybit_perp',
          decision_bid: 100,
          decision_ask: 102,
          update_time: '2025-01-15 12:00:00',
        } as any,
        B: {
          coin: 'PLUME/USDT',
          exchange: 'binance_spot',
          decision_bid: 103,
          decision_ask: 105,
          update_time: '2025-01-15 12:00:00',
        } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    const { container } = renderSignalTable();
    await screen.findByText('spread_mid_strategy');

    expect(screen.getByText('Spread')).toBeInTheDocument();
    const spreadCell = container.querySelector('tbody tr td:nth-child(9)');
    expect(spreadCell?.textContent).toContain('-288.5 bps');
  });

  it('does not render a separate Run column in Signal', async () => {
    const nowMs = Date.now();
    const strategy: SignalStrategy = {
      id: 'plumeusdt_bybit_spot_makerv3',
      params: { bot_on: '0' } as any,
      state: {
        state: 'bot_off',
        ts_ms: nowMs,
        bot_on: false,
      } as any,
      legs: {
        A: { coin: 'PLUME/USDT', exchange: 'bybit_spot', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'PLUME/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    await screen.findByText('plumeusdt_bybit_spot_makerv3');
    expect(screen.getByLabelText('Trading is paused for plumeusdt_bybit_spot_makerv3')).toBeInTheDocument();
    expect(screen.getByText('Runner On')).toBeInTheDocument();
    expect(screen.queryByText('Run')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Run is running for plumeusdt_bybit_spot_makerv3')).not.toBeInTheDocument();
  });

  it('treats fresh state snapshots as runner-on for trading status when explicit running is absent', async () => {
    const nowMs = Date.now();
    const strategy: SignalStrategy = {
      id: 'plumeusdt_bybit_perp_makerv3',
      params: { bot_on: '1' } as any,
      state: {
        state: 'quotes_replaced',
        ts_ms: nowMs,
        bot_on: true,
      } as any,
      legs: {
        A: { coin: 'PLUME/USDT', exchange: 'bybit_linear', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'PLUME/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    await screen.findByText('plumeusdt_bybit_perp_makerv3');
    expect(screen.getByLabelText('Trading is enabled for plumeusdt_bybit_perp_makerv3')).toBeInTheDocument();
    expect(screen.getByText('Enabled')).toBeInTheDocument();
    expect(screen.getByText('Runner On')).toBeInTheDocument();
    expect(screen.queryByText('Pending')).not.toBeInTheDocument();
    expect(screen.queryByText('Runner Unknown')).not.toBeInTheDocument();
  });

  it('shows pending when params are enabled but the live signal is blocked and not tradeable', async () => {
    const nowMs = Date.now();
    const strategy: SignalStrategy = {
      id: 'plumeusdt_bybit_perp_makerv3',
      params: { bot_on: '1' } as any,
      tradeable: false,
      blocked: true,
      state: {
        state: 'bot_off',
        ts_ms: nowMs,
        bot_on: false,
      } as any,
      maker_v3: {
        quote_snapshot: {
          ts_ms: nowMs,
          mode: 'OFF',
          reason: 'bot_off',
        },
      } as any,
      legs: {
        A: { coin: 'PLUME/USDT', exchange: 'bybit_linear', update_time: '2025-01-15 12:00:00' } as any,
        B: { coin: 'PLUME/USDT', exchange: 'binance_spot', update_time: '2025-01-15 12:00:00' } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    await screen.findByText('plumeusdt_bybit_perp_makerv3');
    expect(screen.getByLabelText('Trading is pending for plumeusdt_bybit_perp_makerv3')).toBeInTheDocument();
    expect(screen.getByText('Pending')).toBeInTheDocument();
    expect(screen.getByText('Runner On')).toBeInTheDocument();
    expect(screen.queryByText('Enabled')).not.toBeInTheDocument();
  });

  it('passes through contract_id keyed leg patches in signal_delta', async () => {
    const strategy: SignalStrategy = {
      id: 'contract_patch_strategy',
      params: { bot_on: '1' } as any,
      legs_order: ['BTCUSDT-SPOT', 'BTCUSDT-PERP'],
      legs: {
        'BTCUSDT-PERP': {
          coin: 'BTC-PERP',
          exchange: 'bybit_linear',
          decision_bid: 50000,
          decision_ask: 50100,
          update_time: '2025-01-15 12:00:00',
        } as any,
        'BTCUSDT-SPOT': {
          coin: 'BTC-SPOT',
          exchange: 'bybit_linear',
          decision_bid: 49990,
          decision_ask: 50090,
          update_time: '2025-01-15 12:00:01',
        } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    const deltaHandler = (socketsModule.socket.on as any).mock.calls.find(
      (call: any[]) => call[0] === 'signal_delta'
    )?.[1];
    expect(typeof deltaHandler).toBe('function');

    await act(async () => {
      deltaHandler({
        id: 'contract_patch_strategy',
        legs: {
          'BTCUSDT-PERP': { decision_bid: 50025 },
        },
      });
    });

    expect(mockMergeStrategy).toHaveBeenCalled();
    const merged = mockMergeStrategy.mock.calls.at(-1)?.[0];
    expect(merged?.legs?.['BTCUSDT-PERP']).toBeDefined();
    expect(merged?.legs?.['BTCUSDT-SPOT']).toBeUndefined();
  });

  it('normalizes contract-keyed delta leg coin labels from slash symbols', async () => {
    const strategy: SignalStrategy = {
      id: 'plume_delta_coin_strategy',
      params: { bot_on: '0' } as any,
      legs_order: ['binance_spot:PLUME/USDT', 'okx:PLUME/USDT'],
      legs: {
        'binance_spot:PLUME/USDT': {
          coin: 'PLUME',
          exchange: 'binance_spot',
          decision_bid: 0.01043,
          decision_ask: 0.01044,
          update_time: '2025-01-15 12:00:00',
        } as any,
        'okx:PLUME/USDT': {
          coin: 'PLUME',
          exchange: 'okx',
          decision_bid: 0.01041,
          decision_ask: 0.01042,
          update_time: '2025-01-15 12:00:00',
        } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    const deltaHandler = (socketsModule.socket.on as any).mock.calls.find(
      (call: any[]) => call[0] === 'signal_delta'
    )?.[1];
    expect(typeof deltaHandler).toBe('function');

    await act(async () => {
      deltaHandler({
        id: 'plume_delta_coin_strategy',
        legs: {
          'binance_spot:PLUME/USDT': {
            symbol: 'PLUME/USDT',
            decision_bid: 0.01045,
            decision_ask: 0.01046,
          },
        },
      });
    });

    expect(mockMergeStrategy).toHaveBeenCalled();
    const merged = mockMergeStrategy.mock.calls.at(-1)?.[0];
    expect(merged?.legs?.['binance_spot:PLUME/USDT']?.coin).toBe('PLUME');
  });

  it('passes through legs_order explicit clear in signal_delta', async () => {
    const strategy: SignalStrategy = {
      id: 'contract_order_clear_strategy',
      params: { bot_on: '1' } as any,
      legs_order: ['BTCUSDT-SPOT', 'BTCUSDT-PERP'],
      legs: {
        'BTCUSDT-PERP': {
          coin: 'BTC-PERP',
          exchange: 'bybit_linear',
          decision_bid: 50000,
          decision_ask: 50100,
        } as any,
        'BTCUSDT-SPOT': {
          coin: 'BTC-SPOT',
          exchange: 'bybit_linear',
          decision_bid: 49990,
          decision_ask: 50090,
        } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    const deltaHandler = (socketsModule.socket.on as any).mock.calls.find(
      (call: any[]) => call[0] === 'signal_delta'
    )?.[1];
    expect(typeof deltaHandler).toBe('function');

    await act(async () => {
      deltaHandler({
        id: 'contract_order_clear_strategy',
        legs_order: null,
        legs: {
          'BTCUSDT-PERP': null,
        },
      });
    });

    expect(mockMergeStrategy).toHaveBeenCalled();
    const merged = mockMergeStrategy.mock.calls.at(-1)?.[0];
    expect(merged?.legs_order).toBeNull();
    expect(merged?.legs?.['BTCUSDT-PERP']).toBeNull();
  });

  it('renders same-exchange contract legs using legs_order slot mapping', async () => {
    const strategy: SignalStrategy = {
      id: 'same_exchange_strategy',
      params: { bot_on: '1' } as any,
      legs_order: ['BTCUSDT-SPOT', 'BTCUSDT-PERP'],
      legs: {
        'BTCUSDT-PERP': {
          coin: 'BTC-PERP',
          exchange: 'bybit_linear',
          decision_bid: 50000,
          decision_ask: 50100,
          update_time: '2025-01-15 12:00:00',
        } as any,
        'BTCUSDT-SPOT': {
          coin: 'BTC-SPOT',
          exchange: 'bybit_linear',
          decision_bid: 49990,
          decision_ask: 50090,
          update_time: '2025-01-15 12:00:01',
        } as any,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    const { container } = renderSignalTable();

    await waitFor(() => {
      expect(screen.getByText('same_exchange_strategy')).toBeInTheDocument();
    });

    const legASlot = container.querySelector('tbody tr td:nth-child(7)');
    const legBSlot = container.querySelector('tbody tr td:nth-child(8)');
    expect(legASlot?.textContent).toContain('BTC-SPOT');
    expect(legBSlot?.textContent).toContain('BTC-PERP');
    expect(legASlot?.textContent).toContain('bybit_linear');
    expect(legBSlot?.textContent).toContain('bybit_linear');
  });

  it('renders maker and ref markets from maker_role_map even when extra legs precede them', async () => {
    const strategy: SignalStrategy = {
      id: 'plumeusdt_okx_perp_makerv3',
      params: { bot_on: '0' } as any,
      legs_order: ['bybit:PLUME/USDT', 'binance_spot:PLUME/USDT', 'okx:PLUME/USDT'],
      legs: {
        'bybit:PLUME/USDT': {
          coin: 'PLUME',
          symbol: 'PLUME/USDT',
          exchange: 'bybit',
          decision_bid: null,
          decision_ask: null,
          update_time: '2025-01-15 12:00:00',
        } as any,
        'binance_spot:PLUME/USDT': {
          coin: 'PLUME',
          symbol: 'PLUME/USDT',
          exchange: 'binance_spot',
          decision_bid: 0.01050,
          decision_ask: 0.01051,
          update_time: '2025-01-15 12:00:00',
        } as any,
        'okx:PLUME/USDT': {
          coin: 'PLUME',
          symbol: 'PLUME/USDT',
          exchange: 'okx',
          decision_bid: 0.01047,
          decision_ask: 0.01048,
          update_time: '2025-01-15 12:00:00',
        } as any,
      },
      balances_ok: true,
      maker_role_map: {
        maker_leg: 'okx:PLUME/USDT',
        ref_leg: 'binance_spot:PLUME/USDT',
        hedge_leg: 'binance_spot:PLUME/USDT',
      } as any,
      maker_v3: {
        quote_snapshot: {
          ts_ms: Date.now(),
          mode: 'OFF',
          reason: 'bot_off',
          maker_exchange: 'okx',
          maker_symbol: 'PLUME/USDT',
          ref_exchange: 'binance_spot',
          ref_symbol: 'PLUME/USDT',
          ref_bid: '0.01050',
          ref_ask: '0.01051',
          place_bid: '0.01047',
          place_ask: '0.01048',
        },
      } as any,
      spread_net_bps: -28.6 as any,
      spread_net_case1_bps: -28.6 as any,
      spread_net_case2_bps: -47.6 as any,
      spread_net_best_case: 'case1' as any,
      } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    const { container } = renderSignalTable();

    await waitFor(() => {
      expect(screen.getByText('plumeusdt_okx_perp_makerv3')).toBeInTheDocument();
    });

    const legASlot = container.querySelector('tbody tr td:nth-child(7)');
    const legBSlot = container.querySelector('tbody tr td:nth-child(8)');
    const spreadCell = container.querySelector('tbody tr td:nth-child(9)');
    expect(legASlot?.textContent).toContain('okx');
    expect(legASlot?.textContent).toContain('PLUME');
    expect(legBSlot?.textContent).toContain('binance_spot');
    expect(legBSlot?.textContent).toContain('PLUME');
    expect(legASlot?.textContent).not.toContain('bybit');
    expect(spreadCell?.textContent).toContain('-28.6 bps');
  });

  it('maps maker overlay legs by canonical symbol when maker_role_map is missing', async () => {
    const strategy: SignalStrategy = {
      id: 'maker_overlay_without_role_map',
      params: { bot_on: '0' } as any,
      legs: {
        A: {
          exchange: 'binance_spot',
          coin: 'PLUME',
          pair: 'PLUME/USDT',
          display_name_long: 'Binance PLUME Spot',
          decision_bid: 1.0,
          decision_ask: 1.01,
          update_time: '2025-01-15 12:00:00',
        } as any,
        B: {
          exchange: 'bybit',
          coin: 'PLUME',
          instrument_id: 'PLUMEUSDT-LINEAR.BYBIT',
          raw_symbol: 'PLUMEUSDT',
          pair: 'PLUME/USDT',
          display_name_long: 'Bybit PLUME Perp',
          decision_bid: 1.02,
          decision_ask: 1.03,
          update_time: '2025-01-15 12:00:00',
        } as any,
      },
      balances_ok: true,
      maker_role_map: undefined,
      maker_v3: {
        quote_snapshot: {
          ts_ms: Date.now(),
          mode: 'OFF',
          maker_exchange: 'bybit',
          maker_symbol: 'PLUME/USDT',
          ref_exchange: 'binance_spot',
          ref_symbol: 'PLUME/USDT',
          ref_bid: '1.00',
          ref_ask: '1.01',
          place_bid: '1.02',
          place_ask: '1.03',
        },
      } as any,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    const { container } = renderSignalTable();

    await waitFor(() => expect(screen.getByText('maker_overlay_without_role_map')).toBeInTheDocument());

    const legASlot = container.querySelector('tbody tr td:nth-child(7)');
    const legBSlot = container.querySelector('tbody tr td:nth-child(8)');
    expect(legASlot?.textContent).toContain('Bybit PLUME Perp');
    expect(legBSlot?.textContent).toContain('Binance PLUME Spot');
  });

  it('matches maker overlay legs from canonical pair when contract_id carries instrument text', async () => {
    const strategy: SignalStrategy = {
      id: 'maker_overlay_contract_id_fallback',
      params: { bot_on: '0' } as any,
      legs: {
        A: {
          exchange: 'binance_spot',
          coin: 'PLUME',
          display_name_long: 'Binance PLUME Spot',
          decision_bid: 1.0,
          decision_ask: 1.01,
          update_time: '2025-01-15 12:00:00',
        } as any,
        B: {
          exchange: 'bybit',
          coin: 'PLUME',
          contract_id: 'bybit:PLUMEUSDT-LINEAR.BYBIT',
          instrument_id: 'PLUMEUSDT-LINEAR.BYBIT',
          raw_symbol: 'PLUMEUSDT',
          display_name_long: 'Bybit PLUME Perp',
          decision_bid: 1.02,
          decision_ask: 1.03,
          update_time: '2025-01-15 12:00:00',
        } as any,
      },
      balances_ok: true,
      maker_role_map: undefined,
      maker_v3: {
        quote_snapshot: {
          ts_ms: Date.now(),
          mode: 'OFF',
          maker_exchange: 'bybit',
          maker_symbol: 'PLUME/USDT',
          ref_exchange: 'binance_spot',
          ref_symbol: 'PLUME/USDT',
          ref_bid: '1.00',
          ref_ask: '1.01',
          place_bid: '1.02',
          place_ask: '1.03',
        },
      } as any,
    } as any;

    initSignalState({
      rows: [strategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    const { container } = renderSignalTable();

    await waitFor(() => expect(screen.getByText('maker_overlay_contract_id_fallback')).toBeInTheDocument());

    const legASlot = container.querySelector('tbody tr td:nth-child(7)');
    const legBSlot = container.querySelector('tbody tr td:nth-child(8)');
    expect(legASlot?.textContent).toContain('Bybit PLUME Perp');
    expect(legBSlot?.textContent).toContain('Binance PLUME Spot');
  });

  it('uses mergeStrategies once for market_update strategy arrays', async () => {
    const strategyA: SignalStrategy = {
      id: 'batch_a',
      params: { bot_on: '1' } as any,
      legs: {
        A: { coin: 'BTC/USDT', exchange: 'bybit_linear', update_time: '2025-01-15 12:00:00' } as any,
        B: null,
      },
      balances_ok: true,
    } as any;

    const strategyB: SignalStrategy = {
      id: 'batch_b',
      params: { bot_on: '1' } as any,
      legs: {
        A: { coin: 'ETH/USDT', exchange: 'bybit_linear', update_time: '2025-01-15 12:00:00' } as any,
        B: null,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [strategyA, strategyB],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    const marketUpdateHandler = (socketsModule.socket.on as any).mock.calls.find(
      (call: any[]) => call[0] === 'market_update'
    )?.[1];
    expect(typeof marketUpdateHandler).toBe('function');

    await act(async () => {
      marketUpdateHandler({
        strategies: [strategyA, strategyB],
        server_time: '2025-01-15 12:00:03',
        server_ts_ms: Date.now(),
      });
    });

    expect(mockMergeStrategies).toHaveBeenCalledTimes(1);
    expect(mockMergeStrategies).toHaveBeenCalledWith([strategyA, strategyB]);
    expect(mockMergeStrategy).not.toHaveBeenCalled();
  });

  it('accepts market_update strategies before initial REST rows when suite is all', async () => {
    const strategyA: SignalStrategy = {
      id: 'bootstrap_a',
      params: { bot_on: '1' } as any,
      legs: {
        A: { coin: 'BTC/USDT', exchange: 'bybit_linear', update_time: '2025-01-15 12:00:00' } as any,
        B: null,
      },
      balances_ok: true,
    } as any;

    initSignalState({
      rows: [],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    const marketUpdateHandler = (socketsModule.socket.on as any).mock.calls.find(
      (call: any[]) => call[0] === 'market_update'
    )?.[1];
    expect(typeof marketUpdateHandler).toBe('function');

    await act(async () => {
      marketUpdateHandler({
        strategies: [strategyA],
        server_time: '2025-01-15 12:00:03',
        server_ts_ms: Date.now(),
      });
    });

    expect(mockMergeStrategies).toHaveBeenCalledTimes(1);
    expect(mockMergeStrategies).toHaveBeenCalledWith([strategyA]);
  });

  it('prunes stale strategy IDs when market_update snapshot shrinks', async () => {
    const strategyA: SignalStrategy = {
      id: 'batch_a',
      params: { bot_on: '1' } as any,
      legs: { A: { coin: 'BTC/USDT', exchange: 'bybit_linear' } as any, B: null },
      balances_ok: true,
    } as any;
    const strategyB: SignalStrategy = {
      id: 'batch_b',
      params: { bot_on: '1' } as any,
      legs: { A: { coin: 'ETH/USDT', exchange: 'bybit_linear' } as any, B: null },
      balances_ok: true,
    } as any;
    const staleStrategy: SignalStrategy = {
      id: 'stale_z',
      params: { bot_on: '0' } as any,
      legs: { A: { coin: 'XRP/USDT', exchange: 'bybit_linear' } as any, B: null },
      balances_ok: false,
    } as any;

    initSignalState({
      rows: [strategyA, strategyB, staleStrategy],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });

    renderSignalTable();

    const marketUpdateHandler = (socketsModule.socket.on as any).mock.calls.find(
      (call: any[]) => call[0] === 'market_update'
    )?.[1];
    expect(typeof marketUpdateHandler).toBe('function');

    await act(async () => {
      marketUpdateHandler({
        strategies: [strategyA, strategyB],
        server_time: '2025-01-15 12:00:04',
        server_ts_ms: Date.now(),
      });
    });

    expect(mockMergeStrategies).toHaveBeenCalledWith([strategyA, strategyB]);
    expect(mockSetRows).toHaveBeenCalledWith([strategyA, strategyB]);
  });
});
