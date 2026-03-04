/**
 * Params - Bulk apply row for filtered strategies
 *
 * Verifies the top bulk row lets us set a param once and, after Save All,
 * every strategy in the current filtered view receives that update (and only those).
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import Params from '../../Params';
import * as api from '../../api';

// Mock API module
vi.mock('../../api', () => ({
  api: {
    getParamSchema: vi.fn(),
    getParams: vi.fn(),
    patchStrategyParams: vi.fn(),
    updateParams: vi.fn(),
  },
}));

// Mock polling to avoid timers and background calls
vi.mock('../../hooks', () => ({
  usePolling: vi.fn(),
}));

// Silence toasts
vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}));

describe('Params bulk apply row', () => {
  const mockSchema = {
    params: {
      bot_on: {
        key: 'bot_on',
        label: 'Bot On',
        description: 'Enable trading',
        type: 'select' as const,
        default: '0',
        options: [
          ['0', 'Off'],
          ['1', 'On'],
        ] as [string, string][],
        unit: null,
      },
      cex_bid_edge: {
        key: 'cex_bid_edge',
        label: 'Bid Edge',
        description: 'Bid edge threshold',
        type: 'float' as const,
        default: 1.0,
        min_value: 0,
        max_value: 100,
        step: 0.1,
        unit: 'bps',
      },
    },
    deprecated: {},
  };

  const mockParams = [
    { strategy_id: 'futu_hl_a', running: true, params: { bot_on: '1', cex_bid_edge: '5' } },
    { strategy_id: 'futu_hl_b', running: true, params: { bot_on: '0', cex_bid_edge: '6' } },
    { strategy_id: 'other_x', running: false, params: { bot_on: '0', cex_bid_edge: '7' } },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema as any);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams as any);
    vi.mocked(api.api.updateParams).mockResolvedValue({ success: 2, failed: 0, errors: [] } as any);
  });

  it('applies param change to all filtered strategies and saves them together', async () => {
    render(<Params />);

    // Wait for table
    await waitFor(() => {
      expect(screen.getByText('futu_hl_a')).toBeInTheDocument();
    });

    // Open filters and filter to futu_hl*
    const filtersToggle = screen.getByRole('button', { name: /Filters/i });
    fireEvent.click(filtersToggle);
    const searchInput = screen.getByPlaceholderText('Search strategies, params...');
    fireEvent.change(searchInput, { target: { value: 'futu_hl' } });

    // Enter bulk value for bid edge and apply with Enter
    const bulkRow = await screen.findByTestId('bulk-row');
    expect(bulkRow).toBeInTheDocument();
    const bulkBidEdge = await screen.findByTestId('bulk-param-cex_bid_edge');
    fireEvent.change(bulkBidEdge, { target: { value: '12' } });
    fireEvent.keyDown(bulkBidEdge, { key: 'Enter', code: 'Enter' });

    // Save All should pick up two filtered strategies only
    const saveAll = screen.getByRole('button', { name: /Save All/i });
    fireEvent.click(saveAll);

    await waitFor(() => {
      expect(api.api.updateParams).toHaveBeenCalledTimes(1);
    });

    const [updates] = vi.mocked(api.api.updateParams).mock.calls[0];
    const ids = updates.map((u: any) => u.strategy_id).sort();
    const values = updates.map((u: any) => u.params.cex_bid_edge);

    expect(ids).toEqual(['futu_hl_a', 'futu_hl_b']);
    expect(values.every((v: string) => v === '12')).toBe(true);
  });

  it('supports undo via Ctrl/Cmd+Z for last bulk apply', async () => {
    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('futu_hl_a')).toBeInTheDocument();
    });

    const filtersToggle = screen.getByRole('button', { name: /Filters/i });
    fireEvent.click(filtersToggle);
    const searchInput = screen.getByPlaceholderText('Search strategies, params...');
    fireEvent.change(searchInput, { target: { value: 'futu_hl' } });

    const bulkRow = await screen.findByTestId('bulk-row');
    expect(bulkRow).toBeInTheDocument();
    const bulkBidEdge = await screen.findByTestId('bulk-param-cex_bid_edge');
    fireEvent.change(bulkBidEdge, { target: { value: '15' } });
    fireEvent.keyDown(bulkBidEdge, { key: 'Enter', code: 'Enter' });

    // Undo with Ctrl/Cmd+Z
    fireEvent.keyDown(window, { key: 'z', ctrlKey: true });

    // Save All should be a no-op because values returned to original
    const saveAll = screen.getByRole('button', { name: /Save All/i });
    fireEvent.click(saveAll);

    await waitFor(() => {
      expect(api.api.updateParams).not.toHaveBeenCalled();
    });
  });

  it('toggles trading on for all filtered strategies via bulk switch without errors', async () => {
    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('futu_hl_a')).toBeInTheDocument();
    });

    // Filter to two strategies
    const filtersToggle = screen.getByRole('button', { name: /Filters/i });
    fireEvent.click(filtersToggle);
    const searchInput = screen.getByPlaceholderText('Search strategies, params...');
    fireEvent.change(searchInput, { target: { value: 'futu_hl' } });

    const bulkSwitch = await screen.findByTestId('bulk-trading-toggle');
    expect(bulkSwitch).toHaveAttribute('data-state', 'unchecked');

    fireEvent.click(bulkSwitch);

    // Save All should send bot_on=1 for the filtered strategies without errors
    const saveAll = screen.getByRole('button', { name: /Save All/i });
    fireEvent.click(saveAll);

    await waitFor(() => {
      expect(api.api.updateParams).toHaveBeenCalledTimes(1);
    });

    const [updates] = vi.mocked(api.api.updateParams).mock.calls[0];
    const payload = updates.map((u: any) => ({ id: u.strategy_id, bot_on: u.params.bot_on }));
    expect(payload).toEqual([
      { id: 'futu_hl_b', bot_on: '1' },
    ]);
  });
});
