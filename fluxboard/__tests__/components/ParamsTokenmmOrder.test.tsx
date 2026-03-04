import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import Params from '../../Params';
import * as api from '../../api';

const { mockSetColumnOrder } = vi.hoisted(() => ({
  mockSetColumnOrder: vi.fn(),
}));

vi.mock('../../api', () => ({
  api: {
    getParamSchema: vi.fn(),
    getParams: vi.fn(),
    patchStrategyParams: vi.fn(),
  },
}));

vi.mock('../../hooks', () => ({
  usePolling: vi.fn(),
}));

vi.mock('../../stores', () => {
  const baseStore = {
    auto: false,
    setAuto: vi.fn(),
    viewMode: 'compact' as const,
    setViewMode: vi.fn(),
    activeProfile: 'maker_v3' as const,
    setActiveProfile: vi.fn(),
    columnPrefs: {
      order: [
        'qty',
        'cooldown',
        'bid_edge1',
        'ask_edge1',
        'des_qty_global',
        'max_qty_global',
        'max_skew_bps_global',
        'quote_fail_critical_after_count',
      ] as string[],
      visibility: {} as Record<string, boolean>,
    },
    setColumnOrder: mockSetColumnOrder,
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

describe('Params tokenmm order enforcement', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.history.pushState({}, '', '/tokenmm/params');

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
        qty: { key: 'qty', label: 'qty', description: '', type: 'float', default: 1 },
        cooldown: { key: 'cooldown', label: 'cooldown', description: '', type: 'float', default: 1 },
        des_qty_global: {
          key: 'des_qty_global',
          label: 'des_qty_global',
          description: '',
          type: 'float',
          default: 0,
        },
        max_qty_global: {
          key: 'max_qty_global',
          label: 'max_qty_global',
          description: '',
          type: 'float',
          default: 0,
        },
        max_skew_bps_global: {
          key: 'max_skew_bps_global',
          label: 'max_skew_bps_global',
          description: '',
          type: 'float',
          default: 0,
        },
        bid_edge1: { key: 'bid_edge1', label: 'bid_edge1', description: '', type: 'float', default: 0 },
        ask_edge1: { key: 'ask_edge1', label: 'ask_edge1', description: '', type: 'float', default: 0 },
        quote_fail_critical_after_count: {
          key: 'quote_fail_critical_after_count',
          label: 'quote_fail_critical_after_count',
          description: '',
          type: 'int',
          default: 3,
        },
      },
      deprecated: {},
    } as any);

    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'bybit_binance_plumeusdt_makerv3',
        running: true,
        meta: { class: 'maker_v3' },
        params: {
          bot_on: '1',
          qty: '1000',
          cooldown: '1',
          des_qty_global: '0',
          max_qty_global: '20000',
          max_skew_bps_global: '20',
          bid_edge1: '10',
          ask_edge1: '10',
          quote_fail_critical_after_count: '3',
        },
      },
    ] as any);
  });

  it('resets stale persisted maker_v3 order to canonical tokenmm order', async () => {
    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('bybit_binance_plumeusdt_makerv3')).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(mockSetColumnOrder).toHaveBeenCalledWith([
        'bot_on',
        'cooldown',
        'qty',
        'des_qty_global',
        'max_qty_global',
        'max_skew_bps_global',
        'bid_edge1',
        'ask_edge1',
        'quote_fail_critical_after_count',
      ]);
    });
  });
});
