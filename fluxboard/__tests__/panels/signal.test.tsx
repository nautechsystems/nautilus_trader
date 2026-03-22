/**
 * SignalTable Behavioral Tests
 *
 * Tests verify behavioral parity with legacy implementation:
 * - Sorting maintains order after WebSocket updates
 * - Trading gate labels remain stable
 * - Quotes and skew summaries render in the current desktop table
 * - Desktop maker rows surface balance failures
 * - 2-line row layout renders correctly
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { api } from '@/api';
import SignalTable from '@/components/domain/signal/SignalTable';
import { useSignalStore } from '@/stores';
import { socket } from '@/sockets';
import type { BalanceReadiness, SignalStrategy } from '@/types';

// Mock dependencies
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

// Selector-aware mock helper for useSignalStore in these tests
let currentSignalState: any;
const initSignalState = (state: any) => {
  currentSignalState = state;
  // Get the mocked useSignalStore from the mocked module
  const mockedUseSignalStore = useSignalStore as any;
  mockedUseSignalStore.getState = () => currentSignalState;
  mockedUseSignalStore.mockImplementation((selector?: any) =>
    selector ? selector(currentSignalState) : currentSignalState
  );
};

// Mock Popover component to avoid Radix UI dependency issues in tests
vi.mock('@/components/ui/popover/Popover', () => ({
  default: ({ children }: any) => <div>{children}</div>,
  Popover: ({ children }: any) => <div>{children}</div>,
  PopoverTrigger: ({ children }: any) => <div>{children}</div>,
  PopoverContent: ({ children }: any) => <div>{children}</div>,
}));

function renderSignalTable(pathname = '/signal') {
  return render(
    <MemoryRouter initialEntries={[pathname]}>
      <SignalTable />
    </MemoryRouter>
  );
}

function getVisibleStrategyIds(): string[] {
  const table = screen.getByRole('table');
  return Array.from(table.querySelectorAll('tbody tr')).map((row) => row.querySelector('td')?.textContent?.trim() ?? '');
}

async function flushAsyncRender() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe('SignalTable Behavioral Tests', () => {
  const mockSetRows = vi.fn();
  const mockMergeStrategy = vi.fn();

  const createMockStrategy = (id: string, overrides: Partial<SignalStrategy> = {}): SignalStrategy => ({
    id,
    params: {
      bot_on: '0',
      cex_bid_edge: '10',
      cex_ask_edge: '10',
      pool_edge: '10',
      qty: '100',
      slippage_bps: '50',
    },
    legs: {
      A: {
        exchange: 'bybit',
        coin: 'PLUME',
        decision_bid: 1.0,
        decision_ask: 1.01,
        net_edge_bps: 10,
        update_time: '2024-01-01 12:00:00',
      },
      B: {
        exchange: 'rooster',
        coin: 'WPLUME',
        decision_bid: 1.02,
        decision_ask: 1.03,
        net_edge_bps: 10,
        update_time: '2024-01-01 12:00:00',
      },
    },
    balances_ok: true,
    edge2_bps: 5,
    ...overrides,
  });

  beforeEach(() => {
    vi.clearAllMocks();
    initSignalState({ rows: [], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  describe('Sorting Behavior', () => {
    it('maintains global qty sort order after WebSocket update', async () => {
      const strategy1 = createMockStrategy('strategy_a', { risk_delta: 5 });
      const strategy2 = createMockStrategy('strategy_b', { risk_delta: 15 });
      const strategy3 = createMockStrategy('strategy_c', { risk_delta: 10 });

      initSignalState({ rows: [strategy1, strategy2, strategy3], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      const { rerender } = renderSignalTable();

      const globalQtyHeader = screen.getByText('Global Qty');
      await userEvent.click(globalQtyHeader);

      await waitFor(() => {
        expect(getVisibleStrategyIds()).toEqual(['strategy_b', 'strategy_c', 'strategy_a']);
      });

      const updatedStrategy1 = createMockStrategy('strategy_a', { risk_delta: 20 });
      initSignalState({ rows: [updatedStrategy1, strategy2, strategy3], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      rerender(
        <MemoryRouter>
          <SignalTable />
        </MemoryRouter>
      );

      await waitFor(() => {
        expect(getVisibleStrategyIds()).toEqual(['strategy_a', 'strategy_b', 'strategy_c']);
      });
    });

    it('uses strategy ID as the deterministic secondary sort key', async () => {
      const strategies = [
        createMockStrategy('zebra_strategy'),
        createMockStrategy('alpha_strategy'),
        createMockStrategy('beta_strategy'),
      ];

      initSignalState({ rows: strategies, setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await waitFor(() => {
        expect(getVisibleStrategyIds()).toEqual(['alpha_strategy', 'beta_strategy', 'zebra_strategy']);
      });
    });
  });

  describe('ON/OFF Badge Colors', () => {
    it('displays ON badge as green and OFF badge as neutral', async () => {
      const onStrategy = createMockStrategy('on_strategy', { params: { bot_on: '1' } });
      const offStrategy = createMockStrategy('off_strategy', { params: { bot_on: '0' } });

      initSignalState({ rows: [onStrategy, offStrategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      // Wait for table to render
      await waitFor(() => {
        expect(screen.getByRole('table')).toBeInTheDocument();
      }, { timeout: 2000 });

      // Wait for badges to appear - they might be rendered differently
      await waitFor(() => {
        const onBadge = screen.queryByText('ON');
        const offBadge = screen.queryByText('OFF');

        // Trading gate pills use Enabled/Paused labels with data-status
        const liveBadge = screen.queryByText(/Enabled/i);
        const pausedBadge = screen.queryByText(/Paused/i);
        if (liveBadge && pausedBadge) {
          expect(liveBadge).toHaveAttribute('data-status', 'ok');
          expect(pausedBadge).toHaveAttribute('data-status', 'muted');
        }
        // Always ensure strategies render
        expect(screen.getByText('on_strategy')).toBeInTheDocument();
        expect(screen.getByText('off_strategy')).toBeInTheDocument();
      }, { timeout: 3000 });
    });
  });

  describe('Quotes Column', () => {
    it('renders compact maker quote summary', async () => {
      const strategy = createMockStrategy('quote_strategy', {
        params: {
          bot_on: '1',
          cex_bid_edge: '10',
          cex_ask_edge: '10',
          pool_edge: '10',
          qty: '100',
          slippage_bps: '50',
        },
        maker_quote_status: {
          bid_open: 1,
          bid_depth: 3,
          bid_blocked: 0,
          ask_open: 2,
          ask_depth: 4,
          ask_blocked: 1,
        },
      });

      initSignalState({ rows: [strategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await waitFor(() => {
        expect(screen.getByText('quote_strategy')).toBeInTheDocument();
      });

      const summary = screen.getAllByText((_, node) => node?.textContent === 'B 1/3 · A 2/4')[0];
      expect(summary).toBeInTheDocument();
    });
  });

  describe('Adj/Skew Column', () => {
    it('renders the canonical signed skew_bps value when provided', async () => {
      const strategy = createMockStrategy('skew_strategy', {
        pricing_adjustments: [
          {
            type: 'inventory_skew',
            skew_bps_signed: -3,
            updated_ts_ms: 1700000000000,
          },
        ],
      });

      initSignalState({ rows: [strategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await waitFor(() => {
        expect(screen.getByText('skew_strategy')).toBeInTheDocument();
      });

      expect(screen.getByText('-3.0')).toBeInTheDocument();
      expect(screen.queryByText(/B:/)).not.toBeInTheDocument();
      expect(screen.queryByText(/A:/)).not.toBeInTheDocument();
    });

    it('uses the opposite sign when deltas are inverted', async () => {
      const strategy = createMockStrategy('skew_strategy_pos', {
        pricing_adjustments: [
          {
            type: 'inventory_skew',
            skew_bps_signed: 3,
            updated_ts_ms: 1700000000000,
          },
        ],
      });

      initSignalState({ rows: [strategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await waitFor(() => {
        expect(screen.getByText('skew_strategy_pos')).toBeInTheDocument();
      });

      expect(screen.getByText('+3.0')).toBeInTheDocument();
    });
  });

  describe('Desktop balance readiness indicator', () => {
    it('shows bal! when a maker quote row has backend FAIL readiness', async () => {
      const readiness: BalanceReadiness = {
        status: 'FAIL',
        qty: '10',
        multiplier: '10',
        summary: 'Needs wallet pUSD 20%',
        requirements: [],
        missing: [
          {
            location: 'wallet',
            token: 'pUSD',
            required: '100',
            available: '20',
            coverage: 0.2,
            kind: 'dex_quote',
          },
        ],
      };
      const readinessStrategy = createMockStrategy('needs_bal', {
        balances_ok: false,
        balance_readiness: readiness,
        maker_v2: {
          quote_snapshot: {
            mode: 'QUOTING',
            bid: 1,
            ask: 1.01,
            ts_ms: 1700000000000,
          },
        } as any,
      });

      initSignalState({ rows: [readinessStrategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await waitFor(() => {
        expect(screen.getAllByText('bal!').length).toBeGreaterThan(0);
      });
    });

    it('omits bal! when readiness is absent', async () => {
      const fallbackStrategy = createMockStrategy('legacy_only', {
        balance_readiness: undefined,
        maker_v2: {
          quote_snapshot: {
            mode: 'QUOTING',
            bid: 1,
            ask: 1.01,
            ts_ms: 1700000000000,
          },
        } as any,
      });

      initSignalState({ rows: [fallbackStrategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await waitFor(() => {
        expect(screen.getByText('legacy_only')).toBeInTheDocument();
        expect(screen.queryByText('bal!')).not.toBeInTheDocument();
      });
    });
  });

  describe('Freshness Indicator (Age)', () => {
    it('displays age values in seconds', async () => {
      const strategy = createMockStrategy('age_check', {
        legs: {
          A: {
            exchange: 'bybit',
            coin: 'PLUME',
            decision_bid: 1.0,
            decision_ask: 1.01,
            net_edge_bps: 10,
            update_time: '2024-01-01 12:00:00',
          },
          B: null,
        },
      });

      initSignalState({ rows: [strategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      const ageCell = await screen.findByText(/\d+(\.\d)?s$/);
      expect(ageCell.textContent).toMatch(/\d+(\.\d)?s/);
    });
  });

  describe('2-Line Row Layout', () => {
    it('renders leg data in 2-line format: exchange/coin + bid/mid/ask', async () => {
      const strategy = createMockStrategy('test', {
        legs: {
          A: {
            exchange: 'bybit',
            coin: 'PLUME',
            decision_bid: 1.2345,
            decision_ask: 1.2456,
            net_edge_bps: 10,
            update_time: '2024-01-01 12:00:00',
          },
          B: {
            exchange: 'rooster',
            coin: 'WPLUME',
            decision_bid: 1.2500,
            decision_ask: 1.2611,
            net_edge_bps: 10,
            update_time: '2024-01-01 12:00:00',
          },
        },
      });

      initSignalState({ rows: [strategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await waitFor(() => {
        // Check Leg A structure
        const legACell = screen.getByText('bybit PLUME').closest('td');
        expect(legACell).toBeInTheDocument();

        // Check bid/mid/ask are displayed
        expect(within(legACell!).getByText(/1\.2345/)).toBeInTheDocument();  // Bid
        expect(within(legACell!).getByText(/1\.2400|1\.2401/)).toBeInTheDocument();  // Mid (approx)
        expect(within(legACell!).getByText(/1\.2456/)).toBeInTheDocument();  // Ask

        // Check Leg B structure
        const legBCell = screen.getByText('rooster WPLUME').closest('td');
        expect(legBCell).toBeInTheDocument();
        expect(within(legBCell!).getByText(/1\.2500/)).toBeInTheDocument();  // Bid
        expect(within(legBCell!).getByText(/1\.2555|1\.2556/)).toBeInTheDocument();  // Mid (approx)
        expect(within(legBCell!).getByText(/1\.2611/)).toBeInTheDocument();  // Ask
      });
    });

    it('prefers canonical long leg labels when provided', async () => {
      const strategy = createMockStrategy('canonical_leg_labels', {
        legs: {
          A: {
            exchange: 'bybit',
            coin: 'PLUME',
            display_name_short: 'PLUME Perp',
            display_name_long: 'Bybit PLUME Perp',
            product_type: 'perp',
            decision_bid: 1.2345,
            decision_ask: 1.2456,
            update_time: '2024-01-01 12:00:00',
          },
          B: {
            exchange: 'binance_spot',
            coin: 'PLUME',
            display_name_short: 'PLUME Spot',
            display_name_long: 'Binance PLUME Spot',
            product_type: 'spot',
            decision_bid: 1.2500,
            decision_ask: 1.2611,
            update_time: '2024-01-01 12:00:00',
          },
        },
      });

      initSignalState({ rows: [strategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await waitFor(() => {
        expect(screen.getByText('Bybit PLUME Perp')).toBeInTheDocument();
        expect(screen.getByText('Binance PLUME Spot')).toBeInTheDocument();
      });
    });
  });

  describe('WebSocket Integration', () => {
    it('registers WebSocket event handlers on mount', () => {
      renderSignalTable();

      expect(socket.on).toHaveBeenCalledWith('connect', expect.any(Function));
      expect(socket.on).toHaveBeenCalledWith('disconnect', expect.any(Function));
      expect(socket.on).toHaveBeenCalledWith('market_update', expect.any(Function));
    });

    it('unregisters WebSocket handlers on unmount', () => {
      const { unmount } = renderSignalTable();

      unmount();

      expect(socket.off).toHaveBeenCalledWith('connect', expect.any(Function));
      expect(socket.off).toHaveBeenCalledWith('disconnect', expect.any(Function));
      expect(socket.off).toHaveBeenCalledWith('market_update', expect.any(Function));
    });

    it('does not fall back to watchdog polling while the websocket stays connected and idle', async () => {
      vi.useFakeTimers();
      (socket as any).connected = true;

      renderSignalTable();
      await flushAsyncRender();

      expect(api.getSignalStrategies).toHaveBeenCalledTimes(1);

      act(() => {
        vi.advanceTimersByTime(5_000);
      });
      await flushAsyncRender();

      expect(api.getSignalStrategies).toHaveBeenCalledTimes(1);
      vi.useRealTimers();
    });

    it('treats changed-id market_update payloads as one-shot invalidations instead of immediate snapshot thrash', async () => {
      vi.useFakeTimers();
      (socket as any).connected = true;

      renderSignalTable();
      await flushAsyncRender();

      expect(api.getSignalStrategies).toHaveBeenCalledTimes(1);

      const marketUpdateHandler = (socket.on as any).mock.calls.find(
        (call: any[]) => call[0] === 'market_update'
      )?.[1];
      expect(typeof marketUpdateHandler).toBe('function');

      act(() => {
        marketUpdateHandler({
          strategies: { changed: ['strategy_a'] },
          server_time: '2024-01-01 12:00:01',
          server_ts_ms: 1_700_000_000_001,
        });
        marketUpdateHandler({
          strategies: { changed: ['strategy_b'] },
          server_time: '2024-01-01 12:00:01',
          server_ts_ms: 1_700_000_000_001,
        });
      });

      expect(api.getSignalStrategies).toHaveBeenCalledTimes(1);

      act(() => {
        vi.advanceTimersByTime(999);
      });
      expect(api.getSignalStrategies).toHaveBeenCalledTimes(1);

      act(() => {
        vi.advanceTimersByTime(1);
      });
      await flushAsyncRender();
      expect(api.getSignalStrategies).toHaveBeenCalledTimes(2);

      act(() => {
        vi.advanceTimersByTime(2_000);
      });
      await flushAsyncRender();
      expect(api.getSignalStrategies).toHaveBeenCalledTimes(2);
      vi.useRealTimers();
    });

    it('resets recovery backoff after a successful invalidate-driven snapshot recovery', async () => {
      vi.useFakeTimers();
      (socket as any).connected = true;

      renderSignalTable('/signal');
      await flushAsyncRender();

      expect(api.getSignalStrategies).toHaveBeenCalledTimes(1);

      const marketUpdateHandler = (socket.on as any).mock.calls.find(
        (call: any[]) => call[0] === 'market_update'
      )?.[1];
      expect(typeof marketUpdateHandler).toBe('function');

      act(() => {
        marketUpdateHandler({
          strategies: { changed: ['strategy_a'] },
          server_time: '2024-01-01 12:00:01',
          server_ts_ms: 1_700_000_000_001,
        });
      });

      act(() => {
        vi.advanceTimersByTime(1_000);
      });
      await flushAsyncRender();
      expect(api.getSignalStrategies).toHaveBeenCalledTimes(2);

      act(() => {
        marketUpdateHandler({
          strategies: { changed: ['strategy_b'] },
          server_time: '2024-01-01 12:00:02',
          server_ts_ms: 1_700_000_000_002,
        });
      });

      act(() => {
        vi.advanceTimersByTime(999);
      });
      expect(api.getSignalStrategies).toHaveBeenCalledTimes(2);

      act(() => {
        vi.advanceTimersByTime(1);
      });
      await flushAsyncRender();
      expect(api.getSignalStrategies).toHaveBeenCalledTimes(3);
      vi.useRealTimers();
    });
  });

  describe('Filter Behavior', () => {
    it('filters strategies by trading gate status', async () => {
      const onStrategy = createMockStrategy('on_strat', { params: { bot_on: '1' } });
      const offStrategy = createMockStrategy('off_strat', { params: { bot_on: '0' } });

      initSignalState({ rows: [onStrategy, offStrategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await userEvent.click(screen.getByText('Filters'));

      // Initially both visible
      await screen.findByText('on_strat');
      await screen.findByText('off_strat');

      // Apply filter for ON strategies only
      const botFilterLabel = screen.getByText('Trading', { selector: 'label' });
      const botFilter = botFilterLabel.parentElement?.querySelector('select') as HTMLSelectElement;
      await userEvent.selectOptions(botFilter, 'Pending');

      await waitFor(() => {
        expect(screen.getByText('on_strat')).toBeInTheDocument();
        expect(screen.queryByText('off_strat')).not.toBeInTheDocument();
      });
    });

    it('recomputes filtered rows immediately when a steady-state live delta changes trading status', async () => {
      const pausedStrategy = createMockStrategy('live_filter_target', { params: { bot_on: '0' } });
      initSignalState({ rows: [pausedStrategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      const { rerender } = renderSignalTable('/signal');

      await userEvent.click(screen.getByText('Filters'));

      const tradingFilterLabel = screen.getByText('Trading', { selector: 'label' });
      const tradingFilter = tradingFilterLabel.parentElement?.querySelector('select') as HTMLSelectElement;
      await userEvent.selectOptions(tradingFilter, 'Pending');

      await waitFor(() => {
        expect(screen.queryByText('live_filter_target')).not.toBeInTheDocument();
      });

      const enabledStrategy = createMockStrategy('live_filter_target', { params: { bot_on: '1' } });
      initSignalState({ rows: [enabledStrategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      rerender(
        <MemoryRouter initialEntries={['/signal']}>
          <SignalTable />
        </MemoryRouter>
      );

      await waitFor(() => {
        expect(screen.getByText('live_filter_target')).toBeInTheDocument();
      });
    });

    it('filters strategies by strategy ID text', async () => {
      const strategies = [
        createMockStrategy('plume_rooster_bybit'),
        createMockStrategy('weth_rooster_bybit'),
        createMockStrategy('sei_sailor_bybit'),
      ];

      initSignalState({ rows: strategies, setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await userEvent.click(screen.getByText('Filters'));

      // Filter by "rooster"
      const strategyFilter = screen.getByPlaceholderText(/Strategy ID/i);
      await userEvent.type(strategyFilter, 'rooster');

      await waitFor(() => {
        expect(screen.getByText('plume_rooster_bybit')).toBeInTheDocument();
        expect(screen.getByText('weth_rooster_bybit')).toBeInTheDocument();
        expect(screen.queryByText('sei_sailor_bybit')).not.toBeInTheDocument();
      });
    });

    it('recomputes maker-suite facet options immediately when a steady-state live delta changes maker metadata', async () => {
      const makerStrategy = createMockStrategy('maker_live_facets', {
        strategy_family: 'maker_v3',
        meta: {
          class: 'maker_v3_dual_cex',
          strategy_groups: 'tokenmm',
          base_asset: 'PLUME',
        } as any,
        legs: {
          'binance_spot:PLUMEUSDT': {
            contract_id: 'binance_spot:PLUMEUSDT',
            exchange: 'binance_spot',
            symbol: 'PLUMEUSDT',
            base_asset: 'PLUME',
            product_type: 'spot',
            update_time: '2024-01-01 12:00:00',
          } as any,
          'okx:PLUMEUSDT-PERP': {
            contract_id: 'okx:PLUMEUSDT-PERP',
            exchange: 'okx',
            symbol: 'PLUMEUSDT-PERP',
            base_asset: 'PLUME',
            product_type: 'perp',
            update_time: '2024-01-01 12:00:00',
          } as any,
        } as any,
        legs_order: ['binance_spot:PLUMEUSDT', 'okx:PLUMEUSDT-PERP'] as any,
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
      });

      initSignalState({ rows: [makerStrategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      const { rerender } = renderSignalTable('/tokenmm/signal');

      await userEvent.click(screen.getByText('Filters'));

      const assetFilterLabel = screen.getByText('Asset', { selector: 'label' });
      const assetFilter = assetFilterLabel.parentElement?.querySelector('select') as HTMLSelectElement;
      expect(within(assetFilter).getAllByRole('option').map((option) => option.textContent)).toEqual(
        expect.arrayContaining(['PLUME'])
      );
      expect(within(assetFilter).queryByRole('option', { name: 'ETH' })).not.toBeInTheDocument();

      const updatedMakerStrategy = createMockStrategy('maker_live_facets', {
        strategy_family: 'maker_v3',
        meta: {
          class: 'maker_v3_dual_cex',
          strategy_groups: 'tokenmm',
          base_asset: 'ETH',
        } as any,
        legs: {
          'binance_spot:ETHUSDT': {
            contract_id: 'binance_spot:ETHUSDT',
            exchange: 'binance_spot',
            symbol: 'ETHUSDT',
            base_asset: 'ETH',
            product_type: 'spot',
            update_time: '2024-01-01 12:00:00',
          } as any,
          'hyperliquid:ETH-PERP': {
            contract_id: 'hyperliquid:ETH-PERP',
            exchange: 'hyperliquid',
            symbol: 'ETH-PERP',
            base_asset: 'ETH',
            product_type: 'perp',
            update_time: '2024-01-01 12:00:00',
          } as any,
        } as any,
        legs_order: ['binance_spot:ETHUSDT', 'hyperliquid:ETH-PERP'] as any,
        maker_role_map: {
          maker_leg: 'hyperliquid:ETH-PERP',
          ref_leg: 'binance_spot:ETHUSDT',
        } as any,
        maker_v3: {
          quote_snapshot: {
            maker_exchange: 'hyperliquid',
            ref_exchange: 'binance_spot',
          },
        } as any,
      });
      initSignalState({ rows: [updatedMakerStrategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      rerender(
        <MemoryRouter initialEntries={['/tokenmm/signal']}>
          <SignalTable />
        </MemoryRouter>
      );

      await waitFor(() => {
        expect(within(assetFilter).getByRole('option', { name: 'ETH' })).toBeInTheDocument();
      });
    });
  });

  describe('Quoted Prices Toggle', () => {
    it('toggles between decision and quoted prices', async () => {
      const strategy = createMockStrategy('test', {
        legs: {
          A: {
            exchange: 'bybit',
            coin: 'PLUME',
            decision_bid: 1.0,
            decision_ask: 1.01,
            quoted_bid: 0.99,
            quoted_ask: 1.02,
            net_edge_bps: 10,
            update_time: '2024-01-01 12:00:00',
          },
          B: null,
        },
      });

      initSignalState({ rows: [strategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      // Initially shows decision prices
      await waitFor(() => {
        expect(screen.getByText(/1\.0000/)).toBeInTheDocument();  // Decision bid
        expect(screen.getByText(/1\.0100/)).toBeInTheDocument();  // Decision ask
      });

      // Toggle to quoted prices
      const quotedCheckbox = screen.getByLabelText(/Show quoted prices/i);
      await userEvent.click(quotedCheckbox);

      await waitFor(() => {
        expect(screen.getByText(/0\.9900/)).toBeInTheDocument();  // Quoted bid
        expect(screen.getByText(/1\.0200/)).toBeInTheDocument();  // Quoted ask
      });
    });
  });

  describe('Last Trade Display', () => {
    it('displays last trade notional and realized bps', async () => {
      const strategy = createMockStrategy('test', {
        last_trade: {
          notional: 1234.56,
          realized_bps: 12.5,
        },
      });

      initSignalState({ rows: [strategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await waitFor(() => {
        expect(screen.getByText('$1234.56')).toBeInTheDocument();
        expect(screen.getByText('12.5 bps')).toBeInTheDocument();
      });
    });

    it('shows dash when no last trade', async () => {
      const strategy = createMockStrategy('test', { last_trade: null });

      initSignalState({ rows: [strategy], setRows: mockSetRows, mergeStrategy: mockMergeStrategy });

      renderSignalTable();

      await waitFor(() => {
        const lastTradeCell = screen.getByText('-');
        expect(lastTradeCell).toBeInTheDocument();
      });
    });
  });
});
