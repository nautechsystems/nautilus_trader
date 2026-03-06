import { render, screen, waitFor } from '@testing-library/react';
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
    <MemoryRouter initialEntries={['/signal']}>
      <SignalTable />
    </MemoryRouter>
  );

describe('Signal MakerV2 truth overlay', () => {
  const mockSetRows = vi.fn();
  const mockMergeStrategy = vi.fn();
  const mockMergeStrategies = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();

    (apiModule.api.getSignalStrategies as any).mockResolvedValue({
      strategies: [],
      server_time: '2025-01-15 12:00:02',
      server_ts_ms: Date.now()
    });

    initSignalState({
      rows: [],
      setRows: mockSetRows,
      mergeStrategy: mockMergeStrategy,
      mergeStrategies: mockMergeStrategies,
    });
  });

  const renderSignalTable = () =>
    render(
      <MemoryRouter>
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

  it('separates Run from Trading status in Signal', async () => {
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
    expect(screen.getByLabelText('Run is running for plumeusdt_bybit_spot_makerv3')).toBeInTheDocument();
    expect(screen.getByLabelText('Trading is paused for plumeusdt_bybit_spot_makerv3')).toBeInTheDocument();
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

    deltaHandler({
      id: 'contract_patch_strategy',
      legs: {
        'BTCUSDT-PERP': { decision_bid: 50025 },
      },
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

    deltaHandler({
      id: 'contract_order_clear_strategy',
      legs_order: null,
      legs: {
        'BTCUSDT-PERP': null,
      },
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
    expect(legASlot?.textContent).toContain('okx');
    expect(legASlot?.textContent).toContain('PLUME');
    expect(legBSlot?.textContent).toContain('binance_spot');
    expect(legBSlot?.textContent).toContain('PLUME');
    expect(legASlot?.textContent).not.toContain('bybit');
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

    marketUpdateHandler({
      strategies: [strategyA, strategyB],
      server_time: '2025-01-15 12:00:03',
      server_ts_ms: Date.now(),
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

    marketUpdateHandler({
      strategies: [strategyA],
      server_time: '2025-01-15 12:00:03',
      server_ts_ms: Date.now(),
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

    marketUpdateHandler({
      strategies: [strategyA, strategyB],
      server_time: '2025-01-15 12:00:04',
      server_ts_ms: Date.now(),
    });

    expect(mockMergeStrategies).toHaveBeenCalledWith([strategyA, strategyB]);
    expect(mockSetRows).toHaveBeenCalledWith([strategyA, strategyB]);
  });
});
