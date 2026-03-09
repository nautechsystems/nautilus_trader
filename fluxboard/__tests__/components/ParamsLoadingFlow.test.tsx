/**
 * Tests for Params component loading flow and autorefresh behavior.
 *
 * Critical requirements:
 * 1. Initial load must complete successfully before autorefresh starts
 * 2. Failed initial loads should show error and retry button
 * 3. Autorefresh failures should not block UI
 * 4. Running status should be displayed correctly
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import Params from '../../Params';
import * as api from '../../api';

// Mock the API
vi.mock('../../api', () => ({
  api: {
    getParamSchema: vi.fn(),
    getParams: vi.fn(),
    patchStrategyParams: vi.fn(),
  },
}));

// Mock usePolling hook
vi.mock('../../hooks/index', () => ({
  usePolling: vi.fn((callback, interval, enabled) => {
    (global as any).pollingEnabled = enabled;
    (global as any).pollingCallback = callback;
  }),
}));

// Mock stores
vi.mock('../../stores', () => {
  const baseStore = {
    auto: true,
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

// Mock toast
vi.mock('sonner', () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
  },
}));

describe('Params Loading Flow', () => {
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
      qty: {
        key: 'qty',
        label: 'Quantity',
        description: 'Trade quantity',
        type: 'float' as const,
        default: 1.0,
        min_value: 0,
        max_value: 1000,
        step: 0.1,
        unit: 'units',
      },
    },
    deprecated: {},
  };

  const mockParams = [
    {
      strategy_id: 'strategy_1',
      running: true,
      shard: 'shard_1',
      runner: 'runner_1',
      params: {
        bot_on: '0',
        qty: '100.0',
      },
    },
    {
      strategy_id: 'strategy_2',
      running: false,
      shard: 'shard_2',
      runner: 'runner_2',
      params: {
        bot_on: '1',
        qty: '50.0',
      },
    },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
    (global as any).pollingEnabled = undefined;
    (global as any).pollingCallback = undefined;
  });

  afterEach(() => {
    delete (global as any).pollingEnabled;
    delete (global as any).pollingCallback;
  });

  it('should load data successfully before enabling autorefresh', async () => {
    // Mock successful API responses
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams);

    render(<Params />);

    // Should show loading initially
    expect(screen.getByText('Loading parameters...')).toBeInTheDocument();
    expect(screen.getByText('Ensuring data loads before autorefresh starts')).toBeInTheDocument();

    // Wait for data to load
    await waitFor(() => {
      expect(screen.getByText('strategy_1')).toBeInTheDocument();
    });

    // Verify autorefresh is enabled after successful load
    await waitFor(() => {
      expect((global as any).pollingEnabled).toBe(true);
    });
  });

  it('should NOT enable autorefresh if initial load fails', async () => {
    // Mock failed API responses
    vi.mocked(api.api.getParamSchema).mockRejectedValue(new Error('Network error'));
    vi.mocked(api.api.getParams).mockRejectedValue(new Error('Network error'));

    render(<Params />);

    // Wait for error screen
    await waitFor(() => {
      expect(screen.getByText('Failed to load parameters')).toBeInTheDocument();
    });

    expect(screen.getByText(/Network error/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();

    // Verify autorefresh is NOT enabled
    expect((global as any).pollingEnabled).toBeFalsy();
  });

  it('should show retry button on initial load failure', async () => {
    let schemaCalls = 0;
    let paramsCalls = 0;

    vi.mocked(api.api.getParamSchema).mockImplementation(() => {
      schemaCalls += 1;
      if (schemaCalls === 1) {
        return Promise.reject(new Error('Network error'));
      }
      return Promise.resolve(mockSchema);
    });

    vi.mocked(api.api.getParams).mockImplementation(() => {
      paramsCalls += 1;
      if (paramsCalls === 1) {
        return Promise.reject(new Error('Network error'));
      }
      return Promise.resolve(mockParams);
    });

    render(<Params />);

    // Wait for error screen
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();
    });

    // Click retry
    const retryButton = screen.getByRole('button', { name: 'Retry' });
    await userEvent.click(retryButton);

    await waitFor(() => {
      expect(api.api.getParamSchema).toHaveBeenCalledTimes(2);
      expect(api.api.getParams).toHaveBeenCalledTimes(2);
    });

    // Should now clear error state
    await waitFor(() => {
      expect(screen.queryByText('Failed to load parameters')).not.toBeInTheDocument();
    });

  });

  it('should validate response data before proceeding', async () => {
    // Mock invalid schema response
    vi.mocked(api.api.getParamSchema).mockResolvedValue({} as any);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams);

    render(<Params />);

    // Should show error for invalid schema
    await waitFor(() => {
      expect(screen.getByText('Failed to load parameters')).toBeInTheDocument();
    });

    expect(screen.getByText(/Invalid schema response/i)).toBeInTheDocument();
  });

  it('should validate params array before proceeding', async () => {
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    // Return non-array
    vi.mocked(api.api.getParams).mockResolvedValue({ invalid: 'data' } as any);

    render(<Params />);

    // Should show error for invalid params
    await waitFor(() => {
      expect(screen.getByText('Failed to load parameters')).toBeInTheDocument();
    });

    expect(screen.getByText(/Invalid params response/i)).toBeInTheDocument();
  });

  it('should filter out strategies without strategy_id', async () => {
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    vi.mocked(api.api.getParams).mockResolvedValue([
      ...mockParams,
      {
        strategy_id: '',  // Invalid
        running: null,
        params: {},
      } as any,
    ]);

    const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('strategy_1')).toBeInTheDocument();
    });

    // Should have logged warning about invalid strategy
    expect(consoleSpy).toHaveBeenCalledWith(
      expect.stringContaining('Strategy missing strategy_id'),
      expect.anything()
    );

    // Should only show valid strategies
    expect(screen.getAllByRole('button', { name: /^strategy_/i })).toHaveLength(2);

    consoleSpy.mockRestore();
  });

  it('should display trading state correctly', async () => {
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('strategy_1')).toBeInTheDocument();
    });

    expect(screen.getByLabelText('Toggle trading for strategy_1')).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByLabelText('Toggle trading for strategy_2')).toHaveAttribute('aria-checked', 'true');
  });

  it('should handle missing running status', async () => {
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'strategy_unknown',
        running: null,
        params: { bot_on: '1' },
      },
    ]);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('strategy_unknown')).toBeInTheDocument();
    });

    expect(screen.getByLabelText('Toggle trading for strategy_unknown')).toHaveAttribute('aria-checked', 'true');
  });

  it('pauses auto-refresh while editing a cell', async () => {
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('strategy_1')).toBeInTheDocument();
    });

    const qtyInput = screen.getAllByLabelText('Quantity')[0] as HTMLInputElement;
    await userEvent.click(qtyInput);

    await waitFor(() => {
      expect(screen.getByText('Paused (editing)')).toBeInTheDocument();
    });
  });

  it('keeps auto-refresh running after unsaved changes are blurred', async () => {
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('strategy_1')).toBeInTheDocument();
    });

    const qtyInput = screen.getAllByLabelText('Quantity')[0] as HTMLInputElement;
    await userEvent.click(qtyInput);
    await userEvent.clear(qtyInput);
    await userEvent.type(qtyInput, '123');
    await userEvent.tab();

    await waitFor(() => {
      expect((global as any).pollingEnabled).toBe(true);
    });
    expect(screen.queryByText('Paused (unsaved changes)')).not.toBeInTheDocument();
  });

  it('highlights rows when remote values change without local edits', async () => {
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    vi.mocked(api.api.getParams)
      .mockResolvedValueOnce(mockParams)
      .mockResolvedValueOnce([
        {
          ...mockParams[0],
          params: { ...mockParams[0].params, qty: '150.0' },
        },
        mockParams[1],
      ]);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('strategy_1')).toBeInTheDocument();
    });

    const callback = (global as any).pollingCallback;
    expect(typeof callback).toBe('function');

    await act(async () => {
      await callback?.();
    });

    await waitFor(() => {
      expect(screen.getAllByText('Updated')[0]).toBeInTheDocument();
    });
  });

  it('flags conflicts when backend changes a dirty param', async () => {
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    vi.mocked(api.api.getParams)
      .mockResolvedValueOnce(mockParams)
      .mockResolvedValueOnce([
        {
          ...mockParams[0],
          params: { ...mockParams[0].params, qty: '999.0' },
        },
        mockParams[1],
      ]);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('strategy_1')).toBeInTheDocument();
    });

    const qtyInput = screen.getAllByLabelText('Quantity')[0] as HTMLInputElement;
    await userEvent.clear(qtyInput);
    await userEvent.type(qtyInput, '123');
    await userEvent.tab();

    const callback = (global as any).pollingCallback;
    expect(typeof callback).toBe('function');

    await act(async () => {
      await callback?.();
    });

    await waitFor(() => {
      const editedQty = screen.getAllByLabelText('Quantity')[0] as HTMLInputElement;
      expect(editedQty.value).toBe('123');
    });
  });
});
