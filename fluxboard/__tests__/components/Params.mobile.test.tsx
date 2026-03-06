import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { MemoryRouter } from 'react-router-dom';
import Params from '../../Params';
import * as api from '../../api';

vi.mock('@/hooks/useIsMobile', () => ({ useIsMobile: () => true }));

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
      return selector(baseStore as any);
    }
    return baseStore;
  });
  return { useParamsStore: mockedHook };
});

vi.mock('sonner', () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
  },
}));

describe('Params mobile layout', () => {
  const mockSchema = {
    params: {
      bot_on: { key: 'bot_on', label: 'Bot On', type: 'bool', default: '0' },
      qty: { key: 'qty', label: 'Quantity', type: 'float', default: 1 },
      cex_bid_edge: { key: 'cex_bid_edge', label: 'Bid Edge', type: 'float', default: 0.1 },
      cex_ask_edge: { key: 'cex_ask_edge', label: 'Ask Edge', type: 'float', default: 0.1 },
      cooldown: { key: 'cooldown', label: 'Cooldown', type: 'int', default: 100 },
    },
    deprecated: {},
  } as any;

  const mockParams = [
    {
      strategy_id: 'strat-1',
      running: true,
      params: {
        bot_on: '1',
        qty: '2',
        cex_bid_edge: '0.15',
        cex_ask_edge: '0.2',
        cooldown: '150',
      },
      meta: {
        hot_params: ['bot_on', 'qty', 'cex_ask_edge', 'cex_bid_edge', 'cooldown'],
      },
    },
  ];

  beforeEach(() => {
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams as any);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('shows mobile control bar and renders hot param chips', async () => {
    render(
      <MemoryRouter>
        <Params variant="mobile" />
      </MemoryRouter>
    );

    await waitFor(() => {
      expect(screen.getByText('strat-1')).toBeInTheDocument();
    });

    expect(screen.getByRole('button', { name: /Save All/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Revert All/i })).toBeInTheDocument();

    expect(screen.getByText(/Quantity/i)).toBeInTheDocument();
    expect(
      screen.getByLabelText(/Ask Edge value for strat-1/i)
    ).toHaveValue('0.2');

    const tradingToggle = screen.getByLabelText(/Trading strat-1/i) as HTMLInputElement;
    expect(tradingToggle.checked).toBe(true);

    await userEvent.click(screen.getByRole('button', { name: /Filters/i }));
    expect(screen.getByLabelText('Params family')).toBeInTheDocument();
  });

  it('shows dirty count on Save All, toggles filters, and renders hot param inputs', async () => {
    render(
      <MemoryRouter>
        <Params variant="mobile" />
      </MemoryRouter>
    );

    await waitFor(() => {
      expect(screen.getByText('strat-1')).toBeInTheDocument();
    });

    // Toggle filters visible
    await userEvent.click(screen.getByRole('button', { name: /Filters/i }));
    expect(screen.getByLabelText('Params family')).toBeInTheDocument();

    // Edit a hot param to mark dirty
    const qtyInput = screen.getByLabelText(/Quantity value for strat-1/i);
    await userEvent.clear(qtyInput);
    await userEvent.type(qtyInput, '3');

    const saveAll = screen.getByRole('button', { name: /Save All/ });
    expect(saveAll.textContent).toMatch(/Save All \(1\)/);

    // Trading gate toggle present with aria label
    expect(screen.getByLabelText(/Trading strat-1/i)).toBeInTheDocument();
  });
});
