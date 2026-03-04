import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent, act } from '@testing-library/react';
import { useEffect } from 'react';
import Params from '../../Params';
import * as api from '../../api';

// Mock API
vi.mock('../../api', () => ({
  api: {
    getParamSchema: vi.fn(),
    getParams: vi.fn(),
    patchStrategyParams: vi.fn(),
  },
}));

// Mock polling
vi.mock('../../hooks', () => ({
  usePolling: vi.fn(),
}));

// Mock params store
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

const filterPreset: { current: Record<string, string> } = { current: {} };
const filterHandler: { current: ((filters: Record<string, string>) => void) | null } = { current: null };

vi.mock('../../components/shared/TableFilter', () => ({
  TableFilter: ({ onFilterChange }: any) => {
    filterHandler.current = onFilterChange;
    useEffect(() => {
      if (Object.keys(filterPreset.current).length > 0) {
        onFilterChange(filterPreset.current);
      }
    }, [onFilterChange]);
    return <div data-testid="params-filter" />;
  },
}));

// Silence toasts
vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}));

describe('Params status filter semantics', () => {
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
      qty: {
        key: 'qty',
        label: 'qty',
        description: '',
        type: 'float',
        default: 1.0,
        min_value: 0,
        max_value: 100,
        step: 1,
        unit: 'units',
      },
      max_age_ms: {
        key: 'max_age_ms',
        label: 'max_age_ms',
        description: '',
        type: 'int',
        default: 2500,
        min_value: 100,
        max_value: 100000,
        step: 100,
        unit: 'milliseconds',
      },
    },
    deprecated: {},
  } as any;

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    filterPreset.current = {};
  });

  it('treats unknown running status as Stopped for Status filter', async () => {
    filterPreset.current = { status: 'Stopped' };
    vi.mocked(api.api.getParams).mockResolvedValue([
      { strategy_id: 'run_true', running: true, params: { bot_on: '1' } },
      { strategy_id: 'run_unknown', running: null, params: { bot_on: '1' } },
    ] as any);

    render(<Params />);

    // With Status=Stopped, only the unknown-running strategy should be visible
    await waitFor(() => {
      expect(screen.getByText('run_unknown')).toBeInTheDocument();
      expect(screen.queryByText('run_true')).not.toBeInTheDocument();
    });
  });

  it('shows only running strategies when Status filter is Running', async () => {
    filterPreset.current = { status: 'Running' };
    vi.mocked(api.api.getParams).mockResolvedValue([
      { strategy_id: 'run_true', running: true, params: { bot_on: '1' } },
      { strategy_id: 'run_unknown', running: null, params: { bot_on: '1' } },
    ] as any);

    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('run_true')).toBeInTheDocument();
      expect(screen.queryByText('run_unknown')).not.toBeInTheDocument();
    });
  });

  it('Dirty filter only shows strategies with unsaved edits', async () => {
    vi.mocked(api.api.getParams).mockResolvedValue([
      {
        strategy_id: 'alpha',
        running: true,
        params: { bot_on: '1', qty: '1', max_age_ms: '2500' },
      },
    ] as any);

    render(<Params />);

    // Initial load (no filter) should show the strategy
    await waitFor(() => {
      expect(screen.getByText('alpha')).toBeInTheDocument();
    });

    // Apply dirty filter – no dirty rows yet, so nothing should be visible
    await act(async () => {
      filterHandler.current?.({ dirty: 'Yes' });
    });
    await waitFor(() => {
      expect(screen.queryByText('alpha')).not.toBeInTheDocument();
    });

    // Clear the filter so we can edit the row
    await act(async () => {
      filterHandler.current?.({});
    });
    await waitFor(() => {
      expect(screen.getByText('alpha')).toBeInTheDocument();
    });

    // Edit qty to mark the row dirty
    const qtyInput = (await screen.findByDisplayValue('1')) as HTMLInputElement;
    fireEvent.focus(qtyInput);
    fireEvent.change(qtyInput, { target: { value: '2' } });
    fireEvent.blur(qtyInput);

    // Reapply the dirty filter; now the dirty row should appear
    await act(async () => {
      filterHandler.current?.({ dirty: 'Yes' });
    });
    await waitFor(() => {
      expect(screen.getByText('alpha')).toBeInTheDocument();
    });
  });
});
