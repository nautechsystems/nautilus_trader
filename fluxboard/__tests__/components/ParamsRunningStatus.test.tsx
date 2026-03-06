/**
 * Params - Trading toggle indicators
 *
 * Ensures the compact controls column exposes trading-only toggles
 * with accurate aria-checked state and refresh support.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import Params from '../../Params';
import * as api from '../../api';

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

describe('Params Trading Toggle Display', () => {
  const mockSchema = {
    params: {
      bot_on: {
        key: 'bot_on',
        label: 'Bot On',
        description: 'Enable trading',
        type: 'select' as const,
        default: '0',
        options: [['0', 'Off'], ['1', 'On']] as [string, string][],
        unit: null,
      },
    },
    deprecated: {},
  };

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
  });

  it('reflects trading state via aria-checked attributes', async () => {
    vi.mocked(api.api.getParams).mockResolvedValue([
      { strategy_id: 'alpha', params: { bot_on: '1' } },
      { strategy_id: 'bravo', params: { bot_on: '0' } },
    ]);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('alpha')).toBeInTheDocument();
    });

    const alphaToggle = screen.getByLabelText('Toggle trading for alpha');
    const bravoToggle = screen.getByLabelText('Toggle trading for bravo');
    expect(alphaToggle).toHaveAttribute('aria-checked', 'true');
    expect(bravoToggle).toHaveAttribute('aria-checked', 'false');
  });

  it('updates trading toggle after manual refresh', async () => {
    vi.mocked(api.api.getParams)
      .mockResolvedValueOnce([
        { strategy_id: 'test_strategy', params: { bot_on: '0' } },
      ])
      .mockResolvedValueOnce([
        { strategy_id: 'test_strategy', params: { bot_on: '1' } },
      ]);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByLabelText('Toggle trading for test_strategy')).toHaveAttribute('aria-checked', 'false');
    });

    fireEvent.click(screen.getByRole('button', { name: 'Refresh parameters' }));

    await waitFor(() => {
      expect(screen.getByLabelText('Toggle trading for test_strategy')).toHaveAttribute('aria-checked', 'true');
    });
  });
});
