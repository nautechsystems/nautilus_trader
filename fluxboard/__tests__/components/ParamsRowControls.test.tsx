import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import Params from '../../Params';
import * as api from '../../api';

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
    activeProfile: 'taker' as const,
    setActiveProfile: vi.fn(),
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
  toast: { error: vi.fn(), success: vi.fn() },
}));

describe('Params row controls expose instrumentation attributes', () => {
  const mockSchema = {
    params: {
      bot_on: {
        key: 'bot_on',
        label: 'bot_on',
        description: '',
        type: 'select',
        default: '0',
        options: [['0', 'Off'], ['1', 'On']],
        unit: null,
      },
    },
    deprecated: {},
  } as any;

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
  });

  it('surfaces run/trading states via data-testid and data-state', async () => {
    vi.mocked(api.api.getParams).mockResolvedValue([
      { strategy_id: 'run_true', running: true, params: { bot_on: '1' } },
      { strategy_id: 'run_false', running: false, params: { bot_on: '0' } },
    ] as any);

    render(<Params />);

    const runTrueIndicator = await screen.findByTestId('run-indicator-run_true');
    expect(runTrueIndicator).toHaveAttribute('data-state', 'running');
    expect(runTrueIndicator.textContent?.trim()).toBe('');
    const runFalseIndicator = screen.getByTestId('run-indicator-run_false');
    expect(runFalseIndicator).toHaveAttribute('data-state', 'stopped');
    expect(runFalseIndicator.textContent?.trim()).toBe('');

    const tradingOn = screen.getByTestId('trading-toggle-run_true');
    expect(tradingOn).toHaveAttribute('aria-checked', 'true');
    const tradingOff = screen.getByTestId('trading-toggle-run_false');
    expect(tradingOff).toHaveAttribute('aria-checked', 'false');
  });
});
