/**
 * Params - Trading sort stability
 *
 * Ensures sorting by Trading uses saved values, not unsaved (dirty) toggles,
 * so rows do not jump when a toggle is flipped but not yet saved.
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
    updateParams: vi.fn(),
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
    sortState: { key: '__trading__' as string | null, direction: 'desc' as 'asc' | 'desc' | null },
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

// Silence toasts
vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}));

describe('Params trading sort stability', () => {
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

  const mockParams = [
    { strategy_id: 'gamma', running: true, params: { bot_on: '1' } },
    { strategy_id: 'alpha', running: true, params: { bot_on: '1' } },
    { strategy_id: 'bravo', running: false, params: { bot_on: '0' } },
  ];

  const tradingOrder = () =>
    screen
      .getAllByLabelText(/Toggle trading for/i)
      .map((btn) => btn.getAttribute('aria-label'))
      .filter((label): label is string => Boolean(label && !label.includes('all filtered')));

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema as any);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams as any);
  });

  it('keeps Trading sort stable when toggling a strategy before saving', async () => {
    render(<Params />);

    await waitFor(() => {
      expect(tradingOrder()).toEqual([
        'Toggle trading for alpha',
        'Toggle trading for gamma',
        'Toggle trading for bravo',
      ]);
    });

    const gammaToggle = screen.getByRole('switch', { name: 'Toggle trading for gamma' });
    expect(gammaToggle).toHaveAttribute('aria-checked', 'true');

    fireEvent.click(gammaToggle);

    await waitFor(() => {
      expect(gammaToggle).toHaveAttribute('aria-checked', 'false');
    });

    await waitFor(() => {
      expect(tradingOrder()).toEqual([
        'Toggle trading for alpha',
        'Toggle trading for gamma',
        'Toggle trading for bravo',
      ]);
    });
  });
});
